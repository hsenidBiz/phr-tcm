//! An unattended Auto Test run keeps the signed-in browser for the next case
//! on the same account (`autorun::kept_browser`, `replay::run_cases_planned`).
//! The browser is a fake that signs in, shows its HttpOnly cookie and answers
//! `check_text` for "yes"; every fake writes to one shared log.

use crate::common::{self, account, menu_recipe, ScriptedDriver};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::api_templates::fixture::{Creates, Fixture, FixtureStep};
use v2_lib::api_templates::runner::{FETCH_FN, TOKEN_FN};
use v2_lib::api_templates::{fixture_store, store as template_store, ApiTemplate, Proven};
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
    /// The page re-issues a non-identity HttpOnly cookie as the steps run,
    /// and as a template request runs (a setup).
    RotateSid,
    /// The identity cookie changes as the steps run.
    RotateIdentity,
    /// The sign-in leaves no identity cookie at all.
    NoIdentity,
    /// Every later `check_text` finds the browser gone: a harness failure,
    /// which a retry-transient run goes again for.
    Gone,
    /// The no-save guard cannot be switched on.
    GuardFails,
}

/// An application that signs in (`#go`), shows `#marker` once signed in,
/// holds one HttpOnly cookie, and has a menu with Leave in it.
fn app(log: Arc<Mutex<Vec<String>>>, quirk: Quirk, fetches: Arc<Mutex<VecDeque<Value>>>) -> ScriptedDriver {
    let mut path = "/".to_string();
    let (mut signed_in, mut checks, mut hidden) = (false, 0usize, false);
    let (mut sid, mut identity, mut rotations) = ("abc".to_string(), "who", 0usize);
    let mut read = false;
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
                { "name": "sid", "value": sid.as_str(), "domain": "hr.example.internal", "path": "/", "httpOnly": true, "session": true },
                { "name": if quirk == Quirk::NoIdentity { "other" } else { "ehrm85" }, "value": identity, "domain": "hr.example.internal", "path": "/", "httpOnly": true, "session": true }
            ] }),
            // The recorder, for a run that wants a video of failed cases.
            "Browser.getVersion" => json!({ "product": "Edg/157.0.3456.78" }),
            "Page.startScreenRecording" => json!({}),
            "Page.stopScreenRecording" => json!({ "stream": "stream-1" }),
            "IO.read" => {
                let data = if std::mem::replace(&mut read, true) { String::new() } else { "AAAAGGZ0eXBtcDQy".to_string() };
                json!({ "data": data, "base64Encoded": true, "eof": true })
            }
            "IO.close" => json!({}),
            "Network.setBypassServiceWorker" if quirk == Quirk::GuardFails => return Err(v2_lib::browser::cdp::CdpError::Closed),
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
            // A template run's token and requests.
            "Runtime.callFunctionOn" if f == TOKEN_FN => json!({ "result": { "value": "tok-123" } }),
            "Runtime.callFunctionOn" if f == FETCH_FN => {
                log.lock().unwrap().push("fetch".into());
                if quirk == Quirk::RotateSid {
                    rotations += 1;
                    sid = format!("reissued-{rotations}");
                }
                let reply = fetches.lock().unwrap().pop_front().unwrap_or_else(|| reply(500, json!({ "error": "no" })));
                json!({ "result": { "value": reply } })
            }
            "Runtime.callFunctionOn" if f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => {
                let want = params["arguments"][0]["value"].as_str().unwrap_or("").to_string();
                checks += 1;
                log.lock().unwrap().push(format!("check {want}"));
                if quirk == Quirk::Gone && checks > 1 {
                    return Err(v2_lib::browser::cdp::CdpError::Closed);
                }
                let ok = want == "yes" && !(quirk == Quirk::FailChecks && checks > 1);
                hidden |= quirk == Quirk::HideMarker;
                if quirk == Quirk::RotateSid {
                    rotations += 1;
                    sid = format!("reissued-{rotations}");
                }
                if quirk == Quirk::RotateIdentity {
                    identity = "someone-else";
                }
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
    /// What the page answers each template request with, in turn.
    fetches: Arc<Mutex<VecDeque<Value>>>,
    opened: usize,
    closed: usize,
    /// Browsers open now, and the most there ever were together.
    alive: usize,
    most_alive: usize,
    video: bool,
    /// Where to count the videos on disk as each browser opens, and what
    /// each count was.
    shots: Option<std::path::PathBuf>,
    videos_at_open: Vec<usize>,
}

impl Fake {
    fn new(quirks: &[Quirk]) -> Fake {
        Fake { log: Arc::new(Mutex::new(vec![])), quirks: quirks.to_vec(), fetches: Arc::default(), opened: 0, closed: 0, alive: 0, most_alive: 0, video: false, shots: None, videos_at_open: vec![] }
    }
    fn count(&self, what: &str) -> usize {
        self.log.lock().unwrap().iter().filter(|l| *l == what).count()
    }
    fn sign_ins(&self) -> usize {
        self.log.lock().unwrap().iter().filter(|l| *l == "sign-in").count()
    }
}

impl Browsers for Fake {
    type D = ScriptedDriver;
    fn keep_failed_video(&self) -> bool {
        self.video
    }
    async fn open(&mut self) -> Result<ScriptedDriver, String> {
        let quirk = self.quirks.get(self.opened).copied().unwrap_or(Quirk::Plain);
        self.opened += 1;
        self.alive += 1;
        if let Some(dir) = &self.shots {
            let n = std::fs::read_dir(dir).map(|r| r.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".mp4")).count()).unwrap_or(0);
            self.videos_at_open.push(n);
        }
        self.most_alive = self.most_alive.max(self.alive);
        self.log.lock().unwrap().push("open".into());
        Ok(app(self.log.clone(), quirk, self.fetches.clone()))
    }
    async fn close(&mut self, _d: ScriptedDriver) {
        self.closed += 1;
        self.alive -= 1;
        self.log.lock().unwrap().push("close".into());
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
    run_full(fake, root, ids, resets, stop_after_one, false).await
}

/// `run_with`, and `retry_transient` for the run's one more go at a failure
/// that looked transient.
async fn run_full(fake: &mut Fake, root: &Path, ids: &[i32], resets: &[Pause], stop_after_one: bool, retry_transient: bool) -> LocalRun {
    run_gated(fake, root, ids, resets, stop_after_one, retry_transient, &mut Carry).await
}

/// `run_full` with the run's own gate.
async fn run_gated<G: ResetGate>(fake: &mut Fake, root: &Path, ids: &[i32], resets: &[Pause], stop_after_one: bool, retry_transient: bool, gate: &mut G) -> LocalRun {
    let cancel = AtomicBool::new(false);
    let mut run = new_run();
    let no_db: PreconditionDb<NoDb> = PreconditionDb::Missing(NEED_DB.to_string());
    run_cases_planned(fake, root, ORG, PROJECT, &mut run, &to_run(ids), None, retry_transient, &quick(), &cancel, &no_db, resets, gate, &mut |p: ReplayProgress| {
        if stop_after_one && p.phase == "done" && p.case_id == 1 {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .await
    .unwrap();
    // Never more than one signed-in browser at a time.
    assert!(fake.most_alive <= 1, "{} browsers were open together", fake.most_alive);
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

#[tokio::test]
async fn a_cookie_the_page_reissues_during_the_steps_still_lets_the_next_case_reuse() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[Quirk::RotateSid]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (1, 1));
    assert_eq!(fake.sign_ins(), 1);
}

#[tokio::test]
async fn a_changed_identity_cookie_means_the_next_case_signs_in_fresh() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[Quirk::RotateIdentity]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn the_first_gos_video_is_discarded_when_a_reused_go_is_run_again_fresh() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[Quirk::FailChecks]);
    fake.video = true;
    fake.shots = Some(root.join("shots"));
    let run = run(&mut fake, root, &[1, 2]).await;
    assert_eq!(run.cases[1].proposed, "Passed", "{:?}", proposals(&run));
    assert_eq!(run.cases[1].video, None);
    // The first go's video was on disk when the fresh go opened - so there
    // was something to discard - and is gone at the end.
    assert_eq!(fake.videos_at_open, vec![0, 1], "a video was kept after the reused go failed");
    let videos: Vec<_> = std::fs::read_dir(root.join("shots"))
        .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".mp4")).collect())
        .unwrap_or_default();
    assert!(videos.is_empty(), "{videos:?}");
}

#[tokio::test]
async fn a_sign_in_without_an_identity_cookie_is_still_kept() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[Quirk::NoIdentity]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (1, 1));
    assert_eq!(fake.sign_ins(), 1);
}

// ---- a case's setup in the kept browser ---------------------------------------

fn reply(status: u16, body: Value) -> Value {
    json!({
        "status": status,
        "contentType": "application/json; charset=utf-8",
        "finalUrl": "https://hr.example.internal/hr/pmsv10/performancecycle?handler=x",
        "redirected": false,
        "text": body.to_string(),
    })
}

fn the_cycle() -> Value {
    reply(200, json!({ "cycleId": 274, "cycleName": "AUTOTEST cycle" }))
}

/// The template `make-cycle` and the setup fixture `own`, run as `who`.
fn setup_fixture(root: &Path, who: &str) {
    let mut t: ApiTemplate = serde_json::from_value(json!({
        "id": "make-cycle", "title": "Make a cycle", "module": "PMS", "effect": "create",
        "description": "d", "sources": ["x:1"], "antiforgery": { "page": "/hr/pmsv10/performancecycle?mode=create" },
        "params": [ { "name": "cycleName", "type": "string", "required": true } ],
        "steps": [ { "name": "Cycle setup", "method": "POST", "path": "/hr/pmsv10/performancecycle",
                     "query": { "handler": "SaveProgress" }, "form": { "CycleName": "{{cycleName}}" },
                     "capture": { "cycleId": "$.cycleId", "cycleName": "$.cycleName" } } ],
        "outputs": ["cycleId", "cycleName"]
    }))
    .unwrap();
    t.proven = Some(Proven {
        at: "2026-10-01 09:00:00".into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: BTreeMap::new(),
        environment: None,
        via: None,
    });
    template_store::save(root, ORG, PROJECT, &t).unwrap();
    let f = Fixture {
        id: "own".into(),
        name: "Fixture own".into(),
        account: who.into(),
        steps: vec![FixtureStep {
            template: "make-cycle".into(),
            params: [("cycleName".to_string(), "{{prefix}} cycle".to_string())].into(),
        }],
        outputs: [("cycle_id".to_string(), "{{steps.1.cycleId}}".to_string())].into(),
        creates: vec![Creates { kind: "cycle".into(), id: "{{steps.1.cycleId}}".into(), name: "{{steps.1.cycleName}}".into() }],
        skip_stages: vec![],
    };
    fixture_store::save(root, ORG, PROJECT, &f).unwrap();
}

fn with_setup(mut s: CaseScript) -> CaseScript {
    s.setup = Some(v2_lib::autorun::Setup { fixture: "own".into() });
    s
}

/// The `open` and `close` (and with `with_fetch`, `fetch`) entries of the
/// fake's log, in order.
fn browser_order(fake: &Fake, with_fetch: bool) -> Vec<String> {
    fake.log
        .lock()
        .unwrap()
        .iter()
        .filter(|l| *l == "open" || *l == "close" || (with_fetch && *l == "fetch"))
        .cloned()
        .collect()
}

#[tokio::test]
async fn setup_same_account_runs_in_kept_browser() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    setup_fixture(root, "a");
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    let mut fake = Fake::new(&[]);
    fake.fetches.lock().unwrap().push_back(the_cycle());
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (1, 1));
    assert_eq!(fake.sign_ins(), 1);
    assert_eq!(fake.count("fetch"), 1, "the setup's request ran in the kept browser");
    // The case after the setup still used the same browser.
    let step0 = run.cases[1].steps.iter().find(|s| s.step_number == SIGN_IN_STEP).unwrap();
    assert!(step0.outcomes[0].detail.starts_with(REUSED), "{:?}", step0.outcomes);
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn setup_other_account_closes_kept_first() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    setup_fixture(root, "b");
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    let mut fake = Fake::new(&[]);
    fake.fetches.lock().unwrap().push_back(the_cycle());
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    // Case 1's browser closes before the setup's opens (`run` also asserts
    // never two at once), then the case's own browser.
    assert_eq!(browser_order(&fake, false), ["open", "close", "open", "close", "open", "close"]);
    assert_eq!(fake.count("fetch"), 1);
    assert_eq!(fake.sign_ins(), 3);
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
    assert_eq!(lease::holder_of(&env_of(root), "b"), None);
}

#[tokio::test]
async fn setup_does_not_wait_on_own_lease() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    setup_fixture(root, "a");
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    let mut fake = Fake::new(&[]);
    fake.fetches.lock().unwrap().push_back(the_cycle());
    let run = run(&mut fake, root, &[1, 2]).await;
    // The account's lease is the kept browser's own: the setup never waits
    // on it, so it is never Blocked by it.
    assert_eq!(run.cases[1].proposed, "Passed", "{:?}", proposals(&run));
    assert!(!run.cases[1].reason.contains("setup failed"), "{}", run.cases[1].reason);
}

#[tokio::test]
async fn a_setup_that_fails_in_the_kept_browser_closes_it() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    setup_fixture(root, "a");
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    store::save_script(root, &case(3, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    fake.fetches.lock().unwrap().push_back(reply(500, json!({ "error": "no" })));
    let run = run(&mut fake, root, &[1, 2, 3]).await;
    assert_eq!(run.cases[1].proposed, "Blocked", "{:?}", proposals(&run));
    assert!(run.cases[1].reason.starts_with("setup failed: "), "{}", run.cases[1].reason);
    assert_eq!(run.cases[2].proposed, "Passed", "{:?}", proposals(&run));
    // Case 1's browser was closed by the failed setup; case 3 signed in anew.
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn a_kept_browser_no_longer_signed_in_leaves_the_setup_its_own() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    setup_fixture(root, "a");
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    let mut fake = Fake::new(&[Quirk::HideMarker]);
    fake.fetches.lock().unwrap().push_back(the_cycle());
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!(browser_order(&fake, false), ["open", "close", "open", "close", "open", "close"]);
    assert_eq!(fake.count("fetch"), 1);
}

#[tokio::test]
async fn a_fresh_rerun_runs_the_setup_again_first() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    setup_fixture(root, "a");
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    // The first browser fails every check after case 1's: case 2 fails in it.
    let mut fake = Fake::new(&[Quirk::FailChecks]);
    fake.fetches.lock().unwrap().extend([the_cycle(), the_cycle()]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert_eq!(run.cases[1].proposed, "Passed", "{:?}", proposals(&run));
    // One setup in the kept browser, one more before the fresh go, in a
    // browser of its own, closed before the fresh go's browser opened.
    assert_eq!(fake.count("fetch"), 2);
    assert_eq!((fake.opened, fake.closed), (3, 3));
    assert_eq!(browser_order(&fake, true), ["open", "fetch", "close", "open", "fetch", "close", "open", "close"]);
}

#[tokio::test]
async fn a_fresh_rerun_whose_setup_cannot_run_keeps_the_first_record() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    setup_fixture(root, "a");
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    let mut fake = Fake::new(&[Quirk::FailChecks]);
    // Only the first setup is answered; the second gets a 500.
    fake.fetches.lock().unwrap().push_back(the_cycle());
    let run = run(&mut fake, root, &[1, 2]).await;
    assert_eq!(run.cases[1].proposed, "Failed", "{:?}", proposals(&run));
    assert!(run.cases[1].reason.contains(v2_lib::autorun::transient::RETRY_NOT_STARTED), "{}", run.cases[1].reason);
    assert_eq!(run.cases[1].steps.iter().filter(|s| s.step_number == SIGN_IN_STEP).count(), 1);
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn a_reruns_setup_that_cannot_start_adds_its_sentence_once() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    setup_fixture(root, "a");
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    let mut fake = Fake::new(&[Quirk::Gone]);
    // Only the setup in the kept browser is answered; the rerun's gets a 500.
    fake.fetches.lock().unwrap().push_back(the_cycle());
    let run = run_full(&mut fake, root, &[1, 2], &[], false, true).await;
    let reason = &run.cases[1].reason;
    assert_eq!(reason.matches(v2_lib::autorun::transient::RETRY_NOT_STARTED).count(), 1, "{reason}");
    // The setup ran in the kept browser and once more for the rerun: no third.
    assert_eq!(fake.count("fetch"), 2, "{reason}");
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn a_case_blocked_before_the_browser_leaves_the_kept_browser_open() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    // Case 2's setup fixture is not saved: it is Blocked before it runs.
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    store::save_script(root, &case(3, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    let run = run(&mut fake, root, &[1, 2, 3]).await;
    assert_eq!(run.cases[1].proposed, "Blocked", "{:?}", proposals(&run));
    assert!(run.cases[1].reason.starts_with("setup failed: "), "{}", run.cases[1].reason);
    assert_eq!(run.cases[2].proposed, "Passed", "{:?}", proposals(&run));
    // Case 1's browser stood through the Blocked case and served case 3.
    assert_eq!((fake.opened, fake.closed), (1, 1));
    assert_eq!(fake.sign_ins(), 1);
    let step0 = run.cases[2].steps.iter().find(|s| s.step_number == SIGN_IN_STEP).unwrap();
    assert!(step0.outcomes[0].detail.starts_with(REUSED), "{:?}", step0.outcomes);
}

// ---- the final review's findings ------------------------------------------------

/// Reports a data divergence on `case_id`, so it is Blocked before it begins.
fn needs_a_ruling(root: &Path, case_id: i32) {
    use v2_lib::autorun::divergences::{report_in, update, DivergenceKind};
    update(root, ORG, PROJECT, |list| {
        report_in(list, case_id, 1, "exactly 4 levels", "6 levels", DivergenceKind::Data, 1_700_000_000_000).map(|_| ())
    })
    .unwrap();
}

#[tokio::test]
async fn a_divergence_blocked_case_in_a_reused_browser_keeps_it_and_is_not_rerun() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    for id in [1, 2, 3] {
        store::save_script(root, &case(id, "a", "yes")).unwrap();
    }
    needs_a_ruling(root, 2);
    let mut fake = Fake::new(&[]);
    let run = run(&mut fake, root, &[1, 2, 3]).await;
    assert_eq!(run.cases.len(), 3, "the blocked case is recorded once");
    assert_eq!(run.cases[1].proposed, "Blocked", "{:?}", proposals(&run));
    assert!(run.cases[1].reason.starts_with("needs a ruling"), "{}", run.cases[1].reason);
    assert_eq!(run.cases[2].proposed, "Passed", "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (1, 1));
    assert_eq!(fake.sign_ins(), 1);
    let step0 = run.cases[2].steps.iter().find(|s| s.step_number == SIGN_IN_STEP).unwrap();
    assert!(step0.outcomes[0].detail.starts_with(REUSED), "{:?}", step0.outcomes);
}

#[tokio::test]
async fn a_no_save_guard_that_fails_in_a_reused_browser_closes_it_without_a_rerun() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    let mut guarded = case(2, "a", "yes");
    guarded.no_save = true;
    store::save_script(root, &guarded).unwrap();
    store::save_script(root, &case(3, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[Quirk::GuardFails]);
    let run = run(&mut fake, root, &[1, 2, 3]).await;
    assert_eq!(run.cases.len(), 3, "the blocked case is recorded once");
    assert_eq!(run.cases[1].proposed, "Blocked", "{:?}", proposals(&run));
    assert!(run.cases[1].reason.starts_with(v2_lib::browser::save_guard::SETUP_FAILED), "{}", run.cases[1].reason);
    assert_eq!(run.cases[2].proposed, "Passed", "{:?}", proposals(&run));
    // Case 2 went once, in case 1's browser, which was closed after it; case 3 opened its own.
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
    let step0 = run.cases[2].steps.iter().find(|s| s.step_number == SIGN_IN_STEP).unwrap();
    assert!(!step0.outcomes.last().unwrap().detail.starts_with(REUSED), "{:?}", step0.outcomes);
    assert_eq!(lease::holder_of(&env_of(root), "a"), None);
}

#[tokio::test]
async fn steps_that_go_to_the_sign_in_address_do_not_keep_the_browser() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    // A project that allows opening pages by address.
    save_nav(root, ORG, PROJECT, &NavFile { direct_urls: true, modules: vec![leave()], save_words: vec![], protected: vec![] }).unwrap();
    let mut first = case(1, "a", "yes");
    first.steps[0].actions.insert(0, serde_json::from_value(json!({ "kind": "navigate", "url": "https://hr.example.internal/hr/home/index" })).unwrap());
    store::save_script(root, &first).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
}

#[tokio::test]
async fn a_cookie_the_setup_reissues_still_lets_the_case_reuse_the_browser() {
    let _slot = crate::serial::api_template_run();
    let _log = crate::serial::activity_log();
    let _held = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    setup_fixture(root, "a");
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &with_setup(case(2, "a", "yes"))).unwrap();
    let mut fake = Fake::new(&[Quirk::RotateSid]);
    fake.fetches.lock().unwrap().push_back(the_cycle());
    let run = run(&mut fake, root, &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!(fake.count("fetch"), 1);
    assert_eq!((fake.opened, fake.closed), (1, 1));
    assert_eq!(fake.sign_ins(), 1);
}

/// A gate whose network is down before the second case; it notes whether the
/// account was let go while it waited.
struct DropOnce {
    asked: usize,
    let_go: Vec<bool>,
    root: std::path::PathBuf,
}

impl ResetGate for DropOnce {
    async fn wait(&mut self, _reset: &Pause, _remaining: &[i32], _cancel: &AtomicBool) -> bool {
        true
    }
    async fn network(&mut self) -> v2_lib::autorun::network::Network {
        self.asked += 1;
        if self.asked == 2 {
            v2_lib::autorun::network::Network::Unreachable
        } else {
            v2_lib::autorun::network::Network::Reachable
        }
    }
    async fn wait_for_network(&mut self, _before: i32, _cancel: &AtomicBool) -> v2_lib::autorun::network::WaitEnd {
        self.let_go.push(lease::holder_of(&env_of(&self.root), "a").is_none());
        v2_lib::autorun::network::WaitEnd::Resumed
    }
}

#[tokio::test]
async fn a_network_pause_between_same_account_cases_closes_the_kept_browser() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root, true);
    store::save_script(root, &case(1, "a", "yes")).unwrap();
    store::save_script(root, &case(2, "a", "yes")).unwrap();
    let mut fake = Fake::new(&[]);
    let mut gate = DropOnce { asked: 0, let_go: vec![], root: root.to_path_buf() };
    let run = run_gated(&mut fake, root, &[1, 2], &[], false, false, &mut gate).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", proposals(&run));
    assert_eq!((fake.opened, fake.closed), (2, 2));
    assert_eq!(fake.sign_ins(), 2);
    assert_eq!(gate.let_go, vec![true], "the account was let go during the pause");
}
