//! The suggested run-order file (`tcm-run-order.json`): parsing, picking
//! the newest of several, and the PBI reads/writes that carry it. Design
//! docs/superpowers/specs/2026-09-23-run-order-design.md §4.2, §7.

use std::time::Duration;
use v2_lib::ado::AdoClient;
use v2_lib::model::TestCase;
use v2_lib::run_order::{
    newest, order_after_upload, parse_run_order, spec_order_ids, tester_order_cases, with_rest, Landed,
    OrderHint, RunOrderCase, RunOrderFile, RunOrderRead, NOTE_SUITE_BEHIND, RUN_ORDER_COMMENT,
    RUN_ORDER_FILE_NAME, RUN_ORDER_FORMAT, RUN_ORDER_VERSION,
};
use v2_lib::ado_testplan::{cached_suite, remember_suite, EnsuredSuite};
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

// ── upload helpers (design §4.1, §4.2) ──────────────────────────────────

fn case_at(spec: Option<u32>, tester: Option<u32>, area: &str) -> TestCase {
    TestCase {
        title: "t".into(),
        spec_order: spec,
        tester_order: tester,
        area: area.into(),
        ..Default::default()
    }
}

fn landed(index: usize, id: i32, created: bool) -> Landed {
    Landed { index, id, created }
}

#[test]
fn spec_order_ids_sorts_by_spec_order_when_every_landed_case_has_one() {
    let queue = vec![case_at(Some(3), None, ""), case_at(Some(1), None, ""), case_at(Some(2), None, "")];
    let got = spec_order_ids(&[landed(0, 101, true), landed(1, 102, true), landed(2, 103, false)], &queue, &[]);
    assert_eq!(got, vec![102, 103, 101]);
}

#[test]
fn spec_order_ids_falls_back_to_file_order_when_one_is_missing() {
    let queue = vec![case_at(Some(3), None, ""), case_at(None, None, ""), case_at(Some(1), None, "")];
    let got = spec_order_ids(&[landed(0, 101, true), landed(1, 102, true), landed(2, 103, true)], &queue, &[]);
    assert_eq!(got, vec![101, 102, 103]);
}

#[test]
fn spec_order_ids_only_looks_at_the_cases_that_landed() {
    // Row 1 failed and has no spec_order; it is not in `landed`, so it
    // must not push the rest back to file order.
    let queue = vec![case_at(Some(2), None, ""), case_at(None, None, ""), case_at(Some(1), None, "")];
    let got = spec_order_ids(&[landed(0, 101, true), landed(2, 103, true)], &queue, &[]);
    assert_eq!(got, vec![103, 101]);
}

#[test]
fn tester_order_cases_is_none_when_any_landed_case_lacks_one() {
    let queue = vec![case_at(None, Some(1), ""), case_at(None, None, "")];
    assert!(tester_order_cases(&[landed(0, 101, true), landed(1, 102, true)], &queue, &[]).is_none());
}

#[test]
fn tester_order_cases_sorts_and_takes_groups_from_the_area() {
    let queue = vec![
        case_at(None, Some(2), "Events / Create"),
        case_at(None, Some(1), ""),
        case_at(None, Some(3), "Events / Delete"),
    ];
    let got = tester_order_cases(&[landed(0, 101, true), landed(1, 102, false), landed(2, 103, true)], &queue, &[]).unwrap();
    assert_eq!(
        got,
        vec![
            RunOrderCase { id: 102, group: None },
            RunOrderCase { id: 101, group: Some("Events / Create".into()) },
            RunOrderCase { id: 103, group: Some("Events / Delete".into()) },
        ]
    );
}

#[test]
fn with_rest_appends_the_rest_in_suite_order_without_duplicates() {
    let first = vec![
        RunOrderCase { id: 102, group: Some("A".into()) },
        RunOrderCase { id: 101, group: None },
    ];
    let got = with_rest(first, &[50, 101, 60, 102, 70]);
    assert_eq!(
        got,
        vec![
            RunOrderCase { id: 102, group: Some("A".into()) },
            RunOrderCase { id: 101, group: None },
            RunOrderCase { id: 50, group: None },
            RunOrderCase { id: 60, group: None },
            RunOrderCase { id: 70, group: None },
        ]
    );
}

// ── settle_suite ─────────────────────────────────────────────────────────

fn entries_json(ids: &[i32]) -> serde_json::Value {
    let value: Vec<serde_json::Value> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| serde_json::json!({"suiteId": 77, "sequenceNumber": i + 1, "id": id, "suiteEntryType": "testCase"}))
        .collect();
    serde_json::json!({ "value": value })
}

/// The suite fills itself from the Tested-By links in the background: the
/// first two reads come back without the new case, the third has it.
async fn mount_late_suite(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/testplan/suiteentry/77"))
        .respond_with(ResponseTemplate::new(200).set_body_json(entries_json(&[50])))
        .up_to_n_times(2)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/testplan/suiteentry/77"))
        .respond_with(ResponseTemplate::new(200).set_body_json(entries_json(&[50, 101])))
        .mount(server)
        .await;
}

async fn gets_of(server: &MockServer, p: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method == wiremock::http::Method::GET && r.url.path() == p)
        .count()
}

#[tokio::test]
async fn settle_suite_reads_until_every_new_case_is_there() {
    let server = unshared_server().await;
    mount_late_suite(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let got = client
        .settle_suite("acme", "Web", 77, &[101], 5, Duration::ZERO)
        .await
        .unwrap();
    assert_eq!(got, vec![50, 101]);
    assert_eq!(gets_of(&server, "/acme/Web/_apis/testplan/suiteentry/77").await, 3);
}

#[tokio::test]
async fn settle_suite_gives_up_after_its_tries() {
    let server = unshared_server().await;
    mount_late_suite(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let got = client
        .settle_suite("acme", "Web", 77, &[101], 2, Duration::ZERO)
        .await
        .unwrap();
    assert_eq!(got, vec![50]);
    assert_eq!(gets_of(&server, "/acme/Web/_apis/testplan/suiteentry/77").await, 2);
}

// ── order_after_upload ───────────────────────────────────────────────────

const SUITE_PATH: &str = "/acme/Web/_apis/testplan/suiteentry/77";

async fn mount_suite(server: &MockServer, before: &[i32], after: &[i32]) {
    Mock::given(method("GET"))
        .and(path(SUITE_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(entries_json(before)))
        .mount(server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(SUITE_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(entries_json(after)))
        .mount(server)
        .await;
}

async fn mount_run_order_save(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/acme/Web/_apis/wit/attachments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "newid",
            "url": format!("{}/acme/Web/_apis/wit/attachments/newid", server.uri())
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 42, "rev": 4, "relations": []
        })))
        .mount(server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/acme/Web/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 42 })))
        .mount(server)
        .await;
}

async fn body_of(server: &MockServer, m: wiremock::http::Method, p: &str) -> Option<serde_json::Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .find(|r| r.method == m && r.url.path() == p)
        .map(|r| serde_json::from_slice(&r.body).unwrap())
}

fn patched_ids(body: &serde_json::Value) -> Vec<i64> {
    body.as_array().unwrap().iter().map(|e| e["id"].as_i64().unwrap()).collect()
}

#[tokio::test]
async fn an_upload_that_created_cases_puts_them_in_spec_order_then_the_rest() {
    let server = unshared_server().await;
    mount_suite(&server, &[50, 101, 102, 103], &[102, 103, 101, 50]).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let queue = vec![case_at(Some(3), None, ""), case_at(Some(1), None, ""), case_at(Some(2), None, "")];
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77,
        &[landed(0, 101, true), landed(1, 102, true), landed(2, 103, false)],
        &queue, &[], "me@example.com", Duration::ZERO,
    )
    .await;
    assert!(notes.is_empty(), "{notes:?}");

    let patch = body_of(&server, wiremock::http::Method::PATCH, SUITE_PATH).await.expect("a reorder PATCH");
    // The uploaded cases (created AND updated) in spec order, then the
    // case that was already in the suite, where it was relative to the rest.
    assert_eq!(patched_ids(&patch), vec![102, 103, 101, 50]);
    // No tester_order in the file: no run-order file.
    assert!(body_of(&server, wiremock::http::Method::POST, "/acme/Web/_apis/wit/attachments").await.is_none());
}

#[tokio::test]
async fn an_update_only_upload_sends_nothing() {
    let server = unshared_server().await;
    mount_suite(&server, &[50, 101], &[50, 101]).await;
    mount_run_order_save(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let queue = vec![case_at(Some(1), Some(1), "")];
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77, &[landed(0, 101, false)], &queue, &[], "me@example.com", Duration::ZERO,
    )
    .await;
    assert!(notes.is_empty(), "{notes:?}");
    assert!(server.received_requests().await.unwrap().is_empty(), "an update-only upload leaves the suite alone");
}

#[tokio::test]
async fn tester_order_saves_the_suggested_run_order_after_the_suite_order() {
    let server = unshared_server().await;
    mount_suite(&server, &[50, 101, 102], &[101, 102, 50]).await;
    mount_run_order_save(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let queue = vec![case_at(Some(1), Some(2), "Events"), case_at(Some(2), Some(1), "")];
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77,
        &[landed(0, 101, true), landed(1, 102, true)],
        &queue, &[], "me@example.com", Duration::ZERO,
    )
    .await;
    assert!(notes.is_empty(), "{notes:?}");

    let requests = server.received_requests().await.unwrap();
    let upload = requests
        .iter()
        .find(|r| r.method == wiremock::http::Method::POST && r.url.path() == "/acme/Web/_apis/wit/attachments")
        .expect("the run-order file was uploaded");
    let file: RunOrderFile = serde_json::from_slice(&upload.body).unwrap();
    assert_eq!(file.format, RUN_ORDER_FORMAT);
    assert_eq!(file.version, RUN_ORDER_VERSION);
    assert_eq!(file.saved_by, "me@example.com");
    assert!(file.saved_at.ends_with('Z'), "{}", file.saved_at);
    assert_eq!(
        file.cases,
        vec![
            RunOrderCase { id: 102, group: None },
            RunOrderCase { id: 101, group: Some("Events".into()) },
            RunOrderCase { id: 50, group: None },
        ]
    );
    assert!(
        requests.iter().any(|r| r.method == wiremock::http::Method::PATCH && r.url.path() == "/acme/Web/_apis/wit/workitems/42"),
        "the relation is written in one PATCH on the PBI"
    );
}

#[tokio::test]
async fn a_refused_reorder_is_a_note_and_no_run_order_file() {
    let server = unshared_server().await;
    Mock::given(method("GET"))
        .and(path(SUITE_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(entries_json(&[101])))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(SUITE_PATH))
        .respond_with(ResponseTemplate::new(400).set_body_string("TF400000: You cannot reorder this suite."))
        .mount(&server)
        .await;
    mount_run_order_save(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let queue = vec![case_at(Some(1), Some(1), "")];
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77, &[landed(0, 101, true)], &queue, &[], "me@example.com", Duration::ZERO,
    )
    .await;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("The spec order could not be set in Azure DevOps: "), "{}", notes[0]);
    assert!(notes[0].contains("TF400000"), "{}", notes[0]);
    assert!(!notes[0].contains("http"), "no URL or status in a user sentence: {}", notes[0]);
    assert!(body_of(&server, wiremock::http::Method::POST, "/acme/Web/_apis/wit/attachments").await.is_none());
}

#[tokio::test]
async fn a_refused_run_order_save_is_a_note_after_the_suite_was_ordered() {
    let server = unshared_server().await;
    mount_suite(&server, &[101], &[101]).await;
    Mock::given(method("POST"))
        .and(path("/acme/Web/_apis/wit/attachments"))
        .respond_with(ResponseTemplate::new(400).set_body_string("TF401000: Attachments are turned off."))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let queue = vec![case_at(Some(1), Some(1), "")];
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77, &[landed(0, 101, true)], &queue, &[], "me@example.com", Duration::ZERO,
    )
    .await;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("The suggested run order could not be saved: "), "{}", notes[0]);
    assert!(body_of(&server, wiremock::http::Method::PATCH, SUITE_PATH).await.is_some());
}

#[tokio::test]
async fn a_suite_that_never_caught_up_is_ordered_for_what_it_has_and_says_so() {
    let server = unshared_server().await;
    // 102 never arrives in the suite within the tries.
    mount_suite(&server, &[50, 101], &[101, 50]).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let queue = vec![case_at(Some(1), None, ""), case_at(Some(2), None, "")];
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77,
        &[landed(0, 101, true), landed(1, 102, true)],
        &queue, &[], "me@example.com", Duration::ZERO,
    )
    .await;
    assert_eq!(
        notes,
        vec!["Azure DevOps had not added every new test case to the suite yet, so the order was set for the ones it had. Set it in Suite Management if it looks wrong.".to_string()]
    );
    let patch = body_of(&server, wiremock::http::Method::PATCH, SUITE_PATH).await.expect("a reorder PATCH");
    assert_eq!(patched_ids(&patch), vec![101, 50]);
}

// ── unchanged rows keep their place (fix round 1) ────────────────────────

fn hint(index: u32, id: i32, spec: Option<u32>, tester: Option<u32>, area: &str) -> OrderHint {
    OrderHint { index, id, spec_order: spec, tester_order: tester, area: area.into() }
}

#[test]
fn hints_merge_into_file_order_by_their_place_in_the_queue() {
    // On screen: [unchanged 10, new 101, unchanged 12]. Only the new case
    // was sent, so the sent queue is just it, at sent index 0.
    let sent = vec![case_at(Some(2), None, "")];
    let hints = [hint(0, 10, Some(3), None, ""), hint(2, 12, Some(1), None, "")];
    assert_eq!(spec_order_ids(&[landed(0, 101, true)], &sent, &hints), vec![12, 101, 10]);

    // One spec_order missing anywhere - here on a hint - means file order
    // across the sent AND the hinted rows.
    let hints = [hint(0, 10, Some(3), None, ""), hint(2, 12, None, None, "")];
    assert_eq!(spec_order_ids(&[landed(0, 101, true)], &sent, &hints), vec![10, 101, 12]);
}

#[test]
fn a_hint_without_a_tester_order_means_no_suggested_order() {
    let sent = vec![case_at(None, Some(1), "")];
    let hints = [hint(1, 12, None, None, "")];
    assert!(tester_order_cases(&[landed(0, 101, true)], &sent, &hints).is_none());
}

#[test]
fn a_hint_carries_its_group_into_the_suggested_order() {
    let sent = vec![case_at(None, Some(1), "")];
    let hints = [hint(1, 12, None, Some(2), "Events")];
    assert_eq!(
        tester_order_cases(&[landed(0, 101, true)], &sent, &hints).unwrap(),
        vec![RunOrderCase { id: 101, group: None }, RunOrderCase { id: 12, group: Some("Events".into()) }]
    );
}

#[test]
fn with_rest_drops_repeats_within_first() {
    let first = vec![
        RunOrderCase { id: 101, group: Some("A".into()) },
        RunOrderCase { id: 102, group: None },
        RunOrderCase { id: 101, group: Some("B".into()) },
    ];
    assert_eq!(
        with_rest(first, &[102, 50]),
        vec![
            RunOrderCase { id: 101, group: Some("A".into()) },
            RunOrderCase { id: 102, group: None },
            RunOrderCase { id: 50, group: None },
        ]
    );
}

async fn uploaded_file(server: &MockServer) -> RunOrderFile {
    let upload = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.method == wiremock::http::Method::POST && r.url.path() == "/acme/Web/_apis/wit/attachments")
        .expect("the run-order file was uploaded");
    serde_json::from_slice(&upload.body).unwrap()
}

#[tokio::test]
async fn a_new_case_between_unchanged_ones_is_ordered_between_them() {
    let server = unshared_server().await;
    // The new case arrived at the end of the suite, as Azure DevOps adds it.
    mount_suite(&server, &[10, 12, 50, 101], &[10, 101, 12, 50]).await;
    mount_run_order_save(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let sent = vec![case_at(Some(2), Some(2), "")];
    let hints = [hint(0, 10, Some(1), Some(1), "A"), hint(2, 12, Some(3), Some(3), "")];
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77, &[landed(0, 101, true)], &sent, &hints, "me@example.com", Duration::ZERO,
    )
    .await;
    assert!(notes.is_empty(), "{notes:?}");

    let patch = body_of(&server, wiremock::http::Method::PATCH, SUITE_PATH).await.expect("a reorder PATCH");
    assert_eq!(patched_ids(&patch), vec![10, 101, 12, 50], "not at the top: between its neighbours");
    assert_eq!(
        uploaded_file(&server).await.cases,
        vec![
            RunOrderCase { id: 10, group: Some("A".into()) },
            RunOrderCase { id: 101, group: None },
            RunOrderCase { id: 12, group: None },
            RunOrderCase { id: 50, group: None },
        ]
    );
}

#[tokio::test]
async fn hints_alone_never_start_an_ordering() {
    let server = unshared_server().await;
    mount_suite(&server, &[10, 101], &[10, 101]).await;
    mount_run_order_save(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let sent = vec![case_at(Some(2), Some(2), "")];
    let hints = [hint(0, 10, Some(1), Some(1), "")];
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77, &[landed(0, 101, false)], &sent, &hints, "me@example.com", Duration::ZERO,
    )
    .await;
    assert!(notes.is_empty(), "{notes:?}");
    assert!(server.received_requests().await.unwrap().is_empty(), "no created case: nothing is sent");
}

#[tokio::test]
async fn the_file_lists_only_the_suites_cases_and_this_uploads_new_ones() {
    let server = unshared_server().await;
    // 900 (updated) and 901 (unchanged) live in some other suite; 101 was
    // created here but the suite has not caught up with it yet.
    mount_suite(&server, &[50], &[50]).await;
    mount_run_order_save(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let sent = vec![case_at(None, Some(1), ""), case_at(None, Some(2), "")];
    let hints = [hint(2, 901, None, Some(3), "")];
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77,
        &[landed(0, 900, false), landed(1, 101, true)],
        &sent, &hints, "me@example.com", Duration::ZERO,
    )
    .await;
    assert_eq!(notes, vec![NOTE_SUITE_BEHIND.to_string()]);
    assert_eq!(
        uploaded_file(&server).await.cases,
        vec![RunOrderCase { id: 101, group: None }, RunOrderCase { id: 50, group: None }]
    );
}

fn cached(server: &MockServer) -> EnsuredSuite {
    let s = EnsuredSuite { plan_id: 9, plan_name: "Web - Test Plan".into(), suite_id: 77, created_plan: false };
    remember_suite(&server.uri(), "acme", "Web", 42, &s);
    s
}

#[tokio::test]
async fn a_failed_suite_read_is_one_note_and_nothing_written() {
    let server = unshared_server().await;
    let suite = cached(&server);
    Mock::given(method("GET"))
        .and(path(SUITE_PATH))
        .respond_with(ResponseTemplate::new(500).set_body_string("TF246017: The server is busy."))
        .mount(&server)
        .await;
    mount_run_order_save(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77, &[landed(0, 101, true)], &[case_at(Some(1), Some(1), "")], &[],
        "me@example.com", Duration::ZERO,
    )
    .await;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("The spec order could not be set in Azure DevOps: "), "{}", notes[0]);
    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r.method == wiremock::http::Method::GET), "no PATCH, no POST");
    // A server error says nothing about the suite existing: keep it.
    assert_eq!(cached_suite(&server.uri(), "acme", "Web", 42), Some(suite));
}

#[tokio::test]
async fn a_deleted_suite_is_forgotten_so_the_next_upload_resolves_it_again() {
    let server = unshared_server().await;
    cached(&server);
    Mock::given(method("GET"))
        .and(path(SUITE_PATH))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let notes = order_after_upload(
        &client, "acme", "Web", 42, 77, &[landed(0, 101, true)], &[case_at(Some(1), None, "")], &[],
        "me@example.com", Duration::ZERO,
    )
    .await;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("The spec order could not be set in Azure DevOps: "), "{}", notes[0]);
    assert!(cached_suite(&server.uri(), "acme", "Web", 42).is_none());
}
