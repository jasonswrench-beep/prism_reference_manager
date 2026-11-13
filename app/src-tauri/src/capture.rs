//! Browser capture: items sent by the capture extension over the local connection are staged
//! for review like any other import. A captured PDF waits in the library's `inbox` folder and
//! is attached when the person commits the record; labels, a collection and a note are
//! applied at the same time.

use crate::CallError;
use base64::Engine;
use prm_core::library::Library;
use prm_core::model::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_PDF_BYTES: usize = 200 * 1024 * 1024;

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct CaptureItem {
    /// CSL JSON built from the page's citation metadata, if any.
    pub csl: Option<Value>,
    /// Identifiers found on the page (doi, pmid, pmcid, arxiv, isbn).
    pub identifiers: std::collections::BTreeMap<String, String>,
    pub url: String,
    pub title: String,
    pub pdf_base64: String,
    pub pdf_name: String,
    pub pdf_url: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct CaptureRequest {
    pub items: Vec<CaptureItem>,
    pub project_id: String,
    pub collection_id: String,
    pub labels: Vec<String>,
    pub note: String,
    pub page_url: String,
}

fn err(m: &str) -> CallError {
    CallError { message: m.into(), detail: String::new() }
}

pub fn inbox_dir(lib: &Library) -> PathBuf {
    lib.root.join("inbox")
}

/// True when `p` is a file this program wrote into the library's capture inbox.
pub fn in_inbox(lib: &Library, p: &Path) -> bool {
    let inbox = inbox_dir(lib);
    match (p.canonicalize(), inbox.canonicalize()) {
        (Ok(a), Ok(b)) => a.starts_with(b),
        _ => false,
    }
}

fn save_pdf(lib: &Library, b64: &str, name: &str) -> Result<PathBuf, CallError> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| err("The PDF sent by the browser could not be read."))?;
    if bytes.len() > MAX_PDF_BYTES {
        return Err(err("The PDF is larger than 200 MB; download it and attach it in the program instead."));
    }
    if !bytes.starts_with(b"%PDF") {
        return Err(err("The file the browser fetched is not a PDF (the site may have returned a sign-in page)."));
    }
    let hash = prm_core::util::sha256_hex(&bytes);
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || "-_. ".contains(c) { c } else { '_' }).take(80).collect();
    let safe = if safe.trim().is_empty() { "capture.pdf".to_string() } else if safe.to_lowercase().ends_with(".pdf") { safe } else { format!("{safe}.pdf") };
    let dir = inbox_dir(lib);
    std::fs::create_dir_all(&dir).map_err(|e| CallError { message: "The capture inbox folder could not be created.".into(), detail: e.to_string() })?;
    let p = dir.join(format!("{}-{}", &hash[..16], safe));
    prm_core::util::write_atomic(&p, &bytes).map_err(|e| CallError { message: "The PDF could not be saved.".into(), detail: e.to_string() })?;
    Ok(p)
}

fn csl_record(v: &Value) -> Option<Reference> {
    let text = serde_json::to_string(&json!([v])).ok()?;
    prm_core::formats::parse("csljson", &text, None).records.into_iter().next().map(|p| p.record)
}

/// Stage captured items for review. `lookup` resolves an identifier when network use is allowed.
pub fn stage(lib: &mut Library, req: &CaptureRequest, mut lookup: impl FnMut(&Library, &str, &str) -> Option<Reference>) -> Result<ImportBatch, CallError> {
    if req.items.is_empty() {
        return Err(err("Nothing was selected to save."));
    }
    if req.items.len() > 200 {
        return Err(err("Save at most 200 items at a time."));
    }
    let mut records: Vec<(Reference, Option<PathBuf>, Vec<String>)> = Vec::new();
    for it in &req.items {
        let mut issues = Vec::new();
        let pdf = if it.pdf_base64.is_empty() { None } else { Some(save_pdf(lib, &it.pdf_base64, if it.pdf_name.is_empty() { "capture.pdf" } else { &it.pdf_name })?) };
        let mut r = it.csl.as_ref().and_then(csl_record).unwrap_or_default();
        // Fill gaps from an identifier when the page gave little metadata.
        let thin = r.title().trim().is_empty() || r.creators.is_empty();
        if thin {
            let id = it.identifiers.iter().find(|(k, v)| !v.is_empty() && ["doi", "pmid", "pmcid", "arxiv", "isbn"].contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone()));
            let id = id.or_else(|| r.fields.get("doi").map(|d| ("doi".to_string(), d.clone())));
            if let Some((k, v)) = id {
                match lookup(lib, &k, &v) {
                    Some(mut found) => {
                        for (fk, fv) in std::mem::take(&mut r.fields) {
                            found.fields.entry(fk).or_insert(fv);
                        }
                        r = found;
                    }
                    None => issues.push(format!("The {} could not be looked up (network use may be off); review the details.", k.to_uppercase())),
                }
            }
        }
        // A PDF with no page metadata: use what the file itself says.
        if r.title().trim().is_empty() {
            if let Some(p) = &pdf {
                let info = prm_core::pdf::inspect(p);
                if let Some(d) = info.dois.first() {
                    if let Some(found) = lookup(lib, "doi", d) {
                        r = found;
                    } else {
                        r.fields.insert("doi".into(), d.clone());
                    }
                }
                if r.title().trim().is_empty() && !info.title.trim().is_empty() {
                    r.fields.insert("title".into(), info.title.clone());
                }
            }
        }
        if r.title().trim().is_empty() && !it.title.trim().is_empty() {
            r.fields.insert("title".into(), it.title.trim().to_string());
            issues.push("Only the page title was found; check every field.".into());
        }
        if r.rtype.is_empty() || r.rtype == "document" {
            r.rtype = if r.has("container_title") { "article-journal".into() } else { "webpage".into() };
        }
        if !it.url.is_empty() && !r.has("url") && !r.has("doi") {
            r.fields.insert("url".into(), it.url.clone());
        }
        if r.rtype == "webpage" && r.accessed.is_none() {
            let t = chrono::Utc::now();
            r.accessed = Some(PartialDate::ymd(t.format("%Y").to_string().parse().unwrap_or(2026), t.format("%m").to_string().parse().unwrap_or(1), t.format("%d").to_string().parse().unwrap_or(1)));
        }
        records.push((r, pdf, issues));
    }
    let items: Vec<Value> = records.iter().map(|(r, _, _)| prm_core::formats::csljson::to_csl(r)).collect();
    let text = serde_json::to_string(&items).unwrap_or_default();
    let name = if req.page_url.is_empty() { "Browser capture".to_string() } else { format!("Browser capture: {}", req.page_url.chars().take(120).collect::<String>()) };
    let mut b = lib.stage_import(&name, &text, "csljson", "browser", &req.project_id, &req.collection_id, None).map_err(crate::dispatch::err)?;
    for (i, sr) in b.records.iter_mut().enumerate() {
        let Some((_, pdf, issues)) = records.get(i) else { continue };
        if let Some(p) = pdf {
            sr.record.overflow.insert("pdf:path".into(), p.to_string_lossy().to_string());
        }
        if let Some(u) = req.items.get(i).map(|x| x.url.clone()).filter(|u| !u.is_empty()) {
            sr.record.overflow.insert("capture:url".into(), u);
        }
        if !req.note.trim().is_empty() {
            sr.record.overflow.insert("capture:note".into(), req.note.trim().to_string());
        }
        if !req.labels.is_empty() {
            sr.record.overflow.insert("capture:labels".into(), req.labels.join("; "));
        }
        sr.issues.extend(issues.iter().cloned());
        if sr.status == "new" && !issues.is_empty() {
            sr.status = "unresolved".into();
        }
    }
    lib.save_staged(&b).map_err(crate::dispatch::err)?;
    Ok(b)
}

/// After a batch is committed: attach captured PDFs, apply labels and add the note.
pub fn after_commit(lib: &mut Library, batch: &ImportBatch, allowed: &dyn Fn(&Path) -> bool) -> Result<usize, CallError> {
    let mut attached = 0;
    for r in &batch.records {
        if r.committed_id.is_empty() {
            continue;
        }
        if let Some(p) = r.record.overflow.get("pdf:path") {
            let pb = PathBuf::from(p);
            let from_inbox = in_inbox(lib, &pb);
            if (allowed(&pb) || from_inbox) && lib.attach_file(&r.committed_id, &pb, "managed", "full-text").is_ok() {
                attached += 1;
                if from_inbox {
                    let _ = std::fs::remove_file(&pb);
                }
            }
        }
        if let Some(names) = r.record.overflow.get("capture:labels") {
            let existing: Vec<Label> = lib.store.all().map_err(crate::dispatch::err)?;
            for name in names.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                let id = match existing.iter().find(|l| l.name.eq_ignore_ascii_case(name)) {
                    Some(l) => l.id.clone(),
                    None => lib.save_label(Label { id: String::new(), name: name.into(), color: "#1554d1".into() }).map_err(crate::dispatch::err)?.id,
                };
                lib.set_labels(&[r.committed_id.clone()], &id, true).map_err(crate::dispatch::err)?;
            }
        }
        if let Some(note) = r.record.overflow.get("capture:note") {
            let already = lib.notes_for_ref(&r.committed_id, &batch.destination_project).map_err(crate::dispatch::err)?.iter().any(|n| n.body == *note);
            if !already {
                lib.save_note(Note { kind: "comment".into(), project_id: batch.destination_project.clone(), body: note.clone(), ref_ids: vec![r.committed_id.clone()], origin: "user".into(), origin_detail: "browser capture".into(), ..Default::default() }).map_err(crate::dispatch::err)?;
            }
        }
    }
    Ok(attached)
}

/// Is a source already in the library? Checks the DOI first, then the title.
pub fn check(lib: &Library, doi: &str, title: &str) -> Result<Value, CallError> {
    if let Some(d) = prm_core::util::normalize_doi(doi) {
        let found: Vec<Reference> = lib.store.by("doi", &d).map_err(crate::dispatch::err)?;
        if let Some(r) = found.into_iter().find(|r| r.trashed_at.is_none()) {
            return Ok(json!({"found": true, "by": "DOI", "id": r.id, "title": r.title()}));
        }
    }
    let t = title.trim();
    if t.chars().count() >= 12 {
        let q = format!("title:\"{}\"", t.replace('"', " "));
        if let Ok(res) = prm_core::search::run(&lib.store.conn, &q, None, None, 20) {
            let key = prm_core::util::sort_key(t);
            for id in res.ref_ids {
                if let Ok(r) = lib.reference(&id) {
                    if prm_core::util::sort_key(r.title()) == key {
                        return Ok(json!({"found": true, "by": "title", "id": r.id, "title": r.title()}));
                    }
                }
            }
        }
    }
    Ok(json!({"found": false}))
}
