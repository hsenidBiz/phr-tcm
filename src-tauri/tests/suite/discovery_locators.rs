//! Locators compared the way the live page names them, and every target an
//! action points at.

use v2_lib::browser::actions::Action;
use v2_lib::browser::locator::{fold_name, LocatorStep, SeenKey, Target};

fn role(role: &str, name: &str) -> LocatorStep {
    LocatorStep { role: Some(role.into()), name: Some(name.into()), ..LocatorStep::default() }
}

fn text(t: &str) -> LocatorStep {
    LocatorStep { text: Some(t.into()), ..LocatorStep::default() }
}

fn css(c: &str) -> LocatorStep {
    LocatorStep { css: Some(c.into()), ..LocatorStep::default() }
}

fn click(t: &str) -> Action {
    Action::Click { selector: Target::from(t) }
}

#[test]
fn seen_keys_fold_unusual_spaces_and_case() {
    let a = role("button", "Save\u{00A0} Leave\n").seen_key();
    let b = role("Button", "save leave").seen_key();
    assert_eq!(a, b);
    assert_eq!(a, Some(SeenKey::Role { role: "button".into(), name: "save leave".into() }));
    assert_eq!(fold_name("  A\u{202F}B\u{2009}\tC\u{2007}D "), "a b c d");
}

#[test]
fn text_and_css_keys_are_exact_after_trim() {
    assert_eq!(text(" Leave ").seen_key(), Some(SeenKey::Text("Leave".into())));
    assert_ne!(css("#a").seen_key(), css("#A").seen_key());
    let loose = LocatorStep { exact: true, visible: Some(false), nth: Some(2), ..role("link", "Go") };
    assert_eq!(loose.seen_key(), role("link", "Go").seen_key());
    let nameless = LocatorStep { role: Some("link".into()), ..LocatorStep::default() };
    assert_eq!(nameless.seen_key(), Some(SeenKey::Role { role: "link".into(), name: String::new() }));
}

#[test]
fn chain_links_each_have_a_key() {
    let t = Target::Chain(vec![role("dialog", "Add"), text("Name"), css(".x")]);
    let keys: Vec<_> = t.links().iter().filter_map(LocatorStep::seen_key).collect();
    assert_eq!(keys.len(), 3);
}

#[test]
fn legacy_string_is_a_css_link() {
    let links = Target::from("#save").links();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].seen_key(), Some(SeenKey::Css("#save".into())));
    assert_eq!(Target::One(text("x")).links().len(), 1);
}

#[test]
fn targets_reaches_when_visible_children_and_both_drag_ends() {
    let a = Action::WhenVisible {
        selector: Target::from("#banner"),
        within_ms: None,
        then: vec![
            click("#ok"),
            Action::Drag { from: Target::from("#a"), to: Target::from("#b"), position: None, within_ms: None },
        ],
    };
    assert_eq!(a.targets().len(), 4);
}

#[test]
fn tab_actions_have_no_targets() {
    assert!(Action::SwitchTab { name: "x".into() }.targets().is_empty());
    assert!(Action::OpenTab { name: "x".into(), url: "/".into() }.targets().is_empty());
}

#[test]
fn typed_value_is_only_a_fill() {
    let f = Action::Fill { selector: Target::from("#a"), value: "hi".into() };
    assert_eq!(f.typed_value(), Some("hi"));
    assert_eq!(click("#a").typed_value(), None);
}
