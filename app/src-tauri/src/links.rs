//! prism-rm:// links (spec EXT-013): documents and other programs can open a source, a
//! collection, an annotation, a note or a project in the program.
//!
//! `prism-rm://open/<kind>/<id>?library=<library id>`
//!
//! A link only shows the item; it never changes anything. A link to another library on this
//! computer names it, and the window asks before opening it.

use prm_core::model::*;
use serde_json::{json, Value};
use std::path::PathBuf;

pub const SCHEME: &str = "prism-rm://";
pub const KINDS: &[&str] = &["source", "collection", "annotation", "note", "project"];

#[derive(Debug, PartialEq)]
pub struct Link {
    pub kind: String,
    pub id: String,
    pub library: String,
}

fn safe(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_:.".contains(c))
}

/// Parse a link; anything unexpected is refused.
pub fn parse(url: &str) -> Option<Link> {
    let lower = url.trim();
    let rest = lower.get(SCHEME.len()..).filter(|_| lower.to_lowercase().starts_with(SCHEME))?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut parts = path.trim_end_matches('/').split('/');
    if parts.next()? != "open" {
        return None;
    }
    let kind = parts.next()?.to_string();
    let id = percent_decode(parts.next()?);
    if parts.next().is_some() || !KINDS.contains(&kind.as_str()) || !safe(&id) {
        return None;
    }
    let library = query.split('&').find_map(|kv| kv.strip_prefix("library=")).map(percent_decode).unwrap_or_default();
    if !library.is_empty() && !safe(&library) {
        return None;
    }
    Some(Link { kind, id, library })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn make(kind: &str, id: &str, library: &str) -> String {
    format!("{SCHEME}open/{kind}/{id}?library={library}")
}

/// The library id stored in a library folder, read without opening it for writing.
fn library_id_at(path: &str) -> Option<String> {
    let db = PathBuf::from(path).join(prm_core::store::DB_FILE);
    let c = rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    c.query_row("SELECT value FROM meta WHERE key='library_id'", [], |r| r.get(0)).ok()
}

/// Where a link leads: the view and its parameters in the open library, or the other library
/// that holds it, or why it cannot be followed.
pub fn resolve(st: &crate::AppState, url: &str) -> Value {
    let Some(l) = parse(url) else { return json!({"ok": false, "message": "This is not a link this program can open."}) };
    let Some(lib) = st.lib.as_ref() else { return json!({"ok": false, "message": "Open a library first."}) };
    let here = lib.store.meta("library_id").unwrap_or_default();
    if !l.library.is_empty() && l.library != here {
        let other = st.settings.recent_libraries.iter().find(|p| library_id_at(p).as_deref() == Some(l.library.as_str()));
        return match other {
            Some(p) => json!({"ok": false, "other_library": p, "name": PathBuf::from(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()}),
            None => json!({"ok": false, "message": "The link points to a library that is not open and was not found among recent libraries on this computer."}),
        };
    }
    let gone = |what: &str| json!({"ok": false, "message": format!("That {what} is no longer in this library.")});
    match l.kind.as_str() {
        "source" => match lib.store.get::<Reference>(&l.id) {
            Ok(Some(_)) => json!({"ok": true, "view": "library", "params": {"select": l.id}}),
            _ => gone("source"),
        },
        "collection" => match lib.store.get::<Collection>(&l.id) {
            Ok(Some(_)) => json!({"ok": true, "view": "library", "params": {"collection": l.id}}),
            _ => gone("collection"),
        },
        "annotation" => match lib.store.get::<Annotation>(&l.id) {
            Ok(Some(a)) => match lib.store.get::<Attachment>(&a.attachment_id) {
                Ok(Some(att)) => json!({"ok": true, "view": "reader", "project": a.project_id, "params": {"ref": att.ref_id, "att": att.id, "page": a.page_index + 1, "ann": a.id}}),
                _ => gone("annotation's file"),
            },
            _ => gone("annotation"),
        },
        "note" => match lib.store.get::<Note>(&l.id) {
            Ok(Some(n)) => json!({"ok": true, "view": "notes", "project": n.project_id, "params": {"note": n.id}}),
            _ => gone("note"),
        },
        "project" => match lib.store.get::<Project>(&l.id) {
            Ok(Some(p)) => json!({"ok": true, "view": "projects", "project": p.id, "params": {}}),
            _ => gone("project"),
        },
        _ => json!({"ok": false, "message": "This is not a link this program can open."}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_parse_strictly() {
        let l = parse("prism-rm://open/source/7a39650b-4c65?library=abc-123").unwrap();
        assert_eq!(l, Link { kind: "source".into(), id: "7a39650b-4c65".into(), library: "abc-123".into() });
        assert_eq!(parse(&make("annotation", "x1", "")).unwrap().kind, "annotation");
        assert!(parse("prism-rm://delete/source/x").is_none());
        assert!(parse("prism-rm://open/file/x").is_none());
        assert!(parse("prism-rm://open/source/../../etc").is_none());
        assert!(parse("prism-rm://open/source/a%22b").is_none());
        assert!(parse("https://open/source/x").is_none());
        assert!(parse("prism-rm://open/source/x/extra").is_none());
    }
}
