//! #3: the opt-in local agent bridge.
//!
//! Off by default. When the user turns it on in Settings, ai.md
//! listens on 127.0.0.1 (never the network) for line-delimited JSON
//! requests from other tools on this machine, guarded by a token.
//! The port and token are written to `agent-bridge.json` in the
//! library folder so a local agent can find them. Every request is
//! executed on the main thread against the shared store, so the
//! bridge can never race the UI or corrupt the library. Turning the
//! setting off stops the listener and removes the discovery file.
//! Protocol: docs/AGENT_BRIDGE.md.

use gpui::*;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

struct Job {
    request: serde_json::Value,
    reply: mpsc::Sender<String>,
}

static JOBS: Mutex<Option<mpsc::Sender<Job>>> = Mutex::new(None);
static RUNNING: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// Start the lifecycle pump: reconcile the listener with the
/// setting, and run queued requests on the main thread.
pub fn install(cx: &mut App) {
    let (tx, rx) = mpsc::channel::<Job>();
    *JOBS.lock().unwrap() = Some(tx);
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
            let _ = cx.update(|cx| pump(&rx, cx));
        }
    })
    .detach();
}

fn pump(rx: &mpsc::Receiver<Job>, cx: &mut App) {
    reconcile(cx);
    while let Ok(job) = rx.try_recv() {
        let response = execute(&job.request, cx);
        let _ = job.reply.send(response);
    }
}

fn discovery_path() -> std::path::PathBuf {
    fdl_core::store::Store::root_dir().join("agent-bridge.json")
}

fn reconcile(cx: &mut App) {
    let shared = cx.global::<crate::shared_store::SharedStore>().clone();
    let mut s = shared.settings();
    let running = RUNNING.lock().unwrap().is_some();
    if s.agent_bridge && !running {
        if s.agent_bridge_token.is_empty() {
            s.agent_bridge_token = uuid::Uuid::new_v4().simple().to_string();
            if shared.save_settings(&s).is_err() { return; }
        }
        start(&s.agent_bridge_token);
    } else if !s.agent_bridge && running {
        stop();
    }
}

fn start(token: &str) {
    let stop = Arc::new(AtomicBool::new(false));
    *RUNNING.lock().unwrap() = Some(stop.clone());
    let token = token.to_string();
    std::thread::spawn(move || {
        let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
            crate::logging::write("agent bridge: could not bind 127.0.0.1");
            RUNNING.lock().unwrap().take();
            return;
        };
        let _ = listener.set_nonblocking(true);
        let Some(port) = listener.local_addr().ok().map(|a| a.port()) else { return };
        let info = serde_json::json!({ "app": "ai.md", "port": port, "token": token, "pid": std::process::id() });
        if std::fs::write(discovery_path(), serde_json::to_string_pretty(&info).unwrap_or_default()).is_err() {
            crate::logging::write("agent bridge: could not write the discovery file");
        }
        crate::logging::write(&format!("agent bridge: listening on 127.0.0.1:{port}"));
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let token = token.clone();
                    std::thread::spawn(move || handle(stream, &token));
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
            }
        }
    });
}

fn stop() {
    if let Some(stop) = RUNNING.lock().unwrap().take() {
        stop.store(true, Ordering::Relaxed);
    }
    let _ = std::fs::remove_file(discovery_path());
    crate::logging::write("agent bridge: stopped");
}

fn handle(mut stream: TcpStream, token: &str) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(20)));
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.len() > 1_000_000 { return; }
    let response = route(&line, token);
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(b"\n");
}

fn route(line: &str, token: &str) -> String {
    let Ok(req) = serde_json::from_str::<serde_json::Value>(line) else {
        return envelope(serde_json::Value::Null, Err("request is not valid JSON".into()));
    };
    if req.get("token").and_then(|t| t.as_str()) != Some(token) {
        return envelope(req.get("id").cloned().unwrap_or(serde_json::Value::Null), Err("bad token".into()));
    }
    let Some(jobs) = JOBS.lock().unwrap().clone() else {
        return envelope(req.get("id").cloned().unwrap_or(serde_json::Value::Null), Err("bridge is not running".into()));
    };
    let (tx, rx) = mpsc::channel::<String>();
    if jobs.send(Job { request: req, reply: tx }).is_err() {
        return envelope(serde_json::Value::Null, Err("bridge is not running".into()));
    }
    rx.recv_timeout(std::time::Duration::from_secs(15))
        .unwrap_or_else(|_| envelope(serde_json::Value::Null, Err("timed out waiting for the app".into())))
}

fn envelope(id: serde_json::Value, result: Result<serde_json::Value, String>) -> String {
    match result {
        Ok(v) => serde_json::json!({ "id": id, "ok": true, "result": v }).to_string(),
        Err(e) => serde_json::json!({ "id": id, "ok": false, "error": e }).to_string(),
    }
}

/// Run one request against the shared store (main thread).
fn execute(req: &serde_json::Value, cx: &mut App) -> String {
    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(serde_json::Value::Null);
    let shared = cx.global::<crate::shared_store::SharedStore>().clone();
    let result = run(method, &params, shared);
    envelope(id, result)
}

fn run(method: &str, params: &serde_json::Value, mut shared: crate::shared_store::SharedStore) -> Result<serde_json::Value, String> {
    match method {
        "ping" => Ok(serde_json::json!({
            "app": "ai.md",
            "version": env!("CARGO_PKG_VERSION"),
            "methods": ["ping", "list_docs", "read_doc", "search_docs", "search_catalog", "create_doc"],
        })),
        "list_docs" => {
            let docs: Vec<serde_json::Value> = shared.list().iter()
                .map(|e| serde_json::json!({ "id": e.id, "title": e.title, "modified": e.meta.modified }))
                .collect();
            Ok(serde_json::json!(docs))
        }
        "read_doc" => {
            let id = params.get("id").and_then(|v| v.as_str()).ok_or("params.id is required")?;
            let doc = shared.load(id).map_err(|e| format!("could not read that document: {e}"))?;
            let md = crate::markdown::export(&doc, &crate::markdown::Options::default()).text;
            Ok(serde_json::json!({ "id": id, "title": doc.title.text, "markdown": md }))
        }
        "search_docs" => {
            let q = params.get("query").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
            if q.is_empty() { return Err("params.query is required".into()); }
            let hits: Vec<serde_json::Value> = shared.list().iter()
                .filter(|e| e.title.to_lowercase().contains(&q) || e.body.to_lowercase().contains(&q))
                .take(20)
                .map(|e| serde_json::json!({ "id": e.id, "title": e.title }))
                .collect();
            Ok(serde_json::json!(hits))
        }
        "search_catalog" => {
            let q = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
            if q.is_empty() { return Err("params.query is required".into()); }
            let catalog = fdl_core::catalog::load_catalog(crate::catalog_data::ENTRIES)
                .map_err(|e| format!("catalog unavailable: {e}"))?;
            let hits: Vec<serde_json::Value> = catalog.search(q, None).iter()
                .take(20)
                .map(|e| serde_json::json!({ "id": e.id, "name": e.name, "kind": e.kind, "summary": e.summary }))
                .collect();
            Ok(serde_json::json!(hits))
        }
        "create_doc" => {
            let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled");
            let body = params.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let mut doc = crate::model::Document::default();
            doc.title = crate::model::RichText::plain(title.to_string());
            doc.blocks = body.lines().map(crate::model::Block::text).collect();
            let id = shared.create_from(&doc).map_err(|e| format!("could not create the document: {e}"))?;
            Ok(serde_json::json!({ "id": id }))
        }
        _ => Err(format!("unknown method: {method}")),
    }
}
