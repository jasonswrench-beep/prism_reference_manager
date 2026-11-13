//! RIS tagged format.

use super::{pages, person, ParseOutput, Parsed};
use crate::model::{PartialDate, Reference};

fn ris_type(ty: &str) -> &'static str {
    for t in crate::types::TYPES {
        if t.ris == ty {
            return t.id;
        }
    }
    match ty {
        "ABST" | "INPR" | "EJOUR" | "JFULL" => "article-journal",
        "EBOOK" => "book",
        "ECHAP" => "chapter",
        "SER" => "book",
        "ICOMM" => "webpage",
        "THES" => "thesis",
        "DBASE" | "AGGR" => "dataset",
        "SLIDE" => "presentation",
        _ => "document",
    }
}

pub fn parse(text: &str) -> ParseOutput {
    let mut out = ParseOutput::default();
    let mut cur: Option<(Reference, String, String, String)> = None; // record, raw, sp, ep
    let mut last_tag = String::new();
    for line in text.lines() {
        let l = line.trim_end_matches('\r');
        let (tag, val) = if l.len() >= 6 && &l[2..6] == "  - " {
            (l[..2].to_string(), l[6..].trim().to_string())
        } else if l.len() >= 5 && l.get(2..5) == Some("  -") {
            (l[..2].to_string(), String::new())
        } else {
            // Continuation line of the previous tag.
            if let Some((r, raw, _, _)) = cur.as_mut() {
                raw.push_str(l);
                raw.push('\n');
                if !l.trim().is_empty() {
                    let key = match last_tag.as_str() {
                        "AB" | "N2" => Some("abstract"),
                        "TI" | "T1" => Some("title"),
                        "N1" => Some("note"),
                        _ => None,
                    };
                    if let Some(k) = key {
                        let e = r.fields.entry(k.into()).or_default();
                        e.push(' ');
                        e.push_str(l.trim());
                    }
                }
            }
            continue;
        };
        if tag == "TY" {
            let mut r = Reference::default();
            r.rtype = ris_type(&val).to_string();
            r.overflow.insert("ris:TY".into(), val.clone());
            cur = Some((r, String::new(), String::new(), String::new()));
        }
        let Some((r, raw, sp, ep)) = cur.as_mut() else { continue };
        raw.push_str(l);
        raw.push('\n');
        last_tag = tag.clone();
        let set = |r: &mut Reference, k: &str, v: &str| {
            if !v.is_empty() && !r.fields.contains_key(k) {
                r.fields.insert(k.into(), v.into());
            }
        };
        match tag.as_str() {
            "TY" => {}
            "AU" | "A1" => r.creators.push(person("author", &val)),
            "A2" | "ED" => r.creators.push(person("editor", &val)),
            "A3" => r.creators.push(person("series-editor", &val)),
            "A4" => r.creators.push(person("translator", &val)),
            "TI" | "T1" | "CT" => set(r, "title", &val),
            "T2" | "JO" | "JF" | "BT" => {
                if r.rtype == "book" && tag == "BT" {
                    set(r, "title", &val)
                } else {
                    set(r, "container_title", &val)
                }
            }
            "JA" | "J2" => set(r, "container_title_short", &val),
            "T3" => set(r, "collection_title", &val),
            "ST" => set(r, "short_title", &val),
            "VL" => set(r, "volume", &val),
            "IS" => set(r, "issue", &val),
            "SP" => *sp = val.clone(),
            "EP" => *ep = val.clone(),
            "PY" | "Y1" | "DA" => {
                // "2024///" or "2024/03/05/" forms.
                let cleaned: Vec<&str> = val.split('/').filter(|s| !s.trim().is_empty()).collect();
                let d = PartialDate::parse(&cleaned.join("-")).or_else(|| PartialDate::parse(&val));
                if let Some(d) = d {
                    let better = r.issued.as_ref().map(|old| old.month.is_none() && d.month.is_some()).unwrap_or(true);
                    if better {
                        r.issued = Some(d);
                    }
                }
            }
            "Y2" => r.accessed = PartialDate::parse(&val.replace('/', "-")),
            "DO" => set(r, "doi", &val),
            "UR" | "L2" => {
                if val.to_lowercase().contains("doi.org/") && !r.fields.contains_key("doi") {
                    set(r, "doi", &val);
                } else {
                    set(r, "url", &val);
                }
            }
            "PB" => set(r, "publisher", &val),
            "CY" | "PP" => set(r, "publisher_place", &val),
            "SN" => {
                let digits: String = val.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
                if digits.len() >= 10 {
                    set(r, "isbn", &val)
                } else {
                    set(r, "issn", &val)
                }
            }
            "AB" | "N2" => set(r, "abstract", &val),
            "KW" => {
                r.tags.push(val.clone());
            }
            "N1" => set(r, "note", &val),
            "ET" => set(r, "edition", &val),
            "LA" => set(r, "language", &val),
            "M3" => set(r, "genre", &val),
            "AN" => {
                r.overflow.insert("ris:AN".into(), val.clone());
            }
            "C7" => set(r, "article_number", &val),
            "L1" | "L4" => {
                r.overflow.insert(format!("ris:{tag}"), val.clone());
            }
            "ER" => {
                let p = pages(sp, ep);
                if !p.is_empty() {
                    r.fields.insert("page".into(), p);
                }
                let (mut rec, raw_s, _, _) = cur.take().unwrap();
                rec.origin = "import".into();
                out.records.push(Parsed { raw: raw_s, record: std::mem::take(&mut rec), issues: vec![], confidence: 1.0 });
                continue;
            }
            other => {
                let k = format!("ris:{other}");
                let e = r.overflow.entry(k).or_default();
                if !e.is_empty() {
                    e.push_str("; ");
                }
                e.push_str(&val);
            }
        }
    }
    if let Some((r, raw, _, _)) = cur {
        out.errors.push("The last record has no end tag (ER); it was read anyway.".into());
        out.records.push(Parsed { raw, record: r, issues: vec!["Record was not terminated".into()], confidence: 0.9 });
    }
    out
}

fn name_ris(n: &crate::model::Name) -> String {
    match n.kind {
        crate::model::NameKind::Person => {
            let mut s = n.surname();
            if !n.given.is_empty() {
                s.push_str(", ");
                s.push_str(&n.given);
            }
            if !n.suffix.is_empty() {
                s.push_str(", ");
                s.push_str(&n.suffix);
            }
            s
        }
        _ => n.literal.clone(),
    }
}

pub fn export(refs: &[&Reference]) -> String {
    let mut o = String::new();
    for r in refs {
        let ty = crate::types::get(&r.rtype).map(|t| t.ris).unwrap_or("GEN");
        o.push_str(&format!("TY  - {ty}\r\n"));
        for c in &r.creators {
            let tag = match c.role.as_str() {
                "author" | "inventor" | "director" | "host" => "AU",
                "editor" => "A2",
                "series-editor" => "A3",
                "translator" => "A4",
                _ => "A2",
            };
            o.push_str(&format!("{tag}  - {}\r\n", name_ris(&c.name)));
        }
        let w = |o: &mut String, tag: &str, v: &str| {
            if !v.trim().is_empty() {
                o.push_str(&format!("{tag}  - {}\r\n", v.replace(['\r', '\n'], " ")));
            }
        };
        w(&mut o, "TI", r.title());
        w(&mut o, "T2", r.f("container_title"));
        w(&mut o, "T3", r.f("collection_title"));
        w(&mut o, "ST", r.f("short_title"));
        if let Some(d) = &r.issued {
            if let Some(y) = d.year {
                w(&mut o, "PY", &y.to_string());
                w(&mut o, "DA", &format!("{:04}/{}/{}/", y, d.month.map(|m| format!("{m:02}")).unwrap_or_default(), d.day.map(|x| format!("{x:02}")).unwrap_or_default()));
            } else if !d.status.is_empty() {
                w(&mut o, "PY", &d.status);
            }
        }
        w(&mut o, "VL", r.f("volume"));
        w(&mut o, "IS", r.f("issue"));
        let p = r.f("page");
        if let Some((a, b)) = p.split_once(['-', '–']) {
            w(&mut o, "SP", a);
            w(&mut o, "EP", b);
        } else {
            w(&mut o, "SP", p);
        }
        w(&mut o, "C7", r.f("article_number"));
        w(&mut o, "DO", r.f("doi"));
        w(&mut o, "UR", r.f("url"));
        w(&mut o, "PB", r.f("publisher"));
        w(&mut o, "CY", r.f("publisher_place"));
        w(&mut o, "SN", if r.has("isbn") { r.f("isbn") } else { r.f("issn") });
        w(&mut o, "ET", r.f("edition"));
        w(&mut o, "LA", r.f("language"));
        w(&mut o, "M3", r.f("genre"));
        w(&mut o, "AB", r.f("abstract"));
        w(&mut o, "N1", r.f("note"));
        for t in &r.tags {
            w(&mut o, "KW", t);
        }
        for (k, v) in &r.overflow {
            if let Some(tag) = k.strip_prefix("ris:") {
                if tag != "TY" && tag.len() == 2 {
                    w(&mut o, tag, v);
                }
            }
        }
        o.push_str("ER  - \r\n\r\n");
    }
    o
}
