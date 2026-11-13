//! Confidence for values filled in automatically (spec DAT-002). Values the person typed, values
//! copied from a file another program exported, and deterministic tidying rules carry no
//! confidence: nothing was inferred. Lookups, web pages, PDF guesses and pasted reference lists
//! are inferred, and their confidence says how much checking a value needs.
//!
//! The scale is deliberately coarse:
//! - 0.95 a registration record found by its identifier (DOI, PubMed, PMC, arXiv)
//! - 0.85 a book record found by ISBN (library catalogues vary more)
//! - 0.6 to 0.8 a web page's own citation tags
//! - 0.3 to 0.75 a pasted reference parsed from free text (the parser's own estimate)
//! - 0.4 to 0.5 a guess from a PDF's embedded properties or first page
//!
//! A value that a second resolver contradicts drops to 0.6. Below `REVIEW_BELOW` the interface
//! asks the person to check the value.

use crate::model::{Provenance, Reference};

pub const REVIEW_BELOW: f32 = 0.7;

/// Identifier fields: when an identifier was the lookup key, the value is the key itself.
const IDENTIFIERS: [&str; 5] = ["doi", "pmid", "pmcid", "arxiv", "isbn"];

/// Confidence for `field` from `source`; `base` is a record-level estimate where one exists.
pub fn confidence(source: &str, field: &str, base: Option<f32>) -> Option<f32> {
    let kind = source.split(':').nth(1).unwrap_or("");
    let ident = IDENTIFIERS.contains(&field);
    let c = match source.split(':').next().unwrap_or("") {
        "lookup" => match kind {
            "doi" | "pmid" | "pmcid" | "arxiv" | "pubmed" | "crossref" | "datacite" => 0.95,
            "isbn" => {
                if ident { 0.95 } else { 0.85 }
            }
            "url" | "webpage" => {
                if ident { 0.9 } else { base.unwrap_or(0.7) }
            }
            "crossref-search" => base.unwrap_or(0.8),
            _ => base.unwrap_or(0.8),
        },
        "pdf" => match field {
            f if IDENTIFIERS.contains(&f) => 0.9,
            "title" => 0.5,
            _ => 0.4,
        },
        "import" if kind == "paste" => base.unwrap_or(0.5),
        "capture" => {
            if ident { 0.9 } else { base.unwrap_or(0.7) }
        }
        _ => return None,
    };
    Some(c)
}

/// Record `source` and confidence for every filled field of `r`.
pub fn stamp(r: &mut Reference, source: &str, base: Option<f32>, time: &str) {
    let fields: Vec<String> = r.fields.keys().cloned().chain(["creators".to_string(), "issued".to_string()]).collect();
    for f in fields {
        if f == "creators" && r.creators.is_empty() || f == "issued" && r.issued.is_none() {
            continue;
        }
        let conf = confidence(source, &f, base);
        r.provenance.insert(f.clone(), Provenance { source: source.into(), time: time.into(), user_edited: false, confidence: conf, review: "unreviewed".into() });
    }
}

/// Like `stamp`, but only for fields that have no provenance yet.
pub fn fill(r: &mut Reference, source: &str, base: Option<f32>, time: &str) {
    let mut fresh = r.clone();
    fresh.provenance.clear();
    stamp(&mut fresh, source, base, time);
    for (k, v) in fresh.provenance {
        r.provenance.entry(k).or_insert(v);
    }
}

/// Lower the confidence of a field a second source contradicts.
pub fn contradicted(r: &mut Reference, field: &str) {
    if let Some(p) = r.provenance.get_mut(field) {
        p.confidence = Some(p.confidence.unwrap_or(1.0).min(0.6));
        p.review = "conflict".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scale() {
        assert_eq!(confidence("user", "title", None), None);
        assert_eq!(confidence("import:ris", "title", None), None);
        assert_eq!(confidence("lookup:doi", "title", None), Some(0.95));
        assert_eq!(confidence("lookup:isbn", "title", None), Some(0.85));
        assert_eq!(confidence("pdf", "title", None), Some(0.5));
        assert_eq!(confidence("pdf", "doi", None), Some(0.9));
        assert_eq!(confidence("import:paste", "title", Some(0.3)), Some(0.3));
    }
}
