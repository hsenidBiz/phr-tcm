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
    assert_eq!(q("/examples?flag&pbi=42", "pbi").as_deref(), Some("42"));
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
}

#[tokio::test]
async fn examples_return_real_cases_in_import_shape() {
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
        route(&ctx(), Some(&client), "GET", "/examples?pbi=42&limit=5", "", "1.10.3").await;
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
async fn examples_without_pbi_400_with_guidance() {
    let (_server, client) = ado_stub().await;
    let (status, body) = route(&ctx(), Some(&client), "GET", "/examples", "", "1.10.3").await;
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
