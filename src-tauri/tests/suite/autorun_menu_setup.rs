//! The app sets each account's menu preferences after a sign-in, from the
//! project's recipe and the area's record (`autorun::menu_setup`), and an
//! area is recorded with the values in force.

use crate::common::{account, quick, ready_probe, ScriptedDriver};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::ai_bridge::{discover_area_in, DiscoveryBrowser, DiscoveryParts};
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::menu_setup::CONTROL_STATE_JS;
use v2_lib::autorun::nav::{find_area, go_to_module_noted, load_nav, ModulePath, Route, TripFrom};
use v2_lib::autorun::recipe::{save_recipe, SignInRecipe};
use v2_lib::browser::actions::{CHECK_TEXT_JS, HIGHLIGHT_JS};
use v2_lib::browser::cdp::{Driver, Event};
use v2_lib::browser::input::{FOCUS_JS, HAS_FOCUS_JS, PROBE_JS, SHOWN_JS};
use v2_lib::browser::locator::{Target, VISIBLE_JS};
use v2_lib::commands::autorun::{DiscoveryState, GuardOwner};

const ORG: &str = "acme";
const PROJECT: &str = "Web";
const GROUP: &str = "Group Menu Items";

/// The page as the tests see it: a user menu whose Preferences dialog holds
/// one switch (`#SectionWiseMode`). Changing it and closing the dialog
/// saves it and reloads the page (the menu opener runs again). `#leave`
/// is the area's one click.
struct App {
    log: Arc<Mutex<Vec<String>>>,
    /// The switch, as the account has it saved.
    saved: Arc<AtomicBool>,
    /// The switch while the dialog is open.
    shown: Arc<AtomicBool>,
    /// The element last probed, which the next mouse release lands on.
    probed: Arc<Mutex<String>>,
    /// The Preferences link is not on the page.
    link_missing: Arc<AtomicBool>,
    /// The mode the account is in ("" for an account with only one).
    mode: Arc<Mutex<String>>,
}

impl App {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
    fn count(&self, entry: &str) -> usize {
        self.log().iter().filter(|l| *l == entry).count()
    }
}

fn app(saved_on: bool) -> (ScriptedDriver, App) {
    app_at(saved_on, "/")
}

fn app_at(saved_on: bool, at: &str) -> (ScriptedDriver, App) {
    let app = App {
        log: Arc::new(Mutex::new(vec![])),
        saved: Arc::new(AtomicBool::new(saved_on)),
        shown: Arc::new(AtomicBool::new(saved_on)),
        probed: Arc::new(Mutex::new(String::new())),
        link_missing: Arc::new(AtomicBool::new(false)),
        mode: Arc::new(Mutex::new(String::new())),
    };
    let (log, saved, shown, probed, link_missing, mode) = (
        app.log.clone(),
        app.saved.clone(),
        app.shown.clone(),
        app.probed.clone(),
        app.link_missing.clone(),
        app.mode.clone(),
    );
    let path = Arc::new(Mutex::new(at.to_string()));
    let mut last_css = String::new();
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let object = params["objectId"].as_str().unwrap_or("").to_string();
        Ok(match method {
            "Page.navigate" => {
                *path.lock().unwrap() = v2_lib::autorun::nav::path_of(params["url"].as_str().unwrap_or(""));
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => {
                json!({ "result": { "value": format!("https://hr.example.internal{}", path.lock().unwrap()) } })
            }
            "Runtime.evaluate" => json!({ "result": { "value": null } }),
            "Accessibility.getFullAXTree" => json!({ "nodes": [] }),
            "Network.getAllCookies" => json!({ "cookies": [] }),
            "Runtime.callFunctionOn" if f == PROBE_JS => {
                *probed.lock().unwrap() = object;
                json!({ "result": { "value": ready_probe() } })
            }
            "Runtime.callFunctionOn" if f == CONTROL_STATE_JS => {
                json!({ "result": { "value": shown.load(Ordering::SeqCst) } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == SHOWN_JS || f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == FOCUS_JS => json!({ "result": { "value": "text" } }),
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let now = mode.lock().unwrap().clone();
                let there = !(last_css == "#prefslink" && link_missing.load(Ordering::SeqCst))
                    && !(last_css == "#admin-shown" && now != "Admin")
                    && !(last_css == "#ss-shown" && now != "Self Service");
                json!({ "result": if there {
                    vec![json!({ "name": "0", "value": { "objectId": format!("css:{last_css}") } })]
                } else {
                    vec![]
                } })
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                let on = probed.lock().unwrap().clone();
                match on.as_str() {
                    "css:#userimg" | "css:#prefslink" => {
                        log.lock().unwrap().push(format!("open {}", &on[5..]));
                        shown.store(saved.load(Ordering::SeqCst), Ordering::SeqCst);
                    }
                    "css:#SectionWiseMode" => {
                        log.lock().unwrap().push("click control".to_string());
                        shown.store(!shown.load(Ordering::SeqCst), Ordering::SeqCst);
                    }
                    "css:#closeprefs" => {
                        log.lock().unwrap().push("click close".to_string());
                        saved.store(shown.load(Ordering::SeqCst), Ordering::SeqCst);
                    }
                    "css:#toggle" => log.lock().unwrap().push("click toggle".to_string()),
                    "css:#go" => *path.lock().unwrap() = "/hr/home/index".to_string(),
                    "css:#to-admin" | "css:#to-ss" => {
                        let to = if on == "css:#to-admin" { "Admin" } else { "Self Service" };
                        log.lock().unwrap().push(format!("switch {to}"));
                        *mode.lock().unwrap() = to.to_string();
                    }
                    "css:#leave" => {
                        log.lock().unwrap().push("click leave".to_string());
                        *path.lock().unwrap() = "/hr/leave".to_string();
                    }
                    _ => {}
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

fn recipe() -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "after_sign_in": [ { "kind": "click", "selector": { "css": "#toggle" } } ],
        "menu_preferences": {
            "open": [ { "css": "#userimg" }, { "css": "#prefslink" } ],
            "close": { "css": "#closeprefs" },
            "settings": [ { "name": GROUP, "control": { "css": "#SectionWiseMode" }, "on": false } ]
        },
        "modes": [
            { "name": "Admin", "shown": { "css": "#admin-shown" }, "switch": { "css": "#to-admin" } },
            { "name": "Self Service", "shown": { "css": "#ss-shown" }, "switch": { "css": "#to-ss" } }
        ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

fn leave(preferences: Value) -> ModulePath {
    serde_json::from_value(json!({
        "module": "Leave",
        "clicks": [ { "css": "#leave" } ],
        "arrived": "/hr/leave",
        "recorded": "2026-10-21T10:00:00Z",
        "preferences": preferences
    }))
    .unwrap()
}

/// The trip a run makes right after a sign-in, and what it noted.
async fn trip(d: &mut ScriptedDriver, preferences: Value) -> (Result<String, String>, Vec<String>) {
    let route = Route::new(&recipe(), leave(preferences));
    let mut notes = Vec::new();
    let out = go_to_module_noted(d, &route, TripFrom::SignIn, &quick(), &mut notes).await;
    (out.map_err(|f| f.for_dialog()), notes)
}

/// A trip to the Leave area, recorded in `mode`, from a page in use.
async fn trip_in_mode(d: &mut ScriptedDriver, mode: Option<&str>) -> (Result<String, String>, Vec<String>) {
    let mut path = leave(json!({}));
    path.mode = mode.map(str::to_string);
    let route = Route::new(&recipe(), path);
    let mut notes = Vec::new();
    let out = go_to_module_noted(d, &route, TripFrom::Elsewhere, &quick(), &mut notes).await;
    (out.map_err(|f| f.for_dialog()), notes)
}

#[tokio::test]
async fn a_self_service_area_under_an_admin_account_switches_mode_first() {
    let (mut d, app) = app(false);
    *app.mode.lock().unwrap() = "Admin".to_string();
    let (out, notes) = trip_in_mode(&mut d, Some("Self Service")).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(notes, vec!["switched from Admin to Self Service mode".to_string()]);
    let log = app.log();
    let switched = log.iter().position(|l| l == "switch Self Service").expect("the switch was clicked");
    let first = log.iter().position(|l| l == "click leave").expect("the path was walked");
    assert!(switched < first, "{log:?}");
    assert_eq!(*app.mode.lock().unwrap(), "Self Service");
}

#[tokio::test]
async fn an_admin_area_after_a_self_service_one_switches_back() {
    let (mut d, app) = app(false);
    *app.mode.lock().unwrap() = "Admin".to_string();
    let (_, notes) = trip_in_mode(&mut d, Some("Self Service")).await;
    assert_eq!(notes.len(), 1, "{notes:?}");
    // Already in the area's mode: nothing is clicked.
    let (_, notes) = trip_in_mode(&mut d, Some("Self Service")).await;
    assert!(notes.is_empty(), "{notes:?}");
    let (out, notes) = trip_in_mode(&mut d, Some("Admin")).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(notes, vec!["switched from Self Service to Admin mode".to_string()]);
    assert_eq!(app.count("switch Admin"), 1, "{:?}", app.log());
    assert_eq!(*app.mode.lock().unwrap(), "Admin");
}

#[tokio::test]
async fn an_account_with_one_mode_skips_the_switch() {
    // No mode marker shows at all.
    let (mut d, app) = app(false);
    let (out, notes) = trip_in_mode(&mut d, Some("Self Service")).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(app.count("switch Self Service"), 0, "{:?}", app.log());
    // An area that recorded no mode never looks either.
    *app.mode.lock().unwrap() = "Admin".to_string();
    let (_, notes) = trip_in_mode(&mut d, None).await;
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(app.count("switch Self Service"), 0, "{:?}", app.log());
}

#[tokio::test]
async fn a_mode_the_recipe_does_not_name_fails_the_trip() {
    let (mut d, _app) = app(false);
    let (out, _) = trip_in_mode(&mut d, Some("Manager")).await;
    assert_eq!(out, Err("the area's mode Manager is not one the recipe names".to_string()));
}

#[tokio::test]
async fn an_account_already_matching_changes_nothing() {
    // Off is what the recipe's areas need, and what the account has.
    let (mut d, app) = app(false);
    let (out, notes) = trip(&mut d, json!({})).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(app.count("click control"), 0, "{:?}", app.log());
    // The dialog was opened to look, and closed again.
    assert_eq!(app.log(), vec!["open userimg", "open prefslink", "click close", "click leave"]);
    // Nothing was let through the guard for it.
    assert!(d.holds.is_empty(), "{:?}", d.holds);
}

#[tokio::test]
async fn a_differing_preference_is_switched_once_and_the_next_sign_in_finds_it_set() {
    let (mut d, app) = app(true);
    let (out, notes) = trip(&mut d, json!({})).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(notes, vec![format!("{GROUP} turned off")]);
    assert_eq!(app.count("click control"), 1, "{:?}", app.log());
    // The page keeps it off now: the next sign-in's trip clicks nothing.
    let (out, notes) = trip(&mut d, json!({})).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(app.count("click control"), 1, "{:?}", app.log());
}

#[tokio::test]
async fn an_area_that_recorded_a_value_gets_that_value() {
    let (mut d, app) = app(false);
    let (_, notes) = trip(&mut d, json!({ GROUP: true })).await;
    assert_eq!(notes, vec![format!("{GROUP} turned on")]);
    assert_eq!(app.count("click control"), 1, "{:?}", app.log());
}

#[tokio::test]
async fn after_a_change_the_menu_opener_runs_again() {
    let (mut d, app) = app(true);
    let (out, _) = trip(&mut d, json!({})).await;
    assert!(out.is_ok(), "{out:?}");
    let log = app.log();
    let changed = log.iter().position(|l| l == "click control").expect("the control was clicked");
    let closed = log.iter().position(|l| l == "click close").expect("the dialog was closed");
    let opener = log.iter().position(|l| l == "click toggle").expect("after_sign_in ran");
    assert!(changed < closed && closed < opener, "{log:?}");
}

#[tokio::test]
async fn a_dialog_that_will_not_open_notes_it_and_walks_on() {
    let (mut d, app) = app(true);
    app.link_missing.store(true, Ordering::SeqCst);
    let (out, notes) = trip(&mut d, json!({})).await;
    assert_eq!(out, Ok("/hr/leave".to_string()), "the trip walks on");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("menu preferences could not be checked: the preferences dialog would not open"), "{notes:?}");
    assert_eq!(app.count("click control"), 0, "{:?}", app.log());
}

#[tokio::test]
async fn a_no_save_case_still_sets_its_preferences() {
    let (mut d, app) = app(true);
    d.honour_hold = true;
    d.call("Fetch.enable", json!({})).await.unwrap();
    // The page posts the new preference when the switch is clicked.
    let probed = app.probed.clone();
    d.saves_when.push((
        Box::new(move |method, params| {
            method == "Input.dispatchMouseEvent"
                && params["type"] == "mouseReleased"
                && *probed.lock().unwrap() == "css:#SectionWiseMode"
        }),
        "POST".to_string(),
        "https://hr.example.internal/User/SetUserPreferences".to_string(),
    ));
    let (out, notes) = trip(&mut d, json!({})).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert_eq!(notes, vec![format!("{GROUP} turned off")]);
    assert_eq!(d.take_save_blocked(), None, "the preference save was stopped");
    assert!(d.take_saves_stopped().is_empty());
    // The allowance was taken for the change and given back.
    assert_eq!(d.holds, vec![true, false]);
    // A save anywhere else is still stopped afterwards.
    assert!(d.is_guarding_saves());
}

#[tokio::test]
async fn the_run_report_and_the_replay_answer_say_what_was_changed() {
    let (mut d, _app) = app(true);
    let route = Route::new(&recipe(), leave(json!({})));
    let out = v2_lib::autorun::nav::reach_module(&mut d, &route, TripFrom::SignIn, &quick(), "test", None).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.detail, format!("Go to Leave ({GROUP} turned off)"));

    let reuse = v2_lib::autorun::replay_to::Reuse {
        menu: vec![format!("{GROUP} turned off")],
        ..Default::default()
    };
    assert_eq!(reuse.said(Some("admin")), Some(format!("Menu set up for admin: {GROUP} turned off.")));
    let reuse = v2_lib::autorun::replay_to::Reuse { went: Some("Leave".into()), ..reuse };
    assert_eq!(
        reuse.said(Some("admin")),
        Some(format!("Signed in already: went to Leave. Menu set up for admin: {GROUP} turned off."))
    );
}

// ------------------------------------------------------------ the area's record

struct FakeBrowser {
    d: ScriptedDriver,
    lease: Held,
    account: Option<String>,
    discovery: Option<DiscoveryState>,
    guarded_case: Option<i32>,
    guard_owner: Option<GuardOwner>,
}

impl DiscoveryBrowser for FakeBrowser {
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
    async fn alive(&mut self) -> bool {
        true
    }
    fn opened_by_assistant(&mut self) -> bool {
        false
    }
}

fn css(sel: &str) -> Target {
    serde_json::from_value(json!({ "css": sel })).unwrap()
}

#[tokio::test]
async fn an_area_save_records_the_preferences_in_force() {
    let dir = tempfile::tempdir().unwrap();
    save_recipe(dir.path(), ORG, PROJECT, &recipe()).unwrap();
    save_accounts(dir.path(), &[account()]).unwrap();
    let (d, app) = app_at(true, "/hr/leave");
    let discovery = Some(DiscoveryState {
        area: Some("Leave".to_string()),
        account: Some("admin".to_string()),
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
        explore: None,
        carried: Vec::new(),
    });
    let mut browser = Some(FakeBrowser {
        d,
        lease: Held::supervised(),
        account: Some("admin".to_string()),
        discovery,
        guarded_case: None,
        guard_owner: None,
    });
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", vec![css("#leave")], &quick()).await;
    assert_eq!(status, 200, "{body}");
    let said: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(said["saved"], true, "{body}");
    assert_eq!(said["preferences"], json!({ GROUP: true }), "{body}");
    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    let saved = find_area(&nav, "leave apply").expect("the area was saved");
    assert_eq!(saved.preferences.get(GROUP), Some(&true));
    // Reading changes nothing in the account.
    assert_eq!(app.count("click control"), 0, "{:?}", app.log());
    // It is in the file for the next run to read.
    let file = std::fs::read_to_string(v2_lib::autorun::nav::nav_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(file.contains("\"preferences\""), "{file}");
}

#[tokio::test]
async fn an_area_save_records_the_mode_it_was_saved_in() {
    let dir = tempfile::tempdir().unwrap();
    save_recipe(dir.path(), ORG, PROJECT, &recipe()).unwrap();
    save_accounts(dir.path(), &[account()]).unwrap();
    let (d, app) = app_at(false, "/hr/leave");
    *app.mode.lock().unwrap() = "Admin".to_string();
    let discovery = Some(DiscoveryState {
        area: Some("Leave".to_string()),
        account: Some("admin".to_string()),
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
        explore: None,
        carried: Vec::new(),
    });
    let mut browser = Some(FakeBrowser {
        d,
        lease: Held::supervised(),
        account: Some("admin".to_string()),
        discovery,
        guarded_case: None,
        guard_owner: None,
    });
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", vec![css("#leave")], &quick()).await;
    assert_eq!(status, 200, "{body}");
    let said: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(said["mode"], "Admin", "{body}");
    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    let saved = find_area(&nav, "leave apply").expect("the area was saved");
    assert_eq!(saved.mode.as_deref(), Some("Admin"));
    // Reading the mode switches nothing.
    assert_eq!(app.count("switch Self Service"), 0, "{:?}", app.log());
}

// ----------------------------------------------------------------- the recipe

fn recipe_with(menu_preferences: Value) -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "menu_preferences": menu_preferences,
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

fn setting(name: &str, css: &str) -> Value {
    json!({ "name": name, "control": { "css": css }, "on": false })
}

#[test]
fn recipe_validate_refuses_an_empty_or_duplicate_preference() {
    let open = json!([ { "css": "#userimg" } ]);
    let close = json!({ "css": "#closeprefs" });
    let with = |open: &Value, settings: Value| recipe_with(json!({ "open": open, "close": close, "settings": settings }));

    assert!(with(&open, json!([setting(GROUP, "#a")])).validate().is_ok());
    let err = with(&json!([]), json!([setting(GROUP, "#a")])).validate().unwrap_err();
    assert!(err.contains("\"open\" needs at least one click"), "{err}");
    let err = with(&open, json!([])).validate().unwrap_err();
    assert!(err.contains("\"settings\" needs at least one setting"), "{err}");
    let err = with(&open, json!([setting("  ", "#a")])).validate().unwrap_err();
    assert!(err.contains("needs a name"), "{err}");
    let err = with(&open, json!([setting(GROUP, "#a"), setting("group menu items", "#b")])).validate().unwrap_err();
    assert!(err.contains("used twice"), "{err}");
    let err = with(&open, json!([setting(GROUP, "#{{password}}")])).validate().unwrap_err();
    assert!(err.contains("{{username}} and {{password}}"), "{err}");
    let err = recipe_with(json!({ "open": [ { "css": "#{{username}}" } ], "close": close, "settings": [setting(GROUP, "#a")] }))
        .validate()
        .unwrap_err();
    assert!(err.contains("{{username}} and {{password}}"), "{err}");
}

#[test]
fn recipe_validate_refuses_a_single_mode() {
    let mode = |name: &str, shown: &str| json!({ "name": name, "shown": { "css": shown }, "switch": { "css": "#sw" } });
    let with = |modes: Value| -> SignInRecipe {
        serde_json::from_value(json!({
            "start_url": "https://hr.example.internal/",
            "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
            "modes": modes,
            "signed_in": { "css": "#marker" }
        }))
        .unwrap()
    };
    assert!(with(json!([mode("Admin", "#a"), mode("Self Service", "#b")])).validate().is_ok());
    let err = with(json!([mode("Admin", "#a")])).validate().unwrap_err();
    assert!(err.contains("one mode is nothing to switch between"), "{err}");
    let err = with(json!([mode("Admin", "#a"), mode(" ", "#b")])).validate().unwrap_err();
    assert!(err.contains("needs a name"), "{err}");
    let err = with(json!([mode("Admin", "#a"), mode("admin", "#b")])).validate().unwrap_err();
    assert!(err.contains("used twice"), "{err}");
    let err = with(json!([mode("Admin", "#{{password}}"), mode("Self Service", "#b")])).validate().unwrap_err();
    assert!(err.contains("{{username}} and {{password}}"), "{err}");
    // None named is fine, and writes nothing.
    let none = with(json!([]));
    assert!(none.validate().is_ok());
    assert!(!serde_json::to_string(&none).unwrap().contains("modes"));
}

#[test]
fn a_recipe_without_preferences_reads_and_writes_as_before() {
    let plain: SignInRecipe = serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap();
    assert!(plain.menu_preferences.is_none());
    assert!(!serde_json::to_string(&plain).unwrap().contains("menu_preferences"));
    assert!(leave(json!({})).preferences.is_empty());
    assert!(!serde_json::to_string(&leave(json!({}))).unwrap().contains("preferences"));
}

/// The mode switch may save the mode on the server: a case that must save
/// nothing still reaches its area, the hold taken for exactly the switch.
#[tokio::test]
async fn a_no_save_case_whose_area_needs_the_other_mode_still_reaches_it() {
    let (mut d, app) = app(false);
    *app.mode.lock().unwrap() = "Admin".to_string();
    d.honour_hold = true;
    d.call("Fetch.enable", json!({})).await.unwrap();
    let probed = app.probed.clone();
    d.saves_when.push((
        Box::new(move |method, params| {
            method == "Input.dispatchMouseEvent"
                && params["type"] == "mouseReleased"
                && *probed.lock().unwrap() == "css:#to-ss"
        }),
        "POST".to_string(),
        "https://hr.example.internal/Home/SetMode".to_string(),
    ));
    let (out, notes) = trip_in_mode(&mut d, Some("Self Service")).await;
    assert_eq!(out, Ok("/hr/leave".to_string()), "the switch was stopped by the guard");
    assert_eq!(notes, vec!["switched from Admin to Self Service mode".to_string()]);
    assert_eq!(d.take_save_blocked(), None);
    assert!(d.take_saves_stopped().is_empty());
    assert_eq!(d.holds, vec![true, false], "held for the switch only, then given back");
    assert!(d.is_guarding_saves(), "the guard stands afterwards");
}

#[tokio::test]
async fn no_save_hold_is_taken_when_the_account_is_already_in_the_areas_mode() {
    let (mut d, app) = app(false);
    *app.mode.lock().unwrap() = "Self Service".to_string();
    let (out, notes) = trip_in_mode(&mut d, Some("Self Service")).await;
    assert!(out.is_ok() && notes.is_empty(), "{out:?} {notes:?}");
    assert!(d.holds.is_empty(), "{:?}", d.holds);
}

// ------------------------------------------- areas that already exist (mapping)

use v2_lib::autorun::nav::put_path;
use v2_lib::commands::autorun::MappingRun;

/// A mapping run's own area "Leave Apply", saved earlier with `extra` fields
/// (preferences, mode) and the one click `#leave`.
fn mapped_before(root: &std::path::Path, extra: Value) {
    let mut path: Value = json!({
        "module": "Leave", "area": "Leave Apply", "clicks": [ { "css": "#leave" } ],
        "arrived": "/hr/leave", "recorded": "2026-10-20T10:00:00Z", "made_by": "mapping"
    });
    for (k, v) in extra.as_object().unwrap() {
        path[k] = v.clone();
    }
    put_path(root, ORG, PROJECT, serde_json::from_value(path).unwrap()).unwrap();
}

/// A mapping run over Leave saves "Leave Apply" again from `app`.
async fn map_again(root: &std::path::Path, d: ScriptedDriver) -> (u16, Value) {
    let discovery = Some(DiscoveryState {
        area: Some("Leave".to_string()),
        account: Some("admin".to_string()),
        started_at: 1,
        tried: Vec::new(),
        mapping: Some(MappingRun::new(&["Leave".to_string()])),
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
        explore: None,
        carried: Vec::new(),
    });
    let mut browser = Some(FakeBrowser {
        d,
        lease: Held::supervised(),
        account: Some("admin".to_string()),
        discovery,
        guarded_case: None,
        guard_owner: None,
    });
    let (status, body) =
        discover_area_in(&mut browser, root, ORG, PROJECT, "Leave Apply", "Leave", vec![css("#leave")], &quick()).await;
    (status, serde_json::from_str(&body).unwrap_or(Value::String(body)))
}

fn project_files(dir: &std::path::Path) {
    save_recipe(dir, ORG, PROJECT, &recipe()).unwrap();
    save_accounts(dir, &[account()]).unwrap();
}

#[tokio::test]
async fn an_existing_area_with_the_same_clicks_gains_the_mode_and_preferences_it_lacks() {
    let dir = tempfile::tempdir().unwrap();
    project_files(dir.path());
    mapped_before(dir.path(), json!({}));
    let (d, app) = app_at(true, "/hr/leave");
    *app.mode.lock().unwrap() = "Admin".to_string();
    let (status, said) = map_again(dir.path(), d).await;
    assert_eq!(status, 200, "{said}");
    assert!(said.get("unchanged").is_none(), "not answered as unchanged: {said}");
    assert_eq!(said["saved"], true, "{said}");
    assert_eq!(said["message"], "unchanged path; recorded its mode/menu preferences", "{said}");
    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    let saved = find_area(&nav, "leave apply").unwrap();
    assert_eq!(saved.mode.as_deref(), Some("Admin"));
    assert_eq!(saved.preferences.get(GROUP), Some(&true));
    assert_eq!(saved.clicks.len(), 1, "the clicks stand as they were");
    // Run again: now it is unchanged for real.
    let (d, app) = app_at(true, "/hr/leave");
    *app.mode.lock().unwrap() = "Admin".to_string();
    let (_, said) = map_again(dir.path(), d).await;
    assert_eq!((said["saved"].clone(), said["unchanged"].clone()), (json!(false), json!(true)), "{said}");
}

#[tokio::test]
async fn a_failed_read_keeps_what_the_area_already_held() {
    let dir = tempfile::tempdir().unwrap();
    project_files(dir.path());
    mapped_before(dir.path(), json!({ "mode": "Self Service", "preferences": { GROUP: false } }));
    // The preferences dialog will not open, and no mode shows.
    let (d, app) = app_at(true, "/hr/leave");
    app.link_missing.store(true, Ordering::SeqCst);
    let (status, said) = map_again(dir.path(), d).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!((said["saved"].clone(), said["unchanged"].clone()), (json!(false), json!(true)), "{said}");
    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    let saved = find_area(&nav, "leave apply").unwrap();
    assert_eq!(saved.mode.as_deref(), Some("Self Service"));
    assert_eq!(saved.preferences.get(GROUP), Some(&false));
}

#[tokio::test]
async fn a_repair_keeps_the_mode_and_preferences_it_read() {
    use v2_lib::ai_bridge::repair_area_in;
    let dir = tempfile::tempdir().unwrap();
    project_files(dir.path());
    // A person's area whose old click no longer exists, recorded with no mode.
    let old: ModulePath = serde_json::from_value(json!({
        "module": "Leave", "area": "Leave Apply", "clicks": [ { "css": "#old" } ],
        "arrived": "/hr/leave", "recorded": "2026-10-20T10:00:00Z"
    }))
    .unwrap();
    put_path(dir.path(), ORG, PROJECT, old).unwrap();
    let (d, app) = app_at(true, "/hr/leave");
    *app.mode.lock().unwrap() = "Admin".to_string();
    let discovery = Some(DiscoveryState {
        area: Some("Leave".to_string()),
        account: Some("admin".to_string()),
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
        explore: None,
        carried: Vec::new(),
    });
    let mut browser = Some(FakeBrowser {
        d,
        lease: Held::supervised(),
        account: Some("admin".to_string()),
        discovery,
        guarded_case: None,
        guard_owner: None,
    });
    let (status, body) = repair_area_in(
        &mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "", vec![css("#leave")], "the menu item moved", &quick(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    let saved = find_area(&nav, "leave apply").unwrap();
    assert!(saved.repaired_by_assistant.is_some(), "{saved:?}");
    assert_eq!(saved.mode.as_deref(), Some("Admin"));
    assert!(saved.preferences.contains_key(GROUP), "{:?}", saved.preferences);
}

// ---------------------------------------------- the person's recording

/// The recording saves the state it STARTED in: read before the first click
/// is captured, so a recording that clicks "Switch to Self Service" is still
/// for the mode it began in, and a fresh browser's check begins there too.
#[tokio::test]
async fn a_recording_reads_the_mode_and_preferences_it_starts_in() {
    use v2_lib::commands::autorun_record::read_start_state;
    let (mut d, app) = app_at(true, "/");
    *app.mode.lock().unwrap() = "Admin".to_string();
    let (prefs, mode) = read_start_state(&mut d, &recipe(), &quick()).await;
    assert_eq!(prefs, Some([(GROUP.to_string(), true)].into_iter().collect()));
    assert_eq!(mode.as_deref(), Some("Admin"));
    // Then the recorded clicks switch to Self Service; the state read stands.
    *app.mode.lock().unwrap() = "Self Service".to_string();
    assert_eq!(mode.as_deref(), Some("Admin"));
    // The path saved with it walks from an Admin account without a switch,
    // and from a Self Service one switches back first.
    let mut path = leave(json!({}));
    path.clicks = vec![css("#to-ss"), css("#leave")];
    path.mode = mode;
    let route = Route::new(&recipe(), path);
    *app.mode.lock().unwrap() = "Admin".to_string();
    let mut notes = Vec::new();
    let out = go_to_module_noted(&mut d, &route, TripFrom::Elsewhere, &quick(), &mut notes).await;
    assert!(out.is_ok(), "{:?}", out.map_err(|f| f.for_dialog()));
    assert!(notes.is_empty(), "no switch for an account already in the starting mode: {notes:?}");
    // A dialog that cannot be read leaves no preferences.
    app.link_missing.store(true, Ordering::SeqCst);
    let (prefs, _) = read_start_state(&mut d, &recipe(), &quick()).await;
    assert_eq!(prefs, None);
}

/// The reads happen before listening starts, and not again at Stop.
#[test]
fn the_recording_reads_its_state_before_it_listens_and_not_at_stop() {
    let src = include_str!("../../src/commands/autorun_record.rs").replace(char::from(13), "");
    let start = &src[src.find("pub async fn auto_run_record_start").unwrap()..src.find("pub async fn auto_run_record_stop").unwrap()];
    let read = start.find("read_start_state(").expect("the start reads the state");
    let listen = start.find("listen(&mut cdp").expect("the recording listens");
    assert!(read < listen, "read after listening began");
    assert!(!start[listen..].contains("read_mode") && !start[listen..].contains("read_preferences"), "read again when it stops");
}

/// A re-record whose read failed keeps what the area held.
#[test]
fn a_re_recorded_area_keeps_values_a_failed_read_could_not_give() {
    let dir = tempfile::tempdir().unwrap();
    mapped_before(dir.path(), json!({ "mode": "Self Service", "preferences": { GROUP: false } }));
    let mut again = leave(json!({}));
    again.area = "Leave Apply".to_string();
    v2_lib::autorun::nav::keep_recorded_values(dir.path(), ORG, PROJECT, &mut again);
    assert_eq!(again.mode.as_deref(), Some("Self Service"));
    assert_eq!(again.preferences.get(GROUP), Some(&false));
    // A read that worked wins.
    let mut read = leave(json!({ GROUP: true }));
    read.area = "Leave Apply".to_string();
    read.mode = Some("Admin".to_string());
    v2_lib::autorun::nav::keep_recorded_values(dir.path(), ORG, PROJECT, &mut read);
    assert_eq!((read.mode.as_deref(), read.preferences.get(GROUP)), (Some("Admin"), Some(&true)));
}
