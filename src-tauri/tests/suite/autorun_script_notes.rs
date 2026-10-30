//! The notes a script save gives about a delete: after a check that can
//! fail, and picked by position.

use v2_lib::autorun::script_notes::{delete_after_check_notes, positional_delete_notes};
use v2_lib::autorun::CaseScript;

fn script(steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(serde_json::json!({ "case_id": 7, "title": "t", "steps": steps })).unwrap()
}

fn delete_after_check_script() -> CaseScript {
    script(serde_json::json!([
        { "step_number": 7, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Add" } }] },
        { "step_number": 8, "actions": [
            { "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "x" },
            { "kind": "expect_text", "selector": ".toast", "value": "Saved" },
            { "kind": "click", "selector": { "role": "button", "name": "Delete" } }
        ]}
    ]))
}

#[test]
fn delete_after_failing_check_is_noted() {
    let notes = delete_after_check_notes(&delete_after_check_script(), None);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("step 8 deletes after a check"), "{notes:?}");
    assert!(notes[0].contains("clean-up step"), "{notes:?}");
}

#[test]
fn delete_in_cleanup_step_is_not_noted() {
    let s = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "expect_text", "selector": ".toast", "value": "Saved" }] },
        { "step_number": 2, "cleanup": true, "unchecked": "removes the record", "actions": [
            { "kind": "click", "selector": { "role": "button", "name": "Delete" } }
        ]}
    ]));
    assert!(delete_after_check_notes(&s, None).is_empty());
}

#[test]
fn delete_before_any_check_is_not_noted() {
    let s = script(serde_json::json!([
        { "step_number": 1, "actions": [
            { "kind": "click", "selector": { "role": "button", "name": "Remove row" } },
            { "kind": "expect_text", "selector": ".toast", "value": "Removed" }
        ]}
    ]));
    assert!(delete_after_check_notes(&s, None).is_empty());
}

#[test]
fn nth_delete_is_noted() {
    let s = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Delete", "nth": 2 } }] }
    ]));
    let notes = positional_delete_notes(&s, None);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("step 3: the delete picks by position (nth 2)"), "{notes:?}");
}

#[test]
fn nth_click_then_confirm_dialog_is_noted() {
    let same_step = script(serde_json::json!([
        { "step_number": 4, "actions": [
            { "kind": "click", "selector": { "css": ".icon-trash", "nth": 1 } },
            { "kind": "expect_dialog", "answer": "accept" }
        ]}
    ]));
    let notes = positional_delete_notes(&same_step, None);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("(nth 1)"), "{notes:?}");

    let next_step = script(serde_json::json!([
        { "step_number": 4, "actions": [{ "kind": "click", "selector": { "css": ".icon-trash", "nth": 1 } }] },
        { "step_number": 5, "actions": [{ "kind": "expect_dialog", "answer": "accept" }] }
    ]));
    assert_eq!(positional_delete_notes(&next_step, None).len(), 1);

    let no_dialog = script(serde_json::json!([
        { "step_number": 4, "actions": [{ "kind": "click", "selector": { "css": ".row", "nth": 1 } }] }
    ]));
    assert!(positional_delete_notes(&no_dialog, None).is_empty());
}

#[test]
fn delete_found_by_name_is_not_noted() {
    let s = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": [
            { "role": "row", "name": "AUTOTEST Marks Range Scale" },
            { "role": "button", "name": "Delete" }
        ] }] }
    ]));
    assert!(positional_delete_notes(&s, None).is_empty());
}

#[test]
fn only_steps_limits_notes() {
    let s = delete_after_check_script();
    assert!(delete_after_check_notes(&s, Some(&[7])).is_empty());
    assert_eq!(delete_after_check_notes(&s, Some(&[8])).len(), 1);
    let p = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Delete", "nth": 2 } }] }
    ]));
    assert!(positional_delete_notes(&p, Some(&[1])).is_empty());
    assert_eq!(positional_delete_notes(&p, Some(&[3])).len(), 1);
}
