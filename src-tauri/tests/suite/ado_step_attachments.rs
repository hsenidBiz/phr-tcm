//! Reading step attachments off Azure DevOps and uploading step files.
//!
//! GET and POST only; the DELETE scan in `ado.rs` covers `ado/attachments.rs`
//! because it walks every `.rs` under `src/`.

use serde_json::json;
use std::collections::HashMap;
use v2_lib::ado::attachments::{fill_step_attachments, read_step_file};
use v2_lib::ado::{AdoClient, TestCaseFull};
use v2_lib::step_attachments::{CaseRelation, CaseRelations, StepAttachment, STEP_ATTACHMENT_MAX_BYTES};
use v2_lib::steps_xml::Step;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn case(id: i32, step_ids: &[&str]) -> TestCaseFull {
    TestCaseFull {
        id,
        title: "t".into(),
        tags: String::new(),
        automation_status: "Not Automated".into(),
        steps: step_ids
            .iter()
            .map(|_| Step { action: "a".into(), expected: "e".into(), ..Default::default() })
            .collect(),
        steps_legacy: None,
        step_ids: step_ids.iter().map(|s| s.to_string()).collect(),
        steps_xml: String::new(),
        module_value: String::new(),
        preconditions: String::new(),
        preconditions_legacy: None,
        preconditions_stored: None,
        priority: None,
        attachments_read: false,
    }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("tcm-v2-stepatt-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn case_relations_reads_attached_files_with_indexes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/_apis/wit/workitems"))
        .and(query_param("$expand", "relations"))
        .and(query_param("errorPolicy", "omit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": [{
                "id": 10,
                "rev": 7,
                "relations": [
                    {"rel": "Microsoft.VSTS.Common.TestedBy-Reverse", "url": "https://x/_apis/wit/workItems/5", "attributes": {}},
                    {"rel": "AttachedFile", "url": "https://x/_apis/wit/attachments/aaaa-1",
                     "attributes": {"name": "shot.png", "resourceSize": 321, "comment": "[TestStep=2]:look"}}
                ]
            }]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let got = client.case_relations("o", &[10]).await.unwrap();
    let r = got.get(&10).expect("case 10");
    assert_eq!(r.rev, 7);
    assert_eq!(r.attachments.len(), 1);
    let a = &r.attachments[0];
    assert_eq!(a.index, 1);
    assert_eq!(a.guid, "aaaa-1");
    assert_eq!(a.name, "shot.png");
    assert_eq!(a.size, Some(321));
    assert_eq!(a.comment, "[TestStep=2]:look");
}

fn relation(index: usize, guid: &str, comment: &str) -> CaseRelation {
    CaseRelation { index, guid: guid.into(), name: format!("{guid}.png"), size: Some(9), comment: comment.into() }
}

#[test]
fn fill_puts_attachment_on_its_step() {
    let mut cases = vec![case(1, &["2", "3"]), case(2, &["2"])];
    let mut rel = HashMap::new();
    rel.insert(
        1,
        CaseRelations {
            rev: 4,
            attachments: vec![
                relation(0, "g-on-3", "[TestStep=3]:note"),
                relation(1, "g-no-marker", "added by hand"),
                relation(2, "g-unknown-step", "[TestStep=99]:"),
            ],
        },
    );
    fill_step_attachments(&mut cases, &rel);
    assert!(cases[0].steps[0].attachments.is_empty());
    assert_eq!(cases[0].steps[1].attachments.len(), 1);
    let a = &cases[0].steps[1].attachments[0];
    assert_eq!(a.id.as_deref(), Some("g-on-3"));
    assert_eq!(a.name, "g-on-3.png");
    assert_eq!(a.size, Some(9));
    assert_eq!(a.comment, "note");
    assert!(cases[0].attachments_read);
    // Case 2 was not in the map: not read.
    assert!(!cases[1].attachments_read);
}

#[test]
fn read_step_file_refuses_over_cap_and_unc() {
    let dir = temp_dir("cap");
    let big = dir.join("big.bin");
    let f = std::fs::File::create(&big).unwrap();
    f.set_len(STEP_ATTACHMENT_MAX_BYTES + 1).unwrap();
    drop(f);
    let e = read_step_file(big.to_str().unwrap()).unwrap_err();
    assert!(e.contains("big.bin"), "{e}");
    assert!(!e.to_lowercase().contains("http"), "{e}");

    let e = read_step_file("\\\\server\\share\\x.png").unwrap_err();
    assert!(!e.to_lowercase().contains("http"), "{e}");

    let e = read_step_file(dir.join("gone.png").to_str().unwrap()).unwrap_err();
    assert!(e.contains("gone.png"), "{e}");

    let e = read_step_file(dir.to_str().unwrap()).unwrap_err();
    assert!(!e.is_empty());

    let ok = dir.join("ok.txt");
    std::fs::write(&ok, b"hello").unwrap();
    let (name, bytes) = read_step_file(ok.to_str().unwrap()).unwrap();
    assert_eq!(name, "ok.txt");
    assert_eq!(bytes, b"hello");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn upload_step_files_posts_octet_stream_once_per_path() {
    let dir = temp_dir("up");
    let a = dir.join("a.png");
    let b = dir.join("b.log");
    std::fs::write(&a, b"AAA").unwrap();
    std::fs::write(&b, b"BBB").unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/attachments"))
        .and(query_param("fileName", "a.png"))
        .respond_with(ResponseTemplate::new(201).set_body_json(
            json!({"id": "guid-a", "url": "https://x/_apis/wit/attachments/guid-a?fileName=a.png"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/attachments"))
        .and(query_param("fileName", "b.log"))
        .respond_with(ResponseTemplate::new(201).set_body_json(
            json!({"id": "guid-b", "url": "https://x/_apis/wit/attachments/guid-b?fileName=b.log"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let att = |p: &std::path::Path, n: &str| StepAttachment {
        path: Some(p.to_str().unwrap().to_string()),
        name: n.into(),
        ..Default::default()
    };
    // a.png appears on two steps: uploaded once.
    let steps = vec![
        Step { attachments: vec![att(&a, "a.png"), att(&b, "b.log")], ..Default::default() },
        Step { attachments: vec![att(&a, "a.png")], ..Default::default() },
    ];
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let up = client.upload_step_files("o", "p", &steps).await.unwrap();
    assert_eq!(up.len(), 2);
    let ua = up.iter().find(|u| u.name == "a.png").unwrap();
    assert_eq!(ua.guid, "guid-a");
    assert!(ua.url.contains("guid-a"));
    assert_eq!(ua.path, a.to_str().unwrap());
    let ub = up.iter().find(|u| u.name == "b.log").unwrap();
    assert_eq!(ub.guid, "guid-b");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn upload_step_files_first_failure_names_the_file_not_a_url() {
    let dir = temp_dir("fail");
    let gone = dir.join("vanished.png");
    let server = MockServer::start().await;
    let steps = vec![Step {
        attachments: vec![StepAttachment {
            path: Some(gone.to_str().unwrap().to_string()),
            name: "vanished.png".into(),
            ..Default::default()
        }],
        ..Default::default()
    }];
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let e = client.upload_step_files("o", "p", &steps).await.unwrap_err();
    assert!(e.contains("vanished.png"), "{e}");
    assert!(!e.to_lowercase().contains("http"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn attachments_module_issues_no_delete() {
    let src = include_str!("../../src/ado/attachments.rs");
    assert!(!src.contains(".delete(") && !src.contains("Method::DELETE"));
}

// ---- Task 3: filling is idempotent; the save flow ----

#[test]
fn fill_twice_does_not_duplicate() {
    let mut cases = vec![case(1, &["2", "3"])];
    let mut rel = HashMap::new();
    rel.insert(1, CaseRelations { rev: 4, attachments: vec![relation(0, "g3", "[TestStep=3]:note")] });
    fill_step_attachments(&mut cases, &rel);
    fill_step_attachments(&mut cases, &rel);
    assert_eq!(cases[0].steps[1].attachments.len(), 1);
}

use v2_lib::ado::BlankPolicy;
use v2_lib::model::TestCase;

fn tc_with(steps: Vec<Step>) -> TestCase {
    TestCase { title: "T".into(), steps, update_id: Some(55), ..Default::default() }
}

fn plain_step() -> Step {
    Step { action: "a".into(), expected: "e".into(), ..Default::default() }
}

fn patch_bodies(reqs: &[wiremock::Request]) -> Vec<serde_json::Value> {
    reqs.iter()
        .filter(|r| r.method.as_str() == "PATCH")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

async fn mount_relations(server: &MockServer, relations: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [{"id": 55, "rev": 7, "relations": relations}]})))
        .mount(server)
        .await;
}

async fn mount_patch_ok(server: &MockServer) {
    Mock::given(method("PATCH")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 55}))).mount(server).await;
}

#[tokio::test]
async fn save_without_attachment_work_reads_no_relations() {
    let server = MockServer::start().await;
    mount_patch_ok(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let tc = tc_with(vec![plain_step()]);
    let notes = client
        .update_test_case_with_attachments("org", "proj", 55, &tc, None, None, None, None, None, BlankPolicy::Skip)
        .await
        .unwrap();
    assert!(notes.is_empty());
    let reqs = server.received_requests().await.unwrap();
    assert!(reqs.iter().all(|r| r.method.as_str() != "GET"), "no relations read");
    assert!(!serde_json::to_string(&patch_bodies(&reqs)).unwrap().contains("/relations/"));
}

#[tokio::test]
async fn save_removes_a_removed_attachment_with_a_rev_guard() {
    let server = MockServer::start().await;
    mount_relations(&server, json!([
        {"rel": "AttachedFile", "url": "https://x/_apis/wit/attachments/g1", "attributes": {"name": "a.png", "comment": "[TestStep=2]:"}}
    ])).await;
    mount_patch_ok(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let mut tc = tc_with(vec![plain_step()]);
    tc.removed_attachments = vec!["g1".into()];
    client
        .update_test_case_with_attachments("org", "proj", 55, &tc, None, None, None, None, None, BlankPolicy::Skip)
        .await
        .unwrap();
    let reqs = server.received_requests().await.unwrap();
    assert!(reqs.iter().all(|r| r.method.as_str() != "DELETE"));
    let body = patch_bodies(&reqs).pop().unwrap();
    let ops = body.as_array().unwrap();
    let t = ops.iter().position(|o| o["op"] == "test" && o["path"] == "/rev" && o["value"] == 7).expect("test /rev");
    let r = ops.iter().position(|o| o["op"] == "remove" && o["path"] == "/relations/0").expect("remove");
    assert!(t < r);
}

#[tokio::test]
async fn save_uploads_new_file_then_links_it() {
    let dir = temp_dir("save");
    let f = dir.join("new.png");
    std::fs::write(&f, b"png").unwrap();
    let server = MockServer::start().await;
    mount_relations(&server, json!([])).await;
    Mock::given(method("POST")).and(path("/org/proj/_apis/wit/attachments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": "gnew", "url": "https://x/_apis/wit/attachments/gnew"})))
        .mount(&server).await;
    mount_patch_ok(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let att = StepAttachment { path: Some(f.to_str().unwrap().into()), name: "new.png".into(), ..Default::default() };
    let tc = tc_with(vec![Step { attachments: vec![att], ..plain_step() }]);
    client
        .update_test_case_with_attachments("org", "proj", 55, &tc, None, None, None, None, None, BlankPolicy::Skip)
        .await
        .unwrap();
    let reqs = server.received_requests().await.unwrap();
    let body = patch_bodies(&reqs).pop().unwrap();
    let add = body.as_array().unwrap().iter().find(|o| o["path"] == "/relations/-").expect("add");
    assert_eq!(add["value"]["url"], "https://x/_apis/wit/attachments/gnew");
    assert_eq!(add["value"]["attributes"]["comment"], "[TestStep=2]:");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn save_with_missing_file_sends_nothing() {
    let server = MockServer::start().await;
    mount_relations(&server, json!([])).await;
    mount_patch_ok(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let dir = temp_dir("missing");
    let att = StepAttachment { path: Some(dir.join("gone.png").to_str().unwrap().into()), name: "gone.png".into(), ..Default::default() };
    let tc = tc_with(vec![Step { attachments: vec![att], ..plain_step() }]);
    let err = client
        .update_test_case_with_attachments("org", "proj", 55, &tc, None, None, None, None, None, BlankPolicy::Skip)
        .await
        .unwrap_err();
    assert!(err.contains("gone.png"), "{err}");
    let reqs = server.received_requests().await.unwrap();
    assert!(patch_bodies(&reqs).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn relations_read_failure_still_saves_and_says_so() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    mount_patch_ok(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let mut tc = tc_with(vec![plain_step()]);
    tc.removed_attachments = vec!["g1".into()];
    let notes = client
        .update_test_case_with_attachments("org", "proj", 55, &tc, None, None, None, None, None, BlankPolicy::Skip)
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert!(!notes[0].to_lowercase().contains("http"), "{notes:?}");
    let body = patch_bodies(&server.received_requests().await.unwrap()).pop().unwrap();
    assert!(!body.to_string().contains("/relations/"));
}

#[tokio::test]
async fn update_command_rev_mismatch_message_has_no_url() {
    let server = MockServer::start().await;
    mount_relations(&server, json!([
        {"rel": "AttachedFile", "url": "https://x/_apis/wit/attachments/g1", "attributes": {"name": "a.png", "comment": "[TestStep=2]:"}}
    ])).await;
    Mock::given(method("PATCH"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"message": "Test operation failed at https://dev.azure.com/secret/path"})))
        .mount(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let mut tc = tc_with(vec![plain_step()]);
    tc.removed_attachments = vec!["g1".into()];
    let err = client
        .update_test_case_with_attachments("org", "proj", 55, &tc, None, None, None, None, None, BlankPolicy::Skip)
        .await
        .unwrap_err();
    assert!(err.contains("changed in Azure DevOps"), "{err}");
    assert!(!err.to_lowercase().contains("http"), "{err}");
}
