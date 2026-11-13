// Turns what a page declares (citation meta tags, structured data, embedded citation spans,
// identifier links) into candidate records in CSL JSON. Candidates are reviewed in the program
// before they join the library; nothing here is treated as authoritative.

const first = (meta, ...keys) => {
  for (const k of keys) if (meta[k] && meta[k][0]) return meta[k][0];
  return "";
};
const all = (meta, ...keys) => keys.flatMap((k) => meta[k] || []);

export function nameOf(s) {
  const t = String(s || "").trim().replace(/\s+/g, " ");
  if (!t) return null;
  if (t.includes(",")) {
    const [family, ...rest] = t.split(",");
    return { family: family.trim(), given: rest.join(",").trim() };
  }
  const parts = t.split(" ");
  if (parts.length === 1) return { family: parts[0] };
  return { family: parts.pop(), given: parts.join(" ") };
}

export function dateOf(s) {
  const t = String(s || "").trim();
  const m = t.match(/(\d{4})(?:[-/.](\d{1,2}))?(?:[-/.](\d{1,2}))?/);
  if (!m) return null;
  const parts = [+m[1]];
  if (m[2] && +m[2] >= 1 && +m[2] <= 12) {
    parts.push(+m[2]);
    if (m[3]) parts.push(+m[3]);
  }
  return { "date-parts": [parts] };
}

export function normalizeDoi(s) {
  const m = String(s || "").match(/10\.\d{4,9}\/[^\s"<>?#]+/i);
  return m ? m[0].replace(/[.,;)\]]+$/, "") : "";
}

function fromJsonLd(list) {
  const kinds = ["ScholarlyArticle", "Article", "NewsArticle", "BlogPosting", "Report", "Thesis", "Book", "Chapter", "Dataset", "SoftwareSourceCode"];
  const item = list.find((x) => [].concat(x["@type"] || []).some((t) => kinds.includes(t)));
  if (!item) return null;
  const type = [].concat(item["@type"])[0];
  const csl = {
    type: { ScholarlyArticle: "article-journal", NewsArticle: "article-newspaper", BlogPosting: "post-weblog", Report: "report", Thesis: "thesis", Book: "book", Chapter: "chapter", Dataset: "dataset", SoftwareSourceCode: "software" }[type] || "webpage",
    title: item.headline || item.name || "",
  };
  const authors = [].concat(item.author || []).map((a) => (typeof a === "string" ? nameOf(a) : a && a["@type"] === "Organization" ? { literal: a.name } : nameOf(a && (a.name || [a.familyName, a.givenName].filter(Boolean).join(", "))))).filter(Boolean);
  if (authors.length) csl.author = authors;
  const d = dateOf(item.datePublished || item.dateCreated);
  if (d) csl.issued = d;
  const part = item.isPartOf;
  const container = part && (part.name || (part.isPartOf && part.isPartOf.name));
  if (container) csl["container-title"] = container;
  if (item.publisher) csl.publisher = typeof item.publisher === "string" ? item.publisher : item.publisher.name;
  const doi = normalizeDoi([].concat(item.identifier || [], item.sameAs || [], item["@id"] || []).map((x) => (typeof x === "object" ? x.value || x["@id"] || "" : x)).join(" "));
  if (doi) csl.DOI = doi;
  if (item.description) csl.abstract = String(item.description).slice(0, 5000);
  if (item.url) csl.URL = item.url;
  return csl;
}

/** The record a single-article page describes, or null. */
export function fromMeta(page) {
  const m = page.meta;
  const title = first(m, "citation_title", "dc.title", "dcterms.title", "prism.title");
  const ld = fromJsonLd(page.jsonld || []);
  if (!title && !(ld && ld.title)) return null;
  const csl = { type: "webpage", title: title || ld.title };
  if (first(m, "citation_journal_title", "prism.publicationname")) {
    csl.type = "article-journal";
    csl["container-title"] = first(m, "citation_journal_title", "prism.publicationname");
  } else if (first(m, "citation_conference_title")) {
    csl.type = "paper-conference";
    csl["container-title"] = first(m, "citation_conference_title");
  } else if (first(m, "citation_dissertation_institution")) {
    csl.type = "thesis";
    csl.publisher = first(m, "citation_dissertation_institution");
  } else if (first(m, "citation_technical_report_institution")) {
    csl.type = "report";
    csl.publisher = first(m, "citation_technical_report_institution");
    const n = first(m, "citation_technical_report_number");
    if (n) csl.number = n;
  } else if (first(m, "citation_inbook_title", "citation_book_title")) {
    csl.type = first(m, "citation_inbook_title") ? "chapter" : "book";
    if (first(m, "citation_inbook_title")) csl["container-title"] = first(m, "citation_inbook_title");
  } else if (ld) {
    csl.type = ld.type;
    if (ld["container-title"]) csl["container-title"] = ld["container-title"];
  }
  const authors = all(m, "citation_author", "dc.creator", "dcterms.creator").map(nameOf).filter(Boolean);
  if (authors.length) csl.author = authors;
  else if (ld && ld.author) csl.author = ld.author;
  const d = dateOf(first(m, "citation_publication_date", "citation_date", "citation_online_date", "dc.date", "dcterms.issued", "prism.publicationdate", "article:published_time")) || (ld && ld.issued);
  if (d) csl.issued = d;
  const set = (k, ...keys) => { const v = first(m, ...keys); if (v) csl[k] = v; };
  set("volume", "citation_volume", "prism.volume");
  set("issue", "citation_issue", "prism.number");
  const fp = first(m, "citation_firstpage", "prism.startingpage"), lp = first(m, "citation_lastpage", "prism.endingpage");
  if (fp) csl.page = lp && lp !== fp ? `${fp}-${lp}` : fp;
  set("publisher", "citation_publisher", "dc.publisher", "dcterms.publisher");
  set("ISBN", "citation_isbn", "prism.isbn");
  set("ISSN", "citation_issn", "prism.issn");
  set("language", "citation_language", "dc.language");
  const abs = first(m, "citation_abstract", "dc.description", "dcterms.abstract") || (ld && ld.abstract) || "";
  if (abs) csl.abstract = abs.slice(0, 5000);
  const doi = normalizeDoi(first(m, "citation_doi", "dc.identifier", "dcterms.identifier", "prism.doi", "bepress_citation_doi")) || (ld && ld.DOI) || "";
  if (doi) csl.DOI = doi;
  const kw = all(m, "citation_keywords", "keywords", "dc.subject");
  if (kw.length) csl.keyword = kw.join(", ");
  if (csl.type === "webpage") {
    const site = first(m, "og:site_name");
    if (site) csl["container-title"] = site;
  }
  csl.URL = page.url;
  return csl;
}

/** Parse an embedded OpenURL citation span (COinS). */
export function fromCoins(title) {
  const p = new URLSearchParams(String(title || "").replace(/&amp;/g, "&"));
  const g = (k) => p.get(k) || "";
  const csl = { title: g("rft.atitle") || g("rft.title") || g("rft.btitle") };
  if (!csl.title) return null;
  const genre = g("rft.genre");
  csl.type = genre === "book" ? "book" : genre === "bookitem" ? "chapter" : genre === "proceeding" || genre === "conference" ? "paper-conference" : g("rft.jtitle") ? "article-journal" : "document";
  if (g("rft.jtitle")) csl["container-title"] = g("rft.jtitle");
  else if (g("rft.btitle") && csl.type === "chapter") csl["container-title"] = g("rft.btitle");
  const authors = p.getAll("rft.au").map(nameOf).filter(Boolean);
  if (!authors.length && g("rft.aulast")) authors.push({ family: g("rft.aulast"), given: g("rft.aufirst") || g("rft.auinit") });
  if (authors.length) csl.author = authors;
  const d = dateOf(g("rft.date"));
  if (d) csl.issued = d;
  if (g("rft.volume")) csl.volume = g("rft.volume");
  if (g("rft.issue")) csl.issue = g("rft.issue");
  if (g("rft.spage")) csl.page = g("rft.epage") ? `${g("rft.spage")}-${g("rft.epage")}` : g("rft.spage");
  if (g("rft.isbn")) csl.ISBN = g("rft.isbn");
  if (g("rft.pub")) csl.publisher = g("rft.pub");
  for (const id of p.getAll("rft_id")) {
    const doi = normalizeDoi(id);
    if (doi) csl.DOI = doi;
    else if (/^https?:/i.test(id)) csl.URL = id;
  }
  return csl;
}

/**
 * Candidates for a page: `primary` is the article the page is about (if any); `others` are
 * papers listed or linked on it (a results page, or an article's reference list).
 */
export function candidates(page) {
  const primaryCsl = fromMeta(page);
  const pdfMeta = first(page.meta, "citation_pdf_url");
  let primary = null;
  if (primaryCsl) {
    primary = { key: "primary", csl: primaryCsl, identifiers: primaryCsl.DOI ? { doi: primaryCsl.DOI } : {}, url: page.url, title: primaryCsl.title, pdf_url: pdfMeta || "" };
  } else if (page.isPdf) {
    const name = decodeURIComponent((page.url.split(/[?#]/)[0].split("/").pop() || "document.pdf"));
    primary = { key: "primary", csl: null, identifiers: {}, url: page.url, title: name, pdf_url: page.url };
  } else if (page.dois && page.dois.length === 1) {
    primary = { key: "primary", csl: null, identifiers: { doi: page.dois[0] }, url: page.url, title: page.title, pdf_url: page.pdfLinks && page.pdfLinks.length === 1 ? page.pdfLinks[0] : "" };
  }
  const others = [];
  const seen = new Set();
  const primaryDoi = (primary && primary.identifiers.doi || "").toLowerCase();
  if (primaryDoi) seen.add("doi:" + primaryDoi);
  (page.coins || []).forEach((c, i) => {
    const csl = fromCoins(c);
    if (!csl) return;
    const k = csl.DOI ? "doi:" + csl.DOI.toLowerCase() : "title:" + csl.title.toLowerCase();
    if (seen.has(k)) return;
    seen.add(k);
    others.push({ key: "coins" + i, csl, identifiers: csl.DOI ? { doi: csl.DOI } : {}, url: csl.URL || page.url, title: csl.title, pdf_url: "" });
  });
  (page.links || []).forEach((l, i) => {
    const k = l.kind + ":" + l.id.toLowerCase();
    if (seen.has(k)) return;
    seen.add(k);
    others.push({ key: "link" + i, csl: null, identifiers: { [l.kind]: l.id }, url: l.href, title: l.text || `${l.kind.toUpperCase()} ${l.id}`, pdf_url: "" });
  });
  if (!primary && !others.length) {
    // Any page can still be saved as a web page.
    primary = { key: "primary", csl: { type: "webpage", title: page.title || page.url, URL: page.url, "container-title": first(page.meta || {}, "og:site_name") || "" }, identifiers: {}, url: page.url, title: page.title || page.url, pdf_url: "" };
  }
  return { primary, others };
}
