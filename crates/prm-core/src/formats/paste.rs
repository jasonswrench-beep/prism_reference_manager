//! Pasted bibliographies and identifiers. Parsing arbitrary reference lists is heuristic: every
//! candidate keeps its original text, carries a confidence, and goes to review. Lines that
//! contain only identifiers become identifier lookups instead.

use super::{person, ParseOutput, Parsed};
use crate::model::{PartialDate, Reference};

pub fn identifiers(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let doi = regex::Regex::new(r"(?i)\b(10\.\d{4,9}/[^\s\]\)>,;]+)").unwrap();
    let arxiv = regex::Regex::new(r"(?i)\barxiv:\s*(\d{4}\.\d{4,5}(v\d+)?)").unwrap();
    let pmid = regex::Regex::new(r"(?i)\bPMID:?\s*(\d{5,9})").unwrap();
    let pmc = regex::Regex::new(r"(?i)\b(PMC\d{5,9})\b").unwrap();
    let isbn = regex::Regex::new(r"(?i)\bISBN(?:-1[03])?:?\s*([0-9X][0-9X\- ]{8,16}[0-9X])").unwrap();
    for c in doi.captures_iter(text) {
        out.push(("doi".into(), c[1].trim_end_matches('.').to_string()));
    }
    for c in arxiv.captures_iter(text) {
        out.push(("arxiv".into(), c[1].to_string()));
    }
    for c in pmid.captures_iter(text) {
        out.push(("pmid".into(), c[1].to_string()));
    }
    for c in pmc.captures_iter(text) {
        out.push(("pmcid".into(), c[1].to_uppercase()));
    }
    for c in isbn.captures_iter(text) {
        out.push(("isbn".into(), c[1].replace([' ', '-'], "")));
    }
    out
}

/// Split pasted text into entries: blank-line separated, or one per line when each line looks
/// like a complete reference.
fn entries(text: &str) -> Vec<String> {
    let blocks: Vec<String> = text.split("\n\n").map(|b| b.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|b| !b.is_empty()).collect();
    if blocks.len() > 1 {
        return blocks;
    }
    text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

pub fn parse(text: &str) -> ParseOutput {
    let mut out = ParseOutput::default();
    // Author-date pattern: "Authors (Year). Title. Source, vol(issue), pages. doi"
    let ad = regex::Regex::new(r"^(?P<authors>.+?)\s*\((?P<year>\d{4}[a-z]?|n\.d\.|in press)(?:,[^)]*)?\)\.\s*(?P<title>[^.?!]+[.?!])\s*(?P<rest>.*)$").unwrap();
    let src = regex::Regex::new(r"^(?P<journal>[^,]+),\s*(?P<vol>\d+)(?:\((?P<iss>[^)]+)\))?(?:,\s*(?P<pages>[\dA-Za-z]+(?:[-–][\dA-Za-z]+)?))?").unwrap();
    for e in entries(text) {
        let ids = identifiers(&e);
        let mut r = Reference::default();
        r.origin = "import".into();
        let mut issues = Vec::new();
        let mut confidence = 0.3;
        if let Some(c) = ad.captures(&e) {
            confidence = 0.6;
            let authors = c.name("authors").map(|m| m.as_str()).unwrap_or("");
            // "Chen, L., & Patel, M." -> split on "., " boundaries and "&"
            let cleaned = authors.replace(" & ", ", ").replace(", &", ",");
            let parts: Vec<&str> = cleaned.split(", ").collect();
            let mut i = 0;
            while i < parts.len() {
                let fam = parts[i].trim();
                let given = parts.get(i + 1).map(|s| s.trim()).unwrap_or("");
                if given.chars().all(|c| c.is_uppercase() || c == '.' || c == ' ' || c == '-') && !given.is_empty() {
                    r.creators.push(person("author", &format!("{fam}, {given}")));
                    i += 2;
                } else {
                    r.creators.push(crate::model::Creator { role: "author".into(), name: crate::model::Name::org(fam.trim_end_matches('.')) });
                    i += 1;
                }
            }
            let y = c.name("year").map(|m| m.as_str()).unwrap_or("");
            r.issued = PartialDate::parse(y.trim_end_matches(|c: char| c.is_ascii_lowercase() && y.len() == 5));
            r.fields.insert("title".into(), c.name("title").map(|m| m.as_str().trim_end_matches('.').to_string()).unwrap_or_default());
            let rest = c.name("rest").map(|m| m.as_str()).unwrap_or("");
            if let Some(s) = src.captures(rest) {
                confidence = 0.75;
                r.rtype = "article-journal".into();
                r.fields.insert("container_title".into(), s["journal"].trim().to_string());
                r.fields.insert("volume".into(), s["vol"].to_string());
                if let Some(iss) = s.name("iss") {
                    r.fields.insert("issue".into(), iss.as_str().to_string());
                }
                if let Some(p) = s.name("pages") {
                    r.fields.insert("page".into(), p.as_str().to_string());
                }
            } else {
                r.rtype = "book".into();
                let publisher = rest.split(". ").next().unwrap_or("").trim().trim_end_matches('.');
                if !publisher.is_empty() && !publisher.starts_with("http") {
                    r.fields.insert("publisher".into(), publisher.to_string());
                }
                issues.push("Type guessed as book; check it.".into());
            }
        } else {
            r.rtype = "document".into();
            r.fields.insert("title".into(), e.chars().take(300).collect());
            issues.push("This text could not be read as a reference. Review it, or look it up by identifier.".into());
        }
        for (k, v) in &ids {
            r.fields.entry(k.clone()).or_insert_with(|| v.clone());
            confidence = f32::max(confidence, 0.7);
        }
        if confidence < 0.7 {
            issues.push(format!("Low confidence ({:.0}%): review every field.", confidence * 100.0));
        }
        out.records.push(Parsed { raw: e, record: r, issues, confidence });
    }
    out
}
