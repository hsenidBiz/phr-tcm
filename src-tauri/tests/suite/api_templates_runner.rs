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
    claim, preflight, run_template_within, Mode, RunReport, RunRequest, FETCH_FN, RETRY_PAUSES, RUN_LIMIT, TOKEN_FN,
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
    /// Answers to `TOKEN_FN`, in turn; once empty, `token`.
    tokens: VecDeque<Option<String>>,
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
    /// What `Network.getCookies` answers (its `cookies` array); `None` is
    /// an empty jar.
    cookies: Option<Value>,
    /// `Network.getCookies` fails, as a browser that refuses it does.
    cookies_fail: bool,
    /// The `urls` of every `Network.getCookies` call, in order.
    cookie_urls: Vec<Value>,
    /// What `Network.getAllCookies` answers; `None` leaves it to the
    /// sign-in fake underneath.
    all_cookies: Option<Value>,
    /// `Network.getAllCookies` fails once the run has signed in (the
    /// sign-in's own session capture still gets its answer).
    all_cookies_fail: bool,
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
            let token = {
                let mut s = self.script.lock().unwrap();
                let fallback = s.token.clone();
                s.tokens.pop_front().unwrap_or(fallback)
            };
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
        if method == "Network.getAllCookies" {
            let s = self.script.lock().unwrap();
            // The token page has been asked for: the run is past its sign-in.
            if s.all_cookies_fail && !s.cookie_urls.is_empty() {
                return Err(CdpError::Protocol { method: method.into(), message: "not allowed here".into() });
            }
            if let Some(all) = s.all_cookies.clone() {
                return Ok(json!({ "cookies": all }));
            }
        }
        if method == "Network.getCookies" {
            let mut s = self.script.lock().unwrap();
            s.cookie_urls.push(params["urls"].clone());
            if s.cookies_fail {
                return Err(CdpError::Protocol { method: method.into(), message: "not allowed here".into() });
            }
            return Ok(json!({ "cookies": s.cookies.clone().unwrap_or(json!([])) }));
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

/// A 400 with no body: the application refused the request before any
/// handler read it (hosted PeoplesHR does this at busy moments, and when
/// the account's session was taken over).
fn empty_400() -> Value {
    json!({
        "status": 400,
        "contentType": "",
        "finalUrl": "https://hr.example.internal/hr/pmsv10/performancecycle?handler=x",
        "redirected": false,
        "text": "",
    })
}

/// The activity records of requests the page made - not the token page's.
fn step_records(records: &[Value]) -> Vec<&Value> {
    records.iter().filter(|r| r.get("step").is_some()).collect()
}

/// The retry pauses the tests use: as many as the real ones, but short.
const QUICK_PAUSES: [Duration; 3] = [Duration::from_millis(10); 3];

async fn run(r: &mut Rig, t: ApiTemplate) -> RunReport {
    let req = request(t, prove());
    run_template_within(&mut r.browsers, r.root.path(), &req, &quick(), RUN_LIMIT, &QUICK_PAUSES).await
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
        assert_eq!(args.len(), 3, "{args:?}");
        assert_eq!(args[1], json!(TOKEN));
        assert_eq!(args[2], json!(30_000), "the page aborts at the step's own limit");
        assert!(!args[0].to_string().contains(TOKEN), "the token rode in the request itself: {}", args[0]);
    }

    let text = serde_json::to_string(&report).unwrap();
    assert!(!text.contains(TOKEN), "the token reached the report: {text}");

    let records = activity_records(activity.path(), "api");
    for rec in &records {
        assert!(!rec.to_string().contains(TOKEN), "the token reached an activity record: {rec}");
    }
    let steps = step_records(&records);
    assert_eq!(steps.len(), 2, "{records:?}");
    let first = steps[0];
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

/// The token page at the right address with NO token on it: the session
/// ended where the application does not redirect (hosted PMSV10 renders
/// `/hr/pmsv10/updatehub` for anyone). Same as a login redirect - sign in
/// once more and read the page again.
#[tokio::test]
async fn a_token_page_without_a_token_signs_in_once_more() {
    let _act = crate::serial::activity_log();
    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().tokens.push_back(None);
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.sign_ins(), 2, "it should have signed in exactly once more");
    assert_eq!(r.navigations_to_the_page(), 2);
}

/// Still no token after signing in again: the run fails, once, and says so.
#[tokio::test]
async fn a_token_page_that_never_has_a_token_fails_after_one_more_sign_in() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().token = None;
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    assert_eq!(report.steps.last().unwrap().detail, format!("no anti-forgery token on {PAGE}"));
    assert_eq!(r.sign_ins(), 2);
    assert_eq!(r.navigations_to_the_page(), 2);
    assert!(r.fetched().is_empty());
}

/// What hosted did twice on 2026-09-29: a step refused unread, and the
/// retry's token page had no token because the session had just ended.
/// The retry signs in again and goes through.
#[tokio::test]
async fn a_retry_whose_token_page_lost_its_token_signs_in_again() {
    let _act = crate::serial::activity_log();
    let mut r = rig(
        vec![empty_400(), answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().tokens.extend([Some(TOKEN.to_string()), None]);
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.sign_ins(), 2);
    assert_eq!(r.fetched().len(), 3, "step 1 twice, step 2 once");
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
        run_template_within(&mut r.browsers, r.root.path(), &req, &quick(), Duration::from_secs(2), &QUICK_PAUSES).await;
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

/// The application's own pages carry anti-forgery tokens - every Razor
/// form writes one into a hidden input, and a JSON answer can carry one
/// too. Whether it is the token the runner read or another one entirely,
/// its value reaches no report, no activity record and no log line.
#[tokio::test]
async fn a_token_in_a_response_body_reaches_no_report_record_or_log() {
    const OTHER: &str = "other-secret-999";
    const IN_JSON: &str = "json-secret-777";
    const SINGLE: &str = "single-quoted-555";
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let html = format!(
        "<html><form><input name=\"__RequestVerificationToken\" type=\"hidden\" value=\"{OTHER}\" />\
         <input type='hidden' value='{TOKEN}' name='__RequestVerificationToken'>\
         <input value=\"{SINGLE}\" name=\"RequestVerificationToken\"> the rating method is not set; \
         the token was {TOKEN}</form></html>"
    );
    let mut r = rig(
        vec![
            answer(200, json!({ "success": true, "cycleId": 274, "echo": TOKEN, "RequestVerificationToken": IN_JSON })),
            json!({ "status": 400, "contentType": "text/html",
                    "finalUrl": "https://hr.example.internal/hr/pmsv10/performancecycle?handler=SaveEvalRulesProgress",
                    "redirected": false, "text": html }),
        ],
        None,
    );
    // A capture that picks the token up (a mistaken path) still hands it
    // on to the next step - but the report only ever says "(token)".
    let mut t = template();
    t.steps[0].capture.insert("echo".into(), "$.echo".into());
    t.steps[1].form.as_mut().unwrap().insert("Echo".into(), "{{echo}}".into());
    let report = run(&mut r, t).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("Evaluation rules"));
    assert_eq!(report.created.get("echo"), Some(&json!("(token)")), "{:?}", report.created);
    assert_eq!(r.fetched()[1][0]["body"]["fields"]["Echo"], json!(TOKEN), "the next step got the real value");
    let detail = &report.steps.last().unwrap().detail;
    assert!(detail.contains("the rating method is not set"), "the body was not shown at all: {detail}");
    assert!(detail.contains("(token)"), "the token was not replaced: {detail}");

    let secrets = [TOKEN, OTHER, IN_JSON, SINGLE];
    let text = serde_json::to_string(&report).unwrap();
    let message = report.message();
    for s in secrets {
        assert!(!text.contains(s), "{s} reached the report: {text}");
        assert!(!message.contains(s), "{s} reached the message: {message}");
    }
    let records = activity_records(activity.path(), "api");
    for rec in &records {
        for s in secrets {
            assert!(!rec.to_string().contains(s), "{s} reached an activity record: {rec}");
        }
    }
    let steps = step_records(&records);
    assert_eq!(steps.len(), 2, "{records:?}");
    assert!(steps[0]["response"].as_str().unwrap().contains("274"), "{}", steps[0]);
    let log = v2_lib::applog::recent(400);
    for line in &log {
        for s in secrets {
            assert!(!line.message.contains(s), "{s} reached the app log: {}", line.message);
        }
    }
}

/// A page answer with neither a status nor an error is logged by its
/// keys only - never the body it may be carrying.
#[tokio::test]
async fn an_answer_with_no_status_logs_its_keys_not_its_body() {
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![json!({ "text": "BODY-SHOULD-NOT-BE-LOGGED", "weird": 1 })], None);
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    assert_eq!(
        report.steps.last().unwrap().detail,
        "the page gave no answer for this request - see Settings, Logs"
    );
    let log = v2_lib::applog::recent(400);
    let messages: Vec<&String> = log.iter().map(|l| &l.message).collect();
    assert!(
        messages.iter().all(|m| !m.contains("BODY-SHOULD-NOT-BE-LOGGED")),
        "the body reached the app log: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("the page answered an object with keys: text, weird")),
        "no keys line in {messages:?}"
    );
}

/// The token page is only the token page on the recipe's own origin: the
/// same path somewhere else is "another page", which gets the one more
/// sign-in a stale session does and then fails.
#[tokio::test]
async fn a_token_page_on_another_origin_is_another_page() {
    let _act = crate::serial::activity_log();
    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    let elsewhere = format!("https://elsewhere.example{PAGE}");
    r.script.lock().unwrap().hrefs.extend([elsewhere.clone(), elsewhere]);
    let report = run(&mut r, template()).await;
    assert!(!report.ok, "{report:?}");
    assert_eq!(
        report.steps.last().unwrap().detail,
        "the token page sent us to another page - check the template's antiforgery page"
    );
    assert_eq!(r.sign_ins(), 2);
    assert!(r.fetched().is_empty(), "a request was sent from a page on another origin");

    // Once, then the real page: the stale-session path, and it runs.
    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().hrefs.push_back(format!("https://login.elsewhere.example{PAGE}"));
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.sign_ins(), 2);
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
        environment: None,
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

    // A template that fails its own checks, and a project with no recipe
    // and no site address for the built-in one.
    let mut broken = template();
    broken.steps[0].path = "https://elsewhere.example/x".into();
    let mut req = request(broken, prove());
    req.project = "NoRecipe".into();
    let problems = preflight(root.path(), &req, None).unwrap_err();
    assert!(problems.iter().any(|p| p.contains("not a safe relative path")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("set the site address first")), "{problems:?}");
}

/// The bridge's prove and run, end to end against the fake page: what is
/// saved, what is appended to the history, what is logged and announced.
/// `api_template_prove` / `api_template_run` are the route arms with the
/// browser factory handed in, so these reach everything but a real
/// browser.
mod through_the_bridge {
    use super::*;
    use v2_lib::ai_bridge::{api_template_prove, api_template_run, set_templates_sink, BridgeContext};
    use v2_lib::api_templates::store;
    use v2_lib::browser::launch::Browser;

    const ID: &str = "pms-create-draft-cycle";

    fn ctx() -> BridgeContext {
        BridgeContext { org: ORG.into(), project: PROJECT.into(), api_writes: true, ..BridgeContext::default() }
    }

    /// Every id the templates-changed sink has been handed. The sink is a
    /// process-wide `OnceLock`: every test installs the same recorder, the
    /// first one wins, and each clears the list under the run lock it holds.
    pub(super) fn changes() -> &'static Mutex<Vec<String>> {
        static CHANGES: Mutex<Vec<String>> = Mutex::new(Vec::new());
        set_templates_sink(Box::new(|id| CHANGES.lock().unwrap().push(id)));
        CHANGES.lock().unwrap().clear();
        &CHANGES
    }

    /// The database a template on no flow must never ask for: none of the
    /// templates here names a stage, so each of these runs exactly as it
    /// did before flows existed (design doc "API template flows", Review
    /// Focus 4).
    pub(super) fn no_db(_: &BridgeContext) -> Result<crate::common::FakeStageDb, (u16, String)> {
        panic!("a database was asked for by a template that is on no flow")
    }

    pub(super) fn proven_copy() -> ApiTemplate {
        let mut t = template();
        t.proven = Some(Proven {
            at: "2026-09-01 09:00:00".into(),
            origin: "https://hr.example.internal".into(),
            account: "admin".into(),
            outputs: BTreeMap::from([("cycleId".to_string(), json!(1))]),
            environment: None,
        });
        t
    }

    fn body(extra: Value) -> String {
        let mut b = json!({ "template": template(), "account": "admin", "values": { "cycleName": "FY27" } });
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b.to_string()
    }

    pub(super) fn never_opened(_: Browser) -> FakeBrowsers {
        panic!("a browser was opened for a call that should have been refused first")
    }

    #[tokio::test]
    async fn a_proven_draft_is_saved_with_its_evidence_and_its_first_run() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let Rig { browsers, root, .. } =
            rig(vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))], None);
        v2_lib::autorun::store::set_root(root.path().to_path_buf());

        let (status, out) = api_template_prove(
            &ctx(),
            &body(json!({})),
            |b| {
                assert_eq!(b, Browser::Edge, "Edge unless the call says otherwise");
                browsers
            },
            no_db,
            &quick(),
        )
        .await;
        assert_eq!(status, 200, "{out}");
        let report: RunReport = serde_json::from_str(&out).unwrap();
        assert!(report.ok, "{report:?}");
        assert_eq!(report.outputs, BTreeMap::from([("cycleId".to_string(), json!(274))]));

        let saved = store::load(root.path(), ORG, PROJECT, ID).unwrap().expect("saved");
        let proven = saved.proven.clone().expect("carries the app's proven block");
        assert_eq!(proven.origin, "https://hr.example.internal");
        assert_eq!(proven.account, "admin");
        assert_eq!(proven.environment.as_deref(), Some("Default"), "proven in the active environment");
        assert_eq!(proven.outputs, BTreeMap::from([("cycleId".to_string(), json!(274))]));
        assert!(!proven.at.is_empty());
        assert_eq!(ApiTemplate { proven: None, ..saved }, template(), "saved as sent, plus proven");

        let listed = store::list(root.path(), ORG, PROJECT).unwrap();
        let runs = &listed[0].runs;
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert!(runs[0].ok);
        assert_eq!(runs[0].mode, "prove", "the history says this line was the prove");
        assert_eq!(runs[0].account, "admin");
        assert_eq!(runs[0].failed_step, None);
        assert_eq!(runs[0].detail, None);
        assert_eq!(runs[0].outputs, BTreeMap::from([("cycleId".to_string(), json!(274))]));
        assert_eq!(*changed.lock().unwrap(), vec![ID.to_string()], "the tab was told");
    }

    #[tokio::test]
    async fn a_failed_prove_of_a_new_template_writes_nothing() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let Rig { browsers, root, .. } = rig(
            vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(400, json!({ "success": false }))],
            None,
        );
        v2_lib::autorun::store::set_root(root.path().to_path_buf());

        let (status, out) = api_template_prove(&ctx(), &body(json!({})), |_| browsers, no_db, &quick()).await;
        assert_eq!(status, 502, "{out}");
        let report: RunReport = serde_json::from_str(&out).unwrap();
        assert_eq!(report.failed.as_deref(), Some("Evaluation rules"));
        assert_eq!(report.created, BTreeMap::from([("cycleId".to_string(), json!(274))]), "says what landed");

        assert_eq!(store::load(root.path(), ORG, PROJECT, ID).unwrap(), None);
        assert!(
            !store::templates_dir(root.path(), ORG, PROJECT).exists(),
            "no template, and no history for a template that does not exist"
        );
        assert!(changed.lock().unwrap().is_empty(), "nothing changed, nothing announced");
    }

    #[tokio::test]
    async fn proving_over_a_saved_template_needs_a_reason_and_logs_it() {
        let _log = crate::serial::log_tail();
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let _changed = changes();
        let Rig { browsers, root, .. } =
            rig(vec![answer(200, json!({ "success": true, "cycleId": 275 })), answer(200, json!({ "success": true }))], None);
        v2_lib::autorun::store::set_root(root.path().to_path_buf());
        store::save(root.path(), ORG, PROJECT, &proven_copy()).unwrap();

        let (status, out) = api_template_prove(&ctx(), &body(json!({})), never_opened, no_db, &quick()).await;
        assert_eq!(status, 400, "{out}");
        assert!(out.contains("replace: true"), "{out}");

        let why = json!({ "replace": true, "why": "the evaluation handler was renamed" });
        let (status, out) = api_template_prove(&ctx(), &body(why), |_| browsers, no_db, &quick()).await;
        assert_eq!(status, 200, "{out}");
        let saved = store::load(root.path(), ORG, PROJECT, ID).unwrap().unwrap();
        assert_eq!(saved.proven.unwrap().outputs["cycleId"], json!(275), "the new evidence replaced the old");
        let log = v2_lib::applog::recent(400);
        assert!(
            log.iter().any(|l| l.message == format!("api template {ID} replaced: the evaluation handler was renamed")),
            "the reason was not logged: {:?}",
            log.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }

    /// A failed prove over a saved template - a different draft sent with
    /// replace: true - changed nothing: the saved template is still the
    /// one proven before, so its history gets no red line from a draft
    /// that never replaced it. The assistant already has the failure.
    #[tokio::test]
    async fn a_failed_replace_prove_leaves_the_history_alone() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let Rig { browsers, root, .. } = rig(
            vec![answer(200, json!({ "success": true, "cycleId": 277 })), answer(400, json!({ "success": false }))],
            None,
        );
        v2_lib::autorun::store::set_root(root.path().to_path_buf());
        store::save(root.path(), ORG, PROJECT, &proven_copy()).unwrap();

        let why = json!({ "replace": true, "why": "trying a new handler" });
        let (status, out) = api_template_prove(&ctx(), &body(why), |_| browsers, no_db, &quick()).await;
        assert_eq!(status, 502, "{out}");
        let report: RunReport = serde_json::from_str(&out).unwrap();
        assert_eq!(report.failed.as_deref(), Some("Evaluation rules"));

        assert_eq!(store::load(root.path(), ORG, PROJECT, ID).unwrap(), Some(proven_copy()), "untouched");
        let runs = &store::list(root.path(), ORG, PROJECT).unwrap()[0].runs;
        assert!(runs.is_empty(), "a failed prove reached the saved template's history: {runs:?}");
        assert!(changed.lock().unwrap().is_empty(), "nothing changed, nothing announced");
    }

    /// The reason for a replace is one line in the app log, and a short
    /// one, whatever the assistant sent.
    #[tokio::test]
    async fn a_replace_reason_is_logged_on_one_short_line() {
        let _log = crate::serial::log_tail();
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let _changed = changes();
        let Rig { browsers, root, .. } =
            rig(vec![answer(200, json!({ "success": true, "cycleId": 278 })), answer(200, json!({ "success": true }))], None);
        v2_lib::autorun::store::set_root(root.path().to_path_buf());
        store::save(root.path(), ORG, PROJECT, &proven_copy()).unwrap();

        let long = format!("the handler\n\n   was renamed\r\n{}", "x".repeat(400));
        let why = json!({ "replace": true, "why": long });
        let (status, out) = api_template_prove(&ctx(), &body(why), |_| browsers, no_db, &quick()).await;
        assert_eq!(status, 200, "{out}");
        let prefix = format!("api template {ID} replaced: ");
        let log = v2_lib::applog::recent(400);
        let line = log
            .iter()
            .map(|l| l.message.clone())
            .find(|m| m.starts_with(&prefix))
            .unwrap_or_else(|| panic!("no replace line in {:?}", log.iter().map(|l| &l.message).collect::<Vec<_>>()));
        let reason = &line[prefix.len()..];
        assert!(reason.starts_with("the handler was renamed xxx"), "{reason}");
        assert!(!reason.contains('\n') && !reason.contains('\r'), "{reason:?}");
        assert_eq!(reason.chars().count(), 200, "{reason}");
    }

    #[tokio::test]
    async fn a_run_appends_to_the_history_and_leaves_the_template_alone() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let Rig { browsers, root, .. } = rig(
            vec![answer(200, json!({ "success": true, "cycleId": 276 })), answer(400, json!({ "success": false }))],
            None,
        );
        v2_lib::autorun::store::set_root(root.path().to_path_buf());
        store::save(root.path(), ORG, PROJECT, &proven_copy()).unwrap();

        let body = json!({ "id": ID, "account": "admin", "values": { "cycleName": "FY27" }, "browser": "chrome" });
        let (status, out) = api_template_run(
            &ctx(),
            &body.to_string(),
            |b| {
                assert_eq!(b, Browser::Chrome);
                browsers
            },
            no_db,
            &quick(),
        )
        .await;
        assert_eq!(status, 502, "{out}");

        assert_eq!(store::load(root.path(), ORG, PROJECT, ID).unwrap(), Some(proven_copy()), "untouched");
        let runs = &store::list(root.path(), ORG, PROJECT).unwrap()[0].runs;
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert!(!runs[0].ok);
        assert_eq!(runs[0].mode, "run");
        assert_eq!(runs[0].failed_step.as_deref(), Some("Evaluation rules"));
        let detail = runs[0].detail.clone().expect("a failed run says why");
        assert!(detail.contains("cycleId 276 created") && detail.contains("Evaluation rules"), "{detail}");
        assert_eq!(runs[0].outputs, BTreeMap::from([("cycleId".to_string(), json!(276))]), "what it had created");
        assert_eq!(*changed.lock().unwrap(), vec![ID.to_string()]);
    }
}

/// Templates on a flow, through the bridge - design doc "API template
/// flows" §5 and §6: a template is refused before any browser opens until
/// the stages before its own are done, a prove that did not complete its
/// own stage is not saved, and a saved flow tells the tab. The database is
/// `FakeStageDb`, handed in the way `route` hands in the real one.
mod flows_through_the_bridge {
    use super::through_the_bridge::{changes, never_opened, no_db, proven_copy};
    use super::*;
    use crate::common::{cycle_flow_json, saved_on_stage, template_on_stage, FakeStageDb};
    use v2_lib::ai_bridge::{api_template_flow_save, api_template_prove, api_template_run, real_stage_db, BridgeContext};
    use v2_lib::api_templates::flow::Flow;
    use v2_lib::api_templates::{flow_store, store};

    const FLOW: &str = "pms-performance-cycle";
    const PARTICIPANTS: &str = "pms-add-participants";

    fn ctx() -> BridgeContext {
        BridgeContext { org: ORG.into(), project: PROJECT.into(), api_writes: true, ..BridgeContext::default() }
    }

    fn flow() -> Flow {
        serde_json::from_value(cycle_flow_json()).unwrap()
    }

    /// The rig's root as the bridge's, with the flow saved in it and the
    /// templates on Evaluation rules and Participants.
    fn with_flow(r: &Rig) {
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        flow_store::save(r.root.path(), ORG, PROJECT, &flow()).unwrap();
        store::save(r.root.path(), ORG, PROJECT, &saved_on_stage("pms-set-eval-rules", "Set the evaluation rules", "rules"))
            .unwrap();
        store::save(r.root.path(), ORG, PROJECT, &saved_on_stage(PARTICIPANTS, "Add the participants", "participants"))
            .unwrap();
    }

    fn run_body(id: &str, values: Value) -> String {
        json!({ "id": id, "account": "admin", "values": values }).to_string()
    }

    /// Which stages were asked, by the `/*stage-id*/` marker on each check.
    fn asked(db: &FakeStageDb) -> Vec<String> {
        db.calls()
            .iter()
            .map(|sql| {
                let start = sql.rfind("/*").expect("marker") + 2;
                sql[start..sql.rfind("*/").unwrap()].to_string()
            })
            .collect()
    }

    fn handed(db: &FakeStageDb) -> impl FnOnce(&BridgeContext) -> Result<FakeStageDb, (u16, String)> {
        let db = db.clone();
        move |_| Ok(db)
    }

    #[tokio::test]
    async fn a_template_on_a_flow_is_refused_before_any_browser_opens() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let r = rig(vec![], None);
        with_flow(&r);
        let db = FakeStageDb::new().answer("/*setup*/", Ok(true)).answer("/*rules*/", Ok(false));

        let body = run_body(PARTICIPANTS, json!({ "cycleId": 274 }));
        let (status, out) = api_template_run(&ctx(), &body, never_opened, handed(&db), &quick()).await;
        assert_eq!(status, 400, "{out}");
        assert_eq!(
            out,
            "Evaluation rules is not done for cycleId 274 - do it first with pms-set-eval-rules (Set the evaluation rules)."
        );
        assert_eq!(asked(&db), ["setup", "rules"], "every earlier stage, in flow order");
        assert!(claim().is_some(), "refused before the one-at-a-time slot was taken");
    }

    #[tokio::test]
    async fn a_run_whose_earlier_stages_are_done_goes_ahead() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let r = rig(vec![answer(200, json!({ "success": true }))], None);
        with_flow(&r);
        let db = FakeStageDb::new().answer("/*setup*/", Ok(true)).answer("/*rules*/", Ok(true));

        let body = run_body(PARTICIPANTS, json!({ "cycleId": 274 }));
        let (status, out) = api_template_run(&ctx(), &body, |_| r.browsers, handed(&db), &quick()).await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(asked(&db), ["setup", "rules"], "a run does not check its own stage afterwards");
        let fetched = r.script.lock().unwrap().fetched.clone();
        assert_eq!(fetched[0][0]["body"], json!({ "kind": "form", "fields": { "CycleId": "274" } }));
    }

    /// The subject param is not forced `required`, so a call without it
    /// is refused by name before the database or a browser is touched.
    #[tokio::test]
    async fn a_flow_template_without_its_subject_value_is_refused() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let r = rig(vec![], None);
        with_flow(&r);

        let (status, out) =
            api_template_run(&ctx(), &run_body(PARTICIPANTS, json!({})), never_opened, no_db, &quick()).await;
        assert_eq!(status, 400, "{out}");
        assert_eq!(out, "this template belongs to flow pms-performance-cycle, so it needs \"cycleId\" in values");
    }

    /// A number subject sent as a string is refused naming the subject and
    /// its type - never quoted into a check.
    #[tokio::test]
    async fn a_subject_of_the_wrong_type_is_refused_before_any_check() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let r = rig(vec![], None);
        with_flow(&r);
        let db = FakeStageDb::new();

        let body = run_body(PARTICIPANTS, json!({ "cycleId": "274" }));
        let (status, out) = api_template_run(&ctx(), &body, never_opened, handed(&db), &quick()).await;
        // The template's own param check says it first, naming the subject
        // and its type - the value is never quoted into a check.
        assert_eq!((status, out.as_str()), (400, "param 'cycleId' must be a number"));
        assert!(db.calls().is_empty(), "{:?}", db.calls());
    }

    #[tokio::test]
    async fn a_template_without_a_stage_never_asks_for_a_database() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let r = rig(
            vec![answer(200, json!({ "success": true, "cycleId": 279 })), answer(200, json!({ "success": true }))],
            None,
        );
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        store::save(r.root.path(), ORG, PROJECT, &proven_copy()).unwrap();

        // No database is chosen in this context, so the real one would
        // refuse - and it is never asked.
        assert!(ctx().db_id.is_none());
        let body = run_body("pms-create-draft-cycle", json!({ "cycleName": "FY27" }));
        let (status, out) = api_template_run(&ctx(), &body, |_| r.browsers, real_stage_db, &quick()).await;
        assert_eq!(status, 200, "{out}");
    }

    #[tokio::test]
    async fn a_template_whose_flow_is_gone_is_refused() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let r = rig(vec![], None);
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        store::save(r.root.path(), ORG, PROJECT, &saved_on_stage(PARTICIPANTS, "Add the participants", "participants"))
            .unwrap();
        let body = run_body(PARTICIPANTS, json!({ "cycleId": 274 }));
        let gone = "this template's flow pms-performance-cycle is no longer saved";

        let (status, out) = api_template_run(&ctx(), &body, never_opened, no_db, &quick()).await;
        assert_eq!((status, out.as_str()), (400, gone));

        // A flow file that no longer parses is the same - not a 500.
        let dir = flow_store::flows_dir(r.root.path(), ORG, PROJECT);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{FLOW}.json")), "{ this is not a flow").unwrap();
        let (status, out) = api_template_run(&ctx(), &body, never_opened, no_db, &quick()).await;
        assert_eq!((status, out.as_str()), (400, gone));

        // And a stage the saved flow no longer has.
        flow_store::save(r.root.path(), ORG, PROJECT, &flow()).unwrap();
        store::save(r.root.path(), ORG, PROJECT, &saved_on_stage(PARTICIPANTS, "Add the participants", "reviews"))
            .unwrap();
        let (status, out) = api_template_run(&ctx(), &body, never_opened, no_db, &quick()).await;
        assert_eq!((status, out.as_str()), (400, "stage \"reviews\" is no longer in flow pms-performance-cycle"));
    }

    #[tokio::test]
    async fn a_prove_that_does_not_complete_its_stage_is_not_saved() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let r = rig(vec![answer(200, json!({ "success": true }))], None);
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        flow_store::save(r.root.path(), ORG, PROJECT, &flow()).unwrap();
        let db = FakeStageDb::new()
            .answer("/*setup*/", Ok(true))
            .answer("/*rules*/", Ok(true))
            .answer("/*participants*/", Ok(false));

        let body = json!({
            "template": template_on_stage(PARTICIPANTS, "Add the participants", "participants"),
            "account": "admin",
            "values": { "cycleId": 274 },
        })
        .to_string();
        let (status, out) = api_template_prove(&ctx(), &body, |_| r.browsers, handed(&db), &quick()).await;
        assert_eq!(status, 502, "{out}");
        assert!(out.contains("is still not done for cycleId"), "{out}");
        assert!(
            out.starts_with(
                "every step passed, but Participants is still not done for cycleId 274, so the template was not saved; every step passed (1 steps)"
            ),
            "{out}"
        );
        assert_eq!(asked(&db), ["setup", "rules", "participants"], "the gate, then its own stage");
        assert_eq!(store::load(r.root.path(), ORG, PROJECT, PARTICIPANTS).unwrap(), None);
        assert!(changed.lock().unwrap().is_empty(), "nothing saved, nothing announced");
    }

    #[tokio::test]
    async fn a_creating_prove_checks_the_captured_subject() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let r = rig(
            vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
            None,
        );
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        flow_store::save(r.root.path(), ORG, PROJECT, &flow()).unwrap();
        let db = FakeStageDb::new().answer("/*setup*/", Ok(true));

        let mut creating = serde_json::to_value(template()).unwrap();
        creating["stage"] = json!({ "flow": FLOW, "id": "setup" });
        let body = json!({ "template": creating, "account": "admin", "values": { "cycleName": "FY27" } }).to_string();
        let (status, out) = api_template_prove(&ctx(), &body, |_| r.browsers, handed(&db), &quick()).await;
        assert_eq!(status, 200, "{out}");
        let calls = db.calls();
        assert_eq!(calls.len(), 1, "no gate for the creating stage, then its own check: {calls:?}");
        assert!(calls[0].contains("/*setup*/") && calls[0].contains("274"), "{}", calls[0]);
        let saved = store::load(r.root.path(), ORG, PROJECT, "pms-create-draft-cycle").unwrap().expect("saved");
        assert_eq!(saved.stage.map(|s| s.id), Some("setup".to_string()));
        assert_eq!(*changed.lock().unwrap(), vec!["pms-create-draft-cycle".to_string()]);
    }

    /// Its own check could not run (a database error): never read as "not
    /// done" - nothing saved, nothing announced, and the answer says the
    /// check could not be run (design doc §5, Review Focus 1).
    #[tokio::test]
    async fn a_prove_whose_own_check_could_not_run_is_not_saved_and_says_so() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let r = rig(vec![answer(200, json!({ "success": true }))], None);
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        flow_store::save(r.root.path(), ORG, PROJECT, &flow()).unwrap();
        let db = FakeStageDb::new()
            .answer("/*setup*/", Ok(true))
            .answer("/*rules*/", Ok(true))
            .answer("/*participants*/", Err("Login timeout expired on SQLPROD01".to_string()));

        let body = json!({
            "template": template_on_stage(PARTICIPANTS, "Add the participants", "participants"),
            "account": "admin",
            "values": { "cycleId": 274 },
        })
        .to_string();
        let (status, out) = api_template_prove(&ctx(), &body, |_| r.browsers, handed(&db), &quick()).await;
        assert_eq!(status, 502, "{out}");
        assert_eq!(
            out,
            "every step passed, but the check for Participants could not be run - see the activity folder in Settings, Logs, so the template was not saved; every step passed (1 steps); nothing had been captured yet"
        );
        assert_eq!(asked(&db), ["setup", "rules", "participants"]);
        assert_eq!(store::load(r.root.path(), ORG, PROJECT, PARTICIPANTS).unwrap(), None);
        assert!(changed.lock().unwrap().is_empty(), "nothing saved, nothing announced");
    }

    /// The creating step captured the record id as a string, but the
    /// flow's subject is a number: the answer says the capture has the wrong
    /// type - never quoted into the SQL, never "not done", and never "could
    /// not be run", which would send the assistant to retry a prove that
    /// creates another record each time.
    #[tokio::test]
    async fn a_creating_prove_whose_capture_has_the_wrong_type_is_not_saved() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let r = rig(
            vec![answer(200, json!({ "success": true, "cycleId": "274" })), answer(200, json!({ "success": true }))],
            None,
        );
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        flow_store::save(r.root.path(), ORG, PROJECT, &flow()).unwrap();
        let db = FakeStageDb::new().answer("/*setup*/", Ok(true));

        let mut creating = serde_json::to_value(template()).unwrap();
        creating["stage"] = json!({ "flow": FLOW, "id": "setup" });
        let body = json!({ "template": creating, "account": "admin", "values": { "cycleName": "FY27" } }).to_string();
        let (status, out) = api_template_prove(&ctx(), &body, |_| r.browsers, handed(&db), &quick()).await;
        assert_eq!(status, 502, "{out}");
        assert_eq!(
            out,
            "every step passed, but the captured cycleId is not a number (cycleId is a number subject: give a whole number, 0 or more), so the template was not saved; every step passed (2 steps); cycleId 274 created"
        );
        assert!(!out.contains("could not be run"), "{out}");
        assert!(db.calls().is_empty(), "nothing reached the database: {:?}", db.calls());
        assert_eq!(store::load(r.root.path(), ORG, PROJECT, "pms-create-draft-cycle").unwrap(), None);
        assert!(changed.lock().unwrap().is_empty(), "nothing saved, nothing announced");
    }

    /// The flow a prove's gate passed on, with `participants` taken out -
    /// what a `save_api_flow` replacing it during the run leaves behind.
    fn flow_without_participants() -> Flow {
        let mut f = flow();
        f.stages.retain(|s| s.id != "participants");
        for s in &mut f.stages {
            if s.id == "publish" {
                s.requires = vec!["rules".to_string()];
            }
        }
        f
    }

    /// The flow is read again once the steps have passed: a stage it no
    /// longer has is not saved, and its own check is never asked.
    #[tokio::test]
    async fn a_prove_whose_stage_left_the_flow_during_the_run_is_not_saved() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let r = rig(vec![answer(200, json!({ "success": true }))], None);
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        flow_store::save(r.root.path(), ORG, PROJECT, &flow()).unwrap();
        let db = FakeStageDb::new().answer("/*setup*/", Ok(true)).answer("/*rules*/", Ok(true));

        let body = json!({
            "template": template_on_stage(PARTICIPANTS, "Add the participants", "participants"),
            "account": "admin",
            "values": { "cycleId": 274 },
        })
        .to_string();
        let root = r.root.path().to_path_buf();
        let browsers = r.browsers;
        let open = move |_| {
            flow_store::save(&root, ORG, PROJECT, &flow_without_participants()).unwrap();
            browsers
        };
        let (status, out) = api_template_prove(&ctx(), &body, open, handed(&db), &quick()).await;
        assert_eq!(status, 502, "{out}");
        assert_eq!(
            out,
            "every step passed, but stage \"participants\" is no longer in flow pms-performance-cycle, so the template was not saved; every step passed (1 steps); nothing had been captured yet"
        );
        assert_eq!(asked(&db), ["setup", "rules"], "the gate only - no check for a stage that is gone");
        assert_eq!(store::load(r.root.path(), ORG, PROJECT, PARTICIPANTS).unwrap(), None);
        assert!(changed.lock().unwrap().is_empty(), "nothing saved, nothing announced");
    }

    /// The same when the whole flow was removed during the run.
    #[tokio::test]
    async fn a_prove_whose_flow_was_removed_during_the_run_is_not_saved() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let r = rig(vec![answer(200, json!({ "success": true }))], None);
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        flow_store::save(r.root.path(), ORG, PROJECT, &flow()).unwrap();
        let db = FakeStageDb::new().answer("/*setup*/", Ok(true)).answer("/*rules*/", Ok(true));

        let body = json!({
            "template": template_on_stage(PARTICIPANTS, "Add the participants", "participants"),
            "account": "admin",
            "values": { "cycleId": 274 },
        })
        .to_string();
        let root = r.root.path().to_path_buf();
        let browsers = r.browsers;
        let open = move |_| {
            flow_store::remove(&root, ORG, PROJECT, FLOW).unwrap();
            browsers
        };
        let (status, out) = api_template_prove(&ctx(), &body, open, handed(&db), &quick()).await;
        assert_eq!(status, 502, "{out}");
        assert_eq!(
            out,
            "every step passed, but this template's flow pms-performance-cycle is no longer saved, so the template was not saved; every step passed (1 steps); nothing had been captured yet"
        );
        assert_eq!(asked(&db), ["setup", "rules"]);
        assert_eq!(store::load(r.root.path(), ORG, PROJECT, PARTICIPANTS).unwrap(), None);
        assert!(changed.lock().unwrap().is_empty(), "nothing saved, nothing announced");
    }

    /// Running the creating stage's template has nothing to gate and
    /// nothing to check afterwards, so it asks nothing of the database.
    #[tokio::test]
    async fn a_creating_run_asks_nothing_of_the_database() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let r = rig(
            vec![answer(200, json!({ "success": true, "cycleId": 280 })), answer(200, json!({ "success": true }))],
            None,
        );
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        flow_store::save(r.root.path(), ORG, PROJECT, &flow()).unwrap();
        let mut t = proven_copy();
        t.stage = Some(v2_lib::api_templates::flow::StageRef { flow: FLOW.into(), id: "setup".into() });
        store::save(r.root.path(), ORG, PROJECT, &t).unwrap();

        let body = run_body("pms-create-draft-cycle", json!({ "cycleName": "FY27" }));
        let (status, out) = api_template_run(&ctx(), &body, |_| r.browsers, no_db, &quick()).await;
        assert_eq!(status, 200, "{out}");
    }

    #[tokio::test]
    async fn saving_a_flow_tells_the_tab() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let changed = changes();
        let r = rig(vec![], None);
        v2_lib::autorun::store::set_root(r.root.path().to_path_buf());
        let db = ["setup", "rules", "competencies", "participants", "publish"]
            .iter()
            .fold(FakeStageDb::new(), |db, id| db.answer(&format!("/*{id}*/"), Ok(true)));

        let body = json!({ "flow": cycle_flow_json(), "sample": 274 }).to_string();
        let (status, out) = api_template_flow_save(&ctx(), &body, handed(&db)).await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(*changed.lock().unwrap(), vec![FLOW.to_string()], "the tab was told");
    }
}

/// The hosted failure, handled: the application keeps its anti-forgery
/// cookie on `/hr/pmsv10`, the template calls `/hr/PMSV10/...`, and cookie
/// paths are case-sensitive - sent as written, the save would go without the
/// cookie and come back an empty 400. The server's routing does not care
/// about case, so the step is sent in the cookie's letter case instead, and
/// the activity record says so. One template then runs against a local IIS
/// that keeps the cookie on `/hr/PMSV10` and a hosted one that keeps it on
/// `/hr/pmsv10`.
#[tokio::test]
async fn a_step_that_would_miss_a_cookie_by_letter_case_is_sent_in_the_cookies_case() {
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().all_cookies = Some(json!([
        { "name": ".AspNetCore.Antiforgery.Ab1", "value": "JAR-SECRET-1", "domain": "hr.example.internal",
          "path": "/hr/pmsv10", "httpOnly": true, "secure": true, "session": true },
        { "name": "sid", "value": "JAR-SECRET-2", "domain": "hr.example.internal", "path": "/", "session": true }
    ]));
    let mut t = template();
    for step in &mut t.steps {
        step.path = "/hr/PMSV10/PerformanceCycle".into();
    }
    let report = run(&mut r, t).await;
    assert!(report.ok, "{report:?}");

    let fetched = r.fetched();
    assert_eq!(fetched.len(), 2);
    assert_eq!(fetched[0][0]["url"], "/hr/pmsv10/PerformanceCycle?handler=SaveProgress");
    assert_eq!(fetched[1][0]["url"], "/hr/pmsv10/PerformanceCycle?handler=SaveEvalRulesProgress");

    let records = activity_records(activity.path(), "api");
    let steps = step_records(&records);
    assert_eq!(steps.len(), 2, "{records:?}");
    assert_eq!(steps[0]["url"], "/hr/pmsv10/PerformanceCycle?handler=SaveProgress");
    assert_eq!(
        steps[0]["path_case_adapted"],
        json!({ "from": "/hr/PMSV10/PerformanceCycle", "to": "/hr/pmsv10/PerformanceCycle",
                "cookie": ".AspNetCore.Antiforgery.Ab1" })
    );
    for rec in &records {
        assert!(!rec.to_string().contains("JAR-SECRET"), "a cookie value reached an activity record: {rec}");
    }
    let log = v2_lib::applog::recent(400);
    for line in &log {
        assert!(!line.message.contains("JAR-SECRET"), "a cookie value reached the app log: {}", line.message);
    }
    assert!(
        log.iter().any(|l| l.message.contains("/hr/PMSV10/PerformanceCycle") && l.message.contains("/hr/pmsv10/PerformanceCycle")),
        "the adaptation was not logged"
    );
}

/// A jar holding the step's cookie in BOTH casings: adapting would gain one
/// and lose the other, so the path goes as written, unadapted.
#[tokio::test]
async fn a_path_that_already_carries_a_cookie_is_not_adapted_away_from_it() {
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().all_cookies = Some(json!([
        { "name": "upper", "value": "v", "domain": "hr.example.internal", "path": "/hr/PMSV10", "session": true },
        { "name": "lower", "value": "v", "domain": "hr.example.internal", "path": "/hr/pmsv10", "session": true }
    ]));
    let mut t = template();
    for step in &mut t.steps {
        step.path = "/hr/PMSV10/PerformanceCycle".into();
    }
    let report = run(&mut r, t).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.fetched()[0][0]["url"], "/hr/PMSV10/PerformanceCycle?handler=SaveProgress");
    for step in step_records(&activity_records(activity.path(), "api")) {
        assert_eq!(step["path_case_adapted"], Value::Null, "{step}");
    }
}

/// A browser that will not list its cookies leaves every path as written:
/// the adaptation can prevent a failure, never cause one.
#[tokio::test]
async fn a_browser_that_will_not_list_its_cookies_sends_paths_as_written() {
    let _log = crate::serial::log_tail();
    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().all_cookies_fail = true;
    let mut t = template();
    for step in &mut t.steps {
        step.path = "/hr/PMSV10/PerformanceCycle".into();
    }
    let report = run(&mut r, t).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.fetched()[0][0]["url"], "/hr/PMSV10/PerformanceCycle?handler=SaveProgress");
    assert!(
        v2_lib::applog::recent(400).iter().any(|l| l.message.contains("listing the cookies")),
        "the browser's refusal to list cookies was not logged"
    );
}

/// When the paths already match the cookies' letter case, they are sent as
/// written and nothing is recorded as adapted.
#[tokio::test]
async fn paths_in_the_cookies_letter_case_are_sent_as_written() {
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().all_cookies = Some(json!([
        { "name": ".AspNetCore.Antiforgery.Ab1", "value": "v", "domain": "hr.example.internal", "path": "/hr/pmsv10", "session": true }
    ]));
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.fetched()[0][0]["url"], "/hr/pmsv10/performancecycle?handler=SaveProgress");
    let records = activity_records(activity.path(), "api");
    for step in step_records(&records) {
        assert_eq!(step["path_case_adapted"], Value::Null, "{step}");
    }
}

/// The token page record says how the run signed in - from a saved session
/// or through the recipe, and which optional recipe steps' elements showed
/// up (PeoplesHR's "Continue here", which logs the account out wherever
/// else it is signed in). A session the application ended mid-way shows as
/// a second sign-in.
#[tokio::test]
async fn the_token_page_record_says_how_the_run_signed_in() {
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().hrefs.push_back(LOGIN.to_string());
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");

    let records = activity_records(activity.path(), "api");
    let page = records.iter().find(|rec| rec["event"] == "token_page").expect("no token page record");
    assert_eq!(
        page["sign_ins"],
        json!([
            { "via": "sign-in recipe", "appeared": [] },
            { "via": "sign-in recipe", "appeared": [] }
        ]),
        "{page}"
    );
}

/// A jar as the browser reports it: two cookies, each with a value that
/// must never reach a record.
fn jar() -> Value {
    json!([
        { "name": ".AspNetCore.Antiforgery.Ab1", "value": "JAR-SECRET-1", "domain": "hr.example.internal",
          "path": "/hr/PMSV10", "httpOnly": true, "secure": true, "sameSite": "Strict", "session": true,
          "size": 190, "expires": -1 },
        { "name": "ehrm85", "value": "JAR-SECRET-2", "domain": ".example.internal",
          "path": "/", "httpOnly": true, "secure": true, "session": false, "expires": 1_900_000_000.0 }
    ])
}

/// Diagnosing a rejected save: what the token page was and what it held,
/// and which cookies each step's address would carry - names, paths and
/// flags only, never a value and never the token.
#[tokio::test]
async fn the_token_page_and_each_step_record_cookie_names_but_never_values() {
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().cookies = Some(jar());
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");

    let records = activity_records(activity.path(), "api");
    let page = records.iter().find(|rec| rec["event"] == "token_page").expect("no token page record");
    assert_eq!(page["template"], "pms-create-draft-cycle");
    assert_eq!(page["mode"], "prove");
    assert_eq!(page["account"], "admin");
    assert_eq!(page["origin"], "https://hr.example.internal");
    assert_eq!(page["requested"], PAGE);
    assert_eq!(page["final_url"], format!("https://hr.example.internal{PAGE}"));
    assert_eq!(page["token_found"], true);
    assert_eq!(page["token_length"], TOKEN.len());
    assert_eq!(
        page["cookies"],
        json!([
            { "name": ".AspNetCore.Antiforgery.Ab1", "domain": "hr.example.internal", "path": "/hr/PMSV10",
              "http_only": true, "secure": true, "same_site": "Strict", "session": true },
            { "name": "ehrm85", "domain": ".example.internal", "path": "/",
              "http_only": true, "secure": true, "same_site": null, "session": false }
        ])
    );

    let steps = step_records(&records);
    assert_eq!(steps.len(), 2, "{records:?}");
    for step in &steps {
        assert_eq!(step["cookies_sent"], json!([".AspNetCore.Antiforgery.Ab1", "ehrm85"]), "{step}");
    }
    // The jar was asked about the page itself, then each step's own address.
    let asked = r.script.lock().unwrap().cookie_urls.clone();
    assert_eq!(
        asked,
        vec![
            json!([format!("https://hr.example.internal{PAGE}")]),
            json!(["https://hr.example.internal/hr/pmsv10/performancecycle?handler=SaveProgress"]),
            json!(["https://hr.example.internal/hr/pmsv10/performancecycle?handler=SaveEvalRulesProgress"]),
        ]
    );

    for rec in &records {
        let text = rec.to_string();
        for secret in ["JAR-SECRET-1", "JAR-SECRET-2", TOKEN] {
            assert!(!text.contains(secret), "{secret} reached an activity record: {text}");
        }
    }
    for line in v2_lib::applog::recent(400) {
        for secret in ["JAR-SECRET-1", "JAR-SECRET-2", TOKEN] {
            assert!(!line.message.contains(secret), "{secret} reached the app log: {}", line.message);
        }
    }
}

/// A page with no token is recorded too - that is exactly the case the
/// record is for.
#[tokio::test]
async fn a_token_page_without_a_token_is_still_recorded() {
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(vec![], None);
    {
        let mut s = r.script.lock().unwrap();
        s.token = None;
        s.cookies = Some(jar());
    }
    let report = run(&mut r, template()).await;
    assert!(!report.ok);

    let records = activity_records(activity.path(), "api");
    let page = records.iter().find(|rec| rec["event"] == "token_page").expect("no token page record");
    assert_eq!(page["token_found"], false);
    assert_eq!(page["token_length"], Value::Null);
    assert_eq!(page["cookies"].as_array().map(Vec::len), Some(2), "{page}");
    assert!(step_records(&records).is_empty(), "no step was sent: {records:?}");
}

/// Reading the jar is a diagnostic: when the browser will not say, the run
/// goes on and the records say the cookies are unknown.
#[tokio::test]
async fn cookies_that_cannot_be_read_do_not_stop_the_run() {
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    r.script.lock().unwrap().cookies_fail = true;
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");

    let records = activity_records(activity.path(), "api");
    let page = records.iter().find(|rec| rec["event"] == "token_page").expect("no token page record");
    assert_eq!(page["cookies"], Value::Null, "{page}");
    for step in step_records(&records) {
        assert_eq!(step["cookies_sent"], Value::Null, "{step}");
    }
}

/// An empty 400 means the application refused the request before reading
/// it, so nothing was saved and trying again is safe. The step is sent
/// again with a fresh token, the run goes on, and each attempt has its own
/// activity record.
#[tokio::test]
async fn an_empty_400_is_retried_with_a_fresh_token() {
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(
        vec![empty_400(), answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(report.outputs, BTreeMap::from([("cycleId".to_string(), json!(274))]));
    assert_eq!(report.steps.len(), 2, "one report line per step, retry or not: {:?}", report.steps);
    assert_eq!(r.fetched().len(), 3, "step 1 twice, step 2 once");
    assert_eq!(r.navigations_to_the_page(), 2, "a fresh token was read before the retry");

    let records = activity_records(activity.path(), "api");
    let steps = step_records(&records);
    assert_eq!(steps.len(), 3, "{records:?}");
    assert_eq!((steps[0]["step"].as_str(), steps[0]["status"].as_u64(), steps[0]["attempt"].as_u64()), (Some("Cycle setup"), Some(400), Some(1)));
    assert_eq!((steps[1]["step"].as_str(), steps[1]["status"].as_u64(), steps[1]["attempt"].as_u64()), (Some("Cycle setup"), Some(200), Some(2)));
    assert_eq!(steps[2]["attempt"].as_u64(), Some(1));
    assert!(
        v2_lib::applog::recent(400).iter().any(|l| l.message.contains("Cycle setup") && l.message.contains("empty 400")),
        "the retry was not logged"
    );
}

/// Hosted PeoplesHR refuses writes unread independently of the try before,
/// so a step refused three times running can still go through on the
/// fourth - each try reading its own fresh token.
#[tokio::test]
async fn a_step_refused_unread_three_times_goes_through_on_the_fourth_try() {
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let mut r = rig(
        vec![
            empty_400(),
            empty_400(),
            empty_400(),
            answer(200, json!({ "success": true, "cycleId": 274 })),
            answer(200, json!({ "success": true })),
        ],
        None,
    );
    let report = run(&mut r, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(report.outputs, BTreeMap::from([("cycleId".to_string(), json!(274))]));
    assert_eq!(r.fetched().len(), 5, "step 1 four times, step 2 once");
    assert_eq!(r.navigations_to_the_page(), 4, "a fresh token before every retry");

    let records = activity_records(activity.path(), "api");
    let attempts: Vec<_> = step_records(&records).iter().map(|s| (s["status"].as_u64(), s["attempt"].as_u64())).collect();
    assert_eq!(
        attempts,
        vec![(Some(400), Some(1)), (Some(400), Some(2)), (Some(400), Some(3)), (Some(200), Some(4)), (Some(200), Some(1))]
    );
}

/// Refused unread on every try: the step fails, and says so in words a
/// person can act on.
#[tokio::test]
async fn an_empty_400_on_every_try_fails_the_step() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![empty_400(), empty_400(), empty_400(), empty_400()], None);
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("Cycle setup"));
    let detail = &report.steps.last().unwrap().detail;
    assert!(detail.contains("empty 400") && detail.contains("all 4 tries"), "{detail}");
    assert_eq!(r.fetched().len(), 4, "three retries, no more");
    assert_eq!(r.browsers.closed, 1);
}

/// The real pauses: three retries, spaced further apart each time.
#[test]
fn the_retry_pauses_are_1_3_and_5_seconds() {
    assert_eq!(RETRY_PAUSES.map(|p| p.as_secs()), [1, 3, 5]);
}

/// A 400 WITH a body was read and answered by the application: it is a real
/// refusal, and sending it again would only repeat it.
#[tokio::test]
async fn a_400_with_a_body_is_not_retried() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![answer(400, json!({ "errors": ["The cycle name is required."] }))], None);
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    assert_eq!(r.fetched().len(), 1);
    let detail = &report.steps.last().unwrap().detail;
    assert!(detail.contains("expected status 200, got 400") && detail.contains("cycle name is required"), "{detail}");
}

/// A step that expects a 400 gets one, empty or not: nothing to retry.
#[tokio::test]
async fn a_step_that_expects_a_400_is_not_retried() {
    let _act = crate::serial::activity_log();
    let mut t = template();
    t.steps.truncate(1);
    t.steps[0].expect.status = 400;
    t.steps[0].expect.json = None;
    t.steps[0].capture.clear();
    t.outputs.clear();
    let mut r = rig(vec![empty_400()], None);
    let report = run(&mut r, t).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.fetched().len(), 1);
}

mod test_files {
    //! A form step that uploads a test file: what reaches the page, and
    //! that the file's bytes reach nothing else.

    use super::*;
    use base64::Engine;

    const PDF: &[u8] = b"%PDF-1.7 a test document";

    /// `template()` with its first step also sending `appraisal.pdf` as
    /// `Document`.
    fn uploading() -> ApiTemplate {
        let mut t = template();
        t.steps[0].files = BTreeMap::from([("Document".to_string(), "appraisal.pdf".to_string())]);
        t
    }

    fn put_test_file(r: &Rig) -> String {
        let dir = v2_lib::test_files::folder(r.root.path(), ORG, PROJECT);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("appraisal.pdf"), PDF).unwrap();
        base64::engine::general_purpose::STANDARD.encode(PDF)
    }

    #[tokio::test]
    async fn the_page_gets_the_file_and_nothing_else_gets_its_bytes() {
        let _log = crate::serial::log_tail();
        let _act = crate::serial::activity_log();
        let activity = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(activity.path().to_path_buf());

        let mut r = rig(
            vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
            None,
        );
        let b64 = put_test_file(&r);
        let report = run(&mut r, uploading()).await;
        assert!(report.ok, "{report:?}");

        let fetched = r.fetched();
        assert_eq!(
            fetched[0][0]["body"],
            json!({ "kind": "form", "fields": { "CycleName": "FY27" },
                    "files": [{ "field": "Document", "name": "appraisal.pdf", "contentType": "application/pdf",
                                "size": PDF.len(), "base64": b64 }] })
        );
        assert_eq!(fetched[1][0]["body"], json!({ "kind": "form", "fields": { "CycleId": "274" } }));
        assert!(FETCH_FN.contains("new Blob([Uint8Array.from(atob(f.base64)"), "{FETCH_FN}");
        // The step that sends a file has the longer limit, in the page too.
        assert_eq!(fetched[0][2], json!(120_000));
        assert_eq!(fetched[1][2], json!(30_000));
        assert!(FETCH_FN.contains("setTimeout(() => ctrl.abort(), limitMs || 30000)"), "{FETCH_FN}");

        assert_eq!(report.steps[0].detail, format!("status 200, sent \"appraisal.pdf\" ({} bytes), captured cycleId", PDF.len()));
        let text = serde_json::to_string(&report).unwrap();
        assert!(!text.contains(&b64), "the bytes reached the report: {text}");
        assert!(!report.message().contains(&b64));

        let records = activity_records(activity.path(), "api");
        for rec in &records {
            assert!(!rec.to_string().contains(&b64), "the bytes reached an activity record: {rec}");
        }
        let first = step_records(&records)[0];
        assert!(
            first["request"].as_str().unwrap().contains(&format!("<file appraisal.pdf, {} bytes>", PDF.len())),
            "{first}"
        );
        for line in v2_lib::applog::recent(400) {
            assert!(!line.message.contains(&b64), "the bytes reached the app log: {}", line.message);
        }
    }

    #[tokio::test]
    async fn a_failed_upload_step_names_the_file_never_its_bytes() {
        let _act = crate::serial::activity_log();
        let activity = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(activity.path().to_path_buf());
        let mut r = rig(vec![answer(400, json!({ "success": false, "error": "bad file" }))], None);
        let b64 = put_test_file(&r);
        let report = run(&mut r, uploading()).await;
        assert!(!report.ok);
        assert_eq!(report.failed.as_deref(), Some("Cycle setup"));
        let text = serde_json::to_string(&report).unwrap();
        assert!(!text.contains(&b64), "{text}");
        for rec in activity_records(activity.path(), "api") {
            assert!(!rec.to_string().contains(&b64), "{rec}");
        }
    }

    #[test]
    fn a_step_that_sends_files_gets_the_longer_limit() {
        use v2_lib::api_templates::runner::{step_limit, FETCH_GRACE, STEP_LIMIT, UPLOAD_STEP_LIMIT};
        let t = uploading();
        assert_eq!(step_limit(&t.steps[0]), UPLOAD_STEP_LIMIT);
        assert_eq!(step_limit(&t.steps[1]), STEP_LIMIT);
        assert_eq!(UPLOAD_STEP_LIMIT, Duration::from_secs(120));
        assert!(FETCH_GRACE > Duration::ZERO, "Rust waits past the page's own abort");
    }

    #[test]
    fn an_invalid_file_name_is_reported_once() {
        let root = tempfile::tempdir().unwrap();
        save_recipe(root.path(), ORG, PROJECT, &recipe()).unwrap();
        save_accounts(root.path(), &[account()]).unwrap();
        let mut t = uploading();
        t.steps[0].files = BTreeMap::from([("Document".to_string(), "..\\x.pdf".to_string())]);
        let problems = preflight(root.path(), &request(t, prove()), None).unwrap_err();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("file field 'Document'"), "{problems:?}");
    }

    #[test]
    fn preflight_refuses_a_file_this_project_does_not_have() {
        let root = tempfile::tempdir().unwrap();
        save_recipe(root.path(), ORG, PROJECT, &recipe()).unwrap();
        save_accounts(root.path(), &[account()]).unwrap();
        let mut t = uploading();
        // Uploaded by two steps: still one problem, naming the first.
        t.steps[1].files = BTreeMap::from([("Again".to_string(), "appraisal.pdf".to_string())]);
        let problems = preflight(root.path(), &request(t.clone(), prove()), None).unwrap_err();
        assert_eq!(
            problems,
            vec!["add \"appraisal.pdf\" to Test files (Auto Run or API Templates) - the step \"Cycle setup\" uploads it"
                .to_string()]
        );

        let dir = v2_lib::test_files::folder(root.path(), ORG, PROJECT);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("appraisal.pdf"), PDF).unwrap();
        assert_eq!(preflight(root.path(), &request(t.clone(), prove()), None), Ok(()));

        std::fs::File::create(dir.join("appraisal.pdf"))
            .unwrap()
            .set_len(v2_lib::test_files::MAX_BYTES + 1)
            .unwrap();
        let problems = preflight(root.path(), &request(t, prove()), None).unwrap_err();
        assert!(problems[0].contains("larger than 25 MB"), "{problems:?}");
    }
}

// ---- one sign-in per account at a time --------------------------------

/// The environment `r`'s root signs in to.
fn env_of(r: &Rig) -> String {
    v2_lib::environments::active_id(r.root.path()).unwrap()
}

async fn run_waiting(r: &mut Rig, wait_ms: u64) -> RunReport {
    let req = request(template(), prove());
    let timing = v2_lib::browser::timing::Timing { lease_wait_ms: wait_ms, ..quick() };
    run_template_within(&mut r.browsers, r.root.path(), &req, &timing, RUN_LIMIT, &QUICK_PAUSES).await
}

#[tokio::test]
async fn a_template_run_waits_for_an_unattended_case_then_proceeds() {
    use v2_lib::autorun::lease::{self, Holder};
    let _act = crate::serial::activity_log();
    let _l = crate::serial::account_leases();
    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    let env = env_of(&r);
    let case = lease::try_acquire(&env, "admin", Holder::Case { run: "run-x".into() }).unwrap();
    let released = Arc::new(Mutex::new(None::<Instant>));
    let noted = released.clone();
    let case_ends = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        *noted.lock().unwrap() = Some(Instant::now());
        drop(case);
    });
    let began = Instant::now();
    let report = run_waiting(&mut r, 5_000).await;
    case_ends.await.unwrap();
    assert!(report.ok, "{}", report.message());
    assert!(began.elapsed() >= Duration::from_millis(250), "it did not wait for the case: {:?}", began.elapsed());
    assert!(released.lock().unwrap().is_some());
    assert_eq!(r.sign_ins(), 1);
    assert!(!lease::is_held(&env, "admin"), "the run kept the account after it ended");
}

#[tokio::test]
async fn a_template_run_on_an_account_held_too_long_is_refused_and_opens_no_browser() {
    use v2_lib::autorun::lease::{self, Holder};
    let _act = crate::serial::activity_log();
    let _l = crate::serial::account_leases();
    let mut r = rig(vec![], None);
    let env = env_of(&r);
    let _browser = lease::try_acquire(&env, "admin", Holder::Browser).unwrap();
    let began = Instant::now();
    let report = run_waiting(&mut r, 200).await;
    assert!(began.elapsed() >= Duration::from_millis(150), "it gave up before its wait: {:?}", began.elapsed());
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("Sign in"));
    assert_eq!(
        report.steps.last().unwrap().detail,
        "the account admin was in use by the Auto Run browser - try again when it is free"
    );
    assert_eq!((r.browsers.opened, r.browsers.closed), (0, 0), "a browser was opened for a run that could not sign in");
    assert_eq!(r.sign_ins(), 0);
    assert!(lease::is_held(&env, "admin"), "the refusal took the browser's lease");
}

#[tokio::test]
async fn a_template_run_holds_its_account_and_lets_it_go_on_every_path_out() {
    use v2_lib::autorun::lease;
    let _act = crate::serial::activity_log();
    let _l = crate::serial::account_leases();

    // While it runs, the account is the run's: the page is stuck mid-step.
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().hang_fetch = true;
    let env = env_of(&r);
    let req = request(template(), prove());
    let watching = {
        let env = env.clone();
        tokio::spawn(async move {
            let began = Instant::now();
            while !lease::is_held(&env, "admin") {
                if began.elapsed() > Duration::from_secs(5) {
                    return false;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            true
        })
    };
    let report =
        run_template_within(&mut r.browsers, r.root.path(), &req, &quick(), Duration::from_millis(800), &QUICK_PAUSES).await;
    assert!(watching.await.unwrap(), "the run never held its account");
    assert!(!report.ok, "a run cut off by its limit");
    assert!(!lease::is_held(&env, "admin"), "a run cut off by its limit kept the account");

    // A sign-in that fails: an early return.
    let mut r = rig(vec![], Some("#go"));
    let env = env_of(&r);
    let report = run(&mut r, template()).await;
    assert!(!report.ok);
    assert!(!lease::is_held(&env, "admin"), "a run whose sign-in failed kept the account");

    // A run that passes.
    let mut r = rig(
        vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))],
        None,
    );
    let env = env_of(&r);
    assert!(run(&mut r, template()).await.ok);
    assert!(!lease::is_held(&env, "admin"), "a run that passed kept the account");
}
