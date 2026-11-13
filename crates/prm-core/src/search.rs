//! Search index and query language.
//!
//! The index is a rebuildable cache (SQLite FTS5) over folded text: accents removed, case
//! folded, ligatures expanded and CJK characters separated into tokens. Results always show
//! the original spelling. Query syntax:
//!
//! ```text
//! reflection teaching            all words (prefix match)
//! "reflective practice"          phrase
//! "faculty support"~5            words within 5 tokens of each other
//! -survey   NOT survey           exclusion
//! a OR b, (a OR b) AND c         Boolean logic with grouping
//! title:ai  author:chen  journal:"teaching practice"  doi:10.5555  tag:method
//! type:book  year:2020..2024  year:>=2021  year:<2000  rating:>=3
//! note:reflection  annotation:quote  fulltext:prompt
//! re:/gener(ative|al) AI/        regular expression over titles and abstracts (bounded)
//! ```

use crate::error::{Error, Result};
use crate::util::fold;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const MAX_REGEX_SCAN: usize = 200_000;

fn ins(conn: &Connection, entity: &str, id: &str, scope: &str, field: &str, page: i64, orig: &str) -> Result<()> {
    if orig.trim().is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO fts(entity,entity_id,scope,field,page,orig,text) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![entity, id, scope, field, page, orig, fold(orig)],
    )?;
    Ok(())
}

/// Keep the index in step with a write. Called inside the write transaction.
pub fn reindex(conn: &Connection, table: &str, id: &str, data: Option<&str>) -> Result<()> {
    let entity = match table {
        "refs" => "ref",
        "notes" => "note",
        "annotations" => "annotation",
        _ => return Ok(()),
    };
    conn.execute("DELETE FROM fts WHERE entity=?1 AND entity_id=?2", params![entity, id])?;
    let Some(d) = data else { return Ok(()) };
    let v: serde_json::Value = serde_json::from_str(d)?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    match entity {
        "ref" => {
            if v.get("trashed_at").map(|t| !t.is_null()).unwrap_or(false) {
                return Ok(());
            }
            let f = |k: &str| v.get("fields").and_then(|m| m.get(k)).and_then(|x| x.as_str()).unwrap_or("").to_string();
            ins(conn, entity, id, "", "title", -1, &f("title"))?;
            ins(conn, entity, id, "", "journal", -1, &f("container_title"))?;
            ins(conn, entity, id, "", "abstract", -1, &f("abstract"))?;
            ins(conn, entity, id, "", "doi", -1, &f("doi"))?;
            ins(conn, entity, id, "", "publisher", -1, &f("publisher"))?;
            ins(conn, entity, id, "", "keywords", -1, &f("keywords"))?;
            ins(conn, entity, id, "", "citekey", -1, &s("citekey"))?;
            let mut names = Vec::new();
            if let Some(cs) = v.get("creators").and_then(|c| c.as_array()) {
                for c in cs {
                    let n = &c["name"];
                    let g = |k: &str| n.get(k).and_then(|x| x.as_str()).unwrap_or("");
                    names.push(format!("{} {} {} {}", g("given"), g("particle"), g("family"), g("literal")).split_whitespace().collect::<Vec<_>>().join(" "));
                }
            }
            ins(conn, entity, id, "", "author", -1, &names.join("; "))?;
            if let Some(tags) = v.get("tags").and_then(|t| t.as_array()) {
                let t: Vec<&str> = tags.iter().filter_map(|x| x.as_str()).collect();
                ins(conn, entity, id, "", "tag", -1, &t.join(" "))?;
            }
        }
        "note" => {
            if v.get("trashed_at").map(|t| !t.is_null()).unwrap_or(false) {
                return Ok(());
            }
            let scope = s("project_id");
            ins(conn, entity, id, &scope, "note", -1, &format!("{} {}", s("title"), s("body")))?;
            ins(conn, entity, id, &scope, "quotation", -1, &s("exact_text"))?;
        }
        "annotation" => {
            let scope = s("project_id");
            let page = v.get("page_index").and_then(|p| p.as_i64()).unwrap_or(0);
            ins(conn, entity, id, &scope, "annotation", page, &s("text"))?;
            ins(conn, entity, id, &scope, "comment", page, &s("comment"))?;
        }
        _ => {}
    }
    Ok(())
}

/// Store extracted page text for an attachment (a cache; rebuildable from the file).
pub fn index_pages(conn: &Connection, attachment_id: &str, pages: &[String]) -> Result<()> {
    conn.execute("DELETE FROM fts WHERE entity='page' AND entity_id=?1", params![attachment_id])?;
    conn.execute("DELETE FROM page_text WHERE attachment_id=?1", params![attachment_id])?;
    for (i, p) in pages.iter().enumerate() {
        conn.execute("INSERT INTO page_text(attachment_id,page,text) VALUES(?1,?2,?3)", params![attachment_id, i as i64, p])?;
        ins(conn, "page", attachment_id, "", "fulltext", i as i64, p)?;
    }
    Ok(())
}

/// Rebuild the metadata, note and annotation index from the store.
pub fn rebuild(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM fts WHERE entity IN ('ref','note','annotation')", [])?;
    for (table, _) in [("refs", "ref"), ("notes", "note"), ("annotations", "annotation")] {
        let mut st = conn.prepare(&format!("SELECT id,data FROM {table}"))?;
        let rows: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
        for (id, d) in rows {
            reindex(conn, table, &id, Some(&d))?;
        }
    }
    Ok(())
}

// ---- query parsing -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Q {
    And(Vec<Q>),
    Or(Vec<Q>),
    Not(Box<Q>),
    Term { field: String, value: String, phrase: bool, near: Option<u32> },
    Range { field: String, op: String, a: f64, b: f64 },
    Regex(String),
    All,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LParen,
    RParen,
    Or,
    And,
    Not,
    Word(String, bool, Option<u32>, String), // value, quoted, near, field
}

fn lex(s: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '(' {
            out.push(Tok::LParen);
            i += 1;
            continue;
        }
        if c == ')' {
            out.push(Tok::RParen);
            i += 1;
            continue;
        }
        if c == '-' && i + 1 < chars.len() && !chars[i + 1].is_whitespace() && (i == 0 || chars[i - 1].is_whitespace() || chars[i - 1] == '(') {
            out.push(Tok::Not);
            i += 1;
            continue;
        }
        // field prefix
        let mut field = String::new();
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
        if i < chars.len() && chars[i] == ':' && i > start {
            field = chars[start..i].iter().collect::<String>().to_lowercase();
            i += 1;
        } else {
            i = start;
        }
        if field == "re" && i < chars.len() && chars[i] == '/' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '/' {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let pat: String = chars[i + 1..j.min(chars.len())].iter().collect();
            out.push(Tok::Word(pat, true, None, "re".into()));
            i = (j + 1).min(chars.len());
            continue;
        }
        if i < chars.len() && (chars[i] == '"' || chars[i] == '“') {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '"' && chars[j] != '”' {
                j += 1;
            }
            let val: String = chars[i + 1..j.min(chars.len())].iter().collect();
            i = (j + 1).min(chars.len());
            let mut near = None;
            if i < chars.len() && chars[i] == '~' {
                let mut k = i + 1;
                while k < chars.len() && chars[k].is_ascii_digit() {
                    k += 1;
                }
                near = chars[i + 1..k].iter().collect::<String>().parse().ok();
                i = k;
            }
            out.push(Tok::Word(val, true, near, field));
            continue;
        }
        let mut j = i;
        while j < chars.len() && !chars[j].is_whitespace() && chars[j] != ')' && chars[j] != '(' {
            j += 1;
        }
        let w: String = chars[i..j].iter().collect();
        i = j;
        if field.is_empty() {
            match w.as_str() {
                "OR" | "|" => {
                    out.push(Tok::Or);
                    continue;
                }
                "AND" | "&" => {
                    out.push(Tok::And);
                    continue;
                }
                "NOT" => {
                    out.push(Tok::Not);
                    continue;
                }
                _ => {}
            }
        }
        if !w.is_empty() || !field.is_empty() {
            out.push(Tok::Word(w, false, None, field));
        }
    }
    Ok(out)
}

struct P {
    t: Vec<Tok>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn or(&mut self) -> Result<Q> {
        let mut v = vec![self.and()?];
        while self.peek() == Some(&Tok::Or) {
            self.i += 1;
            v.push(self.and()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { Q::Or(v) })
    }
    fn and(&mut self) -> Result<Q> {
        let mut v = vec![self.unary()?];
        loop {
            match self.peek() {
                Some(Tok::And) => {
                    self.i += 1;
                    v.push(self.unary()?);
                }
                Some(Tok::RParen) | Some(Tok::Or) | None => break,
                _ => v.push(self.unary()?),
            }
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { Q::And(v) })
    }
    fn unary(&mut self) -> Result<Q> {
        if self.peek() == Some(&Tok::Not) {
            self.i += 1;
            return Ok(Q::Not(Box::new(self.unary()?)));
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<Q> {
        match self.t.get(self.i).cloned() {
            Some(Tok::LParen) => {
                self.i += 1;
                let q = self.or()?;
                if self.peek() == Some(&Tok::RParen) {
                    self.i += 1;
                }
                Ok(q)
            }
            Some(Tok::Word(v, quoted, near, field)) => {
                self.i += 1;
                if field == "re" {
                    return Ok(Q::Regex(v));
                }
                if matches!(field.as_str(), "year" | "rating") {
                    return parse_range(&field, &v);
                }
                Ok(Q::Term { field, value: v, phrase: quoted, near })
            }
            Some(other) => {
                self.i += 1;
                Err(Error::user(format!("The search could not be read near {other:?}. Check parentheses and quotation marks.")))
            }
            None => Ok(Q::All),
        }
    }
}

fn parse_range(field: &str, v: &str) -> Result<Q> {
    let num = |s: &str| s.trim().parse::<f64>().map_err(|_| Error::user(format!("“{s}” is not a number in {field}:")));
    let (op, a, b) = if let Some((x, y)) = v.split_once("..") {
        let a = if x.is_empty() { f64::MIN } else { num(x)? };
        let b = if y.is_empty() { f64::MAX } else { num(y)? };
        ("between", a, b)
    } else if let Some(x) = v.strip_prefix(">=") {
        (">=", num(x)?, 0.0)
    } else if let Some(x) = v.strip_prefix("<=") {
        ("<=", num(x)?, 0.0)
    } else if let Some(x) = v.strip_prefix('>') {
        (">", num(x)?, 0.0)
    } else if let Some(x) = v.strip_prefix('<') {
        ("<", num(x)?, 0.0)
    } else {
        ("=", num(v)?, 0.0)
    };
    Ok(Q::Range { field: field.into(), op: op.into(), a, b })
}

pub fn parse(query: &str) -> Result<Q> {
    let toks = lex(query)?;
    if toks.is_empty() {
        return Ok(Q::All);
    }
    let mut p = P { t: toks, i: 0 };
    p.or()
}

// ---- evaluation ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Default)]
pub struct Hit {
    /// ref, note, annotation, page
    pub entity: String,
    pub entity_id: String,
    /// The reference the hit belongs to (for notes, annotations and pages when known).
    pub ref_id: String,
    pub field: String,
    pub page: i64,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Results {
    pub ref_ids: Vec<String>,
    pub hits: Vec<Hit>,
    pub truncated: bool,
}

type Key = (String, String); // (entity, id)

fn fts_expr(value: &str, phrase: bool, near: Option<u32>) -> String {
    let f = fold(value);
    let words: Vec<String> = f
        .split(|c: char| !c.is_alphanumeric() && c != '*')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect();
    if words.is_empty() {
        return String::new();
    }
    if let Some(n) = near {
        let ws: Vec<String> = words.iter().map(|w| format!("\"{}\"", w.trim_end_matches('*'))).collect();
        return format!("NEAR({}, {n})", ws.join(" "));
    }
    if phrase {
        return format!("\"{}\"", words.join(" ").replace('*', ""));
    }
    words
        .iter()
        .map(|w| {
            let bare = w.trim_end_matches('*');
            if bare.is_empty() { String::new() } else { format!("\"{bare}\"*") }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn field_filter(field: &str) -> Option<&'static [&'static str]> {
    Some(match field {
        "" | "any" => return None,
        "title" => &["title"],
        "author" | "creator" => &["author"],
        "journal" | "container" | "source" => &["journal"],
        "abstract" => &["abstract"],
        "doi" => &["doi"],
        "publisher" => &["publisher"],
        "tag" | "keyword" | "keywords" => &["tag", "keywords"],
        "citekey" | "key" => &["citekey"],
        "note" | "notes" => &["note", "quotation"],
        "quote" | "quotation" => &["quotation"],
        "annotation" | "annotations" => &["annotation", "comment"],
        "fulltext" | "text" | "pdf" => &["fulltext"],
        _ => &["__none__"],
    })
}

struct Eval<'a> {
    conn: &'a Connection,
    hits: BTreeMap<Key, Hit>,
    terms: Vec<String>,
}

impl<'a> Eval<'a> {
    fn universe(&self) -> Result<BTreeSet<Key>> {
        let mut s = BTreeSet::new();
        let mut st = self.conn.prepare("SELECT id FROM refs WHERE trashed IS NULL")?;
        for r in st.query_map([], |r| r.get::<_, String>(0))? {
            s.insert(("ref".to_string(), r?));
        }
        Ok(s)
    }

    fn term(&mut self, field: &str, value: &str, phrase: bool, near: Option<u32>) -> Result<BTreeSet<Key>> {
        // Metadata fields that are not indexed text.
        if field == "type" {
            let mut st = self.conn.prepare("SELECT id FROM refs WHERE trashed IS NULL AND json_extract(data,'$.type')=?1")?;
            return Ok(st.query_map(params![value], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).map(|id| ("ref".to_string(), id)).collect());
        }
        if field == "label" {
            let mut st = self.conn.prepare("SELECT refs.id FROM refs, json_each(refs.data,'$.labels') j WHERE refs.trashed IS NULL AND j.value IN (SELECT id FROM labels WHERE lower(json_extract(data,'$.name'))=lower(?1) OR id=?1)")?;
            return Ok(st.query_map(params![value], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).map(|id| ("ref".to_string(), id)).collect());
        }
        if field == "collection" {
            let mut st = self.conn.prepare("SELECT j.value FROM collections, json_each(collections.data,'$.ref_ids') j WHERE collections.id=?1 OR lower(json_extract(collections.data,'$.name'))=lower(?1)")?;
            return Ok(st.query_map(params![value], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).map(|id| ("ref".to_string(), id)).collect());
        }
        if field == "project" {
            let mut st = self.conn.prepare("SELECT ref_id FROM project_sources WHERE project_id=?1")?;
            return Ok(st.query_map(params![value], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).map(|id| ("ref".to_string(), id)).collect());
        }
        if field == "has" {
            let sql = match value {
                "pdf" | "file" => "SELECT DISTINCT ref_id FROM attachments",
                "doi" => "SELECT id FROM refs WHERE json_extract(data,'$.fields.doi') IS NOT NULL AND json_extract(data,'$.fields.doi')!=''",
                "conflict" => "SELECT id FROM refs WHERE json_array_length(json_extract(data,'$.conflicts'))>0",
                "notice" | "retraction" => "SELECT id FROM refs WHERE json_array_length(json_extract(data,'$.notices'))>0",
                "note" | "notes" => "SELECT DISTINCT j.value FROM notes, json_each(notes.data,'$.ref_ids') j",
                _ => return Err(Error::user(format!("has:{value} is not a known filter. Try has:pdf, has:doi, has:conflict, has:notes."))),
            };
            let mut st = self.conn.prepare(sql)?;
            return Ok(st.query_map([], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).map(|id| ("ref".to_string(), id)).collect());
        }
        let expr = fts_expr(value, phrase, near);
        if expr.is_empty() {
            return Ok(BTreeSet::new());
        }
        for w in fold(value).split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 1) {
            self.terms.push(w.to_string());
        }
        let filter = field_filter(field);
        let mut sql = String::from("SELECT entity, entity_id, field, page, orig FROM fts WHERE fts MATCH ?1");
        if let Some(fs) = filter {
            sql.push_str(&format!(" AND field IN ({})", fs.iter().map(|f| format!("'{f}'")).collect::<Vec<_>>().join(",")));
        }
        sql.push_str(" ORDER BY bm25(fts) LIMIT 20000");
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![format!("text : ({expr})")], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?))
        })?;
        let mut out = BTreeSet::new();
        for row in rows {
            let (entity, id, f, page, orig) = row?;
            let key = (entity.clone(), id.clone());
            self.hits.entry(key.clone()).or_insert(Hit { entity, entity_id: id, ref_id: String::new(), field: f, page, snippet: orig });
            out.insert(key);
        }
        Ok(out)
    }

    fn range(&self, field: &str, op: &str, a: f64, b: f64) -> Result<BTreeSet<Key>> {
        let col = match field {
            "year" => "json_extract(data,'$.issued.year')",
            "rating" => "json_extract(data,'$.rating')",
            _ => return Ok(BTreeSet::new()),
        };
        let cond = match op {
            "between" => format!("{col} BETWEEN ?1 AND ?2"),
            ">=" => format!("{col} >= ?1"),
            "<=" => format!("{col} <= ?1"),
            ">" => format!("{col} > ?1"),
            "<" => format!("{col} < ?1"),
            _ => format!("{col} = ?1"),
        };
        let sql = format!("SELECT id FROM refs WHERE trashed IS NULL AND {cond}");
        let mut st = self.conn.prepare(&sql)?;
        let rows: Vec<String> = if op == "between" {
            st.query_map(params![a, b], |r| r.get(0))?.filter_map(|r| r.ok()).collect()
        } else {
            st.query_map(params![a], |r| r.get(0))?.filter_map(|r| r.ok()).collect()
        };
        Ok(rows.into_iter().map(|id| ("ref".to_string(), id)).collect())
    }

    fn regex(&mut self, pat: &str) -> Result<BTreeSet<Key>> {
        let re = regex::RegexBuilder::new(pat)
            .case_insensitive(true)
            .size_limit(1 << 20)
            .dfa_size_limit(1 << 20)
            .build()
            .map_err(|e| Error::user(format!("The regular expression could not be used: {e}")))?;
        let mut st = self.conn.prepare("SELECT id, json_extract(data,'$.fields.title'), json_extract(data,'$.fields.abstract') FROM refs WHERE trashed IS NULL LIMIT ?1")?;
        let rows = st.query_map(params![MAX_REGEX_SCAN as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?)))?;
        let mut out = BTreeSet::new();
        for row in rows {
            let (id, t, ab) = row?;
            for (f, text) in [("title", t), ("abstract", ab)] {
                if let Some(text) = text {
                    if let Some(m) = re.find(&text) {
                        let key = ("ref".to_string(), id.clone());
                        let start = text[..m.start()].char_indices().rev().nth(40).map(|(i, _)| i).unwrap_or(0);
                        let end = text[m.end()..].char_indices().nth(40).map(|(i, _)| m.end() + i).unwrap_or(text.len());
                        self.hits.entry(key.clone()).or_insert(Hit { entity: "ref".into(), entity_id: id.clone(), ref_id: id.clone(), field: f.into(), page: -1, snippet: format!("…{}…", &text[start..end]) });
                        out.insert(key);
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    fn eval(&mut self, q: &Q) -> Result<BTreeSet<Key>> {
        Ok(match q {
            Q::All => self.universe()?,
            Q::Term { field, value, phrase, near } => self.term(field, value, *phrase, *near)?,
            Q::Range { field, op, a, b } => self.range(field, op, *a, *b)?,
            Q::Regex(p) => self.regex(p)?,
            Q::And(v) => {
                let mut acc: Option<BTreeSet<Key>> = None;
                let mut nots = Vec::new();
                for x in v {
                    if let Q::Not(inner) = x {
                        let e = self.eval_owned(inner)?; nots.push(self.lift(e)?);
                        continue;
                    }
                    let e = self.eval(x)?; let s = self.lift(e)?;
                    acc = Some(match acc {
                        None => s,
                        Some(a) => a.intersection(&s).cloned().collect(),
                    });
                }
                let mut a = match acc {
                    Some(a) => a,
                    None => self.universe()?,
                };
                for n in nots {
                    a = a.difference(&n).cloned().collect();
                }
                a
            }
            Q::Or(v) => {
                let mut acc = BTreeSet::new();
                for x in v {
                    let e = self.eval(x)?; acc.extend(self.lift(e)?);
                }
                acc
            }
            Q::Not(inner) => {
                let e = self.eval(inner)?; let s = self.lift(e)?;
                self.universe()?.difference(&s).cloned().collect()
            }
        })
    }

    fn eval_owned(&mut self, q: &Q) -> Result<BTreeSet<Key>> {
        self.eval(q)
    }

    /// Map note, annotation and page hits to their references so Boolean logic combines at the
    /// reference level; unattached notes stay as notes.
    fn lift(&self, s: BTreeSet<Key>) -> Result<BTreeSet<Key>> {
        let mut out = BTreeSet::new();
        for (e, id) in s {
            match e.as_str() {
                "ref" => {
                    out.insert((e, id));
                }
                _ => {
                    let refs = owner_refs(self.conn, &e, &id)?;
                    if refs.is_empty() {
                        out.insert((e, id));
                    } else {
                        for r in refs {
                            out.insert(("ref".to_string(), r));
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

fn owner_refs(conn: &Connection, entity: &str, id: &str) -> Result<Vec<String>> {
    let sql = match entity {
        "note" => "SELECT j.value FROM notes, json_each(notes.data,'$.ref_ids') j WHERE notes.id=?1",
        "annotation" => "SELECT a.ref_id FROM annotations n JOIN attachments a ON a.id=n.attachment_id WHERE n.id=?1",
        "page" => "SELECT ref_id FROM attachments WHERE id=?1",
        _ => return Ok(vec![]),
    };
    let mut st = conn.prepare(sql)?;
    let v: Vec<String> = st.query_map(params![id], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).collect();
    Ok(v)
}

/// Build a short snippet from original text around the first matching folded term.
pub fn snippet(orig: &str, terms: &[String]) -> String {
    let words: Vec<&str> = orig.split_whitespace().collect();
    let pos = words.iter().position(|w| {
        let f = fold(w);
        terms.iter().any(|t| f.contains(t.as_str()))
    });
    match pos {
        Some(p) => {
            let a = p.saturating_sub(8);
            let b = (p + 10).min(words.len());
            format!("{}{}{}", if a > 0 { "…" } else { "" }, words[a..b].join(" "), if b < words.len() { "…" } else { "" })
        }
        None => words.iter().take(18).cloned().collect::<Vec<_>>().join(" "),
    }
}

/// Run a query. `scope_project` restricts results to sources and notes of that project;
/// `scope_refs` restricts to an explicit set (a collection).
pub fn run(conn: &Connection, query: &str, scope_project: Option<&str>, scope_refs: Option<&BTreeSet<String>>, limit: usize) -> Result<Results> {
    let q = parse(query)?;
    let mut ev = Eval { conn, hits: BTreeMap::new(), terms: Vec::new() };
    let keys = ev.eval(&q)?;
    let keys = ev.lift(keys)?;
    let project_refs: Option<BTreeSet<String>> = match scope_project {
        Some(p) if !p.is_empty() => {
            let mut st = conn.prepare("SELECT ref_id FROM project_sources WHERE project_id=?1")?;
            let v: BTreeSet<String> = st.query_map(params![p], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).collect();
            Some(v)
        }
        _ => None,
    };
    let mut ref_ids = Vec::new();
    for (e, id) in &keys {
        if e != "ref" {
            continue;
        }
        if let Some(pr) = &project_refs {
            if !pr.contains(id) {
                continue;
            }
        }
        if let Some(sr) = scope_refs {
            if !sr.contains(id) {
                continue;
            }
        }
        ref_ids.push(id.clone());
    }
    let truncated = ref_ids.len() > limit;
    ref_ids.truncate(limit);
    let terms = ev.terms.clone();
    let mut hits: Vec<Hit> = Vec::new();
    for (_, mut h) in ev.hits {
        let owners = if h.entity == "ref" { vec![h.entity_id.clone()] } else { owner_refs(conn, &h.entity, &h.entity_id)? };
        // Notes and annotations outside the active project are not shown in project scope.
        if let Some(p) = scope_project {
            if !p.is_empty() && (h.entity == "note" || h.entity == "annotation") {
                let scope: String = conn
                    .query_row(&format!("SELECT COALESCE(json_extract(data,'$.project_id'),'') FROM {} WHERE id=?1", if h.entity == "note" { "notes" } else { "annotations" }), params![h.entity_id], |r| r.get(0))
                    .unwrap_or_default();
                if !scope.is_empty() && scope != p {
                    continue;
                }
            }
        }
        h.ref_id = owners.first().cloned().unwrap_or_default();
        if !h.snippet.starts_with('…') {
            h.snippet = snippet(&h.snippet, &terms);
        }
        if h.entity == "note" && h.ref_id.is_empty() {
            hits.push(h);
            continue;
        }
        if ref_ids.contains(&h.ref_id) {
            hits.push(h);
        }
    }
    hits.truncate(2000);
    Ok(Results { ref_ids, hits, truncated })
}

/// Whether the full-text index has unindexed attachments (to tell "no matching text" from
/// "text not yet indexed").
pub fn pending_index_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM attachments WHERE json_extract(data,'$.mime')='application/pdf' AND COALESCE(json_extract(data,'$.indexed'),0)=0 AND COALESCE(json_extract(data,'$.index_excluded'),0)=0",
        [],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses() {
        let q = parse("title:\"generative ai\" (chen OR patel) -survey year:2020..2024").unwrap();
        match q {
            Q::And(v) => assert_eq!(v.len(), 4),
            _ => panic!("{q:?}"),
        }
        assert!(matches!(parse("re:/a+b/").unwrap(), Q::Regex(_)));
        assert!(matches!(parse("\"a b\"~5").unwrap(), Q::Term { near: Some(5), .. }));
    }
}
