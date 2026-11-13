//! Knowledge items, the manuscript outline, tasks, and note/outline exports.

use super::Library;
use crate::error::{Error, Result};
use crate::model::*;
use crate::render::{serialize, Run};
use crate::util::{new_id, now};
use serde::Serialize;

pub const NOTE_KINDS: &[&str] = &["quotation", "paraphrase", "summary", "comment", "question", "task", "evidence", "thought", "note"];

/// Kinds derived from a source: they need a source and a locator, or an explicit unresolved state.
fn source_derived(kind: &str) -> bool {
    matches!(kind, "quotation" | "paraphrase" | "evidence")
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlineNode {
    pub section: OutlineSection,
    pub children: Vec<OutlineNode>,
}

impl Library {
    pub fn notes(&self, project_id: &str, include_library: bool) -> Result<Vec<Note>> {
        let mut v: Vec<Note> = self.store.all::<Note>()?.into_iter().filter(|n| n.trashed_at.is_none() && (n.project_id == project_id || (include_library && n.project_id.is_empty()))).collect();
        v.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(v)
    }

    pub fn notes_for_ref(&self, ref_id: &str, project_id: &str) -> Result<Vec<Note>> {
        Ok(self.store.all::<Note>()?.into_iter().filter(|n| n.trashed_at.is_none() && n.ref_ids.iter().any(|r| r == ref_id) && (n.project_id.is_empty() || n.project_id == project_id)).collect())
    }

    fn validate_note(n: &mut Note) -> Result<()> {
        if !NOTE_KINDS.contains(&n.kind.as_str()) {
            return Err(Error::user(format!("“{}” is not a note type.", n.kind)));
        }
        if n.kind == "quotation" && n.exact_text.trim().is_empty() {
            return Err(Error::user("A direct quotation needs the exact source text."));
        }
        if source_derived(&n.kind) {
            if n.ref_ids.is_empty() {
                n.anchor_state = "unresolved".into();
            } else if n.locator.trim().is_empty() && n.anchor_state != "no-locator" {
                n.anchor_state = "unresolved".into();
            } else if n.anchor_state != "no-locator" {
                n.anchor_state = "resolved".into();
            }
        }
        Ok(())
    }

    /// Create or update a knowledge item. New items default to the active project; a
    /// library-wide note must be chosen deliberately (empty project id).
    pub fn save_note(&mut self, mut n: Note) -> Result<Note> {
        self.link_citekeys(&mut n)?;
        Self::validate_note(&mut n)?;
        let t = now();
        let label;
        if n.id.is_empty() {
            n.id = new_id();
            n.created = t.clone();
            if n.origin.is_empty() {
                n.origin = "user".into();
            }
            label = "Add note";
        } else {
            let old = self.store.must::<Note>(&n.id)?;
            if old.exact_text != n.exact_text && !old.exact_text.is_empty() {
                // Never keep presenting an edited quotation as an untouched extraction.
                n.exact_text_edited = true;
            }
            n.created = old.created;
            label = "Edit note";
        }
        n.modified = t;
        let nn = n.clone();
        self.write(label, |tx| tx.put(&nn))?;
        Ok(n)
    }

    /// Sources cited as @citekey in the note text become source links; unknown keys are listed.
    fn link_citekeys(&self, n: &mut Note) -> Result<()> {
        n.unresolved_keys.clear();
        let mut keys: Vec<String> = vec![];
        for c in re!(r"(?:^|[\s(\[;,])@([A-Za-z0-9_][A-Za-z0-9_:.\-]*[A-Za-z0-9_])").captures_iter(&n.body) {
            let k = c[1].to_string();
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        for k in keys {
            let id: Option<String> = self.store.conn.query_row("SELECT id FROM ref_index WHERE citekey=?1 AND trashed=0", [&k], |r| r.get(0)).ok();
            match id {
                Some(id) => {
                    if !n.ref_ids.contains(&id) {
                        n.ref_ids.push(id);
                    }
                }
                None => n.unresolved_keys.push(k),
            }
        }
        Ok(())
    }

    /// Merge `other` into `keep`: text appended, sources, tags and labels combined; `other` goes to
    /// the trash. One undoable step.
    pub fn merge_notes(&mut self, keep: &str, other: &str) -> Result<Note> {
        if keep == other {
            return Err(Error::user("Choose a different note to merge."));
        }
        let mut a = self.store.must::<Note>(keep)?;
        let mut b = self.store.must::<Note>(other)?;
        if !b.exact_text.is_empty() {
            if a.exact_text.is_empty() {
                a.exact_text = b.exact_text.clone();
            } else if a.exact_text != b.exact_text {
                a.body = format!("{}\n\n> “{}”", a.body.trim_end(), b.exact_text);
            }
        }
        if !b.body.trim().is_empty() {
            a.body = if a.body.trim().is_empty() { b.body.clone() } else { format!("{}\n\n{}", a.body.trim_end(), b.body.trim()) };
        }
        for id in &b.ref_ids {
            if !a.ref_ids.contains(id) {
                a.ref_ids.push(id.clone());
            }
        }
        for t in &b.tags {
            if !a.tags.contains(t) {
                a.tags.push(t.clone());
            }
        }
        for l in &b.labels {
            if !a.labels.contains(l) {
                a.labels.push(l.clone());
            }
        }
        if a.locator.is_empty() && !b.locator.is_empty() {
            a.locator = b.locator.clone();
            a.locator_label = b.locator_label.clone();
        }
        a.modified = now();
        b.trashed_at = Some(now());
        let (aa, bb) = (a.clone(), b.clone());
        self.write("Merge notes", |tx| {
            tx.put(&aa)?;
            tx.put(&bb)
        })?;
        Ok(a)
    }

    /// Split a note's text at a character position: the text after it becomes a new note with
    /// the same type, scope, sources and tags. The quotation stays with the original.
    pub fn split_note(&mut self, id: &str, at: usize) -> Result<Note> {
        let mut a = self.store.must::<Note>(id)?;
        let chars: Vec<char> = a.body.chars().collect();
        if at == 0 || at >= chars.len() {
            return Err(Error::user("Place the cursor inside the note text where it should be split."));
        }
        let first: String = chars[..at].iter().collect();
        let second: String = chars[at..].iter().collect();
        if second.trim().is_empty() || first.trim().is_empty() {
            return Err(Error::user("Both parts need some text."));
        }
        a.body = first.trim_end().to_string();
        a.modified = now();
        let b = Note { id: new_id(), kind: a.kind.clone(), project_id: a.project_id.clone(), title: String::new(), body: second.trim_start().to_string(), ref_ids: a.ref_ids.clone(), tags: a.tags.clone(), labels: a.labels.clone(), origin: "user".into(), created: now(), modified: now(), anchor_state: if a.ref_ids.is_empty() { a.anchor_state.clone() } else { "no-locator".into() }, ..Default::default() };
        let (aa, bb) = (a.clone(), b.clone());
        self.write("Split note", |tx| {
            tx.put(&aa)?;
            tx.put(&bb)
        })?;
        Ok(b)
    }

    pub fn trash_note(&mut self, id: &str) -> Result<()> {
        self.write("Delete note", |tx| {
            let mut n = tx.must::<Note>(id)?;
            n.trashed_at = Some(now());
            tx.put(&n)?;
            for mut s in tx.all::<OutlineSection>()? {
                if s.note_ids.iter().any(|x| x == id) {
                    s.note_ids.retain(|x| x != id);
                    tx.put(&s)?;
                }
            }
            Ok(())
        })
    }

    /// Accept an AI- or agent-created item: it becomes the person's own item with its origin kept.
    pub fn accept_machine_note(&mut self, id: &str) -> Result<Note> {
        let mut n = self.store.must::<Note>(id)?;
        n.origin_detail = format!("{} (accepted {})", n.origin_detail, &now()[..10]);
        self.save_note(n)
    }

    /// Create a quotation note from an annotation, carrying its exact text and locator.
    pub fn note_from_annotation(&mut self, annotation_id: &str, kind: &str) -> Result<Note> {
        let a = self.store.must::<Annotation>(annotation_id)?;
        let att = self.store.must::<Attachment>(&a.attachment_id)?;
        let n = Note {
            id: String::new(),
            kind: if kind.is_empty() { "quotation".into() } else { kind.into() },
            project_id: a.project_id.clone(),
            title: String::new(),
            body: if kind == "quotation" || kind.is_empty() { a.comment.clone() } else { String::new() },
            exact_text: if kind == "quotation" || kind.is_empty() { a.text.clone() } else { String::new() },
            exact_text_edited: false,
            ref_ids: vec![att.ref_id.clone()],
            locator_label: "page".into(),
            locator: if a.locator_confirmed { a.locator.clone() } else { String::new() },
            annotation_id: a.id.clone(),
            anchor_state: String::new(),
            origin: "user".into(),
            origin_detail: String::new(),
            labels: vec![],
            tags: a.tags.clone(),
            trashed_at: None,
            created: String::new(),
            modified: String::new(),
            unresolved_keys: vec![],
        };
        self.save_note(n)
    }

    // ---- outline ------------------------------------------------------------------------------

    pub fn outline(&self, project_id: &str) -> Result<Vec<OutlineNode>> {
        let mut all: Vec<OutlineSection> = self.store.by("project_id", project_id)?;
        all.sort_by_key(|s| s.position);
        fn build(parent: &str, all: &[OutlineSection]) -> Vec<OutlineNode> {
            all.iter().filter(|s| s.parent_id == parent).map(|s| OutlineNode { section: s.clone(), children: build(&s.id, all) }).collect()
        }
        Ok(build("", &all))
    }

    pub fn save_section(&mut self, mut s: OutlineSection) -> Result<OutlineSection> {
        if s.title.trim().is_empty() {
            return Err(Error::user("A section needs a title."));
        }
        if s.id.is_empty() {
            s.id = new_id();
            let siblings: Vec<OutlineSection> = self.store.by::<OutlineSection>("project_id", &s.project_id)?.into_iter().filter(|x| x.parent_id == s.parent_id).collect();
            s.position = siblings.iter().map(|x| x.position).max().unwrap_or(-1) + 1;
        }
        let ss = s.clone();
        self.write("Save outline section", |tx| tx.put(&ss))?;
        Ok(s)
    }

    pub fn delete_section(&mut self, id: &str) -> Result<()> {
        let s = self.store.must::<OutlineSection>(id)?;
        let all: Vec<OutlineSection> = self.store.by("project_id", &s.project_id)?;
        self.write("Delete outline section", |tx| {
            // Children move up to the deleted section's parent; notes are not deleted.
            for mut c in all.into_iter().filter(|c| c.parent_id == id) {
                c.parent_id = s.parent_id.clone();
                tx.put(&c)?;
            }
            tx.delete::<OutlineSection>(id)
        })
    }

    /// Move a section to a new parent and position (keyboard and drag use the same operation).
    pub fn move_section(&mut self, id: &str, parent_id: &str, position: i64) -> Result<()> {
        let s = self.store.must::<OutlineSection>(id)?;
        let mut siblings: Vec<OutlineSection> = self.store.by::<OutlineSection>("project_id", &s.project_id)?.into_iter().filter(|x| x.parent_id == parent_id && x.id != id).collect();
        siblings.sort_by_key(|x| x.position);
        let mut moved = s.clone();
        moved.parent_id = parent_id.into();
        let pos = (position.max(0) as usize).min(siblings.len());
        siblings.insert(pos, moved);
        self.write("Move outline section", |tx| {
            for (i, mut x) in siblings.into_iter().enumerate() {
                x.position = i as i64;
                tx.put(&x)?;
            }
            Ok(())
        })
    }

    /// File a note under a section at a position (membership only; the note is not copied).
    pub fn file_note(&mut self, section_id: &str, note_id: &str, position: Option<usize>, from_section: Option<&str>) -> Result<()> {
        self.write("File note in outline", |tx| {
            if let Some(f) = from_section {
                if f != section_id {
                    let mut src = tx.must::<OutlineSection>(f)?;
                    src.note_ids.retain(|x| x != note_id);
                    tx.put(&src)?;
                }
            }
            let mut s = tx.must::<OutlineSection>(section_id)?;
            s.note_ids.retain(|x| x != note_id);
            let p = position.unwrap_or(s.note_ids.len()).min(s.note_ids.len());
            s.note_ids.insert(p, note_id.into());
            tx.put(&s)
        })
    }

    pub fn unfile_note(&mut self, section_id: &str, note_id: &str) -> Result<()> {
        self.write("Remove note from section", |tx| {
            let mut s = tx.must::<OutlineSection>(section_id)?;
            s.note_ids.retain(|x| x != note_id);
            tx.put(&s)
        })
    }

    // ---- tasks ----------------------------------------------------------------------------------

    pub fn tasks(&self, project_id: &str) -> Result<Vec<Task>> {
        let mut v: Vec<Task> = self.store.all::<Task>()?.into_iter().filter(|t| project_id.is_empty() || t.project_id == project_id).collect();
        v.sort_by(|a, b| a.status.cmp(&b.status).then(a.due.cmp(&b.due)));
        Ok(v)
    }

    pub fn save_task(&mut self, mut t: Task) -> Result<Task> {
        if t.title.trim().is_empty() {
            return Err(Error::user("A task needs a title."));
        }
        if t.id.is_empty() {
            t.id = new_id();
            t.created = now();
        }
        if t.status.is_empty() {
            t.status = "open".into();
        }
        let tt = t.clone();
        self.write("Save task", |tx| tx.put(&tt))?;
        Ok(t)
    }

    pub fn delete_task(&mut self, id: &str) -> Result<()> {
        self.write("Delete task", |tx| tx.delete::<Task>(id))
    }

    // ---- exports --------------------------------------------------------------------------------

    fn note_citation(&self, n: &Note, manuscript_id: &str) -> Option<String> {
        let rid = n.ref_ids.first()?;
        let item = CiteItem { ref_id: rid.clone(), locator_label: if n.locator_label.is_empty() { "page".into() } else { n.locator_label.clone() }, locator: n.locator.clone(), ..Default::default() };
        self.render(&super::CiteRequest { manuscript_id: manuscript_id.into(), items: vec![item], mode: OutputMode::Parenthetical, ..Default::default() }).ok().map(|o| o.text)
    }

    fn note_blocks(&self, n: &Note, manuscript_id: &str) -> Vec<serialize::DocBlock> {
        let mut b = Vec::new();
        let kind = match n.kind.as_str() {
            "quotation" => "Direct quotation",
            "paraphrase" => "Paraphrase",
            "summary" => "Summary",
            "comment" => "Comment",
            "question" => "Question",
            "evidence" => "Evidence",
            "thought" => "Thought",
            "task" => "Task",
            _ => "Note",
        };
        let origin = if matches!(n.origin.as_str(), "ai" | "agent") { format!(" [{} origin: {}]", n.origin.to_uppercase(), n.origin_detail) } else { String::new() };
        b.push(serialize::DocBlock::Para(vec![Run { text: format!("{kind}{}{origin}", if n.title.is_empty() { String::new() } else { format!(": {}", n.title) }), italic: true, ..Default::default() }]));
        if !n.exact_text.is_empty() {
            let cite = self.note_citation(n, manuscript_id).unwrap_or_else(|| "(source unresolved)".into());
            let edited = if n.exact_text_edited { " [quotation edited after extraction]" } else { "" };
            b.push(serialize::DocBlock::Quote(vec![Run { text: format!("“{}” {cite}{edited}", n.exact_text), ..Default::default() }]));
        } else if let Some(c) = self.note_citation(n, manuscript_id) {
            if source_derived(&n.kind) || n.kind == "summary" {
                b.push(serialize::DocBlock::Para(vec![Run { text: format!("Source: {c}"), ..Default::default() }]));
            }
        }
        if !n.body.trim().is_empty() {
            for para in n.body.split("\n\n") {
                b.push(serialize::DocBlock::Para(vec![Run { text: para.trim().to_string(), ..Default::default() }]));
            }
        }
        b
    }

    /// Export notes (optionally only those filed in the outline, in outline order).
    /// `format`: docx, rtf, md, html, json.
    pub fn export_notes(&self, project_id: &str, manuscript_id: &str, outline_only: bool, format: &str) -> Result<(Vec<u8>, String)> {
        let p = self.store.must::<Project>(project_id)?;
        let notes = self.notes(project_id, false)?;
        let mut blocks = vec![serialize::DocBlock::Heading(1, format!("{} — {}", p.title, if outline_only { "Outline" } else { "Notes" }))];
        let mut ordered: Vec<(usize, String, Vec<Note>)> = Vec::new();
        fn walk(lib: &Library, nodes: &[OutlineNode], depth: usize, notes: &[Note], out: &mut Vec<(usize, String, Vec<Note>)>) {
            for n in nodes {
                let ns: Vec<Note> = n.section.note_ids.iter().filter_map(|id| notes.iter().find(|x| &x.id == id).cloned()).collect();
                out.push((depth, n.section.title.clone(), ns));
                walk(lib, &n.children, depth + 1, notes, out);
            }
        }
        let outline = self.outline(project_id)?;
        walk(self, &outline, 2, &notes, &mut ordered);
        if !outline_only {
            let filed: std::collections::BTreeSet<String> = ordered.iter().flat_map(|(_, _, ns)| ns.iter().map(|n| n.id.clone())).collect();
            let unfiled: Vec<Note> = notes.iter().filter(|n| !filed.contains(&n.id)).cloned().collect();
            if !unfiled.is_empty() {
                ordered.push((2, "Not in the outline".into(), unfiled));
            }
        }
        if format == "json" {
            let v = serde_json::json!({
                "format": "prism-notes",
                "version": 1,
                "project": {"id": p.id, "title": p.title},
                "exported": now(),
                "sections": ordered.iter().map(|(d, t, ns)| serde_json::json!({"depth": d, "title": t, "notes": ns})).collect::<Vec<_>>(),
                "sources": self.references(&notes.iter().flat_map(|n| n.ref_ids.clone()).collect::<Vec<_>>())?.iter().map(crate::formats::csljson::to_csl).collect::<Vec<_>>(),
            });
            return Ok((serde_json::to_vec_pretty(&v)?, "notes.json".into()));
        }
        for (depth, title, ns) in &ordered {
            blocks.push(serialize::DocBlock::Heading((*depth).min(3) as u8, title.clone()));
            for n in ns {
                blocks.extend(self.note_blocks(n, manuscript_id));
            }
        }
        // References for every cited source.
        let ids: Vec<String> = ordered.iter().flat_map(|(_, _, ns)| ns.iter().flat_map(|n| n.ref_ids.clone())).collect();
        if !ids.is_empty() {
            if let Ok(bib) = self.render(&super::CiteRequest { manuscript_id: manuscript_id.into(), mode: OutputMode::Bibliography, set: "selected".into(), ref_ids: ids, ..Default::default() }) {
                blocks.push(serialize::DocBlock::Heading(2, "References".into()));
                blocks.extend(bib.paragraphs.into_iter().map(serialize::DocBlock::RefPara));
            }
        }
        Ok(match format {
            "docx" => (serialize::docx(&blocks, None)?, "notes.docx".into()),
            "rtf" => (serialize::rtf_document(&blocks, None).into_bytes(), "notes.rtf".into()),
            "html" => {
                let mut body = String::new();
                for b in &blocks {
                    match b {
                        serialize::DocBlock::Heading(l, t) => body.push_str(&format!("<h{l}>{}</h{l}>\n", crate::util::html_escape(t))),
                        serialize::DocBlock::Para(r) => body.push_str(&format!("<p>{}</p>\n", serialize::html_runs(r))),
                        serialize::DocBlock::Quote(r) => body.push_str(&format!("<blockquote>{}</blockquote>\n", serialize::html_runs(r))),
                        serialize::DocBlock::RefPara(p) => body.push_str(&serialize::html_paragraphs(std::slice::from_ref(p))),
                    }
                }
                (serialize::html_document(&p.title, &body).into_bytes(), "notes.html".into())
            }
            "txt" => {
                let mut t = String::new();
                for b in &blocks {
                    match b {
                        serialize::DocBlock::Heading(l, h) => {
                            t.push_str(h);
                            t.push('\n');
                            t.push_str(&(if *l <= 1 { "=" } else { "-" }).repeat(h.chars().count().min(80)));
                            t.push_str("\n\n");
                        }
                        serialize::DocBlock::Para(r) => t.push_str(&format!("{}\n\n", serialize::plain(r))),
                        serialize::DocBlock::Quote(r) => t.push_str(&format!("    {}\n\n", serialize::plain(r))),
                        serialize::DocBlock::RefPara(p) => t.push_str(&format!("{}\n\n", serialize::plain(&p.runs))),
                    }
                }
                (t.into_bytes(), "notes.txt".into())
            }
            _ => {
                let mut md = String::new();
                for b in &blocks {
                    match b {
                        serialize::DocBlock::Heading(l, t) => md.push_str(&format!("{} {t}\n\n", "#".repeat(*l as usize))),
                        serialize::DocBlock::Para(r) => md.push_str(&format!("{}\n\n", serialize::markdown_runs(r))),
                        serialize::DocBlock::Quote(r) => md.push_str(&format!("> {}\n\n", serialize::markdown_runs(r))),
                        serialize::DocBlock::RefPara(p) => md.push_str(&format!("{}\n\n", serialize::markdown_runs(&p.runs))),
                    }
                }
                (md.into_bytes(), "notes.md".into())
            }
        })
    }

    /// Annotation list export (Markdown, plain text or HTML) with citations.
    pub fn export_annotations(&self, attachment_id: &str, project_id: &str, manuscript_id: &str, format: &str) -> Result<(Vec<u8>, String)> {
        let att = self.store.must::<Attachment>(attachment_id)?;
        let r = self.reference(&att.ref_id)?;
        let mut anns: Vec<Annotation> = self.store.by::<Annotation>("attachment_id", attachment_id)?.into_iter().filter(|a| a.project_id.is_empty() || a.project_id == project_id).collect();
        anns.sort_by(|a, b| a.page_index.cmp(&b.page_index).then(a.rects.first().map(|r| r.y).partial_cmp(&b.rects.first().map(|r| r.y)).unwrap_or(std::cmp::Ordering::Equal)));
        let mut lines = Vec::new();
        for a in &anns {
            let cite = if a.locator_confirmed && !a.locator.is_empty() {
                self.render(&super::CiteRequest { manuscript_id: manuscript_id.into(), items: vec![CiteItem { ref_id: r.id.clone(), locator_label: "page".into(), locator: a.locator.clone(), ..Default::default() }], mode: OutputMode::Parenthetical, ..Default::default() }).map(|o| o.text).unwrap_or_default()
            } else {
                format!("(PDF page {}; citation page not confirmed)", a.page_index + 1)
            };
            lines.push((a.kind.clone(), a.text.clone(), a.comment.clone(), cite));
        }
        let title = r.title().to_string();
        Ok(match format {
            "docx" => {
                let mut blocks = vec![serialize::DocBlock::Heading(1, title.clone())];
                for (k, t, c, cite) in &lines {
                    let label = super::super::render::Run { text: format!("{}: ", k), italic: false, ..Default::default() };
                    if !t.is_empty() {
                        blocks.push(serialize::DocBlock::Quote(vec![label, super::super::render::Run { text: format!("“{t}” {cite}"), ..Default::default() }]));
                    } else {
                        blocks.push(serialize::DocBlock::Para(vec![label, super::super::render::Run { text: cite.clone(), ..Default::default() }]));
                    }
                    if !c.is_empty() {
                        blocks.push(serialize::DocBlock::Para(vec![super::super::render::Run { text: c.clone(), italic: true, ..Default::default() }]));
                    }
                }
                (serialize::docx(&blocks, None)?, "annotations.docx".into())
            }
            "json" => {
                let v = serde_json::json!({
                    "format": "prism-annotations",
                    "version": 1,
                    "source": crate::formats::csljson::to_csl(&r),
                    "file": { "name": att.original_name, "sha256": att.hash },
                    "annotations": anns.iter().zip(lines.iter()).map(|(a, (_, _, _, cite))| serde_json::json!({
                        "id": a.id, "kind": a.kind, "page_index": a.page_index, "page_label": a.locator, "page_confirmed": a.locator_confirmed,
                        "text": a.text, "comment": a.comment, "tags": a.tags, "color": a.color, "citation": cite,
                    })).collect::<Vec<_>>(),
                });
                (serde_json::to_vec_pretty(&v)?, "annotations.json".into())
            }
            "html" => {
                let mut b = format!("<h1>{}</h1>\n<ul>\n", crate::util::html_escape(&title));
                for (k, t, c, cite) in &lines {
                    b.push_str(&format!("<li><strong>{}</strong>: {} {}{}</li>\n", crate::util::html_escape(k), if t.is_empty() { String::new() } else { format!("“{}”", crate::util::html_escape(t)) }, crate::util::html_escape(cite), if c.is_empty() { String::new() } else { format!("<br><em>{}</em>", crate::util::html_escape(c)) }));
                }
                b.push_str("</ul>");
                (serialize::html_document(&title, &b).into_bytes(), "annotations.html".into())
            }
            "txt" => {
                let mut s = format!("{title}\n\n");
                for (k, t, c, cite) in &lines {
                    s.push_str(&format!("[{k}] {}{cite}\n", if t.is_empty() { String::new() } else { format!("“{t}” ") }));
                    if !c.is_empty() {
                        s.push_str(&format!("    {c}\n"));
                    }
                }
                (s.into_bytes(), "annotations.txt".into())
            }
            _ => {
                let mut s = format!("# {title}\n\n");
                for (k, t, c, cite) in &lines {
                    s.push_str(&format!("- **{k}**: {}{cite}\n", if t.is_empty() { String::new() } else { format!("“{t}” ") }));
                    if !c.is_empty() {
                        s.push_str(&format!("  - {c}\n"));
                    }
                }
                (s.into_bytes(), "annotations.md".into())
            }
        })
    }

    /// Versioned exchange file for other Prism research tools: selected sources (as CSL JSON
    /// with stable ids) and source-linked excerpts. This is a file export, not live sync.
    pub fn export_family_exchange(&self, project_id: &str, note_ids: &[String]) -> Result<Vec<u8>> {
        let p = self.store.must::<Project>(project_id)?;
        let notes: Vec<Note> = if note_ids.is_empty() { self.notes(project_id, false)? } else { note_ids.iter().filter_map(|id| self.store.get::<Note>(id).ok().flatten()).collect() };
        let ref_ids: Vec<String> = notes.iter().flat_map(|n| n.ref_ids.clone()).collect();
        let refs = self.references(&ref_ids)?;
        let v = serde_json::json!({
            "manifest": {
                "format": "prism-family-exchange",
                "version": "1.0",
                "producer": "Prism Reference Manager",
                "producer_version": env!("CARGO_PKG_VERSION"),
                "created": now(),
                "library_id": self.store.meta("library_id"),
                "project": {"id": p.id, "title": p.title},
                "note": "Reference Manager owns bibliographic identity and citation rendering. Receivers keep source ids and return links as separate data.",
            },
            "sources": refs.iter().map(|r| serde_json::json!({"id": r.id, "citekey": r.citekey, "csl": crate::formats::csljson::to_csl(r)})).collect::<Vec<_>>(),
            "excerpts": notes.iter().map(|n| serde_json::json!({
                "id": n.id, "type": n.kind, "source_ids": n.ref_ids, "exact_text": n.exact_text, "exact_text_edited": n.exact_text_edited,
                "user_text": n.body, "locator": {"label": n.locator_label, "value": n.locator}, "anchor_state": n.anchor_state, "origin": n.origin, "annotation_id": n.annotation_id
            })).collect::<Vec<_>>(),
        });
        Ok(serde_json::to_vec_pretty(&v)?)
    }
}
