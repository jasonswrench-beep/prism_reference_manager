//! Deterministic citation rendering.
//!
//! The renderer receives reviewed reference records, ordered cluster members with locators and
//! affixes, the output mode, the locale and the manuscript's reference set. It returns
//! structured formatted runs, plain Unicode text, bibliography paragraph properties, warnings
//! and a context fingerprint. Every export serializer consumes the same result; none of them
//! applies punctuation rules of its own.

pub mod apa;
pub mod serialize;
pub mod titlecase;

use crate::model::{CiteItem, Reference};
use serde::{Deserialize, Serialize};

pub const PROCESSOR: &str = "prism-apa-composer";
pub const PROCESSOR_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sup: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sub: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Paragraph {
    pub ref_id: String,
    pub runs: Vec<Run>,
    /// Hanging indent in inches (0.5 for APA reference lists).
    pub hanging_indent: f32,
    /// Line spacing multiple (2.0 = double spaced).
    pub line_spacing: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Warning {
    pub ref_id: String,
    /// info, review, unsupported
    pub level: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Rendered {
    pub runs: Vec<Run>,
    pub text: String,
    #[serde(default)]
    pub paragraphs: Vec<Paragraph>,
    #[serde(default)]
    pub warnings: Vec<Warning>,
    pub fingerprint: String,
    /// True when the output depends on a reference set that was not a manuscript context.
    #[serde(default)]
    pub provisional: bool,
}

pub fn runs_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// Builder that merges adjacent runs with identical formatting.
#[derive(Default, Debug, Clone)]
pub struct Out {
    pub runs: Vec<Run>,
}

impl Out {
    pub fn push_run(&mut self, r: Run) {
        if r.text.is_empty() {
            return;
        }
        if let Some(last) = self.runs.last_mut() {
            if last.italic == r.italic && last.sup == r.sup && last.sub == r.sub && last.link == r.link {
                last.text.push_str(&r.text);
                return;
            }
        }
        self.runs.push(r);
    }
    pub fn t(&mut self, s: &str) {
        self.push_run(Run { text: s.into(), ..Default::default() });
    }
    pub fn i(&mut self, s: &str) {
        self.push_run(Run { text: s.into(), italic: true, ..Default::default() });
    }
    pub fn link(&mut self, s: &str) {
        self.push_run(Run { text: s.into(), link: Some(s.into()), ..Default::default() });
    }
    pub fn extend(&mut self, other: &Out) {
        for r in &other.runs {
            self.push_run(r.clone());
        }
    }
    pub fn text(&self) -> String {
        runs_text(&self.runs)
    }
    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.text.is_empty())
    }
    /// Last visible character.
    pub fn last_char(&self) -> Option<char> {
        self.runs.iter().rev().find_map(|r| r.text.chars().last())
    }
    /// Append a period unless the text already ends with terminal punctuation.
    pub fn period(&mut self) {
        match self.last_char() {
            Some('.') | Some('?') | Some('!') => {}
            Some(_) => self.t("."),
            None => {}
        }
    }
}

/// Context for rendering: the manuscript's reference set controls disambiguation.
#[derive(Debug, Clone, Default)]
pub struct Context<'a> {
    /// All references in the manuscript context (including the cited ones).
    pub set: Vec<&'a Reference>,
    pub locale: String,
    /// Manuscript context revision, part of the fingerprint.
    pub revision: u64,
    pub manuscript_id: String,
    /// True when no manuscript is chosen and the set is just the cited items.
    pub standalone: bool,
}

/// Style descriptor and assurance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleInfo {
    pub id: String,
    pub title: String,
    pub engine: String,
    pub version: String,
    pub hash: String,
    /// verified, limited, custom_unreviewed, unsupported, update_available
    pub assurance: String,
    pub coverage: serde_json::Value,
}

pub fn builtin_styles() -> Vec<StyleInfo> {
    let cov = apa::coverage();
    vec![StyleInfo {
        id: "apa-7".into(),
        title: "APA 7th edition".into(),
        engine: "builtin".into(),
        version: apa::STYLE_VERSION.into(),
        hash: apa::style_hash(),
        assurance: cov.get("assurance").and_then(|v| v.as_str()).unwrap_or("limited").to_string(),
        coverage: cov,
    }]
}

/// Render in-text citations for a cluster.
pub fn cite(style: &str, items: &[CiteItem], narrative: bool, refs: &[&Reference], ctx: &Context) -> Rendered {
    match style {
        _ => apa::cite(items, narrative, refs, ctx),
    }
}

/// Render reference-list entries for `refs`, sorted, using `ctx` for disambiguation.
pub fn bibliography(style: &str, refs: &[&Reference], ctx: &Context) -> Rendered {
    match style {
        _ => apa::bibliography(refs, ctx),
    }
}

pub fn fingerprint(style: &str, refs: &[&Reference], ctx: &Context) -> String {
    let mut parts: Vec<String> = ctx.set.iter().map(|r| format!("{}@{}", r.id, r.revision)).collect();
    parts.sort();
    let mut cited: Vec<String> = refs.iter().map(|r| format!("{}@{}", r.id, r.revision)).collect();
    cited.sort();
    let s = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        style,
        apa::style_hash(),
        PROCESSOR,
        PROCESSOR_VERSION,
        ctx.locale,
        ctx.revision,
        parts.join(","),
        cited.join(",")
    );
    crate::util::sha256_hex(s.as_bytes())[..16].to_string()
}
