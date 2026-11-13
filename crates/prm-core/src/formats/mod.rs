//! Bibliographic interchange formats. Importers return parsed candidates with their original
//! text, unmapped fields (preserved in a namespaced overflow area) and issues; nothing is
//! committed here. Exporters write from canonical records.

pub mod bibtex;
pub mod csljson;
pub mod csvfmt;
pub mod paste;
pub mod ris;
pub mod xmlrecords;

use crate::model::{Creator, Name, Reference};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct Parsed {
    pub raw: String,
    pub record: Reference,
    pub issues: Vec<String>,
    /// 0..1 for heuristic parses (pasted text); 1 for structured formats.
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ParseOutput {
    pub format: String,
    pub records: Vec<Parsed>,
    /// Unmapped field name -> number of records that had it.
    pub unmapped: std::collections::BTreeMap<String, usize>,
    pub errors: Vec<String>,
}

pub const ADAPTER_VERSION: &str = "1.0.0";

/// Detect a format from file name and content.
pub fn detect(name: &str, text: &str) -> &'static str {
    let lower = name.to_lowercase();
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if lower.ends_with(".ris") || t.starts_with("TY  -") {
        return "ris";
    }
    if lower.ends_with(".bib") || lower.ends_with(".bibtex") || t.starts_with('@') {
        return "bibtex";
    }
    if lower.ends_with(".json") || t.starts_with('[') || t.starts_with('{') {
        return "csljson";
    }
    if lower.ends_with(".xml") || t.starts_with("<?xml") || t.starts_with("<xml") {
        return "xml";
    }
    if lower.ends_with(".csv") || lower.ends_with(".tsv") {
        return "csv";
    }
    if lower.ends_with(".nbib") || t.starts_with("PMID-") {
        return "unsupported";
    }
    "paste"
}

pub fn parse(format: &str, text: &str, csv_mapping: Option<&std::collections::BTreeMap<String, String>>) -> ParseOutput {
    let text = text.trim_start_matches('\u{feff}');
    let mut out = match format {
        "ris" => ris::parse(text),
        "bibtex" | "biblatex" => bibtex::parse(text),
        "csljson" => csljson::parse(text),
        "xml" => xmlrecords::parse(text),
        "csv" => csvfmt::parse(text, csv_mapping),
        "paste" => paste::parse(text),
        _ => ParseOutput { errors: vec!["This file format is not supported yet. Supported: RIS, BibTeX, biblatex, CSL JSON, XML records, CSV.".into()], ..Default::default() },
    };
    out.format = format.to_string();
    for p in &mut out.records {
        finish(&mut p.record);
        for k in p.record.overflow.keys() {
            *out.unmapped.entry(k.clone()).or_default() += 1;
        }
    }
    out
}

/// Normalize a parsed record: trim, normalize DOI, fill ids.
pub fn finish(r: &mut Reference) {
    if r.id.is_empty() {
        r.id = crate::util::new_id();
    }
    if r.rtype.is_empty() || crate::types::get(&r.rtype).is_none() {
        r.rtype = "document".into();
    }
    let keys: Vec<String> = r.fields.keys().cloned().collect();
    for k in keys {
        let v = r.fields.get(&k).cloned().unwrap_or_default();
        let t = v.trim().to_string();
        if t.is_empty() {
            r.fields.remove(&k);
        } else {
            r.fields.insert(k, t);
        }
    }
    if let Some(d) = r.fields.get("doi").cloned() {
        if let Some(n) = crate::util::normalize_doi(&d) {
            r.fields.insert("doi".into(), n);
        }
    }
    r.creators.retain(|c| !c.name.is_empty());
}

pub fn person(role: &str, s: &str) -> Creator {
    Creator { role: role.into(), name: Name::parse(s) }
}

/// "Last, F. M." name list separated by " and " or ";".
pub fn split_names(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return vec![];
    }
    if s.contains(';') {
        return s.split(';').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    }
    if s.contains(" and ") {
        return s.split(" and ").map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    }
    vec![s.to_string()]
}

/// Split a page string into (page, article number): "e1042" with no range is an article number
/// candidate but is kept as a page; the health report proposes the move for review.
pub fn pages(sp: &str, ep: &str) -> String {
    let (a, b) = (sp.trim(), ep.trim());
    if b.is_empty() || a == b {
        a.to_string()
    } else if a.is_empty() {
        b.to_string()
    } else {
        format!("{a}-{b}")
    }
}

/// Map a free-text type label to a type id.
pub fn type_from_label(label: &str) -> &'static str {
    let l = label.to_lowercase();
    let pairs: &[(&str, &str)] = &[
        ("journal", "article-journal"),
        ("magazine", "article-magazine"),
        ("newspaper", "article-newspaper"),
        ("preprint", "preprint"),
        ("book section", "chapter"),
        ("chapter", "chapter"),
        ("edited book", "edited-book"),
        ("book", "book"),
        ("thesis", "thesis"),
        ("dissertation", "thesis"),
        ("report", "report"),
        ("conference paper", "paper-conference"),
        ("proceedings", "paper-conference"),
        ("conference", "presentation"),
        ("presentation", "presentation"),
        ("web", "webpage"),
        ("blog", "post-weblog"),
        ("data", "dataset"),
        ("software", "software"),
        ("computer program", "software"),
        ("patent", "patent"),
        ("case", "legal-case"),
        ("statute", "legislation"),
        ("legislation", "legislation"),
        ("bill", "bill"),
        ("standard", "standard"),
        ("manuscript", "manuscript"),
        ("personal communication", "personal-communication"),
        ("letter", "letter"),
        ("interview", "interview"),
        ("film", "film"),
        ("video", "film"),
        ("broadcast", "broadcast"),
        ("podcast", "podcast"),
        ("music", "song"),
        ("artwork", "graphic"),
        ("image", "graphic"),
        ("map", "map"),
        ("encyclopedia", "entry-encyclopedia"),
        ("dictionary", "entry-dictionary"),
        ("government", "government-document"),
        ("article", "article-journal"),
    ];
    for (k, v) in pairs {
        if l.contains(k) {
            return v;
        }
    }
    "document"
}
