//! The actions an explore session keeps for the assistant that ran them
//! (`autorun::kept`), fed from the one place every discovery action ends and
//! read back by `get_kept_actions`.
//!
//! Every test uses its own temp root, and the kept store is keyed by root, so
//! none of them meets another's list and none needs a serial lock.

use crate::autorun_discovery::{
    exploring, leave_page, parsed, root_with_recipe_and_account, slot, FakeBrowser, ORG, PROJECT,
};
use crate::common::{account, pick_a_date, PASSWORD};
use serde_json::{json, Value};
use v2_lib::ai_bridge::{discover_action_in, end_discovery_closing, kept_actions_for};
use v2_lib::autorun::explore::{begin, ExploreRun};
use v2_lib::autorun::kept::{
    keep, kept_answer, last_of, list_of, owner_of, started, types_a_credential, KEPT_MAX, NOT_AN_EXPLORE,
};
use v2_lib::browser::actions::Action;
use v2_lib::commands::autorun::DiscoveryState;

/// A going explore session on `Leave` whose change list is `session`, with
/// `run` shaping the explore state (a setup running, say).
fn exploring_with(session: &str, setup_running: bool) -> Option<DiscoveryState> {
    exploring("Leave").map(|s| DiscoveryState {
        explore: Some(ExploreRun { session: Some(session.to_string()), setup_running, ..ExploreRun::default() }),
        ..s
    })
}

fn click(css: &str) -> Action {
    Action::Click { selector: css.into() }
}

fn fill(css: &str, value: &str) -> Action {
    serde_json::from_value(json!({ "kind": "fill", "selector": { "css": css }, "value": value })).unwrap()
}

/// An action that fails at once: there is no such area to return to.
fn nowhere() -> Action {
    serde_json::from_value(json!({ "kind": "return_to_area", "area": "Nowhere At All" })).unwrap()
}

fn browser(session: &str, setup_running: bool) -> Option<FakeBrowser> {
    slot(
        leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list?page=2"),
        exploring_with(session, setup_running),
    )
    .0
}

async fn run(b: &mut Option<FakeBrowser>, root: &std::path::Path, a: &Action) -> Value {
    let (status, body) = discover_action_in(b, root, ORG, PROJECT, a, None, None).await;
    assert_eq!(status, 200, "{body}");
    parsed(&body)
}

#[tokio::test]
async fn a_successful_explore_action_is_kept_in_order() {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut b = browser(&id, false);

    assert_eq!(run(&mut b, dir.path(), &click("#a")).await["ok"], true);
    assert_eq!(run(&mut b, dir.path(), &click("#b")).await["ok"], true);

    let list = list_of(dir.path(), &id).expect("a list was kept");
    assert_eq!(list.actions.len(), 2);
    assert_eq!((list.actions[0].n, list.actions[1].n), (1, 2));
    assert_eq!(list.actions[0].action, click("#a"));
    assert_eq!(list.actions[1].action, click("#b"));
    assert_eq!(list.actions[0].area.as_deref(), Some("Leave"));
    assert!(list.actions[0].ended_ms >= list.actions[0].started_ms);
}

#[tokio::test]
async fn a_failed_action_is_not_kept() {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut b = browser(&id, false);

    assert_eq!(run(&mut b, dir.path(), &nowhere()).await["ok"], false);
    assert!(list_of(dir.path(), &id).map_or(true, |l| l.actions.is_empty()));
}

#[tokio::test]
async fn a_plain_discovery_keeps_nothing() {
    let dir = root_with_recipe_and_account();
    let mut b = slot(
        leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list?page=2"),
        exploring("Leave"),
    )
    .0;
    assert_eq!(run(&mut b, dir.path(), &click("#a")).await["ok"], true);

    started(dir.path(), "a", None, None);
    assert_eq!(last_of(dir.path(), "a"), Err(NOT_AN_EXPLORE.to_string()));
    let (status, body) = kept_actions_for(dir.path(), "a");
    assert_eq!((status, body.as_str()), (409, NOT_AN_EXPLORE));
}

#[tokio::test]
async fn a_setup_run_keeps_nothing() {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut b = browser(&id, true);

    assert_eq!(run(&mut b, dir.path(), &click("#a")).await["ok"], true);
    assert!(list_of(dir.path(), &id).map_or(true, |l| l.actions.is_empty()));
}

#[tokio::test]
async fn a_use_component_is_kept_as_itself_with_the_steps_it_ran() {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut b = browser(&id, false);
    let c = pick_a_date();
    let inputs = json!({ "field": { "css": "#day" }, "day": "AUTOTEST day" });
    let use_it: Action = serde_json::from_value(json!({
        "kind": "use_component", "component": "Pick a date", "inputs": inputs
    }))
    .unwrap();

    let (status, body) = discover_action_in(&mut b, dir.path(), ORG, PROJECT, &use_it, Some(&c), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["ok"], true, "{body}");

    let list = list_of(dir.path(), &id).expect("a list was kept");
    assert_eq!(list.actions.len(), 1);
    assert_eq!(list.actions[0].action, use_it);
    let want = v2_lib::autorun::components::expand(&c, inputs.as_object().unwrap()).unwrap();
    assert_eq!(list.actions[0].steps, Some(want));
}

#[tokio::test]
async fn a_fill_of_a_login_or_password_is_never_kept() {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut b = browser(&id, false);

    for a in [fill("#day", "kim"), fill("#day", PASSWORD), fill("#password", "anything")] {
        assert_eq!(run(&mut b, dir.path(), &a).await["ok"], true);
    }
    assert!(list_of(dir.path(), &id).map_or(true, |l| l.actions.is_empty()), "a credential was kept");

    assert_eq!(run(&mut b, dir.path(), &fill("#day", "AUTOTEST leave")).await["ok"], true);
    let list = list_of(dir.path(), &id).unwrap();
    assert_eq!(list.actions.len(), 1);
    assert_eq!(list.actions[0].n, 1, "a refused action takes no number");
}

#[test]
fn a_credential_is_found_wherever_the_fill_sits() {
    let accounts = [account()];
    assert!(types_a_credential(&fill("#x", " kim "), None, &accounts));
    assert!(types_a_credential(&click("#x"), Some(&[fill("#x", PASSWORD)]), &accounts));
    assert!(types_a_credential(&fill("#user-PassWord", "x"), None, &accounts));
    assert!(!types_a_credential(&fill("#x", "AUTOTEST"), None, &accounts));
    assert!(!types_a_credential(&click("#x"), None, &accounts));
}

#[test]
fn the_list_keeps_the_last_200_and_numbers_never_shift() {
    let dir = root_with_recipe_and_account();
    for i in 0..205u64 {
        let n = keep(dir.path(), "s205", &click(&format!("#c{i}")), None, None, i, i + 1);
        assert_eq!(n, Some(u32::try_from(i + 1).unwrap()));
    }
    let list = list_of(dir.path(), "s205").unwrap();
    assert_eq!(list.actions.len(), KEPT_MAX);
    assert_eq!(list.actions.first().unwrap().n, 6);
    assert_eq!(list.actions.last().unwrap().n, 205);
    assert_eq!(list.dropped, 5);

    started(dir.path(), "owner", Some("s205"), None);
    let (status, body) = kept_actions_for(dir.path(), "owner");
    assert_eq!(status, 200, "{body}");
    let answer = kept_answer(&list);
    assert_eq!(answer["cut"], true);
    assert_eq!(answer["first"], 6);
    assert_eq!(answer["actions"][0]["n"], 6);
}

#[test]
fn a_new_discovery_by_the_same_assistant_drops_the_old_list() {
    let dir = root_with_recipe_and_account();
    started(dir.path(), "a", Some("s1"), Some("Leave"));
    keep(dir.path(), "s1", &click("#a"), None, Some("Leave"), 1, 2);
    started(dir.path(), "a", Some("s2"), Some("Leave"));

    assert_eq!(list_of(dir.path(), "s1"), None);
    assert_eq!(last_of(dir.path(), "a").unwrap().session, "s2");
}

#[tokio::test]
async fn the_list_outlives_the_end_of_the_discovery() {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut b = browser(&id, false);
    started(dir.path(), "a", Some(&id), Some("Leave"));
    assert_eq!(run(&mut b, dir.path(), &click("#a")).await["ok"], true);

    let (status, out) = end_discovery_closing(&mut b);
    assert_eq!(status, 200, "{out}");
    assert_eq!(last_of(dir.path(), "a").unwrap().actions.len(), 1);
}

#[test]
fn another_assistants_list_is_not_yours() {
    let dir = root_with_recipe_and_account();
    started(dir.path(), "a", Some("s1"), None);
    keep(dir.path(), "s1", &click("#a"), None, None, 1, 2);
    assert_eq!(last_of(dir.path(), "b"), Err(NOT_AN_EXPLORE.to_string()));
    assert!(last_of(dir.path(), "a").is_ok());
}

#[test]
fn the_owner_is_the_agent_or_nothing() {
    use v2_lib::autorun::agents::{AgentKey, Caller};
    assert_eq!(owner_of(&Caller::Default), "");
    let key = AgentKey { id: "a".repeat(32), name: Some("writer".into()) };
    assert_eq!(owner_of(&Caller::Agent(key)), format!("{}:writer", "a".repeat(32)));
    let key = AgentKey { id: "a".repeat(32), name: None };
    assert_eq!(owner_of(&Caller::Agent(key)), format!("{}:", "a".repeat(32)));
}

#[test]
fn kept_actions_answer_shows_typed_values_and_no_login() {
    let dir = root_with_recipe_and_account();
    started(dir.path(), "a", Some("s1"), Some("Leave"));
    keep(dir.path(), "s1", &fill("#name", "AUTOTEST leave"), None, Some("Leave"), 1, 2);

    let (status, body) = kept_actions_for(dir.path(), "a");
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["actions"][0]["action"]["value"], "AUTOTEST leave", "{body}");
    assert_eq!(v["session"], "s1");
    assert_eq!(v["area"], "Leave");
    assert!(!body.contains("kim") && !body.contains(PASSWORD), "{body}");
}
