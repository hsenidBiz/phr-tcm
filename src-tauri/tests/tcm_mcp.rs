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
    // This test binary is a development build (cargo test compiles with
    // debug assertions on), so with nothing disabled the seven dev-only
    // autorun tools are listed like any other switchable tool - between
    // merge_case_files and optimize_cases, where they sit in the source.
    assert_eq!(
        names,
        vec![
            "begin_test_case_writing",
            "get_writing_guide",
            "get_test_cases",
            "search_test_suites",
            "get_suite_test_cases",
            "get_run_failures",
            "check_spec_coverage",
            "merge_case_files",
            "get_autorun_guide",
            "save_autorun_script",
            "get_autorun_page",
            "probe_autorun_locator",
            "try_autorun_action",
            "get_autorun_failures",
            "record_autorun_quirk",
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
    // 22 in this development build: nothing is disabled by an unreachable
    // bridge, including the seven dev-only tools, which default to ON here
    // exactly as they would if the bridge had answered with an empty list.
    assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 22, "an unreachable bridge must not disable anything, dev-only tools included");
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
    let req = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_writing_guide","arguments":{}}}"#;
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
        names.contains(&"get_autorun_guide")
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
/// so the list the bridge reports names all seven together.
#[test]
fn autorun_tools_named_disabled_in_a_dev_build_are_absent_and_refused_the_ordinary_way() {
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((
                200,
                r#"{"disabled":["get_autorun_guide","save_autorun_script","get_autorun_page","probe_autorun_locator","try_autorun_action","get_autorun_failures","record_autorun_quirk"]}"#.into(),
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

/// The release rule cannot be exercised through `handle_message` at all -
/// this test binary is itself a development build - so it is proven
/// directly through the `dev: bool` seam `mcp::refusal_text` exists for.
#[test]
fn a_dev_only_tool_refused_outside_a_development_build_says_not_available() {
    let text = v2_lib::mcp::refusal_text("save_autorun_script", false);
    assert!(text.contains("not available"), "{text}");
    assert!(!text.contains("switched off"), "{text}");
}

/// The same tool, switched off inside a development build, gets the
/// ordinary sentence - the release wording is not a blanket rule for
/// dev-only tools, only for the build kind that has no switch at all.
#[test]
fn a_dev_only_tool_refused_inside_a_development_build_gets_the_ordinary_sentence() {
    let text = v2_lib::mcp::refusal_text("save_autorun_script", true);
    assert!(text.contains("switched off"), "{text}");
    assert!(!text.contains("not available"), "{text}");
}

/// A tool that was never dev-only is unaffected by the `dev` flag either
/// way - the seam must not accidentally widen who gets the release
/// wording.
#[test]
fn an_ordinary_tool_gets_the_switched_off_sentence_regardless_of_dev() {
    for dev in [true, false] {
        let text = v2_lib::mcp::refusal_text("search_wiki", dev);
        assert!(text.contains("switched off"), "dev={dev}: {text}");
    }
}
