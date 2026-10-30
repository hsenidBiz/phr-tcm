//! The notes a script save gives about a delete: after a check that can
//! fail, and picked by position.

use v2_lib::autorun::script_notes::{
    delete_after_check_notes as after_check, is_delete_click, positional_delete_notes as positional,
    positional_delete_notes_for_actions,
};
use v2_lib::autorun::CaseScript;

fn script(steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(serde_json::json!({ "case_id": 7, "title": "t", "steps": steps })).unwrap()
}

fn delete_after_check_script() -> CaseScript {
    script(serde_json::json!([
        { "step_number": 7, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Add" } }] },
        { "step_number": 8, "actions": [
            { "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "AUTOTEST Scale" },
            { "kind": "click", "selector": { "role": "button", "name": "Save" } },
            { "kind": "expect_text", "selector": ".toast", "value": "Saved" },
            { "kind": "click", "selector": { "role": "button", "name": "Delete" } }
        ]}
    ]))
}

#[test]
fn delete_after_failing_check_is_noted() {
    let notes = after_check(&delete_after_check_script(), None, None);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("step 8 deletes what the case made after a check"), "{notes:?}");
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
    assert!(after_check(&s, None, None).is_empty());
}

#[test]
fn delete_before_any_check_is_not_noted() {
    let s = script(serde_json::json!([
        { "step_number": 1, "actions": [
            { "kind": "click", "selector": { "role": "button", "name": "Remove row" } },
            { "kind": "expect_text", "selector": ".toast", "value": "Removed" }
        ]}
    ]));
    assert!(after_check(&s, None, None).is_empty());
}

#[test]
fn nth_delete_is_noted() {
    let s = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Delete", "nth": 2 } }] }
    ]));
    let notes = positional(&s, None, None);
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
    let notes = positional(&same_step, None, None);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("(nth 1)"), "{notes:?}");

    let next_step = script(serde_json::json!([
        { "step_number": 4, "actions": [{ "kind": "click", "selector": { "css": ".row-action", "nth": 1 } }] },
        { "step_number": 5, "actions": [{ "kind": "expect_dialog", "answer": "accept" }] }
    ]));
    assert!(positional(&next_step, None, None).is_empty(), "only the same step");

    let no_dialog = script(serde_json::json!([
        { "step_number": 4, "actions": [{ "kind": "click", "selector": { "css": ".row", "nth": 1 } }] }
    ]));
    assert!(positional(&no_dialog, None, None).is_empty());
}

#[test]
fn delete_found_by_name_is_not_noted() {
    let s = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": [
            { "role": "row", "name": "AUTOTEST Marks Range Scale" },
            { "role": "button", "name": "Delete" }
        ] }] }
    ]));
    assert!(positional(&s, None, None).is_empty());
}

#[test]
fn only_steps_limits_notes() {
    let s = delete_after_check_script();
    assert!(after_check(&s, None, Some(&[7])).is_empty());
    assert_eq!(after_check(&s, None, Some(&[8])).len(), 1);
    let p = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Delete", "nth": 2 } }] }
    ]));
    assert!(positional(&p, None, Some(&[1])).is_empty());
    assert_eq!(positional(&p, None, Some(&[3])).len(), 1);
}

#[test]
fn delete_under_test_across_steps_is_not_noted() {
    let s = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": { "role": "row", "name": "Scale A" } }] },
        { "step_number": 2, "actions": [
            { "kind": "click", "selector": { "role": "button", "name": "Delete" } },
            { "kind": "expect_text", "selector": ".toast", "value": "Deleted" }
        ]}
    ]));
    assert!(after_check(&s, None, None).is_empty());
}

#[test]
fn confirm_modal_delete_is_not_noted() {
    let s = script(serde_json::json!([
        { "step_number": 1, "actions": [
            { "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "AUTOTEST Scale" },
            { "kind": "click", "selector": { "role": "button", "name": "Save" } },
            { "kind": "expect_text", "selector": ".toast", "value": "Saved" }
        ]},
        { "step_number": 2, "actions": [
            { "kind": "click", "selector": { "role": "button", "name": "Delete" } },
            { "kind": "expect_visible", "selector": { "role": "dialog", "name": "Confirm" } },
            { "kind": "click", "selector": { "role": "button", "name": "Yes, remove" } }
        ]}
    ]));
    assert!(after_check(&s, None, None).is_empty());
}

#[test]
fn nth_inside_named_row_is_not_noted() {
    let s = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": [
            { "role": "row", "name": "AUTOTEST Marks Range Scale" },
            { "role": "button", "name": "Delete", "nth": 0 }
        ] }] }
    ]));
    assert!(positional(&s, None, None).is_empty());
}

#[test]
fn a_tab_click_before_an_alert_is_not_a_delete() {
    let s = script(serde_json::json!([
        { "step_number": 3, "actions": [
            { "kind": "click", "selector": { "css": ".tab", "nth": 1 } },
            { "kind": "click", "selector": { "role": "button", "name": "Save" } },
            { "kind": "expect_dialog", "answer": "accept" }
        ]}
    ]));
    assert!(positional(&s, None, None).is_empty());
}

#[test]
fn delete_words_cover_css_and_skip_ui_words() {
    let click = |sel: serde_json::Value| serde_json::from_value::<v2_lib::browser::actions::Action>(
        serde_json::json!({ "kind": "click", "selector": sel })).unwrap();
    assert!(is_delete_click(&click(serde_json::json!({ "css": ".btn-trash" }))));
    assert!(is_delete_click(&click(serde_json::json!({ "css": "[aria-label='Delete row']" }))));
    assert!(!is_delete_click(&click(serde_json::json!({ "role": "button", "name": "Discard changes" }))));
    assert!(!is_delete_click(&click(serde_json::json!({ "role": "button", "name": "Remove filter" }))));
}

#[test]
fn a_component_hides_neither_note() {
    let file: v2_lib::autorun::components::ComponentFile = serde_json::from_value(serde_json::json!({ "components": [{
        "name": "remove scale", "description": "d", "inputs": [],
        "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Delete", "nth": 3 } }]
    }]})).unwrap();
    let s = script(serde_json::json!([
        { "step_number": 2, "actions": [{ "kind": "use_component", "component": "remove scale" }] }
    ]));
    assert!(positional(&s, None, None).is_empty());
    assert_eq!(positional(&s, Some(&file), None).len(), 1);
    assert_eq!(positional_delete_notes_for_actions(&file.components[0].actions).len(), 1);
}

/// A wizard that creates with Finish is a save for the delete note as it is
/// for the no-save guard: one list of words.
#[test]
fn a_finish_click_counts_as_making_something() {
    let s = script(serde_json::json!([
        { "step_number": 8, "actions": [
            { "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "AUTOTEST Scale" },
            { "kind": "click", "selector": { "role": "button", "name": "Finish" } },
            { "kind": "expect_text", "selector": ".toast", "value": "Done" },
            { "kind": "click", "selector": { "role": "button", "name": "Delete" } }
        ]}
    ]));
    let notes = after_check(&s, None, None);
    assert_eq!(notes.len(), 1, "{notes:?}");
}
