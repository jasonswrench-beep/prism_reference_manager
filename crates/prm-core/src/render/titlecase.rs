//! Title case for in-text titles (used when a work has no author). Words of four letters or
//! more and major words are capitalized; short conjunctions, articles and prepositions stay
//! lower case except at the start or after a colon. Words with internal capitals (acronyms,
//! product names, protected terms) are left exactly as written.

const MINOR: &[&str] = &["a", "an", "the", "and", "as", "but", "for", "if", "nor", "or", "so", "yet", "at", "by", "in", "of", "off", "on", "per", "to", "up", "via"];

pub fn title_case(s: &str, protected: &[String]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for (i, word) in s.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if word.is_empty() {
            continue;
        }
        let bare: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
        let is_protected = protected.iter().any(|p| p == word || p == &bare);
        let has_inner_caps = word.chars().skip(1).any(|c| c.is_uppercase());
        let letters = bare.chars().count();
        let lower = bare.to_lowercase();
        let keep = is_protected || has_inner_caps;
        let cap = !keep && (start || letters >= 4 || !MINOR.contains(&lower.as_str()));
        if cap {
            // Capitalize each hyphenated part of a major word ("Self-Report").
            let parts: Vec<String> = word.split('-').map(capitalize_first).collect();
            out.push_str(&parts.join("-"));
        } else {
            out.push_str(word);
        }
        start = word.ends_with(':') || word.ends_with('?') || word.ends_with('!') || word.ends_with('—');
    }
    out
}

fn capitalize_first(w: &str) -> String {
    let mut chars = w.chars();
    let mut out = String::new();
    // Skip leading punctuation such as quotation marks or parentheses.
    let mut done = false;
    for c in chars.by_ref() {
        if !done && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            done = true;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic() {
        assert_eq!(title_case("teaching with generative AI: a guide for the classroom", &[]), "Teaching With Generative AI: A Guide for the Classroom");
        assert_eq!(title_case("self-report of iPhone use", &[]), "Self-Report of iPhone Use");
    }
}
