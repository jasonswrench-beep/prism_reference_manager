//! Optional local API. Binds to 127.0.0.1 only, is off by default, and requires a token per
//! client. New clients ask to pair; the person approves or denies in the window. Each client
//! has scopes (read, format, write, import, files, export); revocation takes effect at once.
//! Imported titles, PDFs and notes reaching a client are data, never instructions.
//!
//! POST /v1/pair      {"name": "...", "scopes": ["read","format"]}  -> {"id": "..."}
//! GET  /v1/pair/<id>                                               -> {"state": "...", "token"?: "..."}
//! GET  /v1/status    (no token)  -> {"program", "version", "api_version": 1, "library_open"}
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

/// Largest request bodies: a pairing request is a name and a list of scopes; a call may carry a
/// captured PDF (base64), up to the 200 MB a capture allows.
const PAIR_BODY_LIMIT: u64 = 4 * 1024;
const CALL_BODY_LIMIT: u64 = 300 * 1024 * 1024;
/// Pairing requests waiting for the person's answer at any one time.
const MAX_PENDING_PAIRS: usize = 3;

fn header(req: &tiny_http::Request, name: &'static str) -> String {
    req.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_string()).unwrap_or_default()
}

/// Whether a request may be handled at all, checked before its body is read. Only programs on
/// this computer that address this port directly may call: the Host must be the loopback address
/// with this port (a web page using a name that resolves to 127.0.0.1 is refused), the Origin must
/// be absent or a browser extension's own (web pages and sandboxed frames are refused), and a POST
/// must declare JSON.
pub fn acceptable(host: &str, origin: &str, content_type: &str, is_post: bool, port: u16) -> Result<(), &'static str> {
    let host_ok = host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}");
    if !host_ok {
        return Err("Only programs on this computer may call this address.");
    }
    let origin_ok = origin.is_empty() || origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://") || origin.starts_with("extension://");
    if !origin_ok {
        return Err("Requests from web pages are not allowed.");
    }
    if is_post && !content_type.to_ascii_lowercase().starts_with("application/json") {
        return Err("Send JSON.");
    }
    Ok(())
}

/// The answer to `GET /v1/status`.
pub fn status_body(library_open: bool) -> Value {
    json!({"program": "Prism Reference Manager", "version": env!("CARGO_PKG_VERSION"), "api_version": 1, "library_open": library_open})
}

fn read_body(req: &mut tiny_http::Request, limit: u64) -> Option<String> {
    let mut body = String::new();
    let n = req.as_reader().take(limit + 1).read_to_string(&mut body).ok()?;
    if n as u64 > limit {
        return None;
    }
    Some(body)
}

pub fn start(app: tauri::AppHandle, shared: Shared) {
    std::thread::spawn(move || {
        let mut running: Option<(u16, std::sync::Arc<tiny_http::Server>)> = None;
        loop {
            let (enabled, port) = {
                let st = shared.lock().unwrap_or_else(|e| e.into_inner());
                (st.settings.api_enabled, st.settings.api_port)
            };
            let before = running.as_ref().map(|(p, _)| *p);
            let mut error = None;
            match (&running, enabled) {
                (None, true) => match tiny_http::Server::http(("127.0.0.1", port)) {
                    Ok(srv) => running = Some((port, std::sync::Arc::new(srv))),
                    Err(_) => error = Some(format!("Port {port} could not be opened. Another program may be using it; choose another port.")),
                },
                (Some((p, _)), true) if *p != port => running = None,
                (Some(_), false) => running = None,
                _ => {}
            }
            let now = running.as_ref().map(|(p, _)| *p);
            if now != before || error.is_some() || (!enabled && before.is_none()) {
                let mut st = shared.lock().unwrap_or_else(|e| e.into_inner());
                st.api_listening = now;
                st.api_error = if enabled { error.unwrap_or_default() } else { String::new() };
            }
            let Some((port, srv)) = &running else {
                std::thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            };
            let Ok(Some(req)) = srv.recv_timeout(std::time::Duration::from_millis(500)) else { continue };
            // Each request runs on its own thread, so one slow client cannot hold up the others.
            let (app, shared, port) = (app.clone(), shared.clone(), *port);
            std::thread::spawn(move || handle(req, &app, &shared, port));
        }
    });
}

fn handle(mut req: tiny_http::Request, app: &tauri::AppHandle, shared: &Shared, port: u16) {
    let origin = header(&req, "Origin");
    let is_post = *req.method() == tiny_http::Method::Post;
    if let Err(why) = acceptable(&header(&req, "Host"), &origin, &header(&req, "Content-Type"), is_post, port) {
        respond(req, 403, json!({"error": why}));
        return;
    }
    let url = req.url().to_string();
    let auth = header(&req, "Authorization").trim_start_matches("Bearer ").trim().to_string();

    if *req.method() == tiny_http::Method::Get && url == "/v1/status" {
        // Needs no token, so a program can tell that this is the right port before pairing.
        // It says nothing about the library beyond whether one is open.
        let library_open = shared.lock().unwrap_or_else(|e| e.into_inner()).lib.is_some();
        respond(req, 200, status_body(library_open));
        return;
    }
    if is_post && url == "/v1/pair" {
        let Some(body) = read_body(&mut req, PAIR_BODY_LIMIT) else {
            respond(req, 413, json!({"error": "The pairing request is too large."}));
            return;
        };
        let v: Value = serde_json::from_str(&body).unwrap_or_default();
        let name: String = v["name"].as_str().unwrap_or("Unnamed client").chars().take(80).collect();
        let scopes: Vec<String> = v["scopes"].as_array().map(|a| a.iter().filter_map(|s| s.as_str()).filter(|s| VALID_SCOPES.contains(s)).map(String::from).collect()).unwrap_or_else(|| vec!["read".into()]);
        let pr = PairRequest { id: prm_core::util::new_id(), name, scopes, origin: origin.clone(), state: "pending".into(), token: None, time: prm_core::util::now() };
        let id = pr.id.clone();
        {
            let mut st = shared.lock().unwrap_or_else(|e| e.into_inner());
            if st.pending_pairs.iter().filter(|p| p.state == "pending").count() >= MAX_PENDING_PAIRS {
                drop(st);
                respond(req, 429, json!({"error": "Too many pairing requests are waiting. Answer them in the program first."}));
                return;
            }
            st.pending_pairs.push(pr.clone());
        }
        let _ = app.emit("api-pair", &pr);
        respond(req, 202, json!({"id": id, "state": "pending"}));
        return;
    }
    if *req.method() == tiny_http::Method::Get && url.starts_with("/v1/pair/") {
        let id = url.trim_start_matches("/v1/pair/").to_string();
        let mut st = shared.lock().unwrap_or_else(|e| e.into_inner());
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
        return;
    }
    if is_post && url == "/v1/call" {
        // Who is calling is settled before anything is read from the body.
        let hash = prm_core::util::sha256_hex(auth.as_bytes());
        let client = {
            let st = shared.lock().unwrap_or_else(|e| e.into_inner());
            st.settings.api_clients.iter().find(|c| !auth.is_empty() && c.token_hash == hash).cloned()
        };
        let Some(client) = client else {
            respond(req, 401, json!({"error": "Missing or revoked token. Pair this client again."}));
            return;
        };
        let Some(body) = read_body(&mut req, CALL_BODY_LIMIT) else {
            respond(req, 413, json!({"error": "The request is too large."}));
            return;
        };
        let v: Value = serde_json::from_str(&body).unwrap_or_default();
        let m = v["method"].as_str().unwrap_or("").to_string();
        let need = crate::dispatch::scope_of(&m);
        let allowed = need != "admin" && client.scopes.iter().any(|s| s == need || (need == "read" && s == "write"));
        {
            let mut st = shared.lock().unwrap_or_else(|e| e.into_inner());
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
            return;
        }
        let res = crate::dispatch::dispatch(app, shared, &m, v["args"].clone(), Some(&client.name));
        match res {
            Ok(v) => respond(req, 200, json!({"result": v})),
            Err(e) => respond(req, 400, json!({"error": e.message})),
        }
        return;
    }
    respond(req, 404, json!({"error": "Unknown endpoint."}));
}

#[cfg(test)]
mod tests {
    use super::acceptable;

    #[test]
    fn only_local_programs_and_extensions_may_call() {
        assert!(acceptable("127.0.0.1:27115", "", "application/json", true, 27115).is_ok());
        assert!(acceptable("127.0.0.1:27115", "chrome-extension://abcdef", "application/json; charset=utf-8", true, 27115).is_ok());
        assert!(acceptable("127.0.0.1:27115", "https://evil.example", "application/json", true, 27115).is_err(), "web pages");
        assert!(acceptable("127.0.0.1:27115", "null", "application/json", true, 27115).is_err(), "sandboxed frames");
        assert!(acceptable("rebind.example:27115", "", "application/json", true, 27115).is_err(), "a name that resolves to this computer");
        assert!(acceptable("127.0.0.1:27115", "", "text/plain", true, 27115).is_err(), "a simple cross-site form post");
        assert!(acceptable("127.0.0.1:27115", "", "", false, 27115).is_ok(), "a GET needs no content type");
    }

    #[test]
    fn status_says_only_what_it_should() {
        let v = super::status_body(true);
        assert_eq!(v["api_version"], 1);
        assert_eq!(v["library_open"], true);
        assert_eq!(v["program"], "Prism Reference Manager");
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 4, "no library details: {keys:?}");
    }
}
