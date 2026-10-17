//! Playwright export: what TCM knows about a case that the repo cannot see.

use serde_json::json;
use v2_lib::autorun::CaseScript;
use v2_lib::pw_export::export::SharedDraft;
use v2_lib::pw_export::handover::{handover_for, step_effects, EffectKind, Handover, HandoverInput, StepEffect};

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
    handover_for(&HandoverInput { script: s, shared_drafts: &[] })
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
    let h = handover_for(&HandoverInput { script: &s, shared_drafts: &drafts });
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
