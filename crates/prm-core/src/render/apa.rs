//! APA 7th edition composition.
//!
//! Parenthetical and narrative citations are separate operations built from the same author
//! and date elements; narrative output is never derived by editing a formatted parenthetical
//! string. Disambiguation (year suffixes, et al. ambiguity, first-author initials) is computed
//! over the manuscript's reference set passed in the context.

use super::titlecase::title_case;
use super::{fingerprint, Context, Out, Paragraph, Rendered, Warning};
use crate::model::{CiteItem, Name, NameKind, PartialDate, Reference};
use crate::util::sort_key;
use std::collections::BTreeMap;

pub const STYLE_VERSION: &str = "apa7-composer-2026.11";

pub fn style_hash() -> String {
    crate::util::sha256_hex(format!("{STYLE_VERSION}|{}", super::PROCESSOR_VERSION).as_bytes())[..16].to_string()
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const SEASONS: [&str; 4] = ["Spring", "Summer", "Fall", "Winter"];

/// Types whose titles are standalone works (italic in the reference list).
fn standalone(rtype: &str) -> bool {
    !matches!(
        rtype,
        "article-journal" | "article-magazine" | "article-newspaper" | "review" | "editorial" | "chapter" | "entry-encyclopedia" | "entry-dictionary" | "post-weblog" | "post" | "paper-conference" | "broadcast" | "podcast" | "song" | "interview"
    )
}

/// Types APA does not place in the reference list.
fn text_only(rtype: &str) -> bool {
    matches!(rtype, "personal-communication")
}

/// Types with dedicated composition in this engine. Others render through a generic pattern
/// and carry an "unsupported case" warning.
fn supported(rtype: &str) -> bool {
    matches!(
        rtype,
        "article-journal" | "article-magazine" | "article-newspaper" | "preprint" | "editorial" | "book" | "edited-book" | "chapter" | "entry-encyclopedia" | "entry-dictionary" | "report" | "thesis" | "paper-conference" | "presentation" | "webpage" | "post-weblog" | "dataset" | "software" | "personal-communication" | "government-document" | "manuscript"
    )
}

fn dash(s: &str) -> String {
    // Number ranges use an en dash.
    let re = re!(r"(\w)\s*[-‐‑‒―]\s*(\w)");
    re.replace_all(s.trim(), "$1–$2").into_owned()
}

/// "Lin-Manuel" -> "L.-M.", "John Paul" -> "J. P.", "J.P." -> "J. P."
pub fn initials(given: &str) -> String {
    let mut parts = Vec::new();
    for word in given.split_whitespace() {
        let hy: Vec<String> = word
            .split('-')
            .filter(|p| !p.is_empty())
            .map(|p| {
                // Already an initial or a run of initials like "J.P."
                let letters: Vec<char> = p.chars().filter(|c| c.is_alphabetic()).collect();
                if p.contains('.') && letters.len() > 1 && p.chars().filter(|c| *c == '.').count() >= letters.len() {
                    letters.iter().map(|c| format!("{c}.")).collect::<Vec<_>>().join(" ")
                } else {
                    letters.first().map(|c| format!("{}.", c.to_uppercase())).unwrap_or_default()
                }
            })
            .filter(|s| !s.is_empty())
            .collect();
        if !hy.is_empty() {
            parts.push(hy.join("-"));
        }
    }
    parts.join(" ")
}

/// "Chen, L." style inverted name for the reference list.
fn inverted(n: &Name) -> String {
    match n.kind {
        NameKind::Organization | NameKind::Mononym => n.literal.clone(),
        NameKind::Person => {
            let mut s = n.surname();
            let i = initials(&n.given);
            if !i.is_empty() {
                s.push_str(", ");
                s.push_str(&i);
            }
            if !n.suffix.is_empty() {
                s.push_str(", ");
                s.push_str(&n.suffix);
            }
            s
        }
    }
}

/// "L. Chen" style name for editors inside a chapter entry.
fn direct(n: &Name) -> String {
    match n.kind {
        NameKind::Organization | NameKind::Mononym => n.literal.clone(),
        NameKind::Person => {
            let i = initials(&n.given);
            let mut s = if i.is_empty() { n.surname() } else { format!("{} {}", i, n.surname()) };
            if !n.suffix.is_empty() {
                s.push_str(", ");
                s.push_str(&n.suffix);
            }
            s
        }
    }
}

fn join_amp(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2 => format!("{}, & {}", names[0], names[1]),
        n if n <= 20 => format!("{}, & {}", names[..n - 1].join(", "), names[n - 1]),
        n => format!("{}, . . . {}", names[..19].join(", "), names[n - 1]),
    }
}

fn join_amp_direct(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2 => format!("{} & {}", names[0], names[1]),
        n => format!("{}, & {}", names[..n - 1].join(", "), names[n - 1]),
    }
}

/// The creators occupying the author position: authors; otherwise editors (for edited works);
/// otherwise directors/hosts/inventors for media and patents.
fn author_position(r: &Reference) -> (Vec<&Name>, &'static str) {
    for role in ["author", "inventor", "director", "host", "composer", "performer"] {
        let v = r.creators_by(role);
        if !v.is_empty() {
            let r: &'static str = match role {
                "director" => "director",
                "host" => "host",
                _ => "author",
            };
            return (v, r);
        }
    }
    let eds = r.creators_by("editor");
    if !eds.is_empty() && !matches!(r.rtype.as_str(), "chapter" | "entry-encyclopedia" | "entry-dictionary") {
        return (eds, "editor");
    }
    (Vec::new(), "")
}

/// Identity of the author element for disambiguation and sorting.
fn author_identity(r: &Reference) -> Vec<String> {
    let (names, _) = author_position(r);
    if names.is_empty() {
        return vec![format!("~title:{}", sort_key(strip_article(r.title())))];
    }
    names.iter().map(|n| sort_key(&format!("{} {}", n.surname(), initials(&n.given)))).collect()
}

fn strip_article(t: &str) -> &str {
    for a in ["The ", "A ", "An "] {
        if let Some(rest) = t.strip_prefix(a) {
            return rest;
        }
    }
    t
}

/// The date label used for suffix grouping and sorting: year, "n.d." or "in press".
fn date_label(r: &Reference) -> String {
    match &r.issued {
        Some(d) if d.year.is_some() => {
            let mut s = d.year.unwrap().to_string();
            if let Some(e) = &d.end {
                if let Some(y2) = e.year {
                    if y2 != d.year.unwrap() {
                        s = format!("{}–{}", s, y2);
                    }
                }
            }
            if d.approximate {
                s = format!("ca. {s}");
            }
            s
        }
        Some(d) if !d.status.is_empty() => d.status.clone(),
        _ => "n.d.".into(),
    }
}

fn date_sort(r: &Reference) -> (i32, i32, i32, i32) {
    match &r.issued {
        Some(d) if d.year.is_some() => (1, d.year.unwrap(), d.month.unwrap_or(0) as i32, d.day.unwrap_or(0) as i32),
        Some(d) if !d.status.is_empty() => (2, 0, 0, 0),
        _ => (0, 0, 0, 0),
    }
}

/// Disambiguation computed over a manuscript context.
pub struct Disambig {
    pub suffix: BTreeMap<String, String>,
    /// Number of names to show before "et al." for each reference with three or more authors.
    pub names_shown: BTreeMap<String, usize>,
    /// References whose first author needs initials in text.
    pub initials: BTreeMap<String, bool>,
}

pub fn sort_refs<'a>(refs: &mut Vec<&'a Reference>) {
    refs.sort_by(|a, b| {
        author_identity(a)
            .cmp(&author_identity(b))
            .then(date_sort(a).cmp(&date_sort(b)))
            .then(sort_key(strip_article(a.title())).cmp(&sort_key(strip_article(b.title()))))
            .then(a.id.cmp(&b.id))
    });
}

pub fn disambiguate(set: &[&Reference]) -> Disambig {
    let mut sorted: Vec<&Reference> = set.iter().copied().filter(|r| !text_only(&r.rtype)).collect();
    sort_refs(&mut sorted);
    // Year suffixes: identical author element and identical date label.
    let mut suffix = BTreeMap::new();
    let mut groups: BTreeMap<(Vec<String>, String), Vec<&Reference>> = BTreeMap::new();
    for r in &sorted {
        groups.entry((author_identity(r), date_label(r))).or_default().push(r);
    }
    for (_, g) in groups {
        if g.len() > 1 {
            for (i, r) in g.iter().enumerate() {
                let letter = suffix_letter(i);
                suffix.insert(r.id.clone(), letter);
            }
        }
    }
    // Et al. ambiguity: works with 3+ authors that shorten to the same form and year.
    let mut names_shown = BTreeMap::new();
    let surnames = |r: &Reference| -> Vec<String> { author_position(r).0.iter().map(|n| sort_key(&n.surname())).collect() };
    for r in set {
        let s = surnames(r);
        if s.len() < 3 {
            continue;
        }
        let mut n = 1;
        loop {
            let clash = set.iter().any(|o| {
                if o.id == r.id {
                    return false;
                }
                let os = surnames(o);
                os.len() >= 3 && os != s && date_label(o) == date_label(r) && os.len() > n && os[..n] == s[..n]
            });
            if !clash || n >= s.len() - 1 {
                break;
            }
            n += 1;
        }
        if n >= s.len() - 1 && n > 1 {
            n = s.len();
        }
        names_shown.insert(r.id.clone(), n);
    }
    // First authors with the same surname but different initials need initials in text.
    let mut initials_map = BTreeMap::new();
    for r in set {
        let (a, _) = author_position(r);
        let Some(first) = a.first() else { continue };
        if first.kind != NameKind::Person {
            continue;
        }
        let clash = set.iter().any(|o| {
            let (b, _) = author_position(o);
            b.first().map(|f| f.kind == NameKind::Person && sort_key(&f.surname()) == sort_key(&first.surname()) && initials(&f.given) != initials(&first.given)).unwrap_or(false)
        });
        if clash {
            initials_map.insert(r.id.clone(), true);
        }
    }
    Disambig { suffix, names_shown, initials: initials_map }
}

fn suffix_letter(i: usize) -> String {
    let letters = "abcdefghijklmnopqrstuvwxyz";
    if i < 26 {
        letters[i..i + 1].to_string()
    } else {
        format!("{}{}", &letters[(i / 26 - 1)..(i / 26)], &letters[i % 26..i % 26 + 1])
    }
}

// ---- in-text ---------------------------------------------------------------------------------

/// In-text author element. `and` is "&" in parentheses and "and" in narrative text.
fn intext_author(r: &Reference, d: &Disambig, and: &str, out: &mut Out) -> bool {
    let (names, _) = author_position(r);
    if names.is_empty() {
        // Title in the author position, title case; quotation marks for parts of works,
        // italics for standalone works.
        let t = if r.has("short_title") { r.f("short_title") } else { r.title() };
        if t.is_empty() {
            return false;
        }
        let tc = title_case(t, &[]);
        if standalone(&r.rtype) {
            out.i(&tc);
        } else {
            out.t(&format!("“{}”", tc));
        }
        return true;
    }
    let with_initials = d.initials.get(&r.id).copied().unwrap_or(false);
    let first = |n: &Name| -> String {
        if with_initials && n.kind == NameKind::Person && !n.given.is_empty() {
            format!("{} {}", initials(&n.given), n.surname())
        } else {
            n.surname()
        }
    };
    let s: Vec<String> = names.iter().enumerate().map(|(i, n)| if i == 0 { first(n) } else { n.surname() }).collect();
    let text = match s.len() {
        1 => s[0].clone(),
        2 => format!("{} {} {}", s[0], and, s[1]),
        n => {
            let shown = d.names_shown.get(&r.id).copied().unwrap_or(1);
            if shown >= n {
                format!("{}, {} {}", s[..n - 1].join(", "), and, s[n - 1])
            } else {
                if shown > 1 { format!("{}, et al.", s[..shown].join(", ")) } else { format!("{} et al.", s[0]) }
            }
        }
    };
    out.t(&text);
    true
}

fn intext_year(r: &Reference, d: &Disambig) -> String {
    let mut y = date_label(r);
    let suf = d.suffix.get(&r.id).cloned().unwrap_or_default();
    if !suf.is_empty() {
        if y == "n.d." || y == "in press" {
            y = format!("{y}-{suf}");
        } else {
            y.push_str(&suf);
        }
    }
    if let Some(od) = &r.original_date {
        if let Some(oy) = od.year {
            y = format!("{oy}/{y}");
        }
    }
    y
}

pub fn locator_text(label: &str, value: &str) -> String {
    let v = dash(value);
    if v.is_empty() {
        return String::new();
    }
    let plural = v.contains('–') || v.contains(',') || v.contains('&');
    let l = match label {
        "" | "page" | "pages" => if plural { "pp." } else { "p." },
        "paragraph" | "para" => if plural { "paras." } else { "para." },
        "chapter" => "Chapter",
        "section" => "Section",
        "figure" => "Figure",
        "table" => "Table",
        "line" => if plural { "lines" } else { "line" },
        "slide" => "Slide",
        "verse" => "",
        "timestamp" | "time" => "",
        "column" => "col.",
        "volume" => "Vol.",
        "note" => "n.",
        other => other,
    };
    if l.is_empty() { v } else { format!("{l} {v}") }
}

fn personal_comm(r: &Reference, item: &CiteItem, narrative: bool, out: &mut Out) {
    let (names, _) = author_position(r);
    let who = names.first().map(|n| direct(n)).unwrap_or_else(|| "Anonymous".into());
    let date = r
        .issued
        .as_ref()
        .map(|d| long_date(d))
        .unwrap_or_else(|| "n.d.".into());
    if narrative {
        out.t(&format!("{who} (personal communication, {date}"));
    } else {
        out.t(&format!("{who}, personal communication, {date}"));
    }
    let _ = item;
}

fn long_date(d: &PartialDate) -> String {
    match (d.year, d.month, d.day) {
        (Some(y), Some(m), Some(day)) if (1..=12).contains(&m) => format!("{} {}, {}", MONTHS[m as usize - 1], day, y),
        (Some(y), Some(m), None) if (1..=12).contains(&m) => format!("{} {}", MONTHS[m as usize - 1], y),
        (Some(y), _, _) => y.to_string(),
        _ => "n.d.".into(),
    }
}

/// Render an in-text citation cluster.
pub fn cite(items: &[CiteItem], narrative: bool, refs: &[&Reference], ctx: &Context) -> Rendered {
    let mut set: Vec<&Reference> = ctx.set.clone();
    for r in refs {
        if !set.iter().any(|s| s.id == r.id) {
            set.push(r);
        }
    }
    let d = disambiguate(&set);
    let find = |id: &str| refs.iter().copied().find(|r| r.id == id);
    let mut warnings = Vec::new();
    let mut out = Out::default();

    // Sort parenthetical clusters into reference-list order; narrative keeps the given order.
    let mut ordered: Vec<(&CiteItem, &Reference)> = items.iter().filter_map(|i| find(&i.ref_id).map(|r| (i, r))).collect();
    for i in items {
        if find(&i.ref_id).is_none() {
            warnings.push(Warning { ref_id: i.ref_id.clone(), level: "review".into(), code: "missing-record".into(), message: "A cited source is missing from the library.".into() });
        }
    }
    if !narrative && ordered.len() > 1 {
        let mut refs_sorted: Vec<&Reference> = ordered.iter().map(|(_, r)| *r).collect();
        sort_refs(&mut refs_sorted);
        ordered.sort_by_key(|(_, r)| refs_sorted.iter().position(|x| x.id == r.id).unwrap_or(0));
    }
    for (_, r) in &ordered {
        collect_warnings(r, &mut warnings, true);
    }

    if narrative {
        let n = ordered.len();
        for (idx, (item, r)) in ordered.iter().enumerate() {
            if idx > 0 {
                out.t(if idx == n - 1 { if n > 2 { ", and " } else { " and " } } else { ", " });
            }
            if !item.prefix.is_empty() {
                out.t(&format!("{} ", item.prefix.trim()));
            }
            if r.rtype == "personal-communication" {
                personal_comm(r, item, true, &mut out);
            } else {
                if !item.suppress_author && !intext_author(r, &d, "and", &mut out) {
                    out.t("Anonymous");
                }
                out.t(" (");
                out.t(&intext_year(r, &d));
            }
            let loc = locator_text(&item.locator_label, &item.locator);
            if !loc.is_empty() {
                out.t(&format!(", {loc}"));
            }
            if !item.suffix.is_empty() {
                out.t(&format!(", {}", item.suffix.trim().trim_start_matches(',').trim()));
            }
            out.t(")");
        }
    } else {
        out.t("(");
        let mut prev_author: Option<String> = None;
        for (idx, (item, r)) in ordered.iter().enumerate() {
            let mut a = Out::default();
            let has_author = if r.rtype == "personal-communication" { false } else { intext_author(r, &d, "&", &mut a) };
            let akey = a.text();
            let combinable = prev_author.as_deref() == Some(akey.as_str()) && item.prefix.is_empty() && item.locator.is_empty() && idx > 0;
            if idx > 0 {
                out.t(if combinable { ", " } else { "; " });
            }
            if !item.prefix.is_empty() {
                out.t(&format!("{} ", item.prefix.trim()));
            }
            if r.rtype == "personal-communication" {
                personal_comm(r, item, false, &mut out);
            } else {
                if !combinable && !item.suppress_author {
                    if has_author {
                        if akey.ends_with('”') {
                            // American punctuation: the comma goes inside the closing quotation mark.
                            let mut a2 = a.clone();
                            if let Some(last) = a2.runs.last_mut() {
                                last.text.pop();
                                last.text.push_str(",” ");
                            }
                            out.extend(&a2);
                        } else {
                            out.extend(&a);
                            out.t(", ");
                        }
                    } else {
                        out.t("Anonymous, ");
                    }
                }
                out.t(&intext_year(r, &d));
            }
            let loc = locator_text(&item.locator_label, &item.locator);
            if !loc.is_empty() {
                out.t(&format!(", {loc}"));
            }
            if !item.suffix.is_empty() {
                out.t(&format!(", {}", item.suffix.trim().trim_start_matches(',').trim()));
            }
            prev_author = if item.locator.is_empty() { Some(akey) } else { None };
        }
        out.t(")");
    }
    let text = out.text();
    Rendered {
        fingerprint: fingerprint("apa-7", refs, ctx),
        provisional: ctx.standalone,
        runs: out.runs,
        text,
        paragraphs: vec![],
        warnings,
    }
}

/// A short quotation with its citation: “quoted words” (Author, Year, p. X).
/// Quotations of 40 words or more are block quotations: no quotation marks, citation after the
/// final punctuation.
pub fn quote_with_citation(quote: &str, cite: &Rendered) -> (Out, bool) {
    let q = quote.trim();
    let words = q.split_whitespace().count();
    let mut out = Out::default();
    if words >= 40 {
        out.t(q);
        if !q.ends_with(['.', '?', '!']) {
            out.t(".");
        }
        out.t(" ");
        for r in &cite.runs {
            out.push_run(r.clone());
        }
        return (out, true);
    }
    let (body, end) = match q.chars().last() {
        Some(c @ ('.' | ',')) => (&q[..q.len() - c.len_utf8()], "."),
        Some(c @ ('?' | '!')) => (q, if c == '?' { "" } else { "" }),
        _ => (q, "."),
    };
    out.t(&format!("“{}”", body.trim_matches(['"', '“', '”'])));
    out.t(" ");
    for r in &cite.runs {
        out.push_run(r.clone());
    }
    out.t(end);
    (out, false)
}

// ---- reference list ----------------------------------------------------------------------------

fn ref_date(r: &Reference, d: &Disambig, full: bool) -> String {
    let suf = d.suffix.get(&r.id).cloned().unwrap_or_default();
    match &r.issued {
        Some(pd) if pd.year.is_some() => {
            let y = pd.year.unwrap();
            let mut s = if pd.approximate { format!("ca. {y}") } else { y.to_string() };
            if let Some(e) = &pd.end {
                if let Some(y2) = e.year {
                    if y2 != y {
                        s = format!("{y}–{y2}");
                    }
                }
            }
            s.push_str(&suf);
            if full {
                if let Some(season) = pd.season {
                    if (1..=4).contains(&season) {
                        s.push_str(&format!(", {}", SEASONS[season as usize - 1]));
                    }
                } else if let Some(m) = pd.month {
                    if (1..=12).contains(&m) {
                        s.push_str(&format!(", {}", MONTHS[m as usize - 1]));
                        if let Some(day) = pd.day {
                            s.push_str(&format!(" {day}"));
                            if let Some(e) = &pd.end {
                                if e.year.is_none() || e.year == pd.year {
                                    if let (Some(em), Some(ed)) = (e.month, e.day) {
                                        if em == m {
                                            s.push_str(&format!("–{ed}"));
                                        } else if (1..=12).contains(&em) {
                                            s.push_str(&format!(" – {} {}", MONTHS[em as usize - 1], ed));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            format!("({s}).")
        }
        Some(pd) if !pd.status.is_empty() => {
            if suf.is_empty() { format!("({}).", pd.status) } else { format!("({}-{}).", pd.status, suf) }
        }
        _ => {
            if suf.is_empty() { "(n.d.).".into() } else { format!("(n.d.-{suf}).") }
        }
    }
}

/// Dated in month-day form in the reference list.
fn full_date_type(t: &str) -> bool {
    matches!(t, "article-magazine" | "article-newspaper" | "webpage" | "post-weblog" | "post" | "presentation" | "broadcast" | "podcast" | "speech" | "personal-communication" | "letter" | "film")
}

fn author_element(r: &Reference, out: &mut Out) -> bool {
    let (names, role) = author_position(r);
    if names.is_empty() {
        return false;
    }
    let inv: Vec<String> = names.iter().map(|n| inverted(n)).collect();
    let mut s = join_amp(&inv);
    match role {
        "editor" => s.push_str(if names.len() > 1 { " (Eds.)" } else { " (Ed.)" }),
        "director" => s.push_str(if names.len() > 1 { " (Directors)" } else { " (Director)" }),
        "host" => s.push_str(if names.len() > 1 { " (Hosts)" } else { " (Host)" }),
        _ => {}
    }
    out.t(&s);
    out.period();
    true
}

/// Parenthetical description after a standalone title: edition, volume, report number,
/// translator, version. Returns e.g. " (2nd ed., Vol. 3)".
fn title_parens(r: &Reference, include_pages: Option<&str>) -> String {
    let mut parts = Vec::new();
    let trans: Vec<String> = r.creators_by("translator").iter().map(|n| direct(n)).collect();
    if !trans.is_empty() {
        parts.push(format!("{}, Trans.", join_amp_direct(&trans)));
    }
    // Editors of an authored book appear here.
    if r.rtype == "book" && !r.creators_by("author").is_empty() {
        let eds: Vec<String> = r.creators_by("editor").iter().map(|n| direct(n)).collect();
        if !eds.is_empty() {
            parts.insert(0, format!("{}, {}", join_amp_direct(&eds), if eds.len() > 1 { "Eds." } else { "Ed." }));
        }
    }
    let ed = r.f("edition").trim();
    if !ed.is_empty() {
        parts.push(edition_text(ed));
    }
    if r.has("version") && matches!(r.rtype.as_str(), "dataset" | "software" | "test-instrument" | "preprint") {
        parts.push(format!("Version {}", r.f("version").trim()));
    }
    if r.has("volume") && matches!(r.rtype.as_str(), "book" | "edited-book" | "chapter" | "entry-encyclopedia" | "proceedings") {
        parts.push(format!("Vol. {}", dash(r.f("volume"))));
    }
    if r.has("number") && matches!(r.rtype.as_str(), "report" | "government-document" | "standard") {
        let g = r.f("genre").trim();
        let label = if g.is_empty() { "Report".to_string() } else { g.to_string() };
        parts.push(format!("{} No. {}", label, r.f("number").trim()));
    }
    if r.has("number") && r.rtype == "thesis" {
        parts.push(format!("Publication No. {}", r.f("number").trim()));
    }
    if let Some(p) = include_pages {
        if !p.is_empty() {
            let p = dash(p);
            parts.push(if p.contains('–') || p.contains(',') { format!("pp. {p}") } else { format!("p. {p}") });
        }
    }
    if parts.is_empty() { String::new() } else { format!(" ({})", parts.join(", ")) }
}

fn edition_text(ed: &str) -> String {
    let lower = ed.to_lowercase();
    if lower.contains("ed") {
        return ed.to_string();
    }
    if let Ok(n) = ed.parse::<u32>() {
        let suffix = match (n % 10, n % 100) {
            (1, x) if x != 11 => "st",
            (2, x) if x != 12 => "nd",
            (3, x) if x != 13 => "rd",
            _ => "th",
        };
        return format!("{n}{suffix} ed.");
    }
    format!("{ed} ed.")
}

/// Bracketed description: [Doctoral dissertation, University], [Data set], [Paper presentation].
fn bracket(r: &Reference) -> Option<String> {
    let g = r.f("genre").trim();
    let m = r.f("medium").trim();
    match r.rtype.as_str() {
        "thesis" => {
            let g = if g.is_empty() { "Doctoral dissertation" } else { g };
            if g.to_lowercase().starts_with("unpublished") || r.f("archive").is_empty() && r.f("url").is_empty() && r.f("doi").is_empty() {
                // Unpublished: institution follows as the source.
                Some(format!("[{g}]"))
            } else if r.has("publisher") {
                Some(format!("[{g}, {}]", r.f("publisher").trim()))
            } else {
                Some(format!("[{g}]"))
            }
        }
        "dataset" => Some(format!("[{}]", if m.is_empty() { "Data set" } else { m })),
        "software" => Some(format!("[{}]", if m.is_empty() { "Computer software" } else { m })),
        "presentation" => Some(format!("[{}]", if g.is_empty() { "Paper presentation" } else { g })),
        "film" | "broadcast" | "podcast" | "song" | "graphic" | "map" | "post" if !m.is_empty() => Some(format!("[{m}]")),
        "report" | "government-document" if !g.is_empty() && !r.has("number") => Some(format!("[{g}]")),
        "manuscript" => Some(format!("[{}]", if g.is_empty() { "Unpublished manuscript" } else { g })),
        _ => None,
    }
}

fn publisher_text(r: &Reference) -> String {
    let p = r.f("publisher").trim().to_string();
    if p.is_empty() {
        return p;
    }
    // Omit the publisher when it is the same as the group author.
    let (names, _) = author_position(r);
    if names.iter().any(|n| n.kind == NameKind::Organization && sort_key(&n.literal) == sort_key(&p)) {
        return String::new();
    }
    p
}

fn doi_url(r: &Reference, out: &mut Out) {
    if r.has("doi") {
        let doi = crate::util::normalize_doi(r.f("doi")).unwrap_or_else(|| r.f("doi").trim().to_string());
        out.t(" ");
        out.link(&format!("https://doi.org/{doi}"));
    } else if r.has("url") {
        out.t(" ");
        if let Some(acc) = &r.accessed {
            if acc.year.is_some() {
                out.t(&format!("Retrieved {}, from ", long_date(acc)));
            }
        }
        out.link(r.f("url").trim());
    }
}

fn title_element(r: &Reference, out: &mut Out, italic: bool, parens: &str) {
    let t = r.title().trim();
    let t = if t.is_empty() {
        // No title: a bracketed description is used instead.
        let g = r.f("genre");
        if g.is_empty() { "[Untitled]".to_string() } else { format!("[{g}]") }
    } else {
        t.to_string()
    };
    if italic {
        out.i(&t);
    } else {
        out.t(&t);
    }
    if !parens.is_empty() {
        out.t(parens);
    }
    if let Some(b) = bracket(r) {
        out.t(&format!(" {b}"));
    }
    out.period();
}

/// Render one reference-list entry.
pub fn entry(r: &Reference, d: &Disambig) -> Out {
    let mut out = Out::default();
    let rt = r.rtype.as_str();
    let is_standalone = standalone(rt) && !matches!(rt, "chapter" | "entry-encyclopedia" | "entry-dictionary");
    let date = ref_date(r, d, full_date_type(rt));

    let parens_main = match rt {
        "chapter" | "entry-encyclopedia" | "entry-dictionary" | "paper-conference" | "article-journal" | "article-magazine" | "article-newspaper" | "review" | "editorial" => String::new(),
        _ => title_parens(r, None),
    };

    let has_author = author_element(r, &mut out);
    if has_author {
        out.t(" ");
        out.t(&date);
        out.t(" ");
        title_element(r, &mut out, is_standalone, &parens_main);
    } else {
        // Title moves to the author position, followed by the date.
        title_element(r, &mut out, is_standalone, &parens_main);
        out.t(" ");
        out.t(&date);
    }

    match rt {
        "article-journal" | "article-magazine" | "review" | "editorial" => {
            let j = r.f("container_title").trim();
            if !j.is_empty() {
                out.t(" ");
                out.i(j);
                if r.has("volume") {
                    out.i(", ");
                    out.i(&dash(r.f("volume")));
                    if r.has("issue") {
                        out.t(&format!("({})", dash(r.f("issue"))));
                    }
                } else if r.has("issue") {
                    out.t(&format!(", ({})", dash(r.f("issue"))));
                }
                if r.has("article_number") {
                    out.t(&format!(", Article {}", r.f("article_number").trim()));
                } else if r.has("page") {
                    out.t(&format!(", {}", dash(r.f("page"))));
                }
                out.t(".");
            }
        }
        "article-newspaper" | "post-weblog" => {
            let j = r.f("container_title").trim();
            if !j.is_empty() {
                out.t(" ");
                out.i(j);
                if r.has("page") {
                    out.t(&format!(", {}", dash(r.f("page"))));
                }
                out.t(".");
            }
        }
        "chapter" | "entry-encyclopedia" | "entry-dictionary" | "paper-conference" => {
            let book = r.f("container_title").trim();
            let eds: Vec<String> = r.creators_by("editor").iter().map(|n| direct(n)).collect();
            if !book.is_empty() {
                out.t(" In ");
                if !eds.is_empty() {
                    out.t(&join_amp_direct(&eds));
                    out.t(if eds.len() > 1 { " (Eds.), " } else { " (Ed.), " });
                }
                out.i(book);
                let parens = title_parens(r, Some(r.f("page")));
                out.t(&parens);
                out.t(".");
            }
            let p = publisher_text(r);
            if !p.is_empty() {
                out.t(&format!(" {p}."));
            }
        }
        "webpage" | "post" => {
            let site = r.f("container_title").trim().to_string();
            let site = if site.is_empty() { publisher_text(r) } else { site };
            let (names, _) = author_position(r);
            let same_as_author = names.iter().any(|n| n.kind == NameKind::Organization && sort_key(&n.literal) == sort_key(&site));
            if !site.is_empty() && !same_as_author {
                out.t(&format!(" {site}."));
            }
        }
        "thesis" => {
            let g = r.f("genre").to_lowercase();
            let unpublished = g.starts_with("unpublished") || (r.f("archive").is_empty() && r.f("url").is_empty() && r.f("doi").is_empty());
            if unpublished && r.has("publisher") {
                out.t(&format!(" {}.", r.f("publisher").trim()));
            } else if r.has("archive") {
                out.t(&format!(" {}.", r.f("archive").trim()));
            }
        }
        "presentation" => {
            let ev = r.f("event").trim();
            let place = r.f("event_place").trim();
            match (ev.is_empty(), place.is_empty()) {
                (false, false) => out.t(&format!(" {ev}, {place}.")),
                (false, true) => out.t(&format!(" {ev}.")),
                (true, false) => out.t(&format!(" {place}.")),
                _ => {}
            }
        }
        "preprint" => {
            let repo = if r.has("publisher") { r.f("publisher").trim().to_string() } else { r.f("container_title").trim().to_string() };
            if !repo.is_empty() {
                out.t(&format!(" {repo}."));
            }
        }
        _ => {
            let p = publisher_text(r);
            if !p.is_empty() {
                out.t(&format!(" {p}."));
            }
        }
    }
    doi_url(r, &mut out);
    if let Some(od) = &r.original_date {
        if let Some(y) = od.year {
            out.t(&format!(" (Original work published {y})"));
        }
    }
    out
}

fn collect_warnings(r: &Reference, w: &mut Vec<Warning>, intext: bool) {
    let push = |w: &mut Vec<Warning>, level: &str, code: &str, msg: String| {
        if !w.iter().any(|x| x.ref_id == r.id && x.code == code) {
            w.push(Warning { ref_id: r.id.clone(), level: level.into(), code: code.into(), message: msg });
        }
    };
    if !supported(&r.rtype) {
        push(w, "unsupported", "unsupported-type", format!("{} output is not covered by the tested APA cases; review it by hand.", crate::types::label(&r.rtype)));
    }
    if r.title().trim().is_empty() && !r.absent("title") {
        push(w, "review", "missing-title", "The title is missing. Add it, or confirm the work has no title so a description is used.".into());
    }
    if r.issued.is_none() && !r.absent("issued") {
        push(w, "info", "no-date", "No date is recorded; “n.d.” is used. Confirm the work has no date, or add it.".into());
    }
    if author_position(r).0.is_empty() && !r.absent("author") && !intext {
        push(w, "info", "no-author", "No author is recorded; the title moves to the author position.".into());
    }
    if r.rtype == "article-journal" {
        if !r.has("container_title") {
            push(w, "review", "missing-journal", "The journal name is missing.".into());
        }
        if !r.has("doi") && !r.absent("doi") && !intext {
            push(w, "info", "doi-unknown", "No DOI is recorded. That alone does not make the reference incomplete; add one if the article has one.".into());
        }
        let p = r.f("page").trim();
        if !r.has("article_number") && !p.is_empty() && !p.contains(['-', '–']) && p.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
            push(w, "review", "article-number-as-page", format!("“{p}” looks like an article number entered as a page range."));
        }
    }
    if text_only(&r.rtype) && !intext {
        push(w, "info", "text-only", "Personal communications are cited in the text only and are left out of the reference list.".into());
    }
    if !r.conflicts.is_empty() {
        push(w, "review", "conflict", "This record has conflicting values from different sources awaiting review.".into());
    }
}

/// Render a reference list for `refs` (sorted), with paragraph properties.
pub fn bibliography(refs: &[&Reference], ctx: &Context) -> Rendered {
    let mut set: Vec<&Reference> = ctx.set.clone();
    for r in refs {
        if !set.iter().any(|s| s.id == r.id) {
            set.push(r);
        }
    }
    let d = disambiguate(&set);
    let mut sorted: Vec<&Reference> = refs.to_vec();
    sort_refs(&mut sorted);
    let mut warnings = Vec::new();
    let mut paragraphs = Vec::new();
    let mut all = Out::default();
    for r in &sorted {
        collect_warnings(r, &mut warnings, false);
        if text_only(&r.rtype) {
            continue;
        }
        let e = entry(r, &d);
        if !all.is_empty() {
            all.t("\n");
        }
        all.extend(&e);
        paragraphs.push(Paragraph { ref_id: r.id.clone(), runs: e.runs, hanging_indent: 0.5, line_spacing: 2.0 });
    }
    let text = all.text();
    Rendered { fingerprint: fingerprint("apa-7", refs, ctx), provisional: ctx.standalone, runs: all.runs, text, paragraphs, warnings }
}

// ---- fixtures and coverage -------------------------------------------------------------------

/// The golden fixture corpus shipped with the program. Each case states its expected output and
/// the reason. Cases marked reviewed have been checked by a qualified person against the manual.
pub const FIXTURES: &str = include_str!("../../tests/fixtures/apa/fixtures.json");

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct Fixture {
    pub id: String,
    pub category: String,
    pub reason: String,
    pub references: Vec<Reference>,
    /// Extra references in the manuscript context (for disambiguation cases).
    #[serde(default)]
    pub context: Vec<Reference>,
    #[serde(default)]
    pub cite: Vec<CiteItem>,
    /// parenthetical, narrative, reference, bibliography, quote
    pub mode: String,
    #[serde(default)]
    pub quote: String,
    pub expected_text: String,
    /// Substrings that must be italic, in order.
    #[serde(default)]
    pub expected_italic: Vec<String>,
    #[serde(default)]
    pub reviewed_by: String,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct FixtureResult {
    pub id: String,
    pub category: String,
    pub mode: String,
    pub passed: bool,
    pub text_ok: bool,
    pub format_ok: bool,
    pub expected: String,
    pub actual: String,
    pub reviewed: bool,
    pub reason: String,
}

pub fn load_fixtures() -> Vec<Fixture> {
    serde_json::from_str(FIXTURES).unwrap_or_default()
}

pub fn run_fixture(f: &Fixture) -> FixtureResult {
    let refs: Vec<&Reference> = f.references.iter().collect();
    let mut set: Vec<&Reference> = f.references.iter().collect();
    set.extend(f.context.iter());
    let ctx = Context { set, locale: "en-US".into(), revision: 0, manuscript_id: "fixture".into(), standalone: false };
    let rendered = match f.mode.as_str() {
        "parenthetical" => cite(&f.cite, false, &refs, &ctx),
        "narrative" => cite(&f.cite, true, &refs, &ctx),
        "quote" => {
            let c = cite(&f.cite, false, &refs, &ctx);
            let (o, _) = quote_with_citation(&f.quote, &c);
            Rendered { text: o.text(), runs: o.runs, ..Default::default() }
        }
        _ => bibliography(&refs, &ctx),
    };
    let text_ok = rendered.text == f.expected_text;
    let italic_runs: Vec<String> = rendered.runs.iter().filter(|r| r.italic).map(|r| r.text.clone()).collect();
    let format_ok = if f.expected_italic.is_empty() { italic_runs.is_empty() || f.mode == "parenthetical" || f.mode == "narrative" } else { italic_runs == f.expected_italic };
    FixtureResult {
        id: f.id.clone(),
        category: f.category.clone(),
        mode: f.mode.clone(),
        passed: text_ok && format_ok,
        text_ok,
        format_ok,
        expected: f.expected_text.clone(),
        actual: rendered.text,
        reviewed: !f.reviewed_by.is_empty(),
        reason: f.reason.clone(),
    }
}

/// Coverage record for the APA profile: which cases ran, passed and were human-reviewed.
/// The profile is only "verified" when every case passed and every case was reviewed.
pub fn coverage() -> serde_json::Value {
    let fx = load_fixtures();
    let results: Vec<FixtureResult> = fx.iter().map(run_fixture).collect();
    let passed = results.iter().filter(|r| r.passed).count();
    let reviewed = results.iter().filter(|r| r.reviewed).count();
    let mut cats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for r in &results {
        let e = cats.entry(r.category.clone()).or_default();
        e.0 += 1;
        if r.passed {
            e.1 += 1;
        }
    }
    let assurance = if !results.is_empty() && passed == results.len() && reviewed == results.len() { "verified" } else { "limited" };
    serde_json::json!({
        "style": "apa-7",
        "style_version": STYLE_VERSION,
        "processor": super::PROCESSOR,
        "processor_version": super::PROCESSOR_VERSION,
        "locale": "en-US",
        "cases": results.len(),
        "passed": passed,
        "reviewed": reviewed,
        "assurance": assurance,
        "categories": cats.iter().map(|(k, (n, p))| serde_json::json!({"category": k, "cases": n, "passed": p})).collect::<Vec<_>>(),
        "results": results,
        "note": if assurance == "verified" { "All approved cases pass for the listed source types." } else { "Cases are written from published APA examples but await review by a qualified person; until then the profile is shown as Limited testing." },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initials_forms() {
        assert_eq!(initials("Lin-Manuel"), "L.-M.");
        assert_eq!(initials("John Paul"), "J. P.");
        assert_eq!(initials("J.P."), "J. P.");
        assert_eq!(initials("Ana"), "A.");
    }
    #[test]
    fn locators() {
        assert_eq!(locator_text("page", "121"), "p. 121");
        assert_eq!(locator_text("page", "121-123"), "pp. 121–123");
        assert_eq!(locator_text("paragraph", "4"), "para. 4");
    }
}
