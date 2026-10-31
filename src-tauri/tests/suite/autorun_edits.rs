//! The declared-edit gate: an assistant's edit to an Auto Run script must
//! name every step it touches and leave every other step as it was. A
//! declared step may change in any way, a check removed included.
//! Pure functions over `CaseScript` - no browser, no filesystem.

use serde_json::json;
use v2_lib::autorun::edits::{apply_precondition_edit, check_edits, Edit, PreconditionEdit};
use v2_lib::autorun::{PreconditionOp, PreconditionRepair};
use v2_lib::autorun::CaseScript;


fn script(json: serde_json::Value) -> CaseScript {
    serde_json::from_value(json).unwrap()
}

fn edit(steps: &[i32], why: &str) -> Edit {
    Edit { case_id: 1, steps: steps.to_vec(), why: why.to_string(), quirk: None, quirk_class: None, area: false, precondition: None, gives_up: Vec::new() }
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
    // A changed copy of step 3 sits first, an untouched copy sits last. A
    // map keyed by step_number would silently keep only the last (unchanged)
    // entry and let the gate wave the changed one through, even though the
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

/// The owner's call: a repair may drop a check, or turn one into an action
/// that checks nothing, in a step it declares. The same change left
/// undeclared is still refused, and the steps it does not declare stay as
/// they were.
#[test]
fn a_repair_may_remove_a_check_in_a_declared_step() {
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [
                { "kind": "check_text", "value": "Saved" },
                { "kind": "expect_visible", "selector": "#a" }
            ] },
            { "step_number": 2, "actions": [{ "kind": "expect_visible", "selector": "#b" }] }
        ]
    }));
    let removed = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "Saved" }] },
            { "step_number": 2, "actions": [{ "kind": "expect_visible", "selector": "#b" }] }
        ]
    }));
    assert_eq!(check_edits(&old, &removed, Some(&edit(&[1], "the second check was a duplicate"))), Ok(()));

    // A check turned into an action that checks nothing.
    let non_check = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [
                { "kind": "check_text", "value": "Saved" },
                { "kind": "expect_visible", "selector": "#a" }
            ] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": "#b" }] }
        ]
    }));
    assert_eq!(check_edits(&old, &non_check, Some(&edit(&[2], "step 2 only opens the panel"))), Ok(()));

    // A whole step, checks and all.
    let dropped = script(json!({
        "case_id": 1, "title": "t",
        "steps": [
            { "step_number": 1, "actions": [
                { "kind": "check_text", "value": "Saved" },
                { "kind": "expect_visible", "selector": "#a" }
            ] }
        ]
    }));
    assert_eq!(check_edits(&old, &dropped, Some(&edit(&[2], "the case no longer has a step 2"))), Ok(()));

    // Undeclared, the same change is refused.
    let err = check_edits(&old, &removed, Some(&edit(&[], "tidied up"))).unwrap_err();
    assert_eq!(
        err,
        "step 1 was changed but not declared - name every step you change in \"edits\", or leave it as it was"
    );
    let err = check_edits(&old, &removed, None).unwrap_err();
    assert!(err.starts_with("case 1: step 1 was changed but not declared"), "{err}");
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
fn reordering_the_steps_is_refused() {
    // Same three steps, same content, only the order in the file changed -
    // rule 1's signature comparison alone would see nothing different.
    let step = |n: i32| {
        serde_json::json!({ "step_number": n, "actions": [{ "kind": "click", "selector": format!("#s{n}") }] })
    };
    let old = script(json!({
        "case_id": 1, "title": "t",
        "steps": [step(1), step(2), step(3)]
    }));
    let new = script(json!({
        "case_id": 1, "title": "t",
        "steps": [step(3), step(1), step(2)]
    }));

    let err = check_edits(&old, &new, None).unwrap_err();
    assert_eq!(err, "the steps are in a different order - a repair does not reorder a script");

    // A declaration naming every step does not excuse it either - a
    // reorder is refused outright, not something `edits` can sign off on.
    let declared = edit(&[1, 2, 3], "reordered for readability");
    let err = check_edits(&old, &new, Some(&declared)).unwrap_err();
    assert_eq!(err, "the steps are in a different order - a repair does not reorder a script");
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

fn in_area(area: Option<&str>) -> CaseScript {
    let mut sc = script(json!({
        "case_id": 1, "title": "t",
        "steps": [ { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] } ]
    }));
    sc.area = area.map(str::to_string);
    sc
}

fn area_edit(steps: &[i32], why: &str) -> Edit {
    Edit { area: true, ..edit(steps, why) }
}

fn undeclared_area(from: &str, to: &str) -> String {
    format!(
        "case 1: the area changed from {from} to {to} but was not declared - add \"area\": true to the case's \"edits\" entry, or leave the area as it was"
    )
}

/// Where a case starts is part of what it does: a repair that moves it to
/// another area has to say so in its declaration, as a changed step does.
/// A declaration for the case that does not say so is not enough.
#[test]
fn an_area_change_needs_its_own_declaration() {
    let module = "the case's Module";
    assert_eq!(
        check_edits(&in_area(None), &in_area(Some("Manage Cycle")), None),
        Err(undeclared_area(module, "\"Manage Cycle\""))
    );
    assert_eq!(
        check_edits(&in_area(Some("Cycle Setup")), &in_area(Some("Manage Cycle")), Some(&edit(&[], "moved"))),
        Err(undeclared_area("\"Cycle Setup\"", "\"Manage Cycle\""))
    );
    assert_eq!(
        check_edits(&in_area(Some("Cycle Setup")), &in_area(Some("Manage Cycle")), Some(&area_edit(&[], "moved"))),
        Ok(())
    );
}

/// Review of Task 8: a repair that leaves `area` out while the saved
/// script has one is a change to the area, like leaving out `account` is a
/// change to the account - never a silent erase. Declaring only the steps
/// it changed does not cover it.
#[test]
fn leaving_out_a_saved_area_is_a_change_not_a_silent_erase() {
    let mut changed_step = in_area(None);
    changed_step.steps[0].actions = vec![serde_json::from_value(json!({ "kind": "click", "selector": "#b" })).unwrap()];
    assert_eq!(
        check_edits(&in_area(Some("Manage Cycle")), &changed_step, Some(&edit(&[1], "the button moved"))),
        Err(undeclared_area("\"Manage Cycle\"", "the case's Module"))
    );
    assert_eq!(
        check_edits(&in_area(Some("Manage Cycle")), &changed_step, Some(&area_edit(&[1], "the button moved; the Module's own area is right"))),
        Ok(())
    );
}

/// A blank area is no area, and the same name in another case is the same
/// area: neither is a change. An area declared but not changed is refused,
/// as a step declared but not changed is.
#[test]
fn a_blank_area_or_one_in_another_case_is_no_change() {
    assert_eq!(check_edits(&in_area(None), &in_area(Some("  ")), None), Ok(()));
    assert_eq!(check_edits(&in_area(Some("Manage Cycle")), &in_area(Some(" manage cycle ")), None), Ok(()));
    assert_eq!(
        check_edits(&in_area(Some("Manage Cycle")), &in_area(Some("Manage Cycle")), Some(&area_edit(&[], "nothing"))),
        Err("the area was declared but not changed".to_string())
    );
}

/// A repair with no precondition edit keeps every precondition the saved
/// script has, exactly, and may add new ones: dropping one, or changing
/// its value or its reason, is refused.
#[test]
fn a_repair_may_add_preconditions_but_never_drop_or_change_one() {
    use v2_lib::autorun::edits::PRECONDITIONS_KEPT;
    assert_eq!(
        PRECONDITIONS_KEPT,
        "a precondition was removed or changed but not declared - add one \"precondition\" edit ({ \"op\": \"remove\" or \"change\", \"flow\", \"stage\", \"to\" for a change, \"reason\" }) to the case's \"edits\" entry, or send every saved precondition back as it is"
    );
    let with = |pre: serde_json::Value| {
        script(json!({
            "case_id": 1, "title": "t", "preconditions": pre,
            "steps": [{ "step_number": 1, "actions": [{ "kind": "click", "selector": "#old" }] }]
        }))
    };
    let publish = json!([{ "flow": "pms-performance-cycle", "stage": "publish", "value": 274 }]);
    let old = with(publish.clone());
    let declared = edit(&[1], "the button moved");
    let mut fixed = with(publish.clone());
    fixed.steps[0] = serde_json::from_value(json!({ "step_number": 1, "actions": [{ "kind": "click", "selector": "#new" }] })).unwrap();
    assert_eq!(check_edits(&old, &fixed, Some(&declared)), Ok(()), "the same preconditions sent back");

    let mut dropped = fixed.clone();
    dropped.preconditions.clear();
    assert_eq!(check_edits(&old, &dropped, Some(&declared)), Err(PRECONDITIONS_KEPT.to_string()));

    let mut changed = fixed.clone();
    changed.preconditions[0].value = json!(275);
    assert_eq!(check_edits(&old, &changed, Some(&declared)), Err(PRECONDITIONS_KEPT.to_string()));

    let mut reasoned = fixed.clone();
    reasoned.preconditions[0].why = Some("a new reason".into());
    assert_eq!(check_edits(&old, &reasoned, Some(&declared)), Err(PRECONDITIONS_KEPT.to_string()));

    // Added: to a script with none, or beside the one it has.
    let none = with(json!([]));
    let added = with(publish.clone());
    assert_eq!(check_edits(&none, &added, None), Ok(()));
    let mut another = fixed.clone();
    another.preconditions.insert(0, serde_json::from_value(json!({ "flow": "pms-performance-cycle", "stage": "rules", "value": 274 })).unwrap());
    assert_eq!(check_edits(&old, &another, Some(&declared)), Ok(()), "kept, with one added in front");
}

/// #46: making a step the clean-up, or no longer the clean-up, changes it.
#[test]
fn turning_a_step_into_the_clean_up_is_a_change_to_that_step() {
    let steps = |cleanup: bool| {
        json!({
            "case_id": 1, "title": "t",
            "steps": [
                { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] },
                { "step_number": 2, "actions": [{ "kind": "click", "selector": "#tidy" }], "unchecked": "tidy", "cleanup": cleanup }
            ]
        })
    };
    let (old, new) = (script(steps(false)), script(steps(true)));
    assert_eq!(check_edits(&old, &new, None), Err("case 1: step 2 was changed but not declared - name every step you change in \"edits\", or leave it as it was".to_string()));
    assert_eq!(check_edits(&old, &new, Some(&edit(&[2], "it tidies up"))), Ok(()));
}

// ------------------------------------------------ precondition edits (#28)

fn pre(stage: &str, value: serde_json::Value) -> v2_lib::autorun::Precondition {
    serde_json::from_value(json!({ "flow": "pms-performance-cycle", "stage": stage, "value": value })).unwrap()
}

fn with_pre(pre: Vec<v2_lib::autorun::Precondition>) -> CaseScript {
    let mut s = script(json!({
        "case_id": 1, "title": "t",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "click", "selector": "#old" }] }]
    }));
    s.preconditions = pre;
    s
}

fn pre_edit(v: serde_json::Value) -> PreconditionEdit {
    serde_json::from_value(v).unwrap()
}

/// The declaration of a precondition edit alone: no steps, no `why`.
fn pre_only(v: serde_json::Value) -> Edit {
    Edit { case_id: 1, steps: vec![], why: String::new(), quirk: None, quirk_class: None, area: false, precondition: Some(pre_edit(v)), gives_up: Vec::new() }
}

/// Remove, add and change: each accepted with a reason, and each comes back
/// as the record the script keeps.
#[test]
fn a_declared_precondition_edit_may_add_remove_or_change_one() {
    let old = with_pre(vec![pre("publish", json!(274)), pre("rules", json!(274))]);

    // Removed.
    let remove = json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "reason": "step 1 now publishes the cycle itself" });
    let (after, record) = apply_precondition_edit(&old.preconditions, &pre_edit(remove.clone())).unwrap();
    assert_eq!(after, vec![pre("rules", json!(274))]);
    assert_eq!(
        record,
        PreconditionRepair {
            op: PreconditionOp::Remove,
            before: Some(pre("publish", json!(274))),
            after: None,
            reason: "step 1 now publishes the cycle itself".into()
        }
    );
    assert_eq!(check_edits(&old, &with_pre(after), Some(&pre_only(remove))), Ok(()));

    // Added, at the end.
    let add = json!({ "op": "add", "to": { "flow": "pms-performance-cycle", "stage": "participants", "value": 274 }, "reason": "the case opens a participant" });
    let (after, record) = apply_precondition_edit(&old.preconditions, &pre_edit(add.clone())).unwrap();
    assert_eq!(after.last(), Some(&pre("participants", json!(274))));
    assert_eq!((record.op, record.before, record.after), (PreconditionOp::Add, None, Some(pre("participants", json!(274)))));
    assert_eq!(check_edits(&old, &with_pre(after), Some(&pre_only(add))), Ok(()));

    // Changed, in its place.
    let change = json!({ "op": "change", "flow": "pms-performance-cycle", "stage": "publish", "to": { "flow": "pms-performance-cycle", "stage": "competencies", "value": 274 }, "reason": "the steps assign the profile themselves now" });
    let (after, record) = apply_precondition_edit(&old.preconditions, &pre_edit(change.clone())).unwrap();
    assert_eq!(after, vec![pre("competencies", json!(274)), pre("rules", json!(274))]);
    assert_eq!(
        (record.op, record.before, record.after),
        (PreconditionOp::Change, Some(pre("publish", json!(274))), Some(pre("competencies", json!(274))))
    );
    assert_eq!(check_edits(&old, &with_pre(after.clone()), Some(&pre_only(change.clone()))), Ok(()));

    // Beside a step edit, with its own `why`.
    let mut fixed = with_pre(after);
    fixed.steps[0] = serde_json::from_value(json!({ "step_number": 1, "actions": [{ "kind": "click", "selector": "#new" }] })).unwrap();
    let both = Edit { precondition: Some(pre_edit(change)), ..edit(&[1], "the button moved") };
    assert_eq!(check_edits(&old, &fixed, Some(&both)), Ok(()));
}

/// A removal - any precondition edit - says why in one line.
#[test]
fn a_precondition_edit_without_a_reason_is_refused() {
    use v2_lib::autorun::edits::{PRECONDITION_NEEDS_REASON, PRECONDITION_REASON_LONG};
    let saved = vec![pre("publish", json!(274))];
    for reason in ["", "   "] {
        let e = pre_edit(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "reason": reason }));
        assert_eq!(apply_precondition_edit(&saved, &e), Err(PRECONDITION_NEEDS_REASON.to_string()));
    }
    let e = pre_edit(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish" }));
    assert_eq!(apply_precondition_edit(&saved, &e), Err(PRECONDITION_NEEDS_REASON.to_string()));
    assert_eq!(
        check_edits(
            &with_pre(saved.clone()),
            &with_pre(vec![]),
            Some(&pre_only(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish" })))
        ),
        Err(PRECONDITION_NEEDS_REASON.to_string())
    );
    for reason in ["two\nlines".to_string(), "x".repeat(201)] {
        let e = pre_edit(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "reason": reason }));
        assert_eq!(apply_precondition_edit(&saved, &e), Err(PRECONDITION_REASON_LONG.to_string()));
    }
    let e = pre_edit(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "reason": "x".repeat(200) }));
    assert!(apply_precondition_edit(&saved, &e).is_ok(), "200 characters is within the limit");
    // A precondition edit's reason stands in for `why`, but a step edit
    // still needs its own.
    let mut fixed = with_pre(vec![]);
    fixed.steps[0] = serde_json::from_value(json!({ "step_number": 1, "actions": [{ "kind": "click", "selector": "#new" }] })).unwrap();
    let both = Edit {
        precondition: Some(pre_edit(
            json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "reason": "no longer needed" }),
        )),
        ..edit(&[1], " ")
    };
    assert_eq!(check_edits(&with_pre(saved), &fixed, Some(&both)), Err("an edit needs a reason".to_string()));
}

/// One precondition edit per save: a second change to the preconditions
/// beside the declared one - another removal, an add, a value changed - is
/// refused, as an undeclared step is.
#[test]
fn a_second_precondition_change_beside_the_declared_one_is_refused() {
    use v2_lib::autorun::edits::PRECONDITIONS_UNDECLARED;
    let old = with_pre(vec![pre("publish", json!(274)), pre("rules", json!(274))]);
    let declared = pre_only(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "reason": "the steps publish it" }));
    for sent in [
        vec![],
        vec![pre("rules", json!(274)), pre("participants", json!(274))],
        vec![pre("rules", json!(275))],
        vec![pre("publish", json!(274)), pre("rules", json!(274))],
    ] {
        assert_eq!(
            check_edits(&old, &with_pre(sent.clone()), Some(&declared)),
            Err(PRECONDITIONS_UNDECLARED.to_string()),
            "{sent:?}"
        );
    }
    // Undeclared, a removal is refused as it always was.
    assert_eq!(
        check_edits(&old, &with_pre(vec![pre("rules", json!(274))]), None),
        Err(v2_lib::autorun::edits::PRECONDITIONS_KEPT.to_string())
    );
}

/// An edit that names nothing it can act on is refused in words that say
/// what to send instead.
#[test]
fn a_precondition_edit_that_names_nothing_it_can_act_on_is_refused() {
    let saved = vec![pre("publish", json!(274)), pre("publish", json!(275))];
    let refused = |v: serde_json::Value| apply_precondition_edit(&saved, &pre_edit(v)).unwrap_err();
    assert_eq!(
        refused(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "rules", "reason": "r" })),
        "the script has no precondition pms-performance-cycle / rules"
    );
    assert_eq!(
        refused(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "reason": "r" })),
        "the script has more than one precondition pms-performance-cycle / publish - name its \"value\" too"
    );
    let (after, _) = apply_precondition_edit(
        &saved,
        &pre_edit(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "value": 275, "reason": "r" })),
    )
    .unwrap();
    assert_eq!(after, vec![pre("publish", json!(274))], "the value picks one of two");
    assert_eq!(
        refused(json!({ "op": "remove", "stage": "publish", "reason": "r" })),
        "a \"remove\" precondition edit names the saved precondition by its \"flow\" and \"stage\""
    );
    assert_eq!(
        refused(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "value": 274, "to": { "flow": "f", "stage": "s", "value": 1 }, "reason": "r" })),
        "a \"remove\" precondition edit takes no \"to\""
    );
    assert_eq!(
        refused(json!({ "op": "add", "reason": "r" })),
        "an \"add\" precondition edit needs \"to\": the precondition to add, { flow, stage, value, why }"
    );
    assert_eq!(
        refused(json!({ "op": "add", "flow": "pms-performance-cycle", "to": { "flow": "f", "stage": "s", "value": 1 }, "reason": "r" })),
        "an \"add\" precondition edit takes the new precondition in \"to\" and names no saved one"
    );
    assert_eq!(
        refused(json!({ "op": "add", "to": { "flow": "pms-performance-cycle", "stage": "publish", "value": 274 }, "reason": "r" })),
        "the script already has that precondition"
    );
    assert_eq!(
        refused(json!({ "op": "change", "flow": "pms-performance-cycle", "stage": "publish", "value": 274, "reason": "r" })),
        "a \"change\" precondition edit needs \"to\": the precondition as it should be"
    );
    assert_eq!(
        refused(json!({ "op": "change", "flow": "pms-performance-cycle", "stage": "publish", "value": 274, "to": { "flow": "pms-performance-cycle", "stage": "publish", "value": 274 }, "reason": "r" })),
        "the precondition edit changes nothing - \"to\" is the precondition as it is"
    );
    // An unknown key, or op, is serde's refusal.
    assert!(serde_json::from_value::<PreconditionEdit>(json!({ "op": "drop", "reason": "r" })).is_err());
    assert!(serde_json::from_value::<PreconditionEdit>(json!({ "op": "add", "why": "r" })).is_err());
}

/// A change into a precondition the script already has (another one, not
/// the one changed) is refused as an add of it is; changing only the
/// changed one's `why` is not a duplicate.
#[test]
fn a_change_into_a_precondition_the_script_has_is_refused() {
    let saved = vec![pre("publish", json!(274)), pre("rules", json!(274))];
    let into_rules = pre_edit(json!({
        "op": "change", "flow": "pms-performance-cycle", "stage": "publish",
        "to": { "flow": "pms-performance-cycle", "stage": "rules", "value": 274, "why": "another reason" },
        "reason": "r"
    }));
    assert_eq!(apply_precondition_edit(&saved, &into_rules), Err("the script already has that precondition".to_string()));
    let reworded = pre_edit(json!({
        "op": "change", "flow": "pms-performance-cycle", "stage": "publish",
        "to": { "flow": "pms-performance-cycle", "stage": "publish", "value": 274, "why": "it opens a published cycle" },
        "reason": "r"
    }));
    assert!(apply_precondition_edit(&saved, &reworded).is_ok());
}

/// The preconditions are compared as a set: a script sent with the
/// declared change made, in another order, is that change.
#[test]
fn a_declared_precondition_edit_accepts_the_preconditions_in_any_order() {
    let old = with_pre(vec![pre("publish", json!(274)), pre("rules", json!(274)), pre("setup", json!(274))]);
    let declared = pre_only(json!({ "op": "remove", "flow": "pms-performance-cycle", "stage": "publish", "reason": "the steps publish it" }));
    let reordered = with_pre(vec![pre("setup", json!(274)), pre("rules", json!(274))]);
    assert_eq!(check_edits(&old, &reordered, Some(&declared)), Ok(()));
    use v2_lib::autorun::edits::same_preconditions;
    assert!(same_preconditions(&[pre("a", json!(1)), pre("b", json!(1))], &[pre("b", json!(1)), pre("a", json!(1))]));
    assert!(!same_preconditions(&[pre("a", json!(1)), pre("a", json!(1))], &[pre("a", json!(1)), pre("b", json!(1))]));
    assert!(!same_preconditions(&[pre("a", json!(1))], &[pre("a", json!(1)), pre("a", json!(1))]));
}

/// A repair keeps every needed record the saved script declares, exactly,
/// and may add new ones.
#[test]
fn a_repair_may_add_needed_records_but_never_drop_or_change_one() {
    use v2_lib::autorun::edits::NEEDS_RECORDS_KEPT;
    let with = |needs: serde_json::Value| {
        script(json!({
            "case_id": 1, "title": "t", "needs_records": needs,
            "steps": [{ "step_number": 1, "actions": [{ "kind": "click", "selector": "#old" }] }]
        }))
    };
    let scale = json!([{ "setup": "Rating scale", "why": "the scale" }]);
    let old = with(scale.clone());
    let declared = edit(&[1], "the button moved");
    let mut fixed = with(scale.clone());
    fixed.steps[0] = serde_json::from_value(json!({ "step_number": 1, "actions": [{ "kind": "click", "selector": "#new" }] })).unwrap();
    assert_eq!(check_edits(&old, &fixed, Some(&declared)), Ok(()));

    let mut dropped = fixed.clone();
    dropped.needs_records.clear();
    assert_eq!(check_edits(&old, &dropped, Some(&declared)), Err(NEEDS_RECORDS_KEPT.to_string()));

    let mut changed = fixed.clone();
    changed.needs_records[0].why = "something else".into();
    assert_eq!(check_edits(&old, &changed, Some(&declared)), Err(NEEDS_RECORDS_KEPT.to_string()));

    let mut added = fixed.clone();
    added.needs_records.push(v2_lib::autorun::NeededRecord { setup: "Tree".into(), why: "the tree".into() });
    assert_eq!(check_edits(&old, &added, Some(&declared)), Ok(()));
    // A script that had none may gain some.
    let mut first = fixed.clone();
    first.steps = old.steps.clone();
    assert_eq!(check_edits(&with(json!([])), &first, None), Ok(()));
}
