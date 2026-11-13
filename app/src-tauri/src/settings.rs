//! Per-user settings, administrator policy and portable mode.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiClient {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub scopes: Vec<String>,
    pub created: String,
    pub last_used: String,
}

impl Default for ApiClient {
    fn default() -> Self {
        ApiClient { id: String::new(), name: String::new(), token_hash: String::new(), scopes: vec![], created: String::new(), last_used: String::new() }
    }
}

/// A folder whose new PDFs are staged for review in one library.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WatchedFolder {
    pub path: String,
    /// Root folder of the library the files go to.
    pub library: String,
    pub project_id: String,
    /// Turn subfolders into collections.
    pub mirror: bool,
}

/// A BibTeX or biblatex file kept up to date for writing tools.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BibExport {
    pub project_id: String,
    pub path: String,
    pub biblatex: bool,
    /// bibtex, biblatex, ris, csljson, csv or xlsx; empty means BibTeX or biblatex by `biblatex`.
    pub format: String,
    /// Fields left out of every rewrite (for example abstract, file, note).
    pub exclude_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub last_library: String,
    pub recent_libraries: Vec<String>,
    /// system, light, dark, high-contrast
    pub theme: String,
    /// comfortable, compact
    pub density: String,
    pub font_size: u32,
    pub reader_theme: String,
    pub reduce_motion: bool,
    pub citekey_pattern: String,
    pub protected_terms: Vec<String>,
    pub network_enabled: bool,
    /// Kinds from the network inventory that are switched off.
    pub network_disabled_kinds: Vec<String>,
    pub auto_backup: bool,
    pub backup_interval_hours: u32,
    pub backup_keep: usize,
    pub backup_dir: String,
    pub backup_include_files: bool,
    pub last_backup: String,
    pub trash_retention_days: i64,
    pub api_enabled: bool,
    pub api_port: u16,
    pub api_clients: Vec<ApiClient>,
    pub shortcuts: Value,
    pub layouts: Value,
    pub layout: Value,
    pub active_project: String,
    pub last_view: String,
    pub last_source: String,
    pub default_paste_font: String,
    pub tour_done: bool,
    pub interface_language: String,
    pub duplicate_sensitivity: f32,
    pub bib_exports: Vec<BibExport>,
    /// Check for a newer version once a day. Off by default.
    pub update_check: bool,
    pub update_last_check: String,
    pub watched_folders: Vec<WatchedFolder>,
    /// Read the text of new PDFs in the background so full-text search finds them.
    pub auto_index: bool,
    /// Tidying rules applied whenever a source is saved.
    pub save_rules: prm_core::cleanup::SaveRules,
    /// Days between automatic quick health checks (0 = off).
    pub health_days: u32,
    pub health_last: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            last_library: String::new(),
            recent_libraries: vec![],
            theme: "system".into(),
            density: "comfortable".into(),
            font_size: 14,
            reader_theme: "light".into(),
            reduce_motion: false,
            citekey_pattern: prm_core::citekey::DEFAULT_PATTERN.into(),
            protected_terms: vec![],
            network_enabled: true,
            network_disabled_kinds: vec![],
            auto_backup: true,
            backup_interval_hours: 24,
            backup_keep: 10,
            backup_dir: String::new(),
            backup_include_files: false,
            last_backup: String::new(),
            trash_retention_days: 30,
            api_enabled: false,
            api_port: 23119,
            api_clients: vec![],
            shortcuts: Value::Object(Default::default()),
            layouts: Value::Object(Default::default()),
            layout: Value::Object(Default::default()),
            active_project: String::new(),
            last_view: "projects".into(),
            last_source: String::new(),
            default_paste_font: String::new(),
            tour_done: false,
            interface_language: "en".into(),
            duplicate_sensitivity: 0.88,
            bib_exports: vec![],
            update_check: false,
            update_last_check: String::new(),
            watched_folders: vec![],
            auto_index: true,
            save_rules: Default::default(),
            health_days: 7,
            health_last: String::new(),
        }
    }
}

/// Administrator policy: values here override and lock the matching settings. Read from
/// `policy.json` next to the program, then from `%ProgramData%\Prism Reference Manager`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Policy {
    pub source: String,
    pub locked: serde_json::Map<String, Value>,
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

/// Portable mode: a `portable.txt` file next to the program keeps settings and the default
/// library beside it instead of in the user profile.
pub fn portable() -> bool {
    exe_dir().join("portable.txt").exists()
}

pub fn config_dir(default: PathBuf) -> PathBuf {
    #[cfg(debug_assertions)]
    if let Ok(d) = std::env::var("PRM_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    if portable() {
        exe_dir().join("data").join("settings")
    } else {
        default
    }
}

pub fn default_library_dir() -> PathBuf {
    if portable() {
        return exe_dir().join("data").join("Library");
    }
    let docs = std::env::var("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")).unwrap_or_else(|_| PathBuf::from("."));
    docs.join("Prism Reference Libraries").join("My Library")
}

pub fn load(dir: &Path) -> Settings {
    std::fs::read_to_string(dir.join("settings.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(dir: &Path, s: &Settings) -> std::io::Result<()> {
    prm_core::util::write_atomic(&dir.join("settings.json"), serde_json::to_string_pretty(s).unwrap_or_default().as_bytes())
}

pub fn load_policy() -> Policy {
    let mut candidates = vec![exe_dir().join("policy.json")];
    if let Ok(pd) = std::env::var("ProgramData") {
        candidates.push(PathBuf::from(pd).join("Prism Reference Manager").join("policy.json"));
    }
    for c in candidates {
        if let Ok(s) = std::fs::read_to_string(&c) {
            if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&s) {
                return Policy { source: c.to_string_lossy().to_string(), locked: m };
            }
        }
    }
    Policy::default()
}

/// Apply locked policy values over the person's settings.
pub fn apply_policy(s: &mut Settings, p: &Policy) {
    if p.locked.is_empty() {
        return;
    }
    let mut v = serde_json::to_value(&*s).unwrap_or_default();
    if let Value::Object(m) = &mut v {
        for (k, val) in &p.locked {
            if m.contains_key(k) {
                m.insert(k.clone(), val.clone());
            }
        }
    }
    if let Ok(ns) = serde_json::from_value(v) {
        *s = ns;
    }
}
