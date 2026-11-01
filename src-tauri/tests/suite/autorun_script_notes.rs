//! The notes a script save gives about a delete: after a check that can
//! fail, and picked by position.

use v2_lib::autorun::script_notes::{
    delete_after_check_notes as after_check, is_delete_click, moved_with_check, order_notes, order_notes_with, KEEP_THE_CASES_DELETE, positional_delete_notes as positional,
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

fn two(step: serde_json::Value, cleanup: serde_json::Value) -> CaseScript {
    script(serde_json::json!([
        { "step_number": 6, "actions": step },
        { "step_number": 7, "cleanup": true, "unchecked": "removes the record", "actions": cleanup }
    ]))
}

fn delete_click() -> serde_json::Value {
    serde_json::json!({ "kind": "click", "selector": { "role": "button", "name": "Delete" } })
}

fn gone() -> serde_json::Value {
    serde_json::json!({ "kind": "expect_hidden", "selector": ".row" })
}

#[test]
fn moving_a_delete_and_its_check_is_noted() {
    let old = script(serde_json::json!([
        { "step_number": 6, "actions": [delete_click(), gone()] }
    ]));
    let new = two(serde_json::json!([{ "kind": "click", "selector": { "role": "button", "name": "Open" } }]), serde_json::json!([delete_click()]));
    let notes = moved_with_check(&old, &new);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("step 6"), "{notes:?}");
    assert!(notes[0].contains(KEEP_THE_CASES_DELETE), "{notes:?}");
}

#[test]
fn a_guarded_fallback_alongside_the_kept_delete_is_not_noted() {
    let old = script(serde_json::json!([{ "step_number": 6, "actions": [delete_click(), gone()] }]));
    let guarded = serde_json::json!({ "kind": "when_visible", "selector": ".row", "then": [delete_click()] });
    let new = two(serde_json::json!([delete_click(), gone()]), serde_json::json!([guarded]));
    assert!(moved_with_check(&old, &new).is_empty());
}

#[test]
fn no_clean_up_step_and_no_lost_check_are_not_noted() {
    let old = script(serde_json::json!([{ "step_number": 6, "actions": [delete_click()] }]));
    let new = two(serde_json::json!([gone()]), serde_json::json!([delete_click()]));
    // No clean-up step in the new script: nothing moved to it.
    assert!(moved_with_check(&old, &old).is_empty());
    // No old check lost: the step kept its count.
    assert!(moved_with_check(&old, &new).is_empty());
}

#[test]
fn a_target_less_action_is_noted_without_a_double_space() {
    let old = script(serde_json::json!([{ "step_number": 6, "actions": [{ "kind": "reload" }, gone()] }]));
    let new = two(serde_json::json!([]), serde_json::json!([{ "kind": "reload" }]));
    let notes = moved_with_check(&old, &new);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(!notes[0].contains("  ") && notes[0].contains("reload moved"), "{notes:?}");
}

#[test]
fn a_delete_picked_by_has_text_is_not_a_positional_pick() {
    let s = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Delete", "has_text": "AUTOTEST Scale", "nth": 0 } }] }
    ]));
    assert!(positional(&s, None, None).is_empty());
    let by_position = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Delete", "nth": 2 } }] }
    ]));
    let notes = positional(&by_position, None, None);
    assert!(notes[0].contains("has_text"), "{notes:?}");
}

#[test]
fn the_delete_after_check_note_allows_a_delete_the_case_asks_for() {
    let notes = after_check(&delete_after_check_script(), None, None);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("unless the case asks for the delete") && notes[0].contains("add a guarded fallback to the clean-up"),
        "{notes:?}"
    );
}

// ---- order notes: a list one script reorders, another picks by position ----

fn in_area(id: i32, area: Option<&str>, changes: &[&str], actions: serde_json::Value) -> CaseScript {
    let mut s: CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": id, "title": "t",
        "steps": [{ "step_number": 3, "actions": actions }]
    }))
    .unwrap();
    s.area = area.map(str::to_string);
    s.changes = changes.iter().map(|c| c.to_string()).collect();
    s
}

fn pick_nth() -> serde_json::Value {
    serde_json::json!([{ "kind": "click", "selector": { "role": "row", "nth": 2 } }])
}

fn pick_text() -> serde_json::Value {
    serde_json::json!([{ "kind": "click", "selector": { "role": "row", "has_text": "FY2026" } }])
}

#[test]
fn a_changer_is_told_who_picks_by_position() {
    let saving = in_area(1, Some("Scales"), &["scale reapplied"], pick_text());
    let saved = [in_area(2, Some("Scales"), &[], pick_nth())];
    let notes = order_notes(&saving, &saved);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("case 1 changes scale reapplied in area Scales; these scripts pick a row there by position"), "{notes:?}");
    assert!(notes[0].contains("case 2 step 3"), "{notes:?}");
}

#[test]
fn a_positional_pick_is_told_who_changes() {
    let saving = in_area(2, Some("Scales"), &[], pick_nth());
    let saved = [in_area(1, Some("Scales"), &["scale reapplied"], pick_text())];
    let notes = order_notes(&saving, &saved);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("case 1"), "{notes:?}");
    assert!(notes[0].contains("case 2 picks"), "{notes:?}");
}

#[test]
fn another_area_is_not_named() {
    let saving = in_area(1, Some("Scales"), &["scale reapplied"], pick_text());
    let saved = [in_area(2, Some("Cycles"), &[], pick_nth())];
    assert!(order_notes(&saving, &saved).is_empty());
    let saving = in_area(2, Some("Cycles"), &[], pick_nth());
    let saved = [in_area(1, Some("Scales"), &["scale reapplied"], pick_text())];
    assert!(order_notes(&saving, &saved).is_empty());
}

#[test]
fn a_text_pick_is_not_positional() {
    let saving = in_area(1, Some("Scales"), &["scale reapplied"], pick_text());
    let saved = [in_area(2, Some("Scales"), &[], pick_text())];
    assert!(order_notes(&saving, &saved).is_empty());
}

#[test]
fn css_position_words_count() {
    for css in [":nth-child(2)", ":first-child", ":last-child", ":first-of-type", ":nth-of-type(3)"] {
        let saving = in_area(1, Some("Scales"), &["x"], pick_text());
        let pick = serde_json::json!([{ "kind": "click", "selector": format!("tr{css} td") }]);
        let saved = [in_area(2, Some("Scales"), &[], pick)];
        assert_eq!(order_notes(&saving, &saved).len(), 1, "{css}");
    }
}

#[test]
fn a_components_positional_locator_counts() {
    let file: v2_lib::autorun::components::ComponentFile = serde_json::from_value(serde_json::json!({ "components": [{
        "name": "pick row", "description": "d", "inputs": [],
        "actions": [{ "kind": "click", "selector": { "role": "row", "nth": 1 } }]
    }]})).unwrap();
    let saving = in_area(1, Some("Scales"), &["x"], pick_text());
    let saved = [in_area(2, Some("Scales"), &[], serde_json::json!([{ "kind": "use_component", "component": "pick row" }]))];
    assert!(order_notes(&saving, &saved).is_empty());
    assert_eq!(order_notes_with(&saving, &saved, Some(&file)).len(), 1);
}

#[test]
fn a_locator_inside_a_guard_counts() {
    let saving = in_area(1, Some("Scales"), &["x"], pick_text());
    let guarded = serde_json::json!([{ "kind": "when_visible", "selector": ".row", "then": [
        { "kind": "click", "selector": { "role": "row", "nth": 0 } }] }]);
    let saved = [in_area(2, Some("Scales"), &[], guarded)];
    assert_eq!(order_notes(&saving, &saved).len(), 1);
}

#[test]
fn the_positional_note_is_capped() {
    let saving = in_area(1, Some("Scales"), &["x"], pick_text());
    let saved: Vec<CaseScript> = (10..30).map(|i| in_area(i, Some("Scales"), &[], pick_nth())).collect();
    let notes = order_notes(&saving, &saved);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("case 19 step 3"), "{notes:?}");
    assert!(!notes[0].contains("case 20 step 3"), "{notes:?}");
    assert!(notes[0].ends_with("and 10 more"), "{notes:?}");
}
