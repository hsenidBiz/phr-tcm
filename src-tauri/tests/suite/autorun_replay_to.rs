//! A replay to a step (`autorun::replay_to`): the supervised browser carried
//! through a case's saved steps 1 to N-1 and stopped before step N. The
//! browser is a fake (`common::menu_app`); `browser_live` replays a real one.

use crate::common::{self, account, cycle_flow_json, menu_recipe, FakeStageDb, ScriptedDriver};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use v2_lib::api_templates::flow::Flow;
use v2_lib::api_templates::flow_store;
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::lease::{self, Held, Holder};
use v2_lib::autorun::nav::{save_nav, ModulePath, NavFile};
use v2_lib::autorun::preconditions::{PreconditionDb, NEED_DB, NOT_CHECKED};
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::replay_to::{
    replay_to, replay_to_checked, OneReplay, ReplayEnd, ReplayRequest, ALREADY_RUNNING, CANCEL,
};
use v2_lib::autorun::runner::NEEDS_SCRIPT_AREA;
use v2_lib::autorun::{store, CaseScript};
use v2_lib::browser::cdp::{CdpError, Driver, Event};
use v2_lib::browser::timing::Timing;

const ID: i32 = 77;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 150, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn leave_area() -> ModulePath {
    serde_json::from_value(json!({
        "module": "Leave",
        "area": "Leave",
        "clicks": [ { "role": "link", "name": "Leave", "exact": true } ],
        "arrived": "/hr/leave",
        "recorded": "2026-10-05T10:00:00Z"
    }))
    .unwrap()
}

fn click(css: &str) -> Value {
    json!({ "kind": "click", "selector": { "css": css } })
}

/// Case 77: three steps, each one click on `#s<n>` - unless `steps` says
/// otherwise - run as `admin` in the Leave area.
fn script(steps: Value) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": ID,
        "title": "Leave request",
        "account": "admin",
        "area": "Leave",
        "steps": steps
    }))
    .unwrap()
}

fn three_steps() -> Value {
    json!([
        { "step_number": 1, "actions": [click("#s1")] },
        { "step_number": 2, "actions": [click("#s2")] },
        { "step_number": 3, "actions": [click("#s3")] }
    ])
}

/// The project, its sign-in, its account and the Leave area, and `script`.
fn project(root: &Path, script: &CaseScript) {
    save_nav(root, "acme", "Web", &NavFile { direct_urls: false, modules: vec![leave_area()], save_words: vec![] })
        .unwrap();
    save_recipe(root, "acme", "Web", &menu_recipe()).unwrap();
    save_accounts(root, &[account()]).unwrap();
    store::save_script(root, script).unwrap();
}

fn app() -> (ScriptedDriver, common::MenuApp) {
    common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0)
}

fn req(step: i32) -> ReplayRequest {
    ReplayRequest { case_id: ID, step, db_read_access: false }
}

fn log(app: &common::MenuApp) -> Vec<String> {
    app.log.lock().unwrap().clone()
}

fn clicked(app: &common::MenuApp, what: &str) -> bool {
    log(app).contains(&format!("click {what}"))
}

/// `replay_to_checked` with the test's timing and reading off.
async fn replay(
    d: &mut impl Driver,
    root: &Path,
    r: &ReplayRequest,
    account: &mut Option<String>,
    guarded: &mut Option<i32>,
    cancel: &AtomicBool,
    progress: impl FnMut(i32, i32),
) -> ReplayEnd {
    let mut held = Held::supervised();
    replay_to_checked(
        d,
        root,
        "acme",
        "Web",
        r,
        account,
        &mut held,
        guarded,
        &quick(),
        cancel,
        || -> PreconditionDb<FakeStageDb> { PreconditionDb::ReadingOff },
        progress,
    )
    .await
}

// ---- the checks before anything opens ------------------------------------

#[tokio::test]
async fn a_case_with_no_saved_script_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = common::FakePage::default().driver();
    let (mut account, mut held, cancel) = (None, Held::supervised(), AtomicBool::new(false));
    let end = replay_to(&mut d, dir.path(), "acme", "Web", &req(2), &mut account, &mut held, &cancel, |_, _| {}).await;
    assert_eq!(end, ReplayEnd::Refused("case 77 has no saved script".into()));
    assert_eq!(end.sentence(), "case 77 has no saved script");
    assert!(d.calls.is_empty(), "the browser was used: {:?}", d.methods());
}

#[tokio::test]
async fn a_step_outside_the_script_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    for step in [0, -1, 5] {
        let mut d = common::FakePage::default().driver();
        let (mut account, mut held, cancel) = (None, Held::supervised(), AtomicBool::new(false));
        let end =
            replay_to(&mut d, dir.path(), "acme", "Web", &req(step), &mut account, &mut held, &cancel, |_, _| {}).await;
        assert_eq!(
            end.sentence(),
            format!("step {step} is not in case 77's script (it has steps 1 to 3)"),
            "step {step}"
        );
        assert!(matches!(end, ReplayEnd::Refused(_)));
        assert!(d.calls.is_empty(), "step {step}: the browser was used: {:?}", d.methods());
    }
}

#[tokio::test]
async fn a_script_with_no_area_stops_before_step_1_with_the_area_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = script(three_steps());
    s.area = None;
    project(dir.path(), &s);
    let (mut d, app) = app();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let end = replay(&mut d, dir.path(), &req(3), &mut account, &mut guarded, &cancel, |_, _| {}).await;
    assert_eq!(end, ReplayEnd::Refused(NEEDS_SCRIPT_AREA.into()));
    assert!(log(&app).is_empty(), "nothing should have happened in the browser: {:?}", log(&app));
    assert_eq!(account, None);
}

// ---- the steps --------------------------------------------------------------

#[tokio::test]
async fn steps_before_n_run_after_the_sign_in_and_the_trip_and_step_n_does_not() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app) = app();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let mut heard = vec![];
    let end = replay(&mut d, dir.path(), &req(3), &mut account, &mut guarded, &cancel, |k, of| heard.push((k, of))).await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 3, notice: None });
    assert_eq!(end.sentence(), "replayed case 77 to step 3 - the browser is on the page before step 3 runs");
    let seen = log(&app);
    let at = |what: &str| seen.iter().position(|l| l == what).unwrap_or_else(|| panic!("no {what:?} in {seen:?}"));
    assert!(at("click #go") < at("click Leave"), "signed in, then went to the area: {seen:?}");
    assert!(at("click Leave") < at("click #s1") && at("click #s1") < at("click #s2"), "{seen:?}");
    assert!(!clicked(&app, "#s3"), "step 3 ran: {seen:?}");
    assert_eq!(heard, vec![(1, 2), (2, 2)]);
    assert_eq!(account.as_deref(), Some("admin"), "the browser is signed in as the case's account");
}

#[tokio::test]
async fn the_last_step_plus_1_replays_every_step() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app) = app();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let mut heard = vec![];
    let end = replay(&mut d, dir.path(), &req(4), &mut account, &mut guarded, &cancel, |k, of| heard.push((k, of))).await;
    assert_eq!(end.sentence(), "replayed case 77 to step 4 - the browser is on the page before step 4 runs");
    for s in ["#s1", "#s2", "#s3"] {
        assert!(clicked(&app, s), "{s} did not run: {:?}", log(&app));
    }
    assert_eq!(heard, vec![(1, 3), (2, 3), (3, 3)]);
}

#[tokio::test]
async fn a_failing_step_stops_the_replay_there_with_its_outcomes() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let steps = json!([
        { "step_number": 1, "actions": [click("#s1")] },
        { "step_number": 2, "actions": [click("#s2"), { "kind": "check_text", "value": "no" }] },
        { "step_number": 3, "actions": [click("#s3")] }
    ]);
    project(dir.path(), &script(steps));
    let (mut d, app) = app();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let end = replay(&mut d, dir.path(), &req(4), &mut account, &mut guarded, &cancel, |_, _| {}).await;
    let ReplayEnd::StoppedAt { step, why, outcomes } = &end else { panic!("{end:?}") };
    assert_eq!(*step, 2);
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes[0].ok && !outcomes[1].ok, "{outcomes:?}");
    assert_eq!(why, &outcomes[1].detail);
    assert!(outcomes[1].screenshot.is_some(), "a failure carries its screenshot: {outcomes:?}");
    assert_eq!(end.sentence(), format!("replay stopped at step 2: {why}"));
    assert!(!clicked(&app, "#s3"), "the replay went on past the failure: {:?}", log(&app));
}

#[tokio::test]
async fn a_stop_mid_replay_ends_it_before_the_next_step() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app) = app();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let end = replay(&mut d, dir.path(), &req(4), &mut account, &mut guarded, &cancel, |k, _| {
        if k == 1 {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .await;
    assert_eq!(end, ReplayEnd::Stopped { step: 2 });
    assert_eq!(end.sentence(), "the replay was stopped at step 2");
    assert!(clicked(&app, "#s1") && !clicked(&app, "#s2"), "{:?}", log(&app));
}

/// A browser that dies - the person closed its window - once `dead` is set.
struct Dies {
    inner: ScriptedDriver,
    dead: Arc<AtomicBool>,
}

impl Driver for Dies {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        if self.dead.load(Ordering::SeqCst) {
            return Err(CdpError::Closed);
        }
        self.inner.call(method, params).await
    }
    async fn wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError> {
        if self.dead.load(Ordering::SeqCst) {
            return Err(CdpError::Closed);
        }
        self.inner.wait_event(method, limit).await
    }
    fn forget_events(&mut self) {
        self.inner.forget_events();
    }
    fn take_dialogs(&mut self) -> Vec<String> {
        self.inner.take_dialogs()
    }
    fn set_deadline(&mut self, deadline: Option<std::time::Instant>) {
        self.inner.set_deadline(deadline);
    }
}

#[tokio::test]
async fn the_browser_closing_mid_replay_ends_it_as_stopped() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (inner, app) = app();
    let dead = Arc::new(AtomicBool::new(false));
    let mut d = Dies { inner, dead: dead.clone() };
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let end = replay(&mut d, dir.path(), &req(4), &mut account, &mut guarded, &cancel, |k, _| {
        if k == 2 {
            dead.store(true, Ordering::SeqCst);
        }
    })
    .await;
    assert_eq!(end, ReplayEnd::Stopped { step: 2 });
    assert!(clicked(&app, "#s1") && !clicked(&app, "#s2"), "{:?}", log(&app));
}

// ---- the guards ---------------------------------------------------------

fn with_precondition() -> CaseScript {
    let mut s = script(three_steps());
    s.preconditions = vec![serde_json::from_value(json!({
        "flow": "pms-performance-cycle", "stage": "publish", "value": 274
    }))
    .unwrap()];
    s
}

#[tokio::test]
async fn a_precondition_not_met_stops_it_with_its_blocked_sentence_and_signs_nobody_in() {
    let _g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &with_precondition());
    let flow: Flow = serde_json::from_value(cycle_flow_json()).unwrap();
    flow_store::save(dir.path(), "acme", "Web", &flow).unwrap();
    let (mut d, app) = app();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let mut held = Held::supervised();
    let db = FakeStageDb::new().answer("/*publish*/", Ok(false));
    let end = replay_to_checked(
        &mut d,
        dir.path(),
        "acme",
        "Web",
        &ReplayRequest { case_id: ID, step: 3, db_read_access: true },
        &mut account,
        &mut held,
        &mut guarded,
        &quick(),
        &cancel,
        || PreconditionDb::Ready(db.clone()),
        |_, _| {},
    )
    .await;
    assert_eq!(end, ReplayEnd::Refused("precondition not met: Publish for 274 (Performance cycle wizard)".into()));
    assert!(log(&app).is_empty(), "something ran in the browser: {:?}", log(&app));
    assert_eq!((account, held.account()), (None, None), "nobody was signed in");
}

#[tokio::test]
async fn with_database_read_access_off_no_database_is_asked_and_the_notice_is_carried() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &with_precondition());
    let (mut d, app) = app();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let mut held = Held::supervised();
    let db = FakeStageDb::new();
    let r = ReplayRequest { case_id: ID, step: 2, db_read_access: false };
    let end = replay_to_checked(
        &mut d,
        dir.path(),
        "acme",
        "Web",
        &r,
        &mut account,
        &mut held,
        &mut guarded,
        &quick(),
        &cancel,
        || if r.db_read_access { PreconditionDb::Ready(db.clone()) } else { PreconditionDb::ReadingOff },
        |_, _| {},
    )
    .await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 2, notice: Some(NOT_CHECKED.into()) });
    assert!(db.calls().is_empty(), "the database was asked: {:?}", db.calls());
    assert!(clicked(&app, "#s1"));
}

#[tokio::test]
async fn with_no_database_to_ask_the_plain_replay_says_so() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &with_precondition());
    let (mut d, app) = app();
    let (mut account, mut held, cancel) = (None, Held::supervised(), AtomicBool::new(false));
    let r = ReplayRequest { case_id: ID, step: 2, db_read_access: true };
    let end = replay_to(&mut d, dir.path(), "acme", "Web", &r, &mut account, &mut held, &cancel, |_, _| {}).await;
    assert_eq!(end, ReplayEnd::Refused(NEED_DB.into()));
    assert!(log(&app).is_empty());
}

#[tokio::test]
async fn an_account_something_else_holds_is_refused_with_the_lease_sentence() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    let _theirs = lease::try_acquire(&env, "admin", Holder::Template).unwrap();
    let (mut d, app) = app();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let end = replay(&mut d, dir.path(), &req(3), &mut account, &mut guarded, &cancel, |_, _| {}).await;
    let ReplayEnd::Refused(why) = &end else { panic!("{end:?}") };
    assert!(lease::is_in_use(why), "{why}");
    assert!(why.contains("an API template run"), "{why}");
    assert!(!clicked(&app, "#go"), "it signed in anyway: {:?}", log(&app));
    assert_eq!(account, None);
}

#[tokio::test]
async fn a_must_not_save_script_is_guarded_before_step_1() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let mut s = script(three_steps());
    s.no_save = true;
    project(dir.path(), &s);
    let (mut d, app) = app();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let end = replay(&mut d, dir.path(), &req(2), &mut account, &mut guarded, &cancel, |_, _| {}).await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert_eq!(guarded, Some(ID));
    let methods = d.methods();
    let guard = methods.iter().position(|m| m == "Fetch.enable").expect("the guard never went on");
    let first_click = methods.iter().position(|m| m == "Input.dispatchMouseEvent").unwrap();
    assert!(guard < first_click, "the guard went on after the browser was used: {methods:?}");
    assert!(d.is_guarding_saves());
    assert!(clicked(&app, "#s1"));
}

// ---- one at a time ------------------------------------------------------

#[test]
fn a_second_replay_is_refused_while_one_is_running() {
    let _a = crate::serial::autorun();
    v2_lib::autorun::replay_to::stop();
    let first = OneReplay::claim().expect("the first replay");
    assert!(!CANCEL.load(Ordering::SeqCst), "a new replay starts with no stop pending");
    assert!(v2_lib::autorun::replay_to::is_running());
    assert_eq!(OneReplay::claim().err().as_deref(), Some(ALREADY_RUNNING));
    assert_eq!(ALREADY_RUNNING, "a replay is already running - wait for it to finish");
    drop(first);
    assert!(!v2_lib::autorun::replay_to::is_running());
    assert!(OneReplay::claim().is_ok(), "the slot frees once the replay ends");
}

// ---- the remembered browser ----------------------------------------------

#[test]
fn the_browser_last_chosen_is_remembered_and_edge_is_the_default() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(store::last_browser(dir.path()), "edge");
    store::remember_browser(dir.path(), "Chrome");
    assert_eq!(store::last_browser(dir.path()), "chrome");
    store::remember_browser(dir.path(), "edge");
    assert_eq!(store::last_browser(dir.path()), "edge");
    store::remember_browser(dir.path(), "netscape");
    assert_eq!(store::last_browser(dir.path()), "edge");
}
