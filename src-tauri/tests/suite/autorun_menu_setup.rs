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
    };
    let (log, saved, shown, probed, link_missing) =
        (app.log.clone(), app.saved.clone(), app.shown.clone(), app.probed.clone(), app.link_missing.clone());
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
                let there = !(last_css == "#prefslink" && link_missing.load(Ordering::SeqCst));
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
                    "css:#leave" => *path.lock().unwrap() = "/hr/leave".to_string(),
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

#[tokio::test]
async fn an_account_already_matching_changes_nothing() {
    // Off is what the recipe's areas need, and what the account has.
    let (mut d, app) = app(false);
    let (out, notes) = trip(&mut d, json!({})).await;
    assert_eq!(out, Ok("/hr/leave".to_string()));
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(app.count("click control"), 0, "{:?}", app.log());
    // The dialog was opened to look, and closed again.
    assert_eq!(app.log(), vec!["open userimg", "open prefslink", "click close"]);
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
    let out = v2_lib::autorun::nav::reach_module(&mut d, &route, TripFrom::SignIn, &quick(), "test").await;
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
