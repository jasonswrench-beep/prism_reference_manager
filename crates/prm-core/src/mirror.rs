//! The text mirror as an open copy of the library (spec DAT-004, DAT-005).
//!
//! The program writes the mirror and records a hash of each file it writes. A file whose content
//! no longer matches was changed outside the program. Such changes are never taken in silently:
//! `scan` validates each one against the entity's type and shows it field by field, and only the
//! changes the person selects are applied, as one undoable step. A change the person declines is
//! replaced by the library's version. When the program has to rewrite a file that was changed
//! outside, the outside version is kept under `mirror/_external` until it is reviewed.
//!
//! `reconstruct` builds a library from the mirror alone; its entities and relationships equal the
//! original's (search caches and the event history are rebuilt, not compared).

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::*;
use crate::store::{Store, TABLES};
use crate::util::{canonical_json, sha256_hex};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Tables whose records a person may sensibly edit in the mirror. Others (import batches, copy
/// events, snapshots and similar records the program keeps for itself) are shown but can only
/// be put back.
pub const EDITABLE: &[&str] = &["refs", "notes", "collections", "labels", "projects", "project_sources", "manuscripts", "annotations", "tasks", "outline_sections", "saved_searches", "custom_fields", "custom_types", "aliases"];

#[derive(Debug, Clone, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MirrorChange {
    /// Stable key for selecting this change: "<table>/<id>/<kind>".
    pub key: String,
    pub table: String,
    pub id: String,
    /// The file that changed, inside the mirror folder.
    pub file: String,
    /// edited, added, deleted
    pub kind: String,
    /// A short description of the record, for example a source's title.
    pub label: String,
    pub fields: Vec<FieldChange>,
    /// Empty when the change can be applied; otherwise why not.
    pub problem: String,
    /// The program also changed this record after the outside edit; its version is current.
    pub superseded: bool,
    #[serde(skip)]
    after: Option<Value>,
    #[serde(skip)]
    source: PathBuf,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MirrorScan {
    pub changes: Vec<MirrorChange>,
    pub files_checked: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ApplyReport {
    pub applied: usize,
    pub declined: usize,
    pub failed: Vec<(String, String)>,
}

/// Deserialize into the entity's own type and back: unknown or mistyped members are rejected,
/// missing optional members take their defaults.
pub fn validate(table: &str, v: &Value) -> std::result::Result<Value, String> {
    macro_rules! check {
        ($($t:literal => $ty:ty),* $(,)?) => {
            match table {
                $($t => serde_json::from_value::<$ty>(v.clone()).map_err(|e| e.to_string()).and_then(|x| serde_json::to_value(x).map_err(|e| e.to_string())),)*
                _ => Err(format!("{table} is not a table of this library")),
            }
        };
    }
    check!(
        "refs" => Reference, "aliases" => Alias, "projects" => Project, "project_sources" => ProjectSource,
        "manuscripts" => Manuscript, "manuscript_sources" => ManuscriptSource, "copy_events" => CopyEvent,
        "snapshots" => Snapshot, "collections" => Collection, "labels" => Label, "saved_searches" => SavedSearch,
        "duplicate_decisions" => DuplicateDecision, "tasks" => Task, "attachments" => Attachment,
        "annotations" => Annotation, "notes" => Note, "outline_sections" => OutlineSection,
        "import_batches" => ImportBatch, "styles" => StylePackage, "integration_runs" => IntegrationRun,
        "custom_fields" => CustomFieldDef, "custom_types" => CustomType,
    )
}

fn short(v: &Value) -> String {
    let s = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => canonical_json(other),
    };
    if s.chars().count() > 300 { format!("{}…", s.chars().take(300).collect::<String>()) } else { s }
}

/// Field-level differences, one level into objects (so `fields.title`, not the whole `fields`).
fn diff(before: &Value, after: &Value) -> Vec<FieldChange> {
    let empty = serde_json::Map::new();
    let (a, b) = (before.as_object().unwrap_or(&empty), after.as_object().unwrap_or(&empty));
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut out = vec![];
    for k in keys {
        let (x, y) = (a.get(k).unwrap_or(&Value::Null), b.get(k).unwrap_or(&Value::Null));
        if x == y || k == "modified" || k == "revision" {
            continue;
        }
        match (x, y) {
            (Value::Object(xo), Value::Object(yo)) => {
                let mut sub: Vec<&String> = xo.keys().chain(yo.keys()).collect();
                sub.sort();
                sub.dedup();
                for s in sub {
                    let (p, q) = (xo.get(s).unwrap_or(&Value::Null), yo.get(s).unwrap_or(&Value::Null));
                    if p != q {
                        out.push(FieldChange { field: format!("{k}.{s}"), before: short(p), after: short(q) });
                    }
                }
            }
            _ => out.push(FieldChange { field: k.clone(), before: short(x), after: short(y) }),
        }
    }
    out
}

fn label_of(table: &str, v: &Value) -> String {
    let g = |p: &str| v.pointer(p).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let l = match table {
        "refs" => g("/fields/title"),
        "notes" | "projects" | "manuscripts" | "tasks" | "outline_sections" => {
            let t = g("/title");
            if t.is_empty() { g("/body").chars().take(60).collect() } else { t }
        }
        "annotations" => g("/text").chars().take(60).collect(),
        _ => g("/name"),
    };
    if l.is_empty() { v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string() } else { l }
}

/// Read a note's Markdown back: title (`# ...`), quotation (`> ` lines) and body.
fn note_from_markdown(current: &Value, md: &str) -> std::result::Result<Value, String> {
    let rest = md.strip_prefix("---\n").and_then(|r| r.split_once("\n---\n")).map(|(_, b)| b).ok_or("The note's header (between the --- lines) is missing or damaged.")?;
    let mut lines = rest.trim_start_matches('\n').lines().peekable();
    let mut title = String::new();
    if let Some(l) = lines.peek() {
        if let Some(t) = l.strip_prefix("# ") {
            title = t.trim().to_string();
            lines.next();
            while lines.peek() == Some(&"") {
                lines.next();
            }
        }
    }
    let mut quote = vec![];
    while let Some(l) = lines.peek() {
        if let Some(q) = l.strip_prefix("> ").or_else(|| if *l == ">" { Some("") } else { None }) {
            quote.push(q.to_string());
            lines.next();
        } else {
            break;
        }
    }
    if !quote.is_empty() {
        while lines.peek() == Some(&"") {
            lines.next();
        }
    }
    let body = lines.collect::<Vec<_>>().join("\n");
    let mut v = current.clone();
    v["title"] = Value::String(title);
    v["exact_text"] = Value::String(quote.join("\n"));
    v["body"] = Value::String(body.trim_end_matches('\n').to_string());
    Ok(v)
}

fn current(store: &Store, table: &str, id: &str) -> Option<Value> {
    store.conn.query_row(&format!("SELECT data FROM {table} WHERE id=?1"), [id], |r| r.get::<_, String>(0)).ok().and_then(|s| serde_json::from_str(&s).ok())
}

impl Library {
    /// Find mirror files changed outside the program and prepare each change for review.
    pub fn scan_mirror(&self) -> Result<MirrorScan> {
        let store = &self.store;
        let dir = store.mirror_dir();
        let mut scan = MirrorScan::default();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut consider = |path: &Path, superseded: bool, scan: &mut MirrorScan| -> Result<()> {
            let Ok(disk) = std::fs::read(path) else { return Ok(()) };
            let rel = if superseded { store.mirror_rel(path).trim_start_matches("_external/").to_string() } else { store.mirror_rel(path) };
            let Some((table, name)) = rel.split_once('/') else { return Ok(()) };
            if !TABLES.contains(&table) || name.contains('/') {
                return Ok(());
            }
            if !superseded {
                scan.files_checked += 1;
                match store.mirror_recorded(&rel) {
                    Some(h) if h == sha256_hex(&disk) => return Ok(()),
                    _ => {}
                }
            }
            let text = String::from_utf8_lossy(&disk).to_string();
            let is_md = name.ends_with(".md");
            // The record id: from the file itself, so ids with characters unsafe in file names work.
            let (id, after, mut problem) = if is_md {
                let id = text.lines().find_map(|l| l.strip_prefix("id: ")).unwrap_or("").trim().to_string();
                match current(store, table, &id) {
                    Some(cur) => match note_from_markdown(&cur, &text) {
                        Ok(v) => (id, Some(v), String::new()),
                        Err(e) => (id, None, e),
                    },
                    None => (id, None, "A note can be added from its JSON file, not its Markdown file.".to_string()),
                }
            } else {
                match serde_json::from_str::<Value>(&text) {
                    Ok(v) => (v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(), Some(v), String::new()),
                    Err(e) => (name.trim_end_matches(".json").to_string(), None, format!("The file is not valid JSON: {e}")),
                }
            };
            let cur = current(store, table, &id);
            // Unrecorded file (written before hashes were kept) that matches the library: ours.
            if !superseded && store.mirror_recorded(&rel).is_none() {
                if let Some((json, md)) = store.mirror_text(table, &id)? {
                    let expected = if is_md { md.unwrap_or_default() } else { json };
                    if expected.as_bytes() == disk.as_slice() {
                        store.mirror_record(&rel, Some(&disk))?;
                        return Ok(());
                    }
                }
            }
            let mut validated = None;
            if problem.is_empty() {
                if id.is_empty() {
                    problem = "The record has no id.".into();
                } else if !EDITABLE.contains(&table) {
                    problem = "The program keeps these records for itself; they cannot be changed in the mirror. Declining puts the file back.".into();
                } else {
                    match validate(table, after.as_ref().unwrap()) {
                        Ok(v) => validated = Some(v),
                        Err(e) => problem = format!("The record does not fit its type: {e}"),
                    }
                }
            }
            let kind = if cur.is_some() { "edited" } else { "added" };
            let key = format!("{table}/{id}/{kind}");
            if !seen.insert(key.clone()) {
                return Ok(());
            }
            let fields = match (&cur, &validated) {
                (Some(c), Some(v)) => diff(c, v),
                (None, Some(v)) => diff(&Value::Null, v),
                _ => vec![],
            };
            if problem.is_empty() && fields.is_empty() {
                // Only formatting changed: nothing to apply.
                return Ok(());
            }
            let label = cur.as_ref().or(validated.as_ref()).map(|v| label_of(table, v)).unwrap_or_else(|| id.clone());
            scan.changes.push(MirrorChange { key, table: table.into(), id, file: rel, kind: kind.into(), label, fields, problem, superseded, after: validated, source: path.to_path_buf() });
            Ok(())
        };
        for t in TABLES {
            for e in std::fs::read_dir(dir.join(t)).into_iter().flatten().flatten() {
                let p = e.path();
                let n = p.to_string_lossy().to_string();
                if n.ends_with(".json") || n.ends_with(".md") {
                    consider(&p, false, &mut scan)?;
                }
            }
            for e in std::fs::read_dir(dir.join("_external").join(t)).into_iter().flatten().flatten() {
                consider(&e.path(), true, &mut scan)?;
            }
        }
        // Files the program wrote that are gone: the record was deleted outside.
        let recorded: Vec<String> = {
            let mut st = store.conn.prepare("SELECT path FROM mirror_files")?;
            let v = st.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
            v
        };
        for rel in recorded {
            if !rel.ends_with(".json") || dir.join(&rel).exists() {
                continue;
            }
            let Some((table, _)) = rel.split_once('/') else { continue };
            let name = rel.rsplit('/').next().unwrap_or("").trim_end_matches(".json");
            // Find the record whose mirror file this was.
            let mut st = store.conn.prepare(&format!("SELECT id, data FROM {table}"))?;
            let rows: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.filter_map(|r| r.ok()).collect();
            if let Some((id, data)) = rows.into_iter().find(|(id, _)| store.mirror_rel(&store.mirror_path(table, id)) == rel) {
                let v: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
                let problem = if EDITABLE.contains(&table) { String::new() } else { "The program keeps these records for itself. Declining puts the file back.".into() };
                scan.changes.push(MirrorChange {
                    key: format!("{table}/{id}/deleted"),
                    table: table.into(),
                    id,
                    file: rel.clone(),
                    kind: "deleted".into(),
                    label: label_of(table, &v),
                    fields: vec![],
                    problem,
                    superseded: false,
                    after: None,
                    source: PathBuf::from(name),
                });
            }
        }
        scan.changes.sort_by(|a, b| a.table.cmp(&b.table).then(a.label.cmp(&b.label)));
        Ok(scan)
    }

    /// Apply the selected changes as one undoable step; decline the others listed in `decline`
    /// (their files are rewritten from the library). A deleted source goes to the trash.
    pub fn apply_mirror(&mut self, apply: &[String], decline: &[String]) -> Result<ApplyReport> {
        let scan = self.scan_mirror()?;
        let mut rep = ApplyReport::default();
        let chosen: Vec<&MirrorChange> = scan.changes.iter().filter(|c| apply.contains(&c.key)).collect();
        for c in &chosen {
            if !c.problem.is_empty() {
                rep.failed.push((c.key.clone(), c.problem.clone()));
            }
        }
        let ok: Vec<&MirrorChange> = chosen.into_iter().filter(|c| c.problem.is_empty()).collect();
        // The accepted file is now the program's own: record it so the rewrite that follows the
        // change does not mistake it for another outside edit.
        for c in &ok {
            if !c.superseded && c.kind != "deleted" {
                if let Ok(b) = std::fs::read(&c.source) {
                    self.store.mirror_record(&c.file, Some(&b))?;
                }
            }
        }
        if !ok.is_empty() {
            let t = crate::util::now();
            self.write(&format!("Apply {} change{} from the text mirror", ok.len(), if ok.len() == 1 { "" } else { "s" }), |tx| {
                for c in &ok {
                    match (c.kind.as_str(), &c.after) {
                        ("deleted", _) if c.table == "refs" => {
                            let mut r = tx.must::<Reference>(&c.id)?;
                            r.trashed_at = Some(t.clone());
                            tx.put(&r)?;
                        }
                        ("deleted", _) => tx.put_raw(&c.table, &c.id, None)?,
                        (_, Some(v)) => {
                            let mut v = v.clone();
                            if c.table == "refs" {
                                // Record that these values came from the mirror.
                                let mut r: Reference = serde_json::from_value(v.clone()).map_err(|e| Error::user(e.to_string()))?;
                                for f in &c.fields {
                                    let name = f.field.strip_prefix("fields.").unwrap_or(&f.field).to_string();
                                    r.provenance.insert(name, Provenance { source: "mirror".into(), time: t.clone(), user_edited: true, confidence: None, review: "reviewed".into() });
                                }
                                v = serde_json::to_value(r)?;
                            }
                            tx.put_raw(&c.table, &c.id, Some(&canonical_json(&v)))?
                        }
                        _ => {}
                    }
                }
                Ok(())
            })?;
            rep.applied = ok.len();
        }
        for c in scan.changes.iter().filter(|c| decline.contains(&c.key)) {
            rep.declined += 1;
            let _ = std::fs::remove_file(self.store.mirror_dir().join(&c.file));
            self.store.mirror_record(&c.file, None)?;
        }
        // Rewrite affected files from the library and clear reviewed outside copies.
        for c in scan.changes.iter().filter(|c| apply.contains(&c.key) || decline.contains(&c.key)) {
            let _ = std::fs::remove_file(self.store.mirror_dir().join("_external").join(&c.file));
            if c.problem.is_empty() || decline.contains(&c.key) {
                self.store.mirror_entity(&c.table, &c.id)?;
            }
        }
        self.store.advance_mirror()?;
        Ok(rep)
    }
}

/// Build a new library at `target` from the mirror folder `mirror` alone.
pub fn reconstruct(mirror: &Path, target: &Path) -> Result<Store> {
    if target.join(crate::store::DB_FILE).exists() {
        return Err(Error::user("Choose an empty folder for the rebuilt library."));
    }
    let mut store = Store::open(target, "rebuild")?;
    let mut records: BTreeMap<(String, String), String> = BTreeMap::new();
    for t in TABLES {
        for e in std::fs::read_dir(mirror.join(t)).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&p)?).map_err(|e| Error::user(format!("{} is not valid JSON: {e}", p.display())))?;
            let v = validate(t, &v).map_err(|e| Error::user(format!("{}: {e}", p.display())))?;
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            records.insert((t.to_string(), id), canonical_json(&v));
        }
    }
    store.write("Rebuilt from the text mirror", |tx| {
        for ((t, id), data) in &records {
            tx.put_raw(t, id, Some(data))?;
        }
        Ok(())
    })?;
    Ok(store)
}
