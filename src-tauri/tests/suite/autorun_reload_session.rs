//! The five actions added for cases Auto Run could not run before: a page
//! refresh (`reload`, then `return_to_area` - the one way back in a project
//! that refuses `navigate`), a timed-out session (`expire_session`), and the
//! keyboard (`press_key`, `expect_focused`). The browser is a fake, as in
//! `browser_actions`; `browser_live` covers a real one.

use crate::common;

use common::{FakePage, ScriptedDriver};
use serde_json::{json, Value};
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::nav::{save_nav, ModulePath, NavFile, Route};
use v2_lib::autorun::recipe::{save_recipe, SignInRecipe};
use v2_lib::autorun::runner::{area_route, run_step_routed, AreaRoute, NEEDS_SCRIPT_AREA, NO_AREA_IN_RUN};
use v2_lib::autorun::{store, CaseScript, StepScript};
use v2_lib::browser::actions::{
    execute_with, Action, FOCUS_NOW_JS, NO_SESSION_TO_END, PRESS_KEYS, RELOAD_DID_NOT_FINISH,
};
use v2_lib::browser::cdp::Event;
use v2_lib::browser::expect::{FOCUSED_JS, NOT_FOCUSED};
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn action(v: Value) -> Action {
    serde_json::from_value(v).expect("an action")
}

fn lifecycle(frame: &str, name: &str) -> Event {
    Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": frame, "name": name }) }
}

// ---- What a script may say ----------------------------------------------

#[test]
fn the_new_kinds_read_from_a_script_and_validate() {
    for v in [
        json!({ "kind": "reload" }),
        json!({ "kind": "expire_session" }),
        json!({ "kind": "return_to_area" }),
        json!({ "kind": "press_key", "key": "Tab" }),
        json!({ "kind": "press_key", "key": "Shift+Tab" }),
        json!({ "kind": "expect_focused", "selector": { "role": "button", "name": "Activate" } }),
    ] {
        let a = action(v.clone());
        assert!(a.validate().is_ok(), "{v}: {:?}", a.validate());
        // And it writes back as it was read.
        assert_eq!(serde_json::to_value(&a).unwrap(), v);
    }
}

#[test]
fn press_key_takes_only_the_keys_it_knows_and_says_which() {
    let why = action(json!({ "kind": "press_key", "key": "F5" })).validate().unwrap_err();
    assert!(why.contains("\"F5\""), "{why}");
    for (name, ..) in PRESS_KEYS {
        assert!(why.contains(name), "{why} should list {name}");
    }
    // A key's own name, not a lower-case guess at one.
    assert!(action(json!({ "kind": "press_key", "key": "tab" })).validate().is_err());
}

#[test]
fn only_expect_focused_counts_as_a_check() {
    assert!(action(json!({ "kind": "expect_focused", "selector": { "css": "#a" } })).is_check());
    for k in ["reload", "expire_session", "return_to_area"] {
        assert!(!action(json!({ "kind": k })).is_check(), "{k}");
    }
    assert!(!action(json!({ "kind": "press_key", "key": "Tab" })).is_check());
}

#[test]
fn a_guard_may_press_a_key_but_never_end_the_session_or_move_the_case() {
    let guarded = |inner: Value| {
        action(json!({ "kind": "when_visible", "selector": { "css": "#banner" }, "then": [inner] })).validate()
    };
    assert!(guarded(json!({ "kind": "press_key", "key": "Escape" })).is_ok());
    assert!(guarded(json!({ "kind": "reload" })).is_ok());
    for k in ["expire_session", "return_to_area"] {
        let why = guarded(json!({ "kind": k })).unwrap_err();
        assert!(why.contains(k), "{why}");
    }
    // expect_focused is a check, and a guard holds none.
    assert!(guarded(json!({ "kind": "expect_focused", "selector": { "css": "#a" } })).is_err());
}

#[test]
fn a_sign_in_recipe_cannot_end_the_session_or_go_to_an_area() {
    for k in ["expire_session", "return_to_area"] {
        let r: Result<SignInRecipe, _> = serde_json::from_value(json!({
            "start_url": "https://hr.example.internal/login",
            "steps": [ { "kind": k } ],
            "signed_in": { "css": "#marker" }
        }));
        let refused = match r {
            Err(e) => e.to_string(),
            Ok(recipe) => recipe.validate().unwrap_err(),
        };
        assert!(refused.contains(k) && refused.contains("case script"), "{refused}");
    }
}

// ---- reload ---------------------------------------------------------------

/// A browser on `before` until the reload, then on `after`.
fn reloading(before: &'static str, after: &'static str) -> ScriptedDriver {
    let mut reloaded = false;
    ScriptedDriver::new(move |method, params| {
        Ok(match method {
            "Page.reload" => {
                reloaded = true;
                json!({})
            }
            "Page.getFrameTree" => json!({ "frameTree": { "frame": { "id": "F" } } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => {
                json!({ "result": { "value": if reloaded { after } else { before } } })
            }
            _ => json!({}),
        })
    })
}

#[tokio::test]
async fn reload_waits_for_the_main_frames_own_load_and_says_where_it_landed() {
    let mut d = reloading("https://hr.example.internal/hr/wizard", "https://hr.example.internal/hr/home/index");
    // A load from before the reload must not count.
    d.events.push_back(lifecycle("F", "load"));
    // A sub-frame's load and the main frame's earlier stage come first.
    for ev in [lifecycle("SUB", "load"), lifecycle("F", "DOMContentLoaded"), lifecycle("F", "load")] {
        d.on_call_events.push(("Page.reload".into(), ev));
    }
    let out = execute_with(&mut d, &action(json!({ "kind": "reload" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(d.calls_to("Page.reload"), vec![json!({ "ignoreCache": false })]);
    assert!(
        out.detail.contains("the page is now https://hr.example.internal/hr/home/index"),
        "a reload that went home says so: {}",
        out.detail
    );
    assert!(d.events.is_empty(), "every lifecycle event was read: {:?}", d.events);
}

#[tokio::test]
async fn reload_gives_up_when_only_a_sub_frame_loads() {
    let mut d = reloading("https://hr.example.internal/hr/wizard", "https://hr.example.internal/hr/wizard");
    d.on_call_events.push(("Page.reload".into(), lifecycle("SUB", "load")));
    let out = execute_with(&mut d, &action(json!({ "kind": "reload" })), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains(RELOAD_DID_NOT_FINISH), "{}", out.detail);
}

// ---- expire_session -------------------------------------------------------

fn with_cookies(cookies: Value) -> ScriptedDriver {
    ScriptedDriver::new(move |method, params| {
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "location.href" => {
                json!({ "result": { "value": "https://hr.example.internal/hr/wizard?step=2" } })
            }
            "Network.getCookies" => json!({ "cookies": cookies.clone() }),
            _ => json!({}),
        })
    })
}

#[tokio::test]
async fn expire_session_drops_every_cookie_of_the_site_and_names_none() {
    let mut d = with_cookies(json!([
        { "name": "ASP.NET_SessionId", "value": "s3cret-session", "domain": "hr.example.internal", "path": "/" },
        { "name": ".ASPXAUTH", "value": "s3cret-auth", "domain": ".example.internal", "path": "/hr" }
    ]));
    let out = execute_with(&mut d, &action(json!({ "kind": "expire_session" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    // Asked for the page's own address, and every one dropped where it lives.
    assert_eq!(d.calls_to("Network.getCookies")[0]["urls"][0], "https://hr.example.internal/hr/wizard?step=2");
    assert_eq!(
        d.calls_to("Network.deleteCookies"),
        vec![
            json!({ "name": "ASP.NET_SessionId", "domain": "hr.example.internal", "path": "/" }),
            json!({ "name": ".ASPXAUTH", "domain": ".example.internal", "path": "/hr" }),
        ]
    );
    assert!(out.detail.contains("dropped 2 cookie(s)"), "{}", out.detail);
    assert!(out.detail.contains("https://hr.example.internal"), "{}", out.detail);
    // A cookie is a credential: neither its value nor its name is said.
    for secret in ["s3cret", "ASP.NET_SessionId", ".ASPXAUTH"] {
        assert!(!out.detail.contains(secret), "{} names {secret}", out.detail);
    }
}

#[tokio::test]
async fn expire_session_with_no_session_fails_and_says_why() {
    let mut d = with_cookies(json!([]));
    let out = execute_with(&mut d, &action(json!({ "kind": "expire_session" })), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, NO_SESSION_TO_END);
    assert!(d.calls_to("Network.deleteCookies").is_empty());
}

// ---- press_key / expect_focused --------------------------------------------

/// `FakePage` for locators, with the focus answered here: `focused` for the
/// element, `now` for where the page says the focus is.
fn focus_page(focused: bool, now: &'static str) -> ScriptedDriver {
    let page = FakePage::default();
    ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.callFunctionOn" if f == FOCUSED_JS => Ok(json!({ "result": { "value": focused } })),
            "Runtime.callFunctionOn" if f == FOCUS_NOW_JS => Ok(json!({ "result": { "value": now } })),
            _ => page.answer(method, params),
        }
    })
}

#[tokio::test]
async fn press_key_sends_the_key_down_and_up_and_says_where_the_focus_went() {
    let mut d = focus_page(true, "button \"Activate\"");
    let out = execute_with(&mut d, &action(json!({ "kind": "press_key", "key": "Tab" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    let sent = d.calls_to("Input.dispatchKeyEvent");
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[0]["type"], "rawKeyDown");
    assert_eq!(sent[1]["type"], "keyUp");
    for e in &sent {
        assert_eq!(e["key"], "Tab");
        assert_eq!(e["windowsVirtualKeyCode"], 9);
        assert_eq!(e["modifiers"], 0);
    }
    assert_eq!(out.detail, "pressed Tab; the focus is on button \"Activate\"");
}

#[tokio::test]
async fn shift_tab_holds_shift_and_enter_types_its_character() {
    let mut d = focus_page(true, "");
    assert!(execute_with(&mut d, &action(json!({ "kind": "press_key", "key": "Shift+Tab" })), &quick()).await.ok);
    let back = d.calls_to("Input.dispatchKeyEvent");
    // Shift goes down first and comes up last; the Tab between carries it.
    let seen: Vec<(&str, &str, i64)> = back
        .iter()
        .map(|e| (e["type"].as_str().unwrap(), e["key"].as_str().unwrap(), e["modifiers"].as_i64().unwrap()))
        .collect();
    assert_eq!(
        seen,
        [("rawKeyDown", "Shift", 8), ("rawKeyDown", "Tab", 8), ("keyUp", "Tab", 8), ("keyUp", "Shift", 0)],
        "Shift is held: {back:?}"
    );

    let mut d = focus_page(true, "");
    let out = execute_with(&mut d, &action(json!({ "kind": "press_key", "key": "Enter" })), &quick()).await;
    let enter = d.calls_to("Input.dispatchKeyEvent");
    // A key that types something goes down as keyDown, with its text, so
    // the page's default (submitting, pressing) happens.
    assert_eq!(enter[0]["type"], "keyDown");
    assert_eq!(enter[0]["text"], "\r");
    assert_eq!(out.detail, "pressed Enter; nothing on the page has the focus");
}

#[tokio::test]
async fn expect_focused_passes_on_the_focused_element() {
    let mut d = focus_page(true, "button \"Activate\"");
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_focused", "selector": { "css": "#activate" } })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert!(out.detail.contains("has the focus"), "{}", out.detail);
}

#[tokio::test]
async fn expect_focused_fails_naming_where_the_focus_actually_is() {
    let mut d = focus_page(false, "button \"Use Custom Levels\"");
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_focused", "selector": { "css": "#activate" }, "timeout_ms": 50 })),
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains(NOT_FOCUSED), "{}", out.detail);
    assert!(out.detail.contains("the focus is on button \"Use Custom Levels\""), "{}", out.detail);
}

// ---- return_to_area --------------------------------------------------------

fn step(actions: Value) -> StepScript {
    serde_json::from_value(json!({ "step_number": 8, "actions": actions })).unwrap()
}

/// PeoplesHR's shape: a recipe whose home is its login page, and
/// `after_sign_in` opening a menu a fresh load draws closed.
fn home_recipe() -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/hr/security/login",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "after_sign_in": [ { "kind": "when_visible", "selector": { "css": "#toggle:not(.active)" }, "within_ms": 100,
            "then": [ { "kind": "click", "selector": { "css": "#toggle" } } ] } ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

fn wizard_area() -> ModulePath {
    serde_json::from_value(json!({
        "module": "Performance",
        "area": "Definition Wizard",
        "clicks": [ { "role": "link", "name": "Definition Wizard", "exact": true } ],
        "arrived": "/hr/pms/wizard",
        "recorded": "2026-10-05T10:00:00Z"
    }))
    .unwrap()
}

#[tokio::test]
async fn return_to_area_takes_the_recorded_path_back_and_the_step_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, app) = common::menu_app(&[("link", "Definition Wizard", "/hr/pms/wizard")], "/hr/home/index", 0);
    // Where a reload left it: home, not the wizard.
    *app.path.lock().unwrap() = "/hr/home/index".to_string();
    let route = Route::new(&home_recipe(), wizard_area());
    let mut account = Some("hr.admin".to_string());
    let mut held = Held::supervised();
    let s = step(json!([ { "kind": "return_to_area" }, { "kind": "check_text", "value": "yes" } ]));
    let out = run_step_routed(&mut d, dir.path(), "Acme", "PMS", &s, &quick(), &mut account, &mut held, None, AreaRoute::To(&route))
        .await
        .unwrap();
    assert!(out[0].ok, "{out:?}");
    assert!(app.log.lock().unwrap().contains(&"click Definition Wizard".to_string()), "{:?}", app.log.lock().unwrap());
    assert_eq!(*app.path.lock().unwrap(), "/hr/pms/wizard");
    assert!(out[1].ok, "the step went on after reaching the area: {out:?}");
}

#[tokio::test]
async fn return_to_area_with_no_route_fails_and_stops_the_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = FakePage::default().driver();
    let mut account = None;
    let mut held = Held::supervised();
    let s = step(json!([ { "kind": "return_to_area" }, { "kind": "click", "selector": { "css": "#save" } } ]));
    let out = run_step_routed(&mut d, dir.path(), "Acme", "PMS", &s, &quick(), &mut account, &mut held, None, AreaRoute::Unknown(NO_AREA_IN_RUN))
        .await
        .unwrap();
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, NO_AREA_IN_RUN);
    assert!(!out[1].ok && out[1].detail.starts_with("not run:"), "the click ran on the wrong screen: {out:?}");
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty(), "nothing was clicked");
}

fn script_with_area(case_id: i32, area: Option<&str>) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": case_id,
        "title": "Proficiency Levels - Page refresh after Save retains the active scale",
        "area": area,
        "steps": [ { "step_number": 8, "actions": [ { "kind": "reload" }, { "kind": "return_to_area" } ] } ]
    }))
    .unwrap()
}

#[test]
fn a_watched_run_finds_its_area_from_the_saved_script() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_nav(root, "Acme", "PMS", &NavFile { direct_urls: false, modules: vec![wizard_area()], save_words: vec![] }).unwrap();
    save_recipe(root, "Acme", "PMS", &home_recipe()).unwrap();

    store::save_script(root, &script_with_area(135523, Some("Definition Wizard"))).unwrap();
    let route = area_route(root, "Acme", "PMS", 135523).unwrap();
    assert_eq!(route.path.name(), "Definition Wizard");
    assert_eq!(route.home.start_url, "https://hr.example.internal/hr/security/login");

    // No area on the script: a watched run does not know the Module.
    store::save_script(root, &script_with_area(135524, None)).unwrap();
    assert_eq!(area_route(root, "Acme", "PMS", 135524).unwrap_err(), NEEDS_SCRIPT_AREA);

    // An area the project has not recorded.
    store::save_script(root, &script_with_area(135525, Some("Appraisals"))).unwrap();
    assert!(area_route(root, "Acme", "PMS", 135525).unwrap_err().contains("Appraisals"));

    // No script at all.
    assert!(area_route(root, "Acme", "PMS", 999).unwrap_err().contains("no saved script"));
}
