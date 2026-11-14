//! Reference operations: list, add, edit with provenance, trash, merge with aliases, versions,
//! duplicates, bulk edits with preview, find and replace, labels and collections.

use super::Library;
use crate::error::{Error, Result};
use crate::model::*;
use crate::util::{new_id, now};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize)]
pub struct RefRow {
    pub id: String,
    pub rtype: String,
    pub type_label: String,
    pub authors: String,
    pub year: String,
    pub title: String,
    pub container: String,
    pub citekey: String,
    pub has_pdf: bool,
    pub reading: String,
    pub inclusion: String,
    pub rating: u8,
    pub labels: Vec<String>,
    pub modified: String,
    pub created: String,
    pub issues: usize,
    pub notices: usize,
    pub origin: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListQuery {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub collection_id: String,
    /// all, trash, recent_added, recent_modified, recent_read, inbox, needs_metadata, to_read
    #[serde(default)]
    pub view: String,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub desc: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub rtype: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: usize,
    /// Quick filters, combined with AND. Each can be inverted.
    #[serde(default)]
    pub filters: Vec<QuickFilter>,
    /// Also count the values of each quick-filter field over the filtered result.
    #[serde(default)]
    pub facets: bool,
}

/// One quick filter: `field` is type, year, label, pdf or reading; `value` is the value to match.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct QuickFilter {
    pub field: String,
    pub value: String,
    #[serde(default)]
    pub negate: bool,
}

/// A value of a quick-filter field and how many sources in the current result have it.
#[derive(Debug, Clone, Serialize)]
pub struct FacetValue {
    pub value: String,
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefPage {
    pub total: usize,
    pub rows: Vec<RefRow>,
    pub pending_index: i64,
    pub search_hits: Vec<crate::search::Hit>,
    /// Words that matched nothing and the close spellings searched instead.
    pub corrections: Vec<crate::search::Correction>,
    /// field -> values with counts, when requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facets: Option<BTreeMap<String, Vec<FacetValue>>>,
}

pub fn author_short(r: &Reference) -> String {
    let names: Vec<String> = r.creators.iter().filter(|c| c.role == "author").map(|c| c.name.surname()).collect();
    let names = if names.is_empty() { r.creators.iter().filter(|c| c.role == "editor").map(|c| c.name.surname()).collect() } else { names };
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2 => format!("{} & {}", names[0], names[1]),
        _ => format!("{} et al.", names[0]),
    }
}

/// Fields compared when deciding if an edit changes citation output or only metadata.
pub fn changed_fields(a: &Reference, b: &Reference) -> Vec<String> {
    let mut out = Vec::new();
    let keys: BTreeSet<&String> = a.fields.keys().chain(b.fields.keys()).collect();
    for k in keys {
        if a.fields.get(k) != b.fields.get(k) {
            out.push(k.clone());
        }
    }
    if a.creators != b.creators {
        out.push("creators".into());
    }
    if a.issued != b.issued {
        out.push("issued".into());
    }
    if a.rtype != b.rtype {
        out.push("type".into());
    }
    if a.original_date != b.original_date {
        out.push("original_date".into());
    }
    if a.accessed != b.accessed {
        out.push("accessed".into());
    }
    if a.tags != b.tags {
        out.push("tags".into());
    }
    if a.labels != b.labels {
        out.push("labels".into());
    }
    if a.citekey != b.citekey {
        out.push("citekey".into());
    }
    if a.confirmed_absent != b.confirmed_absent {
        out.push("confirmed_absent".into());
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct Impact {
    pub projects: Vec<(String, String)>,
    pub manuscripts: Vec<(String, String)>,
    pub copies: usize,
    pub notes: usize,
    pub annotations: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BulkOp {
    /// set, append, prefix, clear, copy, move, rename
    pub op: String,
    pub field: String,
    #[serde(default)]
    pub value: String,
    /// Target field for copy, move and rename.
    #[serde(default)]
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BulkChange {
    pub ref_id: String,
    pub title: String,
    pub field: String,
    pub before: String,
    pub after: String,
}

impl Library {
    pub fn list_references(&self, q: &ListQuery) -> Result<RefPage> {
        // Filtering, sorting and paging run in SQL over the sort-key table; only the rows on the
        // requested page are read in full.
        let conn = &self.store.conn;
        let mut wh: Vec<String> = vec![format!("x.trashed={}", if q.view == "trash" { 1 } else { 0 })];
        let mut args: Vec<String> = Vec::new();
        fn bind(v: &str, args: &mut Vec<String>) -> String {
            args.push(v.to_string());
            format!("?{}", args.len())
        }
        if !q.project_id.is_empty() && q.view != "trash" {
            let p = bind(&q.project_id, &mut args);
            wh.push(format!("x.id IN (SELECT ref_id FROM project_sources WHERE project_id={p})"));
        }
        if !q.label.is_empty() {
            let p = bind(&q.label, &mut args);
            wh.push(format!("x.id IN (SELECT refs.id FROM refs, json_each(refs.data,'$.labels') j WHERE j.value={p})"));
        }
        if !q.rtype.is_empty() {
            let p = bind(&q.rtype, &mut args);
            wh.push(format!("x.rtype={p}"));
        }
        let since = (chrono::Utc::now() - chrono::Duration::days(14)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        match q.view.as_str() {
            "recent_added" => {
                let p = bind(&since, &mut args);
                wh.push(format!("x.created >= {p}"));
            }
            "recent_modified" => {
                let p = bind(&since, &mut args);
                wh.push(format!("x.modified >= {p}"));
            }
            "needs_metadata" => wh.push("x.needs_meta=1".into()),
            "machine" => wh.push("x.origin IN ('ai','agent')".into()),
            "to_read" => {
                let p = bind(&q.project_id, &mut args);
                wh.push(format!("x.id IN (SELECT ref_id FROM project_sources WHERE project_id={p} AND COALESCE(json_extract(data,'$.reading'),'')!='read')"));
            }
            "recent_read" => {
                let p = bind(&q.project_id, &mut args);
                wh.push(format!("x.id IN (SELECT ref_id FROM project_sources WHERE project_id={p} AND json_extract(data,'$.reading_position') IS NOT NULL)"));
            }
            _ => {}
        }
        // Quick filters.
        const PDF_SQL: &str = "SELECT ref_id FROM attachments WHERE json_extract(data,'$.mime')='application/pdf' AND COALESCE(json_extract(data,'$.superseded'),0)=0";
        for f in &q.filters {
            let clause = match f.field.as_str() {
                "type" => {
                    let p = bind(&f.value, &mut args);
                    format!("x.rtype={p}")
                }
                "year" => {
                    if f.value == "none" {
                        "x.year_key NOT GLOB '[0-9]*'".to_string()
                    } else {
                        let p = bind(&f.value, &mut args);
                        format!("CAST(x.year_key AS INTEGER)=CAST({p} AS INTEGER) AND x.year_key GLOB '[0-9]*'")
                    }
                }
                "label" => {
                    let p = bind(&f.value, &mut args);
                    format!("x.id IN (SELECT refs.id FROM refs, json_each(refs.data,'$.labels') j WHERE j.value={p})")
                }
                "pdf" => format!("x.id {} ({PDF_SQL})", if f.value == "yes" { "IN" } else { "NOT IN" }),
                "reading" if !q.project_id.is_empty() => {
                    let pr = bind(&q.project_id, &mut args);
                    let p = bind(&f.value, &mut args);
                    format!("x.id IN (SELECT ref_id FROM project_sources WHERE project_id={pr} AND COALESCE(json_extract(data,'$.reading'),'unread')={p})")
                }
                _ => continue,
            };
            wh.push(if f.negate { format!("NOT ({clause})") } else { clause });
        }
        // Collections and searches narrow the list to an explicit set held in a temporary table.
        let mut id_set: Option<BTreeSet<String>> = None;
        if !q.collection_id.is_empty() {
            let c = self.store.must::<Collection>(&q.collection_id)?;
            let ids: BTreeSet<String> = if !c.query.is_empty() {
                crate::search::run_ids(conn, &c.query, None, None, usize::MAX)?.ref_ids.into_iter().collect()
            } else {
                let mut ids: BTreeSet<String> = c.ref_ids.iter().cloned().collect();
                let all: Vec<Collection> = self.store.all()?;
                let mut stack = vec![c.id.clone()];
                while let Some(p) = stack.pop() {
                    for ch in all.iter().filter(|x| x.parent_id == p) {
                        ids.extend(ch.ref_ids.iter().cloned());
                        stack.push(ch.id.clone());
                    }
                }
                ids
            };
            id_set = Some(ids);
        }
        let mut hits = Vec::new();
        let mut corrections = vec![];
        let mut search_res: Option<crate::search::Results> = None;
        let mut scores: HashMap<String, f64> = HashMap::new();
        if !q.search.trim().is_empty() {
            let res = crate::search::run_ids(conn, &q.search, None, id_set.as_ref(), usize::MAX)?;
            id_set = Some(res.ref_ids.iter().cloned().collect());
            scores = res.scores.clone();
            corrections = res.corrections.clone();
            search_res = Some(res);
        }
        if let Some(ids) = &id_set {
            conn.execute_batch("DROP TABLE IF EXISTS temp.list_ids; CREATE TEMP TABLE list_ids(id TEXT PRIMARY KEY, score REAL NOT NULL DEFAULT 0);")?;
            let mut st = conn.prepare_cached("INSERT OR IGNORE INTO list_ids(id, score) VALUES(?1, ?2)")?;
            for id in ids {
                st.execute(rusqlite::params![id, scores.get(id).copied().unwrap_or(0.0)])?;
            }
            wh.push("x.id IN (SELECT id FROM list_ids)".into());
        }
        // In a project the list shows the project's rating (falling back to the library rating),
        // so sort by the same value.
        let project_rating;
        let col = match q.sort.as_str() {
            "rating" if !q.project_id.is_empty() => {
                // A quoted literal, not a bound parameter: the count query shares `args`.
                let p = format!("'{}'", q.project_id.replace('\'', "''"));
                project_rating = format!("COALESCE(NULLIF((SELECT CAST(json_extract(ps.data,'$.rating') AS INTEGER) FROM project_sources ps WHERE ps.project_id={p} AND ps.ref_id=x.id), 0), x.rating)");
                project_rating.as_str()
            }
            // Relevance applies while searching; without a search it falls back to author order.
            "relevance" if !q.search.trim().is_empty() => "(SELECT -score FROM list_ids l WHERE l.id=x.id)",
            "year" => "x.year_key",
            "title" => "x.title_key",
            "container" => "x.container_key",
            "modified" => "x.modified",
            "created" => "x.created",
            "type" => "x.type_label",
            "rating" => "x.rating",
            "citekey" => "x.citekey",
            _ => "x.author_key",
        };
        let dir = if q.desc { "DESC" } else { "ASC" };
        // Stable tiebreakers keep selection stable through sorting and refresh.
        let order = if col == "x.title_key" { format!("x.title_key {dir}, x.id {dir}") } else { format!("{col} {dir}, x.title_key {dir}, x.id {dir}") };
        let w = wh.join(" AND ");
        let total: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM ref_index x WHERE {w}"), rusqlite::params_from_iter(args.iter()), |r| r.get(0))?;
        let facets = if q.facets { Some(self.facets(&w, &args, &q.project_id)?) } else { None };
        let limit = if q.limit == 0 { 500 } else { q.limit.min(1_000_000) };
        let page_ids: Vec<String> = {
            let mut st = conn.prepare(&format!("SELECT x.id FROM ref_index x WHERE {w} ORDER BY {order} LIMIT {limit} OFFSET {}", q.offset))?;
            let v: Vec<String> = st.query_map(rusqlite::params_from_iter(args.iter()), |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).collect();
            v
        };
        let custom_labels: BTreeMap<String, String> = self.store.all::<crate::model::CustomType>()?.into_iter().map(|t| (t.id, t.label)).collect();
        let mut rows = Vec::with_capacity(page_ids.len());
        for id in &page_ids {
            let Some(r) = self.store.get::<Reference>(id)? else { continue };
            let m = if q.project_id.is_empty() { None } else { self.store.get::<ProjectSource>(&ProjectSource::key(&q.project_id, id))? };
            let has_pdf = conn
                .prepare_cached("SELECT 1 FROM attachments WHERE ref_id=?1 AND json_extract(data,'$.mime')='application/pdf' AND COALESCE(json_extract(data,'$.superseded'),0)=0 LIMIT 1")?
                .exists([id])?;
            rows.push(RefRow {
                id: r.id.clone(),
                rtype: r.rtype.clone(),
                type_label: super::custom::type_label(&r, &custom_labels),
                authors: author_short(&r),
                year: r.issued.as_ref().map(|d| d.year.map(|y| y.to_string()).unwrap_or(d.status.clone())).unwrap_or_default(),
                title: r.title().to_string(),
                container: r.f("container_title").to_string(),
                citekey: r.citekey.clone(),
                has_pdf,
                reading: m.as_ref().map(|m| m.reading.clone()).unwrap_or_default(),
                inclusion: m.as_ref().map(|m| m.inclusion.clone()).unwrap_or_default(),
                rating: m.as_ref().map(|m| m.rating).filter(|x| *x > 0).unwrap_or(r.rating),
                labels: r.labels.clone(),
                modified: r.modified.clone(),
                created: r.created.clone(),
                issues: r.conflicts.len() + usize::from(r.title().is_empty()) + usize::from(!Library::values_to_check(&r).is_empty()),
                notices: r.notices.len(),
                origin: r.origin.clone(),
            });
        }
        if !q.search.trim().is_empty() {
            hits = match &search_res { Some(r) => crate::search::hits_from(conn, r, &page_ids, None)?, None => crate::search::hits_for(conn, &q.search, &page_ids, None)? };
        }
        Ok(RefPage { total: total as usize, rows, pending_index: crate::search::pending_index_count(conn)?, search_hits: hits, facets, corrections })
    }

    /// Counts of each quick-filter value over the rows matching `w`.
    fn facets(&self, w: &str, args: &[String], project_id: &str) -> Result<BTreeMap<String, Vec<FacetValue>>> {
        let conn = &self.store.conn;
        let mut out = BTreeMap::new();
        let run = |sql: &str| -> Result<Vec<(String, i64)>> {
            let mut st = conn.prepare(sql)?;
            let v = st.query_map(rusqlite::params_from_iter(args.iter()), |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, i64>(1)?)))?.filter_map(|r| r.ok()).collect();
            Ok(v)
        };
        let types = run(&format!("SELECT x.rtype, COUNT(*) FROM ref_index x WHERE {w} GROUP BY x.rtype ORDER BY COUNT(*) DESC"))?;
        out.insert("type".to_string(), types.into_iter().map(|(v, c)| FacetValue { label: crate::types::label(&v).to_string(), value: v, count: c }).collect());
        let years = run(&format!("SELECT CASE WHEN x.year_key GLOB '[0-9]*' THEN CAST(CAST(x.year_key AS INTEGER) AS TEXT) ELSE 'none' END AS y, COUNT(*) FROM ref_index x WHERE {w} GROUP BY y ORDER BY y DESC"))?;
        out.insert("year".to_string(), years.into_iter().map(|(v, c)| FacetValue { label: if v == "none" { "No year".into() } else { v.clone() }, value: v, count: c }).collect());
        let labels = run(&format!("SELECT j.value, COUNT(*) FROM refs, json_each(refs.data,'$.labels') j WHERE refs.id IN (SELECT x.id FROM ref_index x WHERE {w}) GROUP BY j.value ORDER BY COUNT(*) DESC, j.value"))?;
        out.insert("label".to_string(), labels.into_iter().map(|(v, c)| FacetValue { label: v.clone(), value: v, count: c }).collect());
        let pdf = run(&format!("SELECT CASE WHEN x.id IN (SELECT ref_id FROM attachments WHERE json_extract(data,'$.mime')='application/pdf' AND COALESCE(json_extract(data,'$.superseded'),0)=0) THEN 'yes' ELSE 'no' END AS p, COUNT(*) FROM ref_index x WHERE {w} GROUP BY p ORDER BY p DESC"))?;
        out.insert("pdf".to_string(), pdf.into_iter().map(|(v, c)| FacetValue { label: if v == "yes" { "Has a PDF".into() } else { "No PDF".into() }, value: v, count: c }).collect());
        if !project_id.is_empty() {
            let pid = project_id.replace('\'', "''");
            let reading = run(&format!("SELECT COALESCE((SELECT json_extract(ps.data,'$.reading') FROM project_sources ps WHERE ps.project_id='{pid}' AND ps.ref_id=x.id),'unread') AS rd, COUNT(*) FROM ref_index x WHERE {w} GROUP BY rd ORDER BY rd"))?;
            out.insert("reading".to_string(), reading.into_iter().map(|(v, c)| FacetValue { label: match v.as_str() { "read" => "Read".into(), "reading" => "Reading".into(), "unread" | "" => "Not read".into(), o => o.to_string() }, value: if v.is_empty() { "unread".into() } else { v }, count: c }).collect());
        }
        Ok(out)
    }

    fn taken_keys(&self) -> Result<BTreeSet<String>> {
        let mut st = self.store.conn.prepare("SELECT json_extract(data,'$.citekey') FROM refs")?;
        let v: BTreeSet<String> = st.query_map([], |r| r.get::<_, Option<String>>(0))?.filter_map(|r| r.ok().flatten()).collect();
        Ok(v)
    }

    pub fn prepare_new(&self, r: &mut Reference, source: &str) -> Result<()> {
        if r.id.is_empty() {
            r.id = new_id();
        }
        let t = now();
        r.created = t.clone();
        r.modified = t.clone();
        r.revision = 1;
        if r.origin.is_empty() {
            r.origin = "user".into();
        }
        crate::formats::finish(r);
        crate::cleanup::apply_rules(r, &self.save_rules, &self.protected_terms);
        self.apply_custom_defaults(r)?;
        self.check_custom(r)?;
        let taken = self.taken_keys()?;
        if r.citekey.is_empty() || taken.contains(&r.citekey) {
            let base = if r.citekey.is_empty() { crate::citekey::generate(r, &self.citekey_pattern) } else { r.citekey.clone() };
            r.citekey = crate::citekey::unique(&base, &taken);
        }
        let fields: Vec<String> = r.fields.keys().cloned().chain(["creators".to_string(), "issued".to_string()]).collect();
        for f in fields {
            r.provenance.entry(f).or_insert(Provenance { source: source.into(), time: t.clone(), user_edited: source == "user", confidence: None, review: if source == "user" { "reviewed".into() } else { "unreviewed".into() } });
        }
        Ok(())
    }

    /// Add a new reference, optionally as a member of a project.
    pub fn add_reference(&mut self, mut r: Reference, project_id: Option<&str>) -> Result<Reference> {
        self.prepare_new(&mut r, "user")?;
        let pid = project_id.unwrap_or("").to_string();
        let rr = r.clone();
        self.write(&format!("Add “{}”", short(&r.title().to_string())), |tx| {
            tx.put(&rr)?;
            if !pid.is_empty() {
                add_membership(tx, &pid, &rr.id)?;
            }
            Ok(())
        })?;
        if r.has("container_title") && r.has("container_title_short") {
            let _ = crate::journals::put(&self.store.conn, r.f("container_title"), r.f("container_title_short"), if r.origin == "user" { "user" } else { "records" });
        }
        Ok(r)
    }

    /// Projects, manuscripts and copies that use a reference (shown before a shared edit).
    pub fn impact(&self, ref_id: &str) -> Result<Impact> {
        let ps: Vec<ProjectSource> = self.store.by("ref_id", ref_id)?;
        let mut projects = Vec::new();
        for p in &ps {
            if let Some(pr) = self.store.get::<Project>(&p.project_id)? {
                projects.push((pr.id, pr.title));
            }
        }
        let ms: Vec<ManuscriptSource> = self.store.by("ref_id", ref_id)?;
        let mut manuscripts = Vec::new();
        for m in &ms {
            if let Some(mm) = self.store.get::<Manuscript>(&m.manuscript_id)? {
                manuscripts.push((mm.id, mm.title));
            }
        }
        let copies: i64 = self.store.conn.query_row("SELECT COUNT(*) FROM copy_events, json_each(copy_events.data,'$.items') j WHERE json_extract(j.value,'$.ref_id')=?1", [ref_id], |r| r.get(0))?;
        let notes: i64 = self.store.conn.query_row("SELECT COUNT(*) FROM notes, json_each(notes.data,'$.ref_ids') j WHERE j.value=?1", [ref_id], |r| r.get(0))?;
        let annotations: i64 = self.store.conn.query_row("SELECT COUNT(*) FROM annotations n JOIN attachments a ON a.id=n.attachment_id WHERE a.ref_id=?1", [ref_id], |r| r.get(0))?;
        Ok(Impact { projects, manuscripts, copies: copies as usize, notes: notes as usize, annotations: annotations as usize })
    }

    /// Save an edited reference. Records provenance for changed fields, bumps the revision and
    /// re-checks copied output that used it.
    /// Mark automatically filled values as checked by the person. The value, its source and its
    /// confidence stay as they were; only the review state changes.
    pub fn confirm_values(&mut self, id: &str, fields: &[String]) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(id)?;
        let mut n = 0;
        for f in fields {
            if let Some(p) = r.provenance.get_mut(f) {
                if p.review != "reviewed" {
                    p.review = "reviewed".into();
                    n += 1;
                }
            }
        }
        if n == 0 {
            return Ok(r);
        }
        let rr = r.clone();
        self.write(if n == 1 { "Mark value as checked" } else { "Mark values as checked" }, |tx| tx.put(&rr))?;
        Ok(r)
    }

    /// Automatically filled values below the review threshold that nobody has checked yet.
    pub fn values_to_check(r: &Reference) -> Vec<String> {
        r.provenance
            .iter()
            .filter(|(_, p)| !p.user_edited && p.review != "reviewed" && p.confidence.map(|c| c < crate::provenance::REVIEW_BELOW).unwrap_or(false))
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub fn update_reference(&mut self, mut r: Reference, source: &str) -> Result<Reference> {
        let old = self.store.must::<Reference>(&r.id)?;
        crate::cleanup::apply_rules(&mut r, &self.save_rules, &self.protected_terms);
        self.check_custom(&mut r)?;
        let changed = changed_fields(&old, &r);
        if changed.is_empty() && old.provenance == r.provenance && old.rating == r.rating && old.notices == r.notices && old.conflicts == r.conflicts && old.custom == r.custom && old.overflow == r.overflow && old.translations == r.translations && old.work_id == r.work_id && old.preferred_version == r.preferred_version && old.citekey_locked == r.citekey_locked {
            return Ok(old);
        }
        let t = now();
        for f in &changed {
            r.provenance.insert(f.clone(), Provenance { source: source.into(), time: t.clone(), user_edited: source == "user", confidence: None, review: if source == "user" { "reviewed".into() } else { "unreviewed".into() } });
        }
        if let Some(d) = r.fields.get("doi").cloned() {
            if let Some(n) = crate::util::normalize_doi(&d) {
                r.fields.insert("doi".into(), n);
            }
        }
        if changed.iter().any(|c| c == "citekey") {
            let mut taken = self.taken_keys()?;
            taken.remove(&old.citekey);
            if taken.contains(&r.citekey) {
                return Err(Error::user(format!("The citation key “{}” is already used by another source.", r.citekey)));
            }
        }
        r.revision = old.revision + 1;
        r.modified = t;
        r.created = old.created.clone();
        let rr = r.clone();
        self.write(&format!("Edit “{}”", short(r.title())), |tx| tx.put(&rr))?;
        self.recompute_copies(None)?;
        // An abbreviation the person types teaches the journal list.
        if changed.iter().any(|f| f == "container_title_short" || f == "container_title") && r.has("container_title") && r.has("container_title_short") {
            let _ = crate::journals::put(&self.store.conn, r.f("container_title"), r.f("container_title_short"), if source == "user" { "user" } else { "records" });
        }
        Ok(r)
    }

    pub fn regenerate_citekey(&mut self, id: &str) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(id)?;
        if r.citekey_locked {
            return Err(Error::user("This citation key is locked. Unlock it to generate a new one."));
        }
        let mut taken = self.taken_keys()?;
        taken.remove(&r.citekey);
        r.citekey = crate::citekey::unique(&crate::citekey::generate(&r, &self.citekey_pattern), &taken);
        self.update_reference(r, "rule:citekey")
    }

    /// Move references to the recoverable trash. Memberships and notes stay so restore is complete.
    pub fn trash(&mut self, ids: &[String]) -> Result<usize> {
        let t = now();
        let mut n = 0;
        self.write(&format!("Move {} source{} to trash", ids.len(), if ids.len() == 1 { "" } else { "s" }), |tx| {
            for id in ids {
                if let Some(mut r) = tx.get::<Reference>(id)? {
                    if r.trashed_at.is_none() {
                        r.trashed_at = Some(t.clone());
                        tx.put(&r)?;
                        n += 1;
                    }
                }
            }
            Ok(())
        })?;
        Ok(n)
    }

    pub fn restore(&mut self, ids: &[String]) -> Result<usize> {
        let mut n = 0;
        self.write("Restore from trash", |tx| {
            for id in ids {
                if let Some(mut r) = tx.get::<Reference>(id)? {
                    if r.trashed_at.is_some() {
                        r.trashed_at = None;
                        tx.put(&r)?;
                        n += 1;
                    }
                }
            }
            Ok(())
        })?;
        Ok(n)
    }

    /// Permanently delete trashed references and everything that belongs only to them.
    /// `confirmed` must be true: the interface asks first.
    pub fn purge(&mut self, ids: &[String], confirmed: bool) -> Result<usize> {
        if !confirmed {
            return Err(Error::user("Permanent deletion needs confirmation."));
        }
        self.safety_snapshot("delete")?;
        let mut n = 0;
        let mut files_to_check = Vec::new();
        self.write("Delete permanently", |tx| {
            for id in ids {
                let Some(r) = tx.get::<Reference>(id)? else { continue };
                if r.trashed_at.is_none() {
                    continue;
                }
                for m in tx.by::<ProjectSource>("ref_id", id)? {
                    tx.delete::<ProjectSource>(&m.id)?;
                }
                for m in tx.by::<ManuscriptSource>("ref_id", id)? {
                    tx.delete::<ManuscriptSource>(&m.id)?;
                }
                for a in tx.by::<Attachment>("ref_id", id)? {
                    for an in tx.by::<Annotation>("attachment_id", &a.id)? {
                        tx.delete::<Annotation>(&an.id)?;
                    }
                    files_to_check.push(a.clone());
                    tx.delete::<Attachment>(&a.id)?;
                }
                for mut c in tx.all::<Collection>()? {
                    if c.ref_ids.contains(id) {
                        c.ref_ids.retain(|x| x != id);
                        tx.put(&c)?;
                    }
                }
                for mut note in tx.all::<Note>()? {
                    if note.ref_ids.contains(id) {
                        note.ref_ids.retain(|x| x != id);
                        if note.ref_ids.is_empty() && !note.exact_text.is_empty() {
                            note.anchor_state = "unresolved".into();
                        }
                        tx.put(&note)?;
                    }
                }
                tx.delete::<Reference>(id)?;
                n += 1;
            }
            Ok(())
        })?;
        // Remove managed files no other attachment uses.
        for a in files_to_check {
            if a.mode == "managed" {
                let used: i64 = self.store.conn.query_row("SELECT COUNT(*) FROM attachments WHERE hash=?1", [&a.hash], |r| r.get(0))?;
                if used == 0 {
                    if let Ok(p) = self.inside_files(&a.path) {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
        }
        Ok(n)
    }

    /// Permanently delete items that have been in the trash longer than `days`.
    pub fn empty_trash_older_than(&mut self, days: i64) -> Result<usize> {
        let refs: Vec<Reference> = crate::store::list_in(&self.store.conn, "WHERE trashed IS NOT NULL", &[])?;
        let old: Vec<String> = refs
            .into_iter()
            .filter(|r| r.trashed_at.as_ref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| chrono::Utc::now().signed_duration_since(t).num_days() >= days).unwrap_or(false))
            .map(|r| r.id)
            .collect();
        if old.is_empty() {
            return Ok(0);
        }
        self.purge(&old, true)
    }

    /// Merge `others` into `target`. `choices` maps a field to the reference id whose value to
    /// keep. Memberships, notes, attachments, annotations and copy references move to the
    /// target; each retired id becomes an alias so old links still resolve.
    pub fn merge(&mut self, target: &str, others: &[String], choices: &BTreeMap<String, String>) -> Result<Reference> {
        self.safety_snapshot("merge")?;
        let mut t = self.store.must::<Reference>(target)?;
        let sources: Vec<Reference> = others.iter().filter(|o| o.as_str() != target).map(|o| self.store.must::<Reference>(o)).collect::<Result<_>>()?;
        for (field, from) in choices {
            if from == target {
                continue;
            }
            let Some(src) = sources.iter().find(|s| &s.id == from) else { continue };
            match field.as_str() {
                "creators" => t.creators = src.creators.clone(),
                "issued" => t.issued = src.issued.clone(),
                "type" => t.rtype = src.rtype.clone(),
                f => {
                    if let Some(v) = src.fields.get(f) {
                        t.fields.insert(f.into(), v.clone());
                    } else {
                        t.fields.remove(f);
                    }
                }
            }
            if let Some(p) = src.provenance.get(field) {
                t.provenance.insert(field.clone(), p.clone());
            }
        }
        // Fill gaps from the merged records without overwriting.
        for s in &sources {
            for (k, v) in &s.fields {
                t.fields.entry(k.clone()).or_insert_with(|| v.clone());
            }
            for tag in &s.tags {
                if !t.tags.contains(tag) {
                    t.tags.push(tag.clone());
                }
            }
            for l in &s.labels {
                if !t.labels.contains(l) {
                    t.labels.push(l.clone());
                }
            }
            for (k, v) in &s.overflow {
                t.overflow.entry(k.clone()).or_insert_with(|| v.clone());
            }
            t.notices.extend(s.notices.clone());
        }
        t.revision += 1;
        t.modified = now();
        let tt = t.clone();
        let ts = now();
        self.write(&format!("Merge {} record{}", sources.len() + 1, if sources.is_empty() { "" } else { "s" }), |tx| {
            tx.put(&tt)?;
            for s in &sources {
                for m in tx.by::<ProjectSource>("ref_id", &s.id)? {
                    let key = ProjectSource::key(&m.project_id, &tt.id);
                    if tx.get::<ProjectSource>(&key)?.is_none() {
                        let mut nm = m.clone();
                        nm.id = key;
                        nm.ref_id = tt.id.clone();
                        tx.put(&nm)?;
                    }
                    tx.delete::<ProjectSource>(&m.id)?;
                }
                for m in tx.by::<ManuscriptSource>("ref_id", &s.id)? {
                    let key = ManuscriptSource::key(&m.manuscript_id, &tt.id);
                    let existing = tx.get::<ManuscriptSource>(&key)?;
                    let mut nm = m.clone();
                    nm.id = key;
                    nm.ref_id = tt.id.clone();
                    // Keep the strongest use state.
                    if let Some(e) = existing {
                        if rank(&e.state) > rank(&nm.state) {
                            nm.state = e.state;
                        }
                    }
                    tx.put(&nm)?;
                    tx.delete::<ManuscriptSource>(&m.id)?;
                }
                for mut a in tx.by::<Attachment>("ref_id", &s.id)? {
                    a.ref_id = tt.id.clone();
                    tx.put(&a)?;
                }
                for mut n in tx.all::<Note>()? {
                    if n.ref_ids.contains(&s.id) {
                        n.ref_ids = n.ref_ids.iter().map(|x| if x == &s.id { tt.id.clone() } else { x.clone() }).collect();
                        n.ref_ids.dedup();
                        tx.put(&n)?;
                    }
                }
                for mut c in tx.all::<Collection>()? {
                    if c.ref_ids.contains(&s.id) {
                        c.ref_ids.retain(|x| x != &s.id);
                        if !c.ref_ids.contains(&tt.id) {
                            c.ref_ids.push(tt.id.clone());
                        }
                        tx.put(&c)?;
                    }
                }
                tx.put(&Alias { id: s.id.clone(), target: tt.id.clone(), merged_at: ts.clone() })?;
                tx.delete::<Reference>(&s.id)?;
            }
            Ok(())
        })?;
        self.recompute_copies(None)?;
        Ok(t)
    }

    /// Link records as versions of one work and choose the one to cite.
    pub fn link_versions(&mut self, ids: &[String], preferred: &str) -> Result<()> {
        let work = new_id();
        self.write("Link versions", |tx| {
            for id in ids {
                let mut r = tx.must::<Reference>(id)?;
                r.work_id = work.clone();
                r.preferred_version = r.id == preferred;
                if r.version_label.is_empty() {
                    r.version_label = if r.rtype == "preprint" { "Preprint".into() } else { "Published".into() };
                }
                tx.put(&r)?;
            }
            Ok(())
        })
    }

    pub fn unlink_version(&mut self, id: &str) -> Result<()> {
        self.write("Unlink version", |tx| {
            let mut r = tx.must::<Reference>(id)?;
            r.work_id.clear();
            r.preferred_version = false;
            tx.put(&r)
        })
    }

    pub fn duplicates(&self, threshold: f32) -> Result<Vec<crate::dedupe::Candidate>> {
        let refs: Vec<Reference> = crate::store::list_in(&self.store.conn, "WHERE trashed IS NULL", &[])?;
        let decided: BTreeSet<String> = self.store.all::<DuplicateDecision>()?.into_iter().map(|d| d.id).collect();
        let mut hashes: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for a in self.store.all::<Attachment>()? {
            hashes.entry(a.ref_id.clone()).or_default().push(a.hash.clone());
        }
        // Records already linked as versions of one work are not duplicates.
        let mut out = crate::dedupe::find(&refs, threshold, &decided, &hashes);
        let work: BTreeMap<&str, &str> = refs.iter().filter(|r| !r.work_id.is_empty()).map(|r| (r.id.as_str(), r.work_id.as_str())).collect();
        out.retain(|c| !(work.get(c.a.as_str()).is_some() && work.get(c.a.as_str()) == work.get(c.b.as_str())));
        Ok(out)
    }

    /// Record a durable duplicate decision: not_duplicate, keep_both or versions.
    pub fn decide_duplicate(&mut self, a: &str, b: &str, decision: &str) -> Result<()> {
        let d = DuplicateDecision { id: DuplicateDecision::key(a, b), a: a.into(), b: b.into(), decision: decision.into(), time: now() };
        self.write("Duplicate decision", |tx| tx.put(&d))
    }

    pub fn field_history(&self, id: &str, field: &str) -> Result<Vec<serde_json::Value>> {
        let path: Vec<&str> = match field {
            "creators" | "issued" | "type" | "citekey" | "tags" | "labels" => vec![field],
            f => vec!["fields", f],
        };
        let path: Vec<&str> = path.into_iter().map(|p| if p == "type" { "type" } else { p }).collect();
        self.store.field_history("refs", id, &path)
    }

    /// Restore an earlier value of one field.
    pub fn restore_field(&mut self, id: &str, field: &str, value: serde_json::Value) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(id)?;
        match field {
            "creators" => r.creators = serde_json::from_value(value)?,
            "issued" => r.issued = serde_json::from_value(value)?,
            f => match value.as_str() {
                Some(s) => {
                    r.fields.insert(f.into(), s.into());
                }
                None => {
                    r.fields.remove(f);
                }
            },
        }
        self.update_reference(r, "user")
    }

    // ---- bulk edits -----------------------------------------------------------------------

    fn bulk_apply(r: &mut Reference, op: &BulkOp) -> Option<(String, String)> {
        let get = |r: &Reference, f: &str| -> String {
            match f {
                "tags" => r.tags.join("; "),
                "type" => r.rtype.clone(),
                f => r.fields.get(f).cloned().unwrap_or_default(),
            }
        };
        let set = |r: &mut Reference, f: &str, v: String| match f {
            "tags" => r.tags = v.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            "type" => r.rtype = v,
            f => {
                if v.is_empty() {
                    r.fields.remove(f);
                } else {
                    r.fields.insert(f.into(), v);
                }
            }
        };
        let before = get(r, &op.field);
        match op.op.as_str() {
            "set" => set(r, &op.field, op.value.clone()),
            "append" => set(r, &op.field, format!("{before}{}", op.value)),
            "prefix" => set(r, &op.field, format!("{}{before}", op.value)),
            "clear" => set(r, &op.field, String::new()),
            "copy" => {
                let b2 = get(r, &op.target);
                set(r, &op.target, before.clone());
                return if b2 != before { Some((b2, before)) } else { None };
            }
            "move" | "rename" => {
                let b2 = get(r, &op.target);
                if before.is_empty() {
                    return None;
                }
                set(r, &op.target, before.clone());
                set(r, &op.field, String::new());
                return Some((format!("{}: {before}; {}: {b2}", op.field, op.target), format!("{}: {before}", op.target)));
            }
            _ => return None,
        }
        let after = get(r, &op.field);
        if after != before { Some((before, after)) } else { None }
    }

    /// Preview (apply=false) or apply a bulk field change. Applying is one undo step.
    pub fn bulk_edit(&mut self, ids: &[String], op: &BulkOp, apply: bool) -> Result<Vec<BulkChange>> {
        let mut changes = Vec::new();
        let mut updated = Vec::new();
        for id in ids {
            let mut r = self.store.must::<Reference>(id)?;
            if let Some((b, a)) = Self::bulk_apply(&mut r, op) {
                changes.push(BulkChange { ref_id: r.id.clone(), title: r.title().to_string(), field: op.field.clone(), before: b, after: a });
                updated.push(r);
            }
        }
        if apply && !updated.is_empty() {
            self.safety_snapshot("bulk-edit")?;
            let t = now();
            let label = format!("Bulk {} {} on {} sources", op.op, crate::types::field_label(&op.field), updated.len());
            self.write(&label, |tx| {
                for mut r in updated {
                    r.revision += 1;
                    r.modified = t.clone();
                    r.provenance.insert(op.field.clone(), Provenance { source: "user:bulk".into(), time: t.clone(), user_edited: true, confidence: None, review: "reviewed".into() });
                    tx.put(&r)?;
                }
                Ok(())
            })?;
            self.recompute_copies(None)?;
        }
        Ok(changes)
    }

    /// Find and replace inside one field (or all text fields with "*"), with preview.
    pub fn find_replace(&mut self, ids: Option<&[String]>, field: &str, find: &str, replace: &str, regex: bool, case_sensitive: bool, apply: bool) -> Result<Vec<BulkChange>> {
        if find.is_empty() {
            return Err(Error::user("Enter the text to find."));
        }
        let re = if regex {
            regex::RegexBuilder::new(find).case_insensitive(!case_sensitive).size_limit(1 << 20).build().map_err(|e| Error::user(format!("The pattern could not be used: {e}")))?
        } else {
            regex::RegexBuilder::new(&regex::escape(find)).case_insensitive(!case_sensitive).build().map_err(|e| Error::user(e.to_string()))?
        };
        let refs: Vec<Reference> = match ids {
            Some(ids) => ids.iter().filter_map(|i| self.store.get::<Reference>(i).ok().flatten()).collect(),
            None => crate::store::list_in(&self.store.conn, "WHERE trashed IS NULL", &[])?,
        };
        let mut changes = Vec::new();
        let mut updated = Vec::new();
        for mut r in refs {
            let fields: Vec<String> = if field == "*" { r.fields.keys().cloned().collect() } else { vec![field.to_string()] };
            let mut touched = false;
            for f in fields {
                let Some(v) = r.fields.get(&f).cloned() else { continue };
                let nv = re.replace_all(&v, replace).into_owned();
                if nv != v {
                    changes.push(BulkChange { ref_id: r.id.clone(), title: r.title().to_string(), field: f.clone(), before: v, after: nv.clone() });
                    r.fields.insert(f, nv);
                    touched = true;
                }
            }
            if touched {
                updated.push(r);
            }
        }
        if apply && !updated.is_empty() {
            self.safety_snapshot("find-replace")?;
            let t = now();
            self.write(&format!("Find and replace in {} sources", updated.len()), |tx| {
                for mut r in updated {
                    r.revision += 1;
                    r.modified = t.clone();
                    tx.put(&r)?;
                }
                Ok(())
            })?;
            self.recompute_copies(None)?;
        }
        Ok(changes)
    }

    // ---- labels, collections, saved searches ---------------------------------------------

    pub fn save_label(&mut self, mut l: Label) -> Result<Label> {
        if l.id.is_empty() {
            l.id = new_id();
        }
        let ll = l.clone();
        self.write("Save label", |tx| tx.put(&ll))?;
        Ok(l)
    }

    /// Create several labels at once (one undo step). Names already in use (ignoring case) are
    /// skipped; returns the labels created.
    pub fn create_labels(&mut self, items: &[(String, String)]) -> Result<Vec<Label>> {
        let existing: Vec<Label> = self.store.all()?;
        let mut taken: std::collections::BTreeSet<String> = existing.iter().map(|l| l.name.trim().to_lowercase()).collect();
        let mut made = vec![];
        for (name, color) in items {
            let n = name.trim();
            if n.is_empty() || !taken.insert(n.to_lowercase()) {
                continue;
            }
            made.push(Label { id: new_id(), name: n.to_string(), color: if color.trim().is_empty() { "#1554d1".into() } else { color.trim().to_string() } });
        }
        if !made.is_empty() {
            let m = made.clone();
            self.write(if m.len() == 1 { "Create label" } else { "Create labels" }, |tx| {
                for l in &m {
                    tx.put(l)?;
                }
                Ok(())
            })?;
        }
        Ok(made)
    }

    /// Merge several labels into one (`into`) or delete them (`into` empty), as one undo step.
    /// Sources keep everything else.
    pub fn merge_labels(&mut self, ids: &[String], into: &str) -> Result<usize> {
        let ids: Vec<&String> = ids.iter().filter(|i| i.as_str() != into).collect();
        if !into.is_empty() {
            self.store.must::<Label>(into)?;
        }
        let refs: Vec<Reference> = self.store.all()?;
        let mut touched = 0;
        self.write(if into.is_empty() { "Delete labels" } else { "Merge labels" }, |tx| {
            for mut r in refs {
                if r.labels.iter().any(|l| ids.contains(&l)) {
                    r.labels.retain(|l| !ids.contains(&l));
                    if !into.is_empty() && !r.labels.iter().any(|l| l == into) {
                        r.labels.push(into.to_string());
                    }
                    tx.put(&r)?;
                    touched += 1;
                }
            }
            for id in &ids {
                tx.delete::<Label>(id)?;
            }
            Ok(())
        })?;
        Ok(touched)
    }

    /// Delete a label, or merge it into `into` when given.
    pub fn delete_label(&mut self, id: &str, into: Option<&str>) -> Result<()> {
        let refs: Vec<Reference> = self.store.all()?;
        self.write(if into.is_some() { "Merge labels" } else { "Delete label" }, |tx| {
            for mut r in refs {
                if r.labels.iter().any(|l| l == id) {
                    r.labels.retain(|l| l != id);
                    if let Some(t) = into {
                        if !r.labels.iter().any(|l| l == t) {
                            r.labels.push(t.to_string());
                        }
                    }
                    tx.put(&r)?;
                }
            }
            tx.delete::<Label>(id)
        })
    }

    pub fn set_labels(&mut self, ids: &[String], label: &str, on: bool) -> Result<()> {
        self.write(if on { "Add label" } else { "Remove label" }, |tx| {
            for id in ids {
                let mut r = tx.must::<Reference>(id)?;
                let has = r.labels.iter().any(|l| l == label);
                if on && !has {
                    r.labels.push(label.into());
                    tx.put(&r)?;
                } else if !on && has {
                    r.labels.retain(|l| l != label);
                    tx.put(&r)?;
                }
            }
            Ok(())
        })
    }

    pub fn set_rating(&mut self, ids: &[String], rating: u8) -> Result<()> {
        self.write("Set rating", |tx| {
            for id in ids {
                let mut r = tx.must::<Reference>(id)?;
                r.rating = rating.min(5);
                tx.put(&r)?;
            }
            Ok(())
        })
    }

    pub fn save_collection(&mut self, mut c: Collection) -> Result<Collection> {
        if c.id.is_empty() {
            c.id = new_id();
            c.created = now();
        }
        if !c.parent_id.is_empty() {
            // Prevent cycles.
            let all: Vec<Collection> = self.store.all()?;
            let mut p = c.parent_id.clone();
            for _ in 0..1000 {
                if p == c.id {
                    return Err(Error::user("A collection cannot be placed inside itself."));
                }
                match all.iter().find(|x| x.id == p) {
                    Some(x) if !x.parent_id.is_empty() => p = x.parent_id.clone(),
                    _ => break,
                }
            }
        }
        let cc = c.clone();
        self.write("Save collection", |tx| tx.put(&cc))?;
        Ok(c)
    }

    pub fn delete_collection(&mut self, id: &str) -> Result<()> {
        let all: Vec<Collection> = self.store.all()?;
        self.write("Delete collection", |tx| {
            let mut stack = vec![id.to_string()];
            while let Some(c) = stack.pop() {
                for ch in all.iter().filter(|x| x.parent_id == c) {
                    stack.push(ch.id.clone());
                }
                tx.delete::<Collection>(&c)?;
            }
            Ok(())
        })
    }

    pub fn collect(&mut self, collection_id: &str, ids: &[String], add: bool) -> Result<()> {
        self.write(if add { "Add to collection" } else { "Remove from collection" }, |tx| {
            let mut c = tx.must::<Collection>(collection_id)?;
            if !c.query.is_empty() {
                return Err(Error::user("A smart collection fills itself from its search."));
            }
            for id in ids {
                if add && !c.ref_ids.contains(id) {
                    c.ref_ids.push(id.clone());
                } else if !add {
                    c.ref_ids.retain(|x| x != id);
                }
            }
            tx.put(&c)
        })
    }

    /// Place a collection inside another (empty `parent_id` for the top level). Refuses to put a
    /// collection inside itself or one of its own sub-collections.
    pub fn move_collection(&mut self, id: &str, parent_id: &str) -> Result<Collection> {
        let mut c = self.store.must::<Collection>(id)?;
        if c.parent_id == parent_id {
            return Ok(c);
        }
        if !parent_id.is_empty() {
            self.store.must::<Collection>(parent_id)?;
        }
        c.parent_id = parent_id.to_string();
        self.save_collection(c)
    }

    /// Move sources from one collection to another as one step (they stay in any other
    /// collections they belong to).
    pub fn move_between_collections(&mut self, from: &str, to: &str, ids: &[String]) -> Result<()> {
        if from == to {
            return Ok(());
        }
        self.write("Move to collection", |tx| {
            let mut a = tx.must::<Collection>(from)?;
            let mut b = tx.must::<Collection>(to)?;
            if !a.query.is_empty() || !b.query.is_empty() {
                return Err(Error::user("A smart collection fills itself from its search."));
            }
            a.ref_ids.retain(|x| !ids.contains(x));
            for id in ids {
                if !b.ref_ids.contains(id) {
                    b.ref_ids.push(id.clone());
                }
            }
            tx.put(&a)?;
            tx.put(&b)
        })
    }

    pub fn save_search(&mut self, mut s: SavedSearch) -> Result<SavedSearch> {
        crate::search::parse(&s.query)?;
        if s.id.is_empty() {
            s.id = new_id();
            s.created = now();
        }
        let ss = s.clone();
        self.write("Save search", |tx| tx.put(&ss))?;
        Ok(s)
    }

    /// Resolve a conflict on a field by choosing a value; the rejected value stays in history.
    pub fn resolve_conflict(&mut self, id: &str, field: &str, value: Option<String>) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(id)?;
        if let Some(v) = value {
            r.fields.insert(field.into(), v);
        }
        r.conflicts.retain(|c| c.field != field);
        self.update_reference(r, "user")
    }

    pub fn confirm_absent(&mut self, id: &str, field: &str, absent: bool) -> Result<Reference> {
        let mut r = self.store.must::<Reference>(id)?;
        r.confirmed_absent.retain(|f| f != field);
        if absent {
            r.confirmed_absent.push(field.into());
        }
        self.update_reference(r, "user")
    }
}

pub(crate) fn rank(s: &UseState) -> u8 {
    match s {
        UseState::Excluded => 0,
        UseState::Candidate => 1,
        UseState::Copied => 2,
        UseState::Confirmed => 3,
    }
}

pub(crate) fn add_membership(tx: &mut crate::store::Tx, project_id: &str, ref_id: &str) -> Result<bool> {
    let key = ProjectSource::key(project_id, ref_id);
    if tx.get::<ProjectSource>(&key)?.is_some() {
        return Ok(false);
    }
    tx.put(&ProjectSource { id: key, project_id: project_id.into(), ref_id: ref_id.into(), reading: "unread".into(), priority: "normal".into(), inclusion: "candidate".into(), added: now(), ..Default::default() })?;
    Ok(true)
}

pub fn short(s: &str) -> String {
    let t: String = s.chars().take(48).collect();
    if s.chars().count() > 48 { format!("{t}…") } else if t.is_empty() { "untitled".into() } else { t }
}
