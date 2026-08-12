//! The MCP dispatcher is pure: JSON-RPC string in, response string out,
//! with the bridge call injected - no stdio, no sockets.

use v2_lib::mcp::handle_message;

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
    assert_eq!(v["result"]["serverInfo"]["name"], "tcm-testcases");
    assert_eq!(v["result"]["serverInfo"]["version"], "1.10.3", "reports the app version, not this crate's");
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
    assert_eq!(
        names,
        vec![
            "begin_test_case_writing",
            "get_writing_guide",
            "get_test_cases",
            "get_run_failures",
            "check_spec_coverage",
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

#[test]
fn bridge_errors_surface_as_tool_errors_not_crashes() {
    let req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_writing_guide","arguments":{}}}"#;
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
/// being closed is not the same as the user disabling everything.
#[test]
fn an_unreachable_bridge_disables_nothing() {
    let call = |_m: &str, _p: &str, _b: &str| -> Result<(u16, String), String> {
        Err("connection refused".into())
    };
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let resp = handle_message(req, "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 12);
}
