//! The declared-edit gate: an assistant's edit to an Auto Run script must
//! name every step it touches, may never weaken a check, and is capped.
//! Pure functions over `CaseScript` - no browser, no filesystem.

use serde_json::json;
use v2_lib::autorun::edits::{check_edits, next_repairs, Edit};
use v2_lib::autorun::CaseScript;


fn script(json: serde_json::Value) -> CaseScript {
    serde_json::from_value(json).unwrap()
}

fn edit(steps: &[i32], why: &str) -> Edit {
    Edit { case_id: 1, steps: steps.to_vec(), why: why.to_string(), quirk: None }
}

#[test]
fn undeclared_steps_are_named_in_one_sentence_sorted() {
    // step 1 is untouched, step 2 is removed, step 3 is added, step 4's
    // actions change - all three of those must be declared.
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": "#gone" }] },
            { "step_number": 4, "actions": [{ "kind": "click", "selector": "#old" }] }
        ]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] },
            { "step_number": 3, "actions": [{ "kind": "click", "selector": "#new" }] },
            { "step_number": 4, "actions": [{ "kind": "click", "selector": "#changed" }] }
        ]
    }));
    let declared = edit(&[], "renamed a couple of buttons");
    let err = check_edits(&old, &new, Some(&declared)).unwrap_err();
    assert_eq!(
        err,
        "step 2, step 3, step 4 were changed but not declared - name every step you change in \"edits\", or leave it as it was"
    );
}

#[test]
fn a_step_whose_only_change_is_its_unchecked_reason_must_still_be_declared() {
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }], "unchecked": "reason A" }
        ]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }], "unchecked": "reason B" }
        ]
    }));
    let err = check_edits(&old, &new, None).unwrap_err();
    assert_eq!(
        err,
        "case 1: step 1 was changed but not declared - name every step you change in \"edits\", or leave it as it was"
    );
}

#[test]
fn a_declared_step_that_did_not_change_is_refused() {
    let sc = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] }
        ]
    }));
    let declared = edit(&[1], "thought I changed this");
    let err = check_edits(&sc, &sc, Some(&declared)).unwrap_err();
    assert_eq!(err, "step 1 was declared but not changed");
}

#[test]
fn several_declared_but_unchanged_steps_are_named_in_one_sentence_sorted() {
    let sc = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 2, "actions": [{ "kind": "click", "selector": "#a" }] },
            { "step_number": 4, "actions": [{ "kind": "click", "selector": "#b" }] }
        ]
    }));
    let declared = edit(&[4, 2], "thought I changed both of these");
    let err = check_edits(&sc, &sc, Some(&declared)).unwrap_err();
    assert_eq!(err, "steps 2, 4 were declared but not changed");
}

#[test]
fn a_duplicate_step_number_in_new_is_refused_before_anything_else_is_compared() {
    // A weakened copy of step 3 sits first, an untouched copy sits last. A
    // map keyed by step_number would silently keep only the last (unchanged)
    // entry and let the gate wave the weakened one through, even though the
    // runner still executes both entries in the Vec - so this is refused
    // outright, with no declaration needed to trigger it.
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 3, "actions": [
                { "kind": "check_text", "value": "Saved" },
                { "kind": "expect_visible", "selector": "#a" }
            ] }
        ]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 3, "actions": [{ "kind": "check_text", "value": "Saved" }] },
            { "step_number": 3, "actions": [
                { "kind": "check_text", "value": "Saved" },
                { "kind": "expect_visible", "selector": "#a" }
            ] }
        ]
    }));
    let err = check_edits(&old, &new, None).unwrap_err();
    assert_eq!(err, "step 3 appears more than once in the script");
}

#[test]
fn a_duplicate_step_number_in_old_is_refused_too() {
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] },
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#b" }] }
        ]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] }
        ]
    }));
    let err = check_edits(&old, &new, None).unwrap_err();
    assert_eq!(err, "step 1 appears more than once in the script");
}

#[test]
fn a_repair_cannot_change_which_case_a_script_belongs_to() {
    let old = script(json!({ "case_id": 1, "title": "t", "steps": [] }));
    let new = script(json!({ "case_id": 2, "title": "t", "steps": [] }));
    let err = check_edits(&old, &new, None).unwrap_err();
    assert_eq!(err, "a repair cannot change which test case a script belongs to");
}

#[test]
fn a_declaration_naming_the_wrong_case_is_refused() {
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] }
        ]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#b" }] }
        ]
    }));
    let declared = edit(&[1], "fixed a locator");
    let declared = Edit { case_id: 99, ..declared };
    let err = check_edits(&old, &new, Some(&declared)).unwrap_err();
    assert_eq!(err, "the declaration names case 99 but this script is case 1");
}

#[test]
fn weakening_a_check_is_refused_even_when_declared() {
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [
                { "kind": "check_text", "value": "Saved" },
                { "kind": "expect_visible", "selector": "#a" }
            ] }
        ]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [
                { "kind": "check_text", "value": "Saved" }
            ] }
        ]
    }));
    let declared = edit(&[1], "removed a duplicate check");
    let err = check_edits(&old, &new, Some(&declared)).unwrap_err();
    assert_eq!(
        err,
        "step 1 had 2 checks and now has 1 - an assertion is never removed or weakened by a repair"
    );
}

#[test]
fn a_locator_fix_with_the_same_check_count_is_allowed_when_declared() {
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": "#a" }] }
        ]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": "#b" }] }
        ]
    }));
    let declared = edit(&[1], "fixed a stale locator");
    assert_eq!(check_edits(&old, &new, Some(&declared)), Ok(()));
}

#[test]
fn a_locator_fix_with_the_same_check_count_is_refused_when_not_declared() {
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": "#a" }] }
        ]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": "#b" }] }
        ]
    }));
    let declared = edit(&[], "unrelated cleanup");
    let err = check_edits(&old, &new, Some(&declared)).unwrap_err();
    assert_eq!(
        err,
        "step 1 was changed but not declared - name every step you change in \"edits\", or leave it as it was"
    );
}

#[test]
fn the_account_a_script_runs_as_cannot_be_changed_by_a_repair() {
    let old = script(json!({ "case_id": 1, "title": "t", "account": "tester1", "steps": [] }));
    let new = script(json!({ "case_id": 1, "title": "t", "account": "tester2", "steps": [] }));
    let err = check_edits(&old, &new, None).unwrap_err();
    assert_eq!(
        err,
        "the account a script runs as cannot be changed by a repair - a person picks it in the app"
    );
}

#[test]
fn a_title_change_is_allowed_silently() {
    let old = script(json!({ "case_id": 1, "title": "Old title", "steps": [] }));
    let new = script(json!({ "case_id": 1, "title": "New title", "steps": [] }));
    assert_eq!(check_edits(&old, &new, None), Ok(()));
}

#[test]
fn an_edit_with_a_blank_reason_is_refused() {
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] }
        ]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#b" }] }
        ]
    }));
    let declared = edit(&[1], "   ");
    let err = check_edits(&old, &new, Some(&declared)).unwrap_err();
    assert_eq!(err, "an edit needs a reason");
}

#[test]
fn next_repairs_increments_below_the_cap() {
    let sc0 = script(json!({ "case_id": 1, "title": "t", "steps": [], "repairs": 0 }));
    assert_eq!(next_repairs(&sc0), Ok(1));

    let sc2 = script(json!({ "case_id": 1, "title": "t", "steps": [], "repairs": 2 }));
    assert_eq!(next_repairs(&sc2), Ok(3));
}

#[test]
fn next_repairs_refuses_once_the_cap_is_reached() {
    let sc3 = script(json!({ "case_id": 1, "title": "t", "steps": [], "repairs": 3 }));
    let err = next_repairs(&sc3).unwrap_err();
    assert_eq!(
        err,
        "this script has been repaired 3 times without a person looking at it - open it in the app, save it there, and the count starts again"
    );
}

#[test]
fn no_declaration_at_all_for_a_changed_case_is_refused_with_the_case_id() {
    let old = script(json!({
        "case_id": 42, "title": "t",
        "steps": [
            { "step_number": 3, "actions": [{ "kind": "click", "selector": "#a" }] }
        ]
    }));
    let new = script(json!({
        "case_id": 42, "title": "t",
        "steps": [
            { "step_number": 3, "actions": [{ "kind": "click", "selector": "#b" }] }
        ]
    }));
    let err = check_edits(&old, &new, None).unwrap_err();
    assert_eq!(
        err,
        "case 42: step 3 was changed but not declared - name every step you change in \"edits\", or leave it as it was"
    );
}

#[test]
fn a_script_with_nothing_changed_needs_no_declaration() {
    // Rule 10 (a brand-new script needs no declaration) cannot be
    // represented as a call into this function at all: `check_edits` takes
    // `old: &CaseScript`, never an `Option`, so there is no way to call it
    // for a script that has not been saved yet - the signature itself is
    // the enforcement, and a brand-new script simply never reaches this
    // gate. The nearest thing provable here is that with nothing actually
    // changed, no declaration is required either.
    let sc = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] }
        ]
    }));
    assert_eq!(check_edits(&sc, &sc, None), Ok(()));
}
