//! The "newly assigned" rule: announce changes, never the backlog someone
//! has been carrying since before the app was installed.

use v2_lib::assigned_watch::{newly_assigned, AssignedItem};

fn item(id: i32, title: &str) -> AssignedItem {
    AssignedItem {
        id,
        title: title.into(),
        work_item_type: "Task".into(),
        state: "Active".into(),
    }
}

/// The whole point: a fresh install must not fire twelve notifications for
/// work the user already knows about.
#[test]
fn the_first_run_learns_the_baseline_without_announcing_it() {
    let current = vec![item(1, "Old work"), item(2, "Also old")];
    let (fresh, seen) = newly_assigned(&current, None);

    assert!(fresh.is_empty(), "nothing is announced on the first run");
    assert_eq!(seen, vec!["1", "2"], "but everything is remembered");
}

#[test]
fn only_items_not_seen_before_are_reported() {
    let current = vec![item(1, "Old work"), item(7, "Brand new")];
    let (fresh, seen) = newly_assigned(&current, Some(vec!["1".into()]));

    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].id, 7);
    assert_eq!(fresh[0].title, "Brand new");
    assert_eq!(seen, vec!["1", "7"]);
}

#[test]
fn an_unchanged_list_reports_nothing() {
    let current = vec![item(1, "Same"), item(2, "Same")];
    let (fresh, _) = newly_assigned(&current, Some(vec!["1".into(), "2".into()]));
    assert!(fresh.is_empty());
}

/// Unassigned work drops out of the stored set, so being assigned the same
/// item again later is news again rather than being silently swallowed.
#[test]
fn an_item_that_goes_away_is_forgotten_and_can_return() {
    let (_, seen) = newly_assigned(&[item(1, "Kept")], Some(vec!["1".into(), "2".into()]));
    assert_eq!(seen, vec!["1"], "#2 is no longer assigned, so it is forgotten");

    let (fresh, _) = newly_assigned(&[item(1, "Kept"), item(2, "Back again")], Some(seen));
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].id, 2);
}

#[test]
fn an_empty_assignment_list_is_not_an_error() {
    let (fresh, seen) = newly_assigned(&[], Some(vec!["1".into()]));
    assert!(fresh.is_empty());
    assert!(seen.is_empty());
}

/// Creating or updating a test case through this app assigns it to you -
/// the watch must not announce your own edit back to you. Only real work
/// (PBIs, bugs, tasks) is news.
#[test]
fn the_watch_query_excludes_test_management_artifacts() {
    let wiql = v2_lib::assigned_watch::ASSIGNED_WIQL;
    for excluded in [
        "'Test Case'",
        "'Test Suite'",
        "'Test Plan'",
        "'Shared Steps'",
        "'Shared Parameter'",
    ] {
        assert!(
            wiql.contains(excluded),
            "{excluded} missing from the exclusion list: {wiql}"
        );
    }
    assert!(wiql.contains("[System.WorkItemType] NOT IN"));
}
