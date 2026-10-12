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
    guarding_other_case, opened_event, replay_to, replay_to_checked, signed_in_event, stopped_before_opening, OneReplay,
    ReplayAnswer, ReplayEnd, ReplayPhase, ReplayRequest, ALREADY_RUNNING, CANCEL,
};
use v2_lib::commands::autorun::guard_for_case;
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
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        root,
        "acme",
        "Web",
        r,
        account,
        &mut held,
        guarded,
        true,
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

/// A replayed step that ran in a second tab carries that tab, as a live
/// step's record does, and a step back in `main` carries none.
#[tokio::test]
async fn a_replayed_step_in_a_second_tab_carries_its_tab() {
    use v2_lib::autorun::replay_to::{replay_to_traced, ReplayedTab};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let steps = json!([
        { "step_number": 1, "actions": [click("#s1"), { "kind": "expect_tab", "name": "report" }] },
        { "step_number": 2, "actions": [{ "kind": "switch_tab", "name": "report" }, click("#s2")] },
        { "step_number": 3, "actions": [{ "kind": "switch_tab", "name": "main" }, click("#s3")] },
        { "step_number": 4, "actions": [click("#s4")] }
    ]);
    project(dir.path(), &script(steps));
    let (mut d, app) = app();
    // A click opens a tab, as the case's link does.
    d.tabs.opens_on = Some("Input.dispatchMouseEvent".into());
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let mut held = Held::supervised();
    let mut tabs = Vec::new();
    let end = replay_to_traced(
        &mut d,
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        dir.path(),
        "acme",
        "Web",
        &req(4),
        &mut account,
        &mut held,
        &mut guarded,
        true,
        &quick(),
        &cancel,
        || -> PreconditionDb<FakeStageDb> { PreconditionDb::ReadingOff },
        |_, _| {},
        &mut tabs,
    )
    .await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 4, notice: None }, "{:?}", log(&app));
    // Step 1 opened the tab from main, step 2 ran in it, step 3 went back.
    assert_eq!(tabs, vec![ReplayedTab { step: 2, tab: "report".into() }]);
    // The person's command answers with them beside the end.
    let answer = ReplayAnswer::with_tabs(end, tabs);
    let json = serde_json::to_value(&answer).unwrap();
    assert_eq!(json["tabs"], json!([{ "step": 2, "tab": "report" }]));
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
    let ReplayEnd::StoppedAt { phase, step, why, outcomes } = &end else { panic!("{end:?}") };
    assert_eq!((*phase, *step), (ReplayPhase::Step, 2));
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
    assert_eq!(end.sentence(), "the replay was stopped at step 2: it was asked to stop (Stop, Close browser or Release Auto Run browser)");
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
async fn the_browser_closing_mid_replay_ends_it_saying_the_browser_closed() {
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
    assert_eq!(end, ReplayEnd::BrowserGone { step: 2, why: None, outcomes: vec![] });
    assert!(end.sentence().starts_with("replay stopped at step 2: the Auto Run browser closed"), "{}", end.sentence());
    assert!(clicked(&app, "#s1") && !clicked(&app, "#s2"), "{:?}", log(&app));
}

/// The reported case: a replay in a browser that had already gone (its
/// window closed, its connection dead) stops at step 1 - and says why,
/// never a bare "stopped at step 1".
#[tokio::test]
async fn a_replay_in_a_browser_already_gone_says_why_it_stopped_at_step_1() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (inner, app) = app();
    let dead = Arc::new(AtomicBool::new(true));
    let mut d = Dies { inner, dead };
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let end = replay(&mut d, dir.path(), &req(4), &mut account, &mut guarded, &cancel, |_, _| {}).await;
    let said = end.sentence();
    assert!(said.contains("step 1"), "{said}");
    assert_ne!(said, "the replay was stopped at step 1", "no reason given");
    let (_, why) = said.split_once(": ").unwrap_or_else(|| panic!("no reason: {said}"));
    assert!(!why.trim().is_empty(), "{said}");
    assert!(!clicked(&app, "#s1"), "{:?}", log(&app));
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
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        dir.path(),
        "acme",
        "Web",
        &ReplayRequest { case_id: ID, step: 3, db_read_access: true },
        &mut account,
        &mut held,
        &mut guarded,
        true,
        &quick(),
        &cancel,
        || PreconditionDb::Ready(db.clone()),
        |_, _| {},
    )
    .await;
    // Blocked, as a watched start blocks it: the pane keeps the sentence as
    // the case's reason.
    assert_eq!(end, ReplayEnd::Blocked("precondition not met: Publish for 274 (Performance cycle wizard)".into()));
    assert_eq!(end.sentence(), "precondition not met: Publish for 274 (Performance cycle wizard)");
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
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        dir.path(),
        "acme",
        "Web",
        &r,
        &mut account,
        &mut held,
        &mut guarded,
        true,
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
    assert_eq!(end, ReplayEnd::Blocked(NEED_DB.into()), "Blocked, as a watched start with no database to ask is");
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

/// An assistant's replay (`may_lift` false) may switch a guard on, never
/// take one away: a guard held for no-save case 5 stays on while unflagged
/// case 77 is replayed, and still belongs to case 5.
#[tokio::test]
async fn an_assistants_replay_never_lifts_a_guard_held_for_another_case() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let mut five = script(three_steps());
    five.case_id = 5;
    five.no_save = true;
    store::save_script(dir.path(), &five).unwrap();
    let (mut d, app) = app();
    let mut guarded = None;
    guard_for_case(&mut d, &mut guarded, dir.path(), "acme", "Web", 5, true).await.unwrap();
    assert_eq!(guarded, Some(5));
    let (mut account, cancel) = (None, AtomicBool::new(false));
    let mut held = Held::supervised();
    let end = replay_to_checked(
        &mut d,
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        dir.path(),
        "acme",
        "Web",
        &req(2),
        &mut account,
        &mut held,
        &mut guarded,
        false,
        &quick(),
        &cancel,
        || -> PreconditionDb<FakeStageDb> { PreconditionDb::ReadingOff },
        |_, _| {},
    )
    .await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert!(clicked(&app, "#s1"));
    assert!(d.is_guarding_saves(), "the guard was lifted: {:?}", d.methods());
    assert!(!d.methods().contains(&"Fetch.disable".to_string()), "{:?}", d.methods());
    assert_eq!(guarded, Some(5));

    // The person's own replay of the same case does lift it. (`replay`
    // brings its own lease, so this one lets the account go first.)
    drop(held);
    let end = replay(&mut d, dir.path(), &req(2), &mut account, &mut guarded, &cancel, |_, _| {}).await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert!(!d.is_guarding_saves());
    assert_eq!(guarded, None);
}

/// A Close that took the session's lock before the replay did: the replay
/// ends before any browser is opened for it.
#[test]
fn a_stop_before_the_browser_opens_ends_the_replay_there() {
    let cancel = AtomicBool::new(false);
    assert_eq!(stopped_before_opening(&cancel), None);
    cancel.store(true, Ordering::SeqCst);
    let end = stopped_before_opening(&cancel).expect("the stop was ignored");
    assert_eq!(end, ReplayEnd::Stopped { step: 1 });
    assert_eq!(end.sentence(), "the replay was stopped at step 1: it was asked to stop (Stop, Close browser or Release Auto Run browser)");
}

/// The person's command answers with the sentence already said.
#[test]
fn the_answer_carries_the_finished_sentence() {
    let answer: ReplayAnswer = ReplayEnd::Ready { case_id: ID, step: 3, notice: None }.into();
    assert_eq!(answer.sentence, "replayed case 77 to step 3 - the browser is on the page before step 3 runs");
    let answer: ReplayAnswer = ReplayEnd::Refused(ALREADY_RUNNING.into()).into();
    assert_eq!(answer.sentence, ALREADY_RUNNING);
    let json = serde_json::to_value(&answer).unwrap();
    assert_eq!(
        json,
        json!({ "end": { "kind": "refused", "detail": ALREADY_RUNNING }, "sentence": ALREADY_RUNNING, "tabs": [] })
    );
}

// ---- where a failure happened --------------------------------------------

#[test]
fn each_phase_says_where_the_replay_stopped() {
    let at = |phase| ReplayEnd::StoppedAt { phase, step: 1, why: "nothing matched #go".into(), outcomes: vec![] };
    assert_eq!(at(ReplayPhase::SignIn).sentence(), "replay stopped while signing in: nothing matched #go");
    assert_eq!(
        at(ReplayPhase::Area).sentence(),
        "replay stopped while going to the case's area: nothing matched #go"
    );
    assert_eq!(at(ReplayPhase::Step).sentence(), "replay stopped at step 1: nothing matched #go");
    assert_eq!(serde_json::to_value(at(ReplayPhase::SignIn)).unwrap()["detail"]["phase"], "sign_in");
    assert_eq!(serde_json::to_value(at(ReplayPhase::Area)).unwrap()["detail"]["phase"], "area");
    assert_eq!(serde_json::to_value(at(ReplayPhase::Step)).unwrap()["detail"]["phase"], "step");
}

#[tokio::test]
async fn an_area_that_cannot_be_reached_stops_the_replay_while_going_there_not_at_step_1() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    // A menu with no Leave link: the trip to the area cannot be made.
    let (mut d, app) = common::menu_app(&[("link", "Payroll", "/hr/payroll")], "/hr/home/index", 0);
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let end = replay(&mut d, dir.path(), &req(3), &mut account, &mut guarded, &cancel, |_, _| {}).await;
    let ReplayEnd::StoppedAt { phase, why, outcomes, .. } = &end else { panic!("{end:?}") };
    assert_eq!(*phase, ReplayPhase::Area);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(end.sentence(), format!("replay stopped while going to the case's area: {why}"));
    assert!(!clicked(&app, "#s1"), "step 1 ran: {:?}", log(&app));
}

/// A pane that saves a replay stopped while signing in keeps the outcomes
/// under the sign-in step (0): the run's failures say STOP for the sign-in,
/// never a defect in step 1.
#[test]
fn a_saved_sign_in_failure_is_a_sign_in_stop_not_a_step_1_defect() {
    use v2_lib::autorun::failures::{describe_failures, stop_reason};
    let case: v2_lib::autorun::CaseRecord = serde_json::from_value(json!({
        "case_id": ID,
        "title": "Leave request",
        "verdict": "Failed",
        "note": "",
        "steps": [
            { "step_number": 0, "outcomes": [ { "ok": false, "detail": "nothing matched #go" } ] },
            { "step_number": 1, "outcomes": [] },
            { "step_number": 2, "outcomes": [] }
        ]
    }))
    .unwrap();
    assert_eq!(
        stop_reason(&case).as_deref(),
        Some("the sign-in failed - fix the account or the recipe in the app, not the script")
    );
    let run = v2_lib::autorun::LocalRun {
        id: "run-1".into(),
        pbi_id: 1,
        started_at: "1".into(),
        cases: vec![case],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
    };
    let out = describe_failures(&run, &[]);
    assert!(out.contains("sign-in: nothing matched #go"), "{out}");
    assert!(!out.contains("step 1"), "{out}");
}

/// A browser whose page tries to save on the first click after `armed` is
/// set: the guard stops it and hands the sentence over, as a guarded
/// browser does.
struct SavesWhenArmed {
    inner: ScriptedDriver,
    armed: Arc<AtomicBool>,
    blocked: Option<String>,
}

impl Driver for SavesWhenArmed {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        if method == "Input.dispatchMouseEvent" && self.armed.swap(false, Ordering::SeqCst) {
            self.blocked = Some(v2_lib::browser::save_guard::blocked("POST", "https://app.example/api/leave/save"));
        }
        self.inner.call(method, params).await
    }
    async fn wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError> {
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
    fn take_save_blocked(&mut self) -> Option<String> {
        self.blocked.take()
    }
}

/// An assistant's replay keeps a guard held for another case (5). A save
/// the replayed case's page tries is stopped for case 5, and the sentence
/// says so rather than calling it this script's own save.
#[tokio::test]
async fn a_save_stopped_for_another_cases_guard_says_whose_draft_it_guards() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (inner, app) = app();
    let armed = Arc::new(AtomicBool::new(false));
    let mut d = SavesWhenArmed { inner, armed: armed.clone(), blocked: None };
    let (mut account, cancel) = (None, AtomicBool::new(false));
    let mut guarded = Some(5);
    let mut held = Held::supervised();
    let end = replay_to_checked(
        &mut d,
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        dir.path(),
        "acme",
        "Web",
        &req(4),
        &mut account,
        &mut held,
        &mut guarded,
        false,
        &quick(),
        &cancel,
        || -> PreconditionDb<FakeStageDb> { PreconditionDb::ReadingOff },
        |k, _| {
            if k == 2 {
                armed.store(true, Ordering::SeqCst);
            }
        },
    )
    .await;
    let ReplayEnd::StoppedAt { phase, step, why, .. } = &end else { panic!("{end:?}") };
    assert_eq!((*phase, *step), (ReplayPhase::Step, 2));
    assert_eq!(why, &guarding_other_case(5));
    assert_eq!(
        end.sentence(),
        "replay stopped at step 2: the Auto Run browser is guarding case 5's draft, and the page tried to save"
    );
    assert!(!clicked(&app, "#s3"), "{:?}", log(&app));
    assert_eq!(guarded, Some(5));
}

// ---- what the panes hear ---------------------------------------------------

#[test]
fn the_panes_hear_a_browser_a_replay_opened_and_the_account_it_signed_in() {
    use v2_lib::events::AutorunSessionChanged;
    assert_eq!(opened_event(false), Some(AutorunSessionChanged { opened: true, account: None }));
    assert_eq!(opened_event(true), None, "a browser already open is no news");
    let (none, admin, clerk) = (None, Some("admin".to_string()), Some("clerk".to_string()));
    assert_eq!(signed_in_event(&none, &admin), Some(AutorunSessionChanged { opened: true, account: admin.clone() }));
    assert_eq!(signed_in_event(&clerk, &admin), Some(AutorunSessionChanged { opened: true, account: admin.clone() }));
    assert_eq!(signed_in_event(&admin, &admin), None, "the same account is no news");
    assert_eq!(signed_in_event(&admin, &none), None);
    let json = serde_json::to_value(signed_in_event(&none, &admin).unwrap()).unwrap();
    assert_eq!(json, json!({ "opened": true, "account": "admin" }));
}

// ---- tabs -------------------------------------------------------------------

/// A replay starts the case afresh: a tab left from before is closed first,
/// and the tabs steps 1 to N-1 opened are opened again by running them.
#[tokio::test]
async fn a_replay_closes_the_tabs_left_from_before_and_recreates_the_cases_own() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let steps = json!([
        { "step_number": 1, "actions": [click("#s1"), { "kind": "expect_tab", "name": "report" }] },
        { "step_number": 2, "actions": [{ "kind": "switch_tab", "name": "report" }, click("#s2")] },
        { "step_number": 3, "actions": [click("#s3")] }
    ]);
    project(dir.path(), &script(steps));
    let (mut d, app) = app();
    d.tabs.open.push("left-over".into());
    d.tabs.current = "left-over".into();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    // A click opens a tab, as the case's link does.
    d.tabs.opens_on = Some("Input.dispatchMouseEvent".into());
    let end = replay(&mut d, dir.path(), &req(3), &mut account, &mut guarded, &cancel, |_, _| {}).await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 3, notice: None }, "{:?}", log(&app));
    assert_eq!(d.tabs.closed_others, 1, "the tabs from before were not closed first");
    assert_eq!(d.tabs.open, ["report"], "the case's own tab was not opened again");
    assert_eq!(d.tab_name(), "report", "the replay leaves the browser where step 2 left it");
    // The sign-in ran in main, not in the tab left from before.
    let first_call = d.tabs.calls.first().map(|(t, _)| t.clone());
    assert_eq!(first_call.as_deref(), Some("main"));
}

/// A replay that stopped on a failure and then found its browser gone says
/// both, and keeps the failed step's rows.
#[test]
fn a_browser_gone_after_a_failure_keeps_the_failure() {
    let rows = vec![v2_lib::browser::actions::ActionOutcome::failed("button \"Save\" not found")];
    let end = ReplayEnd::BrowserGone { step: 2, why: Some("button \"Save\" not found".into()), outcomes: rows.clone() };
    let said = end.sentence();
    assert!(said.starts_with("replay stopped at step 2: button \"Save\" not found; then the Auto Run browser closed"), "{said}");
    assert!(matches!(end, ReplayEnd::BrowserGone { outcomes, .. } if outcomes == rows));
}

// ---- carrying on from where the last replay stood ---------------------------

use v2_lib::autorun::replay_to::{carry_from, carry_on, ReplayFrom, ReplayStand, STAND_FRESH_MS};

/// Where the stands in these tests were left: the Leave page, at 1000 ms.
const HERE: &str = "/hr/leave";
const AT: u64 = 1_000;

fn stand(script: &CaseScript, before: i32) -> ReplayStand {
    ReplayStand::of(script, before, HERE, AT)
}

/// `carry_from` asked on the Leave page a moment after `AT`.
fn from_here(stand: Option<&ReplayStand>, script: &CaseScript, step: i32) -> ReplayFrom {
    carry_from(stand, script, step, HERE, AT + 1)
}

/// `carry_on` with the test's timing.
async fn carry(
    d: &mut impl Driver,
    root: &Path,
    r: &ReplayRequest,
    from: i32,
    account: &mut Option<String>,
    held: &mut Held,
    guarded: &mut Option<i32>,
) -> ReplayEnd {
    let cancel = AtomicBool::new(false);
    carry_on(d, root, "acme", "Web", r, from, account, held, guarded, &quick(), &cancel, |_, _| {}, &mut Vec::new()).await
}

/// A replay that stood before step 2 is carried on from there: only steps
/// 2 and 3 run, with no second sign-in, trip or step 1.
#[tokio::test]
async fn a_replay_carries_on_from_where_the_last_one_stood() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let saved = script(three_steps());
    project(dir.path(), &saved);
    let (mut d, app) = app();
    let mut held = Held::supervised();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let end = replay_to_checked(
        &mut d,
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        dir.path(),
        "acme",
        "Web",
        &req(2),
        &mut account,
        &mut held,
        &mut guarded,
        true,
        &quick(),
        &cancel,
        || -> PreconditionDb<FakeStageDb> { PreconditionDb::ReadingOff },
        |_, _| {},
    )
    .await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 2, notice: None });
    assert_eq!(from_here(Some(&stand(&saved, 2)), &saved, 4), ReplayFrom::CarriedOn(2));

    let before = log(&app).len();
    let end = carry(&mut d, dir.path(), &req(4), 2, &mut account, &mut held, &mut guarded).await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 4, notice: None });
    let after: Vec<String> = log(&app)[before..].to_vec();
    assert!(after.contains(&"click #s2".to_string()) && after.contains(&"click #s3".to_string()), "{after:?}");
    for not_again in ["click #go", "click Leave", "click #s1"] {
        assert!(!after.contains(&not_again.to_string()), "{not_again} ran again: {after:?}");
    }
    assert_eq!(ReplayFrom::CarriedOn(2).said(ID, 4), "Carried on from step 2.");
    assert_eq!(ReplayFrom::Beginning.said(ID, 4), "Started from the beginning.");
}

/// A carry-on arms the case's no-save guard, as every replay does: a
/// script marked Must not save is guarded however the browser got there.
#[tokio::test]
async fn a_carry_on_arms_the_no_save_guard() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let mut s = script(three_steps());
    s.no_save = true;
    project(dir.path(), &s);
    let (mut d, _app) = app();
    let (mut account, mut held, mut guarded) = (Some("admin".to_string()), Held::supervised(), None);
    let end = carry(&mut d, dir.path(), &req(3), 2, &mut account, &mut held, &mut guarded).await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert_eq!(guarded, Some(ID), "the carry-on ran unguarded");
    let methods = d.methods();
    let guard = methods.iter().position(|m| m == "Fetch.enable").expect("the guard never went on");
    let first_click = methods.iter().position(|m| m == "Input.dispatchMouseEvent").unwrap();
    assert!(guard < first_click, "a step ran before the guard: {methods:?}");
}

/// Already standing before the step asked for: nothing runs.
#[test]
fn a_replay_already_standing_before_the_step_runs_nothing() {
    let saved = script(three_steps());
    assert_eq!(from_here(Some(&stand(&saved, 3)), &saved, 3), ReplayFrom::AlreadyThere);
    assert_eq!(
        ReplayFrom::AlreadyThere.said(ID, 3),
        "The browser is already standing before step 3 of case 77: nothing ran."
    );
}

/// Anything else starts from the beginning: no stand (a try, a step, a
/// sign-in or a start in between cleared it, or the browser was closed and
/// a new one opened with none), another case, a browser that moved past
/// the step, or a script whose earlier steps changed since.
#[test]
fn a_replay_starts_over_for_another_case_a_later_stand_or_a_changed_script() {
    let saved = script(three_steps());
    assert_eq!(from_here(None, &saved, 3), ReplayFrom::Beginning);
    let mut other = saved.clone();
    other.case_id = 78;
    assert_eq!(from_here(Some(&stand(&other, 2)), &saved, 3), ReplayFrom::Beginning);
    assert_eq!(from_here(Some(&stand(&saved, 3)), &saved, 2), ReplayFrom::Beginning, "the browser moved on");
    let at_3 = stand(&saved, 3);
    let mut changed = saved.clone();
    changed.steps[0].actions = vec![serde_json::from_value(click("#other")).unwrap()];
    assert_eq!(from_here(Some(&at_3), &changed, 4), ReplayFrom::Beginning, "carried on past a changed step 1");
    // A later step changing does not matter: it has not run.
    let mut later = saved.clone();
    later.steps[2].actions = vec![serde_json::from_value(click("#other")).unwrap()];
    assert_eq!(from_here(Some(&at_3), &later, 4), ReplayFrom::CarriedOn(3));
    // Nor does the bookkeeping a save writes.
    let mut bookkept = saved.clone();
    bookkept.repairs = 2;
    bookkept.last_repair = Some("x".into());
    bookkept.saved_at = Some("later".into());
    assert_eq!(from_here(Some(&at_3), &bookkept, 4), ReplayFrom::CarriedOn(3));
}

/// Anything at the script's own level that shapes a run starts over:
/// turning Must not save on, another account, another area, another dialog
/// or page-error choice.
#[test]
fn a_script_setting_changed_since_starts_over() {
    let saved = script(three_steps());
    let at_2 = stand(&saved, 2);
    let mut guarded = saved.clone();
    guarded.no_save = true;
    assert_eq!(from_here(Some(&at_2), &guarded, 3), ReplayFrom::Beginning, "carried on past Must not save turned on");
    let mut other_account = saved.clone();
    other_account.account = Some("clerk".into());
    assert_eq!(from_here(Some(&at_2), &other_account, 3), ReplayFrom::Beginning);
    let mut other_area = saved.clone();
    other_area.area = Some("Payroll".into());
    assert_eq!(from_here(Some(&at_2), &other_area, 3), ReplayFrom::Beginning);
    let mut dialogs = saved.clone();
    dialogs.fail_on_unexpected_dialog = !saved.fail_on_unexpected_dialog;
    assert_eq!(from_here(Some(&at_2), &dialogs, 3), ReplayFrom::Beginning);
}

/// The stand is stamped from the script that ran: a repair saved while the
/// replay ran makes the next replay start from the beginning.
#[test]
fn a_repair_saved_during_a_replay_makes_the_next_start_over() {
    let ran = script(three_steps());
    let at_3 = stand(&ran, 3);
    let mut repaired = ran.clone();
    repaired.steps[1].actions = vec![serde_json::from_value(click("#fixed")).unwrap()];
    assert_eq!(from_here(Some(&at_3), &repaired, 4), ReplayFrom::Beginning);

    // The app stamps the stand from the copy it loaded before running,
    // never from one loaded after.
    let source = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    let replay = &source[source.find("pub(crate) async fn replay_supervised").unwrap()..];
    let replay = &replay[..replay.find("\n}\n").unwrap()];
    let loaded = replay.find("let saved = store::load_script(&root, case_id)").expect("the copy is not loaded once");
    let decided = replay.find("replay_to::carry_from(Some(stand), saved,").expect("the decision does not use the copy");
    let stamped = replay.find("replay_to::ReplayStand::of(saved, step,").expect("the stand is not stamped from the copy");
    assert!(loaded < decided && decided < stamped, "{replay}");
    assert_eq!(replay.matches("store::load_script(&root, case_id)").count(), 2, "the script is loaded again: {replay}");
}

/// A browser no longer on the page the replay left it on, or a stand older
/// than ten minutes (the application's own session may have ended), starts
/// over.
#[test]
fn another_page_or_an_old_stand_starts_over() {
    let saved = script(three_steps());
    let at_2 = stand(&saved, 2);
    assert_eq!(carry_from(Some(&at_2), &saved, 3, "/hr/home/index", AT + 1), ReplayFrom::Beginning);
    assert_eq!(carry_from(Some(&at_2), &saved, 3, "", AT + 1), ReplayFrom::Beginning);
    assert_eq!(STAND_FRESH_MS, 600_000);
    assert_eq!(carry_from(Some(&at_2), &saved, 3, HERE, AT + STAND_FRESH_MS - 1), ReplayFrom::CarriedOn(2));
    assert_eq!(carry_from(Some(&at_2), &saved, 3, HERE, AT + STAND_FRESH_MS), ReplayFrom::Beginning);
    assert_eq!(carry_from(Some(&ReplayStand::of(&saved, 2, "", AT)), &saved, 3, "", AT + 1), ReplayFrom::Beginning);
}

/// The wiring: only the assistant's replay in a browser that was already
/// open, the assistant's own, carries on; a try, a step, a sign-in and a
/// start clear the stand and count as a use; the person's sign-in and
/// start make the browser theirs; a browser opened anew starts with none;
/// only an assistant's replay that reached its step leaves one.
#[test]
fn every_other_use_of_the_browser_clears_the_stand() {
    let norm = |s: &str| s.replace("\r\n", "\n");
    let source = norm(include_str!("../../src/commands/autorun.rs"));
    let bridge = norm(include_str!("../../src/ai_bridge.rs"));
    let body = |text: &str, start: &str| -> String {
        let from = &text[text.find(start).unwrap_or_else(|| panic!("{start} is gone"))..];
        from[..from.find("\n}\n").unwrap()].to_string()
    };
    for f in ["pub async fn auto_run_step", "pub async fn auto_run_check_preconditions", "pub async fn make_way_for_setup", "pub async fn auto_run_sign_in"] {
        let b = body(&source, f);
        assert!(b.contains("replay_stand = None;"), "{f} does not clear the stand");
        assert!(b.contains("note_session_use();"), "{f} is not counted as a use");
    }
    for f in ["pub async fn auto_run_step", "pub async fn auto_run_check_preconditions", "pub async fn auto_run_sign_in"] {
        assert!(body(&source, f).contains("opened_by = OpenedBy::Person;"), "{f} does not make the browser the person's");
    }
    let try_body = body(&bridge, "async fn autorun_try(");
    assert!(try_body.contains("session.replay_stand = None;") && try_body.contains("note_session_use()"), "a try is not a use");
    assert!(body(&source, "async fn open_into").contains("replay_stand: None,"), "a new browser starts with a stand");
    let replay = body(&source, "pub(crate) async fn replay_supervised");
    assert!(replay.contains("let stand = session.replay_stand.take();"), "{replay}");
    assert!(replay.contains("(ReplayBy::Assistant, true, OpenedBy::Assistant, Some(stand), Some(saved)) =>"), "{replay}");
    assert!(replay.contains("(ReplayEnd::Ready { .. }, Some(saved)) if by == ReplayBy::Assistant =>"), "{replay}");
    assert!(replay.contains("&mut session.guarded_case,"), "{replay}");
}
