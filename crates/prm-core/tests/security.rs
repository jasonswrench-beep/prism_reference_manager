//! Data the program did not make (export packages, restored backups) must not reach files outside
//! the library, network shares, or programs. Findings H2, H3 and M3 of the 2026-11-14 review.

use prm_core::library::Library;
use prm_core::model::*;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn root(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("prm-sec-{name}-{}", prm_core::util::new_id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn sample_pdf() -> Vec<u8> {
    b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n".to_vec()
}

/// Rewrite a package: replace data/<table>.json and fix the manifest's checksums, as someone
/// making a hostile package would.
fn tamper(src: &Path, dest: &Path, edit: impl Fn(&str, &mut Vec<serde_json::Value>)) {
    let mut z = zip::ZipArchive::new(std::fs::File::open(src).unwrap()).unwrap();
    let mut files: Vec<(String, Vec<u8>)> = vec![];
    for i in 0..z.len() {
        let mut e = z.by_index(i).unwrap();
        let mut b = vec![];
        e.read_to_end(&mut b).unwrap();
        files.push((e.name().unwrap().to_string(), b));
    }
    for (name, bytes) in files.iter_mut() {
        if let Some(t) = name.strip_prefix("data/").and_then(|n| n.strip_suffix(".json")) {
            let mut rows: Vec<serde_json::Value> = serde_json::from_slice(bytes).unwrap();
            edit(t, &mut rows);
            *bytes = serde_json::to_vec(&rows).unwrap();
        }
    }
    let sums: std::collections::BTreeMap<String, String> = files.iter().filter(|(n, _)| n != "manifest.json").map(|(n, b)| (n.clone(), prm_core::util::sha256_hex(b))).collect();
    for (name, bytes) in files.iter_mut() {
        if name == "manifest.json" {
            let mut m: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let keep: std::collections::BTreeMap<String, String> = m["checksums"].as_object().unwrap().keys().filter_map(|k| sums.get(k).map(|v| (k.clone(), v.clone()))).collect();
            m["checksums"] = serde_json::to_value(keep).unwrap();
            *bytes = serde_json::to_vec(&m).unwrap();
        }
    }
    let mut w = zip::ZipWriter::new(std::fs::File::create(dest).unwrap());
    for (name, bytes) in &files {
        w.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap();
}

#[test]
fn a_hostile_package_cannot_delete_or_reach_files_outside_the_library() {
    let r = root("pkg");
    // The person's own file that the package tries to delete.
    let victim = r.join("victim.txt");
    std::fs::write(&victim, "keep me").unwrap();
    let mut src = Library::open(&r.join("maker"), "d", "h").unwrap();
    let mut s = Reference { rtype: "book".into(), ..Default::default() };
    s.fields.insert("title".into(), "Old".into());
    let s = src.add_reference(s, None).unwrap();
    let f = r.join("paper.pdf");
    std::fs::write(&f, sample_pdf()).unwrap();
    src.attach_file(&s.id, &f, "managed", "full-text").unwrap();
    src.attach_file(&s.id, &f, "linked", "full-text").unwrap();
    let pkg = r.join("pkg.zip");
    prm_core::backup::export_package(&src, &pkg, true).unwrap();
    src.close();

    let victim_s = victim.to_string_lossy().to_string();
    let hostile = r.join("hostile.zip");
    tamper(&pkg, &hostile, |t, rows| match t {
        // The source was "trashed" long ago, so opening the library purges it at once.
        "refs" => {
            for row in rows.iter_mut() {
                row["trashed_at"] = "2001-01-01T00:00:00Z".into();
            }
        }
        "attachments" => {
            let base = rows[0].clone();
            rows.clear();
            for (i, (mode, path)) in [("managed", victim_s.as_str()), ("managed", "../../victim.txt"), ("linked", r"\\attacker-host\share\paper.pdf"), ("linked", r"\\?\C:\Windows\notepad.exe")].iter().enumerate() {
                let mut a = base.clone();
                a["id"] = format!("bad-{i}").into();
                a["mode"] = (*mode).into();
                a["path"] = (*path).into();
                rows.push(a);
            }
        }
        _ => {}
    });
    let target = r.join("imported");
    let m = prm_core::backup::import_package(&hostile, &target).unwrap();
    assert_eq!(m.skipped.len(), 4, "every unsafe attachment row is refused: {:?}", m.skipped);

    let mut lib = Library::open(&target, "d", "h").unwrap();
    assert!(lib.store.all::<Attachment>().unwrap().is_empty());
    lib.empty_trash_older_than(1).unwrap();
    assert!(victim.exists(), "nothing outside the library was deleted");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
}

#[test]
fn the_file_sinks_refuse_paths_outside_the_files_folder_whatever_the_database_says() {
    let r = root("sinks");
    let victim = r.join("victim.txt");
    std::fs::write(&victim, "keep me").unwrap();
    let mut lib = Library::open(&r.join("lib"), "d", "h").unwrap();
    let s = lib.add_reference(Reference { rtype: "book".into(), ..Default::default() }, None).unwrap();
    // A row written straight into the database (as a hostile library folder could hold).
    let a = Attachment { id: "x".into(), ref_id: s.id.clone(), hash: "a".repeat(64), path: victim.to_string_lossy().to_string(), mode: "managed".into(), mime: "text/plain".into(), original_name: "x.txt".into(), ..Default::default() };
    lib.write("plant", |tx| tx.put(&a)).unwrap();
    assert!(lib.attachment_path(&a).starts_with(lib.files_dir()), "a managed path never resolves outside the files folder");
    assert!(lib.inside_files(&a.path).is_err());
    assert!(lib.inside_files("../x").is_err());
    assert!(lib.inside_files("ab/cdef/file.pdf").is_ok());
    lib.trash(&[s.id.clone()]).unwrap();
    lib.purge(&[s.id.clone()], true).unwrap();
    assert!(victim.exists());
    assert!(lib.rename_files(&["x".into()], "[auth]", true).is_err() || victim.exists());
    assert!(victim.exists());
}

#[test]
fn a_restored_backup_loses_unsafe_attachment_rows() {
    let r = root("restore");
    let mut lib = Library::open(&r.join("lib"), "d", "h").unwrap();
    let s = lib.add_reference(Reference { rtype: "book".into(), ..Default::default() }, None).unwrap();
    let f = r.join("paper.pdf");
    std::fs::write(&f, sample_pdf()).unwrap();
    let good = lib.attach_file(&s.id, &f, "managed", "full-text").unwrap();
    let bad = Attachment { id: "unc".into(), ref_id: s.id.clone(), hash: "b".repeat(64), path: r"\\attacker-host\share\x.pdf".into(), mode: "linked".into(), original_name: "x.pdf".into(), ..Default::default() };
    lib.write("plant", |tx| tx.put(&bad)).unwrap();
    let zip = r.join("b.zip");
    prm_core::backup::backup(&lib, &zip, true).unwrap();
    lib.close();
    let rep = prm_core::backup::restore(&zip, &r.join("restored")).unwrap();
    assert!(rep.checks.iter().any(|(t, _, d)| t.contains("removed") && d.contains("x.pdf")), "{:?}", rep.checks);
    let back = Library::open(&r.join("restored"), "d", "h").unwrap();
    let ids: Vec<String> = back.store.all::<Attachment>().unwrap().into_iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![good.id.clone()]);
    let restored = back.attachment_path(&back.store.must::<Attachment>(&good.id).unwrap());
    assert!(restored.exists());
    if cfg!(windows) {
        let mut ads = restored.as_os_str().to_owned();
        ads.push(":Zone.Identifier");
        assert!(std::fs::read_to_string(PathBuf::from(ads)).unwrap().contains("ZoneId=3"), "restored files carry the downloaded-file mark");
    }
}

#[test]
fn only_unchanged_documents_open_in_another_program() {
    let r = root("open");
    let mut lib = Library::open(&r.join("lib"), "d", "h").unwrap();
    let s = lib.add_reference(Reference { rtype: "book".into(), ..Default::default() }, None).unwrap();
    let pdf = r.join("paper.pdf");
    std::fs::write(&pdf, sample_pdf()).unwrap();
    let a = lib.attach_file(&s.id, &pdf, "linked", "full-text").unwrap();
    assert!(lib.openable_path(&a).is_ok(), "a matching PDF still opens");
    // A runnable file, even with a document-looking display name.
    let exe = r.join("report.pdf.cmd");
    std::fs::write(&exe, "@echo off\r\n").unwrap();
    let mut e = lib.attach_file(&s.id, &exe, "linked", "full-text").unwrap();
    e.original_name = "report.pdf".into();
    assert!(lib.openable_path(&e).is_err());
    // A document whose content changed since it was attached.
    std::fs::write(&pdf, b"%PDF-1.4 changed").unwrap();
    assert!(lib.openable_path(&a).is_err());
}

#[test]
fn a_library_someone_else_made_cannot_run_its_own_sql_or_write_outside_the_mirror() {
    let r = root("db");
    let path = r.join("lib");
    {
        let mut lib = Library::open(&path, "d", "h").unwrap();
        lib.add_reference(Reference { rtype: "book".into(), ..Default::default() }, None).unwrap();
        lib.close();
    }
    // Someone edits the database: a trigger that would run on every later edit, and a change
    // record whose table name climbs out of the mirror folder.
    {
        let c = rusqlite::Connection::open(path.join("library.db")).unwrap();
        c.execute_batch("CREATE TRIGGER planted AFTER UPDATE ON refs BEGIN DELETE FROM notes; END; CREATE VIEW planted_view AS SELECT 1;").unwrap();
        c.execute("INSERT INTO events(id,grp,time,device,actor,tbl,entity_id,before,after) VALUES('e','g','t','d','a','../../escaped','x',NULL,'{}')", []).unwrap();
    }
    let lib = Library::open(&path, "d", "h").unwrap();
    let left: i64 = lib.store.conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type IN ('trigger','view')", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 0, "planted triggers and views are removed on opening");
    lib.store.advance_mirror().unwrap();
    assert!(!r.join("escaped").exists() && !path.join("escaped").exists(), "no folder was made outside the mirror");
}

#[test]
fn a_lookup_whose_switch_is_off_sends_nothing() {
    for (kind, id, switch) in [("doi", "10.1000/xyz", "doi"), ("pmid", "31234567", "pubmed"), ("pmcid", "PMC1234567", "pubmed"), ("arxiv", "2401.01234", "arxiv"), ("isbn", "9780000000000", "isbn"), ("url", "https://example.org/", "webpage")] {
        let mut sent = 0;
        let res = prm_core::lookup::resolve(kind, id, &|k| k != switch, &mut |_, _, _, _, _| sent += 1);
        assert_eq!(sent, 0, "{kind}: no request is made when its switch ({switch}) is off");
        assert!(!res.ok && res.message.contains("turned off"), "{kind}: {}", res.message);
    }
    // A PubMed id that is not a number is refused before anything is sent.
    let mut sent = 0;
    let res = prm_core::lookup::resolve("pmid", "123&db=other", &|_| true, &mut |_, _, _, _, _| sent += 1);
    assert_eq!(sent, 0);
    assert!(!res.ok);
}

#[test]
fn a_package_cannot_put_its_own_markup_in_the_copy_history() {
    use prm_core::library::CiteRequest;
    let r = root("copies");
    let mut src = Library::open(&r.join("maker"), "d", "h").unwrap();
    let p = src.create_project("Copies", "apa-7", None).unwrap();
    let mut b = Reference { rtype: "book".into(), ..Default::default() };
    b.creators.push(Creator { role: "author".into(), name: Name::person("Okafor", "Ada") });
    b.issued = Some(PartialDate::year(2019));
    b.fields.insert("title".into(), "Waiting".into());
    let b = src.add_reference(b, Some(&p.id)).unwrap();
    let req = CiteRequest { manuscript_id: p.active_manuscript.clone(), items: vec![CiteItem { ref_id: b.id.clone(), ..Default::default() }], mode: OutputMode::Parenthetical, ..Default::default() };
    let out = src.render(&req).unwrap();
    src.record_copy(&req, &out).unwrap();
    let pkg = r.join("pkg.zip");
    prm_core::backup::export_package(&src, &pkg, false).unwrap();
    src.close();

    const PLANTED: &str = "<img src=x class=planted><form>Sign in</form>";
    let hostile = r.join("hostile.zip");
    tamper(&pkg, &hostile, |t, rows| {
        if t != "copy_events" {
            return;
        }
        let good = rows[0].clone();
        let plant = |id: &str, f: &dyn Fn(&mut serde_json::Value)| {
            let mut v = good.clone();
            v["id"] = id.into();
            v["changed"] = true.into();
            v["current_html"] = PLANTED.into();
            v["current_text"] = PLANTED.into();
            f(&mut v);
            v
        };
        *rows = vec![
            plant("renders", &|v| v["payload_text"] = "(Old, 1999)".into()),
            plant("no-manuscript", &|v| v["manuscript_id"] = "gone".into()),
            plant("no-source", &|v| v["items"][0]["ref_id"] = "gone".into()),
            plant("discarded", &|v| v["status"] = "discarded".into()),
            plant("unknown", &|v| v["imported_unknown"] = true.into()),
        ];
    });
    let dest = r.join("imported");
    prm_core::backup::import_package(&hostile, &dest).unwrap();
    let mut lib = Library::open(&dest, "d", "h").unwrap();
    assert_eq!(lib.store.all::<CopyEvent>().unwrap().len(), 5);
    // Copying or replacing never uses what the row holds, even before the rows are recomputed.
    assert_eq!(lib.current_copy_output("renders").unwrap().text, "(Okafor, 2019)");
    assert!(!lib.current_copy_output("renders").unwrap().html.contains("planted"));
    assert!(lib.current_copy_output("no-manuscript").is_err());
    assert!(lib.set_copy_status("no-manuscript", CopyStatus::Replaced).is_err(), "nothing to replace it with");
    lib.recompute_copies(None).unwrap();
    for c in lib.store.all::<CopyEvent>().unwrap() {
        assert_eq!(c.current_html, "", "{}", c.id);
        assert!(!c.current_text.contains('<'), "{}: {}", c.id, c.current_text);
    }
    assert_eq!(lib.store.must::<CopyEvent>("renders").unwrap().current_text, "(Okafor, 2019)");
    let done = lib.set_copy_status("renders", CopyStatus::Replaced).unwrap();
    assert_eq!(done.payload_text, "(Okafor, 2019)");
    assert!(!done.payload_html.contains("planted"));
    // The view shows stored copy text as text only.
    let view = include_str!("../../../app/ui/js/views/cite.js");
    assert!(!view.contains("current_html") && !view.contains("payload_html"), "stored HTML is never put in the page");
}
