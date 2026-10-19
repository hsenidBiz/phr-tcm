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
    ReplayAnswer, ReplayEnd, ReplayPhase, ReplayRequest, Reuse, ALREADY_RUNNING, CANCEL,
};
use v2_lib::ai_bridge::{DiscoveryBrowser, DiscoveryParts};
use v2_lib::commands::autorun::{
    guard_before_replay, guard_for_case, settle_guard_owner, settle_replay_guard, GuardOwner, ReplayBy,
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
    save_nav(root, "acme", "Web", &NavFile { direct_urls: false, modules: vec![leave_area()], save_words: vec![], protected: vec![] })
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
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 3, notice: None, whole: None });
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
    // Past the last step: the whole case ran, said as such, never as a
    // replay that stopped partway.
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 4, notice: None, whole: Some(3) });
    assert_eq!(end.sentence(), "replayed the whole of case 77 (all 3 steps) - the browser is on the page its last step left");
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
        &mut Reuse::default(),
    )
    .await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 4, notice: None, whole: None }, "{:?}", log(&app));
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
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 2, notice: Some(NOT_CHECKED.into()), whole: None });
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

/// An assistant's replay may switch a guard on, and never takes away one
/// the PERSON holds for another case: the person's guard for no-save case 5
/// stays on while unflagged case 77 is replayed (`may_lift` false), and
/// still belongs to case 5 and to the person.
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
    let mut owner = None;
    settle_guard_owner(&d, guarded, &mut owner, 5, GuardOwner::Person);
    assert_eq!(owner, Some(GuardOwner::Person));
    let may_lift = guard_before_replay(&mut d, &mut owner, false, ReplayBy::Assistant, dir.path(), "acme", "Web")
        .await
        .unwrap();
    assert!(!may_lift, "the assistant's replay may lift the person's guard");
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
        may_lift,
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
    settle_guard_owner(&d, guarded, &mut owner, ID, ReplayBy::Assistant.owns_as(ID));
    assert_eq!(owner, Some(GuardOwner::Person), "the person's guard became the assistant's");

    // The person's own replay of the same case does lift it. (`replay`
    // brings its own lease, so this one lets the account go first.)
    drop(held);
    let end = replay(&mut d, dir.path(), &req(2), &mut account, &mut guarded, &cancel, |_, _| {}).await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert!(!d.is_guarding_saves());
    assert_eq!(guarded, None);
    settle_guard_owner(&d, guarded, &mut owner, ID, ReplayBy::Person.owns_as(ID));
    assert_eq!(owner, None);
}

/// A browser whose page tries to save on the first click after `armed` is
/// set. Guarded, the guard stops it (`blocked`, `stopped`); unguarded, the
/// save goes through (`sent`).
struct SavesOnClick {
    inner: ScriptedDriver,
    armed: Arc<AtomicBool>,
    blocked: Option<String>,
    stopped: usize,
    sent: usize,
}

impl SavesOnClick {
    fn new(inner: ScriptedDriver, armed: Arc<AtomicBool>) -> Self {
        SavesOnClick { inner, armed, blocked: None, stopped: 0, sent: 0 }
    }
}

impl Driver for SavesOnClick {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        if method == "Input.dispatchMouseEvent" && self.armed.swap(false, Ordering::SeqCst) {
            if self.inner.is_guarding_saves() {
                self.blocked = Some(v2_lib::browser::save_guard::blocked("POST", "https://app.example/api/leave/save"));
                self.stopped += 1;
            } else {
                self.sent += 1;
            }
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
    fn is_guarding_saves(&self) -> bool {
        self.inner.is_guarding_saves()
    }
}

/// The supervised browser as `ai_bridge`'s `_in` functions see it, around
/// a `SavesOnClick`.
struct Supervised {
    d: SavesOnClick,
    lease: Held,
    account: Option<String>,
    discovery: Option<v2_lib::commands::autorun::DiscoveryState>,
    guarded_case: Option<i32>,
    guard_owner: Option<GuardOwner>,
}

impl DiscoveryBrowser for Supervised {
    type D = SavesOnClick;
    fn parts(&mut self) -> DiscoveryParts<'_, SavesOnClick> {
        DiscoveryParts {
            driver: &mut self.d,
            lease: &mut self.lease,
            signed_in: &mut self.account,
            discovery: &mut self.discovery,
            guarded_case: &mut self.guarded_case,
            guard_owner: &mut self.guard_owner,
            overrides: None,
        }
    }
    fn close(self) {}
}

fn supervised(d: SavesOnClick) -> Option<Supervised> {
    Some(Supervised { d, lease: Held::supervised(), account: None, discovery: None, guarded_case: None, guard_owner: None })
}

/// An assistant's replay of case `ID` to step 2 in `b`, as `replay_supervised`
/// runs one: the guard put where it must be first (`guard_before_replay`,
/// `ended_no_save` being whether the replay ended a no_save discovery), the
/// replay with the `may_lift` that gave, and the guard's owner settled after
/// with no discovery running (`settle_replay_guard`). The page tries to save
/// at step 1.
async fn assistant_replay(b: &mut Supervised, root: &Path, armed: &Arc<AtomicBool>, ended_no_save: bool) -> ReplayEnd {
    let may_lift = match guard_before_replay(&mut b.d, &mut b.guard_owner, ended_no_save, ReplayBy::Assistant, root, "acme", "Web").await {
        Ok(may_lift) => may_lift,
        Err(why) => return ReplayEnd::Refused(why),
    };
    let (mut account, cancel) = (b.account.take(), AtomicBool::new(false));
    let end = replay_to_checked(
        &mut b.d,
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        root,
        "acme",
        "Web",
        &req(2),
        &mut account,
        &mut b.lease,
        &mut b.guarded_case,
        may_lift,
        &quick(),
        &cancel,
        || -> PreconditionDb<FakeStageDb> { PreconditionDb::ReadingOff },
        |k, _| {
            if k == 1 {
                armed.store(true, Ordering::SeqCst);
            }
        },
    )
    .await;
    b.account = account;
    settle_replay_guard(&b.d, b.guarded_case, &mut b.guard_owner, ID, ReplayBy::Assistant, false);
    end
}

/// Review Focus 1. A no_save discovery is running and the assistant replays
/// a case NOT marked no_save: the replay ends the discovery to take the
/// browser, and is the discovery's last call - the guard stays on for it, so
/// the page's save at step 1 is stopped. The discovery has ended with it:
/// the guard is that replay's call's after, and the second replay of the
/// unmarked case lifts it and may save.
#[tokio::test]
async fn a_no_save_discovery_stays_guarded_until_it_ends() {
    use v2_lib::ai_bridge::end_discovery_for_replay;
    use v2_lib::commands::autorun::DiscoveryState;
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let armed = Arc::new(AtomicBool::new(false));
    // The discovery, started with no_save and guarded from its sign-in on.
    let (inner, app) = app();
    let mut slot = supervised(SavesOnClick::new(inner, armed.clone()));
    let b = slot.as_mut().unwrap();
    b.discovery = Some(DiscoveryState {
        area: Some("Leave".into()),
        account: Some("admin".into()),
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: true,
        call_no_save: false,
        call_case_id: None,
    });
    b.d.guard_saves(&[]).await.unwrap();
    b.guard_owner = Some(GuardOwner::Discovery);
    // The assistant's replay ends it first, keeping its browser and its
    // guard - never off between the discovery and the replay - and the
    // replay is guarded.
    let ended = end_discovery_for_replay(&mut slot, dir.path()).await;
    assert!(ended.ended && ended.no_save, "{ended:?}");
    let b = slot.as_mut().expect("the discovery's browser was closed");
    assert!(b.discovery.is_none());
    assert!(b.d.is_guarding_saves(), "the guard was lifted between the discovery and its last call");
    assert!(b.d.inner.calls_to("Fetch.disable").is_empty());
    let end = assistant_replay(b, dir.path(), &armed, ended.no_save).await;
    let ReplayEnd::StoppedAt { phase, step, why, .. } = &end else { panic!("{end:?}") };
    assert_eq!((*phase, *step), (ReplayPhase::Step, 1), "{end:?}");
    assert!(v2_lib::browser::save_guard::is_blocked(why), "{why}");
    assert_eq!((b.d.stopped, b.d.sent), (1, 0), "a save got through inside a no_save discovery");
    assert!(clicked(&app, "#s1"));
    assert!(b.d.is_guarding_saves());
    assert_eq!(b.guard_owner, Some(GuardOwner::Call { case_id: None }), "the guard outlived its discovery");
    // The second replay of the unmarked case lifts it, and may save.
    let end = assistant_replay(b, dir.path(), &armed, false).await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert_eq!((b.d.stopped, b.d.sent), (1, 1), "the replay after the discovery's was still guarded");
    assert!(!b.d.is_guarding_saves());
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (None, None));
}

/// A replay while a no_save discovery still runs stays under its guard: the
/// owner is the discovery's after it settles, and no later replay may lift it.
#[test]
fn a_replay_while_the_no_save_discovery_runs_stays_guarded() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    d.calls.push(("Fetch.enable".into(), json!({})));
    let mut owner = Some(GuardOwner::Discovery);
    settle_replay_guard(&d, None, &mut owner, ID, ReplayBy::Assistant, true);
    assert_eq!(owner, Some(GuardOwner::Discovery));
    assert!(!ReplayBy::Assistant.may_lift(owner.as_ref()));
    settle_replay_guard(&d, None, &mut owner, ID, ReplayBy::Assistant, false);
    assert_eq!(owner, Some(GuardOwner::Call { case_id: None }), "the guard outlived its discovery");
    assert!(ReplayBy::Assistant.may_lift(owner.as_ref()));
}

/// The assistant's own guard for another case - its replay or try of
/// no-save case 5 - is lifted by its replay of unflagged case 77, rather
/// than refusing 77's saves for 5: the save at step 1 goes through, and the
/// guard has no owner after.
#[tokio::test]
async fn a_replay_lifts_a_guard_the_assistant_left_for_another_case() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let mut five = script(three_steps());
    five.case_id = 5;
    five.no_save = true;
    store::save_script(dir.path(), &five).unwrap();
    let armed = Arc::new(AtomicBool::new(false));
    let (inner, app) = app();
    let mut slot = supervised(SavesOnClick::new(inner, armed.clone()));
    let b = slot.as_mut().unwrap();
    guard_for_case(&mut b.d, &mut b.guarded_case, dir.path(), "acme", "Web", 5, false).await.unwrap();
    settle_guard_owner(&b.d, b.guarded_case, &mut b.guard_owner, 5, ReplayBy::Assistant.owns_as(5));
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (Some(5), Some(GuardOwner::Call { case_id: Some(5) })));

    let end = assistant_replay(b, dir.path(), &armed, false).await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert!(clicked(&app, "#s1"));
    assert_eq!((b.d.stopped, b.d.sent), (0, 1), "case 77's save was stopped for case 5");
    assert!(!b.d.is_guarding_saves());
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (None, None));
}

/// The person's guard is never the assistant's to lift: the person's step of
/// no-save case 5 guards the browser, and the assistant's replay of
/// unflagged case 77 there keeps it - the save at step 1 is stopped for
/// case 5 - and the guard is still the person's after.
#[tokio::test]
async fn a_replay_never_lifts_the_persons_guard() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let mut five = script(three_steps());
    five.case_id = 5;
    five.no_save = true;
    store::save_script(dir.path(), &five).unwrap();
    let armed = Arc::new(AtomicBool::new(false));
    let (inner, _app) = app();
    let mut slot = supervised(SavesOnClick::new(inner, armed.clone()));
    let b = slot.as_mut().unwrap();
    guard_for_case(&mut b.d, &mut b.guarded_case, dir.path(), "acme", "Web", 5, true).await.unwrap();
    settle_guard_owner(&b.d, b.guarded_case, &mut b.guard_owner, 5, GuardOwner::Person);

    let end = assistant_replay(b, dir.path(), &armed, false).await;
    let ReplayEnd::StoppedAt { why, .. } = &end else { panic!("{end:?}") };
    assert_eq!(why, &guarding_other_case(5));
    assert_eq!((b.d.stopped, b.d.sent), (1, 0), "a save got through the person's guard");
    assert!(b.d.is_guarding_saves());
    assert!(!b.d.inner.methods().contains(&"Fetch.disable".to_string()));
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (Some(5), Some(GuardOwner::Person)));
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
    let answer: ReplayAnswer = ReplayEnd::Ready { case_id: ID, step: 3, notice: None, whole: None }.into();
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
        network_pauses: vec![],
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

/// An assistant's replay keeps a guard the person holds for another case
/// (5): it may not lift it (`ReplayBy::may_lift`, false here). A save the
/// replayed case's page tries is stopped for case 5, and the sentence says
/// so rather than calling it this script's own save.
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
        ReplayBy::Assistant.may_lift(Some(&GuardOwner::Person)),
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
    assert_eq!(signed_in_event(&none, &none), None, "nobody signed in still is no news");
    // A replay (or try) through a step's expire_session ends with nobody
    // signed in, and that is NOT news: the steps after it expect no session,
    // and a pane told so would sign in before them.
    assert_eq!(signed_in_event(&admin, &none), None, "a replay through expire_session told the panes nobody is signed in");
    let json = serde_json::to_value(signed_in_event(&none, &admin).unwrap()).unwrap();
    assert_eq!(json, json!({ "opened": true, "account": "admin" }));
    // Only the assistant's reset that found the sign-in gone says so.
    use v2_lib::autorun::replay_to::sign_in_forgotten_event;
    assert_eq!(sign_in_forgotten_event(&admin, &none), Some(AutorunSessionChanged { opened: true, account: None }));
    assert_eq!(sign_in_forgotten_event(&admin, &admin), None);
    assert_eq!(sign_in_forgotten_event(&none, &none), None);
    let source = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    let body = |name: &str| {
        let at = source.find(name).unwrap_or_else(|| panic!("{name} is gone"));
        let rest = &source[at..];
        rest[..rest.find("\n}\n").unwrap()].to_string()
    };
    let replay = body("pub(crate) async fn replay_supervised");
    assert!(replay.contains("signed_in_event(") && !replay.contains("sign_in_forgotten_event("), "{replay}");
    assert!(body("pub async fn reset_for_assistant").contains("sign_in_forgotten_event("));
    let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
    assert!(!bridge.contains("sign_in_forgotten_event(") && !bridge.contains("tell_panes("), "a try tells the panes of a session it ended");
}

/// The reuse trip's stopped-save line ends the guard's sentence with one
/// dot, whether or not that sentence brought its own.
#[test]
fn a_save_stopped_line_never_doubles_the_dot() {
    use v2_lib::autorun::replay_to::save_stopped_line;
    let want = "Signed in already, but on the way to Leave: it was stopped. Signed in again and went once more.";
    assert_eq!(save_stopped_line("Leave", "it was stopped"), want);
    assert_eq!(save_stopped_line("Leave", "it was stopped."), want);
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
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 3, notice: None, whole: None }, "{:?}", log(&app));
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
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 2, notice: None, whole: None });
    assert_eq!(from_here(Some(&stand(&saved, 2)), &saved, 4), ReplayFrom::CarriedOn(2));

    let before = log(&app).len();
    let end = carry(&mut d, dir.path(), &req(4), 2, &mut account, &mut held, &mut guarded).await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 4, notice: None, whole: Some(3) });
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
        assert!(b.contains("replay_stand = None;") || b.contains("used_otherwise();"), "{f} does not clear the stand");
        assert!(b.contains("note_session_use();"), "{f} is not counted as a use");
    }
    for f in ["pub async fn auto_run_step", "pub async fn auto_run_check_preconditions", "pub async fn auto_run_sign_in"] {
        // `make_persons` marks it, and takes away the assistant's test changes.
        let b = body(&source, f);
        assert!(
            b.contains("opened_by = OpenedBy::Person;") || b.contains("make_persons().await;"),
            "{f} does not make the browser the person's"
        );
    }
    let try_body = body(&bridge, "async fn autorun_try(");
    assert!(
        (try_body.contains("session.replay_stand = None;") || try_body.contains("session.used_otherwise();"))
            && try_body.contains("note_session_use()"),
        "a try is not a use"
    );
    // `used_otherwise` is what most uses call: it must still clear the stand.
    let used = body(&source, "pub(crate) fn used_otherwise(");
    assert!(used.contains("self.replay_stand = None;"), "{used}");
    assert!(body(&source, "async fn open_into").contains("replay_stand: None,"), "a new browser starts with a stand");
    let replay = body(&source, "pub(crate) async fn replay_supervised");
    assert!(replay.contains("let stand = session.replay_stand.take();"), "{replay}");
    assert!(replay.contains("(ReplayBy::Assistant, true, OpenedBy::Assistant, Some(stand), Some(saved)) =>"), "{replay}");
    assert!(replay.contains("(ReplayEnd::Ready { .. }, Some(saved)) if by == ReplayBy::Assistant =>"), "{replay}");
    assert!(replay.contains("&mut session.guarded_case,"), "{replay}");
}

/// The wiring of the replay's guard: `replay_supervised` puts the guard where
/// its owner says before either kind of replay runs (`guard_before_replay`,
/// carrying a no_save discovery it ended), hands its `may_lift` to the replay
/// from the beginning, and settles the owner once the replay is over.
#[test]
fn the_replay_puts_the_guard_where_its_owner_says() {
    let norm = |s: &str| s.replace("\r\n", "\n");
    let source = norm(include_str!("../../src/commands/autorun.rs"));
    let replay = &source[source.find("pub(crate) async fn replay_supervised").unwrap()..];
    let replay = &replay[..replay.find("\n}\n").unwrap()];
    let before = replay.find("guard_before_replay(").expect("the replay does not put the guard first");
    let call = &replay[before..before + replay[before..].find(".await").unwrap()];
    assert!(call.contains("discovery.no_save,"), "a no_save discovery's guard is not carried: {call}");
    let carry = replay.find("replay_to::carry_on(").unwrap();
    let traced = replay.find("replay_to::replay_to_traced(").unwrap();
    let settled = replay.find("settle_replay_guard(").expect("the owner is not settled after");
    assert!(replay.contains("let discovering = session.discovery.is_some();"), "{replay}");
    assert!(before < carry && before < traced && traced < settled, "{replay}");
    assert!(!replay.contains("by.may_lift(),"), "the replay's may_lift ignores the guard's owner");
}

// ---- a browser already signed in as the case's account ----------------------

/// `replay_to_traced` in the one browser a test keeps: its account, its
/// lease and its guard carried from one replay to the next, as the
/// supervised browser's are. The second value is what it made of a browser
/// already signed in (`Reuse`).
async fn replay_kept(
    d: &mut ScriptedDriver,
    root: &Path,
    step: i32,
    account: &mut Option<String>,
    held: &mut Held,
) -> (ReplayEnd, Reuse) {
    let mut guarded = None;
    replay_guarded(d, root, step, account, held, &mut guarded).await
}

/// `replay_kept` with the guard the test keeps too.
async fn replay_guarded(
    d: &mut ScriptedDriver,
    root: &Path,
    step: i32,
    account: &mut Option<String>,
    held: &mut Held,
    guarded: &mut Option<i32>,
) -> (ReplayEnd, Reuse) {
    use v2_lib::autorun::replay_to::replay_to_traced;
    let cancel = AtomicBool::new(false);
    let mut reuse = Reuse::default();
    let end = replay_to_traced(
        d,
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        root,
        "acme",
        "Web",
        &req(step),
        account,
        held,
        guarded,
        true,
        &quick(),
        &cancel,
        || -> PreconditionDb<FakeStageDb> { PreconditionDb::ReadingOff },
        |_, _| {},
        &mut Vec::new(),
        &mut reuse,
    )
    .await;
    (end, reuse)
}

/// How many times the browser was signed in since its calls were last
/// cleared: every sign-in clears it first, whether it then restores the
/// saved session or types the login.
fn sign_ins(d: &ScriptedDriver) -> usize {
    d.calls_to("Network.clearBrowserCookies").len()
}

/// The fake browser's cookies for the application's host, as (name, value).
/// `now` is what it holds; `login` what a sign-in leaves - put in as the
/// sign-in clears the site (`Storage.clearDataForOrigin`), which is before
/// anything reads them. `expire_session` deletes them one by one.
#[derive(Clone)]
struct Jar {
    now: Arc<std::sync::Mutex<Vec<(String, String)>>>,
    login: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

impl Jar {
    /// Change (or add) the cookie `name` the browser holds now.
    fn set(&self, name: &str, value: &str) {
        let mut now = self.now.lock().unwrap();
        now.retain(|(n, _)| n != name);
        now.push((name.to_string(), value.to_string()));
    }
}

/// `app()` with a cookie jar: a sign-in leaves admin's two PeoplesHR cookies.
fn jar_app() -> (ScriptedDriver, common::MenuApp, Jar) {
    let (d, app) = app();
    let admin = vec![(".ASPXAUTH".to_string(), "auth-admin-1".to_string()), ("ehrm85".to_string(), "admin".to_string())];
    let jar = Jar { now: Arc::new(std::sync::Mutex::new(vec![])), login: Arc::new(std::sync::Mutex::new(admin)) };
    let j = jar.clone();
    let d = d.answering_first(move |method, params| {
        let as_json = |cookies: &[(String, String)]| {
            let list: Vec<Value> = cookies
                .iter()
                .map(|(n, v)| json!({ "name": n, "value": v, "domain": "hr.example.internal", "path": "/", "session": true }))
                .collect();
            Ok(json!({ "cookies": list }))
        };
        match method {
            "Network.clearBrowserCookies" => {
                j.now.lock().unwrap().clear();
                None
            }
            "Storage.clearDataForOrigin" => {
                let login = j.login.lock().unwrap().clone();
                *j.now.lock().unwrap() = login;
                None
            }
            "Network.getAllCookies" | "Network.getCookies" => Some(as_json(&j.now.lock().unwrap())),
            "Network.deleteCookies" => {
                let name = params["name"].as_str().unwrap_or("").to_string();
                j.now.lock().unwrap().retain(|(n, _)| *n != name);
                Some(Ok(json!({})))
            }
            _ => None,
        }
    });
    (d, app, jar)
}

/// A browser a first replay signed in as `admin` and left before step 3,
/// with the log and the calls cleared: what a second replay then does is
/// all there is.
async fn signed_in_once(
    root: &Path,
    d: &mut ScriptedDriver,
    app: &common::MenuApp,
    account: &mut Option<String>,
    held: &mut Held,
) {
    let (first, reused) = replay_kept(d, root, 3, account, held).await;
    assert_eq!(first, ReplayEnd::Ready { case_id: ID, step: 3, notice: None, whole: None }, "{:?}", log(app));
    assert_eq!(reused, Reuse::default(), "a fresh browser signs in");
    assert_eq!(sign_ins(d), 1);
    assert_eq!(account.as_deref(), Some("admin"));
    assert!(held.sign_in_print().is_some(), "the sign-in's cookies were not noted");
    app.log.lock().unwrap().clear();
    d.calls.clear();
}

/// The second replay signed in again, with no trip to the area before it:
/// the first thing the page saw was the sign-in loading its start page.
fn signed_in_again(end: &ReplayEnd, reused: &Reuse, d: &ScriptedDriver, app: &common::MenuApp) {
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?} {:?}", log(app));
    assert_eq!(reused.went, None);
    assert_eq!(sign_ins(d), 1, "{:?}", log(app));
    let seen = log(app);
    assert_eq!(seen.first().map(String::as_str), Some("navigate /hr/home/index"), "it went to the area before signing in: {seen:?}");
}

#[tokio::test]
async fn a_replay_in_a_browser_signed_in_as_its_account_skips_the_sign_in() {
    use v2_lib::autorun::replay_to::signed_in_already_line;
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app, _jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;

    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 3, notice: None, whole: None }, "{:?}", log(&app));
    assert_eq!(reused.went.as_deref(), Some("Leave"), "it went to the case's area without signing in");
    assert_eq!(reused.said().as_deref(), Some("Signed in already: went to Leave."));
    assert_eq!(signed_in_already_line("Leave"), "Signed in already: went to Leave.");
    // No sign-in at all: nothing cleared, no login page, no login clicked.
    assert_eq!(sign_ins(&d), 0, "{:?}", log(&app));
    assert!(d.calls_to("Network.clearBrowserCookies").is_empty(), "the sign-in was cleared: {:?}", d.methods());
    assert!(d.calls_to("Page.navigate").is_empty(), "a page was loaded: {:?}", log(&app));
    let seen = log(&app);
    let at = |what: &str| seen.iter().position(|l| l == what).unwrap_or_else(|| panic!("no {what:?} in {seen:?}"));
    assert!(at("click Leave") < at("click #s1") && at("click #s1") < at("click #s2"), "{seen:?}");
    assert_eq!((account.as_deref(), held.account()), (Some("admin"), Some("admin")), "the sign-in and its lease are kept");
    // The print never shows a cookie's value.
    let shown = format!("{held:?}");
    assert!(!shown.contains("auth-admin") && shown.contains("CookiePrint(2 cookies)"), "{shown}");
}

/// A cookie the page set after the sign-in is not the sign-in's: the
/// browser still holds that sign-in.
#[tokio::test]
async fn a_cookie_the_page_added_since_the_sign_in_keeps_the_reuse() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app, jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;
    jar.set("culture", "en-GB");
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert_eq!(reused.went.as_deref(), Some("Leave"));
    assert_eq!(sign_ins(&d), 0, "{:?}", log(&app));
}

/// A step that typed another user into the login form (a login or role
/// test), or the person signing in again by hand: the app's account still
/// says admin and the marker still shows, but the cookies are another
/// user's - the replay signs in as admin.
#[tokio::test]
async fn another_users_cookies_sign_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app, jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;
    jar.set(".ASPXAUTH", "auth-carol-1");
    jar.set("ehrm85", "carol");
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    signed_in_again(&end, &reused, &d, &app);
    assert_eq!(*jar.now.lock().unwrap(), *jar.login.lock().unwrap(), "carol's sign-in was not cleared");
}

/// The application gave the sign-in cookie a new value: no longer provably
/// the app's sign-in, so the replay signs in - slower, never wrong.
#[tokio::test]
async fn a_rotated_cookie_value_signs_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app, jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;
    jar.set(".ASPXAUTH", "auth-admin-2");
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    signed_in_again(&end, &reused, &d, &app);
}

/// A browser with no cookies of the site at all shows nothing of whose
/// sign-in it holds: it is never reused.
#[tokio::test]
async fn a_sign_in_that_left_no_cookies_is_never_reused() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app) = app();
    let (mut account, mut held) = (None, Held::supervised());
    let (first, _) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    assert!(matches!(first, ReplayEnd::Ready { .. }), "{first:?}");
    assert!(held.sign_in_print().is_none());
    app.log.lock().unwrap().clear();
    d.calls.clear();
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    signed_in_again(&end, &reused, &d, &app);
}

/// Review Focus 2: a browser signed in as another account is never reused.
#[tokio::test]
async fn another_account_signs_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let mut boss = script(three_steps());
    boss.account = Some("boss".into());
    project(dir.path(), &boss);
    let mut second = account();
    second.key = "boss".into();
    second.label = "Boss".into();
    save_accounts(dir.path(), &[account(), second]).unwrap();
    let (mut d, app, _jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    let (first, _) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    assert!(matches!(first, ReplayEnd::Ready { .. }), "{first:?}");
    assert_eq!(account.as_deref(), Some("boss"));
    app.log.lock().unwrap().clear();
    d.calls.clear();

    // The case is run as admin now, and the browser is the boss account's:
    // it signs in.
    store::save_script(dir.path(), &script(three_steps())).unwrap();
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    signed_in_again(&end, &reused, &d, &app);
    assert!(!d.calls_to("Network.clearBrowserCookies").is_empty(), "the other sign-in was not cleared first");
    assert_eq!((account.as_deref(), held.account()), (Some("admin"), Some("admin")));
}

/// The replay's browser as a discovery that took it over sees it
/// (`ai_bridge::discover_start_in`).
struct Kept {
    d: ScriptedDriver,
    lease: Held,
    account: Option<String>,
    discovery: Option<v2_lib::commands::autorun::DiscoveryState>,
    guarded_case: Option<i32>,
    guard_owner: Option<GuardOwner>,
    /// How often something used it otherwise - a takeover, a reset - and
    /// so forgot the replay's stand and last page change.
    used_otherwise: usize,
}

impl DiscoveryBrowser for Kept {
    type D = ScriptedDriver;
    fn parts(&mut self) -> DiscoveryParts<'_, ScriptedDriver> {
        DiscoveryParts {
            driver: &mut self.d,
            lease: &mut self.lease,
            signed_in: &mut self.account,
            discovery: &mut self.discovery,
            guarded_case: &mut self.guarded_case,
            guard_owner: &mut self.guard_owner,
            overrides: None,
        }
    }
    fn close(self) {}
    fn opened_by_assistant(&mut self) -> bool {
        true
    }
    fn taken_over(&mut self) {
        self.used_otherwise += 1;
    }
}

impl Kept {
    /// The browser a replay signed in, with no discovery begun in it yet.
    fn of(d: ScriptedDriver, lease: Held, account: Option<String>) -> Kept {
        Kept { d, lease, account, discovery: None, guarded_case: None, guard_owner: None, used_otherwise: 0 }
    }
}

/// A discovery just begun in a browser, before its sign-in.
fn begun_discovery() -> Option<v2_lib::commands::autorun::DiscoveryState> {
    Some(v2_lib::commands::autorun::DiscoveryState {
        area: None,
        account: None,
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
    })
}

/// A discovery that took over the browser a replay signed in as admin
/// keeps that sign-in when it explores as admin - nothing cleared, no
/// login - and goes home first, so the replay's page is never filed under
/// the discovery's area; it signs in as its own account when that is
/// another.
#[tokio::test]
async fn a_discovery_for_another_account_signs_in_as_it() {
    use v2_lib::ai_bridge::{discover_start_in, discovery_signed_in_already};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let mut second = account();
    second.key = "boss".into();
    second.label = "Boss".into();
    save_accounts(dir.path(), &[account(), second]).unwrap();
    let (mut d, app, _jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;
    // The replay left the browser on the Leave screen.
    d.call("Page.navigate", json!({ "url": "https://hr.example.internal/hr/leave" })).await.unwrap();
    app.log.lock().unwrap().clear();
    d.calls.clear();
    let mut slot = Some(Kept::of(d, held, account));
    slot.as_mut().unwrap().discovery = begun_discovery();

    // As admin: the replay's sign-in is kept, and the discovery starts home.
    let (status, out) = discover_start_in(&mut slot, dir.path(), "acme", "Web", "admin", Some("Leave"), &quick()).await;
    assert_eq!(status, 200, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((v["signed_in"].as_bool(), v["detail"].as_str()), (Some(true), Some(discovery_signed_in_already("admin").as_str())));
    assert_eq!(discovery_signed_in_already("admin"), "signed in already as admin; went to the home page");
    assert_eq!(v["path"], "/hr/home/index", "the replay's page was read as the discovery's landing page: {out}");
    let b = slot.as_mut().expect("the browser was closed");
    assert_eq!(sign_ins(&b.d), 0, "the discovery signed in again: {:?}", log(&app));
    assert!(log(&app).contains(&"navigate /hr/home/index".to_string()), "it did not go home: {:?}", log(&app));
    assert_eq!(b.discovery.as_ref().and_then(|s| s.account.as_deref()), Some("admin"));
    assert_eq!((b.account.as_deref(), b.lease.account()), (Some("admin"), Some("admin")));

    // As boss: signed in as boss, the admin sign-in cleared first.
    b.discovery = begun_discovery();
    b.d.calls.clear();
    let (status, out) = discover_start_in(&mut slot, dir.path(), "acme", "Web", "boss", Some("Leave"), &quick()).await;
    assert_eq!(status, 200, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_ne!(v["detail"].as_str(), Some(discovery_signed_in_already("boss").as_str()));
    let b = slot.as_ref().expect("the browser was closed");
    assert_eq!(sign_ins(&b.d), 1, "{:?}", log(&app));
    assert_eq!((b.account.as_deref(), b.lease.account()), (Some("boss"), Some("boss")));
    assert_eq!(b.discovery.as_ref().and_then(|s| s.account.as_deref()), Some("boss"));
}

/// The replay's browser, guarded for its no_save case 77 by the replay's
/// call, as a discovery takes it over.
async fn guarded_replays_browser(root: &Path) -> (Option<Kept>, common::MenuApp) {
    let (mut d, app, _jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(root, &mut d, &app, &mut account, &mut held).await;
    d.guard_saves(&[]).await.unwrap();
    let mut kept = Kept::of(d, held, account);
    kept.guarded_case = Some(77);
    kept.guard_owner = Some(GuardOwner::Call { case_id: Some(77) });
    (Some(kept), app)
}

/// A no_save discovery taking over the replay's browser is never unguarded
/// for a moment: no guard stop is sent from the takeover to the end of its
/// start, whether the sign-in is kept or done again, and the guard is the
/// discovery's after.
#[tokio::test]
async fn a_no_save_discovery_takes_over_the_guard_with_no_gap() {
    use v2_lib::ai_bridge::{discover_start_in, make_way_for_discovery, GuardFrom, MadeWay};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let mut second = account();
    second.key = "boss".into();
    save_accounts(dir.path(), &[account(), second]).unwrap();
    for key in ["admin", "boss"] {
        let (mut slot, app) = guarded_replays_browser(dir.path()).await;
        slot.as_mut().unwrap().d.calls.clear();
        let from = GuardFrom { root: dir.path(), organization: "acme", project: "Web" };
        assert_eq!(make_way_for_discovery(&mut slot, None, Some(from)).await, Ok(MadeWay::TookOver));
        slot.as_mut().unwrap().discovery = begun_discovery().map(|s| v2_lib::commands::autorun::DiscoveryState { no_save: true, ..s });
        let (status, out) = discover_start_in(&mut slot, dir.path(), "acme", "Web", key, Some("Leave"), &quick()).await;
        assert_eq!(status, 200, "{key}: {out} {:?}", log(&app));
        let b = slot.as_ref().unwrap();
        assert!(b.d.calls_to("Fetch.disable").is_empty(), "{key}: the guard was stopped between the takeover and the re-arm");
        assert!(b.d.is_guarding_saves(), "{key}");
        assert_eq!(b.guard_owner, Some(GuardOwner::Discovery), "{key}");
        assert_eq!(b.account.as_deref(), Some(key));
    }
}

/// An unguarded discovery lifts the replay's guard once its start has
/// decided the sign-in; a lift the browser refuses leaves the guard on,
/// still the call's (a later replay lifts it): saves stay stopped, and the
/// discovery starts.
#[tokio::test]
async fn an_unguarded_discovery_lifts_the_replays_guard_or_keeps_it_on() {
    use v2_lib::ai_bridge::{discover_start_in, make_way_for_discovery, MadeWay};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut slot, _app) = guarded_replays_browser(dir.path()).await;
    assert_eq!(make_way_for_discovery(&mut slot, None, None).await, Ok(MadeWay::TookOver));
    slot.as_mut().unwrap().discovery = begun_discovery();
    let (status, out) = discover_start_in(&mut slot, dir.path(), "acme", "Web", "admin", Some("Leave"), &quick()).await;
    assert_eq!(status, 200, "{out}");
    let b = slot.as_ref().unwrap();
    assert!(!b.d.is_guarding_saves(), "the replay's guard outlived the takeover");
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (None, None));
    // Its lease goes with it, as a closed browser's does.
    drop(slot);

    let (mut slot, _app) = guarded_replays_browser(dir.path()).await;
    let refusing = std::mem::replace(&mut slot.as_mut().unwrap().d, ScriptedDriver::new(|_, _| Ok(json!({}))));
    slot.as_mut().unwrap().d = refusing.answering_first(|method, _| {
        (method == "Fetch.disable").then(|| Err(CdpError::Protocol { method: method.into(), message: "busy".into() }))
    });
    assert_eq!(make_way_for_discovery(&mut slot, None, None).await, Ok(MadeWay::TookOver));
    slot.as_mut().unwrap().discovery = begun_discovery();
    let (status, out) = discover_start_in(&mut slot, dir.path(), "acme", "Web", "admin", Some("Leave"), &quick()).await;
    assert_eq!(status, 200, "{out}");
    let b = slot.as_ref().expect("the browser was closed");
    assert_eq!(b.guard_owner, Some(GuardOwner::Call { case_id: Some(77) }), "a refused lift forgot whose guard it is");
}

/// Review Focus 3: a session the application refused ("Cookie has been
/// tampered") is never reused: the replay signs in fresh.
#[tokio::test]
async fn a_refused_session_signs_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (d, app, _jar) = jar_app();
    // The page refuses the session once `refusing` is set, until a sign-in
    // clears it.
    let refusing = Arc::new(AtomicBool::new(false));
    let r = refusing.clone();
    let mut d = d.answering_first(move |method, params| {
        if method == "Network.clearBrowserCookies" {
            r.store(false, Ordering::SeqCst);
        }
        (method == "Runtime.evaluate"
            && params["expression"] == v2_lib::autorun::signin::SESSION_REFUSED_JS
            && r.load(Ordering::SeqCst))
        .then(|| Ok(json!({ "result": { "value": "tampered" } })))
    });
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;

    refusing.store(true, Ordering::SeqCst);
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    signed_in_again(&end, &reused, &d, &app);
    assert!(!refusing.load(Ordering::SeqCst), "the refused session was not cleared");
}

/// Review Focus 4: a setup that made way (`setup::make_way` ended the
/// browser's session and let its lease go) leaves it signed in as nobody:
/// the replay signs in.
#[tokio::test]
async fn after_a_setup_made_way_it_signs_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app, _jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;

    v2_lib::autorun::setup::make_way(&mut d, &mut account, &mut held, "admin", &quick()).await;
    assert_eq!((account.as_deref(), held.account()), (None, None));
    assert!(held.sign_in_print().is_none(), "the print outlived the lease");
    app.log.lock().unwrap().clear();
    d.calls.clear();
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    signed_in_again(&end, &reused, &d, &app);
    assert_eq!(account.as_deref(), Some("admin"));
}

/// The browser's account still names admin but holds no lease for it: not
/// provably admin's browser, so the replay signs in.
#[tokio::test]
async fn an_account_with_no_lease_held_signs_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app, _jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;
    held.let_go();
    assert_eq!(account.as_deref(), Some("admin"));
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    signed_in_again(&end, &reused, &d, &app);
    assert_eq!(held.account(), Some("admin"));
}

/// A step that ends the session (`expire_session`) leaves the browser
/// signed in as nobody: the next replay signs in, with no trip to the area
/// first.
#[tokio::test]
async fn an_expire_session_step_clears_the_sign_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let steps = json!([
        { "step_number": 1, "actions": [click("#s1")] },
        { "step_number": 2, "actions": [{ "kind": "expire_session" }] },
        { "step_number": 3, "actions": [click("#s3")] }
    ]);
    project(dir.path(), &script(steps));
    let (mut d, app, _jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    let (first, _) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    assert_eq!(first, ReplayEnd::Ready { case_id: ID, step: 3, notice: None, whole: None }, "{:?}", log(&app));
    assert_eq!(account, None, "the ended session still names its account");
    assert!(held.sign_in_print().is_none(), "the ended session's print was kept");
    app.log.lock().unwrap().clear();
    d.calls.clear();
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    signed_in_again(&end, &reused, &d, &app);
}

/// The case's no-save guard is armed before anything looks at whether the
/// browser is signed in already, so the trip it then makes is guarded.
#[tokio::test]
async fn the_guard_is_decided_before_the_reuse() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let mut s = script(three_steps());
    s.no_save = true;
    project(dir.path(), &s);
    let (mut d, app, _jar) = jar_app();
    let (mut account, mut held, mut guarded) = (None, Held::supervised(), None);
    let (first, _) = replay_guarded(&mut d, dir.path(), 3, &mut account, &mut held, &mut guarded).await;
    assert!(matches!(first, ReplayEnd::Ready { .. }), "{first:?}");
    app.log.lock().unwrap().clear();
    d.calls.clear();
    let mut guarded = None;
    let (end, reused) = replay_guarded(&mut d, dir.path(), 3, &mut account, &mut held, &mut guarded).await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert_eq!(reused.went.as_deref(), Some("Leave"));
    assert_eq!(guarded, Some(ID));
    let methods = d.methods();
    let guard = methods.iter().position(|m| m == "Fetch.enable").unwrap_or_else(|| panic!("never guarded: {methods:?}"));
    let looked = methods.iter().position(|m| m == "Network.getAllCookies").unwrap();
    assert!(guard < looked, "the reuse was looked at before the guard: {methods:?}");
}

/// A save the guard stopped while a signed-in browser went to the area is
/// said in the answer, then the case signs in again and goes once more.
#[tokio::test]
async fn a_save_stopped_on_the_reuse_trip_is_said_before_signing_in_again() {
    use v2_lib::autorun::replay_to::save_stopped_line;
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut d, app, _jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;
    let sentence = v2_lib::browser::save_guard::blocked("POST", "https://hr.example.internal/api/leave/save");
    d.block_after = Some(("Accessibility.queryAXTree".into(), sentence.clone()));
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 3, notice: None, whole: None }, "{:?}", log(&app));
    assert_eq!(reused.went, None);
    assert_eq!(reused.save_stopped, Some(save_stopped_line("Leave", &sentence)));
    assert_eq!(
        reused.said().unwrap(),
        format!("Signed in already, but on the way to Leave: {sentence}. Signed in again and went once more.")
    );
    assert_eq!(sign_ins(&d), 1, "{:?}", log(&app));
}

/// The menu a signed-in browser shows is gone while `hidden` is set: until
/// the next sign-in clears the browser, or for good when `for_good`.
fn hiding_menu(d: ScriptedDriver, hidden: Arc<AtomicBool>, for_good: bool) -> ScriptedDriver {
    d.answering_first(move |method, _| {
        if method == "Network.clearBrowserCookies" && !for_good {
            hidden.store(false, Ordering::SeqCst);
        }
        (method == "Accessibility.queryAXTree" && hidden.load(Ordering::SeqCst)).then(|| Ok(json!({ "nodes": [] })))
    })
}

/// A browser that looks signed in but cannot reach the area: the replay
/// signs in once, as it would have, and goes again from there.
#[tokio::test]
async fn a_failed_trip_signs_in_once_and_goes_again() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let hidden = Arc::new(AtomicBool::new(false));
    let (d, app, _jar) = jar_app();
    let mut d = hiding_menu(d, hidden.clone(), false);
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;

    hidden.store(true, Ordering::SeqCst);
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    assert_eq!(end, ReplayEnd::Ready { case_id: ID, step: 3, notice: None, whole: None }, "{:?}", log(&app));
    assert_eq!(reused, Reuse::default(), "it signed in after all");
    assert_eq!(sign_ins(&d), 1, "{:?}", log(&app));
    let seen = log(&app);
    assert!(seen.ends_with(&["click Leave".to_string(), "click #s1".to_string(), "click #s2".to_string()]), "{seen:?}");
}

/// The area out of reach even after that sign-in: one sign-in, then the
/// replay stops while going to the area - it never signs in twice.
#[tokio::test]
async fn a_trip_that_fails_after_the_sign_in_too_stops_at_the_area() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let hidden = Arc::new(AtomicBool::new(false));
    let (d, app, _jar) = jar_app();
    let mut d = hiding_menu(d, hidden.clone(), true);
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(dir.path(), &mut d, &app, &mut account, &mut held).await;

    hidden.store(true, Ordering::SeqCst);
    let (end, reused) = replay_kept(&mut d, dir.path(), 3, &mut account, &mut held).await;
    let ReplayEnd::StoppedAt { phase, step, .. } = &end else { panic!("{end:?}") };
    assert_eq!((*phase, *step), (ReplayPhase::Area, 1), "{end:?}");
    assert_eq!(reused.went, None);
    assert_eq!(sign_ins(&d), 1, "{:?}", log(&app));
    assert!(!clicked(&app, "#s1"), "{:?}", log(&app));
}

/// The browser a replay signed in as admin and left on the Leave screen,
/// as the assistant's reset finds it: a report tab its steps opened, a
/// dialog it expected and one it saw, and the guard its call put on for
/// its no_save case 77.
async fn left_by_a_replay(root: &Path) -> (Option<Kept>, common::MenuApp, Jar) {
    let (mut d, app, jar) = jar_app();
    let (mut account, mut held) = (None, Held::supervised());
    signed_in_once(root, &mut d, &app, &mut account, &mut held).await;
    d.call("Page.navigate", json!({ "url": "https://hr.example.internal/hr/leave" })).await.unwrap();
    d.tabs.open.push("report".into());
    // Counted from here: the replay closed other tabs of its own.
    d.tabs.closed_others = 0;
    {
        use v2_lib::browser::actions::DialogAnswer;
        use v2_lib::browser::dialogs::plan_of;
        d.book.arm(vec![plan_of(1, DialogAnswer::Accept, &None)]);
        d.book.opened("alert", "Saved");
    }
    app.log.lock().unwrap().clear();
    d.calls.clear();
    // After the clear: the fake tells its guard from the calls it saw.
    d.guard_saves(&[]).await.unwrap();
    let mut kept = Kept::of(d, held, account);
    kept.guarded_case = Some(ID);
    kept.guard_owner = Some(GuardOwner::Call { case_id: Some(ID) });
    (Some(kept), app, jar)
}

/// The assistant's reset gives it a clean page without signing out: the
/// other tabs closed, the dialogs forgotten, its guard lifted, the replay's
/// stand and last page change forgotten, and the home page loaded with the
/// sign-in kept - nothing cleared, nothing typed.
#[tokio::test]
async fn a_reset_keeps_the_sign_in_and_goes_home() {
    use v2_lib::commands::autorun::{reset_in_for_assistant, reset_sentence};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut slot, app, _jar) = left_by_a_replay(dir.path()).await;
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick()).await;
    assert_eq!((status, said.as_str()), (200, reset_sentence("admin").as_str()), "{:?}", log(&app));
    assert_eq!(
        reset_sentence("admin"),
        "Reset: other tabs closed, dialogs cleared, guard lifted, at the home page, still signed in as admin."
    );
    let b = slot.as_ref().expect("the reset closed the browser");
    assert_eq!(sign_ins(&b.d), 0, "the reset signed in again: {:?}", log(&app));
    assert!(b.d.calls_to("Input.dispatchKeyEvent").is_empty() && b.d.calls_to("Input.insertText").is_empty(), "something was typed");
    assert_eq!(log(&app), vec!["navigate /hr/home/index".to_string()], "it did not just go home");
    assert_eq!(b.d.tabs.closed_others, 1, "the other tabs were left open");
    assert!(!b.d.book.is_armed() && b.d.book.seen().is_empty(), "the replay's dialogs were kept");
    assert!(!b.d.is_guarding_saves(), "the assistant's guard was left on");
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (None, None));
    assert_eq!(b.used_otherwise, 1, "the replay's stand and last page change were kept");
    assert_eq!((b.account.as_deref(), b.lease.account()), (Some("admin"), Some("admin")));
    assert!(b.lease.sign_in_print().is_some(), "the sign-in's cookies were forgotten");

    // The next replay reuses that sign-in.
    let mut b = slot.unwrap();
    let (mut account, mut held) = (b.account.take(), b.lease);
    let (end, reused) = replay_kept(&mut b.d, dir.path(), 3, &mut account, &mut held).await;
    assert!(matches!(end, ReplayEnd::Ready { .. }), "{end:?}");
    assert_eq!(reused.went.as_deref(), Some("Leave"), "the replay after a reset signed in again");
}

/// A reset ends the assistant's discovery as end_autorun_discovery does -
/// its guard lifted, the browser kept - and says so first.
#[tokio::test]
async fn a_reset_ends_the_assistants_discovery_and_keeps_the_browser() {
    use v2_lib::commands::autorun::{reset_in_for_assistant, reset_sentence, DISCOVERY_ENDED_AT_RESET};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut slot, app, _jar) = left_by_a_replay(dir.path()).await;
    {
        let b = slot.as_mut().unwrap();
        b.discovery = begun_discovery().map(|s| v2_lib::commands::autorun::DiscoveryState {
            area: Some("Leave".into()),
            account: Some("admin".into()),
            no_save: true,
            ..s
        });
        b.guarded_case = None;
        b.guard_owner = Some(GuardOwner::Discovery);
    }
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick()).await;
    assert_eq!(status, 200, "{said} {:?}", log(&app));
    assert_eq!(said, format!("{DISCOVERY_ENDED_AT_RESET} {}", reset_sentence("admin")));
    let b = slot.as_ref().expect("the reset closed the discovery's browser");
    assert!(b.discovery.is_none(), "the discovery is still going");
    assert!(!b.d.is_guarding_saves() && b.guard_owner.is_none(), "the discovery's guard outlived it");
    assert_eq!(sign_ins(&b.d), 0);
}

/// A reset whose home page never shows the app's sign-in - its session
/// gone - says so, and forgets the account and its cookies' print, so the
/// next replay signs in again rather than trust it.
#[tokio::test]
async fn a_reset_whose_sign_in_has_gone_says_so() {
    use v2_lib::commands::autorun::{reset_in_for_assistant, SIGN_IN_GONE_AT_RESET};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut slot, app, jar) = left_by_a_replay(dir.path()).await;
    // The application ended the session: its cookies are gone.
    jar.now.lock().unwrap().clear();
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick()).await;
    assert_eq!((status, said.as_str()), (200, SIGN_IN_GONE_AT_RESET), "{:?}", log(&app));
    assert_eq!(SIGN_IN_GONE_AT_RESET, "Reset, but the sign-in has gone: the next replay signs in again.");
    let b = slot.as_ref().expect("the reset closed the browser");
    assert_eq!(b.account, None, "the gone sign-in still names its account");
    assert!(b.lease.sign_in_print().is_none(), "the gone sign-in's print was kept");
    assert_eq!(sign_ins(&b.d), 0, "the reset signed in itself");
    assert!(!b.d.is_guarding_saves(), "the assistant's guard was left on");

    // The next replay signs in.
    let mut b = slot.unwrap();
    let (mut account, mut held) = (b.account.take(), b.lease);
    app.log.lock().unwrap().clear();
    b.d.calls.clear();
    let (end, reused) = replay_kept(&mut b.d, dir.path(), 3, &mut account, &mut held).await;
    signed_in_again(&end, &reused, &b.d, &app);
}

/// `b`'s driver, answering `first` before the fake app does.
fn answering_first(
    b: &mut Kept,
    first: impl FnMut(&str, &Value) -> Option<Result<Value, CdpError>> + Send + 'static,
) {
    let d = std::mem::replace(&mut b.d, ScriptedDriver::new(|_, _| Ok(json!({}))));
    b.d = d.answering_first(first);
}

/// A recipe whose fresh home runs `after_sign_in` once `#marker` shows,
/// as PeoplesHR's does: `nav::load_home` then waits for the marker itself.
fn with_after_sign_in(root: &Path) {
    let recipe: v2_lib::autorun::recipe::SignInRecipe = serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/hr/home/index",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "after_sign_in": [ { "kind": "click", "selector": { "css": "#menu" } } ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap();
    save_recipe(root, "acme", "Web", &recipe).unwrap();
}

/// With `after_sign_in`, a home page that loads but never shows the
/// signed-in marker is a sign-in that has gone - not a page that failed:
/// the account and its cookies' print are forgotten.
#[tokio::test]
async fn a_reset_whose_home_never_shows_the_marker_says_the_sign_in_has_gone() {
    use v2_lib::commands::autorun::{reset_in_for_assistant, SIGN_IN_GONE_AT_RESET};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    with_after_sign_in(dir.path());
    let (mut slot, app, _jar) = left_by_a_replay(dir.path()).await;
    // The page no longer shows the marker: the application signed it out.
    answering_first(slot.as_mut().unwrap(), |method, params| match method {
        "Runtime.callFunctionOn" if params["arguments"][0]["value"] == "#marker" => {
            Some(Ok(json!({ "result": { "objectId": "no-marker" } })))
        }
        "Runtime.getProperties" if params["objectId"] == "no-marker" => Some(Ok(json!({ "result": [] }))),
        _ => None,
    });
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick()).await;
    assert_eq!((status, said.as_str()), (200, SIGN_IN_GONE_AT_RESET), "{:?}", log(&app));
    let b = slot.as_ref().expect("the reset closed the browser");
    assert_eq!(b.account, None, "the gone sign-in still names its account");
    assert!(b.lease.sign_in_print().is_none(), "the gone sign-in's print was kept");
    assert!(b.guard_owner.is_none(), "the assistant's guard was left on");
    assert_eq!(sign_ins(&b.d), 0, "the reset signed in itself");
}

/// A home page that does not open - 502, or 503 when the browser does not
/// answer - still says what the reset cleaned first, after the
/// discovery-ended line when it ended one; the sign-in is not judged.
#[tokio::test]
async fn a_reset_whose_home_page_does_not_open_says_what_it_did() {
    use v2_lib::commands::autorun::{reset_failed_sentence, reset_in_for_assistant, DISCOVERY_ENDED_AT_RESET};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));

    let (mut slot, app, _jar) = left_by_a_replay(dir.path()).await;
    answering_first(slot.as_mut().unwrap(), |method, _| {
        (method == "Page.navigate").then(|| Ok(json!({ "errorText": "net::ERR_CONNECTION_REFUSED" })))
    });
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick()).await;
    assert_eq!((status, said.clone()), (502, reset_failed_sentence("guard lifted", "the home page did not load")), "{:?}", log(&app));
    assert_eq!(
        said,
        "Reset: other tabs closed, dialogs cleared, guard lifted, but the home page did not open: the home page did not load. Try reset_autorun_browser again, or release_autorun_browser if the browser is stuck."
    );
    let b = slot.as_ref().unwrap();
    assert_eq!(b.d.tabs.closed_others, 1);
    assert!(b.guard_owner.is_none() && b.d.book.seen().is_empty());
    assert_eq!(b.account.as_deref(), Some("admin"), "a page that failed to load was taken for a gone sign-in");
    // Its lease goes with it, for the next browser to sign in as admin.
    drop(slot);

    let (mut slot, _app, _jar) = left_by_a_replay(dir.path()).await;
    {
        let b = slot.as_mut().unwrap();
        b.discovery = begun_discovery();
        answering_first(b, |method, _| (method == "Page.navigate").then(|| Err(CdpError::Transport("socket reset".into()))));
    }
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick()).await;
    assert_eq!(status, 503, "{said}");
    assert_eq!(
        said,
        format!(
            "{DISCOVERY_ENDED_AT_RESET} {}",
            reset_failed_sentence("guard lifted", "the browser did not answer while the home page was opening")
        )
    );
    assert!(slot.as_ref().unwrap().discovery.is_none());
}

/// A guard the browser will not lift is said to be still on - saves stay
/// stopped - and stays the call's, for a later call to lift.
#[tokio::test]
async fn a_reset_whose_guard_will_not_lift_says_it_is_still_on() {
    use v2_lib::commands::autorun::{reset_in_for_assistant, reset_sentence, GUARD_STILL_ON_AT_RESET};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut slot, app, _jar) = left_by_a_replay(dir.path()).await;
    answering_first(slot.as_mut().unwrap(), |method, _| {
        (method == "Fetch.disable").then(|| Err(CdpError::Protocol { method: "Fetch.disable".into(), message: "busy".into() }))
    });
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick()).await;
    assert_eq!(GUARD_STILL_ON_AT_RESET, "guard still on (it would not lift yet)");
    assert_eq!(
        (status, said.as_str()),
        (200, "Reset: other tabs closed, dialogs cleared, guard still on (it would not lift yet), at the home page, still signed in as admin."),
        "{:?}",
        log(&app)
    );
    assert_ne!(said, reset_sentence("admin"));
    let b = slot.as_ref().unwrap();
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (Some(ID), Some(GuardOwner::Call { case_id: Some(ID) })), "a refused lift forgot the guard");
}

/// A reset whose sign-in has gone and whose guard will not lift says both,
/// as a sentence: "The guard is still on", never "The guard still on".
#[tokio::test]
async fn a_reset_whose_sign_in_has_gone_and_guard_stays_says_so_as_a_sentence() {
    use v2_lib::commands::autorun::{reset_in_for_assistant, SIGN_IN_GONE_AT_RESET};
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &script(three_steps()));
    let (mut slot, app, jar) = left_by_a_replay(dir.path()).await;
    jar.now.lock().unwrap().clear();
    answering_first(slot.as_mut().unwrap(), |method, _| {
        (method == "Fetch.disable").then(|| Err(CdpError::Protocol { method: "Fetch.disable".into(), message: "busy".into() }))
    });
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick()).await;
    assert_eq!(
        (status, said.as_str()),
        (200, format!("{SIGN_IN_GONE_AT_RESET} The guard is still on (it would not lift yet).").as_str()),
        "{:?}",
        log(&app)
    );
    assert_eq!(slot.as_ref().unwrap().account, None);
}
