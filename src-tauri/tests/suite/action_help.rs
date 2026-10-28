//! What a discovery or a try does for an action that is almost right
//! (`autorun::action_help`): the refusal that shows an example, the wait
//! that has no `timeout_ms`, the wait that is absurdly long, and the call
//! that gives up on an action that never answers.

use serde_json::json;
use v2_lib::autorun::action_help::{
    clamp_wait, example_for, fill_wait_defaults, kinds, no_answer, prepare_waits, refusal_for, wait_budget_ms,
    within_budget, MAX_DISCOVERY_WAIT_MS, WAIT_MARGIN_MS,
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
    assert!(clamp_wait(&mut huge));
    assert_eq!(huge["timeout_ms"], MAX_DISCOVERY_WAIT_MS);
    serde_json::from_value::<Action>(huge).expect("still an action");

    let mut within = json!({ "kind": "expect_tab", "name": "t", "within_ms": 999_999_999u64 });
    assert!(clamp_wait(&mut within));
    assert_eq!(within["within_ms"], MAX_DISCOVERY_WAIT_MS);

    let mut fine = json!({ "kind": "wait_for", "selector": "#x", "timeout_ms": MAX_DISCOVERY_WAIT_MS });
    assert!(!clamp_wait(&mut fine));

    let mut notes = json!({ "kind": "wait_for", "selector": "#x", "timeout_ms": 4_000_000_000u64 });
    assert_eq!(prepare_waits(&mut notes, 10_000), vec!["timeout_ms lowered to 120000".to_string()]);
    let mut bare = json!({ "kind": "wait_for", "selector": "#x" });
    let said = prepare_waits(&mut bare, 10_000);
    assert_eq!(said.len(), 1);
    assert!(said[0].contains("10000") && said[0].contains("saved script needs one"), "{said:?}");
}

/// Every example in the table is an action of its own kind, so the table
/// cannot drift from the type; every kind of the type has one.
#[test]
fn every_example_parses_as_an_action_of_its_kind() {
    assert!(kinds().len() >= 40, "{:?}", kinds());
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
    assert!(budget < 600_000, "{budget}");
    let small: Action = serde_json::from_value(json!({ "kind": "wait_for", "selector": "#x", "timeout_ms": 100 })).unwrap();
    assert!(wait_budget_ms(&small, 30_000) < budget);
    let component: Action = serde_json::from_value(json!({ "kind": "use_component", "component": "c" })).unwrap();
    assert!(wait_budget_ms(&component, 30_000) > budget);
    assert_eq!(no_answer(3, 125), "action 3 did not answer within 125 s; the batch stopped there");
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
