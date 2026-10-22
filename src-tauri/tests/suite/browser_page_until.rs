//! `page_until`: the walk's sentences, how its `until` is read and written,
//! and the rules checked before it runs. The live walk is in
//! `browser_live.rs`.

use serde_json::json;
use v2_lib::browser::actions::{
    page_goals, Action, PAGE_UNTIL_MANY, PAGE_UNTIL_NONE, PAGE_UNTIL_ONE_OF, PAGE_UNTIL_PAGES,
};
use v2_lib::browser::locator::{LocatorStep, Target};
use v2_lib::browser::page_until::{Turn, Walk};

fn walk(names: &[&str], max: u32) -> Walk {
    Walk::new(names.iter().map(|n| n.to_string()).collect(), max)
}

#[test]
fn the_walk_stops_on_the_first_page_that_shows_the_target() {
    let mut w = walk(&["text \"Published\""], 40);
    assert_eq!(w.look(&[false]), Turn::Click);
    assert_eq!(w.look(&[false]), Turn::Click);
    assert_eq!(w.look(&[true]), Turn::Seen("saw text \"Published\" on page 3".to_string()));
}

#[test]
fn the_walk_fails_with_the_count_at_max_pages() {
    let mut w = walk(&["text \"Published\""], 3);
    assert_eq!(w.look(&[false]), Turn::Click);
    assert_eq!(w.look(&[false]), Turn::Click);
    assert_eq!(w.look(&[false]), Turn::Failed("walked 3 pages and never saw text \"Published\"".to_string()));
    // One page is one page.
    let mut one = walk(&["A"], 1);
    assert_eq!(one.look(&[false]), Turn::Failed("walked 1 page and never saw A".to_string()));
}

#[test]
fn several_targets_are_each_seen_once_on_any_page() {
    let mut w = walk(&["A", "B"], 40);
    assert_eq!(w.look(&[true, false]), Turn::Click);
    // A is not on this page any more: it was seen, and stays seen.
    assert_eq!(w.look(&[false, true]), Turn::Seen("saw A on page 1 and B on page 2".to_string()));
}

#[test]
fn a_next_that_is_gone_ends_the_walk_with_the_pages_walked() {
    let mut w = walk(&["B"], 40);
    assert_eq!(w.look(&[false]), Turn::Click);
    assert_eq!(w.look(&[false]), Turn::Click);
    assert_eq!(
        w.no_next("button \"Next\"", "disabled"),
        "walked 2 pages and never saw B: button \"Next\" is disabled on page 2"
    );
    assert_eq!(
        w.no_next("button \"Next\"", "gone"),
        "walked 2 pages and never saw B: button \"Next\" is gone on page 2"
    );
}

#[test]
fn a_page_that_did_not_turn_is_said_with_its_number() {
    let mut w = walk(&["A"], 40);
    assert_eq!(w.look(&[false]), Turn::Click);
    assert_eq!(w.look(&[false]), Turn::Click);
    assert_eq!(w.not_turned(), "Next did not turn the page (page 2).");
}

/// A `page_until` with the given `until` / `until_all` field.
fn page_until_with(field: serde_json::Value) -> serde_json::Value {
    let mut v = json!({ "kind": "page_until", "next": "n", "max_pages": 5 });
    v.as_object_mut().unwrap().extend(field.as_object().unwrap().clone());
    v
}

fn goals_of(field: serde_json::Value) -> Vec<Target> {
    match serde_json::from_value::<Action>(page_until_with(field)).unwrap() {
        Action::PageUntil { until, until_all, .. } => page_goals(&until, &until_all).into_iter().cloned().collect(),
        other => panic!("not a page_until: {other:?}"),
    }
}

#[test]
fn until_is_one_target_and_a_list_of_locators_is_a_chain() {
    let a = LocatorStep { text: Some("A".into()), ..Default::default() };
    let card = LocatorStep { css: Some(".card".into()), ..Default::default() };
    let icon = LocatorStep { css: Some("i.fa-lock".into()), ..Default::default() };

    assert_eq!(goals_of(json!({ "until": { "text": "A" } })), vec![Target::One(a.clone())]);
    assert_eq!(goals_of(json!({ "until": "#x" })), vec![Target::Legacy("#x".into())]);
    // The usual way to write a chain, outermost first: ONE target, the icon
    // inside the card, not two targets.
    assert_eq!(
        goals_of(json!({ "until": [{ "css": ".card" }, { "css": "i.fa-lock" }] })),
        vec![Target::Chain(vec![card.clone(), icon])]
    );
}

#[test]
fn until_all_is_several_targets_each_of_which_may_be_a_chain() {
    let a = LocatorStep { text: Some("A".into()), ..Default::default() };
    let b = LocatorStep { text: Some("B".into()), ..Default::default() };
    let card = LocatorStep { css: Some(".card".into()), ..Default::default() };
    assert_eq!(
        goals_of(json!({ "until_all": [{ "text": "A" }, { "text": "B" }] })),
        vec![Target::One(a.clone()), Target::One(b)]
    );
    assert_eq!(
        goals_of(json!({ "until_all": [[{ "css": ".card" }, { "text": "A" }], { "text": "A" }] })),
        vec![Target::Chain(vec![card, a.clone()]), Target::One(a)]
    );
}

#[test]
fn a_page_until_is_written_and_read_back_equal() {
    for until in [
        json!({ "until": { "text": "A" } }),
        json!({ "until": [{ "css": ".card" }, { "text": "A" }] }),
        json!({ "until_all": [{ "text": "A" }, [{ "css": ".card" }, { "text": "B" }]] }),
    ] {
        let action: Action = serde_json::from_value(page_until_with(until.clone())).unwrap();
        let written = serde_json::to_value(&action).unwrap();
        assert_eq!(serde_json::from_value::<Action>(written.clone()).unwrap(), action, "{written}");
        // Only the field that was given is written.
        assert_eq!(written.get("until").is_some(), until.get("until").is_some(), "{written}");
        assert_eq!(written.get("until_all").is_some(), until.get("until_all").is_some(), "{written}");
    }
}

#[test]
fn page_until_is_checked_before_it_runs() {
    let check = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate();
    let with = |max: u32, until: serde_json::Value| json!({ "kind": "page_until", "next": "n", "until": until, "max_pages": max });
    let with_all = |max: u32, all: serde_json::Value| json!({ "kind": "page_until", "next": "n", "until_all": all, "max_pages": max });
    assert_eq!(check(with(0, json!({ "text": "A" }))).unwrap_err(), PAGE_UNTIL_PAGES);
    assert_eq!(check(with(101, json!({ "text": "A" }))).unwrap_err(), PAGE_UNTIL_PAGES);
    assert_eq!(check(with_all(5, json!([]))).unwrap_err(), PAGE_UNTIL_NONE);
    let eleven: Vec<serde_json::Value> = (0..11).map(|i| json!({ "text": format!("T{i}") })).collect();
    assert_eq!(check(with_all(5, json!(eleven))).unwrap_err(), PAGE_UNTIL_MANY);
    let ten: Vec<serde_json::Value> = (0..10).map(|i| json!({ "text": format!("T{i}") })).collect();
    assert!(check(with_all(100, json!(ten))).is_ok());
    assert!(check(with(1, json!({ "text": "A" }))).is_ok());
    assert!(check(with(1, json!([{ "css": ".card" }, { "text": "A" }]))).is_ok());
    // Both, or neither, is refused with a sentence that says how to write each.
    let both = json!({ "kind": "page_until", "next": "n", "until": { "text": "A" }, "until_all": [{ "text": "B" }], "max_pages": 5 });
    assert_eq!(check(both).unwrap_err(), PAGE_UNTIL_ONE_OF);
    let neither = json!({ "kind": "page_until", "next": "n", "max_pages": 5 });
    assert_eq!(check(neither).unwrap_err(), PAGE_UNTIL_ONE_OF);
    assert!(PAGE_UNTIL_ONE_OF.contains("chain") && PAGE_UNTIL_ONE_OF.contains("until_all"));
    // A bad `next` locator is refused with its own sentence.
    let bad = json!({ "kind": "page_until", "next": "", "until": { "text": "A" }, "max_pages": 5 });
    assert_eq!(check(bad).unwrap_err(), "a selector is empty");
    // And a bad `until_all` locator names where it was.
    let bad = with_all(5, json!([{ "text": "A" }, ""]));
    assert_eq!(check(bad).unwrap_err(), "page_until until: a selector is empty");
}

#[test]
fn page_until_lists_next_then_each_goal_as_its_targets() {
    let mut action: Action =
        serde_json::from_value(json!({ "kind": "page_until", "next": { "text": "N" }, "until_all": [{ "text": "A" }, { "text": "B" }], "max_pages": 5 }))
            .unwrap();
    assert_eq!(action.targets().len(), 3);
    assert_eq!(action.targets_mut().len(), 3);
    assert!(action.is_check());
    let one: Action = serde_json::from_value(json!({ "kind": "page_until", "next": { "text": "N" }, "until": [{ "text": "A" }, { "text": "B" }], "max_pages": 5 })).unwrap();
    assert_eq!(one.targets().len(), 2);
}
