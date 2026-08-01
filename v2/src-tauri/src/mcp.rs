//! MCP (Model Context Protocol) dispatcher, run in-process via `--mcp`.
//! `handle_message` is pure: JSON-RPC message string in, response string
//! out (None for notifications). The bridge call is injected so tests run
//! without stdio or sockets. `run_stdio_proxy` owns the transport framing
//! (newline-delimited stdio) and the HTTP client that proxies to the app's
//! localhost bridge via the handshake file.

use std::io::{BufRead, Write};

type BridgeCall<'a> = &'a dyn Fn(&str, &str, &str) -> Result<(u16, String), String>;

/// A JSON-RPC error object, for the cases where there is nothing else to
/// say. `id` is null when the message could not be parsed far enough to
/// find one - which is what the protocol prescribes, and is still an
/// answer: returning None left the client waiting on an id forever.
fn rpc_error(id: serde_json::Value, code: i32, message: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

pub fn handle_message(msg: &str, version: &str, call: BridgeCall) -> Option<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(msg) else {
        return Some(rpc_error(serde_json::Value::Null, -32700, "Parse error"));
    };
    let id = v.get("id").cloned();
    // Notifications (no id) never get a response - that part was right.
    id.as_ref()?;
    let id = id.unwrap();
    let Some(method) = v["method"].as_str() else {
        // Has an id, so it is a request and something must come back.
        return Some(rpc_error(id, -32600, "Invalid Request: no method"));
    };

    let result = match method {
        "initialize" => serde_json::json!({
            "protocolVersion": v["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "tcm-testcases", "version": version },
        }),
        "tools/list" => tools_list(disabled(call)),
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

/// Tools the user has switched off in the app. Asked fresh on every
/// `tools/list`, so a toggle takes effect without restarting the editor.
/// A bridge that can't be reached disables nothing - losing the whole
/// toolset because the app is closed would be worse than showing tools
/// that then say "sign in first".
fn disabled(call: BridgeCall) -> Vec<String> {
    let Ok((status, body)) = call("GET", "/tools", "") else {
        return vec![];
    };
    if status != 200 {
        return vec![];
    }
    serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v["disabled"].as_array().cloned())
        .map(|a| {
            a.iter()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn tools_list(disabled: Vec<String>) -> serde_json::Value {
    let all = serde_json::json!({ "tools": [
        {
            "name": "begin_test_case_writing",
            "description": "START HERE for any test-case writing job, before reading specs or drafting anything. Call it with only `feature` first: it returns the questions to put to the developer in chat (where the JSON goes, which spec documents are authoritative, whether to check a PBI for duplicates, tags/module/status, what is out of scope) plus this org's real Module values. Ask them those questions - do not answer them yourself - then call this tool again with their answers. It checks the paths and values actually exist, and returns a plan file for them to approve. Do not write a single test case until it returns status \"ready\" and the developer has agreed to the plan.",
            "inputSchema": schema(serde_json::json!({
                "feature": { "type": "string", "description": "What you are about to write cases for, e.g. \"Manager Assessment landing page\" - becomes the plan's title" },
                "output_path": { "type": "string", "description": "Full path, including file name, where the finished JSON goes" },
                "spec_paths": { "type": "array", "items": { "type": "string" }, "description": "Full paths to the specification documents the cases come from" },
                "sections": { "type": "string", "description": "Which parts of those documents are in scope" },
                "authority": { "type": "string", "description": "\"spec\", \"app\", or \"spec-wins\" - which source decides when they disagree" },
                "ordering": { "type": "string", "description": "\"spec\" (cases walk down the specification) or \"tester\" (grouped so the tester changes environment as little as possible)" },
                "examples_pbi": { "type": "integer", "description": "PBI holding existing cases to learn style from and check for duplicates" },
                "check_examples": { "type": "boolean", "description": "Whether to read those existing cases at all" },
                "tags": { "type": "string", "description": "Semicolon-separated" },
                "module": { "type": "string", "description": "Must be one of the org's allowed Module values" },
                "automation_status": { "type": "string", "description": "\"Not Automated\" or \"Planned\"" },
                "out_of_scope": { "type": "string", "description": "Anything explicitly not to cover" },
                "notes": { "type": "string", "description": "House rules, naming, granularity - the developer's own conventions" },
            }), &[]),
        },
        {
            "name": "get_writing_guide",
            "description": "The live guide for writing Test Case Manager import JSON: format rules, the org's allowed Module values, and the recommended workflow. Call this first.",
            "inputSchema": schema(serde_json::json!({}), &[]),
        },
        {
            "name": "get_test_cases",
            "description": "The test cases already linked to a PBI, in the exact import JSON shape. Use them to copy the house style, to check what is already covered before writing more, or just to read what a PBI is currently tested by.",
            "inputSchema": schema(serde_json::json!({
                "pbi_id": { "type": "integer", "description": "Work item id of the PBI" },
                "limit": { "type": "integer", "description": "Max cases (default 5, cap 20)" },
                "offset": { "type": "integer", "description": "Skip this many cases - page through a PBI with more than the cap" },
                "titles_only": { "type": "boolean", "description": "Return only ids and titles (cap 200) - use for duplicate checking instead of pulling full step text" },
            }), &["pbi_id"]),
        },
        {
            "name": "optimize_cases",
            "description": "Reorganise a draft into a run sheet the tester can work straight through: navigation spelled out as explicit steps (not hidden in preconditions), expected results reduced to the outcome alone, and cases ordered so the tester changes environment/options as few times as possible. Returns the new JSON plus a report. Call this once on your finished draft instead of hand-tuning it.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object)" },
                "entry": { "type": "string", "description": "First step of every preamble, e.g. \"Launch the HRM portal.\" (default: \"Launch the application.\"). A non-launch entry (e.g. opening a module) is placed AFTER the sign-in step." },
                "dry_run": { "type": "boolean", "description": "Return only the report of what would change - inspect it before committing to the transformed JSON" },
                "reorder": { "type": "boolean", "description": "Default true: regroup the cases so the tester changes environment as little as possible. Pass false for a set that is meant to be read against the specification in document order - navigation and expected results are still cleaned up, the order is left alone." },
            }), &["json"]),
        },
        {
            "name": "transform_cases",
            "description": "Apply bulk edits to a draft without rewriting it yourself: retag, retitle, set module or automation status, find/replace inside steps, sort, dedupe. Each operation takes an optional `where` filter. Use this instead of writing a script to reshape the JSON.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object)" },
                "operations": {
                    "type": "array",
                    "description": "Ops applied in order. Each: {op, value?, find?, replace?, action?, expected?, cases?, where?}. op is one of set_tags, add_tags, remove_tags, set_module, set_automation_status, set_preconditions, replace_in_title, prefix_title, suffix_title, replace_in_steps, prepend_step, append_step, remove_step_matching, sort_by, group_by (stable - keeps within-group order), dedupe, remove_cases (where filter required), insert_cases. sort_by/group_by take title, module, tags or preconditions. `where` may carry title_contains, has_tag, module_is.",
                    "items": { "type": "object" },
                },
            }), &["json", "operations"]),
        },
        {
            "name": "validate_cases",
            "description": "Validate draft import JSON with Test Case Manager's REAL importer. Returns case count, warnings, and errors - fix every warning before finishing. For large drafts pass `path` (a local file) instead of inlining the JSON; never skip validation because the draft is too big to inline.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object)" },
                "path": { "type": "string", "description": "Absolute path to a local draft file - use this instead of `json` for large drafts" },
            }), &[]),
        },
        {
            "name": "get_tags",
            "description": "The tag names this project already uses. Prefer an existing tag over inventing a near-duplicate. Served from the app's cache - calling this costs no Azure DevOps request.",
            "inputSchema": schema(serde_json::json!({
                "query": { "type": "string", "description": "Case-insensitive substring filter - a project can carry thousands of tags, so filter rather than fetching all of them" },
            }), &[]),
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
    ]});
    let tools: Vec<serde_json::Value> = all["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| {
            let name = t["name"].as_str().unwrap_or_default();
            !disabled.iter().any(|d| d == name)
        })
        .collect();
    serde_json::json!({ "tools": tools })
}

fn tools_call(params: &serde_json::Value, call: BridgeCall) -> serde_json::Value {
    let name = params["name"].as_str().unwrap_or_default();
    let args = &params["arguments"];
    // Checked again here, not just in tools/list: a client may be working
    // from a list it cached before the tool was switched off.
    if disabled(call).iter().any(|d| d == name) {
        return serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "The `{name}` tool is switched off in Test Case Manager.                      Turn it back on in the app's AI Bridge tab if you need it."
                ),
            }],
            "isError": true,
        });
    }
    let outcome = match name {
        "begin_test_case_writing" => {
            let feature = args["feature"].as_str().unwrap_or("");
            let target = if feature.is_empty() {
                "/begin".to_string()
            } else {
                format!("/begin?feature={}", percent_encode(feature))
            };
            // Everything except `feature` is an answer. With none of them
            // present this is phase 1 (the questions); with any of them it
            // is phase 2 (check and plan).
            let answers: serde_json::Map<String, serde_json::Value> = args
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter(|(k, _)| k.as_str() != "feature")
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let body = if answers.is_empty() {
                String::new()
            } else {
                serde_json::Value::Object(answers).to_string()
            };
            call("POST", &target, &body)
        }
        "get_writing_guide" => call("GET", "/guide", ""),
        "get_test_cases" => {
            let pbi = args["pbi_id"].as_i64().unwrap_or(0);
            let limit = args["limit"].as_i64().unwrap_or(5);
            let offset = args["offset"].as_i64().unwrap_or(0);
            let mut target = format!("/test-cases?pbi={pbi}&limit={limit}&offset={offset}");
            if args["titles_only"].as_bool().unwrap_or(false) {
                target.push_str("&titles_only=true");
            }
            call("GET", &target, "")
        }
        "optimize_cases" => {
            let entry = args["entry"].as_str().unwrap_or("");
            let mut params: Vec<String> = vec![];
            if !entry.is_empty() {
                params.push(format!("entry={}", percent_encode(entry)));
            }
            if args["dry_run"].as_bool().unwrap_or(false) {
                params.push("dry_run=true".to_string());
            }
            // Only sent when explicitly false - the bridge defaults to
            // reordering, and an absent flag has to mean the same thing.
            if args["reorder"].as_bool() == Some(false) {
                params.push("reorder=false".to_string());
            }
            let target = if params.is_empty() {
                "/optimize".to_string()
            } else {
                format!("/optimize?{}", params.join("&"))
            };
            call("POST", &target, args["json"].as_str().unwrap_or(""))
        }
        "validate_cases" => {
            let target = match args["path"].as_str().filter(|p| !p.trim().is_empty()) {
                Some(p) => format!("/validate?path={}", percent_encode(p)),
                None => "/validate".to_string(),
            };
            call("POST", &target, args["json"].as_str().unwrap_or(""))
        }
        "transform_cases" => {
            // The bridge takes one body, so the draft and the ops travel
            // together rather than as a query string.
            let body = serde_json::json!({
                "test_cases": args["json"].as_str().unwrap_or(""),
                "operations": args["operations"].clone(),
            });
            call("POST", "/transform", &body.to_string())
        }
        "get_tags" => {
            let target = match args["query"].as_str().filter(|f| !f.trim().is_empty()) {
                Some(f) => format!("/tags?query={}", percent_encode(f)),
                None => "/tags".to_string(),
            };
            call("GET", &target, "")
        }
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
        &std::fs::read_to_string(crate::ai_bridge::handshake_path())
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
    std::fs::read_to_string(crate::ai_bridge::handshake_path())
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
