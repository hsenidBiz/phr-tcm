//! `page_until`: the walk's sentences, how its `until` is read and written,
//! and the rules checked before it runs. The live walk is in
//! `browser_live.rs`.

use serde_json::json;
use v2_lib::browser::actions::{
    Action, PageTargets, PAGE_UNTIL_MANY, PAGE_UNTIL_NONE, PAGE_UNTIL_PAGES,
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

fn until_of(v: serde_json::Value) -> PageTargets {
    match serde_json::from_value::<Action>(json!({ "kind": "page_until", "next": "n", "until": v, "max_pages": 5 })).unwrap() {
        Action::PageUntil { until, .. } => until,
        other => panic!("not a page_until: {other:?}"),
    }
}

#[test]
fn page_until_takes_one_until_or_a_list() {
    let a = LocatorStep { text: Some("A".into()), ..Default::default() };
    let b = LocatorStep { text: Some("B".into()), ..Default::default() };
    let card = LocatorStep { css: Some(".card".into()), ..Default::default() };

    assert_eq!(until_of(json!({ "text": "A" })), PageTargets(vec![Target::One(a.clone())]));
    assert_eq!(until_of(json!("#x")), PageTargets(vec![Target::Legacy("#x".into())]));
    assert_eq!(
        until_of(json!([{ "text": "A" }, { "text": "B" }])),
        PageTargets(vec![Target::One(a.clone()), Target::One(b)])
    );
    assert_eq!(
        until_of(json!([[{ "css": ".card" }, { "text": "A" }]])),
        PageTargets(vec![Target::Chain(vec![card, a])])
    );
    assert_eq!(until_of(json!([])), PageTargets(vec![]));

    // Written, always an array, and read back equal.
    for v in [
        json!({ "text": "A" }),
        json!([{ "text": "A" }, { "text": "B" }]),
        json!([[{ "css": ".card" }, { "text": "A" }]]),
    ] {
        let action: Action =
            serde_json::from_value(json!({ "kind": "page_until", "next": "n", "until": v, "max_pages": 5 })).unwrap();
        let written = serde_json::to_value(&action).unwrap();
        assert!(written["until"].is_array(), "until is written as an array: {written}");
        assert_eq!(serde_json::from_value::<Action>(written).unwrap(), action);
    }
}

#[test]
fn page_until_is_checked_before_it_runs() {
    let check = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate();
    let with = |max: u32, until: serde_json::Value| json!({ "kind": "page_until", "next": "n", "until": until, "max_pages": max });
    assert_eq!(check(with(0, json!({ "text": "A" }))).unwrap_err(), PAGE_UNTIL_PAGES);
    assert_eq!(check(with(101, json!({ "text": "A" }))).unwrap_err(), PAGE_UNTIL_PAGES);
    assert_eq!(check(with(5, json!([]))).unwrap_err(), PAGE_UNTIL_NONE);
    let eleven: Vec<serde_json::Value> = (0..11).map(|i| json!({ "text": format!("T{i}") })).collect();
    assert_eq!(check(with(5, json!(eleven))).unwrap_err(), PAGE_UNTIL_MANY);
    let ten: Vec<serde_json::Value> = (0..10).map(|i| json!({ "text": format!("T{i}") })).collect();
    assert!(check(with(100, json!(ten))).is_ok());
    assert!(check(with(1, json!({ "text": "A" }))).is_ok());
    // A bad `next` locator is refused with its own sentence.
    let bad = json!({ "kind": "page_until", "next": "", "until": { "text": "A" }, "max_pages": 5 });
    assert_eq!(check(bad).unwrap_err(), "a selector is empty");
    // And a bad `until` locator names where it was.
    let bad = with(5, json!([{ "text": "A" }, ""]));
    assert_eq!(check(bad).unwrap_err(), "page_until until: a selector is empty");
}

#[test]
fn page_until_lists_next_then_each_until_as_its_targets() {
    let mut action: Action =
        serde_json::from_value(json!({ "kind": "page_until", "next": { "text": "N" }, "until": [{ "text": "A" }, { "text": "B" }], "max_pages": 5 }))
            .unwrap();
    assert_eq!(action.targets().len(), 3);
    assert_eq!(action.targets_mut().len(), 3);
    assert!(action.is_check());
}
