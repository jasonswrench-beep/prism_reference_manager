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
        "collections" | "collection" | "folders" | "folder" | "groups" | "group" => "collections",
        "rating" | "stars" => "rating",
        "id" | "record id" | "item key" | "key" => "source_id",
        h if h.starts_with("custom:") => "custom",
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
                // Group or folder names, separated by semicolons; nested groups as Parent/Child.
                "collections" => {
                    r.overflow.insert("import:groups".into(), val.to_string());
                }
                "rating" => {
                    r.overflow.insert("import:rating".into(), val.to_string());
                }
                // The record's identity in the exporting program, for recognizing it on re-import.
                "source_id" => {
                    r.overflow.insert("import:source-id".into(), val.to_string());
                }
                "custom" => {
                    let id = header.trim().trim_start_matches("custom:").to_string();
                    r.custom.insert(id, serde_json::Value::String(val.to_string()));
                }
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

/// A cell a spreadsheet would read as a formula (it starts with =, +, -, @, a tab or a carriage
/// return) gets a leading apostrophe, so titles, notes and links from imported records cannot run
/// as formulas when the file is opened.
pub fn inert(cell: &str) -> std::borrow::Cow<'_, str> {
    if cell.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        std::borrow::Cow::Owned(format!("'{cell}"))
    } else {
        std::borrow::Cow::Borrowed(cell)
    }
}

/// Export the main fields as CSV, one row per reference, with the stable id first so a
/// spreadsheet edit can be matched back.
pub fn export(refs: &[&Reference]) -> crate::Result<String> {
    let (head, rows) = table(refs);
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(&head)?;
    for r in rows {
        w.write_record(r.iter().map(|c| inert(c).into_owned()))?;
    }
    let bytes = w.into_inner().map_err(|e| crate::Error::user(e.to_string()))?;
    // A byte-order mark helps spreadsheet programs detect UTF-8.
    Ok(format!("\u{feff}{}", String::from_utf8_lossy(&bytes)))
}

/// The same table as a spreadsheet (.xlsx): bold header row, frozen, with a filter and sensible widths.
pub fn export_xlsx(refs: &[&Reference]) -> crate::Result<Vec<u8>> {
    use rust_xlsxwriter::{Format, Workbook};
    let (head, rows) = table(refs);
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let x = |e: rust_xlsxwriter::XlsxError| crate::Error::user(e.to_string());
    ws.set_name("Sources").map_err(x)?;
    let bold = Format::new().set_bold();
    for (c, h) in head.iter().enumerate() {
        ws.write_string_with_format(0, c as u16, h, &bold).map_err(x)?;
    }
    for (i, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            if !v.is_empty() {
                // Values stay text so identifiers and page ranges are never reinterpreted.
                ws.write_string(i as u32 + 1, c as u16, v).map_err(x)?;
            }
        }
    }
    for (c, h) in head.iter().enumerate() {
        let w = match h.as_str() { "title" | "abstract" => 60.0, "authors" | "container_title" => 32.0, "id" => 38.0, _ => 14.0 };
        ws.set_column_width(c as u16, w).map_err(x)?;
    }
    ws.set_freeze_panes(1, 0).map_err(x)?;
    if !rows.is_empty() {
        ws.autofilter(0, 0, rows.len() as u32, head.len() as u16 - 1).map_err(x)?;
    }
    wb.save_to_buffer().map_err(x)
}

/// Header and rows shared by the CSV and spreadsheet exports.
pub fn table(refs: &[&Reference]) -> (Vec<String>, Vec<Vec<String>>) {
    let custom: Vec<String> = refs.iter().flat_map(|r| r.custom.keys().cloned()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    let mut head: Vec<String> = ["id", "citekey", "type", "authors", "editors", "year", "date", "title", "container_title", "volume", "issue", "page", "article_number", "doi", "url", "publisher", "publisher_place", "edition", "isbn", "issn", "abstract", "keywords", "language", "note"].iter().map(|s| s.to_string()).collect();
    head.extend(custom.iter().map(|c| format!("custom:{c}")));
    let mut rows = vec![];
    for r in refs {
        let cv: Vec<String> = custom.iter().map(|c| match r.custom.get(c) { Some(serde_json::Value::String(s)) => s.clone(), Some(serde_json::Value::Null) | None => String::new(), Some(v) => v.to_string() }).collect();
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
        rows.push(vec![
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
        ].into_iter().map(|s| s.to_string()).chain(cv.into_iter()).collect::<Vec<String>>());
    }
    (head, rows)
}

#[cfg(test)]
mod formula_tests {
    #[test]
    fn formula_cells_are_made_inert() {
        let mut r = crate::model::Reference { id: "a".into(), rtype: "book".into(), ..Default::default() };
        r.fields.insert("title".into(), "=1+1".into());
        r.fields.insert("note".into(), "@SUM(A1)".into());
        let csv = super::export(&[&r]).unwrap();
        assert!(csv.contains("'=1+1") && csv.contains("'@SUM(A1)"), "{csv}");
        assert_eq!(super::inert("Ordinary title"), "Ordinary title");
        assert_eq!(super::inert("-5 degrees"), "'-5 degrees");
    }
}
