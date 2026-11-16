//! Program updates: built in, switched off.
//!
//! The update feed is the project's release page; the public key that verifies update signatures
//! is set here. While either is empty the updater is "not set up": nothing can contact the
//! network and the interface says so. Once set up, checks still follow the network
//! settings, are written to the network log, and happen only when the person asks or (if they
//! turn it on) at most once a day. Downloading and installing always needs the person's go-ahead.

use crate::{AppState, Shared};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

/// Update feed (a JSON manifest). `{{target}}`, `{{arch}}` and `{{current_version}}` are filled in
/// by the updater. "latest" on the release page skips pre-releases.
pub const ENDPOINT: &str = "https://github.com/jasonswrench-beep/prism_reference_manager/releases/latest/download/latest.json";
/// Public half of the update-signing key (key id 176D156BDF837627, from `npx tauri signer generate`).
pub const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDE3NkQxNTZCREY4Mzc2MjcKUldRbmRvUGZheFZ0RjJ3MEJMZExzbXdXN3l5VElISTlNVndWQUxmUTlyTnliRms0eGV3a3EwN2kK";
/// Feed for returning to the previous release (the release process publishes the earlier
/// release's manifest there as previous.json). Signed with the same key.
pub const ROLLBACK_ENDPOINT: &str = "https://github.com/jasonswrench-beep/prism_reference_manager/releases/latest/download/previous.json";

pub const KIND: &str = "update";
const PURPOSE: &str = "Check for a newer version. Sends the program's version and Windows architecture.";

/// Debug builds only: PRM_UPDATE_ENDPOINT and PRM_UPDATE_PUBKEY point the updater at a test feed.
fn endpoint() -> String {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("PRM_UPDATE_ENDPOINT") {
        return v;
    }
    ENDPOINT.to_string()
}

fn pubkey() -> String {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("PRM_UPDATE_PUBKEY") {
        return v;
    }
    PUBKEY.to_string()
}

pub fn configured() -> bool {
    !endpoint().trim().is_empty() && !pubkey().trim().is_empty()
}

fn rollback_endpoint() -> String {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("PRM_ROLLBACK_ENDPOINT") {
        return v;
    }
    ROLLBACK_ENDPOINT.to_string()
}

pub fn rollback_configured() -> bool {
    !rollback_endpoint().trim().is_empty() && !pubkey().trim().is_empty()
}

/// A rollback accepts only a release older than the running one.
pub fn is_rollback(current: &str, offered: &str) -> bool {
    match (semver::Version::parse(current.trim_start_matches('v')), semver::Version::parse(offered.trim_start_matches('v'))) {
        (Ok(c), Ok(o)) => o < c,
        _ => false,
    }
}

fn host() -> String {
    tauri::Url::parse(&endpoint()).ok().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_else(|| "(not set up)".into())
}

pub fn state(st: &AppState) -> Value {
    json!({
        "configured": configured(),
        "automatic": st.settings.update_check,
        "locked": st.policy.locked.contains_key("update_check"),
        "last_check": st.settings.update_last_check,
        "host": host(),
        "version": env!("CARGO_PKG_VERSION"),
        "rollback": rollback_configured(),
    })
}

/// Why a check may not run right now, in plain language.
fn blocked(st: &AppState) -> Option<String> {
    if !configured() {
        return Some("Updates are not set up in this version. Download new versions from the project's release page.".into());
    }
    if !st.settings.network_enabled {
        return Some("Network use is turned off in Settings. Local work is unaffected.".into());
    }
    if st.settings.network_disabled_kinds.iter().any(|k| k == KIND) {
        return Some("Update checks are turned off in Settings › Network.".into());
    }
    None
}

fn log(shared: &Shared, ok: bool, detail: &str) {
    if let Ok(mut st) = shared.lock() {
        st.settings.update_last_check = chrono::Utc::now().to_rfc3339();
        let _ = crate::settings::save(&st.config_dir, &st.settings);
        if let Some(lib) = st.lib.as_ref() {
            lib.store.log_network(KIND, &host(), PURPOSE, &format!("version {}", env!("CARGO_PKG_VERSION")), ok, detail);
        }
    }
}

fn updater(app: &tauri::AppHandle, shared: &Shared) -> Result<tauri_plugin_updater::Updater, String> {
    updater_for(app, shared, &endpoint(), false)
}

fn updater_for(app: &tauri::AppHandle, shared: &Shared, feed: &str, rollback: bool) -> Result<tauri_plugin_updater::Updater, String> {
    let url = tauri::Url::parse(feed).map_err(|e| format!("The update address is not valid: {e}"))?;
    let sh = shared.clone();
    let mut b = app.updater_builder();
    if rollback {
        b = b.version_comparator(|current, remote| is_rollback(&current.to_string(), &remote.version.to_string()));
    }
    b.pubkey(pubkey())
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .on_before_exit(move || {
            // The installer replaces the program; close the library cleanly first.
            if let Ok(st) = sh.lock() {
                if let Some(l) = st.lib.as_ref() {
                    l.close();
                }
                let _ = crate::settings::save(&st.config_dir, &st.settings);
            }
        })
        .build()
        .map_err(|e| e.to_string())
}

/// Ask the feed whether a newer version exists. Runs without holding the state lock.
pub fn check(app: &tauri::AppHandle, shared: &Shared) -> Result<Value, String> {
    if let Some(why) = shared.lock().ok().and_then(|st| blocked(&st)) {
        return Err(why);
    }
    let up = updater(app, shared)?;
    let r = tauri::async_runtime::block_on(up.check());
    match r {
        Ok(Some(u)) => {
            log(shared, true, &format!("Version {} available", u.version));
            Ok(json!({"available": true, "version": u.version, "notes": u.body.clone().unwrap_or_default(), "date": u.date.map(|d| d.to_string()).unwrap_or_default()}))
        }
        Ok(None) => {
            log(shared, true, "Up to date");
            Ok(json!({"available": false}))
        }
        Err(e) => {
            log(shared, false, &e.to_string());
            Err(format!("Could not check for updates: {e}"))
        }
    }
}

/// Download, verify the signature, and run the installer. The program closes when it starts.
pub fn install(app: &tauri::AppHandle, shared: &Shared) -> Result<Value, String> {
    if let Some(why) = shared.lock().ok().and_then(|st| blocked(&st)) {
        return Err(why);
    }
    let up = updater(app, shared)?;
    let found = tauri::async_runtime::block_on(up.check()).map_err(|e| format!("Could not check for updates: {e}"))?;
    let Some(u) = found else { return Ok(json!({"installed": false, "message": "Already up to date."})) };
    let h = app.clone();
    let mut got: usize = 0;
    let res = tauri::async_runtime::block_on(u.download_and_install(
        move |chunk, total| {
            got += chunk;
            let _ = h.emit("update-progress", json!({"received": got, "total": total}));
        },
        || {},
    ));
    match res {
        Ok(()) => {
            log(shared, true, &format!("Installed version {}", u.version));
            Ok(json!({"installed": true, "version": u.version}))
        }
        Err(e) => {
            log(shared, false, &e.to_string());
            Err(format!("The update was not installed: {e}"))
        }
    }
}

/// Return to the previous release: offered only when the rollback feed names an older version,
/// verified with the same signature, and installed only with the person's go-ahead.
pub fn rollback(app: &tauri::AppHandle, shared: &Shared, install: bool) -> Result<Value, String> {
    if !rollback_configured() {
        return Err("Returning to the previous version is not set up in this version.".into());
    }
    if let Some(why) = shared.lock().ok().and_then(|st| blocked(&st)) {
        return Err(why);
    }
    let up = updater_for(app, shared, &rollback_endpoint(), true)?;
    let found = tauri::async_runtime::block_on(up.check()).map_err(|e| format!("Could not reach the release feed: {e}"))?;
    let Some(u) = found else { return Ok(json!({"available": false, "message": "No earlier version is offered."})) };
    if !install {
        log(shared, true, &format!("Previous version {} offered", u.version));
        return Ok(json!({"available": true, "version": u.version, "notes": u.body.clone().unwrap_or_default()}));
    }
    let res = tauri::async_runtime::block_on(u.download_and_install(|_, _| {}, || {}));
    match res {
        Ok(()) => {
            log(shared, true, &format!("Returned to version {}", u.version));
            Ok(json!({"installed": true, "version": u.version}))
        }
        Err(e) => {
            log(shared, false, &e.to_string());
            Err(format!("The earlier version was not installed: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rollback_only_accepts_an_older_release() {
        assert!(super::is_rollback("0.2.0", "0.1.3"));
        assert!(!super::is_rollback("0.2.0", "0.2.0"));
        assert!(!super::is_rollback("0.2.0", "0.3.0"));
        assert!(!super::is_rollback("0.2.0", "not a version"));
    }
}

/// Background check, at most once a day, only when the person turned automatic checks on.
/// Tells the window; never downloads.
pub fn maybe_auto_check(app: &tauri::AppHandle) {
    let Some(shared) = app.try_state::<Shared>().map(|s| s.inner().clone()) else { return };
    let due = match shared.lock() {
        Ok(st) => {
            st.settings.update_check
                && blocked(&st).is_none()
                && chrono::DateTime::parse_from_rfc3339(&st.settings.update_last_check)
                    .map(|t| chrono::Utc::now().signed_duration_since(t) > chrono::Duration::hours(24))
                    .unwrap_or(true)
        }
        Err(_) => false,
    };
    if !due {
        return;
    }
    if let Ok(v) = check(app, &shared) {
        if v["available"].as_bool() == Some(true) {
            let _ = app.emit("update-available", v);
        }
    }
}
