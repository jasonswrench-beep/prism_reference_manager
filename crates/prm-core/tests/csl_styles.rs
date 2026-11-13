//! CSL styles through the embedded processor: every bundled style renders, formatting and
//! locale reach the output, imported and dependent styles work, and copies pin the style.

use prm_core::library::{CiteRequest, Library};
use prm_core::model::*;

fn open(name: &str) -> (Library, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("prm-csl-{name}-{}", prm_core::util::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    (Library::open(&root.join("lib"), "dev", "host").unwrap(), root)
}

fn article(lib: &mut Library, title: &str, year: i32, authors: &[(&str, &str)]) -> Reference {
    let mut r = Reference { rtype: "article-journal".into(), ..Default::default() };
    for (f, g) in authors {
        r.creators.push(Creator { role: "author".into(), name: Name::person(f, g) });
    }
    r.issued = Some(PartialDate::year(year));
    r.fields.insert("title".into(), title.into());
    r.fields.insert("container_title".into(), "Journal of Teaching Practice".into());
    r.fields.insert("volume".into(), "12".into());
    r.fields.insert("issue".into(), "2".into());
    r.fields.insert("page".into(), "115-132".into());
    r.fields.insert("doi".into(), "10.5555/jtp.2024.12".into());
    lib.add_reference(r, None).unwrap()
}

fn manuscript(lib: &mut Library, style: &str, locale: &str) -> String {
    let p = lib.create_project("Styles", "apa-7", None).unwrap();
    lib.create_manuscript(&p.id, "Paper", style, locale).unwrap().id
}

fn paren(lib: &Library, m: &str, id: &str) -> prm_core::library::CiteOutput {
    lib.render(&CiteRequest { manuscript_id: m.into(), items: vec![CiteItem { ref_id: id.into(), ..Default::default() }], mode: OutputMode::Parenthetical, ..Default::default() }).unwrap()
}

fn bib(lib: &Library, m: &str, ids: &[String]) -> prm_core::library::CiteOutput {
    lib.render(&CiteRequest { manuscript_id: m.into(), mode: OutputMode::Bibliography, set: "selected".into(), ref_ids: ids.to_vec(), ..Default::default() }).unwrap()
}

#[test]
fn every_bundled_style_renders_a_citation_and_reference() {
    let (mut lib, _r) = open("all");
    let a = article(&mut lib, "Teaching with generative AI", 2024, &[("Chen", "Li"), ("Patel", "Maya")]);
    let styles = lib.styles();
    assert!(styles.iter().any(|s| s.id == "apa-7" && s.source == "builtin"));
    let csl: Vec<_> = styles.iter().filter(|s| s.source == "bundled").collect();
    assert!(csl.len() >= 70, "only {} bundled styles", csl.len());
    assert!(!csl.iter().any(|s| s.title.contains("APA Style")), "APA must use the tested composer only");
    let mut problems = vec![];
    for s in &csl {
        let m = manuscript(&mut lib, &s.id, "en-US");
        let c = paren(&lib, &m, &a.id);
        let b = bib(&lib, &m, &[a.id.clone()]);
        assert_eq!(c.assurance, "untested");
        if c.text.trim().is_empty() || c.warnings.iter().any(|w| w.code == "style-failed") {
            problems.push(format!("{} citation: {:?}", s.id, c.warnings));
        }
        if b.text.trim().is_empty() || !b.text.to_lowercase().contains("chen") {
            problems.push(format!("{} bibliography: {}", s.id, b.text));
        }
        assert!(c.warnings.iter().any(|w| w.code == "style-untested"), "{} must say it is untested", s.id);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn numeric_style_numbers_and_formats() {
    let (mut lib, _r) = open("ieee");
    let a = article(&mut lib, "Teaching with generative AI", 2024, &[("Chen", "Li"), ("Patel", "Maya")]);
    let b = article(&mut lib, "Faculty reflection", 2023, &[("Baker", "Ann")]);
    let m = manuscript(&mut lib, "csl:institute-of-electrical-and-electronics-engineers", "en-US");
    lib.set_manuscript_use(&m, &[a.id.clone(), b.id.clone()], UseState::Candidate).unwrap();
    let ca = paren(&lib, &m, &a.id);
    let cb = paren(&lib, &m, &b.id);
    assert!(ca.text.starts_with('[') && cb.text.starts_with('['), "{} / {}", ca.text, cb.text);
    assert_ne!(ca.text, cb.text);
    let out = bib(&lib, &m, &[a.id.clone(), b.id.clone()]);
    assert_eq!(out.paragraphs.len(), 2, "{}", out.text);
    assert!(out.runs.iter().any(|r| r.italic && r.text.contains("Journal of Teaching Practice")), "journal italic: {:?}", out.runs);
    assert!(out.html.contains("<i>"), "{}", out.html);
}

#[test]
fn citation_locale_changes_terms() {
    let (mut lib, _r) = open("locale");
    let a = article(&mut lib, "Teaching with generative AI", 2024, &[("Chen", "Li"), ("Patel", "Maya")]);
    let en = manuscript(&mut lib, "csl:chicago-author-date", "en-US");
    let de = manuscript(&mut lib, "csl:chicago-author-date", "de-DE");
    let t_en = paren(&lib, &en, &a.id).text;
    let t_de = paren(&lib, &de, &a.id).text;
    assert!(t_en.contains("and"), "{t_en}");
    assert!(t_de.contains("und"), "{t_de}");
}

#[test]
fn locators_reach_csl_output() {
    let (mut lib, _r) = open("loc");
    let a = article(&mut lib, "Teaching with generative AI", 2024, &[("Chen", "Li")]);
    let m = manuscript(&mut lib, "csl:chicago-author-date", "en-US");
    let out = lib.render(&CiteRequest { manuscript_id: m, items: vec![CiteItem { ref_id: a.id.clone(), locator_label: "page".into(), locator: "121".into(), ..Default::default() }], mode: OutputMode::Parenthetical, ..Default::default() }).unwrap();
    assert!(out.text.contains("121"), "{}", out.text);
}

const CUSTOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <info><title>Test Lab Style</title><id>http://example.org/styles/test-lab</id><updated>2026-01-01T00:00:00+00:00</updated><rights license="https://creativecommons.org/licenses/by-sa/3.0/">CC BY-SA 3.0</rights></info>
  <citation><layout prefix="{" suffix="}" delimiter="; "><names variable="author"><name form="short"/></names><date variable="issued" prefix=" "><date-part name="year"/></date></layout></citation>
  <bibliography><layout><names variable="author"><name/></names><text variable="title" prefix=". " font-style="italic"/></layout></bibliography>
</style>"#;

const DEPENDENT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="de-DE">
  <info><title>Example Journal</title><id>http://example.org/styles/example-journal</id>
    <link href="PARENT" rel="independent-parent"/>
    <updated>2026-01-01T00:00:00+00:00</updated></info>
</style>"#;

#[test]
fn imported_and_dependent_styles() {
    let (mut lib, _r) = open("import");
    let a = article(&mut lib, "Teaching with generative AI", 2024, &[("Chen", "Li"), ("Patel", "Maya")]);
    assert!(lib.import_style("<not a style/>").is_err());
    let info = lib.import_style(CUSTOM).unwrap();
    assert_eq!(info.source, "imported");
    assert_eq!(info.assurance, "custom_unreviewed");
    assert!(info.license.contains("CC BY-SA"), "{}", info.license);
    assert!(lib.styles().iter().any(|s| s.id == info.id));
    let m = manuscript(&mut lib, &info.id, "en-US");
    let c = paren(&lib, &m, &a.id);
    assert!(c.text.starts_with('{') && c.text.contains("Chen"), "{}", c.text);
    let b = bib(&lib, &m, &[a.id.clone()]);
    assert!(b.runs.iter().any(|r| r.italic && r.text.contains("Teaching")), "{:?}", b.runs);
    // A dependent style resolves its bundled parent and forces its own locale.
    let parent = lib.styles().into_iter().find(|s| s.id == "csl:chicago-author-date").unwrap().csl_id;
    let dep = lib.import_style(&DEPENDENT.replace("PARENT", &parent)).unwrap();
    assert!(dep.parent.contains("chicago-author-date"));
    let md = manuscript(&mut lib, &dep.id, "en-US");
    let t = paren(&lib, &md, &a.id).text;
    assert!(t.contains("und"), "{t}");
    // A style in use cannot be removed; an unused one can.
    assert!(lib.remove_style(&info.id).is_err());
    let unused = lib.import_style(&CUSTOM.replace("Test Lab Style", "Test Lab Style v2")).unwrap();
    assert_ne!(unused.id, info.id, "a changed file is a new version; the old one stays pinned");
    lib.remove_style(&unused.id).unwrap();
    assert!(!lib.styles().iter().any(|s| s.id == unused.id));
}

#[test]
fn copies_record_the_style_version_used() {
    let (mut lib, _r) = open("pin");
    let a = article(&mut lib, "Teaching with generative AI", 2024, &[("Chen", "Li")]);
    let m = manuscript(&mut lib, "csl:modern-language-association", "en-US");
    let req = CiteRequest { manuscript_id: m.clone(), items: vec![CiteItem { ref_id: a.id.clone(), ..Default::default() }], mode: OutputMode::Parenthetical, ..Default::default() };
    let out = lib.render(&req).unwrap();
    let copy = lib.record_copy(&req, &out).unwrap();
    let info = lib.styles().into_iter().find(|s| s.id == "csl:modern-language-association").unwrap();
    assert_eq!(copy.style_hash, info.hash);
    assert!(copy.processor.starts_with("hayagriva"));
    // APA still renders through the tested composer.
    let apa = manuscript(&mut lib, "apa-7", "en-US");
    assert_eq!(paren(&lib, &apa, &a.id).text, "(Chen, 2024)");
}
