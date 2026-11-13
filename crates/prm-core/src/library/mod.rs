//! Application services. The desktop shell, the local API and the tests call these validated
//! operations; none of them writes to the store directly.

mod cite;
mod files;
mod imports;
mod notes;
mod projects;
mod refs;
mod terms;

pub use cite::*;
pub use files::*;
pub use imports::*;
pub use notes::*;
pub use projects::*;
pub use refs::*;
pub use terms::*;

use crate::error::{Error, Result};
use crate::model::*;
use crate::store::{self, Store};
use crate::util::now;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct Library {
    pub store: Store,
    pub root: PathBuf,
    pub device: String,
    pub host: String,
    /// Citation key pattern for new records.
    pub citekey_pattern: String,
    /// Library-wide protected terms for title-case changes.
    pub protected_terms: Vec<String>,
    /// Network permission: when false no outbound request is made.
    pub network_allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryInfo {
    pub id: String,
    pub name: String,
    pub root: String,
    pub schema: i64,
    pub read_only: bool,
    pub sync_folder_warning: Option<String>,
    pub counts: std::collections::BTreeMap<String, i64>,
    pub undo: Option<String>,
    pub redo: Option<String>,
}

impl Library {
    /// Open (or create) a library at `root`. Takes the library lock first.
    pub fn open(root: &Path, device: &str, host: &str) -> Result<Library> {
        std::fs::create_dir_all(root)?;
        store::acquire_lock(root, device, host)?;
        let store = match Store::open(root, device) {
            Ok(s) => s,
            Err(e) => {
                store::release_lock(root, device);
                return Err(e);
            }
        };
        let lib = Library {
            store,
            root: root.to_path_buf(),
            device: device.into(),
            host: host.into(),
            citekey_pattern: crate::citekey::DEFAULT_PATTERN.into(),
            protected_terms: vec![],
            network_allowed: true,
        };
        if lib.store.meta("name").is_none() && !lib.store.read_only {
            let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "My Library".into());
            lib.store.set_meta("name", &name)?;
        }
        // Replay any committed events the mirror has not applied yet (crash recovery).
        let _ = lib.store.advance_mirror();
        Ok(lib)
    }

    pub fn close(&self) {
        let _ = self.store.advance_mirror();
        store::release_lock(&self.root, &self.device);
    }

    pub fn heartbeat(&self) -> Result<()> {
        store::heartbeat(&self.root, &self.device, &self.host)
    }

    pub fn info(&self) -> Result<LibraryInfo> {
        let mut counts = std::collections::BTreeMap::new();
        counts.insert("references".into(), self.store.conn.query_row("SELECT COUNT(*) FROM refs WHERE trashed IS NULL", [], |r| r.get(0))?);
        counts.insert("trash".into(), self.store.conn.query_row("SELECT COUNT(*) FROM refs WHERE trashed IS NOT NULL", [], |r| r.get(0))?);
        for t in ["projects", "notes", "annotations", "attachments", "collections", "copy_events"] {
            counts.insert(t.into(), self.store.count(t)?);
        }
        Ok(LibraryInfo {
            id: self.store.meta("library_id").unwrap_or_default(),
            name: self.store.meta("name").unwrap_or_default(),
            root: self.root.to_string_lossy().to_string(),
            schema: crate::store::SCHEMA_VERSION,
            read_only: self.store.read_only,
            sync_folder_warning: store::in_sync_folder(&self.root),
            counts,
            undo: self.store.undo_label(),
            redo: self.store.redo_label(),
        })
    }

    pub fn rename(&self, name: &str) -> Result<()> {
        self.store.set_meta("name", name.trim())
    }

    pub fn files_dir(&self) -> PathBuf {
        self.root.join("files")
    }

    pub fn undo(&mut self) -> Result<Option<String>> {
        let r = self.store.undo()?;
        self.after_change()?;
        self.recompute_copies(None)?;
        Ok(r)
    }

    pub fn redo(&mut self) -> Result<Option<String>> {
        let r = self.store.redo()?;
        self.after_change()?;
        self.recompute_copies(None)?;
        Ok(r)
    }

    /// Work that follows any committed change: advance the mirror and refresh changed-copy
    /// flags. Failures here never undo the committed change; the mirror catches up later.
    pub fn after_change(&mut self) -> Result<()> {
        // Small changes are mirrored at once; large batches are left to the background worker
        // (the mirror is behind a durable checkpoint, so nothing is lost if it lags).
        let checkpoint: i64 = self.store.meta("mirror_seq").and_then(|s| s.parse().ok()).unwrap_or(0);
        if self.last_event_seq() - checkpoint <= 200 {
            let _ = self.store.advance_mirror();
        }
        Ok(())
    }

    pub fn write<T>(&mut self, label: &str, f: impl FnOnce(&mut crate::store::Tx) -> Result<T>) -> Result<T> {
        let out = self.store.write(label, f)?;
        self.after_change()?;
        Ok(out)
    }

    pub fn resolve_id(&self, id: &str) -> Result<String> {
        let mut cur = id.to_string();
        for _ in 0..16 {
            match self.store.get::<Alias>(&cur)? {
                Some(a) => cur = a.target,
                None => return Ok(cur),
            }
        }
        Err(Error::user("An alias chain is too long; the library may need repair."))
    }

    pub fn reference(&self, id: &str) -> Result<Reference> {
        let id = self.resolve_id(id)?;
        self.store.must::<Reference>(&id)
    }

    pub fn references(&self, ids: &[String]) -> Result<Vec<Reference>> {
        let mut out = Vec::new();
        for id in ids {
            if let Ok(r) = self.reference(id) {
                if !out.iter().any(|x: &Reference| x.id == r.id) {
                    out.push(r);
                }
            }
        }
        Ok(out)
    }

    pub fn now() -> String {
        now()
    }
}
