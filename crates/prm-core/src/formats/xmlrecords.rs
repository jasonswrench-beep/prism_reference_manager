//! XML record format written by several desktop reference managers
//! (`<xml><records><record><ref-type name="Journal Article">...`). Text inside each element may
//! be wrapped in `<style>` elements; only the text is kept. Unmapped elements are preserved.

use super::{pages, person, type_from_label, ParseOutput, Parsed};
use crate::model::{PartialDate, Reference};
use quick_xml::events::Event;
use quick_xml::Reader;

/// Collect (path, text, attributes-of-leaf) triples for one record.
fn records(text: &str) -> Result<Vec<(String, Vec<(String, String, String)>)>, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut cur: Option<(usize, Vec<(String, String, String)>)> = None;
    let mut buf_text = String::new();
    let mut leaf_attr = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "record" {
                    cur = Some((reader.buffer_position() as usize, Vec::new()));
                }
                if name != "style" {
                    if !buf_text.trim().is_empty() {
                        if let Some((_, v)) = cur.as_mut() {
                            v.push((stack.join("/"), buf_text.trim().to_string(), leaf_attr.clone()));
                        }
                    }
                    buf_text.clear();
                    stack.push(name.clone());
                    leaf_attr = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == b"name")
                        .map(|a| String::from_utf8_lossy(&a.value).to_string())
                        .unwrap_or_default();
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "style" {
                    continue;
                }
                if let Some((_, v)) = cur.as_mut() {
                    if !buf_text.trim().is_empty() {
                        v.push((stack.join("/"), buf_text.trim().to_string(), leaf_attr.clone()));
                    }
                }
                buf_text.clear();
                leaf_attr.clear();
                stack.pop();
                if name == "record" {
                    if let Some((start, v)) = cur.take() {
                        out.push((format!("record@{start}"), v));
                    }
                }
            }
            Ok(Event::Text(t)) => {
                let s = t.unescape().map(|c| c.to_string()).unwrap_or_default();
                buf_text.push_str(&s);
            }
            Ok(Event::CData(t)) => buf_text.push_str(&String::from_utf8_lossy(&t)),
            Ok(Event::Eof) => break,
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok(out)
}

pub fn parse(text: &str) -> ParseOutput {
    let mut out = ParseOutput::default();
    let recs = match records(text) {
        Ok(r) => r,
        Err(e) => {
            out.errors.push(format!("The XML could not be read: {e}"));
            return out;
        }
    };
    for (_, items) in recs {
        let mut r = Reference::default();
        r.origin = "import".into();
        let mut sp = String::new();
        let mut raw = String::new();
        let mut ref_type_name = String::new();
        let mut year = String::new();
        let mut pubdate = String::new();
        for (path, val, attr) in &items {
            raw.push_str(&format!("{path}: {val}\n"));
            let leaf = path.rsplit('/').next().unwrap_or("");
            let parent = path.rsplit('/').nth(1).unwrap_or("");
            let set = |r: &mut Reference, k: &str| {
                r.fields.entry(k.to_string()).or_insert_with(|| val.clone());
            };
            match (parent, leaf) {
                (_, "ref-type") => ref_type_name = if attr.is_empty() { val.clone() } else { attr.clone() },
                ("authors", "author") => r.creators.push(person("author", val)),
                ("secondary-authors", "author") => r.creators.push(person("editor", val)),
                ("tertiary-authors", "author") => r.creators.push(person("series-editor", val)),
                ("subsidiary-authors", "author") | ("translated-authors", "author") => r.creators.push(person("translator", val)),
                ("titles", "title") => set(&mut r, "title"),
                ("titles", "secondary-title") => set(&mut r, "container_title"),
                ("titles", "tertiary-title") => set(&mut r, "collection_title"),
                ("titles", "short-title") => set(&mut r, "short_title"),
                ("periodical", "full-title") => set(&mut r, "container_title"),
                (_, "pages") => sp = val.clone(),
                (_, "volume") => set(&mut r, "volume"),
                (_, "number") => set(&mut r, "issue"),
                (_, "issue") => set(&mut r, "issue"),
                (_, "edition") => set(&mut r, "edition"),
                ("dates", "year") => year = val.clone(),
                ("pub-dates", "date") => pubdate = val.clone(),
                (_, "publisher") => set(&mut r, "publisher"),
                (_, "pub-location") => set(&mut r, "publisher_place"),
                (_, "isbn") => {
                    let digits: String = val.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
                    if digits.len() >= 10 { set(&mut r, "isbn") } else { set(&mut r, "issn") }
                }
                (_, "electronic-resource-num") => set(&mut r, "doi"),
                ("related-urls", "url") | ("web-urls", "url") => set(&mut r, "url"),
                ("pdf-urls", "url") => {
                    r.overflow.insert("xml:pdf-url".into(), val.clone());
                }
                (_, "abstract") => set(&mut r, "abstract"),
                ("keywords", "keyword") => r.tags.push(val.clone()),
                (_, "notes") => set(&mut r, "note"),
                (_, "language") => set(&mut r, "language"),
                (_, "work-type") => set(&mut r, "genre"),
                (_, "label") => {
                    r.overflow.insert("xml:label".into(), val.clone());
                }
                (_, "research-notes") => {
                    r.overflow.insert("xml:research-notes".into(), val.clone());
                }
                (_, "access-date") => r.accessed = PartialDate::parse(val),
                (_, "rec-number") => {
                    r.overflow.insert("xml:rec-number".into(), val.clone());
                }
                (_, "database") => {
                    r.overflow.insert("xml:database".into(), val.clone());
                }
                (_, "source-app") | (_, "foreign-keys") | (_, "key") => {}
                _ => {
                    let k = format!("xml:{leaf}");
                    let e = r.overflow.entry(k).or_default();
                    if !e.is_empty() {
                        e.push_str("; ");
                    }
                    e.push_str(val);
                }
            }
        }
        r.rtype = match ref_type_name.to_lowercase().as_str() {
            "journal article" => "article-journal",
            "book section" => "chapter",
            "edited book" => "edited-book",
            "book" | "electronic book" => "book",
            "conference paper" | "conference proceedings" => "paper-conference",
            "thesis" => "thesis",
            "report" => "report",
            "web page" => "webpage",
            "dataset" => "dataset",
            "computer program" => "software",
            other => type_from_label(other),
        }
        .to_string();
        let p = pages(&sp, "");
        if !p.is_empty() {
            r.fields.insert("page".into(), p);
        }
        if !year.is_empty() {
            let mut d = PartialDate::parse(&year).unwrap_or_default();
            if let Some(pd) = PartialDate::parse(&format!("{pubdate} {year}")) {
                d.month = pd.month;
                d.day = pd.day;
            }
            r.issued = Some(d);
        }
        out.records.push(Parsed { raw, record: r, issues: vec![], confidence: 1.0 });
    }
    out
}

/// Export the documented Prism XML record format (one element per field).
pub fn export(refs: &[&Reference]) -> String {
    use crate::util::xml_escape as e;
    let mut o = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<references xmlns=\"urn:prism-reference-manager:records:1\">\n");
    for r in refs {
        o.push_str(&format!("  <reference id=\"{}\" type=\"{}\" citekey=\"{}\">\n", e(&r.id), e(&r.rtype), e(&r.citekey)));
        for c in &r.creators {
            let n = &c.name;
            o.push_str(&format!(
                "    <creator role=\"{}\" kind=\"{}\"><family>{}</family><given>{}</given><particle>{}</particle><suffix>{}</suffix><literal>{}</literal></creator>\n",
                e(&c.role),
                e(&format!("{:?}", n.kind).to_lowercase()),
                e(&n.family),
                e(&n.given),
                e(&n.particle),
                e(&n.suffix),
                e(&n.literal)
            ));
        }
        if let Some(d) = &r.issued {
            o.push_str(&format!("    <issued>{}</issued>\n", e(&if d.status.is_empty() { d.iso() } else { d.status.clone() })));
        }
        for (k, v) in &r.fields {
            o.push_str(&format!("    <field name=\"{}\">{}</field>\n", e(k), e(v)));
        }
        for t in &r.tags {
            o.push_str(&format!("    <tag>{}</tag>\n", e(t)));
        }
        for (k, v) in &r.overflow {
            o.push_str(&format!("    <extra name=\"{}\">{}</extra>\n", e(k), e(v)));
        }
        o.push_str("  </reference>\n");
    }
    o.push_str("</references>\n");
    o
}
