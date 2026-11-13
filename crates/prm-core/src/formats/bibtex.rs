//! BibTeX and biblatex.

use super::{ParseOutput, Parsed};
use crate::model::{Name, NameKind, PartialDate, Reference};
use std::collections::BTreeMap;

fn bib_type(t: &str, fields: &BTreeMap<String, String>) -> &'static str {
    match t {
        "article" => {
            let j = fields.get("journal").or(fields.get("journaltitle")).map(|s| s.to_lowercase()).unwrap_or_default();
            if j.contains("arxiv") || fields.contains_key("eprint") && !fields.contains_key("journal") && !fields.contains_key("journaltitle") {
                "preprint"
            } else {
                "article-journal"
            }
        }
        "book" | "mvbook" => {
            if !fields.contains_key("author") && fields.contains_key("editor") {
                "edited-book"
            } else {
                "book"
            }
        }
        "inbook" | "incollection" | "bookinbook" => "chapter",
        "inproceedings" | "conference" => "paper-conference",
        "proceedings" | "mvproceedings" => "proceedings",
        "phdthesis" | "mastersthesis" | "thesis" => "thesis",
        "techreport" | "report" => "report",
        "online" | "electronic" | "www" => "webpage",
        "software" => "software",
        "dataset" => "dataset",
        "patent" => "patent",
        "unpublished" => "manuscript",
        "booklet" => "pamphlet",
        "manual" => "report",
        "inreference" => "entry-encyclopedia",
        "misc" => {
            if fields.contains_key("howpublished") && fields.get("howpublished").map(|h| h.contains("http")).unwrap_or(false) {
                "webpage"
            } else {
                "document"
            }
        }
        _ => "document",
    }
}

/// Decode common LaTeX escapes to Unicode.
pub fn delatex(s: &str) -> String {
    let accents: &[(&str, char)] = &[("'", '\u{301}'), ("`", '\u{300}'), ("^", '\u{302}'), ("\"", '\u{308}'), ("~", '\u{303}'), ("=", '\u{304}'), (".", '\u{307}'), ("u", '\u{306}'), ("v", '\u{30C}'), ("H", '\u{30B}'), ("c", '\u{327}'), ("k", '\u{328}'), ("r", '\u{30A}')];
    let specials: &[(&str, &str)] = &[("\\ss", "ß"), ("\\ae", "æ"), ("\\AE", "Æ"), ("\\oe", "œ"), ("\\OE", "Œ"), ("\\o", "ø"), ("\\O", "Ø"), ("\\aa", "å"), ("\\AA", "Å"), ("\\l", "ł"), ("\\L", "Ł"), ("\\i", "ı"), ("\\&", "&"), ("\\%", "%"), ("\\$", "$"), ("\\#", "#"), ("\\_", "_"), ("\\textendash", "–"), ("\\textemdash", "—"), ("\\textquoteright", "’"), ("\\textquoteleft", "‘")];
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            // accent command
            let next = chars[i + 1];
            let mut matched = false;
            for (cmd, comb) in accents {
                let cc: Vec<char> = cmd.chars().collect();
                if next == cc[0] && (cmd.len() == 1 && !next.is_alphabetic() || (next.is_alphabetic() && i + 2 < chars.len() && (chars[i + 2] == '{' || chars[i + 2] == ' '))) {
                    let mut j = i + 2;
                    while j < chars.len() && chars[j] == ' ' {
                        j += 1;
                    }
                    let (base, end) = if j < chars.len() && chars[j] == '{' {
                        let mut k = j + 1;
                        let mut b = String::new();
                        while k < chars.len() && chars[k] != '}' {
                            b.push(chars[k]);
                            k += 1;
                        }
                        (b, k + 1)
                    } else if j < chars.len() {
                        (chars[j].to_string(), j + 1)
                    } else {
                        (String::new(), j)
                    };
                    let base = if base == "\\i" { "i".to_string() } else { base };
                    use unicode_normalization::UnicodeNormalization;
                    let composed: String = format!("{base}{comb}").nfc().collect();
                    out.push_str(&composed);
                    i = end;
                    matched = true;
                    break;
                }
            }
            if matched {
                continue;
            }
            let rest: String = chars[i..].iter().take(16).collect();
            if let Some((cmd, rep)) = specials.iter().filter(|(cmd, _)| rest.starts_with(cmd) && !rest[cmd.len()..].chars().next().map(|c| c.is_alphabetic()).unwrap_or(false)).max_by_key(|(c, _)| c.len()) {
                out.push_str(rep);
                i += cmd.chars().count();
                continue;
            }
            // \emph{x}, \textit{x}, \textbf{x}: keep content
            out.push(c);
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    let out = out.replace("---", "—").replace("--", "–").replace("``", "“").replace("''", "”");
    let re = re!(r"\\(emph|textit|textbf|textsc|textrm|mathrm|url|href)\s*");
    let out = re.replace_all(&out, "").into_owned();
    out.replace(['{', '}'], "").replace('~', "\u{a0}").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split "A and B and {C and D}" at top-level " and ".
fn split_and(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    let words: Vec<&str> = s.split(' ').collect();
    for w in words {
        let opens = w.matches('{').count() as i32;
        let closes = w.matches('}').count() as i32;
        if depth == 0 && w.eq_ignore_ascii_case("and") {
            out.push(cur.trim().to_string());
            cur.clear();
            continue;
        }
        depth += opens - closes;
        cur.push_str(w);
        cur.push(' ');
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

fn bib_name(raw: &str) -> Name {
    let t = raw.trim();
    if t.starts_with('{') && t.ends_with('}') && t.matches('{').count() == 1 {
        return Name::org(&delatex(t));
    }
    let mut n = Name::parse(&delatex(t));
    if n.given.is_empty() && n.particle.is_empty() && !t.contains(',') && !t.contains(' ') {
        n.kind = NameKind::Person;
    }
    n
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() {
            self.i += 1;
        }
    }
    fn ident(&mut self) -> String {
        self.ws();
        let st = self.i;
        while self.i < self.s.len() {
            let c = self.s[self.i] as char;
            if c.is_alphanumeric() || "_-:.+/'".contains(c) || self.s[self.i] >= 128 {
                self.i += 1;
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.s[st..self.i]).to_string()
    }
    fn braced(&mut self, open: u8, close: u8) -> String {
        // assumes current char is `open`
        let mut depth = 0;
        let st = self.i + 1;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c == b'\\' {
                self.i += 2;
                continue;
            }
            if c == open && (open != close || depth == 0) {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    let v = String::from_utf8_lossy(&self.s[st..self.i]).to_string();
                    self.i += 1;
                    return v;
                }
            } else if open == b'"' && c == b'{' {
                depth += 1;
            } else if open == b'"' && c == b'}' {
                depth -= 1;
            }
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[st.min(self.s.len())..]).to_string()
    }
    fn value(&mut self, strings: &BTreeMap<String, String>) -> String {
        let mut parts = Vec::new();
        loop {
            self.ws();
            if self.i >= self.s.len() {
                break;
            }
            let c = self.s[self.i];
            if c == b'{' {
                parts.push(self.braced(b'{', b'}'));
            } else if c == b'"' {
                // quoted string; braces inside allowed
                let st = self.i + 1;
                let mut depth = 0;
                self.i += 1;
                while self.i < self.s.len() {
                    let ch = self.s[self.i];
                    if ch == b'\\' {
                        self.i += 2;
                        continue;
                    }
                    if ch == b'{' {
                        depth += 1;
                    } else if ch == b'}' {
                        depth -= 1;
                    } else if ch == b'"' && depth == 0 {
                        break;
                    }
                    self.i += 1;
                }
                parts.push(String::from_utf8_lossy(&self.s[st..self.i.min(self.s.len())]).to_string());
                self.i += 1;
            } else {
                let id = self.ident();
                if id.is_empty() {
                    break;
                }
                parts.push(strings.get(&id.to_lowercase()).cloned().unwrap_or(id));
            }
            self.ws();
            if self.i < self.s.len() && self.s[self.i] == b'#' {
                self.i += 1;
                continue;
            }
            break;
        }
        parts.join("")
    }
}

pub fn parse(text: &str) -> ParseOutput {
    let mut out = ParseOutput::default();
    let mut strings: BTreeMap<String, String> = BTreeMap::new();
    for (m, full) in [("jan", "January"), ("feb", "February"), ("mar", "March"), ("apr", "April"), ("may", "May"), ("jun", "June"), ("jul", "July"), ("aug", "August"), ("sep", "September"), ("oct", "October"), ("nov", "November"), ("dec", "December")] {
        strings.insert(m.into(), full.into());
    }
    let bytes = text.as_bytes();
    let mut lx = Lexer { s: bytes, i: 0 };
    while lx.i < bytes.len() {
        let Some(at) = bytes[lx.i..].iter().position(|b| *b == b'@') else { break };
        lx.i += at + 1;
        let entry_start = lx.i - 1;
        let kind = lx.ident().to_lowercase();
        lx.ws();
        if lx.i >= bytes.len() {
            break;
        }
        let open = bytes[lx.i];
        if open != b'{' && open != b'(' {
            continue;
        }
        let close = if open == b'{' { b'}' } else { b')' };
        if kind == "comment" || kind == "preamble" {
            lx.braced(open, close);
            continue;
        }
        lx.i += 1;
        if kind == "string" {
            let name = lx.ident().to_lowercase();
            lx.ws();
            if lx.i < bytes.len() && bytes[lx.i] == b'=' {
                lx.i += 1;
                let v = lx.value(&strings);
                strings.insert(name, v);
            }
            while lx.i < bytes.len() && bytes[lx.i] != close {
                lx.i += 1;
            }
            lx.i += 1;
            continue;
        }
        let key = lx.ident();
        lx.ws();
        if lx.i < bytes.len() && bytes[lx.i] == b',' {
            lx.i += 1;
        }
        let mut fields: BTreeMap<String, String> = BTreeMap::new();
        loop {
            lx.ws();
            if lx.i >= bytes.len() {
                break;
            }
            if bytes[lx.i] == close {
                lx.i += 1;
                break;
            }
            let name = lx.ident().to_lowercase();
            lx.ws();
            if name.is_empty() || lx.i >= bytes.len() || bytes[lx.i] != b'=' {
                // malformed: skip to next comma or close
                while lx.i < bytes.len() && bytes[lx.i] != b',' && bytes[lx.i] != close {
                    lx.i += 1;
                }
                if lx.i < bytes.len() && bytes[lx.i] == b',' {
                    lx.i += 1;
                    continue;
                }
                continue;
            }
            lx.i += 1;
            let v = lx.value(&strings);
            fields.insert(name, v);
            lx.ws();
            if lx.i < bytes.len() && bytes[lx.i] == b',' {
                lx.i += 1;
            }
        }
        let raw = String::from_utf8_lossy(&bytes[entry_start..lx.i.min(bytes.len())]).to_string();
        out.records.push(Parsed { raw, record: to_record(&kind, &key, &fields), issues: vec![], confidence: 1.0 });
    }
    out
}

fn to_record(kind: &str, key: &str, f: &BTreeMap<String, String>) -> Reference {
    let mut r = Reference::default();
    r.rtype = bib_type(kind, f).to_string();
    r.citekey = key.to_string();
    r.origin = "import".into();
    r.overflow.insert("bibtex:entrytype".into(), kind.into());
    let used = std::cell::RefCell::new(std::collections::BTreeSet::<String>::new());
    let get = |k: &str| -> Option<String> {
        used.borrow_mut().insert(k.to_string());
        f.get(k).map(|v| delatex(v))
    };
    for (role, field) in [("author", "author"), ("editor", "editor"), ("translator", "translator")] {
        used.borrow_mut().insert(field.to_string());
        if let Some(v) = f.get(field) {
            for n in split_and(v) {
                if n == "others" {
                    continue;
                }
                r.creators.push(crate::model::Creator { role: role.into(), name: bib_name(&n) });
            }
        }
    }
    let mut set = |r: &mut Reference, k: &str, v: Option<String>| {
        if let Some(v) = v {
            if !v.trim().is_empty() {
                r.fields.insert(k.into(), v);
            }
        }
    };
    set(&mut r, "title", get("title"));
    let book_like = matches!(r.rtype.as_str(), "chapter" | "paper-conference" | "entry-encyclopedia");
    let container = if book_like { get("booktitle") } else { get("journaltitle").or_else(|| get("journal")) };
    set(&mut r, "container_title", container);
    if !book_like {
        used.borrow_mut().insert("booktitle".into());
        if let Some(b) = f.get("booktitle") {
            r.overflow.insert("bibtex:booktitle".into(), delatex(b));
        }
    }
    set(&mut r, "volume", get("volume"));
    let number = get("number").or_else(|| get("issue"));
    if matches!(r.rtype.as_str(), "article-journal" | "article-magazine") {
        set(&mut r, "issue", number);
    } else {
        set(&mut r, "number", number);
    }
    set(&mut r, "page", get("pages"));
    set(&mut r, "article_number", get("eid"));
    set(&mut r, "doi", get("doi"));
    set(&mut r, "url", get("url"));
    let publisher = get("publisher").or_else(|| get("school")).or_else(|| get("institution")).or_else(|| get("organization"));
    set(&mut r, "publisher", publisher);
    set(&mut r, "publisher_place", get("address").or_else(|| get("location")));
    set(&mut r, "edition", get("edition"));
    set(&mut r, "isbn", get("isbn"));
    set(&mut r, "issn", get("issn"));
    set(&mut r, "abstract", get("abstract"));
    set(&mut r, "note", get("note").or_else(|| get("annote")));
    set(&mut r, "language", get("langid").or_else(|| get("language")));
    set(&mut r, "collection_title", get("series"));
    set(&mut r, "version", get("version"));
    set(&mut r, "short_title", get("shorttitle"));
    let ty = get("type");
    if r.rtype == "thesis" {
        let g = ty.unwrap_or_else(|| if kind == "mastersthesis" { "Master's thesis".into() } else { "Doctoral dissertation".into() });
        r.fields.insert("genre".into(), g);
    } else {
        set(&mut r, "genre", ty);
    }
    if let Some(e) = get("eprint") {
        let prefix = get("archiveprefix").or_else(|| get("eprinttype")).unwrap_or_default().to_lowercase();
        if prefix.contains("arxiv") || r.rtype == "preprint" {
            r.fields.insert("arxiv".into(), e);
            if !r.fields.contains_key("publisher") {
                r.fields.insert("publisher".into(), "arXiv".into());
            }
        } else {
            r.overflow.insert("bibtex:eprint".into(), e);
        }
    }
    if let Some(h) = get("howpublished") {
        if h.starts_with("http") && !r.fields.contains_key("url") {
            r.fields.insert("url".into(), h.trim_start_matches("\\url").to_string());
        } else {
            r.overflow.insert("bibtex:howpublished".into(), h);
        }
    }
    if let Some(kw) = get("keywords") {
        for k in kw.split([',', ';']) {
            let k = k.trim();
            if !k.is_empty() {
                r.tags.push(k.to_string());
            }
        }
    }
    // Dates: biblatex date, else year/month/day.
    if let Some(d) = get("date") {
        r.issued = PartialDate::parse(&d);
    } else if let Some(y) = get("year") {
        let mut d = PartialDate::parse(&y).unwrap_or_default();
        if let Some(m) = get("month") {
            let ml = m.to_lowercase();
            let names = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
            d.month = m.trim().parse::<u8>().ok().or_else(|| names.iter().position(|n| ml.starts_with(n)).map(|i| i as u8 + 1));
        }
        if let Some(day) = get("day") {
            d.day = day.trim().parse().ok();
        }
        r.issued = Some(d);
    }
    if let Some(u) = get("urldate") {
        r.accessed = PartialDate::parse(&u);
    }
    if let Some(o) = get("origdate") {
        r.original_date = PartialDate::parse(&o);
    }
    for k in ["archiveprefix", "eprinttype", "month", "day", "year", "school", "institution", "organization", "journal", "journaltitle", "issue", "number", "address", "location", "langid", "language", "annote", "note"] {
        used.borrow_mut().insert(k.to_string());
    }
    for (k, v) in f {
        if !used.borrow().contains(k) {
            r.overflow.insert(format!("bibtex:{k}"), delatex(v));
        }
    }
    r
}

// ---- export ------------------------------------------------------------------------------------

fn escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '&' | '%' | '$' | '#' | '_' => {
                o.push('\\');
                o.push(c);
            }
            '{' | '}' => {
                o.push('\\');
                o.push(c);
            }
            '–' => o.push_str("--"),
            '—' => o.push_str("---"),
            c => o.push(c),
        }
    }
    o
}

/// Protect words whose capitalization must survive BibTeX case changes.
fn protect_title(s: &str) -> String {
    s.split(' ')
        .enumerate()
        .map(|(i, w)| {
            let inner_caps = w.chars().skip(1).any(|c| c.is_uppercase());
            let first_cap_later = i > 0 && w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
            let e = escape(w);
            if inner_caps || first_cap_later { format!("{{{e}}}") } else { e }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn name_bib(n: &Name) -> String {
    match n.kind {
        NameKind::Person => {
            let mut s = String::new();
            if !n.particle.is_empty() {
                s.push_str(&n.particle);
                s.push(' ');
            }
            s.push_str(&escape(&n.family));
            if !n.suffix.is_empty() {
                s.push_str(&format!(", {}", escape(&n.suffix)));
            }
            if !n.given.is_empty() {
                s.push_str(&format!(", {}", escape(&n.given)));
            }
            s
        }
        _ => format!("{{{}}}", escape(&n.literal)),
    }
}

/// Export records. `biblatex` selects biblatex field names and entry types. `exclude` lists
/// fields to leave out.
pub fn export(refs: &[&Reference], biblatex: bool, exclude: &[String]) -> String {
    let mut o = String::new();
    for r in refs {
        let ty = match (r.rtype.as_str(), biblatex) {
            ("thesis", false) => {
                if r.f("genre").to_lowercase().contains("master") { "mastersthesis" } else { "phdthesis" }
            }
            ("thesis", true) => "thesis",
            ("report", true) => "report",
            ("webpage" | "post-weblog" | "post", true) => "online",
            ("dataset", true) => "dataset",
            ("software", true) => "software",
            ("edited-book", _) => "book",
            (t, _) => crate::types::get(t).map(|x| x.bibtex).unwrap_or("misc"),
        };
        let key = if r.citekey.is_empty() { r.id.clone() } else { r.citekey.clone() };
        o.push_str(&format!("@{ty}{{{key},\n"));
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut push = |k: &str, v: String| {
            if !v.trim().is_empty() && !exclude.iter().any(|e| e == k) {
                fields.push((k.to_string(), v));
            }
        };
        for role in ["author", "editor", "translator"] {
            let names: Vec<String> = r.creators.iter().filter(|c| c.role == role).map(|c| name_bib(&c.name)).collect();
            if !names.is_empty() && (role != "translator" || biblatex) {
                push(role, names.join(" and "));
            }
        }
        push("title", protect_title(r.title()));
        let container = escape(r.f("container_title"));
        match r.rtype.as_str() {
            "chapter" | "paper-conference" | "entry-encyclopedia" => push("booktitle", container),
            _ if biblatex => push("journaltitle", container),
            _ => push("journal", container),
        }
        if let Some(d) = &r.issued {
            if biblatex {
                push("date", if d.year.is_some() { d.iso() } else { d.status.clone() });
            } else if let Some(y) = d.year {
                push("year", y.to_string());
                if let Some(m) = d.month {
                    let names = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
                    if (1..=12).contains(&m) {
                        push("month", names[m as usize - 1].to_string());
                    }
                }
            } else if !d.status.is_empty() {
                push("year", d.status.clone());
            }
        }
        push("volume", escape(r.f("volume")));
        if matches!(r.rtype.as_str(), "article-journal" | "article-magazine") {
            push("number", escape(r.f("issue")));
        } else {
            push("number", escape(r.f("number")));
        }
        push("pages", r.f("page").replace('–', "--").replace('-', "--").replace("----", "--"));
        push(if biblatex { "eid" } else { "eid" }, escape(r.f("article_number")));
        let publisher = escape(r.f("publisher"));
        match (r.rtype.as_str(), biblatex) {
            ("thesis", false) => push("school", publisher),
            ("report", false) => push("institution", publisher),
            ("thesis" | "report", true) => push("institution", publisher),
            _ => push("publisher", publisher),
        }
        push(if biblatex { "location" } else { "address" }, escape(r.f("publisher_place")));
        push("edition", escape(r.f("edition")));
        push("series", escape(r.f("collection_title")));
        push("isbn", r.f("isbn").to_string());
        push("issn", r.f("issn").to_string());
        push("doi", r.f("doi").to_string());
        push("url", r.f("url").to_string());
        if biblatex {
            if let Some(a) = &r.accessed {
                push("urldate", a.iso());
            }
            push("langid", escape(r.f("language")));
            push("version", escape(r.f("version")));
        }
        push("type", escape(r.f("genre")));
        push("abstract", escape(r.f("abstract")));
        push("note", escape(r.f("note")));
        if !r.tags.is_empty() {
            push("keywords", escape(&r.tags.join(", ")));
        }
        if r.has("arxiv") {
            push("eprint", r.f("arxiv").to_string());
            push(if biblatex { "eprinttype" } else { "archiveprefix" }, "arXiv".into());
        }
        for (k, v) in &r.overflow {
            if let Some(name) = k.strip_prefix("bibtex:") {
                if name != "entrytype" && !fields.iter().any(|(f, _)| f == name) {
                    fields.push((name.to_string(), escape(v)));
                }
            }
        }
        let n = fields.len();
        for (i, (k, v)) in fields.into_iter().enumerate() {
            o.push_str(&format!("  {k} = {{{v}}}{}\n", if i + 1 < n { "," } else { "" }));
        }
        o.push_str("}\n\n");
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_bibtex() {
        let t = r#"@string{tp = "Journal of Teaching Practice"}
@article{chen2024,
  author = {Chen, Li and Patel, Maya and {Office of Learning Research}},
  title = {Teaching with {Generative AI}},
  journal = tp,
  year = 2024, month = mar,
  volume = {12}, number = {2}, pages = {115--132},
  doi = {10.5555/x}, custom = {kept}
}"#;
        let p = parse(t);
        assert_eq!(p.records.len(), 1);
        let r = &p.records[0].record;
        assert_eq!(r.creators.len(), 3);
        assert_eq!(r.creators[2].name.kind, NameKind::Organization);
        assert_eq!(r.f("container_title"), "Journal of Teaching Practice");
        assert_eq!(r.f("page"), "115–132");
        assert_eq!(r.f("issue"), "2");
        assert_eq!(r.issued.as_ref().unwrap().month, Some(3));
        assert_eq!(r.overflow.get("bibtex:custom").map(|s| s.as_str()), Some("kept"));
        assert_eq!(delatex("M\\\"{u}ller \\& G\\'omez"), "Müller & Gómez");
    }
}
