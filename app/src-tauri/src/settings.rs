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
    /// This computer's base folder for relative linked files, by library id.
    pub linked_bases: std::collections::BTreeMap<String, String>,
    /// Last language chosen for text recognition.
    pub ocr_lang: String,
    /// Name pattern for files copied into the library; empty keeps the original name.
    pub file_name_pattern: String,
    /// System-wide shortcut for the quick-copy launcher; empty keeps it off.
    pub quick_copy_shortcut: String,
    /// The person has answered the first-run question about online lookups.
    pub network_asked: bool,
    /// Look up identifiers found in imported PDFs (asked in the import dialog).
    pub pdf_lookups: bool,
    /// Look up identifiers found in PDFs that appear in watched folders.
    pub watch_lookups: bool,
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
            network_enabled: false,
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
            linked_bases: Default::default(),
            ocr_lang: String::new(),
            file_name_pattern: String::new(),
            quick_copy_shortcut: String::new(),
            network_asked: false,
            pdf_lookups: false,
            watch_lookups: false,
        }
    }
}

/// Administrator policy: values here override and lock the matching settings (see `load_policy`
/// for where it comes from and what it may set).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Policy {
    pub source: String,
    pub locked: serde_json::Map<String, Value>,
    /// Policy files that were found but not trusted, and why (shown in diagnostics).
    #[serde(default)]
    pub ignored: Vec<String>,
}

/// Accounts that may own or change a policy file: Administrators, SYSTEM, TrustedInstaller.
pub const TRUSTED_SIDS: &[&str] = &["S-1-5-32-544", "S-1-5-18", "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"];
/// Rights that let an account change a file or folder, or replace what is inside a folder.
const WRITE_RIGHTS: u32 = 0x0000_0002 | 0x0000_0004 | 0x0000_0010 | 0x0000_0040 | 0x0000_0100 | 0x0001_0000 | 0x0004_0000 | 0x0008_0000 | 0x1000_0000 | 0x4000_0000;
/// For a folder, adding a file (0x2) or a subfolder (0x4) does not let anyone change a file an
/// administrator put there; deleting or replacing it does.
const FOLDER_WRITE_RIGHTS: u32 = WRITE_RIGHTS & !(0x0000_0002 | 0x0000_0004 | 0x0000_0010 | 0x0000_0100 | 0x4000_0000);

/// Decide from an object's owner and its access-allowed entries (SID, rights, inherit-only)
/// whether only trusted accounts can change it.
pub fn only_trusted_can_write(owner: &str, aces: &[(String, u32, bool)], is_dir: bool) -> Result<(), String> {
    if !TRUSTED_SIDS.contains(&owner) {
        return Err(format!("it is owned by {owner}, not by an administrator"));
    }
    let rights = if is_dir { FOLDER_WRITE_RIGHTS } else { WRITE_RIGHTS };
    for (sid, mask, inherit_only) in aces {
        if !inherit_only && mask & rights != 0 && !TRUSTED_SIDS.contains(&sid.as_str()) {
            return Err(format!("{sid} can change it"));
        }
    }
    Ok(())
}

/// Read a file's or folder's owner and access list from Windows and check it.
#[cfg(windows)]
pub fn admin_only(path: &Path) -> Result<(), String> {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{GetAce, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};
    unsafe fn sid_string(sid: PSID) -> String {
        let mut out = PWSTR::null();
        if unsafe { ConvertSidToStringSidW(sid, &mut out) }.is_err() {
            return "an unknown account".into();
        }
        let s = unsafe { out.to_string() }.unwrap_or_default();
        unsafe {
            let _ = LocalFree(Some(HLOCAL(out.0 as _)));
        }
        s
    }
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    let err = unsafe { GetNamedSecurityInfoW(&HSTRING::from(path.as_os_str()), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, Some(&mut owner), None, Some(&mut dacl), None, &mut sd) };
    if err.0 != 0 {
        return Err(format!("its permissions could not be read (error {})", err.0));
    }
    let result = (|| {
        if dacl.is_null() {
            return Err("it has no access list, so everyone can change it".to_string());
        }
        let owner_s = unsafe { sid_string(owner) };
        let mut aces = vec![];
        for i in 0..unsafe { (*dacl).AceCount } as u32 {
            let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
            if unsafe { GetAce(dacl, i, &mut ace) }.is_err() {
                return Err("its access list could not be read".to_string());
            }
            let header = unsafe { &*(ace as *const ACE_HEADER) };
            if header.AceType != 0 {
                continue; // only access-allowed entries grant rights
            }
            let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
            let sid = PSID(&allowed.SidStart as *const u32 as *mut _);
            aces.push((unsafe { sid_string(sid) }, allowed.Mask, header.AceFlags & 0x08 != 0));
        }
        only_trusted_can_write(&owner_s, &aces, path.is_dir())
    })();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sd.0 as _)));
    }
    result
}

#[cfg(not(windows))]
pub fn admin_only(_path: &Path) -> Result<(), String> {
    Err("policy files are read only on Windows".into())
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

/// Settings policy may never set, from any source: they would let whoever writes the policy
/// reach the person's library (a local API client, a backup folder, watched folders, link bases).
pub const POLICY_NEVER: &[&str] = &["api_clients", "watched_folders", "linked_bases", "backup_dir", "api_port", "recent_libraries", "last_library", "bib_exports", "layouts", "layout", "shortcuts"];
/// Settings policy may only switch off.
pub const POLICY_OFF_ONLY: &[&str] = &["api_enabled", "backup_include_files", "update_check", "auto_index"];

/// Keep only what policy may lock: known settings of the right type, never the keys in
/// `POLICY_NEVER`, and the keys in `POLICY_OFF_ONLY` only when they switch something off.
pub fn policy_filter(m: serde_json::Map<String, Value>) -> serde_json::Map<String, Value> {
    let defaults = serde_json::to_value(Settings::default()).unwrap_or_default();
    m.into_iter()
        .filter(|(k, v)| {
            let Some(target) = defaults.get(k) else { return false };
            if POLICY_NEVER.contains(&k.as_str()) || std::mem::discriminant(v) != std::mem::discriminant(target) {
                return false;
            }
            !POLICY_OFF_ONLY.contains(&k.as_str()) || v == &Value::Bool(false)
        })
        .collect()
}

/// Locked settings, from (highest first) Group Policy for the computer, Group Policy for the user,
/// and `%ProgramData%\Prism Reference Manager\policy.json`. The file counts only when an
/// administrator made it: the folder and the file must be owned by Administrators, SYSTEM or
/// TrustedInstaller, and no other account may change them (any account can create folders in
/// ProgramData, so a file someone else planted is ignored). Policy can only restrict.
pub fn load_policy() -> Policy {
    let mut p = Policy::default();
    let mut sources = vec![];
    if let Ok(pd) = std::env::var("ProgramData") {
        let dir = PathBuf::from(pd).join("Prism Reference Manager");
        let file = dir.join("policy.json");
        if file.is_file() {
            match admin_only(&dir).and_then(|_| admin_only(&file)) {
                Ok(()) => {
                    if let Ok(Value::Object(m)) = std::fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string())) {
                        sources.push(file.to_string_lossy().to_string());
                        p.locked = policy_filter(m);
                    }
                }
                Err(why) => p.ignored.push(format!("{} was ignored: {why}", file.display())),
            }
        }
    }
    // Group Policy overrides the file: user policy first, then computer policy on top.
    for (label, values) in registry_policies() {
        if values.is_empty() {
            continue;
        }
        sources.push(label);
        for (k, v) in policy_filter(policy_values(&values)) {
            p.locked.insert(k, v);
        }
    }
    p.source = sources.join("; ");
    p
}

/// A value as read from the registry.
#[derive(Debug, Clone)]
pub enum RegRaw {
    Number(u64),
    Text(String),
}

pub const POLICY_KEY: &str = r"Software\Policies\Prism Reference Manager";

/// Values under the policy key, user policy then computer policy (later wins).
fn registry_policies() -> Vec<(String, Vec<(String, RegRaw)>)> {
    #[cfg(windows)]
    {
        use winreg::enums::{RegType, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        let mut out = vec![];
        for (hive, label) in [(HKEY_CURRENT_USER, "Group Policy (user)"), (HKEY_LOCAL_MACHINE, "Group Policy (computer)")] {
            let mut vals = vec![];
            if let Ok(k) = winreg::RegKey::predef(hive).open_subkey(POLICY_KEY) {
                for (name, v) in k.enum_values().flatten() {
                    let raw = match v.vtype {
                        RegType::REG_DWORD => k.get_value::<u32, _>(&name).ok().map(|n| RegRaw::Number(n as u64)),
                        RegType::REG_QWORD => k.get_value::<u64, _>(&name).ok().map(RegRaw::Number),
                        RegType::REG_SZ | RegType::REG_EXPAND_SZ => k.get_value::<String, _>(&name).ok().map(RegRaw::Text),
                        _ => None,
                    };
                    if let Some(r) = raw {
                        vals.push((name, r));
                    }
                }
            }
            out.push((label.to_string(), vals));
        }
        out
    }
    #[cfg(not(windows))]
    {
        vec![]
    }
}

/// Turn registry values into setting values of the right type. Names that are not settings, and
/// values that do not fit the setting, are ignored.
pub fn policy_values(values: &[(String, RegRaw)]) -> serde_json::Map<String, Value> {
    let defaults = serde_json::to_value(Settings::default()).unwrap_or_default();
    let mut out = serde_json::Map::new();
    for (name, raw) in values {
        let Some(target) = defaults.get(name) else { continue };
        let v = match (target, raw) {
            (Value::Bool(_), RegRaw::Number(n)) => Some(Value::Bool(*n != 0)),
            (Value::Bool(_), RegRaw::Text(s)) => match s.trim().to_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Some(Value::Bool(true)),
                "0" | "false" | "no" | "off" => Some(Value::Bool(false)),
                _ => None,
            },
            (Value::Number(_), RegRaw::Number(n)) => Some(Value::from(*n)),
            (Value::Number(_), RegRaw::Text(s)) => s.trim().parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number),
            (Value::String(_), RegRaw::Text(s)) => Some(Value::String(s.clone())),
            (Value::String(_), RegRaw::Number(n)) => Some(Value::String(n.to_string())),
            (_, RegRaw::Text(s)) => serde_json::from_str::<Value>(s).ok().filter(|v| std::mem::discriminant(v) == std::mem::discriminant(target)),
            _ => None,
        };
        if let Some(v) = v {
            out.insert(name.clone(), v);
        }
    }
    out
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    #[test]
    fn registry_values_take_the_type_of_their_setting() {
        let vals = vec![
            ("network_enabled".to_string(), RegRaw::Number(0)),
            ("auto_backup".to_string(), RegRaw::Text("true".into())),
            ("backup_interval_hours".to_string(), RegRaw::Number(12)),
            ("backup_dir".to_string(), RegRaw::Text(r"\\server\backups".into())),
            ("network_disabled_kinds".to_string(), RegRaw::Text(r#"["abstract","linkcheck"]"#.into())),
            ("not_a_setting".to_string(), RegRaw::Number(1)),
            ("api_enabled".to_string(), RegRaw::Text("maybe".into())),
        ];
        let m = policy_values(&vals);
        assert_eq!(m["network_enabled"], Value::Bool(false));
        assert_eq!(m["auto_backup"], Value::Bool(true));
        assert_eq!(m["backup_interval_hours"], Value::from(12u64));
        assert_eq!(m["backup_dir"], Value::String(r"\\server\backups".into()));
        assert_eq!(m["network_disabled_kinds"], serde_json::json!(["abstract", "linkcheck"]));
        assert!(!m.contains_key("not_a_setting") && !m.contains_key("api_enabled"));
        // Applied over the person's settings, the locked values win.
        let mut s = Settings { network_enabled: true, ..Default::default() };
        apply_policy(&mut s, &Policy { source: "test".into(), locked: m, ignored: vec![] });
        assert!(!s.network_enabled && s.backup_interval_hours == 12);
    }
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

#[cfg(test)]
mod secret_tests {
    use super::*;
    /// SEC-003: the settings file never holds a secret. Local connection tokens are stored only as
    /// hashes; the program stores no service keys or passwords.
    #[test]
    fn settings_hold_no_plain_secrets() {
        let mut s = Settings::default();
        s.api_clients.push(ApiClient { id: "c".into(), name: "Browser".into(), token_hash: "ab12".into(), scopes: vec![], created: String::new(), last_used: String::new() });
        let text = serde_json::to_string(&s).unwrap().to_lowercase();
        for word in ["\"password", "\"secret", "\"api_key", "\"apikey", "\"token\"", "\"access_token", "\"credential"] {
            assert!(!text.contains(word), "settings must not store {word}");
        }
        assert!(text.contains("\"token_hash\""));
    }
}

#[cfg(test)]
mod policy_trust_tests {
    use super::*;
    const ADMIN: &str = "S-1-5-32-544";
    const SYSTEM: &str = "S-1-5-18";
    const USER: &str = "S-1-5-21-1-2-3-1001";
    const USERS: &str = "S-1-5-32-545";

    #[test]
    fn a_policy_file_counts_only_when_administrators_control_it() {
        let read = 0x0012_00a9;
        let full = 0x001f_01ff;
        // An administrator's file: only Administrators and SYSTEM can change it; Users can read.
        assert!(only_trusted_can_write(ADMIN, &[(ADMIN.into(), full, false), (SYSTEM.into(), full, false), (USERS.into(), read, false)], false).is_ok());
        // A file another account created (it owns it) is ignored.
        assert!(only_trusted_can_write(USER, &[(USER.into(), full, false)], false).is_err());
        // An administrator's file that ordinary users may change is ignored.
        assert!(only_trusted_can_write(ADMIN, &[(ADMIN.into(), full, false), (USERS.into(), 0x0012_019f, false)], false).is_err());
        // A folder where users may add files but not delete or replace them is acceptable.
        assert!(only_trusted_can_write(ADMIN, &[(ADMIN.into(), full, false), (USERS.into(), 0x0000_0006 | read, false)], true).is_ok());
        // ...but not one where they may delete what is inside.
        assert!(only_trusted_can_write(ADMIN, &[(USERS.into(), 0x0000_0040, false)], true).is_err());
        // An entry that only applies to files created later does not count for this one.
        assert!(only_trusted_can_write(ADMIN, &[(USER.into(), full, true)], false).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn a_file_the_current_user_made_is_not_trusted() {
        let p = std::env::temp_dir().join(format!("prm-policy-test-{}.json", std::process::id()));
        std::fs::write(&p, "{}").unwrap();
        let r = admin_only(&p);
        let _ = std::fs::remove_file(&p);
        assert!(r.is_err(), "a file in the user's own temp folder must not count as policy");
    }

    #[cfg(windows)]
    #[test]
    fn a_file_only_the_system_controls_is_trusted() {
        let p = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("System32").join("notepad.exe");
        if p.exists() {
            assert_eq!(admin_only(&p), Ok(()), "a system file is owned by TrustedInstaller and only trusted accounts can change it");
        }
    }

    #[test]
    fn policy_only_restricts() {
        let m: serde_json::Map<String, Value> = serde_json::from_value(serde_json::json!({
            "network_enabled": false,
            "api_enabled": true,
            "api_clients": [{"id": "x", "name": "planted", "token_hash": "ab"}],
            "backup_dir": "//server/share",
            "backup_include_files": true,
            "watched_folders": [],
            "linked_bases": {},
            "update_check": false,
            "trash_retention_days": 30,
            "no_such_setting": 1
        }))
        .unwrap();
        let f = policy_filter(m);
        assert_eq!(f.get("network_enabled"), Some(&Value::Bool(false)), "switching lookups off still works");
        assert_eq!(f.get("update_check"), Some(&Value::Bool(false)));
        assert!(f.contains_key("trash_retention_days"));
        for k in ["api_enabled", "api_clients", "backup_dir", "backup_include_files", "watched_folders", "linked_bases", "no_such_setting"] {
            assert!(!f.contains_key(k), "{k} must not come from policy");
        }
        let mut s = Settings::default();
        apply_policy(&mut s, &Policy { source: "test".into(), locked: f, ignored: vec![] });
        assert!(s.api_clients.is_empty() && !s.api_enabled);
    }
}
