//! CSL JSON (Citation Style Language data format).

use super::{ParseOutput, Parsed};
use crate::model::{Creator, Name, NameKind, PartialDate, Reference};
use serde_json::{json, Map, Value};

fn csl_to_type(t: &str, item: &Value) -> &'static str {
    match t {
        "article-journal" => "article-journal",
        "article-magazine" => "article-magazine",
        "article-newspaper" => "article-newspaper",
        "article" => "preprint",
        "book" => {
            if item.get("author").is_none() && item.get("editor").is_some() { "edited-book" } else { "book" }
        }
        "chapter" => "chapter",
        "paper-conference" => "paper-conference",
        "report" => "report",
        "thesis" => "thesis",
        "webpage" => "webpage",
        "post-weblog" => "post-weblog",
        "post" => "post",
        "dataset" => "dataset",
        "software" => "software",
        "patent" => "patent",
        "legal_case" => "legal-case",
        "legislation" => "legislation",
        "bill" => "bill",
        "regulation" => "regulation",
        "hearing" => "hearing",
        "treaty" => "treaty",
        "manuscript" => "manuscript",
        "letter" => "letter",
        "personal_communication" => "personal-communication",
        "interview" => "interview",
        "motion_picture" => "film",
        "broadcast" => "broadcast",
        "song" => "song",
        "graphic" => "graphic",
        "map" => "map",
        "speech" => "speech",
        "performance" => "performance",
        "entry-encyclopedia" => "entry-encyclopedia",
        "entry-dictionary" => "entry-dictionary",
        "review" | "review-book" => "review",
        "standard" => "standard",
        "pamphlet" => "pamphlet",
        "classic" => "classic",
        _ => "document",
    }
}

const FIELD_MAP: &[(&str, &str)] = &[
    ("title", "title"),
    ("title-short", "short_title"),
    ("container-title", "container_title"),
    ("collection-title", "collection_title"),
    ("volume", "volume"),
    ("issue", "issue"),
    ("page", "page"),
    ("DOI", "doi"),
    ("URL", "url"),
    ("publisher", "publisher"),
    ("publisher-place", "publisher_place"),
    ("edition", "edition"),
    ("ISBN", "isbn"),
    ("ISSN", "issn"),
    ("PMID", "pmid"),
    ("PMCID", "pmcid"),
    ("abstract", "abstract"),
    ("language", "language"),
    ("note", "note"),
    ("genre", "genre"),
    ("medium", "medium"),
    ("version", "version"),
    ("event-title", "event"),
    ("event", "event"),
    ("event-place", "event_place"),
    ("archive", "archive"),
    ("archive_location", "archive_location"),
    ("section", "section"),
    ("jurisdiction", "jurisdiction"),
    ("authority", "authority"),
    ("number-of-volumes", "number_of_volumes"),
    ("dimensions", "dimensions"),
    ("scale", "scale"),
    ("status", "status"),
];

fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn date_of(v: &Value) -> Option<PartialDate> {
    if let Some(parts) = v.get("date-parts").and_then(|p| p.as_array()) {
        let first = parts.first()?.as_array()?;
        let num = |i: usize, a: &Vec<Value>| a.get(i).and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse().ok())));
        let mut d = PartialDate { year: num(0, first).map(|y| y as i32), month: num(1, first).map(|m| m as u8), day: num(2, first).map(|d| d as u8), ..Default::default() };
        if let Some(second) = parts.get(1).and_then(|p| p.as_array()) {
            d.end = Some(Box::new(PartialDate { year: num(0, second).map(|y| y as i32), month: num(1, second).map(|m| m as u8), day: num(2, second).map(|d| d as u8), ..Default::default() }));
        }
        if let Some(s) = v.get("season").and_then(|s| s.as_i64().or_else(|| s.as_str().and_then(|x| x.parse().ok()))) {
            d.season = Some(s as u8);
        }
        d.approximate = v.get("circa").map(|c| c.as_bool().unwrap_or(true)).unwrap_or(false);
        return Some(d);
    }
    if let Some(raw) = v.get("raw").or(v.get("literal")).and_then(|x| x.as_str()) {
        return PartialDate::parse(raw);
    }
    v.as_str().and_then(PartialDate::parse)
}

fn name_of(v: &Value) -> Name {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    if !s("literal").is_empty() {
        return Name { kind: NameKind::Organization, literal: s("literal"), ..Default::default() };
    }
    let mut particle = s("non-dropping-particle");
    if !s("dropping-particle").is_empty() {
        particle = format!("{} {}", s("dropping-particle"), particle).trim().to_string();
    }
    Name { kind: NameKind::Person, family: s("family"), given: s("given"), particle, suffix: s("suffix"), ..Default::default() }
}

pub fn parse(text: &str) -> ParseOutput {
    let mut out = ParseOutput::default();
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            out.errors.push(format!("The file is not valid JSON: {e}"));
            return out;
        }
    };
    let items: Vec<Value> = match v {
        Value::Array(a) => a,
        Value::Object(ref o) if o.contains_key("items") => o["items"].as_array().cloned().unwrap_or_default(),
        Value::Object(_) => vec![v],
        _ => vec![],
    };
    for item in items {
        let Some(obj) = item.as_object() else { continue };
        let mut r = Reference::default();
        r.origin = "import".into();
        let ty = obj.get("type").and_then(|t| t.as_str()).unwrap_or("document");
        r.rtype = csl_to_type(ty, &item).to_string();
        let mut used: std::collections::BTreeSet<&str> = ["type", "id", "citation-key", "issued", "accessed", "original-date", "number", "keyword", "categories"].into_iter().collect();
        for (k, f) in FIELD_MAP {
            if let Some(val) = obj.get(*k).and_then(str_of) {
                r.fields.entry(f.to_string()).or_insert(val);
            }
            used.insert(k);
        }
        if let Some(n) = obj.get("number").and_then(str_of) {
            if r.rtype == "article-journal" {
                r.fields.insert("article_number".into(), n);
            } else {
                r.fields.insert("number".into(), n);
            }
        }
        for role in ["author", "editor", "translator", "director", "interviewer", "recipient", "composer", "container-author", "collection-editor", "reviewed-author", "contributor", "host", "guest", "producer", "performer", "chair", "compiler", "inventor"] {
            used.insert(role);
            if let Some(arr) = obj.get(role).and_then(|a| a.as_array()) {
                for n in arr {
                    r.creators.push(Creator { role: role.to_string(), name: name_of(n) });
                }
            }
        }
        r.issued = obj.get("issued").and_then(date_of);
        r.accessed = obj.get("accessed").and_then(date_of);
        r.original_date = obj.get("original-date").and_then(date_of);
        if let Some(k) = obj.get("citation-key").and_then(|x| x.as_str()) {
            r.citekey = k.to_string();
        } else if let Some(k) = obj.get("id").and_then(|x| x.as_str()) {
            r.overflow.insert("csl:id".into(), k.to_string());
        }
        if let Some(kw) = obj.get("keyword").and_then(|x| x.as_str()) {
            r.tags.extend(kw.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
        }
        if let Some(Value::Object(c)) = obj.get("custom") {
            for (k, v) in c {
                r.custom.insert(k.clone(), v.clone());
            }
        }
        for (k, val) in obj {
            if !used.contains(k.as_str()) && k != "custom" {
                r.overflow.insert(format!("csl:{k}"), match val {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
            }
        }
        out.records.push(Parsed { raw: serde_json::to_string_pretty(&item).unwrap_or_default(), record: r, issues: vec![], confidence: 1.0 });
    }
    out
}

fn date_json(d: &PartialDate) -> Value {
    let mut parts = vec![];
    if let Some(y) = d.year {
        let mut p = vec![json!(y)];
        if let Some(m) = d.month {
            p.push(json!(m));
            if let Some(day) = d.day {
                p.push(json!(day));
            }
        }
        parts.push(Value::Array(p));
    }
    if let Some(e) = &d.end {
        if let Some(y) = e.year.or(d.year) {
            let mut p = vec![json!(y)];
            if let Some(m) = e.month {
                p.push(json!(m));
                if let Some(day) = e.day {
                    p.push(json!(day));
                }
            }
            parts.push(Value::Array(p));
        }
    }
    let mut o = Map::new();
    if !parts.is_empty() {
        o.insert("date-parts".into(), Value::Array(parts));
    }
    if let Some(s) = d.season {
        o.insert("season".into(), json!(s));
    }
    if d.approximate {
        o.insert("circa".into(), json!(true));
    }
    if !d.status.is_empty() {
        o.insert("literal".into(), json!(d.status));
    } else if parts_empty(&o) && !d.literal.is_empty() {
        o.insert("literal".into(), json!(d.literal));
    }
    Value::Object(o)
}

fn parts_empty(o: &Map<String, Value>) -> bool {
    !o.contains_key("date-parts")
}

pub fn to_csl(r: &Reference) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(r.id));
    if !r.citekey.is_empty() {
        o.insert("citation-key".into(), json!(r.citekey));
    }
    o.insert("type".into(), json!(crate::types::get(&r.rtype).map(|t| t.csl).unwrap_or("document")));
    for (k, f) in FIELD_MAP {
        if *k == "event" {
            continue;
        }
        if let Some(v) = r.fields.get(*f) {
            if !v.is_empty() && !o.contains_key(*k) {
                o.insert(k.to_string(), json!(v));
            }
        }
    }
    if r.has("article_number") {
        o.insert("number".into(), json!(r.f("article_number")));
    } else if r.has("number") {
        o.insert("number".into(), json!(r.f("number")));
    }
    let mut roles: std::collections::BTreeMap<String, Vec<Value>> = Default::default();
    for c in &r.creators {
        let n = &c.name;
        let v = match n.kind {
            NameKind::Person => {
                let mut m = Map::new();
                m.insert("family".into(), json!(n.family));
                if !n.given.is_empty() {
                    m.insert("given".into(), json!(n.given));
                }
                if !n.particle.is_empty() {
                    m.insert("non-dropping-particle".into(), json!(n.particle));
                }
                if !n.suffix.is_empty() {
                    m.insert("suffix".into(), json!(n.suffix));
                }
                Value::Object(m)
            }
            _ => json!({"literal": n.literal}),
        };
        roles.entry(c.role.clone()).or_default().push(v);
    }
    for (role, list) in roles {
        o.insert(role, Value::Array(list));
    }
    if let Some(d) = &r.issued {
        o.insert("issued".into(), date_json(d));
    }
    if let Some(d) = &r.accessed {
        o.insert("accessed".into(), date_json(d));
    }
    if let Some(d) = &r.original_date {
        o.insert("original-date".into(), date_json(d));
    }
    if !r.tags.is_empty() {
        o.insert("keyword".into(), json!(r.tags.join(", ")));
    }
    if !r.custom.is_empty() {
        o.insert("custom".into(), serde_json::to_value(&r.custom).unwrap_or_default());
    }
    for (k, v) in &r.overflow {
        if let Some(name) = k.strip_prefix("csl:") {
            if !o.contains_key(name) && name != "id" {
                o.insert(name.to_string(), serde_json::from_str(v).unwrap_or(json!(v)));
            }
        }
    }
    Value::Object(o)
}

pub fn export(refs: &[&Reference]) -> String {
    let arr: Vec<Value> = refs.iter().map(|r| to_csl(r)).collect();
    serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".into())
}
