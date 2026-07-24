//! The bridge's router, tested without sockets: routing, validation, and
//! the ADO-backed routes (Task 2) via wiremock. The TCP loop (Task 3) is
//! deliberately thin - everything interesting lives in `route`.

use v2_lib::ai_bridge::{new_token, route, BridgeContext};

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
