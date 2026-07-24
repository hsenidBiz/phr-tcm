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
    let (status, body) = route(&ctx(), None, "GET", "/ping", "").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["app"], "tcm");
    assert_eq!(v["org"], "acme");
}

#[tokio::test]
async fn unknown_routes_404() {
    let (status, _) = route(&ctx(), None, "GET", "/secrets", "").await;
    assert_eq!(status, 404);
    let (status, _) = route(&ctx(), None, "DELETE", "/ping", "").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn validate_runs_the_real_importer() {
    let good = r#"[{"title": "Login works", "steps": [{"action": "Open", "expected": "Shown"}]}]"#;
    let (status, body) = route(&ctx(), None, "POST", "/validate", good).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases"], 1);
    assert_eq!(v["warnings"].as_array().unwrap().len(), 0);

    // Not-JSON is a hard importer error; an empty/foreign wrapper is a
    // lenient zero-case parse - both must be visible to the AI, never a
    // silent success with cases > 0.
    let (status, body) = route(&ctx(), None, "POST", "/validate", "not json at all").await;
    assert_eq!(status, 200); // validation RESULTS are a 200; only transport errors aren't
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["error"].as_str().is_some(), "hard parse failure must carry an error");

    let (_, body) = route(&ctx(), None, "POST", "/validate", r#"{"not": "a wrapper"}"#).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases"], 0, "foreign objects must never count as cases");
}

#[test]
fn query_parsing_survives_valueless_pairs() {
    assert_eq!(q("/examples?flag&pbi=42", "pbi").as_deref(), Some("42"));
    assert_eq!(q("/x?module=Pay+roll%20HR", "module").as_deref(), Some("Pay roll HR"));
    assert_eq!(q("/x?a=1", "b"), None);
    assert_eq!(q("/noquery", "a"), None);
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

    let (status, body) = route(&ctx(), Some(&client), "GET", "/guide", "").await;
    assert_eq!(status, 200);
    assert!(body.contains("Not Automated"), "statuses come from VALID_STATUSES");
    assert!(body.contains("Planned"));
    assert!(body.contains("semicolon"), "tag separator rule");
    assert!(body.contains("Login") && body.contains("Payroll"), "live modules");
    assert!(body.contains("validate_cases"), "guide tells the AI to validate");
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
        route(&ctx(), Some(&client), "GET", "/examples?pbi=42&limit=5", "").await;
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
async fn examples_without_pbi_400_with_guidance() {
    let (_server, client) = ado_stub().await;
    let (status, body) = route(&ctx(), Some(&client), "GET", "/examples", "").await;
    assert_eq!(status, 400);
    assert!(body.contains("pbi"));
}
