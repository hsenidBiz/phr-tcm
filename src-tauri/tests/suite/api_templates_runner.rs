//! Running an API template from inside a signed-in page - design doc "API
//! templates" §5 (the runner) and §9 (errors).
//!
//! The page is `common::stateful_app` (the same sign-in fake the Auto Run
//! tests use) wrapped in `App`, which answers the three things only this
//! runner asks a page: where it is (`location.href`), what its anti-forgery
//! token is (`TOKEN_FN`) and what a request came back with (`FETCH_FN`).

use crate::common::{account, activity_records, quick, recipe, stateful_app, ScriptedDriver, StatefulApp};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use v2_lib::api_templates::runner::{
    claim, preflight, run_template, run_template_within, Mode, RunReport, RunRequest, FETCH_FN, TOKEN_FN,
};
use v2_lib::api_templates::{ApiTemplate, Proven};
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::replay::Browsers;
use v2_lib::browser::cdp::{CdpError, Driver, Event};

const ORG: &str = "acme";
const PROJECT: &str = "PMS";
const TOKEN: &str = "tok-123";
const PAGE: &str = "/hr/pmsv10/performancecycle?mode=create";
const LOGIN: &str = "https://hr.example.internal/hr/security/login?ReturnUrl=%2Fhr%2Fpmsv10";

/// What the page answers with, and what it was asked - shared between the
/// driver (which the runner owns while it runs) and the test.
#[derive(Default)]
struct Script {
    /// Answers to `location.href`, in turn; once empty, the address last
    /// navigated to.
    hrefs: VecDeque<String>,
    token: Option<String>,
    /// Answers to `FETCH_FN`, in turn.
    responses: VecDeque<Value>,
    /// `FETCH_FN` never answers - the page is stuck.
    hang_fetch: bool,
    /// `Page.navigate` starts but its page never finishes loading.
    never_loads: bool,
    /// `Page.navigate` fails as a browser that has gone away does.
    browser_gone: bool,
    /// The arguments of every `FETCH_FN` call, in order.
    fetched: Vec<Vec<Value>>,
    navigated: String,
}

struct App {
    inner: ScriptedDriver,
    script: Arc<Mutex<Script>>,
}

impl Driver for App {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        let f = params["functionDeclaration"].as_str().unwrap_or("").to_string();
        if method == "Runtime.evaluate" && params["expression"] == "location.href" {
            let href = {
                let mut s = self.script.lock().unwrap();
                let navigated = s.navigated.clone();
                s.hrefs.pop_front().unwrap_or(navigated)
            };
            return Ok(json!({ "result": { "type": "string", "value": href } }));
        }
        if method == "Runtime.callFunctionOn" && f == TOKEN_FN {
            let token = self.script.lock().unwrap().token.clone();
            return Ok(json!({ "result": { "value": token } }));
        }
        if method == "Runtime.callFunctionOn" && f == FETCH_FN {
            let args: Vec<Value> =
                params["arguments"].as_array().unwrap().iter().map(|a| a["value"].clone()).collect();
            let (hang, reply) = {
                let mut s = self.script.lock().unwrap();
                s.fetched.push(args);
                (s.hang_fetch, s.responses.pop_front())
            };
            if hang {
                std::future::pending::<()>().await;
            }
            return Ok(json!({ "result": { "value": reply.expect("a request nobody scripted an answer for") } }));
        }
        if method == "Page.navigate" {
            let (never_loads, gone) = {
                let mut s = self.script.lock().unwrap();
                s.navigated = params["url"].as_str().unwrap_or("").to_string();
                (s.never_loads, s.browser_gone)
            };
            if gone {
                return Err(CdpError::Closed);
            }
            if never_loads {
                // Not passed on to `inner`, so no load event ever follows.
                return Ok(json!({ "frameId": "F", "loaderId": "L" }));
            }
        }
        self.inner.call(method, params).await
    }

    async fn wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError> {
        self.inner.wait_event(method, limit).await
    }

    fn forget_events(&mut self) {
        self.inner.forget_events()
    }

    fn take_dialogs(&mut self) -> Vec<String> {
        self.inner.take_dialogs()
    }

    fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.inner.set_deadline(deadline)
    }
}

/// Hands out one prepared `App` and counts opens and closes; keeps the
/// closed driver so the test can read what it was asked.
struct FakeBrowsers {
    next: Option<App>,
    opened: usize,
    closed: usize,
    last: Option<App>,
}

impl Browsers for FakeBrowsers {
    type D = App;

    async fn open(&mut self) -> Result<App, String> {
        self.opened += 1;
        self.next.take().ok_or_else(|| "no browser left".to_string())
    }

    async fn close(&mut self, d: App) {
        self.closed += 1;
        self.last = Some(d);
    }
}

struct Rig {
    browsers: FakeBrowsers,
    script: Arc<Mutex<Script>>,
    state: StatefulApp,
    root: tempfile::TempDir,
}

impl Rig {
    fn fetched(&self) -> Vec<Vec<Value>> {
        self.script.lock().unwrap().fetched.clone()
    }
    /// How many times the recipe was run: its one click per sign-in.
    fn sign_ins(&self) -> usize {
        self.state.clicks.load(Ordering::SeqCst)
    }
    fn navigations_to_the_page(&self) -> usize {
        let d = self.browsers.last.as_ref().expect("the browser was closed");
        d.inner.calls_to("Page.navigate").iter().filter(|p| p["url"].as_str().unwrap_or("").ends_with(PAGE)).count()
    }
}

/// A root with the recipe and the account on disk, and a page that signs
/// in (unless `broken` names a selector it lacks) and answers `responses`.
fn rig(responses: Vec<Value>, broken: Option<&'static str>) -> Rig {
    let root = tempfile::tempdir().unwrap();
    save_recipe(root.path(), ORG, PROJECT, &recipe()).unwrap();
    save_accounts(root.path(), &[account()]).unwrap();
    let (inner, state) = stateful_app(false, broken);
    let script = Arc::new(Mutex::new(Script {
        token: Some(TOKEN.to_string()),
        responses: responses.into(),
        ..Script::default()
    }));
    let app = App { inner, script: script.clone() };
    Rig { browsers: FakeBrowsers { next: Some(app), opened: 0, closed: 0, last: None }, script, state, root }
}

/// The design doc's §4 example, plus a third step for the stop-at-first-
/// failure test.
fn template() -> ApiTemplate {
    serde_json::from_value(json!({
        "id": "pms-create-draft-cycle",
        "title": "Create a draft performance cycle",
        "module": "PMS / Performance Cycle",
        "effect": "create",
        "description": "Cycle setup + evaluation rules; leaves the cycle in Draft.",
        "sources": ["Pages/PerformanceCycle/Index.CycleSetup.cshtml.cs:95"],
        "antiforgery": { "page": PAGE },
        "params": [ { "name": "cycleName", "type": "string", "required": true } ],
        "steps": [
            { "name": "Cycle setup", "method": "POST",
              "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveProgress" },
              "form": { "CycleName": "{{cycleName}}" },
              "expect": { "status": 200, "json": { "success": true } },
              "capture": { "cycleId": "$.cycleId" } },
            { "name": "Evaluation rules", "method": "POST",
              "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveEvalRulesProgress" },
              "form": { "CycleId": "{{cycleId}}" },
              "expect": { "status": 200, "json": { "success": true } } }
        ],
        "outputs": ["cycleId"]
    }))
    .unwrap()
}

fn three_steps() -> ApiTemplate {
    let mut t = template();
    t.steps.push(
        serde_json::from_value(json!({
            "name": "Publish", "method": "POST",
            "path": "/hr/pmsv10/performancecycle", "query": { "handler": "Publish" },
            "json": { "cycleId": "{{cycleId}}" }
        }))
        .unwrap(),
    );
    t
}

fn request(t: ApiTemplate, mode: Mode) -> RunRequest {
    let mut values = serde_json::Map::new();
    values.insert("cycleName".into(), json!("FY27"));
    RunRequest { org: ORG.into(), project: PROJECT.into(), account: "admin".into(), values, mode, template: t }
}

fn prove() -> Mode {
    Mode::Prove { replace: false, why: None }
}

/// What the page's `FETCH_FN` hands back for a plain JSON answer.
fn answer(status: u16, body: Value) -> Value {
    json!({
        "status": status,
        "contentType": "application/json; charset=utf-8",
        "finalUrl": "https://hr.example.internal/hr/pmsv10/performancecycle?handler=x",
        "redirected": false,
        "text": body.to_string(),
    })
}

async fn run(r: &mut Rig, t: ApiTemplate) -> RunReport {
    let req = request(t, prove());
    run_template(&mut r.browsers, r.root.path(), &req, &quick()).await
}

#[tokio::test]
async fn a_clean_run_captures_and_returns_outputs() {
    let _act = crate::serial::activity_log();
    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(report.failed, None);
    assert_eq!(report.template, "pms-create-draft-cycle");
    assert_eq!(report.outputs, BTreeMap::from([("cycleId".to_string(), json!(274))]));
    assert_eq!(report.steps.len(), 2, "{:?}", report.steps);
    assert!(report.steps.iter().all(|s| s.ok && s.status == Some(200)), "{:?}", report.steps);
    assert_eq!(report.steps[0].handler.as_deref(), Some("SaveProgress"));

    let fetched = r.fetched();
    assert_eq!(fetched.len(), 2);
    assert_eq!(
        fetched[0][0],
        json!({ "method": "POST", "url": "/hr/pmsv10/performancecycle?handler=SaveProgress",
                "body": { "kind": "form", "fields": { "CycleName": "FY27" } } })
    );
    assert_eq!(fetched[1][0]["body"], json!({ "kind": "form", "fields": { "CycleId": "274" } }));
    assert_eq!(r.sign_ins(), 1);
}

#[tokio::test]
async fn the_token_goes_in_the_header_and_nowhere_else() {
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");

    let fetched = r.fetched();
    assert_eq!(fetched.len(), 2);
    for args in &fetched {
        assert_eq!(args.len(), 2, "{args:?}");
        assert_eq!(args[1], json!(TOKEN));
        assert!(!args[0].to_string().contains(TOKEN), "the token rode in the request itself: {}", args[0]);
    }

    let text = serde_json::to_string(&report).unwrap();
    assert!(!text.contains(TOKEN), "the token reached the report: {text}");

    let records = activity_records(activity.path(), "api");
    assert_eq!(records.len(), 2, "{records:?}");
    for rec in &records {
        assert!(!rec.to_string().contains(TOKEN), "the token reached an activity record: {rec}");
    }
    let first = &records[0];
    assert_eq!(first["template"], "pms-create-draft-cycle");
    assert_eq!(first["mode"], "prove");
    assert_eq!(first["account"], "admin");
    assert_eq!(first["origin"], "https://hr.example.internal");
    assert_eq!(first["step"], "Cycle setup");
    assert_eq!(first["method"], "POST");
    assert_eq!(first["url"], "/hr/pmsv10/performancecycle?handler=SaveProgress");
    assert_eq!(first["handler"], "SaveProgress");
    assert_eq!(first["status"], 200);
    assert!(first["duration_ms"].is_u64(), "{first}");
    assert!(first["request"].as_str().unwrap().contains("FY27"), "{first}");
    assert!(first["response"].as_str().unwrap().contains("274"), "{first}");

    let log = v2_lib::applog::recent(400);
    for line in &log {
        assert!(!line.message.contains(TOKEN), "the token reached the app log: {}", line.message);
    }
    assert!(
        log.iter().any(|l| l.message == "api template pms-create-draft-cycle: prove ok, 2 steps"),
        "no summary line in {:?}",
        log.iter().map(|l| &l.message).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_failure_stops_the_run_and_says_what_was_created() {
    let _act = crate::serial::activity_log();
    let mut r = rig(
        vec![
            answer(200, json!({ "success": true, "cycleId": 274 })),
            json!({ "status": 400, "contentType": "text/plain", "finalUrl": "https://hr.example.internal/hr/pmsv10/performancecycle?handler=SaveEvalRulesProgress",
                    "redirected": false, "text": "the rating method is not set" }),
            answer(200, json!({ "success": true })),
        ],
        None,
    );
    let report = run(&mut r, three_steps()).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("Evaluation rules"));
    assert_eq!(report.created, BTreeMap::from([("cycleId".to_string(), json!(274))]));
    assert!(report.outputs.is_empty(), "{:?}", report.outputs);
    assert_eq!(r.fetched().len(), 2, "the step after the failure was sent");
    assert_eq!(report.steps.len(), 2, "{:?}", report.steps);
    let failed = &report.steps[1];
    assert!(!failed.ok);
    assert_eq!(failed.status, Some(400));
    assert_eq!(failed.handler.as_deref(), Some("SaveEvalRulesProgress"));
    assert!(failed.detail.contains("expected status 200, got 400"), "{}", failed.detail);
    assert!(failed.detail.contains("the rating method is not set"), "{}", failed.detail);

    let message = report.message();
    assert!(message.contains("cycleId 274 created"), "{message}");
    assert!(message.contains("failed at Evaluation rules"), "{message}");
    assert!(message.contains("SaveEvalRulesProgress"), "{message}");
    assert!(!message.contains("example.internal"), "a host reached the message: {message}");
}

#[tokio::test]
async fn a_stale_session_at_the_token_page_signs_in_once_more() {
    let _act = crate::serial::activity_log();
    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().hrefs.push_back(LOGIN.to_string());
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.sign_ins(), 2, "it should have signed in exactly once more");
    assert_eq!(r.navigations_to_the_page(), 2);
}

#[tokio::test]
async fn a_token_page_that_stays_somewhere_else_fails_after_one_more_try() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().hrefs.extend([LOGIN.to_string(), LOGIN.to_string()]);
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    let last = report.steps.last().unwrap();
    assert_eq!(last.detail, "the token page sent us to another page - check the template's antiforgery page");
    assert_eq!(r.sign_ins(), 2);
    assert!(r.fetched().is_empty());
}

#[tokio::test]
async fn a_login_redirect_mid_run_fails_rather_than_signing_in_again() {
    let _act = crate::serial::activity_log();
    let mut r = rig(
        vec![json!({ "status": 200, "contentType": "text/html", "finalUrl": LOGIN, "redirected": true,
                     "text": "<html><form id=login></form></html>" })],
        None,
    );
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("Cycle setup"));
    let step = &report.steps[0];
    assert!(step.detail.contains("was sent to another page - the session may have ended"), "{}", step.detail);
    assert!(!step.detail.contains("example.internal"), "a host reached the detail: {}", step.detail);
    assert_eq!(r.sign_ins(), 1, "it signed in again halfway through");
    assert_eq!(r.fetched().len(), 1);
}

#[tokio::test]
async fn the_browser_is_closed_on_every_path() {
    let _act = crate::serial::activity_log();

    // Success.
    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    assert!(run(&mut r, template()).await.ok);
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1), "success");

    // A step fails.
    let mut r = rig(vec![answer(500, json!({ "success": false }))], None);
    let report = run(&mut r, template()).await;
    assert_eq!(report.failed.as_deref(), Some("Cycle setup"));
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1), "step failure");

    // The sign-in fails: the recipe's button is not on the page.
    let mut r = rig(vec![], Some("#go"));
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    let detail = &report.steps.last().unwrap().detail;
    assert!(detail.starts_with("could not sign in as \"admin\": "), "{detail}");
    assert!(
        detail.ends_with(" - sign that account in once from Auto Run, then try again"),
        "{detail}"
    );
    assert!(r.fetched().is_empty());
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1), "sign-in failure");

    // No token on the page.
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().token = None;
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    assert_eq!(report.steps.last().unwrap().detail, format!("no anti-forgery token on {PAGE}"));
    assert!(r.fetched().is_empty());
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1), "token missing");

    // The page never answers a request: the run's own limit ends it.
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().hang_fetch = true;
    let req = request(template(), prove());
    let report =
        run_template_within(&mut r.browsers, r.root.path(), &req, &quick(), Duration::from_secs(2)).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("Cycle setup"));
    assert_eq!(report.steps.last().unwrap().detail, "the run took longer than 3 minutes");
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1), "timeout");
    let d = r.browsers.last.as_ref().unwrap();
    assert!(d.inner.deadline_was_cleared(), "the step's deadline outlived the run");
}

/// Nothing a person or the assistant reads names a host or carries a raw
/// browser error: a sign-in failure's own wording can do both, so it goes
/// to the app log as it is and reaches the report without them.
#[tokio::test]
async fn a_failed_sign_in_names_no_host_and_no_raw_browser_error() {
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();

    // The sign-in page never loads: Auto Run's detail is "the sign-in page
    // did not open: https://hr.example.internal/ did not finish loading ...".
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().never_loads = true;
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("Sign in"));
    let detail = report.steps.last().unwrap().detail.clone();
    let message = report.message();
    for text in [&detail, &message] {
        assert!(!text.contains("example.internal") && !text.contains("://"), "a host reached: {text}");
    }
    assert!(detail.starts_with("could not sign in as \"admin\": the sign-in page did not open: / did not finish loading"), "{detail}");
    assert!(detail.ends_with(" - sign that account in once from Auto Run, then try again"), "{detail}");
    let log = v2_lib::applog::recent(400);
    assert!(
        log.iter().any(|l| l.message.contains("https://hr.example.internal/ did not finish loading")),
        "the raw detail did not reach the app log: {:?}",
        log.iter().map(|l| &l.message).collect::<Vec<_>>()
    );
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1));

    // The browser itself stops answering: a fixed sentence, the raw error
    // in the log only.
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().browser_gone = true;
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    let detail = report.steps.last().unwrap().detail.clone();
    assert_eq!(
        detail,
        "could not sign in as \"admin\": the browser stopped answering while signing in - see Settings, Logs"
    );
    let message = report.message();
    assert!(!message.contains("://") && !message.contains("did not answer:"), "{message}");
    let log = v2_lib::applog::recent(400);
    assert!(
        log.iter().any(|l| l.message.contains("signing in as \"admin\"") && l.message.contains("the browser did not answer")),
        "the raw detail did not reach the app log: {:?}",
        log.iter().map(|l| &l.message).collect::<Vec<_>>()
    );
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1));
}

#[test]
fn only_one_run_at_a_time() {
    let _g = crate::serial::api_template_run();
    let first = claim().expect("nothing else is running");
    assert!(claim().is_none(), "a second run was allowed alongside the first");
    drop(first);
    let again = claim();
    assert!(again.is_some(), "dropping the claim did not free the slot");
}

#[test]
fn preflight_needs_replace_and_why_for_an_existing_id() {
    let root = tempfile::tempdir().unwrap();
    save_recipe(root.path(), ORG, PROJECT, &recipe()).unwrap();
    save_accounts(root.path(), &[account()]).unwrap();
    let existing = template();
    let refused = "a template called \"pms-create-draft-cycle\" already exists - send replace: true and a why to change it";

    // No template by that id yet: fine.
    assert_eq!(preflight(root.path(), &request(template(), prove()), None), Ok(()));

    // One exists: refused without replace, without a why, or with a blank why.
    for mode in [
        Mode::Prove { replace: false, why: None },
        Mode::Prove { replace: false, why: Some("a better handler".into()) },
        Mode::Prove { replace: true, why: None },
        Mode::Prove { replace: true, why: Some("   ".into()) },
    ] {
        let problems = preflight(root.path(), &request(template(), mode), Some(&existing)).unwrap_err();
        assert_eq!(problems, vec![refused.to_string()]);
    }
    let replacing = Mode::Prove { replace: true, why: Some("the handler was renamed".into()) };
    assert_eq!(preflight(root.path(), &request(template(), replacing), Some(&existing)), Ok(()));

    // Running a saved template: it exists and carries `proven`, and that is
    // exactly what a saved one looks like.
    let mut saved = template();
    saved.proven = Some(Proven {
        at: "2026-09-28T10:14:00Z".into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: BTreeMap::new(),
    });
    assert_eq!(preflight(root.path(), &request(saved.clone(), Mode::Run), Some(&saved)), Ok(()));

    // Every problem comes back together.
    let mut bad = request(template(), prove());
    bad.account = "nobody".into();
    bad.values.insert("cycleName".into(), json!(33));
    bad.values.insert("extra".into(), json!("x"));
    let problems = preflight(root.path(), &bad, Some(&existing)).unwrap_err();
    assert_eq!(problems.len(), 4, "{problems:?}");
    assert!(problems.iter().any(|p| p == "param 'cycleName' must be a string"), "{problems:?}");
    assert!(problems.iter().any(|p| p == "param 'extra' is not declared by this template"), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("no account \"nobody\"")), "{problems:?}");
    assert!(problems.iter().any(|p| p == refused), "{problems:?}");

    // A template that fails its own checks, and a project with no recipe.
    let mut broken = template();
    broken.steps[0].path = "https://elsewhere.example/x".into();
    let mut req = request(broken, prove());
    req.project = "NoRecipe".into();
    let problems = preflight(root.path(), &req, None).unwrap_err();
    assert!(problems.iter().any(|p| p.contains("not a safe relative path")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("no sign-in recipe")), "{problems:?}");
}
