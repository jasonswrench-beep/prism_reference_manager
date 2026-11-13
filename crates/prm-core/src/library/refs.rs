//! Reference operations: list, add, edit with provenance, trash, merge with aliases, versions,
//! duplicates, bulk edits with preview, find and replace, labels and collections.

use super::Library;
use crate::error::{Error, Result};
use crate::model::*;
use crate::util::{new_id, now, sort_key};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct RefRow {
    pub id: String,
    pub rtype: String,
    pub type_label: String,
    pub authors: String,
    pub year: String,
    pub title: String,
    pub container: String,
    pub citekey: String,
    pub has_pdf: bool,
    pub reading: String,
    pub inclusion: String,
    pub rating: u8,
    pub labels: Vec<String>,
    pub modified: String,
    pub created: String,
    pub issues: usize,
    pub notices: usize,
    pub origin: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListQuery {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub collection_id: String,
    /// all, trash, recent_added, recent_modified, recent_read, inbox, needs_metadata, to_read
    #[serde(default)]
    pub view: String,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub desc: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub rtype: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefPage {
    pub total: usize,
    pub rows: Vec<RefRow>,
    pub pending_index: i64,
    pub search_hits: Vec<crate::search::Hit>,
}

pub fn author_short(r: &Reference) -> String {
    let names: Vec<String> = r.creators.iter().filter(|c| c.role == "author").map(|c| c.name.surname()).collect();
    let names = if names.is_empty() { r.creators.iter().filter(|c| c.role == "editor").map(|c| c.name.surname()).collect() } else { names };
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2 => format!("{} & {}", names[0], names[1]),
        _ => format!("{} et al.", names[0]),
    }
}

/// Fields compared when deciding if an edit changes citation output or only metadata.
pub fn changed_fields(a: &Reference, b: &Reference) -> Vec<String> {
    let mut out = Vec::new();
    let keys: BTreeSet<&String> = a.fields.keys().chain(b.fields.keys()).collect();
    for k in keys {
        if a.fields.get(k) != b.fields.get(k) {
            out.push(k.clone());
        }
    }
    if a.creators != b.creators {
        out.push("creators".into());
    }
    if a.issued != b.issued {
        out.push("issued".into());
    }
    if a.rtype != b.rtype {
        out.push("type".into());
    }
    if a.original_date != b.original_date {
        out.push("original_date".into());
    }
    if a.accessed != b.accessed {
        out.push("accessed".into());
    }
    if a.tags != b.tags {
        out.push("tags".into());
    }
    if a.labels != b.labels {
        out.push("labels".into());
    }
    if a.citekey != b.citekey {
        out.push("citekey".into());
    }
    if a.confirmed_absent != b.confirmed_absent {
        out.push("confirmed_absent".into());
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct Impact {
    pub projects: Vec<(String, String)>,
    pub manuscripts: Vec<(String, String)>,
    pub copies: usize,
    pub notes: usize,
    pub annotations: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BulkOp {
    /// set, append, prefix, clear, copy, move, rename
    pub op: String,
    pub field: String,
    #[serde(default)]
    pub value: String,
    /// Target field for copy, move and rename.
    #[serde(default)]
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BulkChange {
    pub ref_id: String,
    pub title: String,
    pub field: String,
    pub before: String,
    pub after: String,
}

impl Library {
    pub fn list_references(&self, q: &ListQuery) -> Result<RefPage> {
        let mut refs: Vec<Reference> = match q.view.as_str() {
            "trash" => crate::store::list_in(&self.store.conn, "WHERE trashed IS NOT NULL", &[])?,
            _ => crate::store::list_in(&self.store.conn, "WHERE trashed IS NULL", &[])?,
        };
        let memberships: BTreeMap<String, ProjectSource> = if !q.project_id.is_empty() {
            self.store.by::<ProjectSource>("project_id", &q.project_id)?.into_iter().map(|m| (m.ref_id.clone(), m)).collect()
        } else {
            BTreeMap::new()
        };
        if !q.project_id.is_empty() {
            refs.retain(|r| memberships.contains_key(&r.id));
        }
        if !q.collection_id.is_empty() {
            let c = self.store.must::<Collection>(&q.collection_id)?;
            if !c.query.is_empty() {
                let res = crate::search::run(&self.store.conn, &c.query, None, None, usize::MAX)?;
                let set: BTreeSet<String> = res.ref_ids.into_iter().collect();
                refs.retain(|r| set.contains(&r.id));
            } else {
                let mut ids: BTreeSet<String> = c.ref_ids.iter().cloned().collect();
                // Include sub-collections.
                let all: Vec<Collection> = self.store.all()?;
                let mut stack = vec![c.id.clone()];
                while let Some(p) = stack.pop() {
                    for ch in all.iter().filter(|x| x.parent_id == p) {
                        ids.extend(ch.ref_ids.iter().cloned());
                        stack.push(ch.id.clone());
                    }
                }
                refs.retain(|r| ids.contains(&r.id));
            }
        }
        if !q.label.is_empty() {
            refs.retain(|r| r.labels.contains(&q.label));
        }
        if !q.rtype.is_empty() {
            refs.retain(|r| r.rtype == q.rtype);
        }
        let attachments: Vec<Attachment> = self.store.all()?;
        let with_pdf: BTreeSet<&str> = attachments.iter().filter(|a| a.mime == "application/pdf" && !a.superseded).map(|a| a.ref_id.as_str()).collect();
        match q.view.as_str() {
            "recent_added" => refs.retain(|r| chrono::DateTime::parse_from_rfc3339(&r.created).map(|t| chrono::Utc::now().signed_duration_since(t).num_days() <= 14).unwrap_or(false)),
            "recent_modified" => refs.retain(|r| chrono::DateTime::parse_from_rfc3339(&r.modified).map(|t| chrono::Utc::now().signed_duration_since(t).num_days() <= 14).unwrap_or(false)),
            "needs_metadata" => refs.retain(|r| !r.conflicts.is_empty() || r.title().is_empty() || r.issued.is_none() && !r.absent("issued") || r.creators.is_empty() && !r.absent("author")),
            "to_read" => refs.retain(|r| memberships.get(&r.id).map(|m| m.reading != "read").unwrap_or(false)),
            "recent_read" => refs.retain(|r| memberships.get(&r.id).and_then(|m| m.reading_position.as_ref()).is_some()),
            "machine" => refs.retain(|r| matches!(r.origin.as_str(), "ai" | "agent")),
            _ => {}
        }
        let mut hits = Vec::new();
        if !q.search.trim().is_empty() {
            let res = crate::search::run(&self.store.conn, &q.search, None, None, usize::MAX)?;
            let order: BTreeMap<String, usize> = res.ref_ids.iter().enumerate().map(|(i, id)| (id.clone(), i)).collect();
            refs.retain(|r| order.contains_key(&r.id));
            hits = res.hits;
        }
        let mut rows: Vec<RefRow> = refs
            .iter()
            .map(|r| {
                let m = memberships.get(&r.id);
                RefRow {
                    id: r.id.clone(),
                    rtype: r.rtype.clone(),
                    type_label: crate::types::label(&r.rtype).to_string(),
                    authors: author_short(r),
                    year: r.issued.as_ref().map(|d| d.year.map(|y| y.to_string()).unwrap_or(d.status.clone())).unwrap_or_default(),
                    title: r.title().to_string(),
                    container: r.f("container_title").to_string(),
                    citekey: r.citekey.clone(),
                    has_pdf: with_pdf.contains(r.id.as_str()),
                    reading: m.map(|m| m.reading.clone()).unwrap_or_default(),
                    inclusion: m.map(|m| m.inclusion.clone()).unwrap_or_default(),
                    rating: m.map(|m| m.rating).filter(|x| *x > 0).unwrap_or(r.rating),
                    labels: r.labels.clone(),
                    modified: r.modified.clone(),
                    created: r.created.clone(),
                    issues: r.conflicts.len() + usize::from(r.title().is_empty()),
                    notices: r.notices.len(),
                    origin: r.origin.clone(),
                }
            })
            .collect();
        let sort = if q.sort.is_empty() { "authors" } else { q.sort.as_str() };
        rows.sort_by(|a, b| {
            let o = match sort {
                "year" => a.year.cmp(&b.year),
                "title" => sort_key(&a.title).cmp(&sort_key(&b.title)),
                "container" => sort_key(&a.container).cmp(&sort_key(&b.container)),
                "modified" => a.modified.cmp(&b.modified),
                "created" => a.created.cmp(&b.created),
                "type" => a.type_label.cmp(&b.type_label),
                "rating" => a.rating.cmp(&b.rating),
                "citekey" => a.citekey.cmp(&b.citekey),
                _ => sort_key(&a.authors).cmp(&sort_key(&b.authors)),
            };
            // Stable tiebreakers keep selection stable through sorting and refresh.
            let o = o.then(sort_key(&a.title).cmp(&sort_key(&b.title))).then(a.id.cmp(&b.id));
            if q.desc { o.reverse() } else { o }
        });
        let total = rows.len();
        let limit = if q.limit == 0 { 500 } else { q.limit };
        let rows = rows.into_iter().skip(q.offset).take(limit).collect();
        Ok(RefPage { total, rows, pending_index: crate::search::pending_index_count(&self.store.conn)?, search_hits: hits })
    }

    fn taken_keys(&self) -> Result<BTreeSet<String>> {
        let mut st = self.store.conn.prepare("SELECT json_extract(data,'$.citekey') FROM refs")?;
        let v: BTreeSet<String> = st.query_map([], |r| r.get::<_, Option<String>>(0))?.filter_map(|r| r.ok().flatten()).collect();
        Ok(v)
    }

    pub fn prepare_new(&self, r: &mut Reference, source: &str) -> Result<()> {
        if r.id.is_empty() {
            r.id = new_id();
        }
        let t = now();
        r.created = t.clone();
        r.modified = t.clone();
        r.revision = 1;
        if r.origin.is_empty() {
            r.origin = "user".into();
        }
        crate::formats::finish(r);
        let taken = self.taken_keys()?;
        if r.citekey.is_empty() || taken.contains(&r.citekey) {
            let base = if r.citekey.is_empty() { crate::citekey::generate(r, &self.citekey_pattern) } else { r.citekey.clone() };
            r.citekey = crate::citekey::unique(&base, &taken);
        }
        let fields: Vec<String> = r.fields.keys().cloned().chain(["creators".to_string(), "issued".to_string()]).collect();
        for f in fields {
            r.provenance.entry(f).or_insert(Provenance { source: source.into(), time: t.clone(), user_edited: source == "user", confidence: None, review: if source == "user" { "reviewed".into() } else { "unreviewed".into() } });
        }
        Ok(())
    }

    /// Add a new reference, optionally as a member of a project.
    pub fn add_reference(&mut self, mut r: Reference, project_id: Option<&str>) -> Result<Reference> {
        self.prepare_new(&mut r, "user")?;
        let pid = project_id.unwrap_or("").to_string();
        let rr = r.clone();
        self.write(&format!("Add “{}”", short(&r.title().to_string())), |tx| {
            tx.put(&rr)?;
            if !pid.is_empty() {
                add_membership(tx, &pid, &rr.id)?;
            }
            Ok(())
        })?;
        Ok(r)
    }

    /// Projects, manuscripts and copies that use a reference (shown before a shared edit).
    pub fn impact(&self, ref_id: &str) -> Result<Impact> {
        let ps: Vec<ProjectSource> = self.store.by("ref_id", ref_id)?;
        let mut projects = Vec::new();
        for p in &ps {
            if let Some(pr) = self.store.get::<Project>(&p.project_id)? {
                projects.push((pr.id, pr.title));
            }
        }
        let ms: Vec<ManuscriptSource> = self.store.by("ref_id", ref_id)?;
        let mut manuscripts = Vec::new();
        for m in &ms {
            if let Some(mm) = self.store.get::<Manuscript>(&m.manuscript_id)? {
                manuscripts.push((mm.id, mm.title));
            }
        }
        let copies: i64 = self.store.conn.query_row("SELECT COUNT(*) FROM copy_events, json_each(copy_events.data,'$.items') j WHERE json_extract(j.value,'$.ref_id')=?1", [ref_id], |r| r.get(0))?;
        let notes: i64 = self.store.conn.query_row("SELECT COUNT(*) FROM notes, json_each(notes.data,'$.ref_ids') j WHERE j.value=?1", [ref_id], |r| r.get(0))?;
        let annotations: i64 = self.store.conn.query_row("SELECT COUNT(*) FROM annotations n JOIN attachments a ON a.id=n.attachment_id WHERE a.ref_id=?1", [ref_id], |r| r.get(0))?;
        Ok(Impact { projects, manuscripts, copies: copies as usize, notes: notes as usize, annotations: annotations as usize })
    }

    /// Save an edited reference. Records provenance for changed fields, bumps the revision and
    /// re-checks copied output that used it.
    pub fn update_reference(&mut self, mut r: Reference, source: &str) -> Result<Reference> {
        let old = self.store.must::<Reference>(&r.id)?;
        let changed = changed_fields(&old, &r);
        if changed.is_empty() && old.provenance == r.provenance && old.rating == r.rating && old.notices == r.notices && old.conflicts == r.conflicts && old.custom == r.custom && old.overflow == r.overflow && old.translations == r.translations && old.work_id == r.work_id && old.preferred_version == r.preferred_version && old.citekey_locked == r.citekey_locked {
            return Ok(old);
        }
        let t = now();
        for f in &changed {
            r.provenance.insert(f.clone(), Provenance { source: source.into(), time: t.clone(), user_edited: source == "user", confidence: None, review: if source == "user" { "reviewed".into() } else { "unreviewed".into() } });
        }
        if let Some(d) = r.fields.get("doi").cloned() {
            if let Some(n) = crate::util::normalize_doi(&d) {
                r.fields.insert("doi".into(), n);
            }
        }
        if changed.iter().any(|c| c == "citekey") {
            let mut taken = self.taken_keys()?;
            taken.remove(&old.citekey);
            if taken.contains(&r.citekey) {
                return Err(Error::user(format!("The citation key “{}” is already used by another source.", r.citekey)));
            }
        }
        r.revision = old.revision + 1;
        r.modified = t;
        r.created = old.created.clone();
        let rr = r.clone();
        self.write(&format!("Edit “{}”", short(r.title())), |tx| tx.put(&rr))?;
        self.recompute_copies(None)?;
        Ok(r)
    }

    pub fn regenerate_citekey(&mut self, id: &str) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(id)?;
        if r.citekey_locked {
            return Err(Error::user("This citation key is locked. Unlock it to generate a new one."));
        }
        let mut taken = self.taken_keys()?;
        taken.remove(&r.citekey);
        r.citekey = crate::citekey::unique(&crate::citekey::generate(&r, &self.citekey_pattern), &taken);
        self.update_reference(r, "rule:citekey")
    }

    /// Move references to the recoverable trash. Memberships and notes stay so restore is complete.
    pub fn trash(&mut self, ids: &[String]) -> Result<usize> {
        let t = now();
        let mut n = 0;
        self.write(&format!("Move {} source{} to trash", ids.len(), if ids.len() == 1 { "" } else { "s" }), |tx| {
            for id in ids {
                if let Some(mut r) = tx.get::<Reference>(id)? {
                    if r.trashed_at.is_none() {
                        r.trashed_at = Some(t.clone());
                        tx.put(&r)?;
                        n += 1;
                    }
                }
            }
            Ok(())
        })?;
        Ok(n)
    }

    pub fn restore(&mut self, ids: &[String]) -> Result<usize> {
        let mut n = 0;
        self.write("Restore from trash", |tx| {
            for id in ids {
                if let Some(mut r) = tx.get::<Reference>(id)? {
                    if r.trashed_at.is_some() {
                        r.trashed_at = None;
                        tx.put(&r)?;
                        n += 1;
                    }
                }
            }
            Ok(())
        })?;
        Ok(n)
    }

    /// Permanently delete trashed references and everything that belongs only to them.
    /// `confirmed` must be true: the interface asks first.
    pub fn purge(&mut self, ids: &[String], confirmed: bool) -> Result<usize> {
        if !confirmed {
            return Err(Error::user("Permanent deletion needs confirmation."));
        }
        self.safety_snapshot("delete")?;
        let mut n = 0;
        let mut files_to_check = Vec::new();
        self.write("Delete permanently", |tx| {
            for id in ids {
                let Some(r) = tx.get::<Reference>(id)? else { continue };
                if r.trashed_at.is_none() {
                    continue;
                }
                for m in tx.by::<ProjectSource>("ref_id", id)? {
                    tx.delete::<ProjectSource>(&m.id)?;
                }
                for m in tx.by::<ManuscriptSource>("ref_id", id)? {
                    tx.delete::<ManuscriptSource>(&m.id)?;
                }
                for a in tx.by::<Attachment>("ref_id", id)? {
                    for an in tx.by::<Annotation>("attachment_id", &a.id)? {
                        tx.delete::<Annotation>(&an.id)?;
                    }
                    files_to_check.push(a.clone());
                    tx.delete::<Attachment>(&a.id)?;
                }
                for mut c in tx.all::<Collection>()? {
                    if c.ref_ids.contains(id) {
                        c.ref_ids.retain(|x| x != id);
                        tx.put(&c)?;
                    }
                }
                for mut note in tx.all::<Note>()? {
                    if note.ref_ids.contains(id) {
                        note.ref_ids.retain(|x| x != id);
                        if note.ref_ids.is_empty() && !note.exact_text.is_empty() {
                            note.anchor_state = "unresolved".into();
                        }
                        tx.put(&note)?;
                    }
                }
                tx.delete::<Reference>(id)?;
                n += 1;
            }
            Ok(())
        })?;
        // Remove managed files no other attachment uses.
        for a in files_to_check {
            if a.mode == "managed" {
                let used: i64 = self.store.conn.query_row("SELECT COUNT(*) FROM attachments WHERE hash=?1", [&a.hash], |r| r.get(0))?;
                if used == 0 {
                    let p = self.files_dir().join(&a.path);
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        Ok(n)
    }

    /// Permanently delete items that have been in the trash longer than `days`.
    pub fn empty_trash_older_than(&mut self, days: i64) -> Result<usize> {
        let refs: Vec<Reference> = crate::store::list_in(&self.store.conn, "WHERE trashed IS NOT NULL", &[])?;
        let old: Vec<String> = refs
            .into_iter()
            .filter(|r| r.trashed_at.as_ref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| chrono::Utc::now().signed_duration_since(t).num_days() >= days).unwrap_or(false))
            .map(|r| r.id)
            .collect();
        if old.is_empty() {
            return Ok(0);
        }
        self.purge(&old, true)
    }

    /// Merge `others` into `target`. `choices` maps a field to the reference id whose value to
    /// keep. Memberships, notes, attachments, annotations and copy references move to the
    /// target; each retired id becomes an alias so old links still resolve.
    pub fn merge(&mut self, target: &str, others: &[String], choices: &BTreeMap<String, String>) -> Result<Reference> {
        self.safety_snapshot("merge")?;
        let mut t = self.store.must::<Reference>(target)?;
        let sources: Vec<Reference> = others.iter().filter(|o| o.as_str() != target).map(|o| self.store.must::<Reference>(o)).collect::<Result<_>>()?;
        for (field, from) in choices {
            if from == target {
                continue;
            }
            let Some(src) = sources.iter().find(|s| &s.id == from) else { continue };
            match field.as_str() {
                "creators" => t.creators = src.creators.clone(),
                "issued" => t.issued = src.issued.clone(),
                "type" => t.rtype = src.rtype.clone(),
                f => {
                    if let Some(v) = src.fields.get(f) {
                        t.fields.insert(f.into(), v.clone());
                    } else {
                        t.fields.remove(f);
                    }
                }
            }
            if let Some(p) = src.provenance.get(field) {
                t.provenance.insert(field.clone(), p.clone());
            }
        }
        // Fill gaps from the merged records without overwriting.
        for s in &sources {
            for (k, v) in &s.fields {
                t.fields.entry(k.clone()).or_insert_with(|| v.clone());
            }
            for tag in &s.tags {
                if !t.tags.contains(tag) {
                    t.tags.push(tag.clone());
                }
            }
            for l in &s.labels {
                if !t.labels.contains(l) {
                    t.labels.push(l.clone());
                }
            }
            for (k, v) in &s.overflow {
                t.overflow.entry(k.clone()).or_insert_with(|| v.clone());
            }
            t.notices.extend(s.notices.clone());
        }
        t.revision += 1;
        t.modified = now();
        let tt = t.clone();
        let ts = now();
        self.write(&format!("Merge {} record{}", sources.len() + 1, if sources.is_empty() { "" } else { "s" }), |tx| {
            tx.put(&tt)?;
            for s in &sources {
                for m in tx.by::<ProjectSource>("ref_id", &s.id)? {
                    let key = ProjectSource::key(&m.project_id, &tt.id);
                    if tx.get::<ProjectSource>(&key)?.is_none() {
                        let mut nm = m.clone();
                        nm.id = key;
                        nm.ref_id = tt.id.clone();
                        tx.put(&nm)?;
                    }
                    tx.delete::<ProjectSource>(&m.id)?;
                }
                for m in tx.by::<ManuscriptSource>("ref_id", &s.id)? {
                    let key = ManuscriptSource::key(&m.manuscript_id, &tt.id);
                    let existing = tx.get::<ManuscriptSource>(&key)?;
                    let mut nm = m.clone();
                    nm.id = key;
                    nm.ref_id = tt.id.clone();
                    // Keep the strongest use state.
                    if let Some(e) = existing {
                        if rank(&e.state) > rank(&nm.state) {
                            nm.state = e.state;
                        }
                    }
                    tx.put(&nm)?;
                    tx.delete::<ManuscriptSource>(&m.id)?;
                }
                for mut a in tx.by::<Attachment>("ref_id", &s.id)? {
                    a.ref_id = tt.id.clone();
                    tx.put(&a)?;
                }
                for mut n in tx.all::<Note>()? {
                    if n.ref_ids.contains(&s.id) {
                        n.ref_ids = n.ref_ids.iter().map(|x| if x == &s.id { tt.id.clone() } else { x.clone() }).collect();
                        n.ref_ids.dedup();
                        tx.put(&n)?;
                    }
                }
                for mut c in tx.all::<Collection>()? {
                    if c.ref_ids.contains(&s.id) {
                        c.ref_ids.retain(|x| x != &s.id);
                        if !c.ref_ids.contains(&tt.id) {
                            c.ref_ids.push(tt.id.clone());
                        }
                        tx.put(&c)?;
                    }
                }
                tx.put(&Alias { id: s.id.clone(), target: tt.id.clone(), merged_at: ts.clone() })?;
                tx.delete::<Reference>(&s.id)?;
            }
            Ok(())
        })?;
        self.recompute_copies(None)?;
        Ok(t)
    }

    /// Link records as versions of one work and choose the one to cite.
    pub fn link_versions(&mut self, ids: &[String], preferred: &str) -> Result<()> {
        let work = new_id();
        self.write("Link versions", |tx| {
            for id in ids {
                let mut r = tx.must::<Reference>(id)?;
                r.work_id = work.clone();
                r.preferred_version = r.id == preferred;
                if r.version_label.is_empty() {
                    r.version_label = if r.rtype == "preprint" { "Preprint".into() } else { "Published".into() };
                }
                tx.put(&r)?;
            }
            Ok(())
        })
    }

    pub fn unlink_version(&mut self, id: &str) -> Result<()> {
        self.write("Unlink version", |tx| {
            let mut r = tx.must::<Reference>(id)?;
            r.work_id.clear();
            r.preferred_version = false;
            tx.put(&r)
        })
    }

    pub fn duplicates(&self, threshold: f32) -> Result<Vec<crate::dedupe::Candidate>> {
        let refs: Vec<Reference> = crate::store::list_in(&self.store.conn, "WHERE trashed IS NULL", &[])?;
        let decided: BTreeSet<String> = self.store.all::<DuplicateDecision>()?.into_iter().map(|d| d.id).collect();
        let mut hashes: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for a in self.store.all::<Attachment>()? {
            hashes.entry(a.ref_id.clone()).or_default().push(a.hash.clone());
        }
        // Records already linked as versions of one work are not duplicates.
        let mut out = crate::dedupe::find(&refs, threshold, &decided, &hashes);
        let work: BTreeMap<&str, &str> = refs.iter().filter(|r| !r.work_id.is_empty()).map(|r| (r.id.as_str(), r.work_id.as_str())).collect();
        out.retain(|c| !(work.get(c.a.as_str()).is_some() && work.get(c.a.as_str()) == work.get(c.b.as_str())));
        Ok(out)
    }

    /// Record a durable duplicate decision: not_duplicate, keep_both or versions.
    pub fn decide_duplicate(&mut self, a: &str, b: &str, decision: &str) -> Result<()> {
        let d = DuplicateDecision { id: DuplicateDecision::key(a, b), a: a.into(), b: b.into(), decision: decision.into(), time: now() };
        self.write("Duplicate decision", |tx| tx.put(&d))
    }

    pub fn field_history(&self, id: &str, field: &str) -> Result<Vec<serde_json::Value>> {
        let path: Vec<&str> = match field {
            "creators" | "issued" | "type" | "citekey" | "tags" | "labels" => vec![field],
            f => vec!["fields", f],
        };
        let path: Vec<&str> = path.into_iter().map(|p| if p == "type" { "type" } else { p }).collect();
        self.store.field_history("refs", id, &path)
    }

    /// Restore an earlier value of one field.
    pub fn restore_field(&mut self, id: &str, field: &str, value: serde_json::Value) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(id)?;
        match field {
            "creators" => r.creators = serde_json::from_value(value)?,
            "issued" => r.issued = serde_json::from_value(value)?,
            f => match value.as_str() {
                Some(s) => {
                    r.fields.insert(f.into(), s.into());
                }
                None => {
                    r.fields.remove(f);
                }
            },
        }
        self.update_reference(r, "user")
    }

    // ---- bulk edits -----------------------------------------------------------------------

    fn bulk_apply(r: &mut Reference, op: &BulkOp) -> Option<(String, String)> {
        let get = |r: &Reference, f: &str| -> String {
            match f {
                "tags" => r.tags.join("; "),
                "type" => r.rtype.clone(),
                f => r.fields.get(f).cloned().unwrap_or_default(),
            }
        };
        let set = |r: &mut Reference, f: &str, v: String| match f {
            "tags" => r.tags = v.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            "type" => r.rtype = v,
            f => {
                if v.is_empty() {
                    r.fields.remove(f);
                } else {
                    r.fields.insert(f.into(), v);
                }
            }
        };
        let before = get(r, &op.field);
        match op.op.as_str() {
            "set" => set(r, &op.field, op.value.clone()),
            "append" => set(r, &op.field, format!("{before}{}", op.value)),
            "prefix" => set(r, &op.field, format!("{}{before}", op.value)),
            "clear" => set(r, &op.field, String::new()),
            "copy" => {
                let b2 = get(r, &op.target);
                set(r, &op.target, before.clone());
                return if b2 != before { Some((b2, before)) } else { None };
            }
            "move" | "rename" => {
                let b2 = get(r, &op.target);
                if before.is_empty() {
                    return None;
                }
                set(r, &op.target, before.clone());
                set(r, &op.field, String::new());
                return Some((format!("{}: {before}; {}: {b2}", op.field, op.target), format!("{}: {before}", op.target)));
            }
            _ => return None,
        }
        let after = get(r, &op.field);
        if after != before { Some((before, after)) } else { None }
    }

    /// Preview (apply=false) or apply a bulk field change. Applying is one undo step.
    pub fn bulk_edit(&mut self, ids: &[String], op: &BulkOp, apply: bool) -> Result<Vec<BulkChange>> {
        let mut changes = Vec::new();
        let mut updated = Vec::new();
        for id in ids {
            let mut r = self.store.must::<Reference>(id)?;
            if let Some((b, a)) = Self::bulk_apply(&mut r, op) {
                changes.push(BulkChange { ref_id: r.id.clone(), title: r.title().to_string(), field: op.field.clone(), before: b, after: a });
                updated.push(r);
            }
        }
        if apply && !updated.is_empty() {
            self.safety_snapshot("bulk-edit")?;
            let t = now();
            let label = format!("Bulk {} {} on {} sources", op.op, crate::types::field_label(&op.field), updated.len());
            self.write(&label, |tx| {
                for mut r in updated {
                    r.revision += 1;
                    r.modified = t.clone();
                    r.provenance.insert(op.field.clone(), Provenance { source: "user:bulk".into(), time: t.clone(), user_edited: true, confidence: None, review: "reviewed".into() });
                    tx.put(&r)?;
                }
                Ok(())
            })?;
            self.recompute_copies(None)?;
        }
        Ok(changes)
    }

    /// Find and replace inside one field (or all text fields with "*"), with preview.
    pub fn find_replace(&mut self, ids: Option<&[String]>, field: &str, find: &str, replace: &str, regex: bool, case_sensitive: bool, apply: bool) -> Result<Vec<BulkChange>> {
        if find.is_empty() {
            return Err(Error::user("Enter the text to find."));
        }
        let re = if regex {
            regex::RegexBuilder::new(find).case_insensitive(!case_sensitive).size_limit(1 << 20).build().map_err(|e| Error::user(format!("The pattern could not be used: {e}")))?
        } else {
            regex::RegexBuilder::new(&regex::escape(find)).case_insensitive(!case_sensitive).build().map_err(|e| Error::user(e.to_string()))?
        };
        let refs: Vec<Reference> = match ids {
            Some(ids) => ids.iter().filter_map(|i| self.store.get::<Reference>(i).ok().flatten()).collect(),
            None => crate::store::list_in(&self.store.conn, "WHERE trashed IS NULL", &[])?,
        };
        let mut changes = Vec::new();
        let mut updated = Vec::new();
        for mut r in refs {
            let fields: Vec<String> = if field == "*" { r.fields.keys().cloned().collect() } else { vec![field.to_string()] };
            let mut touched = false;
            for f in fields {
                let Some(v) = r.fields.get(&f).cloned() else { continue };
                let nv = re.replace_all(&v, replace).into_owned();
                if nv != v {
                    changes.push(BulkChange { ref_id: r.id.clone(), title: r.title().to_string(), field: f.clone(), before: v, after: nv.clone() });
                    r.fields.insert(f, nv);
                    touched = true;
                }
            }
            if touched {
                updated.push(r);
            }
        }
        if apply && !updated.is_empty() {
            self.safety_snapshot("find-replace")?;
            let t = now();
            self.write(&format!("Find and replace in {} sources", updated.len()), |tx| {
                for mut r in updated {
                    r.revision += 1;
                    r.modified = t.clone();
                    tx.put(&r)?;
                }
                Ok(())
            })?;
            self.recompute_copies(None)?;
        }
        Ok(changes)
    }

    // ---- labels, collections, saved searches ---------------------------------------------

    pub fn save_label(&mut self, mut l: Label) -> Result<Label> {
        if l.id.is_empty() {
            l.id = new_id();
        }
        let ll = l.clone();
        self.write("Save label", |tx| tx.put(&ll))?;
        Ok(l)
    }

    /// Delete a label, or merge it into `into` when given.
    pub fn delete_label(&mut self, id: &str, into: Option<&str>) -> Result<()> {
        let refs: Vec<Reference> = self.store.all()?;
        self.write(if into.is_some() { "Merge labels" } else { "Delete label" }, |tx| {
            for mut r in refs {
                if r.labels.iter().any(|l| l == id) {
                    r.labels.retain(|l| l != id);
                    if let Some(t) = into {
                        if !r.labels.iter().any(|l| l == t) {
                            r.labels.push(t.to_string());
                        }
                    }
                    tx.put(&r)?;
                }
            }
            tx.delete::<Label>(id)
        })
    }

    pub fn set_labels(&mut self, ids: &[String], label: &str, on: bool) -> Result<()> {
        self.write(if on { "Add label" } else { "Remove label" }, |tx| {
            for id in ids {
                let mut r = tx.must::<Reference>(id)?;
                let has = r.labels.iter().any(|l| l == label);
                if on && !has {
                    r.labels.push(label.into());
                    tx.put(&r)?;
                } else if !on && has {
                    r.labels.retain(|l| l != label);
                    tx.put(&r)?;
                }
            }
            Ok(())
        })
    }

    pub fn set_rating(&mut self, ids: &[String], rating: u8) -> Result<()> {
        self.write("Set rating", |tx| {
            for id in ids {
                let mut r = tx.must::<Reference>(id)?;
                r.rating = rating.min(5);
                tx.put(&r)?;
            }
            Ok(())
        })
    }

    pub fn save_collection(&mut self, mut c: Collection) -> Result<Collection> {
        if c.id.is_empty() {
            c.id = new_id();
            c.created = now();
        }
        if !c.parent_id.is_empty() {
            // Prevent cycles.
            let all: Vec<Collection> = self.store.all()?;
            let mut p = c.parent_id.clone();
            for _ in 0..1000 {
                if p == c.id {
                    return Err(Error::user("A collection cannot be placed inside itself."));
                }
                match all.iter().find(|x| x.id == p) {
                    Some(x) if !x.parent_id.is_empty() => p = x.parent_id.clone(),
                    _ => break,
                }
            }
        }
        let cc = c.clone();
        self.write("Save collection", |tx| tx.put(&cc))?;
        Ok(c)
    }

    pub fn delete_collection(&mut self, id: &str) -> Result<()> {
        let all: Vec<Collection> = self.store.all()?;
        self.write("Delete collection", |tx| {
            let mut stack = vec![id.to_string()];
            while let Some(c) = stack.pop() {
                for ch in all.iter().filter(|x| x.parent_id == c) {
                    stack.push(ch.id.clone());
                }
                tx.delete::<Collection>(&c)?;
            }
            Ok(())
        })
    }

    pub fn collect(&mut self, collection_id: &str, ids: &[String], add: bool) -> Result<()> {
        self.write(if add { "Add to collection" } else { "Remove from collection" }, |tx| {
            let mut c = tx.must::<Collection>(collection_id)?;
            if !c.query.is_empty() {
                return Err(Error::user("A smart collection fills itself from its search."));
            }
            for id in ids {
                if add && !c.ref_ids.contains(id) {
                    c.ref_ids.push(id.clone());
                } else if !add {
                    c.ref_ids.retain(|x| x != id);
                }
            }
            tx.put(&c)
        })
    }

    pub fn save_search(&mut self, mut s: SavedSearch) -> Result<SavedSearch> {
        crate::search::parse(&s.query)?;
        if s.id.is_empty() {
            s.id = new_id();
            s.created = now();
        }
        let ss = s.clone();
        self.write("Save search", |tx| tx.put(&ss))?;
        Ok(s)
    }

    /// Resolve a conflict on a field by choosing a value; the rejected value stays in history.
    pub fn resolve_conflict(&mut self, id: &str, field: &str, value: Option<String>) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(id)?;
        if let Some(v) = value {
            r.fields.insert(field.into(), v);
        }
        r.conflicts.retain(|c| c.field != field);
        self.update_reference(r, "user")
    }

    pub fn confirm_absent(&mut self, id: &str, field: &str, absent: bool) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(id)?;
        r.confirmed_absent.retain(|f| f != field);
        if absent {
            r.confirmed_absent.push(field.into());
        }
        self.update_reference(r, "user")
    }
}

pub(crate) fn rank(s: &UseState) -> u8 {
    match s {
        UseState::Excluded => 0,
        UseState::Candidate => 1,
        UseState::Copied => 2,
        UseState::Confirmed => 3,
    }
}

pub(crate) fn add_membership(tx: &mut crate::store::Tx, project_id: &str, ref_id: &str) -> Result<bool> {
    let key = ProjectSource::key(project_id, ref_id);
    if tx.get::<ProjectSource>(&key)?.is_some() {
        return Ok(false);
    }
    tx.put(&ProjectSource { id: key, project_id: project_id.into(), ref_id: ref_id.into(), reading: "unread".into(), priority: "normal".into(), inclusion: "candidate".into(), added: now(), ..Default::default() })?;
    Ok(true)
}

pub fn short(s: &str) -> String {
    let t: String = s.chars().take(48).collect();
    if s.chars().count() > 48 { format!("{t}…") } else if t.is_empty() { "untitled".into() } else { t }
}
