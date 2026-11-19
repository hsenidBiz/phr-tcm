//! An unattended Auto Test run keeps the signed-in browser for the next case
//! on the same account (`autorun::kept_browser`, `replay::run_cases_planned`).
//! The browser is a fake that signs in, shows its HttpOnly cookie and answers
//! `check_text` for "yes"; every fake writes to one shared log.

use crate::common::{self, account, menu_recipe, ScriptedDriver};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::accounts::{save_accounts, Account};
use v2_lib::autorun::lease;
use v2_lib::autorun::nav::{save_nav, ModulePath, NavFile};
use v2_lib::autorun::plan::Pause;
use v2_lib::autorun::preconditions::{NoDb, PreconditionDb, NEED_DB};
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::replay::{run_cases_planned, Browsers, CaseToRun, ResetGate, SIGN_IN_STEP};
use v2_lib::autorun::{store, CaseScript, LocalRun};
use v2_lib::browser::actions::{CHECK_TEXT_JS, HIGHLIGHT_JS};
use v2_lib::browser::cdp::Event;
use v2_lib::browser::input::{HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;
use v2_lib::browser::timing::Timing;
use v2_lib::events::ReplayProgress;

const ORG: &str = "Acme";
const PROJECT: &str = "Web";
const REUSED: &str = "used the browser the case before signed in";

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 20, highlight_ms: 0, lease_wait_ms: 300 }
}

/// How one fake browser misbehaves once its first check has run.
#[derive(Clone, Copy, PartialEq)]
enum Quirk {
    Plain,
    /// The signed-in marker is gone from then on.
    HideMarker,
    /// Every later `check_text` fails.
    FailChecks,
}

/// An application that signs in (`#go`), shows `#marker` once signed in,
/// holds one HttpOnly cookie, and has a menu with Leave in it.
fn app(log: Arc<Mutex<Vec<String>>>, quirk: Quirk) -> ScriptedDriver {
    let mut path = "/".to_string();
    let (mut signed_in, mut checks, mut hidden) = (false, 0usize, false);
    let (mut last_css, mut last_probed) = (String::new(), String::new());
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Page.navigate" => {
                path = v2_lib::autorun::nav::path_of(params["url"].as_str().unwrap_or(""));
                log.lock().unwrap().push(format!("navigate {path}"));
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Network.getAllCookies" => json!({ "cookies": [
                { "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "httpOnly": true, "session": true }
            ] }),
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => {
                json!({ "result": { "value": format!("https://hr.example.internal{path}") } })
            }
            "Runtime.evaluate" if params["expression"] == v2_lib::autorun::replay_to::OPEN_DIALOGS_JS => {
                json!({ "result": { "value": 0 } })
            }
            "Runtime.evaluate" => json!({ "result": { "value": null } }),
            "Accessibility.queryAXTree" => {
                let nodes = if params["role"] == "link" {
                    vec![json!({ "nodeId": "n0", "role": { "value": "link" }, "name": { "value": "Leave" }, "backendDOMNodeId": 100 })]
                } else {
                    vec![]
                };
                json!({ "nodes": nodes })
            }
            "DOM.resolveNode" => json!({ "object": { "objectId": format!("ax-{}", params["backendNodeId"]) } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => {
                last_probed = params["objectId"].as_str().unwrap_or("").to_string();
                json!({ "result": { "value": common::ready_probe() } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS => {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => {
                let want = params["arguments"][0]["value"].as_str().unwrap_or("").to_string();
                checks += 1;
                log.lock().unwrap().push(format!("check {want}"));
                let ok = want == "yes" && !(quirk == Quirk::FailChecks && checks > 1);
                hidden |= quirk == Quirk::HideMarker;
                json!({ "result": { "value": ok } })
            }
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = last_css != "#marker" || (signed_in && !hidden);
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": format!("css:{last_css}") } })] } else { vec![] } })
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                if last_probed == "css:#go" {
                    log.lock().unwrap().push("sign-in".into());
                    signed_in = true;
                    path = "/hr/home/index".into();
                } else if last_probed == "ax-100" {
                    log.lock().unwrap().push("click Leave".into());
                    path = "/hr/leave".into();
                }
                json!({})
            }
            "Page.captureScreenshot" => json!({ "data": "/9j/4AAQ" }),
            _ => json!({}),
        })
    });
    d.on_every_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }) },
    ));
    d
}

/// Hands out one fake per `open`, the quirk of each chosen by its number
/// (from 0), and counts opens and closes.
struct Fake {
    log: Arc<Mutex<Vec<String>>>,
    quirks: Vec<Quirk>,
    opened: usize,
    closed: usize,
}

impl Fake {
    fn new(quirks: &[Quirk]) -> Fake {
        Fake { log: Arc::new(Mutex::new(vec![])), quirks: quirks.to_vec(), opened: 0, closed: 0 }
    }
    fn sign_ins(&self) -> usize {
        self.log.lock().unwrap().iter().filter(|l| *l == "sign-in").count()
    }
}

impl Browsers for Fake {
    type D = ScriptedDriver;
    async fn open(&mut self) -> Result<ScriptedDriver, String> {
        let quirk = self.quirks.get(self.opened).copied().unwrap_or(Quirk::Plain);
        self.opened += 1;
        Ok(app(self.log.clone(), quirk))
    }
    async fn close(&mut self, _d: ScriptedDriver) {
        self.closed += 1;
    }
}

/// A gate that carries on at every pause.
struct Carry;

impl ResetGate for Carry {
    async fn wait(&mut self, _reset: &Pause, _remaining: &[i32], _cancel: &AtomicBool) -> bool {
        true
    }
}

fn leave() -> ModulePath {
    serde_json::from_value(json!({
        "module": "Leave",
        "area": "Leave",
        "clicks": [ { "role": "link", "name": "Leave", "exact": true } ],
        "arrived": "/hr/leave",
        "recorded": "2026-10-05T10:00:00Z"
    }))
    .unwrap()
}

/// A project with the accounts `a` and `b`, the menu app's recipe and, with
/// `routes`, a recorded Leave area (without, no case has a route).
fn project(root: &Path, routes: bool) {
    let modules = if routes { vec![leave()] } else { vec![] };
    save_nav(root, ORG, PROJECT, &NavFile { direct_urls: false, modules, save_words: vec![], protected: vec![] }).unwrap();
    save_recipe(root, ORG, PROJECT, &menu_recipe()).unwrap();
    let list: Vec<Account> = ["a", "b"].iter().map(|k| Account { key: k.to_string(), ..account() }).collect();
    save_accounts(root, &list).unwrap();
}

/// A case in the Leave area whose one check passes when `text` is "yes".
fn case(case_id: i32, account: &str, text: &str) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": case_id, "title": format!("case {case_id}"), "account": account, "area": "Leave",
        "steps": [ { "step_number": 1, "actions": [ { "kind": "check_text", "value": text } ] } ]
    }))
    .unwrap()
}

fn to_run(ids: &[i32]) -> Vec<CaseToRun> {
    ids.iter().map(|&case_id| CaseToRun { case_id, title: format!("case {case_id}"), module: None }).collect()
}

fn new_run() -> LocalRun {
    LocalRun {
        video_unavailable: false,
        id: "run-k".into(),
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

/// Runs `ids` (their scripts already saved) with no pauses and no stop.
async fn run(fake: &mut Fake, root: &Path, ids: &[i32]) -> LocalRun {
    run_with(fake, root, ids, &[], false).await
}

/// `stop_after_one` asks for a stop as case 1 finishes.
async fn run_with(fake: &mut Fake, root: &Path, ids: &[i32], resets: &[Pause], stop_after_one: bool) -> LocalRun {
    let cancel = AtomicBool::new(false);
    let mut run = new_run();
    let no_db: PreconditionDb<NoDb> = PreconditionDb::Missing(NEED_DB.to_string());
    run_cases_planned(fake, root, ORG, PROJECT, &mut run, &to_run(ids), None, false, &quick(), &cancel, &no_db, resets, &mut Carry, &mut |p: ReplayProgress| {
        if stop_after_one && p.phase == "done" && p.case_id == 1 {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .await
    .unwrap();
    run
}

fn env_of(root: &Path) -> String {
    v2_lib::environments::active_id(root).unwrap()
}

fn proposals(run: &LocalRun) -> Vec<String> {
    run.cases.iter().map(|c| format!("{}: {}", c.proposed, c.reason)).collect()
}

#[tokio::test]
async fn two_same_account_cases_open_one_browser() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (1, 1));
    assert_eq!(fake.sign_ins(), 1);
    let step0 = run.cases[1].steps.iter().find(|s| s.step_number == SIGN_IN_STEP).expect("a step 0");
    assert!(step0.outcomes[0].ok && step0.outcomes[0].detail.starts_with(REUSED), "{:?}", step0.outcomes);
    // The first case signed in as it always did.
    assert!(!run.cases[0].steps[0].outcomes.last().unwrap().detail.starts_with(REUSED));
    // The reused case still went to its module and checked.
    assert_eq!(fake.log.lock().unwrap().iter().filter(|l| *l == "check yes").count(), 2);
    // The browser is closed at the end of the run and the account let go.
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn account_change_opens_fresh() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "b", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
}

#[tokio::test]
async fn failed_case_before_opens_fresh() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "no")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert_eq!(run.cases[0].proposed, "Failed");
    assert_eq!(run.cases[1].proposed, "Passed", "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
}

#[tokio::test]
async fn no_save_case_before_opens_fresh() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    let mut first = case(1, "a", "yes");
    first.no_save = true;
    store::save_script(root, &first).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert_eq!(run.cases[1].proposed, "Passed", "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
}

#[tokio::test]
async fn no_route_case_opens_fresh() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, false);
    for id in [1, 2] {
        let mut s = case(id, "a", "yes");
        s.area = None;
        store::save_script(root, &s).unwrap();
    }
    let mut fake = Fake::new(&[]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
}

#[tokio::test]
async fn pause_between_opens_fresh() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let pause = Pause { batch: 2, before_case_id: 2, set: vec![], names: vec!["x".into()], changed_by: vec![] };
    let mut fake = Fake::new(&[]);
    let run = run_with(&mut fake, root, &[1, 2], &[pause], false).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
}

#[tokio::test]
async fn check_failing_signs_in_fresh() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    // The first browser loses its signed-in marker once case 1 has checked.
    let mut fake = Fake::new(&[Quirk::HideMarker]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
    let step0 = run.cases[1].steps.iter().find(|s| s.step_number == SIGN_IN_STEP).unwrap();
    assert!(!step0.outcomes.last().unwrap().detail.starts_with(REUSED), "{:?}", step0.outcomes);
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn failure_in_kept_browser_reruns_fresh() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    // The first browser fails every check after case 1's.
    let mut fake = Fake::new(&[Quirk::FailChecks]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert_eq!(run.cases.len(), 2, "one record for each case");
    assert_eq!(run.cases[1].proposed, "Passed", "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
    // The record kept is the fresh go's: it signed in.
    let step0 = run.cases[1].steps.iter().find(|s| s.step_number == SIGN_IN_STEP).unwrap();
    assert!(!step0.outcomes.last().unwrap().detail.starts_with(REUSED), "{:?}", step0.outcomes);
}

#[tokio::test]
async fn stop_closes_kept_browser_and_lease() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    let run = run_with(&mut fake, root, &[1, 2], &[], true).await;
    assert_eq!(run.cases.len(), 1);
    assert_eq!(fake.opened, 1);
    assert_eq!(fake.closed, fake.opened);
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn run_end_lets_lease_go() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    run(&mut fake, root, &[1, 2]).await;
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn blocked_case_between_keeps_browser() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    let flow: v2_lib::api_templates::flow::Flow = serde_json::from_value(common::cycle_flow_json()).unwrap();
    v2_lib::api_templates::flow_store::save(root, ORG, PROJECT, &flow).unwrap();
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    let mut blocked = case(2, "a", "yes");
    blocked.preconditions = vec![v2_lib::autorun::Precondition {
        flow: "pms-performance-cycle".into(),
        stage: "publish".into(),
        value: json!(274),
        why: None,
    }];
    store::save_script(root, &blocked).unwrap();
    store::save_script(root, &case(3, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    let run = run(&mut fake, root, &[1, 2, 3]).await;
    assert_eq!(run.cases[1].proposed, "Blocked");
    assert_eq!(run.cases[2].proposed, "Passed", "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (1, 1));
    assert_eq!(fake.sign_ins(), 1);
}
