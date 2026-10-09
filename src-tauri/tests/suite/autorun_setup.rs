//! Scripts that use fixtures (`autorun::setup`) and the person's approval
//! of a setup (`autorun::approvals`): the design doc's section 2.
//!
//! The setup's browser is the template runner's fake page
//! (`api_templates_runner::rig`); the case's own browser is a scripted one,
//! as in the replay tests. The fixture runs as `setupper`, an account no
//! other test leases, so the lease checks here need no lock of their own.

use crate::api_templates_runner::{answer, rig, App, Rig, ORG, PAGE, PROJECT, QUICK_PAUSES};
use crate::common::{self, account, quick, ScriptedDriver};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use v2_lib::api_templates::fixture::{Creates, Fixture, FixtureRun, FixtureStep};
use v2_lib::api_templates::fixture_run::Clock;
use v2_lib::api_templates::runner::RUN_LIMIT;
use v2_lib::api_templates::{fixture_store, store as template_store, ApiTemplate, Proven};
use v2_lib::autorun::accounts::{save_accounts, Account};
use v2_lib::autorun::approvals::{self, Approval};
use v2_lib::autorun::edits::{check_edits, SETUP_KEPT};
use v2_lib::autorun::lease::{self, Held};
use v2_lib::autorun::nav::{check_project_rules, save_nav, ModulePath, NavFile};
use v2_lib::autorun::preconditions::{NoDb, PreconditionDb};
use v2_lib::autorun::replay::{run_cases, Browsers, CaseToRun};
use v2_lib::autorun::replay_to::{replay_to_checked, ReplayEnd, ReplayRequest};
use v2_lib::autorun::setup::{
    self, check_saved, not_built, prepare_case, prepare_case_within, resolve, FixtureValues, NOT_APPROVED,
};
use v2_lib::autorun::{store, test_made, CaseScript, LocalRun, Setup};
use v2_lib::browser::cdp::{CdpError, Driver, Event};

const CLOCK: Clock = Clock { year: 2026, month: 10, day: 6, hour: 14, minute: 30, second: 0 };
const ACCOUNT: &str = "setupper";
/// A stop nobody asks for.
static NO_STOP: AtomicBool = AtomicBool::new(false);

fn proven(mut t: ApiTemplate) -> ApiTemplate {
    t.proven = Some(Proven {
        at: "2026-10-01 09:00:00".into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: BTreeMap::new(),
        environment: None,
    });
    t
}

/// Makes a cycle in one request; hands back its id and name.
fn make_cycle() -> ApiTemplate {
    proven(
        serde_json::from_value(json!({
            "id": "make-cycle", "title": "Make a cycle", "module": "PMS", "effect": "create",
            "description": "d", "sources": ["x:1"], "antiforgery": { "page": PAGE },
            "params": [ { "name": "cycleName", "type": "string", "required": true } ],
            "steps": [ { "name": "Cycle setup", "method": "POST", "path": "/hr/pmsv10/performancecycle",
                         "query": { "handler": "SaveProgress" }, "form": { "CycleName": "{{cycleName}}" },
                         "capture": { "cycleId": "$.cycleId", "cycleName": "$.cycleName" } } ],
            "outputs": ["cycleId", "cycleName"]
        }))
        .unwrap(),
    )
}

fn fixture(id: &str) -> Fixture {
    Fixture {
        id: id.into(),
        name: format!("Fixture {id}"),
        account: ACCOUNT.into(),
        steps: vec![FixtureStep {
            template: "make-cycle".into(),
            params: [("cycleName".to_string(), "{{prefix}} cycle".to_string())].into(),
        }],
        outputs: [("cycle_id".to_string(), "{{steps.1.cycleId}}".to_string())].into(),
        creates: vec![Creates { kind: "cycle".into(), id: "{{steps.1.cycleId}}".into(), name: "{{steps.1.cycleName}}".into() }],
    }
}

fn the_cycle() -> Value {
    answer(200, json!({ "cycleId": 274, "cycleName": "AUTOTEST cycle" }))
}

/// A rig whose root holds the template, the setup fixture `own`, a shared
/// fixture `shared` (never run) and the `setupper` account.
fn setup_rig(responses: Vec<Value>) -> Rig {
    let r = rig(responses, None);
    let root = r.root.path();
    save_accounts(root, &[account(), Account { key: ACCOUNT.into(), ..account() }]).unwrap();
    template_store::save(root, ORG, PROJECT, &make_cycle()).unwrap();
    fixture_store::save(root, ORG, PROJECT, &fixture("own")).unwrap();
    fixture_store::save(root, ORG, PROJECT, &fixture("shared")).unwrap();
    r
}

/// The shared fixture built once, with `cycle_id` 100.
fn build_shared(root: &std::path::Path) {
    let run = FixtureRun {
        at: "2026-10-05 10:00:00".into(),
        ok: true,
        failed_step: None,
        detail: None,
        outputs: [("cycle_id".to_string(), json!(100))].into(),
    };
    fixture_store::append_run(root, ORG, PROJECT, "shared", run).unwrap();
}

fn script(case_id: i32, setup: Option<&str>, steps: Value) -> CaseScript {
    let mut v = json!({ "case_id": case_id, "title": format!("case {case_id}"), "steps": steps });
    if let Some(f) = setup {
        v["setup"] = json!({ "fixture": f });
    }
    serde_json::from_value(v).unwrap()
}

/// Records that the live app showed `css` in the Leave area: the run checks
/// a locator a setup value was filled into against what was seen.
fn seen_on_leave(root: &std::path::Path, css: &str) {
    use v2_lib::browser::locator::{LocatorStep, Target};
    let t = Target::One(LocatorStep { css: Some(css.to_string()), ..LocatorStep::default() });
    v2_lib::autorun::discovery_map::record_matched(root, ORG, PROJECT, Some("Leave"), "/hr/leave", &t, 1).unwrap();
}

fn check(value: &str) -> Value {
    json!({ "kind": "check_text", "value": value })
}

/// Approves case `case_id`'s setup as it is saved now.
fn approve(root: &std::path::Path, sc: &CaseScript) -> String {
    let fp = setup::current(root, ORG, PROJECT, sc.setup.as_ref().unwrap()).unwrap().fingerprint;
    approvals::approve(root, sc.case_id, &fp).unwrap();
    fp
}

fn state(root: &std::path::Path, sc: &CaseScript) -> Approval {
    let fp = setup::current(root, ORG, PROJECT, sc.setup.as_ref().unwrap()).unwrap().fingerprint;
    approvals::state(root, sc.case_id, &fp)
}

// ---- save-time refusals ---------------------------------------------------

#[test]
fn a_setup_naming_no_fixture_is_refused_at_save() {
    let sc = script(1, Some("nowhere"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    assert_eq!(check_saved(&sc, &[fixture("own")]), Err(vec!["setup: there is no fixture nowhere".to_string()]));
}

#[test]
fn an_unknown_fixture_or_output_is_refused_at_save() {
    let sc = script(
        1,
        None,
        json!([{ "step_number": 1, "actions": [
            check("{{fixture.own.cycle_id}}"),
            check("{{fixture.own.nothing}}"),
            check("{{fixture.ghost.cycle_id}}")
        ] }]),
    );
    assert_eq!(
        check_saved(&sc, &[fixture("own")]),
        Err(vec![
            "{{fixture.own.nothing}}: there is no such fixture output".to_string(),
            "{{fixture.ghost.cycle_id}}: there is no such fixture output".to_string(),
        ])
    );
}

#[test]
fn an_unknown_setup_output_is_refused_at_save() {
    let sc = script(
        1,
        Some("own"),
        json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}"), check("{{setup.suite_id}}")] }]),
    );
    assert_eq!(
        check_saved(&sc, &[fixture("own")]),
        Err(vec!["{{setup.suite_id}}: fixture Fixture own has no output suite_id".to_string()])
    );
}

/// Every save path goes through `nav::check_project_rules`.
#[test]
fn the_project_rules_refuse_an_unknown_fixture_output() {
    let r = setup_rig(vec![]);
    let bad = script(5, None, json!([{ "step_number": 1, "actions": [check("{{fixture.own.nothing}}")] }]));
    assert_eq!(
        check_project_rules(r.root.path(), ORG, PROJECT, &[bad]),
        Err("case 5: {{fixture.own.nothing}}: there is no such fixture output".to_string())
    );
    let good = script(5, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    assert_eq!(check_project_rules(r.root.path(), ORG, PROJECT, &[good]), Ok(()));
}

// ---- resolving --------------------------------------------------------------

#[test]
fn fixture_and_setup_values_are_put_into_a_copy() {
    let sc = script(
        1,
        Some("own"),
        json!([{ "step_number": 1, "actions": [
            { "kind": "navigate", "url": "https://hr.example.internal/hr/cycle/{{fixture.shared.cycle_id}}" },
            { "kind": "fill", "selector": { "css": "#name" }, "value": "draft {{setup.cycle_id}}" },
            check("{{fixture.shared.cycle_id}} and {{setup.cycle_id}}")
        ] }]),
    );
    let fixtures = BTreeMap::from([(
        "shared".to_string(),
        FixtureValues { name: "Shared".into(), outputs: Some([("cycle_id".to_string(), json!(100))].into()) },
    )]);
    let out = resolve(&sc, &fixtures, &[("cycle_id".to_string(), json!(274))].into()).unwrap();
    let text = serde_json::to_string(&out.steps).unwrap();
    assert!(text.contains("/hr/cycle/100"), "{text}");
    assert!(text.contains("draft 274"), "{text}");
    assert!(text.contains("100 and 274"), "{text}");
    assert!(!text.contains("{{"), "{text}");
    // Everything else is as it was; the original is untouched.
    assert_eq!(CaseScript { steps: sc.steps.clone(), ..out.clone() }, sc);
}

#[test]
fn a_fixture_never_built_blocks_with_the_sentence() {
    let sc = script(1, None, json!([{ "step_number": 1, "actions": [check("{{fixture.shared.cycle_id}}")] }]));
    let fixtures = BTreeMap::from([("shared".to_string(), FixtureValues { name: "Shared draft".into(), outputs: None })]);
    assert_eq!(
        resolve(&sc, &fixtures, &BTreeMap::new()),
        Err("fixture Shared draft has not been built - run it from API Templates, Fixtures".to_string())
    );
    assert_eq!(not_built("Shared draft"), "fixture Shared draft has not been built - run it from API Templates, Fixtures");
}

#[tokio::test]
async fn an_unbuilt_shared_fixture_blocks_the_case_before_anything_opens() {
    let _slot = crate::serial::api_template_run();
    let mut r = setup_rig(vec![]);
    let sc = script(1, None, json!([{ "step_number": 1, "actions": [check("{{fixture.shared.cycle_id}}")] }]));
    let got = prepare_case(&mut r.browsers, r.root.path(), ORG, PROJECT, &sc, &quick(), &NO_STOP).await;
    assert_eq!(got.map(|p| p.script), Err(not_built("Fixture shared")));
    assert_eq!(r.browsers.opened, 0);

    build_shared(r.root.path());
    let got = prepare_case(&mut r.browsers, r.root.path(), ORG, PROJECT, &sc, &quick(), &NO_STOP).await.unwrap();
    assert!(serde_json::to_string(&got.script.steps).unwrap().contains("\"100\""));
    assert_eq!(r.browsers.opened, 0, "a shared draft opens nothing");
}

// ---- the unattended run ----------------------------------------------------

/// The setup's browser (the template runner's page) or the case's own.
enum Either {
    Setup(App),
    Case(ScriptedDriver),
}

impl Driver for Either {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        match self {
            Either::Setup(d) => d.call(method, params).await,
            Either::Case(d) => d.call(method, params).await,
        }
    }
    async fn wait_event(&mut self, method: &str, limit: std::time::Duration) -> Result<Event, CdpError> {
        match self {
            Either::Setup(d) => d.wait_event(method, limit).await,
            Either::Case(d) => d.wait_event(method, limit).await,
        }
    }
    fn forget_events(&mut self) {
        match self {
            Either::Setup(d) => d.forget_events(),
            Either::Case(d) => d.forget_events(),
        }
    }
    fn take_dialogs(&mut self) -> Vec<String> {
        match self {
            Either::Setup(d) => d.take_dialogs(),
            Either::Case(d) => d.take_dialogs(),
        }
    }
    fn set_deadline(&mut self, deadline: Option<Instant>) {
        match self {
            Either::Setup(d) => d.set_deadline(deadline),
            Either::Case(d) => d.set_deadline(deadline),
        }
    }
}

/// A setup's browser, then the case's, and again for a retry; records
/// what happened, in order, and whether the setup's account was still
/// leased when the case's browser opened.
struct Ordered {
    setups: std::collections::VecDeque<App>,
    cases: std::collections::VecDeque<ScriptedDriver>,
    /// The next browser asked for is a setup's.
    want_setup: bool,
    events: Arc<Mutex<Vec<String>>>,
    env: String,
    checks: Arc<Mutex<Vec<String>>>,
}

impl Browsers for Ordered {
    type D = Either;
    async fn open(&mut self) -> Result<Either, String> {
        if self.want_setup {
            if let Some(app) = self.setups.pop_front() {
                self.want_setup = false;
                self.events.lock().unwrap().push("setup opened".into());
                return Ok(Either::Setup(app));
            }
        }
        let held = lease::is_held(&self.env, ACCOUNT);
        self.events.lock().unwrap().push(format!("case opened, setup account leased: {held}"));
        self.want_setup = true;
        self.cases.pop_front().map(Either::Case).ok_or_else(|| "no browser left".to_string())
    }
    async fn close(&mut self, d: Either) {
        let what = match &d {
            Either::Setup(_) => "setup closed",
            Either::Case(_) => "case closed",
        };
        if let Either::Case(c) = &d {
            let mut checks = self.checks.lock().unwrap();
            for p in c.calls_to("Runtime.callFunctionOn") {
                if let Some(v) = p["arguments"][0]["value"].as_str() {
                    checks.push(v.to_string());
                }
            }
        }
        self.events.lock().unwrap().push(what.into());
    }
}

/// A case page whose `check_text` passes only for the values given.
fn case_page(pass: &'static [&'static str]) -> ScriptedDriver {
    ScriptedDriver::new(move |method, params| match method {
        "Runtime.evaluate" if params["expression"] == "document" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" => {
            let arg = params["arguments"][0]["value"].as_str().unwrap_or("");
            Ok(json!({ "result": { "value": pass.contains(&arg) } }))
        }
        "Page.captureScreenshot" => Ok(json!({ "data": "/9j/4AAQ" })),
        _ => Ok(json!({})),
    })
}

fn new_run() -> LocalRun {
    LocalRun {
        id: "run-setup".into(),
        pbi_id: 42,
        started_at: "1700000000000".into(),
        cases: vec![],
        mode: "unattended".into(),
        published: None,
        environment: None,
        resets: vec![],
    }
}

fn ordered(r: &mut Rig, case: ScriptedDriver) -> Ordered {
    let env = v2_lib::environments::active_id(r.root.path()).unwrap();
    Ordered {
        setups: r.browsers.next.take().into_iter().collect(),
        cases: [case].into(),
        want_setup: true,
        events: Arc::default(),
        env,
        checks: Arc::default(),
    }
}

async fn run_one(r: &Rig, browsers: &mut Ordered, case_id: i32) -> LocalRun {
    run_with(r, browsers, case_id, false).await
}

async fn run_with(r: &Rig, browsers: &mut Ordered, case_id: i32, retry: bool) -> LocalRun {
    let mut run = new_run();
    let cancel = AtomicBool::new(false);
    let cases = [CaseToRun { case_id, title: format!("case {case_id}"), module: None }];
    run_cases(browsers, r.root.path(), ORG, PROJECT, &mut run, &cases, None, retry, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    run
}

/// The setup runs after the preconditions and before the case's browser
/// opens; its browser is closed and its lease let go first; its outputs and
/// a shared fixture's reach the steps through a copy; what it made carries
/// the case's id; the saved script is unchanged.
#[tokio::test]
async fn a_setup_runs_before_the_case_signs_in_and_its_outputs_reach_the_steps() {
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let mut r = setup_rig(vec![the_cycle()]);
    build_shared(r.root.path());
    let sc = script(
        1,
        Some("own"),
        json!([{ "step_number": 1, "actions": [check("cycle {{setup.cycle_id}}"), check("{{fixture.shared.cycle_id}}")] }]),
    );
    store::save_script(r.root.path(), &sc).unwrap();
    let saved_before = std::fs::read_to_string(r.root.path().join("scripts").join("case-1.json")).unwrap();
    approve(r.root.path(), &sc);

    let mut browsers = ordered(&mut r, case_page(&["cycle 274", "100"]));
    let run = run_one(&r, &mut browsers, 1).await;

    let case = &run.cases[0];
    assert_eq!(case.proposed, "Passed", "{case:?}");
    assert_eq!(
        *browsers.events.lock().unwrap(),
        vec!["setup opened", "setup closed", "case opened, setup account leased: false", "case closed"]
    );
    assert_eq!(*browsers.checks.lock().unwrap(), vec!["cycle 274", "100"]);
    assert_eq!(r.sign_ins(), 1, "the setup signed in once, as its fixture's account");

    let made = test_made::list(r.root.path());
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!((made[0].id.as_str(), made[0].case_id), ("274", Some(1)));
    assert_eq!(made[0].fixture, "own");

    assert_eq!(std::fs::read_to_string(r.root.path().join("scripts").join("case-1.json")).unwrap(), saved_before);
    assert_eq!(store::load_script(r.root.path(), 1).unwrap().unwrap(), sc);
}

/// Without an approval nothing runs and nothing signs in.
#[tokio::test]
async fn a_setup_not_approved_blocks_the_case_with_nothing_opened() {
    let _slot = crate::serial::api_template_run();
    let mut r = setup_rig(vec![the_cycle()]);
    let sc = script(2, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    store::save_script(r.root.path(), &sc).unwrap();

    let mut browsers = ordered(&mut r, case_page(&["274"]));
    let run = run_one(&r, &mut browsers, 2).await;
    let case = &run.cases[0];
    assert_eq!(case.proposed, "Blocked");
    assert_eq!(case.reason, NOT_APPROVED);
    assert_eq!(NOT_APPROVED, "setup not approved - approve it in the script editor");
    assert!(browsers.events.lock().unwrap().is_empty(), "{:?}", browsers.events);
    assert_eq!(r.sign_ins(), 0);
    assert!(r.fetched().is_empty());
    assert!(test_made::list(r.root.path()).is_empty());
}

/// A transient failure runs the case again from a fresh draft: the setup
/// runs again first, and the second go uses what that run gave.
#[tokio::test]
async fn a_retry_makes_a_fresh_draft_and_uses_it() {
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let mut r = setup_rig(vec![the_cycle(), answer(200, json!({ "cycleId": 275, "cycleName": "AUTOTEST cycle" }))]);
    let sc = script(14, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    store::save_script(r.root.path(), &sc).unwrap();
    approve(r.root.path(), &sc);

    // The first go's browser stops answering: a transient failure.
    let silent = ScriptedDriver::new(|method, _| match method {
        "Runtime.evaluate" => Err(CdpError::Closed),
        _ => Ok(json!({})),
    });
    let second_page = r.another_page();
    let mut browsers = ordered(&mut r, silent);
    browsers.setups.push_back(second_page);
    browsers.cases.push_back(case_page(&["275"]));
    let run = run_with(&r, &mut browsers, 14, true).await;

    let case = &run.cases[0];
    assert!(case.retried.is_some(), "{case:?}");
    assert_eq!(case.proposed, "Passed", "{case:?}");
    assert_eq!(
        *browsers.events.lock().unwrap(),
        vec![
            "setup opened",
            "setup closed",
            "case opened, setup account leased: false",
            "case closed",
            "setup opened",
            "setup closed",
            "case opened, setup account leased: false",
            "case closed",
        ]
    );
    assert_eq!(*browsers.checks.lock().unwrap(), vec!["275"], "the second go checks the second draft");
    let made: Vec<(String, Option<i32>)> =
        test_made::list(r.root.path()).into_iter().map(|m| (m.id, m.case_id)).collect();
    assert_eq!(made, vec![("274".to_string(), Some(14)), ("275".to_string(), Some(14))]);
}

/// A transient failure whose retry cannot make a fresh draft: the second
/// go never starts, the first go's record stays, and the reason says the
/// setup failed - never that a browser did not open.
#[tokio::test]
async fn a_retry_whose_setup_fails_says_the_setup_failed() {
    use v2_lib::autorun::transient::RETRY_NOT_STARTED;
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let mut r = setup_rig(vec![the_cycle(), answer(500, json!({ "error": "no" }))]);
    let sc = script(15, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    store::save_script(r.root.path(), &sc).unwrap();
    approve(r.root.path(), &sc);

    let silent = ScriptedDriver::new(|method, _| match method {
        "Runtime.evaluate" => Err(CdpError::Closed),
        _ => Ok(json!({})),
    });
    let second_page = r.another_page();
    let mut browsers = ordered(&mut r, silent);
    browsers.setups.push_back(second_page);
    let run = run_with(&r, &mut browsers, 15, true).await;

    let case = &run.cases[0];
    let first = case.retried.clone().expect("the first try's sentence is kept");
    let not_started = format!("{first}{RETRY_NOT_STARTED}setup failed: step 1: ");
    assert!(case.reason.starts_with(&not_started), "{}", case.reason);
    assert!(case.reason.ends_with(')'), "{}", case.reason);
    assert!(!case.reason.contains("the browser did not open"), "{}", case.reason);
    assert_eq!(case.proposed, "Blocked");
}

/// A failed setup run Blocks the case with the fixture's own sentence, and
/// leaves nothing open.
#[tokio::test]
async fn a_failed_setup_blocks_with_the_fixtures_sentence() {
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let mut r = setup_rig(vec![answer(500, json!({ "error": "no" }))]);
    let sc = script(3, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    approve(r.root.path(), &sc);
    let got = prepare_case_within(
        &mut r.browsers,
        r.root.path(),
        ORG,
        PROJECT,
        &sc,
        &quick(),
        &NO_STOP,
        |_| std::future::ready(()),
        RUN_LIMIT,
        &QUICK_PAUSES,
        CLOCK,
    )
    .await;
    let why = got.unwrap_err();
    assert!(why.starts_with("setup failed: step 1: "), "{why}");
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1));
    let env = v2_lib::environments::active_id(r.root.path()).unwrap();
    assert!(!lease::is_held(&env, ACCOUNT));
}

/// A setup value filled into a step's own locator must have the shape of
/// what was seen there: digits where digits were seen. Otherwise the case
/// is Blocked, naming the step.
#[tokio::test]
async fn a_setup_value_filled_into_a_locator_must_fit_what_was_seen() {
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    for (given, blocked) in [(json!(10071), false), (json!("draft-7"), true)] {
        let mut r = setup_rig(vec![answer(200, json!({ "cycleId": given, "cycleName": "AUTOTEST cycle" }))]);
        let root = r.root.path().to_path_buf();
        let mut sc = script(
            30,
            Some("own"),
            json!([{ "step_number": 1, "actions": [
                { "kind": "click", "selector": { "css": "div[data-cycle-id=\"{{setup.cycle_id}}\"]" } }
            ] }]),
        );
        sc.area = Some("Leave".into());
        store::save_script(&root, &sc).unwrap();
        seen_on_leave(&root, "div[data-cycle-id=\"10066\"]");
        approve(&root, &sc);
        let got = prepare_case_within(
            &mut r.browsers,
            &root,
            ORG,
            PROJECT,
            &sc,
            &quick(),
            &NO_STOP,
            |_| std::future::ready(()),
            RUN_LIMIT,
            &QUICK_PAUSES,
            CLOCK,
        )
        .await
        .map(|p| p.script.steps);
        if blocked {
            assert_eq!(
                got,
                Err("Step 1: div[data-cycle-id=\"draft-7\"], as filled in, does not fit what was seen on the live app".to_string())
            );
        } else {
            let steps = got.expect("a digits id fits");
            assert_eq!(serde_json::to_value(&steps).unwrap()[0]["actions"][0]["selector"]["css"], json!("div[data-cycle-id=\"10071\"]"));
        }
    }
}

// ---- approvals ----------------------------------------------------------------

/// Review Focus 2: an approval counts only for what was approved. A change
/// to the setup, to a fixture step or to a template body clears it; a
/// template being proven again does not.
#[tokio::test]
async fn any_change_to_what_was_approved_clears_the_approval() {
    let _slot = crate::serial::api_template_run();
    let mut r = setup_rig(vec![]);
    let root = r.root.path().to_path_buf();
    let sc = script(4, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    approve(&root, &sc);
    assert!(matches!(state(&root, &sc), Approval::Approved { .. }));

    // A template proven again: still approved.
    let mut t = make_cycle();
    t.proven.as_mut().unwrap().at = "2026-10-06 08:00:00".into();
    template_store::save(&root, ORG, PROJECT, &t).unwrap();
    assert!(matches!(state(&root, &sc), Approval::Approved { .. }), "proven is not part of the fingerprint");

    async fn blocked(r: &mut Rig, sc: &CaseScript) {
        let got = prepare_case(&mut r.browsers, r.root.path(), ORG, PROJECT, sc, &quick(), &NO_STOP).await;
        assert_eq!(got.map(|p| p.script), Err(NOT_APPROVED.to_string()));
        assert_eq!(r.browsers.opened, 0);
    }

    // The setup itself: another fixture.
    let other = CaseScript { setup: Some(Setup { fixture: "shared".into() }), ..sc.clone() };
    assert_eq!(state(&root, &other), Approval::Changed);
    blocked(&mut r, &other).await;

    // A fixture step.
    let mut f = fixture("own");
    f.steps[0].params.insert("cycleName".into(), "{{prefix}} another cycle".into());
    fixture_store::save(&root, ORG, PROJECT, &f).unwrap();
    assert_eq!(state(&root, &sc), Approval::Changed);
    blocked(&mut r, &sc).await;

    // Approved again, then a template body.
    approve(&root, &sc);
    assert!(matches!(state(&root, &sc), Approval::Approved { .. }));
    let mut t = make_cycle();
    t.steps[0].path = "/hr/pmsv10/othercycle".into();
    template_store::save(&root, ORG, PROJECT, &t).unwrap();
    assert_eq!(state(&root, &sc), Approval::Changed);
    blocked(&mut r, &sc).await;

}

/// Review Focus 2, one part at a time: each of the fixture's params,
/// account, outputs and creates is in the fingerprint on its own.
#[test]
fn each_part_of_the_fixture_is_in_the_fingerprint() {
    let r = setup_rig(vec![]);
    let root = r.root.path();
    let sc = script(15, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    let changes: [(&str, fn(&mut Fixture)); 4] = [
        ("params", |f| {
            f.steps[0].params.insert("cycleName".into(), "{{prefix}} another cycle".into());
        }),
        ("account", |f| f.account = "admin".into()),
        ("outputs", |f| {
            f.outputs.insert("cycle_name".into(), "{{steps.1.cycleName}}".into());
        }),
        ("creates", |f| f.creates[0].kind = "draft cycle".into()),
    ];
    for (what, change) in changes {
        fixture_store::save(root, ORG, PROJECT, &fixture("own")).unwrap();
        approve(root, &sc);
        assert!(matches!(state(root, &sc), Approval::Approved { .. }), "{what}");
        let mut f = fixture("own");
        change(&mut f);
        fixture_store::save(root, ORG, PROJECT, &f).unwrap();
        assert_eq!(state(root, &sc), Approval::Changed, "{what}");
    }
}

/// Approve setup signs what the person was shown: a setup that changed
/// after they looked is refused, and nothing is approved.
#[tokio::test]
async fn an_approval_of_a_setup_that_changed_since_it_was_shown_is_refused() {
    let _slot = crate::serial::api_template_run();
    let mut r = setup_rig(vec![]);
    let root = r.root.path().to_path_buf();
    let sc = script(16, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    let shown = setup::view(&root, ORG, PROJECT, &sc).unwrap().unwrap();
    assert_eq!(setup::approval_target(&root, ORG, PROJECT, &sc, &shown.fingerprint), Ok(shown.fingerprint.clone()));

    let mut f = fixture("own");
    f.steps[0].params.insert("cycleName".into(), "{{prefix}} another cycle".into());
    fixture_store::save(&root, ORG, PROJECT, &f).unwrap();
    assert_eq!(
        setup::approval_target(&root, ORG, PROJECT, &sc, &shown.fingerprint),
        Err("the setup changed while you were looking at it - review it again before approving".to_string())
    );
    assert_eq!(setup::CHANGED_WHILE_LOOKING, "the setup changed while you were looking at it - review it again before approving");
    assert_eq!(state(&root, &sc), Approval::None);
    let got = prepare_case(&mut r.browsers, &root, ORG, PROJECT, &sc, &quick(), &NO_STOP).await;
    assert_eq!(got.map(|p| p.script), Err(NOT_APPROVED.to_string()));
    assert_eq!(r.browsers.opened, 0);
}

#[tokio::test]
async fn a_withdrawn_approval_blocks_the_case_again() {
    let _slot = crate::serial::api_template_run();
    let mut r = setup_rig(vec![]);
    let root = r.root.path().to_path_buf();
    let sc = script(6, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    assert_eq!(state(&root, &sc), Approval::None);
    approve(&root, &sc);
    assert!(root.join("approvals").join("6.json").is_file(), "kept in the Auto Run store by case id");
    approvals::withdraw(&root, 6).unwrap();
    assert_eq!(state(&root, &sc), Approval::None);
    approvals::withdraw(&root, 6).unwrap();
    let got = prepare_case(&mut r.browsers, &root, ORG, PROJECT, &sc, &quick(), &NO_STOP).await;
    assert_eq!(got.map(|p| p.script), Err(NOT_APPROVED.to_string()));
    assert_eq!(r.browsers.opened, 0);
}

#[test]
fn the_fingerprint_is_sha256_hex_and_ignores_key_order() {
    let f = fixture("own");
    let s = Setup { fixture: "own".into() };
    let fp = approvals::fingerprint(&s, &f, &[Some(make_cycle())]);
    assert_eq!(fp.len(), 64);
    assert!(fp.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{fp}");
    // The same template with its request's keys written in another order.
    let with_form = |form: Value| {
        let mut v = serde_json::to_value(make_cycle()).unwrap();
        v["steps"][0]["form"] = form;
        serde_json::from_value::<ApiTemplate>(v).unwrap()
    };
    let ab = with_form(json!({ "A": "1", "B": "{{cycleName}}" }));
    let ba = with_form(json!({ "B": "{{cycleName}}", "A": "1" }));
    assert_eq!(
        approvals::fingerprint(&s, &f, &[Some(ab.clone())]),
        approvals::fingerprint(&s, &f, &[Some(ba)])
    );
    assert_ne!(approvals::fingerprint(&s, &f, &[Some(ab)]), fp);
    assert_ne!(approvals::fingerprint(&s, &f, &[None]), fp, "a template no longer saved changes it");
}

#[test]
fn the_editor_view_says_what_the_setup_does_and_where_its_approval_stands() {
    let r = setup_rig(vec![]);
    let root = r.root.path();
    let sc = script(8, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    let v = setup::view(root, ORG, PROJECT, &sc).unwrap().unwrap();
    assert_eq!(v.fixture_name, "Fixture own");
    assert_eq!(v.account, ACCOUNT);
    assert_eq!(v.steps, vec!["Make a cycle: cycleName = {{prefix}} cycle"]);
    assert_eq!(v.creates, vec!["cycle {{steps.1.cycleName}}"]);
    assert_eq!((v.approval.as_str(), v.approved_at.clone()), ("none", None));
    approve(root, &sc);
    let v = setup::view(root, ORG, PROJECT, &sc).unwrap().unwrap();
    assert_eq!(v.approval, "approved");
    assert!(v.approved_at.is_some());
    assert_eq!(setup::view(root, ORG, PROJECT, &script(8, None, json!([]))).unwrap(), None);
}

// ---- the replay to a step ------------------------------------------------------

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

/// Part 4's replay to step N runs the setup too, before anything signs in,
/// and keeps what it gave for the steps the person runs next.
#[tokio::test]
async fn a_replay_to_a_step_runs_the_setup_first() {
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let mut r = setup_rig(vec![the_cycle()]);
    let root = r.root.path().to_path_buf();
    save_nav(&root, ORG, PROJECT, &NavFile { direct_urls: false, modules: vec![leave_area()], save_words: vec![] })
        .unwrap();
    let mut sc = script(
        9,
        Some("own"),
        json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "css": "#c{{setup.cycle_id}}" } }] },
            { "step_number": 2, "actions": [check("{{setup.cycle_id}}")] }
        ]),
    );
    sc.area = Some("Leave".into());
    store::save_script(&root, &sc).unwrap();
    seen_on_leave(&root, "#c274");

    let (mut d, app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    let req = ReplayRequest { case_id: 9, step: 2, db_read_access: false };
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let mut held = Held::supervised();

    // What an earlier start gave, which a Blocked replay must not leave.
    setup::remember(9, [("cycle_id".to_string(), json!(1))].into());

    // Not approved: Blocked, nothing opened, the case's browser untouched.
    let end = replay_to_checked(
        &mut d,
        &mut r.browsers,
        &root,
        ORG,
        PROJECT,
        &req,
        &mut account,
        &mut held,
        &mut guarded,
        true,
        &quick(),
        &cancel,
        || -> PreconditionDb<NoDb> { PreconditionDb::ReadingOff },
        |_, _| {},
    )
    .await;
    assert_eq!(end, ReplayEnd::Blocked(NOT_APPROVED.into()));
    assert_eq!(r.browsers.opened, 0);
    assert!(d.calls.is_empty(), "{:?}", d.methods());
    let step2 = store::load_script(&root, 9).unwrap().unwrap().steps[1].clone();
    assert_eq!(
        setup::resolve_step(&root, ORG, PROJECT, 9, &step2),
        Err("setup gave no cycle_id - start the case again".to_string()),
        "a Blocked start leaves nothing stale"
    );

    approve(&root, &sc);
    let end = replay_to_checked(
        &mut d,
        &mut r.browsers,
        &root,
        ORG,
        PROJECT,
        &req,
        &mut account,
        &mut held,
        &mut guarded,
        true,
        &quick(),
        &cancel,
        || -> PreconditionDb<NoDb> { PreconditionDb::ReadingOff },
        |_, _| {},
    )
    .await;
    assert!(matches!(end, ReplayEnd::Ready { case_id: 9, step: 2, .. }), "{end:?}");
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1));
    assert!(app.log.lock().unwrap().contains(&"click #c274".to_string()), "{:?}", app.log.lock().unwrap());
    assert_eq!(test_made::list(&root)[0].case_id, Some(9));

    // Step 2, run by the person next, gets what the setup gave.
    let filled = setup::resolve_step(&root, ORG, PROJECT, 9, &step2).unwrap();
    assert_eq!(serde_json::to_value(&filled.actions).unwrap()[0]["value"], json!("274"));
    // A case whose setup never ran here has no value to give.
    assert_eq!(
        setup::resolve_step(&root, ORG, PROJECT, 9_999, &step2),
        Err(setup::start_again("cycle_id"))
    );
}

/// The supervised browser signed in as the account the setup runs as: its
/// session is ended and its lease let go, the setup signs in as that
/// account, and the case's own sign-in takes the lease again.
#[tokio::test]
async fn a_setup_as_the_supervised_browsers_account_runs_and_the_case_signs_in_after() {
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let _leases = crate::serial::account_leases();
    let mut r = setup_rig(vec![the_cycle()]);
    let root = r.root.path().to_path_buf();
    save_nav(&root, ORG, PROJECT, &NavFile { direct_urls: false, modules: vec![leave_area()], save_words: vec![] })
        .unwrap();
    let mut as_admin = fixture("own");
    as_admin.account = "admin".into();
    fixture_store::save(&root, ORG, PROJECT, &as_admin).unwrap();
    let mut sc = script(17, Some("own"), json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": { "css": "#c{{setup.cycle_id}}" } }] },
        { "step_number": 2, "actions": [check("{{setup.cycle_id}}")] }
    ]));
    sc.account = Some("admin".into());
    sc.area = Some("Leave".into());
    store::save_script(&root, &sc).unwrap();
    seen_on_leave(&root, "#c274");
    approve(&root, &sc);

    // The supervised browser holds admin.
    let mut held = Held::supervised();
    held.hold(&root, "admin").await.unwrap();
    let mut account = Some("admin".to_string());
    let env = v2_lib::environments::active_id(&root).unwrap();
    assert!(lease::is_held(&env, "admin"));

    let (mut d, app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    let req = ReplayRequest { case_id: 17, step: 2, db_read_access: false };
    let (mut guarded, cancel) = (None, AtomicBool::new(false));
    let end = replay_to_checked(
        &mut d,
        &mut r.browsers,
        &root,
        ORG,
        PROJECT,
        &req,
        &mut account,
        &mut held,
        &mut guarded,
        true,
        &quick(),
        &cancel,
        || -> PreconditionDb<NoDb> { PreconditionDb::ReadingOff },
        |_, _| {},
    )
    .await;
    assert!(matches!(end, ReplayEnd::Ready { case_id: 17, step: 2, .. }), "{end:?}");
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1), "the setup ran");
    assert_eq!(r.sign_ins(), 1, "the setup signed in as admin");
    assert_eq!(held.account(), Some("admin"), "the case's sign-in took the lease again");
    assert_eq!(account.as_deref(), Some("admin"));
    assert!(app.log.lock().unwrap().contains(&"click #c274".to_string()), "{:?}", app.log.lock().unwrap());
}

/// A supervised browser signed in as another account is left alone.
#[tokio::test]
async fn a_supervised_browser_on_another_account_is_left_alone() {
    let _leases = crate::serial::account_leases();
    let r = setup_rig(vec![]);
    let root = r.root.path();
    let mut held = Held::supervised();
    held.hold(root, "admin").await.unwrap();
    let mut account = Some("admin".to_string());
    let mut d = common::FakePage::default().driver();
    setup::make_way(&mut d, &mut account, &mut held, ACCOUNT, &quick()).await;
    assert_eq!(held.account(), Some("admin"));
    assert_eq!(account.as_deref(), Some("admin"));
    assert!(d.calls.is_empty(), "{:?}", d.methods());
}

// ---- old scripts and repairs ---------------------------------------------------

#[test]
fn an_old_script_round_trips_byte_identical() {
    let old = r#"{
  "case_id": 12,
  "title": "Old",
  "account": "admin",
  "steps": [
    {
      "step_number": 1,
      "actions": [
        {
          "kind": "check_text",
          "value": "ok"
        }
      ]
    }
  ],
  "preconditions": [
    {
      "flow": "cycle",
      "stage": "published",
      "value": 5
    }
  ]
}"#;
    let sc: CaseScript = serde_json::from_str(old).unwrap();
    assert_eq!(sc.setup, None);
    assert_eq!(serde_json::to_string_pretty(&sc).unwrap(), old);

    let with = CaseScript { setup: Some(Setup { fixture: "own".into() }), ..sc };
    let text = serde_json::to_string(&with).unwrap();
    assert!(text.contains(r#""setup":{"fixture":"own"}"#), "{text}");
    assert_eq!(serde_json::from_str::<CaseScript>(&text).unwrap(), with);
}

/// A setup writes data, so a repair may not add, change or remove one.
#[test]
fn a_repair_cannot_touch_the_setup() {
    let steps = json!([{ "step_number": 1, "actions": [check("ok")] }]);
    let none = script(13, None, steps.clone());
    let own = script(13, Some("own"), steps.clone());
    let shared = script(13, Some("shared"), steps);
    for (old, new) in [(&none, &own), (&own, &shared), (&own, &none)] {
        assert_eq!(check_edits(old, new, None), Err(SETUP_KEPT.to_string()));
    }
    assert_eq!(check_edits(&own, &own, None), Ok(()));
}

/// Only the webview's three setup commands write an approval: nothing
/// else in the crate calls `approvals::approve` or `approvals::withdraw`.
#[test]
fn only_the_setup_commands_approve_or_withdraw() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut callers = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap().replace("\r\n", "\n");
                if text.contains("approvals::approve(") || text.contains("approvals::withdraw(") {
                    callers.push(p.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    assert_eq!(callers, vec!["commands/autorun_setup.rs"]);
    let commands = std::fs::read_to_string(src.join("commands/autorun_setup.rs")).unwrap().replace("\r\n", "\n");
    for name in ["fn auto_run_setup_view(", "fn auto_run_approve_setup(", "fn auto_run_withdraw_setup("] {
        assert!(commands.contains(name), "{name}");
    }
}

/// A case whose setup runs as `admin`, the account the supervised browser
/// holds, in the Leave area; with `steps` as given.
fn admin_case(root: &std::path::Path, case_id: i32, steps: Value) -> CaseScript {
    save_nav(root, ORG, PROJECT, &NavFile { direct_urls: false, modules: vec![leave_area()], save_words: vec![] })
        .unwrap();
    let mut as_admin = fixture("own");
    as_admin.account = "admin".into();
    fixture_store::save(root, ORG, PROJECT, &as_admin).unwrap();
    let mut sc = script(case_id, Some("own"), steps);
    sc.account = Some("admin".into());
    sc.area = Some("Leave".into());
    store::save_script(root, &sc).unwrap();
    sc
}

/// Replays `sc` to step 2 with the supervised browser signed in as admin;
/// gives the end, and the account and lease the browser has afterwards.
async fn replay_signed_in(r: &mut Rig, sc: &CaseScript) -> (ReplayEnd, Option<String>, Held, ScriptedDriver) {
    let root = r.root.path().to_path_buf();
    let mut held = Held::supervised();
    held.hold(&root, "admin").await.unwrap();
    let mut account = Some("admin".to_string());
    let (mut d, _app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    let req = ReplayRequest { case_id: sc.case_id, step: 2, db_read_access: false };
    let (mut guarded, cancel) = (None, AtomicBool::new(false));
    let end = replay_to_checked(
        &mut d,
        &mut r.browsers,
        &root,
        ORG,
        PROJECT,
        &req,
        &mut account,
        &mut held,
        &mut guarded,
        true,
        &quick(),
        &cancel,
        || -> PreconditionDb<NoDb> { PreconditionDb::ReadingOff },
        |_, _| {},
    )
    .await;
    (end, account, held, d)
}

/// The person is signed out only when the setup will run: a shared
/// fixture never built Blocks the case with the browser still signed in.
#[tokio::test]
async fn an_unbuilt_shared_fixture_blocks_with_the_supervised_browser_still_signed_in() {
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let _leases = crate::serial::account_leases();
    let mut r = setup_rig(vec![the_cycle()]);
    let sc = admin_case(
        r.root.path(),
        19,
        json!([
            { "step_number": 1, "actions": [check("{{fixture.shared.cycle_id}}")] },
            { "step_number": 2, "actions": [check("{{setup.cycle_id}}")] }
        ]),
    );
    approve(r.root.path(), &sc);
    let (end, account, held, d) = replay_signed_in(&mut r, &sc).await;
    assert_eq!(end, ReplayEnd::Blocked(not_built("Fixture shared")));
    assert_eq!((account.as_deref(), held.account()), (Some("admin"), Some("admin")));
    assert!(d.calls.is_empty(), "the browser's session was not ended: {:?}", d.methods());
    assert_eq!(r.browsers.opened, 0);
}

/// A setup that is not approved leaves the supervised browser signed in,
/// its lease held.
#[tokio::test]
async fn a_setup_not_approved_leaves_the_supervised_browser_signed_in() {
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let _leases = crate::serial::account_leases();
    let mut r = setup_rig(vec![the_cycle()]);
    let sc = admin_case(
        r.root.path(),
        21,
        json!([
            { "step_number": 1, "actions": [check("{{setup.cycle_id}}")] },
            { "step_number": 2, "actions": [check("{{setup.cycle_id}}")] }
        ]),
    );
    let (end, account, held, d) = replay_signed_in(&mut r, &sc).await;
    assert_eq!(end, ReplayEnd::Blocked(NOT_APPROVED.into()));
    assert_eq!((account.as_deref(), held.account()), (Some("admin"), Some("admin")));
    let env = v2_lib::environments::active_id(r.root.path()).unwrap();
    assert!(lease::is_held(&env, "admin"));
    assert!(d.calls.is_empty(), "{:?}", d.methods());
    assert_eq!(r.browsers.opened, 0);
}

/// A supervised start whose setup's step hangs: case 22, approved.
fn hanging_setup() -> Rig {
    let r = setup_rig(vec![]);
    r.script.lock().unwrap().hang_after = Some(0);
    let sc = script(22, Some("own"), json!([{ "step_number": 1, "actions": [check("{{setup.cycle_id}}")] }]));
    store::save_script(r.root.path(), &sc).unwrap();
    approve(r.root.path(), &sc);
    r
}

/// The step limit these tests give the setup: a short hang, never the
/// real 3 minutes.
const SHORT_STEP: std::time::Duration = std::time::Duration::from_millis(800);

/// Close answers at once while a setup runs, and stops it: the case is
/// Blocked with the stop's sentence.
#[tokio::test]
async fn close_answers_at_once_and_stops_a_running_setup() {
    use v2_lib::commands::autorun::{auto_run_close_browser, supervised_setup};
    let _claims = crate::serial::autorun();
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let mut r = hanging_setup();
    let root = r.root.path().to_path_buf();
    let timing = quick();
    let began = Instant::now();
    let (blocked, closed_in) = tokio::join!(
        supervised_setup(&mut r.browsers, &root, ORG, PROJECT, 22, &timing, SHORT_STEP, &QUICK_PAUSES),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let at = Instant::now();
            tokio::time::timeout(std::time::Duration::from_millis(300), auto_run_close_browser())
                .await
                .expect("Close answered while the setup ran")
                .unwrap();
            at.elapsed()
        }
    );
    assert!(closed_in < std::time::Duration::from_millis(300), "{closed_in:?}");
    assert_eq!(blocked.as_deref(), Some("setup failed: the run was stopped"));
    assert!(began.elapsed() < std::time::Duration::from_secs(5), "{:?}", began.elapsed());
    assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1), "the setup's browser is closed");
}

/// Whether the supervised browser is open - what an unattended Start asks
/// first - is answered while a setup runs.
#[tokio::test]
async fn the_supervised_browser_question_is_answered_during_a_setup() {
    use v2_lib::commands::autorun::{supervised_session_is_open, supervised_setup};
    let _claims = crate::serial::autorun();
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let mut r = hanging_setup();
    let root = r.root.path().to_path_buf();
    let timing = quick();
    let (blocked, open) = tokio::join!(
        supervised_setup(&mut r.browsers, &root, ORG, PROJECT, 22, &timing, SHORT_STEP, &QUICK_PAUSES),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            tokio::time::timeout(std::time::Duration::from_millis(300), supervised_session_is_open())
                .await
                .expect("answered while the setup ran")
        }
    );
    assert!(!open);
    // Nobody stopped it: the hanging step ran into its limit.
    let why = blocked.unwrap();
    assert!(why.starts_with("setup failed: step 1: "), "{why}");
}
