//! End-to-end workflow tests mapped to the release gates.

use prm_core::library::{CiteRequest, Library};
use prm_core::model::*;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("prm-test-{name}-{}", prm_core::util::new_id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn open(name: &str) -> (Library, PathBuf) {
    let root = tmp(name);
    let lib = Library::open(&root.join("lib"), "test-device", "test-host").unwrap();
    (lib, root)
}

fn article(title: &str, year: i32, authors: &[(&str, &str)]) -> Reference {
    let mut r = Reference { rtype: "article-journal".into(), ..Default::default() };
    for (f, g) in authors {
        r.creators.push(Creator { role: "author".into(), name: Name::person(f, g) });
    }
    r.issued = Some(PartialDate::year(year));
    r.fields.insert("title".into(), title.into());
    r.fields.insert("container_title".into(), "Journal of Teaching Practice".into());
    r.fields.insert("volume".into(), "12".into());
    r.fields.insert("page".into(), "1-10".into());
    r
}

fn cite(lib: &Library, m: &str, id: &str) -> String {
    lib.render(&CiteRequest { manuscript_id: m.into(), items: vec![CiteItem { ref_id: id.into(), ..Default::default() }], mode: OutputMode::Parenthetical, ..Default::default() }).unwrap().text
}

#[test]
fn g03_context_change_flags_copies_and_isolates_other_manuscripts() {
    let (mut lib, _root) = open("g03");
    let p = lib.create_project("AI literacy", "apa-7", Some("Faculty article")).unwrap();
    let m1 = p.active_manuscript.clone();
    let m2 = lib.create_manuscript(&p.id, "Other chapter", "apa-7", "en-US").unwrap().id;
    let a = lib.add_reference(article("Teaching with generative AI", 2024, &[("Chen", "Li"), ("Patel", "Maya")]), Some(&p.id)).unwrap();
    let req = CiteRequest { manuscript_id: m1.clone(), items: vec![CiteItem { ref_id: a.id.clone(), ..Default::default() }], mode: OutputMode::Parenthetical, ..Default::default() };
    let out = lib.render(&req).unwrap();
    assert_eq!(out.text, "(Chen & Patel, 2024)");
    let copy1 = lib.record_copy(&req, &out).unwrap();
    // The same source copied in a second manuscript.
    let req2 = CiteRequest { manuscript_id: m2.clone(), ..req.clone() };
    let out2 = lib.render(&req2).unwrap();
    let copy2 = lib.record_copy(&req2, &out2).unwrap();
    // A same-author same-year work enters manuscript 1 only.
    let b = lib.add_reference(article("Faculty reflection in practice", 2024, &[("Chen", "Li"), ("Patel", "Maya")]), Some(&p.id)).unwrap();
    lib.set_manuscript_use(&m1, &[b.id.clone()], UseState::Candidate).unwrap();
    let c1 = lib.store.must::<CopyEvent>(&copy1.id).unwrap();
    assert!(c1.changed, "copy in manuscript 1 must be flagged");
    assert_eq!(c1.payload_text, "(Chen & Patel, 2024)");
    assert_eq!(c1.current_text, "(Chen & Patel, 2024b)");
    assert!(c1.change_reason.contains("Year suffix"), "{}", c1.change_reason);
    let c2 = lib.store.must::<CopyEvent>(&copy2.id).unwrap();
    assert!(!c2.changed, "manuscript 2 has its own context and must not change");
    assert_eq!(cite(&lib, &m2, &a.id), "(Chen & Patel, 2024)");
    // Copy updated text and Mark as replaced are separate actions.
    let replaced = lib.set_copy_status(&copy1.id, CopyStatus::Replaced).unwrap();
    assert_eq!(replaced.payload_text, "(Chen & Patel, 2024b)");
    assert!(!replaced.changed);
    // Editing the cited record flags the copy again with the edit as the reason.
    let mut a2 = lib.reference(&a.id).unwrap();
    a2.creators[1].name.family = "Patil".into();
    lib.update_reference(a2, "user").unwrap();
    let c1 = lib.store.must::<CopyEvent>(&copy1.id).unwrap();
    assert!(c1.changed);
    assert!(c1.change_reason.contains("edited"), "{}", c1.change_reason);
}

#[test]
fn g06_project_isolation() {
    let (mut lib, _root) = open("g06");
    let p1 = lib.create_project("One", "apa-7", None).unwrap();
    let p2 = lib.create_project("Two", "apa-7", None).unwrap();
    let r = lib.add_reference(article("Shared source", 2020, &[("Kim", "Soo")]), Some(&p1.id)).unwrap();
    lib.add_to_project(&p2.id, &[r.id.clone()]).unwrap();
    assert_eq!(lib.store.count("refs").unwrap(), 1, "membership must not duplicate the record");
    let n = lib.save_note(Note { kind: "comment".into(), project_id: p1.id.clone(), body: "Private to project one".into(), ref_ids: vec![r.id.clone()], ..Default::default() }).unwrap();
    assert!(lib.notes(&p2.id, false).unwrap().is_empty(), "project two must not see project one's notes");
    assert_eq!(lib.notes(&p1.id, false).unwrap().len(), 1);
    // Project-scoped search does not show the other project's notes.
    let res = prm_core::search::run(&lib.store.conn, "private", Some(&p2.id), None, 100).unwrap();
    assert!(res.hits.iter().all(|h| h.entity_id != n.id));
    // Inclusion decisions are per project.
    let mut m = lib.membership(&p1.id, &r.id).unwrap().unwrap();
    m.inclusion = "excluded".into();
    lib.update_membership(m).unwrap();
    assert_eq!(lib.membership(&p2.id, &r.id).unwrap().unwrap().inclusion, "candidate");
    // Removing from one project keeps the source and the other project's membership.
    let prev = lib.removal_preview(&p1.id, &r.id).unwrap();
    assert_eq!(prev.project_notes, 1);
    assert_eq!(prev.other_projects, vec!["Two".to_string()]);
    lib.remove_from_project(&p1.id, &[r.id.clone()]).unwrap();
    assert!(lib.membership(&p2.id, &r.id).unwrap().is_some());
    assert!(lib.reference(&r.id).is_ok());
    // A shared edit reports both affected projects.
    lib.add_to_project(&p1.id, &[r.id.clone()]).unwrap();
    assert_eq!(lib.impact(&r.id).unwrap().projects.len(), 2);
}

#[test]
fn g10_mirror_replays_after_crash_and_undo_persists() {
    let root = tmp("g10");
    let libdir = root.join("lib");
    let id;
    {
        let mut lib = Library::open(&libdir, "dev", "host").unwrap();
        let r = lib.add_reference(article("Before crash", 2021, &[("Ng", "Ann")]), None).unwrap();
        id = r.id.clone();
        // Simulate a crash after the database commit but before the mirror write.
        let mirror = libdir.join("mirror").join("refs").join(format!("{}.json", id));
        assert!(mirror.exists());
        std::fs::remove_file(&mirror).unwrap();
        lib.store.set_meta("mirror_seq", "0").unwrap();
        // A crash: the process ends without closing (no mirror flush, lock left behind).
        drop(lib);
        assert!(!mirror.exists());
    }
    let mut lib = Library::open(&libdir, "dev", "host").unwrap();
    assert!(libdir.join("mirror").join("refs").join(format!("{}.json", id)).exists(), "open must replay unapplied events into the mirror");
    // Undo survives a restart.
    assert_eq!(lib.store.undo_label().unwrap(), "Add “Before crash”");
    lib.undo().unwrap();
    assert!(lib.store.get::<Reference>(&id).unwrap().is_none());
    lib.redo().unwrap();
    assert!(lib.store.get::<Reference>(&id).unwrap().is_some());
}

#[test]
fn concurrent_writer_is_refused() {
    let root = tmp("lock");
    let libdir = root.join("lib");
    let _a = Library::open(&libdir, "device-a", "host-a").unwrap();
    match Library::open(&libdir, "device-b", "host-b") {
        Err(prm_core::Error::Locked(_)) => {}
        other => panic!("expected lock refusal, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn g13_full_export_round_trip() {
    let (mut lib, root) = open("g13");
    let p = lib.create_project("Round trip", "apa-7", None).unwrap();
    let r = lib.add_reference(article("Áccented títle", 2019, &[("Müller", "Änne")]), Some(&p.id)).unwrap();
    lib.save_note(Note { kind: "quotation".into(), project_id: p.id.clone(), exact_text: "Exact words.".into(), ref_ids: vec![r.id.clone()], locator: "4".into(), locator_label: "page".into(), ..Default::default() }).unwrap();
    lib.save_collection(Collection { name: "Theory".into(), ref_ids: vec![r.id.clone()], ..Default::default() }).unwrap();
    let pkg = root.join("export.zip");
    prm_core::backup::export_package(&lib, &pkg, true).unwrap();
    let target = root.join("imported");
    prm_core::backup::import_package(&pkg, &target).unwrap();
    let other = Library::open(&target, "dev2", "host").unwrap();
    assert_eq!(lib.store.canonical_dump().unwrap(), other.store.canonical_dump().unwrap(), "all supported entities must compare equal after canonicalization");
}

#[test]
fn backup_verify_restore() {
    let (mut lib, root) = open("backup");
    let r = lib.add_reference(article("Backed up", 2018, &[("Ode", "Ife")]), None).unwrap();
    let f = root.join("paper.pdf");
    std::fs::write(&f, minimal_pdf(2)).unwrap();
    lib.attach_file(&r.id, &f, "managed", "full-text").unwrap();
    let zip = root.join("b.zip");
    prm_core::backup::backup(&lib, &zip, true).unwrap();
    let rep = prm_core::backup::verify(&zip).unwrap();
    assert!(rep.ok, "{:?}", rep.checks);
    let target = root.join("restored");
    prm_core::backup::restore(&zip, &target).unwrap();
    let other = Library::open(&target, "dev3", "host").unwrap();
    assert_eq!(lib.store.canonical_dump().unwrap(), other.store.canonical_dump().unwrap());
    let a = &other.store.all::<Attachment>().unwrap()[0];
    assert!(other.attachment_path(a).exists());
    // Restoring into a non-empty folder is refused.
    assert!(prm_core::backup::restore(&zip, &target).is_err());
}

#[test]
fn import_staging_duplicates_and_batch_undo() {
    let (mut lib, _root) = open("import");
    let p = lib.create_project("Import", "apa-7", None).unwrap();
    let mut existing = article("Teaching with generative AI", 2024, &[("Chen", "Li"), ("Patel", "Maya")]);
    existing.fields.insert("doi".into(), "10.5555/x".into());
    lib.add_reference(existing, None).unwrap();
    let ris = "TY  - JOUR\nAU  - Chen, Li\nAU  - Patel, Maya\nTI  - Teaching with generative AI\nT2  - Journal of Teaching Practice\nPY  - 2024\nDO  - 10.5555/x\nZZ  - kept value\nER  - \nTY  - JOUR\nAU  - Garcia, Rosa\nTI  - Faculty development in the age of AI\nT2  - Journal of Higher Education Practice\nPY  - 2022\nSP  - e1042\nER  - \n";
    let b = lib.stage_import("results.ris", ris, "", "file", &p.id, "", None).unwrap();
    assert_eq!(b.records.len(), 2);
    assert_eq!(b.records[0].status, "duplicate");
    assert_eq!(b.records[1].status, "new");
    assert_eq!(b.records[0].record.overflow.get("ris:ZZ").map(|s| s.as_str()), Some("kept value"));
    assert_eq!(lib.store.count("refs").unwrap(), 1, "staging must not commit");
    let rep = lib.commit_import(&b.id).unwrap();
    assert_eq!(rep.added, 1);
    assert_eq!(rep.skipped, 1);
    assert!(lib.commit_import(&b.id).is_err(), "a committed batch cannot be committed twice");
    assert_eq!(lib.store.count("refs").unwrap(), 2);
    // Batch undo removes records without later work.
    let prev = lib.undo_import_preview(&b.id).unwrap();
    assert_eq!(prev.removable.len(), 1);
    lib.undo_import(&b.id, false).unwrap();
    assert_eq!(lib.info().unwrap().counts["references"], 1);
}

#[test]
fn bibtex_and_csl_round_trip_fields() {
    let mut r = article("Teaching with {AI}", 2024, &[("Chen", "Li")]);
    r.id = "x".into();
    r.citekey = "chen2024".into();
    r.fields.insert("doi".into(), "10.5555/abc".into());
    r.fields.insert("issue".into(), "2".into());
    let bib = prm_core::formats::bibtex::export(&[&r], true, &[]);
    let back = prm_core::formats::parse("bibtex", &bib, None);
    let b = &back.records[0].record;
    assert_eq!(b.f("doi"), "10.5555/abc");
    assert_eq!(b.f("issue"), "2");
    assert_eq!(b.citekey, "chen2024");
    let csl = prm_core::formats::csljson::export(&[&r]);
    let back = prm_core::formats::parse("csljson", &csl, None);
    assert_eq!(back.records[0].record.f("title"), r.f("title"));
    assert_eq!(back.records[0].record.year(), Some(2024));
}

#[test]
fn merge_keeps_aliases_and_moves_memberships() {
    let (mut lib, _root) = open("merge");
    let p = lib.create_project("Merge", "apa-7", None).unwrap();
    let a = lib.add_reference(article("Same work", 2020, &[("Lee", "Ann")]), Some(&p.id)).unwrap();
    let b = lib.add_reference(article("Same work", 2020, &[("Lee", "Ann")]), None).unwrap();
    lib.save_note(Note { kind: "comment".into(), project_id: p.id.clone(), ref_ids: vec![b.id.clone()], body: "on b".into(), ..Default::default() }).unwrap();
    let d = lib.duplicates(0.85).unwrap();
    assert!(d.iter().any(|c| (c.a == a.id && c.b == b.id) || (c.a == b.id && c.b == a.id)));
    lib.merge(&a.id, &[b.id.clone()], &Default::default()).unwrap();
    assert_eq!(lib.resolve_id(&b.id).unwrap(), a.id, "retired id resolves through an alias");
    assert!(lib.notes(&p.id, false).unwrap()[0].ref_ids.contains(&a.id));
    lib.undo().unwrap();
    assert!(lib.store.get::<Reference>(&b.id).unwrap().is_some(), "undo restores the merged record");
    // Not-duplicate decisions are durable.
    lib.decide_duplicate(&a.id, &b.id, "not_duplicate").unwrap();
    assert!(lib.duplicates(0.85).unwrap().is_empty());
}

#[test]
fn search_accents_fields_and_booleans() {
    let (mut lib, _root) = open("search");
    let r1 = lib.add_reference(article("Lärande och reflektion", 2022, &[("Müller", "Änne")]), None).unwrap();
    lib.add_reference(article("Assessment practices", 2019, &[("Kim", "Soo")]), None).unwrap();
    let hits = |q: &str| prm_core::search::run(&lib.store.conn, q, None, None, 100).unwrap().ref_ids;
    assert_eq!(hits("larande"), vec![r1.id.clone()]);
    assert_eq!(hits("author:muller"), vec![r1.id.clone()]);
    assert_eq!(hits("year:2020..2023"), vec![r1.id.clone()]);
    assert_eq!(hits("(larande OR assessment) -kim").len(), 1);
    assert_eq!(hits("\"assessment practices\"").len(), 1);
    assert_eq!(hits("re:/refl.ktion/"), vec![r1.id.clone()]);
    let res = prm_core::search::run(&lib.store.conn, "larande", None, None, 100).unwrap();
    assert!(res.hits[0].snippet.contains("Lärande"), "snippets keep the original spelling");
}

#[test]
fn quotation_needs_locator_or_unresolved_state() {
    let (mut lib, _root) = open("notes");
    let r = lib.add_reference(article("Source", 2020, &[("Ode", "Ife")]), None).unwrap();
    let n = lib.save_note(Note { kind: "quotation".into(), exact_text: "Words.".into(), ref_ids: vec![r.id.clone()], ..Default::default() }).unwrap();
    assert_eq!(n.anchor_state, "unresolved");
    assert!(lib.save_note(Note { kind: "quotation".into(), ..Default::default() }).is_err());
    let mut n2 = n.clone();
    n2.exact_text = "Words, edited.".into();
    let n2 = lib.save_note(n2).unwrap();
    assert!(n2.exact_text_edited, "an edited quotation is never presented as untouched");
}

/// A tiny valid PDF with `pages` blank pages and some text so it is not image-only.
pub fn minimal_pdf(pages: u32) -> Vec<u8> {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = vec![];
    for i in 0..pages {
        let content = doc.add_object(Stream::new(dictionary! {}, format!("BT /F1 12 Tf 72 700 Td (Page {} faculty learning) Tj ET", i + 1).into_bytes()));
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content, "Resources" => resources, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()] });
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => pages as i64 }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

#[test]
fn g08_pdf_annotation_round_trip_without_duplicates() {
    let (mut lib, root) = open("pdf");
    let r = lib.add_reference(article("PDF", 2020, &[("Ode", "Ife")]), None).unwrap();
    let f = root.join("paper.pdf");
    std::fs::write(&f, minimal_pdf(3)).unwrap();
    let original_hash = prm_core::util::sha256_file(&f).unwrap();
    let att = lib.attach_file(&r.id, &f, "managed", "full-text").unwrap();
    assert_eq!(att.page_count, 3);
    assert_eq!(att.state, "ok");
    let mk = |kind: &str, page: u32| Annotation { attachment_id: att.id.clone(), kind: kind.into(), color: "#ffd400".into(), page_index: page, rects: vec![Rect { x: 0.1, y: 0.1, w: 0.4, h: 0.03 }], page_width: 612.0, page_height: 792.0, comment: format!("{kind} comment"), text: "faculty learning".into(), ..Default::default() };
    for (k, p) in [("highlight", 0), ("underline", 1), ("note", 1), ("rect", 2), ("strikeout", 0)] {
        lib.save_annotation(mk(k, p)).unwrap();
    }
    let mut ink = mk("ink", 2);
    ink.ink = vec![vec![[0.1, 0.1], [0.2, 0.2], [0.3, 0.15]]];
    lib.save_annotation(ink).unwrap();
    let out = root.join("annotated.pdf");
    let n = lib.export_annotated_pdf(&att.id, "", &out, false).unwrap();
    assert_eq!(n, 6);
    assert!(out.with_extension("prism-annotations.json").exists());
    assert_eq!(prm_core::util::sha256_file(&f).unwrap(), original_hash, "the original PDF is never modified");
    // Reimporting our own export finds no additions and no duplicates.
    let prev = lib.preview_external_annotations(&att.id, &out).unwrap();
    assert_eq!(prev.additions.len(), 0, "{:?}", prev.additions);
    assert_eq!(prev.unchanged, 6);
    // Flattened copy is a separate explicit choice.
    let flat = root.join("flat.pdf");
    assert_eq!(lib.export_annotated_pdf(&att.id, "", &flat, true).unwrap(), 6);
    assert!(prm_core::pdf::read_annotations(&flat).unwrap().0.is_empty());
}

#[test]
fn project_rating_drives_rating_sort_in_project_scope() {
    let (mut lib, _root) = open("rating");
    let p = lib.create_project("Ratings", "apa-7", None).unwrap();
    let a = lib.add_reference(article("Alpha study", 2020, &[("Adams", "A")]), Some(&p.id)).unwrap();
    let b = lib.add_reference(article("Beta study", 2021, &[("Brown", "B")]), Some(&p.id)).unwrap();
    // Library-wide ratings say A > B; the project's own ratings say B > A.
    lib.set_rating(&[a.id.clone()], 5).unwrap();
    lib.set_rating(&[b.id.clone()], 1).unwrap();
    for (id, r) in [(&a.id, 2u8), (&b.id, 4u8)] {
        let mut m = lib.membership(&p.id, id).unwrap().unwrap();
        m.rating = r;
        lib.update_membership(m).unwrap();
    }
    let q = prm_core::library::ListQuery { project_id: p.id.clone(), sort: "rating".into(), desc: true, limit: 10, ..Default::default() };
    let rows = lib.list_references(&q).unwrap().rows;
    assert_eq!(rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), vec![b.id.clone(), a.id.clone()], "project ratings must decide the order");
    // Library scope still sorts by the library rating.
    let q = prm_core::library::ListQuery { sort: "rating".into(), desc: true, limit: 10, ..Default::default() };
    let rows = lib.list_references(&q).unwrap().rows;
    assert_eq!(rows[0].id, a.id);
}

#[test]
fn protected_terms_survive_in_text_title_case() {
    let (mut lib, _root) = open("protected");
    let mut r = article("teaching with ebooks in class", 2024, &[]);
    r.creators.clear();
    let r = lib.add_reference(r, None).unwrap();
    let plain = cite(&lib, "", &r.id);
    assert!(plain.contains("Ebooks"), "{plain}");
    lib.protected_terms = vec!["ebooks".into()];
    let kept = cite(&lib, "", &r.id);
    assert!(kept.contains("ebooks") && !kept.contains("Ebooks"), "{kept}");
}

#[test]
fn citing_a_version_uses_the_one_marked_to_cite() {
    let (mut lib, _root) = open("versions");
    let mut pre = article("Generative tutors in first-year writing", 2023, &[("Ortiz", "Ana")]);
    pre.rtype = "preprint".into();
    let pre = lib.add_reference(pre, None).unwrap();
    let publ = lib.add_reference(article("Generative tutors in first-year writing", 2024, &[("Ortiz", "Ana")]), None).unwrap();
    lib.link_versions(&[pre.id.clone(), publ.id.clone()], &publ.id).unwrap();
    let req = CiteRequest { items: vec![CiteItem { ref_id: pre.id.clone(), ..Default::default() }], mode: OutputMode::Parenthetical, ..Default::default() };
    let out = lib.render(&req).unwrap();
    assert_eq!(out.text, "(Ortiz, 2024)");
    assert!(out.warnings.iter().any(|w| w.code == "preferred-version"), "the swap must be explained");
    let exact = lib.render(&CiteRequest { exact_version: true, ..req }).unwrap();
    assert_eq!(exact.text, "(Ortiz, 2023)");
    // A bibliography that lists both versions keeps one entry.
    let bib = lib.render(&CiteRequest { mode: OutputMode::Bibliography, set: "selected".into(), ref_ids: vec![pre.id.clone(), publ.id.clone()], ..Default::default() }).unwrap();
    assert_eq!(bib.paragraphs.len(), 1, "{}", bib.text);
}

#[test]
fn kept_bib_file_honors_excluded_fields() {
    let (mut lib, root) = open("bibex");
    let mut r = article("Reflective practice", 2022, &[("Lee", "Kim")]);
    r.fields.insert("abstract".into(), "A long abstract that should stay out.".into());
    lib.add_reference(r, None).unwrap();
    let path = root.join("refs.bib");
    lib.write_bib_file("", &path, false, &["abstract".into()]).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("Reflective practice") && !text.to_lowercase().contains("abstract"), "{text}");
}

#[test]
fn quick_filters_combine_invert_and_count() {
    use prm_core::library::{ListQuery, QuickFilter};
    let (mut lib, _root) = open("facets");
    let a = lib.add_reference(article("Alpha", 2020, &[("Adams", "A")]), None).unwrap();
    let b = lib.add_reference(article("Beta", 2021, &[("Brown", "B")]), None).unwrap();
    let mut book = article("Gamma", 2020, &[("Cole", "C")]);
    book.rtype = "book".into();
    let c = lib.add_reference(book, None).unwrap();
    lib.set_labels(&[a.id.clone(), c.id.clone()], "Theory", true).unwrap();
    let q = |filters: Vec<QuickFilter>| ListQuery { filters, facets: true, limit: 50, ..Default::default() };
    let f = |field: &str, value: &str, negate: bool| QuickFilter { field: field.into(), value: value.into(), negate };
    let all = lib.list_references(&q(vec![])).unwrap();
    let facets = all.facets.unwrap();
    let count = |k: &str, v: &str| facets[k].iter().find(|x| x.value == v).map(|x| x.count).unwrap_or(0);
    assert_eq!(count("type", "article-journal"), 2);
    assert_eq!(count("year", "2020"), 2);
    assert_eq!(count("label", "Theory"), 2);
    assert_eq!(count("pdf", "no"), 3);
    let p = lib.list_references(&q(vec![f("year", "2020", false), f("type", "book", true)])).unwrap();
    assert_eq!(p.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), vec![a.id.clone()], "2020 and not a book");
    let p = lib.list_references(&q(vec![f("label", "Theory", true)])).unwrap();
    assert_eq!(p.total, 1);
    assert_eq!(p.rows[0].id, b.id);
}

#[test]
fn pdf_identifiers_found_in_compressed_first_page_text() {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let body = " 0 -14 Td (Faculty learning benefits from structured opportunities to reflect.) Tj".repeat(30);
    let text = format!("BT /F1 10 Tf 72 700 Td (arXiv:2401.01234v2 [cs.CL] 9 Jan 2024) Tj 0 -14 Td (https://doi.org/10.5555/jtp.2024.77) Tj{body} ET");
    let mut stream = Stream::new(dictionary! {}, text.as_bytes().to_vec());
    stream.compress().unwrap();
    let content = doc.add_object(stream);
    let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content, "Resources" => resources, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()] });
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    assert!(!String::from_utf8_lossy(&out).contains("arXiv:2401"), "the text must be compressed for this test");
    let p = tmp("ids").join("paper.pdf");
    std::fs::write(&p, out).unwrap();
    let info = prm_core::pdf::inspect(&p);
    assert_eq!(info.arxiv, vec!["2401.01234v2".to_string()]);
    assert!(info.dois.contains(&"10.5555/jtp.2024.77".to_string()), "{:?}", info.dois);
}

#[test]
fn search_relevance_typos_spellings_cjk_and_dates() {
    use prm_core::library::ListQuery;
    let (mut lib, _root) = open("search2");
    let mut r1 = article("Notes on assessment", 2021, &[("Ode", "A")]);
    r1.fields.insert("abstract".into(), "Some remarks that mention reflection once in passing.".into());
    let r1 = lib.add_reference(r1, None).unwrap();
    let r2 = lib.add_reference(article("Reflection in teaching practice", 2020, &[("Lee", "K")]), None).unwrap();
    fn ids(lib: &Library, q: &str, sort: &str) -> prm_core::library::RefPage {
        lib.list_references(&ListQuery { search: q.into(), sort: sort.into(), limit: 50, ..Default::default() }).unwrap()
    }
    fn found(lib: &Library, q: &str) -> std::collections::BTreeSet<String> {
        ids(lib, q, "").rows.iter().map(|r| r.id.clone()).collect()
    }
    // Relevance: a title match outranks a passing mention in an abstract.
    let p = ids(&lib, "reflection", "relevance");
    assert_eq!(p.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), vec![r2.id.clone(), r1.id.clone()]);
    // Typo: no exact match, close spelling used and reported.
    let p = ids(&lib, "refelction", "relevance");
    assert_eq!(p.total, 2, "typo should still find both");
    assert_eq!(p.corrections.len(), 1);
    assert!(p.corrections[0].used.contains(&"reflection".to_string()), "{:?}", p.corrections);
    // A correct word is never "corrected".
    assert!(ids(&lib, "reflection", "").corrections.is_empty());
    // Spelling equivalents both ways.
    let m1 = lib.add_reference(article("Lernen", 2019, &[("Müller", "Jan")]), None).unwrap();
    let m2 = lib.add_reference(article("Lehren", 2019, &[("Mueller", "Eva")]), None).unwrap();
    assert!(found(&lib, "Mueller").contains(&m1.id), "Mueller finds Müller");
    assert!(found(&lib, "Müller").contains(&m2.id) || found(&lib, "Müller").contains(&m1.id));
    assert!(found(&lib, "Mueller").contains(&m2.id));
    // Chinese words match as words, not scattered characters.
    let c1 = lib.add_reference(article("在线学习的研究", 2022, &[("Wang", "L")]), None).unwrap();
    let c2 = lib.add_reference(article("学校与复习", 2022, &[("Li", "M")]), None).unwrap();
    let f = found(&lib, "学习");
    assert!(f.contains(&c1.id) && !f.contains(&c2.id), "学习 must match the word, not 学 and 习 apart");
    // Date ranges.
    let mut d = article("Dated", 2021, &[("Kim", "S")]);
    d.issued = Some(PartialDate { year: Some(2021), month: Some(4), day: Some(2), ..Default::default() });
    let d = lib.add_reference(d, None).unwrap();
    assert!(found(&lib, "date:2021-03..2021-06").contains(&d.id));
    assert!(!found(&lib, "date:2021-05..2021-06").contains(&d.id));
    assert!(found(&lib, "date:2021").contains(&r1.id), "a year-only work falls inside its year");
    let today = prm_core::util::now()[..10].to_string();
    assert!(found(&lib, &format!("added:{today}")).contains(&d.id));
}

#[test]
fn migration_groups_ratings_reimport_updates_and_stop() {
    let (mut lib, _root) = open("migrate");
    // Groups (with a nested tree) and ratings from a BibTeX export become collections and ratings.
    let bib = "@article{chen2024, title={Teaching with AI}, author={Chen, Li}, journal={J}, year={2024}, groups={Learning}, ranking={rank4}}\n\
@article{lee2020, title={Reflection}, author={Lee, Kim}, journal={J}, year={2020}, groups={Methods}}\n\
@Comment{jabref-meta: grouping:\n0 AllEntriesGroup:;\n1 StaticGroup:Theory\\;0\\;1\\;\\;\\;\\;;\n2 StaticGroup:Learning\\;0\\;1\\;\\;\\;\\;;\n1 StaticGroup:Methods\\;0\\;1\\;\\;\\;\\;;\n}\n";
    let b = lib.stage_import("lib.bib", bib, "bibtex", "zotero", "", "", None).unwrap();
    assert!(b.groups_found.contains(&"Theory/Learning".to_string()) && b.groups_found.contains(&"Methods".to_string()), "{:?}", b.groups_found);
    assert!(b.make_collections);
    let rep = lib.commit_import(&b.id).unwrap();
    assert_eq!(rep.added, 2);
    let cols: Vec<Collection> = lib.store.all().unwrap();
    let find = |n: &str| cols.iter().find(|c| c.name == n).cloned().unwrap();
    assert_eq!(find("Learning").parent_id, find("Theory").id, "nested group keeps its parent");
    assert_eq!(find("Learning").ref_ids.len(), 1);
    assert_eq!(find("Methods").ref_ids.len(), 1);
    let chen: Vec<Reference> = lib.store.all::<Reference>().unwrap().into_iter().filter(|r| r.title() == "Teaching with AI").collect();
    assert_eq!(chen[0].rating, 4);

    // Re-importing a later export of the same program updates changed records only, and never
    // overwrites the person's own edits.
    let ris1 = "TY  - JOUR\nID  - 101\nTI  - First paper\nAU  - Ode, Ade\nPY  - 2019\nSP  - 1\nEP  - 9\nER  - \nTY  - JOUR\nID  - 102\nTI  - Second paper\nAU  - Ito, Aki\nPY  - 2018\nER  - \n";
    let b1 = lib.stage_import("export1.ris", ris1, "ris", "endnote", "", "", None).unwrap();
    lib.commit_import(&b1.id).unwrap();
    let first = lib.store.all::<Reference>().unwrap().into_iter().find(|r| r.title() == "First paper").unwrap();
    let mut edited = first.clone();
    edited.fields.insert("volume".into(), "7".into());
    lib.update_reference(edited, "user").unwrap();
    let ris2 = "TY  - JOUR\nID  - 101\nTI  - First paper\nAU  - Ode, Ade\nPY  - 2019\nSP  - 1\nEP  - 12\nVL  - 8\nER  - \nTY  - JOUR\nID  - 102\nTI  - Second paper\nAU  - Ito, Aki\nPY  - 2018\nER  - \n";
    let b2 = lib.stage_import("export2.ris", ris2, "ris", "endnote", "", "", None).unwrap();
    let st: Vec<String> = b2.records.iter().map(|r| r.status.clone()).collect();
    assert_eq!(st, vec!["update".to_string(), "unchanged".to_string()], "{:?}", b2.records.iter().map(|r| &r.issues).collect::<Vec<_>>());
    assert!(b2.records[0].issues.iter().any(|i| i.contains("Your edits are kept")), "{:?}", b2.records[0].issues);
    let rep = lib.commit_import(&b2.id).unwrap();
    assert_eq!((rep.updated, rep.added), (1, 0));
    let after = lib.store.must::<Reference>(&first.id).unwrap();
    assert_eq!(after.f("page"), "1-12", "changed field updated");
    assert_eq!(after.f("volume"), "7", "the person's edit is kept");
    let undo = lib.undo_import_preview(&b2.id).unwrap();
    assert!(undo.removable.is_empty() && undo.dependent.is_empty(), "undoing a re-import never removes the updated record");

    // Stopping a large import leaves nothing staged.
    let mut big = String::new();
    for i in 0..120 {
        big.push_str(&format!("TY  - JOUR\nID  - s{i}\nTI  - Stop test {i}\nAU  - Roe, R\nPY  - 2001\nER  - \n"));
    }
    lib.progress = Some(std::sync::Arc::new(|_p: &str, _d: usize, _t: usize| prm_core::library::IMPORT_CANCEL.store(true, std::sync::atomic::Ordering::Relaxed)));
    let before = lib.import_batches().unwrap().len();
    let err = lib.stage_import("big.ris", &big, "ris", "", "", "", None).unwrap_err();
    assert!(err.to_string().contains("stopped"), "{err}");
    assert_eq!(lib.import_batches().unwrap().len(), before);
    lib.progress = None;
}

#[test]
fn custom_fields_types_validation_exchange_and_sharing() {
    use prm_core::model::{CustomFieldDef, CustomType};
    let (mut lib, _root) = open("custom");
    let method = lib.save_custom_field(CustomFieldDef { name: "Study design".into(), kind: "list".into(), options: vec!["RCT".into(), "Cohort".into(), "Qualitative".into()], ..Default::default() }).unwrap();
    assert_eq!(method.id, "study_design");
    let n = lib.save_custom_field(CustomFieldDef { name: "Sample size".into(), kind: "number".into(), ..Default::default() }).unwrap();
    let coded = lib.save_custom_field(CustomFieldDef { name: "Coded".into(), kind: "boolean".into(), default: "no".into(), ..Default::default() }).unwrap();
    assert!(lib.save_custom_field(CustomFieldDef { name: "study design".into(), kind: "text".into(), ..Default::default() }).is_err(), "names are unique");
    assert!(lib.save_custom_field(CustomFieldDef { name: "Empty list".into(), kind: "list".into(), ..Default::default() }).is_err());
    // A custom type based on a built-in type; output uses the base type.
    let brief = lib.save_custom_type(CustomType { label: "Policy brief".into(), base: "report".into(), ..Default::default() }).unwrap();
    assert!(brief.id.starts_with("custom-") && !brief.fields.is_empty());
    let mut r = article("Phonics outcomes", 2022, &[("Ode", "A")]);
    r.rtype = "report".into();
    r.subtype = brief.id.clone();
    r.custom.insert(method.id.clone(), serde_json::json!("RCT"));
    r.custom.insert(n.id.clone(), serde_json::json!("1,250"));
    let r = lib.add_reference(r, None).unwrap();
    assert_eq!(r.custom[&n.id], serde_json::json!(1250.0), "a comma between thousands");
    assert_eq!(prm_core::library::check_custom_value(&n, &serde_json::json!("2,5")).unwrap(), serde_json::json!(2.5), "a decimal comma");
    assert_eq!(r.custom[&coded.id], serde_json::json!(false), "default filled");
    let mut bad = r.clone();
    bad.custom.insert(method.id.clone(), serde_json::json!("Survey"));
    assert!(lib.update_reference(bad, "user").is_err(), "a value outside the list is refused");
    let row = lib.list_references(&prm_core::library::ListQuery { limit: 5, ..Default::default() }).unwrap().rows.remove(0);
    assert_eq!(row.type_label, "Policy brief");
    assert!(lib.list_references(&prm_core::library::ListQuery { search: "RCT".into(), limit: 5, ..Default::default() }).unwrap().total == 1, "custom values are searchable");
    // CSL JSON and CSV carry the values.
    let csl = prm_core::formats::csljson::export(&[&r]);
    assert!(csl.contains("\"study_design\""));
    let csv = prm_core::formats::csvfmt::export(&[&r]).unwrap();
    assert!(csv.contains("custom:study_design"));
    // A second library receives the definitions file and the records.
    let defs = lib.export_definitions().unwrap().to_string();
    let (mut other, _r2) = open("custom2");
    let rep = other.import_definitions(&defs).unwrap();
    assert_eq!(rep["fields_added"], 3);
    assert_eq!(rep["types_added"], 1);
    let b = other.stage_import("x.json", &csl, "csljson", "", "", "", None).unwrap();
    assert_eq!(b.records[0].record.custom.get("study_design"), Some(&serde_json::json!("RCT")));
    // Deleting a field with values needs the explicit choice to remove them.
    assert!(lib.delete_custom_field(&method.id, false).is_err());
    assert_eq!(lib.delete_custom_field(&method.id, true).unwrap(), 1);
}
