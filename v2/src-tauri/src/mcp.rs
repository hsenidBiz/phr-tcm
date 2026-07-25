//! MCP (Model Context Protocol) dispatcher, run in-process via `--mcp`.
//! `handle_message` is pure: JSON-RPC message string in, response string
//! out (None for notifications). The bridge call is injected so tests run
//! without stdio or sockets. `run_stdio_proxy` owns the transport framing
//! (newline-delimited stdio) and the HTTP client that proxies to the app's
//! localhost bridge via the handshake file.

use std::io::{BufRead, Write};

type BridgeCall<'a> = &'a dyn Fn(&str, &str, &str) -> Result<(u16, String), String>;

pub fn handle_message(msg: &str, version: &str, call: BridgeCall) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(msg).ok()?;
    let method = v["method"].as_str()?;
    let id = v.get("id").cloned();
    // Notifications (no id) never get a response.
    id.as_ref()?;
    let id = id.unwrap();

    let result = match method {
        "initialize" => serde_json::json!({
            "protocolVersion": v["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "tcm-testcases", "version": version },
        }),
        "tools/list" => tools_list(),
        "tools/call" => tools_call(&v["params"], call),
        _ => {
            return Some(
                serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": format!("unknown method {method}") },
                })
                .to_string(),
            )
        }
    };
    Some(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
}

/// RFC 3986 percent-encoding for query values: keep ALPHA / DIGIT / `-._~`,
/// escape everything else (spaces, `&`, `%`, unicode bytes, ...). Used for
/// free-text search so ai_bridge::q's naive `&`/`=` splitter can't be
/// confused by separator characters embedded in the search text itself.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn schema(props: serde_json::Value, required: &[&str]) -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": props, "required": required })
}

fn tools_list() -> serde_json::Value {
    serde_json::json!({ "tools": [
        {
            "name": "get_writing_guide",
            "description": "The live guide for writing Test Case Manager import JSON: format rules, the org's allowed Module values, and the recommended workflow. Call this first.",
            "inputSchema": schema(serde_json::json!({}), &[]),
        },
        {
            "name": "get_example_cases",
            "description": "Real existing test cases linked to a PBI, in the exact import JSON shape - mimic their style and granularity.",
            "inputSchema": schema(serde_json::json!({
                "pbi_id": { "type": "integer", "description": "Work item id of the PBI" },
                "limit": { "type": "integer", "description": "Max cases (default 5, cap 20)" },
            }), &["pbi_id"]),
        },
        {
            "name": "validate_cases",
            "description": "Validate draft import JSON with Test Case Manager's REAL importer. Returns case count, warnings, and errors - fix every warning before finishing.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object)" },
            }), &["json"]),
        },
        {
            "name": "search_pbis",
            "description": "Search the current project's PBIs by title text to find the right work item id.",
            "inputSchema": schema(serde_json::json!({
                "query": { "type": "string" },
            }), &["query"]),
        },
        {
            "name": "search_wiki",
            "description": "Search the project's Azure DevOps wiki for documentation; returns page paths and snippet highlights.",
            "inputSchema": schema(serde_json::json!({
                "query": { "type": "string" },
            }), &["query"]),
        },
        {
            "name": "get_wiki_page",
            "description": "Fetch a wiki page's full markdown content - use after search_wiki.",
            "inputSchema": schema(serde_json::json!({
                "wiki_id": { "type": "string" },
                "path": { "type": "string" },
            }), &["wiki_id", "path"]),
        },
    ]})
}

fn tools_call(params: &serde_json::Value, call: BridgeCall) -> serde_json::Value {
    let name = params["name"].as_str().unwrap_or_default();
    let args = &params["arguments"];
    let outcome = match name {
        "get_writing_guide" => call("GET", "/guide", ""),
        "get_example_cases" => {
            let pbi = args["pbi_id"].as_i64().unwrap_or(0);
            let limit = args["limit"].as_i64().unwrap_or(5);
            call("GET", &format!("/examples?pbi={pbi}&limit={limit}"), "")
        }
        "validate_cases" => call("POST", "/validate", args["json"].as_str().unwrap_or("")),
        "search_pbis" => {
            let q = args["query"].as_str().unwrap_or("");
            call("GET", &format!("/search-pbis?q={}", percent_encode(q)), "")
        }
        "search_wiki" => {
            let q = args["query"].as_str().unwrap_or("");
            call("GET", &format!("/search-wiki?q={}", percent_encode(q)), "")
        }
        "get_wiki_page" => {
            let wiki_id = args["wiki_id"].as_str().unwrap_or("");
            let path = args["path"].as_str().unwrap_or("");
            call(
                "GET",
                &format!(
                    "/wiki-page?wiki={}&path={}",
                    percent_encode(wiki_id),
                    percent_encode(path)
                ),
                "",
            )
        }
        other => Err(format!("unknown tool {other}")),
    };
    match outcome {
        Ok((status, body)) if status < 400 => serde_json::json!({
            "content": [{ "type": "text", "text": body }],
        }),
        Ok((status, body)) => serde_json::json!({
            "isError": true,
            "content": [{ "type": "text", "text": format!("bridge returned {status}: {body}") }],
        }),
        Err(e) => serde_json::json!({
            "isError": true,
            "content": [{ "type": "text", "text": format!(
                "Could not reach Test Case Manager ({e}). Start the app and sign in, then retry."
            )}],
        }),
    }
}

fn bridge_call(method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    let hs: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::temp_dir().join("tcm-v2-mcp-bridge.json"))
            .map_err(|_| "handshake file missing - is the app running?".to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let port = hs["port"].as_u64().ok_or("bad handshake file")? as u16;
    let token = hs["token"].as_str().ok_or("bad handshake file")?;

    let client = reqwest::blocking::Client::new();
    let url = format!("http://127.0.0.1:{port}{path}");
    let req = match method {
        "POST" => client.post(&url).body(body.to_string()),
        _ => client.get(&url),
    };
    let resp = req
        .header("x-bridge-token", token)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp.text().map_err(|e| e.to_string())?;
    Ok((status, text))
}

/// The running app's version from the handshake file it already writes for
/// port/token, so `serverInfo.version` matches the real app - not this
/// process's own (unrelated) Cargo.toml version. "unknown" when the app
/// isn't running yet; the proxy must still answer `initialize`.
fn read_version() -> String {
    std::fs::read_to_string(std::env::temp_dir().join("tcm-v2-mcp-bridge.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["version"].as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Entry point for `v2.exe --mcp`: MCP stdio bridge to a RUNNING Test Case
/// Manager. Reads the handshake file for the port + token, proxies each
/// tool call over localhost, and never sees credentials. Register in an AI
/// tool as: `claude mcp add tcm-testcases -- "<install dir>\v2.exe" --mcp`
pub fn run_stdio_proxy() {
    let version = read_version();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_message(&line, &version, &bridge_call) {
            let _ = writeln!(stdout, "{resp}");
            let _ = stdout.flush();
        }
    }
}
