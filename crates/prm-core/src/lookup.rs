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

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(25)).user_agent(USER_AGENT).build()
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
        r.overflow.retain(|k, _| !matches!(k.as_str(), "csl:reference" | "csl:link" | "csl:license" | "csl:indexed" | "csl:deposited" | "csl:created" | "csl:content-domain" | "csl:member" | "csl:prefix" | "csl:source" | "csl:score" | "csl:references-count" | "csl:is-referenced-by-count" | "csl:published-print" | "csl:published-online" | "csl:published" | "csl:issued" | "csl:journal-issue" | "csl:subject" | "csl:funder" | "csl:assertion" | "csl:alternative-id" | "csl:relation" | "csl:update-policy" | "csl:container-title-short" | "csl:short-container-title" | "csl:original-title" | "csl:subtitle"));
        r
    })
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'")
}

/// Resolve one identifier. `kind`: doi, pmid, pmcid, arxiv, isbn, url.
pub fn resolve(kind: &str, id: &str, log: &mut dyn FnMut(&str, &str, &str, bool, &str)) -> LookupResult {
    let mut res = LookupResult { kind: kind.into(), id: id.into(), candidate: None, corroboration: vec![], conflicts: vec![], message: String::new(), ok: false };
    let outcome: Result<Reference> = (|| match kind {
        "doi" => {
            let doi = crate::util::normalize_doi(id).ok_or_else(|| Error::user("That is not a DOI."))?;
            let url = format!("https://doi.org/{}", doi);
            let body = get(&url, "application/vnd.citationstyles.csl+json");
            log("doi", "doi.org", &doi, body.is_ok(), body.as_ref().err().map(|e| e.to_string()).unwrap_or_default().as_str());
            let mut r = from_csl(&body?).ok_or_else(|| Error::Network("The resolver returned no usable metadata.".into()))?;
            r.fields.insert("doi".into(), doi.clone());
            // Independent corroboration where available.
            match get(&format!("https://api.crossref.org/works/{}", urlencode(&doi)), "application/json") {
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
            let idv = id.trim().trim_start_matches("PMC");
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
            let body = get(id, "text/html,application/xhtml+xml");
            let host = id.split('/').nth(2).unwrap_or("").to_string();
            log("webpage", &host, id, body.is_ok(), "");
            let html = body?;
            Ok(from_meta_tags(&html, id))
        }
        _ => Err(Error::user("Unknown identifier type.")),
    })();
    match outcome {
        Ok(mut r) => {
            formats::finish(&mut r);
            r.origin = "import".into();
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
