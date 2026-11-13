//! Keeping records tidy: rules applied whenever a source is saved, and batch fixes that are
//! proposed for review before anything changes.

use crate::error::Result;
use crate::library::Library;
use crate::model::{Provenance, Reference};
use crate::util::now;
use serde::{Deserialize, Serialize};

/// Rules applied to every saved source. The person chooses them in Settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SaveRules {
    /// Collapse repeated spaces and remove spaces at the ends of every field.
    pub tidy_spaces: bool,
    /// Write DOIs, ISBNs and ISSNs in one standard form.
    pub standard_identifiers: bool,
    /// Remove a final period from titles (styles add their own punctuation).
    pub title_final_period: bool,
    /// Change titles typed entirely in capitals to sentence case.
    pub capital_titles: bool,
    /// Change author names typed entirely in capitals (SMITH, JOHN) to normal capitals.
    pub capital_names: bool,
}

impl Default for SaveRules {
    fn default() -> Self {
        SaveRules { tidy_spaces: true, standard_identifiers: true, title_final_period: true, capital_titles: false, capital_names: false }
    }
}

fn is_all_caps(s: &str) -> bool {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 4 && letters.iter().all(|c| c.is_uppercase())
}

fn capitalize_word(w: &str) -> String {
    let lower = w.to_lowercase();
    let mut out = String::new();
    let mut up = true;
    for c in lower.chars() {
        if up && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
            if c == '-' || c == '\'' || c == '’' {
                up = c == '-';
            }
        }
    }
    out
}

/// Sentence case for reference-list titles: the first word, the first word after a colon,
/// question mark or exclamation mark, protected terms, and words with inner capitals or that
/// look like acronyms keep their capitals; everything else is lower case. When the whole title
/// is in capitals, only protected terms keep them.
pub fn sentence_case(title: &str, protected: &[String]) -> String {
    let shouting = is_all_caps(title);
    let mut out = String::with_capacity(title.len());
    let mut start = true;
    for (i, word) in title.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if word.is_empty() {
            continue;
        }
        let bare: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
        let protected_hit = protected.iter().find(|p| p.to_lowercase() == bare.to_lowercase());
        // Capitals inside a word part (iPhone, McDonald) are deliberate; a capital after a hyphen
        // (Self-Report) is title case and becomes lower case.
        let inner_caps = !shouting && word.split('-').any(|part| part.chars().skip(1).any(|c| c.is_uppercase()));
        let acronym = !shouting && bare.chars().count() >= 2 && bare.chars().all(|c| c.is_uppercase() || c.is_ascii_digit());
        let new = if let Some(p) = protected_hit {
            word.replace(&bare, p)
        } else if inner_caps || acronym {
            word.to_string()
        } else if start {
            let lower = word.to_lowercase();
            let mut cs = lower.chars();
            let mut first = String::new();
            for c in cs.by_ref() {
                if c.is_alphabetic() {
                    first.extend(c.to_uppercase());
                    break;
                }
                first.push(c);
            }
            first + cs.as_str()
        } else {
            word.to_lowercase()
        };
        out.push_str(&new);
        if bare.chars().any(|c| c.is_alphanumeric()) {
            start = word.ends_with(':') || word.ends_with('?') || word.ends_with('!') || word.ends_with('.') && bare.chars().count() > 1;
        }
    }
    out
}

fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn standard_isbn(s: &str) -> String {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x').collect::<String>().to_uppercase();
    if digits.len() == 10 || digits.len() == 13 { digits } else { s.to_string() }
}

fn standard_issn(s: &str) -> String {
    let d: String = s.chars().filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x').collect::<String>().to_uppercase();
    if d.len() == 8 { format!("{}-{}", &d[..4], &d[4..]) } else { s.to_string() }
}

/// Apply the save rules. Returns a short description of each change, for field history.
pub fn apply_rules(r: &mut Reference, rules: &SaveRules, protected: &[String]) -> Vec<String> {
    let mut changes = vec![];
    let keys: Vec<String> = r.fields.keys().cloned().collect();
    for k in keys {
        let v = r.fields.get(&k).cloned().unwrap_or_default();
        let mut n = v.clone();
        if rules.tidy_spaces && k != "abstract" && k != "note" {
            n = tidy(&n);
        } else if rules.tidy_spaces {
            n = n.trim().to_string();
        }
        if rules.standard_identifiers {
            match k.as_str() {
                "doi" => n = crate::util::normalize_doi(&n).unwrap_or(n),
                "isbn" => n = n.split([';', ',']).map(|x| standard_isbn(x.trim())).collect::<Vec<_>>().join("; "),
                "issn" => n = n.split([';', ',']).map(|x| standard_issn(x.trim())).collect::<Vec<_>>().join("; "),
                _ => {}
            }
        }
        if (k == "title" || k == "short_title") && !n.is_empty() {
            if rules.capital_titles && is_all_caps(&n) {
                n = sentence_case(&n, protected);
                changes.push(format!("{k}: capitals changed to sentence case"));
            }
            if rules.title_final_period && n.ends_with('.') && !n.ends_with("..") && !n.ends_with("etc.") {
                n.pop();
            }
        }
        if n != v {
            if !changes.iter().any(|c| c.starts_with(&format!("{k}:"))) {
                changes.push(format!("{k}: tidied"));
            }
            if n.is_empty() {
                r.fields.remove(&k);
            } else {
                r.fields.insert(k, n);
            }
        }
    }
    if rules.capital_names {
        for c in r.creators.iter_mut() {
            for part in [&mut c.name.family, &mut c.name.given] {
                if is_all_caps(part) || (part.chars().count() >= 2 && part.chars().all(|ch| !ch.is_alphabetic() || ch.is_uppercase()) && part.chars().filter(|ch| ch.is_alphabetic()).count() >= 3) {
                    let fixed = part.split(' ').map(capitalize_word).collect::<Vec<_>>().join(" ");
                    if fixed != *part {
                        *part = fixed;
                        if !changes.iter().any(|x| x.starts_with("creators:")) {
                            changes.push("creators: capitals changed".into());
                        }
                    }
                }
            }
        }
    }
    changes
}

/// A change proposed by a batch fix, applied only if the person accepts it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Proposal {
    pub ref_id: String,
    pub title: String,
    pub field: String,
    pub before: String,
    pub after: String,
    /// Where the new value came from (for provenance), for example "lookup:crossref".
    pub source: String,
    /// Anything the person should check before accepting.
    pub note: String,
}

impl Library {
    /// Sentence-case proposals for the given sources' titles (no network).
    /// Proposed journal abbreviations (or full names) from the journal list, for sources that
    /// have one form but not the other.
    pub fn propose_journal_names(&self, ids: &[String]) -> Result<Vec<Proposal>> {
        let mut out = vec![];
        for id in ids {
            let Some(r) = self.store.get::<Reference>(id)? else { continue };
            if r.has("container_title") && !r.has("container_title_short") {
                if let Some(a) = crate::journals::abbreviation(&self.store.conn, r.f("container_title")) {
                    out.push(Proposal { ref_id: r.id.clone(), title: r.title().into(), field: "container_title_short".into(), before: String::new(), after: a, source: "journal-list".into(), note: String::new() });
                }
            } else if r.has("container_title_short") && !r.has("container_title") {
                if let Some(f) = crate::journals::full_name(&self.store.conn, r.f("container_title_short")) {
                    out.push(Proposal { ref_id: r.id.clone(), title: r.title().into(), field: "container_title".into(), before: String::new(), after: f, source: "journal-list".into(), note: String::new() });
                }
            }
        }
        Ok(out)
    }

    pub fn propose_title_case(&self, ids: &[String]) -> Result<Vec<Proposal>> {
        let mut out = vec![];
        for id in ids {
            let Some(r) = self.store.get::<Reference>(id)? else { continue };
            let t = r.title().to_string();
            if t.is_empty() {
                continue;
            }
            let typed = r.provenance.get("title").map(|p| p.user_edited).unwrap_or(false);
            let s = sentence_case(&t, &self.protected_terms);
            if s != t {
                // Words lowered that began with a capital may be names: say so.
                let lowered: Vec<&str> = t.split(' ').zip(s.split(' ')).filter(|(a, b)| a != b && a.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)).map(|(a, _)| a).skip(1).collect();
                let mut note = if lowered.is_empty() { String::new() } else { format!("Check names and proper nouns: {}. Add words that must keep capitals to Protected terms.", lowered.join(", ")) };
                if typed {
                    note = format!("{}You entered this title yourself.", if note.is_empty() { String::new() } else { format!("{note} ") });
                }
                out.push(Proposal { ref_id: r.id.clone(), title: t.clone(), field: "title".into(), before: t, after: s, source: "cleanup:sentence-case".into(), note });
            }
        }
        Ok(out)
    }

    /// Apply accepted proposals as one undoable step, with provenance for each changed field.
    pub fn apply_proposals(&mut self, proposals: &[Proposal], label: &str) -> Result<usize> {
        let t = now();
        let mut n = 0;
        let ps = proposals.to_vec();
        self.write(label, |tx| {
            for p in &ps {
                let Some(mut r) = tx.get::<Reference>(&p.ref_id)? else { continue };
                if p.after.trim().is_empty() {
                    continue;
                }
                r.fields.insert(p.field.clone(), p.after.trim().to_string());
                r.provenance.insert(p.field.clone(), Provenance { source: p.source.clone(), time: t.clone(), user_edited: false, confidence: None, review: "reviewed".into() });
                r.revision += 1;
                r.modified = t.clone();
                tx.put(&r)?;
                n += 1;
            }
            Ok(())
        })?;
        Ok(n)
    }

    /// Set today's access date on sources whose links were confirmed working.
    pub fn set_accessed_today(&mut self, ids: &[String]) -> Result<usize> {
        let today = crate::model::PartialDate::parse(&now()[..10]);
        let mut n = 0;
        let ids = ids.to_vec();
        self.write("Update access dates", |tx| {
            for id in &ids {
                let Some(mut r) = tx.get::<Reference>(id)? else { continue };
                r.accessed = today.clone();
                r.revision += 1;
                r.modified = now();
                tx.put(&r)?;
                n += 1;
            }
            Ok(())
        })?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentence_case_keeps_acronyms_names_and_subtitles() {
        assert_eq!(sentence_case("Teaching With Generative AI: A Guide For The Classroom", &[]), "Teaching with generative AI: A guide for the classroom");
        assert_eq!(sentence_case("TEACHING WITH AI IN CLASS", &["AI".into()]), "Teaching with AI in class");
        assert_eq!(sentence_case("Learning From iPhone Users in Europe", &["Europe".into()]), "Learning from iPhone users in Europe");
        assert_eq!(sentence_case("Self-Report Measures: What Works?", &[]), "Self-report measures: What works?");
    }

    #[test]
    fn save_rules_tidy_and_standardize() {
        let mut r = Reference::default();
        r.fields.insert("title".into(), "  Reflection   in practice. ".into());
        r.fields.insert("doi".into(), "https://doi.org/10.5555/ABC".into());
        r.fields.insert("isbn".into(), "978-0-306-40615-7".into());
        r.fields.insert("issn".into(), "12345678".into());
        let ch = apply_rules(&mut r, &SaveRules::default(), &[]);
        assert_eq!(r.f("title"), "Reflection in practice");
        assert_eq!(r.f("doi"), "10.5555/abc");
        assert_eq!(r.f("isbn"), "9780306406157");
        assert_eq!(r.f("issn"), "1234-5678");
        assert!(!ch.is_empty());
        let mut s = Reference::default();
        s.fields.insert("title".into(), "A STUDY OF READING".into());
        apply_rules(&mut s, &SaveRules { capital_titles: true, ..Default::default() }, &[]);
        assert_eq!(s.f("title"), "A study of reading");
    }
}
