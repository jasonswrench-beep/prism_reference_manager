//! Term lists (journals, publishers, creators, keywords) with variant merging, safety
//! snapshots before large operations, and automatically refreshed BibTeX files.

use super::Library;
use crate::error::{Error, Result};
use crate::model::*;
use crate::util::now;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct Term {
    pub value: String,
    pub count: usize,
    /// Folded key; variants share it or are close to it.
    pub key: String,
}

fn person_term(n: &Name) -> Option<String> {
    match n.kind {
        NameKind::Person if !n.family.is_empty() => Some(if n.given.is_empty() { n.surname() } else { format!("{}, {}", n.surname(), n.given) }),
        NameKind::Organization | NameKind::Mononym if !n.literal.is_empty() => Some(n.literal.clone()),
        _ => None,
    }
}

impl Library {
    /// Distinct values of a term list with counts. `kind`: journal, publisher, creator, keyword.
    pub fn terms(&self, kind: &str) -> Result<Vec<Term>> {
        let refs: Vec<Reference> = crate::store::list_in(&self.store.conn, "WHERE trashed IS NULL", &[])?;
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for r in &refs {
            match kind {
                "journal" => {
                    if r.has("container_title") {
                        *counts.entry(r.f("container_title").to_string()).or_default() += 1;
                    }
                }
                "publisher" => {
                    if r.has("publisher") {
                        *counts.entry(r.f("publisher").to_string()).or_default() += 1;
                    }
                }
                "creator" => {
                    for c in &r.creators {
                        if let Some(t) = person_term(&c.name) {
                            *counts.entry(t).or_default() += 1;
                        }
                    }
                }
                "keyword" => {
                    for t in &r.tags {
                        *counts.entry(t.clone()).or_default() += 1;
                    }
                }
                _ => return Err(Error::user("Unknown term list.")),
            }
        }
        let mut v: Vec<Term> = counts.into_iter().map(|(value, count)| Term { key: crate::util::sort_key(&value), value, count }).collect();
        v.sort_by(|a, b| a.key.cmp(&b.key).then(b.count.cmp(&a.count)));
        Ok(v)
    }

    /// Groups of likely variants (same folded form, or very similar).
    pub fn term_variants(&self, kind: &str) -> Result<Vec<Vec<Term>>> {
        let terms = self.terms(kind)?;
        let mut groups: Vec<Vec<Term>> = Vec::new();
        for t in terms {
            let found = groups.iter_mut().find(|g| {
                let k = &g[0].key;
                k == &t.key || (k.len() > 6 && strsim::jaro_winkler(k, &t.key) > 0.96)
            });
            match found {
                Some(g) => g.push(t),
                None => groups.push(vec![t]),
            }
        }
        groups.retain(|g| g.len() > 1);
        Ok(groups)
    }

    /// Replace every occurrence of the `from` variants with `to`, as one undo step.
    pub fn merge_terms(&mut self, kind: &str, from: &[String], to: &str) -> Result<usize> {
        if to.trim().is_empty() {
            return Err(Error::user("Enter the preferred form."));
        }
        self.safety_snapshot("merge-terms")?;
        let refs: Vec<Reference> = crate::store::list_in(&self.store.conn, "WHERE trashed IS NULL", &[])?;
        let t = now();
        let mut changed = Vec::new();
        for mut r in refs {
            let mut touched = false;
            match kind {
                "journal" | "publisher" => {
                    let f = if kind == "journal" { "container_title" } else { "publisher" };
                    if from.iter().any(|x| x == r.f(f)) && r.f(f) != to {
                        r.fields.insert(f.into(), to.into());
                        touched = true;
                    }
                }
                "creator" => {
                    for c in &mut r.creators {
                        if let Some(cur) = person_term(&c.name) {
                            if from.contains(&cur) && cur != to {
                                if c.name.kind == NameKind::Person {
                                    let n = Name::parse(to);
                                    c.name.family = n.family;
                                    c.name.given = n.given;
                                    c.name.particle = n.particle;
                                } else {
                                    c.name.literal = to.into();
                                }
                                touched = true;
                            }
                        }
                    }
                }
                "keyword" => {
                    let before = r.tags.clone();
                    r.tags = r.tags.iter().map(|x| if from.contains(x) { to.to_string() } else { x.clone() }).collect();
                    r.tags.dedup();
                    touched = before != r.tags;
                }
                _ => return Err(Error::user("Unknown term list.")),
            }
            if touched {
                r.revision += 1;
                r.modified = t.clone();
                changed.push(r);
            }
        }
        let n = changed.len();
        if n > 0 {
            self.write(&format!("Merge {} variants into “{}”", from.len(), to), |tx| {
                for r in &changed {
                    tx.put(r)?;
                }
                Ok(())
            })?;
            self.recompute_copies(None)?;
        }
        Ok(n)
    }

    /// A quick database copy before merges, bulk changes and imports, in addition to undo.
    /// The newest five are kept.
    pub fn safety_snapshot(&self, op: &str) -> Result<()> {
        if self.store.read_only {
            return Ok(());
        }
        let dir = self.root.join("backups").join("before-changes");
        std::fs::create_dir_all(&dir)?;
        let name = format!("{}-{}.db", now().replace([':', '.'], "-"), op);
        let p = dir.join(name);
        self.store.conn.execute("VACUUM INTO ?1", [p.to_string_lossy()])?;
        let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map(|x| x == "db").unwrap_or(false)).collect();
        v.sort();
        while v.len() > 5 {
            let _ = std::fs::remove_file(v.remove(0));
        }
        Ok(())
    }

    /// Write a project's (or the library's) BibTeX/biblatex file. Used for the automatically
    /// refreshed bibliography file for writing tools.
    pub fn write_bib_file(&self, project_id: &str, path: &std::path::Path, biblatex: bool) -> Result<usize> {
        let refs: Vec<Reference> = if project_id.is_empty() {
            crate::store::list_in(&self.store.conn, "WHERE trashed IS NULL", &[])?
        } else {
            let ids: Vec<String> = self.store.by::<ProjectSource>("project_id", project_id)?.into_iter().map(|m| m.ref_id).collect();
            self.references(&ids)?.into_iter().filter(|r| r.trashed_at.is_none()).collect()
        };
        let mut rr: Vec<&Reference> = refs.iter().collect();
        rr.sort_by(|a, b| a.citekey.cmp(&b.citekey));
        let text = crate::formats::bibtex::export(&rr, biblatex, &[]);
        // Rewrite only when the content changed, so file watchers in writing tools stay quiet.
        if std::fs::read_to_string(path).map(|s| s == text).unwrap_or(false) {
            return Ok(rr.len());
        }
        crate::util::write_atomic(path, text.as_bytes())?;
        Ok(rr.len())
    }

    pub fn last_event_seq(&self) -> i64 {
        self.store.conn.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0)).unwrap_or(0)
    }
}
