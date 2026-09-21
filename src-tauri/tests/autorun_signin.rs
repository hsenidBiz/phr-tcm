//! Getting from a fresh browser to a signed-in one, for one account.

mod common;

use common::ScriptedDriver;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use v2_lib::autorun::accounts::{save_accounts, session_path, Account};
use v2_lib::autorun::recipe::{save_recipe, SignInRecipe};
use v2_lib::autorun::sessions::{now_ms, save_session};
use v2_lib::autorun::signin::{prepare, redact, sign_in};
use v2_lib::browser::cdp::Event;
use v2_lib::browser::input::{HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;
use v2_lib::browser::session::SavedSession;
use v2_lib::browser::timing::Timing;

const PASSWORD: &str = "s3cret-Value";

fn quick() -> Timing {
    Timing { action_ms: 400, expect_ms: 150, nav_ms: 300, poll_ms: 10, highlight_ms: 0 }
}

fn account() -> Account {
    Account { key: "admin".into(), label: "Administrator".into(), username: "kim".into(), password: PASSWORD.into() }
}

fn recipe() -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": [
            { "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" },
            { "kind": "fill", "selector": { "css": "#pass" }, "value": "{{password}}" },
            { "kind": "click", "selector": { "css": "#go" } },
            { "kind": "when_visible", "selector": { "css": "#other-session" }, "within_ms": 60,
              "then": [ { "kind": "click", "selector": { "css": "#other-session" } } ] }
        ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

/// A page with a login form. `#marker` exists only once `signed_in` is
/// set, which happens when the password has been typed and `#go` clicked,
/// or from the start when `cookie_is_good` and cookies were restored.
struct App {
    signed_in: Arc<AtomicBool>,
    typed_password: Arc<AtomicBool>,
    restored: Arc<AtomicBool>,
    clicks: Arc<AtomicUsize>,
}

fn app(cookie_is_good: bool, broken_selector: Option<&'static str>) -> (ScriptedDriver, App) {
    let state = App {
        signed_in: Arc::new(AtomicBool::new(false)),
        typed_password: Arc::new(AtomicBool::new(false)),
        restored: Arc::new(AtomicBool::new(false)),
        clicks: Arc::new(AtomicUsize::new(0)),
    };
    let (signed_in, typed, restored, clicks) =
        (state.signed_in.clone(), state.typed_password.clone(), state.restored.clone(), state.clicks.clone());
    let mut last_selector = String::new();
    let ready = json!({ "visible": true, "onscreen": true, "enabled": true, "editable": true, "hit": true,
        "x": 5.0, "y": 5.0, "covered_by": "", "rect": [0.0, 0.0, 10.0, 10.0] });
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Network.setCookies" => {
                restored.store(true, Ordering::SeqCst);
                json!({})
            }
            "Network.clearBrowserCookies" => {
                signed_in.store(false, Ordering::SeqCst);
                restored.store(false, Ordering::SeqCst);
                json!({})
            }
            "Page.navigate" => {
                if cookie_is_good && restored.load(Ordering::SeqCst) {
                    signed_in.store(true, Ordering::SeqCst);
                }
                json!({ "frameId": "F", "loaderId": "L" })
            }
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
                let there = match last_selector.as_str() {
                    "#marker" => signed_in.load(Ordering::SeqCst),
                    "#other-session" => false,
                    s if Some(s) == broken_selector => false,
                    _ => true,
                };
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Input.insertText" => {
                if params["text"] == PASSWORD {
                    typed.store(true, Ordering::SeqCst);
                }
                json!({})
            }
            "Input.dispatchMouseEvent" => {
                if params["type"] == "mouseReleased" {
                    clicks.fetch_add(1, Ordering::SeqCst);
                    if typed.load(Ordering::SeqCst) {
                        signed_in.store(true, Ordering::SeqCst);
                    }
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
    (d, state)
}

fn no_password_anywhere(v: &impl serde::Serialize) {
    let text = serde_json::to_string(v).unwrap();
    assert!(!text.contains(PASSWORD), "a password reached an outcome: {text}");
}

#[test]
fn redact_hides_the_password_and_leaves_an_empty_one_alone() {
    assert_eq!(redact("the list has no option \"s3cret-Value\"", &account()), "the list has no option \"(hidden)\"");
    let mut none = account();
    none.password.clear();
    assert_eq!(redact("nothing to hide", &none), "nothing to hide");
}

#[tokio::test]
async fn the_recipe_signs_in_and_the_session_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = app(false, None);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(!out.used_saved_session);
    assert!(out.detail.contains("Administrator"), "{}", out.detail);
    assert!(state.typed_password.load(Ordering::SeqCst));
    // navigate + 3 actions + the optional prompt that never showed.
    assert_eq!(out.steps.len(), 5, "{:?}", out.steps);
    assert!(out.steps[4].ok && out.steps[4].detail.contains("carried on"), "{:?}", out.steps[4]);
    no_password_anywhere(&out);
    assert!(session_path(dir.path(), "admin").is_file(), "the session was not saved");
    // It started by clearing whatever the browser held.
    assert_eq!(d.methods().iter().position(|m| m == "Network.clearBrowserCookies"), Some(0));
}

#[tokio::test]
async fn a_fresh_saved_session_skips_the_form_entirely() {
    let dir = tempfile::tempdir().unwrap();
    let saved = SavedSession { saved_at_ms: now_ms(), cookies: vec![json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true })], local_storage: vec![] };
    save_session(dir.path(), "admin", &saved).unwrap();
    let (mut d, state) = app(true, None);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok && out.used_saved_session, "{}", out.detail);
    assert!(d.calls_to("Input.insertText").is_empty(), "nothing should have been typed");
    assert_eq!(state.clicks.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_saved_session_the_application_no_longer_accepts_falls_back_to_the_recipe() {
    let dir = tempfile::tempdir().unwrap();
    let saved = SavedSession { saved_at_ms: now_ms(), cookies: vec![json!({ "name": "sid", "value": "OLD", "domain": "hr.example.internal", "path": "/", "session": true })], local_storage: vec![] };
    save_session(dir.path(), "admin", &saved).unwrap();
    let (mut d, state) = app(false, None);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok && !out.used_saved_session, "{}", out.detail);
    assert!(state.typed_password.load(Ordering::SeqCst));
    assert_eq!(d.calls_to("Page.navigate").len(), 2, "once to try the session, once for the form");
    assert_eq!(d.calls_to("Network.clearBrowserCookies").len(), 2, "the dead session was wiped before the form");
}

#[tokio::test]
async fn a_failing_step_stops_the_sign_in_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _) = app(false, Some("#pass"));
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("step 2") && out.detail.contains("#pass"), "{}", out.detail);
    no_password_anywhere(&out);
    assert!(!session_path(dir.path(), "admin").exists(), "a failed sign-in must not save a session");
}

#[tokio::test]
async fn a_recipe_that_runs_but_never_reaches_the_marker_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut wrong = account();
    wrong.password = "not-the-password".into();
    let (mut d, _) = app(false, None);
    let out = sign_in(&mut d, dir.path(), &recipe(), &wrong, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("#marker") && out.detail.contains("username and password"), "{}", out.detail);
    assert!(!out.detail.contains("not-the-password"));
}

#[test]
fn preparing_a_sign_in_says_what_is_missing_and_where_to_add_it() {
    let dir = tempfile::tempdir().unwrap();
    let no_recipe = prepare(dir.path(), "Acme", "Web", "admin").unwrap_err();
    assert!(no_recipe.contains("sign-in recipe"), "{no_recipe}");
    save_recipe(dir.path(), "Acme", "Web", &recipe()).unwrap();
    let no_account = prepare(dir.path(), "Acme", "Web", "admin").unwrap_err();
    assert!(no_account.contains("\"admin\"") && no_account.contains("Accounts"), "{no_account}");
    save_accounts(dir.path(), &[account()]).unwrap();
    let (r, a) = prepare(dir.path(), "Acme", "Web", "admin").unwrap();
    assert_eq!((r, a.key.as_str()), (recipe(), "admin"));
}

fn _unused(_: Value) {}
