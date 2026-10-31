//! Getting from a fresh browser to a signed-in one, for one account.

use crate::common;

use common::{account, quick, recipe, stateful_app, ScriptedDriver, PASSWORD};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::accounts::session_path;
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::sessions::{now_ms, save_session};
use v2_lib::autorun::signin::{already_signed_in, prepare, redact, sign_in, SESSION_REFUSED_JS};
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
    assert!(session_path(dir.path(), "admin").unwrap().is_file(), "the session was not saved");
    // It started by clearing whatever the browser held.
    assert_eq!(d.methods().iter().position(|m| m == "Network.clearBrowserCookies"), Some(0));
}

/// The capture must never take the page the password was typed on for
/// the application's: the sign-in names that page after each login fill
/// (the username's and the password's), before the marker confirms the
/// site. A saved session types nothing and names only the confirmation.
#[tokio::test]
async fn the_sign_in_names_the_login_page_to_the_capture_before_confirming_the_site() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = stateful_app(false, None);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(d.capture_notes, ["login", "login", "confirm"]);

    let saved = SavedSession { saved_at_ms: now_ms(), cookies: vec![json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true })], local_storage: vec![] };
    save_session(dir.path(), "admin", &saved).unwrap();
    let (mut d, _state) = stateful_app(true, None);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok && out.used_saved_session, "{}", out.detail);
    assert_eq!(d.capture_notes, ["confirm"]);
}

/// A provider that asks for the password on a prompt after the account
/// name: the password fill sits inside a `when_visible`, and the page it is
/// typed on is named to the capture all the same.
#[tokio::test]
async fn a_password_typed_inside_a_prompt_names_its_page_to_the_capture() {
    let recipe: v2_lib::autorun::recipe::SignInRecipe = serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": [
            { "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" },
            { "kind": "click", "selector": { "css": "#next" } },
            { "kind": "when_visible", "selector": { "css": "#pass-prompt" }, "within_ms": 60,
              "then": [ { "kind": "fill", "selector": { "css": "#pass" }, "value": "{{password}}" } ] },
            { "kind": "click", "selector": { "css": "#go" } }
        ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let out = sign_in(&mut d, dir.path(), &recipe, &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(state.typed_password.load(Ordering::SeqCst));
    assert_eq!(d.capture_notes, ["login", "login", "confirm"]);
    no_password_anywhere(&out);
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
        session_path(dir.path(), "admin").unwrap().is_file(),
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
    assert!(!session_path(dir.path(), "admin").unwrap().exists(), "a failed sign-in must not save a session");
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
    assert!(!session_path(dir.path(), "admin").unwrap().exists());
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
    assert!(!session_path(dir.path(), "admin").unwrap().exists());
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
        session_path(dir.path(), "admin").unwrap().is_file(),
        "a browser that stopped answering says nothing about whether the saved session is still good"
    );
}

#[test]
fn preparing_a_sign_in_says_what_is_missing_and_where_to_add_it() {
    let dir = tempfile::tempdir().unwrap();
    // No saved recipe and no site address: the built-in has nowhere to go.
    let no_recipe = prepare(dir.path(), "Acme", "Web", "admin").unwrap_err();
    assert_eq!(no_recipe, v2_lib::autorun::recipe::NO_SITE_ADDRESS);
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
    assert!(session_path(dir.path(), "admin").unwrap().is_file());
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
    assert!(out.appeared.is_empty(), "{:?}", out.appeared);
}

/// An optional step whose element DID show up is named in `appeared` - so a
/// caller can say "Continue here was clicked" (PeoplesHR's take-over-the-
/// other-session button) without reading step sentences.
#[tokio::test]
async fn an_optional_step_that_appears_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let r = recipe_with_after(json!([
        { "kind": "when_visible", "selector": { "css": "#open-menu" }, "within_ms": 200,
          "then": [ { "kind": "click", "selector": { "css": "#open-menu" } } ] }
    ]));
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 2, "#go, then the optional #open-menu");
    assert_eq!(out.appeared.len(), 1, "{:?}", out.appeared);
    assert!(out.appeared[0].contains("#open-menu"), "{:?}", out.appeared);
}

/// Signed in, but the page is not the way scripts expect it: that is a
/// failure the person must see, named as this step - and no session is
/// saved: a sign-in whose after_sign_in did not finish is not complete.
#[tokio::test]
async fn a_failing_after_sign_in_step_fails_the_sign_in_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = stateful_app(false, Some("#open-menu"));
    let r = recipe_with_after(json!([{ "kind": "click", "selector": { "css": "#open-menu" } }]));
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("after_sign_in step 1"), "{}", out.detail);
    no_password_anywhere(&out);
    assert!(!session_path(dir.path(), "admin").unwrap().exists(), "an incomplete sign-in was saved");
}

// ---- a browser already signed in (a replay that skips the sign-in) -------

/// `stateful_app` with the page saying the application refused the session
/// it holds ("Cookie has been tampered").
fn refusing_app() -> (ScriptedDriver, common::StatefulApp) {
    let (d, state) = stateful_app(false, None);
    let d = d.answering_first(|method, params| {
        (method == "Runtime.evaluate" && params["expression"] == SESSION_REFUSED_JS)
            .then(|| Ok(json!({ "result": { "value": "tampered" } })))
    });
    (d, state)
}

/// Nothing a sign-in does was done: no cookie cleared or set, no page
/// loaded, nothing typed or clicked.
fn only_looked(d: &ScriptedDriver) {
    for m in d.methods() {
        assert!(
            !m.starts_with("Network.") && !m.starts_with("Input.") && m != "Page.navigate" && !m.starts_with("Storage."),
            "the check did more than look: {:?}",
            d.methods()
        );
    }
}

#[tokio::test]
async fn a_browser_signed_in_as_the_account_with_its_marker_showing_is_signed_in_already() {
    let (mut d, state) = stateful_app(false, None);
    state.signed_in.store(true, Ordering::SeqCst);
    assert!(already_signed_in(&mut d, &recipe(), Some("admin"), "admin", &quick()).await);
    only_looked(&d);
}

#[tokio::test]
async fn a_browser_signed_in_as_another_account_or_none_is_not_even_looked_at() {
    let (mut d, state) = stateful_app(false, None);
    state.signed_in.store(true, Ordering::SeqCst);
    assert!(!already_signed_in(&mut d, &recipe(), Some("boss"), "admin", &quick()).await);
    assert!(!already_signed_in(&mut d, &recipe(), None, "admin", &quick()).await);
    assert!(d.calls.is_empty(), "the browser was asked: {:?}", d.methods());
}

#[tokio::test]
async fn a_refused_session_is_not_signed_in_already() {
    let (mut d, state) = refusing_app();
    state.signed_in.store(true, Ordering::SeqCst);
    assert!(!already_signed_in(&mut d, &recipe(), Some("admin"), "admin", &quick()).await);
    only_looked(&d);
}

#[tokio::test]
async fn a_page_without_the_marker_is_not_signed_in_already() {
    let (mut d, _state) = stateful_app(false, None);
    let started = std::time::Instant::now();
    assert!(!already_signed_in(&mut d, &recipe(), Some("admin"), "admin", &quick()).await);
    only_looked(&d);
    // A short look: never longer than the expectation's own wait.
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
}

#[tokio::test]
async fn a_browser_that_does_not_answer_is_not_signed_in_already() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    assert!(!already_signed_in(&mut d, &recipe(), Some("admin"), "admin", &quick()).await);
}

// ------------------------------------------- a cold site, and where it ended

use v2_lib::autorun::signin::{landing_words, COLD_SITE_RETRY, MARKER_NEVER_APPEARED};
use v2_lib::browser::cdp::Event;

const LOGIN_URL: &str = "https://hr.example.internal/hr/security/login";

fn recipe_at_login() -> v2_lib::autorun::recipe::SignInRecipe {
    let mut v = serde_json::to_value(recipe()).unwrap();
    v["start_url"] = json!(LOGIN_URL);
    serde_json::from_value(v).unwrap()
}

/// `stateful_app` whose page is at `href` (titled `title`), and whose `#go`
/// click does not show the marker (a cold site still loading). After that
/// click the login form is gone from the page unless `form_stays`. The marker
/// shows only once the sign-in's address has been opened a second time, or
/// never when `never`. A `Page.reload` would post the form again: the tests
/// assert there is none.
fn cold_app(href: &'static str, title: &'static str, form_stays: bool, never: bool) -> (ScriptedDriver, common::StatefulApp) {
    cold_app_without(href, title, form_stays, never, None)
}

/// `cold_app` whose page has no element matching the css `missing`.
fn cold_app_without(
    href: &'static str,
    title: &'static str,
    form_stays: bool,
    never: bool,
    missing: Option<&'static str>,
) -> (ScriptedDriver, common::StatefulApp) {
    let (d, state) = stateful_app(false, missing);
    let (signed_in, typed) = (state.signed_in.clone(), state.typed_password.clone());
    let submitted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let last = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let navs = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut d = d.answering_first(move |method, params| match method {
        "Runtime.evaluate" if params["expression"] == "location.href" => Some(Ok(json!({ "result": { "value": href } }))),
        "Runtime.evaluate" if params["expression"] == "document.title" => Some(Ok(json!({ "result": { "value": title } }))),
        "Input.dispatchMouseEvent" if typed.load(Ordering::SeqCst) => {
            if params["type"] == "mouseReleased" {
                submitted.store(true, Ordering::SeqCst);
            }
            Some(Ok(json!({})))
        }
        "Page.navigate" => {
            if navs.fetch_add(1, Ordering::SeqCst) == 1 && !never {
                signed_in.store(true, Ordering::SeqCst);
            }
            None
        }
        "Runtime.callFunctionOn" if params["objectId"] == "doc" && params["arguments"][0]["value"].is_string() => {
            *last.lock().unwrap() = params["arguments"][0]["value"].as_str().unwrap().to_string();
            None
        }
        "Runtime.getProperties" => {
            let form = matches!(last.lock().unwrap().as_str(), "#user" | "#pass" | "#go");
            (form && submitted.load(Ordering::SeqCst) && !form_stays).then(|| Ok(json!({ "result": [] })))
        }
        _ => None,
    });
    d.on_every_call_events.push((
        "Page.reload".into(),
        Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": "F", "loaderId": "L2", "name": "load" }) },
    ));
    (d, state)
}

fn typed_count(d: &ScriptedDriver) -> usize {
    d.calls_to("Input.insertText").iter().filter(|c| c["text"] == PASSWORD).count()
}

#[tokio::test]
async fn cold_site_with_the_form_gone_opens_the_address_once_never_reloads() {
    let _log = crate::serial::log_tail();
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = cold_app(LOGIN_URL, "Home", false, false);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(d.calls_to("Page.reload").is_empty(), "the page was reloaded: {:?}", d.methods());
    assert_eq!(d.calls_to("Page.navigate").len(), 2, "{:?}", d.methods());
    assert!(v2_lib::applog::recent(6000).iter().any(|l| l.message.contains(COLD_SITE_RETRY)), "the retry was not logged");
    assert!(state.typed_password.load(Ordering::SeqCst));
    assert_eq!(typed_count(&d), 1, "the password was typed again");
    no_password_anywhere(&out);
}

#[tokio::test]
async fn the_form_still_on_the_page_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = cold_app(LOGIN_URL, "Sign in", true, false);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &account(), &quick()).await;
    assert!(!out.ok);
    assert!(d.calls_to("Page.reload").is_empty(), "{:?}", d.methods());
    assert_eq!(d.calls_to("Page.navigate").len(), 1, "the address was opened again: {:?}", d.methods());
    assert_eq!(typed_count(&d), 1);
    assert!(out.detail.starts_with(MARKER_NEVER_APPEARED), "{}", out.detail);
    assert!(out.detail.contains("the browser ended on /hr/security/login (page title: Sign in)"), "{}", out.detail);
    assert!(out.detail.contains("login form is still on the page"), "{}", out.detail);
    assert_eq!(out.landed.as_deref(), Some("/hr/security/login"));
    assert!(!out.detail.contains("hr.example.internal"), "{}", out.detail);
    no_password_anywhere(&out);
}

#[tokio::test]
async fn a_retry_that_still_has_no_marker_fails_naming_where_it_ended() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = cold_app("https://hr.example.internal/hr/home/index?t=1", "Self Service", false, true);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &account(), &quick()).await;
    assert!(!out.ok);
    assert_eq!(d.calls_to("Page.navigate").len(), 2);
    assert!(d.calls_to("Page.reload").is_empty());
    assert_eq!(typed_count(&d), 1);
    assert!(out.detail.contains("the browser ended on /hr/home/index (page title: Self Service)"), "{}", out.detail);
    assert!(!out.detail.contains("t=1"));
}

#[tokio::test]
async fn refused_session_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let (d, _state) = cold_app(LOGIN_URL, "Home", false, false);
    let mut d = d.answering_first(|method, params| {
        (method == "Runtime.evaluate" && params["expression"] == SESSION_REFUSED_JS)
            .then(|| Ok(json!({ "result": { "value": "tampered" } })))
    });
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &account(), &quick()).await;
    assert!(!out.ok);
    assert_eq!(d.calls_to("Page.navigate").len(), 1, "{:?}", d.methods());
    assert_eq!(out.detail, v2_lib::autorun::signin::session_refused_again("admin"));
}

#[tokio::test]
async fn harness_failure_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let (d, _state) = cold_app(LOGIN_URL, "Home", false, false);
    let mut d = d.answering_first(|method, params| {
        (method == "Runtime.callFunctionOn" && params["arguments"][0]["value"] == "#marker").then(|| Err(CdpError::Closed))
    });
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &account(), &quick()).await;
    assert!(!out.ok && out.harness, "{}", out.detail);
    assert_eq!(d.calls_to("Page.navigate").len(), 1, "{:?}", d.methods());
}

#[test]
fn landing_words_names_path_and_title_only() {
    let words = landing_words("https://x.test/hr/home/index?t=1", Some("  Self   Service "));
    assert_eq!(words, "the browser ended on /hr/home/index (page title: Self Service)");
    assert!(!words.contains("x.test") && !words.contains("t=1"), "{words}");
    assert_eq!(landing_words("https://x.test/hr/home?t=1", None), "the browser ended on /hr/home");
    assert_eq!(landing_words("https://x.test/hr/home", Some("  ")), "the browser ended on /hr/home");
    let long = "T".repeat(200);
    let said = landing_words("https://x.test/hr", Some(&long));
    assert!(said.ends_with(&format!("{})", "T".repeat(80))), "{said}");
}

#[test]
fn landing_words_hides_a_session_in_the_path() {
    let words = landing_words("https://x.test/(S(abcdef123))/hr/home;jsessionid=ABC123", None);
    assert_eq!(words, "the browser ended on /hr/home");
}

#[test]
fn path_of_hides_sessions_and_tokens() {
    use v2_lib::browser::save_guard::path_of;
    assert_eq!(path_of("https://x.test/app/home;jsessionid=ABC123?x=1"), "/app/home");
    assert_eq!(path_of("https://x.test/(F(abc123def))/hr/home"), "/hr/home");
    assert_eq!(path_of("https://x.test/(s(abc))/(A(zz))/hr"), "/hr");
    assert_eq!(path_of("https://x.test/sso/callback/Zm9vYmFyMTIzNDU2Nzg5MDEyMzQ"), "/sso/callback/…");
    assert_eq!(path_of("https://x.test/auth/magic/bcdfghjklmnpqrstvw"), "/auth/magic/…");
    assert_eq!(path_of("https://x.test/hr/pmsv10/performancecycle"), "/hr/pmsv10/performancecycle");
    assert_eq!(path_of("https://x.test/hr/GetEmployeeLeaveBalanceSummary"), "/hr/GetEmployeeLeaveBalanceSummary");
    assert_eq!(path_of("https://x.test"), "/");
    assert_eq!(path_of("https://x.test/"), "/");
}

#[tokio::test]
async fn a_success_notes_where_it_landed_without_changing_its_words() {
    let dir = tempfile::tempdir().unwrap();
    let (d, state) = stateful_app(false, None);
    let mut d = d.answering_first(|method, params| {
        (method == "Runtime.evaluate" && params["expression"] == "location.href")
            .then(|| Ok(json!({ "result": { "value": "https://hr.example.internal/hr/home/index?t=1" } })))
    });
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &account(), &quick()).await;
    assert!(out.ok && state.typed_password.load(Ordering::SeqCst), "{}", out.detail);
    assert_eq!(out.landed.as_deref(), Some("/hr/home/index"));
    assert_eq!(out.detail, "signed in as Administrator");
}

// ------------------------------------------------- the password reset page

use v2_lib::autorun::signin::{is_password_expired, PASSWORD_EXPIRED};

const RESET_URL: &str = "https://hr.example.internal/hr/security/PasswordReset";

/// `recipe_at_login` that knows the site's reset page; with `wait`, a
/// `wait_for` of the marker ends the steps (as the built-in recipe's does).
fn reset_recipe(wait: bool) -> v2_lib::autorun::recipe::SignInRecipe {
    let mut v = serde_json::to_value(recipe_at_login()).unwrap();
    v["password_reset"] = json!({ "path_contains": "/security/PasswordReset", "field": { "css": "#txtOldPassword" } });
    if wait {
        v["steps"].as_array_mut().unwrap().push(json!({ "kind": "wait_for", "selector": { "css": "#marker" }, "timeout_ms": 100 }));
    }
    serde_json::from_value(v).unwrap()
}

#[tokio::test]
async fn reset_page_fails_as_password_expired() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = cold_app(RESET_URL, "Reset Password", false, true);
    let out = sign_in(&mut d, dir.path(), &reset_recipe(false), &account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.starts_with(PASSWORD_EXPIRED), "{}", out.detail);
    assert!(is_password_expired(&out.detail));
    assert!(out.detail.contains("/hr/security/PasswordReset"), "{}", out.detail);
    assert!(out.detail.contains("(page title: Reset Password)"), "{}", out.detail);
    assert!(!out.detail.contains("hr.example.internal") && !out.detail.contains(PASSWORD), "{}", out.detail);
    assert_eq!(out.landed.as_deref(), Some("/hr/security/PasswordReset"));
    // Not slowness: the address was not opened again, the password typed once.
    assert_eq!(d.calls_to("Page.navigate").len(), 1, "{:?}", d.methods());
    assert_eq!(typed_count(&d), 1);
    no_password_anywhere(&out);
}

#[tokio::test]
async fn reset_page_after_a_wait_step_fails_as_password_expired_not_a_stopped_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = cold_app(RESET_URL, "Reset Password", false, true);
    let out = sign_in(&mut d, dir.path(), &reset_recipe(true), &account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.starts_with(PASSWORD_EXPIRED), "{}", out.detail);
    assert!(!out.detail.contains("sign-in stopped at step"), "{}", out.detail);
    assert_eq!(typed_count(&d), 1);
}

#[tokio::test]
async fn a_recipe_recorded_without_the_reset_entry_still_sees_the_reset_page() {
    let dir = tempfile::tempdir().unwrap();
    assert!(recipe_at_login().password_reset.is_none());
    let (mut d, _state) = cold_app(RESET_URL, "Reset Password", false, true);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &account(), &quick()).await;
    assert!(out.detail.starts_with(PASSWORD_EXPIRED), "{}", out.detail);
    assert_eq!(typed_count(&d), 1);
    // And when its own wait step is what stops.
    let mut v = serde_json::to_value(recipe_at_login()).unwrap();
    v["steps"].as_array_mut().unwrap().push(json!({ "kind": "wait_for", "selector": { "css": "#marker" }, "timeout_ms": 100 }));
    let waiting: v2_lib::autorun::recipe::SignInRecipe = serde_json::from_value(v).unwrap();
    let (mut d, _state) = cold_app(RESET_URL, "Reset Password", false, true);
    let out = sign_in(&mut d, dir.path(), &waiting, &account(), &quick()).await;
    assert!(out.detail.starts_with(PASSWORD_EXPIRED), "{}", out.detail);
}

#[tokio::test]
async fn the_reset_path_without_the_reset_field_is_not_password_expired() {
    let dir = tempfile::tempdir().unwrap();
    // The path matches, the field is not there: no misfire, with the recipe's
    // own entry or the built-in fallback.
    for recipe in [reset_recipe(false), recipe_at_login()] {
        let (mut d, _state) = cold_app_without(RESET_URL, "Something else", true, true, Some("#txtOldPassword"));
        let out = sign_in(&mut d, dir.path(), &recipe, &account(), &quick()).await;
        assert!(!is_password_expired(&out.detail), "{}", out.detail);
    }
}

#[tokio::test]
async fn another_page_without_the_reset_field_is_not_password_expired() {
    let dir = tempfile::tempdir().unwrap();
    // The recipe knows the reset page and the browser is on the login page.
    let (mut d, _state) = cold_app(LOGIN_URL, "Sign in", true, false);
    let out = sign_in(&mut d, dir.path(), &reset_recipe(false), &account(), &quick()).await;
    assert!(out.detail.starts_with(MARKER_NEVER_APPEARED), "{}", out.detail);
}

#[test]
fn the_built_in_recipe_knows_the_reset_page() {
    let page = v2_lib::autorun::recipe::builtin_recipe().password_reset.expect("the built-in recipe names the reset page");
    assert_eq!(page.path_contains, "/security/PasswordReset");
    assert_eq!(serde_json::to_value(&page.field).unwrap(), json!({ "css": "#txtOldPassword" }));
}

// ------------------------------------------- resetting an expired password

use std::sync::{Arc, Mutex};
use v2_lib::autorun::accounts::{load_accounts, Account};
use v2_lib::autorun::signin::{next_password, PASSWORD_RESET_FAILED, RESET_PASSWORDS};

const HOME_URL: &str = "https://hr.example.internal/hr/home/index";
const KEY: &str = "automation51";

#[test]
fn next_password_skips_current_and_refused() {
    assert_eq!(RESET_PASSWORDS, ["Abc@123", "Test@1234"]);
    assert_eq!(next_password("old-one", &[]), Some("Abc@123"));
    assert_eq!(next_password("Abc@123", &[]), Some("Test@1234"));
    assert_eq!(next_password("Test@1234", &[]), Some("Abc@123"));
    assert_eq!(next_password("old-one", &["Abc@123".to_string()]), Some("Test@1234"));
    assert_eq!(next_password("Abc@123", &["Test@1234".to_string()]), None);
    assert_eq!(next_password("old-one", &["Abc@123".to_string(), "Test@1234".to_string()]), None);
}

fn reset_account() -> Account {
    Account { key: KEY.into(), label: "Automation 51".into(), username: "kim".into(), password: PASSWORD.into() }
}

/// A data folder whose active environment is (or is not) marked a test
/// environment, with the reset switch on or off, holding `reset_account`.
fn reset_root(test_environment: bool, switch: bool) -> tempfile::TempDir {
    use v2_lib::environments::{load_or_init, save_env_with, set_reset_expired_passwords};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut e = load_or_init(root, Some("dev-read")).unwrap().environments[0].clone();
    e.test_environment = test_environment;
    let id = e.id.clone();
    save_env_with(root, e, || vec!["dev-read".to_string()]).unwrap();
    if switch {
        set_reset_expired_passwords(root, &id, true).unwrap();
    }
    save_accounts(root, &[reset_account()]).unwrap();
    dir
}

struct ResetApp {
    /// (the field's selector, what was typed into it), in order.
    typed: Arc<Mutex<Vec<(String, String)>>>,
    submits: Arc<std::sync::atomic::AtomicUsize>,
}

impl ResetApp {
    fn typed_into(&self, selector: &str) -> Vec<String> {
        self.typed.lock().unwrap().iter().filter(|(s, _)| s == selector).map(|(_, t)| t.clone()).collect()
    }
    fn count_of(&self, text: &str) -> usize {
        self.typed.lock().unwrap().iter().filter(|(_, t)| t == text).count()
    }
}

/// A site that sends the login to its reset page. Submitting the reset form
/// is accepted when the new password and its confirmation agree and are one
/// of `accepts`; then the page goes to the login page, and the next sign-in
/// (with whatever was typed) reaches the marker.
fn reset_app(accepts: &'static [&'static str]) -> (ScriptedDriver, ResetApp) {
    let (d, state) = stateful_app(false, None);
    let (signed_in, typed_flag) = (state.signed_in.clone(), state.typed_password.clone());
    let accepted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let last = Arc::new(Mutex::new(String::new()));
    let typed: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let submits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = ResetApp { typed: typed.clone(), submits: submits.clone() };
    let d = d.answering_first(move |method, params| {
        let page_now = || {
            if !accepted.load(Ordering::SeqCst) {
                (RESET_URL, "Reset Password")
            } else if signed_in.load(Ordering::SeqCst) {
                (HOME_URL, "Home")
            } else {
                (LOGIN_URL, "Sign in")
            }
        };
        match method {
            "Runtime.evaluate" if params["expression"] == "location.href" => {
                Some(Ok(json!({ "result": { "value": page_now().0 } })))
            }
            "Runtime.evaluate" if params["expression"] == "document.title" => {
                Some(Ok(json!({ "result": { "value": page_now().1 } })))
            }
            "Runtime.callFunctionOn" if params["objectId"] == "doc" && params["arguments"][0]["value"].is_string() => {
                *last.lock().unwrap() = params["arguments"][0]["value"].as_str().unwrap().to_string();
                None
            }
            "Input.insertText" => {
                let text = params["text"].as_str().unwrap_or("").to_string();
                if text != "kim" {
                    typed_flag.store(true, Ordering::SeqCst);
                }
                typed.lock().unwrap().push((last.lock().unwrap().clone(), text));
                None
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" && !accepted.load(Ordering::SeqCst) => {
                if last.lock().unwrap().as_str() == "#btnsubmit" {
                    submits.fetch_add(1, Ordering::SeqCst);
                    let into = |sel: &str| {
                        typed.lock().unwrap().iter().rev().find(|(s, _)| s == sel).map(|(_, t)| t.clone()).unwrap_or_default()
                    };
                    let (new, confirm) = (into("#txtNewPassword"), into("#txtConfirmPassword"));
                    if new == confirm && accepts.contains(&new.as_str()) {
                        accepted.store(true, Ordering::SeqCst);
                    }
                }
                Some(Ok(json!({})))
            }
            "Runtime.getProperties" if accepted.load(Ordering::SeqCst) && last.lock().unwrap().as_str() == "#txtOldPassword" => {
                Some(Ok(json!({ "result": [] })))
            }
            _ => None,
        }
    });
    (d, app)
}

fn account_file_password(root: &std::path::Path) -> String {
    load_accounts(root).unwrap().into_iter().find(|a| a.key == KEY).unwrap().password
}

#[tokio::test]
async fn reset_on_test_environment_saves_new_password_and_signs_in() {
    let _log = crate::serial::log_tail();
    let dir = reset_root(true, true);
    let (mut d, app) = reset_app(&["Abc@123", "Test@1234"]);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &reset_account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "signed in as Automation 51");
    // The current password was the old one, the alternate the new and its confirmation.
    assert_eq!(app.typed_into("#txtOldPassword"), [PASSWORD]);
    assert_eq!(app.typed_into("#txtNewPassword"), ["Abc@123"]);
    assert_eq!(app.typed_into("#txtConfirmPassword"), ["Abc@123"]);
    assert_eq!(app.submits.load(Ordering::SeqCst), 1);
    // The sign-in carried on with the new password; the account holds it.
    assert_eq!(app.count_of(PASSWORD), 2, "the old password: the first sign-in and the form");
    assert_eq!(app.count_of("Abc@123"), 3, "new, confirm and the sign-in again");
    assert_eq!(account_file_password(dir.path()), "Abc@123");
    let log = v2_lib::applog::recent(6000);
    assert!(log.iter().any(|l| l.message.contains("the expired password of automation51 was reset")));
    assert!(!log.iter().any(|l| l.message.contains("Abc@123") || l.message.contains(PASSWORD)));
    assert!(!out.detail.contains("Abc@123"));
}

#[tokio::test]
async fn the_alternate_is_used_when_the_current_password_is_the_first() {
    let dir = reset_root(true, true);
    let mut acct = reset_account();
    acct.password = "Abc@123".into();
    save_accounts(dir.path(), &[acct.clone()]).unwrap();
    let (mut d, app) = reset_app(&["Abc@123", "Test@1234"]);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &acct, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.typed_into("#txtNewPassword"), ["Test@1234"]);
    assert_eq!(account_file_password(dir.path()), "Test@1234");
}

#[tokio::test]
async fn no_reset_on_non_test_environment() {
    let dir = reset_root(false, true);
    let (mut d, app) = reset_app(&["Abc@123", "Test@1234"]);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &reset_account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.starts_with(PASSWORD_EXPIRED), "{}", out.detail);
    assert!(out.detail.contains("Reset expired passwords"), "{}", out.detail);
    assert!(app.typed_into("#txtNewPassword").is_empty());
    assert_eq!(account_file_password(dir.path()), PASSWORD);
    no_password_anywhere(&out);
}

#[tokio::test]
async fn no_reset_when_switch_off() {
    let dir = reset_root(true, false);
    let (mut d, app) = reset_app(&["Abc@123", "Test@1234"]);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &reset_account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.starts_with(PASSWORD_EXPIRED), "{}", out.detail);
    assert!(is_password_expired(&out.detail));
    assert!(out.detail.contains("Reset expired passwords"), "{}", out.detail);
    assert!(app.typed_into("#txtNewPassword").is_empty());
    assert_eq!(app.submits.load(Ordering::SeqCst), 0);
    assert_eq!(account_file_password(dir.path()), PASSWORD);
}

#[tokio::test]
async fn refused_new_password_tries_the_next_once() {
    let dir = reset_root(true, true);
    let (mut d, app) = reset_app(&["Test@1234"]);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &reset_account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.typed_into("#txtNewPassword"), ["Abc@123", "Test@1234"]);
    assert_eq!(app.submits.load(Ordering::SeqCst), 2);
    assert_eq!(account_file_password(dir.path()), "Test@1234");
}

#[tokio::test]
async fn a_reset_the_site_refuses_every_time_stops_and_leaves_the_password() {
    let dir = reset_root(true, true);
    let (mut d, app) = reset_app(&[]);
    let out = sign_in(&mut d, dir.path(), &recipe_at_login(), &reset_account(), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.starts_with(PASSWORD_RESET_FAILED), "{}", out.detail);
    assert!(is_password_expired(&out.detail));
    // Both candidates tried once, then no more: it never loops.
    assert_eq!(app.submits.load(Ordering::SeqCst), 2);
    assert_eq!(account_file_password(dir.path()), PASSWORD);
    assert!(!out.detail.contains("Abc@123") && !out.detail.contains("Test@1234"), "{}", out.detail);
    no_password_anywhere(&out);
}
