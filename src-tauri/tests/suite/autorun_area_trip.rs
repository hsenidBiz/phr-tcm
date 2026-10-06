//! `return_to_area` with an `area`: a case goes to another recorded area by
//! its own menu path partway through, and a bare `return_to_area` brings it
//! back to the case's own. The browser is a fake (`common::menu_app`).

use crate::common::{self, account, menu_recipe, FakeStageDb};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use v2_lib::ai_bridge::try_in;
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::lease::{Held, Holder};
use v2_lib::autorun::nav::{self, check_areas, check_project_rules, save_nav, ModulePath, NavFile, Route};
use v2_lib::autorun::preconditions::PreconditionDb;
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::replay_to::{replay_to_checked, ReplayEnd, ReplayRequest};
use v2_lib::autorun::runner::{
    area_routes, named_areas, run_step_in_run, AreaRoute, InRun, NamedAreas, AFTER_NAMED_AREA_UNREACHED,
};
use v2_lib::autorun::{store, CaseScript, StepScript};
use v2_lib::browser::actions::Action;
use v2_lib::browser::timing::Timing;

const ORG: &str = "acme";
const PROJECT: &str = "Web";

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 150, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn area(module: &str, name: &str, arrived: &str) -> ModulePath {
    serde_json::from_value(json!({
        "module": module,
        "area": name,
        "clicks": [ { "role": "link", "name": name, "exact": true } ],
        "arrived": arrived,
        "recorded": "2026-10-05T10:00:00Z"
    }))
    .unwrap()
}

fn leave() -> ModulePath {
    area("Leave", "Leave", "/hr/leave")
}

fn configurator() -> ModulePath {
    area("Settings", "Common Configurator", "/hr/config")
}

fn record(root: &Path, areas: Vec<ModulePath>) {
    save_nav(root, ORG, PROJECT, &NavFile { direct_urls: false, modules: areas, save_words: vec![] }).unwrap();
}

/// The project with both areas recorded and the menu app's sign-in.
fn project(root: &Path) {
    record(root, vec![leave(), configurator()]);
    save_recipe(root, ORG, PROJECT, &menu_recipe()).unwrap();
}

fn menu() -> (common::ScriptedDriver, common::MenuApp) {
    common::menu_app(
        &[("link", "Leave", "/hr/leave"), ("link", "Common Configurator", "/hr/config")],
        "/hr/home/index",
        0,
    )
}

fn log(app: &common::MenuApp) -> Vec<String> {
    app.log.lock().unwrap().clone()
}

fn action(v: Value) -> Action {
    serde_json::from_value(v).expect("an action")
}

fn step(n: i32, actions: Value) -> StepScript {
    serde_json::from_value(json!({ "step_number": n, "actions": actions })).unwrap()
}

/// The owner's example: to the configurator, a look, and back.
fn there_and_back() -> Value {
    json!([
        { "kind": "return_to_area", "area": "Common Configurator" },
        { "kind": "check_text", "value": "yes" },
        { "kind": "return_to_area" }
    ])
}

fn script(case_id: i32, steps: Value) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": case_id,
        "title": "Goal groups follow the configurator's limit",
        "account": "admin",
        "area": "Leave",
        "steps": steps
    }))
    .unwrap()
}

// ---- What a script says ------------------------------------------------------

#[test]
fn a_bare_return_to_area_round_trips_byte_identical() {
    let bare = r#"{"kind":"return_to_area"}"#;
    let a: Action = serde_json::from_str(bare).unwrap();
    assert_eq!(a, Action::ReturnToArea { area: None });
    assert_eq!(serde_json::to_string(&a).unwrap(), bare);
    assert_eq!(a.area_named(), None);

    let named = r#"{"kind":"return_to_area","area":"Common Configurator"}"#;
    let a: Action = serde_json::from_str(named).unwrap();
    assert_eq!(serde_json::to_string(&a).unwrap(), named);
    assert_eq!(a.area_named(), Some("Common Configurator"));
    assert!(a.validate().is_ok());

    // A blank name is the case's own area, as a script's blank `area` is.
    assert_eq!(action(json!({ "kind": "return_to_area", "area": "  " })).area_named(), None);

    // A whole saved script that uses the bare kind writes it back the same.
    let s = script(5, json!([ { "step_number": 1, "actions": [ { "kind": "return_to_area" } ] } ]));
    let written = serde_json::to_string(&s.steps[0].actions[0]).unwrap();
    assert_eq!(written, bare);
}

#[test]
fn a_script_naming_an_unrecorded_area_is_refused_with_the_areas_sentence() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let s = script(
        5,
        json!([ { "step_number": 1, "actions": [ { "kind": "return_to_area", "area": "Payroll Setup" } ] } ]),
    );
    let why = check_project_rules(dir.path(), ORG, PROJECT, &[s]).unwrap_err();
    assert!(why.starts_with("case 5: "), "{why}");
    assert!(why.contains(&nav::unrecorded_area("Payroll Setup")), "{why}");
    assert!(why.contains("recorded areas: Leave, Common Configurator"), "{why}");

    // Matched as the script's own area is: trimmed, case ignored.
    let s = script(
        6,
        json!([ { "step_number": 1, "actions": [ { "kind": "return_to_area", "area": " common configurator " } ] } ]),
    );
    check_project_rules(dir.path(), ORG, PROJECT, &[s]).unwrap();
    // A bare one names nothing to check.
    let s = script(7, json!([ { "step_number": 1, "actions": [ { "kind": "return_to_area" } ] } ]));
    check_project_rules(dir.path(), ORG, PROJECT, &[s]).unwrap();
}

#[test]
fn a_return_to_area_inside_a_when_visible_is_resolved_and_validated() {
    let guarded = json!([ { "step_number": 1, "actions": [
        { "kind": "when_visible", "selector": { "css": "#banner" },
          "then": [ { "kind": "return_to_area", "area": "Payroll Setup" } ] },
        { "kind": "return_to_area", "area": "Common Configurator" },
        { "kind": "return_to_area", "area": "COMMON CONFIGURATOR" }
    ] } ]);
    let s = script(8, guarded);
    let actions: Vec<&Action> = s.steps.iter().flat_map(|st| st.actions.iter()).collect();
    // Every name, nested included, once each.
    assert_eq!(named_areas(actions.iter().copied()), vec!["Payroll Setup", "Common Configurator"]);

    let nav_file = NavFile { direct_urls: false, modules: vec![leave(), configurator()], save_words: vec![] };
    let why = check_areas(&nav_file, &[s]).unwrap_err();
    assert!(why.contains(&nav::unrecorded_area("Payroll Setup")), "{why}");

    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let routes = area_routes(dir.path(), ORG, PROJECT, &["Payroll Setup", "Common Configurator"]);
    assert_eq!(routes[&nav::module_key("Payroll Setup")], Err(nav::unrecorded_area("Payroll Setup")));
    assert_eq!(routes[&nav::module_key("Common Configurator")].as_ref().unwrap().path.name(), "Common Configurator");
}

#[test]
fn the_report_and_the_patterns_name_the_area() {
    let named = action(json!({ "kind": "return_to_area", "area": "Common Configurator" }));
    let bare = action(json!({ "kind": "return_to_area" }));
    assert_eq!(v2_lib::autorun::report::action_words(&named), "go to the Common Configurator area");
    assert_eq!(v2_lib::autorun::report::action_words(&bare), "go back to the case's area");
    assert_eq!(v2_lib::autorun::patterns::action_target(&named).as_deref(), Some("the Common Configurator area"));
    assert_eq!(v2_lib::autorun::patterns::action_target(&bare).as_deref(), Some("the case's area"));
}

// ---- The runner -------------------------------------------------------------

/// One step through `run_step_in_run`, with the case's own route and the
/// named areas resolved from the project as it is on disk.
async fn run_one(
    d: &mut common::ScriptedDriver,
    root: &Path,
    s: &StepScript,
    own: &Route,
    areas: Option<&NamedAreas>,
) -> Vec<v2_lib::browser::actions::ActionOutcome> {
    let mut account = Some("admin".to_string());
    let mut held = Held::supervised();
    let mut run = InRun { areas, ..Default::default() };
    run_step_in_run(d, root, ORG, PROJECT, s, &quick(), &mut account, &mut held, None, AreaRoute::To(own), &mut run)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_named_area_takes_its_own_path_and_a_bare_one_the_cases() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let (mut d, app) = menu();
    let own = Route::new(&menu_recipe(), leave());
    let s = step(4, there_and_back());
    let names = named_areas(&s.actions);
    let areas = area_routes(dir.path(), ORG, PROJECT, &names);
    let out = run_one(&mut d, dir.path(), &s, &own, Some(&areas)).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    let seen = log(&app);
    let at = |what: &str| seen.iter().rposition(|l| l == what).unwrap_or_else(|| panic!("no {what:?} in {seen:?}"));
    assert!(at("click Common Configurator") < at("check yes"), "{seen:?}");
    assert!(at("check yes") < at("click Leave"), "went back to the case's own area last: {seen:?}");
    assert_eq!(*app.path.lock().unwrap(), "/hr/leave");
}

#[tokio::test]
async fn an_area_removed_since_the_save_blocks_the_rest_of_the_step() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let s = step(4, json!([
        { "kind": "return_to_area", "area": "Common Configurator" },
        { "kind": "click", "selector": { "css": "#save" } }
    ]));
    // Recorded when the script was saved...
    let saved = CaseScript { steps: vec![s.clone()], ..script(9, json!([])) };
    check_project_rules(dir.path(), ORG, PROJECT, std::slice::from_ref(&saved)).unwrap();
    // ...and removed before the run.
    record(dir.path(), vec![leave()]);

    let (mut d, app) = menu();
    let own = Route::new(&menu_recipe(), leave());
    let areas = area_routes(dir.path(), ORG, PROJECT, &named_areas(&s.actions));
    let out = run_one(&mut d, dir.path(), &s, &own, Some(&areas)).await;
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, format!("return_to_area: {}", nav::unrecorded_area("Common Configurator")));
    assert_eq!(out[1].detail, AFTER_NAMED_AREA_UNREACHED, "the click ran on the wrong screen: {out:?}");
    assert!(!log(&app).iter().any(|l| l == "click #save"), "{:?}", log(&app));

    // With no areas read at all, the same.
    let (mut d, _app) = menu();
    let out = run_one(&mut d, dir.path(), &s, &own, None).await;
    assert_eq!(out[0].detail, format!("return_to_area: {}", nav::unrecorded_area("Common Configurator")));
    assert_eq!(out[1].detail, AFTER_NAMED_AREA_UNREACHED);
}

#[tokio::test]
async fn an_unattended_run_reads_the_named_areas_before_step_1() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let s = CaseScript {
        account: None,
        ..script(10, json!([
            { "step_number": 1, "actions": there_and_back() },
            { "step_number": 2, "actions": [
                { "kind": "return_to_area", "area": "Payroll Setup" },
                { "kind": "click", "selector": { "css": "#s2" } }
            ] }
        ]))
    };
    let (mut d, app) = menu();
    let route = Route::new(&menu_recipe(), leave());
    let mut lease = Held::new(Holder::Case { run: String::new() }, quick().lease_wait());
    let cancel = AtomicBool::new(false);
    let record = v2_lib::autorun::replay::run_case_as(
        &mut d, dir.path(), ORG, PROJECT, &mut lease, &s, None, Some(&route), &quick(), &cancel, &mut |_| {},
    )
    .await;
    let step = |n: i32| record.steps.iter().find(|r| r.step_number == n).unwrap_or_else(|| panic!("no step {n}"));
    assert!(step(1).outcomes.iter().all(|o| o.ok), "{:?}", step(1).outcomes);
    let seen = log(&app);
    let at = |what: &str| seen.iter().rposition(|l| l == what).unwrap_or_else(|| panic!("no {what:?} in {seen:?}"));
    assert!(at("click Common Configurator") < at("check yes") && at("check yes") < at("click Leave"), "{seen:?}");
    assert_eq!(step(2).outcomes[0].detail, format!("return_to_area: {}", nav::unrecorded_area("Payroll Setup")));
    assert_eq!(step(2).outcomes[1].detail, AFTER_NAMED_AREA_UNREACHED);
}

// ---- Watched runs, tries and replays ------------------------------------------

#[tokio::test]
async fn a_tried_return_to_area_goes_to_the_area_it_names() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    // The case's own area is Leave; the try names the configurator.
    store::save_script(dir.path(), &script(11, json!([]))).unwrap();
    let (mut d, app) = menu();
    let mut account = None;
    let mut lease = Held::supervised();
    let named = action(json!({ "kind": "return_to_area", "area": "Common Configurator" }));
    let (status, text) = try_in(&mut d, &mut account, &mut lease, dir.path(), ORG, PROJECT, 11, &named).await;
    assert_eq!(status, 200);
    assert!(text.starts_with("ok:"), "{text}");
    assert!(log(&app).contains(&"click Common Configurator".to_string()), "{:?}", log(&app));
    assert!(!log(&app).contains(&"click Leave".to_string()), "{:?}", log(&app));
    assert_eq!(*app.path.lock().unwrap(), "/hr/config");

    // A bare one goes to the case's own area.
    let bare = action(json!({ "kind": "return_to_area" }));
    let (status, text) = try_in(&mut d, &mut account, &mut lease, dir.path(), ORG, PROJECT, 11, &bare).await;
    assert_eq!(status, 200);
    assert!(text.starts_with("ok:"), "{text}");
    assert_eq!(*app.path.lock().unwrap(), "/hr/leave");

    // A named area a case with no saved script can still reach: only a bare
    // one needs the script's own area.
    let (mut d, app) = menu();
    let (status, text) = try_in(&mut d, &mut account, &mut lease, dir.path(), ORG, PROJECT, 999, &named).await;
    assert_eq!(status, 200);
    assert!(text.starts_with("ok:"), "{text}");
    assert_eq!(*app.path.lock().unwrap(), "/hr/config");
}

#[test]
fn a_watched_step_reads_only_the_areas_it_names() {
    // What `auto_run_step` builds for the step the pane sends.
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let s = step(1, there_and_back());
    let areas = area_routes(dir.path(), ORG, PROJECT, &named_areas(&s.actions));
    assert_eq!(areas.len(), 1);
    let rt = areas[&nav::module_key("Common Configurator")].as_ref().unwrap();
    assert_eq!(rt.path.name(), "Common Configurator");
    assert_eq!(rt.home.start_url, menu_recipe().start_url);
    // A step that names none reads nothing.
    let bare = step(1, json!([ { "kind": "return_to_area" } ]));
    assert!(area_routes(dir.path(), ORG, PROJECT, &named_areas(&bare.actions)).is_empty());
}

#[tokio::test]
async fn a_replay_to_a_step_goes_to_the_named_area_and_back() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    save_accounts(dir.path(), &[account()]).unwrap();
    let s = script(
        12,
        json!([
            { "step_number": 1, "actions": there_and_back() },
            { "step_number": 2, "actions": [ { "kind": "click", "selector": { "css": "#s2" } } ] }
        ]),
    );
    store::save_script(dir.path(), &s).unwrap();
    let (mut d, app) = menu();
    let (mut account, mut guarded, cancel) = (None, None, AtomicBool::new(false));
    let mut held = Held::supervised();
    let end = replay_to_checked(
        &mut d,
        &mut v2_lib::autorun::setup::NoBrowsers::<common::ScriptedDriver>::default(),
        dir.path(),
        ORG,
        PROJECT,
        &ReplayRequest { case_id: 12, step: 2, db_read_access: false },
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
    assert_eq!(end, ReplayEnd::Ready { case_id: 12, step: 2, notice: None });
    let seen = log(&app);
    let at = |what: &str| seen.iter().rposition(|l| l == what).unwrap_or_else(|| panic!("no {what:?} in {seen:?}"));
    assert!(at("click Common Configurator") < at("check yes") && at("check yes") < at("click Leave"), "{seen:?}");
    assert!(!seen.contains(&"click #s2".to_string()), "{seen:?}");
}
