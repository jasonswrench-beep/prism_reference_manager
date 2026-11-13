//! Library health report: checks with evidence and a suggested action. Nothing is changed
//! automatically.

use crate::error::Result;
use crate::library::Library;
use crate::model::*;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct HealthItem {
    pub check: String,
    pub ref_id: String,
    pub title: String,
    pub detail: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub generated: String,
    pub checked: usize,
    pub counts: BTreeMap<String, usize>,
    pub items: Vec<HealthItem>,
    pub duplicates: usize,
    pub missing_files: usize,
    pub unindexed: i64,
}

pub fn run(lib: &Library) -> Result<HealthReport> {
    let refs: Vec<Reference> = crate::store::list_in(&lib.store.conn, "WHERE trashed IS NULL", &[])?;
    let mut items = Vec::new();
    let mut keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let doi_re = regex::Regex::new(r"^10\.\d{4,9}/\S+$").unwrap();
    for r in &refs {
        let t = super::library::short(r.title());
        let mut push = |check: &str, detail: String, action: &str| items.push(HealthItem { check: check.into(), ref_id: r.id.clone(), title: t.clone(), detail, action: action.into() });
        if r.title().trim().is_empty() && !r.absent("title") {
            push("Missing title", "No title is recorded.".into(), "Add the title or confirm the work has none");
        }
        if r.issued.is_none() && !r.absent("issued") {
            push("Missing date", "No date is recorded; citations will use n.d.".into(), "Add the date or confirm none");
        }
        if r.creators.is_empty() && !r.absent("author") {
            push("Missing creators", "No authors or editors are recorded.".into(), "Add creators or confirm none");
        }
        if r.has("doi") && !doi_re.is_match(r.f("doi")) {
            push("Malformed DOI", format!("“{}” is not a valid DOI form.", r.f("doi")), "Correct the DOI");
        }
        let p = r.f("page").trim();
        if r.rtype == "article-journal" && !r.has("article_number") && !p.is_empty() && !p.contains(['-', '–']) && p.starts_with(|c: char| c.is_alphabetic()) {
            push("Article number as page", format!("Pages “{p}” looks like an article number."), "Review fields");
        }
        let title = r.title();
        if title.len() > 12 && title.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) {
            push("Title in capitals", "The title is written entirely in capitals.".into(), "Fix capitalization (protected terms are kept)");
        }
        if title != title.trim() || title.contains("  ") {
            push("Extra spaces", "The title has extra spaces.".into(), "Trim whitespace");
        }
        if !r.conflicts.is_empty() {
            push("Conflicting values", format!("{} fields have values from different sources awaiting review.", r.conflicts.len()), "Compare sources");
        }
        for n in &r.notices {
            push("Notice", format!("{} reported {} by {}.", n.kind, n.date, n.source), "Read the notice");
        }
        if r.rtype == "document" {
            push("Unknown type", "The reference type is generic; output needs manual review.".into(), "Choose a type");
        }
        if !r.citekey.is_empty() {
            keys.entry(r.citekey.clone()).or_default().push(r.id.clone());
        }
    }
    for (k, ids) in keys.iter().filter(|(_, v)| v.len() > 1) {
        for id in ids {
            let title = refs.iter().find(|r| &r.id == id).map(|r| r.title().to_string()).unwrap_or_default();
            items.push(HealthItem { check: "Duplicate citation key".into(), ref_id: id.clone(), title, detail: format!("“{k}” is used by {} sources.", ids.len()), action: "Regenerate the key".into() });
        }
    }
    let files = lib.check_files(false)?;
    let missing = files.iter().filter(|f| f.state != "ok").count();
    for f in files.iter().filter(|f| f.state != "ok") {
        let title = refs.iter().find(|r| r.id == f.ref_id).map(|r| r.title().to_string()).unwrap_or_default();
        items.push(HealthItem { check: "File problem".into(), ref_id: f.ref_id.clone(), title, detail: f.message.clone(), action: "Relink".into() });
    }
    let dups = lib.duplicates(0.88)?.len();
    let mut counts = BTreeMap::new();
    for i in &items {
        *counts.entry(i.check.clone()).or_default() += 1;
    }
    Ok(HealthReport { generated: crate::util::now(), checked: refs.len(), counts, items, duplicates: dups, missing_files: missing, unindexed: crate::search::pending_index_count(&lib.store.conn)? })
}
