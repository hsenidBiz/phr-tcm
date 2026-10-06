//! The Auto Run planner: a selection of cases and their scripts' marks in,
//! an order and its reset points out. See the design "Auto Run reset
//! phases" §2 and §5. Pure: no browser, no Azure DevOps, no store - except
//! the command's own half at the end, which reads marks and the saved
//! order from a store on disk.

use std::collections::HashMap;
use v2_lib::autorun::plan::{phases, plan_for, suggest, Marks, Plan, Reset};
use v2_lib::autorun::{store, CaseScript, StepScript};
use v2_lib::browser::actions::Action;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn m(changes: &[&str], needs: &[&str]) -> Marks {
    Marks { changes: names(changes), needs: names(needs) }
}

fn none() -> Marks {
    Marks::default()
}

fn map(cases: &[(i32, Marks)]) -> HashMap<i32, Marks> {
    cases.iter().cloned().collect()
}

fn reset(before: i32, changed: &[(&str, &[i32])]) -> Reset {
    Reset {
        before_case_id: before,
        names: changed.iter().map(|(n, _)| n.to_string()).collect(),
        changed_by: changed.iter().map(|(n, ids)| (n.to_string(), ids.to_vec())).collect(),
    }
}

// ------------------------------------------------------------ the suggested order

/// No marks: list order, one phase, nothing else.
#[test]
fn no_marks_keeps_list_order_in_one_phase() {
    let cases = vec![(3, none()), (1, none()), (2, none())];
    assert_eq!(suggest(&cases), vec![3, 1, 2]);
    let (plan, counts) = plan_for(&cases, None);
    assert_eq!(plan, Plan { order: vec![3, 1, 2], phases: vec![vec![3, 1, 2]], resets: vec![] });
    assert_eq!(counts, None);
}

/// One change, with a needer before the changer and one after it: the one
/// after moves ahead of the changer, and nothing else moves.
#[test]
fn needers_of_a_name_run_before_its_changer() {
    let cases = vec![
        (1, m(&[], &["cycle published"])),
        (2, m(&["cycle published"], &[])),
        (3, m(&[], &["cycle published"])),
        (4, none()),
    ];
    assert_eq!(suggest(&cases), vec![1, 3, 2, 4]);
    let (plan, counts) = plan_for(&cases, None);
    assert_eq!(plan.phases, vec![vec![1, 3, 2, 4]]);
    assert!(plan.resets.is_empty());
    assert_eq!(counts, None);
}

/// A case that needs X unchanged and then changes X (publishing the cycle)
/// runs after the other needers of X and before the other changers of X.
#[test]
fn a_case_that_needs_and_changes_a_name_sits_between_its_needers_and_changers() {
    let cases = vec![
        (10, m(&["cycle published"], &["cycle published"])),
        (11, m(&["cycle published"], &[])),
        (12, m(&[], &["cycle published"])),
    ];
    assert_eq!(suggest(&cases), vec![12, 10, 11]);
    let (plan, _) = plan_for(&cases, None);
    assert_eq!(plan.phases, vec![vec![12, 10, 11]]);
    assert!(plan.resets.is_empty());
}

/// Two names, each with its own needers and changers, and a case needing
/// both: every needer runs before every changer of what it needs.
#[test]
fn two_names_order_every_needer_before_the_changers() {
    let cases = vec![
        (1, m(&["x"], &[])),
        (2, m(&[], &["x"])),
        (3, m(&["y"], &[])),
        (4, m(&[], &["y"])),
        (5, m(&[], &["x", "y"])),
    ];
    assert_eq!(suggest(&cases), vec![2, 4, 5, 1, 3]);
    let (plan, counts) = plan_for(&cases, None);
    assert_eq!(plan.phases, vec![vec![2, 4, 5, 1, 3]]);
    assert!(plan.resets.is_empty());
    assert_eq!(counts, None);
}

/// A changes X and needs Y unchanged, while B changes Y and needs X
/// unchanged: no order avoids a reset. The backward constraint (B before A)
/// is the one dropped, so list order stands and the reset falls before B.
#[test]
fn a_two_case_cycle_is_broken_at_the_earliest_case_and_becomes_one_reset() {
    let cases = vec![(10, m(&["x"], &["y"])), (20, m(&["y"], &["x"]))];
    assert_eq!(suggest(&cases), vec![10, 20]);
    let (plan, counts) = plan_for(&cases, None);
    assert_eq!(plan.order, vec![10, 20]);
    assert_eq!(plan.phases, vec![vec![10], vec![20]]);
    assert_eq!(plan.resets, vec![reset(20, &[("x", &[10])])]);
    assert_eq!(counts, None);
}

/// Three cases in a ring (1 needs what 3 changes, 3 needs what 2 changes,
/// 2 needs what 1 changes). List order would need two resets. The forward
/// constraint 1 before 3 is kept, then 2 before 1; 3 before 2 would close
/// the ring and is dropped, so the one reset falls before 3.
#[test]
fn a_three_case_cycle_is_broken_once() {
    let cases = vec![
        (1, m(&["x"], &["z"])),
        (2, m(&["y"], &["x"])),
        (3, m(&["z"], &["y"])),
    ];
    assert_eq!(suggest(&cases), vec![2, 1, 3]);
    let (plan, _) = plan_for(&cases, None);
    assert_eq!(plan.phases, vec![vec![2, 1], vec![3]]);
    assert_eq!(plan.resets, vec![reset(3, &[("y", &[2])])]);

    // The same three in list order, for comparison: two resets.
    let in_list = phases(&[1, 2, 3], &map(&cases));
    assert_eq!(in_list.resets.len(), 2);
}

/// A case outside a cycle is not moved by it: list order stands, and the
/// cycle costs one reset.
#[test]
fn an_unmarked_case_after_a_cycle_stays_last() {
    let cases = vec![(1, m(&["x"], &["y"])), (2, m(&["y"], &["x"])), (3, none())];
    assert_eq!(suggest(&cases), vec![1, 2, 3]);
    let (plan, _) = plan_for(&cases, None);
    assert_eq!(plan.phases, vec![vec![1], vec![2, 3]]);
    assert_eq!(plan.resets, vec![reset(2, &[("x", &[1])])]);
}

/// D changes z, A changes x and needs y and z unchanged, B changes y and
/// needs x unchanged. A before D is kept (no cycle); B before A would close
/// a cycle with A before B and is dropped. One reset, before B.
#[test]
fn a_backward_constraint_that_closes_no_cycle_is_kept_and_one_reset_follows() {
    let (d, a, b) = (30, 10, 20);
    let cases = vec![(d, m(&["z"], &[])), (a, m(&["x"], &["y", "z"])), (b, m(&["y"], &["x"]))];
    assert_eq!(suggest(&cases), vec![a, d, b]);
    let (plan, _) = plan_for(&cases, None);
    assert_eq!(plan.resets.len(), 1);
    assert_eq!(phases(&plan.order, &map(&cases)).resets.len(), 1);
    assert_eq!(plan.resets, vec![reset(b, &[("x", &[a])])]);
}

/// Two cases that each need and change the same name (two publishes)
/// cannot both run before the other: one reset between them.
#[test]
fn two_cases_that_both_need_and_change_a_name_need_a_reset_between_them() {
    let cases = vec![(7, m(&["cycle published"], &["cycle published"])), (8, m(&["cycle published"], &["cycle published"]))];
    assert_eq!(suggest(&cases), vec![7, 8]);
    let (plan, _) = plan_for(&cases, None);
    assert_eq!(plan.resets, vec![reset(8, &[("cycle published", &[7])])]);
}

/// A case listed twice is planned once, where it first appears.
#[test]
fn a_case_listed_twice_is_planned_once() {
    let cases = vec![(1, none()), (2, none()), (1, none())];
    assert_eq!(suggest(&cases), vec![1, 2]);
    assert_eq!(plan_for(&cases, None).0.phases, vec![vec![1, 2]]);
}

#[test]
fn an_empty_selection_plans_nothing() {
    let (plan, counts) = plan_for(&[], None);
    assert_eq!(plan, Plan { order: vec![], phases: vec![], resets: vec![] });
    assert_eq!(counts, None);
}

// ------------------------------------------------------------ phases

/// Each boundary names what to revert and, for each name, every case that
/// changed it since the last reset.
#[test]
fn a_reset_names_each_name_and_every_case_that_changed_it() {
    let marks = map(&[
        (1, m(&["x"], &[])),
        (2, m(&["x", "y"], &[])),
        (3, m(&[], &["y", "x"])),
        (4, m(&["y"], &[])),
        (5, m(&[], &["x", "y"])),
    ]);
    let plan = phases(&[1, 2, 3, 4, 5], &marks);
    assert_eq!(plan.phases, vec![vec![1, 2], vec![3, 4], vec![5]]);
    // Before 3: its needs, in its own order. Before 5: only y was changed
    // since the reset, by 4 alone.
    assert_eq!(
        plan.resets,
        vec![reset(3, &[("y", &[2]), ("x", &[1, 2])]), reset(5, &[("y", &[4])])]
    );
}

/// Review Focus 1: names that differ only in case or spacing are one name,
/// shown in the first spelling seen.
#[test]
fn names_that_differ_in_case_or_spacing_are_one_name_shown_as_first_spelled() {
    let cases = vec![
        (1, m(&["Cycle Published"], &[])),
        (2, m(&[], &[" cycle  published "])),
    ];
    // The needer moves ahead: the two spellings are one name.
    assert_eq!(suggest(&cases), vec![2, 1]);
    let plan = phases(&[1, 2], &map(&cases));
    assert_eq!(plan.resets, vec![reset(2, &[("Cycle Published", &[1])])]);
}

/// The spelling shown comes from the ids in ascending order, so a saved
/// order and the suggestion name a reset the same way.
#[test]
fn a_saved_order_shows_names_in_the_first_spelling_by_ascending_id() {
    let cases = vec![
        (1, m(&[], &["cycle published"])),
        (2, m(&["Cycle Published"], &[])),
        (3, m(&[], &["CYCLE published"])),
    ];
    let (plan, counts) = plan_for(&cases, Some(&[2, 3, 1]));
    assert_eq!(plan.resets, vec![reset(3, &[("cycle published", &[2])])]);
    assert_eq!(counts, Some((1, 0)));
    // Walking that order alone would show the changer's spelling.
    assert_eq!(phases(&[2, 3, 1], &map(&cases)).resets[0].names, vec!["Cycle Published".to_string()]);
}

/// The run dialog, the pause, the record and the report may each be sent
/// the cases in a different order: a name is spelled the same whichever.
#[test]
fn a_names_spelling_does_not_depend_on_the_order_the_ids_arrive_in() {
    // N (7) needs "Cycle published"; C (3) changes "cycle published".
    let needer = (7, m(&[], &["Cycle published"]));
    let changer = (3, m(&["cycle published"], &[]));
    let one = plan_for(&[needer.clone(), changer.clone()], Some(&[3, 7])).0;
    let other = plan_for(&[changer, needer], Some(&[3, 7])).0;
    assert_eq!(one.resets, vec![reset(7, &[("cycle published", &[3])])]);
    assert_eq!(other.resets, one.resets);
}

/// A case with no entry in the marks has none.
#[test]
fn a_case_missing_from_the_marks_has_none() {
    let plan = phases(&[1, 9, 2], &map(&[(1, m(&["x"], &[])), (2, m(&[], &["x"]))]));
    assert_eq!(plan.phases, vec![vec![1, 9], vec![2]]);
    assert_eq!(plan.resets, vec![reset(2, &[("x", &[1])])]);
}

/// Review Focus 3: only needers of one name and no changer - one phase, no
/// reset.
#[test]
fn needers_with_no_changer_are_one_phase() {
    let cases = vec![(1, m(&[], &["x"])), (2, m(&[], &["x"])), (3, m(&[], &["x"]))];
    assert_eq!(suggest(&cases), vec![1, 2, 3]);
    let (plan, counts) = plan_for(&cases, None);
    assert_eq!(plan, Plan { order: vec![1, 2, 3], phases: vec![vec![1, 2, 3]], resets: vec![] });
    assert_eq!(counts, None);
}

/// Review Focus 5: marks the order already satisfies change nothing - the
/// plan is the same as with no marks.
#[test]
fn marks_the_order_already_satisfies_give_the_plan_of_no_marks() {
    let marked = vec![(1, m(&[], &["x"])), (2, m(&["x"], &[])), (3, none())];
    let bare = vec![(1, none()), (2, none()), (3, none())];
    assert_eq!(plan_for(&marked, None), plan_for(&bare, None));
    assert_eq!(plan_for(&marked, Some(&[1, 2, 3])), plan_for(&bare, Some(&[1, 2, 3])));
    assert_eq!(plan_for(&marked, Some(&[1, 2, 3])).1, None);
}

// ------------------------------------------------------------ Auto Run's own order

/// A saved order is used as it is, even where the suggestion differs, when
/// it needs no more resets.
#[test]
fn a_saved_order_is_used_as_it_is() {
    let cases = vec![(1, none()), (2, none()), (3, none())];
    let (plan, counts) = plan_for(&cases, Some(&[3, 1, 2]));
    assert_eq!(plan.order, vec![3, 1, 2]);
    assert_eq!(plan.phases, vec![vec![3, 1, 2]]);
    assert_eq!(counts, None);
}

/// Review Focus 2: ids no longer selected (or no longer in the PBI) are
/// dropped, and selected cases the order misses go at the end in list
/// order. Nothing crashes on an order that names none of them.
#[test]
fn a_saved_order_drops_stale_ids_and_appends_missing_cases_in_list_order() {
    let cases = vec![(1, none()), (2, none()), (3, none()), (4, none())];
    assert_eq!(plan_for(&cases, Some(&[99, 3, 1, 3])).0.order, vec![3, 1, 2, 4]);
    assert_eq!(plan_for(&cases, Some(&[98, 99])).0.order, vec![1, 2, 3, 4]);
    assert_eq!(plan_for(&cases, Some(&[])).0.order, vec![1, 2, 3, 4]);
    assert_eq!(plan_for(&[], Some(&[1, 2])).0, Plan { order: vec![], phases: vec![], resets: vec![] });
}

/// A saved order that needs more resets than the suggestion is still used,
/// and says how many each needs: (saved, suggested).
#[test]
fn a_saved_order_that_needs_more_resets_reports_both_counts() {
    let cases = vec![
        (1, m(&["x"], &[])),
        (2, m(&[], &["x"])),
        (3, m(&["y"], &[])),
        (4, m(&[], &["y"])),
        (5, m(&[], &["x", "y"])),
    ];
    let (plan, counts) = plan_for(&cases, Some(&[1, 2, 3, 4, 5]));
    assert_eq!(plan.order, vec![1, 2, 3, 4, 5]);
    assert_eq!(plan.phases, vec![vec![1], vec![2, 3], vec![4, 5]]);
    assert_eq!(plan.resets, vec![reset(2, &[("x", &[1])]), reset(4, &[("y", &[3])])]);
    assert_eq!(counts, Some((2, 0)));

    // A saved order that is no worse reports nothing.
    assert_eq!(plan_for(&cases, Some(&[4, 2, 5, 3, 1])).1, None);
}

// ------------------------------------------------------------ the command's half

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-autorun-plan-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn script(case_id: i32, changes: &[&str], needs: &[&str]) -> CaseScript {
    CaseScript {
        case_id,
        title: format!("Case {case_id}"),
        account: None,
        area: None,
        steps: vec![StepScript {
            step_number: 1,
            actions: vec![Action::Navigate { url: "https://app.example/".into() }],
            unchecked: None,
        }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: names(changes),
        needs_unchanged: names(needs),
        saved_at: None,
        fail_on_unexpected_dialog: false,
    }
}

/// The marks come from each case's saved script (no script, no marks), the
/// saved order from the store, and the view carries both counts only when
/// the saved order needs more resets.
#[test]
fn the_plan_command_reads_marks_and_the_saved_order_from_the_store() {
    let dir = TempDir::new();
    store::save_script(dir.path(), &script(1, &["x"], &[])).unwrap();
    store::save_script(dir.path(), &script(2, &[], &["x"])).unwrap();
    // Case 3 has no script.

    let view = v2_lib::commands::autorun::plan_at(dir.path(), 100, &[1, 2, 3], None);
    assert_eq!(view.order, vec![2, 1, 3]);
    assert_eq!(view.phases, vec![vec![2, 1, 3]]);
    assert!(view.resets.is_empty());
    assert_eq!(view.counts, None);
    assert!(!view.saved);

    store::save_order(dir.path(), 100, &[1, 2, 3]).unwrap();
    let view = v2_lib::commands::autorun::plan_at(dir.path(), 100, &[1, 2, 3], None);
    assert_eq!(view.order, vec![1, 2, 3]);
    assert_eq!(view.phases, vec![vec![1], vec![2, 3]]);
    assert_eq!(view.resets, vec![reset(2, &[("x", &[1])])]);
    assert_eq!(view.counts, Some((1, 0)));
    assert!(view.saved);

    // Another PBI's order is its own.
    let other = v2_lib::commands::autorun::plan_at(dir.path(), 200, &[1, 2, 3], None);
    assert_eq!(other.order, vec![2, 1, 3]);
    assert!(!other.saved);
}

/// A preview order stands in for the saved one for one call: its order, its
/// resets and its counts, with stale ids dropped and missing ones appended.
/// Nothing is written, and `saved` still reports only what is on disk.
#[test]
fn a_preview_order_is_planned_without_being_saved() {
    let dir = TempDir::new();
    store::save_script(dir.path(), &script(1, &["x"], &[])).unwrap();
    store::save_script(dir.path(), &script(2, &[], &["x"])).unwrap();

    let view = v2_lib::commands::autorun::plan_at(dir.path(), 100, &[1, 2, 3], Some(&[1, 99, 2]));
    assert_eq!(view.order, vec![1, 2, 3]);
    assert_eq!(view.phases, vec![vec![1], vec![2, 3]]);
    assert_eq!(view.resets, vec![reset(2, &[("x", &[1])])]);
    assert_eq!(view.counts, Some((1, 0)));
    assert!(!view.saved);

    let orders = dir.path().join("orders");
    assert!(!orders.join("100.json").exists());

    // With a saved order on disk, the preview wins and the file is untouched.
    store::save_order(dir.path(), 100, &[2, 1, 3]).unwrap();
    let before = std::fs::read(orders.join("100.json")).unwrap();
    let view = v2_lib::commands::autorun::plan_at(dir.path(), 100, &[1, 2, 3], Some(&[1, 2, 3]));
    assert_eq!(view.order, vec![1, 2, 3]);
    assert_eq!(view.counts, Some((1, 0)));
    assert_eq!(std::fs::read(orders.join("100.json")).unwrap(), before);
}

// ------------------------------------------------------------ saving from the dialog

/// The dialog may order only the cases ticked now: they take the places
/// they held in the saved order, and every other case keeps its own.
#[test]
fn saving_part_of_the_order_keeps_the_rest_where_it_was() {
    assert_eq!(store::merge_order(&[1, 2, 3, 4, 5], &[4, 2]), vec![1, 4, 3, 2, 5]);
    assert_eq!(store::merge_order(&[1, 2, 3], &[3, 2, 1]), vec![3, 2, 1]);
    let dir = tempfile::tempdir().unwrap();
    store::save_order(dir.path(), 100, &[1, 2, 3, 4, 5]).unwrap();
    store::save_order_merged(dir.path(), 100, &[5, 1]).unwrap();
    assert_eq!(store::load_order(dir.path(), 100), Some(vec![5, 2, 3, 4, 1]));
}

/// Cases the saved order never had go at the end, in the order given.
#[test]
fn cases_new_to_the_saved_order_go_at_the_end() {
    assert_eq!(store::merge_order(&[1, 2, 3], &[9, 3, 1, 8]), vec![3, 2, 1, 9, 8]);
    let dir = tempfile::tempdir().unwrap();
    store::save_order(dir.path(), 100, &[1, 2, 3]).unwrap();
    store::save_order_merged(dir.path(), 100, &[7, 2]).unwrap();
    assert_eq!(store::load_order(dir.path(), 100), Some(vec![1, 2, 3, 7]));
}

/// With no saved order, what the dialog sends is the order.
#[test]
fn with_no_saved_order_the_dialogs_order_is_saved_as_given() {
    let dir = tempfile::tempdir().unwrap();
    store::save_order_merged(dir.path(), 100, &[3, 1, 2]).unwrap();
    assert_eq!(store::load_order(dir.path(), 100), Some(vec![3, 1, 2]));
}

/// A failed save or clear says so without the profile folder's path: that
/// goes to the log.
#[test]
fn an_order_that_cannot_be_written_or_removed_is_said_without_its_path() {
    let dir = tempfile::tempdir().unwrap();
    // A folder where the order file should be cannot be removed as a file.
    std::fs::create_dir_all(dir.path().join("orders").join("100.json")).unwrap();
    assert_eq!(store::clear_order(dir.path(), 100), Err(store::ORDER_NOT_CLEARED.to_string()));
    // A file where the orders folder should be stops a save.
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("orders"), "x").unwrap();
    let err = store::save_order(other.path(), 100, &[1]).unwrap_err();
    assert_eq!(err, store::ORDER_NOT_SAVED);
    let root = other.path().to_string_lossy().to_string();
    assert!(!err.contains(&root));
}
