//! Projects, project membership and manuscripts.

use super::refs::{add_membership, short};
use super::Library;
use crate::error::{Error, Result};
use crate::model::*;
use crate::util::{new_id, now};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    pub project: Project,
    pub sources: usize,
    pub unread: usize,
    pub metadata_review: usize,
    pub changed_copies: usize,
    pub notes: usize,
    pub manuscripts: Vec<Manuscript>,
    pub active_manuscript: Option<Manuscript>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CopyOptions {
    pub title: String,
    #[serde(default = "yes")]
    pub membership: bool,
    #[serde(default)]
    pub notes: bool,
    #[serde(default)]
    pub manuscripts: bool,
    #[serde(default)]
    pub outline: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct RemovalPreview {
    pub ref_id: String,
    pub title: String,
    pub project_notes: usize,
    pub project_annotations: usize,
    pub manuscripts_using: Vec<String>,
    pub copies: usize,
    pub other_projects: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentItem {
    pub time: String,
    pub activity: String,
    pub entity: String,
    pub entity_id: String,
    pub title: String,
    pub project: String,
}

impl Library {
    pub fn projects(&self, include_archived: bool) -> Result<Vec<ProjectSummary>> {
        let mut out = Vec::new();
        let refs: std::collections::BTreeMap<String, Reference> = crate::store::list_in::<Reference>(&self.store.conn, "WHERE trashed IS NULL", &[])?.into_iter().map(|r| (r.id.clone(), r)).collect();
        for p in self.store.all::<Project>()? {
            if p.archived && !include_archived {
                continue;
            }
            out.push(self.project_summary_with(&p, &refs)?);
        }
        out.sort_by(|a, b| b.project.modified.cmp(&a.project.modified));
        Ok(out)
    }

    fn project_summary_with(&self, p: &Project, refs: &std::collections::BTreeMap<String, Reference>) -> Result<ProjectSummary> {
        let ms: Vec<ProjectSource> = self.store.by("project_id", &p.id)?;
        let live: Vec<&ProjectSource> = ms.iter().filter(|m| refs.contains_key(&m.ref_id)).collect();
        let metadata_review = live.iter().filter(|m| refs.get(&m.ref_id).map(|r| !r.conflicts.is_empty() || r.title().is_empty() || (r.issued.is_none() && !r.absent("issued"))).unwrap_or(false)).count();
        let manuscripts: Vec<Manuscript> = self.store.by("project_id", &p.id)?;
        let mut changed = 0;
        for m in &manuscripts {
            changed += self.store.by::<CopyEvent>("manuscript_id", &m.id)?.iter().filter(|c| c.changed && matches!(c.status, CopyStatus::Copied | CopyStatus::Used)).count();
        }
        let notes = self.store.by::<Note>("project_id", &p.id)?.iter().filter(|n| n.trashed_at.is_none()).count();
        let active = manuscripts.iter().find(|m| m.id == p.active_manuscript).cloned().or_else(|| manuscripts.iter().find(|m| !m.archived).cloned());
        Ok(ProjectSummary {
            project: p.clone(),
            sources: live.len(),
            unread: live.iter().filter(|m| m.reading != "read").count(),
            metadata_review,
            changed_copies: changed,
            notes,
            manuscripts: manuscripts.into_iter().filter(|m| !m.archived).collect(),
            active_manuscript: active,
        })
    }

    pub fn project_summary(&self, id: &str) -> Result<ProjectSummary> {
        let refs: std::collections::BTreeMap<String, Reference> = crate::store::list_in::<Reference>(&self.store.conn, "WHERE trashed IS NULL", &[])?.into_iter().map(|r| (r.id.clone(), r)).collect();
        let p = self.store.must::<Project>(id)?;
        self.project_summary_with(&p, &refs)
    }

    /// Create a project with its first manuscript.
    pub fn create_project(&mut self, title: &str, default_style: &str, manuscript_title: Option<&str>) -> Result<Project> {
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::user("Give the project a title."));
        }
        let t = now();
        let style = if default_style.is_empty() { "apa-7" } else { default_style };
        let m = Manuscript {
            id: new_id(),
            project_id: String::new(),
            title: manuscript_title.unwrap_or("Untitled manuscript").to_string(),
            style: style.into(),
            locale: "en-US".into(),
            archived: false,
            sections: vec![],
            context_revision: 1,
            journal_names: String::new(),
            created: t.clone(),
            modified: t.clone(),
        };
        let p = Project { id: new_id(), title: title.into(), description: String::new(), color: "#1554D1".into(), default_style: style.into(), archived: false, policy_no_external: false, policy_no_ai: false, active_manuscript: m.id.clone(), created: t.clone(), modified: t };
        let (pp, mut mm) = (p.clone(), m);
        mm.project_id = p.id.clone();
        self.write(&format!("Create project “{}”", short(title)), |tx| {
            tx.put(&pp)?;
            tx.put(&mm)
        })?;
        Ok(p)
    }

    pub fn update_project(&mut self, mut p: Project) -> Result<Project> {
        if p.title.trim().is_empty() {
            return Err(Error::user("A project needs a title."));
        }
        p.modified = now();
        let pp = p.clone();
        self.write("Edit project", |tx| tx.put(&pp))?;
        Ok(p)
    }

    pub fn archive_project(&mut self, id: &str, archived: bool) -> Result<()> {
        self.write(if archived { "Archive project" } else { "Restore project" }, |tx| {
            let mut p = tx.must::<Project>(id)?;
            p.archived = archived;
            p.modified = now();
            tx.put(&p)
        })
    }

    /// Delete a project and what it owns (memberships, project notes, manuscripts, outline,
    /// copy ledger). Canonical sources and other projects are untouched. Undo restores it.
    pub fn delete_project(&mut self, id: &str, confirmed: bool) -> Result<()> {
        if !confirmed {
            return Err(Error::user("Deleting a project needs confirmation. Archive it instead to keep everything."));
        }
        self.write("Delete project", |tx| {
            for m in tx.by::<ProjectSource>("project_id", id)? {
                tx.delete::<ProjectSource>(&m.id)?;
            }
            for n in tx.by::<Note>("project_id", id)? {
                tx.delete::<Note>(&n.id)?;
            }
            for s in tx.by::<OutlineSection>("project_id", id)? {
                tx.delete::<OutlineSection>(&s.id)?;
            }
            for c in tx.by::<Collection>("project_id", id)? {
                tx.delete::<Collection>(&c.id)?;
            }
            for m in tx.by::<Manuscript>("project_id", id)? {
                for ms in tx.by::<ManuscriptSource>("manuscript_id", &m.id)? {
                    tx.delete::<ManuscriptSource>(&ms.id)?;
                }
                for c in tx.by::<CopyEvent>("manuscript_id", &m.id)? {
                    tx.delete::<CopyEvent>(&c.id)?;
                }
                tx.delete::<Manuscript>(&m.id)?;
            }
            for a in tx.all::<Annotation>()? {
                if a.project_id == id {
                    tx.delete::<Annotation>(&a.id)?;
                }
            }
            tx.delete::<Project>(id)
        })
    }

    /// Copy a project with explicit choices. Canonical sources are shared, never duplicated;
    /// copied notes become new project-owned notes.
    pub fn copy_project(&mut self, id: &str, opt: &CopyOptions) -> Result<Project> {
        let src = self.store.must::<Project>(id)?;
        let t = now();
        let mut p = src.clone();
        p.id = new_id();
        p.title = if opt.title.trim().is_empty() { format!("{} (copy)", src.title) } else { opt.title.trim().to_string() };
        p.created = t.clone();
        p.modified = t.clone();
        p.archived = false;
        let members: Vec<ProjectSource> = self.store.by("project_id", id)?;
        let notes: Vec<Note> = self.store.by("project_id", id)?;
        let manuscripts: Vec<Manuscript> = self.store.by("project_id", id)?;
        let sections: Vec<OutlineSection> = self.store.by("project_id", id)?;
        let pp = p.clone();
        let mut new_active = String::new();
        self.write(&format!("Copy project “{}”", short(&src.title)), |tx| {
            if opt.membership {
                for m in &members {
                    let mut nm = m.clone();
                    nm.id = ProjectSource::key(&pp.id, &m.ref_id);
                    nm.project_id = pp.id.clone();
                    nm.added = t.clone();
                    tx.put(&nm)?;
                }
            }
            let mut note_map = std::collections::BTreeMap::new();
            if opt.notes {
                for n in &notes {
                    let mut nn = n.clone();
                    nn.id = new_id();
                    nn.project_id = pp.id.clone();
                    nn.created = t.clone();
                    nn.modified = t.clone();
                    note_map.insert(n.id.clone(), nn.id.clone());
                    tx.put(&nn)?;
                }
            }
            if opt.outline {
                let mut sec_map = std::collections::BTreeMap::new();
                for s in &sections {
                    sec_map.insert(s.id.clone(), new_id());
                }
                for s in &sections {
                    let mut ns = s.clone();
                    ns.id = sec_map[&s.id].clone();
                    ns.project_id = pp.id.clone();
                    ns.parent_id = sec_map.get(&s.parent_id).cloned().unwrap_or_default();
                    ns.note_ids = s.note_ids.iter().filter_map(|n| note_map.get(n).cloned()).collect();
                    tx.put(&ns)?;
                }
            }
            let mut created_any = false;
            if opt.manuscripts {
                for m in &manuscripts {
                    let mut nm = m.clone();
                    nm.id = new_id();
                    nm.project_id = pp.id.clone();
                    nm.created = t.clone();
                    nm.modified = t.clone();
                    if m.id == src.active_manuscript {
                        new_active = nm.id.clone();
                    }
                    for s in tx.by::<ManuscriptSource>("manuscript_id", &m.id)? {
                        let mut ns = s.clone();
                        ns.id = ManuscriptSource::key(&nm.id, &s.ref_id);
                        ns.manuscript_id = nm.id.clone();
                        // Copy history belongs to the original manuscript.
                        if ns.state == UseState::Copied {
                            ns.state = UseState::Candidate;
                        }
                        tx.put(&ns)?;
                    }
                    tx.put(&nm)?;
                    created_any = true;
                }
            }
            if !created_any {
                let m = Manuscript { id: new_id(), project_id: pp.id.clone(), title: "Untitled manuscript".into(), style: pp.default_style.clone(), locale: "en-US".into(), archived: false, sections: vec![], context_revision: 1, journal_names: String::new(), created: t.clone(), modified: t.clone() };
                new_active = m.id.clone();
                tx.put(&m)?;
            }
            let mut p2 = pp.clone();
            p2.active_manuscript = new_active.clone();
            tx.put(&p2)
        })?;
        self.store.must::<Project>(&p.id)
    }

    pub fn add_to_project(&mut self, project_id: &str, ids: &[String]) -> Result<usize> {
        self.store.must::<Project>(project_id)?;
        let mut n = 0;
        self.write(&format!("Add {} source{} to project", ids.len(), if ids.len() == 1 { "" } else { "s" }), |tx| {
            for id in ids {
                if add_membership(tx, project_id, id)? {
                    n += 1;
                }
            }
            Ok(())
        })?;
        Ok(n)
    }

    /// Consequences of removing a source from a project, shown before the change.
    pub fn removal_preview(&self, project_id: &str, ref_id: &str) -> Result<RemovalPreview> {
        let r = self.store.must::<Reference>(ref_id)?;
        let notes = self.store.by::<Note>("project_id", project_id)?.iter().filter(|n| n.ref_ids.iter().any(|x| x == ref_id)).count();
        let atts: Vec<Attachment> = self.store.by("ref_id", ref_id)?;
        let mut ann = 0;
        for a in &atts {
            ann += self.store.by::<Annotation>("attachment_id", &a.id)?.iter().filter(|x| x.project_id == project_id).count();
        }
        let mut uses = Vec::new();
        let mut copies = 0;
        for m in self.store.by::<Manuscript>("project_id", project_id)? {
            if let Some(ms) = self.store.get::<ManuscriptSource>(&ManuscriptSource::key(&m.id, ref_id))? {
                if ms.state != UseState::Excluded {
                    uses.push(m.title.clone());
                }
            }
            copies += self.store.by::<CopyEvent>("manuscript_id", &m.id)?.iter().filter(|c| c.items.iter().any(|i| i.ref_id == ref_id)).count();
        }
        let others = self
            .store
            .by::<ProjectSource>("ref_id", ref_id)?
            .into_iter()
            .filter(|m| m.project_id != project_id)
            .filter_map(|m| self.store.get::<Project>(&m.project_id).ok().flatten().map(|p| p.title))
            .collect();
        Ok(RemovalPreview { ref_id: ref_id.into(), title: r.title().into(), project_notes: notes, project_annotations: ann, manuscripts_using: uses, copies, other_projects: others })
    }

    /// Remove membership only. The source, its files and other projects' notes stay.
    pub fn remove_from_project(&mut self, project_id: &str, ids: &[String]) -> Result<usize> {
        let mut n = 0;
        self.write("Remove from project", |tx| {
            for id in ids {
                let key = ProjectSource::key(project_id, id);
                if tx.get::<ProjectSource>(&key)?.is_some() {
                    tx.delete::<ProjectSource>(&key)?;
                    n += 1;
                }
                for m in tx.by::<Manuscript>("project_id", project_id)? {
                    let k = ManuscriptSource::key(&m.id, id);
                    if let Some(mut ms) = tx.get::<ManuscriptSource>(&k)? {
                        ms.state = UseState::Excluded;
                        ms.modified = now();
                        tx.put(&ms)?;
                    }
                }
            }
            Ok(())
        })?;
        Ok(n)
    }

    pub fn update_membership(&mut self, m: ProjectSource) -> Result<()> {
        self.write("Update project source", |tx| tx.put(&m))
    }

    pub fn membership(&self, project_id: &str, ref_id: &str) -> Result<Option<ProjectSource>> {
        self.store.get(&ProjectSource::key(project_id, ref_id))
    }

    /// Add reading time (a convenience record, not an undo step).
    pub fn add_reading_time(&mut self, project_id: &str, ref_id: &str, secs: u64) -> Result<u64> {
        let key = ProjectSource::key(project_id, ref_id);
        let Some(mut m) = self.store.get::<ProjectSource>(&key)? else { return Ok(0) };
        m.time_spent_secs += secs.min(3600);
        let s = crate::util::canonical_json(&serde_json::to_value(&m)?);
        self.store.conn.execute("UPDATE project_sources SET data=?1 WHERE id=?2", rusqlite::params![s, key])?;
        Ok(m.time_spent_secs)
    }

    pub fn set_reading_position(&mut self, project_id: &str, ref_id: &str, pos: ReadingPosition) -> Result<()> {
        // Reading position is a convenience and is not an undo step.
        let key = ProjectSource::key(project_id, ref_id);
        if let Some(mut m) = self.store.get::<ProjectSource>(&key)? {
            m.reading_position = Some(pos);
            if m.reading == "unread" || m.reading.is_empty() {
                m.reading = "reading".into();
            }
            let s = crate::util::canonical_json(&serde_json::to_value(&m)?);
            self.store.conn.execute("UPDATE project_sources SET data=?1 WHERE id=?2", rusqlite::params![s, key])?;
        }
        Ok(())
    }

    // ---- manuscripts --------------------------------------------------------------------------

    pub fn create_manuscript(&mut self, project_id: &str, title: &str, style: &str, locale: &str) -> Result<Manuscript> {
        let p = self.store.must::<Project>(project_id)?;
        let t = now();
        let m = Manuscript {
            id: new_id(),
            project_id: project_id.into(),
            title: if title.trim().is_empty() { "Untitled manuscript".into() } else { title.trim().into() },
            style: if style.is_empty() { p.default_style.clone() } else { style.into() },
            locale: if locale.is_empty() { "en-US".into() } else { locale.into() },
            archived: false,
            sections: vec![],
            context_revision: 1,
            journal_names: String::new(),
            created: t.clone(),
            modified: t,
        };
        let mm = m.clone();
        self.write("Create manuscript", |tx| tx.put(&mm))?;
        Ok(m)
    }

    /// Update a manuscript. A style or locale change advances the context revision and
    /// re-checks copied output.
    pub fn update_manuscript(&mut self, mut m: Manuscript) -> Result<Manuscript> {
        let old = self.store.must::<Manuscript>(&m.id)?;
        let output_changes = old.style != m.style || old.locale != m.locale || old.journal_names != m.journal_names;
        if output_changes {
            m.context_revision = old.context_revision + 1;
        }
        m.modified = now();
        let mm = m.clone();
        self.write("Edit manuscript", |tx| tx.put(&mm))?;
        if output_changes {
            self.recompute_copies(Some(&m.id))?;
        }
        Ok(m)
    }

    pub fn set_active_manuscript(&mut self, project_id: &str, manuscript_id: &str) -> Result<()> {
        let mut p = self.store.must::<Project>(project_id)?;
        p.active_manuscript = manuscript_id.into();
        let s = crate::util::canonical_json(&serde_json::to_value(&p)?);
        self.store.conn.execute("UPDATE projects SET data=?1 WHERE id=?2", rusqlite::params![s, project_id])?;
        Ok(())
    }

    pub fn manuscript_sources(&self, manuscript_id: &str) -> Result<Vec<ManuscriptSource>> {
        self.store.by("manuscript_id", manuscript_id)
    }

    /// Set a source's use state in a manuscript (candidate, confirmed, excluded). Adding to the
    /// set changes disambiguation, so the context revision advances and copies are re-checked.
    pub fn set_manuscript_use(&mut self, manuscript_id: &str, ref_ids: &[String], state: UseState) -> Result<()> {
        let mut m = self.store.must::<Manuscript>(manuscript_id)?;
        let mut set_changed = false;
        let t = now();
        let mut entries = Vec::new();
        for id in ref_ids {
            let key = ManuscriptSource::key(manuscript_id, id);
            let old = self.store.get::<ManuscriptSource>(&key)?;
            let was_in = old.as_ref().map(|o| o.state != UseState::Excluded).unwrap_or(false);
            let now_in = state != UseState::Excluded;
            if was_in != now_in {
                set_changed = true;
            }
            let mut ms = old.unwrap_or(ManuscriptSource { id: key, manuscript_id: manuscript_id.into(), ref_id: id.clone(), state: UseState::Candidate, section: String::new(), added: t.clone(), modified: t.clone() });
            ms.state = state.clone();
            ms.modified = t.clone();
            entries.push(ms);
        }
        if set_changed {
            m.context_revision += 1;
            m.modified = t.clone();
        }
        let label = match state {
            UseState::Confirmed => "Confirm use in manuscript",
            UseState::Excluded => "Remove from manuscript",
            _ => "Add to manuscript",
        };
        self.write(label, |tx| {
            for e in &entries {
                tx.put(e)?;
            }
            if set_changed {
                tx.put(&m)?;
            }
            Ok(())
        })?;
        if set_changed {
            self.recompute_copies(Some(manuscript_id))?;
        }
        Ok(())
    }

    /// Recent activity across the library from the change log, newest first.
    pub fn recent_work(&self, limit: usize) -> Result<Vec<RecentItem>> {
        let mut st = self.store.conn.prepare("SELECT time, tbl, entity_id, before IS NULL, after FROM events WHERE tbl IN ('refs','notes','annotations','copy_events','project_sources') ORDER BY seq DESC LIMIT 2000")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?, r.get::<_, Option<String>>(4)?)))?;
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            let (time, tbl, id, created, after) = row?;
            let Some(after) = after else { continue };
            let v: serde_json::Value = serde_json::from_str(&after).unwrap_or_default();
            let (activity, ref_id, project) = match tbl.as_str() {
                "refs" => (if created { "Added" } else { "Edited" }, id.clone(), String::new()),
                "notes" => ("Added note", v["ref_ids"].get(0).and_then(|x| x.as_str()).unwrap_or("").to_string(), v["project_id"].as_str().unwrap_or("").to_string()),
                "annotations" => ("Marked", self.store.get::<Attachment>(v["attachment_id"].as_str().unwrap_or(""))?.map(|a| a.ref_id).unwrap_or_default(), v["project_id"].as_str().unwrap_or("").to_string()),
                "copy_events" => ("Copied citation", v["items"].get(0).and_then(|i| i["ref_id"].as_str()).unwrap_or("").to_string(), v["project_id"].as_str().unwrap_or("").to_string()),
                "project_sources" => (if created { "Added to project" } else { "Read" }, v["ref_id"].as_str().unwrap_or("").to_string(), v["project_id"].as_str().unwrap_or("").to_string()),
                _ => continue,
            };
            if ref_id.is_empty() || !seen.insert(ref_id.clone()) {
                continue;
            }
            let Some(r) = self.store.get::<Reference>(&ref_id)? else { continue };
            if r.trashed_at.is_some() {
                continue;
            }
            let project_title = if project.is_empty() {
                self.store.by::<ProjectSource>("ref_id", &ref_id)?.first().and_then(|m| self.store.get::<Project>(&m.project_id).ok().flatten()).map(|p| p.title).unwrap_or_default()
            } else {
                self.store.get::<Project>(&project)?.map(|p| p.title).unwrap_or_default()
            };
            out.push(RecentItem { time, activity: activity.into(), entity: tbl, entity_id: id, title: r.title().into(), project: project_title });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Sources marked to read, with high priority first, across active projects.
    pub fn reading_queue(&self, limit: usize) -> Result<Vec<serde_json::Value>> {
        let mut out = Vec::new();
        for p in self.store.all::<Project>()?.into_iter().filter(|p| !p.archived) {
            for m in self.store.by::<ProjectSource>("project_id", &p.id)? {
                if m.reading == "read" {
                    continue;
                }
                if m.priority != "high" && m.reading != "reading" {
                    continue;
                }
                if let Some(r) = self.store.get::<Reference>(&m.ref_id)? {
                    if r.trashed_at.is_none() {
                        out.push(serde_json::json!({"ref_id": r.id, "title": r.title(), "project_id": p.id, "project": p.title, "priority": m.priority, "reading": m.reading}));
                    }
                }
            }
        }
        out.sort_by_key(|v| if v["priority"] == "high" { 0 } else { 1 });
        out.truncate(limit);
        Ok(out)
    }
}
