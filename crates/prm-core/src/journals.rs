//! Journal names and their abbreviations (spec QLT-009).
//!
//! The list is built from the library's own sources (a source that carries both the full name and
//! the abbreviation teaches the pair) and from list files the person imports. It is a reference
//! list kept beside the records, like a style: rebuilt from the sources at any time, and not part
//! of the change history. No third-party list is bundled.
//!
//! Each manuscript can show journal names as its style says, always in full, or always
//! abbreviated; the library has a default.

use crate::error::Result;
use crate::model::Reference;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub const FORMS: &[&str] = &["", "full", "abbreviated"];

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS journal_names(key TEXT PRIMARY KEY, full TEXT NOT NULL, abbrev TEXT NOT NULL, abbrev_key TEXT NOT NULL, source TEXT NOT NULL); CREATE INDEX IF NOT EXISTS journal_names_abbrev ON journal_names(abbrev_key);")?;
    Ok(())
}

/// Comparison key: lower case, letters and digits only, a leading "the" dropped, "&" as "and".
pub fn key(name: &str) -> String {
    let s = crate::util::fold(&name.replace('&', " and ")).to_lowercase();
    let words: Vec<&str> = s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let words = if words.first() == Some(&"the") { &words[1..] } else { &words[..] };
    words.join(" ")
}

#[derive(Debug, Clone, Serialize)]
pub struct JournalName {
    pub full: String,
    pub abbrev: String,
    /// records, imported
    pub source: String,
}

/// Record a pair. A pair learned from the person's sources never replaces one they imported.
pub fn put(conn: &Connection, full: &str, abbrev: &str, source: &str) -> Result<bool> {
    let (full, abbrev) = (full.trim(), abbrev.trim());
    if full.is_empty() || abbrev.is_empty() || key(full) == key(abbrev) {
        return Ok(false);
    }
    let k = key(full);
    if source == "records" {
        let existing: Option<String> = conn.query_row("SELECT source FROM journal_names WHERE key=?1", [&k], |r| r.get(0)).optional()?;
        if existing.as_deref() == Some("imported") {
            return Ok(false);
        }
    }
    conn.execute(
        "INSERT INTO journal_names(key,full,abbrev,abbrev_key,source) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(key) DO UPDATE SET full=excluded.full, abbrev=excluded.abbrev, abbrev_key=excluded.abbrev_key, source=excluded.source",
        params![k, full, abbrev, key(abbrev), source],
    )?;
    Ok(true)
}

/// Learn pairs from sources that carry both forms; returns how many pairs are known from them.
pub fn learn(conn: &Connection, refs: &[Reference]) -> Result<usize> {
    let mut n = 0;
    conn.execute_batch("BEGIN")?;
    for r in refs {
        if r.has("container_title") && r.has("container_title_short") && put(conn, r.f("container_title"), r.f("container_title_short"), "records")? {
            n += 1;
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(n)
}

/// Read a list file: one journal per line, the full name and the abbreviation separated by
/// " = ", a tab, a semicolon or a comma (CSV with quotes is accepted). Lines starting with # are
/// comments. Returns (pairs read, lines skipped).
pub fn import_list(conn: &Connection, text: &str) -> Result<(usize, usize)> {
    let (mut ok, mut skipped) = (0, 0);
    conn.execute_batch("BEGIN")?;
    for line in text.lines() {
        let l = line.trim().trim_start_matches('\u{feff}');
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let parts: Vec<String> = if l.contains(" = ") {
            l.splitn(2, " = ").map(|s| s.to_string()).collect()
        } else if l.contains('\t') {
            l.split('\t').map(|s| s.to_string()).collect()
        } else if l.contains(';') {
            l.split(';').map(|s| s.to_string()).collect()
        } else {
            csv_fields(l)
        };
        let clean = |s: &str| s.trim().trim_matches('"').trim().to_string();
        if parts.len() >= 2 && put(conn, &clean(&parts[0]), &clean(&parts[1]), "imported")? {
            ok += 1;
        } else {
            skipped += 1;
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok((ok, skipped))
}

fn csv_fields(l: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q = false;
    for c in l.chars() {
        match c {
            '"' => q = !q,
            ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

pub fn abbreviation(conn: &Connection, full: &str) -> Option<String> {
    conn.query_row("SELECT abbrev FROM journal_names WHERE key=?1", [key(full)], |r| r.get(0)).optional().ok().flatten()
}

pub fn full_name(conn: &Connection, abbrev: &str) -> Option<String> {
    conn.query_row("SELECT full FROM journal_names WHERE abbrev_key=?1", [key(abbrev)], |r| r.get(0)).optional().ok().flatten()
}

pub fn list(conn: &Connection, query: &str, limit: usize) -> Result<Vec<JournalName>> {
    let q = format!("%{}%", key(query));
    let mut st = conn.prepare("SELECT full, abbrev, source FROM journal_names WHERE ?1='%%' OR key LIKE ?1 OR abbrev_key LIKE ?1 ORDER BY key LIMIT ?2")?;
    let v = st.query_map(params![q, limit as i64], |r| Ok(JournalName { full: r.get(0)?, abbrev: r.get(1)?, source: r.get(2)? }))?.filter_map(|r| r.ok()).collect();
    Ok(v)
}

pub fn count(conn: &Connection) -> (i64, i64) {
    let c = |s: &str| conn.query_row("SELECT COUNT(*) FROM journal_names WHERE source=?1", [s], |r| r.get(0)).unwrap_or(0);
    (c("records"), c("imported"))
}

pub fn clear_imported(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM journal_names WHERE source='imported'", [])?)
}

/// Prepare a record for output in the chosen journal-name form. "abbreviated" puts the
/// abbreviation in place of the full name; "full" removes the abbreviation so a style that asks
/// for the short form gets the full name; "" (the style decides) fills in a missing
/// abbreviation from the list so styles that want one can use it.
pub fn apply_form(conn: &Connection, r: &mut Reference, form: &str) {
    if !r.has("container_title") && !r.has("container_title_short") {
        return;
    }
    let full = if r.has("container_title") { r.f("container_title").to_string() } else { full_name(conn, r.f("container_title_short")).unwrap_or_default() };
    let short = if r.has("container_title_short") { r.f("container_title_short").to_string() } else { abbreviation(conn, &full).unwrap_or_default() };
    match form {
        "abbreviated" if !short.is_empty() => {
            r.fields.insert("container_title".into(), short.clone());
            r.fields.insert("container_title_short".into(), short);
        }
        "full" => {
            if !full.is_empty() {
                r.fields.insert("container_title".into(), full);
            }
            r.fields.remove("container_title_short");
        }
        _ => {
            if !short.is_empty() {
                r.fields.insert("container_title_short".into(), short);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_and_lists() {
        assert_eq!(key("The Journal of Teaching & Learning"), key("journal of teaching and learning"));
        let c = Connection::open_in_memory().unwrap();
        ensure_table(&c).unwrap();
        let (n, skipped) = import_list(&c, "# comment\nJournal of Teaching Practice = J. Teach. Pract.\nMedical Education;Med. Educ.\n\"Studies, Higher Education\",Stud. High. Educ.\nnot a pair\n").unwrap();
        assert_eq!((n, skipped), (3, 1));
        assert_eq!(abbreviation(&c, "journal of teaching practice").as_deref(), Some("J. Teach. Pract."));
        assert_eq!(full_name(&c, "Stud High Educ").as_deref(), Some("Studies, Higher Education"));
        // A learned pair does not replace an imported one.
        assert!(!put(&c, "Medical Education", "Med Ed", "records").unwrap());
        let mut r = Reference::default();
        r.fields.insert("container_title".into(), "Medical Education".into());
        let mut a = r.clone();
        apply_form(&c, &mut a, "abbreviated");
        assert_eq!(a.f("container_title"), "Med. Educ.");
        let mut s = r.clone();
        apply_form(&c, &mut s, "");
        assert_eq!((s.f("container_title"), s.f("container_title_short")), ("Medical Education", "Med. Educ."));
        apply_form(&c, &mut s, "full");
        assert!(!s.has("container_title_short"));
    }
}
