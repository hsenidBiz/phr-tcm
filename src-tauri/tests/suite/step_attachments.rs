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
