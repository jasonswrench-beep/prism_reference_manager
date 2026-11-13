//! Bringing PDF files in: chosen files, a whole folder, or a watched folder.
//!
//! Files are inspected and their identifiers looked up without holding the program's state lock,
//! so the window stays responsive. Everything is staged for review; nothing joins the library
//! until the person commits the batch. Exact copies of files already in the library are flagged.

use crate::{AppState, CallError, Shared};
use prm_core::model::{Attachment, ImportBatch, Reference};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tauri::Emitter;

/// Most PDFs taken from one folder in one batch.
pub const MAX_FILES: usize = 2000;

pub struct Found {
    pub path: PathBuf,
    /// Folder path relative to the chosen folder's parent, so it starts with the folder's own name.
    pub folder: String,
    pub info: prm_core::pdf::PdfInfo,
    pub hash: String,
}

pub struct Looked {
    pub candidate: Option<Reference>,
    pub notes: Vec<String>,
}

/// Every PDF below `root`, depth-first, skipping hidden folders. Returns (files, truncated).
pub fn walk(root: &Path) -> (Vec<PathBuf>, bool) {
    let mut out = vec![];
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries.into_iter().rev() {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with('.') || name.starts_with("~$") {
                continue;
            }
            if p.is_dir() {
                if depth < 12 {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().map(|e| e.eq_ignore_ascii_case("pdf")).unwrap_or(false) {
                if out.len() >= MAX_FILES {
                    return (out, true);
                }
                out.push(p);
            }
        }
    }
    out.sort();
    (out, false)
}

/// Inspect files (no lock). `root` makes each file's folder path relative for collection mirroring.
pub fn prepare(paths: &[PathBuf], root: Option<&Path>, progress: &dyn Fn(usize, usize)) -> Vec<Found> {
    let base = root.and_then(|r| r.parent().map(|p| p.to_path_buf()));
    paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            progress(i, paths.len());
            let folder = match (&base, p.parent()) {
                (Some(b), Some(parent)) => parent.strip_prefix(b).map(|x| x.to_string_lossy().replace('\\', "/")).unwrap_or_default(),
                _ => String::new(),
            };
            Found { path: p.clone(), folder, info: prm_core::pdf::inspect(p), hash: prm_core::util::sha256_file(p).unwrap_or_default() }
        })
        .collect()
}

/// Identifier lookups (no lock). `doi_ok`/`arxiv_ok` say whether those requests are allowed.
/// Network log lines are collected and written by the caller once it holds the lock.
pub fn lookups(found: &[Found], doi_ok: bool, arxiv_ok: bool, log: &mut Vec<(String, String, String, bool, String)>, progress: &dyn Fn(usize, usize)) -> Vec<Looked> {
    found
        .iter()
        .enumerate()
        .map(|(i, f)| {
            progress(i, found.len());
            let mut notes = vec![];
            let mut candidate = None;
            let mut logf = |k: &str, h: &str, sent: &str, ok: bool, d: &str| log.push((k.into(), h.into(), sent.into(), ok, d.into()));
            let try_kind = if doi_ok && !f.info.dois.is_empty() { Some(("doi", f.info.dois[0].clone())) } else if arxiv_ok && !f.info.arxiv.is_empty() { Some(("arxiv", f.info.arxiv[0].clone())) } else { None };
            if let Some((kind, id)) = try_kind {
                let res = prm_core::lookup::resolve(kind, &id, &mut logf);
                if let Some(c) = res.candidate {
                    let t = c.title().to_lowercase();
                    if !f.info.title.is_empty() && prm_core::strsim_ratio(&t, &f.info.title.to_lowercase()) < 0.6 {
                        notes.push(format!("The {} found in the file resolves to “{}”, which may not be this PDF. Review it.", if kind == "doi" { "DOI" } else { "arXiv identifier" }, c.title()));
                    }
                    candidate = Some(c);
                }
                notes.extend(res.conflicts);
            }
            Looked { candidate, notes }
        })
        .collect()
}

/// Stage the files as one import batch (with the lock held).
pub fn stage(st: &mut AppState, found: &[Found], looked: &[Looked], log: &[(String, String, String, bool, String)], project_id: &str, name: &str, service: &str, mirror: bool) -> Result<ImportBatch, CallError> {
    let lib = st.lib.as_mut().ok_or_else(|| crate::dispatch::user("Open or create a library first."))?;
    for (k, h, sent, ok, d) in log {
        lib.store.log_network(k, h, "PDF import lookup", sent, *ok, d);
    }
    let mut items = vec![];
    let mut provs = vec![];
    for (f, l) in found.iter().zip(looked) {
        let mut r = l.candidate.clone().unwrap_or_else(|| {
            let mut r = Reference { rtype: if f.info.arxiv.is_empty() { "article-journal".into() } else { "preprint".into() }, ..Default::default() };
            let title = if f.info.title.trim().is_empty() { f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() } else { f.info.title.clone() };
            r.fields.insert("title".into(), title);
            for n in prm_core::formats::split_names(&f.info.author) {
                r.creators.push(prm_core::formats::person("author", &n));
            }
            if let Some(d) = f.info.dois.first() {
                r.fields.insert("doi".into(), d.clone());
            }
            if let Some(a) = f.info.arxiv.first() {
                r.fields.insert("arxiv".into(), a.clone());
            }
            r
        });
        r.overflow.insert("pdf:path".into(), f.path.to_string_lossy().to_string());
        r.overflow.insert("pdf:state".into(), f.info.state.clone());
        // Values guessed from the file itself carry low confidence; looked-up ones keep theirs.
        prm_core::provenance::fill(&mut r, "pdf", None, &prm_core::util::now());
        provs.push(r.provenance.clone());
        items.push(prm_core::formats::csljson::to_csl(&r));
    }
    let text = serde_json::to_string(&items).unwrap_or_default();
    let mut b = lib.stage_import(name, &text, "csljson", service, project_id, "", None).map_err(crate::dispatch::err)?;
    let mut seen_hashes: BTreeSet<String> = BTreeSet::new();
    for (i, rec) in b.records.iter_mut().enumerate() {
        let (Some(f), Some(l)) = (found.get(i), looked.get(i)) else { continue };
        if let Some(p) = provs.get(i) {
            rec.record.provenance = p.clone();
        }
        rec.record.overflow.insert("pdf:path".into(), f.path.to_string_lossy().to_string());
        rec.record.overflow.insert("pdf:hash".into(), f.hash.clone());
        if mirror && !f.folder.is_empty() {
            rec.record.overflow.insert("pdf:folder".into(), f.folder.clone());
        }
        // An exact copy of a file already in the library, or twice in this batch.
        let existing: Vec<Attachment> = if f.hash.is_empty() { vec![] } else { lib.store.by("hash", &f.hash).unwrap_or_default() };
        if let Some(a) = existing.iter().find(|a| !a.superseded) {
            let title = lib.store.get::<Reference>(&a.ref_id).ok().flatten().map(|r| r.title().to_string()).unwrap_or_default();
            rec.status = "duplicate".into();
            rec.match_ref_id = a.ref_id.clone();
            rec.match_reason = "Same file".into();
            rec.issues.push(format!("This exact file is already attached to “{title}”."));
            rec.decision = "skip".into();
            continue;
        }
        if !f.hash.is_empty() && !seen_hashes.insert(f.hash.clone()) {
            rec.status = "duplicate".into();
            rec.issues.push("The same file appears more than once in this batch.".into());
            rec.decision = "skip".into();
            continue;
        }
        if !l.notes.is_empty() || l.candidate.is_none() {
            if rec.status == "new" {
                rec.status = "unresolved".into();
            }
            rec.issues.push(if l.notes.is_empty() { "No identifier was found or looked up; metadata comes from the file and needs review.".into() } else { l.notes.join(" ") });
        }
        if !f.info.message.is_empty() && f.info.state != "ok" {
            rec.issues.push(f.info.message.clone());
        }
    }
    lib.save_staged(&b).map_err(crate::dispatch::err)?;
    Ok(b)
}

/// Whether DOI and arXiv lookups are allowed for this project (brief lock).
pub fn lookup_permissions(shared: &Shared, project_id: &str) -> (bool, bool) {
    match shared.lock() {
        Ok(st) => (crate::dispatch::network_allowed(&st, "doi", project_id).is_ok(), crate::dispatch::network_allowed(&st, "arxiv", project_id).is_ok()),
        Err(_) => (false, false),
    }
}

/// The whole pipeline for a list of files, reporting progress to the window.
pub fn import_paths(app: &tauri::AppHandle, shared: &Shared, paths: &[PathBuf], root: Option<&Path>, project_id: &str, name: &str, service: &str, mirror: bool) -> Result<ImportBatch, CallError> {
    let emit = |phase: &str, i: usize, n: usize| {
        let _ = app.emit("import-progress", json!({"phase": phase, "done": i, "total": n}));
    };
    let found = prepare(paths, root, &|i, n| emit("Reading files", i, n));
    let (doi_ok, arxiv_ok) = lookup_permissions(shared, project_id);
    let mut log = vec![];
    let looked = lookups(&found, doi_ok, arxiv_ok, &mut log, &|i, n| emit("Looking up identifiers", i, n));
    emit("Preparing review", found.len(), found.len());
    let mut st = shared.lock().map_err(|_| crate::dispatch::user("The program is busy; try again."))?;
    stage(&mut st, &found, &looked, &log, project_id, name, service, mirror)
}

/// After commit: put sources imported from subfolders into matching collections.
pub fn mirror_folders(lib: &mut prm_core::library::Library, batch: &ImportBatch) -> Result<usize, CallError> {
    use prm_core::model::Collection;
    let mut placed = 0;
    for r in &batch.records {
        let Some(folder) = r.record.overflow.get("pdf:folder") else { continue };
        if r.committed_id.is_empty() {
            continue;
        }
        let mut parent = String::new();
        for seg in folder.split('/').filter(|s| !s.is_empty()) {
            let all: Vec<Collection> = lib.store.all().map_err(crate::dispatch::err)?;
            let existing = all.into_iter().find(|c| c.parent_id == parent && c.name == seg && c.query.is_empty() && c.project_id == batch.destination_project);
            parent = match existing {
                Some(c) => c.id,
                None => lib.save_collection(Collection { name: seg.to_string(), parent_id: parent.clone(), project_id: batch.destination_project.clone(), ..Default::default() }).map_err(crate::dispatch::err)?.id,
            };
        }
        if !parent.is_empty() {
            lib.collect(&parent, &[r.committed_id.clone()], true).map_err(crate::dispatch::err)?;
            placed += 1;
        }
    }
    Ok(placed)
}

/// Look for new PDFs in this library's watched folders and stage them for review.
/// A file is new when it is not attached and not waiting in a staged batch. Files changed in the
/// last 15 seconds are left for the next pass, so downloads in progress are not taken half-written.
pub fn scan_watched(app: &tauri::AppHandle, shared: &Shared) -> Result<Vec<serde_json::Value>, CallError> {
    let (folders, known) = {
        let st = shared.lock().map_err(|_| crate::dispatch::user("busy"))?;
        let Some(lib) = st.lib.as_ref() else { return Ok(vec![]) };
        let root = lib.root.to_string_lossy().to_string();
        let folders: Vec<_> = st.settings.watched_folders.iter().filter(|w| w.library == root).cloned().collect();
        if folders.is_empty() {
            return Ok(vec![]);
        }
        let mut known: BTreeSet<String> = BTreeSet::new();
        for b in lib.store.all::<ImportBatch>().map_err(crate::dispatch::err)?.into_iter().filter(|b| b.state == "staged") {
            for r in b.records {
                if let Some(h) = r.record.overflow.get("pdf:hash") {
                    known.insert(h.clone());
                }
            }
        }
        (folders, known)
    };
    let mut out = vec![];
    for w in folders {
        let (paths, _) = walk(Path::new(&w.path));
        let now = std::time::SystemTime::now();
        let settled: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| std::fs::metadata(p).and_then(|m| m.modified()).map(|t| now.duration_since(t).map(|d| d.as_secs() >= 15).unwrap_or(false)).unwrap_or(false))
            .collect();
        let fresh: Vec<PathBuf> = {
            let st = shared.lock().map_err(|_| crate::dispatch::user("busy"))?;
            let Some(lib) = st.lib.as_ref() else { return Ok(out) };
            settled
                .into_iter()
                .filter(|p| {
                    let Ok(h) = prm_core::util::sha256_file(p) else { return false };
                    !known.contains(&h) && lib.store.by::<Attachment>("hash", &h).map(|v| v.is_empty()).unwrap_or(false)
                })
                .collect()
        };
        if fresh.is_empty() {
            continue;
        }
        let name = format!("Watched folder: {}", Path::new(&w.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        let b = import_paths(app, shared, &fresh, Some(Path::new(&w.path)), &w.project_id, &name, "watch", w.mirror)?;
        let ev = json!({"batch_id": b.id, "count": b.records.len(), "folder": w.path});
        let _ = app.emit("watch", ev.clone());
        out.push(ev);
    }
    Ok(out)
}
