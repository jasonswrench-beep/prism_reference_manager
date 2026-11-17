//! The exchange file for other Prism research tools (`docs/exchange-format.md`).
//!
//! `exchange_file_follows_the_specification` builds a small project, exports it, and checks the
//! file against the rules of the specification. With `PRM_WRITE_EXCHANGE_SAMPLE=1` it also writes
//! the file to `docs/examples/prism-exchange.json`, the sample other tools test their readers on.
//! `committed_sample_follows_the_specification` holds that sample to the same rules.

use prm_core::library::Library;
use prm_core::model::*;
use serde_json::Value;
use std::path::PathBuf;

fn open(name: &str) -> (Library, PathBuf) {
    let root = std::env::temp_dir().join(format!("prm-test-{name}-{}", prm_core::util::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let lib = Library::open(&root.join("lib"), "test-device", "test-host").unwrap();
    (lib, root)
}

fn work(rtype: &str, title: &str, year: i32, authors: &[(&str, &str)], container: &str) -> Reference {
    let mut r = Reference { rtype: rtype.into(), ..Default::default() };
    for (f, g) in authors {
        r.creators.push(Creator { role: "author".into(), name: Name::person(f, g) });
    }
    r.issued = Some(PartialDate::year(year));
    r.fields.insert("title".into(), title.into());
    if !container.is_empty() {
        r.fields.insert("container_title".into(), container.into());
    }
    r
}

const KINDS: &[&str] = &["quotation", "paraphrase", "summary", "comment", "question", "task", "evidence", "thought", "note"];
const ANCHOR_STATES: &[&str] = &["resolved", "unresolved", "no-locator", ""];
const ORIGINS: &[&str] = &["user", "ai", "agent", "import"];
const INCLUSION: &[&str] = &["candidate", "included", "undecided", ""];
const SOURCE_DERIVED: &[&str] = &["quotation", "paraphrase", "evidence"];

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or_else(|| panic!("{k} must be a string in {v}"))
}

/// The rules of `docs/exchange-format.md` a writer must keep. Returns the problems found.
fn check(v: &Value) -> Vec<String> {
    let mut bad = Vec::new();
    let m = &v["manifest"];
    if m["format"] != "prism-family-exchange" {
        bad.push("manifest.format".into());
    }
    let version = m["version"].as_str().unwrap_or("");
    if !version.starts_with("1.") {
        bad.push(format!("manifest.version {version}"));
    }
    for k in ["producer", "producer_version", "created", "library_id", "note"] {
        if !m[k].is_string() {
            bad.push(format!("manifest.{k}"));
        }
    }
    if !m["project"]["id"].is_string() || !m["project"]["title"].is_string() {
        bad.push("manifest.project".into());
    }
    if !["project", "excerpts"].contains(&m["sources_scope"].as_str().unwrap_or("")) {
        bad.push("manifest.sources_scope".into());
    }
    let sources = v["sources"].as_array().cloned().unwrap_or_default();
    let mut ids = std::collections::BTreeSet::new();
    for src in &sources {
        let id = s(src, "id");
        if !ids.insert(id.to_string()) {
            bad.push(format!("source {id} twice"));
        }
        let _ = s(src, "citekey");
        if !INCLUSION.contains(&s(src, "inclusion")) {
            bad.push(format!("source {id} inclusion {}", s(src, "inclusion")));
        }
        let csl = &src["csl"];
        if csl["id"] != src["id"] || !csl["type"].is_string() {
            bad.push(format!("source {id} csl id or type"));
        }
    }
    for e in v["excerpts"].as_array().cloned().unwrap_or_default() {
        let id = s(&e, "id");
        let kind = s(&e, "type");
        if !KINDS.contains(&kind) {
            bad.push(format!("excerpt {id} type {kind}"));
        }
        for sid in e["source_ids"].as_array().cloned().unwrap_or_default() {
            if !ids.contains(sid.as_str().unwrap_or("")) {
                bad.push(format!("excerpt {id} names {sid}, which is not in the file"));
            }
        }
        if !e["exact_text_edited"].is_boolean() {
            bad.push(format!("excerpt {id} exact_text_edited"));
        }
        let _ = (s(&e, "exact_text"), s(&e, "user_text"), s(&e, "annotation_id"));
        let _ = (s(&e["locator"], "label"), s(&e["locator"], "value"));
        let anchor = s(&e, "anchor_state");
        if !ANCHOR_STATES.contains(&anchor) || (SOURCE_DERIVED.contains(&kind) && anchor.is_empty()) {
            bad.push(format!("excerpt {id} anchor_state {anchor:?} for {kind}"));
        }
        if !ORIGINS.contains(&s(&e, "origin")) {
            bad.push(format!("excerpt {id} origin"));
        }
        if kind == "quotation" && s(&e, "exact_text").trim().is_empty() {
            bad.push(format!("quotation {id} without exact text"));
        }
    }
    bad
}

#[test]
fn exchange_file_follows_the_specification() {
    let (mut lib, _root) = open("exchange");
    let p = lib.create_project("Waiting for the bus: a literature review", "apa-7", None).unwrap();
    let mut a = work("article-journal", "Bus reliability and daily stress", 2021, &[("Hernandez", "Maria"), ("Li", "Wei")], "Journal of Transit Studies");
    a.fields.insert("abstract".into(), "Late buses raise riders' reported stress.".into());
    a.fields.insert("volume".into(), "14".into());
    a.fields.insert("page".into(), "33-51".into());
    let a = lib.add_reference(a, Some(&p.id)).unwrap();
    let b = lib.add_reference(work("book", "Waiting", 2019, &[("Okafor", "Ada")], ""), Some(&p.id)).unwrap();
    // Two records of one work; the second is merged into the first after a note names it.
    let c = lib.add_reference(work("report", "Rider survey 2022", 2022, &[("Transit Authority", "")], ""), Some(&p.id)).unwrap();
    let c2 = lib.add_reference(work("report", "Rider survey 2022 (copy)", 2022, &[("Transit Authority", "")], ""), Some(&p.id)).unwrap();
    // A source with no excerpt, and one the review excluded.
    let d = lib.add_reference(work("article-journal", "Timetables and trust", 2018, &[("Novak", "Petra")], "Urban Mobility"), Some(&p.id)).unwrap();
    let x = lib.add_reference(work("article-journal", "Ferries in winter", 2017, &[("Berg", "Ola")], "Maritime Notes"), Some(&p.id)).unwrap();
    let mut mx = lib.membership(&p.id, &x.id).unwrap().unwrap();
    mx.inclusion = "excluded".into();
    lib.update_membership(mx).unwrap();
    let mut md = lib.membership(&p.id, &d.id).unwrap().unwrap();
    md.inclusion = "included".into();
    lib.update_membership(md).unwrap();

    let q = lib
        .save_note(Note { kind: "quotation".into(), project_id: p.id.clone(), ref_ids: vec![a.id.clone()], exact_text: "Riders waited a mean of 14 minutes.".into(), body: "Compare with our own counts.".into(), locator_label: "page".into(), locator: "36".into(), ..Default::default() })
        .unwrap();
    lib.save_note(Note { kind: "paraphrase".into(), project_id: p.id.clone(), ref_ids: vec![a.id.clone(), b.id.clone()], body: "Both link delay to stress.".into(), origin: "ai".into(), ..Default::default() }).unwrap();
    lib.save_note(Note { kind: "summary".into(), project_id: p.id.clone(), ref_ids: vec![c2.id.clone()], body: "A third of riders gave up on the route.".into(), locator_label: "section".into(), locator: "2".into(), ..Default::default() }).unwrap();
    lib.save_note(Note { kind: "thought".into(), project_id: p.id.clone(), body: "Ask the interviewees about winter.".into(), ..Default::default() }).unwrap();
    let gone = lib.save_note(Note { kind: "comment".into(), project_id: p.id.clone(), ref_ids: vec![b.id.clone()], body: "Trashed, so never exported.".into(), ..Default::default() }).unwrap();
    lib.trash_note(&gone.id).unwrap();
    lib.merge(&c.id, &[c2.id.clone()], &Default::default()).unwrap();

    let v: Value = serde_json::from_slice(&lib.export_family_exchange(&p.id, &[], true).unwrap()).unwrap();
    let bad = check(&v);
    assert!(bad.is_empty(), "{bad:#?}");
    assert_eq!(v["manifest"]["version"], "1.1");
    assert_eq!(v["manifest"]["sources_scope"], "project");
    let src_ids: Vec<&str> = v["sources"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(src_ids.contains(&d.id.as_str()), "a source with no excerpt is in the file");
    assert!(!src_ids.contains(&x.id.as_str()), "an excluded source is left out");
    assert!(!src_ids.contains(&c2.id.as_str()), "a merged-away record is not a source");
    assert_eq!(src_ids.len(), 4);
    let ex = v["excerpts"].as_array().unwrap();
    assert_eq!(ex.len(), 4, "the trashed note is left out");
    let summary = ex.iter().find(|e| e["type"] == "summary").unwrap();
    assert_eq!(summary["source_ids"], serde_json::json!([c.id]), "an excerpt names the surviving record after a merge");
    let quote = ex.iter().find(|e| e["id"] == q.id.as_str()).unwrap();
    assert_eq!(quote["anchor_state"], "resolved");
    assert_eq!(quote["locator"]["value"], "36");
    let para = ex.iter().find(|e| e["type"] == "paraphrase").unwrap();
    assert_eq!(para["anchor_state"], "unresolved", "no locator");
    assert_eq!(para["origin"], "ai");
    assert_eq!(ex.iter().find(|e| e["type"] == "thought").unwrap()["anchor_state"], "");
    let inc = |id: &str| v["sources"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap()["inclusion"].as_str().unwrap().to_string();
    assert_eq!(inc(&d.id), "included");

    // Only the sources the excerpts name.
    let w: Value = serde_json::from_slice(&lib.export_family_exchange(&p.id, &[], false).unwrap()).unwrap();
    assert!(check(&w).is_empty());
    assert_eq!(w["manifest"]["sources_scope"], "excerpts");
    assert_eq!(w["sources"].as_array().unwrap().len(), 3);

    if std::env::var("PRM_WRITE_EXCHANGE_SAMPLE").is_ok() {
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/prism-exchange.json");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let mut text = serde_json::to_string_pretty(&v).unwrap();
        text.push('\n');
        std::fs::write(&out, text).unwrap();
    }
}

#[test]
fn committed_sample_follows_the_specification() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/prism-exchange.json");
    let v: Value = serde_json::from_slice(&std::fs::read(&path).expect("docs/examples/prism-exchange.json")).unwrap();
    let bad = check(&v);
    assert!(bad.is_empty(), "{bad:#?}");
    assert_eq!(v["manifest"]["producer"], "Prism Reference Manager");
}

/// The examples in `docs/local-api.md` ("Call reference") are what the program does.
#[test]
fn local_api_doc_examples_hold() {
    use prm_core::library::CiteRequest;
    let (mut lib, _root) = open("apidoc");
    let r: Reference = serde_json::from_value(serde_json::json!({
        "type": "article-journal",
        "creators": [{"role": "author", "name": {"family": "Hernandez", "given": "Maria"}}, {"role": "author", "name": {"family": "Li", "given": "Wei"}}],
        "issued": {"year": 2021},
        "fields": {"title": "Bus reliability and daily stress", "container_title": "Journal of Transit Studies", "volume": "14", "page": "33-51"}
    }))
    .unwrap();
    let r = lib.add_reference(r, None).unwrap();
    let req: CiteRequest = serde_json::from_value(serde_json::json!({"mode": "parenthetical", "items": [{"ref_id": r.id, "locator_label": "page", "locator": "36"}], "style": "apa-7"})).unwrap();
    assert_eq!(lib.render(&req).unwrap().text, "(Hernandez & Li, 2021, p. 36)");
    let req: CiteRequest = serde_json::from_value(serde_json::json!({"mode": "reference", "ref_ids": [r.id], "style": "apa-7"})).unwrap();
    let out = lib.render(&req).unwrap();
    assert_eq!(out.text, "Hernandez, M., & Li, W. (2021). Bus reliability and daily stress. Journal of Transit Studies, 14, 33–51.");
    assert_eq!(out.style, "apa-7");
    let org: Name = serde_json::from_value(serde_json::json!({"kind": "organization", "literal": "Transit Authority"})).unwrap();
    assert_eq!(org.kind, NameKind::Organization);
}
