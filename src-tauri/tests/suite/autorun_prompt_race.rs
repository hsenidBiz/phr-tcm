//! The prompts a signed-in page may or may not show (a cookie banner, the
//! "another active session" modal, a menu drawn closed) are watched
//! together in ONE window, not each waited out in turn: a page that shows
//! none of them costs one short window, not the sum of every prompt's own.
//! The window is short after a saved session or a trip home, and longer
//! after a fresh login, where the session modal can arrive late.
//!
//! These run on a fake clock (`ScriptedDriver::idle_clock`): an idle adds
//! its wait to the clock and returns at once, so "the modal shows 3 s in"
//! costs no real time and the time a sign-in spent waiting is read back
//! exactly.

use crate::common;

use common::{account, quick, ScriptedDriver, PASSWORD};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::nav::{load_home, Home};
use v2_lib::autorun::recipe::SignInRecipe;
use v2_lib::autorun::sessions::{now_ms, save_session};
use v2_lib::autorun::signin::sign_in;
use v2_lib::autorun::timing::{FRESH_LOGIN_WINDOW_MS, PROMPT_WINDOW_MS};
use v2_lib::browser::cdp::Event;
use v2_lib::browser::input::{HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;
use v2_lib::browser::session::SavedSession;

/// Slack on top of a window: the idles a click's readiness check and the
/// loop's last poll add. Far below any second prompt's own window.
const MARGIN_MS: u64 = 200;

/// A sign-in like the built-in one: a form, then three optional prompts
/// with the built-in recipe's own windows (4 s, 4 s, 5 s).
fn prompt_recipe(sign_in_steps: serde_json::Value) -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": sign_in_steps,
        "after_sign_in": [
            { "kind": "when_visible", "selector": { "css": "#cookie" }, "within_ms": 4000,
              "then": [ { "kind": "click", "selector": { "css": "#cookie" } } ] },
            { "kind": "when_visible", "selector": { "css": "#modal" }, "within_ms": 4000,
              "then": [ { "kind": "click", "selector": { "css": "#modal" } } ] },
            { "kind": "when_visible", "selector": { "css": "#sidebar" }, "within_ms": 5000,
              "then": [ { "kind": "click", "selector": { "css": "#sidebar" } } ] }
        ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

fn form() -> serde_json::Value {
    json!([
        { "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" },
        { "kind": "fill", "selector": { "css": "#pass" }, "value": "{{password}}" },
        { "kind": "click", "selector": { "css": "#go" } }
    ])
}

/// What the fake page did: the clock (ms idled so far) and what was
/// clicked, by css, in order.
struct PromptApp {
    clock: Arc<AtomicU64>,
    clicked: Arc<Mutex<Vec<String>>>,
}

impl PromptApp {
    fn now(&self) -> u64 {
        self.clock.load(Ordering::SeqCst)
    }
    fn clicked(&self) -> Vec<String> {
        self.clicked.lock().unwrap().clone()
    }
}

/// A page whose form, home marker and buttons are always there, and whose
/// prompts each show from a time on the fake clock until clicked. A css
/// the page does not know is never there.
fn prompt_app(prompts: &[(&'static str, u64)]) -> (ScriptedDriver, PromptApp) {
    let clock = Arc::new(AtomicU64::new(0));
    let clicked = Arc::new(Mutex::new(Vec::<String>::new()));
    let app = PromptApp { clock: clock.clone(), clicked: clicked.clone() };
    let prompts: Vec<(String, u64)> = prompts.iter().map(|(c, at)| (c.to_string(), *at)).collect();
    let mut last_selector = String::new();
    let ready = json!({ "visible": true, "onscreen": true, "enabled": true, "editable": true, "hit": true,
        "x": 5.0, "y": 5.0, "covered_by": "", "rect": [0.0, 0.0, 10.0, 10.0] });
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Page.navigate" => json!({ "frameId": "F", "loaderId": "L" }),
            "Network.getAllCookies" => json!({ "cookies": [
                { "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true }
            ] }),
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": ready } }),
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last_selector = params["arguments"][0]["value"].as_str().unwrap().to_string();
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.callFunctionOn" => json!({ "result": { "value": "text" } }),
            "Runtime.getProperties" => {
                let now = clock.load(Ordering::SeqCst);
                let there = match prompts.iter().find(|(c, _)| *c == last_selector) {
                    Some((c, at)) => now >= *at && !clicked.lock().unwrap().contains(c),
                    None => ["#user", "#pass", "#go", "#marker"].contains(&last_selector.as_str()),
                };
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Input.dispatchMouseEvent" => {
                if params["type"] == "mouseReleased" {
                    clicked.lock().unwrap().push(last_selector.clone());
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
    d.idle_clock = Some(app.clock.clone());
    (d, app)
}

fn with_saved_session(dir: &std::path::Path) {
    let saved = SavedSession {
        saved_at_ms: now_ms(),
        cookies: vec![json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true })],
        local_storage: vec![],
    };
    save_session(dir, "admin", &saved).unwrap();
}

#[tokio::test]
async fn prompts_that_never_appear_cost_one_window_not_five() {
    // After a saved session: one short window, not 4 + 4 + 5 s.
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = prompt_app(&[]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok && out.used_saved_session, "{}", out.detail);
    assert!(app.now() >= PROMPT_WINDOW_MS, "the window was not watched out: {} ms", app.now());
    assert!(app.now() <= PROMPT_WINDOW_MS + MARGIN_MS, "waited {} ms for prompts that never came", app.now());
    let carried: Vec<_> = out.steps.iter().filter(|s| s.detail.contains("did not appear, carried on")).collect();
    assert_eq!(carried.len(), 3, "each prompt still says it was carried past: {:?}", out.steps);
    assert!(app.clicked().is_empty(), "{:?}", app.clicked());

    // After a fresh login: one longer window, still not the sum.
    let dir = tempfile::tempdir().unwrap();
    let (mut d, app) = prompt_app(&[]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok && !out.used_saved_session, "{}", out.detail);
    assert!(app.now() <= FRESH_LOGIN_WINDOW_MS + MARGIN_MS, "waited {} ms after a fresh login", app.now());
    assert!(!serde_json::to_string(&out).unwrap().contains(PASSWORD));
}

#[tokio::test]
async fn a_late_session_modal_after_a_fresh_login_is_still_dismissed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, app) = prompt_app(&[("#modal", 3000)]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok && !out.used_saved_session, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#go", "#modal"], "the modal 3 s in was dismissed");
    assert_eq!(out.appeared.len(), 1, "{:?}", out.appeared);
    assert!(out.appeared[0].contains("#modal"), "{:?}", out.appeared);
}

#[tokio::test]
async fn a_saved_session_reuse_uses_the_short_window() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    // Something that would only show 3 s in is past the short window.
    let (mut d, app) = prompt_app(&[("#modal", 3000)]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok && out.used_saved_session, "{}", out.detail);
    assert!(app.clicked().is_empty(), "the short window should have ended first: {:?}", app.clicked());
    assert!(app.now() <= PROMPT_WINDOW_MS + MARGIN_MS, "waited {} ms", app.now());
}

#[tokio::test]
async fn prompts_are_handled_in_recipe_order_when_several_appear() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = prompt_app(&[("#sidebar", 0), ("#modal", 0), ("#cookie", 0)]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#cookie", "#modal", "#sidebar"]);
    assert_eq!(out.appeared.len(), 3, "{:?}", out.appeared);
    // Every prompt handled: the window ends there, not at its end.
    assert!(app.now() < PROMPT_WINDOW_MS / 2, "waited {} ms with nothing left to watch", app.now());
}

#[tokio::test]
async fn a_trip_home_uses_the_short_window() {
    let r = prompt_recipe(form());
    let (mut d, app) = prompt_app(&[("#cookie", 0), ("#modal", 3000)]);
    let out = load_home(&mut d, &Home::of(&r), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#cookie"], "the modal 3 s in is past the short window");
    assert!(app.now() >= PROMPT_WINDOW_MS, "the rest of the window was not watched: {} ms", app.now());
    assert!(app.now() <= PROMPT_WINDOW_MS + MARGIN_MS, "waited {} ms", app.now());
}

/// A sign-in step's optional prompt is looked for once, not waited out,
/// when the login form is already showing: the page has loaded, and a
/// banner that is not there now is not coming.
#[tokio::test]
async fn a_sign_in_prompt_is_looked_for_once_when_the_login_field_is_showing() {
    let dir = tempfile::tempdir().unwrap();
    let steps = json!([
        { "kind": "when_visible", "selector": { "css": "#banner" }, "within_ms": 3000,
          "then": [ { "kind": "click", "selector": { "css": "#banner" } } ] },
        { "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" },
        { "kind": "fill", "selector": { "css": "#pass" }, "value": "{{password}}" },
        { "kind": "click", "selector": { "css": "#go" } }
    ]);
    let mut r = prompt_recipe(steps);
    r.after_sign_in.clear();
    let (mut d, app) = prompt_app(&[]);
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(out.steps.iter().any(|s| s.detail.contains("#banner") && s.detail.contains("did not appear, carried on")), "{:?}", out.steps);
    assert!(app.now() < 500, "the banner was waited for: {} ms", app.now());

    // And one that IS showing is still dismissed.
    let dir = tempfile::tempdir().unwrap();
    let (mut d, app) = prompt_app(&[("#banner", 0)]);
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#banner", "#go"]);
}
