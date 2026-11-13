//! Duplicate detection: identifiers, file hashes and weighted fuzzy comparison of title,
//! creators, year and venue. Preprint/published pairs are reported as versions to link rather
//! than records to merge. Durable "not duplicates" and "keep both" decisions are respected.

use crate::model::Reference;
use crate::util::{normalize_doi, sort_key};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub a: String,
    pub b: String,
    pub score: f32,
    /// identifier, file, fuzzy, version
    pub kind: String,
    pub reasons: Vec<String>,
}

fn first_surname(r: &Reference) -> String {
    r.creators.first().map(|c| sort_key(&c.name.surname())).unwrap_or_default()
}

pub fn title_key(r: &Reference) -> String {
    sort_key(r.title())
}

/// Similarity between two records in 0..1 with reasons, and whether they look like two
/// versions (preprint and published) of one work.
pub fn compare(a: &Reference, b: &Reference) -> (f32, Vec<String>, bool) {
    let mut reasons = Vec::new();
    let da = normalize_doi(a.f("doi"));
    let db = normalize_doi(b.f("doi"));
    if da.is_some() && da == db {
        return (1.0, vec!["Same DOI".into()], false);
    }
    for k in ["pmid", "pmcid", "arxiv", "isbn"] {
        if a.has(k) && a.f(k).trim().eq_ignore_ascii_case(b.f(k).trim()) {
            return (0.98, vec![format!("Same {}", crate::types::field_label(k))], false);
        }
    }
    let ta = title_key(a);
    let tb = title_key(b);
    if ta.is_empty() || tb.is_empty() {
        return (0.0, reasons, false);
    }
    let ts = strsim::jaro_winkler(&ta, &tb) as f32;
    let mut score = ts * 0.6;
    if ts > 0.95 {
        reasons.push("Nearly identical title".into());
    } else if ts > 0.88 {
        reasons.push("Similar title".into());
    }
    let sa = first_surname(a);
    let sb = first_surname(b);
    if !sa.is_empty() && sa == sb {
        score += 0.2;
        reasons.push("Same first author".into());
    } else if !sa.is_empty() && !sb.is_empty() && strsim::jaro_winkler(&sa, &sb) > 0.92 {
        score += 0.12;
        reasons.push("Similar first author".into());
    }
    match (a.year(), b.year()) {
        (Some(x), Some(y)) if x == y => {
            score += 0.15;
            reasons.push("Same year".into());
        }
        (Some(x), Some(y)) if (x - y).abs() <= 1 => score += 0.08,
        _ => {}
    }
    if a.has("container_title") && sort_key(a.f("container_title")) == sort_key(b.f("container_title")) {
        score += 0.05;
    }
    if da.is_some() && db.is_some() && da != db {
        score -= 0.15;
    }
    let version = (a.rtype == "preprint") != (b.rtype == "preprint") && ts > 0.9;
    (score.clamp(0.0, 1.0), reasons, version)
}

/// Find duplicate candidates among `refs`. `threshold` (0.7 to 0.95) controls sensitivity.
/// `decided` holds pair keys already marked not-duplicate or keep-both. `file_hashes` maps a
/// reference id to its attachment hashes.
pub fn find(refs: &[Reference], threshold: f32, decided: &BTreeSet<String>, file_hashes: &BTreeMap<String, Vec<String>>) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut push = |c: Candidate, out: &mut Vec<Candidate>| {
        let k = crate::model::DuplicateDecision::key(&c.a, &c.b);
        if decided.contains(&k) || !seen.insert(k) {
            return;
        }
        out.push(c);
    };
    let mut by_doi: BTreeMap<String, Vec<&Reference>> = BTreeMap::new();
    for r in refs {
        if let Some(d) = normalize_doi(r.f("doi")) {
            by_doi.entry(d).or_default().push(r);
        }
    }
    for group in by_doi.values() {
        for i in 0..group.len() {
            for j in i + 1..group.len() {
                push(Candidate { a: group[i].id.clone(), b: group[j].id.clone(), score: 1.0, kind: "identifier".into(), reasons: vec!["Same DOI".into()] }, &mut out);
            }
        }
    }
    let mut by_hash: BTreeMap<&String, Vec<&String>> = BTreeMap::new();
    for (rid, hs) in file_hashes {
        for h in hs {
            by_hash.entry(h).or_default().push(rid);
        }
    }
    for group in by_hash.values() {
        for i in 0..group.len() {
            for j in i + 1..group.len() {
                if group[i] != group[j] {
                    push(Candidate { a: group[i].clone(), b: group[j].clone(), score: 0.97, kind: "file".into(), reasons: vec!["Identical attached file".into()] }, &mut out);
                }
            }
        }
    }
    // Block by title prefix and by first author + year to keep comparisons manageable.
    let mut blocks: BTreeMap<String, Vec<&Reference>> = BTreeMap::new();
    for r in refs {
        let t = title_key(r);
        if t.chars().count() >= 4 {
            blocks.entry(format!("t:{}", t.chars().take(4).collect::<String>())).or_default().push(r);
        }
        let s = first_surname(r);
        if !s.is_empty() {
            if let Some(y) = r.year() {
                blocks.entry(format!("a:{s}:{y}")).or_default().push(r);
            }
        }
    }
    for group in blocks.values() {
        if group.len() > 400 {
            continue;
        }
        for i in 0..group.len() {
            for j in i + 1..group.len() {
                let (s, reasons, version) = compare(group[i], group[j]);
                if s >= threshold {
                    push(Candidate { a: group[i].id.clone(), b: group[j].id.clone(), score: s, kind: if version { "version".into() } else { "fuzzy".into() }, reasons }, &mut out);
                }
            }
        }
    }
    out.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Best existing match for an incoming record (used during import staging).
pub fn best_match<'a>(r: &Reference, existing: &'a [Reference], threshold: f32) -> Option<(&'a Reference, f32, Vec<String>, bool)> {
    let mut best: Option<(&Reference, f32, Vec<String>, bool)> = None;
    let doi = normalize_doi(r.f("doi"));
    let tk: String = title_key(r).chars().take(4).collect();
    for e in existing {
        if e.trashed_at.is_some() {
            continue;
        }
        let etk: String = title_key(e).chars().take(4).collect();
        let quick = (doi.is_some() && normalize_doi(e.f("doi")) == doi) || (!tk.is_empty() && etk == tk);
        if !quick {
            continue;
        }
        let (s, reasons, version) = compare(r, e);
        if s >= threshold && best.as_ref().map(|b| s > b.1).unwrap_or(true) {
            best = Some((e, s, reasons, version));
        }
    }
    best
}
