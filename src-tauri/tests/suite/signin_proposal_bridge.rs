//! The assistant's two sign-in tools (`inspect_in`, `propose_in`): what each
//! refuses before a browser opens, what a trial may and may not do, and what
//! is kept. The browser is a fake; nothing here opens a real one.

use crate::common::{account, quick, recipe, stateful_app, ScriptedDriver, PASSWORD};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::accounts::{load_accounts, save_accounts, Account};
use v2_lib::autorun::recipe::{load_recipe, save_recipe, SignInRecipe};
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
    // A trial only goes to a site the app was given: the address, or a saved recipe's.
    root_with(start_url, start_url.is_empty().then(recipe))
}

fn root_with(start_url: &str, saved: Option<SignInRecipe>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    if let Some(r) = saved {
        save_recipe(dir.path(), ORG, PROJECT, &r).unwrap();
    }
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
async fn inspect_takes_a_recipe_only_together_with_an_account() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, &json!({ "recipe": good() }).to_string(), &quick()).await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("recipe needs account"), "{text}");
    assert_eq!(fake.opened, 0);
}

#[tokio::test]
async fn inspect_with_an_account_alone_runs_the_saved_sign_in_and_reads_where_it_lands() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, &json!({ "account": "sup" }).to_string(), &quick()).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["page"]["path"], "/home");
    assert!(v["sign_in"].as_str().unwrap().contains("signed in"), "{text}");
    no_password_or_host(&text);
    let typed = fake.closed[0].calls_to("Input.insertText");
    assert!(typed.iter().any(|c| c["text"] == PASSWORD), "the saved sign-in was not run");
    assert!(signin_attempts::is_open(dir.path(), "sup"));
    signin_attempts::close(dir.path(), "sup");

    // Nothing saved and no address: nothing to sign in with.
    let bare = root_with("", None);
    let (status, text) = inspect_in(&mut fake, bare.path(), ORG, PROJECT, &json!({ "account": "sup" }).to_string(), &quick()).await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("a trial never signs in"), "{text}");
    assert!(!signin_attempts::is_open(bare.path(), "sup"));
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
    let saved = load_recipe(dir.path(), ORG, PROJECT).unwrap().unwrap();
    assert_eq!(serde_json::to_value(&saved).unwrap(), good(), "a proposal changed the saved recipe");
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

fn elsewhere() -> Value {
    let mut v = good();
    v["start_url"] = json!("https://evil.example.test/");
    v["steps"] = json!([{ "kind": "navigate", "url": "https://evil.example.test/login" }]);
    v
}

fn saved_on_another_site() -> SignInRecipe {
    let mut v = good();
    v["start_url"] = json!("https://hr.example.test/");
    serde_json::from_value(v).unwrap()
}

#[tokio::test]
async fn a_trial_never_goes_to_a_site_the_app_was_not_given() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    // A saved recipe elsewhere: a draft on a third host is refused, and so is
    // one that names its own allowed sites to get there.
    let dir = root_with("", Some(saved_on_another_site()));
    let mut fake = Fake::new(signs_in);
    let mut wide = elsewhere();
    wide["allowed_origins"] = json!(["https://evil.example.test"]);
    for draft in [elsewhere(), wide] {
        let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(draft.clone()), &quick()).await;
        assert_eq!(status, 400, "{text}");
        assert!(text.contains("does not use") && !text.contains("evil"), "{text}");
        let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, &body(draft), &quick()).await;
        assert_eq!(status, 400, "{text}");
        assert!(!text.contains("evil"), "{text}");
    }
    assert_eq!(fake.opened, 0);
    assert!(!signin_attempts::is_open(dir.path(), "sup"), "a refused trial spent an attempt");

    // Neither an address nor a saved recipe: nothing to go on.
    let bare = root_with("", None);
    for text in [
        propose_in(&mut fake, bare.path(), ORG, PROJECT, &body(good()), &quick()).await,
        inspect_in(&mut fake, bare.path(), ORG, PROJECT, &body(good()), &quick()).await,
    ] {
        assert_eq!(text.0, 400, "{}", text.1);
        assert!(text.1.contains("a trial never signs in"), "{}", text.1);
    }
    assert_eq!(fake.opened, 0);
    assert!(!signin_attempts::is_open(bare.path(), "sup"));
}

#[tokio::test]
async fn an_inspect_with_an_account_at_the_cap_is_refused_before_a_browser_opens() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    for _ in 0..3 {
        signin_attempts::take(dir.path(), "sup").unwrap();
    }
    let mut fake = Fake::new(signs_in);
    let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, &body(good()), &quick()).await;
    assert_eq!(status, 409, "{text}");
    assert_eq!(text, signin_attempts::attempts_used("sup"));
    assert_eq!(fake.opened, 0);
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn a_failed_trial_and_an_unreadable_page_still_say_why_the_trial_failed() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    // Nothing answers the page reader: the answer is not 200 but carries the trial's words.
    let mut fake = Fake::new(|| stateful_app(false, Some("#go")).0.answering_first(|m, p| {
        (m == "Runtime.evaluate" && p["expression"] == INSPECT_JS).then(|| Ok(json!({ "result": { "value": "nope" } })))
    }));
    let (status, text) = inspect_in(&mut fake, dir.path(), ORG, PROJECT, &body(good()), &quick()).await;
    assert_eq!(status, 422, "{text}");
    assert!(text.contains("could not be read") && text.contains("sign-in"), "{text}");
    no_password_or_host(&text);
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn with_an_environment_address_and_no_saved_recipe_approval_saves_the_environments_site() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root(HOME);
    let mut fake = Fake::new(signs_in);
    let mut draft = good();
    draft["start_url"] = json!("https://evil.example.test/");
    draft["allowed_origins"] = json!(["https://evil.example.test"]);
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(draft), &quick()).await;
    assert_eq!(status, 200, "{text}");
    v2_lib::commands::autorun_signin_proposal::approve_with(dir.path(), ORG, PROJECT, false).unwrap();
    let saved = load_recipe(dir.path(), ORG, PROJECT).unwrap().unwrap();
    let env = active(dir.path()).unwrap();
    assert_eq!(saved.start_url, env.start_url);
    assert_eq!(saved.allowed_origins, env.allowed_origins);
    signin_attempts::close(dir.path(), "sup");
}

#[test]
fn a_planted_off_site_proposal_is_refused_on_approval_and_kept() {
    let dir = root(HOME);
    let env = active(dir.path()).unwrap();
    let mut off = good();
    off["start_url"] = json!("https://evil.example.test/");
    let recipe: SignInRecipe = serde_json::from_value(off).unwrap();
    let stored = v2_lib::autorun::signin_proposal::StoredSignInProposal {
        summary: v2_lib::autorun::signin_proposal::summary(&recipe),
        recipe,
        account: "sup".into(),
        proposed_ms: 1.0,
        saved_hash: String::new(),
        changes: vec![],
    };
    v2_lib::autorun::signin_proposal::save(dir.path(), &env.id, ORG, PROJECT, &stored).unwrap();
    let why = v2_lib::commands::autorun_signin_proposal::approve_with(dir.path(), ORG, PROJECT, false).unwrap_err();
    assert!(!why.contains("evil"), "{why}");
    assert!(load_recipe(dir.path(), ORG, PROJECT).unwrap().is_none(), "an off-site recipe was saved");
    assert!(proposal_path(dir.path(), &env.id, ORG, PROJECT).exists(), "the proposal was dropped");
}

#[tokio::test]
async fn clearing_the_address_before_approval_cannot_make_the_drafts_site_the_recipe() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root_with(HOME, Some(recipe()));
    let mut fake = Fake::new(signs_in);
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(off_site_start()), &quick()).await;
    assert_eq!(status, 200, "{text}");
    let env = active(dir.path()).unwrap();
    let kept = load(dir.path(), &env.id, ORG, PROJECT).unwrap().unwrap();
    assert_eq!(kept.recipe.start_url, HOME, "the stored proposal carries the draft's site");
    // The owner clears the address; the proposal was not made for that.
    let mut cleared = env.clone();
    cleared.start_url = String::new();
    save_env_with(dir.path(), cleared, || vec!["dev-read".to_string()]).unwrap();
    let mut planted = kept.clone();
    planted.recipe.start_url = "https://evil.example.test/".to_string();
    v2_lib::autorun::signin_proposal::save(dir.path(), &env.id, ORG, PROJECT, &planted).unwrap();
    let why = v2_lib::commands::autorun_signin_proposal::approve_with(dir.path(), ORG, PROJECT, false).unwrap_err();
    assert!(!why.contains("evil"), "{why}");
    let saved = load_recipe(dir.path(), ORG, PROJECT).unwrap().unwrap();
    assert_eq!(serde_json::to_value(&saved).unwrap(), good(), "the saved recipe changed");
    assert!(proposal_path(dir.path(), &env.id, ORG, PROJECT).exists());
    signin_attempts::close(dir.path(), "sup");
}

#[test]
fn approval_with_no_address_and_nothing_saved_is_refused() {
    let dir = root_with("", None);
    let env = active(dir.path()).unwrap();
    let r: SignInRecipe = serde_json::from_value(good()).unwrap();
    let stored = v2_lib::autorun::signin_proposal::StoredSignInProposal {
        summary: v2_lib::autorun::signin_proposal::summary(&r),
        recipe: r,
        account: "sup".into(),
        proposed_ms: 1.0,
        saved_hash: String::new(),
        changes: vec![],
    };
    v2_lib::autorun::signin_proposal::save(dir.path(), &env.id, ORG, PROJECT, &stored).unwrap();
    assert!(v2_lib::commands::autorun_signin_proposal::approve_with(dir.path(), ORG, PROJECT, false).is_err());
    assert!(load_recipe(dir.path(), ORG, PROJECT).unwrap().is_none());
    assert!(proposal_path(dir.path(), &env.id, ORG, PROJECT).exists());
}

fn off_site_start() -> Value {
    let mut v = good();
    v["start_url"] = json!("https://evil.example.test/");
    v
}

fn reset_part() -> Value {
    json!({ "path_contains": "/security/PasswordReset", "field": { "css": "#txtOldPassword" } })
}

#[tokio::test]
async fn a_full_draft_that_only_reaches_the_reset_page_keeps_the_saved_sign_in() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let submits = Arc::new(AtomicUsize::new(0));
    let seen = submits.clone();
    let mut fake = Fake::new(move || on_the_reset_page(seen.clone()));
    let mut draft = good();
    draft["password_reset"] = reset_part();
    draft["signed_in_in_menu"] = json!(true);
    draft["steps"] = Value::Array(draft["steps"].as_array().unwrap()[..3].to_vec());
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(draft), &quick()).await;
    assert_eq!(status, 200, "{text}");
    let env = active(dir.path()).unwrap();
    let kept = load(dir.path(), &env.id, ORG, PROJECT).unwrap().expect("the proposal was not kept");
    let saved = load_recipe(dir.path(), ORG, PROJECT).unwrap().unwrap();
    assert_eq!(kept.recipe.steps, saved.steps, "the draft's steps replaced the saved ones");
    assert_eq!(kept.recipe.signed_in_in_menu, saved.signed_in_in_menu, "the draft's check replaced the saved one");
    assert_eq!(kept.recipe.start_url, saved.start_url);
    assert_eq!(kept.recipe.password_reset, serde_json::from_value(reset_part()).ok());
    assert_eq!(kept.changes, vec!["change-password part".to_string()], "{:?}", kept.changes);
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn a_full_draft_that_only_reaches_the_reset_page_with_nothing_saved_is_refused() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root_with(HOME, None);
    let submits = Arc::new(AtomicUsize::new(0));
    let seen = submits.clone();
    let mut fake = Fake::new(move || on_the_reset_page(seen.clone()));
    let mut draft = good();
    draft["password_reset"] = reset_part();
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(draft), &quick()).await;
    assert_eq!(status, 422, "{text}");
    assert!(text.contains("never signed in") && text.contains("record a sign-in first"), "{text}");
    no_password_or_host(&text);
    let env = active(dir.path()).unwrap();
    assert!(!proposal_path(dir.path(), &env.id, ORG, PROJECT).exists(), "a sign-in that never signed in was kept");
    signin_attempts::close(dir.path(), "sup");
}

#[tokio::test]
async fn a_reset_part_alone_is_laid_over_the_saved_sign_in() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let submits = Arc::new(AtomicUsize::new(0));
    let seen = submits.clone();
    let mut fake = Fake::new(move || on_the_reset_page(seen.clone()));
    let asked = json!({ "password_reset": reset_part(), "account": "sup" }).to_string();
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &asked, &quick()).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["summary"].as_str().unwrap().ends_with(RESET_PART_WORDS), "{text}");
    assert_eq!(submits.load(Ordering::SeqCst), 0, "the change-password form was sent");
    let env = active(dir.path()).unwrap();
    let kept = load(dir.path(), &env.id, ORG, PROJECT).unwrap().expect("the proposal was not kept");
    let saved = load_recipe(dir.path(), ORG, PROJECT).unwrap().unwrap();
    assert_eq!(kept.recipe.steps, saved.steps);
    assert_eq!(kept.recipe.signed_in, saved.signed_in);
    assert!(kept.recipe.password_reset.is_some());
    signin_attempts::close(dir.path(), "sup");

    // With nothing saved there is no sign-in to put it on.
    let bare = root_with(HOME, None);
    let (status, text) = propose_in(&mut fake, bare.path(), ORG, PROJECT, &asked, &quick()).await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("record a sign-in first"), "{text}");
    assert!(!signin_attempts::is_open(bare.path(), "sup"));
}

#[tokio::test]
async fn a_proposal_gives_a_recipe_or_a_reset_part_never_both_or_neither() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    for asked in [
        json!({ "recipe": good(), "password_reset": reset_part(), "account": "sup" }),
        json!({ "account": "sup" }),
    ] {
        let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &asked.to_string(), &quick()).await;
        assert_eq!(status, 400, "{text}");
        assert!(text.contains("never both"), "{text}");
    }
    assert_eq!(fake.opened, 0);
    assert!(!signin_attempts::is_open(dir.path(), "sup"));
}

/// A browser that will not open.
struct Broken;

impl Browsers for Broken {
    type D = ScriptedDriver;
    async fn open(&mut self) -> Result<ScriptedDriver, String> {
        Err("no browser".to_string())
    }
    async fn close(&mut self, _d: ScriptedDriver) {}
}

#[tokio::test]
async fn a_browser_that_will_not_open_spends_no_attempt() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let (status, text) = propose_in(&mut Broken, dir.path(), ORG, PROJECT, &body(good()), &quick()).await;
    assert_eq!(status, 422, "{text}");
    let asked = json!({ "account": "sup" }).to_string();
    let (status, text) = inspect_in(&mut Broken, dir.path(), ORG, PROJECT, &asked, &quick()).await;
    assert_eq!(status, 422, "{text}");
    assert!(!signin_attempts::is_open(dir.path(), "sup"), "an attempt was spent with no browser");
}

#[tokio::test]
async fn approving_a_proposal_closes_the_accounts_sign_in_problem() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _n = crate::serial::sign_in_attempts();
    let dir = root("");
    let mut fake = Fake::new(signs_in);
    let (status, text) = propose_in(&mut fake, dir.path(), ORG, PROJECT, &body(good()), &quick()).await;
    assert_eq!(status, 200, "{text}");
    assert!(signin_attempts::is_open(dir.path(), "sup"));
    v2_lib::commands::autorun_signin_proposal::approve_with(dir.path(), ORG, PROJECT, false).unwrap();
    assert!(!signin_attempts::is_open(dir.path(), "sup"), "the problem stayed open after approval");
}
