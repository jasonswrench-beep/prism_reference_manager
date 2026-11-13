//! Copying or moving sources into another library (spec LIB-017).
//!
//! A copied source keeps its id, so copying it again is recognised; a source the other library
//! already holds (same id, same DOI or a near-certain match) is skipped and reported, never
//! duplicated. Attached files go with it (managed files are copied into the other library;
//! linked files stay linked to the same place), with their marks and page text. Library-wide
//! notes about the copied sources go too, and labels are matched by name. Projects and
//! collections belong to their own library and are not carried. Moving copies first and then
//! moves the originals to the trash here, so both steps can be undone.

use super::Library;
use crate::error::{Error, Result};
use crate::model::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Default)]
pub struct TransferReport {
    pub copied: usize,
    /// (title, reason) for sources not copied.
    pub skipped: Vec<(String, String)>,
    pub files: usize,
    pub marks: usize,
    pub notes: usize,
    pub labels_created: usize,
    /// Marks made in a project here become library-wide marks there.
    pub marks_made_library_wide: usize,
    pub moved_to_trash: usize,
}

impl Library {
    /// Copy `ids` into `target`; with `move_sources`, then move the originals to the trash.
    pub fn transfer_to(&mut self, target: &mut Library, ids: &[String], with_files: bool, move_sources: bool) -> Result<TransferReport> {
        if self.store.meta("library_id") == target.store.meta("library_id") {
            return Err(Error::user("Choose a different library to copy into."));
        }
        if target.store.read_only {
            return Err(Error::user("That library is open read-only (it was made by a newer version)."));
        }
        let mut rep = TransferReport::default();
        let here_name = self.store.meta("name").unwrap_or_default();
        let existing: Vec<Reference> = crate::store::list_in(&target.store.conn, "WHERE trashed IS NULL", &[])?;
        let existing_ids: BTreeSet<String> = target.store.all::<Reference>()?.into_iter().map(|r| r.id).collect();
        let matcher = crate::dedupe::Matcher::new(&existing);
        let my_labels: BTreeMap<String, Label> = self.store.all::<Label>()?.into_iter().map(|l| (l.id.clone(), l)).collect();
        let mut their_labels: BTreeMap<String, String> = target.store.all::<Label>()?.into_iter().map(|l| (l.name.to_lowercase(), l.id)).collect();
        let mut new_labels: Vec<Label> = vec![];
        let mut refs_out: Vec<Reference> = vec![];
        for r in self.references(ids)? {
            if r.trashed_at.is_some() {
                continue;
            }
            if existing_ids.contains(&r.id) {
                rep.skipped.push((r.title().to_string(), "already in that library (copied before)".into()));
                continue;
            }
            if let Some((m, score, _, _)) = matcher.best(&r, 0.95) {
                rep.skipped.push((r.title().to_string(), format!("that library already has “{}” ({:.0}% match)", m.title(), score * 100.0)));
                continue;
            }
            let mut c = r.clone();
            c.overflow.insert("library:copied_from".into(), here_name.clone());
            c.labels = r
                .labels
                .iter()
                .filter_map(|lid| my_labels.get(lid))
                .map(|l| {
                    their_labels.get(&l.name.to_lowercase()).cloned().unwrap_or_else(|| {
                        let nl = Label { id: crate::util::new_id(), name: l.name.clone(), color: l.color.clone() };
                        their_labels.insert(l.name.to_lowercase(), nl.id.clone());
                        new_labels.push(nl.clone());
                        nl.id
                    })
                })
                .collect();
            refs_out.push(c);
        }
        let copied_ids: BTreeSet<String> = refs_out.iter().map(|r| r.id.clone()).collect();
        // Files, marks and page text.
        let mut atts_out: Vec<Attachment> = vec![];
        let mut anns_out: Vec<Annotation> = vec![];
        let mut pages: Vec<(String, Vec<String>)> = vec![];
        if with_files {
            for a in self.store.all::<Attachment>()?.into_iter().filter(|a| copied_ids.contains(&a.ref_id)) {
                let src = self.attachment_path(&a);
                let mut na = a.clone();
                if a.mode == "managed" {
                    if !src.exists() {
                        rep.skipped.push((a.original_name.clone(), "attached file is missing; the source was copied without it".into()));
                        continue;
                    }
                    na.path = target.copy_into_store(&src, &a.hash, &a.original_name)?;
                } else {
                    na.path = target.link_path(&src);
                }
                rep.files += 1;
                pages.push((a.id.clone(), self.page_text(&a.id)?));
                atts_out.push(na);
            }
            let att_ids: BTreeSet<String> = atts_out.iter().map(|a| a.id.clone()).collect();
            for an in self.store.all::<Annotation>()?.into_iter().filter(|x| att_ids.contains(&x.attachment_id)) {
                let mut m = an.clone();
                if !m.project_id.is_empty() {
                    m.project_id.clear();
                    rep.marks_made_library_wide += 1;
                }
                anns_out.push(m);
            }
        }
        let notes_out: Vec<Note> = self
            .store
            .all::<Note>()?
            .into_iter()
            .filter(|n| n.project_id.is_empty() && !n.ref_ids.is_empty() && n.ref_ids.iter().all(|r| copied_ids.contains(r)))
            .collect();
        rep.copied = refs_out.len();
        rep.marks = anns_out.len();
        rep.notes = notes_out.len();
        rep.labels_created = new_labels.len();
        if rep.copied == 0 {
            return Ok(rep);
        }
        let label = format!("Copied {} source{} from {}", rep.copied, if rep.copied == 1 { "" } else { "s" }, if here_name.is_empty() { "another library" } else { &here_name });
        target.write(&label, |tx| {
            for l in &new_labels {
                tx.put(l)?;
            }
            for r in &refs_out {
                tx.put(r)?;
            }
            for a in &atts_out {
                tx.put(a)?;
            }
            for a in &anns_out {
                tx.put(a)?;
            }
            for n in &notes_out {
                tx.put(n)?;
            }
            Ok(())
        })?;
        for (id, p) in pages {
            if !p.is_empty() {
                target.set_page_text(&id, &p)?;
            }
        }
        if move_sources {
            let ids: Vec<String> = copied_ids.into_iter().collect();
            rep.moved_to_trash = self.trash(&ids)?;
        }
        Ok(rep)
    }
}
