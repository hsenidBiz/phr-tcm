//! The bridge's router, tested without sockets: routing, validation, and
//! the ADO-backed routes (Task 2) via wiremock. The TCP loop (Task 3) is
//! deliberately thin - everything interesting lives in `route`.

use v2_lib::ai_bridge::{new_token, q, route, BridgeContext};

fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: Some("Custom.Module".into()),
        preconditions_ref: Some("Custom.Preconditions".into()),
        disabled_tools: vec![],
    }
}

#[test]
fn tokens_are_32_hex_and_unique() {
    let a = new_token();
    let b = new_token();
    assert_eq!(a.len(), 32);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

#[tokio::test]
async fn ping_answers_without_a_client() {
    let (status, body) = route(&ctx(), None, "GET", "/ping", "", "1.10.3").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["app"], "tcm");
    assert_eq!(v["org"], "acme");
    assert_eq!(v["version"], "1.10.3", "reports the real app version, not this crate's");
}

#[tokio::test]
async fn unknown_routes_404() {
    let (status, _) = route(&ctx(), None, "GET", "/secrets", "", "1.10.3").await;
    assert_eq!(status, 404);
    let (status, _) = route(&ctx(), None, "DELETE", "/ping", "", "1.10.3").await;
    assert_eq!(status, 404);
}

#[test]
fn query_parsing_survives_valueless_pairs() {
    assert_eq!(q("/test-cases?flag&pbi=42", "pbi").as_deref(), Some("42"));
    assert_eq!(q("/x?module=Pay+roll%20HR", "module").as_deref(), Some("Pay roll HR"));
    assert_eq!(q("/x?a=1", "b"), None);
    assert_eq!(q("/noquery", "a"), None);
}

#[test]
fn query_parsing_decodes_arbitrary_percent_escapes() {
    // mcp.rs percent-encodes search text with a generic RFC 3986 encoder
    // (not just spaces) so literal '&'/'='/etc. in the query text survive
    // the naive '&'-split in `q`. The decoder here must be the matching
    // generic counterpart, not a %20-only special case.
    assert_eq!(
        q("/search-pbis?q=Search%20%26%20Filter", "q").as_deref(),
        Some("Search & Filter")
    );
}

use v2_lib::ado::AdoClient;
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Wiremock host standing in for ADO; the routes hit the same endpoints
/// the app's own screens use.
async fn ado_stub() -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

/// The two warnings that came out of round-2 feedback, exercised through
/// the route rather than against the checker directly - the wiring is the
/// part that was missing, not the detection.
#[tokio::test]
async fn validate_warns_about_markup_and_merged_branches() {
    let draft = serde_json::json!({
        "test_cases": [
            {
                "title": "Quote a spec element",
                "automation_status": "Not Automated",
                "steps": [
                    { "action": "Open the reject popup.", "expected": "A <textarea> is shown." },
                    { "action": "Read the body.", "expected": "It is <div> wrapped." }
                ]
            },
            {
                "title": "Actions and Measures visibility",
                "automation_status": "Not Automated",
                "steps": [
                    { "action": "Sign in as the manager.", "expected": "The dashboard is shown." },
                    { "action": "Open the appraisal.", "expected": "The form is shown." },
                    { "action": "Expand the goal row.", "expected": "An Actions and Measures section is shown." },
                    { "action": "Turn off the action_measure_enabled setting.", "expected": "Saved." },
                    { "action": "Expand the goal row again.", "expected": "No Actions and Measures section is shown." }
                ]
            },
            {
                "title": "A plain case with a placeholder",
                "automation_status": "Not Automated",
                "steps": [
                    { "action": "Run SELECT * FROM t WHERE id = <cycleId>;", "expected": "One row is returned." }
                ]
            }
        ]
    })
    .to_string();

    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.18.6").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], serde_json::Value::Null, "{body}");
    assert_eq!(v["cases"], 3, "{body}");
    let warnings = v["warnings"].as_array().unwrap();
    let all = warnings
        .iter()
        .map(|w| w.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" | ");

    // A real element name will be eaten by the round trip; say so.
    assert!(
        all.contains("Quote a spec element") && all.contains("HTML tag name"),
        "expected a markup warning, got: {all}"
    );
    // The merged positive/negative.
    assert!(
        all.contains("Actions and Measures visibility") && all.contains("both branches"),
        "expected a merged-branch warning, got: {all}"
    );
    // And the ordinary case stays quiet - <cycleId> survives now, so
    // warning about it would be noise.
    assert!(
        !all.contains("A plain case with a placeholder"),
        "a safe placeholder must not warn: {all}"
    );
}
#[tokio::test]
async fn guide_carries_format_rules_and_live_modules() {
    let (server, client) = ado_stub().await;
    // Module picklist: allowedValues empty -> falls back to values-in-use,
    // exactly like the app's own module picker.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wit/workitemtypes/Test%20Case/fields/Custom.Module"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "allowedValues": ["Login", "Payroll"]
        })))
        .mount(&server)
        .await;

    let (status, body) = route(&ctx(), Some(&client), "GET", "/guide", "", "1.10.3").await;
    assert_eq!(status, 200);
    assert!(body.contains("Not Automated"), "statuses come from VALID_STATUSES");
    assert!(body.contains("Planned"));
    assert!(body.contains("semicolon"), "tag separator rule");
    assert!(body.contains("Login") && body.contains("Payroll"), "live modules");
    assert!(body.contains("optimize_cases"), "the guide points at the optimizer");

    // Reviewer notes have to be told to stay SHORT. Asked only for "the
    // spec section, the acceptance criterion, a quote and what was out of
    // scope", an assistant dutifully wrote a paragraph per case - correct,
    // and slower to read than the steps it was annotating. The brief is a
    // pointer, and the guide has to say so or the notes drift back.
    let notes = body
        .split("## reviewer_notes")
        .nth(1)
        .and_then(|rest| rest.split("## Allowed Module values").next())
        .expect("the guide still has a reviewer_notes section");
    assert!(notes.contains("POINTER"), "says what the field is FOR: {notes}");
    assert!(notes.contains("ONE OR TWO LINES"), "bounds the length: {notes}");
    assert!(
        notes.contains("Do NOT restate the test"),
        "names the failure mode it guards against: {notes}"
    );
    // And a shape to copy, not just a prohibition.
    assert!(notes.contains("Spec:") && notes.contains("Code:"), "{notes}");
}

#[tokio::test]
async fn test_cases_return_real_cases_in_import_shape() {
    let (server, client) = ado_stub().await;
    // The same two calls the runner/edit screens make: ids-for-PBI, then
    // batch details. Match loosely on path; the client's own tests pin the
    // exact query strings.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 42,
            "relations": [
                {"rel": "Microsoft.VSTS.Common.TestedBy-Forward",
                 "url": format!("{}/acme/Web/_apis/wit/workitems/201", server.uri())}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 201,
                "fields": {
                    "System.Title": "Login - valid credentials",
                    "System.Tags": "smoke",
                    "Microsoft.VSTS.TCM.AutomationStatus": "Planned",
                    "Custom.Module": "Login",
                    "Custom.Preconditions": "Account exists",
                    "Microsoft.VSTS.TCM.Steps": "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open page</parameterizedString><parameterizedString isformatted=\"true\">Shown</parameterizedString><description/></step></steps>"
                }
            }]
        })))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/test-cases?pbi=42&limit=5", "", "1.10.3").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let cases = v["test_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0]["id"], 201);
    assert_eq!(cases[0]["title"], "Login - valid credentials");
    assert_eq!(cases[0]["module"], "Login");
    assert_eq!(cases[0]["steps"][0]["action"], "Open page");
}

#[tokio::test]
async fn search_wiki_503_without_a_client() {
    let (status, body) = route(&ctx(), None, "GET", "/search-wiki?q=auth", "", "1.10.3").await;
    assert_eq!(status, 503);
    assert!(body.contains("sign in"));
}

#[tokio::test]
async fn wiki_page_503_without_a_client() {
    let (status, body) =
        route(&ctx(), None, "GET", "/wiki-page?wiki=w1&path=/Docs/Guide", "", "1.10.3").await;
    assert_eq!(status, 503);
    assert!(body.contains("sign in"));
}

#[tokio::test]
async fn search_wiki_returns_hits_via_wiremock() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("POST"))
        .and(wm_path("/acme/Web/_apis/search/wikisearchresults"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [{
                "fileName": "Auth.md",
                "path": "/Docs/Auth",
                "wiki": {"id": "w1", "name": "Team.wiki"},
                "hits": [{"highlights": ["snippet"]}]
            }]
        })))
        .mount(&server)
        .await;
    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/search-wiki?q=auth", "", "1.10.3").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["wiki_id"], "w1");
    assert_eq!(results[0]["highlights"], "snippet");
}

#[tokio::test]
async fn wiki_page_returns_content_via_wiremock() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wiki/wikis/w1/pages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Docs/Auth",
            "content": "# Auth\nfull text"
        })))
        .mount(&server)
        .await;
    let (status, body) = route(
        &ctx(),
        Some(&client),
        "GET",
        "/wiki-page?wiki=w1&path=/Docs/Auth",
        "",
        "1.10.3",
    )
    .await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["path"], "/Docs/Auth");
    assert!(v["content"].as_str().unwrap().contains("full text"));
}

#[tokio::test]
async fn test_cases_without_pbi_400_with_guidance() {
    let (_server, client) = ado_stub().await;
    let (status, body) = route(&ctx(), Some(&client), "GET", "/test-cases", "", "1.10.3").await;
    assert_eq!(status, 400);
    assert!(body.contains("pbi"));
}

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn tcp_server_guards_with_token_and_serves_ping() {
    let shared = v2_lib::ai_bridge::BridgeState::new(ctx(), "0.0.0-test".into());
    // Test-specific handshake path: the real handshake_path() location belongs
    // to a live app - a test run must never clobber it (it did once: proxies
    // then saw a dead port + test token until Settings was reopened).
    let hs_path = std::env::temp_dir().join(format!("tcm-v2-hs-test-{}.json", std::process::id()));
    let (port, token) =
        v2_lib::ai_bridge::start_listener(Arc::clone(&shared), None, Some(hs_path.clone()))
            .await
            .unwrap();

    async fn send(port: u16, req: String) -> String {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf).to_string()
    }

    // Wrong token -> 401, no body.
    let resp = send(
        port,
        "GET /ping HTTP/1.1\r\nHost: x\r\nx-bridge-token: wrong\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(resp.starts_with("HTTP/1.1 401"), "got: {resp}");
    assert!(!resp.contains("tcm"));

    // Right token -> 200 with the ping payload.
    let resp = send(
        port,
        format!("GET /ping HTTP/1.1\r\nHost: x\r\nx-bridge-token: {token}\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert!(resp.starts_with("HTTP/1.1 200"), "got: {resp}");
    assert!(resp.contains("\"app\":\"tcm\""));

    // The handshake file exists and matches, including the version tcm-mcp
    // reads for its own serverInfo.version.
    let hs: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&hs_path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&hs_path);
    assert_eq!(hs["port"].as_u64().unwrap() as u16, port);
    assert_eq!(hs["token"].as_str().unwrap(), token);
    assert_eq!(hs["version"].as_str().unwrap(), "0.0.0-test");
}


/// Tool toggles: the app publishes what is switched off, and the MCP layer
/// honours it.
#[tokio::test]
async fn the_bridge_publishes_the_disabled_tool_set() {
    let mut ctx = ctx();
    ctx.disabled_tools = vec!["search_wiki".into(), "get_wiki_page".into()];
    let (status, body) = route(&ctx, None, "GET", "/tools", "", "1.0.0").await;

    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["disabled"][0], "search_wiki");
    assert_eq!(v["disabled"].as_array().unwrap().len(), 2);
}

/// Nothing configured means nothing disabled - a fresh install must not
/// come up with an empty toolset.
#[tokio::test]
async fn no_configuration_disables_nothing() {
    let (_, body) = route(&ctx(), None, "GET", "/tools", "", "1.0.0").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["disabled"].as_array().unwrap().is_empty());
}

/// The importer skips a case it cannot read and says why. Both tools threw
/// those warnings away, so a draft came back shorter with no indication -
/// "success" over work that had quietly gone missing.
#[tokio::test]
async fn optimize_and_transform_report_what_the_importer_could_not_read() {
    // Two cases; the second has no steps, which the importer skips.
    let draft = serde_json::json!({
        "test_cases": [
            { "id": null, "title": "Good", "steps": [{ "action": "Do", "expected": "Done" }] },
            { "id": null, "title": "No steps at all", "steps": [] }
        ]
    })
    .to_string();

    let (status, body) = route(&ctx(), None, "POST", "/optimize", &draft, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let warned = v["import_warnings"].as_array().expect("import_warnings present");
    assert!(
        warned.iter().any(|w| w.as_str().unwrap_or_default().contains("No steps at all")),
        "the dropped case must be named: {warned:?}"
    );
    // And the output really is shorter, which is why silence was wrong.
    assert_eq!(v["test_cases"].as_array().unwrap().len(), 1);

    let t_body = serde_json::json!({
        "test_cases": draft,
        "operations": [{ "op": "set_tags", "value": "smoke" }],
    })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/transform", &t_body, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        v["import_warnings"]
            .as_array()
            .expect("import_warnings present")
            .iter()
            .any(|w| w.as_str().unwrap_or_default().contains("No steps at all")),
        "transform must report it too"
    );
}

/// Offsets used to come from `String::from_utf8_lossy(raw)` and then index
/// `raw`. Those are not the same string - every invalid byte becomes a
/// three-byte U+FFFD - so one bad byte in the headers slid the body offset
/// and the request was read from the wrong place.
#[test]
fn the_body_is_found_by_byte_not_by_a_lossy_copy() {
    use v2_lib::ai_bridge::{parse_http, Parsed};

    // A header carrying a byte that is not valid UTF-8. Under the old
    // parser the decoded head was longer than the real one, so body_start
    // pointed past the start of the body.
    let mut raw = b"POST /validate?x=1 HTTP/1.1\r\nX-Note: ".to_vec();
    raw.push(0xFF);
    raw.extend_from_slice(b"\r\nContent-Length: 9\r\n\r\n{\"a\":123}");
    match parse_http(&raw) {
        // Headers that are not text at all is a fine thing to refuse - what
        // must never happen is reading the body from the wrong offset and
        // treating the result as a real request.
        Parsed::Malformed => {}
        Parsed::Complete { body, .. } => assert_eq!(body, "{\"a\":123}", "body read at the wrong offset"),
        Parsed::Incomplete => panic!("a complete request was read as incomplete"),
    }

    // The ordinary case still works, body and all.
    let ok = b"POST /validate HTTP/1.1\r\nX-Bridge-Token: abc\r\nContent-Length: 9\r\n\r\n{\"a\":123}";
    let Parsed::Complete { method, target, token, body } = parse_http(ok) else {
        panic!("a well-formed request did not parse");
    };
    assert_eq!((method.as_str(), target.as_str()), ("POST", "/validate"));
    assert_eq!(token.as_deref(), Some("abc"));
    assert_eq!(body, "{\"a\":123}");
}

/// "Keep reading" and "this will never be a request" used to be the same
/// answer (None). A malformed request therefore read as incomplete, and the
/// connection sat there - to the 64 KB cap if the client kept sending, or
/// for the life of the process if it simply stopped.
#[test]
fn a_malformed_request_is_told_apart_from_an_unfinished_one() {
    use v2_lib::ai_bridge::{parse_http, Parsed};

    // Genuinely unfinished: no blank line yet, and short.
    assert!(matches!(parse_http(b"POST /validate HTTP/1.1\r\nX-A: 1\r\n"), Parsed::Incomplete));
    // Headers complete, body still arriving.
    assert!(matches!(
        parse_http(b"POST /v HTTP/1.1\r\nContent-Length: 20\r\n\r\nshort"),
        Parsed::Incomplete
    ));

    // Never going to be a request.
    assert!(matches!(
        parse_http(b"POST /v HTTP/1.1\r\nContent-Length: not-a-number\r\n\r\n"),
        Parsed::Malformed
    ));
    assert!(matches!(parse_http(b"\r\n\r\n"), Parsed::Malformed), "no request line");
    assert!(matches!(parse_http(b"GET\r\n\r\n"), Parsed::Malformed), "no target");

    // A header line without a colon is not a reason to refuse the request -
    // only Content-Length has to be right.
    assert!(matches!(
        parse_http(b"GET /ping HTTP/1.1\r\ngarbage-line\r\n\r\n"),
        Parsed::Complete { .. }
    ));

    // Endless garbage with no blank line stops being "incomplete".
    let flood = vec![b'x'; 17 * 1024];
    assert!(matches!(parse_http(&flood), Parsed::Malformed));
}
