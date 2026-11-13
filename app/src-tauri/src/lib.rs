//! Prism Reference Manager desktop shell.
//!
//! The window calls one command, `call(method, args)`, which dispatches to validated core
//! operations. The optional local API (loopback only, token scopes) uses the same dispatcher.

mod api;
mod clipboard;
mod dispatch;
mod settings;

use prm_core::library::Library;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub lib: Option<Library>,
    pub settings: settings::Settings,
    pub policy: settings::Policy,
    pub config_dir: PathBuf,
    pub device: String,
    pub host: String,
    pub pending_pairs: Vec<api::PairRequest>,
    pub api_log: Vec<serde_json::Value>,
    pub jobs: Vec<serde_json::Value>,
}

pub type Shared = Arc<Mutex<AppState>>;

#[derive(serde::Serialize)]
pub struct CallError {
    pub message: String,
    pub detail: String,
}

/// The single command the interface uses.
#[tauri::command]
async fn call(app: tauri::AppHandle, state: tauri::State<'_, Shared>, method: String, args: serde_json::Value) -> Result<serde_json::Value, CallError> {
    let shared = state.inner().clone();
    let r = tauri::async_runtime::spawn_blocking(move || dispatch::dispatch(&app, &shared, &method, args, None)).await;
    match r {
        Ok(v) => v,
        Err(e) => Err(CallError { message: "The operation stopped unexpectedly.".into(), detail: e.to_string() }),
    }
}

/// Binary file contents for the reader (PDF bytes) without JSON encoding.
#[tauri::command]
async fn read_attachment(state: tauri::State<'_, Shared>, id: String) -> Result<tauri::ipc::Response, CallError> {
    let shared = state.inner().clone();
    let r = tauri::async_runtime::spawn_blocking(move || -> Result<Vec<u8>, CallError> {
        let st = shared.lock().unwrap();
        let lib = st.lib.as_ref().ok_or(CallError { message: "No library is open.".into(), detail: String::new() })?;
        let a = lib.store.must::<prm_core::model::Attachment>(&id).map_err(dispatch::err)?;
        let p = lib.attachment_path(&a);
        std::fs::read(&p).map_err(|e| CallError { message: "The file could not be opened. It may have moved; use Relink to find it.".into(), detail: format!("{}: {e}", p.display()) })
    })
    .await
    .map_err(|e| CallError { message: "The file could not be read.".into(), detail: e.to_string() })??;
    Ok(tauri::ipc::Response::new(r))
}

/// Binary contents of a file the person picked in a dialog (for a replacement version).
#[tauri::command]
async fn read_picked(path: String) -> Result<tauri::ipc::Response, CallError> {
    let p = PathBuf::from(&path);
    if !dispatch::picked_paths().lock().unwrap().contains(&p) {
        return Err(CallError { message: "Only files chosen in a dialog can be read.".into(), detail: String::new() });
    }
    let bytes = std::fs::read(&p).map_err(|e| CallError { message: "The file could not be read.".into(), detail: e.to_string() })?;
    Ok(tauri::ipc::Response::new(bytes))
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "this computer".into())
}

fn device_id(dir: &std::path::Path) -> String {
    let p = dir.join("device-id");
    if let Ok(s) = std::fs::read_to_string(&p) {
        if !s.trim().is_empty() {
            return s.trim().to_string();
        }
    }
    let id = prm_core::util::new_id();
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&p, &id);
    id
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![call, read_attachment, read_picked])
        .setup(|app| {
            let default_cfg = app.path().app_config_dir().unwrap_or_else(|_| PathBuf::from("."));
            let config_dir = settings::config_dir(default_cfg);
            std::fs::create_dir_all(&config_dir).ok();
            let mut s = settings::load(&config_dir);
            let policy = settings::load_policy();
            settings::apply_policy(&mut s, &policy);
            let device = device_id(&config_dir);
            let state: Shared = Arc::new(Mutex::new(AppState {
                lib: None,
                settings: s,
                policy,
                config_dir,
                device,
                host: hostname(),
                pending_pairs: vec![],
                api_log: vec![],
                jobs: vec![],
            }));
            app.manage(state.clone());

            // Background: library heartbeat, mirror catch-up, trash retention, scheduled backup.
            let st2 = state.clone();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
              let mut last_bib_seq: i64 = -1;
              loop {
                std::thread::sleep(std::time::Duration::from_secs(10));
                let mut msg = None;
                if let Ok(mut st) = st2.lock() {
                    let s = st.settings.clone();
                    if let Some(lib) = st.lib.as_mut() {
                        let _ = lib.heartbeat();
                        let _ = lib.store.advance_mirror();
                        // Keep automatically refreshed BibTeX files current.
                        let seq = lib.last_event_seq();
                        if seq != last_bib_seq {
                            for b in &s.bib_exports {
                                let _ = lib.write_bib_file(&b.project_id, std::path::Path::new(&b.path), b.biblatex);
                            }
                            last_bib_seq = seq;
                        }
                        let due = s.auto_backup
                            && (s.last_backup.is_empty()
                                || chrono::DateTime::parse_from_rfc3339(&s.last_backup).map(|t| chrono::Utc::now().signed_duration_since(t).num_hours() >= s.backup_interval_hours as i64).unwrap_or(true));
                        if due {
                            msg = Some(match dispatch::scheduled_backup(lib, &s) {
                                Ok(p) => serde_json::json!({"kind": "backup", "ok": true, "message": format!("Backup saved: {p}")}),
                                Err(e) => serde_json::json!({"kind": "backup", "ok": false, "message": format!("Scheduled backup failed: {e}")}),
                            });
                        }
                    }
                    if let Some(m) = &msg {
                        if m["ok"].as_bool() == Some(true) {
                            st.settings.last_backup = prm_core::util::now();
                            let _ = settings::save(&st.config_dir, &st.settings);
                        }
                        st.jobs.push(m.clone());
                    }
                }
                if let Some(m) = msg {
                    let _ = handle.emit("job", m);
                }
              }
            });

            // Optional local API.
            api::start(app.handle().clone(), state.clone());

            let icon = app.default_window_icon().cloned();
            let win = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
                .title("Prism Reference Manager")
                .inner_size(1440.0, 920.0)
                .min_inner_size(760.0, 560.0)
                .disable_drag_drop_handler()
                .build()?;
            if let Some(i) = icon {
                let _ = win.set_icon(i);
            }
            // Debug builds only: run an end-to-end test script inside the window.
            #[cfg(debug_assertions)]
            if let Ok(p) = std::env::var("PRM_TEST_SCRIPT") {
                let w = win.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if let Ok(js) = std::fs::read_to_string(&p) {
                        let _ = w.eval(&js);
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.try_state::<Shared>() {
                    if let Ok(st) = state.lock() {
                        if let Some(lib) = st.lib.as_ref() {
                            lib.close();
                        }
                        let _ = settings::save(&st.config_dir, &st.settings);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Prism Reference Manager");
}
