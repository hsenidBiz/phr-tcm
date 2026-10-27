//! Running a saved setup (`autorun::setup_run`): the find rule first, a
//! record built in the explore session only when nothing was found, and
//! whatever it built put on the change list.
//!
//! The real browser is never opened here: `run_setup_in` runs against the
//! fake browser slot `autorun_discovery` builds, and the database is a fake
//! `RowSource`.

use crate::autorun_discovery::{exploring, root_with_recipe_and_account, FakeBrowser, ORG, PROJECT};
use crate::common::{quick, ready_probe, ScriptedDriver};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::ai_bridge::DiscoveryBrowser;
use v2_lib::autorun::explore::{begin, load, note_writes, unaccounted, Change, ExploreRun, NO_EXPLORE_SESSION};
use v2_lib::autorun::kept;
use v2_lib::autorun::nav::unrecorded_area;
use v2_lib::autorun::setup_find::{DbFind, FindRule, RowSource, DATABASE_OFF};
use v2_lib::autorun::setup_run::{
    action_failed, find_could_not_run, ids_of_built, not_confirmed, record_built, run_setup_in, values_for,
    BuiltCreate, BUILD_NOT_SAVED, BUILT, FOUND,
};
use v2_lib::autorun::setups::{
    fresh_value, load_setups, save, Built, SavedSetup, SetupCreate, SetupInput,
};
use v2_lib::autorun::test_made;
use v2_lib::browser::actions::{Action, CHECK_TEXT_JS, HIGHLIGHT_JS};
use v2_lib::browser::cdp::Event;
use v2_lib::browser::input::{FOCUS_JS, HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;
use v2_lib::browser::net_capture::{Body, Captured};
use v2_lib::commands::autorun::DiscoveryState;

const NOW: u64 = 1_760_000_000_000;
const NAME: &str = "Leave type";

fn act(v: Value) -> Action {
    serde_json::from_value(v).unwrap()
}

fn click(css: &str) -> Value {
    json!({ "kind": "click", "selector": { "css": css } })
}

/// A database that answers `answer` and counts how often it was asked.
struct Rows {
    answer: Result<String, String>,
    asked: Arc<AtomicUsize>,
}

impl Rows {
    fn new(answer: Result<&str, &str>) -> Rows {
        Rows {
            answer: answer.map(str::to_string).map_err(str::to_string),
            asked: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn found() -> Rows {
        Rows::new(Ok("Id\tName\n--\t----\n9\tAUTOTEST leave\n\n(1 rows affected)\n"))
    }
    fn none() -> Rows {
        Rows::new(Ok("Id\tName\n--\t----\n\n(0 rows affected)\n"))
    }
}

impl RowSource for Rows {
    async fn select(&self, _sql: &str) -> Result<String, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.answer.clone()
    }
}

fn database_rule() -> FindRule {
    FindRule {
        database: Some(DbFind {
            sql: "SELECT Id, Name FROM dbo.LeaveType WHERE Name LIKE {prefix} + N'%'".into(),
            id_column: "Id".into(),
            name_column: "Name".into(),
        }),
        browser: None,
    }
}

fn browser_rule(actions: Vec<Value>) -> FindRule {
    FindRule { database: None, browser: Some(actions.into_iter().map(act).collect()) }
}

fn leave_input() -> SetupInput {
    SetupInput { name: "leave_name".into(), description: "its name".into(), unique: true, names_the_record: true }
}

fn leave_create() -> SetupCreate {
    SetupCreate {
        kind: "leave type".into(),
        name: "{{leave_name}}".into(),
        request: "POST /hr/leave/save".into(),
        id_at: "/data/Id".into(),
    }
}

fn setup_with(find: FindRule, area: Option<&str>, actions: Vec<Value>) -> SavedSetup {
    SavedSetup {
        name: NAME.into(),
        description: "makes a leave type".into(),
        area: area.map(str::to_string),
        inputs: vec![leave_input()],
        actions: actions.into_iter().map(act).collect(),
        find,
        creates: vec![leave_create()],
        returns: 0,
        version: 1,
        changes: 0,
        saved_at: 1,
        last_used_at: 0,
    }
}

fn leave_setup(find: FindRule) -> SavedSetup {
    setup_with(
        find,
        None,
        vec![json!({ "kind": "fill", "selector": { "css": "#name" }, "value": "{{leave_name}}" }), click("#save")],
    )
}

fn saved(root: &std::path::Path, setup: SavedSetup) {
    save(root, ORG, PROJECT, Built { setup, replaced: vec![], cut: false }, None).unwrap();
}

/// A small app: every element is found except the `missing` ones, and the
/// first element's text is `row_text`. Page.navigate lands, and `#marker`
/// says the sign-in arrived, so a trip home works.
fn app(missing: &'static [&'static str], row_text: &'static str) -> ScriptedDriver {
    let path = Arc::new(Mutex::new("/".to_string()));
    let mut last_css = String::new();
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Page.navigate" => {
                *path.lock().unwrap() = v2_lib::autorun::nav::path_of(params["url"].as_str().unwrap_or(""));
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => json!({ "result": {
                "value": format!("https://hr.example.internal{}", path.lock().unwrap())
            } }),
            "Runtime.evaluate" if params["expression"] == "document.title" => json!({ "result": { "value": "Home" } }),
            "Runtime.evaluate" => {
                json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } })
            }
            "Accessibility.getFullAXTree" => json!({ "nodes": [
                { "nodeId": "1", "ignored": false, "role": { "value": "form" }, "name": { "value": "Leave" },
                  "childIds": [] }
            ] }),
            "Network.getAllCookies" => json!({ "cookies": [] }),
            "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": ready_probe() } }),
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == FOCUS_JS => json!({ "result": { "value": "text" } }),
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if f == v2_lib::autorun::setup_run::MATCHED_TEXT_JS => {
                json!({ "result": { "value": row_text } })
            }
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = !missing.contains(&last_css.as_str());
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            _ => json!({}),
        })
    });
    d.on_every_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }) },
    ));
    d
}

/// A going explore session on `Leave` whose change list is `session`.
fn going(session: &str, no_save: bool) -> Option<DiscoveryState> {
    exploring("Leave").map(|s| DiscoveryState {
        no_save,
        explore: Some(ExploreRun { session: Some(session.to_string()), ..ExploreRun::default() }),
        ..s
    })
}

fn browser(d: ScriptedDriver, state: Option<DiscoveryState>) -> Option<FakeBrowser> {
    crate::autorun_discovery::slot(d, state).0
}

async fn run(
    b: &mut Option<FakeBrowser>,
    root: &std::path::Path,
    rows: Option<&Rows>,
    body: Value,
) -> (u16, String) {
    run_setup_in(b, root, ORG, PROJECT, &body.to_string(), rows, &quick(), NOW).await
}

fn parsed(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("not JSON ({e}): {body}"))
}

fn input_calls(b: &mut Option<FakeBrowser>) -> usize {
    b.as_mut().unwrap().d.calls.iter().filter(|(m, _)| m.starts_with("Input.")).count()
}

fn mouse_releases(b: &mut Option<FakeBrowser>) -> usize {
    b.as_mut()
        .unwrap()
        .d
        .calls_to("Input.dispatchMouseEvent")
        .iter()
        .filter(|p| p["type"] == "mouseReleased")
        .count()
}

fn navigations(b: &mut Option<FakeBrowser>) -> usize {
    b.as_mut().unwrap().d.calls_to("Page.navigate").len()
}

#[tokio::test]
async fn run_setup_needs_an_explore_session() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let body = json!({ "name": NAME });

    let mut none: Option<FakeBrowser> = None;
    assert_eq!(run(&mut none, dir.path(), None, body.clone()).await, (409, NO_EXPLORE_SESSION.to_string()));

    let mut plain = browser(app(&[], ""), exploring("Leave"));
    assert_eq!(run(&mut plain, dir.path(), None, body.clone()).await, (409, NO_EXPLORE_SESSION.to_string()));

    let no_list = exploring("Leave").map(|s| DiscoveryState { explore: Some(ExploreRun::default()), ..s });
    let mut b = browser(app(&[], ""), no_list);
    assert_eq!(run(&mut b, dir.path(), None, body).await, (409, NO_EXPLORE_SESSION.to_string()));
}

#[tokio::test]
async fn a_found_record_builds_nothing() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    saved(dir.path(), leave_setup(database_rule()));
    let mut b = browser(app(&[], ""), going(&id, false));
    let rows = Rows::found();

    let (status, body) = run(&mut b, dir.path(), Some(&rows), json!({ "name": NAME })).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["said"], FOUND);
    assert_eq!(v["found"], true);
    assert_eq!(v["record"], json!({ "id": "9", "name": "AUTOTEST leave" }));
    assert_eq!(v["setup"], NAME);
    assert_eq!(input_calls(&mut b), 0, "a found record drove the page");
    assert_eq!(navigations(&mut b), 0, "a database find makes no trip");
    assert_eq!(load_setups(dir.path(), ORG, PROJECT).unwrap().setups[0].last_used_at, NOW);
    assert!(!b.as_mut().unwrap().parts().discovery.as_ref().unwrap().explore.as_ref().unwrap().setup_running);
}

#[tokio::test]
async fn a_database_rule_that_cannot_run_builds_nothing() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    saved(dir.path(), leave_setup(database_rule()));
    let mut b = browser(app(&[], ""), going(&id, false));
    let rows = Rows::new(Err("x"));

    let answer = run(&mut b, dir.path(), Some(&rows), json!({ "name": NAME })).await;
    assert_eq!(answer, (409, find_could_not_run("x")));
    assert_eq!(input_calls(&mut b), 0);
    assert!(load(dir.path(), &id).unwrap().changes.is_empty());
}

#[tokio::test]
async fn database_off_and_no_browser_rule_refuses() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    saved(dir.path(), leave_setup(database_rule()));
    let mut b = browser(app(&[], ""), going(&id, false));

    let answer = run(&mut b, dir.path(), None, json!({ "name": NAME })).await;
    assert_eq!(answer, (409, DATABASE_OFF.to_string()));
    assert_eq!(input_calls(&mut b), 0);
}

#[tokio::test]
async fn a_browser_rule_that_cannot_run_builds_nothing() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    let rule = browser_rule(vec![
        click("#missing"),
        json!({ "kind": "expect_visible", "selector": { "css": "#row" }, "timeout_ms": 50 }),
    ]);
    saved(dir.path(), leave_setup(rule));
    let mut b = browser(app(&["#missing"], ""), going(&id, false));

    let (status, body) = run(&mut b, dir.path(), None, json!({ "name": NAME })).await;
    assert_eq!(status, 409, "{body}");
    assert!(body.starts_with("The find rule could not run"), "{body}");
    assert_eq!(mouse_releases(&mut b), 0, "a build action ran");
    assert!(load(dir.path(), &id).unwrap().changes.is_empty());
}

#[tokio::test]
async fn a_browser_rule_whose_check_fails_builds() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    let rule = browser_rule(vec![json!({ "kind": "expect_visible", "selector": { "css": "#row" }, "timeout_ms": 50 })]);
    saved(dir.path(), leave_setup(rule));
    let mut b = browser(app(&["#row"], ""), going(&id, false));

    let (status, body) = run(&mut b, dir.path(), None, json!({ "name": NAME })).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert!(v.get("built").is_some(), "{body}");
    assert_eq!(mouse_releases(&mut b), 1, "the build did not run");
}

#[tokio::test]
async fn a_browser_rule_whose_row_starts_with_the_prefix_is_found() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    let rule = browser_rule(vec![json!({ "kind": "expect_visible", "selector": { "css": "#row" } })]);
    saved(dir.path(), leave_setup(rule));

    let mut b = browser(app(&[], "  AUTOTEST leave A\nedit  delete"), going(&id, false));
    let (status, body) = run(&mut b, dir.path(), None, json!({ "name": NAME })).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["found"], true, "{body}");
    assert_eq!(v["record"], json!({ "name": "AUTOTEST leave A" }));
    assert_eq!(mouse_releases(&mut b), 0);

    // A row that is not a test record is not found, so the build runs.
    let mut other = browser(app(&[], "Annual leave"), going(&id, false));
    let (status, body) = run(&mut other, dir.path(), None, json!({ "name": NAME })).await;
    assert_eq!(status, 200, "{body}");
    assert!(parsed(&body).get("built").is_some(), "{body}");
}

#[tokio::test]
async fn the_first_failing_build_action_stops_the_run() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    let setup = setup_with(database_rule(), None, vec![click("#a"), click("#missing"), click("#c")]);
    saved(dir.path(), SavedSetup { inputs: vec![], ..setup });
    let mut b = browser(app(&["#missing"], ""), going(&id, false));
    let rows = Rows::none();

    let (status, body) = run(&mut b, dir.path(), Some(&rows), json!({ "name": NAME })).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["built"], false, "{body}");
    assert_eq!(v["failed_at"], 2, "{body}");
    assert!(v["said"].as_str().unwrap().contains("may need saving again"), "{body}");
    assert!(v["said"].as_str().unwrap().starts_with("Action 2 (click) failed: "), "{body}");
    assert_eq!(mouse_releases(&mut b), 1, "action 3 ran");
    assert!(!b.as_mut().unwrap().parts().discovery.as_ref().unwrap().explore.as_ref().unwrap().setup_running);
}

#[tokio::test]
async fn values_are_checked_before_anything_runs() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    let mut setup = leave_setup(database_rule());
    setup.inputs.push(SetupInput { name: "note".into(), description: "a note".into(), unique: false, names_the_record: false });
    saved(dir.path(), setup);
    let mut b = browser(app(&[], ""), going(&id, false));
    let rows = Rows::none();

    let body = json!({ "name": NAME, "values": { "leave_name": "leave", "bogus": "x" } });
    let (status, said) = run(&mut b, dir.path(), Some(&rows), body).await;
    assert_eq!(status, 400, "{said}");
    assert!(said.contains("note needs a value."), "{said}");
    assert!(said.contains(&format!("{NAME} has no input bogus.")), "{said}");
    assert!(said.contains("leave_name must start with the test prefix AUTOTEST."), "{said}");
    assert!(b.as_mut().unwrap().d.calls.is_empty(), "the browser was driven");
    assert_eq!(rows.asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_unique_value_left_out_is_made_fresh() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    saved(dir.path(), leave_setup(database_rule()));
    let mut b = browser(app(&[], ""), going(&id, false));
    let rows = Rows::none();

    let (status, body) = run(&mut b, dir.path(), Some(&rows), json!({ "name": NAME })).await;
    assert_eq!(status, 200, "{body}");
    let fresh = fresh_value("AUTOTEST", NAME, NOW);
    assert_eq!(parsed(&body)["values"]["leave_name"], fresh.as_str(), "{body}");
    let file = std::fs::read_to_string(v2_lib::autorun::setups::setups_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(!file.contains(&fresh), "a run's value reached the setups file");
}

/// Review Focus 2.
#[tokio::test]
async fn the_setups_area_gone_refuses_before_the_find() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    let setup = setup_with(database_rule(), Some("Old Leave"), vec![click("#a")]);
    saved(dir.path(), SavedSetup { inputs: vec![], ..setup });
    let mut b = browser(app(&[], ""), going(&id, false));
    let rows = Rows::found();

    let answer = run(&mut b, dir.path(), Some(&rows), json!({ "name": NAME })).await;
    assert_eq!(answer, (409, unrecorded_area("Old Leave")));
    assert_eq!(rows.asked.load(Ordering::SeqCst), 0, "the database was asked");
    assert!(b.as_mut().unwrap().d.calls.is_empty(), "the browser was driven");
}

#[tokio::test]
async fn a_missing_setup_and_a_damaged_file_are_refused() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    let mut b = browser(app(&[], ""), going(&id, false));
    assert_eq!(
        run(&mut b, dir.path(), None, json!({ "name": "Nope" })).await,
        (404, v2_lib::autorun::setups::no_such_setup("Nope"))
    );

    let path = v2_lib::autorun::setups::setups_path(dir.path(), ORG, PROJECT);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ \"setups\": [").unwrap();
    let (status, said) = run(&mut b, dir.path(), None, json!({ "name": NAME })).await;
    assert_eq!(status, 409, "{said}");
    assert!(said.contains("-setups.json"), "{said}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ \"setups\": [");
}

#[tokio::test]
async fn a_no_save_session_finds_but_does_not_build() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    saved(dir.path(), leave_setup(database_rule()));
    let mut b = browser(app(&[], ""), going(&id, true));

    let none = Rows::none();
    let answer = run(&mut b, dir.path(), Some(&none), json!({ "name": NAME })).await;
    assert_eq!(answer, (409, BUILD_NOT_SAVED.to_string()));
    assert_eq!(input_calls(&mut b), 0);

    let found = Rows::found();
    let (status, body) = run(&mut b, dir.path(), Some(&found), json!({ "name": NAME })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["found"], true);
}

/// A save the page made at `started_ms`, answered with `body`.
fn save_at(method: &str, path: &str, started_ms: u64, body: &str) -> Captured {
    Captured {
        method: method.to_string(),
        url_path_query: path.to_string(),
        started_ms,
        status: Some(200),
        resp_body: Some(Body::Text(body.to_string())),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_setup_run_keeps_no_actions_and_a_build_is_stamped() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    saved(dir.path(), leave_setup(database_rule()));
    kept::started(dir.path(), "owner", Some(&id), Some("Leave"));
    let mut b = browser(app(&[], ""), going(&id, false));
    let rows = Rows::none();

    let (status, body) = run(&mut b, dir.path(), Some(&rows), json!({ "name": NAME })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["built"], true, "{body}");
    assert!(kept::list_of(dir.path(), &id).map_or(true, |l| l.actions.is_empty()), "the setup's actions were kept");
    assert_eq!(load_setups(dir.path(), ORG, PROJECT).unwrap().setups[0].last_used_at, NOW);
    assert!(!b.as_mut().unwrap().parts().discovery.as_ref().unwrap().explore.as_ref().unwrap().setup_running);
}

#[test]
fn ids_of_built_reads_the_id_from_the_same_place() {
    let creates = vec![leave_create()];
    let ids = |caps: &[Captured]| {
        let refs: Vec<&Captured> = caps.iter().collect();
        ids_of_built(&refs, 2000, &creates)
    };
    assert_eq!(ids(&[save_at("POST", "/hr/leave/save?x=1", 3000, r#"{"data":{"Id":301}}"#)]), vec![Some("301".to_string())]);
    // A read of that path, a save from before the floor, and an answer with the id elsewhere.
    assert_eq!(ids(&[save_at("GET", "/hr/leave/save", 3000, r#"{"data":{"Id":301}}"#)]), vec![None]);
    assert_eq!(ids(&[save_at("POST", "/hr/leave/save", 1500, r#"{"data":{"Id":301}}"#)]), vec![None]);
    assert_eq!(ids(&[save_at("POST", "/hr/leave/save", 3000, r#"{"Id":301}"#)]), vec![None]);

    // Two creates of the same request take the first and second saves in order.
    let twice = vec![leave_create(), leave_create()];
    let first = save_at("POST", "/hr/leave/save", 3000, r#"{"data":{"Id":301}}"#);
    let second = save_at("POST", "/hr/leave/save", 4000, r#"{"data":{"Id":302}}"#);
    assert_eq!(
        ids_of_built(&[&second, &first], 2000, &twice),
        vec![Some("301".to_string()), Some("302".to_string())]
    );
}

fn values_of(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn a_built_record_is_recorded_verified_and_can_be_undone() {
    let dir = root_with_recipe_and_account();
    let session = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    let post = save_at("POST", "/hr/leave/save?x=1", 3000, r#"{"data":{"Id":301}}"#);
    let setup = leave_setup(database_rule());

    let (built, unconfirmed) =
        record_built(dir.path(), &session, &setup, &values_of(&[("leave_name", "AUTOTEST x")]), &[&post], 2000);
    assert!(unconfirmed.is_empty(), "{unconfirmed:?}");
    assert_eq!(
        built,
        vec![BuiltCreate {
            entry: 1,
            kind: "leave type".into(),
            id: "301".into(),
            name: "AUTOTEST x".into(),
            verified: true
        }]
    );
    let s = load(dir.path(), &session).unwrap();
    assert!(matches!(&s.changes[0].change, Change::Created { verified: true, .. }));
    assert_eq!(s.changes[0].proof.as_ref().unwrap().id_at.as_deref(), Some("/data/Id"));
    let made = test_made::list(dir.path());
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0].run_id, session);
    note_writes(dir.path(), &session, &[("POST".to_string(), "/hr/leave/save".to_string())]).unwrap();
    assert!(unaccounted(&load(dir.path(), &session).unwrap()).is_empty());
}

#[tokio::test]
async fn a_save_without_the_id_where_expected_is_not_a_clean_build() {
    let _g = crate::serial::autorun();
    let dir = root_with_recipe_and_account();
    let session = begin(dir.path(), ORG, PROJECT, "admin", NOW).unwrap();
    let setup = leave_setup(database_rule());
    let post = save_at("POST", "/hr/leave/save", 3000, r#"{"ok":true}"#);

    let (built, unconfirmed) =
        record_built(dir.path(), &session, &setup, &values_of(&[("leave_name", "AUTOTEST x")]), &[&post], 2000);
    assert!(built.is_empty(), "{built:?}");
    assert_eq!(unconfirmed, vec!["POST /hr/leave/save".to_string()]);

    // Through the run: the browser captured nothing, so nothing is confirmed.
    saved(dir.path(), setup);
    let mut b = browser(app(&[], ""), going(&session, false));
    let rows = Rows::none();
    let (status, body) = run(&mut b, dir.path(), Some(&rows), json!({ "name": NAME })).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["built"], true, "{body}");
    assert_eq!(v["confirmed"], false, "{body}");
    assert_eq!(v["said"], not_confirmed("POST /hr/leave/save").as_str(), "{body}");
    assert!(!v["said"].as_str().unwrap().contains(BUILT), "{body}");
}

#[test]
fn values_for_names_every_problem_and_makes_unique_ones() {
    let mut setup = leave_setup(database_rule());
    setup.inputs.push(SetupInput { name: "note".into(), description: "".into(), unique: false, names_the_record: false });
    let given = |v: Value| v.as_object().unwrap().clone();

    let ok = values_for(&setup, &given(json!({ "note": 5 })), "AUTOTEST", NOW).unwrap();
    assert_eq!(ok["note"], "5");
    assert_eq!(ok["leave_name"], fresh_value("AUTOTEST", NAME, NOW));

    let said = values_for(&setup, &given(json!({ "leave_name": "AUTOTEST ok", "note": "n" })), "AUTOTEST", NOW).unwrap();
    assert_eq!(said["leave_name"], "AUTOTEST ok");

    let bad = values_for(&setup, &given(json!({ "leave_name": "leave", "zzz": "1" })), "AUTOTEST", NOW).unwrap_err();
    assert_eq!(bad.len(), 3, "{bad:?}");
    assert!(bad.contains(&"note needs a value.".to_string()));
    assert!(bad.contains(&format!("{NAME} has no input zzz.")));
    assert!(bad.contains(&"leave_name must start with the test prefix AUTOTEST.".to_string()));
}

#[test]
fn the_sentences_say_what_to_do() {
    assert_eq!(
        action_failed(2, "click", "no such element"),
        "Action 2 (click) failed: no such element. The setup may need saving again; what it created so far is on the change list."
    );
    assert_eq!(find_could_not_run("x"), "The find rule could not run, so nothing was built: x");
    assert!(!BUILD_NOT_SAVED.contains('\u{2014}') && !BUILD_NOT_SAVED.contains('\u{2013}'));
}
