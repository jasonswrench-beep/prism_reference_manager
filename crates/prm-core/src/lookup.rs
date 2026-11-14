//! Identifier resolution through replaceable open resolvers. Every request is optional, logged
//! in the network panel, rate-limited and retried with backoff. Results are candidates for
//! review, never committed directly.

use crate::error::{Error, Result};
use crate::formats::{self, Parsed};
use crate::model::{PartialDate, Reference};
use serde::Serialize;
use std::time::Duration;

pub const USER_AGENT: &str = concat!("PrismReferenceManager/", env!("CARGO_PKG_VERSION"), " (desktop; metadata lookup)");

/// Outbound operations the program can make, for the network inventory.
pub const NETWORK_INVENTORY: &[(&str, &str, &str)] = &[
    ("doi", "doi.org", "Resolve a DOI to bibliographic metadata. Sends the DOI."),
    ("doi-corroborate", "api.crossref.org", "Corroborate DOI metadata (title, authors, year). Sends the DOI."),
    ("doi-datacite", "api.datacite.org", "Resolve DOIs for datasets and software. Sends the DOI."),
    ("pubmed", "eutils.ncbi.nlm.nih.gov", "Resolve PubMed and PMC identifiers. Sends the identifier."),
    ("arxiv", "export.arxiv.org", "Resolve arXiv identifiers. Sends the identifier."),
    ("isbn", "openlibrary.org", "Resolve ISBNs for books. Sends the ISBN."),
    ("webpage", "(the page's own site)", "Read embedded citation metadata from a web page you entered. Sends a request for that URL."),
    ("retraction", "api.crossref.org", "Check for retraction and correction notices. Sends DOIs."),
    ("openaccess", "api.openalex.org", "Find a free, legal copy of an article. Sends the DOI."),
    ("doi-search", "api.crossref.org", "Find a missing DOI when you ask. Sends the title, first author's surname and year."),
    ("abstract", "api.crossref.org", "Fetch a missing abstract when you ask. Sends the DOI."),
    ("linkcheck", "(each source's own web address)", "Check that saved web addresses still work, when you ask. Sends a request for each address."),
    ("openaccess-download", "(the repository or publisher hosting the free copy)", "Download a free copy that was found. Sends a request for that file."),
    ("update", "(the update feed, once set up)", "Check for a newer version of this program when you ask, or once a day if you turn that on. Sends the program's version and Windows architecture."),
];

#[derive(Debug, Clone, Serialize)]
pub struct LookupResult {
    pub kind: String,
    pub id: String,
    pub candidate: Option<Reference>,
    pub corroboration: Vec<String>,
    pub conflicts: Vec<String>,
    pub message: String,
    pub ok: bool,
}

/// Whether an address is on the public internet. Requests (and every redirect) may only reach
/// such addresses, so a web address someone else chose cannot make this program talk to the
/// computer itself, the local network, or a router.
pub fn is_public(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_multicast() || v.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || o[0] >= 240)
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let seg = v.segments();
            !(v.is_loopback() || v.is_unspecified() || v.is_multicast() || (seg[0] & 0xfe00) == 0xfc00 || (seg[0] & 0xffc0) == 0xfe80 || seg[0] == 0x2001 && seg[1] == 0x0db8)
        }
    }
}

/// Resolves names to public addresses only (see `is_public`). Used for every request, so the
/// check happens at connect time for each redirect as well.
struct PublicOnly;
impl ureq::Resolver for PublicOnly {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
        use std::net::ToSocketAddrs;
        let v: Vec<std::net::SocketAddr> = netloc.to_socket_addrs()?.filter(|a| is_public(a.ip())).collect();
        if v.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "that address is not on the public internet"));
        }
        Ok(v)
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(25)).user_agent(USER_AGENT).resolver(PublicOnly).build()
}

/// GET with retries: exponential backoff with jitter; honours Retry-After on 429/503.
fn get(url: &str, accept: &str) -> Result<String> {
    let a = agent();
    let mut delay = 600u64;
    for attempt in 0..4 {
        match a.get(url).set("Accept", accept).call() {
            Ok(r) => return r.into_string().map_err(|e| Error::Network(e.to_string())),
            Err(ureq::Error::Status(404, _)) => return Err(Error::Network("Not found at the resolver.".into())),
            Err(ureq::Error::Status(code, r)) if code == 429 || code >= 500 => {
                let wait = r.header("Retry-After").and_then(|s| s.parse::<u64>().ok()).map(|s| s * 1000).unwrap_or(delay);
                if attempt == 3 {
                    return Err(Error::Network(format!("The service is busy (HTTP {code}). Try again later.")));
                }
                let jitter = (std::process::id() as u64 * 37 + attempt as u64 * 101) % 250;
                std::thread::sleep(Duration::from_millis(wait.min(8000) + jitter));
                delay *= 2;
            }
            Err(ureq::Error::Status(code, _)) => return Err(Error::Network(format!("The service answered HTTP {code}."))),
            Err(e) => {
                if attempt == 3 {
                    return Err(Error::Network(format!("No connection to the service ({e}). Local work is unaffected.")));
                }
                std::thread::sleep(Duration::from_millis(delay));
                delay *= 2;
            }
        }
    }
    Err(Error::Network("The request did not complete.".into()))
}

fn from_csl(text: &str) -> Option<Reference> {
    let wrapped = if text.trim_start().starts_with('[') { text.to_string() } else { format!("[{text}]") };
    let p = formats::parse("csljson", &wrapped, None);
    p.records.into_iter().next().map(|p: Parsed| {
        let mut r = p.record;
        r.overflow.retain(|k, _| !matches!(k.as_str(), "csl:reference" | "csl:link" | "csl:license" | "csl:indexed" | "csl:deposited" | "csl:created" | "csl:content-domain" | "csl:member" | "csl:prefix" | "csl:source" | "csl:score" | "csl:references-count" | "csl:is-referenced-by-count" | "csl:published-print" | "csl:published-online" | "csl:published" | "csl:issued" | "csl:journal-issue" | "csl:subject" | "csl:funder" | "csl:assertion" | "csl:alternative-id" | "csl:relation" | "csl:update-policy" | "csl:original-title" | "csl:subtitle"));
        r
    })
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'")
}

/// Where a free, legal copy of a work can be found.
#[derive(Debug, Clone, Serialize, Default)]
pub struct OpenAccess {
    pub pdf_url: String,
    pub landing_url: String,
    pub license: String,
    /// publishedVersion, acceptedVersion, submittedVersion
    pub version: String,
    pub host: String,
    pub is_oa: bool,
}

/// Ask the open scholarly index for the best open-access location of a DOI.
pub fn open_access(doi: &str, log: &mut dyn FnMut(&str, &str, &str, bool, &str)) -> Result<OpenAccess> {
    let d = crate::util::normalize_doi(doi).ok_or_else(|| Error::user("This source has no valid DOI."))?;
    let url = format!("https://api.openalex.org/works/doi:{}?select=open_access,best_oa_location,locations", urlencode_path(&d));
    let res = get(&url, "application/json");
    log("openaccess", "api.openalex.org", &d, res.is_ok(), res.as_ref().err().map(|e| e.to_string()).unwrap_or_default().as_str());
    let v: serde_json::Value = serde_json::from_str(&res?)?;
    let best = &v["best_oa_location"];
    let s = |x: &serde_json::Value| x.as_str().unwrap_or("").to_string();
    let mut oa = OpenAccess {
        pdf_url: s(&best["pdf_url"]),
        landing_url: s(&best["landing_page_url"]),
        license: s(&best["license"]),
        version: s(&best["version"]),
        host: String::new(),
        is_oa: v["open_access"]["is_oa"].as_bool().unwrap_or(false),
    };
    if oa.landing_url.is_empty() {
        oa.landing_url = s(&v["open_access"]["oa_url"]);
    }
    // The best location may list only a web page; take a PDF from another open location,
    // preferring the published version, then the accepted manuscript, then the submitted one.
    if oa.pdf_url.is_empty() {
        let rank = |ver: &str| match ver { "publishedVersion" => 0, "acceptedVersion" => 1, "submittedVersion" => 2, _ => 3 };
        let mut best: Option<&serde_json::Value> = None;
        for l in v["locations"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            if l["is_oa"].as_bool() != Some(true) || l["pdf_url"].as_str().unwrap_or("").is_empty() {
                continue;
            }
            if best.map_or(true, |b| rank(l["version"].as_str().unwrap_or("")) < rank(b["version"].as_str().unwrap_or(""))) {
                best = Some(l);
            }
        }
        if let Some(l) = best {
            oa.pdf_url = s(&l["pdf_url"]);
            oa.license = s(&l["license"]);
            oa.version = s(&l["version"]);
            if !s(&l["landing_page_url"]).is_empty() {
                oa.landing_url = s(&l["landing_page_url"]);
            }
        }
    }
    oa.host = url_host(if oa.pdf_url.is_empty() { &oa.landing_url } else { &oa.pdf_url });
    Ok(oa)
}

/// Download a PDF to `dest`, refusing anything that is not a PDF or is larger than 200 MB.
pub fn download_pdf(url: &str, dest: &std::path::Path, log: &mut dyn FnMut(&str, &str, &str, bool, &str)) -> Result<u64> {
    use std::io::Read;
    if !url.starts_with("https://") {
        return Err(Error::user("The free copy's address is not a secure (https) web address, so it was not downloaded."));
    }
    let host = url_host(url);
    let r = agent().get(url).set("Accept", "application/pdf").call();
    let resp = match r {
        Ok(r) => r,
        Err(e) => {
            log("openaccess-download", &host, url, false, &e.to_string());
            return Err(Error::Network(format!("The free copy could not be downloaded from {host}.")));
        }
    };
    let mut buf = Vec::new();
    resp.into_reader().take(200 * 1024 * 1024 + 1).read_to_end(&mut buf).map_err(|e| Error::Network(e.to_string()))?;
    if buf.len() > 200 * 1024 * 1024 {
        log("openaccess-download", &host, url, false, "larger than 200 MB");
        return Err(Error::user("The free copy is larger than 200 MB and was not downloaded."));
    }
    if !buf.starts_with(b"%PDF") {
        log("openaccess-download", &host, url, false, "not a PDF");
        return Err(Error::user(&format!("{host} returned a web page rather than a PDF. Open the free copy's page instead.")));
    }
    std::fs::write(dest, &buf)?;
    log("openaccess-download", &host, url, true, &format!("{} bytes", buf.len()));
    Ok(buf.len() as u64)
}

/// A DOI found for a source without one, with how closely the found record matches.
#[derive(Debug, Clone, Serialize)]
pub struct DoiMatch {
    pub doi: String,
    pub title: String,
    pub year: Option<i32>,
    pub similarity: f64,
}

/// Search the DOI registry by title, first author and year. Returns the best match only when the
/// title is very close and the year agrees, so the person reviews likely matches, not guesses.
pub fn find_doi(title: &str, surname: &str, year: Option<i32>, log: &mut dyn FnMut(&str, &str, &str, bool, &str)) -> Result<Option<DoiMatch>> {
    let q = format!("{title} {surname} {}", year.map(|y| y.to_string()).unwrap_or_default());
    let enc: String = q.trim().bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else if b == b' ' { "+".into() } else { format!("%{b:02X}") }).collect();
    let url = format!("https://api.crossref.org/works?rows=3&select=DOI,title,issued&query.bibliographic={enc}");
    let res = get(&url, "application/json");
    log("doi-search", "api.crossref.org", q.trim(), res.is_ok(), res.as_ref().err().map(|e| e.to_string()).unwrap_or_default().as_str());
    let v: serde_json::Value = serde_json::from_str(&res?)?;
    let want = crate::util::sort_key(title);
    let mut best: Option<DoiMatch> = None;
    for it in v["message"]["items"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let t = it["title"][0].as_str().unwrap_or("").to_string();
        let y = it["issued"]["date-parts"][0][0].as_i64().map(|y| y as i32);
        let sim = strsim::normalized_levenshtein(&want, &crate::util::sort_key(&t));
        let year_ok = match (year, y) { (Some(a), Some(b)) => (a - b).abs() <= 1, _ => true };
        if sim >= 0.9 && year_ok && best.as_ref().map_or(true, |b| sim > b.similarity) {
            best = Some(DoiMatch { doi: it["DOI"].as_str().unwrap_or("").to_lowercase(), title: t, year: y, similarity: sim });
        }
    }
    Ok(best)
}

/// The abstract the DOI registry holds for a work, as plain text.
pub fn fetch_abstract(doi: &str, log: &mut dyn FnMut(&str, &str, &str, bool, &str)) -> Result<Option<String>> {
    let d = crate::util::normalize_doi(doi).ok_or_else(|| Error::user("This source has no valid DOI."))?;
    let url = format!("https://api.crossref.org/works/{}", urlencode_path(&d));
    let res = get(&url, "application/json");
    log("abstract", "api.crossref.org", &d, res.is_ok(), res.as_ref().err().map(|e| e.to_string()).unwrap_or_default().as_str());
    let v: serde_json::Value = serde_json::from_str(&res?)?;
    let Some(a) = v["message"]["abstract"].as_str() else { return Ok(None) };
    // Strip the markup the registry uses (JATS) and a leading "Abstract" heading.
    let text = re!(r"<[^>]+>").replace_all(a, " ");
    let text = html_unescape(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    let text = text.trim_start_matches("Abstract").trim_start_matches(['.', ':', ' ']).to_string();
    Ok(if text.len() < 40 { None } else { Some(text) })
}

/// Result of checking one web address.
#[derive(Debug, Clone, Serialize)]
pub struct LinkStatus {
    pub url: String,
    /// working, moved, broken, unreachable
    pub state: String,
    pub code: u16,
    pub final_url: String,
    pub message: String,
}

pub fn check_link(url: &str, log: &mut dyn FnMut(&str, &str, &str, bool, &str)) -> LinkStatus {
    let host = url_host(url);
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return LinkStatus { url: url.into(), state: "unreachable".into(), code: 0, final_url: String::new(), message: "Only web addresses are checked.".into() };
    }
    let a = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout(Duration::from_secs(15)).user_agent(USER_AGENT).redirects(5).resolver(PublicOnly).build();
    let st = match a.head(url).call().or_else(|_| a.get(url).call()) {
        Ok(r) => {
            let fin = r.get_url().to_string();
            let moved = url_host(&fin) != host;
            LinkStatus { url: url.into(), state: if moved { "moved".into() } else { "working".into() }, code: r.status(), message: if moved { format!("Now leads to {}", url_host(&fin)) } else { "Works".into() }, final_url: fin }
        }
        Err(ureq::Error::Status(code, r)) => LinkStatus { url: url.into(), state: if code == 404 || code == 410 { "broken".into() } else { "unreachable".into() }, code, final_url: r.get_url().to_string(), message: format!("The site answered HTTP {code}") },
        Err(e) => LinkStatus { url: url.into(), state: "unreachable".into(), code: 0, final_url: String::new(), message: format!("No answer ({e})") },
    };
    log("linkcheck", &host, url, st.state == "working" || st.state == "moved", &st.message);
    st
}

fn url_host(u: &str) -> String {
    u.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or("").to_string()
}

fn urlencode_path(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "-._~/:".contains(c) { c.to_string() } else { c.to_string().bytes().map(|b| format!("%{b:02X}")).collect() }).collect()
}

/// Resolve one identifier. `kind`: doi, pmid, pmcid, arxiv, isbn, url.
/// The network switch (Settings › Network) a lookup of this identifier kind falls under.
pub fn switch_for(kind: &str) -> &'static str {
    match kind {
        "pmid" | "pmcid" => "pubmed",
        "url" => "webpage",
        "arxiv" => "arxiv",
        "isbn" => "isbn",
        _ => "doi",
    }
}

/// Look up one identifier. `may` says whether a kind of request is allowed (the person's switches
/// in Settings › Network and the project's policy): it is asked before every request, including
/// the second DOI check, so a switch that is off stops exactly that request.
pub fn resolve(kind: &str, id: &str, may: &dyn Fn(&str) -> bool, log: &mut dyn FnMut(&str, &str, &str, bool, &str)) -> LookupResult {
    let mut res = LookupResult { kind: kind.into(), id: id.into(), candidate: None, corroboration: vec![], conflicts: vec![], message: String::new(), ok: false };
    if !may(switch_for(kind)) {
        res.message = "This kind of lookup is turned off in Settings › Network.".into();
        return res;
    }
    let mut web_base: Option<f32> = None;
    let outcome: Result<Reference> = (|| match kind {
        "doi" => {
            let doi = crate::util::normalize_doi(id).ok_or_else(|| Error::user("That is not a DOI."))?;
            let url = format!("https://doi.org/{}", doi);
            let body = get(&url, "application/vnd.citationstyles.csl+json");
            log("doi", "doi.org", &doi, body.is_ok(), body.as_ref().err().map(|e| e.to_string()).unwrap_or_default().as_str());
            let mut r = from_csl(&body?).ok_or_else(|| Error::Network("The resolver returned no usable metadata.".into()))?;
            r.fields.insert("doi".into(), doi.clone());
            // Independent corroboration where available, when that request is allowed.
            let second = if may("doi-corroborate") { get(&format!("https://api.crossref.org/works/{}", urlencode(&doi)), "application/json") } else { Err(Error::user("turned off")) };
            match second {
                Ok(b) => {
                    log("doi-corroborate", "api.crossref.org", &doi, true, "");
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&b) {
                        let m = &v["message"];
                        let t = m["title"][0].as_str().unwrap_or("");
                        if !t.is_empty() {
                            if crate::strsim_ratio(t, r.title()) > 0.9 {
                                res.corroboration.push("Title confirmed by a second resolver".into());
                            } else {
                                res.conflicts.push(format!("A second resolver gives the title “{t}”."));
                            }
                        }
                        if let Some(y) = m["issued"]["date-parts"][0][0].as_i64() {
                            if r.year() == Some(y as i32) {
                                res.corroboration.push("Year confirmed".into());
                            } else {
                                res.conflicts.push(format!("A second resolver gives the year {y}."));
                            }
                        }
                        if let Some(arr) = m["update-to"].as_array() {
                            for u in arr {
                                let kind = u["type"].as_str().unwrap_or("");
                                if kind.contains("retract") || kind.contains("correct") || kind.contains("concern") {
                                    r.notices.push(crate::model::Notice { kind: kind.into(), date: u["updated"]["date-time"].as_str().unwrap_or("").into(), source: "Crossref".into(), url: format!("https://doi.org/{}", u["DOI"].as_str().unwrap_or("")), note: "Reported by the publisher's registration metadata.".into() });
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log("doi-corroborate", "api.crossref.org", &doi, false, &e.to_string());
                    res.corroboration.push("No second resolver was reachable; review the record.".into());
                }
            }
            Ok(r)
        }
        "pmid" | "pmcid" => {
            let db = if kind == "pmid" { "pubmed" } else { "pmc" };
            let idv = id.trim().trim_start_matches("PMC").trim_start_matches("pmc");
            if idv.is_empty() || idv.len() > 12 || !idv.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::user("A PubMed or PMC identifier is a number."));
            }
            let url = format!("https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db={db}&id={idv}&retmode=json");
            let body = get(&url, "application/json");
            log("pubmed", "eutils.ncbi.nlm.nih.gov", id, body.is_ok(), "");
            let v: serde_json::Value = serde_json::from_str(&body?)?;
            let item = &v["result"][idv];
            if item.is_null() {
                return Err(Error::Network("No record was found for that identifier.".into()));
            }
            let mut r = Reference { rtype: "article-journal".into(), ..Default::default() };
            r.fields.insert("title".into(), item["title"].as_str().unwrap_or("").trim_end_matches('.').to_string());
            r.fields.insert("container_title".into(), item["fulljournalname"].as_str().or(item["source"].as_str()).unwrap_or("").to_string());
            r.fields.insert("volume".into(), item["volume"].as_str().unwrap_or("").into());
            r.fields.insert("issue".into(), item["issue"].as_str().unwrap_or("").into());
            r.fields.insert("page".into(), item["pages"].as_str().unwrap_or("").into());
            r.fields.insert(kind.into(), id.into());
            if let Some(ids) = item["articleids"].as_array() {
                for i in ids {
                    if i["idtype"] == "doi" {
                        r.fields.insert("doi".into(), i["value"].as_str().unwrap_or("").into());
                    }
                }
            }
            r.issued = item["pubdate"].as_str().and_then(PartialDate::parse);
            if let Some(a) = item["authors"].as_array() {
                for au in a {
                    // "Chen L" form: surname then initials.
                    let n = au["name"].as_str().unwrap_or("");
                    let (fam, ini) = n.rsplit_once(' ').unwrap_or((n, ""));
                    r.creators.push(crate::model::Creator { role: "author".into(), name: crate::model::Name::person(fam, &ini.chars().map(|c| format!("{c}.")).collect::<Vec<_>>().join(" ")) });
                }
            }
            Ok(r)
        }
        "arxiv" => {
            let url = format!("https://export.arxiv.org/api/query?id_list={}", urlencode(id.trim()));
            let body = get(&url, "application/atom+xml");
            log("arxiv", "export.arxiv.org", id, body.is_ok(), "");
            let body = body?;
            let entry = body.split("<entry>").nth(1).ok_or_else(|| Error::Network("No arXiv record was found.".into()))?;
            let tag = |t: &str| -> String { entry.split(&format!("<{t}>")).nth(1).and_then(|s| s.split(&format!("</{t}>")).next()).map(|s| html_unescape(&s.split_whitespace().collect::<Vec<_>>().join(" "))).unwrap_or_default() };
            let mut r = Reference { rtype: "preprint".into(), ..Default::default() };
            r.fields.insert("title".into(), tag("title"));
            r.fields.insert("abstract".into(), tag("summary"));
            r.fields.insert("arxiv".into(), id.trim().into());
            r.fields.insert("publisher".into(), "arXiv".into());
            r.fields.insert("url".into(), format!("https://arxiv.org/abs/{}", id.trim()));
            r.issued = PartialDate::parse(&tag("published").chars().take(10).collect::<String>());
            for a in entry.split("<author>").skip(1) {
                let n = a.split("<name>").nth(1).and_then(|s| s.split("</name>").next()).unwrap_or("");
                r.creators.push(formats::person("author", &html_unescape(n)));
            }
            if let Some(d) = entry.split("<arxiv:doi").nth(1).and_then(|s| s.split('>').nth(1)).and_then(|s| s.split('<').next()) {
                r.overflow.insert("arxiv:published-doi".into(), d.into());
            }
            Ok(r)
        }
        "isbn" => {
            let isbn: String = id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            let url = format!("https://openlibrary.org/api/books?bibkeys=ISBN:{isbn}&format=json&jscmd=data");
            let body = get(&url, "application/json");
            log("isbn", "openlibrary.org", &isbn, body.is_ok(), "");
            let v: serde_json::Value = serde_json::from_str(&body?)?;
            let item = &v[format!("ISBN:{isbn}")];
            if item.is_null() {
                return Err(Error::Network("No book was found for that ISBN.".into()));
            }
            let mut r = Reference { rtype: "book".into(), ..Default::default() };
            let title = match item["subtitle"].as_str() {
                Some(s) => format!("{}: {}", item["title"].as_str().unwrap_or(""), s),
                None => item["title"].as_str().unwrap_or("").into(),
            };
            r.fields.insert("title".into(), title);
            r.fields.insert("isbn".into(), isbn.clone());
            r.fields.insert("publisher".into(), item["publishers"][0]["name"].as_str().unwrap_or("").into());
            r.fields.insert("publisher_place".into(), item["publish_places"][0]["name"].as_str().unwrap_or("").into());
            r.issued = item["publish_date"].as_str().and_then(PartialDate::parse);
            if let Some(a) = item["authors"].as_array() {
                for au in a {
                    r.creators.push(formats::person("author", au["name"].as_str().unwrap_or("")));
                }
            }
            Ok(r)
        }
        "url" => {
            if !id.starts_with("https://") {
                return Err(Error::user("Only secure (https) web pages are looked up."));
            }
            let body = get(id, "text/html,application/xhtml+xml");
            let host = id.split('/').nth(2).unwrap_or("").to_string();
            log("webpage", &host, id, body.is_ok(), "");
            let html = body?;
            // How much the page says about itself: scholarly citation tags, general tags, or
            // only a page title.
            let low = html.to_lowercase();
            web_base = Some(if low.contains("citation_title") { 0.8 } else if low.contains("dc.title") || low.contains("og:title") { 0.65 } else { 0.5 });
            Ok(from_meta_tags(&html, id))
        }
        _ => Err(Error::user("Unknown identifier type.")),
    })();
    match outcome {
        Ok(mut r) => {
            formats::finish(&mut r);
            r.origin = "import".into();
            crate::provenance::stamp(&mut r, &format!("lookup:{kind}"), web_base, &crate::util::now());
            for c in &res.conflicts {
                if c.contains("title") {
                    crate::provenance::contradicted(&mut r, "title");
                }
                if c.contains("year") {
                    crate::provenance::contradicted(&mut r, "issued");
                }
            }
            res.ok = true;
            res.message = if res.conflicts.is_empty() { "Found. Review before adding.".into() } else { "Found, with disagreements between resolvers. Review carefully.".into() };
            res.candidate = Some(r);
        }
        Err(e) => res.message = e.to_string(),
    }
    res
}

/// Embedded citation metadata (citation_*, Dublin Core, Open Graph) as a candidate; it never
/// overrides a person's corrections.
pub fn from_meta_tags(html: &str, url: &str) -> Reference {
    let re = re!(r#"(?is)<meta\s+[^>]*?(?:name|property)\s*=\s*["']([^"']+)["'][^>]*?content\s*=\s*["']([^"']*)["'][^>]*>"#);
    let re2 = re!(r#"(?is)<meta\s+[^>]*?content\s*=\s*["']([^"']*)["'][^>]*?(?:name|property)\s*=\s*["']([^"']+)["'][^>]*>"#);
    let mut tags: Vec<(String, String)> = re.captures_iter(html).map(|c| (c[1].to_lowercase(), html_unescape(&c[2]))).collect();
    tags.extend(re2.captures_iter(html).map(|c| (c[2].to_lowercase(), html_unescape(&c[1]))));
    let first = |k: &str| tags.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let mut r = Reference { rtype: if !first("citation_journal_title").is_empty() { "article-journal".into() } else { "webpage".into() }, ..Default::default() };
    let title = [first("citation_title"), first("dc.title"), first("og:title")].into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| {
        html.split("<title>").nth(1).and_then(|s| s.split("</title>").next()).map(|s| html_unescape(s.trim())).unwrap_or_default()
    });
    r.fields.insert("title".into(), title);
    r.fields.insert("container_title".into(), [first("citation_journal_title"), first("og:site_name")].into_iter().find(|s| !s.is_empty()).unwrap_or_default());
    r.fields.insert("volume".into(), first("citation_volume"));
    r.fields.insert("issue".into(), first("citation_issue"));
    let (fp, lp) = (first("citation_firstpage"), first("citation_lastpage"));
    r.fields.insert("page".into(), formats::pages(&fp, &lp));
    r.fields.insert("doi".into(), first("citation_doi"));
    r.fields.insert("publisher".into(), first("citation_publisher"));
    r.fields.insert("isbn".into(), first("citation_isbn"));
    r.fields.insert("url".into(), url.into());
    r.fields.insert("abstract".into(), first("citation_abstract"));
    let date = [first("citation_publication_date"), first("citation_date"), first("dc.date"), first("article:published_time")].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
    r.issued = PartialDate::parse(&date.replace('/', "-").chars().take(10).collect::<String>());
    for (n, v) in &tags {
        if n == "citation_author" || n == "dc.creator" {
            r.creators.push(formats::person("author", v));
        }
    }
    if r.creators.is_empty() {
        let a = first("author");
        if !a.is_empty() {
            r.creators.push(formats::person("author", &a));
        }
    }
    if !first("citation_pdf_url").is_empty() {
        r.overflow.insert("web:pdf_url".into(), first("citation_pdf_url"));
    }
    let today = chrono::Utc::now();
    r.accessed = Some(PartialDate::ymd(today.format("%Y").to_string().parse().unwrap_or(2026), today.format("%m").to_string().parse().unwrap_or(1), today.format("%d").to_string().parse().unwrap_or(1)));
    r
}

fn urlencode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => o.push(b as char),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

#[cfg(test)]
mod address_tests {
    use super::is_public;
    #[test]
    fn only_public_addresses_are_reachable() {
        for a in ["127.0.0.1", "10.0.0.5", "192.168.1.1", "172.16.0.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "::1", "fd00::1", "fe80::1", "::ffff:127.0.0.1"] {
            assert!(!is_public(a.parse().unwrap()), "{a} must be refused");
        }
        for a in ["8.8.8.8", "151.101.1.69", "2606:4700::6810:84e5"] {
            assert!(is_public(a.parse().unwrap()), "{a} is public");
        }
        // A name that resolves to this computer is refused at connect time.
        assert!(ureq::Resolver::resolve(&super::PublicOnly, "localhost:443").is_err());
    }
}
