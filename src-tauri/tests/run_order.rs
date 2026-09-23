//! The suggested run-order file (`tcm-run-order.json`): parsing, picking
//! the newest of several, and the PBI reads/writes that carry it. Design
//! docs/superpowers/specs/2026-09-23-run-order-design.md §4.2, §7.

use v2_lib::ado::AdoClient;
use v2_lib::run_order::{
    newest, parse_run_order, RunOrderCase, RunOrderFile, RunOrderRead, RUN_ORDER_COMMENT,
    RUN_ORDER_FILE_NAME, RUN_ORDER_FORMAT, RUN_ORDER_VERSION,
};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A server of its own, never `MockServer::start()` - a pooled server can
/// arrive already carrying state from an earlier test (see ado_boards.rs's
/// `unshared_server`, the precedent this plan's house rules point at).
async fn unshared_server() -> MockServer {
    MockServer::builder().start().await
}

fn sample_file(saved_at: &str) -> RunOrderFile {
    RunOrderFile {
        format: RUN_ORDER_FORMAT.to_string(),
        version: RUN_ORDER_VERSION,
        saved_by: "tester@example.com".to_string(),
        saved_at: saved_at.to_string(),
        cases: vec![
            RunOrderCase { id: 157941, group: Some("HRM\\Gamma Guardians".to_string()) },
            RunOrderCase { id: 157953, group: None },
        ],
    }
}

fn file_json(saved_at: &str) -> String {
    serde_json::to_string(&sample_file(saved_at)).unwrap()
}

// ── parse_run_order ─────────────────────────────────────────────────────

#[test]
fn parse_run_order_accepts_the_spec_example() {
    let text = r#"{
      "format": "tcm-run-order",
      "version": 1,
      "saved_by": "someone@example.com",
      "saved_at": "2026-09-23T10:15:00Z",
      "cases": [
        { "id": 157941, "group": "HRM\\Gamma Guardians" },
        { "id": 157953 }
      ]
    }"#;
    let file = parse_run_order(text).unwrap();
    assert_eq!(file.format, RUN_ORDER_FORMAT);
    assert_eq!(file.version, 1);
    assert_eq!(file.saved_by, "someone@example.com");
    assert_eq!(file.saved_at, "2026-09-23T10:15:00Z");
    assert_eq!(file.cases.len(), 2);
    assert_eq!(file.cases[0].id, 157941);
    assert_eq!(file.cases[0].group.as_deref(), Some("HRM\\Gamma Guardians"));
    assert_eq!(file.cases[1].id, 157953);
    assert_eq!(file.cases[1].group, None);
}

#[test]
fn parse_run_order_rejects_wrong_format() {
    let text = r#"{"format":"tcm-draft-review","version":1,"saved_by":"a","saved_at":"2026-09-23T10:15:00Z","cases":[]}"#;
    let err = parse_run_order(text).unwrap_err();
    assert!(!err.to_lowercase().contains("http"), "got: {err}");
}

#[test]
fn parse_run_order_rejects_version_2() {
    let text = r#"{"format":"tcm-run-order","version":2,"saved_by":"a","saved_at":"2026-09-23T10:15:00Z","cases":[]}"#;
    let err = parse_run_order(text).unwrap_err();
    assert!(!err.to_lowercase().contains("http"), "got: {err}");
}

#[test]
fn parse_run_order_rejects_invalid_json() {
    let err = parse_run_order("not json").unwrap_err();
    assert!(!err.to_lowercase().contains("http"), "got: {err}");
}

// ── newest ───────────────────────────────────────────────────────────────

#[test]
fn newest_picks_the_later_saved_at() {
    let older = sample_file("2026-09-20T08:00:00Z");
    let newer = sample_file("2026-09-23T10:15:00Z");
    let picked = newest(vec![older, newer.clone()]).unwrap();
    assert_eq!(picked, newer);
}

#[test]
fn newest_of_empty_is_none() {
    assert!(newest(vec![]).is_none());
}

// ── read_run_order ───────────────────────────────────────────────────────

#[tokio::test]
async fn read_run_order_with_no_relations_is_none() {
    let server = unshared_server().await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/145386"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 145386, "rev": 1, "relations": []
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let read = client.read_run_order("acme", "Web", 145386).await.unwrap();
    assert!(matches!(read, RunOrderRead::None), "{read:?}");
}

#[tokio::test]
async fn read_run_order_finds_one() {
    let server = unshared_server().await;
    let json = file_json("2026-09-23T10:15:00Z");
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/145386"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 145386, "rev": 3,
            "relations": [
                { "rel": "AttachedFile",
                  "url": format!("{}/acme/Web/_apis/wit/attachments/aaaa", server.uri()),
                  "attributes": { "name": RUN_ORDER_FILE_NAME, "comment": RUN_ORDER_COMMENT } }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/attachments/aaaa"))
        .respond_with(ResponseTemplate::new(200).set_body_string(json))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let read = client.read_run_order("acme", "Web", 145386).await.unwrap();
    match read {
        RunOrderRead::Found { file } => {
            assert_eq!(file.saved_at, "2026-09-23T10:15:00Z");
            assert_eq!(file.cases.len(), 2);
        }
        other => panic!("expected Found, got {other:?}"),
    }
}

#[tokio::test]
async fn read_run_order_picks_the_newer_of_two() {
    let server = unshared_server().await;
    let older = file_json("2026-09-20T08:00:00Z");
    let newer = file_json("2026-09-23T10:15:00Z");
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/145386"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 145386, "rev": 5,
            "relations": [
                { "rel": "AttachedFile",
                  "url": format!("{}/acme/Web/_apis/wit/attachments/aaaa", server.uri()),
                  "attributes": { "name": RUN_ORDER_FILE_NAME } },
                { "rel": "AttachedFile",
                  "url": format!("{}/acme/Web/_apis/wit/attachments/bbbb", server.uri()),
                  "attributes": { "name": RUN_ORDER_FILE_NAME } }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/attachments/aaaa"))
        .respond_with(ResponseTemplate::new(200).set_body_string(older))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/attachments/bbbb"))
        .respond_with(ResponseTemplate::new(200).set_body_string(newer))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let read = client.read_run_order("acme", "Web", 145386).await.unwrap();
    match read {
        RunOrderRead::Found { file } => assert_eq!(file.saved_at, "2026-09-23T10:15:00Z"),
        other => panic!("expected Found, got {other:?}"),
    }
}

#[tokio::test]
async fn read_run_order_unreadable_when_the_only_one_does_not_parse() {
    let server = unshared_server().await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/145386"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 145386, "rev": 2,
            "relations": [
                { "rel": "AttachedFile",
                  "url": format!("{}/acme/Web/_apis/wit/attachments/aaaa", server.uri()),
                  "attributes": { "name": RUN_ORDER_FILE_NAME } }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/attachments/aaaa"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let read = client.read_run_order("acme", "Web", 145386).await.unwrap();
    match read {
        RunOrderRead::Unreadable { reason } => assert!(!reason.to_lowercase().contains("http"), "got: {reason}"),
        other => panic!("expected Unreadable, got {other:?}"),
    }
}

#[tokio::test]
async fn read_run_order_ignores_a_differently_named_attached_file() {
    let server = unshared_server().await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/145386"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 145386, "rev": 1,
            "relations": [
                { "rel": "AttachedFile",
                  "url": format!("{}/acme/Web/_apis/wit/attachments/zzzz", server.uri()),
                  "attributes": { "name": "tcm-draft-review-145386-abcdef.json" } }
            ]
        })))
        .mount(&server)
        .await;
    // Deliberately no mock for the attachment GET: if the code tried to
    // download the unrelated file, the missing mock would fail the call.

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let read = client.read_run_order("acme", "Web", 145386).await.unwrap();
    assert!(matches!(read, RunOrderRead::None), "{read:?}");
}

// ── save_run_order ───────────────────────────────────────────────────────

#[tokio::test]
async fn save_run_order_uploads_then_patches_removing_only_the_old_run_order_relation() {
    let server = unshared_server().await;
    Mock::given(method("POST"))
        .and(path("/acme/Web/_apis/wit/attachments"))
        .and(query_param("fileName", RUN_ORDER_FILE_NAME))
        .and(body_string_contains(r#""format":"tcm-run-order""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "newid",
            "url": format!("{}/acme/Web/_apis/wit/attachments/newid", server.uri())
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/145386"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 145386, "rev": 7,
            "relations": [
                { "rel": "AttachedFile", "url": "https://x/att/other",
                  "attributes": { "name": "something-else.png" } },
                { "rel": "AttachedFile", "url": "https://x/att/old",
                  "attributes": { "name": RUN_ORDER_FILE_NAME } }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/acme/Web/_apis/wit/workitems/145386"))
        .and(body_string_contains(r#""op":"test""#))
        .and(body_string_contains(r#""value":7"#))
        .and(body_string_contains(r#""path":"/relations/1""#))
        .and(body_string_contains(RUN_ORDER_FILE_NAME))
        .and(body_string_contains(RUN_ORDER_COMMENT))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 145386 })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let file = sample_file("2026-09-23T10:15:00Z");
    client.save_run_order("acme", "Web", 145386, &file).await.unwrap();

    // "something-else.png" is relation 0 and must survive - only relation 1
    // (the old run-order file) may be removed.
    let requests = server.received_requests().await.unwrap();
    let patch_body: String = requests
        .iter()
        .find(|r| r.method == wiremock::http::Method::PATCH)
        .map(|r| String::from_utf8(r.body.clone()).unwrap())
        .unwrap();
    assert!(
        !patch_body.contains(r#""path":"/relations/0""#),
        "removed the wrong relation: {patch_body}"
    );
}

#[tokio::test]
async fn save_run_order_removes_two_old_relations_highest_index_first() {
    let server = unshared_server().await;
    Mock::given(method("POST"))
        .and(path("/acme/Web/_apis/wit/attachments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "newid",
            "url": format!("{}/acme/Web/_apis/wit/attachments/newid", server.uri())
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/145386"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 145386, "rev": 9,
            "relations": [
                { "rel": "AttachedFile", "url": "https://x/att/0",
                  "attributes": { "name": "something-else.png" } },
                { "rel": "AttachedFile", "url": "https://x/att/1",
                  "attributes": { "name": RUN_ORDER_FILE_NAME } },
                { "rel": "Microsoft.VSTS.Common.TestedBy-Forward", "url": "https://x/wi/2" },
                { "rel": "AttachedFile", "url": "https://x/att/3",
                  "attributes": { "name": RUN_ORDER_FILE_NAME } }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/acme/Web/_apis/wit/workitems/145386"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 145386 })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let file = sample_file("2026-09-23T10:15:00Z");
    client.save_run_order("acme", "Web", 145386, &file).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let patch_body: String = requests
        .iter()
        .find(|r| r.method == wiremock::http::Method::PATCH)
        .map(|r| String::from_utf8(r.body.clone()).unwrap())
        .unwrap();
    assert!(patch_body.contains(r#""path":"/relations/3""#), "{patch_body}");
    assert!(patch_body.contains(r#""path":"/relations/1""#), "{patch_body}");
    assert!(
        !patch_body.contains(r#""path":"/relations/0""#) && !patch_body.contains(r#""path":"/relations/2""#),
        "removed an unrelated relation: {patch_body}"
    );
    let idx_3 = patch_body.find(r#""path":"/relations/3""#).unwrap();
    let idx_1 = patch_body.find(r#""path":"/relations/1""#).unwrap();
    assert!(idx_3 < idx_1, "relation 3 must be removed before relation 1: {patch_body}");
}

// ── no DELETE, ever ──────────────────────────────────────────────────────

#[test]
fn run_order_module_never_sends_delete() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/run_order.rs"),
    )
    .unwrap();
    assert!(!src.contains("Method::DELETE"), "run_order.rs must never send DELETE");
    assert!(!src.contains(".delete("), "run_order.rs must never send DELETE");
}
