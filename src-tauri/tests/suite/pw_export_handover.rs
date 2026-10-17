//! Playwright export: what TCM knows about a case that the repo cannot see.

use serde_json::json;
use v2_lib::autorun::CaseScript;
use v2_lib::pw_export::export::SharedDraft;
use v2_lib::pw_export::handover::{
    handover_for, order_note, step_effects, EffectKind, Handover, HandoverInput, StepEffect, ORDER_PROPOSAL,
};

/// A script of `steps` (each a list of actions), with `extra` merged in.
fn script(steps: serde_json::Value, extra: serde_json::Value) -> CaseScript {
    let steps: Vec<serde_json::Value> = steps
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, actions)| json!({ "step_number": i + 1, "actions": actions }))
        .collect();
    let mut v = json!({ "case_id": 7, "title": "T", "steps": steps });
    for (k, val) in extra.as_object().unwrap() {
        v[k] = val.clone();
    }
    serde_json::from_value(v).unwrap()
}

fn of(s: &CaseScript) -> Handover {
    handover_for(&HandoverInput { script: s, shared_drafts: &[], protected: &[] })
}

fn button(name: &str) -> serde_json::Value {
    json!({ "kind": "click", "selector": { "role": "button", "name": name } })
}

#[test]
fn preconditions_come_from_the_script() {
    let s = script(
        json!([[]]),
        json!({
            "preconditions": [
                { "flow": "Definition Wizard", "stage": "goal-groups", "value": "C1", "why": "the case edits its goal groups" },
                { "flow": "Cycle", "stage": "published", "value": 5 }
            ],
            "needs_unchanged": ["cycle published"],
            "needs_state": { "Approval workflow": "ON" }
        }),
    );
    let drafts = [SharedDraft { fixture: "f2".into(), name: "FY26 Draft".into() }];
    let h = handover_for(&HandoverInput { script: &s, shared_drafts: &drafts, protected: &[] });
    assert_eq!(
        h.preconditions,
        vec![
            "Definition Wizard at goal-groups stage: the case edits its goal groups",
            "Cycle at published stage",
            "requires: cycle published unchanged",
            "requires: Approval workflow = ON",
            "shared draft \"FY26 Draft\", rebuilt by TCM fixture `f2`",
        ]
    );
    assert!(h.from_tcm.is_empty() && h.cleanup.is_empty(), "{h:?}");
    // A script that names none of it hands nothing over.
    assert_eq!(of(&script(json!([[button("Open")]]), json!({}))), Handover::default());
}

#[test]
fn a_look_only_case_says_so_first() {
    let s = script(json!([[button("Save")]]), json!({ "no_save": true, "changes": ["cycle published"] }));
    assert_eq!(
        of(&s).from_tcm,
        vec![
            "Look-only: must not save anything.",
            "step 1 appears to save data (\"Save\")",
            "changes shared state: cycle published",
        ]
    );
}

#[test]
fn delete_save_publish_steps_are_described_as_appearing_to() {
    let s = script(
        json!([
            [{ "kind": "click", "selector": [{ "role": "dialog", "name": "Save changes?" }, { "role": "button", "name": "Remove" }] }],
            [{ "kind": "click", "selector": { "text": "SUBMIT" } }, { "kind": "fill", "selector": "#a", "value": "Delete me" }],
            [button("Save & Publish")],
            [
                { "kind": "click", "selector": "#delete" },
                button("Address book"),
                button("Added items"),
                { "kind": "when_visible", "selector": { "text": "Sure?" }, "then": [button("delete")] }
            ],
            [{ "kind": "repeat_while_visible", "selector": { "text": "Row" }, "max": 3, "then": [button("Delete")] }]
        ]),
        json!({}),
    );
    let e = |step, kind, control: &str| StepEffect { step, kind, control: control.into() };
    assert_eq!(
        step_effects(&s),
        vec![
            e(1, EffectKind::Delete, "Remove"),
            e(2, EffectKind::Save, "SUBMIT"),
            e(3, EffectKind::Publish, "Save & Publish"),
            e(4, EffectKind::Delete, "delete"),
            e(5, EffectKind::Delete, "Delete"),
        ]
    );
    assert_eq!(
        of(&s).from_tcm,
        vec![
            "step 1 appears to delete data (\"Remove\")",
            "step 2 appears to save data (\"SUBMIT\")",
            "step 3 appears to publish - irreversible; run on demand only",
            "step 4 appears to delete data (\"delete\")",
            "step 5 appears to delete data (\"Delete\")",
        ]
    );
    // One line per control, however often a step clicks it.
    let twice = script(json!([[button("Save"), button("Save"), button("Update")]]), json!({}));
    assert_eq!(
        of(&twice).from_tcm,
        vec!["step 1 appears to save data (\"Save\")", "step 1 appears to save data (\"Update\")"]
    );
}

#[test]
fn a_suspected_defect_names_its_step_date_and_note() {
    // 2025-10-18 22:59:59 UTC: the date is the UTC one, whatever the clock here.
    let s = script(
        json!([[], [], []]),
        json!({ "suspected_defect": { "step_number": 3, "note": "The total is not recalculated", "marked_at": "1760828399000" } }),
    );
    assert_eq!(
        of(&s).from_tcm,
        vec!["step 3 is a known product defect (TCM mark 2025-10-18): The total is not recalculated"]
    );
}

#[test]
fn a_created_record_deleted_later_is_the_cleanup() {
    let row = |text: &str| {
        json!({ "kind": "click", "selector": [{ "role": "row", "name": text }, { "role": "button", "name": "Delete" }] })
    };
    let s = script(
        json!([
            [{ "kind": "fill", "selector": "#name", "value": "QA Goal Group 7" }, button("Save")],
            // Searched for, never saved: a record the case found, not one it made.
            [{ "kind": "fill", "selector": "#search", "value": "Baseline Group" }, row("Baseline Group")],
            [row("qa goal group 7 - Active")]
        ]),
        json!({}),
    );
    let h = of(&s);
    assert_eq!(h.cleanup, vec!["step 3 deletes \"QA Goal Group 7\" it created"]);
    assert_eq!(
        h.from_tcm,
        vec![
            "step 1 appears to save data (\"Save\")",
            "step 2 appears to delete data (\"Delete\")",
            "step 3 appears to delete data (\"Delete\")",
            "relies on step 3 to put data back",
        ]
    );
}

#[test]
fn a_search_typed_before_a_save_is_not_a_record_the_case_created() {
    let row = |text: &str| {
        json!({ "kind": "click", "selector": [{ "role": "row", "name": text }, { "role": "button", "name": "Delete" }] })
    };
    let fill = |selector: serde_json::Value, value: &str| json!({ "kind": "fill", "selector": selector, "value": value });
    // Search for a baseline group, create and save another, then delete the baseline row: the
    // delete is of a record the case found, so there is no cleanup to hand over.
    let s = script(
        json!([
            [fill(json!({ "role": "searchbox", "name": "Find" }), "Baseline Group")],
            [fill(json!({ "role": "textbox", "name": "Name" }), "New Group X"), button("Save")],
            [row("Baseline Group")]
        ]),
        json!({}),
    );
    let h = of(&s);
    assert!(h.cleanup.is_empty(), "{h:?}");
    assert!(!h.from_tcm.iter().any(|l| l.starts_with("relies on")), "{h:?}");
    // The field reads as a search by its name, its css, or a filter, whatever the case.
    for field in [
        json!({ "role": "textbox", "name": "Search groups" }),
        json!({ "role": "textbox", "name": "FILTER by name" }),
        json!("#search-box"),
    ] {
        let s = script(
            json!([
                [fill(field, "Baseline Group")],
                [fill(json!({ "role": "textbox", "name": "Name" }), "New Group X"), button("Save")],
                [row("Baseline Group")]
            ]),
            json!({}),
        );
        assert!(of(&s).cleanup.is_empty());
    }
    // A record saved stays created through later saves.
    let s = script(
        json!([
            [fill(json!({ "role": "textbox", "name": "Name" }), "Group Y"), button("Save")],
            [button("Save")],
            [row("Group Y")]
        ]),
        json!({}),
    );
    assert_eq!(of(&s).cleanup, vec!["step 3 deletes \"Group Y\" it created"]);
    let s = script(
        json!([
            [fill(json!({ "role": "textbox", "name": "Name" }), "Group Z"), button("Save")],
            [fill(json!({ "role": "textbox", "name": "Name" }), "Other Q"), button("Delete")],
            [row("Other Q")]
        ]),
        json!({}),
    );
    // A value typed before a delete is spent by it: the delete is not a save.
    assert!(of(&s).cleanup.is_empty(), "{:?}", of(&s));
}

#[test]
fn a_protected_baseline_adds_one_line_after_the_changes() {
    use v2_lib::autorun::nav::{ProtectedKind, ProtectedRecord};
    let s = script(
        json!([[button("Open")]]),
        json!({ "no_save": true, "changes": ["the cycle"], "suspected_defect": { "step_number": 1, "note": "n", "marked_at": "1760828399000" } }),
    );
    let protected = [ProtectedRecord { kind: ProtectedKind::Cycle, name: "FY2026".into() }];
    let h = handover_for(&HandoverInput { script: &s, shared_drafts: &[], protected: &protected });
    assert_eq!(
        h.from_tcm,
        vec![
            "Look-only: must not save anything.",
            "changes shared state: the cycle",
            "must never touch the protected records in docs/tcm-protected-baseline.md",
            "step 1 is a known product defect (TCM mark 2025-10-18): n",
        ]
    );
    // An empty baseline says nothing.
    assert!(!of(&s).from_tcm.iter().any(|l| l.contains("protected")));
}

fn case(id: i32, steps: serde_json::Value, extra: serde_json::Value) -> CaseScript {
    let mut extra = extra;
    extra["case_id"] = json!(id);
    script(steps, extra)
}

fn note(cases: &[&CaseScript], saved: Option<&[i32]>) -> Vec<String> {
    let list: Vec<(i32, &CaseScript)> = cases.iter().map(|s| (s.case_id, *s)).collect();
    order_note(&list, saved)
}

#[test]
fn order_lines_from_marks_and_saved_order() {
    // 1 needs the cycle unchanged; 2 changes it; 3 publishes; 4 changes
    // something nobody needs; 5 and 6 are independent.
    let one = case(1, json!([[button("Open")]]), json!({ "needs_unchanged": ["Cycle Published"] }));
    let two = case(2, json!([[button("Open")]]), json!({ "changes": ["cycle  published"] }));
    let three = case(3, json!([[button("Save & Publish")]]), json!({}));
    let four = case(4, json!([[button("Open")]]), json!({ "changes": ["rating scale", "weights"] }));
    let five = case(5, json!([[button("Open")]]), json!({}));
    let six = case(6, json!([[button("Open")]]), json!({}));
    assert_eq!(
        note(&[&one, &two, &three, &four, &five, &six], Some(&[6, 3, 99, 1, 2])),
        vec![
            "run 1 before 2 (1 needs Cycle Published unchanged and 2 changes it)".to_string(),
            "run 2 alone and last: it changes shared state (cycle  published)".to_string(),
            "run 3 alone and last: it publishes".to_string(),
            "run 4 alone and last: it changes shared state (rating scale, weights)".to_string(),
            // Only the cases of this file, in the saved order.
            "saved TCM order: 6, 3, 1, 2".to_string(),
            "suggested group: 1, 2 (run together, in this order)".to_string(),
            ORDER_PROPOSAL.to_string(),
        ]
    );
}

#[test]
fn a_publisher_that_also_changes_state_is_said_once_as_publishing() {
    let p = case(8, json!([[button("Publish")]]), json!({ "changes": ["cycle published"] }));
    assert_eq!(note(&[&p], None), vec!["run 8 alone and last: it publishes".to_string(), ORDER_PROPOSAL.to_string()]);
}

#[test]
fn a_chain_is_one_group_in_the_suggested_order() {
    // 11 needs X unchanged and changes Y; 12 needs Y unchanged and changes Z;
    // 13 needs Z unchanged. Listed backwards, so the suggestion moves them.
    let c = case(13, json!([[]]), json!({ "needs_unchanged": ["z"] }));
    let b = case(12, json!([[]]), json!({ "needs_unchanged": ["y"], "changes": ["z"] }));
    let a = case(11, json!([[]]), json!({ "needs_unchanged": ["x"], "changes": ["y"] }));
    let lines = note(&[&c, &b, &a], None);
    assert!(lines.contains(&"suggested group: 13, 12, 11 (run together, in this order)".to_string()), "{lines:?}");
    assert!(lines.contains(&"run 13 before 12 (13 needs z unchanged and 12 changes it)".to_string()), "{lines:?}");
    assert!(lines.contains(&"run 12 before 11 (12 needs y unchanged and 11 changes it)".to_string()), "{lines:?}");
}

/// The ids `suggest` puts in order for these cases, as listed.
fn planned(cases: &[&CaseScript]) -> Vec<i32> {
    let list: Vec<(i32, v2_lib::autorun::plan::Marks)> =
        cases.iter().map(|s| (s.case_id, v2_lib::autorun::plan::Marks::of(s))).collect();
    v2_lib::autorun::plan::suggest(&list)
}

fn before_lines(lines: &[String]) -> Vec<&String> {
    lines.iter().filter(|l| l.starts_with("run ") && l.contains(" before ")).collect()
}

#[test]
fn two_cases_that_each_need_and_change_one_thing_get_one_before_line() {
    let a = case(1, json!([[]]), json!({ "needs_unchanged": ["x"], "changes": ["x"] }));
    let b = case(2, json!([[]]), json!({ "needs_unchanged": ["x"], "changes": ["x"] }));
    let lines = note(&[&a, &b], None);
    assert_eq!(before_lines(&lines), vec!["run 1 before 2 (1 needs x unchanged and 2 changes it)"], "{lines:?}");
    assert_eq!(planned(&[&a, &b]), vec![1, 2]);
    // Listed the other way round, the planner (and so the note) puts 2 first.
    let lines = note(&[&b, &a], None);
    assert_eq!(before_lines(&lines), vec!["run 2 before 1 (2 needs x unchanged and 1 changes it)"], "{lines:?}");
    assert_eq!(planned(&[&b, &a]), vec![2, 1]);
}

#[test]
fn a_two_cycle_gets_one_before_line_in_the_planners_order() {
    // 1 changes x and needs y; 2 changes y and needs x: one reset, one order.
    let a = case(1, json!([[]]), json!({ "needs_unchanged": ["y"], "changes": ["x"] }));
    let b = case(2, json!([[]]), json!({ "needs_unchanged": ["x"], "changes": ["y"] }));
    let lines = note(&[&a, &b], None);
    assert_eq!(before_lines(&lines), vec!["run 1 before 2 (1 needs y unchanged and 2 changes it)"], "{lines:?}");
    assert_eq!(planned(&[&a, &b]), vec![1, 2]);
}
