//! The MCP dispatcher is pure: JSON-RPC string in, response string out,
//! with the bridge call injected - no stdio, no sockets.

use v2_lib::ai_tools::module_of_tool;
use v2_lib::mcp::{version_warning, PROXY_VERSION};
use v2_lib::modules::Module;

/// Every call in this file goes to a development app with every module on
/// (`common::dev_app`): a `/tools` answer that names no modules gains them
/// all. A test about the modules themselves calls
/// `v2_lib::mcp::handle_message` directly.
fn handle_message(
    msg: &str,
    version: &str,
    call: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String>,
) -> Option<String> {
    v2_lib::mcp::handle_message(msg, version, &crate::common::dev_app(call))
}

/// Bridge stub: records the call, returns a canned body.
fn stub(status: u16, body: &str) -> impl Fn(&str, &str, &str) -> Result<(u16, String), String> + '_ {
    move |_method, _path, _payload| Ok((status, body.to_string()))
}

#[test]
fn initialize_echoes_protocol_and_advertises_tools() {
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#;
    let resp = handle_message(req, "1.10.3", &stub(200, "")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
    assert!(v["result"]["capabilities"]["tools"].is_object());
    assert_eq!(v["result"]["serverInfo"]["name"], "tcm");
    // The proxy's OWN version. Reporting the app's (read from the handshake
    // file) is what hid a 1.22.1 proxy behind a 1.25.25 app for weeks: the
    // version looked current while the tool list was 1.22.1's (2026-09-24).
    assert_eq!(v["result"]["serverInfo"]["version"], PROXY_VERSION);
}

/// A proxy that is not the running app's build says so where the assistant
/// reads it - the initialize result's `instructions` - naming both versions,
/// the usual cause and the fix.
#[test]
fn initialize_warns_when_the_running_app_is_a_different_version() {
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#;
    let resp = handle_message(req, "9.9.9", &stub(200, "")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let said = v["result"]["instructions"].as_str().expect("a warning in instructions");
    assert!(said.contains("9.9.9"), "{said}");
    assert!(said.contains(PROXY_VERSION), "{said}");
    assert!(said.contains("LocalCache"), "names the stale copy's usual home: {said}");
    assert!(said.contains("new session"), "says what to do: {said}");
    // The server names itself by the key it is registered under.
    assert!(said.starts_with("The tcm MCP server"), "{said}");
}

/// Nothing extra when they match, or when the app is not running and its
/// version is simply not known yet - that is not a mismatch.
#[test]
fn initialize_says_nothing_extra_when_versions_match_or_the_app_is_not_running() {
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    for app in [PROXY_VERSION, "unknown", ""] {
        let resp = handle_message(req, app, &stub(200, "")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["result"].get("instructions").is_none(), "{app:?}: {resp}");
    }
    assert_eq!(version_warning("1.25.25", " 1.25.25 "), None);
    assert!(version_warning("1.22.1", "1.25.25").is_some());
}

#[test]
fn notifications_get_no_response() {
    assert!(handle_message(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "1.10.3",
        &stub(200, "")
    )
    .is_none());
}

#[test]
fn tools_list_names_every_tool() {
    let resp = handle_message(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "1.10.3",
        &stub(200, ""),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let names: Vec<&str> = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    // This test binary is a development build (cargo test compiles with
    // debug assertions on), so with nothing disabled and the network tools'
    // switch on the thirty-five dev-only tools (Auto Run's twenty-eight, then
    // the API templates row's seven) are listed like any other switchable
    // tool - between merge_case_files and db_lookup, where they sit in the
    // source.
    assert_eq!(
        names,
        vec![
            "begin_test_case_writing",
            "get_guide",
            "get_test_cases",
            "search_test_suites",
            "get_suite_test_cases",
            "get_run_results",
            "check_spec_coverage",
            "merge_case_files",
            "save_autorun_script",
            "get_autorun_script",
            "delete_autorun_script",
            "get_autorun_page",
            "probe_autorun_locator",
            "try_autorun_action",
            "replay_autorun_to_step",
            "start_autorun_discovery",
            "discover_autorun_actions",
            "save_autorun_area",
            "end_autorun_discovery",
            "reset_autorun_browser",
            "save_autorun_component",
            "get_autorun_failures",
            "record_quirk",
            "record_validation_rule",
            "mark_autorun_suspected_defect",
            "report_case_divergence",
            "rule_case_divergence",
            "clear_autorun_suspected_defect",
            "set_autorun_order",
            "propose_accounts",
            "get_accounts",
            "save_test_file",
            "get_autorun_network_wait",
            "search_autorun_requests",
            "get_autorun_request",
            "autorun_capture",
            "autorun_har",
            "set_autorun_headers",
            "autorun_cookies",
            "record_explore_change",
            "get_explore_changes",
            "list_api_templates",
            "prove_api_template",
            "run_api_template",
            "save_api_flow",
            "get_api_flow_progress",
            "save_api_fixture",
            "run_api_fixture",
            "send_api_request",
            "undo_explore_creates",
            "db_lookup",
            "db_query",
            "open_report",
            "read_report",
            "edit_report",
            "compare_reports",
            "check_report",
            "preview_report",
            "preview_report_live",
            "optimize_cases",
            "transform_cases",
            "validate_cases",
            "get_tags",
            "search_pbis",
            "search_wiki",
            "get_wiki_page"
        ]
    );
    // Every tool must carry an inputSchema (clients reject tools without one).
    for t in v["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object");
    }
}

#[test]
fn tools_call_proxies_to_the_bridge_and_wraps_text() {
    let req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_test_cases","arguments":{"pbi_id":42,"limit":3}}}"#;
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, r#"{"test_cases":[]}"#.to_string()))
    };
    let resp = handle_message(req, "1.10.3", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["content"][0]["type"], "text");
    assert_eq!(v["result"]["content"][0]["text"], r#"{"test_cases":[]}"#);
    let recorded = calls.borrow();
    let last = recorded.last().unwrap();
    assert_eq!(last.0, "GET");
    assert_eq!(last.1, "/test-cases?pbi=42&limit=3&offset=0");
}

/// `save_autorun_script` forwards ONE body, and what that body looks like
/// depends on whether the call declares any edits: the bare array a new
/// bundle has always sent, or the object that carries the declarations
/// alongside it. A caller that wrapped its payload in a JSON string (the
/// pattern every sibling tool on this server uses) gets the same result
/// either way - the point of building the body with `json!` rather than
/// by pasting text together.
#[test]
fn save_autorun_script_sends_the_array_alone_and_the_object_when_edits_come_too() {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "saved 1 script(s): case 7 (new)".to_string()))
    };
    let scripts = serde_json::json!([{
        "case_id": 7,
        "title": "Save a rating",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "Saved" }] }]
    }]);
    let edits = serde_json::json!([{ "case_id": 7, "steps": [1], "why": "the toast moved" }]);

    // No edits: the array itself, which is what the route has always taken.
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "save_autorun_script", "arguments": { "scripts": scripts } },
    });
    handle_message(&req.to_string(), "1.10.3", &call).unwrap();
    let (method, path, body) = calls.borrow().last().unwrap().clone();
    assert_eq!((method.as_str(), path.as_str()), ("POST", "/autorun-script"));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap(), scripts);

    // With edits: an object carrying both, each parsed back as JSON.
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {
            "name": "save_autorun_script",
            "arguments": { "scripts": scripts, "edits": edits },
        },
    });
    handle_message(&req.to_string(), "1.10.3", &call).unwrap();
    let body = calls.borrow().last().unwrap().2.clone();
    let sent: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(sent.is_object(), "an edits call has to send the object shape: {body}");
    assert_eq!(sent["scripts"], scripts);
    assert_eq!(sent["edits"], edits);

    // The same call with both payloads as JSON STRINGS lands identically.
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {
            "name": "save_autorun_script",
            "arguments": { "scripts": scripts.to_string(), "edits": edits.to_string() },
        },
    });
    handle_message(&req.to_string(), "1.10.3", &call).unwrap();
    let body = calls.borrow().last().unwrap().2.clone();
    let sent: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent["scripts"], scripts, "{body}");
    assert_eq!(sent["edits"], edits, "{body}");
}

/// get_test_cases reads cases by their own ids too, with or without a PBI:
/// an assistant holding "#151331" must not need to know which PBI it is on.
#[test]
fn get_test_cases_passes_case_ids_with_or_without_a_pbi() {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, r#"{"test_cases":[]}"#.to_string()))
    };
    let by_id = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_test_cases","arguments":{"case_ids":[151331,7]}}}"#;
    handle_message(by_id, "1.10.3", &call).unwrap();
    assert_eq!(calls.borrow().last().unwrap().1, "/test-cases?ids=151331,7&limit=5&offset=0");

    let both = r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"get_test_cases","arguments":{"pbi_id":42,"case_ids":[201]}}}"#;
    handle_message(both, "1.10.3", &call).unwrap();
    assert_eq!(calls.borrow().last().unwrap().1, "/test-cases?pbi=42&ids=201&limit=5&offset=0");
}

/// `save_autorun_area` takes an optional `repair: { reason }` (owner
/// decision 2026-10-20): the reason is required inside it, and the tool
/// still needs a name, a module and clicks.
#[test]
fn save_autorun_area_takes_an_optional_repair_with_a_reason() {
    let req = r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#;
    let call = |_: &str, _: &str, _: &str| Ok((200, r#"{"disabled":[]}"#.to_string()));
    let v: serde_json::Value = serde_json::from_str(&handle_message(req, "1.10.3", &call).unwrap()).unwrap();
    let tool = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "save_autorun_area").unwrap();
    assert_eq!(tool["inputSchema"]["required"], serde_json::json!(["name", "module", "clicks"]));
    let repair = &tool["inputSchema"]["properties"]["repair"];
    assert_eq!(repair["type"], "object");
    assert_eq!(repair["required"], serde_json::json!(["reason"]));
    assert_eq!(repair["properties"]["reason"]["type"], "string");
    let said = tool["description"].as_str().unwrap();
    assert!(said.contains("put the old path back"), "{said}");
}

#[test]
fn get_test_cases_requires_neither_a_pbi_nor_ids_in_its_schema() {
    let req = r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#;
    let call = |_: &str, _: &str, _: &str| Ok((200, "[]".to_string()));
    let v: serde_json::Value = serde_json::from_str(&handle_message(req, "1.10.3", &call).unwrap()).unwrap();
    let tool = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "get_test_cases")
        .unwrap();
    // Either argument alone is a valid call; the bridge says what is
    // missing when neither is given.
    assert_eq!(tool["inputSchema"]["required"], serde_json::json!([]));
    assert_eq!(tool["inputSchema"]["properties"]["case_ids"]["type"], "array");
}

#[test]
fn bridge_errors_surface_as_tool_errors_not_crashes() {
    let req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_guide","arguments":{"topic":"writing"}}}"#;
    let down = |_: &str, _: &str, _: &str| Err("connection refused".to_string());
    let resp = handle_message(req, "1.10.3", &down).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], true);
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Test Case Manager"), "tells the user to start the app");
}

#[test]
fn search_pbis_percent_encodes_special_query_chars() {
    // `&` in the search text must not be mistaken for a query-string
    // separator by ai_bridge::q's naive splitter - it has to survive as a
    // percent-escape all the way through.
    let req = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"search_pbis","arguments":{"query":"Search & Filter"}}}"#;
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, r#"{"pbis":[]}"#.to_string()))
    };
    handle_message(req, "1.10.3", &call).unwrap();
    let recorded = calls.borrow();
    let last = recorded.last().unwrap();
    assert_eq!(last.1, "/search-pbis?q=Search%20%26%20Filter");
}

#[test]
fn search_wiki_percent_encodes_special_query_chars() {
    let req = r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"search_wiki","arguments":{"query":"auth & login"}}}"#;
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, r#"{"results":[]}"#.to_string()))
    };
    handle_message(req, "1.10.3", &call).unwrap();
    let recorded = calls.borrow();
    let last = recorded.last().unwrap();
    assert_eq!(last.0, "GET");
    assert_eq!(last.1, "/search-wiki?q=auth%20%26%20login");
}

#[test]
fn get_wiki_page_percent_encodes_wiki_id_and_path() {
    // Path has both spaces and slashes - both must survive as percent
    // escapes through ai_bridge::q's naive '&'/'='' splitter.
    let req = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"get_wiki_page","arguments":{"wiki_id":"wiki-1","path":"/Docs/API Guide"}}}"#;
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, r#"{"path":"/Docs/API Guide","content":""}"#.to_string()))
    };
    handle_message(req, "1.10.3", &call).unwrap();
    let recorded = calls.borrow();
    let last = recorded.last().unwrap();
    assert_eq!(last.0, "GET");
    assert_eq!(last.1, "/wiki-page?wiki=wiki-1&path=%2FDocs%2FAPI%20Guide");
}

/// A tool's free text is percent-encoded exactly as RFC 3986 unreserved
/// allows: ALPHA / DIGIT / `-._~` kept, every other UTF-8 byte as `%XX`
/// in upper case. Pinned against the encoder the bridge used to carry, over
/// every ASCII character and some multi-byte ones.
#[test]
fn free_text_is_percent_encoded_byte_for_byte_as_before() {
    fn reference(s: &str) -> String {
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
    let mut texts: Vec<String> = (1u8..0x80).map(|b| (b as char).to_string()).collect();
    texts.push((1u8..0x80).map(|b| b as char).collect());
    texts.push("caf\u{e9} \u{2022} \u{1f600} a+b=c&d%e/f?g#h".to_string());
    for text in texts {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 8, "method": "tools/call",
            "params": { "name": "search_wiki", "arguments": { "query": text } }
        })
        .to_string();
        let calls = std::cell::RefCell::new(vec![]);
        let call = |method: &str, path: &str, body: &str| {
            calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
            Ok((200, r#"{"results":[]}"#.to_string()))
        };
        handle_message(&req, "1.10.3", &call).unwrap();
        let last = calls.borrow().last().unwrap().1.clone();
        assert_eq!(last, format!("/search-wiki?q={}", reference(&text)), "{text:?}");
    }
    assert_eq!(
        reference("caf\u{e9} ~a-b.c_d"),
        "caf%C3%A9%20~a-b.c_d",
        "the reference itself: upper-case hex, unreserved kept"
    );
}

/// A switched-off tool disappears from tools/list.
#[test]
fn disabled_tools_are_hidden_from_the_list() {
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":["search_wiki","get_wiki_page"]}"#.into()));
        }
        Ok((200, "{}".into()))
    };
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let resp = handle_message(req, "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let names: Vec<&str> = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();

    assert!(!names.contains(&"search_wiki"));
    assert!(!names.contains(&"get_wiki_page"));
    assert!(names.contains(&"optimize_cases"), "the rest are untouched");
}

/// The two database tools are ordinary switchable tools - neither core
/// nor development-only - so the read switch on the AI Bridge tab takes
/// them both out of the list, and a call made from a list cached before
/// the switch moved is refused the ordinary way.
#[test]
fn the_database_tools_are_switchable_and_refused_the_ordinary_way() {
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":["db_lookup","db_query"]}"#.into()));
        }
        panic!("a disabled tool must never reach the bridge");
    };
    let resp =
        handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let names: Vec<&str> = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(!names.contains(&"db_lookup"), "{names:?}");
    assert!(!names.contains(&"db_query"), "{names:?}");
    assert!(names.contains(&"get_tags"), "the rest are untouched");

    for name in ["db_lookup", "db_query"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{name}","arguments":{{}}}}}}"#
        );
        let resp = handle_message(&req, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], true, "{name}");
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("switched off in Test Case Manager"), "{name}: {text}");
    }
}

/// Both forward the whole arguments object as the body, so a field can
/// never be dropped between the tool and the route.
#[test]
fn the_database_tools_forward_their_arguments_as_the_body() {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "dbo.LeaveRequest (1240 rows est.)".to_string()))
    };
    handle_message(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"db_lookup","arguments":{"query":"leave request","limit":5}}}"#,
        "1.0.0",
        &call,
    )
    .unwrap();
    handle_message(
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"db_query","arguments":{"sql":"SELECT 1 AS n"}}}"#,
        "1.0.0",
        &call,
    )
    .unwrap();

    let recorded = calls.borrow();
    let lookup = recorded.iter().find(|c| c.1 == "/db-lookup").expect("the lookup was proxied");
    assert_eq!(lookup.0, "POST");
    assert!(lookup.2.contains("\"query\":\"leave request\""), "{}", lookup.2);
    assert!(lookup.2.contains("\"limit\":5"), "{}", lookup.2);
    let query = recorded.iter().find(|c| c.1 == "/db-query").expect("the statement was proxied");
    assert_eq!(query.0, "POST");
    assert!(query.2.contains("SELECT 1 AS n"), "{}", query.2);
}

/// And calling it anyway - from a cached list - is refused rather than
/// quietly proxied.
#[test]
fn calling_a_disabled_tool_is_refused() {
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":["search_wiki"]}"#.into()));
        }
        panic!("a disabled tool must never reach the bridge");
    };
    let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search_wiki","arguments":{"query":"x"}}}"#;
    let resp = handle_message(req, "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();

    assert_eq!(v["result"]["isError"], true);
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("switched off"), "and says why: {text}");
}

/// A bridge that cannot be reached must not strip every tool - the app
/// being closed is not the same as the user disabling everything. With no
/// answer AND no saved modules there is nothing to offer from a module
/// (the saved-modules fallback is `tools_reply_or_saved`, tested below).
#[test]
fn an_unreachable_bridge_disables_nothing() {
    let call = |_m: &str, _p: &str, _b: &str| -> Result<(u16, String), String> {
        Err("connection refused".into())
    };
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let resp = v2_lib::mcp::handle_message(req, "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    // 53 tools in all; an unreachable bridge cannot say which modules are
    // on, so the 38 module tools are not offered - and nothing else goes.
    assert_eq!(names.len(), 53 - 38, "an unreachable bridge must not disable a Core tool: {names:?}");
    assert!(names.iter().all(|n| module_of_tool(n).is_none()), "{names:?}");
}

/// The editor keeps the tool list it got when it connected. An editor
/// started before the app must still see the modules the person has on,
/// so with no answer from the app the proxy reads this machine's saved
/// modules (`modules.json`), mended, and offers exactly those.
#[test]
fn an_unreachable_bridge_falls_back_to_the_saved_modules() {
    use v2_lib::mcp::tools_reply_or_saved;
    let names_with = |saved: Option<BTreeSet<Module>>| -> Vec<String> {
        let call = move |_m: &str, _p: &str, _b: &str| tools_reply_or_saved(Err("connection refused".into()), saved.clone().map(|s| (s, false)));
        let resp =
            v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    };
    let db_only = names_with(Some([Module::DbAccess].into_iter().collect()));
    assert!(db_only.iter().any(|n| n == "db_lookup") && db_only.iter().any(|n| n == "db_query"), "{db_only:?}");
    assert!(AUTORUN_TOOLS.iter().all(|t| !db_only.iter().any(|n| n == t)), "{db_only:?}");
    // no file (or an unreadable one): none listed, as before
    let none = names_with(None);
    assert!(none.iter().all(|n| module_of_tool(n).is_none()), "{none:?}");
    // a 503 from a bridge that is up but unwell reads the file too
    let (_, on) = v2_lib::mcp::tool_policy_from(
        tools_reply_or_saved(Ok((503, String::new())), Some(([Module::DbAccess].into_iter().collect(), false))),
    );
    assert_eq!(on, [Module::DbAccess].into_iter().collect());
    // but a real answer is never overridden by the file
    let (_, on) = v2_lib::mcp::tool_policy_from(tools_reply_or_saved(
        Ok((200, r#"{"disabled":[],"modules":[]}"#.into())),
        Some(([Module::DbAccess].into_iter().collect(), false)),
    ));
    assert!(on.is_empty());
}

/// The description is the only thing an assistant reads. It used to name
/// 21 ops while the server accepted 24; the gap hid replace_in_preconditions
/// and normalise_citations, and set_comment was reachable when it should
/// never have been. Now one list feeds both.
#[test]
fn the_transform_description_names_every_supported_op_and_nothing_else() {
    let resp = handle_message(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "1.10.3",
        &stub(200, ""),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "transform_cases")
        .unwrap()
        .clone();
    let desc = format!(
        "{} {}",
        tool["description"].as_str().unwrap(),
        tool["inputSchema"]["properties"]["operations"]["description"].as_str().unwrap()
    );
    for name in v2_lib::transform::SUPPORTED_OPS {
        assert!(desc.contains(name), "description omits {name}");
    }
    assert!(!desc.contains("set_comment"), "a removed op must not be advertised");
    // Which keys each op reads is the writing guide's "Transform ops" part,
    // which the description points at; the never-written `comment` and the
    // `ignored` report stay in the description too.
    assert!(desc.contains("get_guide (topic writing), its \"Transform ops\" part"), "{desc}");
    assert!(desc.contains("There is no op for `comment`") && desc.contains("`ignored`"), "{desc}");
    let reference = v2_lib::transform::ops_reference();
    assert!(reference.starts_with("## Transform ops\n"), "{reference}");
    for name in v2_lib::transform::SUPPORTED_OPS {
        assert!(reference.contains(name), "the guide's reference omits {name}");
    }
    for said in ["{find, replace}", "{action, expected}", "at_index", "There is no op for `comment`", "read all three"] {
        assert!(reference.contains(said), "the reference never says {said:?}");
    }
    assert!(!reference.contains("set_comment"));
}

/// The intake's questions and `begin_test_case_writing`'s inputSchema are
/// two halves of one contract, and nothing bound them together: a field
/// added to `intake::questions()` (and made required in `problems()`)
/// without a matching schema property is silently STRIPPED by the MCP
/// client before the call is ever made. The tool then asks for an answer
/// there is no way to give - an unbreakable intake loop, which is exactly
/// what shipped in 1.20.2 when `reference_cases` was added to one side
/// only, past all 39 green suites.
#[test]
fn every_intake_question_is_answerable_through_the_mcp_schema() {
    let resp = handle_message(
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/list"}"#,
        "1.0.0",
        &stub(200, ""),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let begin = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "begin_test_case_writing")
        .expect("begin_test_case_writing is registered");
    let props = begin["inputSchema"]["properties"]
        .as_object()
        .expect("inputSchema has properties");

    for q in v2_lib::intake::questions() {
        assert!(
            props.contains_key(&q.field),
            "intake asks for '{}' but begin_test_case_writing's schema has no such property - \
             an MCP client will drop the answer and the intake can never reach \"ready\"",
            q.field
        );
    }
}

/// A client working from a tool list cached before the merge may still call
/// an old name. It is told where the tool went - checked before the
/// write-refusal name check, so `remove_autorun_component` does not get the
/// Azure DevOps refusal - and the bridge is never reached.
#[test]
fn a_call_to_a_retired_tool_says_where_it_went() {
    let call_tool = |name: &str| -> (String, bool) {
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{{"name":"{name}","arguments":{{}}}}}}"#
        );
        let never = |_m: &str, p: &str, _b: &str| -> Result<(u16, String), String> {
            panic!("a retired tool reached the bridge: {p}");
        };
        let resp = handle_message(&req, "1.0.0", &never).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        (v["result"]["content"][0]["text"].as_str().unwrap().to_string(), v["result"]["isError"] == true)
    };
    let (removed, is_error) = call_tool("remove_autorun_component");
    assert!(is_error);
    assert!(
        removed.contains("`remove_autorun_component` is now `save_autorun_component` with `name` and `remove` true"),
        "{removed}"
    );
    assert!(!removed.contains("must be done through the app itself"), "{removed}");
    let (stop, _) = call_tool("stop_autorun_replay");
    assert!(stop.contains("`stop_autorun_replay` is now `replay_autorun_to_step` with `stop` true"), "{stop}");
    let (cookie, _) = call_tool("set_autorun_cookie");
    assert!(cookie.contains("`autorun_cookies` with `action` \"set\""), "{cookie}");
    // Each says why the name may be missing from the client's own list.
    for answer in [&removed, &stop, &cookie] {
        assert!(answer.ends_with(v2_lib::mcp::STALE_TOOL_LIST), "{answer}");
    }
}

/// The Rust table of retired names is the webview's `RENAMED_TOOLS`, name
/// for name, and each names a tool the server offers now.
#[test]
fn the_retired_tool_table_matches_the_webviews_renamed_tools() {
    let ts = include_str!("../../../src/lib/mcpTools.ts");
    let block = ts.split("export const RENAMED_TOOLS").nth(1).unwrap().split("};").next().unwrap();
    let mut from_ts: Vec<(String, String)> = block
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let (old, new) = l.split_once(':')?;
            if l.starts_with("//") || old.is_empty() || !old.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                return None;
            }
            Some((old.to_string(), new.trim().trim_end_matches(',').trim_matches('"').to_string()))
        })
        .collect();
    from_ts.sort();
    let mut from_rs: Vec<(String, String)> = v2_lib::mcp::RETIRED_TOOLS
        .iter()
        .map(|(old, now)| (old.to_string(), now.trim_start_matches('`').split('`').next().unwrap().to_string()))
        .collect();
    from_rs.sort();
    assert_eq!(from_rs, from_ts);
    let listed = v2_lib::mcp::all_tool_names();
    for (old, new) in &from_rs {
        assert!(listed.iter().any(|n| n == new), "{old} is now {new}, which is not a tool");
        assert!(!listed.iter().any(|n| n == old), "{old} is still a tool");
    }
}

/// Audit finding P-8: every tool-call failure used to be reported as
/// "Could not reach Test Case Manager ... Start the app and sign in",
/// including a plain typo in the tool name. An assistant that reads that
/// goes off to debug a healthy bridge instead of correcting its own call -
/// and the live smoke confirmed the message was identical whether the
/// bridge was up or down, so it carried no information at all.
#[test]
fn a_failed_tool_call_says_which_kind_of_failure_it_was() {
    let call_tool = |name: &str| -> String {
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{{"name":"{name}","arguments":{{}}}}}}"#
        );
        // The bridge is HEALTHY in every case here: any "cannot reach"
        // wording would therefore be a lie, not a race.
        let resp = handle_message(&req, "1.0.0", &stub(200, "{}")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        v["result"]["content"][0]["text"].as_str().unwrap().to_string()
    };

    // 1. A misspelled tool name names itself and points at the tool list.
    let unknown = call_tool("get_test_casez");
    assert!(unknown.contains("Unknown tool: get_test_casez"), "{unknown}");
    assert!(unknown.contains("tools/list"), "{unknown}");
    assert!(
        !unknown.contains("Could not reach"),
        "a healthy bridge must never be reported as unreachable: {unknown}"
    );

    // 2. A write-smelling unknown name still gets the refusal - and ONLY
    //    the refusal, not the refusal wrapped in a bridge error.
    let write = call_tool("create_test_case");
    assert!(
        write.contains("must be done through the app itself"),
        "the write refusal should be the whole message: {write}"
    );
    assert!(!write.contains("Could not reach"), "{write}");

    // 3. A REAL transport failure still says so - the fix must not have
    //    swapped one blanket message for another.
    let req = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_guide","arguments":{"topic":"writing"}}}"#;
    let down = |_m: &str, _p: &str, _b: &str| -> Result<(u16, String), String> {
        Err("handshake file missing - is the app running?".into())
    };
    let resp = handle_message(req, "1.0.0", &down).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Could not reach Test Case Manager"), "{text}");
    assert!(text.contains("handshake file missing"), "{text}");
    assert_eq!(v["result"]["isError"], true);
}

/// Round 8 follow-up, revised for Task 9: the core tools still cannot be
/// switched off, and in this development build the autorun tools are
/// ordinary switchable tools - listed and callable when nothing has named
/// them, absent and refused once the person's own list does.
#[test]
fn core_tools_survive_a_disabled_list_and_autorun_tools_are_switchable_in_a_dev_build() {
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":["begin_test_case_writing","transform_cases"]}"#.into()));
        }
        Ok((200, "{}".into()))
    };
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let resp = handle_message(req, "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"begin_test_case_writing") && names.contains(&"transform_cases"), "{names:?}");
    assert!(
        names.contains(&"start_autorun_discovery")
            && names.contains(&"save_autorun_script")
            && names.contains(&"get_autorun_page"),
        "none was named as disabled, and this is a development build: {names:?}"
    );

    // A call reaches the bridge rather than being refused - the stub
    // answers every non-/tools call with "{}", which the dispatcher wraps
    // as an ordinary (non-error) text result.
    for req in [
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"save_autorun_script","arguments":{"scripts":[]}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_autorun_page","arguments":{}}}"#,
    ] {
        let resp = handle_message(req, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_ne!(v["result"]["isError"], serde_json::json!(true), "a dev-build autorun call must reach the bridge, not be refused");
        assert_eq!(v["result"]["content"][0]["text"], "{}");
    }
}

/// Switched off by name, in a development build, the autorun tools
/// disappear from the list and a call gets the ORDINARY "switched off"
/// sentence - not "not available", which would claim there is no switch
/// when there plainly is one right here. They move as one row in the app,
/// so the list the bridge reports names them all together.
#[test]
fn autorun_tools_named_disabled_in_a_dev_build_are_absent_and_refused_the_ordinary_way() {
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((
                200,
                r#"{"disabled":["save_autorun_script","get_autorun_script","delete_autorun_script","get_autorun_network_wait","get_autorun_page","probe_autorun_locator","try_autorun_action","replay_autorun_to_step","start_autorun_discovery","discover_autorun_actions","save_autorun_area","end_autorun_discovery","reset_autorun_browser","save_autorun_component","get_autorun_failures","record_quirk","record_validation_rule","mark_autorun_suspected_defect","report_case_divergence","rule_case_divergence","clear_autorun_suspected_defect","set_autorun_order","search_autorun_requests","get_autorun_request","autorun_capture","autorun_har","set_autorun_headers","autorun_cookies","record_explore_change","get_explore_changes"]}"#.into(),
            ));
        }
        Ok((200, "{}".into()))
    };
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let resp = handle_message(req, "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.iter().all(|n| !n.contains("autorun")), "{names:?}");

    for name in ["save_autorun_script", "get_autorun_page"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{name}","arguments":{{}}}}}}"#
        );
        let resp = handle_message(&req, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], true, "{name}");
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("switched off"), "off by the person's own choice, in a dev build: {text}");
        assert!(!text.contains("not available"), "{text}");
    }
}

/// A module's tool refused because its module is off names the module -
/// there is no switch on the AI Bridge tab to turn back on.
#[test]
fn a_tool_refused_because_its_module_is_off_names_the_module() {
    let none = BTreeSet::new();
    for (name, module) in [
        ("save_autorun_script", "Auto Run"),
        ("run_api_template", "API Templates"),
        ("db_query", "Database Access"),
    ] {
        let text = v2_lib::mcp::refusal_text(name, &none);
        assert_eq!(text, format!("The `{name}` tool is not available: {module} is switched off in Modules."));
        assert!(!text.contains("AI Bridge"), "{text}");
    }
}

/// The same tool, switched off by the person while its module is on, gets
/// the ordinary sentence - the module wording is only for a module that
/// is off.
#[test]
fn a_module_tool_switched_off_with_its_module_on_gets_the_ordinary_sentence() {
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    let text = v2_lib::mcp::refusal_text("save_autorun_script", &all);
    assert!(text.contains("switched off in Test Case Manager"), "{text}");
    assert!(!text.contains("not available"), "{text}");
}

/// A Core tool is unaffected by the modules either way.
#[test]
fn an_ordinary_tool_gets_the_switched_off_sentence_whatever_is_on() {
    for on in [BTreeSet::new(), Module::ALL.into_iter().collect()] {
        let text = v2_lib::mcp::refusal_text("search_wiki", &on);
        assert!(text.contains("switched off"), "{on:?}: {text}");
    }
}

use std::collections::BTreeSet;
use v2_lib::ai_tools::{API_TEMPLATE_TOOLS as API_TOOLS, AUTORUN_TOOLS, CRYSTAL_TOOLS, DB_TOOLS};
use v2_lib::mcp::tool_policy_from;

/// The proxy is its own process and cannot read the app's modules; the
/// bridge's /tools answer carries them as `modules`, and the proxy takes
/// exactly those.
#[test]
fn the_proxy_takes_the_modules_from_the_app() {
    let (off, on) = tool_policy_from(Ok((
        200,
        r#"{"disabled":["search_wiki"],"modules":["db_access","writing_style","auto_run","api_templates","playwright","crystal_reports"],"network_tools":true}"#.into(),
    )));
    assert_eq!(on, Module::ALL.into_iter().collect());
    assert_eq!(off, vec!["search_wiki"]);

    let (off, on) =
        tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["db_access","auto_run","a_module_from_later"],"network_tools":true}"#.into())));
    assert_eq!(on, [Module::DbAccess, Module::AutoRun].into_iter().collect(), "an unknown name is ignored");
    // Crystal Reports is not named either, so its tools are off with them.
    let expected: Vec<String> = API_TOOLS.iter().chain(CRYSTAL_TOOLS).map(|s| s.to_string()).collect();
    assert_eq!(off, expected, "API Templates and Crystal Reports off: exactly their tools");
}

/// The answer is mended like a stored choice: a module whose parent is
/// not named is off, so `auto_run` alone offers no Auto Run tool.
#[test]
fn the_proxy_mends_a_broken_tree() {
    let (off, on) = tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["auto_run"]}"#.into())));
    assert!(on.is_empty(), "{on:?}");
    for t in AUTORUN_TOOLS {
        assert!(off.iter().any(|o| o == t), "{t} offered without Database Access");
    }
    let (_, on) = tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["db_access","playwright"]}"#.into())));
    assert_eq!(on, [Module::DbAccess].into_iter().collect());
}

/// Everything short of a `modules` list naming a module means that module
/// is off: an app too old to send the field, a non-200 answer, garbage, or
/// no app at all. A development build adds nothing - the proxy never
/// widens the tool list on a guess.
#[test]
fn an_older_absent_or_confused_app_offers_no_module_tools() {
    let mut module_tools: Vec<String> = AUTORUN_TOOLS.iter().map(|s| s.to_string()).collect();
    module_tools.extend(API_TOOLS.iter().map(|s| s.to_string()));
    module_tools.extend(DB_TOOLS.iter().map(|s| s.to_string()));
    module_tools.extend(CRYSTAL_TOOLS.iter().map(|s| s.to_string()));
    let replies: Vec<Result<(u16, String), String>> = vec![
        Ok((200, r#"{"disabled":[],"modules":[]}"#.to_string())),
        Ok((200, r#"{"disabled":[],"autorun":true}"#.to_string())),
        Ok((200, r#"{"disabled":[]}"#.to_string())),
        Ok((503, String::new())),
        Ok((200, "not json".to_string())),
        Err("connection refused".to_string()),
    ];
    for reply in replies {
        let shown = format!("{reply:?}");
        let (off, on) = tool_policy_from(reply);
        assert!(on.is_empty(), "{shown}");
        assert_eq!(off, module_tools, "{shown}");
    }
}

/// Each module off removes exactly its tools from tools/list and refuses
/// them at call time - through the real dispatch, no dev-build widening.
#[test]
fn each_module_off_removes_its_tools_from_the_list_and_refuses_them() {
    let names_for = |modules: &str| -> Vec<String> {
        let answer = format!(r#"{{"disabled":[],"modules":{modules},"network_tools":true}}"#);
        let call = move |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
            assert_eq!(path, "/tools");
            Ok((200, answer.clone()))
        };
        let resp =
            v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    };
    let all = names_for(r#"["db_access","writing_style","auto_run","api_templates","playwright","crystal_reports"]"#);
    for (module, on_list, tools) in [
        ("api_templates", r#"["db_access","writing_style","auto_run","playwright","crystal_reports"]"#, API_TOOLS),
        ("auto_run", r#"["db_access","writing_style","crystal_reports"]"#, AUTORUN_TOOLS),
        ("db_access", r#"["writing_style","crystal_reports"]"#, DB_TOOLS),
        (
            "crystal_reports",
            r#"["db_access","writing_style","auto_run","api_templates","playwright"]"#,
            CRYSTAL_TOOLS,
        ),
    ] {
        let names = names_for(on_list);
        for t in tools {
            assert!(all.iter().any(|n| n == t), "{t} missing with every module on");
            assert!(!names.iter().any(|n| n == t), "{t} listed with {module} off");
        }
        // Nothing else went: what is missing is only the off modules' tools.
        let gone: Vec<&String> = all.iter().filter(|n| !names.contains(n)).collect();
        assert!(gone.iter().all(|n| module_of_tool(n).is_some()), "{module}: a Core tool went: {gone:?}");

        let answer = format!(r#"{{"disabled":[],"modules":{on_list},"unlocked":true}}"#);
        let call = move |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
            assert_eq!(path, "/tools", "a tool of an off module must never reach the bridge");
            Ok((200, answer.clone()))
        };
        for t in tools {
            let req = serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": t, "arguments": {} },
            });
            let resp = v2_lib::mcp::handle_message(&req.to_string(), "1.0.0", &call).unwrap();
            let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(v["result"]["isError"], true, "{t}");
            let text = v["result"]["content"][0]["text"].as_str().unwrap();
            let m = Module::from_key(module).unwrap();
            assert!(text.contains(&m.off_sentence_for(true)), "{t}: {text}");
        }
    }
    // Writing Style and Playwright have no tools: switching them off leaves the list as it is.
    assert_eq!(names_for(r#"["db_access","auto_run","api_templates","crystal_reports"]"#), all);
}

/// The proxy asks `/tools` on every tools/list and every tools/call, so a
/// module switched off in the app is gone on the assistant's next request,
/// with no restart.
#[test]
fn the_proxy_asks_the_app_about_its_modules_on_every_list_and_call() {
    let modules = std::cell::RefCell::new(r#"["db_access","auto_run"]"#.to_string());
    let asked = std::cell::Cell::new(0);
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            asked.set(asked.get() + 1);
            return Ok((200, format!(r#"{{"disabled":[],"modules":{}}}"#, modules.borrow())));
        }
        Ok((200, "{}".into()))
    };
    let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_autorun_page","arguments":{}}}"#;
    let v: serde_json::Value = serde_json::from_str(&v2_lib::mcp::handle_message(req, "1.0.0", &call).unwrap()).unwrap();
    assert_ne!(v["result"]["isError"], true, "{v}");
    assert_eq!(asked.get(), 1);

    *modules.borrow_mut() = r#"["writing_style"]"#.to_string();
    let v: serde_json::Value = serde_json::from_str(&v2_lib::mcp::handle_message(req, "1.0.0", &call).unwrap()).unwrap();
    assert_eq!(v["result"]["isError"], true, "{v}");
    assert_eq!(asked.get(), 2);
    let list =
        v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
    assert!(!list.contains("get_autorun_page"), "{list}");
    assert_eq!(asked.get(), 3);
}

/// "Run database changes without asking" reaches the assistant as one line
/// at the end of db_query's description - only on an explicit yes from the
/// app, so an older app, a failed answer or no app at all never adds it.
#[test]
fn db_query_says_not_to_ask_only_when_the_app_says_so() {
    let db_query_description = |tools_reply: &'static str| {
        let call = move |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
            if path == "/tools" {
                return Ok((200, tools_reply.into()));
            }
            Ok((200, "{}".into()))
        };
        let resp =
            handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "db_query")
            .map(|t| t["description"].as_str().unwrap().to_string())
            .unwrap()
    };
    let asked = db_query_description(r#"{"disabled":[],"db_no_ask":true}"#);
    assert!(asked.ends_with(v2_lib::mcp::DB_NO_ASK), "{asked}");
    for reply in [r#"{"disabled":[],"db_no_ask":false}"#, r#"{"disabled":[]}"#, "not json"] {
        let plain = db_query_description(reply);
        assert!(!plain.contains("without confirmation"), "{reply}: {plain}");
    }
    assert!(!v2_lib::mcp::DB_NO_ASK.is_empty());
}

/// The live report preview runs the report's stored procedure, so it is
/// approved like db_query: a "do not ask" line on an explicit yes, and no
/// other tool carries one. Its line is its own - it has no batch and no
/// dry run, so db_query's advice would mean nothing there.
#[test]
fn the_live_preview_says_not_to_ask_only_when_the_app_says_so() {
    let descriptions = |tools_reply: &'static str| -> Vec<(String, String)> {
        let call = move |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
            if path == "/tools" {
                return Ok((200, tools_reply.into()));
            }
            Ok((200, "{}".into()))
        };
        let resp =
            handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| (t["name"].as_str().unwrap().to_string(), t["description"].as_str().unwrap().to_string()))
            .collect()
    };
    let asked = descriptions(r#"{"disabled":[],"db_no_ask":true}"#);
    let carrying: Vec<&str> =
        asked.iter().filter(|(_, d)| d.ends_with(v2_lib::mcp::DB_NO_ASK)).map(|(n, _)| n.as_str()).collect();
    assert_eq!(carrying, ["db_query"]);
    let carrying: Vec<&str> = asked
        .iter()
        .filter(|(_, d)| d.ends_with(v2_lib::mcp::PREVIEW_LIVE_NO_ASK))
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(carrying, ["preview_report_live"]);
    let live = &asked.iter().find(|(n, _)| n == "preview_report_live").unwrap().1;
    assert!(!live.contains("dry_run"), "{live}");
    assert!(v2_lib::mcp::PREVIEW_LIVE_NO_ASK.contains("without confirmation"));
    assert!(v2_lib::mcp::PREVIEW_LIVE_NO_ASK.contains("parameter"));
    let plain = descriptions(r#"{"disabled":[],"db_no_ask":false}"#);
    assert!(plain.iter().all(|(_, d)| !d.contains("without confirmation")), "{plain:?}");
}

/// db_query's description says what the Dev login may now create and
/// alter, what is still never allowed, and how large a request may be.
#[test]
fn db_query_describes_the_object_allowance_and_the_size_cap() {
    // On a machine that unlocked the held-back module: locked, the allowance is not described.
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "1.0.0", &stub(200, r#"{"unlocked":true}"#)).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let text = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "db_query")
        .and_then(|t| t["description"].as_str())
        .unwrap()
        .to_string();
    for needed in ["CREATE OR ALTER", "procedure, view or function", "Crystal Reports", "never DROP", "64 KiB"] {
        assert!(text.contains(needed), "db_query's description does not say {needed:?}: {text}");
    }
    assert!(!text.contains("never DROP, ALTER or CREATE"), "{text}");
    assert!(!text.contains('\u{2014}'), "no em dashes in text an assistant reads");
}

/// The network tools say when they answer 409 (a replay, a step or a
/// discovery action using the browser), which headers are refused - the
/// URL-rewrite ones with the method overrides - which cookies are refused,
/// that a clear is a real sign-out reaching a parent-domain cookie, and the
/// probe's `show` says what `listeners.ancestors` holds and what is hidden.
#[test]
fn the_network_tools_name_their_refusals_and_the_busy_answer() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "1.0.0", &stub(200, "")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap().clone();
    let desc = |name: &str| -> String {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .and_then(|t| t["description"].as_str())
            .unwrap_or_else(|| panic!("no tool {name}"))
            .to_string()
    };
    for name in ["search_autorun_requests", "get_autorun_request", "autorun_capture", "autorun_har", "autorun_cookies"] {
        let text = desc(name);
        assert!(text.contains("409"), "{name} does not say it answers 409 while the browser is busy: {text}");
    }
    assert!(desc("search_autorun_requests").contains("409 while a replay, a step or a discovery action is using it"));
    assert!(desc("search_autorun_requests").contains("redacted"));
    assert!(desc("get_autorun_request").contains("never cookies or authorization"));
    assert!(desc("autorun_har").contains("never anywhere else"));
    let headers = desc("set_autorun_headers");
    for name in ["X-HTTP-Method-Override", "X-HTTP-Method", "X-Method-Override", "X-Original-URL", "X-Rewrite-URL"] {
        assert!(headers.contains(name), "set_autorun_headers does not name {name}: {headers}");
    }
    assert!(headers.contains("Refused in the person's browser and while a replay runs"), "{headers}");
    let cookies = desc("autorun_cookies");
    assert!(!cookies.contains("never the identity provider's"), "{cookies}");
    assert!(cookies.contains("parent domains") && cookies.contains("sibling host"), "{cookies}");
    for said in [".ASPXAUTH", "__RequestVerificationToken*", "sign out for real", "with no default", "refused in the person's browser and while a replay runs"] {
        assert!(cookies.contains(said), "autorun_cookies never says {said:?}: {cookies}");
    }
    let probe = desc("probe_autorun_locator");
    assert!(probe.contains("`listeners.ancestors`"), "{probe}");
    assert!(probe.contains("A password field's value, attributes named like a secret and tokens in text are hidden"), "{probe}");
    for name in ["set_autorun_headers", "autorun_cookies", "probe_autorun_locator", "search_autorun_requests", "get_autorun_request", "autorun_capture", "autorun_har"] {
        let text = desc(name);
        assert!(!text.contains('\u{2014}') && !text.contains('\u{2013}'), "no em or en dashes: {name}");
    }
}

#[test]
fn db_no_ask_needs_an_explicit_yes() {
    use v2_lib::mcp::db_no_ask_from;
    assert!(db_no_ask_from(&Ok((200, r#"{"db_no_ask":true}"#.into()))));
    assert!(!db_no_ask_from(&Ok((200, r#"{"db_no_ask":"true"}"#.into()))));
    assert!(!db_no_ask_from(&Ok((500, r#"{"db_no_ask":true}"#.into()))));
    assert!(!db_no_ask_from(&Err("the app is closed".into())));
}

const API_TEMPLATE_TOOLS: [&str; 7] = [
    "list_api_templates",
    "prove_api_template",
    "run_api_template",
    "save_api_flow",
    "get_api_flow_progress",
    "save_api_fixture",
    "run_api_fixture",
];

fn listed_names(call: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String>) -> Vec<String> {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
}

/// The six API template tools ride with the Auto Run ones: listed where
/// Auto Run is offered (this development build), and - where it is not -
/// disabled first and refused with "not available", exactly as the Auto
/// Run tools are.
#[test]
fn the_api_template_tools_are_listed_where_auto_run_is_offered_and_absent_where_not() {
    let names = listed_names(&stub(200, r#"{"disabled":[]}"#));
    for name in API_TEMPLATE_TOOLS {
        assert!(names.iter().any(|n| n == name), "{name} missing from {names:?}");
    }

    let (off, on) = tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["db_access","writing_style"]}"#.into())));
    assert!(!on.contains(&Module::AutoRun));
    for name in API_TEMPLATE_TOOLS {
        assert!(off.iter().any(|n| n == name), "{name} not disabled with its module off: {off:?}");
        let text = v2_lib::mcp::refusal_text(name, &on);
        assert!(text.contains("not available"), "{name}: {text}");
    }

    // Switched off by the person, in a development build, they leave the
    // list like any other switchable tool.
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((
                200,
                r#"{"disabled":["list_api_templates","prove_api_template","run_api_template","save_api_flow","get_api_flow_progress","save_api_fixture","run_api_fixture"]}"#
                    .into(),
            ));
        }
        Ok((200, "{}".into()))
    };
    let names = listed_names(&call);
    assert!(
        names.iter().all(|n| !n.contains("api_template")
            && !n.contains("api_flow")
            && !n.contains("api_fixture")
            && !n.contains("app_quirk")),
        "{names:?}"
    );
}

/// The fixtures list is `list_api_templates` with `kind: "fixtures"` (the old
/// `list_api_fixtures` is gone); a `kind` it does not list is refused by
/// name before anything reaches the app, never read as templates.
#[test]
fn the_template_list_takes_a_kind_and_refuses_an_unknown_one() {
    use v2_lib::mcp::LIST_KIND_REFUSED;
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().all(|t| t["name"] != "list_api_fixtures"), "list_api_fixtures is gone");
    let list = tools.iter().find(|t| t["name"] == "list_api_templates").unwrap();
    assert_eq!(list["inputSchema"]["properties"]["kind"]["enum"], serde_json::json!(["templates", "fixtures"]));
    assert_eq!(list["inputSchema"]["properties"]["full"]["type"], "boolean");
    assert!(list["description"].as_str().unwrap().contains("answers with the API templates switch off"), "{list}");

    let posted = std::cell::Cell::new(0);
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path != "/tools" {
            posted.set(posted.get() + 1);
        }
        Ok((200, "{}".into()))
    };
    for kind in [r#""fixture""#, r#""Fixtures""#, "true", "[]"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"list_api_templates","arguments":{{"kind":{kind}}}}}}}"#
        );
        let v: serde_json::Value = serde_json::from_str(&handle_message(&req, "1.0.0", &call).unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true, "{kind}: {v}");
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains(LIST_KIND_REFUSED), "{kind}: {v}");
    }
    assert_eq!(posted.get(), 0, "nothing reached the app");
}

/// Each tool reaches its own route; prove and run forward the arguments
/// object as the body, untouched.
#[test]
fn the_api_template_tools_call_their_routes() {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".to_string()))
    };
    let run_args = serde_json::json!({ "id": "pms-create-draft-cycle", "account": "admin", "values": { "cycleName": "FY27" } });
    let prove_args = serde_json::json!({
        "template": { "id": "x" }, "account": "admin", "values": {}, "replace": true, "why": "the handler moved",
        "browser": "chrome",
    });
    let fixture_args = serde_json::json!({ "fixture": { "id": "draft-cycle", "steps": [] } });
    let run_fixture_args = serde_json::json!({ "id": "draft-cycle", "browser": "chrome" });
    let cases = [
        ("get_guide", serde_json::json!({ "topic": "api_templates" }), "GET", "/api-template-guide", None),
        ("list_api_templates", serde_json::json!({}), "GET", "/api-templates", None),
        ("prove_api_template", prove_args.clone(), "POST", "/api-template-prove", Some(prove_args)),
        ("run_api_template", run_args.clone(), "POST", "/api-template-run", Some(run_args)),
        ("save_api_fixture", fixture_args.clone(), "POST", "/api-template-fixture-save", Some(fixture_args)),
        ("run_api_fixture", run_fixture_args.clone(), "POST", "/api-template-fixture-run", Some(run_fixture_args)),
        ("list_api_templates", serde_json::json!({ "kind": "templates" }), "GET", "/api-templates", None),
        ("list_api_templates", serde_json::json!({ "kind": null, "id": "x" }), "GET", "/api-templates?id=x", None),
        ("list_api_templates", serde_json::json!({ "kind": "fixtures" }), "GET", "/api-template-fixtures", None),
        ("list_api_templates", serde_json::json!({ "kind": "fixtures", "id": "draft cycle" }), "GET", "/api-template-fixtures?id=draft%20cycle", None),
        ("list_api_templates", serde_json::json!({ "kind": "fixtures", "full": true }), "GET", "/api-template-fixtures?full=true", None),
    ];
    for (name, args, method, path, body) in cases {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args },
        });
        let resp = handle_message(&req.to_string(), "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_ne!(v["result"]["isError"], serde_json::json!(true), "{name}: {resp}");
        let (m, p, b) = calls.borrow().last().unwrap().clone();
        assert_eq!((m.as_str(), p.as_str()), (method, path), "{name}");
        if let Some(body) = body {
            assert_eq!(serde_json::from_str::<serde_json::Value>(&b).unwrap(), body, "{name}");
        }
    }
}

/// The two flow tools reach their own routes, and the arguments object
/// travels whole as the body.
#[test]
fn the_flow_tools_reach_their_routes() {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".to_string()))
    };
    let save_args = serde_json::json!({
        "flow": { "id": "pms-performance-cycle" }, "sample": 273, "replace": true, "why": "a stage was renamed",
    });
    let progress_args = serde_json::json!({ "flow": "pms-performance-cycle", "subject": 273 });
    let cases = [
        ("save_api_flow", save_args, "/api-template-flow-save"),
        ("get_api_flow_progress", progress_args, "/api-template-flow-progress"),
    ];
    for (name, args, path) in cases {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args },
        });
        let resp = handle_message(&req.to_string(), "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_ne!(v["result"]["isError"], serde_json::json!(true), "{name}: {resp}");
        let (m, p, b) = calls.borrow().last().unwrap().clone();
        assert_eq!((m.as_str(), p.as_str()), ("POST", path), "{name}");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&b).unwrap(), args, "{name}");
    }
}

/// `list_api_templates` forwards its filters and paging as the query,
/// percent-encoded; numbers may arrive as numbers or as numeric strings.
/// Its schema and description say how to page.
#[test]
fn list_api_templates_forwards_filters_and_paging() {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".to_string()))
    };
    let path_for = |args: serde_json::Value| {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "list_api_templates", "arguments": args },
        });
        handle_message(&req.to_string(), "1.0.0", &call).unwrap();
        calls.borrow().last().unwrap().1.clone()
    };
    assert_eq!(path_for(serde_json::json!({})), "/api-templates");
    assert_eq!(
        path_for(serde_json::json!({
            "module": "PMS / Cycle", "search": "cycle&id", "flow": "pms-performance-cycle", "offset": 25, "limit": "50",
        })),
        "/api-templates?module=PMS%20%2F%20Cycle&search=cycle%26id&flow=pms-performance-cycle&offset=25&limit=50"
    );
    assert_eq!(path_for(serde_json::json!({ "id": "pms-create-draft-cycle" })), "/api-templates?id=pms-create-draft-cycle");
    assert_eq!(
        path_for(serde_json::json!({ "id": "pms-create-draft-cycle", "flow": false })),
        "/api-templates?with_flow=false&id=pms-create-draft-cycle"
    );

    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "list_api_templates").unwrap().clone();
    let props = &tool["inputSchema"]["properties"];
    for key in ["module", "search", "flow", "id", "offset", "limit"] {
        assert!(props.get(key).is_some(), "schema has {key}: {tool}");
    }
    let description = tool["description"].as_str().unwrap();
    assert!(description.contains("next_offset"), "says how to page: {description}");
    assert!(
        description.contains("With `id`, the other filters and the paging are ignored."),
        "says what `id` overrides: {description}"
    );
}

/// A failed run comes back as a tool error that still carries the run's
/// report - which step failed, and what had already been created.
#[test]
fn a_failed_api_template_run_is_a_tool_error_with_the_report() {
    let report = r#"{"ok":false,"template":"x","created":{"cycleId":274},"failed":"Evaluation rules"}"#;
    let call = move |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":[]}"#.into()));
        }
        Ok((502, report.to_string()))
    };
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_api_template","arguments":{"id":"x","account":"admin","values":{}}}}"#;
    let resp = handle_message(req, "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], true);
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Evaluation rules") && text.contains("274"), "{text}");
}

/// One quirk tool records and retires: listed with `text`, `cases` and
/// `from` to record, `retire`, `reason` and `replacement` to retire, none of
/// them required by the schema - the call says which it is. A record goes to
/// the record route whole; a retire goes to the retire route with the id as
/// that route's own `id`.
#[test]
fn the_quirk_tool_records_and_retires_through_their_routes() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let quirk = tools.iter().find(|t| t["name"] == "record_quirk").expect("the quirk tool is listed");
    let schema = &quirk["inputSchema"];
    assert_eq!(schema["type"], "object");
    for prop in ["text", "retire", "reason", "replacement"] {
        assert_eq!(schema["properties"][prop]["type"], "string", "{prop}: {schema}");
    }
    assert_eq!(schema["properties"]["from"]["enum"], serde_json::json!(["autorun", "api"]));
    assert_eq!(schema["properties"]["cases"]["items"]["required"], serde_json::json!(["case_id", "steps"]));
    assert_eq!(schema["required"], serde_json::json!([]));
    let desc = quirk["description"].as_str().unwrap();
    assert!(desc.contains("person"), "says a person's note is refused: {desc}");
    assert!(desc.contains("60 active quirks"), "{desc}");
    for old in ["record_autorun_quirk", "retire_autorun_quirk", "record_app_quirk", "retire_app_quirk"] {
        assert!(tools.iter().all(|t| t["name"] != old), "{old} is gone");
    }

    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "done".to_string()))
    };
    let send = |args: &str| -> serde_json::Value {
        let req = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"record_quirk","arguments":{args}}}}}"#);
        serde_json::from_str(&handle_message(&req, "1.0.0", &call).unwrap()).unwrap()
    };
    let last = || calls.borrow().iter().rev().find(|c| c.1 != "/tools").cloned().unwrap();

    send(r#"{"text":"the grid pages at 25","from":"api","cases":[{"case_id":7,"steps":[2]}]}"#);
    let (method, path, body) = last();
    assert_eq!((method.as_str(), path.as_str()), ("POST", "/autorun-quirk"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({ "text": "the grid pages at 25", "from": "api", "cases": [{ "case_id": 7, "steps": [2] }] })
    );

    let v = send(r#"{"retire":"q1a2b3c","reason":"never helped","from":"api"}"#);
    assert_eq!(v["result"]["content"][0]["text"], "done");
    let (method, path, body) = last();
    assert_eq!((method.as_str(), path.as_str()), ("POST", "/autorun-quirk-retire"));
    // `from` travels on, so the bridge's guard still refuses an API
    // assistant's retire while API Templates is off.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({ "id": "q1a2b3c", "reason": "never helped", "from": "api" })
    );
}

/// A call that says both "record this" and "retire that" is refused before
/// anything reaches the app, and so is a retire that gives no reason - the
/// reason the old retire tool's schema required.
#[test]
fn the_quirk_tool_refuses_a_record_and_a_retire_at_once_and_a_retire_without_a_reason() {
    use v2_lib::mcp::{QUIRK_RECORD_OR_RETIRE, QUIRK_RETIRE_NEEDS_REASON};
    let posted = std::cell::Cell::new(false);
    let call = |method: &str, _path: &str, _body: &str| {
        if method == "POST" {
            posted.set(true);
        }
        Ok((200, "{}".to_string()))
    };
    for (args, said) in [
        (r#"{"text":"a line","retire":"q1","reason":"why"}"#, QUIRK_RECORD_OR_RETIRE),
        (r#"{"retire":"q1"}"#, QUIRK_RETIRE_NEEDS_REASON),
        (r#"{"retire":"q1","reason":null}"#, QUIRK_RETIRE_NEEDS_REASON),
    ] {
        let req = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"record_quirk","arguments":{args}}}}}"#);
        let v: serde_json::Value = serde_json::from_str(&handle_message(&req, "1.0.0", &call).unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true, "{args}: {v}");
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains(said), "{args}: {v}");
    }
    assert!(!posted.get(), "nothing reached the app");
}

/// The validation-rule tool: listed with a `message` always required, the
/// action (`after`) asked for in its words, and a call forwards its arguments
/// whole to the bridge's rule route.
#[test]
fn the_validation_rule_tool_asks_for_the_message_and_reaches_its_route() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let rule = tools.iter().find(|t| t["name"] == "record_validation_rule").expect("the rule tool is listed");
    let schema = &rule["inputSchema"];
    assert_eq!(schema["required"], serde_json::json!(["message"]));
    for prop in ["message", "after", "area", "page", "step"] {
        assert_eq!(schema["properties"][prop]["type"], "string", "{prop}: {schema}");
    }
    assert_eq!(schema["properties"]["remove"]["type"], "boolean");
    assert!(rule["description"].as_str().unwrap().contains("after"), "says when `after` is needed");
    assert!(module_of_tool("record_validation_rule") == Some(Module::AutoRun));

    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "Remembered for Cycles.".to_string()))
    };
    let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"record_validation_rule","arguments":{"message":"Name is required","after":"clicked Save"}}}"#;
    let v: serde_json::Value = serde_json::from_str(&handle_message(req, "1.0.0", &call).unwrap()).unwrap();
    assert_eq!(v["result"]["content"][0]["text"], "Remembered for Cycles.");
    let recorded = calls.borrow();
    let last = recorded.last().unwrap();
    assert_eq!((last.0.as_str(), last.1.as_str()), ("POST", "/autorun-validation-rule"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&last.2).unwrap(),
        serde_json::json!({ "message": "Name is required", "after": "clicked Save" })
    );
}

/// With the Auto Run row switched off and the API templates row on, the app
/// leaves `record_quirk` out of the disabled list (the tool rides in both
/// rows), so the API templates assistant still has it.
#[test]
fn with_auto_run_off_and_api_templates_on_the_quirk_tool_stays() {
    let call = |_method: &str, path: &str, _body: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((
                200,
                r#"{"disabled":["save_autorun_script","get_autorun_script","delete_autorun_script","get_autorun_network_wait","get_autorun_page","probe_autorun_locator","try_autorun_action","replay_autorun_to_step","start_autorun_discovery","discover_autorun_actions","save_autorun_area","end_autorun_discovery","reset_autorun_browser","save_autorun_component","get_autorun_failures","record_validation_rule","mark_autorun_suspected_defect","report_case_divergence","rule_case_divergence","clear_autorun_suspected_defect","set_autorun_order","search_autorun_requests","get_autorun_request","autorun_capture","autorun_har","set_autorun_headers","autorun_cookies","record_explore_change","get_explore_changes"]}"#.into(),
            ));
        }
        Ok((200, "ok".into()))
    };
    let names = listed_names(&call);
    assert!(names.iter().all(|n| !n.contains("autorun")), "{names:?}");
    for name in ["record_quirk", "list_api_templates"] {
        assert!(names.iter().any(|n| n == name), "{name} missing from {names:?}");
    }
    assert!(module_of_tool("record_quirk") == Some(Module::AutoRun));
}

/// With only the API templates row on, the API templates row is the one
/// reason `record_quirk` is offered, so a quirk sent without `from` is that
/// assistant's: `from: "api"` is filled in, as `record_app_quirk` did. A
/// `from` the caller sent is kept, and with the Auto Run row on nothing is
/// filled in.
#[test]
fn with_only_the_api_templates_row_on_a_quirk_without_from_is_the_api_assistants() {
    const AUTORUN_ROW_OFF: &str = r#"{"disabled":["save_autorun_script","get_autorun_script","delete_autorun_script","get_autorun_network_wait","get_autorun_page","probe_autorun_locator","try_autorun_action","replay_autorun_to_step","start_autorun_discovery","discover_autorun_actions","save_autorun_area","end_autorun_discovery","reset_autorun_browser","save_autorun_component","get_autorun_failures","record_validation_rule","mark_autorun_suspected_defect","report_case_divergence","rule_case_divergence","clear_autorun_suspected_defect","set_autorun_order","propose_accounts","get_accounts","save_test_file","search_autorun_requests","get_autorun_request","autorun_capture","autorun_har","set_autorun_headers","autorun_cookies","record_explore_change","get_explore_changes"]}"#;
    let posted = |tools: &'static str, args: &str| -> (String, serde_json::Value) {
        let calls = std::cell::RefCell::new(vec![]);
        let call = |_m: &str, path: &str, body: &str| -> Result<(u16, String), String> {
            if path == "/tools" {
                return Ok((200, tools.to_string()));
            }
            calls.borrow_mut().push((path.to_string(), body.to_string()));
            Ok((200, "ok".into()))
        };
        let req = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"record_quirk","arguments":{args}}}}}"#);
        let v: serde_json::Value = serde_json::from_str(&handle_message(&req, "1.0.0", &call).unwrap()).unwrap();
        assert_ne!(v["result"]["isError"], true, "{args}: {v}");
        let (path, body) = calls.borrow().last().unwrap().clone();
        (path, serde_json::from_str(&body).unwrap())
    };

    let (path, body) = posted(AUTORUN_ROW_OFF, r#"{"text":"the handler wants a CSRF header"}"#);
    assert_eq!(path, "/autorun-quirk");
    assert_eq!(body, serde_json::json!({ "text": "the handler wants a CSRF header", "from": "api" }));
    let (path, body) = posted(AUTORUN_ROW_OFF, r#"{"retire":"q1a2b3c","reason":"never helped"}"#);
    assert_eq!(path, "/autorun-quirk-retire");
    assert_eq!(body, serde_json::json!({ "id": "q1a2b3c", "reason": "never helped", "from": "api" }));
    let (_, body) = posted(AUTORUN_ROW_OFF, r#"{"text":"a line","from":"autorun"}"#);
    assert_eq!(body["from"], "autorun", "a `from` the caller sent is kept");
    let (_, body) = posted(r#"{"disabled":[]}"#, r#"{"text":"a line"}"#);
    assert!(body.get("from").is_none(), "nothing filled in while the Auto Run row is on: {body}");
}

/// `get_guide` follows the row switches as well as the modules, as the old
/// guide tools did: a topic whose row's own tools are all off is not in the
/// enum, gets no sentence, and a call is refused as "switched off". Its
/// module off, it is refused naming the module.
#[test]
fn the_guide_tool_offers_a_topic_only_while_its_row_is_on() {
    use v2_lib::mcp::{guide_offered, offered_topics};
    let guide_of = |tools: &'static str| -> serde_json::Value {
        let call = move |_m: &str, _p: &str, _b: &str| -> Result<(u16, String), String> { Ok((200, tools.to_string())) };
        let v: serde_json::Value = serde_json::from_str(
            &v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap(),
        )
        .unwrap();
        v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "get_guide").unwrap().clone()
    };

    // A Core-only install: the writing guide alone, and nothing said about
    // the others.
    let core = guide_of(r#"{"disabled":[],"modules":["writing_style"]}"#);
    assert_eq!(core["inputSchema"]["properties"]["topic"]["enum"], serde_json::json!(["writing"]));
    let d = core["description"].as_str().unwrap();
    for gone in ["autorun", "api_templates", "reports", "Auto Run", "Crystal", "Quick rules"] {
        assert!(!d.contains(gone), "a Core-only description names {gone:?}: {d}");
    }
    for part in ["quick", "section", "full"] {
        assert!(core["inputSchema"]["properties"].get(part).is_none(), "{part}: {core}");
    }
    // Crystal Reports on: its guide comes with it.
    let reports = guide_of(r#"{"disabled":[],"modules":["writing_style","crystal_reports"]}"#);
    assert_eq!(reports["inputSchema"]["properties"]["topic"]["enum"], serde_json::json!(["writing", "reports"]));

    // Every module on, the Auto Run row switched off: no autorun topic, its
    // parts gone, the others there.
    const ROW_OFF: &str = r#"{"disabled":["save_autorun_script","get_autorun_script","delete_autorun_script","get_autorun_page","probe_autorun_locator","try_autorun_action","replay_autorun_to_step","start_autorun_discovery","discover_autorun_actions","save_autorun_area","end_autorun_discovery","reset_autorun_browser","save_autorun_component","get_autorun_failures","record_validation_rule","mark_autorun_suspected_defect","report_case_divergence","rule_case_divergence","clear_autorun_suspected_defect","set_autorun_order","propose_accounts","get_accounts","save_test_file","get_autorun_network_wait","record_explore_change","get_explore_changes"],"modules":["writing_style","auto_run","api_templates","crystal_reports","db_access","playwright"]}"#;
    let g = guide_of(ROW_OFF);
    assert_eq!(g["inputSchema"]["properties"]["topic"]["enum"], serde_json::json!(["writing", "api_templates", "reports"]));
    assert!(g["inputSchema"]["properties"].get("quick").is_none(), "{g}");
    assert!(!g["description"].as_str().unwrap().contains("\"autorun\""), "{g}");

    let ask = |tools: &'static str, topic: &str| -> String {
        let call = move |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
            assert_eq!(path, "/tools", "nothing past /tools for a refused topic");
            Ok((200, tools.to_string()))
        };
        let req = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"get_guide","arguments":{{"topic":"{topic}"}}}}}}"#);
        let v: serde_json::Value = serde_json::from_str(&v2_lib::mcp::handle_message(&req, "1.0.0", &call).unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true, "{topic}: {v}");
        v["result"]["content"][0]["text"].as_str().unwrap().to_string()
    };
    assert_eq!(
        ask(ROW_OFF, "autorun"),
        "The `autorun` guide is switched off in Test Case Manager. Turn it back on in the app's AI Bridge tab if you need it."
    );
    let said = ask(r#"{"disabled":[],"modules":["writing_style"]}"#, "reports");
    assert_eq!(said, format!("The `reports` guide is not available: {}.", Module::CrystalReports.off_sentence_for(false).trim_end_matches('.')));
    // An unknown topic names only the topics offered.
    let said = ask(r#"{"disabled":[],"modules":["writing_style"]}"#, "selectors");
    assert!(said.contains("\"writing\"") && !said.contains("autorun"), "{said}");

    // The quirk tool and the network tools decide nothing about a guide.
    let only_shared: Vec<String> =
        ["record_quirk", "search_autorun_requests", "autorun_cookies"].iter().map(|s| s.to_string()).collect();
    assert!(guide_offered("autorun", &only_shared));
    assert_eq!(
        offered_topics(&[]),
        vec!["writing", "autorun", "api_templates", "reports", "playwright_skills", "playwright_repo"]
    );
}

/// The argument mixes that used to be dropped without a word are refused
/// with the shape to send, before anything reaches the app: the autorun
/// guide's parts on another topic, a template filter on the fixtures list
/// (and `full` on the template list), the old new_report's summary fields
/// beside `create`, and the old retire tool's `id`.
#[test]
fn the_merged_tools_refuse_the_mixes_they_used_to_ignore() {
    use v2_lib::mcp::{
        FIXTURES_TAKE_ID_OR_FULL, FULL_IS_FOR_FIXTURES, GUIDE_PARTS_AUTORUN_ONLY, QUIRK_ID_WITHOUT_RETIRE,
        SUMMARY_GOES_IN_CREATE,
    };
    let posted = std::cell::Cell::new(0);
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path != "/tools" {
            posted.set(posted.get() + 1);
        }
        Ok((200, "{}".into()))
    };
    for (tool, args, said) in [
        ("get_guide", r#"{"topic":"writing","full":true}"#, GUIDE_PARTS_AUTORUN_ONLY),
        ("get_guide", r#"{"topic":"reports","section":"Ids"}"#, GUIDE_PARTS_AUTORUN_ONLY),
        ("get_guide", r#"{"topic":"api_templates","quick":true}"#, GUIDE_PARTS_AUTORUN_ONLY),
        ("list_api_templates", r#"{"kind":"fixtures","module":"PMS"}"#, FIXTURES_TAKE_ID_OR_FULL),
        ("list_api_templates", r#"{"kind":"fixtures","offset":25}"#, FIXTURES_TAKE_ID_OR_FULL),
        ("list_api_templates", r#"{"full":true}"#, FULL_IS_FOR_FIXTURES),
        ("list_api_templates", r#"{"kind":"templates","full":true}"#, FULL_IS_FOR_FIXTURES),
        ("open_report", r#"{"path":"C:/r/n.rpt","title":"T","create":true}"#, SUMMARY_GOES_IN_CREATE),
        ("open_report", r#"{"path":"C:/r/n.rpt","author":"A"}"#, SUMMARY_GOES_IN_CREATE),
        ("record_quirk", r#"{"id":"q1a2b3c","reason":"never helped"}"#, QUIRK_ID_WITHOUT_RETIRE),
    ] {
        let req = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#);
        let v: serde_json::Value = serde_json::from_str(&handle_message(&req, "1.0.0", &call).unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true, "{tool} {args}: {v}");
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains(said), "{tool} {args}: {v}");
    }
    assert_eq!(posted.get(), 0, "nothing reached the app");

    // An empty or false part is no part: still answered.
    for args in [r#"{"topic":"writing","quick":false}"#, r#"{"topic":"writing","section":""}"#] {
        let req = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"get_guide","arguments":{args}}}}}"#);
        let v: serde_json::Value = serde_json::from_str(&handle_message(&req, "1.0.0", &call).unwrap()).unwrap();
        assert_ne!(v["result"]["isError"], true, "{args}: {v}");
    }
}

/// The two account tools belong to the Auto Run row: listed while that row
/// is on and Auto Run is offered, gone when the row is switched off, and
/// "not available" where Auto Run is not offered at all.
#[test]
fn the_account_tools_are_listed_only_with_the_auto_run_row_where_auto_run_is_offered() {
    const ACCOUNT_TOOLS: [&str; 2] = ["propose_accounts", "get_accounts"];
    let names = listed_names(&stub(200, r#"{"disabled":[]}"#));
    for name in ACCOUNT_TOOLS {
        assert!(names.iter().any(|n| n == name), "{name} missing from {names:?}");
        assert!(module_of_tool(name) == Some(Module::AutoRun), "{name}");
    }

    let row_off = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((
                200,
                r#"{"disabled":["save_autorun_script","get_autorun_script","delete_autorun_script","get_autorun_network_wait","get_autorun_page","probe_autorun_locator","try_autorun_action","replay_autorun_to_step","start_autorun_discovery","discover_autorun_actions","save_autorun_area","end_autorun_discovery","reset_autorun_browser","save_autorun_component","get_autorun_failures","record_quirk","record_validation_rule","mark_autorun_suspected_defect","report_case_divergence","rule_case_divergence","clear_autorun_suspected_defect","set_autorun_order","propose_accounts","get_accounts","save_test_file","search_autorun_requests","get_autorun_request","autorun_capture","autorun_har","set_autorun_headers","autorun_cookies","record_explore_change","get_explore_changes"]}"#.into(),
            ));
        }
        Ok((200, "{}".into()))
    };
    let names = listed_names(&row_off);
    for name in ACCOUNT_TOOLS {
        assert!(!names.iter().any(|n| n == name), "{name} listed with the row off: {names:?}");
    }

    let (off, on) = tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["db_access","writing_style"]}"#.into())));
    assert!(!on.contains(&Module::AutoRun));
    for name in ACCOUNT_TOOLS {
        assert!(off.iter().any(|n| n == name), "{name} not disabled with its module off: {off:?}");
        assert!(v2_lib::mcp::refusal_text(name, &on).contains("not available"), "{name}");
    }
}

/// `propose_accounts` forwards its arguments whole; `get_accounts` is a
/// plain read. A proposal may carry each login's password from the same
/// database lookup - optional, and never a hash.
#[test]
fn the_account_tools_reach_their_routes() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let propose = tools.iter().find(|t| t["name"] == "propose_accounts").unwrap();
    assert_eq!(propose["inputSchema"]["required"], serde_json::json!(["accounts"]));
    let item = &propose["inputSchema"]["properties"]["accounts"]["items"];
    assert_eq!(item["properties"]["password"]["type"], "string", "{item}");
    assert_eq!(item["properties"]["password"]["maxLength"], 256, "{item}");
    let description = propose["description"].as_str().unwrap();
    assert!(description.contains("never a hash"), "{description}");
    assert!(description.contains("test environment"), "{description}");
    // On a test environment each account carries its password or a reason.
    assert_eq!(item["properties"]["no_password"]["type"], "string", "{item}");
    assert_eq!(item["properties"]["no_password"]["maxLength"], 200, "{item}");
    assert!(description.contains("no_password"), "{description}");
    assert!(
        description.contains(
            "On a test environment each account's password from the same database lookup that found its login is required"
        ),
        "{description}"
    );
    assert!(
        description.ends_with(
            "The user accounts are generally in the HR_SM_USER_ACCOUNT table: look there first (with db_query) for the logins and their passwords."
        ),
        "{description}"
    );
    assert_eq!(item["required"], serde_json::json!(["key", "label", "username"]));

    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".to_string()))
    };
    let args = serde_json::json!({ "accounts": [{ "key": "hr.sup", "label": "Supervisor", "username": "sup1" }] });
    for (name, args, method, path) in [
        ("propose_accounts", args.clone(), "POST", "/accounts-propose"),
        ("get_accounts", serde_json::json!({}), "GET", "/accounts"),
    ] {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args },
        });
        let resp = handle_message(&req.to_string(), "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_ne!(v["result"]["isError"], serde_json::json!(true), "{name}: {resp}");
        let (m, p, _) = calls.borrow().last().unwrap().clone();
        assert_eq!((m.as_str(), p.as_str()), (method, path), "{name}");
    }
    let (_, _, body) = calls.borrow().iter().find(|c| c.1 == "/accounts-propose").unwrap().clone();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap(), args);
}

/// Review of Task 8: an assistant learns a script names its `area` - and
/// that changing it is declared - from the save tool itself. Since the save
/// check credits a script only with what was seen in the area it names,
/// the tool says to always set it (`nav::SET_AREA_RULE`), so a script's
/// `area` is no longer shown as one to leave out.
#[test]
fn the_save_tool_documents_a_scripts_area() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "1.10.3", &stub(200, "")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "save_autorun_script")
        .expect("the save tool is listed in a development build")
        .clone();
    let description = tool["description"].as_str().unwrap();
    assert!(description.contains(v2_lib::autorun::nav::SET_AREA_RULE), "{description}");
    // The edit's `area` flag stays optional: true only when it changed.
    assert!(description.contains("its section \"Saving it\" has the seen-locator rules and the shape of `edits`"), "{description}");
    let props = &tool["inputSchema"]["properties"];
    let scripts = props["scripts"]["description"].as_str().unwrap();
    assert!(scripts.contains("area,") && !scripts.contains("area?"), "{props}");
    assert!(props["edits"]["description"].as_str().unwrap().contains("area? (true when you changed the script's area"), "{props}");
}

/// The project's Test files come with `get_accounts` (the old
/// `list_test_files` is gone): its description says so, and it is still a
/// plain read of `/accounts`.
#[test]
fn get_accounts_also_gives_the_test_files() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().all(|t| t["name"] != "list_test_files"), "list_test_files is gone");
    let d = tools.iter().find(|t| t["name"] == "get_accounts").unwrap()["description"].as_str().unwrap();
    assert!(d.contains("`test_files`") && d.contains("you cannot"), "{d}");
    assert!(d.contains("Never write a username or password into a script"), "{d}");
}

/// `save_test_file` rides with the Auto Run tools, where `get_accounts` shows the Test files: listed
/// only while Auto Run (and so, below it, API Templates) is on, refused as "not available" otherwise,
/// and a call forwards its arguments whole to `/test-file-save`.
#[test]
fn save_test_file_is_offered_only_with_auto_run_or_api_templates() {
    let listed = |modules: &'static str| -> bool {
        let call = move |_m: &str, _p: &str, _b: &str| -> Result<(u16, String), String> { Ok((200, modules.to_string())) };
        let v: serde_json::Value = serde_json::from_str(
            &handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap(),
        )
        .unwrap();
        v["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "save_test_file")
    };
    assert!(!listed(r#"{"disabled":[],"modules":["writing_style"]}"#));
    assert!(!listed(r#"{"disabled":[],"modules":["db_access"]}"#));
    assert!(listed(r#"{"disabled":[],"modules":["db_access","auto_run"]}"#));
    assert!(listed(r#"{"disabled":[],"modules":["db_access","auto_run","api_templates"]}"#));
    assert_eq!(v2_lib::ai_tools::module_of_tool("save_test_file"), Some(Module::AutoRun));

    // Off: a call is refused naming the module, and nothing reaches the app.
    let off = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        assert_eq!(path, "/tools", "nothing past /tools for a refused tool");
        Ok((200, r#"{"disabled":[],"modules":["writing_style"]}"#.to_string()))
    };
    let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"save_test_file","arguments":{"name":"a.txt","text":"x"}}}"#;
    let v: serde_json::Value = serde_json::from_str(&handle_message(req, "1.0.0", &off).unwrap()).unwrap();
    assert_eq!(v["result"]["isError"], true, "{v}");
    assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("not available"), "{v}");

    // On: the arguments travel whole.
    let seen = std::cell::RefCell::new(vec![]);
    let on = |m: &str, path: &str, body: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":[],"modules":["db_access","auto_run"]}"#.to_string()));
        }
        seen.borrow_mut().push((m.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".into()))
    };
    handle_message(req, "1.0.0", &on).unwrap();
    let seen = seen.borrow();
    assert_eq!((seen[0].0.as_str(), seen[0].1.as_str()), ("POST", "/test-file-save"));
    let sent: serde_json::Value = serde_json::from_str(&seen[0].2).unwrap();
    assert_eq!(sent, serde_json::json!({ "name": "a.txt", "text": "x" }));
}

/// `replay_autorun_to_step` is one of the Auto Run tools: listed with them
/// where Auto Run is offered, gone with the row switched off, and refused as
/// "not available" where Auto Run is not offered. A call forwards its
/// arguments to `/autorun-replay`, and the description says what the
/// assistant must know before calling it.
#[test]
fn the_replay_tool_rides_with_the_auto_run_tools() {
    const NAME: &str = "replay_autorun_to_step";
    assert!(module_of_tool(NAME) == Some(Module::AutoRun));
    assert!(listed_names(&stub(200, r#"{"disabled":[]}"#)).iter().any(|n| n == NAME));

    let row_off = stub(200, r#"{"disabled":["replay_autorun_to_step"]}"#);
    assert!(!listed_names(&row_off).iter().any(|n| n == NAME));
    let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"replay_autorun_to_step","arguments":{"case_id":7,"step":3}}}"#;
    let v: serde_json::Value = serde_json::from_str(&handle_message(req, "1.0.0", &row_off).unwrap()).unwrap();
    assert_eq!(v["result"]["isError"], true, "{v}");
    assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("switched off"), "{v}");

    let (off, on) = tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["db_access","writing_style"]}"#.into())));
    assert!(!on.contains(&Module::AutoRun));
    assert!(off.iter().any(|n| n == NAME), "{off:?}");
    assert!(v2_lib::mcp::refusal_text(NAME, &on).contains("not available"));

    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| -> Result<(u16, String), String> {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, r#"{"sentence":"replayed"}"#.into()))
    };
    let v: serde_json::Value = serde_json::from_str(&handle_message(req, "1.0.0", &call).unwrap()).unwrap();
    assert_ne!(v["result"]["isError"], serde_json::json!(true), "{v}");
    let recorded = calls.borrow();
    let last = recorded.last().unwrap();
    assert_eq!((last.0.as_str(), last.1.as_str()), ("POST", "/autorun-replay"));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&last.2).unwrap(), serde_json::json!({ "case_id": 7, "step": 3 }));

    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == NAME).unwrap();
    // Nothing is required: `stop: true` stops a replay with no case or step.
    assert_eq!(tool["inputSchema"]["required"], serde_json::json!([]));
    assert_eq!(tool["inputSchema"]["properties"]["stop"]["type"], "boolean");
    let d = tool["description"].as_str().unwrap();
    assert!(d.contains("Needs no browser open"), "{d}");
    assert!(d.contains("must not save replays at once too, its saves stopped throughout"), "{d}");
    assert!(!d.contains("Allow"), "{d}");
    assert!(d.contains("call again with the same case and step to keep waiting"), "{d}");
    assert!(d.contains("Do not release it between replays"), "{d}");
    assert!(d.contains("its `release` is for a broken browser only"), "{d}");
    assert!(!d.contains("closes a browser your replay opened"), "{d}");
    assert!(d.contains("never past it"), "{d}");
    assert!(d.contains("try_autorun_action"), "{d}");
    assert!(!d.contains('\u{2014}'), "{d}");
    // How a replay carries on, and a browser already signed in, are the
    // guide's: the description points at the section that has them.
    assert!(d.contains("get_guide (topic autorun), section \"Repairing a script that failed\""), "{d}");
    let g = v2_lib::autorun::guide::autorun_guide().split_whitespace().collect::<Vec<_>>().join(" ");
    for said in [
        "goes straight to its area (\"Signed in already: went to <area>.\")",
        "`start_autorun_discovery` takes it over (its other tabs closed, then home)",
        "the answer starts with \"Carried on from step K\" or \"Started from the beginning\"",
        "A finished answer nobody collected is kept for 90 seconds",
    ] {
        assert!(g.contains(said), "the guide never says {said:?}");
    }
}

/// The browser is shared by replays, discovery and resets, and the tool
/// descriptions say so: release only for a broken browser, never between
/// replays; a discovery takes a replay's browser over; the assistant's end
/// keeps it; a guard lasts as long as what set it.
#[test]
fn the_descriptions_keep_the_browser_between_replays() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let tool = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("no tool {name}")).clone();
    let desc = |name: &str| tool(name)["description"].as_str().unwrap().to_string();

    // Release is reset's `release`, kept for a broken browser: never between
    // replays, and never a stop.
    let reset = desc("reset_autorun_browser");
    assert!(reset.contains("only when it is broken"), "{reset}");
    assert!(reset.contains("frozen, on the wrong account, its session gone"), "{reset}");
    assert!(reset.contains("\"the browser did not answer\""), "{reset}");
    assert!(reset.contains("Never release between replays"), "{reset}");
    assert!(reset.contains("never stops a replay that is going"), "{reset}");
    assert!(reset.contains("Refused while a replay is running (replay_autorun_to_step with `stop: true` stops one)"), "{reset}");
    let release = &tool("reset_autorun_browser")["inputSchema"]["properties"]["release"];
    assert_eq!(release["type"], "boolean", "{release}");
    assert!(release["description"].as_str().unwrap().contains("Never between replays"), "{release}");
    // Stop is replay's `stop`, with the stop tool's own sentences.
    let stop = desc("replay_autorun_to_step");
    for said in [
        "`stop: true`, sent alone, stops your replay that is still going",
        "with no replay going it answers 404",
        "the person's own replay is refused",
    ] {
        assert!(stop.contains(said), "{said:?} missing from {stop}");
    }
    // How a stop ends is the guide's, in the replay part.
    let guide = v2_lib::autorun::guide::autorun_guide().split_whitespace().collect::<Vec<_>>().join(" ");
    for said in [
        "A failed action ends a replay at once.",
        "it stops before the next action and says where",
        "stopped on request, after which action of which step, with no page",
        "send `stop: true` again to keep waiting",
    ] {
        assert!(guide.contains(said), "the guide never says {said:?}");
    }

    let start = desc("start_autorun_discovery");
    assert!(start.contains("takes over a browser your own replay left open"), "{start}");
    assert!(guide.contains("Took over the browser your replay left open"), "the guide quotes the takeover");
    assert!(!start.contains("is closed first, and the answer says"), "{start}");
    assert!(start.contains("until the discovery ends"), "{start}");

    // Not only the person's button closes it: release does, and so does an
    // end whose guard will not lift.
    let end = desc("end_autorun_discovery");
    assert!(!end.contains("Only the person's End discovery button closes it"), "{end}");
    assert!(end.contains("the person's End discovery button, a release (reset_autorun_browser)"), "{end}");
    assert!(end.contains("an end whose save guard will not lift"), "{end}");

    // A takeover closes the other tabs and goes home, as the start says.
    let replay = desc("replay_autorun_to_step");
    assert!(!replay.contains("so does start_autorun_discovery"), "{replay}");
    assert!(!replay.contains("as it is"), "{replay}");
    assert!(guide.contains("`start_autorun_discovery` takes it over (its other tabs closed, then home)"), "the guide says how a takeover goes");

    // A batch's guard lasts as its parameters say.
    let batch = desc("discover_autorun_actions");
    assert!(
        batch.contains("A guard this call's own `case_id` or `no_save` sets lasts for this call only; one the discovery started with lasts until it ends."),
        "{batch}"
    );

    for name in ["discover_autorun_actions", "start_autorun_discovery"] {
        let props = &tool(name)["inputSchema"]["properties"];
        let case_id = props["case_id"]["description"].as_str().unwrap().to_string();
        let no_save = props["no_save"]["description"].as_str().unwrap().to_string();
        if name == "start_autorun_discovery" {
            assert!(case_id.contains("until the discovery ends"), "{case_id}");
            assert!(no_save.contains("until the discovery ends"), "{no_save}");
        } else {
            assert!(case_id.contains("for this call only"), "{name}: {case_id}");
            assert!(case_id.contains("name it on every call"), "{name}: {case_id}");
            assert!(no_save.contains("for this call only"), "{name}: {no_save}");
        }
    }
    for name in ["reset_autorun_browser", "start_autorun_discovery", "end_autorun_discovery", "replay_autorun_to_step"] {
        assert!(!desc(name).contains('\u{2014}'), "{name} has an em dash");
    }
}

/// `set_autorun_order` is one of the Auto Run tools: listed with them where
/// Auto Run is offered, gone with the row switched off, and refused as "not
/// available" where Auto Run is not offered. A call forwards its arguments
/// to `/autorun-order`, and the description says what the order is for and
/// that Run Tests' order is not changed.
#[test]
fn the_order_tool_rides_with_the_auto_run_tools() {
    const NAME: &str = "set_autorun_order";
    assert!(module_of_tool(NAME) == Some(Module::AutoRun));
    assert!(listed_names(&stub(200, r#"{"disabled":[]}"#)).iter().any(|n| n == NAME));

    let row_off = stub(200, r#"{"disabled":["set_autorun_order"]}"#);
    assert!(!listed_names(&row_off).iter().any(|n| n == NAME));
    let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"set_autorun_order","arguments":{"pbi_id":100,"case_ids":[503,501]}}}"#;
    let v: serde_json::Value = serde_json::from_str(&handle_message(req, "1.0.0", &row_off).unwrap()).unwrap();
    assert_eq!(v["result"]["isError"], true, "{v}");
    assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("switched off"), "{v}");

    let (off, on) = tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["db_access","writing_style"]}"#.into())));
    assert!(!on.contains(&Module::AutoRun));
    assert!(off.iter().any(|n| n == NAME), "{off:?}");
    assert!(v2_lib::mcp::refusal_text(NAME, &on).contains("not available"));

    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| -> Result<(u16, String), String> {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "saved Auto Run's order for PBI 100: 503, 501".into()))
    };
    let v: serde_json::Value = serde_json::from_str(&handle_message(req, "1.0.0", &call).unwrap()).unwrap();
    assert_ne!(v["result"]["isError"], serde_json::json!(true), "{v}");
    let recorded = calls.borrow();
    let last = recorded.last().unwrap();
    assert_eq!((last.0.as_str(), last.1.as_str()), ("POST", "/autorun-order"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&last.2).unwrap(),
        serde_json::json!({ "pbi_id": 100, "case_ids": [503, 501] })
    );

    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == NAME).unwrap();
    assert_eq!(tool["inputSchema"]["required"], serde_json::json!(["pbi_id", "case_ids"]));
    assert_eq!(tool["inputSchema"]["properties"]["case_ids"]["type"], "array");
    let d = tool["description"].as_str().unwrap();
    assert!(d.contains("Run selected and Run unattended"), "{d}");
    assert!(d.contains("It does not change Run Tests' order."), "{d}");
    assert!(d.contains("is not in PBI"), "{d}");
    assert!(d.contains("no saved script"), "{d}");
    assert!(!d.contains('\u{2014}'), "{d}");
}

/// The five discovery tools, Release and Reset ride with the Auto Run tools, exactly as
/// `probe_autorun_locator` does: listed where Auto Run is offered, gone and
/// refused "switched off" when the person's list names them, refused "not
/// available" where Auto Run is not offered. Each call forwards its
/// arguments whole to its own bridge route.
#[test]
fn the_discovery_tools_ride_with_the_auto_run_tools_and_reach_their_routes() {
    let tools: [(&str, &str, serde_json::Value, serde_json::Value); 5] = [
        (
            "start_autorun_discovery",
            "/autorun-discover-start",
            serde_json::json!({ "account": "admin", "area": "Leave Apply", "browser": "edge", "mapping": true, "modules": ["Leave"] }),
            serde_json::json!(["account"]),
        ),
        (
            "discover_autorun_actions",
            "/autorun-discover-actions",
            serde_json::json!({ "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Add" } }], "stop_on_failure": false, "area": "Leave Apply" }),
            serde_json::json!([]),
        ),
        (
            "save_autorun_area",
            "/autorun-discover-area",
            serde_json::json!({ "name": "Leave Apply", "module": "Leave", "clicks": [{ "role": "link", "name": "Leave" }] }),
            serde_json::json!(["name", "module", "clicks"]),
        ),
        ("end_autorun_discovery", "/autorun-discover-end", serde_json::json!({}), serde_json::json!([])),
        ("reset_autorun_browser", "/autorun-reset", serde_json::json!({}), serde_json::json!([])),
    ];
    rides_with_the_auto_run_tools(&tools);
}

/// A tools/call of `name` with `args` through a recording bridge: the
/// result, and every call that reached the app other than `/tools`.
fn call_recorded(name: &str, args: serde_json::Value) -> (serde_json::Value, Vec<(String, String, String)>) {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| -> Result<(u16, String), String> {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".into()))
    };
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": name, "arguments": args },
    });
    let v: serde_json::Value = serde_json::from_str(&handle_message(&req.to_string(), "1.0.0", &call).unwrap()).unwrap();
    let reached = calls.into_inner().into_iter().filter(|c| c.1 != "/tools").collect();
    (v, reached)
}

/// The tool said `want` and nothing reached the app.
fn refused_before_the_app(name: &str, args: serde_json::Value, want: &str) {
    let (v, reached) = call_recorded(name, args.clone());
    assert_eq!(v["result"]["isError"], true, "{name} {args}: {v}");
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains(want), "{name} {args}: {text}");
    assert!(reached.is_empty(), "{name} {args} reached the app: {reached:?}");
}

/// The one route `name` with `args` reached, and the body it sent.
fn reached(name: &str, args: serde_json::Value) -> (String, String, serde_json::Value) {
    let (v, reached) = call_recorded(name, args.clone());
    assert_ne!(v["result"]["isError"], true, "{name} {args}: {v}");
    assert_eq!(reached.len(), 1, "{name} {args}: {reached:?}");
    let (m, p, b) = reached.into_iter().next().unwrap();
    let body = if b.is_empty() { serde_json::Value::Null } else { serde_json::from_str(&b).unwrap() };
    (m, p, body)
}

/// `discover_autorun_actions` takes one `action` (the one-action route,
/// whose answer is the richer one) or `actions` (the batch route), the
/// arguments whole either way; both, neither, or `stop_on_failure` beside
/// one `action` are refused before anything reaches the app.
#[test]
fn discover_autorun_actions_takes_one_action_or_several() {
    use v2_lib::mcp::{DISCOVER_ACTION_OR_ACTIONS, STOP_ON_FAILURE_GOES_WITH_ACTIONS};
    let one = serde_json::json!({ "action": { "kind": "click", "selector": { "role": "button", "name": "Add" } }, "area": "Leave Apply", "case_id": 7, "no_save": true, "page": false });
    assert_eq!(reached("discover_autorun_actions", one.clone()), ("POST".into(), "/autorun-discover-action".into(), one));
    let several = serde_json::json!({ "actions": [{ "kind": "reload" }], "stop_on_failure": false });
    assert_eq!(reached("discover_autorun_actions", several.clone()), ("POST".into(), "/autorun-discover-actions".into(), several));
    let draft = serde_json::json!({ "action": { "kind": "use_component", "name": "Pick a date" }, "draft": { "name": "Pick a date" }, "actions": null });
    assert_eq!(reached("discover_autorun_actions", draft.clone()).1, "/autorun-discover-action");

    for args in [
        serde_json::json!({ "action": { "kind": "reload" }, "actions": [{ "kind": "reload" }] }),
        serde_json::json!({}),
        serde_json::json!({ "area": "Leave Apply" }),
    ] {
        refused_before_the_app("discover_autorun_actions", args, DISCOVER_ACTION_OR_ACTIONS);
    }
    refused_before_the_app(
        "discover_autorun_actions",
        serde_json::json!({ "action": { "kind": "reload" }, "stop_on_failure": false }),
        STOP_ON_FAILURE_GOES_WITH_ACTIONS,
    );
    let names = listed_names(&stub(200, r#"{"disabled":[]}"#));
    assert!(!names.iter().any(|n| n == "discover_autorun_action"), "the one-action tool is gone: {names:?}");
}

/// `replay_autorun_to_step` with `stop: true` alone stops the assistant's
/// replay through the stop route (the old `stop_autorun_replay` is gone);
/// `stop` beside anything else, or not a boolean, is refused before the
/// app; `stop: false` is a plain replay without the flag.
#[test]
fn replay_autorun_to_step_stops_with_stop_and_nothing_else() {
    use v2_lib::mcp::STOP_TAKES_NOTHING_ELSE;
    assert_eq!(
        reached("replay_autorun_to_step", serde_json::json!({ "stop": true })),
        ("POST".into(), "/autorun-replay-stop".into(), serde_json::json!({}))
    );
    assert_eq!(
        reached("replay_autorun_to_step", serde_json::json!({ "case_id": 7, "step": 3, "stop": false })),
        ("POST".into(), "/autorun-replay".into(), serde_json::json!({ "case_id": 7, "step": 3 }))
    );
    refused_before_the_app("replay_autorun_to_step", serde_json::json!({ "stop": true, "case_id": 7, "step": 3 }), STOP_TAKES_NOTHING_ELSE);
    refused_before_the_app("replay_autorun_to_step", serde_json::json!({ "stop": true, "page": false }), STOP_TAKES_NOTHING_ELSE);
    refused_before_the_app("replay_autorun_to_step", serde_json::json!({ "stop": "yes" }), "`stop` is true or false");
    let names = listed_names(&stub(200, r#"{"disabled":[]}"#));
    assert!(!names.iter().any(|n| n == "stop_autorun_replay"), "{names:?}");
}

/// `reset_autorun_browser` with `release: true` reaches the release route
/// (the old `release_autorun_browser` is gone); a `release` that is not a
/// boolean is refused before the app.
#[test]
fn reset_autorun_browser_releases_with_release() {
    assert_eq!(
        reached("reset_autorun_browser", serde_json::json!({ "release": true })),
        ("POST".into(), "/autorun-release".into(), serde_json::json!({}))
    );
    for args in [serde_json::json!({}), serde_json::json!({ "release": false }), serde_json::json!({ "release": null })] {
        assert_eq!(reached("reset_autorun_browser", args.clone()), ("POST".into(), "/autorun-reset".into(), serde_json::json!({})), "{args}");
    }
    refused_before_the_app("reset_autorun_browser", serde_json::json!({ "release": "now" }), "`release` is true or false");
    let names = listed_names(&stub(200, r#"{"disabled":[]}"#));
    assert!(!names.iter().any(|n| n == "release_autorun_browser"), "{names:?}");
}

/// Arguments a merged network or reset tool would drop are refused by name,
/// with the shape to send, before anything reaches the app.
#[test]
fn the_network_and_reset_tools_refuse_arguments_they_would_drop() {
    refused_before_the_app("autorun_har", serde_json::json!({ "export": "a", "name": "b" }), "`autorun_har` takes only `export` or `import`, and the name is its value - not name");
    refused_before_the_app("autorun_har", serde_json::json!({ "import": "a", "from": "b" }), "not from");
    refused_before_the_app("autorun_capture", serde_json::json!({ "from": "x" }), "`autorun_capture` takes only the filter fields (include_types, exclude_types, path_contains) - not from; to read an imported set pass `from` to `search_autorun_requests` or `get_autorun_request`");
    refused_before_the_app("autorun_capture", serde_json::json!({ "path_contains": "/api", "name": "x" }), "not name");
    refused_before_the_app("reset_autorun_browser", serde_json::json!({ "release": true, "case_id": 3 }), "`reset_autorun_browser` takes only `release` (true, to let a broken browser go) - not case_id");
    // What they do take still goes through.
    assert_eq!(
        reached("autorun_har", serde_json::json!({ "export": "a", "import": null })),
        ("POST".into(), "/autorun-requests-export".into(), serde_json::json!({ "name": "a" }))
    );
    assert_eq!(reached("autorun_capture", serde_json::json!({ "from": null })).0, "GET");
}

/// `probe_autorun_locator` with `show` looks deeper at one element through
/// the inspect route (the old `inspect_autorun_element` is gone): the
/// selector travels as the route's `locator`, `show: true` asks for every
/// part, a list asks for those; `properties` without `show`, or a `show`
/// that is neither, is refused before the app.
#[test]
fn probe_autorun_locator_shows_one_element_with_show() {
    use v2_lib::mcp::{PROPERTIES_GO_WITH_SHOW, SHOW_IS_TRUE_OR_PARTS};
    let chain = serde_json::json!([{ "role": "dialog", "name": "Add Rating" }, { "role": "button", "name": "Save" }]);
    assert_eq!(
        reached("probe_autorun_locator", serde_json::json!({ "selector": chain.clone() })),
        ("POST".into(), "/autorun-probe".into(), serde_json::json!({ "selector": chain.clone() }))
    );
    assert_eq!(
        reached(
            "probe_autorun_locator",
            serde_json::json!({ "selector": chain.clone(), "show": ["styles", "listeners"], "properties": ["color", "pointer-events"] })
        ),
        (
            "POST".into(),
            "/autorun-inspect".into(),
            serde_json::json!({ "locator": chain.clone(), "show": ["styles", "listeners"], "properties": ["color", "pointer-events"] })
        )
    );
    assert_eq!(
        reached("probe_autorun_locator", serde_json::json!({ "selector": "#save", "show": true })),
        ("POST".into(), "/autorun-inspect".into(), serde_json::json!({ "locator": "#save" }))
    );
    assert_eq!(reached("probe_autorun_locator", serde_json::json!({ "selector": "#save", "show": false })).1, "/autorun-probe");
    refused_before_the_app("probe_autorun_locator", serde_json::json!({ "selector": "#save", "properties": ["color"] }), PROPERTIES_GO_WITH_SHOW);
    refused_before_the_app("probe_autorun_locator", serde_json::json!({ "selector": "#save", "show": "styles" }), SHOW_IS_TRUE_OR_PARTS);
    // `show` is worded as the probe's own: the element is `selector`. The
    // retired inspect shape ({ locator, show }), a `show` with no selector
    // and an empty `show` list are each refused before the inspect route.
    use v2_lib::mcp::SHOW_NEEDS_A_SELECTOR;
    refused_before_the_app("probe_autorun_locator", serde_json::json!({ "show": ["styles"] }), SHOW_NEEDS_A_SELECTOR);
    refused_before_the_app("probe_autorun_locator", serde_json::json!({ "show": true, "selector": null }), SHOW_NEEDS_A_SELECTOR);
    refused_before_the_app("probe_autorun_locator", serde_json::json!({ "locator": "#save", "show": ["styles"] }), SHOW_NEEDS_A_SELECTOR);
    refused_before_the_app("probe_autorun_locator", serde_json::json!({ "selector": "#save", "locator": "#save", "show": true }), SHOW_NEEDS_A_SELECTOR);
    refused_before_the_app("probe_autorun_locator", serde_json::json!({ "selector": "#save", "show": [] }), SHOW_IS_TRUE_OR_PARTS);

    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let probe = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "probe_autorun_locator").unwrap();
    assert_eq!(probe["inputSchema"]["required"], serde_json::json!(["selector"]));
    assert_eq!(probe["inputSchema"]["properties"]["show"]["items"]["enum"], serde_json::json!(v2_lib::mcp::SHOW_PARTS));
    let d = probe["description"].as_str().unwrap();
    for said in ["refused when the locator matches nothing or more than one", "Read only: nothing is clicked, typed, focused or scrolled", "recorded as seen"] {
        assert!(d.contains(said), "{said:?} missing from {d}");
    }
    let names = listed_names(&stub(200, r#"{"disabled":[]}"#));
    assert!(!names.iter().any(|n| n == "inspect_autorun_element"), "{names:?}");
}

/// The discovery tool takes one action or a list of them and an optional
/// boolean `stop_on_failure`, and says its limit, that it stops at a failure, and
/// that it answers the page once.
#[test]
fn discover_autorun_actions_documents_its_list_its_limit_and_stopping() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let list: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = list["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "discover_autorun_actions").unwrap();
    let props = &tool["inputSchema"]["properties"];
    assert_eq!(props["actions"]["type"], "array");
    assert_eq!(props["actions"]["items"]["type"], "object");
    assert_eq!(props["stop_on_failure"]["type"], "boolean");
    assert_eq!(props["area"]["type"], "string");
    assert_eq!(props["draft"]["type"], "object");
    assert_eq!(props["action"]["type"], "object");
    assert_eq!(tool["inputSchema"]["required"], serde_json::json!([]));
    let d = tool["description"].as_str().unwrap();
    for said in ["at most 20 actions", "never both", "stop_on_failure", "the page once", "if any would be refused, none runs", "A `draft` that no use_component action names is refused", "start_autorun_discovery"] {
        assert!(d.contains(said), "the description never says {said}: {d}");
    }
}

/// Starting a discovery documents the mapping run: a boolean `mapping` and a
/// list of `modules`, neither required.
#[test]
fn start_autorun_discovery_documents_mapping_and_modules() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let list: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "start_autorun_discovery")
        .expect("start_autorun_discovery is listed");
    let props = &tool["inputSchema"]["properties"];
    assert_eq!(props["mapping"]["type"], "boolean", "{props}");
    assert_eq!(props["modules"]["type"], "array", "{props}");
    assert_eq!(props["modules"]["items"]["type"], "string", "{props}");
    assert_eq!(tool["inputSchema"]["required"], serde_json::json!(["account"]));
}

/// Each tool is gated like `probe_autorun_locator`, refused the right way
/// when off, and forwards its arguments whole to its own route.
fn rides_with_the_auto_run_tools(tools: &[(&str, &str, serde_json::Value, serde_json::Value)]) {
    let listed = listed_names(&stub(200, r#"{"disabled":[]}"#));
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let list: serde_json::Value = serde_json::from_str(&resp).unwrap();
    for (name, route, args, required) in tools {
        let (name, route) = (*name, *route);
        assert!(module_of_tool(name) == Some(Module::AutoRun), "{name} must gate like probe_autorun_locator");
        assert!(!v2_lib::ai_tools::CORE_TOOLS.contains(&name), "{name} must be switchable");
        assert!(listed.iter().any(|n| n == name), "{name} missing from {listed:?}");

        let off = format!(r#"{{"disabled":["probe_autorun_locator","{name}"]}}"#);
        let row_off = stub(200, &off);
        assert!(!listed_names(&row_off).iter().any(|n| n == name), "{name}");
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": name, "arguments": args },
        })
        .to_string();
        let v: serde_json::Value = serde_json::from_str(&handle_message(&req, "1.0.0", &row_off).unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true, "{name}: {v}");
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("switched off"), "{name}: {v}");

        let (off, on) = tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["db_access","writing_style"]}"#.into())));
        assert!(!on.contains(&Module::AutoRun));
        assert!(off.iter().any(|n| n == name), "{name}: {off:?}");
        assert!(v2_lib::mcp::refusal_text(name, &on).contains("not available"), "{name}");

        let calls = std::cell::RefCell::new(vec![]);
        let call = |method: &str, path: &str, body: &str| -> Result<(u16, String), String> {
            calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
            Ok((200, "{}".into()))
        };
        let v: serde_json::Value = serde_json::from_str(&handle_message(&req, "1.0.0", &call).unwrap()).unwrap();
        assert_ne!(v["result"]["isError"], serde_json::json!(true), "{name}: {v}");
        let recorded = calls.borrow();
        let last = recorded.last().unwrap();
        assert_eq!((last.0.as_str(), last.1.as_str()), ("POST", route), "{name}");
        assert_eq!(&serde_json::from_str::<serde_json::Value>(&last.2).unwrap(), args, "{name}");

        let tool = list["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(&tool["inputSchema"]["required"], required, "{name}");
        let d = tool["description"].as_str().unwrap();
        assert!(!d.is_empty(), "{name}");
        assert!(!d.contains('\u{2014}') && !d.contains('\u{2013}'), "{name}: {d}");
    }
}

/// The two component tools ride with the Auto Run tools as discovery's
/// do, and reach the save and retire routes with their arguments whole.
/// `discover_autorun_actions` says it takes a `use_component` with a
/// `draft`, and the save says it needs that live try first.
#[test]
fn the_component_tools_ride_with_the_auto_run_tools_and_reach_their_routes() {
    let tools: [(&str, &str, serde_json::Value, serde_json::Value); 1] = [
        (
            "save_autorun_component",
            "/autorun-component-save",
            serde_json::json!({
                "name": "Pick a date",
                "description": "Picks a day in a date field",
                "inputs": [
                    { "name": "field", "kind": "target", "description": "the date field" },
                    { "name": "day", "kind": "text", "description": "the day of the month" }
                ],
                "actions": [
                    { "kind": "click", "selector": { "input": "field" } },
                    { "kind": "click", "selector": { "role": "gridcell", "name": "{{day}}" } }
                ],
                "why": "the calendar now opens on focus"
            }),
            // Only the name: a removal needs nothing else, and the save
            // route names whatever else a save is missing.
            serde_json::json!(["name"]),
        ),
    ];
    rides_with_the_auto_run_tools(&tools);

    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = |name: &str| v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap().clone();
    let discover = tool("discover_autorun_actions");
    let d = discover["description"].as_str().unwrap();
    assert!(d.contains("use_component") && d.contains("draft"), "{d}");
    assert!(discover["inputSchema"]["properties"]["draft"].is_object(), "{discover}");
    let save = tool("save_autorun_component");
    let d = save["description"].as_str().unwrap();
    for said in ["discover_autorun_actions", "draft", "why", "tried live again"] {
        assert!(d.contains(said), "the save tool never says {said:?}: {d}");
    }
    assert!(d.contains("`remove: true`") && d.contains("uses it"), "{d}");
    assert!(v["result"]["tools"].as_array().unwrap().iter().all(|t| t["name"] != "remove_autorun_component"));
}

/// `save_autorun_component` with `remove: true` and only the name reaches
/// the retire route (the old `remove_autorun_component` is gone); `remove`
/// beside a component, or a `remove` that is not true or false, is refused
/// before anything reaches the app; `remove: false` is a plain save.
#[test]
fn save_autorun_component_removes_with_remove_and_only_the_name() {
    use v2_lib::mcp::{REMOVE_IS_TRUE_OR_FALSE, REMOVE_TAKES_ONLY_THE_NAME};
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".to_string()))
    };
    let send = |args: serde_json::Value| -> serde_json::Value {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "save_autorun_component", "arguments": args },
        });
        serde_json::from_str(&handle_message(&req.to_string(), "1.0.0", &call).unwrap()).unwrap()
    };
    let posts = || calls.borrow().iter().filter(|c| c.1 != "/tools").cloned().collect::<Vec<_>>();

    let v = send(serde_json::json!({ "name": "Pick a date", "remove": true }));
    assert_ne!(v["result"]["isError"], true, "{v}");
    let (m, p, b) = posts().last().unwrap().clone();
    assert_eq!((m.as_str(), p.as_str()), ("POST", "/autorun-component-retire"));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&b).unwrap(), serde_json::json!({ "name": "Pick a date" }));

    let save = serde_json::json!({ "name": "Pick a date", "description": "d", "inputs": [], "actions": [], "remove": false });
    send(save);
    let (_, p, b) = posts().last().unwrap().clone();
    assert_eq!(p, "/autorun-component-save");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&b).unwrap(),
        serde_json::json!({ "name": "Pick a date", "description": "d", "inputs": [], "actions": [] })
    );

    let before = posts().len();
    for (args, said) in [
        (serde_json::json!({ "name": "Pick a date", "remove": true, "actions": [] }), REMOVE_TAKES_ONLY_THE_NAME),
        (serde_json::json!({ "name": "Pick a date", "remove": true, "dry_run": true }), REMOVE_TAKES_ONLY_THE_NAME),
        (serde_json::json!({ "name": "Pick a date", "remove": "yes" }), REMOVE_IS_TRUE_OR_FALSE),
    ] {
        let v = send(args.clone());
        assert_eq!(v["result"]["isError"], true, "{args}: {v}");
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains(said), "{args}: {v}");
    }
    assert_eq!(posts().len(), before, "nothing reached the app");
}

/// The owner's call: scripts are repaired and components changed as often
/// as needed, and either may drop a check. Neither save tool names a cap
/// or a rule against dropping a check; the script save still says steps
/// left out of `edits` stay as they were.
#[test]
fn the_save_tools_name_no_cap_and_let_a_check_go() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let description = |name: &str| {
        v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap()["description"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let script = description("save_autorun_script");
    assert!(script.contains("Steps you leave out of `edits` must stay exactly as they were"), "{script}");
    assert!(script.contains("must name each one it gives up in `gives_up`"), "{script}");
    let component = description("save_autorun_component");
    for d in [&script, &component] {
        for gone in ["three repairs", "never removed or weakened", "cap_reached", "may not remove a check", "accepted changes"] {
            assert!(!d.contains(gone), "a save tool still says {gone:?}: {d}");
        }
    }
}

/// The guide tool no longer sends the assistant to the source: the live
/// app is where locators come from now.
#[test]
fn the_guide_tool_no_longer_points_at_the_source() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "get_guide").unwrap();
    let d = tool["description"].as_str().unwrap();
    assert!(!d.contains("You may read the application's source for SELECTORS"), "{d}");
    assert!(d.contains("every assertion from the case's own expected result, never from what the code does"), "{d}");
}

/// `get_guide` with topic autorun takes `quick`, says what it answers, and
/// forwards it to the bridge as `?quick=true`; with no other arguments it
/// asks for the overview (the Quick rules and the section titles), and for
/// the whole guide only with `full` true.
#[test]
fn the_guide_tool_takes_quick() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "get_guide").unwrap();
    assert_eq!(tool["inputSchema"]["properties"]["quick"]["type"], "boolean", "{tool}");
    assert_eq!(tool["inputSchema"]["properties"]["full"]["type"], "boolean", "{tool}");
    assert!(tool["description"].as_str().unwrap().contains("Quick rules"), "{tool}");

    for (args, want) in [
        (r#"{"topic":"autorun","quick":true}"#, "/autorun-guide?quick=true"),
        (r#"{"topic":"autorun","quick":false}"#, "/autorun-guide?overview=true"),
        (r#"{"topic":"autorun"}"#, "/autorun-guide?overview=true"),
        (r#"{"topic":" autorun "}"#, "/autorun-guide?overview=true"),
        (r#"{"topic":"autorun","full":true}"#, "/autorun-guide"),
        (r#"{"topic":"autorun","full":false}"#, "/autorun-guide?overview=true"),
        (r#"{"topic":"writing"}"#, "/guide"),
        (r#"{"topic":"writing","full":false}"#, "/guide"),
        (r#"{"topic":"api_templates"}"#, "/api-template-guide"),
        (r#"{"topic":"reports"}"#, "/crystal-guide"),
    ] {
        let calls = std::cell::RefCell::new(vec![]);
        let call = |method: &str, path: &str, _body: &str| {
            calls.borrow_mut().push((method.to_string(), path.to_string()));
            Ok((200, "{}".to_string()))
        };
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"get_guide","arguments":{args}}}}}"#
        );
        handle_message(&req, "1.0.0", &call).unwrap();
        let recorded = calls.borrow();
        assert_eq!(recorded.last().unwrap(), &("GET".to_string(), want.to_string()), "{args}");
    }
}

/// `get_guide` is Core - listed whatever is on - but a topic whose module
/// is off is refused before anything reaches the app, in the words the
/// topic's own guide tool was refused in while it was the module's. The
/// writing guide is always answered.
#[test]
fn the_guide_tool_refuses_a_topic_whose_module_is_off() {
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":[],"modules":["db_access","writing_style"]}"#.into()));
        }
        calls.borrow_mut().push(path.to_string());
        Ok((200, "the guide".into()))
    };
    let ask = |topic: &str| -> serde_json::Value {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "get_guide", "arguments": { "topic": topic } },
        });
        serde_json::from_str(&v2_lib::mcp::handle_message(&req.to_string(), "1.0.0", &call).unwrap()).unwrap()
    };
    for (topic, module) in
        [("autorun", Module::AutoRun), ("api_templates", Module::ApiTemplates), ("reports", Module::CrystalReports)]
    {
        let v = ask(topic);
        assert_eq!(v["result"]["isError"], true, "{topic}: {v}");
        assert_eq!(
            v["result"]["content"][0]["text"],
            format!("The `{topic}` guide is not available: {}.", module.off_sentence_for(false).trim_end_matches('.')),
            "{topic}"
        );
    }
    assert!(calls.borrow().is_empty(), "nothing reached the app: {:?}", calls.borrow());

    let v = ask("writing");
    assert_ne!(v["result"]["isError"], true, "{v}");
    assert_eq!(v["result"]["content"][0]["text"], "the guide");
    assert_eq!(*calls.borrow(), vec!["/guide".to_string()]);

    let list = v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
    assert!(list.contains("\"name\":\"get_guide\""), "get_guide is Core: {list}");
    assert!(module_of_tool("get_guide").is_none());
}

/// No topic, an empty one or one there is no guide for is refused before
/// anything reaches the app, naming the four topics. Each topic belongs to
/// the module its old guide tool did.
#[test]
fn the_guide_tool_refuses_a_missing_or_unknown_topic() {
    use v2_lib::mcp::{guide_refusal, guide_topic_module, GUIDE_TOPICS};
    let posted = std::cell::Cell::new(0);
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path != "/tools" {
            posted.set(posted.get() + 1);
        }
        Ok((200, "{}".into()))
    };
    for args in ["{}", r#"{"topic":""}"#, r#"{"topic":"selectors"}"#, r#"{"topic":7}"#, r#"{"section":"Selectors"}"#] {
        let req = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"get_guide","arguments":{args}}}}}"#);
        let v: serde_json::Value = serde_json::from_str(&handle_message(&req, "1.0.0", &call).unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true, "{args}: {v}");
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        for topic in GUIDE_TOPICS {
            assert!(text.contains(&format!("\"{topic}\"")), "{args}: names {topic}: {text}");
        }
    }
    assert_eq!(posted.get(), 0, "nothing reached the app");

    assert_eq!(GUIDE_TOPICS, ["writing", "autorun", "api_templates", "reports", "playwright_skills", "playwright_repo"]);
    assert_eq!(guide_topic_module("playwright_skills"), Ok(Some(Module::AutoRun)));
    assert_eq!(guide_topic_module("playwright_repo"), Ok(Some(Module::AutoRun)));
    assert_eq!(guide_topic_module("writing"), Ok(None));
    assert_eq!(guide_topic_module("autorun"), Ok(Some(Module::AutoRun)));
    assert_eq!(guide_topic_module("api_templates"), Ok(Some(Module::ApiTemplates)));
    assert_eq!(guide_topic_module("reports"), Ok(Some(Module::CrystalReports)));
    assert_eq!(guide_topic_module("Autorun"), Err(()));
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    for topic in GUIDE_TOPICS {
        assert_eq!(guide_refusal(Some(topic), &[], &all), None, "{topic}");
    }
}

/// `get_guide` (topic autorun) takes `section` and forwards it percent-encoded
/// (it wins over `quick`); `get_autorun_page` takes `limit` and `within`,
/// a locator forwarded as its JSON text (a css string as itself), and says
/// it answers 120 lines by default.
#[test]
fn the_guide_takes_a_section_and_the_page_takes_within() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let guide = tools.iter().find(|t| t["name"] == "get_guide").unwrap();
    assert_eq!(guide["inputSchema"]["properties"]["section"]["type"], "string", "{guide}");
    let page = tools.iter().find(|t| t["name"] == "get_autorun_page").unwrap();
    assert!(page["inputSchema"]["properties"]["within"].is_object(), "{page}");
    let d = page["description"].as_str().unwrap();
    assert!(d.contains("first 120 lines") && d.contains("`within`"), "{d}");

    for (tool, args, want) in [
        ("get_guide", r#"{"topic":"autorun","section":"Saving it"}"#, "/autorun-guide?section=Saving%20it"),
        ("get_guide", r#"{"topic":"autorun","section":"Selectors","quick":true}"#, "/autorun-guide?section=Selectors"),
        ("get_guide", r#"{"topic":"autorun","section":"  ","quick":true}"#, "/autorun-guide?quick=true"),
        ("get_autorun_page", "{}", "/autorun-page"),
        ("get_autorun_page", r#"{"limit":40}"#, "/autorun-page?limit=40"),
        ("get_autorun_page", r##"{"within":"#form"}"##, "/autorun-page?within=%23form"),
        (
            "get_autorun_page",
            r#"{"limit":10,"within":{"role":"dialog","name":"Edit"}}"#,
            "/autorun-page?limit=10&within=%7B%22role%22%3A%22dialog%22%2C%22name%22%3A%22Edit%22%7D",
        ),
    ] {
        let calls = std::cell::RefCell::new(vec![]);
        let call = |method: &str, path: &str, _body: &str| {
            calls.borrow_mut().push((method.to_string(), path.to_string()));
            Ok((200, "{}".to_string()))
        };
        let req = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#);
        handle_message(&req, "1.0.0", &call).unwrap();
        let recorded = calls.borrow();
        assert_eq!(recorded.last().unwrap(), &("GET".to_string(), want.to_string()), "{tool} {args}");
    }
}

/// `get_autorun_network_wait` rides with the Auto Run tools: listed where
/// Auto Run is offered, refused as "not available" where it is not, and a
/// plain read of `/autorun-network-wait` that takes no arguments.
#[test]
fn the_network_wait_tool_is_an_auto_run_read_of_its_own_route() {
    const NAME: &str = "get_autorun_network_wait";
    assert!(module_of_tool(NAME) == Some(Module::AutoRun));
    assert!(listed_names(&stub(200, r#"{"disabled":[]}"#)).iter().any(|n| n == NAME));

    let (off, on) = tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["db_access","writing_style"]}"#.into())));
    assert!(!on.contains(&Module::AutoRun));
    assert!(off.iter().any(|n| n == NAME), "not disabled with its module off: {off:?}");
    assert!(v2_lib::mcp::refusal_text(NAME, &on).contains("not available"));

    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, r#"{"waiting":false}"#.to_string()))
    };
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": NAME, "arguments": {} },
    });
    let resp = handle_message(&req.to_string(), "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_ne!(v["result"]["isError"], serde_json::json!(true), "{resp}");
    let (m, p, _) = calls.borrow().last().unwrap().clone();
    assert_eq!((m.as_str(), p.as_str()), ("GET", "/autorun-network-wait"));

    // Read only: the description says nothing can end the wait.
    let list = handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
    assert!(list.contains("nothing you call can"), "{list}");
}

/// The six network tools are Auto Run's, behind their own switch: listed
/// only while the app says `network_tools: true` (a development app with
/// every module on, in this file), refused as "not available" where Auto
/// Run is not offered, and refused naming the Network tools switch where
/// it is.
#[test]
fn the_network_tools_are_auto_run_tools_behind_their_own_switch() {
    use v2_lib::ai_tools::NETWORK_TOOLS;
    assert_eq!(
        NETWORK_TOOLS,
        ["search_autorun_requests", "get_autorun_request", "autorun_capture", "autorun_har", "set_autorun_headers", "autorun_cookies"]
    );
    let listed = listed_names(&stub(200, r#"{"disabled":[]}"#));
    let (_, auto_run_off) = tool_policy_from(Ok((200, r#"{"disabled":[],"modules":["db_access","writing_style"]}"#.into())));
    for tool in NETWORK_TOOLS {
        assert!(module_of_tool(tool) == Some(Module::AutoRun), "{tool}");
        assert!(AUTORUN_TOOLS.contains(tool), "{tool}");
        assert!(listed.iter().any(|n| n == tool), "{tool} listed with the switch on");
        assert!(v2_lib::mcp::refusal_text(tool, &auto_run_off).contains("not available"), "{tool}");
        let said = v2_lib::mcp::refusal_text(tool, &Module::ALL.into_iter().collect());
        assert!(said.contains("switched off") && said.contains("Network tools switch"), "{tool}: {said}");
    }
    for name in [
        "list_autorun_requests",
        "get_autorun_response",
        "get_autorun_response_body",
        "set_autorun_capture",
        "get_autorun_capture",
        "export_autorun_requests",
        "import_autorun_requests",
        "get_autorun_cookies",
        "set_autorun_cookie",
        "clear_autorun_cookies",
    ] {
        assert!(!listed.iter().any(|n| n == name), "{name} is gone");
    }
}

/// With the switch off - the app says so, says nothing, or cannot be asked
/// and the proxy falls back to the saved modules - the six are neither
/// listed nor callable, while the rest of Auto Run is. Only an explicit
/// `true` turns them on.
#[test]
fn the_network_tools_are_off_until_the_app_says_yes() {
    use v2_lib::ai_tools::NETWORK_TOOLS;
    let all: Vec<&str> = crate::common::ALL_MODULES.to_vec();
    for answer in [
        serde_json::json!({ "disabled": [], "modules": all, "network_tools": false }),
        serde_json::json!({ "disabled": [], "modules": all }),
        serde_json::json!({ "disabled": [], "modules": all, "network_tools": "yes" }),
    ] {
        let body = answer.to_string();
        let call = |_m: &str, _p: &str, _b: &str| -> Result<(u16, String), String> { Ok((200, body.clone())) };
        let resp = v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        for tool in NETWORK_TOOLS {
            assert!(!names.contains(tool), "{tool} listed for {answer}");
            let req = serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": tool, "arguments": {} } });
            let resp = v2_lib::mcp::handle_message(&req.to_string(), "1.0.0", &call).unwrap();
            let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(v["result"]["isError"], true, "{tool}: {v}");
            assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("Network tools switch"), "{tool}: {v}");
        }
        assert!(names.contains(&"get_autorun_page") && names.contains(&"get_autorun_network_wait"), "{names:?}");
        assert_eq!(names.len(), 60, "{names:?}");
    }
    // The proxy's own fallback when the app does not answer names no switch.
    let saved = v2_lib::mcp::tools_reply_or_saved(Err("down".into()), Some((Module::ALL.into_iter().collect(), true)));
    let (off, on) = tool_policy_from(saved);
    assert!(on.contains(&Module::AutoRun));
    for tool in NETWORK_TOOLS {
        assert!(off.iter().any(|o| o == tool), "{tool} offered on the saved modules: {off:?}");
    }
    let (off, _) = tool_policy_from(Ok((200, serde_json::json!({ "disabled": [], "modules": all, "network_tools": true }).to_string())));
    for tool in NETWORK_TOOLS {
        assert!(!off.iter().any(|o| o == tool), "{tool} off with the switch on: {off:?}");
    }
}

/// Every route a network tool reaches is one the bridge refuses while the
/// switch is off.
#[test]
fn every_route_a_network_tool_reaches_is_guarded_by_the_switch() {
    use v2_lib::ai_bridge::NETWORK_ROUTES;
    for (tool, args) in [
        ("search_autorun_requests", serde_json::json!({})),
        ("search_autorun_requests", serde_json::json!({ "path": "/api" })),
        ("get_autorun_request", serde_json::json!({ "id": 1 })),
        ("get_autorun_request", serde_json::json!({ "id": 1, "part": "response" })),
        ("get_autorun_request", serde_json::json!({ "id": 1, "part": "body" })),
        ("autorun_capture", serde_json::json!({})),
        ("autorun_capture", serde_json::json!({ "include_types": [] })),
        ("autorun_har", serde_json::json!({ "export": "a" })),
        ("autorun_har", serde_json::json!({ "import": "a" })),
        ("set_autorun_headers", serde_json::json!({ "headers": {} })),
        ("autorun_cookies", serde_json::json!({ "action": "list" })),
        ("autorun_cookies", serde_json::json!({ "action": "set", "name": "a", "value": "b" })),
        ("autorun_cookies", serde_json::json!({ "action": "clear" })),
    ] {
        let (_, path, _) = reached(tool, args.clone());
        assert!(NETWORK_ROUTES.contains(&path.as_str()), "{tool} {args} reached {path}, which the switch does not guard");
    }
}

/// `search_autorun_requests` with no filter is the plain list's route, with
/// any filter the search's; the arguments travel whole either way.
#[test]
fn search_autorun_requests_lists_with_no_filter() {
    for (args, path) in [
        (serde_json::json!({ "offset": 25, "from": "edge" }), "/autorun-requests"),
        (serde_json::json!({}), "/autorun-requests"),
        (serde_json::json!({ "path": null, "limit": 5 }), "/autorun-requests"),
        (serde_json::json!({ "path": "/api", "status": "4xx" }), "/autorun-requests-search"),
        (serde_json::json!({ "response_contains": "Annual" }), "/autorun-requests-search"),
    ] {
        assert_eq!(reached("search_autorun_requests", args.clone()), ("POST".into(), path.into(), args.clone()), "{args}");
    }
}

/// `get_autorun_request` reads the request (the default), the response or
/// its body through each one's own route, `part` taken off; an unknown
/// part, or an `offset` on the response, is refused before the app.
#[test]
fn get_autorun_request_reads_each_part_through_its_route() {
    use v2_lib::mcp::{PART_REFUSED, RESPONSE_HAS_NO_OFFSET};
    for (args, path, body) in [
        (serde_json::json!({ "id": 3 }), "/autorun-request", serde_json::json!({ "id": 3 })),
        (serde_json::json!({ "id": 3, "part": "request", "offset": 20000 }), "/autorun-request", serde_json::json!({ "id": 3, "offset": 20000 })),
        (serde_json::json!({ "id": 3, "part": "response", "from": "flow" }), "/autorun-response", serde_json::json!({ "id": 3, "from": "flow" })),
        (serde_json::json!({ "id": 3, "part": "body", "offset": 20000 }), "/autorun-response-body", serde_json::json!({ "id": 3, "offset": 20000 })),
    ] {
        assert_eq!(reached("get_autorun_request", args.clone()), ("POST".into(), path.into(), body), "{args}");
    }
    refused_before_the_app("get_autorun_request", serde_json::json!({ "id": 3, "part": "headers" }), PART_REFUSED);
    refused_before_the_app("get_autorun_request", serde_json::json!({ "id": 3, "part": 2 }), PART_REFUSED);
    refused_before_the_app("get_autorun_request", serde_json::json!({ "id": 3, "part": "response", "offset": 10 }), RESPONSE_HAS_NO_OFFSET);
}

/// `autorun_capture` reads the filter with no filter field and sets it with
/// any - an empty list included, which keeps every request again.
#[test]
fn autorun_capture_reads_or_sets_the_filter() {
    assert_eq!(reached("autorun_capture", serde_json::json!({})), ("GET".into(), "/autorun-capture".into(), serde_json::Value::Null));
    for args in [
        serde_json::json!({ "include_types": ["XHR"] }),
        serde_json::json!({ "include_types": [] }),
        serde_json::json!({ "path_contains": "/api", "exclude_types": ["Document"] }),
    ] {
        assert_eq!(reached("autorun_capture", args.clone()), ("POST".into(), "/autorun-capture".into(), args.clone()), "{args}");
    }
}

/// `autorun_har` exports or imports, the file's name sent as the route's
/// own `name`; both or neither is refused before the app.
#[test]
fn autorun_har_exports_or_imports() {
    use v2_lib::mcp::HAR_EXPORT_OR_IMPORT;
    assert_eq!(
        reached("autorun_har", serde_json::json!({ "export": "flow" })),
        ("POST".into(), "/autorun-requests-export".into(), serde_json::json!({ "name": "flow" }))
    );
    assert_eq!(
        reached("autorun_har", serde_json::json!({ "import": "flow", "export": null })),
        ("POST".into(), "/autorun-requests-import".into(), serde_json::json!({ "name": "flow" }))
    );
    refused_before_the_app("autorun_har", serde_json::json!({ "export": "a", "import": "b" }), HAR_EXPORT_OR_IMPORT);
    refused_before_the_app("autorun_har", serde_json::json!({}), HAR_EXPORT_OR_IMPORT);
}

/// `set_autorun_headers` forwards its arguments whole; `autorun_cookies`
/// needs an `action` (there is no default: clear signs out), sends each
/// action's own fields to its own route, and refuses a field another action
/// takes - so a clear meant for one cookie never signs the site out.
#[test]
fn the_header_and_cookie_tools_reach_their_routes() {
    use v2_lib::mcp::COOKIE_ACTION_REFUSED;
    let headers = serde_json::json!({ "headers": { "X-Feature": "beta", "Accept-Language": null } });
    assert_eq!(reached("set_autorun_headers", headers.clone()), ("POST".into(), "/autorun-headers".into(), headers));
    for (args, path, body) in [
        (serde_json::json!({ "action": "list", "url": "https://hr.example/" }), "/autorun-cookies", serde_json::json!({ "url": "https://hr.example/" })),
        (serde_json::json!({ "action": "list" }), "/autorun-cookies", serde_json::json!({})),
        (
            serde_json::json!({ "action": "set", "name": "culture", "value": "fr-FR", "same_site": "Lax" }),
            "/autorun-cookie",
            serde_json::json!({ "name": "culture", "value": "fr-FR", "same_site": "Lax" }),
        ),
        (serde_json::json!({ "action": "clear" }), "/autorun-cookies-clear", serde_json::json!({})),
    ] {
        assert_eq!(reached("autorun_cookies", args.clone()), ("POST".into(), path.into(), body), "{args}");
    }
    for args in [serde_json::json!({}), serde_json::json!({ "action": "delete" }), serde_json::json!({ "action": true })] {
        refused_before_the_app("autorun_cookies", args, COOKIE_ACTION_REFUSED);
    }
    refused_before_the_app("autorun_cookies", serde_json::json!({ "action": "clear", "name": "culture" }), "takes nothing else - not name");
    refused_before_the_app("autorun_cookies", serde_json::json!({ "action": "list", "value": "x" }), "not value");
    refused_before_the_app("autorun_cookies", serde_json::json!({ "action": "set", "name": "a", "value": "b", "url": "https://hr.example/" }), "not url");
}

/// The Crystal Reports tools forward to their bridge routes: the arguments
/// travel whole as the body, and the guide is the one GET.
#[test]
fn the_crystal_tools_reach_their_routes() {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".to_string()))
    };
    for (tool, args, method, path) in [
        ("open_report", serde_json::json!({ "path": "C:/r/a.rpt" }), "POST", "/crystal-open"),
        ("read_report", serde_json::json!({ "section": "Details" }), "POST", "/crystal-read"),
        ("edit_report", serde_json::json!({ "ops": [] }), "POST", "/crystal-change"),
        ("compare_reports", serde_json::json!({ "a": "a.rpt", "b": "b.rpt" }), "POST", "/crystal-compare"),
        ("check_report", serde_json::json!({}), "POST", "/crystal-check"),
        ("preview_report", serde_json::json!({}), "POST", "/crystal-preview"),
        (
            "preview_report_live",
            serde_json::json!({ "params": { "performance_cycle_id": "42" } }),
            "POST",
            "/crystal-preview-live",
        ),
        ("get_guide", serde_json::json!({ "topic": "reports" }), "GET", "/crystal-guide"),
    ] {
        calls.borrow_mut().clear();
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": tool, "arguments": args },
        });
        let resp = handle_message(&req.to_string(), "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_ne!(v["result"]["isError"], true, "{tool}: {v}");
        let recorded = calls.borrow();
        let last = recorded.last().unwrap();
        assert_eq!((last.0.as_str(), last.1.as_str()), (method, path), "{tool}");
        if method == "POST" {
            assert_eq!(serde_json::from_str::<serde_json::Value>(&last.2).unwrap(), args, "{tool}");
        } else {
            assert_eq!(last.2, "", "{tool}");
        }
    }
}

/// `open_report` with `create` makes a blank report first (the old
/// `new_report` is gone): true sends the path alone, an object sends its
/// summary fields beside the path, false or null is a plain open, and
/// anything else is refused before anything reaches the app.
#[test]
fn open_report_creates_a_blank_report_with_create() {
    use v2_lib::mcp::CREATE_REFUSED;
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".to_string()))
    };
    let send = |args: serde_json::Value| -> serde_json::Value {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "open_report", "arguments": args },
        });
        serde_json::from_str(&handle_message(&req.to_string(), "1.0.0", &call).unwrap()).unwrap()
    };
    let last = || calls.borrow().iter().rev().find(|c| c.1 != "/tools").cloned().unwrap();
    for (args, path, body) in [
        (
            serde_json::json!({ "path": "C:/r/n.rpt", "create": { "title": "T", "author": "A", "comments": "C" } }),
            "/crystal-new",
            serde_json::json!({ "path": "C:/r/n.rpt", "title": "T", "author": "A", "comments": "C" }),
        ),
        (
            serde_json::json!({ "path": "C:/r/n.rpt", "create": true }),
            "/crystal-new",
            serde_json::json!({ "path": "C:/r/n.rpt" }),
        ),
        (
            serde_json::json!({ "path": "C:/r/a.rpt", "create": false }),
            "/crystal-open",
            serde_json::json!({ "path": "C:/r/a.rpt", "create": false }),
        ),
        (serde_json::json!({ "path": "C:/r/a.rpt" }), "/crystal-open", serde_json::json!({ "path": "C:/r/a.rpt" })),
    ] {
        let v = send(args.clone());
        assert_ne!(v["result"]["isError"], true, "{args}: {v}");
        let (m, p, b) = last();
        assert_eq!((m.as_str(), p.as_str()), ("POST", path), "{args}");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&b).unwrap(), body, "{args}");
    }

    calls.borrow_mut().clear();
    for create in [serde_json::json!("yes"), serde_json::json!(1), serde_json::json!(["T"])] {
        let v = send(serde_json::json!({ "path": "C:/r/n.rpt", "create": create }));
        assert_eq!(v["result"]["isError"], true, "{create}: {v}");
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains(CREATE_REFUSED), "{v}");
    }
    assert!(calls.borrow().iter().all(|c| c.1 == "/tools"), "nothing reached the app: {:?}", calls.borrow());

    let resp = handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "1.0.0", &stub(200, "")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().all(|t| t["name"] != "new_report"), "new_report is gone");
    let open = tools.iter().find(|t| t["name"] == "open_report").unwrap();
    assert!(open["description"].as_str().unwrap().contains("never over an existing file"), "{open}");
    assert_eq!(open["inputSchema"]["required"], serde_json::json!(["path"]));
}

/// Each Crystal Reports tool is described for the assistant: the required
/// arguments are the brief's, the long-form detail lives in the guide, and
/// edit_report says what saving means and where ids come from.
#[test]
fn the_crystal_tool_descriptions_guide_the_assistant() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "1.0.0", &stub(200, "")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let find = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name} is listed"));
    let required = |name: &str| -> Vec<String> {
        find(name)["inputSchema"]["required"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().into()).collect()
    };
    assert_eq!(required("open_report"), ["path"]);
    assert_eq!(required("compare_reports"), ["a", "b"]);
    assert!(required("edit_report").is_empty());
    for name in CRYSTAL_TOOLS {
        let text = find(name)["description"].as_str().unwrap();
        assert!(!text.contains('\u{2014}'), "{name}: no em dashes in text an assistant reads");
    }
    for name in ["open_report", "read_report", "edit_report"] {
        let text = find(name)["description"].as_str().unwrap();
        assert!(text.contains("get_guide (topic reports)"), "{name} points at the guide: {text}");
    }
    let edit = find("edit_report")["description"].as_str().unwrap();
    assert!(edit.contains("saved over the file"), "{edit}");
    assert!(edit.contains("git"), "{edit}");
    assert!(edit.contains("latest"), "{edit}");
    assert_eq!(find("edit_report")["inputSchema"]["properties"]["ops"]["type"], "array");
    assert_eq!(find("edit_report")["inputSchema"]["properties"]["plan"]["type"], "object");
}

/// Every tool whose answer carries the page declares `page` (a boolean),
/// and a call with `"page": false` forwards it to the bridge untouched, so
/// the route can leave the page out.
#[test]
fn page_false_reaches_the_bridge_for_every_tool_that_answers_the_page() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "1.0.0", &stub(200, "")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap().clone();
    let find = |name: &str| tools.iter().find(|t| t["name"] == name).cloned().unwrap_or_else(|| panic!("{name} is listed"));
    for (tool, path, args) in [
        ("start_autorun_discovery", "/autorun-discover-start", serde_json::json!({ "account": "admin", "page": false })),
        ("discover_autorun_actions", "/autorun-discover-action", serde_json::json!({ "action": { "kind": "reload" }, "page": false })),
        ("discover_autorun_actions", "/autorun-discover-actions", serde_json::json!({ "actions": [{ "kind": "reload" }], "page": false })),
        ("replay_autorun_to_step", "/autorun-replay", serde_json::json!({ "case_id": 7, "step": 2, "page": false })),
    ] {
        let page = &find(tool)["inputSchema"]["properties"]["page"];
        assert_eq!(page["type"], "boolean", "{tool} declares page");
        assert!(page["description"].as_str().is_some_and(|d| d.contains("get_autorun_page")), "{tool}: {page}");
        let calls = std::cell::RefCell::new(vec![]);
        let call = |method: &str, path: &str, body: &str| {
            calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
            Ok((200, "{}".to_string()))
        };
        let req = serde_json::json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": tool, "arguments": args } });
        handle_message(&req.to_string(), "1.0.0", &call).unwrap();
        let recorded = calls.borrow();
        let last = recorded.last().unwrap_or_else(|| panic!("{tool} called the bridge"));
        assert_eq!((last.0.as_str(), last.1.as_str()), ("POST", path), "{tool}");
        let body: serde_json::Value = serde_json::from_str(&last.2).unwrap();
        assert_eq!(body["page"], false, "{tool}: {body}");
    }
}

/// `get_autorun_page` declares `nth` beside `within`, and forwards it in
/// the query.
#[test]
fn get_autorun_page_forwards_within_and_its_nth() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "1.0.0", &stub(200, "")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let page = tools.iter().find(|t| t["name"] == "get_autorun_page").unwrap();
    let nth = &page["inputSchema"]["properties"]["nth"];
    assert_eq!(nth["type"], "number", "{page}");
    assert!(nth["description"].as_str().is_some_and(|d| d.contains("from 0")), "{nth}");
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".to_string()))
    };
    let req = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_autorun_page","arguments":{"within":"main","nth":1}}}"#;
    handle_message(req, "1.0.0", &call).unwrap();
    assert_eq!(calls.borrow().last().unwrap().1, "/autorun-page?within=main&nth=1");
}

/// One `tools/call` through the dispatcher: the tool's text and whether it is
/// an error, with every bridge request it made as (method, path, body).
fn call_with_arguments(
    name: &str,
    arguments: serde_json::Value,
    answer: (u16, &str),
) -> (String, bool, Vec<(String, String, String)>) {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        if path == "/tools" {
            return Ok((200, "{}".to_string()));
        }
        Ok((answer.0, answer.1.to_string()))
    };
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    let resp = handle_message(&req.to_string(), "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let text = v["result"]["content"][0]["text"].as_str().unwrap().to_string();
    let is_error = v["result"]["isError"] == true;
    let made = calls.borrow().clone();
    (text, is_error, made)
}

/// The paths of the requests a call made, leaving out the app's `/tools`
/// read that every call starts with.
fn paths_of(made: &[(String, String, String)]) -> Vec<String> {
    made.iter().filter(|(_, p, _)| p != "/tools").map(|(_, p, _)| p.clone()).collect()
}

/// A client whose tool list is older than this app may send a boolean as
/// text; it is read as the boolean. Text that is neither true nor false is
/// still refused by name.
#[test]
fn a_stale_client_sending_release_as_text_is_read_as_a_boolean() {
    let (_, _, made) = call_with_arguments("reset_autorun_browser", serde_json::json!({ "release": "true" }), (200, "ok"));
    assert_eq!(paths_of(&made), vec!["/autorun-release".to_string()]);

    let (_, _, made) = call_with_arguments("reset_autorun_browser", serde_json::json!({ "release": " False " }), (200, "ok"));
    assert_eq!(paths_of(&made), vec!["/autorun-reset".to_string()]);

    let (text, is_error, made) =
        call_with_arguments("reset_autorun_browser", serde_json::json!({ "release": "yes" }), (200, "ok"));
    assert!(is_error);
    assert!(text.contains("`release` is true or false"), "{text}");
    assert!(paths_of(&made).is_empty(), "{made:?}");
}

/// An object or array parameter sent as JSON text is parsed.
#[test]
fn an_object_or_array_param_sent_as_json_text_is_parsed() {
    let (_, _, made) = call_with_arguments(
        "discover_autorun_actions",
        serde_json::json!({ "action": "{\"kind\":\"click\",\"selector\":\"#a\"}" }),
        (200, "ok"),
    );
    assert_eq!(paths_of(&made), vec!["/autorun-discover-action".to_string()]);
    let body: serde_json::Value = serde_json::from_str(&made.last().unwrap().2).unwrap();
    assert!(body["action"].is_object(), "{body}");
    assert_eq!(body["action"]["kind"], "click");

    let (_, _, made) = call_with_arguments(
        "discover_autorun_actions",
        serde_json::json!({ "actions": "[{\"kind\":\"click\",\"selector\":\"#a\"}]" }),
        (200, "ok"),
    );
    assert_eq!(paths_of(&made), vec!["/autorun-discover-actions".to_string()]);
    let body: serde_json::Value = serde_json::from_str(&made.last().unwrap().2).unwrap();
    assert_eq!(body["actions"][0]["kind"], "click", "{body}");
}

/// A parameter whose type takes text is never read as anything else.
#[test]
fn a_param_that_also_takes_text_is_never_coerced() {
    let (_, _, made) = call_with_arguments("list_api_templates", serde_json::json!({ "flow": "false" }), (200, "{}"));
    let paths = paths_of(&made);
    assert_eq!(paths.len(), 1, "{paths:?}");
    assert!(paths[0].contains("flow=false"), "{}", paths[0]);
    assert!(!paths[0].contains("with_flow=false"), "{}", paths[0]);
}

/// A list's items sent as JSON text are parsed to objects.
#[test]
fn array_items_sent_as_json_text_are_parsed() {
    let (_, _, made) = call_with_arguments(
        "save_autorun_script",
        serde_json::json!({
            "scripts": [{ "case_id": 7, "title": "t", "steps": [] }],
            "edits": ["{\"case_id\":7,\"steps\":[1],\"why\":\"w\"}"],
        }),
        (200, "saved"),
    );
    assert_eq!(paths_of(&made), vec!["/autorun-script".to_string()]);
    let body: serde_json::Value = serde_json::from_str(&made.last().unwrap().2).unwrap();
    assert!(body["edits"][0].is_object(), "{body}");
    assert_eq!(body["edits"][0]["case_id"], 7);
}

/// A save with a key the dispatch would drop is refused before the app.
#[test]
fn save_autorun_script_refuses_keys_it_would_drop() {
    let never = |_m: &str, p: &str, _b: &str| -> Result<(u16, String), String> {
        if p == "/tools" {
            return Ok((200, "{}".to_string()));
        }
        panic!("a save with a stray key reached the bridge: {p}");
    };
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "save_autorun_script", "arguments": {
            "scripts": [{ "case_id": 7, "title": "t", "steps": [] }],
            "edit": [{ "case_id": 7, "steps": [1], "why": "w" }],
        } },
    });
    let resp = handle_message(&req.to_string(), "1.0.0", &never).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], true);
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("400"), "{text}");
    assert!(text.contains("not edit"), "{text}");
    assert!(text.contains("\"edits\""), "{text}");
}

/// A repair the route refuses for its missing `edits` says what the call
/// carried, so the report that follows is conclusive.
#[test]
fn a_repair_refused_for_missing_edits_says_what_the_call_carried() {
    let (text, is_error, _) = call_with_arguments(
        "save_autorun_script",
        serde_json::json!({ "scripts": [{ "case_id": 7, "title": "t", "steps": [] }] }),
        (400, "case 7 already has a script, so this save is a repair, and \"edits\" is missing. x"),
    );
    assert!(is_error);
    assert!(text.ends_with("(This call carried: scripts.)"), "{text}");
}

/// `report_case_divergence` is listed with its three kinds, all five
/// arguments required, and forwards to `/autorun-divergence`.
#[test]
fn the_divergence_tool_is_listed_with_its_kinds() {
    const NAME: &str = "report_case_divergence";
    assert!(module_of_tool(NAME) == Some(Module::AutoRun));
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == NAME).expect("listed");
    assert_eq!(tool["inputSchema"]["properties"]["kind"]["enum"], serde_json::json!(["data", "app", "wording"]));
    assert_eq!(
        tool["inputSchema"]["required"],
        serde_json::json!(["case_id", "step", "case_says", "app_shows", "kind"])
    );
    let d = tool["description"].as_str().unwrap();
    assert!(d.contains("instead of writing the check to fit the page"), "{d}");
    assert!(!d.contains('\u{2014}') && !d.contains('\u{2013}'), "{d}");

    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| -> Result<(u16, String), String> {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "Reported as d1.".into()))
    };
    let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"report_case_divergence","arguments":{"case_id":147556,"step":2,"case_says":"exactly 4 levels","app_shows":"6 levels","kind":"data"}}}"#;
    let v: serde_json::Value = serde_json::from_str(&handle_message(req, "1.0.0", &call).unwrap()).unwrap();
    assert_ne!(v["result"]["isError"], serde_json::json!(true), "{v}");
    let recorded = calls.borrow();
    let last = recorded.last().unwrap();
    assert_eq!((last.0.as_str(), last.1.as_str()), ("POST", "/autorun-divergence"));
}

#[test]
fn the_quirk_tool_offers_a_class_of_three() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let quirk = tools.iter().find(|t| t["name"] == "record_quirk").unwrap();
    assert_eq!(
        quirk["inputSchema"]["properties"]["class"]["enum"],
        serde_json::json!(["app quirk", "environment flake", "product bug"])
    );
    let save = tools.iter().find(|t| t["name"] == "save_autorun_script").unwrap();
    assert!(save["inputSchema"]["properties"]["edits"]["description"].as_str().unwrap().contains("quirk_class"));
}

/// A fake bridge for the role tests: `/tools` answers a development app, the quick autorun guide answers
/// a fixed text, a role route answers `APP RULES <key>`, and any other path fails.
fn role_fake(
    seen: &std::cell::RefCell<Vec<String>>,
) -> impl Fn(&str, &str, &str) -> Result<(u16, String), String> + '_ {
    move |_m: &str, path: &str, _b: &str| {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":[]}"#.to_string()));
        }
        seen.borrow_mut().push(path.to_string());
        match path {
            "/autorun-guide?quick=true" => Ok((200, "the quick guide".to_string())),
            role if role.starts_with("/autorun-role?role=") => Ok((200, format!("APP RULES {}", &role["/autorun-role?role=".len()..]))),
            other => panic!("no guide route is called for a role: {other}"),
        }
    }
}

fn guide_call(args: &str, call: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String>) -> serde_json::Value {
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"get_guide","arguments":{args}}}}}"#
    );
    serde_json::from_str(&handle_message(&req, "1.0.0", call).unwrap()).unwrap()
}

#[test]
fn get_guide_answers_a_role_by_name() {
    let seen = std::cell::RefCell::new(vec![]);
    let call = role_fake(&seen);
    let v = guide_call(r#"{"topic":"autorun","role":"healer"}"#, &call);
    assert_ne!(v["result"]["isError"], true, "{v}");
    assert_eq!(v["result"]["content"][0]["text"], "APP RULES healer");
    assert_eq!(*seen.borrow(), vec!["/autorun-role?role=healer".to_string()]);
}

#[test]
fn get_guide_role_asks_the_app_for_the_rules() {
    let seen = std::cell::RefCell::new(vec![]);
    let call = role_fake(&seen);
    let v = guide_call(r#"{"topic":"autorun","role":"writer"}"#, &call);
    assert_eq!(v["result"]["content"][0]["text"], "APP RULES writer", "{v}");
    assert_eq!(*seen.borrow(), vec!["/autorun-role?role=writer".to_string()]);
}

#[test]
fn a_role_with_quick_comes_first() {
    let seen = std::cell::RefCell::new(vec![]);
    let call = role_fake(&seen);
    let v = guide_call(r#"{"topic":"autorun","role":"writer","quick":true}"#, &call);
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(text, "APP RULES writer\n\nthe quick guide", "{v}");
    assert_eq!(
        *seen.borrow(),
        vec!["/autorun-guide?quick=true".to_string(), "/autorun-role?role=writer".to_string()]
    );
}

#[test]
fn an_unknown_role_names_them_all() {
    let seen = std::cell::RefCell::new(vec![]);
    let call = role_fake(&seen);
    let v = guide_call(r#"{"topic":"autorun","role":"tester"}"#, &call);
    assert_eq!(v["result"]["isError"], true, "{v}");
    assert_eq!(
        v["result"]["content"][0]["text"],
        "bridge returned 400: There is no role \"tester\": role is one of \"writer\", \"healer\", \"case-rewriter\", \"auditor\" and \"explorer\"."
    );
}

#[test]
fn an_explore_start_opens_with_the_explorers_rules() {
    use v2_lib::autorun::roles::Role;
    use v2_lib::mcp_session::Session;
    let mut s = Session::new();
    s.tick(1_000);
    let args = serde_json::json!({"account": "a", "explore": true});
    assert_eq!(s.note_for("start_autorun_discovery", &args, &|r| r.text()), Some(Role::Explorer.text()));
    assert_eq!(
        s.note_for("start_autorun_discovery", &args, &|r| r.text()),
        Some(v2_lib::autorun::roles::pointer(Role::Explorer))
    );
    // The writer's rules are a separate delivery: a plain start still brings them.
    assert_eq!(
        s.note_for("start_autorun_discovery", &serde_json::json!({"account": "a"}), &|r| r.text()),
        Some(Role::Writer.text())
    );
}

#[test]
fn role_is_for_the_autorun_topic_only() {
    use v2_lib::mcp::GUIDE_PARTS_AUTORUN_ONLY;
    let seen = std::cell::RefCell::new(vec![]);
    let call = role_fake(&seen);
    let v = guide_call(r#"{"topic":"writing","role":"healer"}"#, &call);
    assert_eq!(v["result"]["isError"], true, "{v}");
    assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains(GUIDE_PARTS_AUTORUN_ONLY), "{v}");
    assert!(GUIDE_PARTS_AUTORUN_ONLY.contains("`role`"));
    assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
}

#[test]
fn get_guide_offers_role_with_the_autorun_topic() {
    let guide_of = |tools: &'static str| -> serde_json::Value {
        let call = move |_m: &str, _p: &str, _b: &str| -> Result<(u16, String), String> { Ok((200, tools.to_string())) };
        let v: serde_json::Value = serde_json::from_str(
            &v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap(),
        )
        .unwrap();
        v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "get_guide").unwrap().clone()
    };
    let on = guide_of(r#"{"disabled":[],"modules":["writing_style","auto_run","api_templates","crystal_reports","db_access","playwright"]}"#);
    assert_eq!(
        on["inputSchema"]["properties"]["role"]["enum"],
        serde_json::json!(["writer", "healer", "case-rewriter", "auditor", "explorer"]),
        "{on}"
    );
    assert!(on["description"].as_str().unwrap().contains("`role` gives one role's rules."), "{on}");
    let off = guide_of(r#"{"disabled":[],"modules":["writing_style"]}"#);
    assert!(off["inputSchema"]["properties"].get("role").is_none(), "{off}");
}

/// What the `--mcp` proxy lists when the app is closed and `dir` is the data
/// folder: the saved modules less what the lock holds back, read the way the
/// app reads them.
fn tools_list_from_saved_dir(dir: &std::path::Path) -> serde_json::Value {
    let saved = v2_lib::modules::effective_in(dir);
    let call = move |_m: &str, _p: &str, _b: &str| v2_lib::mcp::tools_reply_or_saved(Err("connection refused".into()), saved.clone());
    let resp = v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
    serde_json::from_str::<serde_json::Value>(&resp).unwrap()["result"].clone()
}

/// The app is closed, the person switched the held-back module on before it
/// was held back, and this machine has not unlocked it: the proxy lists none
/// of its tools and its guide topic, and no description names it. Unlocked,
/// they are all there.
#[test]
fn a_closed_app_offers_the_held_back_module_only_when_the_machine_unlocked_it() {
    let dir = tempfile::tempdir().unwrap();
    v2_lib::modules::save(dir.path(), &Module::ALL.into_iter().collect()).unwrap();
    v2_lib::extras::save(dir.path(), false).unwrap();

    let locked = tools_list_from_saved_dir(dir.path());
    let names: Vec<&str> = locked["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for t in CRYSTAL_TOOLS {
        assert!(!names.contains(t), "{t} listed on a locked machine: {names:?}");
    }
    assert!(names.contains(&"db_query") && names.contains(&"save_autorun_script"), "the other modules stay: {names:?}");
    let said = locked.to_string().to_lowercase();
    assert!(!said.contains("crystal"), "a locked list names the module");
    let guide = locked["tools"].as_array().unwrap().iter().find(|t| t["name"] == "get_guide").unwrap().to_string();
    assert!(!guide.contains("reports"), "the guide offers the hidden topic: {guide}");

    v2_lib::extras::save(dir.path(), true).unwrap();
    let open = tools_list_from_saved_dir(dir.path());
    let names: Vec<&str> = open["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for t in CRYSTAL_TOOLS {
        assert!(names.contains(t), "{t} missing on an unlocked machine");
    }
    let guide = open["tools"].as_array().unwrap().iter().find(|t| t["name"] == "get_guide").unwrap().to_string();
    assert!(guide.contains("reports"), "{guide}");
}

/// A closed app words its refusals by the same unlock state: locked, the
/// module is never named; unlocked and switched off, it is told where to
/// switch it on.
#[test]
fn a_closed_apps_refusals_follow_the_unlock_state_of_the_data_folder() {
    let dir = tempfile::tempdir().unwrap();
    v2_lib::modules::save(dir.path(), &[Module::DbAccess].into_iter().collect()).unwrap();
    let ask = |what: serde_json::Value| -> String {
        let saved = v2_lib::modules::effective_in(dir.path());
        let call = move |_m: &str, _p: &str, _b: &str| v2_lib::mcp::tools_reply_or_saved(Err("down".into()), saved.clone());
        let req = serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": what });
        let v: serde_json::Value = serde_json::from_str(&v2_lib::mcp::handle_message(&req.to_string(), "1.0.0", &call).unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true, "{v}");
        v["result"]["content"][0]["text"].as_str().unwrap().to_string()
    };
    let tool = serde_json::json!({ "name": "open_report", "arguments": {} });
    let guide = serde_json::json!({ "name": "get_guide", "arguments": { "topic": "reports" } });

    v2_lib::extras::save(dir.path(), false).unwrap();
    let said = ask(tool.clone());
    assert_eq!(said, format!("The `open_report` tool is not available: {}.", v2_lib::modules::NOT_AVAILABLE.trim_end_matches('.')));
    assert!(!said.contains("Crystal"), "{said}");
    let said = ask(guide.clone());
    assert_eq!(said, format!("The `reports` guide is not available: {}.", v2_lib::modules::NOT_AVAILABLE.trim_end_matches('.')));

    v2_lib::extras::save(dir.path(), true).unwrap();
    assert_eq!(ask(tool), "The `open_report` tool is not available: Crystal Reports is switched off in Modules.");
    assert_eq!(ask(guide), "The `reports` guide is not available: Crystal Reports is switched off in Modules.");
}

/// `db_query`'s description is sent to every assistant whenever Database
/// Access is on, so while the module is held back it never names it.
#[test]
fn db_querys_description_names_the_held_back_module_only_when_unlocked() {
    let desc = |tools_answer: &str| -> String {
        let a = tools_answer.to_string();
        let call = move |_m: &str, _p: &str, _b: &str| -> Result<(u16, String), String> { Ok((200, a.clone())) };
        let resp = v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "db_query").unwrap()["description"].as_str().unwrap().to_string()
    };
    let locked = desc(r#"{"disabled":[],"modules":["db_access"]}"#);
    assert!(!locked.to_lowercase().contains("crystal"), "{locked}");
    assert!(locked.contains("CREATE, ALTER and CREATE OR ALTER are refused too."), "{locked}");
    let locked_all = desc(r#"{"disabled":[],"modules":["db_access","crystal_reports"],"unlocked":false}"#);
    assert_eq!(locked, locked_all);

    let open = desc(r#"{"disabled":[],"modules":["db_access","crystal_reports"],"unlocked":true}"#);
    assert!(open.contains("except on the Dev - dev login with Crystal Reports and Database Access on and writes switched on"), "{open}");
}

/// The four explore tools are listed, each reaches its own route, and the
/// arguments object travels whole as the body.
#[test]
fn the_explore_tools_are_listed_and_reach_their_routes() {
    let cases = [
        ("send_api_request", "/api-template-send"),
        ("record_explore_change", "/autorun-explore-change"),
        ("get_explore_changes", "/autorun-explore-changes"),
        ("undo_explore_creates", "/api-template-explore-undo"),
    ];
    let names = listed_names(&stub(200, r#"{"disabled":[]}"#));
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "{}".to_string()))
    };
    for (name, path) in cases {
        assert!(names.iter().any(|n| n == name), "{name} missing from {names:?}");
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": {} },
        });
        let resp = handle_message(&req.to_string(), "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_ne!(v["result"]["isError"], serde_json::json!(true), "{name}: {resp}");
        let (m, p, b) = calls.borrow().last().unwrap().clone();
        assert_eq!((m.as_str(), p.as_str()), ("POST", path), "{name}");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&b).unwrap(), serde_json::json!({}), "{name}");
    }

    // The schemas and descriptions say what the brief says, in plain words.
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let tool = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap().clone();
    let send = tool("send_api_request");
    assert_eq!(send["inputSchema"]["required"], serde_json::json!(["step"]));
    assert_eq!(send["inputSchema"]["properties"]["effect"]["enum"], serde_json::json!(["read", "create", "edit", "delete"]));
    assert_eq!(send["inputSchema"]["properties"]["person_said_yes"]["type"], "boolean");
    let undo = tool("undo_explore_creates");
    assert_eq!(undo["inputSchema"]["required"], serde_json::json!(["session", "ids"]));
    assert_eq!(undo["inputSchema"]["properties"]["ids"]["type"], "array");
    // The undo needs the person's yes, like a delete call.
    assert_eq!(undo["inputSchema"]["properties"]["person_said_yes"]["type"], "boolean");
    let undo_says = undo["description"].as_str().unwrap();
    assert!(undo_says.contains("it needs the person's yes") && undo_says.contains("nothing is deleted"), "{undo_says}");
    // `created` says what the code requires of it (I2).
    let created = send["inputSchema"]["properties"]["created"].to_string();
    assert!(created.contains("the NAME of one of the step's `capture`s"), "{created}");
    assert!(created.contains("cannot be recorded here"), "{created}");
    assert_eq!(send["inputSchema"]["properties"]["created"]["required"], serde_json::json!(["kind", "id", "name"]));
    assert!(!send["description"].as_str().unwrap().contains("when the answer carries no id"));
    let record = tool("record_explore_change");
    assert_eq!(record["inputSchema"]["properties"]["change"]["enum"], serde_json::json!(["created", "changed", "deleted"]));
    assert_eq!(record["inputSchema"]["properties"]["undone"]["type"], "array");
    for name in ["send_api_request", "record_explore_change", "get_explore_changes", "undo_explore_creates"] {
        let text = tool(name).to_string();
        for bad in ['\u{2014}', '\u{2013}'] {
            assert!(!text.contains(bad), "{name} has a dash");
        }
        assert!(!text.contains("http"), "{name} names a URL");
    }
}

/// `start_autorun_discovery` takes `explore`, a boolean, and says what an
/// explore start opens and ends with.
#[test]
fn start_autorun_discovery_takes_explore() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let start = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "start_autorun_discovery").unwrap();
    assert_eq!(start["inputSchema"]["properties"]["explore"]["type"], "boolean");
    assert!(start["description"].as_str().unwrap().contains("explorer's rules"), "{start}");
}

/// The two ruling tools are listed, say who may send them, and reach their own routes with the arguments whole.
#[test]
fn the_ruling_tools_are_listed_and_reach_their_routes() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, "{}")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let tool = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name} not listed")).clone();

    let rule = tool("rule_case_divergence");
    let d = rule["description"].as_str().unwrap();
    assert!(d.contains("Only after the person has ruled in this chat"), "{d}");
    assert!(d.contains("relayed by another assistant"), "{d}");
    assert_eq!(rule["inputSchema"]["properties"]["ruling"]["enum"], serde_json::json!(["case_right", "data_fixed", "rewrite_case"]));
    assert_eq!(rule["inputSchema"]["required"], serde_json::json!(["ruling", "said"]));
    assert_eq!(rule["inputSchema"]["properties"]["divergences"]["items"]["required"], serde_json::json!(["case_id", "divergence_id"]));

    let clear = tool("clear_autorun_suspected_defect");
    assert_eq!(clear["inputSchema"]["required"], serde_json::json!(["case_id", "said"]));
    assert!(clear["description"].as_str().unwrap().starts_with("Only after the person said so in this chat"));

    for t in [&rule, &clear] {
        let text = t.to_string();
        assert!(!text.contains('\u{2014}') && !text.contains('\u{2013}'), "{text}");
        assert!(!text.contains("http"), "{text}");
    }

    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, "ok".to_string()))
    };
    for (name, path, args) in [
        (
            "rule_case_divergence",
            "/autorun-divergence-ruling",
            serde_json::json!({ "case_id": 1, "divergence_id": "d1", "ruling": "case_right", "said": "yes" }),
        ),
        ("clear_autorun_suspected_defect", "/autorun-defect-clear", serde_json::json!({ "case_id": 1, "said": "yes" })),
    ] {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args },
        });
        let resp: serde_json::Value = serde_json::from_str(&handle_message(&req.to_string(), "1.0.0", &call).unwrap()).unwrap();
        assert_ne!(resp["result"]["isError"], serde_json::json!(true), "{name}: {resp}");
        let (m, p, b) = calls.borrow().last().unwrap().clone();
        assert_eq!((m.as_str(), p.as_str()), ("POST", path), "{name}");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&b).unwrap(), args, "{name}");
    }
}

fn pw_fake(seen: &std::cell::RefCell<Vec<String>>) -> impl Fn(&str, &str, &str) -> Result<(u16, String), String> + '_ {
    move |m: &str, path: &str, _b: &str| {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":[]}"#.to_string()));
        }
        assert_eq!(m, "GET", "{path}");
        seen.borrow_mut().push(path.to_string());
        Ok((200, format!("APP {path}")))
    }
}

#[test]
fn get_guide_reaches_the_playwright_topics() {
    let seen = std::cell::RefCell::new(vec![]);
    let call = pw_fake(&seen);
    for (args, want) in [
        (r#"{"topic":"playwright_skills","skill":"edit-spec"}"#, "/autorun-pw-skills?skill=edit-spec"),
        (r#"{"topic":"playwright_skills"}"#, "/autorun-pw-skills"),
        (r#"{"topic":"playwright_skills","skill":""}"#, "/autorun-pw-skills"),
        (r#"{"topic":"playwright_skills","skill":"../x y"}"#, "/autorun-pw-skills?skill=..%2Fx%20y"),
        (r#"{"topic":"playwright_repo","file":"gen-test.md"}"#, "/autorun-pw-repo?file=gen-test.md"),
        (r#"{"topic":"playwright_repo"}"#, "/autorun-pw-repo"),
        (r#"{"topic":"playwright_repo","page":2}"#, "/autorun-pw-repo?page=2"),
        (r#"{"topic":"playwright_repo","file":"CLAUDE.md","page":" 2 "}"#, "/autorun-pw-repo?file=CLAUDE.md&page=2"),
        (r#"{"topic":"playwright_repo","page":""}"#, "/autorun-pw-repo"),
    ] {
        seen.borrow_mut().clear();
        let v = guide_call(args, &call);
        assert_ne!(v["result"]["isError"], true, "{args}: {v}");
        assert_eq!(v["result"]["content"][0]["text"], format!("APP {want}"), "{args}");
        assert_eq!(*seen.borrow(), vec![want.to_string()], "{args}");
    }
}

#[test]
fn get_guide_describes_the_playwright_topics() {
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &stub(200, r#"{"disabled":[]}"#)).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let guide = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "get_guide").unwrap().clone();
    let props = &guide["inputSchema"]["properties"];
    assert_eq!(props["skill"]["type"], "string", "{guide}");
    assert_eq!(props["file"]["enum"], serde_json::json!(v2_lib::autorun::pw_rules::REPO_FILES), "{guide}");
    assert_eq!(props["page"]["type"], "integer", "{guide}");
    assert!(props["page"]["description"].as_str().unwrap().contains("playwright_repo only"), "{guide}");
    let d = guide["description"].as_str().unwrap();
    assert!(d.contains("\"playwright_skills\": the team's Playwright repo's skills, read fresh from the repo set on the Playwright tab"), "{d}");
    assert!(d.contains("\"playwright_repo\": the team's Playwright repo's own files, read fresh"), "{d}");
}

#[test]
fn skill_and_file_are_for_their_topics_only() {
    use v2_lib::mcp::GUIDE_SKILL_FILE_ONLY;
    let seen = std::cell::RefCell::new(vec![]);
    let call = pw_fake(&seen);
    for args in [
        r#"{"topic":"autorun","skill":"x"}"#,
        r#"{"topic":"playwright_skills","file":"CLAUDE.md"}"#,
        r#"{"topic":"playwright_repo","skill":"edit-spec"}"#,
        r#"{"topic":"writing","file":"gen-test.md"}"#,
        r#"{"topic":"playwright_skills","page":2}"#,
        r#"{"topic":"autorun","page":1}"#,
    ] {
        let v = guide_call(args, &call);
        assert_eq!(v["result"]["isError"], true, "{args}: {v}");
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains(GUIDE_SKILL_FILE_ONLY), "{args}: {v}");
    }
    assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
    // The autorun guide's own parts still belong to autorun only.
    let v = guide_call(r#"{"topic":"playwright_skills","section":"x"}"#, &call);
    assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains(v2_lib::mcp::GUIDE_PARTS_AUTORUN_ONLY), "{v}");
    assert!(seen.borrow().is_empty());
}

#[test]
fn the_playwright_topics_go_with_auto_run() {
    use v2_lib::mcp::{guide_refusal, guide_topic_module};
    for topic in ["playwright_skills", "playwright_repo"] {
        assert_eq!(guide_topic_module(topic), Ok(Some(Module::AutoRun)), "{topic}");
        let without_auto_run: BTreeSet<Module> = Module::ALL.into_iter().filter(|m| *m != Module::AutoRun).collect();
        let said = guide_refusal(Some(topic), &[], &without_auto_run).unwrap_or_default();
        assert!(said.contains(&format!("The `{topic}` guide is not available")), "{topic}: {said}");
        let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
        assert_eq!(guide_refusal(Some(topic), &[], &all), None, "{topic}");
    }
}

#[test]
fn take_agent_arg_splits_the_name_out() {
    use v2_lib::mcp::{take_agent_arg, AGENT_SHAPE};
    let (rest, name) = take_agent_arg(&serde_json::json!({ "case_id": 1, "agent": "writer-1" })).unwrap();
    assert_eq!(rest, serde_json::json!({ "case_id": 1 }));
    assert_eq!(name.as_deref(), Some("writer-1"));
    // Trimmed, as the registry keeps it.
    let (_, name) = take_agent_arg(&serde_json::json!({ "agent": "  writer 2 " })).unwrap();
    assert_eq!(name.as_deref(), Some("writer 2"));

    // No agent: the arguments come back as they were, with no name.
    let (rest, name) = take_agent_arg(&serde_json::json!({ "case_id": 1 })).unwrap();
    assert_eq!((rest, name), (serde_json::json!({ "case_id": 1 }), None));
    // Arguments that are not an object are left alone.
    let (rest, name) = take_agent_arg(&serde_json::Value::Null).unwrap();
    assert_eq!((rest, name), (serde_json::Value::Null, None));

    // Null and blank are no name, and the key is gone either way.
    for blank in [serde_json::Value::Null, serde_json::json!("  "), serde_json::json!("")] {
        let (rest, name) = take_agent_arg(&serde_json::json!({ "case_id": 1, "agent": blank })).unwrap();
        assert_eq!((rest, name), (serde_json::json!({ "case_id": 1 }), None), "{blank}");
    }

    // Not a string, or a name the registry refuses.
    for bad in [
        serde_json::json!(7),
        serde_json::json!(true),
        serde_json::json!(["a"]),
        serde_json::json!({ "n": "a" }),
        serde_json::json!("a/b"),
        serde_json::json!("a\"b"),
        serde_json::json!("a".repeat(33)),
        serde_json::json!("caf\u{e9}"),
    ] {
        assert_eq!(take_agent_arg(&serde_json::json!({ "agent": bad })), Err(AGENT_SHAPE.to_string()), "{bad}");
    }
}

#[test]
fn a_malformed_agent_is_refused_before_the_app_is_asked() {
    for bad in [serde_json::json!("a/b"), serde_json::json!(5), serde_json::json!("x".repeat(40))] {
        let call = |_m: &str, _p: &str, _b: &str| -> Result<(u16, String), String> {
            panic!("a malformed agent reached the app")
        };
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "get_autorun_page", "arguments": { "agent": bad } },
        });
        let v: serde_json::Value = serde_json::from_str(&handle_message(&req.to_string(), "1.0.0", &call).unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true, "{bad}: {v}");
        assert_eq!(v["result"]["content"][0]["text"], v2_lib::mcp::AGENT_SHAPE, "{bad}: {v}");
        assert_eq!(v2_lib::mcp::calling_agent(), None);
    }
}

#[test]
fn the_agent_name_is_held_for_the_call_only() {
    let seen = std::cell::RefCell::new(Vec::<(Option<String>, String, String)>::new());
    let call = |_m: &str, path: &str, body: &str| -> Result<(u16, String), String> {
        seen.borrow_mut().push((v2_lib::mcp::calling_agent(), path.to_string(), body.to_string()));
        Ok((200, "{}".into()))
    };
    assert_eq!(v2_lib::mcp::calling_agent(), None);
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 5, "method": "tools/call",
        "params": { "name": "get_autorun_page", "arguments": { "agent": "writer-1" } },
    });
    let v: serde_json::Value = serde_json::from_str(&handle_message(&req.to_string(), "1.0.0", &call).unwrap()).unwrap();
    assert_ne!(v["result"]["isError"], true, "{v}");
    let seen = seen.into_inner();
    let page: Vec<_> = seen.iter().filter(|(_, path, _)| path.starts_with("/autorun-page")).collect();
    assert_eq!(page.len(), 1, "{seen:?}");
    assert_eq!(page[0].0.as_deref(), Some("writer-1"), "{seen:?}");
    // The route sees the call as if the argument had never been sent.
    assert_eq!(page[0].1, "/autorun-page", "{seen:?}");
    assert_eq!(page[0].2, "", "{seen:?}");
    // Nothing the assistant named outlives its call.
    assert_eq!(v2_lib::mcp::calling_agent(), None);

    // A tool that forwards its whole arguments object as the body gets it
    // without `agent`, and with everything else.
    let (_, reached) = call_recorded("get_explore_changes", serde_json::json!({ "marker": 7, "agent": "writer-1" }));
    assert_eq!(reached.len(), 1, "{reached:?}");
    let (_, path, body) = &reached[0];
    assert_eq!(path, "/autorun-explore-changes");
    let sent: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(sent, serde_json::json!({ "marker": 7 }), "{body}");
}

/// How long a test listener waits for anything: a request that never comes
/// fails its test; it never hangs the suite.
const LISTEN_FOR: std::time::Duration = std::time::Duration::from_secs(10);

/// The next connection on `listener`, or None once `LISTEN_FOR` has passed.
fn accept_within(listener: &std::net::TcpListener) -> Option<std::net::TcpStream> {
    listener.set_nonblocking(true).unwrap();
    let until = std::time::Instant::now() + LISTEN_FOR;
    while std::time::Instant::now() < until {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return Some(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(e) => panic!("accept failed: {e}"),
        }
    }
    None
}

/// Reads one request off `stream` up to the end of its head, answers `{}`,
/// and returns the raw bytes (what had arrived, if the head never finished).
fn serve_one(stream: &mut std::net::TcpStream) -> Vec<u8> {
    use std::io::{Read, Write};
    stream.set_read_timeout(Some(LISTEN_FOR)).unwrap();
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(n) if n > 0 => raw.extend_from_slice(&chunk[..n]),
            _ => break,
        }
    }
    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    raw
}

fn agent_headers_of(raw: &[u8]) -> v2_lib::autorun::agents::AgentHeaders {
    match v2_lib::ai_bridge::parse_http(raw) {
        v2_lib::ai_bridge::Parsed::Complete { agent, .. } => agent,
        _ => panic!("not a whole request: {}", String::from_utf8_lossy(raw)),
    }
}

#[test]
fn send_to_bridge_carries_the_agent_headers() {
    let _agents = crate::serial::agents();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream = accept_within(&listener).expect("no request reached the listener");
        serve_one(&mut stream)
    });
    let (status, _) = v2_lib::mcp::send_to_bridge(port, "tok", "GET", "/guide", "").unwrap();
    assert_eq!(status, 200);
    let seen = agent_headers_of(&server.join().unwrap());
    assert_eq!(seen.id.as_deref(), Some(v2_lib::mcp::proxy_agent_id()));
    assert_eq!(seen.pid, Some(std::process::id()));
    assert_eq!(seen.client, v2_lib::mcp::client_name());
    // No subagent is being called for: no name.
    assert_eq!(seen.name, None);
    // The id is one the app accepts as an agent's.
    assert!(v2_lib::autorun::agents::valid_id(v2_lib::mcp::proxy_agent_id()));
    assert_eq!(v2_lib::mcp::proxy_agent_id(), v2_lib::mcp::proxy_agent_id());
}

#[test]
fn a_tool_call_for_a_subagent_names_it_on_every_request() {
    let _agents = crate::serial::agents();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The `/tools` read first, then the route the tool is for.
    let server = std::thread::spawn(move || {
        let mut all = Vec::new();
        // A refusal means no `/autorun-page` ever comes: the wait ends and
        // the assertions below fail, rather than the suite hanging.
        while let Some(mut stream) = accept_within(&listener) {
            let raw = serve_one(&mut stream);
            let last = String::from_utf8_lossy(&raw).starts_with("GET /autorun-page");
            all.push(raw);
            if last {
                break;
            }
        }
        all
    });
    let send = |m: &str, p: &str, b: &str| v2_lib::mcp::send_to_bridge(port, "tok", m, p, b);
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 6, "method": "tools/call",
        "params": { "name": "get_autorun_page", "arguments": { "agent": "writer-1" } },
    });
    let v: serde_json::Value = serde_json::from_str(&handle_message(&req.to_string(), "1.0.0", &send).unwrap()).unwrap();
    assert_ne!(v["result"]["isError"], true, "{v}");
    let requests = server.join().unwrap();
    assert!(requests.len() >= 2, "the policy read and the route: {}", requests.len());
    assert!(
        String::from_utf8_lossy(requests.last().unwrap()).starts_with("GET /autorun-page"),
        "the route was never asked"
    );
    for raw in &requests {
        let seen = agent_headers_of(raw);
        assert_eq!(seen.id.as_deref(), Some(v2_lib::mcp::proxy_agent_id()));
        assert_eq!(seen.name.as_deref(), Some("writer-1"), "{}", String::from_utf8_lossy(raw));
    }
    assert_eq!(v2_lib::mcp::calling_agent(), None);
}

/// A closing proxy is not held by an app that does not answer: the goodbye
/// gives up after about two seconds, far short of the 300 s it guards against.
#[test]
fn the_goodbye_gives_up_on_an_app_that_never_answers() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // Accepts, reads nothing, answers nothing, until the test is done.
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let held = accept_within(&listener);
        let _ = wait.recv_timeout(LISTEN_FOR);
        drop(held);
    });
    let started = std::time::Instant::now();
    v2_lib::mcp::send_bye_to(port, "tok");
    let took = started.elapsed();
    let _ = done.send(());
    holder.join().unwrap();
    assert!(took < std::time::Duration::from_secs(6), "the goodbye held the proxy for {took:?}");
}

#[test]
fn initialize_keeps_the_client_name() {
    let _agents = crate::serial::agents();
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","clientInfo":{"name":"claude-code","version":"2.0"}}}"#;
    handle_message(req, "1.10.3", &stub(200, "")).unwrap();
    assert_eq!(v2_lib::mcp::client_name().as_deref(), Some("claude-code"));
    // Only what a name may hold is kept.
    v2_lib::mcp::set_client_name("Claude <b>Desktop</b>/1");
    assert_eq!(v2_lib::mcp::client_name().as_deref(), Some("Claude bDesktopb1"));
    // An initialize that names no client leaves the last one.
    let bare = r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#;
    handle_message(bare, "1.10.3", &stub(200, "")).unwrap();
    assert_eq!(v2_lib::mcp::client_name().as_deref(), Some("Claude bDesktopb1"));
    v2_lib::mcp::set_client_name("   ");
    assert_eq!(v2_lib::mcp::client_name(), None);
    v2_lib::mcp::set_client_name("claude-code");
}

// ---- The proxy's loop: tool calls answered concurrently, one whole line per response ----

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use v2_lib::mcp::{serve_lines, SharedCall};
use v2_lib::mcp_session::SessionBook;

/// The longest any wait in these tests lasts: a failure is a failed assertion, never a hang.
const SERVE_DEADLINE: Duration = Duration::from_secs(30);

/// A writer that gives up the thread between bytes, so a response written in pieces under separate
/// locks of the output would be mixed with another's.
struct YieldingWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for YieldingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for b in buf {
            self.0.lock().unwrap().push(*b);
            std::thread::yield_now();
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn all_modules_policy() -> String {
    let names: Vec<&str> = Module::ALL.iter().map(|m| m.key()).collect();
    serde_json::json!({ "disabled": [], "modules": names }).to_string()
}

fn tool_call_line(id: u64, tool: &str, arguments: serde_json::Value) -> String {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": arguments } }).to_string()
}

/// The complete lines written so far (those ending in a newline): a line still being written is not one yet.
fn response_lines(bytes: &Arc<Mutex<Vec<u8>>>) -> Vec<String> {
    let text = String::from_utf8_lossy(&bytes.lock().unwrap()).into_owned();
    text.split_inclusive('\n').filter(|l| l.ends_with('\n')).map(|l| l.trim_end_matches(['\r', '\n']).to_string()).collect()
}

/// `serve_lines` over `lines` on a thread of its own; the output it writes, and the channel that says it returned.
fn start_serving(lines: &[String], call: SharedCall) -> (Arc<Mutex<Vec<u8>>>, std::sync::mpsc::Receiver<()>) {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let output = Arc::new(Mutex::new(YieldingWriter(bytes.clone())));
    let input = std::io::Cursor::new(lines.join("\n").into_bytes());
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        serve_lines(input, output, "1.0.0".to_string(), call, Arc::new(SessionBook::new()));
        let _ = done_tx.send(());
    });
    (bytes, done_rx)
}

fn ids_of(lines: &[String]) -> Vec<u64> {
    lines
        .iter()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap_or_else(|e| panic!("not a whole JSON line ({e}): {l}"))["id"].as_u64().unwrap())
        .collect()
}

#[test]
fn a_slow_call_does_not_hold_up_a_quick_one() {
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let call: SharedCall = Arc::new(move |_m: &str, path: &str, _b: &str| {
        if path == "/tools" {
            return Ok((200, all_modules_policy()));
        }
        if path == "/autorun-replay" {
            // Held until the test lets go; gives up on its own so a broken test cannot hang the suite.
            let _ = release_rx.lock().unwrap().recv_timeout(SERVE_DEADLINE);
            return Ok((200, "{\"replayed\":true}".into()));
        }
        Ok((200, "{}".into()))
    });
    let lines = [
        tool_call_line(1, "replay_autorun_to_step", serde_json::json!({ "case_id": 7, "step": 3 })),
        tool_call_line(2, "get_autorun_page", serde_json::json!({})),
    ];
    let (bytes, done) = start_serving(&lines, call);
    // The quick call answers while the replay is still waiting.
    let started = Instant::now();
    while response_lines(&bytes).is_empty() {
        assert!(started.elapsed() < SERVE_DEADLINE, "the quick call never answered while the replay waited");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(ids_of(&response_lines(&bytes)), vec![2]);
    assert!(done.try_recv().is_err(), "serve_lines returned before the replay answered");
    release_tx.send(()).unwrap();
    done.recv_timeout(SERVE_DEADLINE).expect("serve_lines returns once every call has answered");
    assert_eq!(ids_of(&response_lines(&bytes)), vec![2, 1]);
}

#[test]
fn responses_are_whole_lines() {
    let call: SharedCall = Arc::new(|_m: &str, path: &str, _b: &str| {
        if path == "/tools" {
            return Ok((200, all_modules_policy()));
        }
        std::thread::sleep(Duration::from_millis(3));
        Ok((200, "{\"a\":\"some text to make the line longer\"}".into()))
    });
    let lines: Vec<String> = (1..=20).map(|id| tool_call_line(id, "get_autorun_page", serde_json::json!({}))).collect();
    let (bytes, done) = start_serving(&lines, call);
    done.recv_timeout(SERVE_DEADLINE).expect("serve_lines returns");
    let out = response_lines(&bytes);
    assert_eq!(out.len(), 20);
    let mut ids = ids_of(&out);
    ids.sort_unstable();
    assert_eq!(ids, (1..=20).collect::<Vec<u64>>());
}

#[test]
fn initialize_and_list_are_answered_in_order() {
    let call: SharedCall = Arc::new(|_m: &str, path: &str, _b: &str| {
        if path == "/tools" {
            return Ok((200, all_modules_policy()));
        }
        Ok((200, "{}".into()))
    });
    let lines = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_string(),
    ];
    let (bytes, done) = start_serving(&lines, call);
    done.recv_timeout(SERVE_DEADLINE).expect("serve_lines returns");
    assert_eq!(ids_of(&response_lines(&bytes)), vec![1, 2]);
}

#[test]
fn each_concurrent_call_names_its_own_agent() {
    let seen: Arc<Mutex<Vec<(String, Option<String>)>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let call: SharedCall = Arc::new(move |_m: &str, path: &str, _b: &str| {
        if path == "/tools" {
            return Ok((200, all_modules_policy()));
        }
        // Long enough for the other calls to be in the middle of theirs.
        std::thread::sleep(Duration::from_millis(20));
        recorded.lock().unwrap().push((path.to_string(), v2_lib::mcp::calling_agent()));
        Ok((200, "{}".into()))
    });
    let lines: Vec<String> = (1..=8u64)
        .map(|id| {
            let mut arguments = serde_json::json!({ "limit": id });
            match id % 3 {
                0 => arguments["agent"] = serde_json::json!("alpha"),
                1 => arguments["agent"] = serde_json::json!("beta"),
                _ => {}
            }
            tool_call_line(id, "get_autorun_page", arguments)
        })
        .collect();
    let (bytes, done) = start_serving(&lines, call);
    done.recv_timeout(SERVE_DEADLINE).expect("serve_lines returns");
    assert_eq!(response_lines(&bytes).len(), 8);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 8);
    for id in 1..=8u64 {
        let want = match id % 3 {
            0 => Some("alpha".to_string()),
            1 => Some("beta".to_string()),
            _ => None,
        };
        let (_, got) = seen.iter().find(|(p, _)| p.contains(&format!("limit={id}"))).unwrap_or_else(|| panic!("no request for call {id}"));
        assert_eq!(got, &want, "call {id}");
    }
    // The loop's own thread was never given a name.
    assert_eq!(v2_lib::mcp::calling_agent(), None);
}

#[test]
fn a_call_that_panics_is_answered_as_an_internal_error() {
    let call: SharedCall = Arc::new(|_m: &str, path: &str, _b: &str| {
        if path == "/tools" {
            return Ok((200, all_modules_policy()));
        }
        panic!("a bridge call that goes wrong");
    });
    let lines = [tool_call_line(5, "get_autorun_page", serde_json::json!({}))];
    let (bytes, done) = start_serving(&lines, call);
    done.recv_timeout(SERVE_DEADLINE).expect("serve_lines returns");
    let out = response_lines(&bytes);
    assert_eq!(ids_of(&out), vec![5]);
    let v: serde_json::Value = serde_json::from_str(&out[0]).unwrap();
    assert_eq!(v["error"]["code"], -32603, "{v}");
}

/// A burst of calls from one client is answered a few at a time: never more than the cap run at once, and
/// every one of them is answered.
#[test]
fn no_more_than_the_cap_of_calls_run_at_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    assert_eq!(v2_lib::mcp::MAX_CALLS_IN_FLIGHT, 8);
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let (r, m) = (running.clone(), most.clone());
    let call: SharedCall = Arc::new(move |_m: &str, path: &str, _b: &str| {
        if path == "/tools" {
            return Ok((200, all_modules_policy()));
        }
        let now = r.fetch_add(1, Ordering::SeqCst) + 1;
        m.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        r.fetch_sub(1, Ordering::SeqCst);
        Ok((200, "{}".into()))
    });
    let lines: Vec<String> = (1..=10).map(|id| tool_call_line(id, "get_autorun_page", serde_json::json!({}))).collect();
    let (bytes, done) = start_serving(&lines, call);
    done.recv_timeout(SERVE_DEADLINE).expect("serve_lines returns");
    let mut ids = ids_of(&response_lines(&bytes));
    ids.sort_unstable();
    assert_eq!(ids, (1..=10).collect::<Vec<u64>>(), "every call is answered");
    let most = most.load(Ordering::SeqCst);
    assert!(most <= v2_lib::mcp::MAX_CALLS_IN_FLIGHT, "{most} calls ran at once");
    assert!(most > 1, "the calls still overlap: {most}");
}

/// `tools/list` against an app whose `/tools` answer is `tools_body`: the
/// tools by name.
fn tools_by_name(tools_body: &str) -> std::collections::HashMap<String, serde_json::Value> {
    let resp = handle_message(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "1.10.3",
        &stub(200, tools_body),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["name"].as_str().unwrap().to_string(), t.clone()))
        .collect()
}

fn description_of(tools: &std::collections::HashMap<String, serde_json::Value>, name: &str) -> String {
    tools[name]["description"].as_str().unwrap().to_string()
}

#[test]
fn the_tool_list_says_every_project_only_while_combined() {
    let on = tools_by_name(r#"{"all_projects":true}"#);
    let off = tools_by_name(r#"{"all_projects":false}"#);
    // An app that does not say (older, or closed) reads per project.
    let silent = tools_by_name("{}");

    assert!(
        description_of(&on, "search_test_suites")
            .starts_with("The test plans and suites in every project of the organization, one row per suite with its project, plan id, suite id,"),
        "{}",
        description_of(&on, "search_test_suites")
    );
    assert_eq!(
        description_of(&on, "search_pbis"),
        "Search every project's PBIs by title text to find the right work item id; each hit names its project."
    );
    assert_eq!(
        description_of(&on, "search_wiki"),
        "Search the organization's Azure DevOps wikis for documentation; returns page paths, snippet highlights and each page's project."
    );
    for name in ["get_suite_test_cases", "get_wiki_page"] {
        assert_eq!(on[name]["inputSchema"]["properties"]["project"]["type"], "string", "{name}");
        let required: Vec<&str> =
            on[name]["inputSchema"]["required"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
        assert!(!required.contains(&"project"), "{name} takes project as an option");
    }

    for other in [&off, &silent] {
        assert!(description_of(other, "search_test_suites").starts_with("The test plans and suites in the current project, one row per suite with its plan id,"));
        assert_eq!(description_of(other, "search_pbis"), "Search the current project's PBIs by title text to find the right work item id.");
        assert_eq!(
            description_of(other, "search_wiki"),
            "Search the project's Azure DevOps wiki for documentation; returns page paths and snippet highlights."
        );
        for name in ["get_suite_test_cases", "get_wiki_page"] {
            assert!(other[name]["inputSchema"]["properties"].get("project").is_none(), "{name}");
        }
    }

    // The home-project tools read the same in both modes.
    for name in ["run_api_template", "list_api_templates", "send_api_request"] {
        assert_eq!(description_of(&on, name), description_of(&off, name), "{name}");
    }
    assert!(!description_of(&on, "get_tags").contains("every project"));

    // Tags follow the writing job's PBI only while combined, and the
    // description says so, including the one work item read it may cost and
    // the `project` the answer names. Off or silent, it keeps its words.
    let tags_on = description_of(&on, "get_tags");
    assert!(tags_on.contains("project of the PBI the job named"), "{tags_on}");
    assert!(tags_on.contains("`project`"), "{tags_on}");
    assert!(tags_on.contains("one work item read"), "{tags_on}");
    for other in [&off, &silent] {
        assert_eq!(
            description_of(other, "get_tags"),
            "The tag names this project already uses. Prefer an existing tag over inventing a near-duplicate. Served from the app's cache - calling this costs no Azure DevOps request."
        );
    }

    // The words the assistant reads never use a term it is not given.
    for tools in [&on, &off, &silent] {
        let text = serde_json::to_string(&tools["get_wiki_page"]).unwrap();
        assert!(!text.contains("home project"), "{text}");
    }
    assert_eq!(
        on["get_wiki_page"]["inputSchema"]["properties"]["project"]["description"],
        "The project a search_wiki hit names. Optional: the project the app is set to is read when it is left out (a page URL names its own)."
    );
}

#[test]
fn suite_cases_and_wiki_pages_forward_an_explicit_project() {
    let (_, _, made) = call_with_arguments(
        "get_suite_test_cases",
        serde_json::json!({ "plan_id": 9, "suite_id": 901, "project": "Pay roll" }),
        (200, "{}"),
    );
    assert_eq!(
        paths_of(&made),
        vec!["/suite-cases?plan=9&suite=901&limit=5&offset=0&project=Pay%20roll"]
    );

    let (_, _, made) = call_with_arguments(
        "get_wiki_page",
        serde_json::json!({ "wiki_id": "w1", "path": "/Docs/Pay-Run", "project": "Payroll" }),
        (200, "{}"),
    );
    assert_eq!(paths_of(&made), vec!["/wiki-page?wiki=w1&path=%2FDocs%2FPay-Run&project=Payroll"]);

    // Left out or blank: the request is the one it always was.
    for project in [serde_json::json!(null), serde_json::json!("  ")] {
        let (_, _, made) = call_with_arguments(
            "get_suite_test_cases",
            serde_json::json!({ "plan_id": 9, "suite_id": 901, "project": project }),
            (200, "{}"),
        );
        assert_eq!(paths_of(&made), vec!["/suite-cases?plan=9&suite=901&limit=5&offset=0"]);
    }
    let (_, _, made) = call_with_arguments(
        "get_wiki_page",
        serde_json::json!({ "wiki_id": "w1", "path": "/Docs/Pay-Run" }),
        (200, "{}"),
    );
    assert_eq!(paths_of(&made), vec!["/wiki-page?wiki=w1&path=%2FDocs%2FPay-Run"]);
}
