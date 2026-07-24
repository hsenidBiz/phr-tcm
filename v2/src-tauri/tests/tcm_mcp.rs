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
fn tools_list_names_all_four() {
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
        vec!["get_writing_guide", "get_example_cases", "validate_cases", "search_pbis"]
    );
    // Every tool must carry an inputSchema (clients reject tools without one).
    for t in v["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object");
    }
}

#[test]
fn tools_call_proxies_to_the_bridge_and_wraps_text() {
    let req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_example_cases","arguments":{"pbi_id":42,"limit":3}}}"#;
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
    assert_eq!(recorded[0].0, "GET");
    assert_eq!(recorded[0].1, "/examples?pbi=42&limit=3");
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
    assert_eq!(recorded[0].1, "/search-pbis?q=Search%20%26%20Filter");
}
