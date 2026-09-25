//! Getting from a fresh browser to a signed-in one, for one account.

mod common;

use common::{account, quick, recipe, stateful_app, ScriptedDriver, PASSWORD};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::accounts::session_path;
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::sessions::{now_ms, save_session};
use v2_lib::autorun::signin::{prepare, redact, sign_in};
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::session::{OriginStorage, SavedSession};

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
    let (mut d, state) = stateful_app(false, None);
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
    let (mut d, state) = stateful_app(true, None);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok && out.used_saved_session, "{}", out.detail);
    assert!(d.calls_to("Input.insertText").is_empty(), "nothing should have been typed");
    assert_eq!(state.clicks.load(Ordering::SeqCst), 0);
}

/// The browser, not the saved session, is what just failed here - a
/// `Page.navigate` transport error while trying it must not throw the
/// session file away, or the next run pays for a browser hiccup with a
/// real sign-in it did not need.
#[tokio::test]
async fn a_transport_failure_while_trying_a_saved_session_keeps_the_session_file() {
    let dir = tempfile::tempdir().unwrap();
    let saved = SavedSession {
        saved_at_ms: now_ms(),
        cookies: vec![json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true })],
        local_storage: vec![],
    };
    save_session(dir.path(), "admin", &saved).unwrap();
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Page.navigate" => Err(CdpError::Closed),
        _ => Ok(json!({})),
    });
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("browser"), "{}", out.detail);
    no_password_anywhere(&out);
    assert!(
        session_path(dir.path(), "admin").is_file(),
        "a browser that stopped answering says nothing about whether the saved session is still good"
    );
}

#[tokio::test]
async fn a_saved_session_the_application_no_longer_accepts_falls_back_to_the_recipe() {
    let dir = tempfile::tempdir().unwrap();
    let saved = SavedSession { saved_at_ms: now_ms(), cookies: vec![json!({ "name": "sid", "value": "OLD", "domain": "hr.example.internal", "path": "/", "session": true })], local_storage: vec![] };
    save_session(dir.path(), "admin", &saved).unwrap();
    let (mut d, state) = stateful_app(false, None);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok && !out.used_saved_session, "{}", out.detail);
    assert!(state.typed_password.load(Ordering::SeqCst));
    assert_eq!(d.calls_to("Page.navigate").len(), 2, "once to try the session, once for the form");
    assert_eq!(d.calls_to("Network.clearBrowserCookies").len(), 2, "the dead session was wiped before the form");
}

#[tokio::test]
async fn a_failing_step_stops_the_sign_in_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _) = stateful_app(false, Some("#pass"));
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
    let (mut d, _) = stateful_app(false, None);
    let out = sign_in(&mut d, dir.path(), &recipe(), &wrong, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("#marker") && out.detail.contains("username and password"), "{}", out.detail);
    assert!(!out.detail.contains("not-the-password"));
}

#[tokio::test]
async fn a_browser_that_will_not_clear_cookies_says_someone_may_still_be_signed_in() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Network.clearBrowserCookies" => Err(CdpError::Closed),
        _ => Ok(json!({})),
    });
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("browser") && out.detail.contains("may still be signed in"), "{}", out.detail);
    assert!(d.calls_to("Page.navigate").is_empty(), "nothing was cleared, so nothing should have been visited");
    assert!(d.calls_to("Input.insertText").is_empty());
    assert!(!session_path(dir.path(), "admin").exists());
}

#[tokio::test]
async fn a_browser_that_clears_cookies_but_not_storage_says_someone_may_still_be_signed_in() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Storage.clearDataForOrigin" => Err(CdpError::Closed),
        _ => Ok(json!({})),
    });
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("browser") && out.detail.contains("may still be signed in"), "{}", out.detail);
    assert!(d.calls_to("Page.navigate").is_empty());
    assert!(d.calls_to("Input.insertText").is_empty());
    assert!(!session_path(dir.path(), "admin").exists());
}

#[tokio::test]
async fn a_browser_that_fails_mid_restore_is_cleared_before_giving_up_but_keeps_the_session_file() {
    let dir = tempfile::tempdir().unwrap();
    let saved = SavedSession {
        saved_at_ms: now_ms(),
        cookies: vec![json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true })],
        local_storage: vec![OriginStorage {
            origin: "https://hr.example.internal".into(),
            entries: vec![("k".into(), "v".into())],
        }],
    };
    save_session(dir.path(), "admin", &saved).unwrap();
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Page.addScriptToEvaluateOnNewDocument" => Err(CdpError::Closed),
        _ => Ok(json!({})),
    });
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(!out.ok, "{}", out.detail);
    assert_eq!(
        d.calls_to("Network.clearBrowserCookies").len(),
        2,
        "cleared once up front, once best-effort after the failed restore"
    );
    assert!(
        session_path(dir.path(), "admin").is_file(),
        "a browser that stopped answering says nothing about whether the saved session is still good"
    );
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

// ------------------------------------------------------------ after_sign_in

fn recipe_with_after(after: Value) -> v2_lib::autorun::recipe::SignInRecipe {
    let mut v = serde_json::to_value(recipe()).unwrap();
    v["after_sign_in"] = after;
    serde_json::from_value(v).unwrap()
}

/// After the recipe's own steps and the signed-in check - and BEFORE the
/// session is captured, so a saved session carries whatever these steps
/// left in the page's storage (PeoplesHR keeps its menu state there).
#[tokio::test]
async fn after_sign_in_runs_after_a_recipe_sign_in_and_before_the_session_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let r = recipe_with_after(json!([{ "kind": "click", "selector": { "css": "#open-menu" } }]));
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 2, "#go, then #open-menu");
    assert!(out.steps.last().unwrap().ok, "{:?}", out.steps);
    let methods = d.methods();
    let last_click = methods.iter().rposition(|m| m == "Input.dispatchMouseEvent").unwrap();
    let captured = methods.iter().position(|m| m == "Network.getAllCookies").unwrap();
    assert!(last_click < captured, "the session was captured before the steps ran: {methods:?}");
    assert!(session_path(dir.path(), "admin").is_file());
}

/// A saved session skips the recipe's steps, but not these: the page was
/// never opened in THIS browser, so whatever they set up is not there yet.
#[tokio::test]
async fn after_sign_in_runs_after_a_saved_session_too() {
    let dir = tempfile::tempdir().unwrap();
    let saved = SavedSession { saved_at_ms: now_ms(), cookies: vec![json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true })], local_storage: vec![] };
    save_session(dir.path(), "admin", &saved).unwrap();
    let (mut d, state) = stateful_app(true, None);
    let r = recipe_with_after(json!([{ "kind": "click", "selector": { "css": "#open-menu" } }]));
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(out.ok && out.used_saved_session, "{}", out.detail);
    assert!(d.calls_to("Input.insertText").is_empty(), "the form was not touched");
    assert_eq!(state.clicks.load(Ordering::SeqCst), 1, "only the after_sign_in click");
}

/// The idempotent form: nothing to do when the thing is not there.
#[tokio::test]
async fn an_after_sign_in_prompt_that_does_not_appear_is_carried_past() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let r = recipe_with_after(json!([
        { "kind": "when_visible", "selector": { "css": "#other-session" }, "within_ms": 40,
          "then": [ { "kind": "click", "selector": { "css": "#other-session" } } ] }
    ]));
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 1, "only #go");
    assert!(out.steps.last().unwrap().detail.contains("carried on"), "{:?}", out.steps);
}

/// Signed in, but the page is not the way scripts expect it: that is a
/// failure the person must see, named as this step - and the session the
/// sign-in itself earned is still kept.
#[tokio::test]
async fn a_failing_after_sign_in_step_fails_the_sign_in_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = stateful_app(false, Some("#open-menu"));
    let r = recipe_with_after(json!([{ "kind": "click", "selector": { "css": "#open-menu" } }]));
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("after_sign_in step 1"), "{}", out.detail);
    no_password_anywhere(&out);
    assert!(session_path(dir.path(), "admin").is_file(), "the sign-in itself worked");
}
