//! Step attachments: the model, the `[TestStep=N]:` marker and the pure
//! relation-ops function.
//!
//! UNCONFIRMED: the exact relation comment Azure DevOps' web form writes for
//! a step attachment (`[TestStep=N]:` + optional text) has NOT been checked
//! against a live case; the format lives in one place
//! (`step_attachments::marker` / `parse_marker`). Confirm by reading one case
//! that has a step attachment added through the ADO web form
//! (`?$expand=relations`, GET only) and, if it differs, change only those two.

use serde_json::json;
use v2_lib::model::TestCase;
use v2_lib::step_attachments::{
    attachment_guid, has_attachment_work, marker, parse_marker, relation_ops, CaseRelation,
    CaseRelations, StepAttachment, Uploaded,
};
use v2_lib::steps_xml::Step;

fn step(action: &str, atts: Vec<StepAttachment>) -> Step {
    Step { action: action.into(), expected: String::new(), attachments: atts, ..Default::default() }
}

fn new_file(path: &str) -> StepAttachment {
    StepAttachment {
        path: Some(path.into()),
        name: path.rsplit(['\\', '/']).next().unwrap().into(),
        ..Default::default()
    }
}

fn existing(guid: &str) -> StepAttachment {
    StepAttachment { id: Some(guid.into()), name: format!("{guid}.png"), ..Default::default() }
}

fn rel(index: usize, guid: &str, comment: &str) -> CaseRelation {
    CaseRelation {
        index,
        guid: guid.into(),
        name: format!("{guid}.png"),
        size: None,
        comment: comment.into(),
    }
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn uploaded(path: &str, guid: &str) -> Uploaded {
    Uploaded {
        path: path.into(),
        guid: guid.into(),
        url: format!("https://x/_apis/wit/attachments/{guid}"),
        name: path.rsplit(['\\', '/']).next().unwrap().into(),
    }
}

#[test]
fn marker_round_trips() {
    assert_eq!(marker("3", ""), "[TestStep=3]:");
    assert_eq!(marker("3", "see log"), "[TestStep=3]:see log");
    assert_eq!(parse_marker("[TestStep=3]:see log"), Some(("3".to_string(), "see log".to_string())));
    assert_eq!(parse_marker("  [TestStep=12]:"), Some(("12".to_string(), String::new())));
    assert_eq!(parse_marker("Linked by X"), None);
}

#[test]
fn guid_from_url() {
    assert_eq!(
        attachment_guid("https://x/_apis/wit/attachments/aaaa-1?fileName=a.png"),
        Some("aaaa-1".to_string())
    );
    assert_eq!(attachment_guid("https://x/_apis/wit/attachments/bbbb"), Some("bbbb".to_string()));
}

#[test]
fn no_attachment_work_means_no_ops() {
    let cur = CaseRelations {
        rev: 3,
        attachments: vec![rel(0, "g1", "[TestStep=2]:"), rel(1, "g2", "[TestStep=3]:")],
    };
    let steps = vec![step("a", vec![]), step("b", vec![])];
    let out = relation_ops(&ids(&["2", "3"]), &steps, &[], Some(&cur), &[]);
    assert!(out.ops.is_empty());
    assert!(out.notes.is_empty());
}

#[test]
fn new_file_on_create_adds_relation() {
    let steps = vec![step("a", vec![]), step("b", vec![new_file("C:\\a.png")])];
    let up = vec![uploaded("C:\\a.png", "g9")];
    let out = relation_ops(&ids(&["2", "3"]), &steps, &[], None, &up);
    assert_eq!(out.ops.len(), 1);
    let op = &out.ops[0];
    assert_eq!(op["op"], "add");
    assert_eq!(op["path"], "/relations/-");
    assert_eq!(op["value"]["rel"], "AttachedFile");
    assert_eq!(op["value"]["url"], "https://x/_apis/wit/attachments/g9");
    assert_eq!(op["value"]["attributes"]["name"], "a.png");
    assert_eq!(op["value"]["attributes"]["comment"], "[TestStep=3]:");
    assert!(out.ops.iter().all(|o| o["op"] != "test"));
}

#[test]
fn moved_and_edited_step_repoints_comment() {
    let cur = CaseRelations { rev: 5, attachments: vec![rel(4, "g1", "[TestStep=2]:note")] };
    let steps = vec![step("a", vec![existing("g1")])];
    let out = relation_ops(&ids(&["9"]), &steps, &[], Some(&cur), &[]);
    assert_eq!(
        out.ops,
        vec![
            json!({"op":"test","path":"/rev","value":5}),
            json!({"op":"replace","path":"/relations/4/attributes/comment","value":"[TestStep=9]:note"}),
        ]
    );
}

#[test]
fn carried_on_the_right_step_writes_nothing() {
    let cur = CaseRelations { rev: 5, attachments: vec![rel(4, "g1", "[TestStep=5]:note")] };
    let steps = vec![step("a", vec![]), step("b", vec![existing("g1")])];
    let out = relation_ops(&ids(&["2", "5"]), &steps, &[], Some(&cur), &[]);
    assert!(out.ops.is_empty());
}

#[test]
fn removed_guid_is_removed_desc() {
    let cur = CaseRelations {
        rev: 8,
        attachments: vec![
            rel(1, "g1", "[TestStep=2]:"),
            rel(3, "g2", "[TestStep=2]:"),
            rel(5, "g3", "[TestStep=2]:"),
        ],
    };
    let steps = vec![step("a", vec![existing("g2")])];
    let out = relation_ops(&ids(&["2"]), &steps, &["g1".into(), "g3".into()], Some(&cur), &[]);
    assert_eq!(
        out.ops,
        vec![
            json!({"op":"test","path":"/rev","value":8}),
            json!({"op":"remove","path":"/relations/5"}),
            json!({"op":"remove","path":"/relations/1"}),
        ]
    );
}

#[test]
fn carried_and_removed_keeps() {
    let cur = CaseRelations { rev: 8, attachments: vec![rel(1, "g1", "[TestStep=2]:")] };
    let steps = vec![step("a", vec![existing("g1")])];
    let out = relation_ops(&ids(&["2"]), &steps, &["g1".into()], Some(&cur), &[]);
    assert!(out.ops.is_empty());
}

#[test]
fn carried_guid_missing_on_item_notes() {
    let cur = CaseRelations { rev: 8, attachments: vec![rel(1, "g1", "[TestStep=2]:")] };
    let steps = vec![step("a", vec![existing("gone")])];
    let out = relation_ops(&ids(&["2"]), &steps, &[], Some(&cur), &[]);
    assert!(out.ops.is_empty());
    assert_eq!(out.notes.len(), 1);
    assert!(!out.notes[0].contains("http"));
}

#[test]
fn existing_guid_on_create_notes() {
    let steps = vec![step("a", vec![existing("g")])];
    let out = relation_ops(&ids(&["2"]), &steps, &[], None, &[]);
    assert!(out.ops.is_empty());
    assert_eq!(out.notes.len(), 1);
}

#[test]
fn mixed_ops_come_in_the_documented_order() {
    let cur = CaseRelations {
        rev: 2,
        attachments: vec![
            rel(0, "g1", "[TestStep=2]:"),
            rel(1, "g2", "[TestStep=2]:"),
            rel(2, "g3", "[TestStep=2]:"),
        ],
    };
    // g3 moved to step id 7 (replace), g1 removed, one new file added.
    let steps = vec![
        step("a", vec![existing("g2")]),
        step("b", vec![existing("g3"), new_file("C:\\n.txt")]),
    ];
    let up = vec![uploaded("C:\\n.txt", "gn")];
    let out = relation_ops(&ids(&["2", "7"]), &steps, &["g1".into()], Some(&cur), &up);
    let kinds: Vec<&str> = out.ops.iter().map(|o| o["op"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["test", "replace", "remove", "add"]);
}

#[test]
fn old_json_step_has_no_attachments_key() {
    let s = Step { action: "a".into(), expected: "b".into(), ..Default::default() };
    let v = serde_json::to_value(&s).unwrap();
    assert!(v.get("attachments").is_none());
    let tc = TestCase { title: "t".into(), steps: vec![s], ..Default::default() };
    let v = serde_json::to_value(&tc).unwrap();
    assert!(v.get("removed_attachments").is_none());
    // And an old file still reads.
    let back: Step = serde_json::from_value(json!({"action":"a","expected":"b"})).unwrap();
    assert!(back.attachments.is_empty());
}

#[test]
fn attachment_work_is_detected() {
    let mut tc = TestCase { title: "t".into(), steps: vec![step("a", vec![])], ..Default::default() };
    assert!(!has_attachment_work(&tc));
    tc.removed_attachments = vec!["g".into()];
    assert!(has_attachment_work(&tc));
    tc.removed_attachments.clear();
    tc.steps[0].attachments = vec![new_file("C:\\a.png")];
    assert!(has_attachment_work(&tc));
}

#[test]
fn shared_step_with_attachment_is_invalid() {
    let tc = TestCase {
        title: "t".into(),
        automation_status: "Not Automated".into(),
        steps: vec![Step {
            shared: Some(10),
            attachments: vec![new_file("C:\\a.png")],
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(tc.is_valid().is_err());
    let ok = TestCase {
        title: "t".into(),
        automation_status: "Not Automated".into(),
        steps: vec![Step { shared: Some(10), ..Default::default() }],
        ..Default::default()
    };
    assert!(ok.is_valid().is_ok());
}

#[test]
fn a_case_level_attachment_is_never_re_pointed() {
    // No `[TestStep=` marker: a case-level attachment. A hand-written draft
    // naming its GUID on a step must not mark it.
    let cur = CaseRelations { rev: 5, attachments: vec![rel(2, "case-level", "uploaded by a person")] };
    let steps = vec![step("a", vec![existing("case-level")])];
    let out = relation_ops(&ids(&["2"]), &steps, &[], Some(&cur), &[]);
    assert!(out.ops.is_empty(), "{:?}", out.ops);
    assert_eq!(out.notes.len(), 1);
    assert!(!out.notes[0].to_lowercase().contains("http"));
}

#[test]
fn a_case_level_attachment_is_never_removed() {
    let cur = CaseRelations {
        rev: 5,
        attachments: vec![rel(2, "case-level", "uploaded by a person"), rel(3, "on-step", "[TestStep=2]:")],
    };
    let steps = vec![step("a", vec![])];
    let removed = vec!["case-level".to_string(), "on-step".to_string()];
    let out = relation_ops(&ids(&["2"]), &steps, &removed, Some(&cur), &[]);
    // Only the step's own attachment goes.
    assert_eq!(
        out.ops,
        vec![
            json!({"op": "test", "path": "/rev", "value": 5}),
            json!({"op": "remove", "path": "/relations/3"}),
        ]
    );
    assert_eq!(out.notes.len(), 1);
    assert!(!out.notes[0].to_lowercase().contains("http"));
}

// ---- Task 3: the ops inside the create and update documents ----

use v2_lib::ado::{AdoClient, BlankPolicy};
use v2_lib::ado::endpoints::final_step_ids;
use v2_lib::step_attachments::AttachmentInput;

fn doc_client() -> AdoClient {
    AdoClient::with_base_urls("tok".into(), "http://ado.invalid".into(), "http://ado.invalid".into())
}

fn case_of(steps: Vec<Step>) -> TestCase {
    TestCase { title: "T".into(), steps, ..Default::default() }
}

fn relation_paths(ops: &[serde_json::Value]) -> Vec<String> {
    ops.iter()
        .filter_map(|o| o["path"].as_str())
        .filter(|p| p.starts_with("/relations") && *p != "/relations/-")
        .map(String::from)
        .collect()
}

#[test]
fn create_doc_links_new_file_to_built_step_id() {
    let tc = case_of(vec![step("one", vec![]), step("two", vec![new_file("C:/a.png")])]);
    let up = [Uploaded { path: "C:/a.png".into(), guid: "g".into(), url: "https://x/_apis/wit/attachments/g".into(), name: "a.png".into() }];
    let (ops, notes) = doc_client().create_test_case_doc(
        "org", "Web", &tc, None, "", "", None, None,
        &AttachmentInput { current: None, uploaded: &up, skip: false },
    );
    assert!(notes.is_empty(), "{notes:?}");
    let add = ops.iter().find(|o| o["path"] == "/relations/-").expect("a relation add");
    assert_eq!(add["value"]["attributes"]["comment"], "[TestStep=3]:");
    assert_eq!(add["value"]["attributes"]["name"], "a.png");
    assert!(!ops.iter().any(|o| o["op"] == "test"));
}

#[test]
fn unchanged_steps_use_original_ids() {
    let steps = vec![step("one", vec![]), step("two", vec![existing("g5")])];
    let original = v2_lib::steps_xml::build_steps_xml(&steps).replace("id=\"3\"", "id=\"5\"");
    assert_eq!(final_step_ids(&steps, Some(&original)), vec!["2".to_string(), "5".to_string()]);
    let tc = case_of(steps);
    let cur = CaseRelations { rev: 9, attachments: vec![rel(1, "g5", "[TestStep=5]:")] };
    let (ops, notes) = doc_client().update_test_case_doc(
        &tc, None, None, Some(&original), None, None, BlankPolicy::Skip,
        &AttachmentInput { current: Some(&cur), uploaded: &[], skip: false },
    );
    assert!(notes.is_empty(), "{notes:?}");
    assert!(relation_paths(&ops).is_empty(), "{ops:?}");
    assert!(!ops.iter().any(|o| o["op"] == "test"));
}

#[test]
fn update_doc_appends_repoint_and_remove_after_test_rev() {
    // Step 2 is edited, so the merge gives it a fresh id; the relation
    // that said another id now has to say what the written XML says.
    let steps = vec![step("one", vec![]), step("two edited", vec![existing("g1")])];
    let original = v2_lib::steps_xml::build_steps_xml(&[step("one", vec![]), step("two", vec![])]);
    let ids = final_step_ids(&steps, Some(&original));
    assert_eq!(ids.len(), 2);
    let mut tc = case_of(steps);
    tc.removed_attachments = vec!["g2".into()];
    let cur = CaseRelations {
        rev: 9,
        attachments: vec![rel(1, "g1", "[TestStep=9999]:note"), rel(3, "g2", "[TestStep=2]:")],
    };
    let (ops, _) = doc_client().update_test_case_doc(
        &tc, None, None, Some(&original), None, None, BlankPolicy::Skip,
        &AttachmentInput { current: Some(&cur), uploaded: &[], skip: false },
    );
    let tail: Vec<&serde_json::Value> = ops.iter().rev().take(3).collect();
    assert_eq!(tail[2]["op"], "test");
    assert_eq!(tail[2]["value"], 9);
    assert_eq!(tail[1]["path"], "/relations/1/attributes/comment");
    assert_eq!(tail[1]["value"], format!("[TestStep={}]:note", ids[1]));
    assert_eq!(tail[0]["op"], "remove");
    assert_eq!(tail[0]["path"], "/relations/3");
}

#[test]
fn update_doc_without_attachment_work_is_what_2_4_2_built() {
    let plain = case_of(vec![step("one", vec![]), step("two", vec![])]);
    let cur = CaseRelations { rev: 1, attachments: vec![rel(0, "g1", "[TestStep=2]:")] };
    let a = doc_client().update_test_case_doc(
        &plain, None, None, None, None, None, BlankPolicy::Skip, &AttachmentInput::NONE,
    );
    let b = doc_client().update_test_case_doc(
        &plain, None, None, None, None, None, BlankPolicy::Skip,
        &AttachmentInput { current: Some(&cur), uploaded: &[], skip: false },
    );
    assert_eq!(a, b);
    assert!(a.1.is_empty());
    assert!(!a.0.iter().any(|o| o["path"].as_str().is_some_and(|p| p.starts_with("/relations"))));
    // NONE ignores a draft that does carry attachments: no relation ops.
    let with = case_of(vec![step("one", vec![new_file("C:/a.png")])]);
    let (ops, _) = doc_client().update_test_case_doc(
        &with, None, None, None, None, None, BlankPolicy::Skip, &AttachmentInput::NONE,
    );
    assert!(!ops.iter().any(|o| o["path"].as_str().is_some_and(|p| p.starts_with("/relations"))));
}

#[test]
fn skip_means_no_ops_and_no_notes() {
    let mut tc = case_of(vec![step("one", vec![existing("g1")])]);
    tc.removed_attachments = vec!["g9".into()];
    let cur = CaseRelations { rev: 1, attachments: vec![rel(0, "g9", "[TestStep=2]:")] };
    let (ops, notes) = doc_client().update_test_case_doc(
        &tc, None, None, None, None, None, BlankPolicy::Skip,
        &AttachmentInput { current: Some(&cur), uploaded: &[], skip: true },
    );
    assert!(notes.is_empty());
    assert!(!ops.iter().any(|o| o["path"].as_str().is_some_and(|p| p.starts_with("/relations"))));
}

#[test]
fn id_count_mismatch_gives_note_not_ops() {
    // No original and a shared reference nobody can read: steps_patch
    // refuses, there are no ids, and attachment work is skipped with a note.
    let tc = case_of(vec![Step { shared: Some(0), ..Default::default() }, step("two", vec![new_file("C:/a.png")])]);
    let up = [Uploaded { path: "C:/a.png".into(), guid: "g".into(), url: "u".into(), name: "a.png".into() }];
    let cur = CaseRelations::default();
    let (ops, notes) = doc_client().update_test_case_doc(
        &tc, None, None, None, None, None, BlankPolicy::Skip,
        &AttachmentInput { current: Some(&cur), uploaded: &up, skip: false },
    );
    assert!(!ops.iter().any(|o| o["path"] == "/relations/-"));
    assert_eq!(notes.len(), 1);
    assert!(!notes[0].to_lowercase().contains("http"));
}
