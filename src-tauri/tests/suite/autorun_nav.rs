//! Module paths: the per-project file an unattended run reads to get from
//! the home page to a case's module screen, and the rules around it.

use crate::common;

use serde_json::json;
use v2_lib::autorun::nav::{
    area_changed, area_taken_meanwhile, check_areas, check_no_addresses, find_area, find_path, go_home, go_to_module,
    guide_section, is_setup_problem, load_nav, module_key, nav_path, no_address, no_default_area, no_path, path_of, put_path,
    put_path_if, remove_path, route_for, same_page, save_nav, set_direct_urls, view, AreaExpect, AssistantRepair, MadeBy, ModulePath,
    NavFile, PathFailure, Route, TripFrom, Where, NO_ACCOUNT, NO_MODULE,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::area_repair::{check_reason, put_old_path_back, save_repair, MAX_REASON};
use v2_lib::autorun::recipe::project_slug;
use v2_lib::browser::actions::HIGHLIGHT_JS;
use v2_lib::browser::cdp::{CdpError, Event};
use v2_lib::browser::input::{HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;

fn path(module: &str, arrived: &str) -> ModulePath {
    serde_json::from_value(json!({
        "module": module,
        "clicks": [
            { "role": "link", "name": "Leave", "exact": true },
            { "role": "link", "name": "Apply Leave", "exact": true }
        ],
        "arrived": arrived,
        "recorded": "2026-09-24T10:00:00Z"
    }))
    .unwrap()
}

fn with(modules: Vec<ModulePath>) -> NavFile {
    NavFile { direct_urls: true, modules, save_words: vec![], protected: vec![] }
}

/// A named area under a module, as the Areas dialog records one.
fn area(name: &str, module: &str, arrived: &str) -> ModulePath {
    let mut p = path(module, arrived);
    p.area = name.to_string();
    p
}

#[test]
fn the_blocked_sentences_are_the_designs_own_words() {
    assert_eq!(NO_MODULE, "This case has no Module - set one in Azure DevOps, or record a path for it.");
    assert_eq!(NO_ACCOUNT, "Choose an account when starting the run, or set Runs as on the script.");
    assert_eq!(
        no_path(" Payroll "),
        "No menu path recorded for module \"Payroll\" - record one in Auto Run, Areas."
    );
}

#[test]
fn no_file_and_a_file_without_the_switch_both_allow_addresses() {
    let dir = tempfile::tempdir().unwrap();
    let nav = load_nav(dir.path(), "Acme", "Web").unwrap();
    assert!(nav.direct_urls);
    assert!(nav.modules.is_empty());
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), "\u{feff}{ \"modules\": [] }").unwrap();
    assert!(load_nav(dir.path(), "Acme", "Web").unwrap().direct_urls, "absent means true");
}

#[test]
fn the_file_sits_beside_the_sign_in_recipe_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let nav = NavFile { direct_urls: false, modules: vec![area("Leave Apply", "Leave", "/hr/leave/apply")], save_words: vec![], protected: vec![] };
    save_nav(dir.path(), "Acme", "Web", &nav).unwrap();
    let expected = dir.path().join("projects").join(format!("{}.nav.json", project_slug("Acme", "Web")));
    assert_eq!(nav_path(dir.path(), "Acme", "Web"), expected);
    assert!(expected.is_file());
    // Atomic: no temp file (`*.tcm-tmp-*`) is left beside it.
    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("projects"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("tcm-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav);
}

#[test]
fn an_unreadable_file_says_so() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), "{ not json").unwrap();
    let err = load_nav(dir.path(), "Acme", "Web").unwrap_err();
    assert!(err.starts_with("the areas file is not readable"), "{err}");
}

#[test]
fn two_paths_for_the_same_module_are_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let err = save_nav(dir.path(), "Acme", "Web", &with(vec![path("Leave", "/a"), path("  leave ", "/b")])).unwrap_err();
    assert_eq!(err, "area \"leave\" has two paths - keep one");
    assert!(!nav_path(dir.path(), "Acme", "Web").exists());
}

#[test]
fn a_path_needs_a_name_clicks_and_an_address_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut no_clicks = path("Leave", "/hr/leave/apply");
    no_clicks.clicks.clear();
    assert_eq!(
        save_nav(dir.path(), "Acme", "Web", &with(vec![no_clicks])).unwrap_err(),
        "area \"Leave\" has no clicks - record it again"
    );
    assert!(save_nav(dir.path(), "Acme", "Web", &with(vec![path("Leave", "hr/leave")])).is_err());
    assert!(save_nav(dir.path(), "Acme", "Web", &with(vec![path("  ", "/x")])).is_err());
    assert_eq!(
        save_nav(dir.path(), " ", "Web", &with(vec![])).unwrap_err(),
        "pick an organization and a project first"
    );
}

#[test]
fn a_module_matches_trimmed_and_ignoring_case() {
    let nav = with(vec![path("Leave", "/hr/leave/apply")]);
    assert!(find_path(&nav, "  LEAVE ").is_some());
    assert!(find_path(&nav, "Leav").is_none());
    assert!(find_path(&nav, "   ").is_none());
    assert_eq!(module_key(" Apply Leave "), "apply leave");
}

/// Review focus 2: Re-record, or the same module typed in another case,
/// replaces the path rather than being refused or kept twice.
#[test]
fn recording_a_module_again_replaces_its_path_whatever_its_case() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", path("Leave", "/hr/leave/old")).unwrap();
    let nav = put_path(dir.path(), "Acme", "Web", path(" leave ", "/hr/leave/apply")).unwrap();
    assert_eq!(nav.modules.len(), 1);
    assert_eq!(nav.modules[0].module, "leave");
    assert_eq!(nav.modules[0].arrived, "/hr/leave/apply");
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav);
}

#[test]
fn removing_a_path_and_turning_the_switch_change_only_their_own_part() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", path("Leave", "/hr/leave/apply")).unwrap();
    put_path(dir.path(), "Acme", "Web", path("Payroll", "/hr/payroll")).unwrap();
    let nav = set_direct_urls(dir.path(), "Acme", "Web", false).unwrap();
    assert!(!nav.direct_urls);
    assert_eq!(nav.modules.len(), 2);
    let nav = remove_path(dir.path(), "Acme", "Web", "LEAVE").unwrap();
    assert!(!nav.direct_urls, "removing a path leaves the switch alone");
    assert_eq!(nav.modules.iter().map(|m| m.module.as_str()).collect::<Vec<_>>(), vec!["Payroll"]);
    let again = remove_path(dir.path(), "Acme", "Web", "Leave").unwrap();
    assert_eq!(again, nav, "removing what is not there changes nothing");
}

/// A named area whose last click is `last`, so two of one name differ in
/// their clicks.
fn area_clicking(name: &str, last: &str) -> ModulePath {
    let mut p = area(name, "Leave", "/hr/leave/apply");
    p.clicks = serde_json::from_value(json!([
        { "role": "link", "name": "Leave", "exact": true },
        { "role": "link", "name": last, "exact": true }
    ]))
    .unwrap();
    p
}

#[test]
fn put_path_if_refuses_an_area_changed_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", area_clicking("A", "x")).unwrap();
    // What the save read before its browser trips.
    let e = load_nav(dir.path(), "Acme", "Web").unwrap().modules[0].clone();
    // Another call changed the area meanwhile.
    put_path(dir.path(), "Acme", "Web", area_clicking("A", "y")).unwrap();
    let err = put_path_if(dir.path(), "Acme", "Web", area_clicking("A", "z"), AreaExpect::Was(&e)).unwrap_err();
    assert_eq!(err, area_changed("A"));
    assert_eq!(err, "area A changed while it was being saved - nothing was saved; look at it again");
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap().modules, vec![area_clicking("A", "y")], "the file holds the other call's area");
}

#[test]
fn put_path_if_refuses_an_area_removed_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", area_clicking("A", "x")).unwrap();
    let e = load_nav(dir.path(), "Acme", "Web").unwrap().modules[0].clone();
    remove_path(dir.path(), "Acme", "Web", "A").unwrap();
    let err = put_path_if(dir.path(), "Acme", "Web", area_clicking("A", "z"), AreaExpect::Was(&e)).unwrap_err();
    assert_eq!(err, area_changed("A"));
    assert!(load_nav(dir.path(), "Acme", "Web").unwrap().modules.is_empty());
}

#[test]
fn put_path_if_refuses_a_new_area_whose_name_was_taken() {
    let dir = tempfile::tempdir().unwrap();
    // The save began with no area A; another call saved one meanwhile.
    put_path(dir.path(), "Acme", "Web", area_clicking("A", "y")).unwrap();
    let err = put_path_if(dir.path(), "Acme", "Web", area_clicking("a", "z"), AreaExpect::New).unwrap_err();
    assert_eq!(err, area_taken_meanwhile("a"));
    assert_eq!(
        err,
        "area a was saved by another call while this one was being saved - nothing was saved; look at it again"
    );
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap().modules, vec![area_clicking("A", "y")]);
}

#[test]
fn put_path_if_writes_when_nothing_changed() {
    let dir = tempfile::tempdir().unwrap();
    // New, and still no such area.
    let nav = put_path_if(dir.path(), "Acme", "Web", area_clicking("A", "x"), AreaExpect::New).unwrap();
    assert_eq!(nav.modules, vec![area_clicking("A", "x")]);
    // Was, and still exactly that.
    let e = nav.modules[0].clone();
    let nav = put_path_if(dir.path(), "Acme", "Web", area_clicking("A", "y"), AreaExpect::Was(&e)).unwrap();
    assert_eq!(nav.modules, vec![area_clicking("A", "y")]);
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav);
    // Any adds and replaces as put_path does.
    let nav = put_path_if(dir.path(), "Acme", "Web", area_clicking("B", "x"), AreaExpect::Any).unwrap();
    assert_eq!(nav.modules.len(), 2);
}

#[test]
fn area_writes_from_many_threads_lose_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    // The threads start together, and each saves several areas, so their
    // load-change-save steps overlap if nothing orders them.
    let start = Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let (root, start) = (root.clone(), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                for n in 0..5 {
                    put_path(&root, "Acme", "Web", area_clicking(&format!("Area {i}-{n}"), "x")).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let nav = load_nav(&root, "Acme", "Web").unwrap();
    let mut names: Vec<String> = nav.modules.iter().map(|m| m.name().to_string()).collect();
    names.sort();
    let mut expected: Vec<String> = (0..8).flat_map(|i| (0..5).map(move |n| format!("Area {i}-{n}"))).collect();
    expected.sort();
    assert_eq!(names, expected);
}

#[test]
fn put_path_if_refuses_an_area_re_recorded_with_the_same_clicks() {
    let dir = tempfile::tempdir().unwrap();
    // An assistant's mapping run saved the area; this save read it so.
    let mut mapped = area_clicking("A", "x");
    mapped.made_by = MadeBy::Mapping;
    put_path(dir.path(), "Acme", "Web", mapped).unwrap();
    let e = load_nav(dir.path(), "Acme", "Web").unwrap().modules[0].clone();
    // The person recorded it again with the very same clicks.
    let mut again = area_clicking("A", "x");
    again.made_by = MadeBy::Person;
    again.recorded = "2026-10-23T09:00:00Z".to_string();
    put_path(dir.path(), "Acme", "Web", again.clone()).unwrap();
    let err = put_path_if(dir.path(), "Acme", "Web", area_clicking("A", "z"), AreaExpect::Was(&e)).unwrap_err();
    assert_eq!(err, area_changed("A"));
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap().modules, vec![again], "the person's recording stands");
    // Only the time differs: caught as well.
    let e = load_nav(dir.path(), "Acme", "Web").unwrap().modules[0].clone();
    let mut later = area_clicking("A", "x");
    later.recorded = "2026-10-23T09:05:00Z".to_string();
    put_path(dir.path(), "Acme", "Web", later).unwrap();
    assert_eq!(
        put_path_if(dir.path(), "Acme", "Web", area_clicking("A", "z"), AreaExpect::Was(&e)).unwrap_err(),
        area_changed("A")
    );
}

#[test]
fn a_script_with_no_area_goes_to_its_modules_only_area_whatever_its_name() {
    let nav = with(vec![area("Manage Cycle", "Performance", "/hr/home/index"), area("Payslips", "Payroll", "/hr/pay")]);
    let to = route_for(&nav, None, Some(" performance "), Some("hr.admin")).unwrap().unwrap();
    assert_eq!(to.name(), "Manage Cycle");
    assert_eq!(find_path(&nav, "Payroll").map(|p| p.name()), Some("Payslips"));
}

#[test]
fn the_area_named_like_the_module_wins_over_its_other_areas() {
    let nav = with(vec![area("Manage Cycle", "Performance", "/hr/a"), area("Performance", "Performance", "/hr/b")]);
    assert_eq!(route_for(&nav, None, Some("Performance"), Some("hr.admin")).unwrap().map(|p| p.arrived.as_str()), Some("/hr/b"));
}

#[test]
fn several_areas_and_none_named_like_the_module_lists_them_instead_of_guessing() {
    let nav = with(vec![area("Manage Cycle", "Performance", "/hr/a"), area("My Assessments", "Performance", "/hr/b")]);
    let why = route_for(&nav, None, Some("Performance"), Some("hr.admin")).unwrap_err();
    assert_eq!(why, no_default_area("Performance", &["Manage Cycle", "My Assessments"]));
    assert_eq!(
        why,
        "No menu path recorded for module \"Performance\" by its own name - its areas are \"Manage Cycle\", \"My Assessments\": \
         set the script's area to one of them, or record an area named \"Performance\" in Auto Run, Areas."
    );
    assert!(is_setup_problem(&why), "a run's summary counts it with the other setup problems");
    assert!(find_path(&nav, "Performance").is_none());
}

#[test]
fn a_project_with_no_paths_needs_neither_a_module_nor_an_account() {
    assert_eq!(route_for(&NavFile::default(), None, None, None), Ok(None));
    assert_eq!(route_for(&NavFile::default(), None, Some("Leave"), Some("hr.admin")), Ok(None));
}

#[test]
fn with_paths_a_case_needs_its_module_a_path_for_it_and_an_account_in_that_order() {
    let nav = with(vec![path("Leave", "/hr/leave/apply")]);
    assert_eq!(route_for(&nav, None, None, None), Err(NO_MODULE.to_string()));
    assert_eq!(route_for(&nav, None, Some("  "), Some("hr.admin")), Err(NO_MODULE.to_string()));
    assert_eq!(route_for(&nav, None, Some("Payroll"), None), Err(no_path("Payroll")));
    assert_eq!(route_for(&nav, None, Some("leave"), None), Err(NO_ACCOUNT.to_string()));
    assert_eq!(route_for(&nav, None, Some("leave"), Some(" ")), Err(NO_ACCOUNT.to_string()));
    assert_eq!(route_for(&nav, None, Some(" Leave "), Some("hr.admin")).unwrap().map(|p| p.arrived.as_str()), Some("/hr/leave/apply"));
}

#[test]
fn only_the_path_of_an_address_counts_as_where_a_page_is() {
    assert_eq!(path_of("https://hr.example.internal/hr/leave/apply?tab=2#top"), "/hr/leave/apply");
    assert_eq!(path_of("https://hr.example.internal"), "/");
    assert_eq!(path_of("https://hr.example.internal/"), "/");
    assert_eq!(path_of("file:///C:/app/menu.html"), "/C:/app/menu.html");
    assert_eq!(path_of("/hr/x?y=1"), "/hr/x");
    let home = "https://hr.example.internal/hr/home/index";
    assert!(same_page("https://HR.example.internal/hr/home/index?from=login", home));
    assert!(!same_page("https://hr.example.internal/hr/welcome", home));
    assert!(!same_page("https://other.example/hr/home/index", home));
}

#[test]
fn the_dialog_view_reads_every_click_in_words() {
    let v = view(&with(vec![path("Leave", "/hr/leave/apply")]));
    assert!(v.direct_urls);
    assert_eq!(v.modules[0].module, "Leave");
    assert_eq!(v.modules[0].area, "Leave", "a path with no area name is the area named after its module");
    assert_eq!(v.modules[0].clicks, vec!["link \"Leave\"".to_string(), "link \"Apply Leave\"".to_string()]);
    assert_eq!(v.modules[0].arrived, "/hr/leave/apply");
}

#[test]
fn a_failed_trip_reads_as_the_designs_sentence_in_a_run_and_its_short_form_in_the_dialog() {
    let at_click = PathFailure {
        at: Where::Click { n: 2, locator: "link \"Apply Leave\"".into() },
        reason: "no visible match".into(),
        harness: false,
    };
    assert_eq!(at_click.for_run(" Leave "), "Could not reach module \"Leave\": click 2, link \"Apply Leave\" - no visible match.");
    assert_eq!(at_click.for_dialog(), "click 2, link \"Apply Leave\": no visible match");
    let at_home = PathFailure { at: Where::Home, reason: "the home page did not load".into(), harness: false };
    // The reason already says it was the home page: said once, not twice.
    assert_eq!(at_home.for_run("Leave"), "Could not reach module \"Leave\": the home page did not load.");
    assert_eq!(at_home.for_dialog(), "the home page did not load");
    // A path with no clicks fails its arrival check "at home" too, and must
    // not blame the home page for it.
    let no_clicks = PathFailure {
        at: Where::Home,
        reason: "the page ended on /hr/home/index, not /hr/leave".into(),
        harness: false,
    };
    assert_eq!(
        no_clicks.for_run("Leave"),
        "Could not reach module \"Leave\": the page ended on /hr/home/index, not /hr/leave."
    );
    assert_eq!(no_clicks.for_dialog(), "the page ended on /hr/home/index, not /hr/leave");
    let dotted = PathFailure { reason: "it moved.".into(), ..at_click };
    assert!(dotted.for_run("Leave").ends_with("it moved."), "one full stop, not two");
}

/// Review m2: right after a sign-in a redirect may still be in flight, and
/// the page refuses to say where it is. That is "not home yet", not a
/// failed trip.
#[tokio::test]
async fn going_home_while_the_page_is_between_documents_navigates_instead_of_giving_up() {
    let mut first = true;
    let mut d = common::ScriptedDriver::new(move |method, params| match method {
        "Runtime.evaluate" if params["expression"] == "location.href" && first => {
            first = false;
            Err(CdpError::Protocol { method: method.to_string(), message: "Cannot find context with specified id".into() })
        }
        "Page.navigate" => Ok(json!({ "frameId": "F", "loaderId": "L" })),
        _ => Ok(json!({})),
    });
    d.on_every_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }) },
    ));
    // `menu_recipe` starts on /hr/home/index and has no `after_sign_in`.
    let home = v2_lib::autorun::nav::Home::of(&common::menu_recipe());
    let out = go_home(&mut d, &home, &common::quick()).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(d.calls_to("Page.navigate").len(), 1);
}

fn case_with(actions: serde_json::Value) -> v2_lib::autorun::CaseScript {
    serde_json::from_value(json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] },
        { "step_number": 2, "actions": actions }
    ] }))
    .unwrap()
}

#[test]
fn the_address_sentence_is_the_designs_own_words() {
    assert_eq!(
        no_address(2),
        "this project does not allow opening pages by address: a run starts on the case's module screen - use clicks instead of \"navigate\" (step 2)."
    );
}

#[test]
fn with_the_switch_off_any_navigate_absolute_or_relative_is_refused_and_named() {
    let off = NavFile { direct_urls: false, modules: vec![], save_words: vec![], protected: vec![] };
    for url in ["https://hr.example.internal/hr/leave", "/hr/leave/apply"] {
        let sc = case_with(json!([{ "kind": "navigate", "url": url }]));
        assert_eq!(check_no_addresses(&off, &[sc.clone()]).unwrap_err(), format!("case 7: {}", no_address(2)));
        assert!(check_no_addresses(&NavFile::default(), &[sc]).is_ok(), "on by default");
    }
    let clicks_only = case_with(json!([{ "kind": "click", "selector": { "role": "link", "name": "Leave" } }]));
    assert!(check_no_addresses(&off, &[clicks_only]).is_ok());
}

#[test]
fn the_guide_section_is_there_only_while_the_switch_is_off() {
    assert_eq!(guide_section(&NavFile::default()), "");
    let text = guide_section(&NavFile { direct_urls: false, modules: vec![], save_words: vec![], protected: vec![] });
    assert!(text.starts_with("## This project's runs start on the module screen"), "{text}");
    for must in ["before step 1", "starts there", "Never use `navigate`", "`sign_in`"] {
        assert!(text.contains(must), "missing {must:?}: {text}");
    }
    assert!(!text.contains('\u{2014}'), "no em dashes in text an assistant reads");
}

/// A recipe whose home is its login page, like PeoplesHR's: `after_sign_in`
/// opens a menu that a fresh page load draws closed.
fn login_home_recipe(start_path: &str) -> v2_lib::autorun::recipe::SignInRecipe {
    serde_json::from_value(json!({
        "start_url": format!("https://hr.example.internal{start_path}"),
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "after_sign_in": [ { "kind": "when_visible", "selector": { "css": "#toggle:not(.active)" }, "within_ms": 100,
            "then": [ { "kind": "click", "selector": { "css": "#toggle" } } ] } ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

fn leave_path() -> ModulePath {
    serde_json::from_value(json!({
        "module": "Leave",
        "clicks": [ { "role": "link", "name": "Leave", "exact": true } ],
        "arrived": "/hr/leave",
        "recorded": "2026-09-30T10:00:00Z"
    }))
    .unwrap()
}

/// 2026-09-30: PeoplesHR's recipe starts on its login page, so going home
/// after a sign-in is a fresh page load - and a fresh load draws the menu
/// closed again, undoing what `after_sign_in` had just opened. The module
/// path's first click then found its menu entry hidden. Going home by
/// address must leave the page the way `after_sign_in` promises. The trip
/// starts on a screen whose menu is closed, so the try from where the page
/// is cannot click the path, and the trip goes home.
#[tokio::test]
async fn going_home_by_address_runs_after_sign_in_again_before_the_first_click() {
    let (mut d, app) = sidebar_app("/hr/payroll", false, false);
    let route = Route::new(&login_home_recipe("/hr/security/login"), leave_path());
    let out = go_to_module(&mut d, &route, TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["navigate /hr/security/login", "click #toggle", "click Leave"]);
}

/// The same from the recording's side: `go_home` on its own, as a recording
/// calls it right after signing in.
#[tokio::test]
async fn go_home_that_navigates_leaves_the_menu_open() {
    let (mut d, app) = common::menu_app(&[], "/hr/home/index", 0);
    let home = v2_lib::autorun::nav::Home::of(&login_home_recipe("/hr/security/login"));
    let out = go_home(&mut d, &home, &common::quick()).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(*app.log.lock().unwrap(), vec!["navigate /hr/security/login".to_string(), "click #toggle".to_string()]);
}

/// A trip from the home page with the menu showing: the path is clicked
/// from there, with no reload and no `after_sign_in` run again. Going home
/// when already there is `go_home_on_the_home_page_neither_reloads_nor_reruns_after_sign_in`.
#[tokio::test]
async fn already_home_does_not_run_after_sign_in_again() {
    let (mut d, app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    *app.path.lock().unwrap() = "/hr/home/index".to_string();
    let route = v2_lib::autorun::nav::Route::new(&login_home_recipe("/hr/home/index"), leave_path());
    let out = v2_lib::autorun::nav::go_to_module(&mut d, &route, v2_lib::autorun::nav::TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(*app.log.lock().unwrap(), vec!["click Leave".to_string()]);
}

/// Already home: nothing reloaded, so nothing to put back - `after_sign_in`
/// already ran when the browser signed in, and a toggle run twice would
/// close what it opened.
#[tokio::test]
async fn go_home_on_the_home_page_neither_reloads_nor_reruns_after_sign_in() {
    let (mut d, app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    *app.path.lock().unwrap() = "/hr/home/index".to_string();
    let home = v2_lib::autorun::nav::Home::of(&login_home_recipe("/hr/home/index"));
    let out = go_home(&mut d, &home, &common::quick()).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(d.calls_to("Page.navigate").len(), 0);
    assert!(app.log.lock().unwrap().is_empty(), "{:?}", app.log.lock().unwrap());
}

// ---- A trip from where the page is -----------------------------------------

/// What `sidebar_app` saw, in order: `navigate <path>`, `click <name or
/// css>`. While `cover_on_toggle` is set, the next toggle click covers the
/// page (a toast, a slow render) and clears it. `outside_on` is the page
/// that also shows a link "Leave" outside the menu, a breadcrumb that
/// lands on /hr/elsewhere; it is gone once the page leaves.
struct Sidebar {
    log: Arc<Mutex<Vec<String>>>,
    cover_on_toggle: Arc<AtomicBool>,
    outside_on: Arc<Mutex<String>>,
}

impl Sidebar {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

/// PeoplesHR's left menu: `#toggle` flips it open and closed, and a page
/// load draws it closed. Its one entry, link "Leave", shows only while the
/// menu is open and lands on /hr/leave. While `covered` (a modal, a
/// full-screen grid) nothing on the page can be clicked; a page load
/// clears it. `#toggle:not(.active)` is found only while the menu is
/// closed, as the built-in recipe's `after_sign_in` asks. `#marker` is
/// always there.
fn sidebar_app(at: &str, open: bool, covered: bool) -> (common::ScriptedDriver, Sidebar) {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let open = Arc::new(AtomicBool::new(open));
    let covered = Arc::new(AtomicBool::new(covered));
    let path = Arc::new(Mutex::new(at.to_string()));
    let cover_on_toggle = Arc::new(AtomicBool::new(false));
    let outside_on = Arc::new(Mutex::new(String::new()));
    let app = Sidebar { log: log.clone(), cover_on_toggle: cover_on_toggle.clone(), outside_on: outside_on.clone() };
    let mut last_css = String::new();
    let mut last_probed = String::new();
    let mut d = common::ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let object = params["objectId"].as_str().unwrap_or("").to_string();
        // Only the menu's own entry hides with the menu.
        let shows = |o: &str| o != "ax-100" || open.load(Ordering::SeqCst);
        Ok(match method {
            "Page.navigate" => {
                let p = path_of(params["url"].as_str().unwrap_or(""));
                log.lock().unwrap().push(format!("navigate {p}"));
                *path.lock().unwrap() = p;
                open.store(false, Ordering::SeqCst);
                covered.store(false, Ordering::SeqCst);
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => {
                let at = path.lock().unwrap().clone();
                let href = if at == "about:blank" { at } else { format!("https://hr.example.internal{at}") };
                json!({ "result": { "value": href } })
            }
            "Runtime.evaluate" => json!({ "result": { "value": null } }),
            "Accessibility.queryAXTree" if params["role"] == "link" => {
                let mut nodes =
                    vec![json!({ "nodeId": "n0", "role": { "value": "link" }, "name": { "value": "Leave" }, "backendDOMNodeId": 100 })];
                if *outside_on.lock().unwrap() == *path.lock().unwrap() {
                    nodes.push(json!({ "nodeId": "n1", "role": { "value": "link" }, "name": { "value": "Leave" }, "backendDOMNodeId": 101 }));
                }
                json!({ "nodes": nodes })
            }
            "Accessibility.queryAXTree" => json!({ "nodes": [] }),
            "DOM.resolveNode" => json!({ "object": { "objectId": format!("ax-{}", params["backendNodeId"]) } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => {
                last_probed = object.clone();
                let hit = !covered.load(Ordering::SeqCst);
                let mut p = common::ready_probe();
                p["visible"] = json!(shows(&object));
                p["hit"] = json!(hit);
                p["covered_by"] = json!(if hit { "" } else { "div.modal" });
                json!({ "result": { "value": p } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS => json!({ "result": { "value": shows(&object) } }),
            "Runtime.callFunctionOn" if f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = last_css != "#toggle:not(.active)" || !open.load(Ordering::SeqCst);
                let base = last_css.split(':').next().unwrap_or("").to_string();
                json!({ "result": if there {
                    vec![json!({ "name": "0", "value": { "objectId": format!("css:{base}") } })]
                } else {
                    vec![]
                } })
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                if last_probed == "css:#toggle" {
                    log.lock().unwrap().push("click #toggle".to_string());
                    open.store(!open.load(Ordering::SeqCst), Ordering::SeqCst);
                    if cover_on_toggle.swap(false, Ordering::SeqCst) {
                        covered.store(true, Ordering::SeqCst);
                    }
                } else if last_probed == "ax-101" {
                    log.lock().unwrap().push("click outside Leave".to_string());
                    *path.lock().unwrap() = "/hr/elsewhere".to_string();
                } else if last_probed == "ax-100" {
                    log.lock().unwrap().push("click Leave".to_string());
                    *path.lock().unwrap() = "/hr/leave".to_string();
                }
                json!({})
            }
            _ => json!({}),
        })
    });
    d.on_every_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }) },
    ));
    (d, app)
}

/// A path that opens the menu with its toggle and then clicks Leave: the
/// shape that closes an open menu if its toggle is clicked regardless.
fn toggle_then_leave(start: &str) -> ModulePath {
    serde_json::from_value(json!({
        "module": "Leave",
        "clicks": [ { "css": "#toggle" }, { "role": "link", "name": "Leave", "exact": true } ],
        "arrived": "/hr/leave",
        "start": start,
        "recorded": "2026-10-09T10:00:00Z"
    }))
    .unwrap()
}

/// Spec A: a trip that starts on a module screen with the menu open clicks
/// the path from there. No page load: the home page, its signed-in check
/// and `after_sign_in` are never paid for.
#[tokio::test]
async fn a_trip_from_inside_the_app_does_not_reload_home() {
    let (mut d, app) = sidebar_app("/hr/payroll", true, false);
    let route = Route::new(&login_home_recipe("/hr/security/login"), leave_path());
    let out = go_to_module(&mut d, &route, TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(d.calls_to("Page.navigate").len(), 0, "the trip reloaded home");
    assert_eq!(app.log(), vec!["click Leave"]);
}

/// Review focus 1: the toggle flips, so clicking it on a menu that is
/// already open would close it. With Leave already showing, the toggle is
/// skipped; with the menu closed, it is clicked.
#[tokio::test]
async fn an_open_menu_keeps_its_toggle_unclicked() {
    let route = Route::new(&login_home_recipe("/hr/security/login"), toggle_then_leave(""));

    let (mut d, app) = sidebar_app("/hr/payroll", true, false);
    let out = go_to_module(&mut d, &route, TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["click Leave"], "the toggle closed an open menu");

    let (mut d, app) = sidebar_app("/hr/payroll", false, false);
    let out = go_to_module(&mut d, &route, TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["click #toggle", "click Leave"], "a closed menu is opened");
}

/// Review focus 2: a screen whose menu is covered (a modal, a full-screen
/// grid) cannot be clicked from. The trip gives up on the quick try, says
/// so once in the log, goes home the old way and arrives.
#[tokio::test]
async fn a_covered_menu_falls_back_to_home_and_arrives() {
    let _tail = crate::serial::log_tail();
    let (mut d, app) = sidebar_app("/hr/payroll", true, true);
    let route = Route::new(&login_home_recipe("/hr/security/login"), leave_path());
    let out = go_to_module(&mut d, &route, TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["navigate /hr/security/login", "click #toggle", "click Leave"]);
    let lines: Vec<_> = v2_lib::applog::recent(400).into_iter().filter(|l| l.message.contains("went home and tried")).collect();
    let last = lines.last().expect("the fallback was logged");
    assert_eq!(last.level, "info");
    assert_eq!(last.message, "went home and tried Leave again");
}

/// Spec A, what stays the same: right after a sign-in the trip keeps
/// today's rule. On the path's own first page it clicks the whole path as
/// recorded (no skipping); anywhere else it goes home first, even when the
/// path could have been clicked from where the page is.
#[tokio::test]
async fn the_first_trip_after_sign_in_is_unchanged() {
    let route = Route::new(&login_home_recipe("/hr/security/login"), toggle_then_leave("/hr/home/index"));
    let (mut d, app) = sidebar_app("/hr/home/index", false, false);
    let out = go_to_module(&mut d, &route, TripFrom::SignIn, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["click #toggle", "click Leave"]);

    let route = Route::new(&login_home_recipe("/hr/security/login"), leave_path());
    let (mut d, app) = sidebar_app("/hr/payroll", true, false);
    let out = go_to_module(&mut d, &route, TripFrom::SignIn, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["navigate /hr/security/login", "click #toggle", "click Leave"]);
}

/// Review finding 1: on the home page with the menu closed, the quick try
/// opens the menu and then cannot click Leave (something covers it). The
/// page was changed, so the trip reloads home before the full path;
/// without the reload the path's toggle would close the open menu.
#[tokio::test]
async fn a_quick_try_that_opened_the_menu_and_then_failed_reloads_before_the_full_path() {
    let (mut d, app) = sidebar_app("/hr/home/index", false, false);
    app.cover_on_toggle.store(true, Ordering::SeqCst);
    // `menu_recipe`: home is /hr/home/index, and no `after_sign_in`.
    let route = Route::new(&common::menu_recipe(), toggle_then_leave(""));
    let out = go_to_module(&mut d, &route, TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["click #toggle", "navigate /hr/home/index", "click #toggle", "click Leave"]);
}

/// The skip rule's known failure: a breadcrumb named like the menu entry
/// makes the toggle look unneeded, and the click lands somewhere else. The
/// arrived check catches it, and the trip reloads home and arrives,
/// without touching the breadcrumb again.
#[tokio::test]
async fn a_skipped_toggle_that_was_wrong_falls_back_and_arrives() {
    let _tail = crate::serial::log_tail();
    let (mut d, app) = sidebar_app("/hr/payroll", false, false);
    *app.outside_on.lock().unwrap() = "/hr/payroll".to_string();
    let route = Route::new(&common::menu_recipe(), toggle_then_leave(""));
    let out = go_to_module(&mut d, &route, TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["click outside Leave", "navigate /hr/home/index", "click #toggle", "click Leave"]);
    assert_eq!(d.calls_to("Page.navigate").len(), 1, "one fallback, one reload");
    let fallbacks = v2_lib::applog::recent(400)
        .into_iter()
        .filter(|l| l.message == "went home and tried Leave again")
        .count();
    assert!(fallbacks >= 1, "the fallback was not logged");
}

/// A quick-try click that was tried and failed may still have changed the
/// page, so the trip reloads home before the full path even on a page
/// whose address reads home. Here the home page is covered: the toggle is
/// tried and fails, and only a reload clears the cover.
#[tokio::test]
async fn a_quick_try_click_that_failed_still_reloads_home() {
    let (mut d, app) = sidebar_app("/hr/home/index", false, true);
    let route = Route::new(&common::menu_recipe(), toggle_then_leave(""));
    let out = go_to_module(&mut d, &route, TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["navigate /hr/home/index", "click #toggle", "click Leave"]);
}

/// An area saved before clicks had to match one element can come to match
/// two (a breadcrumb named like the menu entry). The failure then says the
/// area's click is what to repair, so an assistant repairs the area and not
/// the script.
#[tokio::test]
async fn a_path_click_that_matches_two_elements_says_to_repair_the_area() {
    use v2_lib::autorun::nav::AREA_PATH_NOT_EXACT;
    let (mut d, app) = sidebar_app("/hr/payroll", true, false);
    *app.outside_on.lock().unwrap() = "/hr/payroll".to_string();
    let mut path = leave_path();
    path.start = "/hr/payroll".into();
    let route = Route::new(&login_home_recipe("/hr/security/login"), path);
    let failure = go_to_module(&mut d, &route, TripFrom::SignIn, &common::quick()).await.unwrap_err();
    assert!(failure.reason.contains("2 elements"), "{}", failure.reason);
    assert!(failure.reason.ends_with(AREA_PATH_NOT_EXACT), "{}", failure.reason);
    assert!(app.log().is_empty(), "a click was made: {:?}", app.log());

    // A path that fails for another reason does not get the sentence.
    let (mut d, _) = sidebar_app("/hr/payroll", false, false);
    let mut path = leave_path();
    path.start = "/hr/payroll".into();
    let route = Route::new(&login_home_recipe("/hr/security/login"), path);
    let failure = go_to_module(&mut d, &route, TripFrom::SignIn, &common::quick()).await.unwrap_err();
    assert!(!failure.reason.contains(AREA_PATH_NOT_EXACT), "{}", failure.reason);
}

/// Which ids may scope a click. A scope must outlast the next render: an
/// id with a counter in it (a framework numbers its panels as it draws
/// them) names another list the next time. `PIN_ID_JS` applies the same
/// rule in the page.
#[test]
fn only_a_stable_id_may_scope_a_click() {
    use v2_lib::autorun::nav::{plain_id, GENERATED_ID_PREFIXES, GENERATED_ID_STEMS, PIN_ID_JS};
    for kept in ["admin-list", "sidebar", "nav_main", "ehrm-modulemenus-8", "main2", "left-menu", "Recent"] {
        assert!(plain_id(kept), "{kept} was rejected");
    }
    for rejected in [
        "ui-id-5",
        "mat-menu-panel-3",
        "mat-input-0",
        "cdk-overlay-0",
        "ember12",
        "tabs-1",
        "panel_2",
        "react-select-3-listbox",
        "radix-3",
        "a1b2",
        "x-123",
        ":r1:",
        "3f2a9c1e-77b0-4d1a-9e0f-5c1d2b3a4e5f",
        "",
        "9lives",
        "a b",
    ] {
        assert!(!plain_id(rejected), "{rejected} was accepted");
    }
    // The page-side rule carries the same lists.
    for word in GENERATED_ID_PREFIXES.iter().chain(GENERATED_ID_STEMS) {
        assert!(PIN_ID_JS.contains(&format!("'{word}'")), "{word} is not in PIN_ID_JS");
    }
    assert!(!GENERATED_ID_PREFIXES.is_empty() && !GENERATED_ID_STEMS.is_empty());
}

/// A page on another origin than the route's home (a fresh browser's
/// `about:blank`) has none of the path's clicks: the trip goes home at
/// once, with no quick try looked for or waited on first.
#[tokio::test]
async fn a_trip_from_about_blank_makes_no_quick_try() {
    let (mut d, app) = sidebar_app("about:blank", false, false);
    let route = Route::new(&login_home_recipe("/hr/security/login"), leave_path());
    let out = go_to_module(&mut d, &route, TripFrom::Elsewhere, &common::quick()).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(app.log(), vec!["navigate /hr/security/login", "click #toggle", "click Leave"]);
    let m = d.methods();
    let home = m.iter().position(|x| x == "Page.navigate").expect("the trip never went home");
    assert!(
        !m[..home].iter().any(|x| x == "Accessibility.queryAXTree" || x == "Runtime.callFunctionOn"),
        "a quick try looked for a click before going home: {m:?}"
    );
}

// ---- Areas ---------------------------------------------------------------

const UNRECORDED_PMS: &str = "the area \"Appraisals\" is not recorded - record it in Auto Run, Areas";

/// Two areas under PMS and two under Leave, one of them named after it.
fn pms_and_leave() -> NavFile {
    with(vec![
        area("Cycle Setup", "PMS", "/pms/cycle/setup"),
        area("Manage Cycle", "PMS", "/pms/cycle/manage"),
        area("Leave", "Leave", "/hr/leave/apply"),
        area("Leave Balance", "Leave", "/hr/leave/balance"),
    ])
}

/// Spec §9 decision 3: a file written before areas existed has only
/// modules. Each recorded path reads as an area named after its module, so
/// a script with no `area` still goes where it went.
#[test]
fn an_old_paths_file_reads_as_areas_named_after_modules() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let old = json!({ "direct_urls": false, "modules": [
        { "module": "Leave", "clicks": [{ "role": "link", "name": "Leave" }], "arrived": "/hr/leave", "recorded": "2026-09-24T10:00:00Z" },
        { "module": " Payroll ", "clicks": [{ "role": "link", "name": "Payroll" }], "arrived": "/hr/payroll", "recorded": "2026-09-24T10:00:00Z" }
    ] });
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), old.to_string()).unwrap();
    let nav = load_nav(dir.path(), "Acme", "Web").unwrap();
    assert!(!nav.direct_urls);
    assert_eq!(nav.modules.iter().map(|m| m.area.as_str()).collect::<Vec<_>>(), vec!["Leave", "Payroll"]);
    assert_eq!(find_area(&nav, "payroll").map(|p| p.arrived.as_str()), Some("/hr/payroll"));
    let v = view(&nav);
    assert_eq!(v.modules[0].area, "Leave");
    assert_eq!(v.modules[0].module, "Leave");
    // A script with no area routes exactly as it did.
    assert_eq!(route_for(&nav, None, Some("Leave"), Some("hr.admin")).unwrap().map(|p| p.arrived.as_str()), Some("/hr/leave"));
    // Saving it again writes the areas down; it reads back the same.
    save_nav(dir.path(), "Acme", "Web", &nav).unwrap();
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav);
}

/// Spec 2026-10-09 section 2: a file written before areas said who made
/// them reads every area as a person's, so no mapping run touches it, and
/// a save writes it down in words.
#[test]
fn an_old_area_loads_as_made_by_a_person() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let old = json!({ "modules": [
        { "area": "Leave Apply", "module": "Leave", "clicks": [{ "role": "link", "name": "Leave" }],
          "arrived": "/hr/leave", "recorded": "2026-09-24T10:00:00Z" },
        { "area": "Payroll Run", "module": "Payroll", "clicks": [{ "role": "link", "name": "Payroll" }],
          "arrived": "/hr/payroll", "recorded": "2026-09-24T10:00:00Z", "made_by": "mapping" }
    ] });
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), old.to_string()).unwrap();
    let nav = load_nav(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(find_area(&nav, "Leave Apply").unwrap().made_by, MadeBy::Person);
    assert_eq!(find_area(&nav, "Payroll Run").unwrap().made_by, MadeBy::Mapping);

    save_nav(dir.path(), "Acme", "Web", &nav).unwrap();
    let file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(nav_path(dir.path(), "Acme", "Web")).unwrap()).unwrap();
    assert_eq!(file["modules"][0]["made_by"], "person");
    assert_eq!(file["modules"][1]["made_by"], "mapping");
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav);
}

/// Review Focus 4: an old file whose modules differ only in case would
/// become two areas whose names differ only in case. The first wins; the
/// rest are dropped and logged, never kept as a second way into "the same"
/// area.
#[test]
fn old_modules_differing_only_in_case_keep_the_first() {
    let _tail = crate::serial::log_tail();
    let _warned = crate::serial::nav_warnings();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let old = json!({ "modules": [
        { "module": "Leave", "clicks": [{ "role": "link", "name": "Leave" }], "arrived": "/hr/leave/first", "recorded": "2026-09-24T10:00:00Z" },
        { "module": "Payroll", "clicks": [{ "role": "link", "name": "Payroll" }], "arrived": "/hr/payroll", "recorded": "2026-09-24T10:00:00Z" },
        { "module": "LEAVE", "clicks": [{ "role": "link", "name": "Leave" }], "arrived": "/hr/leave/second", "recorded": "2026-09-25T10:00:00Z" }
    ] });
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), old.to_string()).unwrap();
    let nav = load_nav(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(nav.modules.len(), 2, "{nav:?}");
    assert_eq!(nav.modules[0].area, "Leave");
    assert_eq!(nav.modules[0].arrived, "/hr/leave/first");
    assert_eq!(nav.modules[1].area, "Payroll");
    assert_eq!(find_area(&nav, "leave").map(|p| p.arrived.as_str()), Some("/hr/leave/first"));
    let lines: Vec<String> = v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect();
    assert!(
        lines.iter().any(|m| m.contains("\"LEAVE\"") && m.contains("\"Leave\"")),
        "the dropped area is logged: {lines:?}"
    );
    // What is left is a file that saves.
    save_nav(dir.path(), "Acme", "Web", &nav).unwrap();
}

/// Area names are unique per project ignoring case. Re-recording an area
/// (the same name under the same module, in any case) replaces it, as a
/// module path's Re-record always has; a NEW area whose name clashes with
/// one under another module is refused and the file is left alone.
#[test]
fn area_names_are_unique_ignoring_case() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", area("Manage Cycle", "PMS", "/pms/old")).unwrap();
    let before = std::fs::read_to_string(nav_path(dir.path(), "Acme", "Web")).unwrap();

    let err = put_path(dir.path(), "Acme", "Web", area(" manage cycle ", "Leave", "/hr/leave")).unwrap_err();
    assert_eq!(err, "an area named \"Manage Cycle\" is already recorded under PMS - choose another name");
    assert_eq!(std::fs::read_to_string(nav_path(dir.path(), "Acme", "Web")).unwrap(), before, "nothing written");

    let nav = put_path(dir.path(), "Acme", "Web", area("MANAGE CYCLE", " pms ", "/pms/cycle/manage")).unwrap();
    assert_eq!(nav.modules.len(), 1, "a re-record replaces: {nav:?}");
    assert_eq!(nav.modules[0].area, "MANAGE CYCLE");
    assert_eq!(nav.modules[0].module, "pms");
    assert_eq!(nav.modules[0].arrived, "/pms/cycle/manage");

    // The rule holds for a hand-made file too.
    let twice = with(vec![area("Manage Cycle", "PMS", "/a"), area("manage cycle", "PMS", "/b")]);
    assert_eq!(
        save_nav(dir.path(), "Acme", "Web", &twice).unwrap_err(),
        "area \"manage cycle\" has two paths - keep one"
    );

    // Removing is by area name too, ignoring case, and leaves the module's
    // other areas alone.
    put_path(dir.path(), "Acme", "Web", area("Cycle Setup", "PMS", "/pms/cycle/setup")).unwrap();
    let nav = remove_path(dir.path(), "Acme", "Web", "manage cycle").unwrap();
    assert_eq!(nav.modules.iter().map(|m| m.area.as_str()).collect::<Vec<_>>(), vec!["Cycle Setup"]);
}

#[test]
fn two_areas_under_one_module_both_route() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", area("Cycle Setup", "PMS", "/pms/cycle/setup")).unwrap();
    let nav = put_path(dir.path(), "Acme", "Web", area("Manage Cycle", "PMS", "/pms/cycle/manage")).unwrap();
    assert_eq!(nav.modules.len(), 2, "a second area under PMS keeps the first");
    let to = |a: &str| route_for(&nav, Some(a), Some("PMS"), Some("hr.admin")).unwrap().map(|p| p.arrived.clone());
    assert_eq!(to("Cycle Setup"), Some("/pms/cycle/setup".to_string()));
    assert_eq!(to(" manage cycle "), Some("/pms/cycle/manage".to_string()));
}

#[test]
fn a_scripts_area_wins_over_its_module() {
    let nav = pms_and_leave();
    let arrived =
        |a: Option<&str>, m: Option<&str>| route_for(&nav, a, m, Some("hr.admin")).unwrap().map(|p| p.arrived.clone());
    // The Module has an area of its own name, and the script names another.
    assert_eq!(arrived(Some("Leave Balance"), Some("Leave")), Some("/hr/leave/balance".to_string()));
    assert_eq!(arrived(None, Some("Leave")), Some("/hr/leave/apply".to_string()));
    // The script's area routes even when the Module has no area of its name,
    // or the case has no Module at all.
    assert_eq!(arrived(Some("Manage Cycle"), Some("PMS")), Some("/pms/cycle/manage".to_string()));
    assert_eq!(arrived(Some("Manage Cycle"), None), Some("/pms/cycle/manage".to_string()));
    // An area still needs an account, like any path.
    assert_eq!(route_for(&nav, Some("Manage Cycle"), Some("PMS"), None), Err(NO_ACCOUNT.to_string()));
}

/// No `area` (or a blank one) is exactly today: the area named like the
/// case's Module, with today's sentences word for word.
#[test]
fn no_area_routes_by_module_as_today() {
    let nav = pms_and_leave();
    for blank in [None, Some(""), Some("   ")] {
        assert_eq!(
            route_for(&nav, blank, None, Some("hr.admin")),
            Err("This case has no Module - set one in Azure DevOps, or record a path for it.".to_string())
        );
        // PMS has two areas, but none named PMS: they are named, not guessed.
        assert_eq!(
            route_for(&nav, blank, Some(" PMS "), Some("hr.admin")),
            Err("No menu path recorded for module \"PMS\" by its own name - its areas are \"Cycle Setup\", \"Manage Cycle\": \
                 set the script's area to one of them, or record an area named \"PMS\" in Auto Run, Areas."
                .to_string())
        );
        assert_eq!(
            route_for(&nav, blank, Some("Payroll"), Some("hr.admin")),
            Err("No menu path recorded for module \"Payroll\" - record one in Auto Run, Areas.".to_string())
        );
        assert_eq!(
            route_for(&nav, blank, Some("leave"), None),
            Err("Choose an account when starting the run, or set Runs as on the script.".to_string())
        );
        assert_eq!(
            route_for(&nav, blank, Some("leave"), Some("hr.admin")).unwrap().map(|p| p.arrived.as_str()),
            Some("/hr/leave/apply")
        );
        assert_eq!(route_for(&NavFile::default(), blank, Some("Leave"), Some("hr.admin")), Ok(None));
    }
}

#[test]
fn an_unrecorded_area_refuses_the_case() {
    let nav = pms_and_leave();
    let err = route_for(&nav, Some(" Appraisals "), Some("PMS"), Some("hr.admin")).unwrap_err();
    assert_eq!(err, UNRECORDED_PMS);
    // The Module's own area is no fallback for an area name that is wrong.
    assert_eq!(route_for(&nav, Some("Appraisals"), Some("Leave"), Some("hr.admin")).unwrap_err(), UNRECORDED_PMS);
    // A project with no areas at all cannot run a script that names one.
    assert_eq!(
        route_for(&NavFile::default(), Some("Appraisals"), Some("PMS"), Some("hr.admin")).unwrap_err(),
        UNRECORDED_PMS
    );
    // It is about the project's setup, not the script's steps.
    assert!(is_setup_problem(&err));
}

fn script_in(case_id: i32, area: Option<&str>) -> serde_json::Value {
    let mut sc = json!({ "case_id": case_id, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ] });
    if let Some(a) = area {
        sc["area"] = json!(a);
    }
    sc
}

#[test]
fn check_areas_names_the_recorded_areas() {
    let nav = pms_and_leave();
    let ok: Vec<v2_lib::autorun::CaseScript> = vec![
        serde_json::from_value(script_in(1, None)).unwrap(),
        serde_json::from_value(script_in(2, Some(" "))).unwrap(),
        serde_json::from_value(script_in(3, Some("manage cycle"))).unwrap(),
    ];
    assert!(check_areas(&nav, &ok).is_ok());
    let bad: v2_lib::autorun::CaseScript = serde_json::from_value(script_in(4, Some("Appraisals"))).unwrap();
    assert_eq!(
        check_areas(&nav, &[ok[0].clone(), bad.clone()]).unwrap_err(),
        format!("case 4: {UNRECORDED_PMS} (recorded areas: Cycle Setup, Manage Cycle, Leave, Leave Balance)")
    );
    assert_eq!(
        check_areas(&NavFile::default(), &[bad]).unwrap_err(),
        format!("case 4: {UNRECORDED_PMS} (no areas are recorded yet)")
    );
    // `area` is written only when it is set.
    let plain = serde_json::to_value(&ok[0]).unwrap();
    assert!(plain.get("area").is_none(), "{plain}");
    assert_eq!(serde_json::to_value(&ok[2]).unwrap()["area"], "manage cycle");
}

/// Every door a script comes in by - the editor, a file import, the
/// assistant's save - refuses an area the project has not recorded, and
/// writes nothing.
#[tokio::test]
async fn saving_a_script_with_an_unrecorded_area_is_refused() {
    use v2_lib::ai_bridge::{route, BridgeContext};
    use v2_lib::autorun::store::{load_script, set_root};
    use v2_lib::commands::autorun::{import_scripts_from_path, save_script_from_editor};

    let _root = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("data");
    save_nav(&root, "acme", "Web", &pms_and_leave()).unwrap();
    let expected =
        |id: i32| format!("case {id}: {UNRECORDED_PMS} (recorded areas: Cycle Setup, Manage Cycle, Leave, Leave Balance)");

    // The editor.
    let script: v2_lib::autorun::CaseScript = serde_json::from_value(script_in(7, Some("Appraisals"))).unwrap();
    assert_eq!(save_script_from_editor(&root, "acme", "Web", script).unwrap_err(), expected(7));
    assert!(load_script(&root, 7).unwrap().is_none());

    // A file import: all or nothing, so the good script beside it is not
    // written either.
    let file = dir.path().join("bundle.json");
    std::fs::write(&file, json!([script_in(8, Some("Manage Cycle")), script_in(9, Some("Appraisals"))]).to_string()).unwrap();
    assert_eq!(import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap(), None).unwrap_err(), expected(9));
    assert!(load_script(&root, 8).unwrap().is_none());
    assert!(load_script(&root, 9).unwrap().is_none());

    // The assistant's save, refused before it ever needs Azure DevOps.
    set_root(root.clone());
    let ctx = BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() };
    let body = json!([script_in(10, Some("Appraisals"))]).to_string();
    let (status, out) = route(&ctx, None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, expected(10));
    assert!(load_script(&root, 10).unwrap().is_none());

    // A recorded area saves, and is kept on the script.
    let good: v2_lib::autorun::CaseScript = serde_json::from_value(script_in(11, Some("manage cycle"))).unwrap();
    save_script_from_editor(&root, "acme", "Web", good).unwrap();
    assert_eq!(load_script(&root, 11).unwrap().unwrap().area.as_deref(), Some("manage cycle"));
}

/// The assistant's guide lists every area - name, module and where it
/// lands - and says when to set `area`, whatever the address switch says.
#[test]
fn the_guide_lists_each_area_with_its_module_and_where_it_lands() {
    assert_eq!(guide_section(&NavFile::default()), "", "nothing to say with no areas and addresses allowed");
    let text = guide_section(&pms_and_leave());
    assert!(!text.contains("## This project's runs start on the module screen"), "addresses are allowed: {text}");
    for line in [
        "- Cycle Setup - PMS - /pms/cycle/setup",
        "- Manage Cycle - PMS - /pms/cycle/manage",
        "- Leave - Leave - /hr/leave/apply",
        "- Leave Balance - Leave - /hr/leave/balance",
    ] {
        assert!(text.contains(line), "missing {line:?}: {text}");
    }
    for must in ["`area`", v2_lib::autorun::nav::SET_AREA_RULE, "named like the case's Module", "the module's only area"] {
        assert!(text.contains(must), "missing {must:?}: {text}");
    }
    assert!(!text.contains('\u{2014}'), "no em dashes in text an assistant reads");
    // With the switch off, both sections.
    let both = guide_section(&NavFile { direct_urls: false, ..pms_and_leave() });
    assert!(both.contains("## This project's runs start on the module screen"));
    assert!(both.contains("- Manage Cycle - PMS - /pms/cycle/manage"));
}

/// Review of Task 8, minor: the paths file is read on every run, check and
/// save, so the case-only duplicate it drops is logged once per file per
/// process - not once per read.
#[test]
fn a_dropped_duplicate_area_is_logged_once_per_file() {
    let _tail = crate::serial::log_tail();
    let _warned = crate::serial::nav_warnings();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let old = json!({ "modules": [
        { "module": "Payroll", "clicks": [{ "role": "link", "name": "Payroll" }], "arrived": "/hr/payroll", "recorded": "2026-09-24T10:00:00Z" },
        { "module": "PAYROLL", "clicks": [{ "role": "link", "name": "Payroll" }], "arrived": "/hr/payroll/2", "recorded": "2026-09-25T10:00:00Z" }
    ] });
    std::fs::write(nav_path(dir.path(), "Acme", "Logged once"), old.to_string()).unwrap();
    for _ in 0..3 {
        assert_eq!(load_nav(dir.path(), "Acme", "Logged once").unwrap().modules.len(), 1);
    }
    let count = v2_lib::applog::recent(400)
        .into_iter()
        .filter(|l| l.message.contains("Acme / Logged once") && l.message.contains("\"PAYROLL\""))
        .count();
    assert_eq!(count, 1);
}

/// Final review, finding 1: a discovery reaches a listed area by clicking
/// its menu path, so each area line gives that path, click by click, and
/// the section says to carry it out with `discover_autorun_actions`.
#[test]
fn the_areas_section_gives_each_areas_menu_path() {
    let text = guide_section(&pms_and_leave());
    let menu = format!(
        "- Leave - Leave - /hr/leave/apply - menu path: {}, then {}\n",
        path("Leave", "/x").clicks[0].describe(),
        path("Leave", "/x").clicks[1].describe()
    );
    assert!(text.contains(&menu), "missing {menu:?}: {text}");
    assert!(text.contains("`discover_autorun_actions`"), "{text}");
}

// ------------------------------------------- an assistant's repair

/// An area an assistant repaired: its clicks now `#new`, its old path
/// `Leave › Apply Leave` from `/old-start` to `/hr/leave/old`.
fn repaired_leave() -> ModulePath {
    let mut p = area("Leave Apply", "Leave", "/hr/leave/apply");
    p.clicks = vec![serde_json::from_value(json!({ "css": "#new" })).unwrap()];
    p.start = "/".into();
    p.repaired_by_assistant = Some(AssistantRepair {
        previous_clicks: path("Leave", "/x").clicks,
        previous_arrived: "/hr/leave/old".into(),
        previous_start: "/old-start".into(),
        reason: "The Leave menu moved under Self Service".into(),
        at: "2026-10-20T09:00:00Z".into(),
    });
    p
}

/// A file from before repairs loads with no note, and an area never
/// repaired writes none: its file reads exactly as it did.
#[test]
fn an_old_area_file_loads_with_no_repair_note_and_writes_none() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let old = json!({ "modules": [
        { "area": "Leave Apply", "module": "Leave", "clicks": [{ "role": "link", "name": "Leave" }],
          "arrived": "/hr/leave", "recorded": "2026-09-24T10:00:00Z", "made_by": "person" }
    ] });
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), old.to_string()).unwrap();
    let nav = load_nav(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(find_area(&nav, "Leave Apply").unwrap().repaired_by_assistant, None);
    save_nav(dir.path(), "Acme", "Web", &nav).unwrap();
    let file = std::fs::read_to_string(nav_path(dir.path(), "Acme", "Web")).unwrap();
    assert!(!file.contains("repaired_by_assistant"), "{file}");
}

/// A repaired area keeps its note through a save and a load, and the Areas
/// dialog's view shows the reason and when - never the old clicks.
#[test]
fn a_repair_note_is_kept_and_shown() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", repaired_leave()).unwrap();
    let nav = load_nav(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(find_area(&nav, "Leave Apply"), Some(&repaired_leave()));
    let shown = view(&nav).modules.remove(0);
    let note = shown.repaired_by_assistant.expect("the view lost the note");
    assert_eq!(note.reason, "The Leave menu moved under Self Service");
    assert_eq!(note.at, "2026-10-20T09:00:00Z");
}

/// Put old path back restores the clicks, where they arrive and the page
/// they begin on, and clears the note; the rest of the area is unchanged.
/// An area with nothing to put back, or no such area, is refused.
#[test]
fn put_old_path_back_restores_the_old_path_and_clears_the_note() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", repaired_leave()).unwrap();
    let nav = put_old_path_back(dir.path(), "Acme", "Web", " leave APPLY ").unwrap();
    let back = find_area(&nav, "Leave Apply").unwrap();
    assert_eq!(back.clicks, path("Leave", "/x").clicks);
    assert_eq!(back.arrived, "/hr/leave/old");
    assert_eq!(back.start, "/old-start");
    assert_eq!(back.repaired_by_assistant, None);
    assert_eq!(back.module, "Leave");
    assert_eq!(back.made_by, MadeBy::Person);
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav, "it was not saved");

    let again = put_old_path_back(dir.path(), "Acme", "Web", "Leave Apply").unwrap_err();
    assert!(again.contains("no repaired path"), "{again}");
    let none = put_old_path_back(dir.path(), "Acme", "Web", "Payroll").unwrap_err();
    assert!(none.contains("no area named"), "{none}");
}

/// A person's re-record writes a fresh area, and the note goes with the
/// path it repaired.
#[test]
fn a_persons_re_record_clears_the_repair_note() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", repaired_leave()).unwrap();
    let recorded = v2_lib::autorun::recorder::finish(
        "Leave",
        "Leave Apply",
        v2_lib::autorun::recorder::Captured {
            clicks: path("Leave", "/x").clicks,
            ended: v2_lib::autorun::recorder::Ended::Stopped { href: "https://hr.example.com/hr/leave/apply".into() },
        },
        "2026-10-21T00:00:00Z",
    )
    .unwrap();
    let nav = put_path(dir.path(), "Acme", "Web", recorded).unwrap();
    assert_eq!(find_area(&nav, "Leave Apply").unwrap().repaired_by_assistant, None);
}

/// The reason is one line of at most 200 characters, in words.
#[test]
fn a_repair_reason_is_one_short_line_in_words() {
    assert_eq!(check_reason("  The menu moved  ").unwrap(), "The menu moved");
    assert!(check_reason(&"x".repeat(MAX_REASON)).is_ok());
    for bad in ["", " ", "a\nb", "a\rb"] {
        assert!(check_reason(bad).is_err(), "{bad:?}");
    }
    assert!(check_reason(&"x".repeat(MAX_REASON + 1)).is_err());
    // Addresses keep only their path, with or without a scheme.
    let kept = check_reason("moved: see https://hr.example.com/self/leave?t=1 or www.hr.example.com/x").unwrap();
    assert!(!kept.contains("hr.example.com") && !kept.contains("t=1"), "{kept}");
    assert!(kept.contains("/self/leave"), "{kept}");
}

/// `without_addresses` (every note an assistant writes goes through it) also
/// hides a host written without a scheme: a name that ends like a domain
/// with a path or a port after it, or an IPv4 address - and leaves ordinary
/// words with dots in them alone.
#[test]
fn bare_hosts_with_a_path_or_port_are_hidden_but_ordinary_words_are_not() {
    use v2_lib::autorun::mapping_summary::without_addresses as hide;
    assert_eq!(hide("moved to hr.example.com/leave/apply?x=1 today"), "moved to /leave/apply today");
    assert_eq!(hide("see (hr.example.com/x)."), "see (/x).");
    assert_eq!(hide("tried hr.example.com:8080 first"), "tried / first");
    assert_eq!(hide("on 10.0.0.5:8080/hr then"), "on /hr then");
    assert_eq!(hide("on 10.0.0.5, again"), "on /, again");
    assert_eq!(hide("on 10.0.0.5."), "on /.");
    assert_eq!(hide("got hr-test.example.co.uk/a#frag"), "got /a");
    let words = [
        "v2.2.3 shipped, e.g. the menu, i.e. the old one",
        "see cv.txt and notes.md, or Index.cshtml.cs:40 and main.rs:12",
        "Leave > Apply at 12.30 on 3.5 of 4",
        "the Save & Continue button (Pages/Leave/Index.cshtml.cs:95)",
        "version 1.2.3 of example.com", // no path or port: a word
    ];
    for w in words {
        assert_eq!(hide(w), w, "{w}");
    }
}

/// Code locations in the target app's own languages and four-part version
/// numbers are words; an address is hidden when it has a port or a path, or
/// is a bare 0-255 quad not written as a version.
#[test]
fn aspnet_file_names_and_version_numbers_are_left_alone() {
    use v2_lib::autorun::mapping_summary::without_addresses as hide;
    for w in [
        "failed in Leave.aspx:40 today",
        "see web.config:12 and App.vue:3",
        "Default.aspx/GetData answered 500",
        "Service.asmx/List and Main.kt:40 and App.swift:9",
        "build version 1.0.0.0 shipped",
        "Version 7.2.0.15.",
        "running v7.2.0.15 and v1.0.0.0",
        "ver 1.0.0.0",
    ] {
        assert_eq!(hide(w), w, "{w}");
    }
    assert_eq!(hide("hr.example.com/x"), "/x");
    assert_eq!(hide("10.0.0.5:8080/hr"), "/hr");
    assert_eq!(hide("on 10.0.0.5"), "on /");
    assert_eq!(hide("on 999.0.0.5"), "on 999.0.0.5");
    assert_eq!(hide("version 1.0.0.0:8080 and version 10.0.0.5/hr"), "version / and version /hr");
}

/// A repair is written to the areas file as it is after the browser trips:
/// an area removed or changed meanwhile is refused and nothing is written;
/// one as the trips found it is repaired, its note keeping the path it had.
#[test]
fn a_repair_is_saved_only_to_the_area_as_the_trips_found_it() {
    let dir = tempfile::tempdir().unwrap();
    let tried = area("Leave Apply", "Leave", "/hr/leave/apply");
    let new_clicks = || -> Vec<v2_lib::browser::locator::Target> { vec![serde_json::from_value(json!({ "css": "#new" })).unwrap()] };
    let save = |dir: &std::path::Path| {
        save_repair(
            dir,
            "Acme",
            "Web",
            &tried,
            new_clicks(),
            "/hr/self/leave".into(),
            "/".into(),
            "Moved".into(),
            "2026-10-20T09:00:00Z".into(),
        )
    };
    let file = |dir: &std::path::Path| std::fs::read(nav_path(dir, "Acme", "Web")).unwrap();

    // Removed meanwhile.
    put_path(dir.path(), "Acme", "Web", area("Payroll", "Payroll", "/hr/payroll")).unwrap();
    let before = file(dir.path());
    let why = save(dir.path()).unwrap_err();
    assert!(why.contains("was removed while it was being repaired"), "{why}");
    assert_eq!(file(dir.path()), before);

    // Changed meanwhile (put back, or recorded again).
    let mut changed = tried.clone();
    changed.arrived = "/hr/leave/other".into();
    put_path(dir.path(), "Acme", "Web", changed).unwrap();
    let before = file(dir.path());
    let why = save(dir.path()).unwrap_err();
    assert!(why.contains("changed while it was being repaired"), "{why}");
    assert_eq!(file(dir.path()), before);

    // As the trips found it.
    put_path(dir.path(), "Acme", "Web", tried.clone()).unwrap();
    let nav = save(dir.path()).unwrap();
    let saved = find_area(&nav, "Leave Apply").unwrap();
    assert_eq!(saved.clicks, new_clicks());
    assert_eq!(saved.arrived, "/hr/self/leave");
    let note = saved.repaired_by_assistant.as_ref().unwrap();
    assert_eq!(note.previous_clicks, tried.clicks);
    assert_eq!(note.previous_arrived, "/hr/leave/apply");
    assert!(find_area(&nav, "Payroll").is_some(), "another area was lost");
}
