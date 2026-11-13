//! MCP shim for the ai.md agent bridge.
//!
//! A standard Model Context Protocol server on stdio that forwards
//! tool calls to ai.md's opt-in local agent bridge (see
//! docs/AGENT_BRIDGE.md). AI clients that speak MCP — Claude, Cursor,
//! VS Code and others — can then read/search the user's library and
//! the bundled catalog, and create documents, with the bridge's own
//! limits: 127.0.0.1 only, token-guarded, read + create, never edit
//! or delete. The shim holds no state: every call re-reads the
//! discovery file, so restarting ai.md never strands it.
//!
//! The shim is a separate binary so release builds can ship it beside
//! ai_md.exe without changing the app.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

const PROTOCOL: &str = "2024-11-05";

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in BufReader::new(stdin).lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() { continue; }
        let Ok(req) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(resp) = handle(&req) else { continue };
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
}

/// One JSON-RPC request -> the response line, or None for a
/// notification (no id), which MCP never answers.
fn handle(req: &serde_json::Value) -> Option<String> {
    let id = req.get("id")?.clone();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let result = match method {
        "initialize" => Ok(serde_json::json!({
            "protocolVersion": PROTOCOL,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "ai.md", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => Ok(serde_json::json!({})),
        "tools/list" => Ok(serde_json::json!({ "tools": tools() })),
        "tools/call" => call_tool(req.get("params")),
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(v) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": v }).to_string(),
        Err((code, msg)) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }).to_string(),
    })
}

fn tools() -> serde_json::Value {
    let obj = |props: serde_json::Value, required: &[&str]| serde_json::json!({
        "type": "object", "properties": props, "required": required,
    });
    let text = serde_json::json!({ "type": "string" });
    serde_json::json!([
        { "name": "list_documents", "description": "List every document in the ai.md library (id, title, modified).",
          "inputSchema": obj(serde_json::json!({}), &[]) },
        { "name": "read_document", "description": "Read one document from the ai.md library as Markdown.",
          "inputSchema": obj(serde_json::json!({ "id": text }), &["id"]) },
        { "name": "search_documents", "description": "Search ai.md document titles and text (up to 20 hits).",
          "inputSchema": obj(serde_json::json!({ "query": text }), &["query"]) },
        { "name": "search_catalog", "description": "Search the catalog of MCP servers, skills, prompts and guides bundled with ai.md (up to 20 hits).",
          "inputSchema": obj(serde_json::json!({ "query": text }), &["query"]) },
        { "name": "create_document", "description": "Create a new document in the ai.md library from plain text (one paragraph per line).",
          "inputSchema": obj(serde_json::json!({ "title": text, "body": text }), &["title", "body"]) },
    ])
}

/// Map one MCP tool call onto the bridge and shape the result as MCP
/// text content. Bridge failures are tool errors, not protocol
/// errors, so the client can show them to the user.
fn call_tool(params: Option<&serde_json::Value>) -> Result<serde_json::Value, (i64, String)> {
    let params = params.ok_or((-32602, "missing params".to_string()))?;
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(serde_json::json!({}));
    let (method, forwarded) = match name {
        "list_documents" => ("list_docs", serde_json::json!({})),
        "read_document" => ("read_doc", args),
        "search_documents" => ("search_docs", args),
        "search_catalog" => ("search_catalog", args),
        "create_document" => ("create_doc", args),
        _ => return Ok(tool_error(&format!("unknown tool: {name}"))),
    };
    match bridge(method, &forwarded) {
        Ok(v) => Ok(serde_json::json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
        })),
        Err(e) => Ok(tool_error(&e)),
    }
}

fn tool_error(message: &str) -> serde_json::Value {
    serde_json::json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

/// One bridge round-trip: read the discovery file, send one line,
/// read one line back.
fn bridge(method: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let root = fdl_core::store::Store::root_dir();
    let raw = std::fs::read_to_string(root.join("agent-bridge.json")).map_err(|_| {
        "ai.md's agent bridge is off. Open ai.md, go to Settings > General, turn on the local agent bridge, and try again.".to_string()
    })?;
    let info: serde_json::Value = serde_json::from_str(&raw).map_err(|_| "the agent bridge discovery file is unreadable; toggle the bridge off and on in ai.md's Settings.".to_string())?;
    let port = info.get("port").and_then(|p| p.as_u64()).ok_or("the agent bridge discovery file has no port; toggle the bridge off and on in ai.md's Settings.")?;
    let token = info.get("token").and_then(|t| t.as_str()).unwrap_or("");
    let mut stream = TcpStream::connect(("127.0.0.1", port as u16))
        .map_err(|_| "could not reach ai.md on this machine — is the app running with the bridge on?".to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let request = serde_json::json!({ "id": 1, "token": token, "method": method, "params": params });
    stream.write_all(request.to_string().as_bytes()).and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| format!("could not send the request to ai.md: {e}"))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| format!("could not read ai.md's reply: {e}"))?;
    let resp: serde_json::Value = serde_json::from_str(&line).map_err(|_| "ai.md sent an unreadable reply.".to_string())?;
    if resp.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        Ok(resp.get("result").cloned().unwrap_or(serde_json::Value::Null))
    } else {
        Err(resp.get("error").and_then(|e| e.as_str()).unwrap_or("ai.md reported an unknown error.").to_string())
    }
}
