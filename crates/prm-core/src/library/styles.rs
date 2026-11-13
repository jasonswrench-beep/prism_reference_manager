//! The style registry: the tested APA composer, CSL styles bundled with the program, and CSL
//! files the person imports. Imported files are kept in the library's `styles` folder under
//! their content hash, so a manuscript that chose one keeps rendering with exactly that version
//! even after a newer file of the same style is imported.

use super::Library;
use crate::error::{Error, Result};
use crate::render::{self, csl, Engine, StyleInfo};
use std::path::PathBuf;
use std::sync::Arc;

impl Library {
    fn styles_dir(&self) -> PathBuf {
        self.root.join("styles")
    }

    fn imported_files(&self) -> Vec<(PathBuf, String)> {
        let mut out = vec![];
        if let Ok(rd) = std::fs::read_dir(self.styles_dir()) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("csl") {
                    if let Ok(xml) = std::fs::read_to_string(&p) {
                        out.push((p, xml));
                    }
                }
            }
        }
        out
    }

    /// Parent lookup for dependent styles: imported independent styles first, then bundled.
    fn parent_lookup(&self) -> impl Fn(&str) -> Option<hayagriva::citationberg::IndependentStyle> + '_ {
        move |csl_id: &str| {
            for (_, xml) in self.imported_files() {
                if let Ok(hayagriva::citationberg::Style::Independent(s)) = hayagriva::citationberg::Style::from_xml(&xml) {
                    if s.info.id == csl_id {
                        return Some(s);
                    }
                }
            }
            csl::bundled_by_csl_id(csl_id)
        }
    }

    pub fn imported_styles(&self) -> Vec<StyleInfo> {
        let lookup = self.parent_lookup();
        let mut v: Vec<(StyleInfo, std::time::SystemTime)> = self
            .imported_files()
            .into_iter()
            .filter_map(|(p, xml)| {
                let mut info = csl::parse_style(&xml, &lookup).ok()?.info;
                let when = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                info.title = format!("{} (imported)", info.title);
                Some((info, when))
            })
            .collect();
        v.sort_by(|a, b| a.0.title.to_lowercase().cmp(&b.0.title.to_lowercase()).then(b.1.cmp(&a.1)));
        v.into_iter().map(|x| x.0).collect()
    }

    /// Every style this library can use.
    pub fn styles(&self) -> Vec<StyleInfo> {
        let mut v = render::installed_styles();
        v.extend(self.imported_styles());
        v
    }

    /// The renderer and descriptor for a style id. Unknown ids fall back to APA 7 with a clear
    /// title, so a manuscript whose imported style was removed still renders.
    pub fn style_engine(&self, id: &str) -> Result<(StyleInfo, Engine)> {
        if id.is_empty() || id == "apa-7" {
            return Ok((render::builtin_styles().remove(0), Engine::Apa));
        }
        if id.starts_with("csl:") {
            if let (Some(info), Some(ls)) = (csl::bundled_styles().into_iter().find(|s| s.id == id), csl::load_bundled(id)) {
                return Ok((info, Engine::Csl(ls)));
            }
        }
        if let Some(hash) = id.strip_prefix("user:") {
            let p = self.styles_dir().join(format!("{hash}.csl"));
            if let Ok(xml) = std::fs::read_to_string(&p) {
                let parsed = csl::parse_style(&xml, &self.parent_lookup()).map_err(|e| Error::user(&e))?;
                let mut info = parsed.info;
                info.title = format!("{} (imported)", info.title);
                return Ok((info, Engine::Csl(Arc::new(parsed.loaded))));
            }
        }
        Err(Error::user("The style this manuscript uses is no longer installed. Choose another style, or import the style file again."))
    }

    /// Validate and keep a CSL file. Returns the new style's descriptor.
    pub fn import_style(&self, xml: &str) -> Result<StyleInfo> {
        let parsed = csl::parse_style(xml, &self.parent_lookup()).map_err(|e| Error::user(&e))?;
        std::fs::create_dir_all(self.styles_dir())?;
        let p = self.styles_dir().join(format!("{}.csl", parsed.info.hash));
        if !p.exists() {
            crate::util::write_atomic(&p, xml.as_bytes())?;
        }
        let mut info = parsed.info;
        info.title = format!("{} (imported)", info.title);
        Ok(info)
    }

    /// Remove an imported style file. Manuscripts that use it are reported, not changed.
    pub fn remove_style(&self, id: &str) -> Result<Vec<String>> {
        let Some(hash) = id.strip_prefix("user:") else { return Err(Error::user("Only imported styles can be removed.")) };
        let users: Vec<String> = self.store.all::<crate::model::Manuscript>()?.into_iter().filter(|m| m.style == id).map(|m| m.title).collect();
        if !users.is_empty() {
            return Err(Error::user(&format!("This style is used by: {}. Choose another style for them first.", users.join(", "))));
        }
        let p = self.styles_dir().join(format!("{hash}.csl"));
        if p.exists() {
            std::fs::remove_file(p)?;
        }
        Ok(users)
    }
}
