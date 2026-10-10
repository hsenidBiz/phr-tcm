//! PeoplesHR's "Cookie has been tampered. Reason [EMPUSERID] dont match
//! current context user." page (and an empty 400): the application no
//! longer accepts the session the browser holds. A restored session it
//! refuses is dropped and the account signed in fresh, once; a refusal
//! after a fresh sign-in fails with a sentence naming the account key; a
//! session is saved only once it is complete; and a mid-case `sign_in`
//! clears the site's cookies before the next account signs in.
//!
//! Fake drivers only: `stateful_app` behind a wrapper that answers the
//! refusal check and the cookie list as each test needs.

use crate::common::{account, quick, recipe, stateful_app, ScriptedDriver, StatefulApp, PASSWORD};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use v2_lib::autorun::accounts::{save_accounts, session_path};
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::runner::run_step;
use v2_lib::autorun::sessions::{load_fresh_session, now_ms, save_session};
use v2_lib::autorun::signin::{session_complete, session_refused_again, sign_in, SESSION_REFUSED_JS};
use v2_lib::autorun::StepScript;
use v2_lib::browser::actions::Action;
use v2_lib::browser::cdp::{CdpError, Driver, Event};
use v2_lib::browser::session::SavedSession;

/// `stateful_app`, with the page refusing its session while `tampered` is
/// set (cleared by a fresh login's password unless `always`), and
/// `Network.getAllCookies` answering `cookies` when given.
struct App {
    inner: ScriptedDriver,
    state: StatefulApp,
    tampered: Arc<AtomicBool>,
    always: bool,
    cookies: Option<Value>,
}

impl App {
    fn new(cookie_is_good: bool, tampered: bool, always: bool) -> Self {
        let (inner, state) = stateful_app(cookie_is_good, None);
        App { inner, state, tampered: Arc::new(AtomicBool::new(tampered)), always, cookies: None }
    }
    fn methods(&self) -> Vec<String> {
        self.inner.calls.iter().map(|(m, _)| m.clone()).collect()
    }
}

impl Driver for App {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        if method == "Runtime.evaluate" && params["expression"] == SESSION_REFUSED_JS {
            self.inner.calls.push((method.to_string(), params.clone()));
            let word = if self.tampered.load(Ordering::SeqCst) { "tampered" } else { "" };
            return Ok(json!({ "result": { "value": word } }));
        }
        if method == "Input.insertText" && params["text"] == PASSWORD && !self.always {
            self.tampered.store(false, Ordering::SeqCst);
        }
        if method == "Network.getAllCookies" {
            if let Some(c) = &self.cookies {
                self.inner.calls.push((method.to_string(), params.clone()));
                return Ok(json!({ "cookies": c }));
            }
        }
        self.inner.call(method, params).await
    }
    async fn wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError> {
        self.inner.wait_event(method, limit).await
    }
    fn forget_events(&mut self) {
        self.inner.forget_events();
    }
    fn take_dialogs(&mut self) -> Vec<String> {
        self.inner.take_dialogs()
    }
    fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.inner.set_deadline(deadline);
    }
}

fn cookie(name: &str, value: &str) -> Value {
    json!({ "name": name, "value": value, "domain": "hr.example.internal", "path": "/", "session": true })
}

fn saved_old_session(root: &std::path::Path) {
    let s = SavedSession { saved_at_ms: now_ms(), cookies: vec![cookie("sid", "OLD")], local_storage: vec![] };
    save_session(root, "admin", &s).unwrap();
}

/// The report's case: a restored session the application answers with
/// "Cookie has been tampered" is dropped, and the account signed in fresh
/// once - the sign-in succeeds, and the session saved now is the new one.
#[tokio::test]
async fn a_tampered_page_on_a_restored_session_drops_it_and_signs_in_fresh_once() {
    let dir = tempfile::tempdir().unwrap();
    saved_old_session(dir.path());
    let mut d = App::new(true, true, false);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok && !out.used_saved_session, "{}", out.detail);
    assert!(d.state.typed_password.load(Ordering::SeqCst), "it never signed in fresh");
    let navigates = d.methods().iter().filter(|m| *m == "Page.navigate").count();
    assert_eq!(navigates, 2, "once for the saved session, once for the form - no second try");
    // The marker was never waited for on the refused page: the fresh form
    // came straight after it.
    let saved = load_fresh_session(dir.path(), "admin", 60, now_ms()).expect("the fresh session was saved");
    assert_eq!(saved.cookies[0]["value"], "abc", "the refused session was kept");
}

/// Refused again after the fresh sign-in: the case's sign-in fails with a
/// plain sentence that names the account key, never the login, and no
/// session is kept.
#[tokio::test]
async fn a_second_tampered_page_fails_with_the_account_key_sentence() {
    let dir = tempfile::tempdir().unwrap();
    saved_old_session(dir.path());
    let mut d = App::new(true, true, true);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, session_refused_again("admin"));
    assert!(out.detail.contains("account \"admin\""), "{}", out.detail);
    assert!(!out.detail.contains("kim") && !out.detail.contains(PASSWORD), "a login or password: {}", out.detail);
    assert!(!out.detail.contains('\u{2014}') && !out.detail.contains('\u{2013}'), "{}", out.detail);
    assert!(!session_path(dir.path(), "admin").unwrap().exists(), "a refused session was kept");
    let navigates = d.methods().iter().filter(|m| *m == "Page.navigate").count();
    assert_eq!(navigates, 2, "signed in fresh once, never twice");
}

/// A session is saved only once complete: a PeoplesHR sign-in caught with
/// `.ASPXAUTH` but no `ehrm85` yet (the pair every restore is checked
/// against) is not saved; with both it is. Only names are looked at.
#[tokio::test]
async fn a_session_is_not_saved_before_it_is_complete() {
    let half = json!([cookie(".ASPXAUTH", "a1"), cookie("ASP.NET_SessionId", "s1")]);
    let whole = json!([cookie(".ASPXAUTH", "a1"), cookie("ASP.NET_SessionId", "s1"), cookie("ehrm85", "e1")]);

    let dir = tempfile::tempdir().unwrap();
    let mut d = App::new(false, false, false);
    d.cookies = Some(half);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok, "the sign-in itself worked: {}", out.detail);
    assert!(!session_path(dir.path(), "admin").unwrap().exists(), "a half sign-in was saved");

    let dir = tempfile::tempdir().unwrap();
    let mut d = App::new(false, false, false);
    d.cookies = Some(whole);
    let out = sign_in(&mut d, dir.path(), &recipe(), &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(session_path(dir.path(), "admin").unwrap().is_file(), "a complete sign-in was not saved");

    let names = |list: &[&str]| SavedSession {
        saved_at_ms: 1,
        cookies: list.iter().map(|n| cookie(n, "v")).collect(),
        local_storage: vec![],
    };
    assert!(session_complete(&names(&["sid"])), "a site with neither cookie");
    assert!(!session_complete(&names(&[])), "no cookie at all");
    assert!(!session_complete(&names(&["ehrm85"])), "the PeoplesHR cookie with no sign-in");
}

fn step(actions: Vec<Action>) -> StepScript {
    StepScript { step_number: 1, actions, unchecked: None }
}

fn project(root: &std::path::Path) {
    save_recipe(root, "Acme", "Web", &recipe()).unwrap();
    save_accounts(root, &[account()]).unwrap();
}

/// A mid-case `sign_in` clears every cookie of the browser, and the
/// site's own cookies and storage for each of the recipe's origins, before
/// the next account signs in or a saved session is put back: no cookie of
/// the account before survives.
#[tokio::test]
async fn a_mid_case_sign_in_clears_the_origins_cookies_first() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    saved_old_session(dir.path());
    let mut d = App::new(true, false, false);
    // Signed in as someone else in this browser already.
    let mut acc = Some("previous".to_string());
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![Action::SignIn { account: "admin".into() }]), &quick(), &mut acc)
        .await
        .unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    let calls = &d.inner.calls;
    let at = |m: &str| calls.iter().position(|(c, _)| c == m).unwrap_or_else(|| panic!("no {m}: {calls:?}"));
    let cleared_origin = calls
        .iter()
        .position(|(c, p)| {
            c == "Storage.clearDataForOrigin"
                && p["origin"] == "https://hr.example.internal"
                && p["storageTypes"].as_str().is_some_and(|t| t.split(',').any(|k| k == "cookies"))
        })
        .expect("the origin's cookies were not cleared");
    let first_cookie_in = at("Network.setCookies");
    assert!(at("Network.clearBrowserCookies") < first_cookie_in, "{calls:?}");
    assert!(cleared_origin < first_cookie_in, "{calls:?}");
    assert!(cleared_origin < at("Page.navigate"), "{calls:?}");
}

/// Mid-case, an action that fails on the refused page signs the account in
/// fresh once and is tried again, rather than failing the case.
#[tokio::test]
async fn a_mid_case_refusal_signs_in_fresh_once_and_tries_the_action_again() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    saved_old_session(dir.path());
    let mut d = App::new(false, true, false);
    let mut acc = Some("admin".to_string());
    let marker: Action = serde_json::from_value(json!({ "kind": "expect_visible", "selector": { "css": "#marker" } })).unwrap();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![marker.clone()]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(d.state.typed_password.load(Ordering::SeqCst), "never signed in fresh");
    assert_eq!(acc.as_deref(), Some("admin"));

    // Refused again after the fresh sign-in: the account-key sentence.
    let mut d = App::new(false, true, true);
    let mut acc = Some("admin".to_string());
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![marker]), &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, session_refused_again("admin"));
    assert!(!session_path(dir.path(), "admin").unwrap().exists());
}
