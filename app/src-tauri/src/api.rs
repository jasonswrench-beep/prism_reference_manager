//! Optional local API. Binds to 127.0.0.1 only, is off by default, and requires a token per
//! client. New clients ask to pair; the person approves or denies in the window. Each client
//! has scopes (read, format, write, import, files, export); revocation takes effect at once.
//! Imported titles, PDFs and notes reaching a client are data, never instructions.
//!
//! POST /v1/pair      {"name": "...", "scopes": ["read","format"]}  -> {"id": "..."}
//! GET  /v1/pair/<id>                                               -> {"state": "...", "token"?: "..."}
//! POST /v1/call      Authorization: Bearer <token>  {"method": "...", "args": {...}}

use crate::Shared;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Read;
use tauri::Emitter;

#[derive(Debug, Clone, Serialize)]
pub struct PairRequest {
    pub id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub origin: String,
    pub state: String,
    #[serde(skip_serializing)]
    pub token: Option<String>,
    pub time: String,
}

const VALID_SCOPES: &[&str] = &["read", "format", "write", "import", "files", "export"];

fn respond(req: tiny_http::Request, code: u16, body: Value) {
    let data = serde_json::to_vec(&body).unwrap_or_default();
    let resp = tiny_http::Response::from_data(data)
        .with_status_code(code)
        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap())
        .with_header(tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap());
    let _ = req.respond(resp);
}

pub fn start(app: tauri::AppHandle, shared: Shared) {
    std::thread::spawn(move || {
        let mut running: Option<(u16, tiny_http::Server)> = None;
        loop {
            let (enabled, port) = {
                let st = shared.lock().unwrap();
                (st.settings.api_enabled, st.settings.api_port)
            };
            match (&running, enabled) {
                (None, true) => {
                    if let Ok(srv) = tiny_http::Server::http(("127.0.0.1", port)) {
                        running = Some((port, srv));
                    }
                }
                (Some((p, _)), true) if *p != port => running = None,
                (Some(_), false) => running = None,
                _ => {}
            }
            let Some((_, srv)) = &running else {
                std::thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            };
            let Ok(Some(mut req)) = srv.recv_timeout(std::time::Duration::from_millis(500)) else { continue };
            let origin = req.headers().iter().find(|h| h.field.equiv("Origin")).map(|h| h.value.as_str().to_string()).unwrap_or_default();
            // Web pages may not call the API: reject any request carrying a web origin.
            if origin.starts_with("http://") || origin.starts_with("https://") {
                respond(req, 403, json!({"error": "Requests from web pages are not allowed."}));
                continue;
            }
            let url = req.url().to_string();
            let method = req.method().clone();
            let mut body = String::new();
            // Captured PDFs arrive base64-encoded, so the body limit allows a 200 MB file.
            let _ = req.as_reader().take(300 * 1024 * 1024).read_to_string(&mut body);
            let auth = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str().trim_start_matches("Bearer ").trim().to_string()).unwrap_or_default();

            if method == tiny_http::Method::Post && url == "/v1/pair" {
                let v: Value = serde_json::from_str(&body).unwrap_or_default();
                let name: String = v["name"].as_str().unwrap_or("Unnamed client").chars().take(80).collect();
                let scopes: Vec<String> = v["scopes"].as_array().map(|a| a.iter().filter_map(|s| s.as_str()).filter(|s| VALID_SCOPES.contains(s)).map(String::from).collect()).unwrap_or_else(|| vec!["read".into()]);
                let pr = PairRequest { id: prm_core::util::new_id(), name, scopes, origin: origin.clone(), state: "pending".into(), token: None, time: prm_core::util::now() };
                let id = pr.id.clone();
                shared.lock().unwrap().pending_pairs.push(pr.clone());
                let _ = app.emit("api-pair", &pr);
                respond(req, 202, json!({"id": id, "state": "pending"}));
                continue;
            }
            if method == tiny_http::Method::Get && url.starts_with("/v1/pair/") {
                let id = url.trim_start_matches("/v1/pair/").to_string();
                let mut st = shared.lock().unwrap();
                let found = st.pending_pairs.iter().position(|p| p.id == id);
                match found {
                    Some(i) => {
                        let p = st.pending_pairs[i].clone();
                        if p.state != "pending" {
                            // A token is handed out exactly once.
                            st.pending_pairs.remove(i);
                        }
                        drop(st);
                        respond(req, 200, json!({"state": p.state, "token": p.token}));
                    }
                    None => {
                        drop(st);
                        respond(req, 404, json!({"error": "Unknown pairing request."}));
                    }
                }
                continue;
            }
            if method == tiny_http::Method::Post && url == "/v1/call" {
                let hash = prm_core::util::sha256_hex(auth.as_bytes());
                let client = {
                    let st = shared.lock().unwrap();
                    st.settings.api_clients.iter().find(|c| !auth.is_empty() && c.token_hash == hash).cloned()
                };
                let Some(client) = client else {
                    respond(req, 401, json!({"error": "Missing or revoked token. Pair this client again."}));
                    continue;
                };
                let v: Value = serde_json::from_str(&body).unwrap_or_default();
                let m = v["method"].as_str().unwrap_or("").to_string();
                let need = crate::dispatch::scope_of(&m);
                let allowed = need != "admin" && client.scopes.iter().any(|s| s == need || (need == "read" && s == "write"));
                {
                    let mut st = shared.lock().unwrap();
                    st.api_log.push(json!({"time": prm_core::util::now(), "client": client.name, "method": m, "allowed": allowed}));
                    if st.api_log.len() > 2000 {
                        st.api_log.drain(..500);
                    }
                    if let Some(c) = st.settings.api_clients.iter_mut().find(|c| c.id == client.id) {
                        c.last_used = prm_core::util::now();
                    }
                }
                if !allowed {
                    respond(req, 403, json!({"error": format!("This client does not have the “{need}” permission for {m}.")}));
                    continue;
                }
                let res = crate::dispatch::dispatch(&app, &shared, &m, v["args"].clone(), Some(&client.name));
                match res {
                    Ok(v) => respond(req, 200, json!({"result": v})),
                    Err(e) => respond(req, 400, json!({"error": e.message})),
                }
                continue;
            }
            respond(req, 404, json!({"error": "Unknown endpoint."}));
        }
    });
}
