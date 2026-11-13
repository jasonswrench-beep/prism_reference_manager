//! Entities of the library. Every entity is stored as canonical JSON in its own table and
//! every change to it is recorded as a change event (see `store`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Implemented by every stored entity.
pub trait Entity: Serialize + for<'de> Deserialize<'de> + Clone {
    const TABLE: &'static str;
    fn id(&self) -> &str;
}

macro_rules! entity {
    ($t:ty, $table:literal) => {
        impl Entity for $t {
            const TABLE: &'static str = $table;
            fn id(&self) -> &str {
                &self.id
            }
        }
    };
}

// ---------------------------------------------------------------------------------------------
// Names and dates

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NameKind {
    #[default]
    Person,
    /// An organization or group written as a single literal ("American Psychological Association").
    Organization,
    /// A person known by one name ("Plato", "Sukarno").
    Mononym,
}

/// A structured name. Display values keep their original script; folding is only for search.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Name {
    #[serde(default)]
    pub kind: NameKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub given: String,
    /// Particle kept with the surname for display and sorting ("van" in "van Gogh" when written that way).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub particle: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    /// Organization name or mononym.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub literal: String,
    /// Optional transliterated or alternative-script forms, tagged by language or script.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<NameVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct NameVariant {
    pub lang: String,
    pub family: String,
    pub given: String,
}

impl Name {
    pub fn person(family: &str, given: &str) -> Self {
        Name { kind: NameKind::Person, family: family.into(), given: given.into(), ..Default::default() }
    }
    pub fn org(literal: &str) -> Self {
        Name { kind: NameKind::Organization, literal: literal.into(), ..Default::default() }
    }
    pub fn mononym(n: &str) -> Self {
        Name { kind: NameKind::Mononym, literal: n.into(), ..Default::default() }
    }
    /// The surname as written in citations, including a kept particle.
    pub fn surname(&self) -> String {
        match self.kind {
            NameKind::Person => {
                if self.particle.is_empty() {
                    self.family.clone()
                } else {
                    format!("{} {}", self.particle, self.family)
                }
            }
            _ => self.literal.clone(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.family.trim().is_empty() && self.literal.trim().is_empty() && self.given.trim().is_empty()
    }
    /// Parse a single name written "Family, Given", "Given Family" or "{Organization}".
    pub fn parse(s: &str) -> Name {
        let t = s.trim();
        if t.starts_with('{') && t.ends_with('}') {
            return Name::org(t.trim_matches(|c| c == '{' || c == '}').trim());
        }
        let particles = ["van der", "van den", "van de", "von der", "van", "von", "de la", "de", "del", "della", "di", "da", "du", "le", "la", "ter", "ten", "al", "el", "bin", "ibn"];
        let split_particle = |family: &str| -> (String, String) {
            let lower = family.to_lowercase();
            for p in particles {
                if lower.starts_with(&format!("{p} ")) && family.is_char_boundary(p.len()) && family.is_char_boundary(p.len() + 1) {
                    return (family[..p.len()].to_string(), family[p.len() + 1..].trim().to_string());
                }
            }
            (String::new(), family.to_string())
        };
        if let Some((fam, rest)) = t.split_once(',') {
            let mut given = rest.trim().to_string();
            let mut suffix = String::new();
            if let Some((g, suf)) = rest.split_once(',') {
                // "Family, Suffix, Given" (BibTeX "von Last, Jr, First") or "Family, Given, Suffix"
                let (a, b) = (g.trim(), suf.trim());
                if is_suffix(a) {
                    suffix = a.into();
                    given = b.into();
                } else {
                    given = a.into();
                    suffix = b.into();
                }
            }
            let (particle, family) = split_particle(fam.trim());
            return Name { kind: NameKind::Person, family, given, particle, suffix, ..Default::default() };
        }
        let words: Vec<&str> = t.split_whitespace().collect();
        if words.is_empty() {
            return Name::default();
        }
        if words.len() == 1 {
            return Name::person(words[0], "");
        }
        // Given Family; detect lower-case particles in the middle.
        let mut idx = words.len() - 1;
        let mut suffix = String::new();
        if is_suffix(words[idx]) && idx > 1 {
            suffix = words[idx].trim_end_matches(',').to_string();
            idx -= 1;
        }
        let mut start = idx;
        while start > 1 && words[start - 1].chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
            start -= 1;
        }
        let given = words[..start].join(" ").trim_end_matches(',').to_string();
        let particle = words[start..idx].join(" ");
        let family = words[idx].to_string();
        Name { kind: NameKind::Person, family, given, particle, suffix, ..Default::default() }
    }
}

fn is_suffix(s: &str) -> bool {
    matches!(s.trim().trim_end_matches(',').to_lowercase().as_str(), "jr" | "jr." | "sr" | "sr." | "ii" | "iii" | "iv" | "v")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Creator {
    /// author, editor, translator, director, producer, guest, host, inventor, interviewer,
    /// recipient, contributor, chair, compiler, performer, reviewed-author ...
    pub role: String,
    pub name: Name,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PartialDate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<u8>,
    /// 1 spring, 2 summer, 3 autumn/fall, 4 winter
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<Box<PartialDate>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub approximate: bool,
    /// "in press", "forthcoming" and similar statuses.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    /// Original text when it could not be parsed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub literal: String,
}

impl PartialDate {
    pub fn year(y: i32) -> Self {
        PartialDate { year: Some(y), ..Default::default() }
    }
    pub fn ymd(y: i32, m: u8, d: u8) -> Self {
        PartialDate { year: Some(y), month: Some(m), day: Some(d), ..Default::default() }
    }
    pub fn is_empty(&self) -> bool {
        self.year.is_none() && self.status.is_empty() && self.literal.is_empty()
    }
    /// Parse common textual forms: "2024", "2024-03", "2024-03-05", "March 5, 2024", "2019/2020",
    /// "in press", "n.d.".
    pub fn parse(s: &str) -> Option<PartialDate> {
        let t = s.trim();
        if t.is_empty() {
            return None;
        }
        let lower = t.to_lowercase();
        if lower == "n.d." || lower == "nd" || lower == "n.d" {
            return None;
        }
        if lower.contains("in press") || lower == "forthcoming" {
            return Some(PartialDate { status: if lower.contains("press") { "in press".into() } else { "forthcoming".into() }, ..Default::default() });
        }
        let re_iso = re!(r"^(\d{4})(?:[-/.](\d{1,2})(?:[-/.](\d{1,2}))?)?$");
        if let Some(c) = re_iso.captures(t) {
            let y = c[1].parse().ok();
            let m = c.get(2).and_then(|m| m.as_str().parse().ok());
            let d = c.get(3).and_then(|m| m.as_str().parse().ok());
            if m.map(|m: u8| m <= 12).unwrap_or(true) {
                return Some(PartialDate { year: y, month: m, day: d, ..Default::default() });
            }
        }
        let re_range = re!(r"^(\d{4})\s*[-–/]\s*(\d{4})$");
        if let Some(c) = re_range.captures(t) {
            return Some(PartialDate {
                year: c[1].parse().ok(),
                end: Some(Box::new(PartialDate::year(c[2].parse().unwrap_or(0)))),
                ..Default::default()
            });
        }
        let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
        let seasons = [("spring", 1u8), ("summer", 2), ("fall", 3), ("autumn", 3), ("winter", 4)];
        let year_re = re!(r"\b(\d{4})\b");
        let year: Option<i32> = year_re.captures(t).and_then(|c| c[1].parse().ok());
        if year.is_some() {
            let mut d = PartialDate { year, ..Default::default() };
            for (i, m) in months.iter().enumerate() {
                if lower.contains(m) {
                    d.month = Some(i as u8 + 1);
                    let day_re = re!(r"\b(\d{1,2})\b");
                    d.day = day_re.captures(t).and_then(|c| c[1].parse().ok());
                    break;
                }
            }
            for (name, n) in seasons {
                if lower.contains(name) {
                    d.season = Some(n);
                }
            }
            if lower.starts_with('c') || lower.contains("ca.") || lower.contains("circa") {
                d.approximate = true;
            }
            return Some(d);
        }
        Some(PartialDate { literal: t.into(), ..Default::default() })
    }
    pub fn iso(&self) -> String {
        match (self.year, self.month, self.day) {
            (Some(y), Some(m), Some(d)) => format!("{y:04}-{m:02}-{d:02}"),
            (Some(y), Some(m), None) => format!("{y:04}-{m:02}"),
            (Some(y), _, _) => format!("{y:04}"),
            _ => self.literal.clone(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// References

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Provenance {
    /// "user", "import:<adapter>", "lookup:<resolver>", "pdf", "rule:<name>", "ai:<provider>"
    pub source: String,
    pub time: String,
    #[serde(default)]
    pub user_edited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// "reviewed", "unreviewed", "conflict"
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub review: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct FieldConflict {
    pub field: String,
    pub value: String,
    pub source: String,
    pub time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Notice {
    /// "retraction", "correction", "expression-of-concern"
    pub kind: String,
    pub date: String,
    pub source: String,
    pub url: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Reference {
    pub id: String,
    /// Reference type id from `types::TYPES` ("article-journal", "book", "chapter", ...).
    #[serde(rename = "type")]
    pub rtype: String,
    /// A custom type built on `rtype`, when the person chose one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subtype: String,
    #[serde(default)]
    pub citekey: String,
    #[serde(default)]
    pub citekey_locked: bool,
    #[serde(default)]
    pub creators: Vec<Creator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued: Option<PartialDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_date: Option<PartialDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessed: Option<PartialDate>,
    /// Plain fields: title, container_title, volume, issue, page, article_number, doi, url,
    /// publisher, publisher_place, edition, genre, medium, number, isbn, issn, pmid, pmcid,
    /// arxiv, abstract, language, collection_title, event, archive, archive_location, ...
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// Language-tagged alternative values: field -> language tag -> value.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub translations: BTreeMap<String, BTreeMap<String, String>>,
    /// Fields the person confirmed do not exist for this work (distinct from unknown).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub confirmed_absent: Vec<String>,
    /// Imported fields with no mapping, kept under a namespace ("ris:M3", "bibtex:keywords").
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overflow: BTreeMap<String, String>,
    /// Values of custom fields defined by the person.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, Provenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<FieldConflict>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub rating: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
    /// Groups versions of one work (preprint, published version, reprint).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub work_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version_label: String,
    /// When several versions share a work, the one to cite.
    #[serde(default)]
    pub preferred_version: bool,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub modified: String,
    #[serde(default)]
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trashed_at: Option<String>,
    /// Identifier of the import batch that created the record, if any.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub batch_id: String,
    /// "user", "import", "ai", "agent" — machine-created records are filterable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub origin: String,
}
entity!(Reference, "refs");

impl Reference {
    pub fn f(&self, k: &str) -> &str {
        self.fields.get(k).map(|s| s.as_str()).unwrap_or("")
    }
    pub fn has(&self, k: &str) -> bool {
        !self.f(k).trim().is_empty()
    }
    pub fn absent(&self, k: &str) -> bool {
        self.confirmed_absent.iter().any(|a| a == k)
    }
    pub fn creators_by(&self, role: &str) -> Vec<&Name> {
        self.creators.iter().filter(|c| c.role == role).map(|c| &c.name).collect()
    }
    pub fn year(&self) -> Option<i32> {
        self.issued.as_ref().and_then(|d| d.year)
    }
    pub fn title(&self) -> &str {
        self.f("title")
    }
}

/// A retired identifier that now resolves to a canonical record after a merge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Alias {
    pub id: String,
    pub target: String,
    pub merged_at: String,
}
entity!(Alias, "aliases");

// ---------------------------------------------------------------------------------------------
// Projects and manuscripts

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Project {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub default_style: String,
    #[serde(default)]
    pub archived: bool,
    /// Prohibit sending any project material to outside services.
    #[serde(default)]
    pub policy_no_external: bool,
    #[serde(default)]
    pub policy_no_ai: bool,
    #[serde(default)]
    pub active_manuscript: String,
    pub created: String,
    pub modified: String,
}
entity!(Project, "projects");

/// Membership of a canonical source in a project, with project-specific decisions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ProjectSource {
    /// "<project id>:<reference id>"
    pub id: String,
    pub project_id: String,
    pub ref_id: String,
    /// unread, reading, read
    #[serde(default)]
    pub reading: String,
    /// low, normal, high
    #[serde(default)]
    pub priority: String,
    /// candidate, included, excluded, undecided
    #[serde(default)]
    pub inclusion: String,
    #[serde(default)]
    pub exclusion_reason: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub evaluation: String,
    #[serde(default)]
    pub rating: u8,
    #[serde(default)]
    pub reading_position: Option<ReadingPosition>,
    /// Seconds spent with the source open in the reader while working in this project.
    #[serde(default)]
    pub time_spent_secs: u64,
    pub added: String,
}
entity!(ProjectSource, "project_sources");

impl ProjectSource {
    pub fn key(project_id: &str, ref_id: &str) -> String {
        format!("{project_id}:{ref_id}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ReadingPosition {
    pub attachment_id: String,
    pub page_index: u32,
    #[serde(default)]
    pub scroll: f32,
    #[serde(default)]
    pub zoom: f32,
    pub time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Manuscript {
    pub id: String,
    pub project_id: String,
    pub title: String,
    /// Style id, e.g. "apa-7".
    pub style: String,
    /// Citation locale, independent of the interface language.
    pub locale: String,
    #[serde(default)]
    pub archived: bool,
    /// Section structure used by section bibliographies.
    #[serde(default)]
    pub sections: Vec<String>,
    /// Revision of the citation context: increments when the set, order, style or locale changes.
    #[serde(default)]
    pub context_revision: u64,
    pub created: String,
    pub modified: String,
}
entity!(Manuscript, "manuscripts");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UseState {
    /// In the manuscript's reference set but not yet copied or confirmed.
    #[default]
    Candidate,
    /// Some output for it was copied to the clipboard; pasting is not presumed.
    Copied,
    /// The person confirmed it is cited in the manuscript.
    Confirmed,
    Excluded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ManuscriptSource {
    /// "<manuscript id>:<reference id>"
    pub id: String,
    pub manuscript_id: String,
    pub ref_id: String,
    pub state: UseState,
    #[serde(default)]
    pub section: String,
    pub added: String,
    pub modified: String,
}
entity!(ManuscriptSource, "manuscript_sources");

impl ManuscriptSource {
    pub fn key(m: &str, r: &str) -> String {
        format!("{m}:{r}")
    }
}

// ---------------------------------------------------------------------------------------------
// Citation output

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct CiteItem {
    pub ref_id: String,
    /// page, pages, paragraph, chapter, section, figure, table, line, timestamp, ...
    #[serde(default)]
    pub locator_label: String,
    #[serde(default)]
    pub locator: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub suppress_author: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    #[default]
    Parenthetical,
    Narrative,
    Reference,
    Bibliography,
    QuoteWithCitation,
    /// A short quotation introduced by a narrative citation, page after the quotation.
    NarrativeQuote,
    AuthorOnly,
    Citekey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CopyStatus {
    #[default]
    Copied,
    /// The person confirmed the copied text is in the manuscript.
    Used,
    /// The person confirmed they replaced the text with updated output.
    Replaced,
    Discarded,
}

/// A successful clipboard write. It records copying only, never presumed insertion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct CopyEvent {
    pub id: String,
    pub manuscript_id: String,
    pub project_id: String,
    pub mode: OutputMode,
    pub items: Vec<CiteItem>,
    /// Revision of each referenced record when copied.
    pub ref_revisions: BTreeMap<String, u64>,
    pub context_fingerprint: String,
    pub style: String,
    pub style_hash: String,
    pub locale: String,
    pub processor: String,
    pub payload_text: String,
    pub payload_html: String,
    pub payload_hash: String,
    #[serde(default)]
    pub quote: String,
    pub status: CopyStatus,
    /// Set when re-rendering produced different output.
    #[serde(default)]
    pub changed: bool,
    #[serde(default)]
    pub current_text: String,
    #[serde(default)]
    pub current_html: String,
    #[serde(default)]
    pub change_reason: String,
    #[serde(default)]
    pub imported_unknown: bool,
    pub created: String,
    pub modified: String,
}
entity!(CopyEvent, "copy_events");

/// A frozen submission snapshot: immutable citation-context package.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Snapshot {
    pub id: String,
    pub manuscript_id: String,
    pub project_id: String,
    pub label: String,
    pub created: String,
    pub style: String,
    pub style_hash: String,
    pub locale: String,
    pub processor: String,
    /// Full reference records as they were.
    pub references: Vec<Reference>,
    pub bibliography_text: String,
    pub bibliography_html: String,
    pub copies: Vec<CopyEvent>,
    pub checksum: String,
}
entity!(Snapshot, "snapshots");

// ---------------------------------------------------------------------------------------------
// Organization

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Collection {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent_id: String,
    /// Empty for library-wide collections.
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub ref_ids: Vec<String>,
    /// Non-empty for smart collections: a saved query in search syntax.
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub position: i64,
    pub created: String,
}
entity!(Collection, "collections");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Label {
    pub id: String,
    pub name: String,
    pub color: String,
}
entity!(Label, "labels");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SavedSearch {
    pub id: String,
    pub name: String,
    pub query: String,
    #[serde(default)]
    pub scope: String,
    pub created: String,
}
entity!(SavedSearch, "saved_searches");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct DuplicateDecision {
    /// "<smaller id>|<larger id>"
    pub id: String,
    pub a: String,
    pub b: String,
    /// "not_duplicate", "keep_both", "versions"
    pub decision: String,
    pub time: String,
}
entity!(DuplicateDecision, "duplicate_decisions");

impl DuplicateDecision {
    pub fn key(a: &str, b: &str) -> String {
        if a < b { format!("{a}|{b}") } else { format!("{b}|{a}") }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub ref_id: String,
    #[serde(default)]
    pub annotation_id: String,
    #[serde(default)]
    pub due: String,
    #[serde(default)]
    pub priority: String,
    /// open, done
    pub status: String,
    pub created: String,
}
entity!(Task, "tasks");

// ---------------------------------------------------------------------------------------------
// Files and annotations

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Attachment {
    pub id: String,
    pub ref_id: String,
    /// SHA-256 of the file bytes; identifies this immutable version.
    pub hash: String,
    /// Path relative to the library's file root for managed files; absolute for linked files.
    pub path: String,
    /// "managed" or "linked"
    pub mode: String,
    pub mime: String,
    /// full-text, supplement, data, image, notes, derivative, ocr
    pub role: String,
    pub original_name: String,
    pub size: u64,
    #[serde(default)]
    pub page_count: u32,
    /// Displayed page labels by physical index, when the file defines them.
    #[serde(default)]
    pub page_labels: Vec<String>,
    /// Offset added to the physical page number to obtain the printed page number, when the
    /// person has confirmed one (PDF page 7 = printed page 121 -> offset 114).
    #[serde(default)]
    pub citation_page_offset: Option<i64>,
    /// Previous attachment version this one replaces.
    #[serde(default)]
    pub replaces: String,
    /// Set on a version that a newer one replaced; retained, not deleted.
    #[serde(default)]
    pub superseded: bool,
    /// Where a downloaded copy came from (for example an open-access repository).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_url: String,
    /// Licence stated by the source for a downloaded copy.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    /// Version stated by the source: published, accepted or submitted manuscript.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_version: String,
    /// ok, missing, protected, malformed, large, image-only, placeholder
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub indexed: bool,
    #[serde(default)]
    pub index_excluded: bool,
    #[serde(default)]
    pub old_names: Vec<String>,
    pub created: String,
}
entity!(Attachment, "attachments");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Rect {
    /// Page-relative coordinates in 0..1 of the unrotated page, origin top-left.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Annotation {
    pub id: String,
    pub attachment_id: String,
    /// Hash of the attachment version the anchors belong to.
    pub version_hash: String,
    /// Empty for a library-wide annotation; otherwise the owning project.
    #[serde(default)]
    pub project_id: String,
    /// highlight, underline, strikeout, note, freetext, rect, ellipse, region, ink
    pub kind: String,
    /// Typed meaning chosen by the person: quotation, important, question, disagree, method, ...
    #[serde(default)]
    pub meaning: String,
    pub color: String,
    /// Zero-based physical page index.
    pub page_index: u32,
    /// Displayed PDF page label, if the file defines one.
    #[serde(default)]
    pub page_label: String,
    /// User-confirmed citation locator ("121", "para. 4"); empty until reviewed.
    #[serde(default)]
    pub locator: String,
    #[serde(default)]
    pub locator_confirmed: bool,
    #[serde(default)]
    pub rects: Vec<Rect>,
    /// Freehand strokes, page-relative.
    #[serde(default)]
    pub ink: Vec<Vec<[f32; 2]>>,
    pub page_width: f32,
    pub page_height: f32,
    #[serde(default)]
    pub rotation: i32,
    /// Exact selected text as extracted.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Linked note, annotation or reference ids.
    #[serde(default)]
    pub links: Vec<String>,
    /// matched, uncertain, unmatched — set by reanchoring after a file replacement.
    #[serde(default)]
    pub anchor_state: String,
    /// "prism" for annotations made here, or "external:<hash of identity>" for imported ones.
    #[serde(default)]
    pub external_key: String,
    #[serde(default)]
    pub author: String,
    pub created: String,
    pub modified: String,
}
entity!(Annotation, "annotations");

// ---------------------------------------------------------------------------------------------
// Notes and outline

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Note {
    pub id: String,
    /// quotation, paraphrase, summary, comment, question, task, evidence, thought, note
    pub kind: String,
    /// Empty for a deliberate library-wide note; otherwise the owning project.
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub title: String,
    /// The person's own text (Markdown).
    #[serde(default)]
    pub body: String,
    /// Exact source text for quotations; never edited silently.
    #[serde(default)]
    pub exact_text: String,
    /// Set when the person edited a quotation's exact text after extraction.
    #[serde(default)]
    pub exact_text_edited: bool,
    #[serde(default)]
    pub ref_ids: Vec<String>,
    #[serde(default)]
    pub locator_label: String,
    #[serde(default)]
    pub locator: String,
    #[serde(default)]
    pub annotation_id: String,
    /// resolved, unresolved — source-derived items must have a locator or be marked unresolved.
    #[serde(default)]
    pub anchor_state: String,
    /// user, ai, agent, import
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub origin_detail: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trashed_at: Option<String>,
    pub created: String,
    pub modified: String,
}
entity!(Note, "notes");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct OutlineSection {
    pub id: String,
    pub project_id: String,
    #[serde(default)]
    pub manuscript_id: String,
    #[serde(default)]
    pub parent_id: String,
    pub title: String,
    pub position: i64,
    /// Ordered note ids filed under this section. One note may appear in many sections.
    #[serde(default)]
    pub note_ids: Vec<String>,
}
entity!(OutlineSection, "outline_sections");

// ---------------------------------------------------------------------------------------------
// Imports, styles, integrations

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct StagedRecord {
    pub index: usize,
    /// Original input text for this record.
    pub raw: String,
    pub record: Reference,
    /// new, duplicate, conflict, unresolved, invalid
    pub status: String,
    #[serde(default)]
    pub match_ref_id: String,
    #[serde(default)]
    pub match_reason: String,
    #[serde(default)]
    pub issues: Vec<String>,
    /// accept, skip, merge, keep_both, link_version
    #[serde(default)]
    pub decision: String,
    /// Id of the committed record once the batch is committed.
    #[serde(default)]
    pub committed_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ImportBatch {
    pub id: String,
    /// Groups or folders found in the export, as paths ("Parent/Child").
    #[serde(default)]
    pub groups_found: Vec<String>,
    /// Create collections for the groups when committing.
    #[serde(default)]
    pub make_collections: bool,
    /// Treat the XML Label field as a group name too.
    #[serde(default)]
    pub label_as_groups: bool,
    pub adapter: String,
    pub adapter_version: String,
    pub file_name: String,
    pub file_hash: String,
    /// Where the file came from ("file", or the generic service label chosen by the person).
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub destination_project: String,
    #[serde(default)]
    pub destination_collection: String,
    #[serde(default)]
    pub mapping: BTreeMap<String, String>,
    #[serde(default)]
    pub unmapped_fields: BTreeMap<String, usize>,
    pub records: Vec<StagedRecord>,
    /// staged, committed, undone, cancelled, partial
    pub state: String,
    #[serde(default)]
    pub summary: BTreeMap<String, usize>,
    #[serde(default)]
    pub files_included: usize,
    pub created: String,
    #[serde(default)]
    pub committed: String,
}
entity!(ImportBatch, "import_batches");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct StylePackage {
    pub id: String,
    pub title: String,
    /// builtin, csl
    pub engine: String,
    /// verified, limited, custom_unreviewed, unsupported, update_available
    pub assurance: String,
    pub version: String,
    pub hash: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub csl: String,
    pub installed: String,
}
entity!(StylePackage, "styles");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct IntegrationRun {
    pub id: String,
    pub provider: String,
    pub operation: String,
    /// file_exchange, browser_handoff, api, native_sync
    pub transport: String,
    pub adapter_version: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub data_sent: String,
    #[serde(default)]
    pub result_ids: Vec<String>,
    pub ok: bool,
    #[serde(default)]
    pub message: String,
    pub time: String,
}
entity!(IntegrationRun, "integration_runs");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct CustomFieldDef {
    pub id: String,
    pub name: String,
    /// text, number, date, list, boolean, link
    pub kind: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub types: Vec<String>,
    /// A value must be given before the source can be saved.
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub help: String,
}
entity!(CustomFieldDef, "custom_fields");

/// A person-defined source type: a named variant of a built-in type with its own field set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct CustomType {
    pub id: String,
    pub label: String,
    /// Built-in type used for citation styles and exchange formats.
    pub base: String,
    /// Fields shown in the editor, in order.
    pub fields: Vec<String>,
    pub help: String,
}
entity!(CustomType, "custom_types");
