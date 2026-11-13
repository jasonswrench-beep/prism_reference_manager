//! Global quick-copy launcher (spec WRT-019). Optional and off until the person chooses a
//! system-wide shortcut. The shortcut opens a small window over whatever program is in front:
//! search a source, pick the citation form, press Enter. The citation is written to the clipboard
//! with the program's own clipboard code (the same tested write as everywhere else); nothing is
//! typed into the other program. The window then hides, so the program that was in front gets
//! focus back and the person pastes.

use crate::Shared;
use tauri::Manager;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub const LABEL: &str = "quickcopy";

/// Open (or bring back) the launcher window.
pub fn show(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = tauri::Emitter::emit_to(&w, LABEL, "quickcopy-open", ());
        return;
    }
    if let Ok(w) = tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("quickcopy.html".into()))
        .title("Quick copy — Prism Reference Manager")
        .inner_size(600.0, 440.0)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .build()
    {
        if let Some(i) = app.default_window_icon().cloned() {
            let _ = w.set_icon(i);
        }
        let _ = w.set_focus();
    }
}

pub fn hide(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

/// Register the chosen shortcut (empty turns the launcher off). Fails, with a plain message, when
/// the combination cannot be used or another program already holds it.
pub fn apply(app: &tauri::AppHandle, shortcut: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let s = shortcut.trim();
    if s.is_empty() {
        return Ok(());
    }
    let parsed: tauri_plugin_global_shortcut::Shortcut = s.parse().map_err(|_| format!("{s} cannot be used as a system-wide shortcut."))?;
    gs.register(parsed).map_err(|_| format!("{s} is already used by Windows or another program. Choose another combination."))
}

/// At start: register the saved shortcut, if any.
pub fn start(app: &tauri::AppHandle, shared: &Shared) {
    let s = shared.lock().map(|st| st.settings.quick_copy_shortcut.clone()).unwrap_or_default();
    if !s.is_empty() {
        let _ = apply(app, &s);
    }
}
