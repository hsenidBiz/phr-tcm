//! Components: the `use_component` step and the `input` placeholder a
//! component's locator leaves to the caller.

use serde_json::json;
use v2_lib::browser::actions::Action;
use v2_lib::browser::locator::{has_input_placeholder, LocatorStep, Target};

#[test]
fn scripts_without_components_still_parse() {
    let a: Action = serde_json::from_value(json!({"kind": "click", "selector": {"role": "button", "name": "Save"}})).unwrap();
    assert!(matches!(a, Action::Click { .. }));
    let t: Target = serde_json::from_value(json!({"role": "button", "name": "Save"})).unwrap();
    assert!(!has_input_placeholder(&t));
    assert!(t.validate().is_ok());
}

#[test]
fn use_component_round_trips() {
    let v = json!({"kind": "use_component", "component": "pick-date", "inputs": {"day": "5", "n": 2}});
    let a: Action = serde_json::from_value(v.clone()).unwrap();
    match &a {
        Action::UseComponent { component, inputs } => {
            assert_eq!(component, "pick-date");
            assert_eq!(inputs.len(), 2);
        }
        other => panic!("wrong variant: {other:?}"),
    }
    assert_eq!(serde_json::to_value(&a).unwrap(), v);
}

#[test]
fn an_input_placeholder_round_trips_inside_a_chain() {
    let v = json!([{"role": "dialog", "name": "Pick"}, {"input": "day"}]);
    let t: Target = serde_json::from_value(v.clone()).unwrap();
    assert!(has_input_placeholder(&t));
    assert!(t.validate().is_ok());
    assert_eq!(serde_json::to_value(&t).unwrap(), v);
}

#[test]
fn an_input_placeholder_with_a_role_is_refused() {
    for step in [
        LocatorStep { input: Some("x".into()), role: Some("button".into()), ..Default::default() },
        LocatorStep { input: Some("x".into()), text: Some("t".into()), ..Default::default() },
        LocatorStep { input: Some("x".into()), css: Some(".c".into()), ..Default::default() },
    ] {
        let err = Target::One(step).validate().unwrap_err();
        assert!(err.contains("an input placeholder stands alone"), "{err}");
    }
}

#[test]
fn use_component_has_no_targets() {
    let a = Action::UseComponent { component: "c".into(), inputs: serde_json::Map::new() };
    assert!(a.targets().is_empty());
}

// ---- the store ----

use v2_lib::autorun::components::{
    components_path, find, load_components, put, remove, reset_components, users_of, Component, ComponentInput,
    InputKind,
};
use v2_lib::autorun::store::save_script;
use v2_lib::autorun::{CaseScript, StepScript};

fn component(name: &str) -> Component {
    Component {
        name: name.into(),
        description: "pick a date".into(),
        inputs: vec![ComponentInput { name: "day".into(), kind: InputKind::Text, description: "the day".into() }],
        actions: vec![Action::CheckText { value: "x".into() }],
        tried_at: 7,
        tried_area: "Leave".into(),
        version: 1,
        changes: 0,
    }
}

fn script(case_id: i32, actions: Vec<Action>) -> CaseScript {
    CaseScript {
        case_id,
        title: "a case".into(),
        account: None,
        area: None,
        steps: vec![StepScript { step_number: 2, actions, unchecked: None }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None,
        ignore_page_errors: vec![],
    }
}

fn use_it(name: &str) -> Action {
    Action::UseComponent { component: name.into(), inputs: serde_json::Map::new() }
}

#[test]
fn put_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_components(dir.path(), "o", "p").unwrap().components.is_empty());
    put(dir.path(), "o", "p", component("pick-date")).unwrap();
    put(dir.path(), "o", "p", component("other")).unwrap();
    let f = load_components(dir.path(), "o", "p").unwrap();
    assert_eq!(f.components.len(), 2);
    assert_eq!(find(&f, "pick-date"), Some(&component("pick-date")));
    // Replaced by key, not added.
    let mut again = component("Pick-Date");
    again.version = 2;
    put(dir.path(), "o", "p", again).unwrap();
    let f = load_components(dir.path(), "o", "p").unwrap();
    assert_eq!(f.components.len(), 2);
    assert_eq!(find(&f, "pick-date").unwrap().version, 2);
    remove(dir.path(), "o", "p", "OTHER").unwrap();
    assert_eq!(load_components(dir.path(), "o", "p").unwrap().components.len(), 1);
}

#[test]
fn names_match_ignoring_case_and_spaces() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "o", "p", component("Pick  The Date")).unwrap();
    let f = load_components(dir.path(), "o", "p").unwrap();
    assert!(find(&f, "pick the date").is_some());
    assert!(find(&f, "  PICK THE  DATE ").is_some());
    assert!(find(&f, "pick date").is_none());
}

#[test]
fn a_corrupt_file_names_itself_and_reset_moves_it_aside() {
    let dir = tempfile::tempdir().unwrap();
    // A file that reads cannot be reset.
    put(dir.path(), "o", "p", component("c")).unwrap();
    assert!(reset_components(dir.path(), "o", "p").unwrap_err().contains("nothing to reset"));
    let path = components_path(dir.path(), "o", "p");
    std::fs::write(&path, "{ not json").unwrap();
    let err = load_components(dir.path(), "o", "p").unwrap_err();
    assert!(err.contains("the components file projects/"), "{err}");
    assert!(err.contains("-components.json could not be read; Reset it in Auto Run"), "{err}");
    assert!(!err.contains(dir.path().to_str().unwrap()), "{err}");
    assert!(put(dir.path(), "o", "p", component("d")).is_err());
    let aside = reset_components(dir.path(), "o", "p").unwrap();
    assert!(aside.starts_with("projects/") && aside.contains("-components.corrupt-") && aside.ends_with(".json"), "{aside}");
    assert!(!path.exists());
    assert_eq!(std::fs::read_to_string(dir.path().join(&aside)).unwrap(), "{ not json");
    assert!(load_components(dir.path(), "o", "p").unwrap().components.is_empty());
}

#[test]
fn put_without_change_does_not_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "o", "p", component("c")).unwrap();
    let path = components_path(dir.path(), "o", "p");
    // Mark the file so a rewrite would show.
    let before = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{before}

")).unwrap();
    put(dir.path(), "o", "p", component("c")).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{before}

"));
    remove(dir.path(), "o", "p", "absent").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{before}

"));
    let mut changed = component("c");
    changed.changes = 1;
    put(dir.path(), "o", "p", changed).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), serde_json::to_string_pretty(&load_components(dir.path(), "o", "p").unwrap()).unwrap());
}

#[test]
fn users_of_lists_the_scripts_that_use_it() {
    let dir = tempfile::tempdir().unwrap();
    let guarded = Action::WhenVisible {
        selector: Target::One(LocatorStep { role: Some("dialog".into()), ..Default::default() }),
        within_ms: None,
        then: vec![use_it("Pick-Date")],
    };
    save_script(dir.path(), &script(12, vec![use_it("pick-date")])).unwrap();
    save_script(dir.path(), &script(5, vec![guarded])).unwrap();
    save_script(dir.path(), &script(9, vec![use_it("something else")])).unwrap();
    save_script(dir.path(), &script(3, vec![])).unwrap();
    let u = users_of(dir.path(), "pick-date");
    assert_eq!(u.cases, vec![5, 12]);
    let none = users_of(dir.path(), "never used");
    assert!(none.cases.is_empty());
}

// ---- expansion ----

use v2_lib::autorun::components::expand;

fn made(name: &str, inputs: serde_json::Value, actions: serde_json::Value) -> Component {
    serde_json::from_value(json!({ "name": name, "description": "d", "inputs": inputs, "actions": actions, "version": 1 }))
        .expect("a component")
}

fn given(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    v.as_object().cloned().expect("an object")
}

fn actions(v: serde_json::Value) -> Vec<Action> {
    serde_json::from_value(v).expect("actions")
}

#[test]
fn text_inputs_fill_values() {
    let c = made(
        "Pick a date",
        json!([{ "name": "day", "kind": "text", "description": "" }]),
        json!([
            { "kind": "fill", "selector": { "role": "textbox", "name": "Day" }, "value": "{{day}}" },
            { "kind": "check_text", "value": "picked {{day}} and {{day}}, not {{other}}" },
            { "kind": "click", "selector": { "role": "gridcell", "name": "{{day}}" } }
        ]),
    );
    let out = expand(&c, &given(json!({ "day": "5" }))).unwrap();
    assert_eq!(
        out,
        actions(json!([
            { "kind": "fill", "selector": { "role": "textbox", "name": "Day" }, "value": "5" },
            { "kind": "check_text", "value": "picked 5 and 5, not {{other}}" },
            { "kind": "click", "selector": { "role": "gridcell", "name": "5" } }
        ]))
    );
}

/// A text input holding `{{` or `}}` is refused: put into a locator, a
/// value like "{{day}}" would leave the locator reading as the component
/// wrote it, and slip past both the seen check and the placeholder check.
#[test]
fn a_text_input_holding_a_placeholder_is_refused() {
    let c = made(
        "Pick a date",
        json!([{ "name": "day", "kind": "text", "description": "" }]),
        json!([{ "kind": "click", "selector": { "role": "gridcell", "name": "{{day}}" } }]),
    );
    for value in ["{{day}}", "5 {{", "}} 5", "{{other}}"] {
        let err = expand(&c, &given(json!({ "day": value }))).unwrap_err();
        assert_eq!(err, "Pick a date got a placeholder as day", "{value}");
    }
    // Single braces are ordinary text.
    let out = expand(&c, &given(json!({ "day": "{5}" }))).unwrap();
    assert_eq!(out, actions(json!([{ "kind": "click", "selector": { "role": "gridcell", "name": "{5}" } }])));
}

#[test]
fn a_target_input_replaces_its_placeholder() {
    let c = made(
        "Open a menu",
        json!([{ "name": "field", "kind": "target", "description": "" }]),
        json!([
            { "kind": "click", "selector": { "input": "field" } },
            { "kind": "when_visible", "selector": { "role": "menu" }, "then": [
                { "kind": "click", "selector": { "input": "field" } }
            ] }
        ]),
    );
    let out = expand(&c, &given(json!({ "field": { "role": "textbox", "name": "Leave start" } }))).unwrap();
    assert_eq!(
        out,
        actions(json!([
            { "kind": "click", "selector": { "role": "textbox", "name": "Leave start" } },
            { "kind": "when_visible", "selector": { "role": "menu" }, "then": [
                { "kind": "click", "selector": { "role": "textbox", "name": "Leave start" } }
            ] }
        ]))
    );
    // A chain given for a lone placeholder becomes the whole chain.
    let out = expand(&c, &given(json!({ "field": [{ "role": "dialog" }, { "role": "button", "name": "Go" }] }))).unwrap();
    assert_eq!(out[0], actions(json!([{ "kind": "click", "selector": [{ "role": "dialog" }, { "role": "button", "name": "Go" }] }]))[0]);
    // A caller's locator is never searched for text placeholders.
    let t = made(
        "Type",
        json!([{ "name": "field", "kind": "target", "description": "" }, { "name": "v", "kind": "text", "description": "" }]),
        json!([{ "kind": "fill", "selector": { "input": "field" }, "value": "{{v}}" }]),
    );
    let out = expand(&t, &given(json!({ "field": { "text": "{{v}}" }, "v": "x" }))).unwrap();
    assert_eq!(out, actions(json!([{ "kind": "fill", "selector": { "text": "{{v}}" }, "value": "x" }])));
}

#[test]
fn a_target_input_inside_a_chain_expands_in_place() {
    let c = made(
        "Edit a row",
        json!([{ "name": "row", "kind": "target", "description": "" }]),
        json!([{ "kind": "click", "selector": [{ "css": "#grid" }, { "input": "row" }, { "role": "button", "name": "Edit" }] }]),
    );
    let one = expand(&c, &given(json!({ "row": { "role": "row", "name": "Annual" } }))).unwrap();
    assert_eq!(
        one,
        actions(json!([{ "kind": "click", "selector": [
            { "css": "#grid" }, { "role": "row", "name": "Annual" }, { "role": "button", "name": "Edit" }
        ] }]))
    );
    let chain = expand(&c, &given(json!({ "row": [{ "role": "rowgroup" }, { "role": "row", "name": "Annual" }] }))).unwrap();
    assert_eq!(
        chain,
        actions(json!([{ "kind": "click", "selector": [
            { "css": "#grid" }, { "role": "rowgroup" }, { "role": "row", "name": "Annual" }, { "role": "button", "name": "Edit" }
        ] }]))
    );
    for wrong in [json!("#row"), json!(5), json!({ "input": "row" }), json!([])] {
        let err = expand(&c, &given(json!({ "row": wrong }))).unwrap_err();
        assert_eq!(err, "Edit a row needs row to be a locator", "{wrong}");
    }
}

#[test]
fn a_missing_input_is_named() {
    let c = made(
        "Pick a date",
        json!([{ "name": "field", "kind": "target", "description": "" }, { "name": "day", "kind": "text", "description": "" }]),
        json!([{ "kind": "fill", "selector": { "input": "field" }, "value": "{{day}}" }]),
    );
    assert_eq!(expand(&c, &given(json!({ "field": { "css": "#d" } }))).unwrap_err(), "Pick a date needs day");
    assert_eq!(expand(&c, &given(json!({ "field": { "css": "#d" }, "day": null }))).unwrap_err(), "Pick a date needs day");
    assert_eq!(expand(&c, &given(json!({ "day": "5" }))).unwrap_err(), "Pick a date needs field");
    assert_eq!(
        expand(&c, &given(json!({ "field": { "css": "#d" }, "day": { "css": "#x" } }))).unwrap_err(),
        "Pick a date needs day to be text"
    );
}

// ---- saving and removing ----

use v2_lib::autorun::components::{draft_fingerprint, remove_unused, save_tried, TriedIn, CHANGE_CAP};
use v2_lib::autorun::discovery_map::record_matched;

/// "Pick a date": a target input for the field the calendar opens from, a
/// text input for the day, and two fixed locators: the calendar and its
/// Done button.
fn pick_a_date() -> Component {
    made(
        "Pick a date",
        json!([
            { "name": "field", "kind": "target", "description": "the date field" },
            { "name": "day", "kind": "text", "description": "the day of the month" }
        ]),
        json!([
            { "kind": "click", "selector": { "input": "field" } },
            { "kind": "expect_visible", "selector": { "role": "dialog", "name": "Calendar" } },
            { "kind": "click", "selector": { "role": "gridcell", "name": "{{day}}" } },
            { "kind": "click", "selector": { "role": "button", "name": "Done" } }
        ]),
    )
}

/// Files `locators` as matched on a page of `area`.
fn seen_in(root: &std::path::Path, area: &str, locators: &[serde_json::Value]) {
    for l in locators {
        let t: Target = serde_json::from_value(l.clone()).unwrap();
        record_matched(root, "o", "p", Some(area), "/leave", &t, 1).unwrap();
    }
}

fn calendar_seen(root: &std::path::Path) {
    seen_in(
        root,
        "Leave",
        &[json!({ "role": "dialog", "name": "Calendar" }), json!({ "role": "button", "name": "Done" })],
    );
}

/// What a discovery keeps for each of `drafts` it tried and saw work.
fn tried(drafts: &[&Component]) -> Vec<String> {
    drafts.iter().map(|c| draft_fingerprint(c)).collect()
}

/// A discovery in the Leave area that tried `prints`.
fn session(prints: &[String]) -> Option<TriedIn<'_>> {
    Some(TriedIn { area: Some("Leave"), tried: prints })
}

#[test]
fn an_untried_component_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    calendar_seen(dir.path());
    let c = pick_a_date();
    // No discovery at all.
    let err = save_tried(dir.path(), "o", "p", c.clone(), None, None, 50, no_users()).unwrap_err();
    assert!(err.contains("Try the component live in discovery first"), "{err}");
    // A discovery that tried this one with other actions.
    let mut other = pick_a_date();
    other.actions.pop();
    let prints = tried(&[&other]);
    let err = save_tried(dir.path(), "o", "p", c.clone(), None, session(&prints), 50, no_users()).unwrap_err();
    assert!(err.contains("Try the component live in discovery first"), "{err}");
    assert!(load_components(dir.path(), "o", "p").unwrap().components.is_empty());
    // Tried as it is sent: saved, with when and where it was tried. Its
    // description does not count toward what was tried.
    let mut described = c.clone();
    described.description = "Picks a day in the calendar".into();
    let prints = tried(&[&c]);
    let saved = save_tried(dir.path(), "o", "p", described, None, session(&prints), 50, no_users()).unwrap();
    assert_eq!((saved.saved.as_str(), saved.version, saved.changes, saved.cap_reached), ("Pick a date", 1, 0, false));
    let f = load_components(dir.path(), "o", "p").unwrap();
    let kept = find(&f, "pick a date").unwrap();
    assert_eq!((kept.tried_at, kept.tried_area.as_str(), kept.version), (50, "Leave", 1));
    assert_eq!(kept.description, "Picks a day in the calendar");
}

#[test]
fn undeclared_or_unused_inputs_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    calendar_seen(dir.path());
    let refused = |c: Component| save_tried(dir.path(), "o", "p", c, None, None, 50, no_users()).unwrap_err();

    let mut blank = pick_a_date();
    blank.name = "  ".into();
    assert_eq!(refused(blank), "A component needs a name.");
    let mut blank = pick_a_date();
    blank.description = " ".into();
    assert_eq!(refused(blank), "Pick a date needs a description.");

    let mut twice = pick_a_date();
    twice.inputs.push(ComponentInput { name: " day ".into(), kind: InputKind::Text, description: "".into() });
    assert_eq!(refused(twice), "Pick a date declares the input day more than once.");

    let mut unused = pick_a_date();
    unused.inputs.push(ComponentInput { name: "month".into(), kind: InputKind::Text, description: "".into() });
    assert_eq!(refused(unused), "Pick a date declares the input month but never uses it.");

    let mut no_text = pick_a_date();
    no_text.inputs.retain(|i| i.name != "day");
    assert_eq!(refused(no_text), "Pick a date uses {{day}} but declares no text input day.");

    let mut no_target = pick_a_date();
    no_target.inputs.retain(|i| i.name != "field");
    assert_eq!(refused(no_target), "Pick a date uses {\"input\": \"field\"} but declares no target input field.");

    // A text input is not a locator, and a target input is not text.
    let mut swapped = pick_a_date();
    for i in swapped.inputs.iter_mut() {
        i.kind = if i.kind == InputKind::Text { InputKind::Target } else { InputKind::Text };
    }
    assert_eq!(refused(swapped), "Pick a date uses {{day}} but declares no text input day.");
}

#[test]
fn sign_in_inside_a_component_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let refused = |c: Component| save_tried(dir.path(), "o", "p", c, None, None, 50, no_users()).unwrap_err();

    let mut signs_in = pick_a_date();
    signs_in.actions.insert(0, Action::SignIn { account: "admin".into() });
    assert_eq!(refused(signs_in), "A component cannot sign in: a script signs in as its account.");

    // Declared or not, a username or password is never typed by a component.
    for inputs in [json!([]), json!([{ "name": "password", "kind": "text", "description": "" }])] {
        let c = made(
            "Log in",
            inputs,
            json!([{ "kind": "fill", "selector": { "role": "textbox", "name": "Password" }, "value": "{{password}}" }]),
        );
        assert_eq!(refused(c), "A component cannot type a username or password: a script signs in as its account.");
    }
    let c = made(
        "Who",
        json!([{ "name": "username", "kind": "text", "description": "" }]),
        json!([{ "kind": "check_text", "value": "Hello {{ username }}" }]),
    );
    assert_eq!(refused(c), "A component cannot type a username or password: a script signs in as its account.");

    // Nor does it use another component, a guarded use included.
    let nested = made(
        "Twice",
        json!([]),
        json!([{ "kind": "when_visible", "selector": { "role": "dialog" }, "then": [
            { "kind": "use_component", "component": "Pick a date", "inputs": {} }
        ] }]),
    );
    assert_eq!(refused(nested), "A component cannot use another component.");

    // Nor go to an address an input makes.
    for go in [
        json!({ "kind": "navigate", "url": "/leave/{{id}}" }),
        json!({ "kind": "open_tab", "name": "t", "url": "https://hr.example.com/{{id}}" }),
    ] {
        let c = made("Go", json!([{ "name": "id", "kind": "text", "description": "" }]), json!([go]));
        assert_eq!(refused(c), "A component's address cannot come from an input.");
    }
}

#[test]
fn an_unseen_fixed_locator_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let c = pick_a_date();
    let prints = tried(&[&c]);
    let save = |c: Component| save_tried(dir.path(), "o", "p", c, None, session(&prints), 50, no_users());

    // Done was seen only in another area.
    seen_in(dir.path(), "Leave", &[json!({ "role": "dialog", "name": "Calendar" })]);
    seen_in(dir.path(), "Payroll", &[json!({ "role": "button", "name": "Done" })]);
    let err = save(c.clone()).unwrap_err();
    assert!(err.starts_with("Action 4: "), "{err}");
    assert!(err.contains("was never seen on the live app"), "{err}");

    // Seen in the area: the input's link and the {{day}} cell are exempt.
    seen_in(dir.path(), "Leave", &[json!({ "role": "button", "name": "Done" })]);
    save(c).unwrap();

    // The fixed link of a chain an input sits in is still checked.
    let chained = made(
        "Pick in a frame",
        json!([{ "name": "field", "kind": "target", "description": "" }]),
        json!([{ "kind": "click", "selector": [{ "role": "dialog", "name": "Frame" }, { "input": "field" }] }]),
    );
    let prints = tried(&[&chained]);
    let err = save_tried(dir.path(), "o", "p", chained, None, session(&prints), 50, no_users()).unwrap_err();
    assert!(err.starts_with("Action 1: ") && err.contains("was never seen on the live app"), "{err}");

    // A page it goes to must have been seen too.
    let goes = made("Go", json!([]), json!([{ "kind": "navigate", "url": "/payroll/run" }]));
    let prints = tried(&[&goes]);
    let err = save_tried(dir.path(), "o", "p", goes, None, session(&prints), 50, no_users()).unwrap_err();
    assert!(err.starts_with("Action 1: /payroll/run was never seen"), "{err}");
}

#[test]
fn a_change_needs_why_and_may_not_weaken() {
    let dir = tempfile::tempdir().unwrap();
    calendar_seen(dir.path());
    let first = pick_a_date();
    let mut more = pick_a_date();
    more.actions.push(Action::CheckText { value: "picked".into() });
    let mut weaker = pick_a_date();
    weaker.actions.remove(1);
    let prints = tried(&[&first, &more, &weaker]);
    let save = |c: Component, why: Option<&str>| save_tried(dir.path(), "o", "p", c, why, session(&prints), 50, no_users());

    save(first, None).unwrap();
    let needs_why = "Pick a date is already saved: say why it changes in \"why\".";
    assert_eq!(save(more.clone(), None).unwrap_err(), needs_why);
    assert_eq!(save(more.clone(), Some("  ")).unwrap_err(), needs_why);
    let err = save(weaker, Some("the calendar check is flaky")).unwrap_err();
    assert_eq!(err, "Pick a date had 1 checks and now has 0 - an assertion is never removed or weakened by a repair");
    assert_eq!(find(&load_components(dir.path(), "o", "p").unwrap(), "pick a date").unwrap().version, 1);
    save(more, Some("checks the day was picked")).unwrap();
}

#[test]
fn a_change_bumps_version_and_counts_toward_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    calendar_seen(dir.path());
    let drafts: Vec<Component> = (0..5)
        .map(|n| {
            let mut c = pick_a_date();
            for _ in 0..n {
                c.actions.push(Action::CheckText { value: "picked".into() });
            }
            c
        })
        .collect();
    let prints = tried(&drafts.iter().collect::<Vec<_>>());
    let mut got = Vec::new();
    for (i, c) in drafts.into_iter().enumerate() {
        let why = (i > 0).then_some("one more check");
        let s = save_tried(dir.path(), "o", "p", c, why, session(&prints), 50 + i as u64, no_users()).unwrap();
        got.push((s.version, s.changes, s.cap_reached));
    }
    assert_eq!(CHANGE_CAP, 3);
    // Past the cap a change still saves: the guide tells the assistant to
    // stop and report.
    assert_eq!(got, vec![(1, 0, false), (2, 1, false), (3, 2, false), (4, 3, true), (5, 4, true)]);
    let kept = load_components(dir.path(), "o", "p").unwrap();
    let kept = find(&kept, "Pick a date").unwrap();
    assert_eq!((kept.version, kept.changes, kept.tried_at), (5, 4, 54));
}

#[test]
fn a_component_in_use_cannot_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "o", "p", component("pick-date")).unwrap();
    put(dir.path(), "o", "p", component("unused")).unwrap();
    save_script(dir.path(), &script(12, vec![use_it("Pick-Date")])).unwrap();
    save_script(dir.path(), &script(5, vec![use_it("pick-date")])).unwrap();
    assert_eq!(
        remove_unused(dir.path(), "o", "p", "pick-date").unwrap_err(),
        "pick-date is used by cases 5, 12: change those scripts first."
    );
    assert!(find(&load_components(dir.path(), "o", "p").unwrap(), "pick-date").is_some());
    remove_unused(dir.path(), "o", "p", "UNUSED").unwrap();
    assert!(find(&load_components(dir.path(), "o", "p").unwrap(), "unused").is_none());
    assert_eq!(remove_unused(dir.path(), "o", "p", "unused").unwrap_err(), "unused is not saved in this project");
}

#[test]
fn a_fixed_link_beside_a_placeholder_link_is_checked() {
    let dir = tempfile::tempdir().unwrap();
    let c = made(
        "Pick in a panel",
        json!([{ "name": "day", "kind": "text", "description": "" }]),
        json!([{ "kind": "click", "selector": [{ "css": "#guessed-panel" }, { "role": "gridcell", "name": "{{day}}" }] }]),
    );
    let prints = tried(&[&c]);
    let err = save_tried(dir.path(), "o", "p", c.clone(), None, session(&prints), 50, no_users()).unwrap_err();
    assert!(err.starts_with("Action 1: ") && err.contains("was never seen on the live app"), "{err}");
    // Once the panel is seen, the {{day}} cell beside it is still exempt.
    seen_in(dir.path(), "Leave", &[json!({ "css": "#guessed-panel" })]);
    save_tried(dir.path(), "o", "p", c, None, session(&prints), 50, no_users()).unwrap();
}

#[test]
fn an_unclosed_placeholder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for stray in ["{{day", "day}}", "}} {{day}}", "{{day}} {{"] {
        let c = made(
            "Pick a day",
            json!([{ "name": "day", "kind": "text", "description": "" }]),
            json!([
                { "kind": "click", "selector": { "role": "gridcell", "name": "{{day}}" } },
                { "kind": "click", "selector": [{ "css": "#unseen-panel" }, { "role": "gridcell", "name": stray }] }
            ]),
        );
        let err = save_tried(dir.path(), "o", "p", c, None, None, 50, no_users()).unwrap_err();
        assert_eq!(err, "A component has an unclosed {{ placeholder.", "{stray}");
    }
}

// ---- the Components dialog's view ----

#[test]
fn load_components_command_counts_users() {
    use v2_lib::commands::autorun::components_view;
    let dir = tempfile::tempdir().unwrap();
    assert!(components_view(dir.path(), "o", "p").unwrap().components.is_empty());
    let mut used = component("Pick-Date");
    used.changes = 2;
    used.tried_at = 1_759_000_000_000;
    put(dir.path(), "o", "p", used).unwrap();
    put(dir.path(), "o", "p", component("unused")).unwrap();
    save_script(dir.path(), &script(12, vec![use_it("pick-date")])).unwrap();
    save_script(dir.path(), &script(4, vec![use_it("PICK-DATE")])).unwrap();

    let view = components_view(dir.path(), "o", "p").unwrap();
    assert_eq!(view.components.len(), 2);
    let pick = view.components.iter().find(|c| c.name == "Pick-Date").unwrap();
    assert_eq!(pick.used_by_cases, vec![4, 12]);
    assert_eq!(pick.description, "pick a date");
    assert_eq!(pick.inputs.len(), 1);
    assert_eq!(pick.tried_area, "Leave");
    assert_eq!(pick.tried_at, Some(1_759_000_000_000));
    assert_eq!(pick.version, 1);
    assert_eq!(pick.changes, 2);
    assert_eq!(pick.cap, v2_lib::autorun::components::CHANGE_CAP);
    let unused = view.components.iter().find(|c| c.name == "unused").unwrap();
    assert!(unused.used_by_cases.is_empty());

    // Never tried reads as no date, not the epoch.
    let mut never = component("never");
    never.tried_at = 0;
    put(dir.path(), "o", "p", never).unwrap();
    let view = components_view(dir.path(), "o", "p").unwrap();
    assert_eq!(view.components.iter().find(|c| c.name == "never").unwrap().tried_at, None);

    // A damaged file is the load's refusal, which offers Reset.
    std::fs::write(components_path(dir.path(), "o", "p"), "{ not json").unwrap();
    assert!(components_view(dir.path(), "o", "p").unwrap_err().contains("Reset it in Auto Run"));
}

// ---- a change is checked against the scripts that use it ----

use v2_lib::autorun::components::{UserCases, SIGN_IN_TO_CHECK_USERS};

/// No script uses the component, or every user's case is at hand.
fn no_users() -> Option<&'static UserCases> {
    static NONE: std::sync::OnceLock<UserCases> = std::sync::OnceLock::new();
    Some(NONE.get_or_init(UserCases::default))
}

/// "Pick a date" before the calendar cell was clicked: the day is only
/// checked as text, so no locator of it holds `{{day}}`.
fn pick_a_date_v1() -> Component {
    made(
        "Pick a date",
        json!([
            { "name": "field", "kind": "target", "description": "the date field" },
            { "name": "day", "kind": "text", "description": "the day of the month" }
        ]),
        json!([
            { "kind": "click", "selector": { "input": "field" } },
            { "kind": "expect_visible", "selector": { "role": "dialog", "name": "Calendar" } },
            { "kind": "check_text", "value": "{{day}}" },
            { "kind": "click", "selector": { "role": "button", "name": "Done" } }
        ]),
    )
}

/// v1 with a click on the day's cell: a new locator built from `{{day}}`.
fn pick_a_date_v2() -> Component {
    let mut c = pick_a_date_v1();
    c.actions.insert(
        3,
        serde_json::from_value(json!({ "kind": "click", "selector": { "role": "gridcell", "name": "{{day}}" } }))
            .unwrap(),
    );
    c
}

/// Case `case_id`, in the Leave area, picking `day` at step 2.
fn picks(case_id: i32, day: &str) -> CaseScript {
    let mut s = script(
        case_id,
        vec![serde_json::from_value(json!({
            "kind": "use_component",
            "component": "Pick a date",
            "inputs": { "field": { "role": "textbox", "name": "Leave start" }, "day": day }
        }))
        .unwrap()],
    );
    s.area = Some("Leave".into());
    s
}

/// The calendar, its Done button and the Leave start field, seen in Leave.
fn leave_form_seen(root: &std::path::Path) {
    calendar_seen(root);
    seen_in(root, "Leave", &[json!({ "role": "textbox", "name": "Leave start" })]);
}

/// This project's cases: `here` with their text, `elsewhere` in no case of it.
fn cases(here: &[(i32, &[&str])], elsewhere: &[i32]) -> UserCases {
    UserCases {
        here: here.iter().map(|(id, t)| (*id, t.iter().map(|s| s.to_string()).collect())).collect(),
        elsewhere: elsewhere.to_vec(),
    }
}

#[test]
fn a_change_that_puts_an_unseen_locator_into_a_using_script_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    leave_form_seen(dir.path());
    let (v1, v2) = (pick_a_date_v1(), pick_a_date_v2());
    let mut v3 = pick_a_date_v1();
    v3.inputs.push(ComponentInput { name: "month".into(), kind: InputKind::Text, description: "".into() });
    v3.actions.push(Action::CheckText { value: "{{month}}".into() });
    let prints = tried(&[&v1, &v2, &v3]);
    save_tried(dir.path(), "o", "p", v1, None, session(&prints), 50, no_users()).unwrap();
    save_script(dir.path(), &picks(12, "15")).unwrap();
    let users = cases(&[(12, &["Pick the start date"])], &[]);

    // The cell "15" was never seen: case 12 would click a guess.
    let err =
        save_tried(dir.path(), "o", "p", v2, Some("click the day"), session(&prints), 60, Some(&users)).unwrap_err();
    assert!(err.starts_with("Case 12, step 2: "), "{err}");
    assert!(err.ends_with("was never seen on the live app; this change would break it."), "{err}");
    assert!(err.contains("gridcell") && err.contains("15"), "{err}");

    // A new input the script does not give breaks it too.
    let err =
        save_tried(dir.path(), "o", "p", v3, Some("check the month"), session(&prints), 60, Some(&users)).unwrap_err();
    assert_eq!(err, "Case 12, step 2: Pick a date needs month; this change would break it.");
    assert_eq!(find(&load_components(dir.path(), "o", "p").unwrap(), "pick a date").unwrap().version, 1);
}

#[test]
fn a_change_that_keeps_every_using_script_seen_saves() {
    let dir = tempfile::tempdir().unwrap();
    leave_form_seen(dir.path());
    let (v1, v2) = (pick_a_date_v1(), pick_a_date_v2());
    let prints = tried(&[&v1, &v2]);
    save_tried(dir.path(), "o", "p", v1, None, session(&prints), 50, no_users()).unwrap();
    save_script(dir.path(), &picks(12, "15")).unwrap();
    // Case 30 is another project's: its day is never checked here.
    save_script(dir.path(), &picks(30, "31")).unwrap();
    seen_in(dir.path(), "Leave", &[json!({ "role": "gridcell", "name": "15" })]);
    let users = cases(&[(12, &[])], &[30]);
    let saved =
        save_tried(dir.path(), "o", "p", v2, Some("click the day"), session(&prints), 60, Some(&users)).unwrap();
    assert_eq!((saved.version, saved.changes), (2, 1));
}

#[test]
fn a_fresh_save_under_a_used_name_rechecks_its_users() {
    let dir = tempfile::tempdir().unwrap();
    leave_form_seen(dir.path());
    // The script was saved against a component since reset or removed.
    save_script(dir.path(), &picks(12, "15")).unwrap();
    let v2 = pick_a_date_v2();
    let prints = tried(&[&v2]);
    let users = cases(&[(12, &[])], &[]);
    let err = save_tried(dir.path(), "o", "p", v2.clone(), None, session(&prints), 50, Some(&users)).unwrap_err();
    assert!(err.starts_with("Case 12, step 2: ") && err.ends_with("this change would break it."), "{err}");
    assert!(load_components(dir.path(), "o", "p").unwrap().components.is_empty());
    seen_in(dir.path(), "Leave", &[json!({ "role": "gridcell", "name": "15" })]);
    assert_eq!(save_tried(dir.path(), "o", "p", v2, None, session(&prints), 50, Some(&users)).unwrap().version, 1);
}

#[test]
fn a_change_with_no_cases_available_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    leave_form_seen(dir.path());
    let (v1, v2) = (pick_a_date_v1(), pick_a_date_v2());
    let prints = tried(&[&v1, &v2]);
    // With no script using it, nothing needs the cases.
    save_tried(dir.path(), "o", "p", v1, None, session(&prints), 50, None).unwrap();
    save_script(dir.path(), &picks(12, "15")).unwrap();
    seen_in(dir.path(), "Leave", &[json!({ "role": "gridcell", "name": "15" })]);
    let err =
        save_tried(dir.path(), "o", "p", v2.clone(), Some("click the day"), session(&prints), 60, None).unwrap_err();
    assert_eq!(err, SIGN_IN_TO_CHECK_USERS);
    assert_eq!(SIGN_IN_TO_CHECK_USERS, "Sign in so the scripts that use this component can be checked.");
    // A user whose case was not read (it began using the component after
    // the cases were read) is never skipped either.
    let err =
        save_tried(dir.path(), "o", "p", v2, Some("click the day"), session(&prints), 60, no_users()).unwrap_err();
    assert!(err.starts_with("Case 12 "), "{err}");
    assert_eq!(find(&load_components(dir.path(), "o", "p").unwrap(), "pick a date").unwrap().version, 1);
}

// ---- reading an old run ----

use v2_lib::autorun::components::{ran_actions, ComponentUse};
use v2_lib::browser::actions::ActionOutcome;

#[test]
fn an_old_run_of_a_changed_component_is_not_paired_with_the_new_actions() {
    let mut now = made("Say yes", json!([]), json!([{ "kind": "click", "selector": { "role": "button", "name": "Yes" } }]));
    now.version = 2;
    let file = v2_lib::autorun::components::ComponentFile { components: vec![now] };
    let step = vec![use_it("Say yes")];
    let mut outcome = ActionOutcome::failed("not found".to_string());
    outcome.component = Some("Say yes".into());
    let outcomes = vec![outcome];

    // Run with version 1: the action that ran is not the one there now.
    let old = ran_actions(&step, &outcomes, &file, &[ComponentUse { name: "Say yes".into(), version: 1 }]);
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].action, None);
    assert_eq!(old[0].component.as_deref(), Some("Say yes"));

    // Run with the version there now: paired.
    let same = ran_actions(&step, &outcomes, &file, &[ComponentUse { name: "Say yes".into(), version: 2 }]);
    assert!(same[0].action.is_some());
}

#[test]
fn an_input_link_with_other_fields_is_refused() {
    for step in [
        LocatorStep { input: Some("x".into()), name: Some("n".into()), ..Default::default() },
        LocatorStep { input: Some("x".into()), exact: true, ..Default::default() },
        LocatorStep { input: Some("x".into()), visible: Some(false), ..Default::default() },
        LocatorStep { input: Some("x".into()), nth: Some(2), ..Default::default() },
    ] {
        let err = Target::One(step.clone()).validate().unwrap_err();
        assert!(err.contains("an input placeholder stands alone"), "{step:?}: {err}");
        let chain = Target::Chain(vec![LocatorStep { css: Some("#a".into()), ..Default::default() }, step]);
        let err = chain.validate().unwrap_err();
        assert!(err.contains("an input placeholder stands alone"), "{err}");
    }
}
