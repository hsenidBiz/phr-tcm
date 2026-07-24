//! MCP (Model Context Protocol) dispatcher for the tcm-mcp bridge binary.
//! Pure: JSON-RPC message string in, response string out (None for
//! notifications). The bridge call is injected so tests run without
//! stdio or sockets. Transport framing (newline-delimited stdio) and the
//! HTTP client live in src/bin/tcm_mcp.rs.

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
