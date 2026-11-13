//! Citation key generation with per-type patterns, uniqueness and locking.
//!
//! Pattern tokens: `[auth]` first author surname, `[Auth]` capitalized, `[authors2]` first two
//! surnames, `[year]`, `[shorttitle]` first significant title word, `[title3]` first three
//! significant words, `[type]`. Non-ASCII letters are transliterated to ASCII for portability.

use crate::model::Reference;
use crate::util::fold;
use std::collections::BTreeSet;

pub const DEFAULT_PATTERN: &str = "[auth][year][shorttitle]";

const STOP: &[&str] = &["a", "an", "the", "of", "on", "in", "and", "for", "to", "with", "at", "by", "from"];

fn ascii(s: &str) -> String {
    fold(s).chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

fn surname(r: &Reference, i: usize) -> String {
    let names: Vec<_> = r.creators.iter().filter(|c| c.role == "author" || c.role == "editor").collect();
    names.get(i).map(|c| ascii(&if c.name.literal.is_empty() { c.name.family.clone() } else { c.name.literal.split_whitespace().next().unwrap_or("").to_string() })).unwrap_or_default()
}

fn title_words(r: &Reference, n: usize) -> String {
    r.title()
        .split_whitespace()
        .map(|w| ascii(w))
        .filter(|w| !w.is_empty() && !STOP.contains(&w.as_str()))
        .take(n)
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect()
}

pub fn generate(r: &Reference, pattern: &str) -> String {
    let year = r.year().map(|y| y.to_string()).unwrap_or_else(|| "nd".into());
    let a = surname(r, 0);
    let a_low = a.to_lowercase();
    let cap = |s: &str| {
        let mut c = s.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    };
    let mut out = pattern
        .replace("[authors2]", &format!("{}{}", a_low, cap(&surname(r, 1).to_lowercase())))
        .replace("[Auth]", &cap(&a_low))
        .replace("[auth]", &a_low)
        .replace("[year]", &year)
        .replace("[shorttitle]", &title_words(r, 1).to_lowercase())
        .replace("[title3]", &title_words(r, 3))
        .replace("[type]", &r.rtype);
    if out.is_empty() || out == year {
        out = format!("ref{year}{}", title_words(r, 1).to_lowercase());
    }
    out
}

/// Make `key` unique among `taken` by appending a, b, c ...
pub fn unique(key: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(key) {
        return key.to_string();
    }
    for i in 0..26 * 27 {
        let suffix = if i < 26 { ((b'a' + i as u8) as char).to_string() } else { format!("{}{}", (b'a' + (i / 26 - 1) as u8) as char, (b'a' + (i % 26) as u8) as char) };
        let k = format!("{key}{suffix}");
        if !taken.contains(&k) {
            return k;
        }
    }
    format!("{key}{}", crate::util::new_id()[..6].to_string())
}
