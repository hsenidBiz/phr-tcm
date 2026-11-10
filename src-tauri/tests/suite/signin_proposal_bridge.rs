//! The assistant's two sign-in tools (`inspect_in`, `propose_in`): what each
//! refuses before a browser opens, what a trial may and may not do, and what
//! is kept. The browser is a fake; nothing here opens a real one.

use crate::common::{account, quick, recipe, stateful_app, ScriptedDriver, PASSWORD};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::accounts::{load_accounts, save_accounts, Account};
use v2_lib::autorun::recipe::load_recipe;
use v2_lib::autorun::replay::Browsers;
use v2_lib::autorun::signin_attempts;
use v2_lib::autorun::signin_inspect::INSPECT_JS;
use v2_lib::autorun::signin_proposal::{load, proposal_path, RESET_PART_WORDS};
use v2_lib::autorun::signin_recorder::MARKER_NAMES_ACCOUNT;
use v2_lib::commands::autorun_signin_proposal::{inspect_in, propose_in};
use v2_lib::environments::{active, load_or_init, save_env_with, set_reset_expired_passwords};

const ORG: &str = "acme";
const PROJECT: &str = "portal";
const HOME: &str = "https://hr.example.internal/";
const RESET_URL: &str = "https://hr.example.internal/hr/security/PasswordReset";

/// Hands out a fresh fake browser per `open` and keeps each one once it is
/// closed, so a test can look at what was sent to it.
struct Fake {
    make: Box<dyn FnMut() -> ScriptedDriver>,
    opened: usize,
    closed: Vec<ScriptedDriver>,
}

impl Fake {
    fn new(make: impl FnMut() -> ScriptedDriver + 'static) -> Self {
        Fake { make: Box::new(make), opened: 0, closed: vec![] }
    }
}

impl Browsers for Fake {
    type D = ScriptedDriver;
    async fn open(&mut self) -> Result<ScriptedDriver, String> {
        self.opened += 1;
        Ok((self.make)())
    }
    async fn close(&mut self, d: ScriptedDriver) {
        self.closed.push(d);
    }
}

fn sup() -> Account {
    Account { key: "sup".into(), ..account() }
}

/// A data folder whose active environment is a test environment with the
/// reset switch on, has `start_url` as its address and holds the account `sup`.
fn root(start_url: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut e = load_or_init(dir.path(), Some("dev-read")).unwrap().environments[0].clone();
    e.test_environment = true;
    e.start_url = start_url.to_string();
    let id = e.id.clone();
    save_env_with(dir.path(), e, || vec!["dev-read".to_string()]).unwrap();
    set_reset_expired_passwords(dir.path(), &id, true).unwrap();
    save_accounts(dir.path(), &[sup()]).unwrap();
    dir
}

fn body(recipe: Value) -> String {
    json!({ "recipe": recipe, "account": "sup" }).to_string()
}

fn good() -> Value {
    serde_json::to_value(recipe()).unwrap()
}

/// `stateful_app`'s page, which also answers the page reader.
fn readable(d: ScriptedDriver, href: &'static str) -> ScriptedDriver {
    d.answering_first(move |m, p| {
        if m != "Runtime.evaluate" {
            return None;
        }
        let value = match p["expression"].as_str().unwrap_or("") {
            e if e == INSPECT_JS => json!({
                "controls": [ { "tag": "input", "type": "text", "id": "user", "name": "", "label": "", "placeholder": "", "password": false, "role": "" } ],
                "actions": [ { "kind": "button", "text": "Sign in", "id": "go" } ]
            }),
            "location.href" => json!(href),
            "document.title" => json!("Sign in"),
            _ => return None,
        };
        Some(Ok(json!({ "result": { "value": value } })))
    })
}

fn signs_in() -> ScriptedDriver {
    readable(stateful_app(false, None).0, "https://hr.example.internal/home")
}

fn never_signs_in() -> ScriptedDriver {
    stateful_app(false, Some("#go")).0
}

/// The site sends the login to its change-password page and the form is
/// there; `submits` counts presses of its send button.
fn on_the_reset_page(submits: Arc<AtomicUsize>) -> ScriptedDriver {
    let last = Arc::new(Mutex::new(String::new()));
    stateful_app(false, None).0.answering_first(move |m, p| match m {
        "Runtime.evaluate" if p["expression"] == "location.href" => Some(Ok(json!({ "result": { "value": RESET_URL } }))),
        "Runtime.evaluate" if p["expression"] == "document.title" => Some(Ok(json!({ "result": { "value": "Reset Password" } }))),
        "Runtime.callFunctionOn" if p["objectId"] == "doc" && p["arguments"][0]["value"].is_string() => {
            *last.lock().unwrap() = p["arguments"][0]["value"].as_str().unwrap().to_string();
            None
        }
        "Input.dispatchMouseEvent" => {
            if p["type"] == "mouseReleased" && last.lock().unwrap().as_str() == "#btnsubmit" {
                submits.fetch_add(1, Ordering::SeqCst);
            }
            Some(Ok(json!({})))
        }
        _ => None,
    })
}

fn no_password_or_host(text: &str) {
    assert!(!text.contains(PASSWORD), "{text}");
    assert!(!text.contains("example.internal"), "{text}");
}

#[tokio::test]
async fn inspect_without_an_account_never_signs_in() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("https://hr.example.internal/login");
    let mut fake = Fake::new(|| readable(stateful_app(false, None).0, "https://hr.example.internal/login?x=1"));
    let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, "{}", &quick()).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["page"]["path"], "/login");
    assert!(v.get("sign_in").is_none(), "{text}");
    no_password_or_host(&text);
    assert_eq!(fake.opened, 1);
    assert_eq!(fake.closed.len(), 1, "the browser was left open");
    let typed = fake.closed[0].calls_to("Input.insertText");
    assert!(typed.is_empty(), "something was typed: {typed:?}");
    assert!(!signin_attempts::is_open(dir.path(), "sup"), "a look at the page counted as an attempt");
}

#[tokio::test]
async fn inspect_with_an_account_tries_the_draft_counts_it_and_reads_where_it_ended() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, &body(good()), &quick()).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["page"]["path"], "/home");
    assert!(v["sign_in"].as_str().unwrap().contains("signed in"), "{text}");
    no_password_or_host(&text);
    assert!(signin_attempts::is_open(dir.path(), "sup"));
    assert_eq!(fake.closed.len(), 1);
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn inspect_takes_both_or_neither() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    for one in [json!({ "account": "sup" }), json!({ "recipe": good() })] {
        let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, &one.to_string(), &quick()).await;
        assert_eq!(status, 400, "{text}");
        assert!(text.contains("both or neither"), "{text}");
    }
    assert_eq!(fake.opened, 0);
}

#[tokio::test]
async fn inspect_refuses_an_unsaved_account_before_a_browser_opens() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    let asked = json!({ "account": "ghost", "recipe": good() }).to_string();
    let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, &asked, &quick()).await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("\"ghost\""), "{text}");
    assert_eq!(fake.opened, 0);
}

#[tokio::test]
async fn a_proposal_that_signs_in_is_stored_and_says_so() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(good()), &quick()).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("\"proposed\":true"), "{text}");
    no_password_or_host(&text);
    let env = active(dir.path()).unwrap();
    let kept = load(dir.path(), &env.id, ORG, PROJECT).unwrap().expect("the proposal was not kept");
    assert_eq!(kept.account, "sup");
    assert!(load_recipe(dir.path(), ORG, PROJECT).unwrap().is_none(), "a proposal saved the recipe");
    assert_eq!(fake.closed.len(), 1);
    assert!(signin_attempts::is_open(dir.path(), "sup"));
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn an_environment_with_its_own_address_makes_the_address_no_change() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root(HOME);
    let mut fake = Fake::new(signs_in);
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(good()), &quick()).await;
    assert_eq!(status, 200, "{text}");
    let env = active(dir.path()).unwrap();
    let kept = load(dir.path(), &env.id, ORG, PROJECT).unwrap().unwrap();
    assert!(kept.changes.iter().any(|c| c == "steps"), "{:?}", kept.changes);
    assert!(!kept.changes.iter().any(|c| c == "address"), "{:?}", kept.changes);
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn a_failed_trial_stores_nothing() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(never_signs_in);
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(good()), &quick()).await;
    assert_eq!(status, 422, "{text}");
    assert!(text.starts_with("the trial did not sign in - nothing was proposed"), "{text}");
    no_password_or_host(&text);
    let env = active(dir.path()).unwrap();
    assert!(!proposal_path(dir.path(), &env.id, ORG, PROJECT).exists());
    assert_eq!(fake.closed.len(), 1, "the browser was left open");
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn a_reset_part_passes_on_the_described_page_without_sending() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let submits = Arc::new(AtomicUsize::new(0));
    let seen = submits.clone();
    let mut fake = Fake::new(move || on_the_reset_page(seen.clone()));
    let mut draft = good();
    draft["password_reset"] = json!({ "path_contains": "/security/PasswordReset", "field": { "css": "#txtOldPassword" } });
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(draft), &quick()).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["summary"].as_str().unwrap().ends_with(RESET_PART_WORDS), "{text}");
    assert_eq!(submits.load(Ordering::SeqCst), 0, "the change-password form was sent");
    let typed = fake.closed[0].calls_to("Input.insertText");
    assert!(typed.iter().all(|c| c["text"] == "kim" || c["text"] == PASSWORD), "{typed:?}");
    let held = load_accounts(dir.path()).unwrap().into_iter().find(|a| a.key == "sup").unwrap();
    assert_eq!(held.password, PASSWORD, "the account's password changed");
    no_password_or_host(&text);
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn a_reset_part_does_not_pass_a_site_that_stops_somewhere_else() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(never_signs_in);
    let mut draft = good();
    draft["password_reset"] = json!({ "path_contains": "/security/PasswordReset", "field": { "css": "#txtOldPassword" } });
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(draft), &quick()).await;
    assert_eq!(status, 422, "{text}");
    let env = active(dir.path()).unwrap();
    assert!(!proposal_path(dir.path(), &env.id, ORG, PROJECT).exists());
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn a_marker_naming_the_account_is_refused() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    let mut draft = good();
    draft["signed_in"] = json!({ "role": "link", "name": "Welcome kim", "exact": true });
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(draft), &quick()).await;
    assert_eq!(status, 400, "{text}");
    assert_eq!(text, MARKER_NAMES_ACCOUNT);
    assert_eq!(fake.opened, 0);
    assert!(!signin_attempts::is_open(dir.path(), "sup"), "a refusal before any sign-in counted as an attempt");
}

#[tokio::test]
async fn a_parse_error_never_quotes_the_body() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    let mut draft = good();
    draft["steps"] = json!("hunter2");
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(draft), &quick()).await;
    assert_eq!(status, 400, "{text}");
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains("propose_sign_in_recipe"), "{text}");
    let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, "{\"password\": \"hunter2\"}", &quick()).await;
    assert_eq!(status, 400, "{text}");
    assert!(!text.contains("hunter2"), "{text}");
    assert_eq!(fake.opened, 0);
}

#[tokio::test]
async fn an_unsaved_account_and_a_spent_account_are_refused_before_a_browser_opens() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    let ghost = json!({ "recipe": good(), "account": "ghost" }).to_string();
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &ghost, &quick()).await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("\"ghost\""), "{text}");
    for _ in 0..3 {
        signin_attempts::take(dir.path(), "sup").unwrap();
    }
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(good()), &quick()).await;
    assert_eq!(status, 409, "{text}");
    assert_eq!(text, signin_attempts::attempts_used("sup"));
    assert_eq!(fake.opened, 0);
    signin_attempts::close(dir.path(), "sup");
}
