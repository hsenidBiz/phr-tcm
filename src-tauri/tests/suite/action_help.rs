//! What a discovery or a try does for an action that is almost right
//! (`autorun::action_help`): the refusal that shows an example, the wait
//! that has no `timeout_ms`, the wait that is absurdly long, and the call
//! that gives up on an action that never answers.

use serde_json::json;
use v2_lib::autorun::action_help::{
    action_budget, clamp_wait, time_to_run, did_not_answer, example_for, fill_wait_defaults, kinds, no_answer, prepare_waits,
    refusal_for, wait_budget_ms, within_budget, CALL_BUDGET_MS, MAX_DISCOVERY_WAIT_MS, WAIT_MARGIN_MS,
};
use v2_lib::browser::actions::Action;

/// A wait with no `timeout_ms` is given the discovery's; one that has its
/// own, and every other kind, is left alone.
#[test]
fn a_wait_for_without_a_timeout_is_given_the_discoverys() {
    let mut bare = json!({ "kind": "wait_for", "selector": { "css": "#x" } });
    assert!(fill_wait_defaults(&mut bare, 10_000));
    assert_eq!(bare["timeout_ms"], 10_000);
    serde_json::from_value::<Action>(bare).expect("filled, it is an action");

    let mut own = json!({ "kind": "wait_for", "selector": { "css": "#x" }, "timeout_ms": 250 });
    assert!(!fill_wait_defaults(&mut own, 10_000));
    assert_eq!(own["timeout_ms"], 250);

    let mut click = json!({ "kind": "click", "selector": { "css": "#x" } });
    assert!(!fill_wait_defaults(&mut click, 10_000));
    assert!(click.get("timeout_ms").is_none());

    // Inside a `when_visible` too.
    let mut guarded = json!({ "kind": "when_visible", "selector": "#b", "then": [
        { "kind": "wait_for", "selector": "#c" }
    ] });
    assert!(fill_wait_defaults(&mut guarded, 7_000));
    assert_eq!(guarded["then"][0]["timeout_ms"], 7_000);
}

/// Filling is for discovery and try only: the type, and so a save, still
/// requires the `timeout_ms`.
#[test]
fn a_saved_wait_for_still_needs_its_timeout() {
    let err = serde_json::from_value::<Action>(json!({ "kind": "wait_for", "selector": { "css": "#x" } }))
        .expect_err("a script's wait_for without timeout_ms is not an action");
    assert!(err.to_string().contains("timeout_ms"), "{err}");
}

/// A wait above the ceiling is lowered to it, `within_ms` too; a smaller one
/// is left alone.
#[test]
fn a_huge_wait_is_lowered_to_the_ceiling() {
    let mut huge = json!({ "kind": "wait_for", "selector": "#x", "timeout_ms": 4_000_000_000u64 });
    assert_eq!(clamp_wait(&mut huge), vec!["timeout_ms"]);
    assert_eq!(huge["timeout_ms"], MAX_DISCOVERY_WAIT_MS);
    serde_json::from_value::<Action>(huge).expect("still an action");

    let mut within = json!({ "kind": "expect_tab", "name": "t", "within_ms": 999_999_999u64 });
    assert_eq!(clamp_wait(&mut within), vec!["within_ms"]);
    assert_eq!(within["within_ms"], MAX_DISCOVERY_WAIT_MS);

    let mut fine = json!({ "kind": "wait_for", "selector": "#x", "timeout_ms": MAX_DISCOVERY_WAIT_MS });
    assert!(clamp_wait(&mut fine).is_empty());

    let mut notes = json!({ "kind": "wait_for", "selector": "#x", "timeout_ms": 4_000_000_000u64 });
    assert_eq!(prepare_waits(&mut notes, 10_000), vec!["timeout_ms lowered to 120000".to_string()]);
    // The note names the key that was lowered, `within_ms` or one in a `then`.
    let mut within = json!({ "kind": "expect_tab", "name": "t", "within_ms": 999_999_999u64 });
    assert_eq!(prepare_waits(&mut within, 10_000), vec!["within_ms lowered to 120000".to_string()]);
    let mut nested = json!({ "kind": "when_visible", "selector": "#b", "within_ms": 5, "then": [
        { "kind": "wait_for", "selector": "#c", "timeout_ms": 999_999_999u64 }
    ] });
    assert_eq!(prepare_waits(&mut nested, 10_000), vec!["timeout_ms lowered to 120000".to_string()]);
    let mut bare = json!({ "kind": "wait_for", "selector": "#x" });
    let said = prepare_waits(&mut bare, 10_000);
    assert_eq!(said.len(), 1);
    assert!(said[0].contains("10000") && said[0].contains("saved script needs one"), "{said:?}");
}

/// Every example in the table is an action of its own kind, so the table
/// cannot drift from the type; every kind of the type has one.
#[test]
fn every_example_parses_as_an_action_of_its_kind() {
    // Every kind of the type has an example: serde's own list of the kinds
    // it knows is the table's.
    let err = serde_json::from_value::<Action>(json!({ "kind": "zzz" })).unwrap_err().to_string();
    let listed: std::collections::BTreeSet<String> = err
        .split("expected one of ")
        .nth(1)
        .unwrap_or_else(|| panic!("{err}"))
        .split(", ")
        .map(|k| k.trim_matches('`').to_string())
        .collect();
    let ours: std::collections::BTreeSet<String> = kinds().into_iter().map(str::to_string).collect();
    assert_eq!(listed, ours);
    for kind in kinds() {
        let example = example_for(kind).expect("listed");
        let value: serde_json::Value = serde_json::from_str(example).unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert_eq!(value["kind"], kind, "{example}");
        let action: Action = serde_json::from_value(value).unwrap_or_else(|e| panic!("{kind}: {e}: {example}"));
        let back = serde_json::to_value(&action).unwrap();
        assert_eq!(back["kind"], kind, "{example}");
    }
    assert_eq!(example_for("nonsense"), None);
}

/// A known kind with a wrong field names the field and shows an example; an
/// unknown or missing kind lists the kinds; both end at the guide.
#[test]
fn a_refused_action_shows_what_it_should_look_like() {
    let wrong = json!({ "kind": "click", "target": "#x" });
    let err = serde_json::from_value::<Action>(wrong.clone()).unwrap_err().to_string();
    let said = refusal_for(&wrong, &err);
    assert!(said.contains("selector"), "{said}");
    assert!(said.contains(r#""kind":"click""#), "{said}");
    assert!(said.contains("that is not a click action"), "{said}");
    assert!(said.ends_with(" - call get_guide (topic autorun) for the vocabulary."), "{said}");
    assert!(!said.contains(".. "), "{said}");

    let typo = json!({ "kind": "clik" });
    let said = refusal_for(&typo, "unknown variant `clik`");
    assert!(said.contains("\"clik\" is not an action kind"), "{said}");
    assert!(said.contains("click") && said.contains("wait_for"), "{said}");
    assert!(said.ends_with(" - call get_guide (topic autorun) for the vocabulary."), "{said}");

    let none = refusal_for(&json!({ "selector": "#x" }), "missing field `kind`");
    assert!(none.contains("needs a \"kind\"") && none.contains("expect_visible"), "{none}");
    for s in [&none, &said] {
        assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'), "{s}");
    }
}

/// An action's time to answer is its own wait, the loading indicator's, and
/// the margin; a composite action gets a long allowance of its own.
#[test]
fn an_actions_budget_is_its_wait_plus_the_margin() {
    let wait: Action = serde_json::from_value(
        json!({ "kind": "wait_for", "selector": "#x", "timeout_ms": MAX_DISCOVERY_WAIT_MS }),
    )
    .unwrap();
    let budget = wait_budget_ms(&wait, 30_000);
    assert!(budget >= u64::from(MAX_DISCOVERY_WAIT_MS) + WAIT_MARGIN_MS, "{budget}");
    assert!(budget <= CALL_BUDGET_MS, "{budget}");
    let small: Action = serde_json::from_value(json!({ "kind": "wait_for", "selector": "#x", "timeout_ms": 100 })).unwrap();
    assert!(wait_budget_ms(&small, 30_000) < budget);
    let component: Action = serde_json::from_value(json!({ "kind": "use_component", "component": "c" })).unwrap();
    assert!(wait_budget_ms(&component, 30_000) >= budget);
    // No action, a component or a page walk included, may be given longer
    // than the call: the proxy gives up on the call at 300 s.
    assert_eq!(wait_budget_ms(&component, 30_000), CALL_BUDGET_MS);
    let walk: Action = serde_json::from_value(json!({ "kind": "page_until", "next": "#n", "until": "#u", "max_pages": 100 })).unwrap();
    assert!(wait_budget_ms(&walk, 30_000) <= CALL_BUDGET_MS);
    // A recipe's longest loading wait (180 s) still fits before a click.
    let click: Action = serde_json::from_value(json!({ "kind": "click", "selector": "#x" })).unwrap();
    assert!(wait_budget_ms(&click, 30_000) >= 180_000 + 15_000, "{}", wait_budget_ms(&click, 30_000));
    assert!(CALL_BUDGET_MS < 300_000);
    assert_eq!(no_answer(3, 125), "action 3 did not answer within 125 s; the batch stopped there");
}

/// A batch shares the call's time: each action gets the smaller of its own
/// budget and what is left, and none once it is spent.
#[test]
fn a_batch_gives_each_action_what_is_left_of_the_call() {
    assert_eq!(action_budget(30_000, 0), Some(30_000));
    assert_eq!(action_budget(30_000, CALL_BUDGET_MS - 10_000), Some(10_000));
    assert_eq!(action_budget(CALL_BUDGET_MS * 2, 0), Some(CALL_BUDGET_MS));
    assert_eq!(action_budget(30_000, CALL_BUDGET_MS), None);
    assert_eq!(action_budget(30_000, CALL_BUDGET_MS + 1), None);
    // No time left: the action is not run, it fails as one that did not answer.
    assert_eq!(time_to_run(None), Err("did not answer within 280 s".to_string()));
    assert_eq!(time_to_run(Some(5_000)), Ok(5_000));
    assert_eq!(did_not_answer(125), "did not answer within 125 s");
}

/// A future that never resolves is given up on after its budget, not waited
/// for; one that resolves answers as it would.
#[tokio::test]
async fn a_call_gives_up_on_work_that_never_answers() {
    let started = std::time::Instant::now();
    let gave_up = within_budget(50, std::future::pending::<u8>()).await;
    assert_eq!(gave_up, Err(1));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(within_budget(5_000, async { 7u8 }).await, Ok(7));
}

/// A `when_hidden` shows an example of its own kind, and a `wait_for` in a
/// guard inside it is given the discovery's wait like any other `then`.
#[test]
fn a_when_hidden_has_its_example_and_its_nested_waits_are_filled() {
    let example = example_for("when_hidden").expect("when_hidden has an example");
    assert!(example.contains(r#""kind":"when_hidden""#), "{example}");
    let mut nested = json!({ "kind": "when_hidden", "selector": "#b", "then": [
        { "kind": "when_visible", "selector": "#c", "then": [ { "kind": "wait_for", "selector": "#d" } ] }
    ] });
    assert!(fill_wait_defaults(&mut nested, 7_000));
    assert_eq!(nested["then"][0]["then"][0]["timeout_ms"], 7_000);
}

/// A `hover` has an example of its own kind.
#[test]
fn a_hover_has_its_example() {
    let example = example_for("hover").expect("hover has an example");
    assert!(example.contains(r#""kind":"hover""#), "{example}");
}

// ---- unknown keys (backlog 87) -------------------------------------------

/// A key an action's kind does not have is named, with the ones it has; a
/// nested action's too. Nothing is flagged for a key the kind does take.
#[test]
fn unknown_keys_are_named() {
    use v2_lib::autorun::action_help::{refuse_unknown_keys, unknown_keys};
    let stray = json!({ "kind": "click", "selector": "#a", "foo": 1 });
    assert_eq!(unknown_keys(&stray), vec!["foo".to_string()]);
    let said = refuse_unknown_keys(&stray).unwrap_err();
    assert_eq!(said, "click does not take foo - it takes: selector");

    let nested = json!({ "kind": "when_visible", "selector": "#b", "then": [
        { "kind": "click", "selector": "#c", "force": true, "speed": 2 }
    ] });
    assert_eq!(unknown_keys(&nested), vec!["force".to_string(), "speed".to_string()]);
    let said = refuse_unknown_keys(&nested).unwrap_err();
    assert_eq!(said, "click does not take force, speed - it takes: selector");

    let fine = json!({ "kind": "when_visible", "selector": "#b", "within_ms": 10, "then": [
        { "kind": "click", "selector": "#c" }
    ] });
    assert!(unknown_keys(&fine).is_empty());
    assert!(refuse_unknown_keys(&fine).is_ok());
    // Not an object, or no kind it knows: nothing to compare with.
    assert!(unknown_keys(&json!("click")).is_empty());
    assert!(unknown_keys(&json!({ "kind": "zzz", "a": 1 })).is_empty());
}

/// The fields of a kind come from the type itself: `press_key` has `key`
/// and `times`, and every example in the table is free of unknown keys.
#[test]
fn press_key_fields_come_from_the_type() {
    use v2_lib::autorun::action_help::{fields_of, unknown_keys};
    assert_eq!(fields_of("press_key"), Some(vec!["key".to_string(), "times".to_string()]));
    assert_eq!(fields_of("select_option"), Some(vec![
        "selector".to_string(), "label".to_string(), "value".to_string(), "within_ms".to_string()
    ]));
    assert_eq!(fields_of("nonsense"), None);
    for kind in kinds() {
        let example: serde_json::Value = serde_json::from_str(example_for(kind).unwrap()).unwrap();
        assert!(unknown_keys(&example).is_empty(), "{kind}: {:?}", unknown_keys(&example));
        // The three that gather other keys themselves refuse them when validated.
        let reads_its_own = ["expect_response", "api_request", "expect_download"].contains(&kind);
        assert_eq!(fields_of(kind).is_none(), reads_its_own, "{kind}");
    }
}

/// `screenshot` is a discovery action, read before any action is: only
/// `full_page` goes with it, and a saved script is told why it cannot hold one.
#[test]
fn a_screenshot_is_read_as_a_discovery_action_and_never_saved() {
    use v2_lib::autorun::action_help::{refuse_raw_saved_action, screenshot_ask, screenshot_from};
    let plain = screenshot_from(&json!({ "kind": "screenshot" })).expect("recognised").expect("read");
    assert_eq!(screenshot_ask(&plain), Some(false));
    let full = screenshot_from(&json!({ "kind": "screenshot", "full_page": true })).unwrap().unwrap();
    assert_eq!(screenshot_ask(&full), Some(true));
    assert_eq!(screenshot_ask(&Action::Reload), None);
    assert!(screenshot_from(&json!({ "kind": "click", "selector": "#a" })).is_none());

    let stray = screenshot_from(&json!({ "kind": "screenshot", "zoom": 2 })).unwrap().unwrap_err();
    assert_eq!(stray, "screenshot does not take zoom - it takes: full_page");
    let bad = screenshot_from(&json!({ "kind": "screenshot", "full_page": "yes" })).unwrap().unwrap_err();
    assert!(bad.contains("full_page"), "{bad}");

    let saved = refuse_raw_saved_action(&json!({ "kind": "screenshot" })).unwrap_err();
    assert_eq!(saved, "screenshot is a discovery action: a saved script takes its pictures when a step fails");
    let inside = json!({ "kind": "when_visible", "selector": "#b", "then": [{ "kind": "screenshot" }] });
    assert_eq!(refuse_raw_saved_action(&inside).unwrap_err(), saved);
    let said = refusal_for(&json!({ "kind": "screenshot" }), "unknown variant `screenshot`");
    assert!(said.starts_with("screenshot is a discovery action"), "{said}");
}

/// `repeat` on a `press_key` is `times`, said so; both together are refused.
#[test]
fn repeat_on_a_press_key_is_moved_to_times() {
    use v2_lib::autorun::action_help::prepare_repeat;
    let mut raw = json!({ "kind": "press_key", "key": "Tab", "repeat": 12 });
    let notes = prepare_repeat(&mut raw).expect("accepted");
    assert_eq!(raw, json!({ "kind": "press_key", "key": "Tab", "times": 12 }));
    assert_eq!(notes, vec!["repeat read as times".to_string()]);
    let mut nested = json!({ "kind": "when_visible", "selector": "#b", "then": [
        { "kind": "press_key", "key": "Tab", "repeat": 3 }
    ] });
    prepare_repeat(&mut nested).expect("accepted");
    assert_eq!(nested["then"][0]["times"], 3);
    let mut both = json!({ "kind": "press_key", "key": "Tab", "times": 2, "repeat": 3 });
    assert_eq!(prepare_repeat(&mut both).unwrap_err(), "press_key takes times or repeat, not both");
    // Any other kind keeps its keys: `repeat` there is an unknown key.
    let mut other = json!({ "kind": "click", "selector": "#a", "repeat": 3 });
    assert!(prepare_repeat(&mut other).expect("untouched").is_empty());
    assert_eq!(other["repeat"], 3);
}
