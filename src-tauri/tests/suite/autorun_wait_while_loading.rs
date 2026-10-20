//! A project's loading indicators (`wait_while_shown` in its sign-in
//! recipe): right before every action that touches or checks the page the
//! runner looks once, without waiting, and while a listed indicator is up
//! it waits for it to go - up to the entry's `max_ms` - before the action
//! starts its own timeout. Written for PeoplesHR's full-page spinner, up for
//! 20-60 s on a cold start, which made the first step of almost every run
//! time out. Runs, tries and replays all reach it through the same step loop
//! (`run_step_in_run`), where pop-ups are closed too.

use crate::common;

use common::{quick, ScriptedDriver};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use v2_lib::autorun::loading::{still_loading, waited, DEFAULT_MAX_MS};
use v2_lib::autorun::recipe::{builtin_recipe, save_recipe, SignInRecipe};
use v2_lib::autorun::replay::{run_selection, Browsers};
use v2_lib::autorun::runner::{run_step, AFTER_STOP};
use v2_lib::autorun::{store, CaseScript, LocalRun, StepScript};
use v2_lib::browser::actions::Action;

fn recipe(v: serde_json::Value) -> SignInRecipe {
    serde_json::from_value(v).expect("the test wrote an unparseable recipe")
}

fn page_loader() -> serde_json::Value {
    json!({ "name": "page loader", "shown": { "css": "#ehrmLoader" } })
}

fn with_loaders(loaders: serde_json::Value) -> serde_json::Value {
    json!({
        "start_url": "https://hr.example.internal/",
        "steps": [ { "kind": "click", "selector": { "css": "#login" } } ],
        "signed_in": { "css": "#marker" },
        "wait_while_shown": loaders
    })
}

fn refusal(loaders: serde_json::Value) -> String {
    recipe(with_loaders(loaders)).validate().expect_err("this loader list should have been refused")
}

fn action(v: serde_json::Value) -> Action {
    serde_json::from_value(v).expect("the test wrote an action that does not parse")
}

fn step(actions: Vec<Action>) -> StepScript {
    StepScript { step_number: 1, actions, unchecked: None, cleanup: false }
}

// ---------------------------------------------------------------- the recipe

#[test]
fn wait_while_shown_parses_round_trips_and_is_optional() {
    let mut capped = page_loader();
    capped["max_ms"] = json!(120_000);
    let r = recipe(with_loaders(json!([page_loader(), capped.clone()])));
    assert_eq!(r.validate(), Ok(()));
    assert_eq!(r.wait_while_shown.len(), 2);
    assert_eq!(r.wait_while_shown[0].name, "page loader");
    assert_eq!(r.wait_while_shown[0].max_ms, None);
    assert_eq!(r.wait_while_shown[0].limit_ms(), DEFAULT_MAX_MS);
    assert_eq!(DEFAULT_MAX_MS, 90_000);
    assert_eq!(r.wait_while_shown[1].limit_ms(), 120_000);
    let out = serde_json::to_value(&r).unwrap();
    assert_eq!(out["wait_while_shown"], json!([page_loader(), capped]), "it goes back out as written");

    let mut none = with_loaders(json!([]));
    none.as_object_mut().unwrap().remove("wait_while_shown");
    let r = recipe(none);
    assert!(r.wait_while_shown.is_empty());
    assert!(serde_json::to_value(&r).unwrap().get("wait_while_shown").is_none(), "an empty list is not written");
}

#[test]
fn an_entry_with_an_unknown_field_is_refused_as_it_is_read() {
    let mut entry = page_loader();
    entry["close"] = json!({ "css": "#x" });
    let err = serde_json::from_value::<SignInRecipe>(with_loaders(json!([entry]))).unwrap_err().to_string();
    assert!(err.contains("close"), "{err}");
}

#[test]
fn an_entry_needs_a_name_and_a_good_selector() {
    let mut nameless = page_loader();
    nameless["name"] = json!("  ");
    assert_eq!(
        refusal(json!([nameless])),
        "wait_while_shown 1: give the loading indicator a short name, such as \"page loader\" - a step that waited for it says \"waited 12 s for page loader\""
    );

    let mut empty_shown = page_loader();
    empty_shown["shown"] = json!({ "css": "" });
    let err = refusal(json!([page_loader(), empty_shown]));
    assert!(err.starts_with("wait_while_shown 2 (page loader): \"shown\" - "), "{err}");

    let mut blank = page_loader();
    blank["shown"] = json!("");
    assert_eq!(refusal(json!([blank])), "wait_while_shown 1 (page loader): \"shown\" - a selector is empty");

    let mut list = page_loader();
    list["shown"] = json!([]);
    assert_eq!(refusal(json!([list])), "wait_while_shown 1 (page loader): \"shown\" - a locator list is empty");
}

#[test]
fn a_placeholder_in_an_entry_is_refused() {
    let mut entry = page_loader();
    entry["shown"] = json!({ "css": "#{{password}}" });
    assert_eq!(
        refusal(json!([entry])),
        "wait_while_shown 1 (page loader): {{username}} and {{password}} are filled in for the recipe's own steps only - here they would be typed as they are"
    );
}

#[test]
fn max_ms_out_of_range_is_refused() {
    let want = "wait_while_shown 1 (page loader): max_ms must be from 1000 to 180000 (1 to 180 s) - leave it out to wait up to 90000";
    for bad in [0, 999, 180_001] {
        let mut entry = page_loader();
        entry["max_ms"] = json!(bad);
        assert_eq!(refusal(json!([entry])), want, "max_ms {bad}");
    }
    for good in [1_000, 180_000] {
        let mut entry = page_loader();
        entry["max_ms"] = json!(good);
        assert_eq!(recipe(with_loaders(json!([entry]))).validate(), Ok(()), "max_ms {good}");
    }
}

/// PeoplesHR's spinner, read from the live page (2026-10-16): one div in
/// the HRM host shell, left in the page hidden once loading finishes - so
/// it is waited for while VISIBLE, never while merely present.
#[test]
fn the_built_in_recipe_waits_for_the_peopleshr_page_loader() {
    let r = builtin_recipe();
    assert_eq!(serde_json::to_value(&r.wait_while_shown).unwrap(), json!([page_loader()]));
    assert_eq!(r.wait_while_shown[0].limit_ms(), DEFAULT_MAX_MS);
}

#[test]
fn the_sentences() {
    assert_eq!(still_loading("page loader", 90_000), "the page was still loading (page loader) after 90 s");
    assert_eq!(still_loading("page loader", 1_500), "the page was still loading (page loader) after 1.5 s");
    assert_eq!(waited("page loader", 12_340), "waited 12.3 s for page loader");
    assert_eq!(waited("page loader", 2_000), "waited 2 s for page loader");
}

// ---------------------------------------------------------------- the runner

/// What the fake page saw: every click by the locator it was aimed at, and
/// every locator looked up, in order.
struct Seen {
    clicks: Arc<Mutex<Vec<String>>>,
    looks: Arc<Mutex<Vec<String>>>,
}

impl Seen {
    fn clicks(&self) -> Vec<String> {
        self.clicks.lock().unwrap().clone()
    }
    fn looks_for(&self, css: &str) -> usize {
        self.looks.lock().unwrap().iter().filter(|s| *s == css).count()
    }
}

/// A page under a full-page spinner, the way PeoplesHR draws it:
/// `#ehrmLoader` is always in the page, and is visible for `loading_ms`
/// from the page's first look (for ever with `None`) - after that it stays
/// in the DOM, hidden (0x0). While it is up every element is covered by
/// it, the way a real browser's hit test says so. `#ready` is drawn only once the spinner has gone.
/// `#missing` is never on the page. Every other css locator is.
fn loading_page(loading_ms: Option<u64>) -> (ScriptedDriver, Seen) {
    let clicks = Arc::new(Mutex::new(Vec::<String>::new()));
    let looks = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Seen { clicks: clicks.clone(), looks: looks.clone() };
    let mut last = String::new();
    let mut began: Option<Instant> = None;
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let since = *began.get_or_insert_with(Instant::now);
        let up = loading_ms.is_none_or(|ms| since.elapsed() < Duration::from_millis(ms));
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => {
                let mut probe = common::ready_probe();
                if up {
                    probe["hit"] = json!(false);
                    probe["covered_by"] = json!("div#ehrmLoader");
                }
                json!({ "result": { "value": probe } })
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::locator::VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS => {
                json!({ "result": { "value": params["objectId"] != "loader" || up } })
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last = params["arguments"][0]["value"].as_str().unwrap().to_string();
                looks.lock().unwrap().push(last.clone());
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                if last == "#ehrmLoader" {
                    return Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "loader" } } ] }));
                }
                let there = match last.as_str() {
                    "#ready" => !up,
                    "#missing" => false,
                    _ => true,
                };
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Runtime.callFunctionOn" => json!({ "result": { "value": "text" } }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            "Page.captureScreenshot" => json!({ "data": "/9j/4AAQ" }),
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                clicks.lock().unwrap().push(last.clone());
                json!({})
            }
            _ => json!({}),
        })
    });
    (d, seen)
}

fn project_with(loader: serde_json::Value) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    save_recipe(dir.path(), "Acme", "Web", &recipe(with_loaders(json!([loader])))).unwrap();
    dir
}

fn project_with_loader() -> tempfile::TempDir {
    project_with(page_loader())
}

#[tokio::test]
async fn a_shown_loader_is_waited_out_then_the_click_runs() {
    let dir = project_with_loader();
    let (mut d, seen) = loading_page(Some(500));
    let mut acc: Option<String> = None;
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![save]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(seen.clicks(), vec!["#save".to_string()]);
    assert_eq!(out[0].detail, "clicked #save", "a wait of under a second is not noted");
    assert!(seen.looks_for("#ehrmLoader") >= 2, "it polled while the loader was up");
}

#[tokio::test]
async fn a_wait_over_a_second_is_noted_on_the_step() {
    let dir = project_with_loader();
    let (mut d, _seen) = loading_page(Some(1_300));
    let mut acc: Option<String> = None;
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![save]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.starts_with("clicked #save (waited 1."), "{}", out[0].detail);
    assert!(out[0].detail.ends_with(" s for page loader)"), "{}", out[0].detail);
}

/// `quick()` gives a click 400 ms and a wait_for here 200 ms: both are far
/// shorter than the spinner, so they pass only because their own time
/// starts once it has gone.
#[tokio::test]
async fn the_actions_own_timeout_starts_after_the_loader_is_gone() {
    let dir = project_with_loader();
    let (mut d, seen) = loading_page(Some(700));
    let mut acc: Option<String> = None;
    let wait = action(json!({ "kind": "wait_for", "selector": { "css": "#ready" }, "timeout_ms": 200 }));
    let check = action(json!({ "kind": "expect_visible", "selector": { "css": "#ready" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![wait, check]), &quick(), &mut acc).await.unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert!(seen.clicks().is_empty());
}

#[tokio::test]
async fn without_a_list_the_same_page_fails_the_way_it_always_did() {
    let dir = tempfile::tempdir().unwrap();
    let mut plain = with_loaders(json!([]));
    plain.as_object_mut().unwrap().remove("wait_while_shown");
    save_recipe(dir.path(), "Acme", "Web", &recipe(plain)).unwrap();
    let (mut d, seen) = loading_page(Some(700));
    let mut acc: Option<String> = None;
    let wait = action(json!({ "kind": "wait_for", "selector": { "css": "#ready" }, "timeout_ms": 200 }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![wait]), &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok, "{:?}", out[0]);
    assert_eq!(seen.looks_for("#ehrmLoader"), 0);
}

#[tokio::test]
async fn with_nothing_shown_the_check_is_one_look_and_nothing_more() {
    let dir = project_with_loader();
    let (mut d, seen) = loading_page(Some(0));
    let mut acc: Option<String> = None;
    let a = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let b = action(json!({ "kind": "expect_visible", "selector": { "css": "#ready" } }));
    let began = Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a, b]), &quick(), &mut acc).await.unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[0].detail, "clicked #save", "no note when nothing was waited for");
    assert_eq!(seen.looks_for("#ehrmLoader"), 2, "one look per action - no poll");
    assert!(began.elapsed() < Duration::from_millis(400), "took {:?}", began.elapsed());
}

#[tokio::test]
async fn a_loader_still_up_at_max_ms_fails_the_action_and_the_step() {
    let mut capped = page_loader();
    capped["max_ms"] = json!(1_000);
    let dir = project_with(capped);
    let (mut d, seen) = loading_page(None);
    let mut acc: Option<String> = None;
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let next = action(json!({ "kind": "click", "selector": { "css": "#next" } }));
    let began = Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![save, next]), &quick(), &mut acc).await.unwrap();
    let took = began.elapsed();
    assert!(!out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].detail, "the page was still loading (page loader) after 1 s");
    assert!(!out[1].ok, "the rest of the step does not run: {:?}", out[1]);
    assert!(seen.clicks().is_empty(), "nothing was clicked under the loader");
    assert!(took >= Duration::from_millis(1_000) && took < Duration::from_millis(2_500), "took {took:?}");
}

/// `#ehrmLoader` stays in the page once loading is done, hidden: that is
/// not loading, and nothing waits.
#[tokio::test]
async fn a_loader_left_in_the_page_hidden_causes_no_wait() {
    let dir = project_with_loader();
    let (mut d, seen) = loading_page(Some(0));
    let mut acc: Option<String> = None;
    let wait = action(json!({ "kind": "wait_for", "selector": { "css": "#ready" }, "timeout_ms": 200 }));
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let began = Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![wait, save]), &quick(), &mut acc).await.unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[1].detail, "clicked #save");
    assert_eq!(seen.looks_for("#ehrmLoader"), 2, "one look per action, each finding it hidden");
    assert!(began.elapsed() < Duration::from_millis(400), "took {:?}", began.elapsed());
}

/// A script that checks the spinner itself is left to do so.
#[tokio::test]
async fn an_action_aimed_at_the_loader_itself_is_not_held_up() {
    let dir = project_with_loader();
    let (mut d, _seen) = loading_page(None);
    let mut acc: Option<String> = None;
    let spinner = action(json!({ "kind": "expect_visible", "selector": { "css": "#ehrmLoader" } }));
    let began = Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![spinner]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(began.elapsed() < Duration::from_millis(500), "took {:?}", began.elapsed());
}

/// A `when_visible`'s own look waits too: under the spinner its target
/// would read as not there, and the guard would skip what it is for.
#[tokio::test]
async fn a_guard_looks_only_once_the_loader_is_gone() {
    let dir = project_with_loader();
    let (mut d, seen) = loading_page(Some(600));
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": "#ready" }, "within_ms": 100,
                               "then": [ { "kind": "click", "selector": { "css": "#ready" } } ] }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(seen.clicks(), vec!["#ready".to_string()], "{}", out[0].detail);
}

// ---------------------------------------------------------------- a whole run

struct One(Option<ScriptedDriver>);

impl Browsers for One {
    type D = ScriptedDriver;
    async fn open(&mut self) -> Result<Self::D, String> {
        self.0.take().ok_or_else(|| "no browser".to_string())
    }
    async fn close(&mut self, _d: Self::D) {}
}

fn new_run() -> LocalRun {
    LocalRun {
        id: "run-l".into(),
        pbi_id: 42,
        started_at: "1700000000000".into(),
        cases: vec![],
        mode: "unattended".into(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    }
}

fn first_step_waits() -> CaseScript {
    serde_json::from_value(json!({
        "case_id": 1, "title": "c",
        "steps": [{ "step_number": 1, "actions": [
            { "kind": "wait_for", "selector": { "css": "#ready" }, "timeout_ms": 200 },
            { "kind": "click", "selector": { "css": "#ready" } },
            { "kind": "expect_visible", "selector": { "css": "#ready" } }
        ] }]
    }))
    .unwrap()
}

async fn replay(root: &Path, d: ScriptedDriver, cancel: &AtomicBool) -> LocalRun {
    let script = first_step_waits();
    store::save_script(root, &script).unwrap();
    let mut run = new_run();
    run_selection(&mut One(Some(d)), root, "Acme", "Web", &mut run, &[(1, "c".into())], &quick(), cancel, &mut |_| {})
        .await
        .unwrap();
    run
}

#[tokio::test]
async fn an_unattended_run_waits_out_the_loader_on_its_first_step() {
    let dir = project_with_loader();
    let (d, seen) = loading_page(Some(700));
    let cancel = AtomicBool::new(false);
    let run = replay(dir.path(), d, &cancel).await;
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Passed", "{} {:?}", rec.reason, rec.steps);
    assert_eq!(seen.clicks(), vec!["#ready".to_string()]);
}

/// A run's Stop ends the wait at its next look.
#[tokio::test]
async fn a_stop_ends_the_wait() {
    let dir = project_with_loader();
    let (d, seen) = loading_page(None);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        flag.store(true, Ordering::SeqCst);
    });
    let began = Instant::now();
    let run = replay(dir.path(), d, &cancel).await;
    assert!(began.elapsed() < Duration::from_secs(5), "took {:?}", began.elapsed());
    let outcomes = &run.cases[0].steps.iter().find(|s| s.step_number == 1).expect("step 1 ran").outcomes;
    assert_eq!(outcomes[0].detail, AFTER_STOP, "{outcomes:?}");
    assert!(seen.clicks().is_empty());
}
