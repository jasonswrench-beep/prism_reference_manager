//! Transactional working store.
//!
//! Every entity lives in its own table as canonical JSON. Each write happens inside a
//! transaction that also records a change event (before and after values). Events drive
//! persistent undo and redo, per-field history, and the deterministic text mirror, which a
//! worker advances behind a durable checkpoint so a crash between the database commit and the
//! mirror write is repaired by replaying the unapplied events.

use crate::error::{Error, Result};
use crate::model::Entity;
use crate::util::{canonical_json, canonical_json_pretty, new_id, now, write_atomic};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: i64 = 1;
pub const DB_FILE: &str = "library.db";

/// Entity tables that hold durable data. They are mirrored, exported and compared in round trips.
pub const TABLES: &[&str] = &[
    "refs",
    "aliases",
    "projects",
    "project_sources",
    "manuscripts",
    "manuscript_sources",
    "copy_events",
    "snapshots",
    "collections",
    "labels",
    "saved_searches",
    "duplicate_decisions",
    "tasks",
    "attachments",
    "annotations",
    "notes",
    "outline_sections",
    "import_batches",
    "styles",
    "integration_runs",
    "custom_fields",
];

/// Generated index columns for tables that need lookups by a JSON member.
const INDEXES: &[(&str, &str, &str)] = &[
    ("refs", "trashed", "$.trashed_at"),
    ("refs", "doi", "$.fields.doi"),
    ("project_sources", "project_id", "$.project_id"),
    ("project_sources", "ref_id", "$.ref_id"),
    ("manuscripts", "project_id", "$.project_id"),
    ("manuscript_sources", "manuscript_id", "$.manuscript_id"),
    ("manuscript_sources", "ref_id", "$.ref_id"),
    ("copy_events", "manuscript_id", "$.manuscript_id"),
    ("attachments", "ref_id", "$.ref_id"),
    ("attachments", "hash", "$.hash"),
    ("annotations", "attachment_id", "$.attachment_id"),
    ("notes", "project_id", "$.project_id"),
    ("outline_sections", "project_id", "$.project_id"),
    ("collections", "project_id", "$.project_id"),
];

pub struct Store {
    pub conn: Connection,
    pub root: PathBuf,
    pub device: String,
    pub actor: String,
    pub read_only: bool,
}

/// An open write transaction. All changes and their events commit together.
pub struct Tx<'a> {
    pub tx: rusqlite::Transaction<'a>,
    pub group: String,
    pub device: String,
    pub actor: String,
    pub changes: usize,
}

fn table_ok(t: &str) -> Result<()> {
    if TABLES.contains(&t) {
        Ok(())
    } else {
        Err(Error::user(format!("Unknown table {t}")))
    }
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=FULL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS events(
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT NOT NULL, grp TEXT NOT NULL, time TEXT NOT NULL, device TEXT NOT NULL,
            actor TEXT NOT NULL, tbl TEXT NOT NULL, entity_id TEXT NOT NULL,
            before TEXT, after TEXT);
         CREATE INDEX IF NOT EXISTS events_entity ON events(tbl, entity_id);
         CREATE INDEX IF NOT EXISTS events_grp ON events(grp);
         CREATE TABLE IF NOT EXISTS undo_groups(
            grp TEXT PRIMARY KEY, label TEXT NOT NULL, time TEXT NOT NULL,
            state TEXT NOT NULL, seq INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS page_text(
            attachment_id TEXT NOT NULL, page INTEGER NOT NULL, text TEXT NOT NULL,
            PRIMARY KEY(attachment_id, page));
         CREATE TABLE IF NOT EXISTS network_log(
            seq INTEGER PRIMARY KEY AUTOINCREMENT, time TEXT NOT NULL, kind TEXT NOT NULL,
            host TEXT NOT NULL, purpose TEXT NOT NULL, sent TEXT NOT NULL, ok INTEGER NOT NULL, detail TEXT NOT NULL);
         CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
            entity UNINDEXED, entity_id UNINDEXED, scope UNINDEXED, field UNINDEXED, page UNINDEXED,
            orig UNINDEXED, text, tokenize = 'unicode61 remove_diacritics 2');",
    )?;
    for t in TABLES {
        conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS {t}(id TEXT PRIMARY KEY, data TEXT NOT NULL);"))?;
    }
    for (t, col, path) in INDEXES {
        let exists: bool = conn
            .prepare(&format!("SELECT 1 FROM pragma_table_xinfo('{t}') WHERE name='{col}'"))?
            .exists([])?;
        if !exists {
            conn.execute_batch(&format!(
                "ALTER TABLE {t} ADD COLUMN {col} TEXT GENERATED ALWAYS AS (json_extract(data, '{path}')) VIRTUAL;
                 CREATE INDEX IF NOT EXISTS {t}_{col} ON {t}({col});"
            ))?;
        }
    }
    Ok(())
}

impl Store {
    /// Open or create the store in `root`. Creates a verified backup of the database before a
    /// schema migration, and opens a too-new schema read-only.
    pub fn open(root: &Path, device: &str) -> Result<Store> {
        std::fs::create_dir_all(root)?;
        let db = root.join(DB_FILE);
        let existed = db.exists();
        let conn = Connection::open(&db)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut read_only = false;
        if existed {
            let v: Option<String> = conn
                .query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get(0))
                .optional()
                .unwrap_or(None);
            let v: i64 = v.and_then(|s| s.parse().ok()).unwrap_or(0);
            if v > SCHEMA_VERSION {
                read_only = true;
            } else if v < SCHEMA_VERSION {
                // Pre-migration backup of the whole database file.
                let backup_dir = root.join("backups");
                std::fs::create_dir_all(&backup_dir)?;
                let name = format!("pre-migration-schema{}-{}.db", v, now().replace([':', '.'], "-"));
                conn.execute("VACUUM INTO ?1", params![backup_dir.join(name).to_string_lossy()])?;
            }
        }
        if !read_only {
            init_schema(&conn)?;
            conn.execute(
                "INSERT INTO meta(key,value) VALUES('schema_version',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![SCHEMA_VERSION.to_string()],
            )?;
            let has_id: bool = conn.prepare("SELECT 1 FROM meta WHERE key='library_id'")?.exists([])?;
            if !has_id {
                conn.execute("INSERT INTO meta(key,value) VALUES('library_id',?1)", params![new_id()])?;
                conn.execute("INSERT INTO meta(key,value) VALUES('created',?1)", params![now()])?;
            }
        } else {
            conn.execute_batch("PRAGMA query_only=ON;")?;
        }
        Ok(Store { conn, root: root.to_path_buf(), device: device.into(), actor: "user".into(), read_only })
    }

    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn.query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Run `f` inside one transaction. Changes and events commit atomically. A group with at
    /// least one change becomes an undo step labelled `label`, and the redo stack is cleared.
    pub fn write<T>(&mut self, label: &str, f: impl FnOnce(&mut Tx) -> Result<T>) -> Result<T> {
        if self.read_only {
            return Err(Error::user("This library is open read-only. Export what you need, or open it with a newer version."));
        }
        let group = new_id();
        let tx = self.conn.transaction()?;
        let mut t = Tx { tx, group: group.clone(), device: self.device.clone(), actor: self.actor.clone(), changes: 0 };
        let out = f(&mut t)?;
        if t.changes > 0 {
            let seq: i64 = t.tx.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))?;
            t.tx.execute("UPDATE undo_groups SET state='dead' WHERE state='undone'", [])?;
            t.tx.execute(
                "INSERT INTO undo_groups(grp,label,time,state,seq) VALUES(?1,?2,?3,'done',?4)",
                params![group, label, now(), seq],
            )?;
        }
        t.tx.commit()?;
        Ok(out)
    }

    pub fn get<E: Entity>(&self, id: &str) -> Result<Option<E>> {
        get_in(&self.conn, id)
    }

    pub fn must<E: Entity>(&self, id: &str) -> Result<E> {
        self.get::<E>(id)?.ok_or_else(|| Error::NotFound(format!("{} {}", E::TABLE, id)))
    }

    pub fn all<E: Entity>(&self) -> Result<Vec<E>> {
        list_in(&self.conn, "", &[])
    }

    /// Rows where generated column `col` equals `value`.
    pub fn by<E: Entity>(&self, col: &str, value: &str) -> Result<Vec<E>> {
        list_in(&self.conn, &format!("WHERE {col}=?1"), &[value])
    }

    pub fn count(&self, table: &str) -> Result<i64> {
        table_ok(table)?;
        Ok(self.conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
    }

    // ---- undo and redo -------------------------------------------------------------------

    pub fn undo_label(&self) -> Option<String> {
        self.conn
            .query_row("SELECT label FROM undo_groups WHERE state='done' ORDER BY seq DESC LIMIT 1", [], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn redo_label(&self) -> Option<String> {
        self.conn
            .query_row("SELECT label FROM undo_groups WHERE state='undone' ORDER BY seq ASC LIMIT 1", [], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    /// Undo the latest step: restore every entity in the group to its value before the step.
    /// The restoration is itself recorded as events, so history and the mirror stay complete.
    pub fn undo(&mut self) -> Result<Option<String>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row("SELECT grp,label FROM undo_groups WHERE state='done' ORDER BY seq DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let Some((grp, label)) = row else { return Ok(None) };
        let events = self.group_events(&grp)?;
        let tx = self.conn.transaction()?;
        let mut t = Tx { tx, group: new_id(), device: self.device.clone(), actor: self.actor.clone(), changes: 0 };
        for (tbl, id, before, _after) in events.iter().rev() {
            t.put_raw(tbl, id, before.as_deref())?;
        }
        // Keep the original group's sequence so redo order is stable; record the undo time.
        t.tx.execute("UPDATE undo_groups SET state='undone' WHERE grp=?1", params![grp])?;
        t.tx.commit()?;
        Ok(Some(label))
    }

    pub fn redo(&mut self) -> Result<Option<String>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row("SELECT grp,label FROM undo_groups WHERE state='undone' ORDER BY seq ASC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let Some((grp, label)) = row else { return Ok(None) };
        let events = self.group_events(&grp)?;
        let tx = self.conn.transaction()?;
        let mut t = Tx { tx, group: new_id(), device: self.device.clone(), actor: self.actor.clone(), changes: 0 };
        for (tbl, id, _before, after) in events.iter() {
            t.put_raw(tbl, id, after.as_deref())?;
        }
        t.tx.execute("UPDATE undo_groups SET state='done' WHERE grp=?1", params![grp])?;
        t.tx.commit()?;
        Ok(Some(label))
    }

    fn group_events(&self, grp: &str) -> Result<Vec<(String, String, Option<String>, Option<String>)>> {
        let mut st = self.conn.prepare("SELECT tbl,entity_id,before,after FROM events WHERE grp=?1 ORDER BY seq")?;
        let rows = st.query_map(params![grp], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Recent undo steps, newest first, for the history panel.
    pub fn undo_history(&self, limit: usize) -> Result<Vec<serde_json::Value>> {
        let mut st = self.conn.prepare("SELECT grp,label,time,state FROM undo_groups WHERE state!='dead' ORDER BY seq DESC LIMIT ?1")?;
        let rows = st.query_map(params![limit as i64], |r| {
            Ok(serde_json::json!({"group": r.get::<_, String>(0)?, "label": r.get::<_, String>(1)?, "time": r.get::<_, String>(2)?, "state": r.get::<_, String>(3)?}))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Every recorded value of one field of one entity, oldest first: (time, actor, value).
    pub fn field_history(&self, table: &str, id: &str, field_path: &[&str]) -> Result<Vec<serde_json::Value>> {
        table_ok(table)?;
        let mut st = self.conn.prepare("SELECT time,actor,after,before FROM events WHERE tbl=?1 AND entity_id=?2 ORDER BY seq")?;
        let rows = st.query_map(params![table, id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?)))?;
        let pick = |s: &Option<String>| -> serde_json::Value {
            let mut v: serde_json::Value = s.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or(serde_json::Value::Null);
            for p in field_path {
                v = v.get(*p).cloned().unwrap_or(serde_json::Value::Null);
            }
            v
        };
        let mut out = Vec::new();
        let mut last = serde_json::Value::Null;
        for row in rows {
            let (time, actor, after, before) = row?;
            let a = pick(&after);
            if out.is_empty() {
                let b = pick(&before);
                if !b.is_null() {
                    out.push(serde_json::json!({"time": "", "actor": "", "value": b}));
                    last = b;
                }
            }
            if a != last {
                out.push(serde_json::json!({"time": time, "actor": actor, "value": a}));
                last = a;
            }
        }
        Ok(out)
    }

    // ---- text mirror ---------------------------------------------------------------------

    pub fn mirror_dir(&self) -> PathBuf {
        self.root.join("mirror")
    }

    /// Apply committed events after the mirror checkpoint to the text mirror, writing each file
    /// atomically, then advance the checkpoint. Safe to call repeatedly and after a crash.
    pub fn advance_mirror(&self) -> Result<usize> {
        let checkpoint: i64 = self.meta("mirror_seq").and_then(|s| s.parse().ok()).unwrap_or(0);
        let mut st = self.conn.prepare("SELECT seq,tbl,entity_id FROM events WHERE seq>?1 ORDER BY seq")?;
        let rows = st.query_map(params![checkpoint], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        let mut touched = std::collections::BTreeSet::new();
        let mut last = checkpoint;
        for r in rows {
            let (seq, tbl, id) = r?;
            touched.insert((tbl, id));
            last = seq;
        }
        for (tbl, id) in &touched {
            self.mirror_entity(tbl, id)?;
        }
        if last != checkpoint && !self.read_only {
            self.set_meta("mirror_seq", &last.to_string())?;
        }
        Ok(touched.len())
    }

    fn mirror_path(&self, tbl: &str, id: &str) -> PathBuf {
        let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
        self.mirror_dir().join(tbl).join(format!("{safe}.json"))
    }

    fn mirror_entity(&self, tbl: &str, id: &str) -> Result<()> {
        let data: Option<String> = self
            .conn
            .query_row(&format!("SELECT data FROM {tbl} WHERE id=?1"), params![id], |r| r.get(0))
            .optional()?;
        let path = self.mirror_path(tbl, id);
        let md = path.with_extension("md");
        match data {
            Some(d) => {
                let v: serde_json::Value = serde_json::from_str(&d)?;
                write_atomic(&path, canonical_json_pretty(&v).as_bytes())?;
                if tbl == "notes" {
                    write_atomic(&md, note_markdown(&v).as_bytes())?;
                }
            }
            None => {
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_file(&md);
            }
        }
        Ok(())
    }

    /// Rebuild the whole mirror from the store.
    pub fn rebuild_mirror(&self) -> Result<()> {
        let dir = self.mirror_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        for t in TABLES {
            let mut st = self.conn.prepare(&format!("SELECT id FROM {t}"))?;
            let ids: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
            for id in ids {
                self.mirror_entity(t, &id)?;
            }
        }
        let seq: i64 = self.conn.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))?;
        if !self.read_only {
            self.set_meta("mirror_seq", &seq.to_string())?;
        }
        Ok(())
    }

    /// Canonical dump of all durable entities: table -> sorted list of canonical JSON strings.
    /// Used by round-trip and recovery equality checks.
    pub fn canonical_dump(&self) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
        let mut out = std::collections::BTreeMap::new();
        for t in TABLES {
            let mut st = self.conn.prepare(&format!("SELECT data FROM {t} ORDER BY id"))?;
            let rows: Vec<String> = st.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<_, _>>()?;
            let mut canon: Vec<String> = rows
                .iter()
                .map(|s| serde_json::from_str::<serde_json::Value>(s).map(|v| canonical_json(&v)).unwrap_or_default())
                .collect();
            canon.sort();
            out.insert(t.to_string(), canon);
        }
        Ok(out)
    }

    pub fn log_network(&self, kind: &str, host: &str, purpose: &str, sent: &str, ok: bool, detail: &str) {
        let _ = self.conn.execute(
            "INSERT INTO network_log(time,kind,host,purpose,sent,ok,detail) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![now(), kind, host, purpose, sent, ok as i64, detail],
        );
    }
}

fn note_markdown(v: &serde_json::Value) -> String {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let mut o = String::new();
    o.push_str("---\n");
    o.push_str(&format!("id: {}\nkind: {}\nproject: {}\n", s("id"), s("kind"), s("project_id")));
    if let Some(refs) = v.get("ref_ids").and_then(|r| r.as_array()) {
        let ids: Vec<String> = refs.iter().filter_map(|x| x.as_str().map(String::from)).collect();
        o.push_str(&format!("sources: [{}]\n", ids.join(", ")));
    }
    if !s("locator").is_empty() {
        o.push_str(&format!("locator: {} {}\n", s("locator_label"), s("locator")));
    }
    o.push_str(&format!("origin: {}\n---\n\n", s("origin")));
    if !s("title").is_empty() {
        o.push_str(&format!("# {}\n\n", s("title")));
    }
    if !s("exact_text").is_empty() {
        for line in s("exact_text").lines() {
            o.push_str(&format!("> {line}\n"));
        }
        o.push('\n');
    }
    o.push_str(&s("body"));
    o.push('\n');
    o
}

pub fn get_in<E: Entity>(conn: &Connection, id: &str) -> Result<Option<E>> {
    let data: Option<String> = conn
        .query_row(&format!("SELECT data FROM {} WHERE id=?1", E::TABLE), params![id], |r| r.get(0))
        .optional()?;
    Ok(match data {
        Some(d) => Some(serde_json::from_str(&d)?),
        None => None,
    })
}

pub fn list_in<E: Entity>(conn: &Connection, clause: &str, args: &[&str]) -> Result<Vec<E>> {
    let mut st = conn.prepare(&format!("SELECT data FROM {} {}", E::TABLE, clause))?;
    let rows = st.query_map(rusqlite::params_from_iter(args.iter()), |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(serde_json::from_str(&r?)?);
    }
    Ok(out)
}

impl<'a> Tx<'a> {
    pub fn get<E: Entity>(&self, id: &str) -> Result<Option<E>> {
        get_in(&self.tx, id)
    }

    pub fn must<E: Entity>(&self, id: &str) -> Result<E> {
        self.get::<E>(id)?.ok_or_else(|| Error::NotFound(format!("{} {}", E::TABLE, id)))
    }

    pub fn by<E: Entity>(&self, col: &str, value: &str) -> Result<Vec<E>> {
        list_in(&self.tx, &format!("WHERE {col}=?1"), &[value])
    }

    pub fn all<E: Entity>(&self) -> Result<Vec<E>> {
        list_in(&self.tx, "", &[])
    }

    pub fn put<E: Entity>(&mut self, e: &E) -> Result<()> {
        let v = serde_json::to_value(e)?;
        let s = canonical_json(&v);
        self.put_raw(E::TABLE, e.id(), Some(&s))
    }

    pub fn delete<E: Entity>(&mut self, id: &str) -> Result<()> {
        self.put_raw(E::TABLE, id, None)
    }

    /// Write (or delete when `data` is None) one row and record the event.
    pub fn put_raw(&mut self, table: &str, id: &str, data: Option<&str>) -> Result<()> {
        table_ok(table)?;
        let before: Option<String> = self
            .tx
            .query_row(&format!("SELECT data FROM {table} WHERE id=?1"), params![id], |r| r.get(0))
            .optional()?;
        if before.as_deref() == data {
            return Ok(());
        }
        match data {
            Some(d) => {
                self.tx.execute(
                    &format!("INSERT INTO {table}(id,data) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data"),
                    params![id, d],
                )?;
            }
            None => {
                self.tx.execute(&format!("DELETE FROM {table} WHERE id=?1"), params![id])?;
            }
        }
        self.tx.execute(
            "INSERT INTO events(id,grp,time,device,actor,tbl,entity_id,before,after) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![new_id(), self.group, now(), self.device, self.actor, table, id, before, data],
        )?;
        crate::search::reindex(&self.tx, table, id, data)?;
        self.changes += 1;
        Ok(())
    }
}

// ---- concurrent access ---------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct LockInfo {
    pub device: String,
    pub host: String,
    pub pid: u32,
    pub heartbeat: String,
}

pub fn lock_path(root: &Path) -> PathBuf {
    root.join("library.lock")
}

/// Take the library lock. Fails when another device or process holds a recent lock, so two
/// computers never write the same live database through a synchronized folder.
pub fn acquire_lock(root: &Path, device: &str, host: &str) -> Result<()> {
    let p = lock_path(root);
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Ok(info) = serde_json::from_str::<LockInfo>(&s) {
            let fresh = chrono::DateTime::parse_from_rfc3339(&info.heartbeat)
                .map(|t| chrono::Utc::now().signed_duration_since(t).num_seconds() < 90)
                .unwrap_or(false);
            let ours = info.device == device && info.pid == std::process::id();
            if fresh && !ours {
                return Err(Error::Locked(format!("{} (process {})", info.host, info.pid)));
            }
        }
    }
    heartbeat(root, device, host)
}

pub fn heartbeat(root: &Path, device: &str, host: &str) -> Result<()> {
    let info = LockInfo { device: device.into(), host: host.into(), pid: std::process::id(), heartbeat: now() };
    write_atomic(&lock_path(root), serde_json::to_string(&info)?.as_bytes())?;
    Ok(())
}

pub fn release_lock(root: &Path, device: &str) {
    let p = lock_path(root);
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Ok(info) = serde_json::from_str::<LockInfo>(&s) {
            if info.device == device && info.pid == std::process::id() {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

/// True when the path looks like a consumer file-synchronization folder.
pub fn in_sync_folder(root: &Path) -> Option<String> {
    let s = root.to_string_lossy().to_lowercase();
    for marker in ["onedrive", "dropbox", "google drive", "googledrive", "icloud", "box sync", "my drive", "sync"] {
        if s.contains(marker) {
            return Some(marker.to_string());
        }
    }
    None
}
