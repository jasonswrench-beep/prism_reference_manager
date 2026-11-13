//! The text mirror (spec DAT-004, DAT-005): a library rebuilt from it equals the original, and
//! edits made to it outside the program are validated, previewed and applied only when chosen.

use prm_core::library::Library;
use prm_core::model::*;
use std::path::PathBuf;

fn open(name: &str) -> (Library, PathBuf) {
    let root = std::env::temp_dir().join(format!("prm-mirror-{name}-{}", prm_core::util::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    (Library::open(&root.join("lib"), "dev", "host").unwrap(), root)
}

fn source(lib: &mut Library, title: &str) -> Reference {
    let mut r = Reference { rtype: "article-journal".into(), ..Default::default() };
    r.fields.insert("title".into(), title.into());
    r.creators.push(Creator { role: "author".into(), name: Name::person("Ode", "I.") });
    r.issued = Some(PartialDate::year(2020));
    lib.add_reference(r, None).unwrap()
}

fn mirror_file(lib: &Library, table: &str, id: &str, ext: &str) -> PathBuf {
    lib.store.mirror_path(table, id).with_extension(ext)
}

fn edit_json(path: &PathBuf, f: impl FnOnce(&mut serde_json::Value)) {
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    f(&mut v);
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

#[test]
fn a_library_rebuilt_from_its_mirror_equals_the_original() {
    let (mut lib, root) = open("rebuild");
    let r = source(&mut lib, "Reflective teaching");
    let p = lib.create_project("Article", "apa-7", Some("Draft")).unwrap();
    lib.add_to_project(&p.id, &[r.id.clone()]).unwrap();
    lib.save_note(Note { kind: "note".into(), project_id: p.id.clone(), title: "Idea".into(), body: "Links to @x".into(), ref_ids: vec![r.id.clone()], ..Default::default() }).unwrap();
    lib.store.advance_mirror().unwrap();
    let rebuilt = prm_core::mirror::reconstruct(&lib.store.mirror_dir(), &root.join("rebuilt")).unwrap();
    assert_eq!(rebuilt.canonical_dump().unwrap(), lib.store.canonical_dump().unwrap());
}

#[test]
fn an_outside_edit_is_previewed_and_applied_only_when_chosen() {
    let (mut lib, _root) = open("edit");
    let r = source(&mut lib, "Reflective teaching");
    assert!(lib.scan_mirror().unwrap().changes.is_empty(), "the program's own files are not changes");
    let f = mirror_file(&lib, "refs", &r.id, "json");
    edit_json(&f, |v| v["fields"]["title"] = "Reflective teaching in practice".into());
    let scan = lib.scan_mirror().unwrap();
    assert_eq!(scan.changes.len(), 1);
    let c = &scan.changes[0];
    assert_eq!((c.kind.as_str(), c.problem.as_str()), ("edited", ""));
    assert_eq!(c.fields.len(), 1);
    assert_eq!((c.fields[0].field.as_str(), c.fields[0].before.as_str(), c.fields[0].after.as_str()), ("fields.title", "Reflective teaching", "Reflective teaching in practice"));
    assert_eq!(lib.reference(&r.id).unwrap().title(), "Reflective teaching", "nothing changes until chosen");
    let rep = lib.apply_mirror(&[c.key.clone()], &[]).unwrap();
    assert_eq!(rep.applied, 1);
    let now = lib.reference(&r.id).unwrap();
    assert_eq!(now.title(), "Reflective teaching in practice");
    assert_eq!(now.provenance["title"].source, "mirror");
    assert!(lib.scan_mirror().unwrap().changes.is_empty());
    assert!(!lib.store.mirror_dir().join("_external").join("refs").exists() || std::fs::read_dir(lib.store.mirror_dir().join("_external").join("refs")).unwrap().next().is_none());
    lib.undo().unwrap();
    assert_eq!(lib.reference(&r.id).unwrap().title(), "Reflective teaching", "applying is one undoable step");
}

#[test]
fn a_declined_edit_is_replaced_by_the_library_version() {
    let (mut lib, _root) = open("decline");
    let r = source(&mut lib, "Kept");
    let f = mirror_file(&lib, "refs", &r.id, "json");
    let original = std::fs::read_to_string(&f).unwrap();
    edit_json(&f, |v| v["fields"]["title"] = "Changed outside".into());
    let key = lib.scan_mirror().unwrap().changes[0].key.clone();
    let rep = lib.apply_mirror(&[], &[key]).unwrap();
    assert_eq!(rep.declined, 1);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), original);
    assert_eq!(lib.reference(&r.id).unwrap().title(), "Kept");
    assert!(lib.scan_mirror().unwrap().changes.is_empty());
}

#[test]
fn invalid_files_are_explained_and_never_applied() {
    let (mut lib, _root) = open("invalid");
    let r = source(&mut lib, "Valid");
    let f = mirror_file(&lib, "refs", &r.id, "json");
    std::fs::write(&f, "{ not json").unwrap();
    let c = lib.scan_mirror().unwrap().changes.remove(0);
    assert!(c.problem.contains("not valid JSON"), "{}", c.problem);
    let rep = lib.apply_mirror(&[c.key.clone()], &[]).unwrap();
    assert_eq!((rep.applied, rep.failed.len()), (0, 1));
    // Declining puts the library's version back; then a record that does not fit its type.
    lib.apply_mirror(&[], &[c.key.clone()]).unwrap();
    edit_json(&f, |v| v["creators"] = "not a list".into());
    let c = lib.scan_mirror().unwrap().changes.remove(0);
    assert!(c.problem.contains("does not fit its type"), "{}", c.problem);
    assert_eq!(lib.reference(&r.id).unwrap().title(), "Valid");
}

#[test]
fn note_markdown_edits_and_deletions() {
    let (mut lib, _root) = open("notes");
    let n = lib.save_note(Note { kind: "note".into(), title: "Plan".into(), body: "First draft.".into(), ..Default::default() }).unwrap();
    let md = mirror_file(&lib, "notes", &n.id, "md");
    let text = std::fs::read_to_string(&md).unwrap();
    std::fs::write(&md, text.replace("First draft.", "Second draft,\nwith two lines.")).unwrap();
    let c = lib.scan_mirror().unwrap().changes.remove(0);
    assert_eq!(c.fields.iter().map(|f| f.field.as_str()).collect::<Vec<_>>(), vec!["body"]);
    lib.apply_mirror(&[c.key], &[]).unwrap();
    assert_eq!(lib.store.must::<Note>(&n.id).unwrap().body, "Second draft,\nwith two lines.");

    // A source file deleted outside: offered as moving the source to the trash.
    let r = source(&mut lib, "Deleted outside");
    std::fs::remove_file(mirror_file(&lib, "refs", &r.id, "json")).unwrap();
    let c = lib.scan_mirror().unwrap().changes.into_iter().find(|c| c.kind == "deleted").unwrap();
    assert_eq!(c.id, r.id);
    lib.apply_mirror(&[c.key], &[]).unwrap();
    assert!(lib.reference(&r.id).unwrap().trashed_at.is_some());
}

#[test]
fn an_outside_edit_survives_a_later_change_in_the_program() {
    let (mut lib, _root) = open("both");
    let r = source(&mut lib, "Original");
    let f = mirror_file(&lib, "refs", &r.id, "json");
    edit_json(&f, |v| v["fields"]["title"] = "Edited outside".into());
    // The program changes the same record before the outside edit was reviewed.
    let mut e = lib.reference(&r.id).unwrap();
    e.fields.insert("volume".into(), "7".into());
    lib.update_reference(e, "user").unwrap();
    let kept = lib.store.mirror_dir().join("_external").join("refs").join(f.file_name().unwrap());
    assert!(kept.exists(), "the outside version was kept before the mirror was rewritten");
    let c = lib.scan_mirror().unwrap().changes.remove(0);
    assert!(c.superseded);
    assert!(c.fields.iter().any(|x| x.field == "fields.title" && x.after == "Edited outside"));
    lib.apply_mirror(&[], &[c.key]).unwrap();
    assert!(!kept.exists());
    assert_eq!(lib.reference(&r.id).unwrap().title(), "Original");
}
