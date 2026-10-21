//! Auto Run run batches: settings a case needs (`needs_state`) group the
//! cases into batches, and the run pauses before each batch to set what
//! changes (and to put back what a reset needs). Pure planner tests, plus
//! the command's half that reads the settings from saved scripts.

use std::collections::{BTreeMap, HashMap};
use v2_lib::autorun::plan::{batches, batches_for, plan_for, Batches, Marks, Needs, Pause};
use v2_lib::autorun::{store, CaseScript, StepScript};
use v2_lib::browser::actions::Action;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn m(changes: &[&str], needs: &[&str]) -> Marks {
    Marks { changes: names(changes), needs: names(needs) }
}

fn s(pairs: &[(&str, &str)]) -> Needs {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn set(before: i32, batch: u32, pairs: &[(&str, &str)]) -> Pause {
    Pause { batch, before_case_id: before, set: s(pairs), ..Pause::default() }
}

const AW: &str = "Approval workflow";
const RS: &str = "Rating scale";

// ------------------------------------------------------------ grouping

/// Two settings, interleaved in the list: the cases that agree run
/// together, so the run pauses once before Batch 1 and once between the
/// two batches, instead of before almost every case.
#[test]
fn two_settings_group_the_cases_into_the_fewest_batches() {
    let cases = vec![
        (1, m(&[], &[]), s(&[(AW, "ON")])),
        (2, m(&[], &[]), s(&[(AW, "OFF")])),
        (3, m(&[], &[]), s(&[(AW, "ON"), (RS, "5 point")])),
        (4, m(&[], &[]), s(&[(AW, "OFF")])),
        (5, m(&[], &[]), s(&[(RS, "3 point")])),
        (6, m(&[], &[]), s(&[])),
    ];
    let (plan, counts) = batches_for(&cases, None);
    assert_eq!(plan.order, vec![1, 3, 6, 2, 4, 5]);
    assert_eq!(plan.batches, vec![vec![1, 3, 6], vec![2, 4, 5]]);
    assert_eq!(
        plan.pauses,
        vec![
            set(1, 1, &[(AW, "ON"), (RS, "5 point")]),
            set(2, 2, &[(AW, "OFF"), (RS, "3 point")]),
        ]
    );
    assert_eq!(counts, None);

    // In list order the same cases need four pauses.
    let wants: HashMap<i32, Needs> = cases.iter().map(|(id, _, n)| (*id, n.clone())).collect();
    let marks: HashMap<i32, Marks> = cases.iter().map(|(id, mk, _)| (*id, mk.clone())).collect();
    assert_eq!(batches(&[1, 2, 3, 4, 5, 6], &marks, &wants).pauses.len(), 4);
}

/// A case that names no setting does not care: it runs wherever list
/// order puts it, and a single batch needs one pause, before it starts.
#[test]
fn cases_that_do_not_care_stay_in_list_order_in_one_batch() {
    let cases = vec![(1, m(&[], &[]), s(&[])), (2, m(&[], &[]), s(&[(AW, "ON")])), (3, m(&[], &[]), s(&[]))];
    let (plan, _) = batches_for(&cases, None);
    assert_eq!(plan.order, vec![1, 2, 3]);
    assert_eq!(plan.batches, vec![vec![1, 2, 3]]);
    // Before Batch 1: make sure the setting is on. It names the batch's
    // first case, even though that case itself does not care.
    assert_eq!(plan.pauses, vec![set(1, 1, &[(AW, "ON")])]);
}

/// Names and values compare by `normalise`, and show the spelling of the
/// lowest case id.
#[test]
fn settings_compare_like_marks_and_show_the_first_spelling() {
    let cases = vec![
        (4, m(&[], &[]), s(&[(" approval  WORKFLOW ", "on")])),
        (2, m(&[], &[]), s(&[(AW, "ON")])),
    ];
    let (plan, _) = batches_for(&cases, None);
    assert_eq!(plan.batches, vec![vec![4, 2]]);
    assert_eq!(plan.pauses, vec![set(4, 1, &[(AW, "ON")])]);
}

/// A reset no order avoids (a cycle) and a setting that changes at the same
/// point share one pause: set the setting and put the state back together.
#[test]
fn a_reset_and_a_setting_change_share_one_pause() {
    let cases = vec![
        (1, m(&["cycle published"], &["rota"]), s(&[(AW, "ON")])),
        (2, m(&["rota"], &["cycle published"]), s(&[(AW, "OFF")])),
    ];
    let (plan, _) = batches_for(&cases, None);
    assert_eq!(plan.order, vec![1, 2]);
    assert_eq!(
        plan.pauses,
        vec![
            set(1, 1, &[(AW, "ON")]),
            Pause {
                batch: 2,
                before_case_id: 2,
                set: s(&[(AW, "OFF")]),
                names: names(&["cycle published"]),
                changed_by: vec![("cycle published".into(), vec![1])],
            },
        ]
    );
    // The reset plan of the same selection still has just the reset.
    let plain = plan.clone().into_plan();
    assert_eq!(plain.phases, vec![vec![1], vec![2]]);
    assert_eq!(plain.resets.len(), 1);
}

/// A batch that needs nothing new after a reset pauses only to put back;
/// a setting already set stays set and is not asked again.
#[test]
fn a_setting_already_set_is_not_asked_again() {
    let cases = vec![
        (1, m(&["x"], &["y"]), s(&[(AW, "ON")])),
        (2, m(&["y"], &["x"]), s(&[(AW, "ON")])),
    ];
    let (plan, _) = batches_for(&cases, None);
    assert_eq!(plan.pauses.len(), 2);
    assert_eq!(plan.pauses[1].set, vec![]);
    assert_eq!(plan.pauses[1].names, names(&["x"]));
}

/// The grouping keeps every needer-before-changer constraint: a needer of
/// X never moves after its changer to save a settings pause.
#[test]
fn grouping_never_breaks_a_needs_unchanged_constraint() {
    let cases = vec![
        (1, m(&[], &["x"]), s(&[(AW, "ON")])),
        (2, m(&["x"], &[]), s(&[(AW, "OFF")])),
        (3, m(&[], &[]), s(&[(AW, "ON")])),
    ];
    let (plan, _) = batches_for(&cases, None);
    let at = |id: i32| plan.order.iter().position(|&o| o == id).unwrap();
    assert!(at(1) < at(2), "{:?}", plan.order);
    assert!(plan.pauses.iter().all(|p| p.names.is_empty()), "{:?}", plan.pauses);
    assert_eq!(plan.order, vec![1, 3, 2]);
}

/// On a tie the suggested order stays: another order with as few pauses
/// never replaces it.
#[test]
fn a_tie_keeps_the_suggested_order() {
    let cases = vec![
        (1, m(&[], &[]), s(&[(AW, "ON")])),
        (2, m(&[], &[]), s(&[(RS, "5 point")])),
        (3, m(&[], &[]), s(&[(AW, "OFF")])),
    ];
    let (plan, _) = batches_for(&cases, None);
    assert_eq!(plan.order, vec![1, 2, 3]);
    assert_eq!(plan.batches, vec![vec![1, 2], vec![3]]);
}

/// The same selection plans the same way every time, whatever order its
/// settings maps were written in.
#[test]
fn the_plan_is_deterministic() {
    let cases = vec![
        (1, m(&[], &[]), s(&[(AW, "ON"), (RS, "3 point")])),
        (2, m(&[], &[]), s(&[(RS, "5 point")])),
        (3, m(&[], &[]), s(&[(AW, "OFF")])),
        (4, m(&[], &[]), s(&[(RS, "3 point"), (AW, "ON")])),
    ];
    let first = batches_for(&cases, None);
    for _ in 0..5 {
        assert_eq!(batches_for(&cases, None), first);
    }
}

/// A saved order is honoured: its own pauses are worked out, and the counts
/// say it needs more than the suggestion.
#[test]
fn a_saved_order_is_honoured_and_counted_in_pauses() {
    let cases = vec![
        (1, m(&[], &[]), s(&[(AW, "ON")])),
        (2, m(&[], &[]), s(&[(AW, "OFF")])),
        (3, m(&[], &[]), s(&[(AW, "ON")])),
    ];
    assert_eq!(batches_for(&cases, None).0.order, vec![1, 3, 2]);
    let (plan, counts) = batches_for(&cases, Some(&[1, 2, 3]));
    assert_eq!(plan.order, vec![1, 2, 3]);
    assert_eq!(plan.batches, vec![vec![1], vec![2], vec![3]]);
    assert_eq!(
        plan.pauses,
        vec![set(1, 1, &[(AW, "ON")]), set(2, 2, &[(AW, "OFF")]), set(3, 3, &[(AW, "ON")])]
    );
    assert_eq!(counts, Some((3, 2)));
    // As good as the suggestion: no counts.
    assert_eq!(batches_for(&cases, Some(&[3, 1, 2])).1, None);
}

/// A pause that only switches settings puts nothing back: what was changed
/// stays changed, so the reset a later case needs still comes, at the pause
/// where it is due.
#[test]
fn a_settings_pause_does_not_drop_a_reset_still_due() {
    // Saved order A, B, C: A changes X and needs S=1, B needs S=2, C needs X
    // unchanged.
    let cases = vec![
        (1, m(&["x"], &[]), s(&[("S", "1")])),
        (2, m(&[], &[]), s(&[("S", "2")])),
        (3, m(&[], &["x"]), s(&[])),
    ];
    let (plan, _) = batches_for(&cases, Some(&[1, 2, 3]));
    assert_eq!(plan.batches, vec![vec![1], vec![2], vec![3]]);
    assert_eq!(
        plan.pauses,
        vec![
            set(1, 1, &[("S", "1")]),
            set(2, 2, &[("S", "2")]),
            Pause {
                batch: 3,
                before_case_id: 3,
                set: vec![],
                names: names(&["x"]),
                changed_by: vec![("x".into(), vec![1])],
            },
        ]
    );
}

/// The suggested order counts the resets a settings pause no longer hides,
/// and puts a reset that is due anyway on the same pause as the settings.
#[test]
fn the_suggestion_counts_resets_behind_settings_pauses() {
    // 1 and 3 form a cycle (one reset whatever the order); 2 and 3 need
    // the same setting.
    let cases = vec![
        (1, m(&["x"], &["y"]), s(&[("S", "1")])),
        (2, m(&[], &[]), s(&[("S", "2")])),
        (3, m(&["y"], &["x"]), s(&[("S", "2")])),
    ];
    // In list order the reset needs a pause of its own: three in all.
    let marks: HashMap<i32, Marks> = cases.iter().map(|(id, mk, _)| (*id, mk.clone())).collect();
    let wants: HashMap<i32, Needs> = cases.iter().map(|(id, _, n)| (*id, n.clone())).collect();
    let listed = batches(&[1, 2, 3], &marks, &wants);
    assert_eq!(listed.pauses.len(), 3);
    assert_eq!(listed.pauses[2].names, names(&["x"]));

    let (plan, _) = batches_for(&cases, None);
    assert_eq!(plan.order, vec![1, 3, 2]);
    assert_eq!(
        plan.pauses,
        vec![
            set(1, 1, &[("S", "1")]),
            Pause {
                batch: 2,
                before_case_id: 3,
                set: s(&[("S", "2")]),
                names: names(&["x"]),
                changed_by: vec![("x".into(), vec![1])],
            },
        ]
    );
}

/// Without settings, the batches are the reset plan exactly.
#[test]
fn without_settings_the_batches_are_the_reset_plan() {
    let marks = vec![(1, m(&["x"], &["y"])), (2, m(&["y"], &["x"])), (3, m(&[], &[]))];
    let with: Vec<(i32, Marks, Needs)> = marks.iter().map(|(id, mk)| (*id, mk.clone(), Vec::new())).collect();
    for saved in [None, Some(&[3, 2, 1][..])] {
        let (b, counts) = batches_for(&with, saved);
        let (p, plain_counts) = plan_for(&marks, saved);
        assert_eq!(counts, plain_counts);
        assert!(b.pauses.iter().all(|x| x.set.is_empty() && x.batch >= 2));
        assert_eq!(Batches::into_plan(b), p);
    }
}

// ------------------------------------------------------------ the command

fn script(case_id: i32, state: &[(&str, &str)]) -> CaseScript {
    let mut sc: CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": case_id,
        "title": format!("Case {case_id}"),
        "steps": [],
    }))
    .unwrap();
    sc.steps = vec![StepScript {
        step_number: 1,
        actions: vec![Action::Navigate { url: "https://app.example/".into() }],
        unchecked: None,
        cleanup: false,
        because: Vec::new(),
    }];
    sc.needs_state = state.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>();
    sc
}

/// The plan command reads each case's settings from its saved script.
#[test]
fn the_plan_command_reads_settings_from_the_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &script(1, &[(AW, "ON")])).unwrap();
    store::save_script(root, &script(2, &[(AW, "OFF")])).unwrap();
    store::save_script(root, &script(3, &[(AW, "ON")])).unwrap();
    let view = v2_lib::commands::autorun::plan_at(root, 100, &[1, 2, 3], None);
    assert_eq!(view.order, vec![1, 3, 2]);
    assert_eq!(view.phases, vec![vec![1, 3], vec![2]]);
    assert_eq!(view.pauses, vec![set(1, 1, &[(AW, "ON")]), set(2, 2, &[(AW, "OFF")])]);

    let view = v2_lib::commands::autorun::plan_at(root, 100, &[1, 2, 3], Some(&[1, 2, 3][..]));
    assert_eq!(view.counts, Some((3, 2)));
}
