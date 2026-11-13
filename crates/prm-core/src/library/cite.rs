//! Citation output in manuscript context, the copy ledger, changed-output detection,
//! manuscript readiness, bibliography export and frozen submission snapshots.

use super::Library;
use crate::error::{Error, Result};
use crate::model::*;
use crate::render::{self, serialize, Context, Rendered};
use crate::util::{new_id, now, sha256_hex};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CiteRequest {
    #[serde(default)]
    pub manuscript_id: String,
    #[serde(default)]
    pub items: Vec<CiteItem>,
    pub mode: OutputMode,
    #[serde(default)]
    pub quote: String,
    /// For bibliographies: selected, project, confirmed, manuscript
    #[serde(default)]
    pub set: String,
    #[serde(default)]
    pub ref_ids: Vec<String>,
    /// Cite exactly these records, even when another version of the work is marked to cite.
    #[serde(default)]
    pub exact_version: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Check {
    /// ok, review, info, unsupported
    pub state: String,
    pub label: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CiteOutput {
    pub text: String,
    pub html: String,
    pub rtf: String,
    pub markdown: String,
    pub runs: Vec<render::Run>,
    pub paragraphs: Vec<render::Paragraph>,
    pub warnings: Vec<render::Warning>,
    pub fingerprint: String,
    pub provisional: bool,
    pub style: String,
    pub style_title: String,
    pub assurance: String,
    pub locale: String,
    pub context_label: String,
    pub source_count: usize,
    /// Separate checks: identity, metadata, style, context.
    pub checks: Vec<Check>,
    pub block_quote: bool,
    pub locator_fragment: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangedCopy {
    pub copy: CopyEvent,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StyleIssue {
    pub ref_id: String,
    pub source: String,
    pub level: String,
    pub code: String,
    pub message: String,
    pub action: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Readiness {
    pub manuscript: Manuscript,
    pub project_sources: usize,
    pub manuscript_sources: usize,
    pub confirmed: usize,
    pub copied_items: usize,
    pub need_review: usize,
    pub changed: Vec<ChangedCopy>,
    pub copies: Vec<CopyEvent>,
    pub issues: Vec<StyleIssue>,
    pub coverage: serde_json::Value,
    pub snapshots: Vec<serde_json::Value>,
}

fn short_label(r: &Reference) -> String {
    let a = super::refs::author_short(r);
    let y = r.year().map(|y| y.to_string()).unwrap_or_else(|| "n.d.".into());
    if a.is_empty() { format!("{}, {}", super::refs::short(r.title()), y) } else { format!("{a}, {y}") }
}

impl Library {
    /// When `id` is one version of a work and another version is marked to cite, use that one.
    fn version_to_cite(&self, id: &str, notes: &mut Vec<render::Warning>) -> Result<String> {
        let Some(r) = self.store.get::<Reference>(id)? else { return Ok(id.to_string()) };
        if r.work_id.is_empty() || r.preferred_version {
            return Ok(id.to_string());
        }
        let chosen = self.store.by::<Reference>("work_id", &r.work_id)?.into_iter().find(|v| v.preferred_version && v.trashed_at.is_none() && v.id != r.id);
        let Some(v) = chosen else { return Ok(id.to_string()) };
        let label = |x: &Reference| if x.version_label.is_empty() { "the other version".to_string() } else { format!("the {} version", x.version_label.to_lowercase()) };
        notes.push(render::Warning {
            ref_id: v.id.clone(),
            level: "info".into(),
            code: "preferred-version".into(),
            message: format!("Cited {} of “{}”, the version marked to cite, instead of {}.", label(&v), super::refs::short(v.title()), label(&r)),
        });
        Ok(v.id)
    }

    /// The manuscript's reference set (everything not excluded).
    pub fn context_refs(&self, manuscript_id: &str) -> Result<Vec<Reference>> {
        if manuscript_id.is_empty() {
            return Ok(vec![]);
        }
        let ids: Vec<String> = self.store.by::<ManuscriptSource>("manuscript_id", manuscript_id)?.into_iter().filter(|m| m.state != UseState::Excluded).map(|m| m.ref_id).collect();
        let mut refs = self.references(&ids)?;
        refs.retain(|r| r.trashed_at.is_none());
        Ok(refs)
    }

    fn style_of(&self, manuscript_id: &str) -> Result<(String, String, u64)> {
        if manuscript_id.is_empty() {
            return Ok(("apa-7".into(), "en-US".into(), 0));
        }
        let m = self.store.must::<Manuscript>(manuscript_id)?;
        Ok((m.style, m.locale, m.context_revision))
    }

    /// Render output without side effects.
    pub fn render(&self, req: &CiteRequest) -> Result<CiteOutput> {
        let (style, locale, revision) = self.style_of(&req.manuscript_id)?;
        let styles = render::builtin_styles();
        let sinfo = styles.iter().find(|s| s.id == style).cloned().unwrap_or_else(|| styles[0].clone());
        let ctx_refs = self.context_refs(&req.manuscript_id)?;
        let standalone = req.manuscript_id.is_empty();

        let bib_ids: Vec<String> = match req.mode {
            OutputMode::Bibliography => self.bibliography_ids(&req.manuscript_id, &req.set, &req.ref_ids)?,
            OutputMode::Reference => req.ref_ids.iter().chain(req.items.iter().map(|i| &i.ref_id)).cloned().collect(),
            _ => req.items.iter().map(|i| i.ref_id.clone()).collect(),
        };
        let mut items = req.items.clone();
        let mut version_notes: Vec<render::Warning> = vec![];
        for it in &mut items {
            it.ref_id = self.resolve_id(&it.ref_id)?;
            if !req.exact_version {
                it.ref_id = self.version_to_cite(&it.ref_id, &mut version_notes)?;
            }
        }
        let bib_ids: Vec<String> = if req.exact_version {
            bib_ids
        } else {
            let mut out: Vec<String> = vec![];
            for id in bib_ids {
                let id = self.version_to_cite(&self.resolve_id(&id)?, &mut version_notes)?;
                if !out.contains(&id) {
                    out.push(id);
                }
            }
            out
        };
        let cited = self.references(&bib_ids)?;
        if cited.is_empty() {
            return Err(Error::user("Choose at least one source."));
        }
        let cited_refs: Vec<&Reference> = cited.iter().collect();
        let mut set: Vec<&Reference> = ctx_refs.iter().collect();
        for r in &cited {
            if !set.iter().any(|s| s.id == r.id) {
                set.push(r);
            }
        }
        let ctx = Context { set, locale: locale.clone(), revision, manuscript_id: req.manuscript_id.clone(), standalone, protected_terms: self.protected_terms.clone() };
        let mut block_quote = false;
        let mut locator_fragment = String::new();
        let mut rendered: Rendered = match req.mode {
            OutputMode::Parenthetical => render::cite(&style, &items, false, &cited_refs, &ctx),
            OutputMode::Narrative => {
                let r = render::cite(&style, &items, true, &cited_refs, &ctx);
                // A separate locator fragment for placing after a quotation in narrative writing.
                if let Some(i) = items.first() {
                    let l = render::apa::locator_text(&i.locator_label, &i.locator);
                    if !l.is_empty() {
                        locator_fragment = format!("({l})");
                    }
                }
                r
            }
            OutputMode::AuthorOnly => {
                let mut its = items.clone();
                for i in &mut its {
                    i.locator.clear();
                }
                let r = render::cite(&style, &its, true, &cited_refs, &ctx);
                let t = r.text.split(" (").next().unwrap_or("").to_string();
                Rendered { runs: vec![render::Run { text: t.clone(), ..Default::default() }], text: t, ..r }
            }
            OutputMode::QuoteWithCitation => {
                let c = render::cite(&style, &items, false, &cited_refs, &ctx);
                let (o, block) = render::apa::quote_with_citation(&req.quote, &c);
                block_quote = block;
                Rendered { text: o.text(), runs: o.runs, ..c }
            }
            OutputMode::Reference | OutputMode::Bibliography => render::bibliography(&style, &cited_refs, &ctx),
            OutputMode::Citekey => {
                let keys: Vec<String> = cited.iter().map(|r| format!("@{}", r.citekey)).collect();
                let t = if keys.len() == 1 { keys[0].clone() } else { format!("[{}]", keys.join("; ")) };
                Rendered { runs: vec![render::Run { text: t.clone(), ..Default::default() }], text: t, fingerprint: render::fingerprint(&style, &cited_refs, &ctx), ..Default::default() }
            }
        };
        let is_bib = matches!(req.mode, OutputMode::Reference | OutputMode::Bibliography);
        let html = if is_bib { serialize::html_paragraphs(&rendered.paragraphs) } else { serialize::html_inline(&rendered.runs) };
        let rtf = if is_bib { serialize::rtf_paragraphs(&rendered.paragraphs, None) } else { serialize::rtf_inline(&rendered.runs) };
        let markdown = if is_bib { serialize::markdown_paragraphs(&rendered.paragraphs) } else { serialize::markdown_runs(&rendered.runs) };
        for n in version_notes {
            if !rendered.warnings.iter().any(|w| w.code == n.code && w.ref_id == n.ref_id) {
                rendered.warnings.push(n);
            }
        }
        let checks = self.checks(&cited, &rendered, &sinfo, standalone, req.mode == OutputMode::Citekey);
        Ok(CiteOutput {
            text: rendered.text.clone(),
            html,
            rtf,
            markdown,
            runs: rendered.runs.clone(),
            paragraphs: rendered.paragraphs.clone(),
            warnings: rendered.warnings.clone(),
            fingerprint: rendered.fingerprint.clone(),
            provisional: standalone && !matches!(req.mode, OutputMode::Citekey),
            style: sinfo.id.clone(),
            style_title: sinfo.title.clone(),
            assurance: sinfo.assurance.clone(),
            locale,
            context_label: if standalone { "Standalone preview (provisional): no manuscript chosen, so disambiguation uses only these sources.".into() } else { format!("Manuscript reference set: {} sources", ctx_refs.len()) },
            source_count: cited.len(),
            checks,
            block_quote,
            locator_fragment,
        })
    }

    fn checks(&self, refs: &[Reference], r: &Rendered, style: &render::StyleInfo, standalone: bool, citekey: bool) -> Vec<Check> {
        let mut out = Vec::new();
        // Source identity: identifiers present and provenance reviewed.
        let unreviewed: Vec<String> = refs.iter().filter(|x| x.provenance.values().any(|p| p.review == "unreviewed") || !x.conflicts.is_empty()).map(short_label).collect();
        out.push(Check {
            state: if unreviewed.is_empty() { "ok".into() } else { "review".into() },
            label: "Source identity".into(),
            detail: if unreviewed.is_empty() { "Records are user-reviewed. A DOI confirms identity only, not a source's conclusions.".into() } else { format!("Imported values not yet reviewed: {}", unreviewed.join("; ")) },
        });
        let review: Vec<&render::Warning> = r.warnings.iter().filter(|w| w.level == "review").collect();
        let unsupported: Vec<&render::Warning> = r.warnings.iter().filter(|w| w.level == "unsupported").collect();
        out.push(Check {
            state: if !review.is_empty() { "review".into() } else { "ok".into() },
            label: "Metadata readiness".into(),
            detail: if review.is_empty() { "Required information for these source types is present or confirmed absent.".into() } else { review.iter().map(|w| w.message.clone()).collect::<Vec<_>>().join(" ") },
        });
        out.push(Check {
            state: if citekey { "info".into() } else if !unsupported.is_empty() { "unsupported".into() } else if style.assurance == "verified" { "ok".into() } else { "info".into() },
            label: "Style assurance".into(),
            detail: if citekey {
                "Citation keys are not rendered output and carry no style validation.".into()
            } else if !unsupported.is_empty() {
                unsupported.iter().map(|w| w.message.clone()).collect::<Vec<_>>().join(" ")
            } else {
                format!("{}: {}", style.title, match style.assurance.as_str() {
                    "verified" => "Verified profile for the tested cases.",
                    "limited" => "Limited testing. The cases pass but await review by a qualified person.",
                    "custom_unreviewed" => "Custom style, not reviewed.",
                    _ => "Unsupported case.",
                })
            },
        });
        out.push(Check {
            state: if standalone { "info".into() } else { "ok".into() },
            label: "Output context".into(),
            detail: if standalone { "Standalone preview. Choose a manuscript so its reference set controls year suffixes and name disambiguation.".into() } else { "Rendered from the manuscript's current reference set.".into() },
        });
        out
    }

    fn bibliography_ids(&self, manuscript_id: &str, set: &str, selected: &[String]) -> Result<Vec<String>> {
        match set {
            "selected" => Ok(selected.to_vec()),
            "project" => {
                let m = self.store.must::<Manuscript>(manuscript_id)?;
                Ok(self.store.by::<ProjectSource>("project_id", &m.project_id)?.into_iter().filter(|p| p.inclusion != "excluded").map(|p| p.ref_id).collect())
            }
            "manuscript" => Ok(self.context_refs(manuscript_id)?.into_iter().map(|r| r.id).collect()),
            _ => {
                if manuscript_id.is_empty() {
                    return Ok(selected.to_vec());
                }
                Ok(self.store.by::<ManuscriptSource>("manuscript_id", manuscript_id)?.into_iter().filter(|m| m.state == UseState::Confirmed).map(|m| m.ref_id).collect())
            }
        }
    }

    /// Record a copy after the clipboard write succeeded. Never marks the text as pasted.
    pub fn record_copy(&mut self, req: &CiteRequest, out: &CiteOutput) -> Result<CopyEvent> {
        let m = if req.manuscript_id.is_empty() { None } else { Some(self.store.must::<Manuscript>(&req.manuscript_id)?) };
        let ids: Vec<String> = if matches!(req.mode, OutputMode::Bibliography | OutputMode::Reference) {
            self.bibliography_ids(&req.manuscript_id, &req.set, &req.ref_ids).unwrap_or_default().into_iter().chain(req.items.iter().map(|i| i.ref_id.clone())).collect()
        } else {
            req.items.iter().map(|i| i.ref_id.clone()).collect()
        };
        let refs = self.references(&ids)?;
        let items: Vec<CiteItem> = if req.items.is_empty() { refs.iter().map(|r| CiteItem { ref_id: r.id.clone(), ..Default::default() }).collect() } else { req.items.clone() };
        let t = now();
        let ev = CopyEvent {
            id: new_id(),
            manuscript_id: req.manuscript_id.clone(),
            project_id: m.as_ref().map(|m| m.project_id.clone()).unwrap_or_default(),
            mode: req.mode.clone(),
            items,
            ref_revisions: refs.iter().map(|r| (r.id.clone(), r.revision)).collect(),
            context_fingerprint: out.fingerprint.clone(),
            style: out.style.clone(),
            style_hash: render::apa::style_hash(),
            locale: out.locale.clone(),
            processor: format!("{} {}", render::PROCESSOR, render::PROCESSOR_VERSION),
            payload_text: out.text.clone(),
            payload_html: out.html.clone(),
            payload_hash: sha256_hex(out.text.as_bytes()),
            quote: req.quote.clone(),
            status: CopyStatus::Copied,
            changed: false,
            current_text: String::new(),
            current_html: String::new(),
            change_reason: String::new(),
            imported_unknown: false,
            created: t.clone(),
            modified: t.clone(),
        };
        let e2 = ev.clone();
        let mut set_changed = false;
        let mut manuscript = m.clone();
        let mut entries = Vec::new();
        if let Some(mm) = &m {
            for r in &refs {
                let key = ManuscriptSource::key(&mm.id, &r.id);
                let old = self.store.get::<ManuscriptSource>(&key)?;
                let in_set = old.as_ref().map(|o| o.state != UseState::Excluded).unwrap_or(false);
                if !in_set {
                    set_changed = true;
                }
                let mut ms = old.unwrap_or(ManuscriptSource { id: key, manuscript_id: mm.id.clone(), ref_id: r.id.clone(), state: UseState::Candidate, section: String::new(), added: t.clone(), modified: t.clone() });
                if super::refs::rank(&ms.state) < super::refs::rank(&UseState::Copied) {
                    ms.state = UseState::Copied;
                    ms.modified = t.clone();
                    entries.push(ms);
                }
            }
            if set_changed {
                if let Some(mm) = manuscript.as_mut() {
                    mm.context_revision += 1;
                    mm.modified = t.clone();
                }
            }
        }
        self.write("Copy citation", |tx| {
            tx.put(&e2)?;
            for e in &entries {
                tx.put(e)?;
            }
            if set_changed {
                if let Some(mm) = &manuscript {
                    tx.put(mm)?;
                }
            }
            Ok(())
        })?;
        if set_changed {
            self.recompute_copies(Some(&req.manuscript_id))?;
        }
        Ok(ev)
    }

    /// Re-render copied output and flag meaningful differences. Flags are derived state, so they
    /// are written without creating an undo step.
    pub fn recompute_copies(&mut self, manuscript_id: Option<&str>) -> Result<usize> {
        let copies: Vec<CopyEvent> = match manuscript_id {
            Some(m) if !m.is_empty() => self.store.by("manuscript_id", m)?,
            _ => self.store.all()?,
        };
        let mut flagged = 0;
        for mut c in copies {
            if !matches!(c.status, CopyStatus::Copied | CopyStatus::Used) || c.imported_unknown {
                continue;
            }
            let req = CiteRequest {
                manuscript_id: c.manuscript_id.clone(),
                items: c.items.clone(),
                mode: c.mode.clone(),
                quote: c.quote.clone(),
                set: "selected".into(),
                ref_ids: c.items.iter().map(|i| i.ref_id.clone()).collect(),
                exact_version: false,
            };
            if !c.manuscript_id.is_empty() && self.store.get::<Manuscript>(&c.manuscript_id)?.is_none() {
                continue;
            }
            let cur = match self.render(&req) {
                Ok(o) => o,
                Err(_) => continue,
            };
            let changed = cur.text != c.payload_text;
            let reason = if changed { self.change_reason(&c, &cur)? } else { String::new() };
            if changed != c.changed || (changed && (cur.text != c.current_text || reason != c.change_reason)) {
                c.changed = changed;
                c.current_text = if changed { cur.text.clone() } else { String::new() };
                c.current_html = if changed { cur.html.clone() } else { String::new() };
                c.change_reason = reason;
                let s = crate::util::canonical_json(&serde_json::to_value(&c)?);
                self.store.conn.execute("UPDATE copy_events SET data=?1 WHERE id=?2", rusqlite::params![s, c.id])?;
            }
            if changed {
                flagged += 1;
            }
        }
        Ok(flagged)
    }

    fn change_reason(&self, c: &CopyEvent, cur: &CiteOutput) -> Result<String> {
        let mut reasons = Vec::new();
        let suffix = re!(r"(\d{4}|n\.d\.|in press)(-?[a-z]{1,2})?\b");
        let strip = |s: &str| suffix.replace_all(s, "$1").into_owned();
        if strip(&c.payload_text) == strip(&cur.text) {
            reasons.push("Year suffix changed after another work by the same authors entered the manuscript".to_string());
        }
        for (id, rev) in &c.ref_revisions {
            if let Ok(r) = self.reference(id) {
                if r.revision != *rev {
                    reasons.push(format!("{} was edited", short_label(&r)));
                }
            }
        }
        if c.style_hash != render::apa::style_hash() {
            reasons.push("The style or processor version changed".into());
        }
        if c.style != cur.style || c.locale != cur.locale {
            reasons.push("The manuscript style or locale changed".into());
        }
        if reasons.is_empty() {
            reasons.push("The manuscript reference set changed".into());
        }
        Ok(reasons.join("; "))
    }

    /// Mark a copy as used, replaced or discarded. "Replaced" means the person put the current
    /// text into their document; the saved payload becomes the current text.
    pub fn set_copy_status(&mut self, id: &str, status: CopyStatus) -> Result<CopyEvent> {
        let mut c = self.store.must::<CopyEvent>(id)?;
        if status == CopyStatus::Replaced && c.changed {
            c.payload_text = c.current_text.clone();
            c.payload_html = c.current_html.clone();
            c.payload_hash = sha256_hex(c.payload_text.as_bytes());
            for (id, rev) in c.ref_revisions.iter_mut() {
                if let Ok(r) = self.reference(id) {
                    *rev = r.revision;
                }
            }
            c.style_hash = render::apa::style_hash();
            c.changed = false;
            c.current_text.clear();
            c.current_html.clear();
            c.change_reason.clear();
            c.status = CopyStatus::Used;
        } else {
            c.status = status.clone();
        }
        c.modified = now();
        let cc = c.clone();
        let label = match status {
            CopyStatus::Used => "Mark copy as used",
            CopyStatus::Replaced => "Mark copy as replaced",
            CopyStatus::Discarded => "Discard copy",
            CopyStatus::Copied => "Reopen copy",
        };
        // Confirming use also confirms the sources in the manuscript.
        let confirm: Vec<String> = if matches!(status, CopyStatus::Used | CopyStatus::Replaced) && !c.manuscript_id.is_empty() { c.items.iter().map(|i| i.ref_id.clone()).collect() } else { vec![] };
        let mid = c.manuscript_id.clone();
        let mut entries = Vec::new();
        for r in &confirm {
            let key = ManuscriptSource::key(&mid, r);
            if let Some(mut ms) = self.store.get::<ManuscriptSource>(&key)? {
                if ms.state != UseState::Confirmed {
                    ms.state = UseState::Confirmed;
                    ms.modified = now();
                    entries.push(ms);
                }
            }
        }
        self.write(label, |tx| {
            tx.put(&cc)?;
            for e in &entries {
                tx.put(e)?;
            }
            Ok(())
        })?;
        Ok(c)
    }

    /// Record an old copy brought in from elsewhere whose source data and context were not
    /// saved. It is labelled unknown, never given a reconstructed history.
    pub fn record_unknown_copy(&mut self, manuscript_id: &str, text: &str, ref_ids: &[String]) -> Result<CopyEvent> {
        let t = now();
        let m = self.store.must::<Manuscript>(manuscript_id)?;
        let ev = CopyEvent {
            id: new_id(),
            manuscript_id: manuscript_id.into(),
            project_id: m.project_id,
            mode: OutputMode::Parenthetical,
            items: ref_ids.iter().map(|r| CiteItem { ref_id: r.clone(), ..Default::default() }).collect(),
            payload_text: text.into(),
            payload_hash: sha256_hex(text.as_bytes()),
            status: CopyStatus::Copied,
            imported_unknown: true,
            change_reason: "History unknown: copied outside this program".into(),
            created: t.clone(),
            modified: t,
            ..Default::default()
        };
        let e = ev.clone();
        self.write("Record earlier copy", |tx| tx.put(&e))?;
        Ok(ev)
    }

    pub fn readiness(&mut self, manuscript_id: &str) -> Result<Readiness> {
        self.recompute_copies(Some(manuscript_id))?;
        let m = self.store.must::<Manuscript>(manuscript_id)?;
        let ps = self.store.by::<ProjectSource>("project_id", &m.project_id)?.len();
        let ms = self.store.by::<ManuscriptSource>("manuscript_id", manuscript_id)?;
        let mut copies: Vec<CopyEvent> = self.store.by("manuscript_id", manuscript_id)?;
        copies.sort_by(|a, b| b.created.cmp(&a.created));
        let changed: Vec<ChangedCopy> = copies
            .iter()
            .filter(|c| c.changed && matches!(c.status, CopyStatus::Copied | CopyStatus::Used))
            .map(|c| ChangedCopy { copy: c.clone(), sources: c.items.iter().filter_map(|i| self.reference(&i.ref_id).ok()).map(|r| short_label(&r)).collect() })
            .collect();
        let set_refs = self.context_refs(manuscript_id)?;
        let check_refs = if set_refs.is_empty() { self.references(&self.store.by::<ProjectSource>("project_id", &m.project_id)?.into_iter().map(|p| p.ref_id).collect::<Vec<_>>())? } else { set_refs };
        let issues = self.style_issues(&check_refs, &copies)?;
        let snaps: Vec<serde_json::Value> = self
            .store
            .all::<Snapshot>()?
            .into_iter()
            .filter(|s| s.manuscript_id == manuscript_id)
            .map(|s| serde_json::json!({"id": s.id, "label": s.label, "created": s.created, "sources": s.references.len(), "checksum": s.checksum}))
            .collect();
        Ok(Readiness {
            manuscript: m,
            project_sources: ps,
            manuscript_sources: ms.iter().filter(|x| x.state != UseState::Excluded).count(),
            confirmed: ms.iter().filter(|x| x.state == UseState::Confirmed).count(),
            copied_items: copies.iter().filter(|c| c.status != CopyStatus::Discarded).count(),
            need_review: changed.len(),
            changed,
            copies,
            issues,
            coverage: render::apa::coverage(),
            snapshots: snaps,
        })
    }

    /// Evidence-based issues for a set of sources, with a concrete suggested action.
    pub fn style_issues(&self, refs: &[Reference], copies: &[CopyEvent]) -> Result<Vec<StyleIssue>> {
        let mut out = Vec::new();
        let ctx = Context { set: refs.iter().collect(), locale: "en-US".into(), revision: 0, manuscript_id: String::new(), standalone: false, protected_terms: self.protected_terms.clone() };
        let rr: Vec<&Reference> = refs.iter().collect();
        let bib = render::bibliography("apa-7", &rr, &ctx);
        for w in bib.warnings.iter().filter(|w| w.level != "info" || w.code == "no-date") {
            let r = refs.iter().find(|r| r.id == w.ref_id);
            let src = r.map(short_label).unwrap_or_default();
            let (action, before, after) = match w.code.as_str() {
                "article-number-as-page" => {
                    let p = r.map(|r| r.f("page").to_string()).unwrap_or_default();
                    ("Review fields".to_string(), format!("Pages: {p}"), format!("Article number: {p}"))
                }
                "conflict" => ("Compare sources".into(), String::new(), String::new()),
                "unsupported-type" => ("Review output".into(), String::new(), String::new()),
                "no-date" => ("Add date or confirm none".into(), String::new(), String::new()),
                _ => ("Review fields".into(), String::new(), String::new()),
            };
            out.push(StyleIssue { ref_id: w.ref_id.clone(), source: src, level: w.level.clone(), code: w.code.clone(), message: w.message.clone(), action, before, after });
        }
        // Organization names that might have been split into person parts.
        for r in refs {
            for c in &r.creators {
                let n = &c.name;
                if n.kind == NameKind::Person && n.given.split_whitespace().count() >= 2 && ["Association", "Institute", "University", "Center", "Centre", "Council", "Organization", "Department", "Office", "Society", "Agency", "Foundation"].iter().any(|w| n.family.contains(w) || n.given.contains(w)) {
                    out.push(StyleIssue { ref_id: r.id.clone(), source: short_label(r), level: "review".into(), code: "possible-organization".into(), message: format!("“{} {}” may be an organization entered as a person.", n.given, n.family), action: "Review author".into(), before: format!("Person: {}, {}", n.family, n.given), after: format!("Organization: {} {}", n.given, n.family) });
                }
                if n.kind == NameKind::Person && n.family.len() > 3 && n.family.chars().all(|c| !c.is_lowercase()) {
                    out.push(StyleIssue { ref_id: r.id.clone(), source: short_label(r), level: "review".into(), code: "all-caps-name".into(), message: format!("“{}” is written in capitals.", n.family), action: "Review author".into(), before: n.family.clone(), after: title_word(&n.family) });
                }
            }
        }
        for c in copies.iter().filter(|c| c.changed && matches!(c.status, CopyStatus::Copied | CopyStatus::Used)) {
            let src = c.items.first().and_then(|i| self.reference(&i.ref_id).ok()).map(|r| short_label(&r)).unwrap_or_default();
            out.push(StyleIssue { ref_id: c.items.first().map(|i| i.ref_id.clone()).unwrap_or_default(), source: src, level: "review".into(), code: "copy-changed".into(), message: "Copied output changed after an edit.".into(), action: "Review copies".into(), before: c.payload_text.clone(), after: c.current_text.clone() });
        }
        Ok(out)
    }

    /// Apply a reviewed proposal from the style checks. Never invents values.
    pub fn apply_suggestion(&mut self, ref_id: &str, code: &str) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(ref_id)?;
        match code {
            "article-number-as-page" => {
                let p = r.f("page").to_string();
                if p.is_empty() {
                    return Err(Error::user("There is no page value to move."));
                }
                r.fields.insert("article_number".into(), p);
                r.fields.remove("page");
            }
            "possible-organization" => {
                for c in &mut r.creators {
                    let n = &mut c.name;
                    if n.kind == NameKind::Person && n.given.split_whitespace().count() >= 2 {
                        n.literal = format!("{} {}", n.given, n.family).trim().to_string();
                        n.kind = NameKind::Organization;
                        n.given.clear();
                        n.family.clear();
                        break;
                    }
                }
            }
            "all-caps-name" => {
                for c in &mut r.creators {
                    if c.name.family.len() > 3 && c.name.family.chars().all(|ch| !ch.is_lowercase()) {
                        c.name.family = title_word(&c.name.family);
                    }
                    if c.name.given.len() > 1 && c.name.given.chars().all(|ch| !ch.is_lowercase()) {
                        c.name.given = c.name.given.split(' ').map(title_word).collect::<Vec<_>>().join(" ");
                    }
                }
            }
            _ => return Err(Error::user("No automatic change is available for this issue; edit the record.")),
        }
        self.update_reference(r, "user")
    }

    /// Bibliography as a file. `format`: docx, rtf, html, txt, md.
    pub fn export_bibliography(&self, req: &CiteRequest, format: &str, font: Option<&str>) -> Result<(Vec<u8>, String)> {
        let mut r = req.clone();
        r.mode = OutputMode::Bibliography;
        let out = self.render(&r)?;
        let title = "References";
        let note = format!(
            "{} · {} · {}",
            out.style_title,
            match out.assurance.as_str() { "verified" => "Verified profile", "limited" => "Limited testing", _ => "Review required" },
            if out.provisional { "standalone preview" } else { "manuscript context" }
        );
        Ok(match format {
            "docx" => {
                let mut blocks = vec![serialize::DocBlock::Heading(1, title.into())];
                blocks.extend(out.paragraphs.iter().cloned().map(serialize::DocBlock::RefPara));
                (serialize::docx(&blocks, font)?, "references.docx".into())
            }
            "rtf" => {
                let mut blocks = vec![serialize::DocBlock::Heading(1, title.into())];
                blocks.extend(out.paragraphs.iter().cloned().map(serialize::DocBlock::RefPara));
                (serialize::rtf_document(&blocks, font).into_bytes(), "references.rtf".into())
            }
            "html" => (serialize::html_document(title, &format!("<h1>{title}</h1>\n{}\n<!-- {} -->", out.html, crate::util::html_escape(&note))).into_bytes(), "references.html".into()),
            "md" => (format!("# {title}\n\n{}", out.markdown).into_bytes(), "references.md".into()),
            _ => (out.text.into_bytes(), "references.txt".into()),
        })
    }

    /// Freeze a submission snapshot: immutable copy of source records, style dependencies,
    /// rendered bibliography and the copy ledger, with a checksum.
    pub fn save_snapshot(&mut self, manuscript_id: &str, label: &str) -> Result<Snapshot> {
        let m = self.store.must::<Manuscript>(manuscript_id)?;
        let refs = self.context_refs(manuscript_id)?;
        let confirmed: Vec<String> = self.store.by::<ManuscriptSource>("manuscript_id", manuscript_id)?.into_iter().filter(|x| x.state == UseState::Confirmed).map(|x| x.ref_id).collect();
        let bib_ids = if confirmed.is_empty() { refs.iter().map(|r| r.id.clone()).collect() } else { confirmed };
        let (text, html) = if bib_ids.is_empty() {
            (String::new(), String::new())
        } else {
            let out = self.render(&CiteRequest { manuscript_id: manuscript_id.into(), mode: OutputMode::Bibliography, set: "selected".into(), ref_ids: bib_ids, ..Default::default() })?;
            (out.text, out.html)
        };
        let mut s = Snapshot {
            id: new_id(),
            manuscript_id: manuscript_id.into(),
            project_id: m.project_id.clone(),
            label: if label.trim().is_empty() { format!("Snapshot {}", &now()[..10]) } else { label.trim().into() },
            created: now(),
            style: m.style.clone(),
            style_hash: render::apa::style_hash(),
            locale: m.locale.clone(),
            processor: format!("{} {}", render::PROCESSOR, render::PROCESSOR_VERSION),
            references: refs,
            bibliography_text: text,
            bibliography_html: html,
            copies: self.store.by("manuscript_id", manuscript_id)?,
            checksum: String::new(),
        };
        s.checksum = sha256_hex(crate::util::canonical_json(&serde_json::to_value(&s)?).as_bytes());
        let ss = s.clone();
        self.write("Save snapshot", |tx| tx.put(&ss))?;
        Ok(s)
    }

    /// Verify a snapshot's checksum (detects any modification).
    pub fn verify_snapshot(&self, id: &str) -> Result<bool> {
        let mut s = self.store.must::<Snapshot>(id)?;
        let stored = std::mem::take(&mut s.checksum);
        Ok(sha256_hex(crate::util::canonical_json(&serde_json::to_value(&s)?).as_bytes()) == stored)
    }

    /// Text with the cited sources and their reference entries, for a quotation note.
    pub fn quote_text(&self, note_id: &str, manuscript_id: &str) -> Result<CiteOutput> {
        let n = self.store.must::<Note>(note_id)?;
        if n.exact_text.is_empty() {
            return Err(Error::user("This note has no saved quotation text."));
        }
        if n.ref_ids.is_empty() {
            return Err(Error::user("This quotation has no source; link a source first."));
        }
        if n.locator.is_empty() && n.anchor_state != "no-locator" {
            return Err(Error::user("This quotation's locator has not been confirmed. Review the page or paragraph first."));
        }
        let items = vec![CiteItem { ref_id: n.ref_ids[0].clone(), locator_label: if n.locator_label.is_empty() { "page".into() } else { n.locator_label.clone() }, locator: n.locator.clone(), ..Default::default() }];
        self.render(&CiteRequest { manuscript_id: manuscript_id.into(), items, mode: OutputMode::QuoteWithCitation, quote: n.exact_text.clone(), ..Default::default() })
    }

    /// Context ids and display text for every source in a set (used by the UI pickers).
    pub fn source_labels(&self, ids: &[String]) -> Result<BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for id in ids {
            if seen.insert(id.clone()) {
                if let Ok(r) = self.reference(id) {
                    out.insert(id.clone(), short_label(&r));
                }
            }
        }
        Ok(out)
    }
}

fn title_word(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::new();
    let mut up = true;
    for c in lower.chars() {
        if up && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
        if c == '-' || c == '\'' || c == ' ' {
            up = true;
        }
    }
    out
}
