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
            "description": "START HERE for any test-case writing job, before reading specs or drafting anything. Call it with only `feature` first: it returns the questions to put to the developer in chat (what the file is called, which spec documents are authoritative, whether to check a PBI for duplicates, tags/module/status, what is out of scope) plus this org's real Module values. Ask them those questions - do not answer them yourself - then call this tool again with their answers. It checks the paths and values actually exist, and returns a plan file for them to approve. Do not write a single test case until it returns status \"ready\" and the developer has agreed to the plan.",
            "inputSchema": schema(serde_json::json!({
                "feature": { "type": "string", "description": "What you are about to write cases for, e.g. \"Manager Assessment landing page\" - becomes the plan's title" },
                "output_path": { "type": "string", "description": "File name for the finished JSON (e.g. login.json) — it goes in the working repository's .test-cases folder; a path outside that folder is refused" },
                "spec_paths": { "type": "array", "items": { "type": "string" }, "description": "Full paths to the specification documents the cases come from" },
                "sections": { "type": "string", "description": "Which parts of those documents are in scope" },
                "authority": { "type": "string", "description": "\"spec\", \"app\", or \"spec-wins\" - which source decides when they disagree" },
                "ordering": { "type": "string", "description": "\"spec\" (cases walk down the specification) or \"tester\" (grouped so the tester changes environment as little as possible)" },
                "reference_cases": { "type": "string", "description": "The developer's answer to \"any reference test cases to model on?\" - a PBI id, a file path, prose, or \"none\". Required: asked on every writing job, and \"none\" is the way to say no" },
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
            "name": "get_run_failures",
            "description": "The failed cases from a PBI's latest test runs, each with the tester's failure comment and any bugs they linked. Use this to write regression cases for what actually broke: read the failure, read the failed case itself with get_test_cases, then extend the coverage rather than restating it.",
            "inputSchema": schema(serde_json::json!({
                "pbi_id": { "type": "integer", "description": "Work item id of the PBI whose runs to read" },
            }), &["pbi_id"]),
        },
        {
            "name": "check_spec_coverage",
            "description": "Reports coverage as findings to read and account for, not as pass/fail - a partial draft is a normal state, not an error. Joins a draft's `Spec:` citations against one or more spec documents and returns which sections have no case yet (`uncovered`), which cases could not be attributed to any section, which citations point at a section or file that does not exist, which quoted text was not found in the document, which citations carry no quote and no exemption (`cited_without_quote` - fix these here, do not wait for validate_cases), and which sections are excluded by the plan's own scope. A citation may stop short of a heading's trailing parenthetical, and `;` may join a second document's pointer onto the same Spec: line - both resolve. AC-level sections (e.g. \"8.2 (AC-2)\") are reported individually - a covered parent section does not silence its acceptance criteria. Run before optimize_cases so gaps are found while the draft is still easy to extend.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object) - use this or `path`, never both" },
                "path": { "type": "string", "description": "Absolute path to a local draft file - use this or `json`, never both" },
                "spec_paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Absolute paths to the specification documents the draft cites",
                },
                "sections": { "type": "string", "description": "Which sections are in scope for this batch, if narrower than the whole document" },
                "out_of_scope": { "type": "string", "description": "Anything explicitly not covered by this batch, e.g. \"7.4 is deferred to phase 2\"" },
            }), &["spec_paths"]),
        },
        {
            "name": "merge_case_files",
            "description": "Merge slice files from a fan-out into one draft through the real importer - never merge by hand. Reads every path in `paths` with the same importer the app uses, concatenates the cases in that order, and writes the result to `output_path` (refused if that path already exists - pick a new one rather than overwriting). Returns the merged case count, a per-file breakdown, and the importer's warnings from every slice, each prefixed with the slice file it came from. A title appearing in more than one slice is warned about by name - fan-out writers cannot see each other's titles, and such a collision usually needs disambiguating, not deduping. Does not deduplicate - if slices may overlap, run optimize_cases or transform_cases' dedupe on the merged file afterward.",
            "inputSchema": schema(serde_json::json!({
                "paths": { "type": "array", "items": { "type": "string" }, "description": "Absolute paths to the slice files, in the order they should be concatenated" },
                "output_path": { "type": "string", "description": "File name for the merged draft (e.g. login.json) - it goes in the working repository's .test-cases folder, and must not already exist; a path outside that folder is refused" },
            }), &["paths", "output_path"]),
        },
        {
            "name": "get_autorun_guide",
            "description": "How to write an Auto Run action script: the browser actions the runner understands, the selector forms, and - the part that matters - which source is allowed to decide what. Read this before writing a script. You may read the application's source for SELECTORS, but every assertion comes from the test case's own expected result, never from what the code happens to do.",
            "inputSchema": schema(serde_json::json!({}), &[]),
        },
        {
            "name": "save_autorun_script",
            "description": "Save action scripts so the app can drive those test cases through a real browser. Takes a LIST, so one call can cover a whole PBI. Each entry is { case_id, title, steps: [{ step_number, actions }] }. All or nothing: one bad action kind rejects the whole batch rather than leaving half the cases updated. Call get_autorun_guide first for the action vocabulary.",
            "inputSchema": schema(serde_json::json!({
                "scripts": {
                    "type": "array",
                    "description": "One entry per test case: { case_id, title, steps }",
                    "items": { "type": "object" },
                },
            }), &["scripts"]),
        },
        {
            "name": "optimize_cases",
            "description": "Reorganise a draft into a run sheet the tester can work straight through: navigation spelled out as explicit steps (not hidden in preconditions), expected results reduced to the outcome alone, and cases ordered so the tester changes environment/options as few times as possible. Every case comes back stamped with BOTH orders - spec_order (the order you wrote, following the document) and tester_order (the grouped run sequence) - so keep those fields as returned; the app flips between the two readings. Returns the new JSON plus a report. Call this once on your finished draft instead of hand-tuning it. For large drafts pass `path` (a local file) instead of inlining the JSON, and `in_place: true` to write the result back to that file and get only the report - NEVER shard a draft to fit it inline: tester_order is one sequence across the whole set, and per-shard orderings cannot be stitched together.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object). Use `path` instead for large drafts - never both." },
                "path": { "type": "string", "description": "Absolute path to the draft file - use this instead of `json` for large drafts." },
                "in_place": { "type": "boolean", "description": "With `path`: write the optimized draft back to the same file (atomic) and return only the report, skipping the JSON echo." },
                "entry": { "type": "string", "description": "First step of every preamble, e.g. \"Launch the HRM portal.\" (default: \"Launch the application.\"). A non-launch entry (e.g. opening a module) is placed AFTER the sign-in step." },
                "dry_run": { "type": "boolean", "description": "Return only the report of what would change - inspect it before committing to the transformed JSON" },
                "reorder": { "type": "boolean", "description": "Default true: regroup the cases so the tester changes environment as little as possible. Pass false for a set that is meant to be read against the specification in document order - navigation and expected results are still cleaned up, the order is left alone." },
            }), &[]),
        },
        {
            "name": "transform_cases",
            "description": "Apply bulk edits to a draft without rewriting it yourself: retag, retitle, set module or automation status, find/replace inside steps, split a step in two, sort, dedupe, insert at a position. Each operation takes an optional `where` filter. Pass `path` instead of `json` for large drafts, and `in_place: true` to write the result back to that file. The report's `applied` lines give the count of cases actually MODIFIED for find-driven ops, its `warnings` list what an op declined to do, and its `ignored` list everything you passed that was not used - read all three.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object). Use `path` instead for large drafts - never both." },
                "path": { "type": "string", "description": "Absolute path to the draft file - use this instead of `json` for large drafts." },
                "in_place": { "type": "boolean", "description": "With `path`: write the transformed draft back to the same file (atomic) and skip echoing the JSON." },
                "operations": {
                    "type": "array",
                    "description": "Ops applied in order. WHICH KEYS EACH OP READS: set_tags/add_tags/remove_tags/set_module/set_automation_status/set_preconditions/set_reviewer_notes/prefix_title/suffix_title/sort_by/group_by take {value}; replace_in_title/replace_in_steps/replace_in_notes take {find, replace} (replace_in_notes edits the local reviewer_notes - the bulk repair for check_spec_coverage findings); normalise_citations takes only where - it moves a Spec: line above its blockquote and quotes it, or writes the exemption form for a table/code block, and leaves anything with two pointers or two blocks for a person; prepend_step/append_step take {action, expected}; remove_step_matching takes {value|find|action} (substring against step actions); split_step takes {find, into: [{action, expected}, ...]} and replaces each matching step with that sequence; remove_cases takes only a required `where`; insert_cases takes {cases, and optionally ONE of at_index (zero-based) | before | after (a title fragment)} - without one it appends; dedupe takes nothing (first copy wins, no merge). Every op accepts `where` with title_contains/has_tag/module_is/at_index - at_index (zero-based position in the current draft) is the selector of last resort when two cases share a title. sort_by/group_by values: title, module, tags, preconditions. A key an op does not read is reported in `ignored`, never silently dropped.",
                    "items": { "type": "object" },
                },
            }), &["operations"]),
        },
        {
            "name": "validate_cases",
            "description": "Validate draft import JSON with Test Case Manager's REAL importer. Returns case count, warnings, and errors - fix every warning before finishing. It may also return `advisories`: judgement calls (such as a case that looks like it merges a positive and its negative). Do not treat those as defects - read each one, decide, and tell the developer what you decided. For large drafts pass `path` (a local file) instead of inlining the JSON; never skip validation because the draft is too big to inline.",
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
        "get_run_failures" => {
            let pbi = args["pbi_id"].as_i64().unwrap_or(0);
            call("GET", &format!("/run-failures?pbi={pbi}"), "")
        }
        "check_spec_coverage" => call("POST", "/check-coverage", &args.to_string()),
        // The bridge takes one body, so forwarding the raw arguments object
        // is structurally unable to drop `paths` or `output_path` - the
        // same pattern as `check_spec_coverage` above.
        "merge_case_files" => call("POST", "/merge-cases", &args.to_string()),
        "get_autorun_guide" => call("GET", "/autorun-guide", ""),
        "save_autorun_script" => {
            // The bridge takes the bare array, so a caller that wrapped it
            // in {scripts: [...]} and one that sent the list directly both
            // work - the wrapper is a JSON-schema convenience, not a shape
            // the assistant should have to get right twice. Every sibling
            // tool on this server (transform_cases, validate_cases, ...)
            // takes its JSON payload as a STRING, so a caller that follows
            // that pattern here sends {"scripts": "[...]"} - a
            // Value::String, not a Value::Array. `.to_string()` on that
            // would re-quote and escape it into a JSON string literal
            // instead of forwarding the array text, so it is unwrapped
            // first.
            let body = match args.get("scripts") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => args.to_string(),
            };
            call("POST", "/autorun-script", &body)
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
            // Round 7 §1: a large draft travels as a file path, like every
            // other tool in the family. Forwarded only when present, so
            // the route can refuse path+json as two sources rather than
            // silently preferring one.
            if let Some(p) = args["path"].as_str().filter(|p| !p.trim().is_empty()) {
                params.push(format!("path={}", percent_encode(p)));
            }
            if args["in_place"].as_bool().unwrap_or(false) {
                params.push("in_place=true".to_string());
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
            // together rather than as a query string. Keys are forwarded
            // only when PRESENT - the old shape hardcoded json+operations,
            // which is precisely how a passed `path` was silently dropped
            // and the result built from an empty inline draft (round 5
            // §10). Absent stays absent so the route can tell "not given"
            // from "given empty".
            let mut body = serde_json::Map::new();
            if let Some(j) = args["json"].as_str() {
                body.insert("test_cases".into(), serde_json::json!(j));
            }
            if let Some(p) = args["path"].as_str() {
                body.insert("path".into(), serde_json::json!(p));
            }
            if let Some(w) = args["in_place"].as_bool() {
                body.insert("in_place".into(), serde_json::json!(w));
            }
            body.insert("operations".into(), args["operations"].clone());
            call("POST", "/transform", &serde_json::Value::Object(body).to_string())
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
        // An unknown tool whose NAME says the assistant wanted to mutate
        // Azure DevOps gets the refusal sentence, not a bare "unknown" -
        // "unknown tool update_test_case" reads as a spelling problem and
        // invites another guess; the refusal ends the attempt and points
        // at the path that is allowed.
        other
            if ["create", "update", "delete", "edit", "remove", "submit"]
                .iter()
                .any(|w| other.to_ascii_lowercase().contains(w)) =>
        {
            Err(ToolError::WRITE_REFUSED_TAG.to_string())
        }
        other => Err(format!("{}{other}", ToolError::UNKNOWN_TOOL_TAG)),
    };
    match outcome {
        Ok((status, body)) if status < 400 => serde_json::json!({
            "content": [{ "type": "text", "text": body }],
        }),
        Ok((status, body)) => serde_json::json!({
            "isError": true,
            "content": [{ "type": "text", "text": format!("bridge returned {status}: {body}") }],
        }),
        // Each failure says what actually went wrong. Until the 2026-08
        // audit (P-8) every one of these - including a plain typo in the
        // tool name - was reported as "Could not reach Test Case Manager.
        // Start the app and sign in", sending an assistant off to debug a
        // healthy bridge instead of fixing its own call.
        Err(e) => serde_json::json!({
            "isError": true,
            "content": [{ "type": "text", "text": ToolError::message(&e) }],
        }),
    }
}

/// Why a tool call could not be answered.
///
/// The dispatch arms above all yield `Result<_, String>` (the bridge's own
/// error type), so the KIND travels as a short internal prefix that this
/// classifier strips. Until the 2026-08 audit (P-8) there was no kind at
/// all: every failure - including a plain typo in the tool name - was
/// reported as "Could not reach Test Case Manager. Start the app and sign
/// in", which sent assistants off to debug a healthy bridge.
struct ToolError;

impl ToolError {
    // U+0001 cannot occur in a tool name or a bridge error message, so a
    // tag can never be produced by accident.
    const UNKNOWN_TOOL_TAG: &'static str = "\u{1}unknown-tool\u{1}";
    const WRITE_REFUSED_TAG: &'static str = "\u{1}write-refused\u{1}";

    /// The user-facing sentence for one failed tool call.
    fn message(raw: &str) -> String {
        if let Some(name) = raw.strip_prefix(Self::UNKNOWN_TOOL_TAG) {
            return format!(
                "Unknown tool: {name}. Call tools/list for the tools this server offers."
            );
        }
        if raw == Self::WRITE_REFUSED_TAG {
            return crate::ai_bridge::WRITE_REFUSAL.to_string();
        }
        format!("Could not reach Test Case Manager ({raw}). Start the app and sign in, then retry.")
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
        // 300s, not 30: get_run_failures resolves a PBI's suite by
        // scanning every test plan in the project, throttle-paced - ~60s
        // against a large org on a cold cache. At 30s the proxy gave up
        // mid-scan and reported the app as unreachable, which it wasn't.
        .timeout(std::time::Duration::from_secs(300))
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
