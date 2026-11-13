//! Import staging: parse, normalize, compare with the library, review, commit, undo.
//! Nothing reaches the library until the person commits reviewed records.

use super::refs::add_membership;
use super::Library;
use crate::error::{Error, Result};
use crate::formats;
use crate::model::*;
use crate::util::{new_id, now, sha256_hex};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct CommitReport {
    pub batch_id: String,
    pub added: usize,
    pub merged: usize,
    pub linked: usize,
    pub skipped: usize,
    pub failed: Vec<String>,
    #[serde(default)]
    pub updated: usize,
    #[serde(default)]
    pub collections: usize,
}

/// Set from outside (without the library lock) to stop a running stage or commit.
pub static IMPORT_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn cancelled() -> bool {
    IMPORT_CANCEL.load(std::sync::atomic::Ordering::Relaxed)
}

/// The record's identity inside the program that exported it, used to recognize the same record
/// when a later export of that library is imported again.
/// Provenance for a value taken from a staged record: what staging recorded, else the import.
fn staged_prov(incoming: &Reference, field: &str, source: &str, t: &str) -> Provenance {
    match incoming.provenance.get(field) {
        Some(p) if !p.source.is_empty() => Provenance { time: t.into(), ..p.clone() },
        _ => Provenance { source: source.into(), time: t.into(), user_edited: false, confidence: None, review: "unreviewed".into() },
    }
}

pub fn source_key(adapter: &str, r: &Reference) -> Option<String> {
    let o = |k: &str| r.overflow.get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    match adapter {
        "ris" => o("ris:ID").or_else(|| o("ris:AN")).map(|v| format!("ris:{v}")),
        "bibtex" | "biblatex" => (!r.citekey.trim().is_empty()).then(|| format!("bib:{}", r.citekey.trim())),
        "csljson" => (!r.citekey.trim().is_empty()).then(|| format!("csl:{}", r.citekey.trim())),
        "xml" => o("xml:rec-number").map(|n| format!("xml:{}:{n}", o("xml:database").unwrap_or_default())),
        "csv" => o("import:source-id").map(|v| format!("csv:{v}")),
        _ => None,
    }
}

/// Group paths a record belongs to in the exporting program.
fn record_groups(r: &Reference, hierarchy: &BTreeMap<String, String>, label_as_groups: bool) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut add = |name: &str| {
        let n = name.trim();
        if n.is_empty() {
            return;
        }
        let path = hierarchy.get(n).cloned().unwrap_or_else(|| n.to_string());
        if !out.contains(&path) {
            out.push(path);
        }
    };
    if let Some(g) = r.overflow.get("bibtex:groups") {
        for n in g.split(',') {
            add(n);
        }
    }
    if let Some(g) = r.overflow.get("import:groups") {
        for n in g.split(';') {
            add(n);
        }
    }
    if label_as_groups {
        if let Some(g) = r.overflow.get("xml:label") {
            for n in g.split(';') {
                add(n);
            }
        }
    }
    out
}

/// Group tree from a BibTeX file's grouping comment: each group's full path ("Parent/Child").
fn bibtex_group_tree(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(start) = text.find("jabref-meta: grouping:") else { return out };
    let body = &text[start + "jabref-meta: grouping:".len()..];
    let body = &body[..body.find('}').unwrap_or(body.len())];
    let mut stack: Vec<String> = vec![];
    for line in body.lines() {
        let line = line.trim().trim_end_matches(';');
        let Some((depth, rest)) = line.split_once(' ') else { continue };
        let Ok(depth) = depth.parse::<usize>() else { continue };
        let name = rest.split_once(':').map(|x| x.1).unwrap_or(rest);
        let name = name.split(['\\', ';']).next().unwrap_or("").trim().to_string();
        if depth == 0 || name.is_empty() {
            stack.clear();
            continue;
        }
        stack.truncate(depth - 1);
        stack.push(name.clone());
        out.insert(name, stack.join("/"));
    }
    out
}

/// A rating carried in the export (a 1 to 5 value, or "rank3").
fn import_rating(r: &mut Reference) {
    for k in ["bibtex:ranking", "bibtex:rating", "import:rating", "xml:rating"] {
        if let Some(v) = r.overflow.get(k) {
            let n: u8 = v.trim().trim_start_matches("rank").parse().unwrap_or(0);
            if (1..=5).contains(&n) && r.rating == 0 {
                r.rating = n;
            }
        }
    }
}

/// Fields that differ between an imported record and the library's copy. Values the person
/// edited in the library are listed separately and never overwritten.
fn changed_fields(existing: &Reference, incoming: &Reference) -> (Vec<String>, Vec<String>) {
    let mut changed = vec![];
    let mut kept = vec![];
    let edited = |k: &str| existing.provenance.get(k).map(|p| p.user_edited).unwrap_or(false);
    for (k, v) in &incoming.fields {
        let cur = existing.fields.get(k).map(|s| crate::util::sort_key(s)).unwrap_or_default();
        if cur != crate::util::sort_key(v) {
            if edited(k) { kept.push(k.clone()) } else { changed.push(k.clone()) }
        }
    }
    let names = |r: &Reference| r.creators.iter().map(|c| format!("{}|{}|{}", c.role, c.name.surname(), c.name.given)).collect::<Vec<_>>();
    if !incoming.creators.is_empty() && names(existing) != names(incoming) {
        if edited("creators") { kept.push("creators".into()) } else { changed.push("creators".into()) }
    }
    if incoming.issued.is_some() && existing.issued != incoming.issued {
        if edited("issued") { kept.push("issued".into()) } else { changed.push("issued".into()) }
    }
    (changed, kept)
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoPreview {
    pub batch_id: String,
    pub removable: Vec<String>,
    /// Records with later work (notes, annotations, copies, edits) that depend on them.
    pub dependent: Vec<(String, String)>,
}

impl Library {
    /// Stage a file or pasted text for review. `format` may be empty to detect it.
    #[allow(clippy::too_many_arguments)]
    pub fn stage_import(&mut self, file_name: &str, text: &str, format: &str, service: &str, project_id: &str, collection_id: &str, csv_mapping: Option<&BTreeMap<String, String>>) -> Result<ImportBatch> {
        let fmt = if format.is_empty() { formats::detect(file_name, text) } else { format };
        IMPORT_CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
        self.report_progress("Reading the file", 0, 0);
        let parsed = formats::parse(fmt, text, csv_mapping);
        if parsed.records.is_empty() {
            let msg = parsed.errors.first().cloned().unwrap_or_else(|| "No records were found in this file.".into());
            return Err(Error::user(msg));
        }
        let hierarchy = if fmt == "bibtex" || fmt == "biblatex" { bibtex_group_tree(text) } else { BTreeMap::new() };
        // Records imported earlier from the same program, by their identity in that program.
        let mut by_key: BTreeMap<String, String> = BTreeMap::new();
        {
            let mut st = self.store.conn.prepare("SELECT id, json_extract(data,'$.overflow.\"import:key\"') FROM refs WHERE trashed IS NULL AND json_extract(data,'$.overflow.\"import:key\"') IS NOT NULL")?;
            for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (id, k) = row?;
                by_key.insert(k, id);
            }
        }
        let mut groups_found: Vec<String> = vec![];
        let custom_defs = self.custom_fields()?;
        let total = parsed.records.len();
        let file_hash = sha256_hex(text.as_bytes());
        let already = self.store.all::<ImportBatch>()?.into_iter().find(|b| b.file_hash == file_hash && b.state == "committed");
        let existing: Vec<Reference> = crate::store::list_in(&self.store.conn, "WHERE trashed IS NULL", &[])?;
        let matcher = crate::dedupe::Matcher::new(&existing);
        let decided: std::collections::BTreeSet<String> = self.store.all::<DuplicateDecision>()?.into_iter().map(|d| d.id).collect();
        let mut records = Vec::new();
        let mut summary: BTreeMap<String, usize> = BTreeMap::new();
        for (i, p) in parsed.records.into_iter().enumerate() {
            if i % 50 == 0 {
                if cancelled() {
                    return Err(Error::user("Import stopped. Nothing was staged; you can start it again."));
                }
                self.report_progress("Checking for duplicates", i, total);
            }
            let mut rec = p.record;
            rec.origin = "import".into();
            // A pasted reference list is parsed from free text: its values carry the parser's
            // own confidence.
            if fmt == "paste" {
                crate::provenance::stamp(&mut rec, "import:paste", Some(p.confidence), &now());
            }
            import_rating(&mut rec);
            if let Some(k) = source_key(fmt, &rec) {
                rec.overflow.insert("import:key".into(), k);
            }
            let gs = record_groups(&rec, &hierarchy, true);
            for g in &gs {
                if !groups_found.contains(g) {
                    groups_found.push(g.clone());
                }
            }
            let mut issues = p.issues.clone();
            // Custom values: canonical form for defined fields, a note for anything that does not fit.
            if !rec.custom.is_empty() {
                let mut unknown = vec![];
                for k in rec.custom.keys().cloned().collect::<Vec<_>>() {
                    match custom_defs.iter().find(|d| d.id == k || d.name.to_lowercase() == k.to_lowercase()) {
                        Some(d) => {
                            let v = rec.custom.remove(&k).unwrap_or_default();
                            match super::custom::check_value(&crate::model::CustomFieldDef { required: false, ..d.clone() }, &v) {
                                Ok(serde_json::Value::Null) => {}
                                Ok(n) => {
                                    rec.custom.insert(d.id.clone(), n);
                                }
                                Err(e) => {
                                    issues.push(format!("{e} The value “{}” is kept for you to correct.", v.as_str().unwrap_or(&v.to_string())));
                                    rec.custom.insert(d.id.clone(), v);
                                }
                            }
                        }
                        None => unknown.push(k),
                    }
                }
                if !unknown.is_empty() {
                    issues.push(format!("Values for fields not defined in this library are kept but hidden: {}. Define them under Settings › Fields and types to see them.", unknown.join(", ")));
                }
            }
            let mut status = "new".to_string();
            let mut match_id = String::new();
            let mut reason = String::new();
            if rec.title().trim().is_empty() {
                status = "unresolved".into();
                issues.push("No title".into());
            }
            // The same record imported before from the same program: offer its changes.
            let keyed = rec.overflow.get("import:key").and_then(|k| by_key.get(k)).cloned();
            if let Some(eid) = keyed {
                if let Some(e) = self.store.get::<Reference>(&eid)? {
                    let (changed, kept) = changed_fields(&e, &rec);
                    match_id = eid;
                    reason = "Same record in the exporting program".into();
                    if changed.is_empty() {
                        status = "unchanged".into();
                    } else {
                        status = "update".into();
                        issues.push(format!("Changed since it was last imported: {}.", changed.iter().map(|f| crate::types::field_label(f)).collect::<Vec<_>>().join(", ")));
                    }
                    if !kept.is_empty() {
                        issues.push(format!("Your edits are kept: {}.", kept.iter().map(|f| crate::types::field_label(f)).collect::<Vec<_>>().join(", ")));
                    }
                    *summary.entry(status.clone()).or_default() += 1;
                    let decision = if status == "update" { "update" } else { "skip" };
                    records.push(StagedRecord { index: i, raw: p.raw, record: rec, status, match_ref_id: match_id, match_reason: reason, issues, decision: decision.into(), committed_id: String::new() });
                    continue;
                }
            }
            if let Some((m, score, reasons, version)) = matcher.best(&rec, 0.82) {
                if !decided.contains(&DuplicateDecision::key(&rec.id, &m.id)) {
                    match_id = m.id.clone();
                    reason = reasons.join(", ");
                    let same_doi = crate::util::normalize_doi(rec.f("doi")).is_some() && crate::util::normalize_doi(rec.f("doi")) == crate::util::normalize_doi(m.f("doi"));
                    if same_doi && crate::util::sort_key(rec.title()) != crate::util::sort_key(m.title()) && crate::strsim_ratio(rec.title(), m.title()) < 0.8 {
                        status = "conflict".into();
                        issues.push("The DOI matches a library record with a different title. Check that the DOI describes this work.".into());
                    } else if version {
                        status = "version".into();
                    } else {
                        status = "duplicate".into();
                    }
                    let _ = score;
                }
            }
            if p.confidence < 0.7 && status == "new" {
                status = "unresolved".into();
            }
            if rec.rtype == "document" && status == "new" {
                issues.push("Type not recognized; review it.".into());
            }
            *summary.entry(status.clone()).or_default() += 1;
            let decision = match status.as_str() {
                "new" => "accept",
                "unresolved" => "accept",
                "version" => "link_version",
                _ => "skip",
            };
            records.push(StagedRecord { index: i, raw: p.raw, record: rec, status, match_ref_id: match_id, match_reason: reason, issues, decision: decision.into(), committed_id: String::new() });
        }
        let mut errors_note = parsed.errors.clone();
        if let Some(b) = already {
            errors_note.push(format!("This exact file was imported on {}. Matching records are marked as duplicates.", &b.committed.get(..10).unwrap_or("")));
        }
        let files_included = records.iter().filter(|r| r.record.overflow.keys().any(|k| k.ends_with(":L1") || k.ends_with("pdf-url") || k.ends_with(":file"))).count();
        self.report_progress("Preparing review", total, total);
        let batch = ImportBatch {
            make_collections: !groups_found.is_empty(),
            label_as_groups: false,
            groups_found,
            id: new_id(),
            adapter: fmt.into(),
            adapter_version: formats::ADAPTER_VERSION.into(),
            file_name: file_name.into(),
            file_hash,
            service: if service.is_empty() { "file".into() } else { service.into() },
            destination_project: project_id.into(),
            destination_collection: collection_id.into(),
            mapping: csv_mapping.cloned().unwrap_or_default(),
            unmapped_fields: parsed.unmapped,
            records,
            state: "staged".into(),
            summary: {
                let mut s = summary;
                if !errors_note.is_empty() {
                    s.insert("messages".into(), errors_note.len());
                }
                s
            },
            files_included,
            created: now(),
            committed: String::new(),
        };
        // Staged batches are stored so a review can be resumed; they are not an undo step.
        let s = crate::util::canonical_json(&serde_json::to_value(&batch)?);
        self.store.conn.execute("INSERT INTO import_batches(id,data) VALUES(?1,?2)", rusqlite::params![batch.id, s])?;
        Ok(batch)
    }

    pub fn import_batches(&self) -> Result<Vec<ImportBatch>> {
        let mut v: Vec<ImportBatch> = self.store.all()?;
        v.sort_by(|a, b| b.created.cmp(&a.created));
        Ok(v)
    }

    /// Save changes to a staged (uncommitted) batch; staged review state is not an undo step.
    pub fn save_staged(&self, b: &ImportBatch) -> Result<()> {
        let s = crate::util::canonical_json(&serde_json::to_value(b)?);
        self.store.conn.execute("UPDATE import_batches SET data=?1 WHERE id=?2", rusqlite::params![s, b.id])?;
        Ok(())
    }

    /// Change the decision for staged records, or edit a staged record before committing.
    pub fn set_staged(&mut self, batch_id: &str, index: usize, decision: Option<&str>, record: Option<Reference>) -> Result<ImportBatch> {
        let mut b = self.store.must::<ImportBatch>(batch_id)?;
        if b.state != "staged" {
            return Err(Error::user("This import was already committed or cancelled."));
        }
        let r = b.records.iter_mut().find(|r| r.index == index).ok_or_else(|| Error::NotFound("staged record".into()))?;
        if let Some(d) = decision {
            r.decision = d.into();
        }
        if let Some(rec) = record {
            r.record = rec;
            if r.status == "unresolved" && !r.record.title().trim().is_empty() {
                r.status = "new".into();
            }
        }
        self.save_staged(&b)?;
        Ok(b)
    }

    pub fn cancel_import(&mut self, batch_id: &str) -> Result<()> {
        let mut b = self.store.must::<ImportBatch>(batch_id)?;
        if b.state == "staged" {
            b.state = "cancelled".into();
            self.save_staged(&b)?;
        }
        Ok(())
    }

    /// Commit records whose decision is accept, merge, keep_both or link_version, all as one
    /// undo step. Re-running a committed batch is refused (idempotent by batch id).
    pub fn commit_import(&mut self, batch_id: &str) -> Result<CommitReport> {
        let mut b = self.store.must::<ImportBatch>(batch_id)?;
        if b.state != "staged" {
            return Err(Error::user("This import was already committed or cancelled; nothing was changed."));
        }
        IMPORT_CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
        self.safety_snapshot("import")?;
        let t = now();
        let hierarchy: BTreeMap<String, String> = BTreeMap::new();
        let mut updates: Vec<(String, Reference)> = Vec::new();
        let mut group_of: Vec<(String, Vec<String>)> = Vec::new();
        let source = format!("import:{}", b.adapter);
        let mut report = CommitReport { batch_id: b.id.clone(), added: 0, merged: 0, linked: 0, skipped: 0, failed: vec![], updated: 0, collections: 0 };
        let mut new_refs = Vec::new();
        let mut merges = Vec::new();
        let mut decisions = Vec::new();
        let mut taken: std::collections::BTreeSet<String> = {
            let mut st = self.store.conn.prepare("SELECT json_extract(data,'$.citekey') FROM refs")?;
            let v: std::collections::BTreeSet<String> = st.query_map([], |r| r.get::<_, Option<String>>(0))?.filter_map(|r| r.ok().flatten()).collect();
            v
        };
        for sr in b.records.iter_mut() {
            match sr.decision.as_str() {
                "accept" | "keep_both" | "link_version" => {
                    let mut r = sr.record.clone();
                    r.id = new_id();
                    r.batch_id = b.id.clone();
                    r.created = t.clone();
                    r.modified = t.clone();
                    r.revision = 1;
                    formats::finish(&mut r);
                    crate::cleanup::apply_rules(&mut r, &self.save_rules, &self.protected_terms);
                    let base = if r.citekey.is_empty() { crate::citekey::generate(&r, &self.citekey_pattern) } else { r.citekey.clone() };
                    r.citekey = crate::citekey::unique(&base, &taken);
                    taken.insert(r.citekey.clone());
                    let keys: Vec<String> = r.fields.keys().cloned().chain(["creators".to_string(), "issued".to_string()]).collect();
                    // Provenance recorded at staging (a lookup, a PDF guess, a pasted list) is kept;
                    // other values are attributed to the import.
                    for f in keys {
                        match r.provenance.get_mut(&f) {
                            Some(p) if !p.source.is_empty() => p.time = t.clone(),
                            _ => {
                                r.provenance.insert(f, Provenance { source: source.clone(), time: t.clone(), user_edited: false, confidence: None, review: "unreviewed".into() });
                            }
                        }
                    }
                    if sr.decision == "keep_both" && !sr.match_ref_id.is_empty() {
                        decisions.push(DuplicateDecision { id: DuplicateDecision::key(&r.id, &sr.match_ref_id), a: r.id.clone(), b: sr.match_ref_id.clone(), decision: "keep_both".into(), time: t.clone() });
                    }
                    if sr.decision == "link_version" && !sr.match_ref_id.is_empty() {
                        report.linked += 1;
                    } else {
                        report.added += 1;
                    }
                    sr.committed_id = r.id.clone();
                    new_refs.push((r, if sr.decision == "link_version" { sr.match_ref_id.clone() } else { String::new() }));
                }
                "merge" if !sr.match_ref_id.is_empty() => {
                    merges.push((sr.match_ref_id.clone(), sr.record.clone()));
                    sr.committed_id = sr.match_ref_id.clone();
                    report.merged += 1;
                }
                "update" if !sr.match_ref_id.is_empty() => {
                    updates.push((sr.match_ref_id.clone(), sr.record.clone()));
                    sr.committed_id = sr.match_ref_id.clone();
                    report.updated += 1;
                }
                _ => report.skipped += 1,
            }
            if b.make_collections && !sr.committed_id.is_empty() {
                let gs = record_groups(&sr.record, &hierarchy, b.label_as_groups);
                if !gs.is_empty() {
                    group_of.push((sr.committed_id.clone(), gs));
                }
            }
        }
        // Group names were resolved to full paths while staging; keep that tree for commit.
        let group_paths: std::collections::BTreeSet<String> = b.groups_found.iter().cloned().collect();
        let group_of: Vec<(String, Vec<String>)> = group_of
            .into_iter()
            .map(|(id, gs)| (id, gs.into_iter().map(|g| group_paths.iter().find(|p| p.rsplit('/').next() == Some(g.as_str()) || **p == g).cloned().unwrap_or(g)).collect()))
            .collect();
        let total_puts = new_refs.len() + merges.len() + updates.len();
        b.state = "committed".into();
        b.committed = t.clone();
        let project = b.destination_project.clone();
        let collection = b.destination_collection.clone();
        let label = format!("Import {} ({} added)", b.file_name, report.added + report.linked);
        let bb = b.clone();
        let progress = self.progress.clone();
        let mut done = 0usize;
        let tick = |done: &mut usize| -> Result<()> {
            *done += 1;
            if *done % 100 == 0 {
                if cancelled() {
                    return Err(Error::user("Import stopped. Nothing was added; the review is kept so you can commit it later."));
                }
                if let Some(p) = &progress {
                    p("Adding records", *done, total_puts);
                }
            }
            Ok(())
        };
        let r_written = self.store.write(&label, |tx| {
            for (target, incoming) in &updates {
                tick(&mut done)?;
                let mut e = tx.must::<Reference>(target)?;
                let (changed, _) = changed_fields(&e, incoming);
                for f in &changed {
                    match f.as_str() {
                        "creators" => e.creators = incoming.creators.clone(),
                        "issued" => e.issued = incoming.issued.clone(),
                        k => {
                            if let Some(v) = incoming.fields.get(k) {
                                e.fields.insert(k.to_string(), v.clone());
                            }
                        }
                    }
                    e.provenance.insert(f.clone(), staged_prov(incoming, f, &source, &t));
                }
                for tag in &incoming.tags {
                    if !e.tags.contains(tag) {
                        e.tags.push(tag.clone());
                    }
                }
                e.revision += 1;
                e.modified = t.clone();
                tx.put(&e)?;
                if !project.is_empty() {
                    add_membership(tx, &project, &e.id)?;
                }
            }
            for (r, link_to) in &new_refs {
                tick(&mut done)?;
                let mut r = r.clone();
                if !link_to.is_empty() {
                    if let Some(mut other) = tx.get::<Reference>(link_to)? {
                        if other.work_id.is_empty() {
                            other.work_id = new_id();
                            other.preferred_version = other.rtype != "preprint";
                            other.version_label = if other.rtype == "preprint" { "Preprint".into() } else { "Published".into() };
                            tx.put(&other)?;
                        }
                        r.work_id = other.work_id.clone();
                        r.version_label = if r.rtype == "preprint" { "Preprint".into() } else { "Published".into() };
                    }
                }
                tx.put(&r)?;
                if !project.is_empty() {
                    add_membership(tx, &project, &r.id)?;
                }
            }
            for (target, incoming) in &merges {
                let mut e = tx.must::<Reference>(target)?;
                // Fill missing values; keep conflicting values for review, never overwrite user edits.
                for (k, v) in &incoming.fields {
                    match e.fields.get(k) {
                        None => {
                            e.fields.insert(k.clone(), v.clone());
                            e.provenance.insert(k.clone(), staged_prov(incoming, k, &source, &t));
                        }
                        Some(cur) if crate::util::sort_key(cur) != crate::util::sort_key(v) => {
                            if !e.conflicts.iter().any(|c| &c.field == k && &c.value == v) {
                                e.conflicts.push(FieldConflict { field: k.clone(), value: v.clone(), source: source.clone(), time: t.clone() });
                            }
                        }
                        _ => {}
                    }
                }
                if e.creators.is_empty() {
                    e.creators = incoming.creators.clone();
                }
                if e.issued.is_none() {
                    e.issued = incoming.issued.clone();
                }
                for tag in &incoming.tags {
                    if !e.tags.contains(tag) {
                        e.tags.push(tag.clone());
                    }
                }
                for (k, v) in &incoming.overflow {
                    e.overflow.entry(k.clone()).or_insert_with(|| v.clone());
                }
                e.revision += 1;
                e.modified = t.clone();
                tx.put(&e)?;
                if !project.is_empty() {
                    add_membership(tx, &project, &e.id)?;
                }
            }
            for d in &decisions {
                tx.put(d)?;
            }
            // Groups from the exporting program become collections (nested by their paths).
            let mut made = 0usize;
            for (id, paths) in &group_of {
                for path in paths {
                    let mut parent = String::new();
                    for seg in path.split('/').filter(|s| !s.trim().is_empty()) {
                        let all: Vec<Collection> = tx.all()?;
                        parent = match all.into_iter().find(|c| c.parent_id == parent && c.name == seg.trim() && c.query.is_empty() && c.project_id == project) {
                            Some(c) => c.id,
                            None => {
                                made += 1;
                                let c = Collection { id: new_id(), name: seg.trim().to_string(), parent_id: parent.clone(), project_id: project.clone(), ..Default::default() };
                                tx.put(&c)?;
                                c.id
                            }
                        };
                    }
                    if !parent.is_empty() {
                        let mut c = tx.must::<Collection>(&parent)?;
                        if !c.ref_ids.contains(id) {
                            c.ref_ids.push(id.clone());
                            tx.put(&c)?;
                        }
                    }
                }
            }
            if !collection.is_empty() {
                if let Some(mut c) = tx.get::<Collection>(&collection)? {
                    for (r, _) in &new_refs {
                        c.ref_ids.push(r.id.clone());
                    }
                    for (target, _) in &merges {
                        if !c.ref_ids.contains(target) {
                            c.ref_ids.push(target.clone());
                        }
                    }
                    tx.put(&c)?;
                }
            }
            tx.put(&bb)?;
            tx.put(&IntegrationRun {
                id: new_id(),
                provider: bb.service.clone(),
                operation: "import".into(),
                transport: "file_exchange".into(),
                adapter_version: format!("{} {}", bb.adapter, bb.adapter_version),
                project_id: project.clone(),
                data_sent: "nothing (local file)".into(),
                result_ids: new_refs.iter().map(|(r, _)| r.id.clone()).collect(),
                ok: true,
                message: format!("{} added, {} merged, {} updated, {} skipped", report.added + report.linked, report.merged, report.updated, report.skipped),
                time: t.clone(),
            })?;
            Ok(made)
        });
        report.collections = r_written?;
        self.after_change()?;
        Ok(report)
    }

    /// What undoing a batch would do: records that can be removed and records that later work
    /// depends on (those are kept unless the person chooses otherwise).
    pub fn undo_import_preview(&self, batch_id: &str) -> Result<UndoPreview> {
        let b = self.store.must::<ImportBatch>(batch_id)?;
        let mut removable = Vec::new();
        let mut dependent = Vec::new();
        for sr in &b.records {
            // Merged and updated records existed before this import; undoing it never removes them
            // (their changes can be reversed with Undo or field history).
            if sr.committed_id.is_empty() || sr.decision == "merge" || sr.decision == "update" {
                continue;
            }
            let Some(r) = self.store.get::<Reference>(&sr.committed_id)? else { continue };
            let imp = self.impact(&r.id)?;
            let attachments = self.store.by::<Attachment>("ref_id", &r.id)?.len();
            let mut why = Vec::new();
            if imp.notes > 0 {
                why.push(format!("{} notes", imp.notes));
            }
            if imp.annotations > 0 {
                why.push(format!("{} annotations", imp.annotations));
            }
            if imp.copies > 0 {
                why.push(format!("{} copied citations", imp.copies));
            }
            if attachments > 0 {
                why.push(format!("{attachments} files"));
            }
            if r.revision > 1 {
                why.push("edited since import".into());
            }
            if why.is_empty() {
                removable.push(r.id.clone());
            } else {
                dependent.push((r.id.clone(), format!("{}: {}", super::refs::short(r.title()), why.join(", "))));
            }
        }
        Ok(UndoPreview { batch_id: batch_id.into(), removable, dependent })
    }

    /// Undo an import batch: move its records to the trash (recoverable). Records with later
    /// work are kept unless `include_dependent`.
    pub fn undo_import(&mut self, batch_id: &str, include_dependent: bool) -> Result<usize> {
        let p = self.undo_import_preview(batch_id)?;
        let mut ids = p.removable.clone();
        if include_dependent {
            ids.extend(p.dependent.iter().map(|(id, _)| id.clone()));
        }
        let n = self.trash(&ids)?;
        let mut b = self.store.must::<ImportBatch>(batch_id)?;
        b.state = if p.dependent.is_empty() || include_dependent { "undone".into() } else { "partial".into() };
        let bb = b.clone();
        self.write("Mark import undone", |tx| tx.put(&bb))?;
        Ok(n)
    }

    /// Migration scan and dry run: what an exported library contains and what can be carried.
    pub fn migration_scan(&self, file_name: &str, text: &str) -> Result<serde_json::Value> {
        let fmt = formats::detect(file_name, text);
        let parsed = formats::parse(fmt, text, None);
        let mut types: BTreeMap<String, usize> = BTreeMap::new();
        let mut with_files = 0;
        let mut with_notes = 0;
        let mut collections: BTreeMap<String, usize> = BTreeMap::new();
        let mut tags = std::collections::BTreeSet::new();
        for p in &parsed.records {
            *types.entry(crate::types::label(&p.record.rtype).to_string()).or_default() += 1;
            if p.record.overflow.keys().any(|k| k.ends_with(":L1") || k.contains("pdf") || k.ends_with(":file")) {
                with_files += 1;
            }
            if p.record.has("note") || p.record.overflow.keys().any(|k| k.contains("research-notes") || k.contains("annote")) {
                with_notes += 1;
            }
            for t in &p.record.tags {
                tags.insert(t.clone());
            }
            if let Some(g) = p.record.overflow.get("bibtex:groups").or(p.record.overflow.get("xml:label")) {
                *collections.entry(g.clone()).or_default() += 1;
            }
        }
        Ok(serde_json::json!({
            "format": fmt,
            "records": parsed.records.len(),
            "types": types,
            "tags": tags.len(),
            "groups_found": collections,
            "records_with_file_links": with_files,
            "records_with_notes": with_notes,
            "unmapped_fields": parsed.unmapped,
            "errors": parsed.errors,
            "cannot_carry": [
                "Annotations stored inside the other program's database (export them into PDFs, then import those annotations)",
                "Saved searches and smart groups (recreate them as smart collections)",
                "File attachments, unless you attach the companion folder of PDFs",
            ],
            "note": "An export file is not a complete library backup. Records are staged for review before anything is added; unmapped fields are kept in each record's overflow area.",
        }))
    }
}
