//! Duplicate detection: identifiers, file hashes and weighted fuzzy comparison of title,
//! creators, year and venue. Preprint/published pairs are reported as versions to link rather
//! than records to merge. Durable "not duplicates" and "keep both" decisions are respected.
//!
//! Each record's comparison keys (folded title, first surname, DOI, venue) are computed once;
//! pairs are then compared on those keys.

use crate::model::Reference;
use crate::util::{normalize_doi, sort_key};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub a: String,
    pub b: String,
    pub score: f32,
    /// identifier, file, fuzzy, version
    pub kind: String,
    pub reasons: Vec<String>,
}

/// Comparison keys for one record.
pub struct Prepared<'a> {
    pub r: &'a Reference,
    pub title: String,
    pub surname: String,
    pub year: Option<i32>,
    pub doi: Option<String>,
    pub venue: String,
    pub preprint: bool,
}

impl<'a> Prepared<'a> {
    pub fn new(r: &'a Reference) -> Self {
        Prepared {
            r,
            title: sort_key(r.title()),
            surname: r.creators.first().map(|c| sort_key(&c.name.surname())).unwrap_or_default(),
            year: r.year(),
            doi: normalize_doi(r.f("doi")),
            venue: sort_key(r.f("container_title")),
            preprint: r.rtype == "preprint",
        }
    }
}

pub fn title_key(r: &Reference) -> String {
    sort_key(r.title())
}

/// Score two prepared records (no allocation). Returns (score, title similarity, identifier match).
fn score(a: &Prepared, b: &Prepared) -> (f32, f32, Option<&'static str>) {
    if a.doi.is_some() && a.doi == b.doi {
        return (1.0, 1.0, Some("Same DOI"));
    }
    for (k, label) in [("pmid", "Same PubMed ID"), ("pmcid", "Same PMC ID"), ("arxiv", "Same arXiv ID"), ("isbn", "Same ISBN")] {
        if a.r.has(k) && a.r.f(k).trim().eq_ignore_ascii_case(b.r.f(k).trim()) {
            return (0.98, 1.0, Some(label));
        }
    }
    if a.title.is_empty() || b.title.is_empty() {
        return (0.0, 0.0, None);
    }
    let ts = strsim::jaro_winkler(&a.title, &b.title) as f32;
    let mut s = ts * 0.6;
    if !a.surname.is_empty() && a.surname == b.surname {
        s += 0.2;
    } else if !a.surname.is_empty() && !b.surname.is_empty() && strsim::jaro_winkler(&a.surname, &b.surname) > 0.92 {
        s += 0.12;
    }
    match (a.year, b.year) {
        (Some(x), Some(y)) if x == y => s += 0.15,
        (Some(x), Some(y)) if (x - y).abs() <= 1 => s += 0.08,
        _ => {}
    }
    if !a.venue.is_empty() && a.venue == b.venue {
        s += 0.05;
    }
    if a.doi.is_some() && b.doi.is_some() && a.doi != b.doi {
        s -= 0.15;
    }
    (s.clamp(0.0, 1.0), ts, None)
}

/// Full comparison with reasons; used for the few pairs that pass the threshold.
fn explain(a: &Prepared, b: &Prepared) -> (f32, Vec<String>, bool) {
    let (s, ts, ident) = score(a, b);
    if let Some(label) = ident {
        return (s, vec![label.to_string()], false);
    }
    let mut reasons = Vec::new();
    if ts > 0.95 {
        reasons.push("Nearly identical title".into());
    } else if ts > 0.88 {
        reasons.push("Similar title".into());
    }
    if !a.surname.is_empty() && a.surname == b.surname {
        reasons.push("Same first author".into());
    } else if !a.surname.is_empty() && !b.surname.is_empty() && strsim::jaro_winkler(&a.surname, &b.surname) > 0.92 {
        reasons.push("Similar first author".into());
    }
    if a.year.is_some() && a.year == b.year {
        reasons.push("Same year".into());
    }
    let version = a.preprint != b.preprint && ts > 0.9;
    (s, reasons, version)
}

/// Similarity between two records in 0..1 with reasons, and whether they look like two
/// versions (preprint and published) of one work.
pub fn compare(a: &Reference, b: &Reference) -> (f32, Vec<String>, bool) {
    explain(&Prepared::new(a), &Prepared::new(b))
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
    let prepared: Vec<Prepared> = refs.iter().map(Prepared::new).collect();
    let mut by_doi: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, p) in prepared.iter().enumerate() {
        if let Some(d) = &p.doi {
            by_doi.entry(d.as_str()).or_default().push(i);
        }
    }
    for group in by_doi.values() {
        for i in 0..group.len() {
            for j in i + 1..group.len() {
                push(Candidate { a: refs[group[i]].id.clone(), b: refs[group[j]].id.clone(), score: 1.0, kind: "identifier".into(), reasons: vec!["Same DOI".into()] }, &mut out);
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
    let mut blocks: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in prepared.iter().enumerate() {
        if p.title.chars().count() >= 4 {
            blocks.entry(format!("t:{}", p.title.chars().take(4).collect::<String>())).or_default().push(i);
        }
        if !p.surname.is_empty() {
            if let Some(y) = p.year {
                blocks.entry(format!("a:{}:{y}", p.surname)).or_default().push(i);
            }
        }
    }
    let mut compared: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for group in blocks.values() {
        if group.len() > 400 {
            continue;
        }
        for x in 0..group.len() {
            for y in x + 1..group.len() {
                let (i, j) = (group[x].min(group[y]), group[x].max(group[y]));
                let (s, _, _) = score(&prepared[i], &prepared[j]);
                if s >= threshold && compared.insert((i, j)) {
                    let (s, reasons, version) = explain(&prepared[i], &prepared[j]);
                    push(Candidate { a: refs[i].id.clone(), b: refs[j].id.clone(), score: s, kind: if version { "version".into() } else { "fuzzy".into() }, reasons }, &mut out);
                }
            }
        }
    }
    out.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// An index of existing records for matching incoming ones during import staging.
/// Built once per import; each incoming record is compared only with records that share its
/// DOI or the first four letters of its title.
pub struct Matcher<'a> {
    prepared: Vec<Prepared<'a>>,
    by_doi: HashMap<String, Vec<usize>>,
    by_prefix: HashMap<String, Vec<usize>>,
}

impl<'a> Matcher<'a> {
    pub fn new(existing: &'a [Reference]) -> Self {
        let prepared: Vec<Prepared> = existing.iter().filter(|r| r.trashed_at.is_none()).map(Prepared::new).collect();
        let mut by_doi: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_prefix: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, p) in prepared.iter().enumerate() {
            if let Some(d) = &p.doi {
                by_doi.entry(d.clone()).or_default().push(i);
            }
            let k: String = p.title.chars().take(4).collect();
            if !k.is_empty() {
                by_prefix.entry(k).or_default().push(i);
            }
        }
        Matcher { prepared, by_doi, by_prefix }
    }

    pub fn best(&self, r: &Reference, threshold: f32) -> Option<(&'a Reference, f32, Vec<String>, bool)> {
        let p = Prepared::new(r);
        let mut cands: Vec<usize> = Vec::new();
        if let Some(d) = &p.doi {
            if let Some(v) = self.by_doi.get(d) {
                cands.extend(v);
            }
        }
        let k: String = p.title.chars().take(4).collect();
        if !k.is_empty() {
            if let Some(v) = self.by_prefix.get(&k) {
                cands.extend(v);
            }
        }
        let mut best: Option<(usize, f32)> = None;
        for i in cands {
            let (s, _, _) = score(&p, &self.prepared[i]);
            if s >= threshold && best.map(|b| s > b.1).unwrap_or(true) {
                best = Some((i, s));
            }
        }
        best.map(|(i, _)| {
            let (s, reasons, version) = explain(&p, &self.prepared[i]);
            (self.prepared[i].r, s, reasons, version)
        })
    }
}

/// Best existing match for one incoming record. For many records, build a [`Matcher`] once.
pub fn best_match<'a>(r: &Reference, existing: &'a [Reference], threshold: f32) -> Option<(&'a Reference, f32, Vec<String>, bool)> {
    Matcher::new(existing).best(r, threshold)
}
