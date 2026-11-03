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
