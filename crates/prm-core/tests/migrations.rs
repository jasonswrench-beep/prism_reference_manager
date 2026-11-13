//! Library format upgrades (spec DAT-003): verified backup first, all steps or none, history
//! kept, newer formats read-only, and an upgrade can be reversed from its backup.

use prm_core::model::*;
use prm_core::store::{self, Migration, Store};
use rusqlite::Connection;
use std::path::PathBuf;

fn root(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("prm-mig-{name}-{}", prm_core::util::new_id())).join("lib");
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A library in format 1 (before the schema history), with one source.
fn v1_library(name: &str) -> (PathBuf, String) {
    let r = root(name);
    let (mut s, _) = Store::open_with(&r, "t", &[], 1).unwrap();
    let mut rf = Reference { id: prm_core::util::new_id(), rtype: "book".into(), revision: 1, ..Default::default() };
    rf.fields.insert("title".into(), "Teaching as reflection".into());
    let id = rf.id.clone();
    s.write("add", |tx| tx.put(&rf)).unwrap();
    // Format 1 had no schema history table.
    s.conn.execute_batch("DROP TABLE schema_history").unwrap();
    drop(s);
    (r, id)
}

fn title(s: &Store, id: &str) -> String {
    s.must::<Reference>(id).unwrap().title().to_string()
}

fn upper_titles(conn: &Connection) -> prm_core::error::Result<()> {
    conn.execute("UPDATE refs SET data = json_set(data, '$.fields.title', upper(json_extract(data, '$.fields.title')))", [])?;
    Ok(())
}

fn failing(_: &Connection) -> prm_core::error::Result<()> {
    Err(prm_core::error::Error::user("simulated failure"))
}

#[test]
fn upgrade_backs_up_runs_every_step_and_records_history() {
    let (r, id) = v1_library("up");
    let mut steps: Vec<Migration> = store::MIGRATIONS.iter().map(|m| Migration { to: m.to, label: m.label, run: m.run }).collect();
    steps.push(Migration { to: 3, label: "Capitalize titles (test)", run: upper_titles });
    let (s, rep) = Store::open_with(&r, "t", &steps, 3).unwrap();
    assert_eq!(rep.migrated_from, Some(1));
    assert_eq!(s.schema_version(), 3);
    assert_eq!(title(&s, &id), "TEACHING AS REFLECTION");
    let hist = s.schema_history().unwrap();
    assert_eq!(hist.iter().map(|h| h.version).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert!(hist[2].backup.starts_with("backups/pre-migration-schema1-"));
    // The backup is a complete, readable copy in the old format.
    let b = Connection::open(r.join(&hist[2].backup)).unwrap();
    let v: String = b.query_row("SELECT value FROM meta WHERE key='schema_version'", [], |x| x.get(0)).unwrap();
    assert_eq!(v, "1");
    let n: i64 = b.query_row("SELECT COUNT(*) FROM refs", [], |x| x.get(0)).unwrap();
    assert_eq!(n, 1);
    assert_eq!(store::upgrade_backups(&r).len(), 1);
}

#[test]
fn a_failing_step_changes_nothing() {
    let (r, id) = v1_library("fail");
    let steps = [Migration { to: 2, label: "ok", run: |_| Ok(()) }, Migration { to: 3, label: "Broken step", run: failing }];
    let err = Store::open_with(&r, "t", &steps, 3).err().expect("the upgrade fails");
    assert!(err.to_string().contains("Broken step"), "{err}");
    let (s, rep) = Store::open_with(&r, "t", &[], 1).unwrap();
    assert_eq!(rep.migrated_from, None);
    assert_eq!(s.schema_version(), 1, "no step was kept");
    assert_eq!(title(&s, &id), "Teaching as reflection");
    assert_eq!(store::upgrade_backups(&r).len(), 1, "the verified backup stays");
}

#[test]
fn a_newer_format_opens_read_only() {
    let (r, id) = v1_library("new");
    let steps = [Migration { to: 2, label: "ok", run: |_| Ok(()) }];
    drop(Store::open_with(&r, "t", &steps, 2).unwrap());
    let (s, rep) = Store::open_with(&r, "t", &[], 1).unwrap();
    assert!(rep.too_new && s.read_only);
    assert_eq!(title(&s, &id), "Teaching as reflection", "still readable");
    assert!(s.conn.execute("DELETE FROM refs", []).is_err(), "writes are refused");
}

#[test]
fn an_upgrade_can_be_reversed_from_its_backup() {
    let (r, id) = v1_library("rev");
    let steps = [Migration { to: 2, label: "Capitalize titles (test)", run: upper_titles }];
    let (s, _) = Store::open_with(&r, "t", &steps, 2).unwrap();
    assert_eq!(title(&s, &id), "TEACHING AS REFLECTION");
    drop(s);
    let b = store::upgrade_backups(&r);
    let kept = store::revert_upgrade(&r, &b[0].path).unwrap();
    assert!(kept.exists(), "the upgraded file is kept beside the library");
    let (s, _) = Store::open_with(&r, "t", &[], 1).unwrap();
    assert_eq!(s.schema_version(), 1);
    assert_eq!(title(&s, &id), "Teaching as reflection");
    assert!(store::revert_upgrade(&r, "../library.db").is_err());
}

#[test]
fn the_program_upgrades_a_format_1_library() {
    let (r, id) = v1_library("real");
    let lib = prm_core::library::Library::open(&r, "t", "h").unwrap();
    let info = lib.info().unwrap();
    assert_eq!((info.schema, info.opened.migrated_from), (store::SCHEMA_VERSION, Some(1)));
    assert_eq!(lib.reference(&id).unwrap().title(), "Teaching as reflection");
    assert_eq!(lib.store.schema_history().unwrap().len(), 2);
}
