//! Method dispatcher shared by the window and the local API.

use crate::settings;
use crate::{CallError, Shared};
use prm_core::library::*;
use prm_core::model::*;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

pub fn err(e: prm_core::Error) -> CallError {
    let detail = format!("{e:?}");
    CallError { message: e.to_string(), detail }
}

pub fn user(msg: &str) -> CallError {
    CallError { message: msg.into(), detail: String::new() }
}

/// Paths the person chose in a file dialog during this session. Only these may be read by
/// path from the interface.
pub fn picked_paths() -> &'static Mutex<BTreeSet<PathBuf>> {
    static P: std::sync::OnceLock<Mutex<BTreeSet<PathBuf>>> = std::sync::OnceLock::new();
    P.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn remember(p: &Path) {
    picked_paths().lock().unwrap().insert(p.to_path_buf());
}

fn arg<T: DeserializeOwned>(a: &Value, k: &str) -> Result<T, CallError> {
    serde_json::from_value(a.get(k).cloned().unwrap_or(Value::Null)).map_err(|e| CallError { message: format!("A value for “{k}” was missing or invalid."), detail: e.to_string() })
}

fn opt<T: DeserializeOwned + Default>(a: &Value, k: &str) -> T {
    a.get(k).cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

fn s(a: &Value, k: &str) -> String {
    a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn ok<T: serde::Serialize>(v: T) -> Result<Value, CallError> {
    serde_json::to_value(v).map_err(|e| CallError { message: "The result could not be prepared.".into(), detail: e.to_string() })
}

fn filters(kind: &str) -> Vec<(&'static str, Vec<&'static str>)> {
    match kind {
        "pdf" => vec![("PDF documents", vec!["pdf"])],
        "bib" => vec![("Bibliographic files", vec!["ris", "bib", "bibtex", "json", "xml", "csv", "tsv", "txt"]), ("All files", vec!["*"])],
        "zip" => vec![("Archives", vec!["zip"])],
        "any" => vec![("All files", vec!["*"])],
        "docx" => vec![("DOCX document", vec!["docx"])],
        "xlsx" => vec![("Excel workbook", vec!["xlsx"])],
        "rtf" => vec![("Rich Text", vec!["rtf"])],
        "html" => vec![("Web page", vec!["html"])],
        "md" => vec![("Markdown", vec!["md"])],
        "txt" => vec![("Plain text", vec!["txt"])],
        "json" => vec![("JSON", vec!["json"])],
        "ris" => vec![("RIS", vec!["ris"])],
        "bibtex" | "biblatex" => vec![("BibTeX", vec!["bib"])],
        "csljson" => vec![("CSL JSON", vec!["json"])],
        "csv" => vec![("CSV", vec!["csv"])],
        "xml" => vec![("XML", vec!["xml"])],
        "csl" => vec![("Citation style (CSL)", vec!["csl", "xml"])],
        _ => vec![("All files", vec!["*"])],
    }
}

fn pick_file(app: &tauri::AppHandle, kind: &str, title: &str) -> Option<PathBuf> {
    let mut d = app.dialog().file().set_title(title);
    for (n, e) in filters(kind) {
        d = d.add_filter(n, &e);
    }
    let p = d.blocking_pick_file()?.into_path().ok()?;
    remember(&p);
    Some(p)
}

fn pick_files(app: &tauri::AppHandle, kind: &str, title: &str) -> Vec<PathBuf> {
    let mut d = app.dialog().file().set_title(title);
    for (n, e) in filters(kind) {
        d = d.add_filter(n, &e);
    }
    let v: Vec<PathBuf> = d.blocking_pick_files().unwrap_or_default().into_iter().filter_map(|f| f.into_path().ok()).collect();
    for p in &v {
        remember(p);
    }
    v
}

fn pick_folder(app: &tauri::AppHandle, title: &str) -> Option<PathBuf> {
    let p = app.dialog().file().set_title(title).blocking_pick_folder()?.into_path().ok()?;
    remember(&p);
    Some(p)
}

fn save_file(app: &tauri::AppHandle, kind: &str, name: &str, title: &str) -> Option<PathBuf> {
    let mut d = app.dialog().file().set_title(title).set_file_name(name);
    for (n, e) in filters(kind) {
        d = d.add_filter(n, &e);
    }
    d.blocking_save_file()?.into_path().ok()
}

fn write_out(path: &Path, bytes: &[u8]) -> Result<(), CallError> {
    prm_core::util::write_atomic(path, bytes).map_err(|e| CallError { message: "The file could not be saved. Check the folder and try again.".into(), detail: e.to_string() })
}

fn read_text(p: &Path) -> Result<String, CallError> {
    let bytes = std::fs::read(p).map_err(|e| CallError { message: "The file could not be read.".into(), detail: e.to_string() })?;
    Ok(match String::from_utf8(bytes.clone()) {
        Ok(s) => s,
        Err(_) => {
            // UTF-16 with BOM, or Windows-1252 fallback.
            if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
                let u: Vec<u16> = bytes[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&u)
            } else {
                bytes.iter().map(|b| *b as char).collect()
            }
        }
    })
}

pub fn scheduled_backup(lib: &Library, s: &settings::Settings) -> Result<String, String> {
    let dir = if s.backup_dir.is_empty() { lib.root.join("backups") } else { PathBuf::from(&s.backup_dir) };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!("auto-backup-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let p = dir.join(&name);
    prm_core::backup::backup(lib, &p, s.backup_include_files).map_err(|e| e.to_string())?;
    let v = prm_core::backup::verify(&p).map_err(|e| e.to_string())?;
    if !v.ok {
        return Err("The backup was written but did not verify; it was kept for inspection.".into());
    }
    let _ = prm_core::backup::prune(&dir, "auto-backup-", s.backup_keep.max(1));
    Ok(p.to_string_lossy().to_string())
}

/// Scope needed by each method for local API clients.
pub fn scope_of(method: &str) -> &'static str {
    let ns = method.split('.').next().unwrap_or("");
    match method {
        "refs.list" | "refs.get" | "search.run" | "projects.list" | "projects.summary" | "notes.list" | "collections.list" | "labels.list" | "types.list" | "styles.list" | "library.info" | "custom.fields" | "custom.types" | "fields.values" => "read",
        "cite.render" => "format",
        "capture.options" | "capture.check" => "read",
        // Connected programs may stage imports for review; committing, changing or undoing an
        // import stays with the person in the program's window.
        "capture.stage" | "import.stage" | "import.get" | "import.batches" => "import",
        _ => match ns {
            "lookup" => "import",
            "files" | "ann" => "files",
            "export" => "export",
            "refs" | "notes" | "projects" | "manuscripts" | "collections" | "labels" | "outline" | "tasks" => "write",
            _ => "admin",
        },
    }
}

/// Find and attach a free, legal copy of one source. Network requests run without the lock.
fn find_open_access(shared: &Shared, id: &str, project_id: &str) -> Result<Value, CallError> {
    let (doi, title, has_pdf) = {
        let st = shared.lock().map_err(|_| user("The program is busy; try again."))?;
        network_allowed(&st, "openaccess", project_id)?;
        let l = st.lib.as_ref().ok_or(user("Open or create a library first."))?;
        let r = l.store.must::<Reference>(id).map_err(err)?;
        let has_pdf = l.store.by::<prm_core::model::Attachment>("ref_id", id).map_err(err)?.iter().any(|a| a.mime == "application/pdf" && !a.superseded);
        (r.f("doi").to_string(), r.title().to_string(), has_pdf)
    };
    let base = json!({"ref_id": id, "title": title, "publisher_url": if doi.is_empty() { String::new() } else { format!("https://doi.org/{doi}") }});
    let with = |mut v: Value, extra: Value| { if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) { for (k, x) in e { o.insert(k.clone(), x.clone()); } } v };
    if has_pdf {
        return Ok(with(base, json!({"status": "has_pdf", "message": "This source already has a PDF."})));
    }
    if doi.trim().is_empty() {
        return Ok(with(base, json!({"status": "no_doi", "message": "This source has no DOI, so a free copy cannot be looked up. Add the DOI, or attach a file."})));
    }
    let mut log: Vec<(String, String, String, bool, String)> = vec![];
    let res = prm_core::lookup::open_access(&doi, &mut |k, h, sent, okk, d| log.push((k.into(), h.into(), sent.into(), okk, d.into())));
    let flush = |log: &[(String, String, String, bool, String)]| {
        if let Ok(st) = shared.lock() {
            if let Some(l) = st.lib.as_ref() {
                for (k, h, sent, okk, d) in log {
                    l.store.log_network(k, h, "Find a free copy", sent, *okk, d);
                }
            }
        }
    };
    let oa = match res {
        Ok(oa) => oa,
        Err(e) => {
            flush(&log);
            return Ok(with(base, json!({"status": "error", "message": e.to_string()})));
        }
    };
    if oa.pdf_url.is_empty() {
        flush(&log);
        let msg = if oa.landing_url.is_empty() { "No free copy is known for this DOI." } else { "A free copy is listed, but only as a web page, not a PDF file." };
        return Ok(with(base, json!({"status": "not_found", "message": msg, "landing_url": oa.landing_url, "license": oa.license, "version": oa.version, "host": oa.host})));
    }
    let downloading_ok = { shared.lock().map(|st| network_allowed(&st, "openaccess-download", project_id).is_ok()).unwrap_or(false) };
    if !downloading_ok {
        flush(&log);
        return Ok(with(base, json!({"status": "blocked", "message": "Downloading free copies is turned off in Settings › Network.", "landing_url": oa.landing_url, "pdf_url": oa.pdf_url, "license": oa.license, "version": oa.version, "host": oa.host})));
    }
    let tmp = std::env::temp_dir().join(format!("prm-oa-{}.pdf", prm_core::util::new_id()));
    let dl = prm_core::lookup::download_pdf(&oa.pdf_url, &tmp, &mut |k, h, sent, okk, d| log.push((k.into(), h.into(), sent.into(), okk, d.into())));
    flush(&log);
    if let Err(e) = dl {
        let _ = std::fs::remove_file(&tmp);
        return Ok(with(base, json!({"status": "download_failed", "message": e.to_string(), "landing_url": oa.landing_url, "pdf_url": oa.pdf_url, "license": oa.license, "version": oa.version, "host": oa.host})));
    }
    let mut st = shared.lock().map_err(|_| user("The program is busy; try again."))?;
    let l = lib_mut(&mut st)?;
    let attached = l.attach_file(id, &tmp, "managed", "full-text");
    let _ = std::fs::remove_file(&tmp);
    let mut a = attached.map_err(err)?;
    a.source_url = oa.pdf_url.clone();
    a.license = oa.license.clone();
    a.source_version = oa.version.clone();
    a.original_name = format!("{} (open access).pdf", title.chars().take(80).collect::<String>().trim());
    l.store.write("Record free copy source", |tx| tx.put(&a)).map_err(err)?;
    Ok(with(base, json!({"status": "attached", "message": format!("Attached a free copy from {}.", oa.host), "landing_url": oa.landing_url, "pdf_url": oa.pdf_url, "license": oa.license, "version": oa.version, "host": oa.host})))
}

fn write_log(shared: &Shared, log: &[(String, String, String, bool, String)], purpose: &str) {
    if let Ok(st) = shared.lock() {
        if let Some(l) = st.lib.as_ref() {
            for (k, h, sent, okk, d) in log {
                l.store.log_network(k, h, purpose, sent, *okk, d);
            }
        }
    }
}

/// Find missing DOIs or abstracts for review. Nothing is changed here.
fn propose_online(app: &tauri::AppHandle, shared: &Shared, kind: &str, ids: &[String], pid: &str) -> Result<Vec<prm_core::cleanup::Proposal>, CallError> {
    let net_kind = if kind == "doi" { "doi-search" } else { "abstract" };
    let refs: Vec<Reference> = {
        let st = shared.lock().map_err(|_| user("The program is busy; try again."))?;
        network_allowed(&st, net_kind, pid)?;
        let l = st.lib.as_ref().ok_or(user("Open or create a library first."))?;
        ids.iter().filter_map(|id| l.store.get::<Reference>(id).ok().flatten()).collect()
    };
    let todo: Vec<&Reference> = refs.iter().filter(|r| if kind == "doi" { !r.has("doi") && !r.title().trim().is_empty() } else { r.has("doi") && !r.has("abstract") }).collect();
    let mut log: Vec<(String, String, String, bool, String)> = vec![];
    let mut out = vec![];
    for (i, r) in todo.iter().enumerate() {
        let _ = app.emit("cleanup-progress", json!({"phase": if kind == "doi" { "Looking for DOIs" } else { "Fetching abstracts" }, "done": i, "total": todo.len()}));
        let mut lg = |k: &str, h: &str, sent: &str, okk: bool, d: &str| log.push((k.into(), h.into(), sent.into(), okk, d.into()));
        if kind == "doi" {
            let surname = r.creators.first().map(|c| c.name.surname()).unwrap_or_default();
            if let Ok(Some(m)) = prm_core::lookup::find_doi(r.title(), &surname, r.year(), &mut lg) {
                out.push(prm_core::cleanup::Proposal { ref_id: r.id.clone(), title: r.title().to_string(), field: "doi".into(), before: String::new(), after: m.doi.clone(), source: "lookup:crossref-search".into(),
                    note: format!("Registered as “{}”{}. Check it is the same work.", m.title, m.year.map(|y| format!(" ({y})")).unwrap_or_default()) });
            }
        } else if let Ok(Some(text)) = prm_core::lookup::fetch_abstract(r.f("doi"), &mut lg) {
            out.push(prm_core::cleanup::Proposal { ref_id: r.id.clone(), title: r.title().to_string(), field: "abstract".into(), before: String::new(), after: text, source: "lookup:crossref".into(), note: String::new() });
        }
    }
    let _ = app.emit("cleanup-progress", json!({"phase": "Done", "done": todo.len(), "total": todo.len()}));
    write_log(shared, &log, if kind == "doi" { "Find missing DOIs" } else { "Fetch missing abstracts" });
    Ok(out)
}

/// Progress reports for the window during long imports.
fn progress_emitter(app: &tauri::AppHandle) -> std::sync::Arc<dyn Fn(&str, usize, usize) + Send + Sync> {
    let app = app.clone();
    std::sync::Arc::new(move |phase: &str, done: usize, total: usize| {
        let _ = app.emit("import-progress", json!({"phase": phase, "done": done, "total": total}));
    })
}

fn lib_mut<'a>(st: &'a mut crate::AppState) -> Result<&'a mut Library, CallError> {
    st.lib.as_mut().ok_or(user("Open or create a library first."))
}

pub fn network_allowed(st: &crate::AppState, kind: &str, project_id: &str) -> Result<(), CallError> {
    if !st.settings.network_enabled {
        return Err(user("Network use is turned off in Settings. Local work is unaffected."));
    }
    if st.settings.network_disabled_kinds.iter().any(|k| k == kind) {
        return Err(user("This kind of network request is turned off in Settings › Network."));
    }
    if !project_id.is_empty() {
        if let Some(lib) = st.lib.as_ref() {
            if let Ok(p) = lib.store.must::<Project>(project_id) {
                if p.policy_no_external {
                    return Err(user("This project's policy does not allow sending anything to outside services."));
                }
            }
        }
    }
    Ok(())
}

fn open_library(st: &mut crate::AppState, path: &Path) -> Result<Value, CallError> {
    if let Some(old) = st.lib.take() {
        old.close();
    }
    let mut lib = Library::open(path, &st.device, &st.host).map_err(err)?;
    lib.citekey_pattern = st.settings.citekey_pattern.clone();
    lib.protected_terms = st.settings.protected_terms.clone();
    lib.save_rules = st.settings.save_rules.clone();
    lib.file_name_pattern = st.settings.file_name_pattern.clone();
    lib.network_allowed = st.settings.network_enabled;
    if let Some(b) = lib.info().ok().map(|i| i.id).and_then(|id| st.settings.linked_bases.get(&id).cloned()) {
        if !b.is_empty() {
            lib.linked_base_override = Some(PathBuf::from(b));
        }
    }
    let _ = lib.empty_trash_older_than(st.settings.trash_retention_days.max(1));
    let _ = lib.recompute_copies(None);
    let info = lib.info().map_err(err)?;
    let p = path.to_string_lossy().to_string();
    st.settings.last_library = p.clone();
    st.settings.recent_libraries.retain(|x| x != &p);
    st.settings.recent_libraries.insert(0, p);
    st.settings.recent_libraries.truncate(8);
    let _ = settings::save(&st.config_dir, &st.settings);
    st.lib = Some(lib);
    ok(info)
}

/// Dispatch one call. `client` is set for local API callers (scopes are checked by the caller).
pub fn dispatch(app: &tauri::AppHandle, shared: &Shared, method: &str, a: Value, client: Option<&str>) -> Result<Value, CallError> {
    // Dialog-driven methods must not hold the state lock while a dialog is open.
    match method {
        "dialog.pick_file" => return ok(pick_file(app, &s(&a, "kind"), &s(&a, "title")).map(|p| p.to_string_lossy().to_string())),
        "dialog.pick_files" => return ok(pick_files(app, &s(&a, "kind"), &s(&a, "title")).iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>()),
        "ocr.languages" => return ok(crate::ocr::languages().map_err(|e| user(&e))?),
        "ocr.page" => {
            use base64::Engine as _;
            let img = base64::engine::general_purpose::STANDARD.decode(s(&a, "image").as_bytes()).map_err(|_| user("The page image could not be read."))?;
            return ok(crate::ocr::recognize(img, s(&a, "lang")).map_err(|e| user(&e))?);
        }
        "dialog.pick_folder" => return ok(pick_folder(app, &s(&a, "title")).map(|p| p.to_string_lossy().to_string())),
        "system.open_url" => {
            let url = s(&a, "url");
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err(user("Only web addresses can be opened."));
            }
            open_with_shell(&url);
            return ok(true);
        }
        // Bringing in files: inspection and lookups run without holding the state lock.
        "import.pick_pdfs" | "import.folder" if client.is_none() => {
            let pid = s(&a, "project_id");
            let (paths, root, truncated) = if method == "import.pick_pdfs" {
                (pick_files(app, "pdf", "Choose PDF files to import"), None, false)
            } else {
                let Some(dir) = pick_folder(app, "Choose a folder of PDF files") else { return ok(json!({"cancelled": true})) };
                let (v, t) = crate::pdfimport::walk(&dir);
                for p in &v {
                    remember(p);
                }
                (v, Some(dir), t)
            };
            if paths.is_empty() {
                return if root.is_some() { Err(user("No PDF files were found in that folder or its subfolders.")) } else { ok(json!({"cancelled": true})) };
            }
            let name = root.as_ref().and_then(|r| r.file_name()).map(|n| format!("Folder: {}", n.to_string_lossy())).unwrap_or_else(|| "PDF files".into());
            let mut b = crate::pdfimport::import_paths(app, shared, &paths, root.as_deref(), &pid, &name, if root.is_some() { "folder" } else { "pdf" }, opt(&a, "mirror"))?;
            if truncated {
                b.file_name = format!("{name} (first {} PDFs)", crate::pdfimport::MAX_FILES);
            }
            return ok(b);
        }
        "watch.add" if client.is_none() => {
            let Some(dir) = pick_folder(app, "Choose a folder to watch for new PDFs") else { return ok(json!({"cancelled": true})) };
            let mut st = shared.lock().map_err(|_| user("The program is busy; try again."))?;
            let lib_root = st.lib.as_ref().map(|l| l.root.to_string_lossy().to_string()).ok_or_else(|| user("Open or create a library first."))?;
            if dir.starts_with(&lib_root) {
                return Err(user("Choose a folder outside the library folder."));
            }
            let path = dir.to_string_lossy().to_string();
            st.settings.watched_folders.retain(|w| !(w.path == path && w.library == lib_root));
            st.settings.watched_folders.push(settings::WatchedFolder { path: path.clone(), library: lib_root, project_id: s(&a, "project_id"), mirror: opt(&a, "mirror") });
            let _ = settings::save(&st.config_dir, &st.settings);
            return ok(json!({"path": path}));
        }
        "watch.scan" if client.is_none() => return ok(crate::pdfimport::scan_watched(app, shared)?),
        "cleanup.propose" if client.is_none() && matches!(s(&a, "kind").as_str(), "doi" | "abstract") => {
            return ok(propose_online(app, shared, &s(&a, "kind"), &opt::<Vec<String>>(&a, "ids"), &s(&a, "project_id"))?);
        }
        "links.check" if client.is_none() => {
            let ids: Vec<String> = opt(&a, "ids");
            let pid = s(&a, "project_id");
            let targets: Vec<(String, String, String)> = {
                let st = shared.lock().map_err(|_| user("The program is busy; try again."))?;
                network_allowed(&st, "linkcheck", &pid)?;
                let l = st.lib.as_ref().ok_or(user("Open or create a library first."))?;
                ids.iter().filter_map(|id| l.store.get::<Reference>(id).ok().flatten()).filter(|r| r.has("url")).map(|r| (r.id.clone(), r.title().to_string(), r.f("url").to_string())).collect()
            };
            let mut log: Vec<(String, String, String, bool, String)> = vec![];
            let mut out = vec![];
            for (i, (id, title, url)) in targets.iter().enumerate() {
                let _ = app.emit("cleanup-progress", json!({"phase": "Checking web addresses", "done": i, "total": targets.len()}));
                let st = prm_core::lookup::check_link(url, &mut |k, h, sent, okk, d| log.push((k.into(), h.into(), sent.into(), okk, d.into())));
                out.push(json!({"ref_id": id, "title": title, "status": st}));
            }
            write_log(shared, &log, "Link check");
            return ok(json!({"checked": out, "without_url": ids.len() - targets.len()}));
        }
        "import.stop" if client.is_none() => {
            prm_core::library::IMPORT_CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
            return ok(true);
        }
        // Debug builds only: the same pipelines without the folder picker, for end-to-end tests.
        #[cfg(debug_assertions)]
        "debug.import_folder" => {
            let dir = PathBuf::from(s(&a, "path"));
            let (v, _) = crate::pdfimport::walk(&dir);
            for p in &v {
                remember(p);
            }
            return ok(crate::pdfimport::import_paths(app, shared, &v, Some(&dir), &s(&a, "project_id"), "Folder", "folder", opt(&a, "mirror"))?);
        }
        #[cfg(debug_assertions)]
        "debug.watch_add" => {
            let mut st = shared.lock().map_err(|_| user("busy"))?;
            let lib_root = st.lib.as_ref().map(|l| l.root.to_string_lossy().to_string()).unwrap_or_default();
            st.settings.watched_folders.push(settings::WatchedFolder { path: s(&a, "path"), library: lib_root, project_id: String::new(), mirror: opt(&a, "mirror") });
            return ok(true);
        }
        "files.find_oa" | "files.find_oa_many" if client.is_none() => {
            let ids: Vec<String> = if method == "files.find_oa" { vec![s(&a, "ref_id")] } else { opt(&a, "ids") };
            let pid = s(&a, "project_id");
            let mut results = vec![];
            for (i, id) in ids.iter().enumerate() {
                let _ = app.emit("oa-progress", json!({"done": i, "total": ids.len()}));
                results.push(find_open_access(shared, id, &pid));
            }
            let _ = app.emit("oa-progress", json!({"done": ids.len(), "total": ids.len()}));
            return if method == "files.find_oa" { results.into_iter().next().unwrap_or_else(|| Err(user("Choose a source."))).and_then(ok) } else { ok(results.into_iter().map(|r| r.unwrap_or_else(|e| json!({"error": e.message}))).collect::<Vec<_>>()) };
        }
        // Network work for updates runs without holding the state lock. Never for API clients.
        "update.check" | "update.install" if client.is_none() => {
            let r = if method == "update.check" { crate::updates::check(app, shared) } else { crate::updates::install(app, shared) };
            return r.map_err(|m| user(&m));
        }
        // The quick-copy launcher: never for local API clients.
        "quickcopy.set" if client.is_none() => {
            let sc = s(&a, "shortcut");
            crate::quickcopy::apply(app, &sc).map_err(|m| user(&m))?;
            let mut st = shared.lock().map_err(|_| user("The program is busy."))?;
            if st.policy.locked.contains_key("quick_copy_shortcut") {
                return Err(user("The quick-copy shortcut is set by your administrator."));
            }
            st.settings.quick_copy_shortcut = sc;
            let _ = settings::save(&st.config_dir, &st.settings);
            return ok(true);
        }
        "quickcopy.show" if client.is_none() => {
            crate::quickcopy::show(app);
            return ok(true);
        }
        "quickcopy.hide" if client.is_none() => {
            crate::quickcopy::hide(app);
            return ok(true);
        }
        "update.rollback_check" | "update.rollback" if client.is_none() => {
            return crate::updates::rollback(app, shared, method == "update.rollback").map_err(|m| user(&m));
        }
        _ => {}
    }
    if client.is_some() && method.starts_with("dialog.") {
        return Err(user("Not available to API clients."));
    }

    // Methods that need a save or open dialog first.
    let mut pre_path: Option<PathBuf> = None;
    let mut pre_paths: Vec<PathBuf> = vec![];
    let needs_save = |kind: &str, name: &str, title: &str| save_file(app, kind, name, title);
    match method {
        "cite.export_bibliography" => {
            let f = s(&a, "format");
            pre_path = needs_save(&f, &format!("references.{}", if f == "txt" { "txt" } else { &f }), "Export bibliography");
        }
        "notes.export" => {
            let f = s(&a, "format");
            pre_path = needs_save(&f, &format!("notes.{}", if f == "json" { "json" } else { &f }), "Export notes");
        }
        "family.export" => pre_path = needs_save("json", "prism-exchange.json", "Export exchange file"),
        "custom.export" => pre_path = needs_save("json", "prism-fields-and-types.json", "Save fields and types"),
        "custom.import" => pre_path = pick_file(app, "json", "Choose a fields and types file"),
        "styles.import" => pre_path = pick_file(app, "csl", "Choose a CSL style file"),
        "ann.export_pdf" => pre_path = needs_save("pdf", &format!("{}-annotated.pdf", s(&a, "name").trim_end_matches(".pdf")), "Export annotated PDF copy"),
        "ann.export_list" => {
            let f = s(&a, "format");
            pre_path = needs_save(&f, &format!("annotations.{f}"), "Export annotations");
        }
        "ann.preview_external" => pre_path = pick_file(app, "pdf", "Choose the PDF with annotations made in another reader"),
        "export.refs" => {
            let f = s(&a, "format");
            let ext = match f.as_str() { "ris" => "ris", "bibtex" | "biblatex" => "bib", "csljson" => "json", "csv" => "csv", "xlsx" => "xlsx", "xml" => "xml", _ => "txt" };
            pre_path = needs_save(&f, &format!("references.{ext}"), "Export references");
        }
        "export.package" => pre_path = needs_save("zip", "prism-library-export.zip", "Export complete library package"),
        "export.attachments" => pre_path = needs_save("zip", "attachments.zip", "Export attachments"),
        "backup.now" => pre_path = needs_save("zip", &format!("prism-backup-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M")), "Save backup"),
        "backup.verify" => pre_path = pick_file(app, "zip", "Choose a backup to verify"),
        "backup.restore" | "package.import" => {
            pre_path = pick_file(app, "zip", if method == "backup.restore" { "Choose a backup to restore" } else { "Choose an export package" });
            if pre_path.is_some() {
                pre_paths = pick_folder(app, "Choose an empty folder for the restored library").into_iter().collect();
            }
        }
        "files.attach_pick" => pre_paths = pick_files(app, "any", "Attach files"),
        "files.relink_pick" => pre_path = pick_file(app, "any", "Find the moved file"),
        "files.replace_pick" => pre_path = pick_file(app, "any", "Choose the new version"),
        "journals.import_pick" => pre_path = pick_file(app, "any", "Choose a journal abbreviation list (text or CSV)"),
        "import.pick" => pre_path = pick_file(app, "bib", "Choose a bibliographic file"),
        "library.choose" => pre_path = pick_folder(app, "Choose a library folder"),
        "diagnostics.save" => pre_path = needs_save("zip", "prism-diagnostics.zip", "Save diagnostic bundle"),
        _ => {}
    }
    let dialog_methods = ["custom.export", "custom.import", "styles.import", "cite.export_bibliography", "notes.export", "family.export", "ann.export_pdf", "ann.export_list", "ann.preview_external", "export.refs", "export.package", "export.attachments", "backup.now", "backup.verify", "backup.restore", "package.import", "files.relink_pick", "files.replace_pick", "journals.import_pick", "import.pick", "library.choose", "diagnostics.save"];
    if dialog_methods.contains(&method) && pre_path.is_none() {
        return ok(json!({"cancelled": true}));
    }
    if matches!(method, "files.attach_pick") && pre_paths.is_empty() {
        return ok(json!({"cancelled": true}));
    }
    if matches!(method, "backup.restore" | "package.import") && pre_paths.is_empty() {
        return ok(json!({"cancelled": true}));
    }

    let mut guard = shared.lock().map_err(|_| user("The program is busy; try again."))?;
    let st: &mut crate::AppState = &mut guard;
    let out = route(app, st, method, &a, pre_path, pre_paths, client);
    if let Ok(v) = &out {
        let _ = v;
        if !method.ends_with(".list") && !method.ends_with(".get") && !method.starts_with("cite.render") && !method.starts_with("search.") {
            if let Some(lib) = st.lib.as_ref() {
                let _ = app.emit("changed", json!({"method": method, "undo": lib.store.undo_label(), "redo": lib.store.redo_label()}));
            }
        }
    }
    out
}

fn open_with_shell(target: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", target]).creation_flags(0x08000000).spawn();
    }
}

#[allow(clippy::too_many_lines)]
fn route(app: &tauri::AppHandle, st: &mut crate::AppState, method: &str, a: &Value, path: Option<PathBuf>, paths: Vec<PathBuf>, client: Option<&str>) -> Result<Value, CallError> {
    match method {
        // ---- application ---------------------------------------------------------------------
        "app.info" => {
            let lib_info = st.lib.as_ref().map(|l| l.info()).transpose().map_err(err)?;
            ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "settings": st.settings,
                "policy": st.policy,
                "portable": settings::portable(),
                "default_library": settings::default_library_dir().to_string_lossy(),
                "library": lib_info,
                "styles": st.lib.as_ref().map(|l| l.styles()).unwrap_or_else(prm_core::render::installed_styles),
                "types": prm_core::types::TYPES,
                "custom_fields": st.lib.as_ref().and_then(|l| l.custom_fields().ok()).unwrap_or_default(),
                "custom_types": st.lib.as_ref().and_then(|l| l.custom_types().ok()).unwrap_or_default(),
                "note_kinds": NOTE_KINDS,
                "network_inventory": prm_core::lookup::NETWORK_INVENTORY.iter().map(|(k, h, p)| json!({"kind": k, "host": h, "purpose": p})).collect::<Vec<_>>(),
            }))
        }
        "update.state" => ok(crate::updates::state(st)),
        "settings.set" => {
            let patch = a.get("settings").cloned().unwrap_or(Value::Null);
            let mut v = serde_json::to_value(&st.settings).unwrap_or_default();
            if let (Value::Object(cur), Value::Object(p)) = (&mut v, patch) {
                for (k, val) in p {
                    if st.policy.locked.contains_key(&k) {
                        return Err(user(&format!("“{k}” is set by your administrator and cannot be changed here.")));
                    }
                    cur.insert(k, val);
                }
            }
            st.settings = serde_json::from_value(v).map_err(|e| CallError { message: "A setting was not valid.".into(), detail: e.to_string() })?;
            if let Some(lib) = st.lib.as_mut() {
                lib.citekey_pattern = st.settings.citekey_pattern.clone();
                lib.protected_terms = st.settings.protected_terms.clone();
                lib.save_rules = st.settings.save_rules.clone();
                lib.file_name_pattern = st.settings.file_name_pattern.clone();
                lib.network_allowed = st.settings.network_enabled;
            }
            settings::save(&st.config_dir, &st.settings).map_err(|e| CallError { message: "Settings could not be saved.".into(), detail: e.to_string() })?;
            ok(&st.settings)
        }
        // Recent libraries with their names, for the switcher and for copying between libraries.
        "library.recent" => {
            let current = st.lib.as_ref().map(|l| l.root.to_string_lossy().to_string()).unwrap_or_default();
            let v: Vec<Value> = st
                .settings
                .recent_libraries
                .iter()
                .filter(|p| PathBuf::from(p).join(prm_core::store::DB_FILE).exists())
                .map(|p| {
                    let name = rusqlite::Connection::open_with_flags(PathBuf::from(p).join(prm_core::store::DB_FILE), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .ok()
                        .and_then(|c| c.query_row("SELECT value FROM meta WHERE key='name'", [], |r| r.get::<_, String>(0)).ok())
                        .unwrap_or_default();
                    json!({"path": p, "name": name, "current": *p == current})
                })
                .collect();
            ok(v)
        }
        "library.transfer" => {
            let target = PathBuf::from(s(a, "target"));
            if !target.join(prm_core::store::DB_FILE).exists() {
                return Err(user("No library was found in that folder."));
            }
            let ids: Vec<String> = arg(a, "ids")?;
            let (device, host) = (st.device.clone(), st.host.clone());
            let mut other = Library::open(&target, &device, &host).map_err(err)?;
            let l = lib_mut(st)?;
            if other.root == l.root {
                other.close();
                return Err(user("Choose a different library to copy into."));
            }
            let res = l.transfer_to(&mut other, &ids, opt(a, "files"), opt(a, "move"));
            other.close();
            ok(res.map_err(err)?)
        }
        "library.open" => {
            let p = PathBuf::from(s(a, "path"));
            if !p.join(prm_core::store::DB_FILE).exists() {
                return Err(user("No library was found in that folder. Choose another folder or create a new library."));
            }
            open_library(st, &p)
        }
        "library.create" => {
            let p = PathBuf::from(s(a, "path"));
            if p.join(prm_core::store::DB_FILE).exists() {
                return open_library(st, &p);
            }
            let info = open_library(st, &p)?;
            let name = s(a, "name");
            if !name.is_empty() {
                lib_mut(st)?.rename(&name).map_err(err)?;
            }
            let _ = info;
            ok(lib_mut(st)?.info().map_err(err)?)
        }
        "library.choose" => {
            let p = path.unwrap();
            if p.join(prm_core::store::DB_FILE).exists() {
                open_library(st, &p)
            } else {
                ok(json!({"empty_folder": p.to_string_lossy()}))
            }
        }
        "library.close" => {
            if let Some(l) = st.lib.take() {
                l.close();
            }
            ok(true)
        }
        "library.info" => ok(lib_mut(st)?.info().map_err(err)?),
        "link.pending" => ok(st.pending_link.take()),
        "link.resolve" => ok(crate::links::resolve(st, &s(a, "url"))),
        "link.make" => {
            let kind = s(a, "kind");
            if !crate::links::KINDS.contains(&kind.as_str()) {
                return Err(user("Links can point to a source, collection, annotation, note or project."));
            }
            let lid = lib_mut(st)?.store.meta("library_id").unwrap_or_default();
            ok(crate::links::make(&kind, &s(a, "id"), &lid))
        }
        "library.schema" => {
            let l = lib_mut(st)?;
            ok(json!({
                "version": l.store.schema_version(),
                "supported": prm_core::store::SCHEMA_VERSION,
                "history": l.store.schema_history().map_err(err)?,
                "backups": prm_core::store::upgrade_backups(&l.root),
            }))
        }
        // Put the library back as it was before a format upgrade. It is closed first and not
        // reopened here, because this program would upgrade it again.
        "library.revert_upgrade" => {
            if !opt::<bool>(a, "confirmed") {
                return Err(user("Returning to the earlier format needs confirmation."));
            }
            let root = lib_mut(st)?.root.clone();
            if let Some(l) = st.lib.take() {
                l.close();
            }
            let kept = prm_core::store::revert_upgrade(&root, &s(a, "backup")).map_err(err)?;
            st.settings.last_library.clear();
            let _ = settings::save(&st.config_dir, &st.settings);
            ok(json!({"kept": kept.to_string_lossy(), "root": root.to_string_lossy()}))
        }
        "library.rename" => ok(lib_mut(st)?.rename(&s(a, "name")).map_err(err)?),
        "library.reveal" => {
            let l = lib_mut(st)?;
            open_with_shell(&l.root.to_string_lossy());
            ok(true)
        }
        "library.reveal_mirror" => {
            let l = lib_mut(st)?;
            let d = l.store.mirror_dir();
            let _ = std::fs::create_dir_all(&d);
            open_with_shell(&d.to_string_lossy());
            ok(true)
        }
        "undo" => ok(lib_mut(st)?.undo().map_err(err)?),
        "redo" => ok(lib_mut(st)?.redo().map_err(err)?),
        "history" => ok(lib_mut(st)?.store.undo_history(100).map_err(err)?),
        "jobs.list" => ok(&st.jobs),
        "jobs.clear" => {
            st.jobs.clear();
            ok(true)
        }

        // ---- references --------------------------------------------------------------------
        "refs.list" => {
            let q: ListQuery = opt(a, "query");
            ok(lib_mut(st)?.list_references(&q).map_err(err)?)
        }
        "refs.get" => {
            let id = s(a, "id");
            let project = s(a, "project_id");
            let l = lib_mut(st)?;
            let r = l.reference(&id).map_err(err)?;
            let atts = l.attachments(&r.id).map_err(err)?;
            let notes = l.notes_for_ref(&r.id, &project).map_err(err)?;
            let membership = if project.is_empty() { None } else { l.membership(&project, &r.id).map_err(err)? };
            let impact = l.impact(&r.id).map_err(err)?;
            let versions: Vec<Reference> = if r.work_id.is_empty() { vec![] } else { l.store.all::<Reference>().map_err(err)?.into_iter().filter(|x| x.work_id == r.work_id && x.id != r.id).collect() };
            ok(json!({"ref": r, "attachments": atts, "notes": notes, "membership": membership, "impact": impact, "versions": versions, "type": prm_core::types::get(&r.rtype)}))
        }
        "refs.add" => {
            let r: Reference = arg(a, "ref")?;
            let p = s(a, "project_id");
            let l = lib_mut(st)?;
            if client.is_some() {
                let mut r = r;
                r.origin = "agent".into();
                return ok(l.add_reference(r, if p.is_empty() { None } else { Some(&p) }).map_err(err)?);
            }
            ok(l.add_reference(r, if p.is_empty() { None } else { Some(&p) }).map_err(err)?)
        }
        "refs.update" => {
            let r: Reference = arg(a, "ref")?;
            let src = s(a, "source");
            let source = if client.is_some() { "agent".to_string() } else if src.starts_with("lookup:") { src } else { "user".to_string() };
            ok(lib_mut(st)?.update_reference(r, &source).map_err(err)?)
        }
        "terms.list" => ok(lib_mut(st)?.terms(&s(a, "kind")).map_err(err)?),
        "terms.variants" => ok(lib_mut(st)?.term_variants(&s(a, "kind")).map_err(err)?),
        "terms.merge" => ok(lib_mut(st)?.merge_terms(&s(a, "kind"), &arg::<Vec<String>>(a, "from")?, &s(a, "to")).map_err(err)?),
        "bib.add_export" => {
            let pid = s(a, "project_id");
            let biblatex: bool = opt(a, "biblatex");
            let mut format = s(a, "format");
            if format.is_empty() {
                format = if biblatex { "biblatex".into() } else { "bibtex".into() };
            }
            let (kind, name) = match format.as_str() { "ris" => ("ris", "references.ris"), "csljson" => ("csljson", "references.json"), "csv" => ("csv", "references.csv"), "xlsx" => ("xlsx", "references.xlsx"), _ => ("bibtex", "references.bib") };
            let Some(p) = save_file(app, kind, name, "Choose where to keep the file") else { return ok(json!({"cancelled": true})) };
            let exclude_fields: Vec<String> = opt(a, "exclude_fields");
            let n = lib_mut(st)?.write_kept_file(&pid, &p, &format, &exclude_fields).map_err(err)?;
            st.settings.bib_exports.retain(|b| b.path != p.to_string_lossy());
            st.settings.bib_exports.push(settings::BibExport { project_id: pid, path: p.to_string_lossy().to_string(), biblatex: format == "biblatex", format, exclude_fields });
            let _ = settings::save(&st.config_dir, &st.settings);
            ok(json!({"path": p.to_string_lossy(), "count": n}))
        }
        "bib.remove_export" => {
            let path = s(a, "path");
            st.settings.bib_exports.retain(|b| b.path != path);
            let _ = settings::save(&st.config_dir, &st.settings);
            ok(true)
        }
        "refs.trash" => ok(lib_mut(st)?.trash(&arg::<Vec<String>>(a, "ids")?).map_err(err)?),
        "refs.restore" => ok(lib_mut(st)?.restore(&arg::<Vec<String>>(a, "ids")?).map_err(err)?),
        "refs.purge" => ok(lib_mut(st)?.purge(&arg::<Vec<String>>(a, "ids")?, opt(a, "confirmed")).map_err(err)?),
        "refs.empty_trash" => {
            if !opt::<bool>(a, "confirmed") {
                return Err(user("Emptying the trash needs confirmation."));
            }
            let l = lib_mut(st)?;
            let ids: Vec<String> = l.list_references(&ListQuery { view: "trash".into(), limit: usize::MAX, ..Default::default() }).map_err(err)?.rows.into_iter().map(|r| r.id).collect();
            ok(l.purge(&ids, true).map_err(err)?)
        }
        "refs.merge" => ok(lib_mut(st)?.merge(&s(a, "target"), &arg::<Vec<String>>(a, "others")?, &opt::<BTreeMap<String, String>>(a, "choices")).map_err(err)?),
        "refs.duplicates" => {
            let th: f32 = a.get("threshold").and_then(|v| v.as_f64()).map(|f| f as f32).unwrap_or(st.settings.duplicate_sensitivity);
            ok(lib_mut(st)?.duplicates(th).map_err(err)?)
        }
        "refs.decide_duplicate" => ok(lib_mut(st)?.decide_duplicate(&s(a, "a"), &s(a, "b"), &s(a, "decision")).map_err(err)?),
        "refs.link_versions" => ok(lib_mut(st)?.link_versions(&arg::<Vec<String>>(a, "ids")?, &s(a, "preferred")).map_err(err)?),
        "refs.unlink_version" => ok(lib_mut(st)?.unlink_version(&s(a, "id")).map_err(err)?),
        "refs.field_history" => ok(lib_mut(st)?.field_history(&s(a, "id"), &s(a, "field")).map_err(err)?),
        "refs.restore_field" => ok(lib_mut(st)?.restore_field(&s(a, "id"), &s(a, "field"), a.get("value").cloned().unwrap_or(Value::Null)).map_err(err)?),
        "refs.bulk_edit" => ok(lib_mut(st)?.bulk_edit(&arg::<Vec<String>>(a, "ids")?, &arg::<BulkOp>(a, "op")?, opt(a, "apply")).map_err(err)?),
        "refs.find_replace" => {
            let ids: Option<Vec<String>> = a.get("ids").and_then(|v| serde_json::from_value(v.clone()).ok());
            ok(lib_mut(st)?.find_replace(ids.as_deref(), &s(a, "field"), &s(a, "find"), &s(a, "replace"), opt(a, "regex"), opt(a, "case_sensitive"), opt(a, "apply")).map_err(err)?)
        }
        "refs.set_labels" => ok(lib_mut(st)?.set_labels(&arg::<Vec<String>>(a, "ids")?, &s(a, "label"), opt(a, "on")).map_err(err)?),
        "refs.set_rating" => ok(lib_mut(st)?.set_rating(&arg::<Vec<String>>(a, "ids")?, opt(a, "rating")).map_err(err)?),
        "refs.confirm_values" => {
            let fields: Vec<String> = arg(a, "fields")?;
            ok(lib_mut(st)?.confirm_values(&s(a, "id"), &fields).map_err(err)?)
        }
        "refs.regenerate_citekey" => ok(lib_mut(st)?.regenerate_citekey(&s(a, "id")).map_err(err)?),
        "refs.impact" => ok(lib_mut(st)?.impact(&s(a, "id")).map_err(err)?),
        "refs.confirm_absent" => ok(lib_mut(st)?.confirm_absent(&s(a, "id"), &s(a, "field"), opt(a, "absent")).map_err(err)?),
        "refs.resolve_conflict" => ok(lib_mut(st)?.resolve_conflict(&s(a, "id"), &s(a, "field"), a.get("value").and_then(|v| v.as_str()).map(String::from)).map_err(err)?),
        "refs.citekey_copy" => {
            let l = lib_mut(st)?;
            let ids: Vec<String> = arg(a, "ids")?;
            let keys: Vec<String> = l.references(&ids).map_err(err)?.into_iter().map(|r| r.citekey).collect();
            let syntax = s(a, "syntax");
            let text = match syntax.as_str() {
                "latex" => format!("\\cite{{{}}}", keys.join(",")),
                "pandoc" => format!("[{}]", keys.iter().map(|k| format!("@{k}")).collect::<Vec<_>>().join("; ")),
                _ => keys.join(", "),
            };
            crate::clipboard::write(&text, None, None).map_err(|e| user(&e))?;
            ok(text)
        }
        "refs.labels" => ok(lib_mut(st)?.source_labels(&arg::<Vec<String>>(a, "ids")?).map_err(err)?),
        "types.list" => ok(prm_core::types::TYPES),
        "custom.fields" => ok(lib_mut(st)?.custom_fields().map_err(err)?),
        "custom.types" => ok(lib_mut(st)?.custom_types().map_err(err)?),
        "custom.save_field" => ok(lib_mut(st)?.save_custom_field(arg(a, "field")?).map_err(err)?),
        "custom.delete_field" => ok(lib_mut(st)?.delete_custom_field(&s(a, "id"), opt(a, "remove_values")).map_err(err)?),
        "custom.save_type" => ok(lib_mut(st)?.save_custom_type(arg(a, "type")?).map_err(err)?),
        "custom.delete_type" => ok(lib_mut(st)?.delete_custom_type(&s(a, "id")).map_err(err)?),
        "custom.export" => {
            let v = lib_mut(st)?.export_definitions().map_err(err)?;
            let p = path.unwrap();
            write_out(&p, serde_json::to_string_pretty(&v).unwrap_or_default().as_bytes())?;
            ok(json!({"path": p.to_string_lossy()}))
        }
        "custom.import" => {
            let text = std::fs::read_to_string(path.unwrap()).map_err(|_| user("The file could not be read."))?;
            ok(lib_mut(st)?.import_definitions(&text).map_err(err)?)
        }
        "fields.values" => ok(lib_mut(st)?.field_values(&s(a, "field"), &s(a, "prefix")).map_err(err)?),
        "labels.list" => ok(lib_mut(st)?.store.all::<Label>().map_err(err)?),
        "labels.save" => ok(lib_mut(st)?.save_label(arg(a, "label")?).map_err(err)?),
        "labels.create_many" => {
            #[derive(serde::Deserialize)]
            struct L {
                name: String,
                #[serde(default)]
                color: String,
            }
            let items: Vec<L> = arg(a, "labels")?;
            ok(lib_mut(st)?.create_labels(&items.into_iter().map(|l| (l.name, l.color)).collect::<Vec<_>>()).map_err(err)?)
        }
        "labels.merge" => ok(lib_mut(st)?.merge_labels(&arg::<Vec<String>>(a, "ids")?, &s(a, "into")).map_err(err)?),
        "labels.delete" => {
            let into = s(a, "into");
            ok(lib_mut(st)?.delete_label(&s(a, "id"), if into.is_empty() { None } else { Some(&into) }).map_err(err)?)
        }
        "collections.list" => ok(lib_mut(st)?.store.all::<Collection>().map_err(err)?),
        "collections.save" => ok(lib_mut(st)?.save_collection(arg(a, "collection")?).map_err(err)?),
        "collections.delete" => ok(lib_mut(st)?.delete_collection(&s(a, "id")).map_err(err)?),
        "collections.move" => ok(lib_mut(st)?.move_collection(&s(a, "id"), &s(a, "parent_id")).map_err(err)?),
        "collections.move_sources" => ok(lib_mut(st)?.move_between_collections(&s(a, "from"), &s(a, "to"), &arg::<Vec<String>>(a, "ids")?).map_err(err)?),
        "collections.collect" => ok(lib_mut(st)?.collect(&s(a, "id"), &arg::<Vec<String>>(a, "ids")?, opt(a, "add")).map_err(err)?),
        "searches.list" => ok(lib_mut(st)?.store.all::<SavedSearch>().map_err(err)?),
        "searches.save" => ok(lib_mut(st)?.save_search(arg(a, "search")?).map_err(err)?),
        "searches.delete" => {
            let id = s(a, "id");
            ok(lib_mut(st)?.write("Delete saved search", |tx| tx.delete::<SavedSearch>(&id)).map_err(err)?)
        }
        "search.run" => {
            let l = lib_mut(st)?;
            let project = s(a, "project_id");
            let res = prm_core::search::run(&l.store.conn, &s(a, "query"), if project.is_empty() { None } else { Some(&project) }, None, opt::<usize>(a, "limit").max(50)).map_err(err)?;
            let labels = l.source_labels(&res.ref_ids).map_err(err)?;
            ok(json!({"results": res, "labels": labels, "pending_index": prm_core::search::pending_index_count(&l.store.conn).map_err(err)?}))
        }
        "search.check" => ok(prm_core::search::parse(&s(a, "query")).map(|q| format!("{q:?}")).map_err(err)?),
        "palette.search" => ok(prm_core::search::palette(&lib_mut(st)?.store.conn, &s(a, "query")).map_err(err)?),

        // ---- projects and manuscripts ----------------------------------------------------------
        "projects.list" => ok(lib_mut(st)?.projects(opt(a, "include_archived")).map_err(err)?),
        "projects.summary" => ok(lib_mut(st)?.project_summary(&s(a, "id")).map_err(err)?),
        "projects.create" => {
            let ms = s(a, "manuscript");
            ok(lib_mut(st)?.create_project(&s(a, "title"), &s(a, "style"), if ms.is_empty() { None } else { Some(&ms) }).map_err(err)?)
        }
        "projects.update" => ok(lib_mut(st)?.update_project(arg(a, "project")?).map_err(err)?),
        "projects.archive" => ok(lib_mut(st)?.archive_project(&s(a, "id"), opt(a, "archived")).map_err(err)?),
        "projects.delete" => ok(lib_mut(st)?.delete_project(&s(a, "id"), opt(a, "confirmed")).map_err(err)?),
        "projects.copy" => ok(lib_mut(st)?.copy_project(&s(a, "id"), &arg::<CopyOptions>(a, "options")?).map_err(err)?),
        "projects.add_sources" => ok(lib_mut(st)?.add_to_project(&s(a, "id"), &arg::<Vec<String>>(a, "ids")?).map_err(err)?),
        "projects.removal_preview" => ok(lib_mut(st)?.removal_preview(&s(a, "id"), &s(a, "ref_id")).map_err(err)?),
        "projects.remove_sources" => ok(lib_mut(st)?.remove_from_project(&s(a, "id"), &arg::<Vec<String>>(a, "ids")?).map_err(err)?),
        "projects.recent" => ok(lib_mut(st)?.recent_work(opt::<usize>(a, "limit").max(10)).map_err(err)?),
        "projects.reading_queue" => ok(lib_mut(st)?.reading_queue(opt::<usize>(a, "limit").max(5)).map_err(err)?),
        "projects.set_active" => {
            st.settings.active_project = s(a, "id");
            let _ = settings::save(&st.config_dir, &st.settings);
            ok(true)
        }
        "membership.update" => ok(lib_mut(st)?.update_membership(arg(a, "membership")?).map_err(err)?),
        "membership.set_rating" => {
            let l = lib_mut(st)?;
            let pid = s(a, "project_id");
            let rating: u8 = opt(a, "rating");
            let mut n = 0;
            for id in arg::<Vec<String>>(a, "ids")? {
                if let Some(mut m) = l.membership(&pid, &id).map_err(err)? {
                    m.rating = rating.min(5);
                    l.update_membership(m).map_err(err)?;
                    n += 1;
                }
            }
            ok(n)
        }
        "membership.add_time" => ok(lib_mut(st)?.add_reading_time(&s(a, "project_id"), &s(a, "ref_id"), opt(a, "secs")).map_err(err)?),
        "membership.position" => ok(lib_mut(st)?.set_reading_position(&s(a, "project_id"), &s(a, "ref_id"), arg(a, "position")?).map_err(err)?),
        "manuscripts.create" => ok(lib_mut(st)?.create_manuscript(&s(a, "project_id"), &s(a, "title"), &s(a, "style"), &s(a, "locale")).map_err(err)?),
        "manuscripts.update" => ok(lib_mut(st)?.update_manuscript(arg(a, "manuscript")?).map_err(err)?),
        "manuscripts.set_active" => ok(lib_mut(st)?.set_active_manuscript(&s(a, "project_id"), &s(a, "id")).map_err(err)?),
        "manuscripts.sources" => ok(lib_mut(st)?.manuscript_sources(&s(a, "id")).map_err(err)?),
        "manuscripts.set_use" => ok(lib_mut(st)?.set_manuscript_use(&s(a, "id"), &arg::<Vec<String>>(a, "ref_ids")?, arg(a, "state")?).map_err(err)?),

        // ---- citation output ------------------------------------------------------------------
        "cite.render" => ok(lib_mut(st)?.render(&arg::<CiteRequest>(a, "request")?).map_err(err)?),
        "cite.copy" => {
            let req: CiteRequest = arg(a, "request")?;
            let plain: bool = opt(a, "plain");
            let l = lib_mut(st)?;
            let out = l.render(&req).map_err(err)?;
            let text = out.text.clone();
            let res = if plain { crate::clipboard::write(&text, None, None) } else { crate::clipboard::write(&text, Some(&out.html), Some(&out.rtf)) };
            match res {
                Ok(()) => {
                    let ev = if req.mode == OutputMode::Citekey { None } else { Some(l.record_copy(&req, &out).map_err(err)?) };
                    ok(json!({"output": out, "copy": ev}))
                }
                Err(e) => Err(CallError { message: format!("{e} The preview is unchanged; you can retry, or export the text to a file."), detail: String::new() }),
            }
        }
        "cite.copy_text" => {
            // Copy text exactly as shown (for "Copy updated text" in the ledger).
            let text = s(a, "text");
            let html = s(a, "html");
            crate::clipboard::write(&text, if html.is_empty() { None } else { Some(&html) }, None).map_err(|e| user(&e))?;
            ok(true)
        }
        "cite.export_bibliography" => {
            let req: CiteRequest = arg(a, "request")?;
            let font = s(a, "font");
            let (bytes, _) = lib_mut(st)?.export_bibliography(&req, &s(a, "format"), if font.is_empty() { None } else { Some(&font) }).map_err(err)?;
            let p = path.unwrap();
            write_out(&p, &bytes)?;
            ok(json!({"path": p.to_string_lossy()}))
        }
        "cite.quote_note" => {
            let l = lib_mut(st)?;
            let out = l.quote_text(&s(a, "note_id"), &s(a, "manuscript_id")).map_err(err)?;
            crate::clipboard::write(&out.text, Some(&out.html), Some(&out.rtf)).map_err(|e| user(&e))?;
            let n = l.store.must::<Note>(&s(a, "note_id")).map_err(err)?;
            let req = CiteRequest { manuscript_id: s(a, "manuscript_id"), items: vec![CiteItem { ref_id: n.ref_ids[0].clone(), locator_label: n.locator_label.clone(), locator: n.locator.clone(), ..Default::default() }], mode: OutputMode::QuoteWithCitation, quote: n.exact_text.clone(), ..Default::default() };
            let ev = l.record_copy(&req, &out).map_err(err)?;
            ok(json!({"output": out, "copy": ev}))
        }
        "copies.list" => ok(lib_mut(st)?.store.by::<CopyEvent>("manuscript_id", &s(a, "manuscript_id")).map_err(err)?),
        "copies.set_status" => ok(lib_mut(st)?.set_copy_status(&s(a, "id"), arg(a, "status")?).map_err(err)?),
        "copies.record_unknown" => ok(lib_mut(st)?.record_unknown_copy(&s(a, "manuscript_id"), &s(a, "text"), &arg::<Vec<String>>(a, "ref_ids")?).map_err(err)?),
        "readiness" => ok(lib_mut(st)?.readiness(&s(a, "manuscript_id")).map_err(err)?),
        "style.issues" => {
            let l = lib_mut(st)?;
            let ids: Vec<String> = arg(a, "ref_ids")?;
            let refs = l.references(&ids).map_err(err)?;
            ok(l.style_issues(&refs, &[]).map_err(err)?)
        }
        "style.apply" => ok(lib_mut(st)?.apply_suggestion(&s(a, "ref_id"), &s(a, "code")).map_err(err)?),
        "styles.list" => ok(st.lib.as_ref().map(|l| l.styles()).unwrap_or_else(prm_core::render::installed_styles)),
        "styles.import" => {
            let p = path.unwrap();
            let xml = std::fs::read_to_string(&p).map_err(|_| user("The style file could not be read. CSL styles are UTF-8 text files ending in .csl."))?;
            ok(lib_mut(st)?.import_style(&xml).map_err(err)?)
        }
        "styles.remove" => ok(lib_mut(st)?.remove_style(&s(a, "id")).map_err(err)?),
        "snapshot.save" => ok(lib_mut(st)?.save_snapshot(&s(a, "manuscript_id"), &s(a, "label")).map_err(err)?),
        "snapshot.verify" => ok(lib_mut(st)?.verify_snapshot(&s(a, "id")).map_err(err)?),
        "snapshot.get" => ok(lib_mut(st)?.store.must::<Snapshot>(&s(a, "id")).map_err(err)?),

        // ---- notes, outline, tasks ------------------------------------------------------------
        "notes.list" => ok(lib_mut(st)?.notes(&s(a, "project_id"), opt(a, "include_library")).map_err(err)?),
        "notes.for_ref" => ok(lib_mut(st)?.notes_for_ref(&s(a, "ref_id"), &s(a, "project_id")).map_err(err)?),
        "notes.save" => {
            let mut n: Note = arg(a, "note")?;
            if client.is_some() && n.id.is_empty() {
                n.origin = "agent".into();
                n.origin_detail = client.unwrap_or("").into();
            }
            ok(lib_mut(st)?.save_note(n).map_err(err)?)
        }
        "notes.merge" => ok(lib_mut(st)?.merge_notes(&s(a, "keep"), &s(a, "other")).map_err(err)?),
        "notes.split" => ok(lib_mut(st)?.split_note(&s(a, "id"), opt(a, "at")).map_err(err)?),
        "notes.trash" => ok(lib_mut(st)?.trash_note(&s(a, "id")).map_err(err)?),
        "notes.accept" => ok(lib_mut(st)?.accept_machine_note(&s(a, "id")).map_err(err)?),
        "notes.from_annotation" => ok(lib_mut(st)?.note_from_annotation(&s(a, "annotation_id"), &s(a, "kind")).map_err(err)?),
        "notes.export" => {
            let (bytes, _) = lib_mut(st)?.export_notes(&s(a, "project_id"), &s(a, "manuscript_id"), opt(a, "outline_only"), &s(a, "format")).map_err(err)?;
            let p = path.unwrap();
            write_out(&p, &bytes)?;
            ok(json!({"path": p.to_string_lossy()}))
        }
        "family.export" => {
            let bytes = lib_mut(st)?.export_family_exchange(&s(a, "project_id"), &opt::<Vec<String>>(a, "note_ids")).map_err(err)?;
            let p = path.unwrap();
            write_out(&p, &bytes)?;
            ok(json!({"path": p.to_string_lossy()}))
        }
        "outline.get" => ok(lib_mut(st)?.outline(&s(a, "project_id")).map_err(err)?),
        "outline.save_section" => ok(lib_mut(st)?.save_section(arg(a, "section")?).map_err(err)?),
        "outline.delete_section" => ok(lib_mut(st)?.delete_section(&s(a, "id")).map_err(err)?),
        "outline.move_section" => ok(lib_mut(st)?.move_section(&s(a, "id"), &s(a, "parent_id"), opt(a, "position")).map_err(err)?),
        "outline.file_note" => {
            let from = s(a, "from_section");
            ok(lib_mut(st)?.file_note(&s(a, "section_id"), &s(a, "note_id"), a.get("position").and_then(|v| v.as_u64()).map(|v| v as usize), if from.is_empty() { None } else { Some(&from) }).map_err(err)?)
        }
        "outline.unfile_note" => ok(lib_mut(st)?.unfile_note(&s(a, "section_id"), &s(a, "note_id")).map_err(err)?),
        "tasks.list" => ok(lib_mut(st)?.tasks(&s(a, "project_id")).map_err(err)?),
        "tasks.save" => ok(lib_mut(st)?.save_task(arg(a, "task")?).map_err(err)?),
        "tasks.delete" => ok(lib_mut(st)?.delete_task(&s(a, "id")).map_err(err)?),

        // ---- files and annotations ----------------------------------------------------------
        "files.list" => ok(lib_mut(st)?.attachments(&s(a, "ref_id")).map_err(err)?),
        "files.attach_pick" => {
            let l = lib_mut(st)?;
            let mode = if s(a, "mode").is_empty() { "managed".to_string() } else { s(a, "mode") };
            let mut out = vec![];
            for p in &paths {
                out.push(l.attach_file(&s(a, "ref_id"), p, &mode, &s(a, "role")).map_err(err)?);
            }
            ok(out)
        }
        "files.page_text" => ok(lib_mut(st)?.page_text(&s(a, "id")).map_err(err)?),
        "files.set_page_text" => ok(lib_mut(st)?.set_page_text(&s(a, "id"), &arg::<Vec<String>>(a, "pages")?).map_err(err)?),
        "files.unindexed" => {
            let l = lib_mut(st)?;
            let v: Vec<Attachment> = l.store.all::<Attachment>().map_err(err)?.into_iter().filter(|x| x.mime == "application/pdf" && !x.indexed && !x.index_excluded && !x.superseded).collect();
            ok(v)
        }
        "files.exclude_index" => ok(lib_mut(st)?.set_index_excluded(&s(a, "id"), opt(a, "excluded")).map_err(err)?),
        "files.set_offset" => ok(lib_mut(st)?.set_page_offset(&s(a, "id"), a.get("offset").and_then(|v| v.as_i64())).map_err(err)?),
        "files.check" => ok(lib_mut(st)?.check_files(opt(a, "rehash")).map_err(err)?),
        "files.relink_pick" => ok(lib_mut(st)?.relink(&s(a, "id"), &path.unwrap()).map_err(err)?),
        "files.relink_folder" => ok(lib_mut(st)?.relink_folder(&s(a, "old_prefix"), &s(a, "new_prefix")).map_err(err)?),
        "files.base" => {
            let l = lib_mut(st)?;
            let linked: i64 = l.store.conn.query_row("SELECT COUNT(*) FROM attachments WHERE json_extract(data,'$.mode')='linked'", [], |r| r.get(0)).unwrap_or(0);
            let relative: i64 = l.store.conn.query_row("SELECT COUNT(*) FROM attachments WHERE json_extract(data,'$.mode')='linked' AND json_extract(data,'$.path') LIKE 'base:%'", [], |r| r.get(0)).unwrap_or(0);
            let managed: i64 = l.store.conn.query_row("SELECT COUNT(*) FROM attachments WHERE json_extract(data,'$.mode')='managed'", [], |r| r.get(0)).unwrap_or(0);
            ok(json!({
                "library": l.store.meta("linked_base").unwrap_or_default(),
                "this_computer": l.linked_base_override.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                "effective": l.linked_base().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                "linked": linked, "relative": relative, "managed": managed,
            }))
        }
        "files.set_base" => {
            let folder = s(a, "folder");
            if opt::<bool>(a, "this_computer") {
                if !folder.trim().is_empty() && !Path::new(folder.trim()).is_dir() {
                    return Err(user(&format!("The folder {} was not found.", folder.trim())));
                }
                let id = lib_mut(st)?.info().map_err(err)?.id;
                if folder.trim().is_empty() {
                    st.settings.linked_bases.remove(&id);
                } else {
                    st.settings.linked_bases.insert(id, folder.trim().to_string());
                }
                let _ = settings::save(&st.config_dir, &st.settings);
                let l = lib_mut(st)?;
                l.linked_base_override = if folder.trim().is_empty() { None } else { Some(PathBuf::from(folder.trim())) };
                ok(l.make_links_relative().map_err(err)?)
            } else {
                ok(lib_mut(st)?.set_linked_base(&folder).map_err(err)?)
            }
        }
        "files.set_ocr_text" => {
            #[derive(serde::Deserialize)]
            struct P {
                text: String,
                #[serde(default)]
                words: Value,
            }
            let pages: Vec<P> = arg(a, "pages")?;
            let pages: Vec<(String, String)> = pages.into_iter().map(|p| (p.text, if p.words.is_null() { "[]".into() } else { p.words.to_string() })).collect();
            ok(lib_mut(st)?.set_ocr_text(&s(a, "id"), &pages, &s(a, "lang")).map_err(err)?)
        }
        "files.clear_ocr" => ok(lib_mut(st)?.clear_ocr_text(&s(a, "id")).map_err(err)?),
        "files.ocr_words" => ok(lib_mut(st)?.ocr_words(&s(a, "id"), opt::<u32>(a, "page")).map_err(err)?),
        "files.ocr_text_in" => {
            let rects: Vec<prm_core::model::Rect> = arg(a, "rects")?;
            ok(lib_mut(st)?.ocr_text_in(&s(a, "id"), opt::<u32>(a, "page"), &rects).map_err(err)?)
        }
        // PDFs whose text layer is missing or nearly empty and that have no recognized text.
        "files.scanned" => {
            let l = lib_mut(st)?;
            let thin: std::collections::HashSet<String> = l
                .store
                .conn
                .prepare("SELECT attachment_id FROM page_text GROUP BY attachment_id HAVING SUM(length(trim(text))) < 25 * COUNT(*)")
                .and_then(|mut q| q.query_map([], |r| r.get::<_, String>(0)).map(|it| it.filter_map(|r| r.ok()).collect()))
                .unwrap_or_default();
            let v: Vec<Attachment> = l.store.all::<Attachment>().map_err(err)?.into_iter().filter(|x| x.mime == "application/pdf" && !x.superseded && !x.index_excluded && x.text_source != "ocr" && (x.state == "image-only" || thin.contains(&x.id))).collect();
            ok(v)
        }
        "files.find_moved" => {
            let folders: Vec<String> = arg(a, "folders")?;
            let folders: Vec<PathBuf> = folders.into_iter().filter(|f| !f.trim().is_empty()).map(PathBuf::from).collect();
            ok(lib_mut(st)?.find_moved_files(&folders, 200_000).map_err(err)?)
        }
        "files.relink_many" => {
            #[derive(serde::Deserialize)]
            struct P {
                id: String,
                path: String,
            }
            let pairs: Vec<P> = arg(a, "pairs")?;
            let pairs: Vec<(String, PathBuf)> = pairs.into_iter().map(|p| (p.id, PathBuf::from(p.path))).collect();
            ok(lib_mut(st)?.relink_many(&pairs).map_err(err)?)
        }
        "files.storage" => ok(lib_mut(st)?.storage_report().map_err(err)?),
        "files.remove_duplicates" => ok(lib_mut(st)?.remove_duplicate_attachments(&s(a, "hash")).map_err(err)?),
        "files.delete_unused" => {
            if !opt::<bool>(a, "confirmed") {
                return Err(user("Deleting unused files needs confirmation."));
            }
            let paths: Vec<String> = arg(a, "paths")?;
            ok(lib_mut(st)?.delete_unused_files(&paths).map_err(err)?)
        }
        "files.make_relative" => ok(lib_mut(st)?.make_links_relative().map_err(err)?),
        "files.convert" => {
            let ids: Vec<String> = arg(a, "ids")?;
            let folder = s(a, "folder");
            let l = lib_mut(st)?;
            // An empty id list means every attachment not already in the requested mode.
            let ids = if ids.is_empty() { l.store.all::<prm_core::model::Attachment>().map_err(err)?.into_iter().map(|x| x.id).collect() } else { ids };
            ok(l.convert_storage(&ids, &s(a, "to"), if folder.is_empty() { None } else { Some(Path::new(&folder)) }).map_err(err)?)
        }
        "files.replace_pick" => ok(json!({"path": path.unwrap().to_string_lossy()})),
        "files.replace" => {
            let p = PathBuf::from(s(a, "path"));
            if !picked_paths().lock().unwrap().contains(&p) {
                return Err(user("Choose the new version in the file dialog first."));
            }
            ok(lib_mut(st)?.replace_attachment(&s(a, "id"), &p, &opt::<Vec<String>>(a, "pages")).map_err(err)?)
        }
        "files.open_external" => {
            let l = lib_mut(st)?;
            let att = l.store.must::<Attachment>(&s(a, "id")).map_err(err)?;
            open_with_shell(&l.attachment_path(&att).to_string_lossy());
            ok(true)
        }
        "files.reveal" => {
            let l = lib_mut(st)?;
            let att = l.store.must::<Attachment>(&s(a, "id")).map_err(err)?;
            let p = l.attachment_path(&att);
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{}", p.display())).spawn();
            }
            ok(true)
        }
        "files.rename" => ok(lib_mut(st)?.rename_files(&arg::<Vec<String>>(a, "ids")?, &s(a, "pattern"), opt(a, "apply")).map_err(err)?),
        "export.attachments" => {
            let l = lib_mut(st)?;
            let ids: Vec<String> = arg(a, "ids")?;
            let pat = if s(a, "pattern").is_empty() { "[auth][year][shorttitle]".to_string() } else { s(a, "pattern") };
            ok(l.export_attachments(&ids, &pat, &path.unwrap()).map_err(err)?)
        }
        "files.delete" => {
            let id = s(a, "id");
            ok(lib_mut(st)?.write("Remove attachment", |tx| {
                for an in tx.by::<Annotation>("attachment_id", &id)? {
                    tx.delete::<Annotation>(&an.id)?;
                }
                tx.delete::<Attachment>(&id)
            }).map_err(err)?)
        }
        "ann.list" => ok(lib_mut(st)?.annotations(&s(a, "attachment_id"), &s(a, "project_id")).map_err(err)?),
        "ann.save" => ok(lib_mut(st)?.save_annotation(arg(a, "annotation")?).map_err(err)?),
        "ann.delete" => ok(lib_mut(st)?.delete_annotation(&s(a, "id")).map_err(err)?),
        "ann.resolve_anchor" => ok(lib_mut(st)?.resolve_anchor(&s(a, "id"), a.get("page_index").and_then(|v| v.as_u64()).map(|v| v as u32), opt(a, "keep")).map_err(err)?),
        "ann.export_pdf" => {
            let p = path.unwrap();
            let n = lib_mut(st)?.export_annotated_pdf(&s(a, "attachment_id"), &s(a, "project_id"), &p, opt(a, "flatten")).map_err(err)?;
            ok(json!({"path": p.to_string_lossy(), "count": n}))
        }
        "ann.export_list" => {
            let (bytes, _) = lib_mut(st)?.export_annotations(&s(a, "attachment_id"), &s(a, "project_id"), &s(a, "manuscript_id"), &s(a, "format")).map_err(err)?;
            let p = path.unwrap();
            write_out(&p, &bytes)?;
            ok(json!({"path": p.to_string_lossy()}))
        }
        "ann.preview_external" => {
            let p = path.unwrap();
            let pv = lib_mut(st)?.preview_external_annotations(&s(a, "attachment_id"), &p).map_err(err)?;
            ok(json!({"preview": pv, "path": p.to_string_lossy()}))
        }
        "ann.import_external" => ok(lib_mut(st)?.import_external_annotations(&s(a, "attachment_id"), &s(a, "project_id"), &arg::<Vec<ExternalImportItem>>(a, "items")?, opt(a, "accept_conflicts")).map_err(err)?),

        // ---- imports and lookups --------------------------------------------------------------
        "import.pick" => {
            let p = path.unwrap();
            let text = read_text(&p)?;
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let fmt = prm_core::formats::detect(&name, &text);
            let headers: Vec<(String, String)> = if fmt == "csv" {
                let first = text.trim_start_matches('\u{feff}').lines().next().unwrap_or("");
                let delim = if first.matches('\t').count() > first.matches(',').count() { '\t' } else if first.matches(';').count() > first.matches(',').count() { ';' } else { ',' };
                first.split(delim).map(|h| { let h = h.trim().trim_matches('"').to_string(); let sug = prm_core::formats::csvfmt::suggest(&h).to_string(); (h, sug) }).collect()
            } else {
                vec![]
            };
            ok(json!({"name": name, "text": text, "format": fmt, "csv_headers": headers, "size": text.len()}))
        }
        "import.stage" => {
            let mapping: Option<BTreeMap<String, String>> = a.get("mapping").and_then(|m| serde_json::from_value(m.clone()).ok());
            let l = lib_mut(st)?;
            l.progress = Some(progress_emitter(app));
            let r = l.stage_import(&s(a, "name"), &s(a, "text"), &s(a, "format"), &s(a, "service"), &s(a, "project_id"), &s(a, "collection_id"), mapping.as_ref());
            l.progress = None;
            ok(r.map_err(err)?)
        }
        "import.options" => {
            let l = lib_mut(st)?;
            let mut b = l.store.must::<ImportBatch>(&s(a, "id")).map_err(err)?;
            if b.state != "staged" {
                return Err(user("This import was already committed or cancelled."));
            }
            b.make_collections = opt(a, "make_collections");
            b.label_as_groups = opt(a, "label_as_groups");
            l.save_staged(&b).map_err(err)?;
            ok(b)
        }
        "import.batches" => ok(lib_mut(st)?.import_batches().map_err(err)?),
        "import.get" => ok(lib_mut(st)?.store.must::<ImportBatch>(&s(a, "id")).map_err(err)?),
        "import.set" => {
            let rec: Option<Reference> = a.get("record").and_then(|r| serde_json::from_value(r.clone()).ok());
            let d = s(a, "decision");
            ok(lib_mut(st)?.set_staged(&s(a, "id"), opt(a, "index"), if d.is_empty() { None } else { Some(&d) }, rec).map_err(err)?)
        }
        "import.set_all" => {
            let l = lib_mut(st)?;
            let id = s(a, "id");
            let d = s(a, "decision");
            let status = s(a, "status");
            let b = l.store.must::<ImportBatch>(&id).map_err(err)?;
            let mut last = Value::Null;
            for r in b.records.iter().filter(|r| status.is_empty() || r.status == status) {
                last = ok(l.set_staged(&id, r.index, Some(&d), None).map_err(err)?)?;
            }
            ok(last)
        }
        "import.commit" => {
            let l = lib_mut(st)?;
            let id = s(a, "id");
            l.progress = Some(progress_emitter(app));
            let report = l.commit_import(&id);
            l.progress = None;
            let report = report.map_err(err)?;
            // Attach PDFs (chosen in a dialog, or captured into the inbox), labels and notes.
            let b = l.store.must::<ImportBatch>(&id).map_err(err)?;
            let lib_root = l.root.to_string_lossy().to_string();
            let watched: Vec<PathBuf> = st.settings.watched_folders.iter().filter(|w| w.library == lib_root).map(|w| PathBuf::from(&w.path)).collect();
            let l = lib_mut(st)?;
            let attached = crate::capture::after_commit(l, &b, &|p: &Path| picked_paths().lock().unwrap().contains(p) || watched.iter().any(|w| p.starts_with(w)))?;
            let placed = crate::pdfimport::mirror_folders(l, &b)?;
            // New sources may teach journal abbreviations.
            if let Ok(refs) = l.store.all::<Reference>() {
                let _ = prm_core::journals::learn(&l.store.conn, &refs);
            }
            ok(json!({"report": report, "attached": attached, "collected": placed}))
        }
        "import.cancel" => ok(lib_mut(st)?.cancel_import(&s(a, "id")).map_err(err)?),
        "cleanup.propose" if s(a, "kind") == "journals" => ok(lib_mut(st)?.propose_journal_names(&opt::<Vec<String>>(a, "ids")).map_err(err)?),
        "cleanup.propose" => ok(lib_mut(st)?.propose_title_case(&opt::<Vec<String>>(a, "ids")).map_err(err)?),
        "journals.summary" => {
            let l = lib_mut(st)?;
            let (records, imported) = prm_core::journals::count(&l.store.conn);
            ok(json!({"records": records, "imported": imported, "default": l.store.meta("journal_names").unwrap_or_default()}))
        }
        "journals.abbrev" => ok(prm_core::journals::abbreviation(&lib_mut(st)?.store.conn, &s(a, "full"))),
        "journals.list" => ok(prm_core::journals::list(&lib_mut(st)?.store.conn, &s(a, "query"), 200).map_err(err)?),
        "journals.learn" => {
            let l = lib_mut(st)?;
            let refs: Vec<Reference> = l.store.all().map_err(err)?;
            ok(prm_core::journals::learn(&l.store.conn, &refs).map_err(err)?)
        }
        "journals.import_pick" => {
            let text = std::fs::read_to_string(path.unwrap()).map_err(|_| user("That file could not be read as text."))?;
            ok(prm_core::journals::import_list(&lib_mut(st)?.store.conn, &text).map_err(err)?)
        }
        "journals.import_text" => ok(prm_core::journals::import_list(&lib_mut(st)?.store.conn, &s(a, "text")).map_err(err)?),
        "journals.clear_imported" => ok(prm_core::journals::clear_imported(&lib_mut(st)?.store.conn).map_err(err)?),
        "journals.set_default" => {
            let f = s(a, "form");
            if !prm_core::journals::FORMS.contains(&f.as_str()) {
                return Err(user("Journal names can follow the style, be full or be abbreviated."));
            }
            let l = lib_mut(st)?;
            l.store.set_meta("journal_names", &f).map_err(err)?;
            let _ = l.recompute_copies(None);
            ok(true)
        }
        "cleanup.apply" => {
            let ps: Vec<prm_core::cleanup::Proposal> = arg(a, "proposals")?;
            let label = s(a, "label");
            ok(lib_mut(st)?.apply_proposals(&ps, if label.is_empty() { "Apply reviewed fixes" } else { &label }).map_err(err)?)
        }
        "links.set_accessed" => ok(lib_mut(st)?.set_accessed_today(&opt::<Vec<String>>(a, "ids")).map_err(err)?),
        "watch.list" => {
            let lib_root = st.lib.as_ref().map(|l| l.root.to_string_lossy().to_string()).unwrap_or_default();
            ok(st.settings.watched_folders.iter().filter(|w| w.library == lib_root).cloned().collect::<Vec<_>>())
        }
        "watch.remove" => {
            let path = s(a, "path");
            let lib_root = st.lib.as_ref().map(|l| l.root.to_string_lossy().to_string()).unwrap_or_default();
            st.settings.watched_folders.retain(|w| !(w.path == path && w.library == lib_root));
            let _ = settings::save(&st.config_dir, &st.settings);
            ok(true)
        }
        "import.undo_preview" => ok(lib_mut(st)?.undo_import_preview(&s(a, "id")).map_err(err)?),
        "import.undo" => ok(lib_mut(st)?.undo_import(&s(a, "id"), opt(a, "include_dependent")).map_err(err)?),
        "import.scan" => ok(lib_mut(st)?.migration_scan(&s(a, "name"), &s(a, "text")).map_err(err)?),
        "lookup.resolve" => {
            let kind = s(a, "kind");
            let inv_kind = match kind.as_str() { "pmid" | "pmcid" => "pubmed", "url" => "webpage", k => k }.to_string();
            network_allowed(st, &inv_kind, &s(a, "project_id"))?;
            let l = lib_mut(st)?;
            let store = &l.store;
            let res = prm_core::lookup::resolve(&kind, &s(a, "id"), &mut |k, h, sent, okk, d| store.log_network(k, h, "Identifier lookup", sent, okk, d));
            ok(res)
        }
        "lookup.stage" => {
            // Stage looked-up candidates through the same review pipeline.
            let refs: Vec<Reference> = arg(a, "refs")?;
            let items: Vec<Value> = refs.iter().map(prm_core::formats::csljson::to_csl).collect();
            let l = lib_mut(st)?;
            let mut b = l.stage_import(&s(a, "name"), &serde_json::to_string(&items).unwrap_or_default(), "csljson", &s(a, "service"), &s(a, "project_id"), "", None).map_err(err)?;
            // Keep each value's source and confidence from the lookup (the exchange format has none).
            for sr in b.records.iter_mut() {
                if let Some(src) = refs.get(sr.index) {
                    sr.record.provenance = src.provenance.clone();
                    prm_core::provenance::fill(&mut sr.record, "lookup:other", None, &prm_core::util::now());
                }
            }
            l.save_staged(&b).map_err(err)?;
            ok(b)
        }
        "lookup.identifiers" => ok(prm_core::formats::paste::identifiers(&s(a, "text"))),
        "capture.options" => {
            let active = st.settings.active_project.clone();
            let l = lib_mut(st)?;
            let projects: Vec<Value> = l.store.all::<Project>().map_err(err)?.into_iter().filter(|p| !p.archived).map(|p| json!({"id": p.id, "title": p.title})).collect();
            let collections: Vec<Value> = l.store.all::<Collection>().map_err(err)?.into_iter().filter(|c| c.query.is_empty()).map(|c| json!({"id": c.id, "name": c.name, "project_id": c.project_id})).collect();
            let labels: Vec<String> = l.store.all::<Label>().map_err(err)?.into_iter().map(|x| x.name).collect();
            ok(json!({"library": l.store.meta("name"), "projects": projects, "active_project": active, "collections": collections, "labels": labels}))
        }
        "capture.check" => ok(crate::capture::check(lib_mut(st)?, &s(a, "doi"), &s(a, "title"))?),
        "capture.stage" => {
            let req: crate::capture::CaptureRequest = serde_json::from_value(a.clone()).map_err(|e| CallError { message: "The capture request was not valid.".into(), detail: e.to_string() })?;
            let net = network_allowed(st, "doi", &req.project_id).is_ok();
            let l = lib_mut(st)?;
            let b = crate::capture::stage(l, &req, |lib, kind, id| {
                if !net {
                    return None;
                }
                let store = &lib.store;
                prm_core::lookup::resolve(kind, id, &mut |k, h, sent, okk, d| store.log_network(k, h, "Browser capture lookup", sent, okk, d)).candidate
            })?;
            let first = b.records.first().map(|r| r.record.fields.get("title").cloned().unwrap_or_default()).unwrap_or_default();
            let _ = app.emit("capture", json!({"batch_id": b.id, "count": b.records.len(), "title": first}));
            ok(json!({"batch_id": b.id, "count": b.records.len(), "statuses": b.records.iter().map(|r| r.status.clone()).collect::<Vec<_>>()}))
        }
        "capture.extension_folder" => {
            let dir = app.path().resource_dir().map(|d| d.join("extension")).unwrap_or_default();
            let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("extension");
            let target = if dir.join("manifest.json").exists() { dir } else { dev };
            if !target.join("manifest.json").exists() {
                return Err(user("The browser extension folder was not found in the install folder."));
            }
            if s(a, "open") == "1" {
                open_with_shell(&target.to_string_lossy());
            }
            ok(json!({"path": target.to_string_lossy()}))
        }
        "network.log" => {
            let l = lib_mut(st)?;
            let mut stmt = l.store.conn.prepare("SELECT time,kind,host,purpose,sent,ok,detail FROM network_log ORDER BY seq DESC LIMIT 300").map_err(|e| err(prm_core::Error::Db(e)))?;
            let rows: Vec<Value> = stmt
                .query_map([], |r| Ok(json!({"time": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "host": r.get::<_, String>(2)?, "purpose": r.get::<_, String>(3)?, "sent": r.get::<_, String>(4)?, "ok": r.get::<_, i64>(5)? == 1, "detail": r.get::<_, String>(6)?})))
                .map_err(|e| err(prm_core::Error::Db(e)))?
                .filter_map(|r| r.ok())
                .collect();
            ok(rows)
        }
        "integrations.runs" => ok(lib_mut(st)?.store.all::<IntegrationRun>().map_err(err)?),

        // ---- exports, backups, health --------------------------------------------------------
        "export.refs" => {
            let l = lib_mut(st)?;
            let ids: Vec<String> = opt(a, "ids");
            let refs: Vec<Reference> = if ids.is_empty() { prm_core::store::list_in(&l.store.conn, "WHERE trashed IS NULL", &[]).map_err(err)? } else { l.references(&ids).map_err(err)? };
            let rr: Vec<&Reference> = refs.iter().collect();
            let exclude: Vec<String> = opt(a, "exclude_fields");
            let text = match s(a, "format").as_str() {
                "ris" => prm_core::formats::ris::export(&rr),
                "bibtex" => prm_core::formats::bibtex::export(&rr, false, &exclude),
                "biblatex" => prm_core::formats::bibtex::export(&rr, true, &exclude),
                "csljson" => prm_core::formats::csljson::export(&rr),
                "csv" => prm_core::formats::csvfmt::export(&rr).map_err(err)?,
                "xml" => prm_core::formats::xmlrecords::export(&rr),
                "xlsx" => {
                    let bytes = prm_core::formats::csvfmt::export_xlsx(&rr).map_err(err)?;
                    let p = path.unwrap();
                    write_out(&p, &bytes)?;
                    return ok(json!({"path": p.to_string_lossy(), "count": refs.len()}));
                }
                _ => return Err(user("Choose an export format.")),
            };
            let p = path.unwrap();
            write_out(&p, text.as_bytes())?;
            ok(json!({"path": p.to_string_lossy(), "count": refs.len()}))
        }
        "export.copy" => {
            let l = lib_mut(st)?;
            let ids: Vec<String> = arg(a, "ids")?;
            let refs = l.references(&ids).map_err(err)?;
            let rr: Vec<&Reference> = refs.iter().collect();
            let text = match s(a, "format").as_str() {
                "ris" => prm_core::formats::ris::export(&rr),
                "bibtex" => prm_core::formats::bibtex::export(&rr, false, &[]),
                "biblatex" => prm_core::formats::bibtex::export(&rr, true, &[]),
                "csljson" => prm_core::formats::csljson::export(&rr),
                _ => return Err(user("Choose a format to copy.")),
            };
            crate::clipboard::write(&text, None, None).map_err(|e| user(&format!("The clipboard could not be written: {e}")))?;
            ok(json!({"count": refs.len(), "chars": text.len()}))
        }
        "export.package" => {
            let p = path.unwrap();
            let m = prm_core::backup::export_package(lib_mut(st)?, &p, opt(a, "include_files")).map_err(err)?;
            ok(json!({"path": p.to_string_lossy(), "manifest": m}))
        }
        "package.import" => {
            let target = paths[0].clone();
            let m = prm_core::backup::import_package(&path.unwrap(), &target).map_err(err)?;
            ok(json!({"path": target.to_string_lossy(), "manifest": m}))
        }
        "backup.now" => {
            let p = path.unwrap();
            let l = lib_mut(st)?;
            let m = prm_core::backup::backup(l, &p, opt(a, "include_files")).map_err(err)?;
            let v = prm_core::backup::verify(&p).map_err(err)?;
            st.settings.last_backup = prm_core::util::now();
            let _ = settings::save(&st.config_dir, &st.settings);
            ok(json!({"path": p.to_string_lossy(), "manifest": m, "verify": v}))
        }
        "backup.verify" => ok(prm_core::backup::verify(&path.unwrap()).map_err(err)?),
        "backup.restore" => {
            let target = paths[0].clone();
            let rep = prm_core::backup::restore(&path.unwrap(), &target).map_err(err)?;
            ok(json!({"path": target.to_string_lossy(), "report": rep}))
        }
        "backup.list" => {
            let bd = st.settings.backup_dir.clone();
            let l = lib_mut(st)?;
            let dir = if bd.is_empty() { l.root.join("backups") } else { PathBuf::from(&bd) };
            let mut v: Vec<Value> = std::fs::read_dir(&dir)
                .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().extension().map(|x| x == "zip" || x == "db").unwrap_or(false)).map(|e| json!({"path": e.path().to_string_lossy(), "name": e.file_name().to_string_lossy(), "size": e.metadata().map(|m| m.len()).unwrap_or(0)})).collect())
                .unwrap_or_default();
            v.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
            ok(json!({"dir": dir.to_string_lossy(), "backups": v}))
        }
        "health.run" => ok(prm_core::health::run(lib_mut(st)?).map_err(err)?),
        "mirror.scan" => {
            let l = lib_mut(st)?;
            let _ = l.store.advance_mirror();
            let scan = l.scan_mirror().map_err(err)?;
            ok(json!({"dir": l.store.mirror_dir().to_string_lossy(), "scan": scan}))
        }
        "mirror.apply" => {
            let apply: Vec<String> = arg(a, "apply")?;
            let decline: Vec<String> = arg(a, "decline")?;
            ok(lib_mut(st)?.apply_mirror(&apply, &decline).map_err(err)?)
        }
        // Rebuild a library from the mirror alone in a temporary folder and compare it with this
        // one, record by record.
        "mirror.verify" => {
            let l = lib_mut(st)?;
            l.store.advance_mirror().map_err(err)?;
            let tmp = std::env::temp_dir().join(format!("prm-mirror-check-{}", prm_core::util::new_id()));
            let res = prm_core::mirror::reconstruct(&l.store.mirror_dir(), &tmp).and_then(|s| {
                let (a, b) = (l.store.canonical_dump()?, s.canonical_dump()?);
                let mut differ = vec![];
                for (t, rows) in &a {
                    let other = b.get(t).cloned().unwrap_or_default();
                    if rows != &other {
                        differ.push(json!({"table": t, "library": rows.len(), "mirror": other.len()}));
                    }
                }
                let records: usize = a.values().map(|v| v.len()).sum();
                Ok(json!({"ok": differ.is_empty(), "records": records, "differ": differ}))
            });
            let _ = std::fs::remove_dir_all(&tmp);
            ok(res.map_err(err)?)
        }
        "diagnostics.preview" => ok(diagnostics(st, opt(a, "expanded"))),
        "diagnostics.save" => {
            let d = diagnostics(st, opt(a, "expanded"));
            let p = path.unwrap();
            write_out(&p, serde_json::to_string_pretty(&d).unwrap_or_default().as_bytes())?;
            ok(json!({"path": p.to_string_lossy()}))
        }

        #[cfg(debug_assertions)]
        "debug.log" => {
            use std::io::Write;
            if let Ok(p) = std::env::var("PRM_TEST_LOG") {
                for _ in 0..20 {
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
                        let _ = writeln!(f, "{}", a.get("line").and_then(|v| v.as_str()).unwrap_or(""));
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            ok(true)
        }
        #[cfg(debug_assertions)]
        "debug.attach" => {
            let p = PathBuf::from(s(a, "path"));
            remember(&p);
            let mode = if s(a, "mode") == "linked" { "linked" } else { "managed" };
            ok(lib_mut(st)?.attach_file(&s(a, "ref_id"), &p, mode, "full-text").map_err(err)?)
        }
        #[cfg(debug_assertions)]
        "debug.rename" => {
            std::fs::create_dir_all(Path::new(&s(a, "to")).parent().unwrap_or(Path::new("."))).map_err(|e| user(&e.to_string()))?;
            std::fs::rename(s(a, "from"), s(a, "to")).map_err(|e| user(&e.to_string()))?;
            ok(true)
        }
        #[cfg(debug_assertions)]
        "debug.exit" => {
            if let Some(l) = st.lib.as_ref() {
                l.close();
            }
            std::process::exit(0);
        }

        // ---- local API administration ---------------------------------------------------------
        "api.state" => ok(json!({"enabled": st.settings.api_enabled, "port": st.settings.api_port, "clients": st.settings.api_clients, "pending": st.pending_pairs, "log": st.api_log.iter().rev().take(200).collect::<Vec<_>>()})),
        "api.approve" => {
            let id = s(a, "id");
            let Some(pos) = st.pending_pairs.iter().position(|p| p.id == id) else { return Err(user("That request is no longer waiting.")) };
            let mut req = st.pending_pairs.remove(pos);
            let token = format!("prm_{}", prm_core::util::new_id().replace('-', ""));
            st.settings.api_clients.push(settings::ApiClient { id: req.id.clone(), name: req.name.clone(), token_hash: prm_core::util::sha256_hex(token.as_bytes()), scopes: req.scopes.clone(), created: prm_core::util::now(), last_used: String::new() });
            req.token = Some(token);
            req.state = "approved".into();
            st.pending_pairs.push(req);
            let _ = settings::save(&st.config_dir, &st.settings);
            ok(true)
        }
        "api.deny" => {
            let id = s(a, "id");
            for p in st.pending_pairs.iter_mut().filter(|p| p.id == id) {
                p.state = "denied".into();
            }
            ok(true)
        }
        "api.revoke" => {
            let id = s(a, "id");
            st.settings.api_clients.retain(|c| c.id != id);
            let _ = settings::save(&st.config_dir, &st.settings);
            ok(true)
        }
        _ => {
            let _ = app;
            Err(user(&format!("Unknown action “{method}”.")))
        }
    }
}

fn diagnostics(st: &crate::AppState, expanded: bool) -> Value {
    // Redacted by default: no library content, titles, notes, tokens or full paths.
    let redact = |p: &str| -> String {
        if expanded {
            p.to_string()
        } else {
            let user = std::env::var("USERNAME").unwrap_or_default();
            if user.is_empty() { p.to_string() } else { p.replace(&user, "<user>") }
        }
    };
    let lib = st.lib.as_ref().and_then(|l| l.info().ok());
    json!({
        "program": format!("Prism Reference Manager {}", env!("CARGO_PKG_VERSION")),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "portable": settings::portable(),
        "policy_file": redact(&st.policy.source),
        "library": lib.map(|i| json!({"schema": i.schema, "read_only": i.read_only, "counts": i.counts, "sync_folder_warning": i.sync_folder_warning, "root": redact(&i.root)})),
        "settings": {
            "theme": st.settings.theme, "network_enabled": st.settings.network_enabled, "api_enabled": st.settings.api_enabled,
            "auto_backup": st.settings.auto_backup, "api_clients": st.settings.api_clients.len(),
        },
        "jobs": st.jobs.iter().rev().take(20).map(|j| json!({"kind": j["kind"], "ok": j["ok"]})).collect::<Vec<_>>(),
        "note": "This bundle excludes library content, titles, notes, quotations, document text and tokens.",
    })
}
