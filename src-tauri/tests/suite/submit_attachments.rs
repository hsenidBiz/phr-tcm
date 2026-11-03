//! The queue upload with step attachments: files are uploaded before the
//! batch, a row whose file cannot be sent fails alone, rows without
//! attachment work are sent exactly as before, and the per-row results
//! carry the new files' ids and any notes.

use serde_json::json;
use std::collections::HashMap;
use v2_lib::ado::wit_batch::BatchItem;
use v2_lib::ado::AdoClient;
use v2_lib::commands::queue::{
    map_batch_results, plan_chunk, read_queue_relations, QueueContext, SubmitItemResult,
};
use v2_lib::model::TestCase;
use v2_lib::step_attachments::StepAttachment;
use v2_lib::steps_xml::Step;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> AdoClient {
    AdoClient::with_base_urls("tok".into(), server.uri(), server.uri())
}

fn step(a: &str) -> Step {
    Step { action: a.into(), expected: "ok".into(), ..Default::default() }
}

fn attach_path(p: &std::path::Path) -> StepAttachment {
    StepAttachment {
        path: Some(p.to_string_lossy().into_owned()),
        name: p.file_name().unwrap().to_string_lossy().into_owned(),
        ..Default::default()
    }
}

fn case(title: &str, update_id: Option<i32>, steps: Vec<Step>) -> TestCase {
    TestCase {
        title: title.into(),
        steps,
        automation_status: "Not Automated".into(),
        update_id,
        ..Default::default()
    }
}

fn temp_file(tag: &str, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("tcm-v2-submitatt-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join(name);
    std::fs::write(&f, bytes).unwrap();
    f
}

struct Held {
    steps: HashMap<i32, String>,
    tags: HashMap<i32, String>,
    prereq: HashMap<i32, v2_lib::ado::StoredText>,
    relations: HashMap<i32, v2_lib::step_attachments::CaseRelations>,
}

impl Held {
    fn none() -> Self {
        Held { steps: HashMap::new(), tags: HashMap::new(), prereq: HashMap::new(), relations: HashMap::new() }
    }
    fn ctx(&self) -> QueueContext<'_> {
        QueueContext {
            organization: "o",
            project: "p",
            pbi_id: 42,
            module_ref: None,
            preconditions_ref: None,
            area_path: "",
            iteration_path: "",
            steps_before: &self.steps,
            tags_before: &self.tags,
            prereq_before: &self.prereq,
            relations: &self.relations,
        }
    }
}

async fn mount_upload(server: &MockServer, guid: &str) {
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/attachments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(
            json!({"id": guid, "url": format!("{}/o/_apis/wit/attachments/{guid}", server.uri())}),
        ))
        .mount(server)
        .await;
}

#[tokio::test]
async fn missing_file_fails_row_before_sending() {
    let server = MockServer::start().await;
    mount_upload(&server, "g1").await;
    let gone = std::env::temp_dir().join("tcm-v2-submitatt-nowhere").join("login.png");
    let mut bad = case("Has a missing file", None, vec![step("one")]);
    bad.steps[0].attachments.push(attach_path(&gone));
    let fine = case("Plain", None, vec![step("one")]);
    let queue = vec![bad, fine];
    let held = Held::none();
    let plan = plan_chunk(&client(&server), &held.ctx(), &queue, 0..2).await;
    assert_eq!(plan.failed.len(), 1);
    assert_eq!(plan.failed[0].index, 0);
    assert_eq!(plan.failed[0].action, "failed");
    let msg = plan.failed[0].error.clone().unwrap();
    assert!(msg.contains("login.png"), "{msg}");
    assert!(!msg.contains("http") && !msg.contains(&*gone.to_string_lossy()), "{msg}");
    assert_eq!(plan.sent_idx, vec![1], "only row 1 is sent");
    assert_eq!(plan.reqs.len(), 1);
    assert!(server.received_requests().await.unwrap().is_empty(), "nothing went to Azure DevOps");
}

#[tokio::test]
async fn old_queue_update_sends_no_relation_ops() {
    let server = MockServer::start().await;
    let queue = vec![case("Old update", Some(77), vec![step("one")]), case("New", None, vec![step("one")])];
    let client = client(&server);
    let read = read_queue_relations(&client, "o", &queue).await;
    assert!(read.found.is_empty() && !read.failed);
    let mut held = Held::none();
    held.relations = read.found;
    let plan = plan_chunk(&client, &held.ctx(), &queue, 0..2).await;
    assert_eq!(plan.reqs.len(), 2);
    for r in &plan.reqs {
        let body = r.body.to_string();
        assert!(!body.contains("AttachedFile"), "{body}");
        if r.uri.contains("77") {
            assert!(!body.contains("/relations/"), "{body}");
        }
    }
    assert!(plan.notes.is_empty());
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "no relations read, no upload for a queue without attachment work"
    );
}

#[tokio::test]
async fn created_row_reports_attached() {
    let server = MockServer::start().await;
    mount_upload(&server, "guid-9").await;
    let f = temp_file("created", "login.png", b"png-bytes");
    let mut tc = case("Has a file", None, vec![step("one"), step("two")]);
    tc.steps[1].attachments.push(attach_path(&f));
    let queue = vec![tc];
    let held = Held::none();
    let plan = plan_chunk(&client(&server), &held.ctx(), &queue, 0..1).await;
    assert!(plan.failed.is_empty());
    let body = plan.reqs[0].body.to_string();
    assert!(body.contains("\"rel\":\"AttachedFile\"") && body.contains("[TestStep=3]:"), "{body}");
    assert!(!body.contains("\"op\":\"test\""), "a create needs no index guard: {body}");
    let items = vec![BatchItem { code: 200, body: json!({"id": 901}) }];
    let mut results = map_batch_results(&queue, &plan.sent_idx, &items);
    plan.apply(&mut results, &items);
    assert_eq!(results[0].action, "created");
    assert_eq!(results[0].attached.len(), 1);
    let at = &results[0].attached[0];
    assert_eq!((at.step, at.id.as_str(), at.name.as_str()), (1, "guid-9", "login.png"));
    assert_eq!(at.path, f.to_string_lossy());
    assert!(results[0].error.is_none());
}

#[tokio::test]
async fn relations_read_failure_notes_row() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    let f = temp_file("readfail", "x.png", b"x");
    let mut tc = case("Update with a file", Some(77), vec![step("one")]);
    tc.steps[0].attachments.push(attach_path(&f));
    let queue = vec![tc];
    let client = client(&server);
    let read = read_queue_relations(&client, "o", &queue).await;
    assert!(read.failed && read.found.is_empty());
    let mut held = Held::none();
    held.relations = read.found;
    let plan = plan_chunk(&client, &held.ctx(), &queue, 0..1).await;
    assert_eq!(plan.reqs.len(), 1, "the rest of the update still goes");
    assert!(!plan.reqs[0].body.to_string().contains("/relations/"));
    let posted = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "POST")
        .count();
    assert_eq!(posted, 0, "no file is uploaded when its relations could not be read");
    let items = vec![BatchItem { code: 200, body: json!({"id": 77}) }];
    let mut results = map_batch_results(&queue, &plan.sent_idx, &items);
    plan.apply(&mut results, &items);
    assert_eq!(results[0].action, "updated");
    let note = results[0].error.clone().unwrap();
    assert!(note.contains("attachments were not changed"), "{note}");
    assert!(!note.to_lowercase().contains("http"), "{note}");
    assert!(results[0].attached.is_empty());
}

#[tokio::test]
async fn failed_rev_guard_reads_plainly() {
    use v2_lib::step_attachments::{CaseRelation, CaseRelations};
    let server = MockServer::start().await;
    let mut tc = case("Removes one", Some(77), vec![step("one")]);
    tc.removed_attachments = vec!["old-guid".into()];
    let queue = vec![tc];
    let mut held = Held::none();
    held.relations.insert(
        77,
        CaseRelations {
            rev: 5,
            attachments: vec![CaseRelation {
                index: 3,
                guid: "old-guid".into(),
                name: "old.png".into(),
                size: None,
                comment: "[TestStep=2]:".into(),
            }],
        },
    );
    let plan = plan_chunk(&client(&server), &held.ctx(), &queue, 0..1).await;
    assert!(plan.reqs[0].body.to_string().contains("\"op\":\"test\""));
    let items = vec![BatchItem { code: 412, body: json!({"message": "TF401232: Work item 77 has been changed"}) }];
    let mut results = map_batch_results(&queue, &plan.sent_idx, &items);
    plan.apply(&mut results, &items);
    let msg = results[0].error.clone().unwrap();
    assert_eq!(results[0].action, "failed");
    assert!(msg.contains("changed in Azure DevOps") && msg.contains("Upload it again"), "{msg}");
}

#[test]
fn result_always_carries_the_attached_list() {
    let r = SubmitItemResult {
        index: 0,
        title: "t".into(),
        action: "created".into(),
        id: Some(1),
        error: None,
        attached: vec![],
    };
    assert_eq!(serde_json::to_value(&r).unwrap()["attached"], json!([]));
}
