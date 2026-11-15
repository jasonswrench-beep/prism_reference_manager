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

/// Version of the working store's format. Raise it only together with a step in `MIGRATIONS`.
pub const SCHEMA_VERSION: i64 = 2;
pub const DB_FILE: &str = "library.db";

/// One upgrade step from `to - 1` to `to`. Steps run in order inside a single transaction after a
/// verified backup; if any step fails nothing is changed. Additive changes (new tables, columns,
/// indexes) also happen in `init_schema`, which is idempotent; a step is for changes that alter
/// existing data or structure.
pub struct Migration {
    pub to: i64,
    pub label: &'static str,
    pub run: fn(&Connection) -> Result<()>,
}

/// Version 2 starts the schema history: libraries created before it get a first row saying so.
fn m2_schema_history(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO schema_history(version, applied, label, backup, app_version) VALUES(1, COALESCE((SELECT value FROM meta WHERE key='created'), ''), 'Library created before the schema history was kept', '', '')",
        [],
    )?;
    Ok(())
}

pub const MIGRATIONS: &[Migration] = &[Migration { to: 2, label: "Keep a history of format upgrades", run: m2_schema_history }];

/// How the store was opened, for the window to explain.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct OpenReport {
    /// Version found in the file (0 for a new library).
    pub found: i64,
    pub migrated_from: Option<i64>,
    /// Verified copy made before upgrading.
    pub backup: String,
    /// The library was made by a newer program version and is open read-only.
    pub too_new: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SchemaStep {
    pub version: i64,
    pub applied: String,
    pub label: String,
    pub backup: String,
    pub app_version: String,
}

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
    "custom_types",
];

/// Generated index columns for tables that need lookups by a JSON member.
const INDEXES: &[(&str, &str, &str)] = &[
    ("refs", "trashed", "$.trashed_at"),
    ("refs", "doi", "$.fields.doi"),
    ("refs", "work_id", "$.work_id"),
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
    /// What happened when the store was opened (an upgrade, or a too-new format).
    pub opened: OpenReport,
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

/// Connection settings; they cannot change inside a transaction, so they run first.
/// A library database may come from someone else (a folder they shared, a restored backup).
/// Defensive mode stops SQL from corrupting the file through its own structure, the schema is not
/// trusted to run functions, and any trigger or view is removed: the program makes neither, so
/// one found here was added by someone else and would run on later edits.
pub fn harden(conn: &Connection, writable: bool) -> Result<()> {
    conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    conn.execute_batch("PRAGMA trusted_schema=OFF;")?;
    if writable {
        let extra: Vec<(String, String)> = {
            let mut st = conn.prepare("SELECT type, name FROM sqlite_master WHERE type IN ('trigger','view')")?;
            let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.filter_map(|r| r.ok()).collect();
            v
        };
        for (kind, name) in extra {
            conn.execute_batch(&format!("DROP {} IF EXISTS \"{}\"", if kind == "trigger" { "TRIGGER" } else { "VIEW" }, name.replace('"', "\"\"")))?;
        }
    }
    Ok(())
}

fn connection_pragmas(conn: &Connection, writable: bool) -> Result<()> {
    harden(conn, writable)?;
    if writable {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    }
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA cache_size=-65536; PRAGMA temp_store=MEMORY; PRAGMA mmap_size=268435456;")?;
    Ok(())
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS mirror_files(path TEXT PRIMARY KEY, hash TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS schema_history(
            version INTEGER NOT NULL, applied TEXT NOT NULL, label TEXT NOT NULL,
            backup TEXT NOT NULL, app_version TEXT NOT NULL);
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
         CREATE TABLE IF NOT EXISTS ocr_words(
            attachment_id TEXT NOT NULL, page INTEGER NOT NULL, words TEXT NOT NULL,
            PRIMARY KEY(attachment_id, page));
         CREATE TABLE IF NOT EXISTS network_log(
            seq INTEGER PRIMARY KEY AUTOINCREMENT, time TEXT NOT NULL, kind TEXT NOT NULL,
            host TEXT NOT NULL, purpose TEXT NOT NULL, sent TEXT NOT NULL, ok INTEGER NOT NULL, detail TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS ref_index(
            id TEXT PRIMARY KEY, trashed INTEGER NOT NULL, rtype TEXT NOT NULL, author_key TEXT NOT NULL,
            title_key TEXT NOT NULL, year_key TEXT NOT NULL, container_key TEXT NOT NULL, type_label TEXT NOT NULL,
            citekey TEXT NOT NULL, created TEXT NOT NULL, modified TEXT NOT NULL, rating INTEGER NOT NULL,
            origin TEXT NOT NULL, needs_meta INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS ref_index_author ON ref_index(trashed, author_key, title_key, id);
         CREATE INDEX IF NOT EXISTS ref_index_title ON ref_index(trashed, title_key, id);
         CREATE INDEX IF NOT EXISTS ref_index_year ON ref_index(trashed, year_key, title_key, id);
         CREATE TABLE IF NOT EXISTS fts_map(entity TEXT NOT NULL, entity_id TEXT NOT NULL, row INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS fts_map_entity ON fts_map(entity, entity_id);
         CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
            entity UNINDEXED, entity_id UNINDEXED, scope UNINDEXED, field UNINDEXED, page UNINDEXED,
            orig UNINDEXED, text, tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3 4');
         CREATE VIRTUAL TABLE IF NOT EXISTS fts_vocab USING fts5vocab(fts, row);",
    )?;
    for t in TABLES {
        conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS {t}(id TEXT PRIMARY KEY, data TEXT NOT NULL);"))?;
    }
    crate::journals::ensure_table(conn)?;
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

/// Version recorded in a database file (0 when it has none).
fn file_version(conn: &Connection) -> i64 {
    conn.query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get::<_, String>(0)).optional().ok().flatten().and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Copy the database to `dest` and check the copy: SQLite's integrity check and the same number
/// of rows in every table.
fn verified_copy(conn: &Connection, dest: &Path) -> Result<()> {
    conn.execute("VACUUM INTO ?1", params![dest.to_string_lossy()])?;
    let copy = Connection::open_with_flags(dest, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let ok: String = copy.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    let tables: Vec<String> = {
        let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'fts%'")?;
        let v = st.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
        v
    };
    let mut same = ok == "ok";
    for t in &tables {
        let a: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM \"{t}\""), [], |r| r.get(0))?;
        let b: i64 = copy.query_row(&format!("SELECT COUNT(*) FROM \"{t}\""), [], |r| r.get(0)).unwrap_or(-1);
        same &= a == b;
    }
    drop(copy);
    if !same {
        let _ = std::fs::remove_file(dest);
        return Err(Error::user("The library could not be backed up before upgrading its format, so it was not changed. Free some disk space and open it again."));
    }
    Ok(())
}

/// Run every step after `from` up to `target` in one transaction. Nothing changes on failure.
fn migrate(conn: &Connection, from: i64, target: i64, steps: &[Migration], backup: &str) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let res = (|| -> Result<()> {
        init_schema(conn)?;
        for m in steps.iter().filter(|m| m.to > from && m.to <= target) {
            (m.run)(conn).map_err(|e| Error::user(format!("Upgrading the library format (step {}: {}) failed: {e}", m.to, m.label)))?;
            conn.execute(
                "INSERT INTO schema_history(version, applied, label, backup, app_version) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![m.to, now(), m.label, backup, env!("CARGO_PKG_VERSION")],
            )?;
        }
        conn.execute(
            "INSERT INTO meta(key,value) VALUES('schema_version',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![target.to_string()],
        )?;
        Ok(())
    })();
    match res {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

impl Store {
    /// Open or create the store in `root`. An older format is upgraded after a verified backup;
    /// a newer one (made by a later program version) opens read-only.
    pub fn open(root: &Path, device: &str) -> Result<Store> {
        Ok(Self::open_with(root, device, MIGRATIONS, SCHEMA_VERSION)?.0)
    }

    /// `open` with an explicit list of steps and target version (tests use their own).
    pub fn open_with(root: &Path, device: &str, steps: &[Migration], target: i64) -> Result<(Store, OpenReport)> {
        std::fs::create_dir_all(root)?;
        let db = root.join(DB_FILE);
        let existed = db.exists();
        let conn = Connection::open(&db)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut rep = OpenReport::default();
        let mut read_only = false;
        if existed {
            let v = file_version(&conn);
            rep.found = v;
            if v > target {
                read_only = true;
                rep.too_new = true;
            } else if v < target && v > 0 {
                connection_pragmas(&conn, true)?;
                let backup_dir = root.join("backups");
                std::fs::create_dir_all(&backup_dir)?;
                let name = format!("pre-migration-schema{}-{}.db", v, now().replace([':', '.'], "-"));
                let dest = backup_dir.join(&name);
                verified_copy(&conn, &dest)?;
                migrate(&conn, v, target, steps, &format!("backups/{name}"))?;
                rep.migrated_from = Some(v);
                rep.backup = dest.to_string_lossy().to_string();
            }
        }
        if !read_only {
            connection_pragmas(&conn, true)?;
            init_schema(&conn)?;
            if !existed || file_version(&conn) == 0 {
                conn.execute(
                    "INSERT INTO meta(key,value) VALUES('schema_version',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![target.to_string()],
                )?;
                conn.execute("INSERT INTO schema_history(version, applied, label, backup, app_version) VALUES(?1, ?2, 'Library created', '', ?3)", params![target, now(), env!("CARGO_PKG_VERSION")])?;
            }
            crate::search::ensure_ref_index(&conn)?;
            crate::search::ensure_fts_version(&conn)?;
            let has_id: bool = conn.prepare("SELECT 1 FROM meta WHERE key='library_id'")?.exists([])?;
            if !has_id {
                conn.execute("INSERT INTO meta(key,value) VALUES('library_id',?1)", params![new_id()])?;
                conn.execute("INSERT INTO meta(key,value) VALUES('created',?1)", params![now()])?;
            }
        } else {
            connection_pragmas(&conn, false)?;
            conn.execute_batch("PRAGMA query_only=ON;")?;
        }
        let store = Store { conn, root: root.to_path_buf(), device: device.into(), actor: "user".into(), read_only, opened: rep.clone() };
        Ok((store, rep))
    }

    /// Format upgrades applied to this library, oldest first.
    pub fn schema_history(&self) -> Result<Vec<SchemaStep>> {
        let mut st = self.conn.prepare("SELECT version, applied, label, backup, app_version FROM schema_history ORDER BY rowid")?;
        let v = st
            .query_map([], |r| Ok(SchemaStep { version: r.get(0)?, applied: r.get(1)?, label: r.get(2)?, backup: r.get(3)?, app_version: r.get(4)? }))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    }

    pub fn schema_version(&self) -> i64 {
        file_version(&self.conn)
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
            // A change record naming a table the program does not have is ignored: its name would
            // otherwise become a folder name in the mirror.
            if table_ok(&tbl).is_ok() {
                touched.insert((tbl, id));
            }
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

    pub fn mirror_path(&self, tbl: &str, id: &str) -> PathBuf {
        let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
        self.mirror_dir().join(tbl).join(format!("{safe}.json"))
    }

    /// Path inside the mirror folder, with forward slashes.
    pub fn mirror_rel(&self, path: &Path) -> String {
        path.strip_prefix(self.mirror_dir()).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }

    /// Hash of a mirror file as the program last wrote it.
    pub fn mirror_recorded(&self, rel: &str) -> Option<String> {
        self.conn.query_row("SELECT hash FROM mirror_files WHERE path=?1", params![rel], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn mirror_record(&self, rel: &str, bytes: Option<&[u8]>) -> Result<()> {
        if self.read_only {
            return Ok(());
        }
        match bytes {
            Some(b) => self.conn.execute("INSERT INTO mirror_files(path,hash) VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET hash=excluded.hash", params![rel, crate::util::sha256_hex(b)])?,
            None => self.conn.execute("DELETE FROM mirror_files WHERE path=?1", params![rel])?,
        };
        Ok(())
    }

    /// The mirror text the program writes for one entity: JSON, and Markdown for notes.
    pub fn mirror_text(&self, tbl: &str, id: &str) -> Result<Option<(String, Option<String>)>> {
        table_ok(tbl)?;
        let data: Option<String> = self.conn.query_row(&format!("SELECT data FROM {tbl} WHERE id=?1"), params![id], |r| r.get(0)).optional()?;
        Ok(match data {
            Some(d) => {
                let v: serde_json::Value = serde_json::from_str(&d)?;
                Some((canonical_json_pretty(&v), if tbl == "notes" { Some(note_markdown(&v)) } else { None }))
            }
            None => None,
        })
    }

    /// Before the program overwrites or removes a mirror file, keep a copy if someone changed it
    /// outside the program, so the change can still be reviewed (never silently lost).
    fn keep_external(&self, path: &Path) -> Result<()> {
        let Ok(disk) = std::fs::read(path) else { return Ok(()) };
        let rel = self.mirror_rel(path);
        if let Some(h) = self.mirror_recorded(&rel) {
            if h != crate::util::sha256_hex(&disk) {
                let dest = self.mirror_dir().join("_external").join(&rel);
                write_atomic(&dest, &disk)?;
            }
        }
        Ok(())
    }

    fn write_mirror_file(&self, path: &Path, text: &str) -> Result<()> {
        let rel = self.mirror_rel(path);
        if std::fs::read(path).ok().as_deref() == Some(text.as_bytes()) {
            if self.mirror_recorded(&rel).is_none() {
                self.mirror_record(&rel, Some(text.as_bytes()))?;
            }
            return Ok(());
        }
        self.keep_external(path)?;
        write_atomic(path, text.as_bytes())?;
        self.mirror_record(&rel, Some(text.as_bytes()))
    }

    fn remove_mirror_file(&self, path: &Path) -> Result<()> {
        if path.exists() {
            self.keep_external(path)?;
            let _ = std::fs::remove_file(path);
        }
        self.mirror_record(&self.mirror_rel(path), None)
    }

    pub fn mirror_entity(&self, tbl: &str, id: &str) -> Result<()> {
        table_ok(tbl)?;
        let path = self.mirror_path(tbl, id);
        let md = path.with_extension("md");
        match self.mirror_text(tbl, id)? {
            Some((json, note)) => {
                self.write_mirror_file(&path, &json)?;
                if let Some(n) = note {
                    self.write_mirror_file(&md, &n)?;
                }
            }
            None => {
                self.remove_mirror_file(&path)?;
                self.remove_mirror_file(&md)?;
            }
        }
        Ok(())
    }

    /// Rebuild the whole mirror from the store. Files changed outside the program are kept
    /// under `mirror/_external` for review first.
    pub fn rebuild_mirror(&self) -> Result<()> {
        let dir = self.mirror_dir();
        if dir.exists() {
            for t in TABLES {
                for e in std::fs::read_dir(dir.join(t)).into_iter().flatten().flatten() {
                    let _ = self.keep_external(&e.path());
                }
            }
            for t in TABLES {
                let _ = std::fs::remove_dir_all(dir.join(t));
            }
        }
        if !self.read_only {
            self.conn.execute("DELETE FROM mirror_files", [])?;
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

/// A lock whose last heartbeat is older than this is left over from a program that stopped.
const LOCK_FRESH_SECS: i64 = 90;

/// What the person is told when the library is marked open elsewhere.
fn locked_message(info: &LockInfo, this_computer: bool, secs_left: i64) -> String {
    if this_computer {
        // Another window on this computer, or a program that stopped suddenly and left its mark.
        let wait = if secs_left > 60 { "about two minutes".to_string() } else { format!("about {} seconds", secs_left.max(10)) };
        format!("The library is still marked open by Prism Reference Manager on this computer (process {}). If it is open in another window, close it there first. If the program stopped suddenly, the mark clears by itself: try again in {wait}.", info.pid)
    } else {
        format!("The library is open on another computer ({}). Close it there first, or open a copy. If that computer stopped suddenly, the mark clears by itself within two minutes.", info.host)
    }
}

/// Take the library lock. Fails when another device or process holds a recent lock, so two
/// computers never write the same live database through a synchronized folder.
pub fn acquire_lock(root: &Path, device: &str, host: &str) -> Result<()> {
    let p = lock_path(root);
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Ok(info) = serde_json::from_str::<LockInfo>(&s) {
            let age = chrono::DateTime::parse_from_rfc3339(&info.heartbeat)
                .map(|t| chrono::Utc::now().signed_duration_since(t).num_seconds())
                .unwrap_or(LOCK_FRESH_SECS);
            let fresh = age < LOCK_FRESH_SECS;
            let ours = info.device == device && info.pid == std::process::id();
            if fresh && !ours {
                return Err(Error::Locked(locked_message(&info, info.device == device, LOCK_FRESH_SECS - age.max(0))));
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

/// A verified copy made before a format upgrade, still on disk.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpgradeBackup {
    /// Path relative to the library folder.
    pub path: String,
    /// Format version of the copy.
    pub version: i64,
    pub made: String,
    pub size: u64,
}

/// Pre-upgrade copies of the library in `root`, newest first.
pub fn upgrade_backups(root: &Path) -> Vec<UpgradeBackup> {
    let dir = root.join("backups");
    let mut out: Vec<UpgradeBackup> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let rest = name.strip_prefix("pre-migration-schema")?.strip_suffix(".db")?;
            let (v, made) = rest.split_once('-')?;
            Some(UpgradeBackup { path: format!("backups/{name}"), version: v.parse().ok()?, made: made.to_string(), size: e.metadata().map(|m| m.len()).unwrap_or(0) })
        })
        .collect();
    out.sort_by(|a, b| b.made.cmp(&a.made));
    out
}

/// Put the library in `root` back as it was before a format upgrade, from a pre-upgrade copy.
/// The library must be closed. The current database is kept beside it (returned), so nothing is
/// lost: changes made since the upgrade are in that kept file. The restored library has the older
/// format; this program upgrades it again the next time it opens it, so the step is for going
/// back to an earlier program version.
pub fn revert_upgrade(root: &Path, backup: &str) -> Result<PathBuf> {
    if backup.contains("..") || !backup.starts_with("backups/pre-migration-schema") {
        return Err(Error::user("That is not a pre-upgrade copy of this library."));
    }
    let src = root.join(backup);
    let copy = Connection::open_with_flags(&src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| Error::user("The pre-upgrade copy could not be opened."))?;
    let ok: String = copy.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap_or_default();
    let v = file_version(&copy);
    drop(copy);
    if ok != "ok" || v <= 0 {
        return Err(Error::user("The pre-upgrade copy is damaged, so nothing was changed."));
    }
    let db = root.join(DB_FILE);
    let keep = root.join("backups").join(format!("before-revert-{}.db", now().replace([':', '.'], "-")));
    {
        let cur = Connection::open(&db)?;
        verified_copy(&cur, &keep)?;
    }
    let tmp = root.join("library.db.reverting");
    std::fs::copy(&src, &tmp)?;
    for ext in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(root.join(format!("{DB_FILE}{ext}")));
    }
    std::fs::rename(&tmp, &db)?;
    Ok(keep)
}
