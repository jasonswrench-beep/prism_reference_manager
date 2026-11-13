//! Mapped CSV and tab-separated import, and CSV export.

use super::{person, split_names, type_from_label, ParseOutput, Parsed};
use crate::model::{PartialDate, Reference};
use std::collections::BTreeMap;

/// Suggest a target for a column header. Targets: type, authors, editors, year, date, title,
/// container_title, volume, issue, page, article_number, doi, url, publisher, abstract,
/// keywords, isbn, issn, edition, language, note, ignore. Anything else is kept in overflow.
pub fn suggest(header: &str) -> &'static str {
    let h = header.trim().to_lowercase();
    match h.as_str() {
        "type" | "item type" | "reference type" | "publication type" | "document type" => "type",
        "author" | "authors" | "creator" | "creators" | "author(s)" | "author full names" => "authors",
        "editor" | "editors" => "editors",
        "year" | "publication year" | "pub year" | "py" => "year",
        "date" | "publication date" | "published" => "date",
        "title" | "article title" | "paper title" | "document title" => "title",
        "journal" | "source" | "source title" | "publication" | "publication title" | "venue" | "container" | "journal title" => "container_title",
        "volume" | "vol" => "volume",
        "issue" | "number" | "no" => "issue",
        "pages" | "page" | "page range" => "page",
        "article number" | "eid" => "article_number",
        "doi" | "doi link" => "doi",
        "url" | "link" | "paper link" => "url",
        "publisher" => "publisher",
        "abstract" | "summary" => "abstract",
        "keywords" | "tags" | "author keywords" => "keywords",
        "isbn" => "isbn",
        "issn" => "issn",
        "edition" => "edition",
        "language" => "language",
        "notes" | "note" | "extra" => "note",
        "pmid" | "pubmed id" => "pmid",
        _ => "",
    }
}

pub fn parse(text: &str, mapping: Option<&BTreeMap<String, String>>) -> ParseOutput {
    let mut out = ParseOutput::default();
    let first = text.lines().next().unwrap_or("");
    let delim = if first.matches('\t').count() > first.matches(',').count() { b'\t' } else if first.matches(';').count() > first.matches(',').count() { b';' } else { b',' };
    let mut rdr = csv::ReaderBuilder::new().delimiter(delim).flexible(true).from_reader(text.as_bytes());
    let headers: Vec<String> = match rdr.headers() {
        Ok(h) => h.iter().map(|s| s.to_string()).collect(),
        Err(e) => {
            out.errors.push(format!("The spreadsheet header could not be read: {e}"));
            return out;
        }
    };
    let targets: Vec<String> = headers
        .iter()
        .map(|h| mapping.and_then(|m| m.get(h).cloned()).unwrap_or_else(|| suggest(h).to_string()))
        .collect();
    for (row_i, rec) in rdr.records().enumerate() {
        let rec = match rec {
            Ok(r) => r,
            Err(e) => {
                out.errors.push(format!("Row {} could not be read: {e}", row_i + 2));
                continue;
            }
        };
        let mut r = Reference::default();
        r.origin = "import".into();
        r.rtype = "article-journal".into();
        let mut raw = Vec::new();
        for (i, val) in rec.iter().enumerate() {
            let val = val.trim();
            let header = headers.get(i).cloned().unwrap_or_else(|| format!("column{i}"));
            raw.push(format!("{header}: {val}"));
            if val.is_empty() {
                continue;
            }
            let target = targets.get(i).map(|s| s.as_str()).unwrap_or("");
            match target {
                "ignore" => {}
                "type" => r.rtype = type_from_label(val).to_string(),
                "authors" => r.creators.extend(split_names(val).iter().map(|n| person("author", n))),
                "editors" => r.creators.extend(split_names(val).iter().map(|n| person("editor", n))),
                "year" | "date" => {
                    if r.issued.as_ref().map(|d| d.month.is_none()).unwrap_or(true) {
                        r.issued = PartialDate::parse(val);
                    }
                }
                "keywords" => r.tags.extend(val.split([';', ',']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())),
                "" => {
                    r.overflow.insert(format!("csv:{header}"), val.to_string());
                }
                field => {
                    r.fields.insert(field.to_string(), val.to_string());
                }
            }
        }
        out.records.push(Parsed { raw: raw.join("\n"), record: r, issues: vec![], confidence: 1.0 });
    }
    out
}

/// Export the main fields as CSV, one row per reference, with the stable id first so a
/// spreadsheet edit can be matched back.
pub fn export(refs: &[&Reference]) -> crate::Result<String> {
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(["id", "citekey", "type", "authors", "editors", "year", "date", "title", "container_title", "volume", "issue", "page", "article_number", "doi", "url", "publisher", "publisher_place", "edition", "isbn", "issn", "abstract", "keywords", "language", "note"])?;
    for r in refs {
        let names = |role: &str| {
            r.creators
                .iter()
                .filter(|c| c.role == role)
                .map(|c| if c.name.literal.is_empty() { format!("{}, {}", c.name.surname(), c.name.given).trim_end_matches(", ").to_string() } else { c.name.literal.clone() })
                .collect::<Vec<_>>()
                .join("; ")
        };
        let year = r.year().map(|y| y.to_string()).unwrap_or_default();
        let date = r.issued.as_ref().map(|d| if d.status.is_empty() { d.iso() } else { d.status.clone() }).unwrap_or_default();
        w.write_record([
            r.id.as_str(),
            &r.citekey,
            &r.rtype,
            &names("author"),
            &names("editor"),
            &year,
            &date,
            r.title(),
            r.f("container_title"),
            r.f("volume"),
            r.f("issue"),
            r.f("page"),
            r.f("article_number"),
            r.f("doi"),
            r.f("url"),
            r.f("publisher"),
            r.f("publisher_place"),
            r.f("edition"),
            r.f("isbn"),
            r.f("issn"),
            r.f("abstract"),
            &r.tags.join("; "),
            r.f("language"),
            r.f("note"),
        ])?;
    }
    let bytes = w.into_inner().map_err(|e| crate::Error::user(e.to_string()))?;
    // A byte-order mark helps spreadsheet programs detect UTF-8.
    Ok(format!("\u{feff}{}", String::from_utf8_lossy(&bytes)))
}
