//! Finding files that were moved or renamed outside the program (by content hash), the storage
//! report (space used, the same file attached more than once, files no source uses) and clean-up.

use super::Library;
use crate::error::{Error, Result};
use crate::model::*;
use crate::util::sha256_file;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A missing attachment found again under another name or folder.
#[derive(Debug, Clone, Serialize)]
pub struct FoundFile {
    pub attachment_id: String,
    pub ref_id: String,
    pub title: String,
    pub old_path: String,
    pub new_path: String,
    /// renamed (same folder), moved (another folder)
    pub how: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct FindReport {
    pub found: Vec<FoundFile>,
    /// Missing attachments not found in the folders searched.
    pub still_missing: usize,
    pub files_looked_at: usize,
    /// The search stopped at the file limit.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateUse {
    pub attachment_id: String,
    pub ref_id: String,
    pub title: String,
    pub name: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateFile {
    pub hash: String,
    pub size: u64,
    pub uses: Vec<DuplicateUse>,
    /// All uses belong to one source (the file is attached to it more than once).
    pub same_source: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnusedFile {
    /// Relative to the library's files folder.
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StorageReport {
    pub managed_files: usize,
    pub managed_bytes: u64,
    pub linked_files: usize,
    pub linked_bytes: u64,
    /// Earlier versions kept after a replacement (included above).
    pub earlier_versions: usize,
    pub earlier_bytes: u64,
    pub duplicates: Vec<DuplicateFile>,
    /// Files in the library's files folder that no attachment uses.
    pub unused: Vec<UnusedFile>,
    pub unused_bytes: u64,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, limit: usize, seen: &mut HashSet<PathBuf>) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return true };
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if !seen.insert(canon) {
        return true;
    }
    for e in rd.flatten() {
        if out.len() >= limit {
            return false;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let p = e.path();
        if ft.is_dir() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || name == "node_modules" || name == "$recycle.bin" {
                continue;
            }
            if !walk(&p, out, limit, seen) {
                return false;
            }
        } else if ft.is_file() {
            out.push(p);
        }
    }
    true
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.parent(), b.parent()) {
        (Some(x), Some(y)) => x.to_string_lossy().replace('/', "\\").eq_ignore_ascii_case(&y.to_string_lossy().replace('/', "\\")),
        _ => false,
    }
}

impl Library {
    /// Look for missing attachments by content in `folders`, plus each missing file's old folder
    /// and the linked-files base folder. Only files whose size matches a missing attachment are
    /// hashed. At most `limit` files are looked at.
    pub fn find_moved_files(&self, folders: &[PathBuf], limit: usize) -> Result<FindReport> {
        let atts: Vec<Attachment> = self.store.all()?;
        let missing: Vec<&Attachment> = atts.iter().filter(|a| !self.attachment_path(a).exists()).collect();
        let mut rep = FindReport::default();
        if missing.is_empty() {
            return Ok(rep);
        }
        let mut dirs: Vec<PathBuf> = folders.to_vec();
        for a in &missing {
            if let Some(p) = self.attachment_path(a).parent() {
                if p.is_dir() {
                    dirs.push(p.to_path_buf());
                }
            }
        }
        if let Some(b) = self.linked_base() {
            dirs.push(b);
        }
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        for d in &dirs {
            if !walk(d, &mut files, limit, &mut seen) {
                rep.truncated = true;
                break;
            }
        }
        rep.files_looked_at = files.len();
        let sizes: HashSet<u64> = missing.iter().map(|a| a.size).collect();
        let mut by_hash: HashMap<String, PathBuf> = HashMap::new();
        let total = files.len();
        let mut checked: HashSet<PathBuf> = HashSet::new();
        for (i, f) in files.iter().enumerate() {
            if i % 200 == 0 {
                self.report_progress("Looking for moved files", i, total);
            }
            let Ok(m) = std::fs::metadata(f) else { continue };
            if !sizes.contains(&m.len()) || !checked.insert(f.clone()) {
                continue;
            }
            if let Ok(h) = sha256_file(f) {
                by_hash.entry(h).or_insert_with(|| f.clone());
            }
        }
        for a in missing {
            let old = self.attachment_path(a);
            match by_hash.get(&a.hash) {
                Some(p) => rep.found.push(FoundFile {
                    attachment_id: a.id.clone(),
                    ref_id: a.ref_id.clone(),
                    title: self.store.get::<Reference>(&a.ref_id)?.map(|r| r.title().to_string()).unwrap_or_default(),
                    old_path: old.to_string_lossy().to_string(),
                    new_path: p.to_string_lossy().to_string(),
                    how: if same_dir(&old, p) { "renamed".into() } else { "moved".into() },
                }),
                None => rep.still_missing += 1,
            }
        }
        Ok(rep)
    }

    /// Relink several attachments at once, as one undo step. Each file's content must match.
    /// Returns how many were relinked.
    pub fn relink_many(&mut self, pairs: &[(String, PathBuf)]) -> Result<usize> {
        let mut updates = Vec::new();
        for (id, p) in pairs {
            let a = self.store.must::<Attachment>(id)?;
            if sha256_file(p)? != a.hash {
                return Err(Error::user(format!("{} is different from the attached file; nothing was relinked.", p.display())));
            }
            let mut na = a.clone();
            if a.mode == "managed" {
                // A managed file found elsewhere is copied back into the library.
                let dest = self.inside_files(&a.path)?;
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(p, &dest)?;
            } else {
                let old_name = self.attachment_path(&a).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let new_name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if !old_name.is_empty() && old_name != new_name && !na.old_names.contains(&old_name) {
                    na.old_names.push(old_name);
                }
                na.path = self.link_path(p);
            }
            na.state = "ok".into();
            updates.push(na);
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
        Ok(n)
    }

    pub fn storage_report(&self) -> Result<StorageReport> {
        let atts: Vec<Attachment> = self.store.all()?;
        let mut rep = StorageReport::default();
        let mut used: HashSet<String> = HashSet::new();
        let mut by_hash: BTreeMap<String, Vec<&Attachment>> = BTreeMap::new();
        let mut counted: HashSet<String> = HashSet::new();
        for a in &atts {
            if a.mode == "managed" {
                used.insert(a.path.replace('\\', "/").to_lowercase());
                // Managed files are stored once per content; count each stored file once.
                if counted.insert(a.path.clone()) {
                    rep.managed_files += 1;
                    rep.managed_bytes += a.size;
                }
            } else {
                rep.linked_files += 1;
                rep.linked_bytes += a.size;
            }
            if a.superseded {
                rep.earlier_versions += 1;
                rep.earlier_bytes += a.size;
            } else {
                by_hash.entry(a.hash.clone()).or_default().push(a);
            }
        }
        for (hash, list) in by_hash.into_iter().filter(|(_, l)| l.len() > 1) {
            let mut uses = vec![];
            for a in &list {
                let title = self.store.get::<Reference>(&a.ref_id)?.map(|r| if r.trashed_at.is_some() { format!("{} (in the trash)", r.title()) } else { r.title().to_string() }).unwrap_or_default();
                uses.push(DuplicateUse { attachment_id: a.id.clone(), ref_id: a.ref_id.clone(), title, name: a.original_name.clone(), mode: a.mode.clone() });
            }
            let same_source = list.iter().all(|a| a.ref_id == list[0].ref_id);
            rep.duplicates.push(DuplicateFile { hash, size: list[0].size, uses, same_source });
        }
        let root = self.files_dir();
        let mut files = Vec::new();
        walk(&root, &mut files, 1_000_000, &mut HashSet::new());
        for f in files {
            let Ok(rel) = f.strip_prefix(&root) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if rel.ends_with(".part") || used.contains(&rel.to_lowercase()) {
                continue;
            }
            let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
            rep.unused_bytes += size;
            rep.unused.push(UnusedFile { path: rel, size });
        }
        Ok(rep)
    }

    /// Permanently delete files in the library's files folder that no attachment uses. Paths are
    /// checked again: anything outside the files folder or still in use is skipped.
    pub fn delete_unused_files(&mut self, paths: &[String]) -> Result<usize> {
        let rep = self.storage_report()?;
        let unused: HashSet<&str> = rep.unused.iter().map(|u| u.path.as_str()).collect();
        let root = self.files_dir();
        let mut n = 0;
        for p in paths {
            if !unused.contains(p.as_str()) || p.contains("..") {
                continue;
            }
            let full = root.join(p);
            if std::fs::remove_file(&full).is_ok() {
                n += 1;
                // Remove emptied hash folders.
                let mut d = full.parent().map(|x| x.to_path_buf());
                while let Some(dir) = d {
                    if dir == root || std::fs::remove_dir(&dir).is_err() {
                        break;
                    }
                    d = dir.parent().map(|x| x.to_path_buf());
                }
            }
        }
        Ok(n)
    }

    /// Remove extra attachments of the same file to the same source, keeping the oldest.
    /// Annotations on a removed copy move to the kept one (same content, so anchors hold).
    pub fn remove_duplicate_attachments(&mut self, hash: &str) -> Result<usize> {
        let mut list: Vec<Attachment> = self.store.all::<Attachment>()?.into_iter().filter(|a| a.hash == hash && !a.superseded).collect();
        list.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
        let mut removed = vec![];
        let mut kept: HashMap<String, String> = HashMap::new();
        for a in &list {
            match kept.get(&a.ref_id) {
                Some(k) => removed.push((a.id.clone(), k.clone())),
                None => {
                    kept.insert(a.ref_id.clone(), a.id.clone());
                }
            }
        }
        if removed.is_empty() {
            return Ok(0);
        }
        let anns: Vec<Annotation> = self.store.all()?;
        let n = removed.len();
        self.write(&format!("Remove {n} duplicate attachments"), |tx| {
            for (gone, keep) in &removed {
                for an in anns.iter().filter(|x| &x.attachment_id == gone) {
                    let mut m = an.clone();
                    m.attachment_id = keep.clone();
                    tx.put(&m)?;
                }
                tx.delete::<Attachment>(gone)?;
            }
            Ok(())
        })?;
        Ok(n)
    }
}
