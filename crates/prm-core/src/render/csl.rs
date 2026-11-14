//! Citation Style Language styles, rendered by an embedded CSL processor.
//!
//! APA 7 keeps its own tested composer (`apa.rs`). Every other style goes through this bridge:
//! records are converted to CSL JSON, rendered, and the processor's formatted output is turned
//! back into the same runs and paragraphs every serializer uses. Styles rendered here carry no
//! golden-case evidence, so they are labelled *Untested* (bundled) or *Custom, not reviewed*
//! (imported) and every output says so.

use super::{Context, Out, Paragraph, Rendered, Run, StyleInfo, Warning};
use crate::model::{CiteItem, Reference};
use hayagriva::archive::{self, ArchivedStyle};
use hayagriva::citationberg::json::Item;
use hayagriva::citationberg::{IndependentStyle, Locale, LocaleCode, Style, StyleClass};
use hayagriva::{BibliographyDriver, BibliographyRequest, CitationItem, CitationRequest, CitePurpose, ElemChild, ElemChildren, LocatorPayload, SpecificLocator};
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

pub const PROCESSOR: &str = "hayagriva";
pub const PROCESSOR_VERSION: &str = "0.10.1";
/// Licence of the styles distributed by the CSL project, which the bundled archive comes from.
pub const BUNDLED_LICENSE: &str = "Creative Commons Attribution-ShareAlike 3.0";

/// A parsed style ready to render.
pub struct LoadedStyle {
    pub style: IndependentStyle,
    /// Locale forced by a dependent style, if any.
    pub forced_locale: Option<String>,
}

static LOCALES: LazyLock<Vec<Locale>> = LazyLock::new(archive::locales);
static BUNDLED: LazyLock<Mutex<BTreeMap<String, Arc<LoadedStyle>>>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));

fn bundled_id(a: ArchivedStyle) -> String {
    format!("csl:{}", a.names()[0])
}

/// Hidden from the list: APA (APA 7 uses the tested composer) and anything that is not a CSL
/// project style, so the licence shown for bundled styles is accurate.
fn hidden(a: ArchivedStyle) -> bool {
    a.names().iter().any(|n| n.starts_with("apa") || n.contains("american-psychological"))
        || !a.csl_id().contains("/styles/")
        || KNOWN_BROKEN.contains(&a.names()[0])
}

/// Bundled styles withheld because the processor version in use renders them wrongly in a way
/// a person could miss (checked by `every_bundled_style_renders_a_citation_and_reference`).
/// The NLM citation-sequence styles drop every author name from reference entries.
pub const KNOWN_BROKEN: &[&str] = &["nlm-citation-sequence", "nlm-citation-sequence-superscript"];

fn hash_of(s: &str) -> String {
    crate::util::sha256_hex(s.as_bytes())[..16].to_string()
}

fn class_of(s: &IndependentStyle) -> &'static str {
    if s.settings.class == StyleClass::Note { "note" } else { "in-text" }
}

fn coverage(assurance: &str, note: &str) -> serde_json::Value {
    serde_json::json!({ "assurance": assurance, "cases": 0, "passed": 0, "reviewed": 0, "processor": PROCESSOR, "processor_version": PROCESSOR_VERSION, "note": note, "categories": [], "results": [] })
}

fn independent(s: Style) -> Option<IndependentStyle> {
    match s {
        Style::Independent(i) => Some(i),
        Style::Dependent(_) => None,
    }
}

/// Styles bundled with the program, in display order.
pub fn bundled_styles() -> Vec<StyleInfo> {
    static LIST: LazyLock<Vec<StyleInfo>> = LazyLock::new(|| {
        let mut v: Vec<StyleInfo> = ArchivedStyle::all()
            .iter()
            .filter(|a| !hidden(**a))
            .filter_map(|a| {
                let s = independent(a.get())?;
                Some(StyleInfo {
                    id: bundled_id(*a),
                    title: a.display_name().to_string(),
                    engine: "csl".into(),
                    version: format!("{PROCESSOR} {PROCESSOR_VERSION}"),
                    hash: hash_of(&format!("{PROCESSOR} {PROCESSOR_VERSION} {}", a.csl_id())),
                    assurance: "untested".into(),
                    coverage: coverage("untested", "Bundled CSL style. No golden cases have been run or reviewed for it in this program, so check every citation against the style's manual or the publisher's instructions."),
                    source: "bundled".into(),
                    class: class_of(&s).into(),
                    license: BUNDLED_LICENSE.into(),
                    csl_id: a.csl_id().to_string(),
                    parent: String::new(),
                })
            })
            .collect();
        v.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        v
    });
    LIST.clone()
}

pub fn load_bundled(id: &str) -> Option<Arc<LoadedStyle>> {
    if let Some(s) = BUNDLED.lock().ok()?.get(id) {
        return Some(s.clone());
    }
    let a = ArchivedStyle::all().iter().copied().find(|a| bundled_id(*a) == id && !hidden(*a))?;
    let loaded = Arc::new(LoadedStyle { style: independent(a.get())?, forced_locale: None });
    BUNDLED.lock().ok()?.insert(id.to_string(), loaded.clone());
    Some(loaded)
}

/// Find an independent bundled style by its CSL id (used to resolve dependent styles).
pub fn bundled_by_csl_id(csl_id: &str) -> Option<IndependentStyle> {
    let a = ArchivedStyle::by_id(csl_id)?;
    independent(a.get())
}

/// What an imported style file contains, before it is accepted.
pub struct ParsedStyle {
    pub info: StyleInfo,
    pub loaded: LoadedStyle,
}

/// Parse a .csl file. Dependent styles are resolved against `lookup` (CSL id → parent style).
pub fn parse_style(xml: &str, lookup: &dyn Fn(&str) -> Option<IndependentStyle>) -> Result<ParsedStyle, String> {
    // Real citation styles are well under 1 MB; a larger file is refused before the XML reader
    // sees it (that reader's version has a memory-exhaustion bug with huge inputs).
    if xml.len() > 2 * 1024 * 1024 {
        return Err("This file is too large to be a citation style (over 2 MB).".into());
    }
    let style = Style::from_xml(xml).map_err(|e| format!("This is not a valid CSL style file: {e}"))?;
    let hash = hash_of(xml);
    let (independent, forced_locale, info, parent) = match style {
        Style::Independent(s) => {
            let info = s.info.clone();
            (s, None, info, String::new())
        }
        Style::Dependent(d) => {
            let href = d.parent_link.href.clone();
            let parent = lookup(&href).ok_or_else(|| format!("This style is based on another style ({href}) that is not installed. Import that style first."))?;
            (parent, d.default_locale.as_ref().map(|l| l.0.clone()), d.info.clone(), href)
        }
    };
    let license = info.rights.as_ref().map(|r| r.name.clone()).unwrap_or_else(|| "Not stated in the style file".into());
    let class = class_of(&independent).to_string();
    Ok(ParsedStyle {
        info: StyleInfo {
            id: format!("user:{hash}"),
            title: info.title.value.clone(),
            engine: "csl".into(),
            version: format!("{PROCESSOR} {PROCESSOR_VERSION}"),
            hash,
            assurance: "custom_unreviewed".into(),
            coverage: coverage("custom_unreviewed", "Imported CSL style. It has not been tested or reviewed in this program; check every citation before submitting."),
            source: "imported".into(),
            class,
            license,
            csl_id: info.id.clone(),
            parent,
        },
        loaded: LoadedStyle { style: independent, forced_locale },
    })
}

// ---- record conversion -----------------------------------------------------------------------

/// A record as the processor's CSL JSON item. Literal dates become `status`; date ranges are
/// reduced to their start because the processor's JSON reader does not accept ranges.
pub fn item(r: &Reference) -> Option<Item> {
    let mut v = crate::formats::csljson::to_csl(r);
    let o = v.as_object_mut()?;
    let mut status = None;
    o.retain(|_, d| {
        if let Some(l) = d.get("literal").and_then(|x| x.as_str()) {
            if d.get("date-parts").is_none() {
                status = Some(l.to_string());
                return false;
            }
        }
        true
    });
    if let Some(st) = status {
        o.entry("status").or_insert(serde_json::json!(st));
    }
    for (_, d) in o.iter_mut() {
        if let Some(parts) = d.get_mut("date-parts").and_then(|p| p.as_array_mut()) {
            parts.truncate(1);
        }
    }
    // Drop anything the reader would reject rather than failing the whole item.
    let keys: Vec<String> = o.keys().cloned().collect();
    for k in keys {
        let one = serde_json::json!({ k.clone(): o[&k].clone() });
        if serde_json::from_value::<Item>(one).is_err() {
            o.remove(&k);
        }
    }
    serde_json::from_value(v).ok()
}

// ---- output conversion -----------------------------------------------------------------------

fn push_children(children: &ElemChildren, link: Option<&str>, out: &mut Out) {
    for c in &children.0 {
        match c {
            ElemChild::Text(t) => out.push_run(run(&t.text, &t.formatting, link)),
            ElemChild::Elem(e) => push_children(&e.children, link, out),
            ElemChild::Markup(m) => out.push_run(Run { text: m.clone(), link: link.map(|l| l.to_string()), ..Default::default() }),
            ElemChild::Link { text, url } => out.push_run(run(&text.text, &text.formatting, Some(url))),
            ElemChild::Transparent { .. } => {}
        }
    }
}

fn run(text: &str, f: &hayagriva::Formatting, link: Option<&str>) -> Run {
    use hayagriva::citationberg::{FontStyle, FontVariant, FontWeight, VerticalAlign};
    Run {
        text: text.to_string(),
        italic: matches!(f.font_style, FontStyle::Italic),
        bold: matches!(f.font_weight, FontWeight::Bold),
        small_caps: matches!(f.font_variant, FontVariant::SmallCaps),
        sup: matches!(f.vertical_align, VerticalAlign::Sup),
        sub: matches!(f.vertical_align, VerticalAlign::Sub),
        link: link.map(|l| l.to_string()),
    }
}

fn locator<'a>(i: &'a CiteItem) -> Option<SpecificLocator<'a>> {
    if i.locator.trim().is_empty() {
        return None;
    }
    let label = if i.locator_label.is_empty() { "page" } else { i.locator_label.as_str() };
    let kind = serde_json::from_value(serde_json::json!(label)).unwrap_or(hayagriva::citationberg::taxonomy::Locator::Page);
    Some(SpecificLocator(kind, LocatorPayload::Str(i.locator.trim())))
}

fn locale(ls: &LoadedStyle, ctx: &Context) -> Option<LocaleCode> {
    ls.forced_locale.clone().or_else(|| if ctx.locale.is_empty() { None } else { Some(ctx.locale.clone()) }).map(LocaleCode)
}

fn untested_note(info: &StyleInfo) -> Warning {
    Warning {
        ref_id: String::new(),
        level: "review".into(),
        code: "style-untested".into(),
        message: if info.source == "imported" {
            format!("{} is an imported style that has not been reviewed. Check this output against the style's instructions.", info.title)
        } else {
            format!("{} has not been tested in this program. Check this output against the style's manual or the publisher's instructions.", info.title)
        },
    }
}

fn failed(msg: &str) -> Rendered {
    Rendered {
        warnings: vec![Warning { ref_id: String::new(), level: "unsupported".into(), code: "style-failed".into(), message: msg.into() }],
        ..Default::default()
    }
}

/// The items of the context set and the cited records, de-duplicated, in context order.
fn items_for<'a>(refs: &[&'a Reference], ctx: &Context<'a>) -> (Vec<String>, BTreeMap<String, Item>, Vec<Warning>) {
    let mut order = vec![];
    let mut map = BTreeMap::new();
    let mut warnings = vec![];
    for r in ctx.set.iter().chain(refs.iter()) {
        if map.contains_key(&r.id) {
            continue;
        }
        match item(r) {
            Some(i) => {
                order.push(r.id.clone());
                map.insert(r.id.clone(), i);
            }
            None => warnings.push(Warning { ref_id: r.id.clone(), level: "unsupported".into(), code: "csl-item".into(), message: "This source could not be prepared for the style processor.".into() }),
        }
    }
    (order, map, warnings)
}

/// In-text citation (or note text for note styles).
pub fn cite(ls: &LoadedStyle, info: &StyleInfo, items: &[CiteItem], narrative: bool, refs: &[&Reference], ctx: &Context) -> Rendered {
    let (order, map, mut warnings) = items_for(refs, ctx);
    let loc = locale(ls, ctx);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut driver = BibliographyDriver::new();
        // Register the context first so numbering and disambiguation see the whole set.
        for id in &order {
            if let Some(it) = map.get(id) {
                driver.citation(CitationRequest::new(vec![CitationItem::with_entry(it)], &ls.style, loc.clone(), &LOCALES, None));
            }
        }
        let cluster: Vec<CitationItem<Item>> = items
            .iter()
            .filter_map(|ci| {
                let it = map.get(&ci.ref_id)?;
                let mut c = CitationItem::with_locator(it, locator(ci));
                if ci.suppress_author {
                    c = c.kind(CitePurpose::Year);
                } else if narrative {
                    c = c.kind(CitePurpose::Prose);
                }
                Some(c)
            })
            .collect();
        if cluster.is_empty() {
            return None;
        }
        driver.citation(CitationRequest::new(cluster, &ls.style, loc.clone(), &LOCALES, Some(1)));
        let r = driver.finish(BibliographyRequest::new(&ls.style, loc.clone(), &LOCALES));
        r.citations.last().map(|c| {
            let mut out = Out::default();
            push_children(&c.citation, None, &mut out);
            out
        })
    }));
    let out = match res {
        Ok(Some(o)) => o,
        Ok(None) => return failed("Choose at least one source."),
        Err(_) => return failed("The style processor could not format this citation. Try another style or report the source and style."),
    };
    if items.iter().any(|i| !i.prefix.trim().is_empty() || !i.suffix.trim().is_empty()) {
        warnings.push(Warning { ref_id: String::new(), level: "review".into(), code: "csl-affixes".into(), message: "Prefixes and suffixes are not placed automatically in this style; add them in your document.".into() });
    }
    if ls.style.settings.class == StyleClass::Note {
        warnings.push(Warning { ref_id: String::new(), level: "info".into(), code: "note-style".into(), message: "This is a note style: paste the citation into a footnote or endnote.".into() });
    }
    warnings.insert(0, untested_note(info));
    let text = out.text();
    Rendered { runs: out.runs, text, paragraphs: vec![], warnings, fingerprint: String::new(), provisional: ctx.standalone }
}

/// Reference-list entries for `refs`, in the style's order.
pub fn bibliography(ls: &LoadedStyle, info: &StyleInfo, refs: &[&Reference], ctx: &Context) -> Rendered {
    let (order, map, mut warnings) = items_for(refs, ctx);
    let wanted: Vec<String> = refs.iter().map(|r| r.id.clone()).collect();
    let loc = locale(ls, ctx);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut driver = BibliographyDriver::new();
        for id in &order {
            if let Some(it) = map.get(id) {
                driver.citation(CitationRequest::new(vec![CitationItem::with_entry(it)], &ls.style, loc.clone(), &LOCALES, None));
            }
        }
        driver.finish(BibliographyRequest::new(&ls.style, loc.clone(), &LOCALES)).bibliography
    }));
    let bib = match res {
        Ok(Some(b)) => b,
        Ok(None) => return failed("This style has no reference list."),
        Err(_) => return failed("The style processor could not format this reference list. Try another style or report the sources and style."),
    };
    let indent = if bib.hanging_indent { 0.5 } else { 0.0 };
    let spacing = f32::from(bib.line_spacing.get());
    let mut paragraphs = vec![];
    let mut all = Out::default();
    for it in bib.items.iter().filter(|i| wanted.contains(&i.key)) {
        let mut out = Out::default();
        if let Some(first) = &it.first_field {
            push_children(&ElemChildren(vec![first.clone()]), None, &mut out);
            out.t(" ");
        }
        push_children(&it.content, None, &mut out);
        if !all.is_empty() {
            all.t("\n");
        }
        all.extend(&out);
        paragraphs.push(Paragraph { ref_id: it.key.clone(), runs: out.runs, hanging_indent: indent, line_spacing: spacing });
    }
    warnings.insert(0, untested_note(info));
    let text = all.text();
    Rendered { runs: all.runs, text, paragraphs, warnings, fingerprint: String::new(), provisional: ctx.standalone }
}
