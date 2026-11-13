//! Attachments (immutable versions identified by hash) and annotations with durable anchors.

use super::Library;
use crate::error::{Error, Result};
use crate::model::*;
use crate::util::{new_id, now, sha256_file};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct FileCheck {
    pub attachment_id: String,
    pub ref_id: String,
    pub path: String,
    /// ok, missing, changed, placeholder
    pub state: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReanchorResult {
    pub matched: usize,
    pub uncertain: Vec<Annotation>,
    pub unmatched: Vec<Annotation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExternalImportItem {
    pub external: crate::pdf::ExternalAnnotation,
    /// Text recovered under the mark by the reader (may be empty).
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalPreview {
    pub additions: Vec<crate::pdf::ExternalAnnotation>,
    /// Same identity as an existing annotation with different content.
    pub conflicts: Vec<(crate::pdf::ExternalAnnotation, Annotation)>,
    pub unchanged: usize,
    pub unsupported: Vec<String>,
    pub version_matches: bool,
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("pdf") => "application/pdf",
        Some("epub") => "application/epub+zip",
        Some("html" | "htm") => "text/html",
        Some("txt") => "text/plain",
        Some("md") => "text/markdown",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("csv") => "text/csv",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

fn safe_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if "\\/:*?\"<>|".contains(c) || (c as u32) < 32 { '_' } else { c }).collect();
    let t = t.trim().trim_end_matches('.').to_string();
    let t: String = t.chars().take(120).collect();
    if t.is_empty() { "file".into() } else { t }
}

impl Library {
    pub fn attachment_path(&self, a: &Attachment) -> PathBuf {
        if a.mode == "managed" {
            self.files_dir().join(&a.path)
        } else {
            PathBuf::from(&a.path)
        }
    }

    pub fn attachments(&self, ref_id: &str) -> Result<Vec<Attachment>> {
        let mut v: Vec<Attachment> = self.store.by("ref_id", ref_id)?;
        v.sort_by(|a, b| a.superseded.cmp(&b.superseded).then(b.created.cmp(&a.created)));
        Ok(v)
    }

    /// Attach a file. Managed files are copied into the library (the original stays where it
    /// is); linked files are referenced in place. The file is hashed and, for PDFs, inspected.
    pub fn attach_file(&mut self, ref_id: &str, src: &Path, mode: &str, role: &str) -> Result<Attachment> {
        self.store.must::<Reference>(ref_id)?;
        if !src.exists() {
            return Err(Error::user(format!("The file {} was not found.", src.display())));
        }
        let hash = sha256_file(src)?;
        let size = std::fs::metadata(src)?.len();
        let original_name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
        let mime = mime_for(src).to_string();
        let path = if mode == "linked" {
            src.to_string_lossy().to_string()
        } else {
            let rel = format!("{}/{}/{}", &hash[..2], &hash[..16], safe_name(&original_name));
            let dest = self.files_dir().join(&rel);
            if !dest.exists() {
                std::fs::create_dir_all(dest.parent().unwrap())?;
                let tmp = dest.with_extension("part");
                std::fs::copy(src, &tmp)?;
                if sha256_file(&tmp)? != hash {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(Error::user("The copied file did not match the original. Nothing was attached; try again."));
                }
                std::fs::rename(&tmp, &dest)?;
            }
            rel
        };
        let mut a = Attachment {
            id: new_id(),
            ref_id: ref_id.into(),
            hash,
            path,
            mode: if mode == "linked" { "linked".into() } else { "managed".into() },
            mime: mime.clone(),
            role: if role.is_empty() { "full-text".into() } else { role.into() },
            original_name,
            size,
            state: "ok".into(),
            created: now(),
            ..Default::default()
        };
        if mime == "application/pdf" {
            let info = crate::pdf::inspect(src);
            a.page_count = info.page_count;
            a.page_labels = info.page_labels;
            a.state = info.state;
        }
        let aa = a.clone();
        self.write("Attach file", |tx| tx.put(&aa))?;
        Ok(a)
    }

    /// Replace a file with a new version. The old version is kept; annotations are copied to
    /// the new version and re-anchored by text. Uncertain and unmatched anchors go to review.
    pub fn replace_attachment(&mut self, attachment_id: &str, src: &Path, new_pages: &[String]) -> Result<(Attachment, ReanchorResult)> {
        let old = self.store.must::<Attachment>(attachment_id)?;
        let mut new = self.attach_file(&old.ref_id, src, &old.mode, &old.role)?;
        if new.hash == old.hash {
            return Err(Error::user("The new file is identical to the current version."));
        }
        new.replaces = old.id.clone();
        new.citation_page_offset = old.citation_page_offset;
        let anns: Vec<Annotation> = self.store.by("attachment_id", &old.id)?;
        let mut matched = 0;
        let mut uncertain = Vec::new();
        let mut unmatched = Vec::new();
        let mut copies = Vec::new();
        for a in &anns {
            let mut c = a.clone();
            c.id = new_id();
            c.attachment_id = new.id.clone();
            c.version_hash = new.hash.clone();
            c.links.push(a.id.clone());
            if a.text.trim().is_empty() {
                // Geometry-only marks keep their page; flag for review if the page count changed.
                if (a.page_index as usize) < new_pages.len() && new.page_count == old.page_count {
                    c.anchor_state = "matched".into();
                    matched += 1;
                } else {
                    c.anchor_state = "uncertain".into();
                    uncertain.push(c.clone());
                }
            } else {
                let needle = crate::util::fold(&a.text).split_whitespace().collect::<Vec<_>>().join(" ");
                let hits: Vec<usize> = new_pages.iter().enumerate().filter(|(_, p)| crate::util::fold(p).split_whitespace().collect::<Vec<_>>().join(" ").contains(&needle)).map(|(i, _)| i).collect();
                match hits.as_slice() {
                    [one] => {
                        c.anchor_state = if *one as u32 == a.page_index { "matched".into() } else { "uncertain".into() };
                        if *one as u32 != a.page_index {
                            c.page_index = *one as u32;
                            c.locator_confirmed = false;
                            uncertain.push(c.clone());
                        } else {
                            matched += 1;
                        }
                    }
                    [] => {
                        c.anchor_state = "unmatched".into();
                        unmatched.push(c.clone());
                    }
                    _ => {
                        // Several candidates: keep the nearest page but ask for review.
                        let nearest = *hits.iter().min_by_key(|i| (**i as i64 - a.page_index as i64).abs()).unwrap();
                        c.page_index = nearest as u32;
                        c.anchor_state = "uncertain".into();
                        uncertain.push(c.clone());
                    }
                }
            }
            copies.push(c);
        }
        let mut old2 = old.clone();
        old2.superseded = true;
        let new2 = new.clone();
        self.write("Replace file version", |tx| {
            tx.put(&old2)?;
            tx.put(&new2)?;
            for c in &copies {
                tx.put(c)?;
            }
            Ok(())
        })?;
        if !new_pages.is_empty() {
            self.set_page_text(&new.id, new_pages)?;
        }
        Ok((new, ReanchorResult { matched, uncertain, unmatched }))
    }

    /// Confirm or move a re-anchored annotation after review.
    pub fn resolve_anchor(&mut self, annotation_id: &str, page_index: Option<u32>, keep: bool) -> Result<()> {
        self.write("Review annotation anchor", |tx| {
            let mut a = tx.must::<Annotation>(annotation_id)?;
            if !keep {
                return tx.delete::<Annotation>(annotation_id);
            }
            if let Some(p) = page_index {
                a.page_index = p;
            }
            a.anchor_state = "matched".into();
            a.modified = now();
            tx.put(&a)
        })
    }

    /// Store extracted page text (from the reader) and index it for full-text search.
    pub fn set_page_text(&mut self, attachment_id: &str, pages: &[String]) -> Result<()> {
        crate::search::index_pages(&self.store.conn, attachment_id, pages)?;
        if let Some(mut a) = self.store.get::<Attachment>(attachment_id)? {
            if !a.indexed {
                a.indexed = true;
                let s = crate::util::canonical_json(&serde_json::to_value(&a)?);
                self.store.conn.execute("UPDATE attachments SET data=?1 WHERE id=?2", rusqlite::params![s, attachment_id])?;
            }
        }
        Ok(())
    }

    pub fn page_text(&self, attachment_id: &str) -> Result<Vec<String>> {
        let mut st = self.store.conn.prepare("SELECT text FROM page_text WHERE attachment_id=?1 ORDER BY page")?;
        let v: Vec<String> = st.query_map([attachment_id], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
        Ok(v)
    }

    pub fn set_index_excluded(&mut self, attachment_id: &str, excluded: bool) -> Result<()> {
        self.write(if excluded { "Exclude from full-text index" } else { "Include in full-text index" }, |tx| {
            let mut a = tx.must::<Attachment>(attachment_id)?;
            a.index_excluded = excluded;
            tx.put(&a)
        })?;
        if excluded {
            self.store.conn.execute("DELETE FROM fts WHERE entity='page' AND entity_id=?1", [attachment_id])?;
        }
        Ok(())
    }

    /// Confirm the mapping from PDF page to printed page for citation locators.
    pub fn set_page_offset(&mut self, attachment_id: &str, offset: Option<i64>) -> Result<()> {
        self.write("Set citation page mapping", |tx| {
            let mut a = tx.must::<Attachment>(attachment_id)?;
            a.citation_page_offset = offset;
            tx.put(&a)
        })
    }

    /// Check every attachment: present, unchanged (hash), or a cloud placeholder.
    pub fn check_files(&self, rehash: bool) -> Result<Vec<FileCheck>> {
        let mut out = Vec::new();
        for a in self.store.all::<Attachment>()? {
            let p = self.attachment_path(&a);
            let (state, message) = if !p.exists() {
                ("missing", "The file is missing. Relink it, or attach it again.".to_string())
            } else if std::fs::metadata(&p).map(|m| m.len() == 0 && a.size > 0).unwrap_or(false) {
                ("placeholder", "The file has not been downloaded from cloud storage. Make it available offline.".to_string())
            } else if rehash && sha256_file(&p).map(|h| h != a.hash).unwrap_or(true) {
                ("changed", "The file changed outside the program. Attach it as a new version to keep annotations anchored.".to_string())
            } else {
                ("ok", String::new())
            };
            out.push(FileCheck { attachment_id: a.id.clone(), ref_id: a.ref_id.clone(), path: p.to_string_lossy().to_string(), state: state.into(), message });
        }
        Ok(out)
    }

    /// Relink one attachment to a moved file; the hash must match.
    pub fn relink(&mut self, attachment_id: &str, new_path: &Path) -> Result<()> {
        let a = self.store.must::<Attachment>(attachment_id)?;
        let h = sha256_file(new_path)?;
        if h != a.hash {
            return Err(Error::user("That file is different from the one attached (its contents do not match). Attach it as a new version instead."));
        }
        let p = new_path.to_string_lossy().to_string();
        if a.mode == "managed" {
            // Put the found copy back into the managed store; the record stays the same.
            let dest = self.files_dir().join(&a.path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(new_path, &dest)?;
        }
        self.write("Relink file", |tx| {
            let mut a = tx.must::<Attachment>(attachment_id)?;
            if a.mode != "managed" {
                a.path = p.clone();
            }
            a.state = "ok".into();
            tx.put(&a)
        })
    }

    /// Relink all linked files after a folder move (drive letter or path prefix change).
    /// Only files whose hash matches are relinked; returns (relinked, still missing).
    pub fn relink_folder(&mut self, old_prefix: &str, new_prefix: &str) -> Result<(usize, usize)> {
        let all: Vec<Attachment> = self.store.all()?;
        let mut updates = Vec::new();
        let mut missing = 0;
        for a in all.into_iter().filter(|a| a.mode == "linked") {
            let lower = a.path.to_lowercase().replace('/', "\\");
            let old = old_prefix.to_lowercase().replace('/', "\\");
            if !lower.starts_with(&old) {
                continue;
            }
            let rest = &a.path[old_prefix.len()..];
            let candidate = PathBuf::from(format!("{}{}", new_prefix.trim_end_matches(['\\', '/']), if rest.starts_with(['\\', '/']) { rest.to_string() } else { format!("\\{rest}") }));
            if candidate.exists() && sha256_file(&candidate).map(|h| h == a.hash).unwrap_or(false) {
                let mut na = a.clone();
                na.path = candidate.to_string_lossy().to_string();
                updates.push(na);
            } else {
                missing += 1;
            }
        }
        let n = updates.len();
        if n > 0 {
            self.write(&format!("Relink {n} files"), |tx| {
                for u in &updates {
                    tx.put(u)?;
                }
                Ok(())
            })?;
        }
        Ok((n, missing))
    }

    /// Rename managed files by a pattern ([auth], [year], [shorttitle], [citekey]); old names are kept.
    pub fn rename_files(&mut self, ids: &[String], pattern: &str, apply: bool) -> Result<Vec<(String, String, String)>> {
        let mut plan = Vec::new();
        for id in ids {
            let a = self.store.must::<Attachment>(id)?;
            if a.mode != "managed" {
                continue;
            }
            let r = self.reference(&a.ref_id)?;
            let ext = Path::new(&a.original_name).extension().and_then(|e| e.to_str()).unwrap_or("pdf").to_string();
            let base = crate::citekey::generate(&r, &pattern.replace("[citekey]", &r.citekey));
            let new_name = safe_name(&format!("{base}.{ext}"));
            let old_rel = a.path.clone();
            let new_rel = format!("{}/{}", Path::new(&old_rel).parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default(), new_name);
            if new_rel != old_rel {
                plan.push((a.id.clone(), old_rel, new_rel));
            }
        }
        if apply {
            for (id, old_rel, new_rel) in &plan {
                std::fs::rename(self.files_dir().join(old_rel), self.files_dir().join(new_rel))?;
                let mut a = self.store.must::<Attachment>(id)?;
                a.old_names.push(a.path.clone());
                a.path = new_rel.clone();
                let aa = a.clone();
                self.write("Rename file", |tx| tx.put(&aa))?;
            }
        }
        Ok(plan)
    }

    /// Bundle attachments into a zip with names from a pattern.
    pub fn export_attachments(&self, ids: &[String], pattern: &str, dest: &Path) -> Result<usize> {
        use std::io::Write;
        let f = std::fs::File::create(dest)?;
        let mut z = zip::ZipWriter::new(f);
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let mut n = 0;
        let mut used = std::collections::BTreeSet::new();
        for id in ids {
            let a = self.store.must::<Attachment>(id)?;
            let r = self.reference(&a.ref_id)?;
            let p = self.attachment_path(&a);
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let ext = Path::new(&a.original_name).extension().and_then(|e| e.to_str()).unwrap_or("bin");
            let mut name = safe_name(&format!("{}.{ext}", crate::citekey::generate(&r, pattern)));
            let mut i = 2;
            while !used.insert(name.clone()) {
                name = safe_name(&format!("{}-{i}.{ext}", crate::citekey::generate(&r, pattern)));
                i += 1;
            }
            z.start_file(name, opt)?;
            z.write_all(&bytes)?;
            n += 1;
        }
        z.finish()?;
        Ok(n)
    }

    // ---- annotations --------------------------------------------------------------------------

    pub fn annotations(&self, attachment_id: &str, project_id: &str) -> Result<Vec<Annotation>> {
        let mut v: Vec<Annotation> = self.store.by::<Annotation>("attachment_id", attachment_id)?.into_iter().filter(|a| a.project_id.is_empty() || a.project_id == project_id).collect();
        v.sort_by(|a, b| a.page_index.cmp(&b.page_index).then(a.rects.first().map(|r| r.y).partial_cmp(&b.rects.first().map(|r| r.y)).unwrap_or(std::cmp::Ordering::Equal)));
        Ok(v)
    }

    /// Suggested citation locator for a physical page, when a page mapping is confirmed or the
    /// file's own page label is numeric. The locator stays unconfirmed until the person reviews it.
    pub fn suggest_locator(&self, a: &Attachment, page_index: u32) -> (String, bool) {
        if let Some(off) = a.citation_page_offset {
            return ((page_index as i64 + 1 + off).to_string(), true);
        }
        if let Some(l) = a.page_labels.get(page_index as usize) {
            if !l.is_empty() && *l != (page_index + 1).to_string() {
                return (l.clone(), false);
            }
        }
        (String::new(), false)
    }

    pub fn save_annotation(&mut self, mut a: Annotation) -> Result<Annotation> {
        let att = self.store.must::<Attachment>(&a.attachment_id)?;
        a.version_hash = att.hash.clone();
        let t = now();
        if a.id.is_empty() {
            a.id = new_id();
            a.created = t.clone();
            if a.external_key.is_empty() {
                a.external_key = "prism".into();
            }
            if a.locator.is_empty() {
                let (loc, confirmed) = self.suggest_locator(&att, a.page_index);
                a.locator = loc;
                a.locator_confirmed = confirmed;
            }
            if a.page_label.is_empty() {
                a.page_label = att.page_labels.get(a.page_index as usize).cloned().unwrap_or_else(|| (a.page_index + 1).to_string());
            }
        }
        if a.anchor_state.is_empty() {
            a.anchor_state = "matched".into();
        }
        a.modified = t;
        let aa = a.clone();
        let label = format!("{} on page {}", match a.kind.as_str() {
            "highlight" => "Highlight",
            "underline" => "Underline",
            "strikeout" => "Strikeout",
            "note" => "Sticky note",
            "freetext" => "Text box",
            "ink" => "Drawing",
            "rect" | "ellipse" => "Shape",
            "region" => "Area",
            _ => "Mark",
        }, a.page_index + 1);
        self.write(&label, |tx| tx.put(&aa))?;
        Ok(a)
    }

    pub fn delete_annotation(&mut self, id: &str) -> Result<()> {
        self.write("Delete annotation", |tx| tx.delete::<Annotation>(id))
    }

    /// Export the selected annotation layer into a PDF copy (and a sidecar next to it).
    pub fn export_annotated_pdf(&self, attachment_id: &str, project_id: &str, dest: &Path, flatten: bool) -> Result<usize> {
        let a = self.store.must::<Attachment>(attachment_id)?;
        let src = self.attachment_path(&a);
        let anns = self.annotations(attachment_id, project_id)?;
        let n = crate::pdf::export_annotated(&src, dest, &anns, flatten)?;
        if !flatten {
            let side = dest.with_extension("prism-annotations.json");
            crate::util::write_atomic(&side, serde_json::to_string_pretty(&crate::pdf::sidecar(&a.hash, &anns))?.as_bytes())?;
        }
        Ok(n)
    }

    /// Preview annotations in a PDF edited elsewhere: additions, conflicts and duplicates.
    pub fn preview_external_annotations(&self, attachment_id: &str, pdf: &Path) -> Result<ExternalPreview> {
        let att = self.store.must::<Attachment>(attachment_id)?;
        let (ext, unsupported) = crate::pdf::read_annotations(pdf)?;
        let existing: Vec<Annotation> = self.store.by("attachment_id", attachment_id)?;
        let mut additions = Vec::new();
        let mut conflicts = Vec::new();
        let mut unchanged = 0;
        for e in ext {
            // Annotations exported from here carry our id as their name.
            let found = existing.iter().find(|a| (!e.name.is_empty() && a.id == e.name) || a.external_key == format!("external:{}", e.identity));
            match found {
                Some(a) => {
                    let same = a.comment.trim() == e.comment.trim() && a.page_index == e.page_index && (e.kind != "freetext" || a.text == e.text);
                    if same {
                        unchanged += 1;
                    } else {
                        conflicts.push((e, a.clone()));
                    }
                }
                None => additions.push(e),
            }
        }
        let version_matches = crate::util::sha256_file(pdf).map(|h| h == att.hash).unwrap_or(false);
        Ok(ExternalPreview { additions, conflicts, unchanged, unsupported, version_matches })
    }

    /// Commit reviewed external annotations. Conflicts update the comment as a new revision of
    /// the existing annotation only when `accept_conflicts`; project notes are never deleted.
    pub fn import_external_annotations(&mut self, attachment_id: &str, project_id: &str, items: &[ExternalImportItem], accept_conflicts: bool) -> Result<usize> {
        let att = self.store.must::<Attachment>(attachment_id)?;
        let existing: Vec<Annotation> = self.store.by("attachment_id", attachment_id)?;
        let t = now();
        let mut puts = Vec::new();
        for it in items {
            let e = &it.external;
            let key = format!("external:{}", e.identity);
            let found = existing.iter().find(|a| (!e.name.is_empty() && a.id == e.name) || a.external_key == key);
            match found {
                Some(a) if accept_conflicts => {
                    let mut a2 = a.clone();
                    a2.comment = e.comment.clone();
                    if e.kind == "freetext" {
                        a2.text = e.text.clone();
                    }
                    a2.modified = t.clone();
                    puts.push(a2);
                }
                Some(_) => {}
                None => {
                    let (loc, confirmed) = self.suggest_locator(&att, e.page_index);
                    puts.push(Annotation {
                        id: new_id(),
                        attachment_id: attachment_id.into(),
                        version_hash: att.hash.clone(),
                        project_id: project_id.into(),
                        kind: e.kind.clone(),
                        meaning: String::new(),
                        color: e.color.clone(),
                        page_index: e.page_index,
                        page_label: att.page_labels.get(e.page_index as usize).cloned().unwrap_or_else(|| (e.page_index + 1).to_string()),
                        locator: loc,
                        locator_confirmed: confirmed,
                        rects: e.rects.clone(),
                        ink: e.ink.clone(),
                        page_width: e.page_width,
                        page_height: e.page_height,
                        rotation: 0,
                        text: if e.kind == "freetext" { e.text.clone() } else { it.text.clone() },
                        prefix: String::new(),
                        suffix: String::new(),
                        comment: e.comment.clone(),
                        tags: vec![],
                        links: vec![],
                        anchor_state: "matched".into(),
                        external_key: key,
                        author: e.author.clone(),
                        created: t.clone(),
                        modified: t.clone(),
                    });
                }
            }
        }
        let n = puts.len();
        self.write(&format!("Import {n} annotations"), |tx| {
            for p in &puts {
                tx.put(p)?;
            }
            Ok(())
        })?;
        Ok(n)
    }
}
