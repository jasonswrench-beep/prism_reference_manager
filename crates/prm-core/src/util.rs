//! Small shared helpers: ids, time, hashing, Unicode normalization for search and sorting.

use sha2::{Digest, Sha256};
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Canonical JSON: object keys sorted at every level, compact separators. Used for the text
/// mirror, hashes and export equality so the same data always produces the same bytes.
pub fn canonical_json(v: &serde_json::Value) -> String {
    fn sort(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), sort(&m[k]));
                }
                serde_json::Value::Object(out)
            }
            serde_json::Value::Array(a) => serde_json::Value::Array(a.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(v)).unwrap_or_default()
}

pub fn canonical_json_pretty(v: &serde_json::Value) -> String {
    let compact = canonical_json(v);
    let parsed: serde_json::Value = serde_json::from_str(&compact).unwrap_or_default();
    serde_json::to_string_pretty(&parsed).unwrap_or_default()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF |
        0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x20000..=0x2A6DF)
}

/// Fold text for searching: compatibility decomposition, drop combining marks, lower case,
/// expand common ligatures, and separate CJK characters so each becomes an index token.
/// Display values are never replaced with this form.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.nfkd() {
        if is_combining_mark(c) {
            continue;
        }
        match c {
            'ß' => out.push_str("ss"),
            'æ' | 'Æ' => out.push_str("ae"),
            'œ' | 'Œ' => out.push_str("oe"),
            'ø' | 'Ø' => out.push('o'),
            'đ' | 'Đ' => out.push('d'),
            'ł' | 'Ł' => out.push('l'),
            'ı' => out.push('i'),
            c if is_cjk(c) => {
                out.push(' ');
                out.push(c);
                out.push(' ');
            }
            c => {
                for l in c.to_lowercase() {
                    out.push(l);
                }
            }
        }
    }
    out
}

/// Sort key: folded, punctuation removed, whitespace collapsed.
pub fn sort_key(s: &str) -> String {
    let f = fold(s);
    let mut out = String::with_capacity(f.len());
    let mut space = false;
    for c in f.chars() {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if c.is_whitespace() && !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim().to_string()
}

/// Normalize a DOI to its bare lower-case form without resolver prefix.
pub fn normalize_doi(s: &str) -> Option<String> {
    let t = s.trim();
    let lower = t.to_lowercase();
    let idx = lower.find("10.")?;
    let rest = &t[idx..];
    let rest = rest.trim_end_matches(['.', ',', ';', ')', ']']);
    if rest.contains('/') {
        Some(rest.to_lowercase())
    } else {
        None
    }
}

/// Escape for HTML text content and attributes.
pub fn html_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

pub fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => o.push(c),
        }
    }
    o
}

/// Write a file safely: write to a temporary sibling, flush, then rename into place.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        std::process::id()
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folding() {
        assert_eq!(fold("Gómez Ærø"), "gomez aero");
        assert_eq!(sort_key("  Ödön, von  "), "odon von");
        assert!(fold("研究").contains(" 研 "));
    }
    #[test]
    fn doi() {
        assert_eq!(normalize_doi("https://doi.org/10.1037/ABC.123."), Some("10.1037/abc.123".into()));
        assert_eq!(normalize_doi("no doi"), None);
    }
}
