//! One sign-in per account at a time (`autorun::lease`): a lease per
//! (environment id, account key), waited for up to a limit, refused at once
//! for the supervised browser, and released when it is dropped - at an end,
//! an error, a stop and a panic alike.
//!
//! The registry is process-wide, so every test here takes
//! `serial::account_leases` first.

use crate::common;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use v2_lib::autorun::accounts::{save_accounts, Account};
use v2_lib::autorun::lease::{self, Held, Holder};
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::replay::{run_cases, Browsers, CaseToRun};
use v2_lib::autorun::runner::run_step_routed;
use v2_lib::autorun::{store, CaseScript, LocalRun};
use v2_lib::browser::cdp::{CdpError, Driver, Event};
use v2_lib::browser::timing::Timing;

const KEY: &str = "admin";

fn env_of(root: &Path) -> String {
    v2_lib::environments::active_id(root).unwrap()
}

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 20, highlight_ms: 0, lease_wait_ms: 300 }
}

fn case_of(run: &str) -> Holder {
    Holder::Case { run: run.to_string() }
}

#[tokio::test]
async fn a_second_acquire_waits_and_gets_the_lease_once_the_first_is_dropped() {
    let _l = crate::serial::account_leases();
    let first = lease::try_acquire("env-aa000001", KEY, Holder::Browser).unwrap();
    let let_go = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(first);
    });
    let began = Instant::now();
    let second = lease::acquire("env-aa000001", KEY, Holder::Template, Duration::from_secs(5)).await;
    let waited = began.elapsed();
    assert!(second.is_ok(), "{second:?}");
    assert!(waited >= Duration::from_millis(150), "it did not wait for the first: {waited:?}");
    assert!(waited < Duration::from_secs(3), "it waited far longer than the first was held: {waited:?}");
    let_go.await.unwrap();
    assert!(lease::is_held("env-aa000001", KEY));
    drop(second);
    assert!(!lease::is_held("env-aa000001", KEY));
}

#[tokio::test]
async fn a_wait_that_runs_out_says_who_had_the_account() {
    let _l = crate::serial::account_leases();
    let env = "env-aa000002";
    let cases = [
        (Holder::Browser, case_of("run-1"), "the Auto Run browser"),
        (Holder::Template, case_of("run-1"), "an API template run"),
        (case_of("run-1"), case_of("run-1"), "another case in this run"),
        (case_of("run-1"), case_of("run-2"), "an unattended run"),
        (case_of("run-1"), Holder::Template, "an unattended run"),
    ];
    for (holder, asking, said) in cases {
        let held = lease::try_acquire(env, KEY, holder).unwrap();
        let began = Instant::now();
        let refused = lease::acquire(env, KEY, asking, Duration::from_millis(120)).await.unwrap_err();
        assert!(began.elapsed() >= Duration::from_millis(100), "it gave up before its wait: {:?}", began.elapsed());
        assert_eq!(refused, format!("the account admin was in use by {said} - try again when it is free"));
        assert!(lease::is_held(env, KEY), "a refusal never takes the lease from its holder");
        drop(held);
    }
    assert!(!lease::is_held(env, KEY));
}

#[tokio::test]
async fn different_accounts_never_block_each_other() {
    let _l = crate::serial::account_leases();
    let a = lease::try_acquire("env-aa000003", "admin", Holder::Browser).unwrap();
    let began = Instant::now();
    let b = lease::acquire("env-aa000003", "manager", Holder::Template, Duration::from_secs(5)).await.unwrap();
    // The same key in another environment is another account.
    let c = lease::acquire("env-aa000004", "admin", case_of("run-1"), Duration::from_secs(5)).await.unwrap();
    assert!(began.elapsed() < Duration::from_millis(200), "an unrelated lease was waited for: {:?}", began.elapsed());
    assert!(lease::is_held("env-aa000003", "admin") && lease::is_held("env-aa000003", "manager"));
    assert!(lease::is_held("env-aa000004", "admin"));
    drop((a, b, c));
}

#[test]
fn a_lease_is_released_when_it_is_dropped() {
    let _l = crate::serial::account_leases();
    {
        let _held = lease::try_acquire("env-aa000005", KEY, Holder::Template).unwrap();
        assert!(lease::is_held("env-aa000005", KEY));
    }
    assert!(!lease::is_held("env-aa000005", KEY));
    // Free again: the next one gets it at once.
    let again = lease::try_acquire("env-aa000005", KEY, Holder::Browser);
    assert!(again.is_ok(), "{again:?}");
}

#[test]
fn a_lease_is_released_on_an_error_return() {
    let _l = crate::serial::account_leases();
    fn holds_then_fails() -> Result<(), String> {
        let _held = lease::try_acquire("env-aa000006", KEY, Holder::Template)?;
        assert!(lease::is_held("env-aa000006", KEY));
        Err("the page did not open".to_string())?;
        Ok(())
    }
    assert!(holds_then_fails().is_err());
    assert!(!lease::is_held("env-aa000006", KEY));
}

#[tokio::test]
async fn a_lease_is_released_when_its_future_is_dropped() {
    let _l = crate::serial::account_leases();
    let job = tokio::spawn(async {
        let _held = lease::acquire("env-aa000007", KEY, case_of("run-1"), Duration::ZERO).await.unwrap();
        std::future::pending::<()>().await;
    });
    let began = Instant::now();
    while !lease::is_held("env-aa000007", KEY) {
        assert!(began.elapsed() < Duration::from_secs(5), "the job never took its lease");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    job.abort();
    assert!(job.await.unwrap_err().is_cancelled());
    assert!(!lease::is_held("env-aa000007", KEY));
}

#[test]
fn a_lease_is_released_on_a_panic() {
    let _l = crate::serial::account_leases();
    let out = std::panic::catch_unwind(|| {
        let _held = lease::try_acquire("env-aa000008", KEY, Holder::Template).unwrap();
        assert!(lease::is_held("env-aa000008", KEY));
        panic!("a step blew up");
    });
    assert!(out.is_err());
    assert!(!lease::is_held("env-aa000008", KEY));
    assert!(lease::try_acquire("env-aa000008", KEY, Holder::Browser).is_ok());
}

/// The supervised browser never waits: asked to sign in as an account
/// someone else holds, it is refused at once.
#[tokio::test]
async fn a_supervised_sign_in_on_a_held_account_is_refused_at_once() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let env = env_of(dir.path());
    let _template = lease::try_acquire(&env, KEY, Holder::Template).unwrap();
    let browser = Held::supervised();
    let began = Instant::now();
    let refused = browser.ready(dir.path(), KEY).await.err().expect("a held account was not refused");
    assert!(began.elapsed() < Duration::from_millis(100), "the supervised browser waited: {:?}", began.elapsed());
    assert_eq!(refused, "the account admin was in use by an API template run - try again when it is free");
    assert_eq!(browser.account(), None);
}

/// A script's `sign_in` in the supervised browser goes through the same
/// refusal: nothing is typed, the rest of the step is not run.
#[tokio::test]
async fn a_supervised_sign_in_step_on_a_held_account_signs_nobody_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    save_accounts(root, &[common::account()]).unwrap();
    let _case = lease::try_acquire(&env_of(root), KEY, case_of("run-1")).unwrap();
    let step = script(1, None, serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "sign_in", "account": "admin" }, { "kind": "check_text", "value": "yes" }
    ] }]))
    .steps
    .remove(0);
    let (mut d, state) = common::stateful_app(false, None);
    let mut account = Some("manager".to_string());
    let mut held = Held::supervised();
    let out = run_step_routed(&mut d, root, "Acme", "Web", &step, &quick(), &mut account, &mut held, None).await.unwrap();
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, "the account admin was in use by an unattended run - try again when it is free");
    assert!(!out[1].ok, "the rest of the step ran after a refused sign-in: {out:?}");
    assert_eq!(state.clicks.load(std::sync::atomic::Ordering::SeqCst), 0, "the sign-in ran");
    assert!(!d.methods().iter().any(|m| m == "Page.navigate"), "the sign-in page was opened");
    assert_eq!(account.as_deref(), Some("manager"), "a refused sign-in changes nothing in the browser");
}

/// The supervised browser holds its account from a sign-in that worked
/// until it signs in as another account (or is dropped with its session).
#[tokio::test]
async fn the_supervised_browser_holds_its_account_until_it_signs_in_as_another() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let env = env_of(root);
    let mut browser = Held::supervised();

    let ready = browser.ready(root, "admin").await.unwrap();
    browser.signed_in(ready, true);
    assert_eq!(browser.account(), Some("admin"));
    assert!(lease::is_held(&env, "admin"));

    // The same account again keeps the lease it has: no refusal from itself.
    let ready = browser.ready(root, "admin").await.unwrap();
    browser.signed_in(ready, true);
    assert!(lease::is_held(&env, "admin"));

    // A failed sign-in as another keeps what was held, and holds nothing new.
    let ready = browser.ready(root, "manager").await.unwrap();
    browser.signed_in(ready, false);
    assert_eq!(browser.account(), Some("admin"));
    assert!(lease::is_held(&env, "admin"));
    assert!(!lease::is_held(&env, "manager"));

    // Another account that worked: the old one is let go.
    let ready = browser.ready(root, "manager").await.unwrap();
    browser.signed_in(ready, true);
    assert_eq!(browser.account(), Some("manager"));
    assert!(!lease::is_held(&env, "admin"));
    assert!(lease::is_held(&env, "manager"));

    drop(browser);
    assert!(!lease::is_held(&env, "manager"), "a closed browser still held its account");
}

// ---- the unattended run ----------------------------------------------

/// Hands out its drivers in turn and keeps each one given back.
struct Queue<D> {
    drivers: VecDeque<D>,
    returned: Vec<D>,
}

impl<D: Driver> Browsers for Queue<D> {
    type D = D;
    async fn open(&mut self) -> Result<D, String> {
        self.drivers.pop_front().ok_or_else(|| "no browser left".to_string())
    }
    async fn close(&mut self, d: D) {
        self.returned.push(d);
    }
}

fn queue<D>(drivers: Vec<D>) -> Queue<D> {
    Queue { drivers: drivers.into(), returned: vec![] }
}

fn script(case_id: i32, account: Option<&str>, steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(
        serde_json::json!({ "case_id": case_id, "title": format!("case {case_id}"), "account": account, "steps": steps }),
    )
    .unwrap()
}

/// A case that signs in as `account` (if any) and then clicks once - a
/// click `stateful_app` always answers `ok`.
fn clicking(case_id: i32, account: Option<&str>) -> CaseScript {
    script(case_id, account, serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": { "css": "#go" } }] }
    ]))
}

/// The case signed in (step 0) and every action of it passed.
fn signed_in_and_ran(rec: &v2_lib::autorun::CaseRecord) -> bool {
    rec.steps.first().is_some_and(|s| s.step_number == 0)
        && rec.steps.iter().flat_map(|s| &s.outcomes).all(|o| o.ok)
}

fn to_run(case_id: i32) -> CaseToRun {
    CaseToRun { case_id, title: format!("case {case_id}"), module: None }
}

fn new_run(id: &str) -> LocalRun {
    LocalRun {
        id: id.into(),
        pbi_id: 42,
        started_at: "1700000000000".into(),
        cases: vec![],
        mode: "unattended".into(),
        published: None,
        environment: None,
    }
}

fn signing_in_root(root: &Path) {
    save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    let mut manager: Account = common::account();
    manager.key = "manager".into();
    save_accounts(root, &[common::account(), manager]).unwrap();
}

#[tokio::test]
async fn an_unattended_case_on_a_held_account_is_blocked_and_the_run_goes_on() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    signing_in_root(root);
    store::save_script(root, &clicking(1, Some("admin"))).unwrap();
    store::save_script(root, &clicking(2, Some("manager"))).unwrap();
    let _browser = lease::try_acquire(&env_of(root), KEY, Holder::Browser).unwrap();
    let (first, first_state) = common::stateful_app(false, None);
    let (second, _) = common::stateful_app(false, None);
    let mut browsers = queue(vec![first, second]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1), to_run(2)], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();

    let blocked = &run.cases[0];
    let sentence = "the account admin was in use by the Auto Run browser - try again when it is free";
    assert_eq!(blocked.proposed, "Blocked");
    assert_eq!(blocked.reason, sentence);
    assert!(blocked.steps.iter().all(|s| s.step_number != 0), "a sign-in was recorded: {:?}", blocked.steps);
    assert!(
        blocked.steps.iter().flat_map(|s| &s.outcomes).all(|o| !o.ok && o.detail == format!("not run: {sentence}")),
        "{:?}",
        blocked.steps
    );
    assert_eq!(first_state.clicks.load(std::sync::atomic::Ordering::SeqCst), 0, "the blocked case signed in");
    assert!(!browsers.returned[0].methods().iter().any(|m| m == "Page.navigate"), "the blocked case opened the sign-in page");
    // The next case, on another account, ran as usual.
    assert!(signed_in_and_ran(&run.cases[1]), "{:?}", run.cases[1]);
    assert!(lease::is_held(&env_of(root), KEY), "the case took the browser's lease");
    assert!(!lease::is_held(&env_of(root), "manager"), "the second case kept its lease after it ended");
}

#[tokio::test]
async fn two_cases_on_one_account_in_a_run_never_block_each_other() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    signing_in_root(root);
    store::save_script(root, &clicking(1, Some("admin"))).unwrap();
    store::save_script(root, &clicking(2, Some("admin"))).unwrap();
    let (first, _) = common::stateful_app(false, None);
    let (second, second_state) = common::stateful_app(false, None);
    let mut browsers = queue(vec![first, second]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    let began = Instant::now();
    // A long wait: a lease the first case kept would show as a slow second.
    let patient = Timing { lease_wait_ms: 5_000, ..quick() };
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1), to_run(2)], None, false, &patient, &cancel, &mut |_| {})
        .await
        .unwrap();
    assert!(began.elapsed() < Duration::from_secs(4), "the second case waited for the first: {:?}", began.elapsed());
    assert!(signed_in_and_ran(&run.cases[0]), "{:?}", run.cases[0]);
    assert!(signed_in_and_ran(&run.cases[1]), "{:?}", run.cases[1]);
    assert!(second_state.clicks.load(std::sync::atomic::Ordering::SeqCst) > 0, "the second case never signed in");
    // The run is over: the account is free.
    assert!(!lease::is_held(&env_of(root), KEY));
}

#[tokio::test]
async fn a_case_whose_sign_in_fails_lets_its_account_go() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    signing_in_root(root);
    store::save_script(root, &clicking(1, Some("admin"))).unwrap();
    // The recipe's button is not on this page: the sign-in fails.
    let (d, _) = common::stateful_app(false, Some("#go"));
    let mut browsers = queue(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1)], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert!(!signed_in_and_ran(&run.cases[0]), "{:?}", run.cases[0]);
    assert!(run.cases[0].steps[0].outcomes.iter().any(|o| !o.ok), "{:?}", run.cases[0].steps[0]);
    assert!(!lease::is_held(&env_of(root), KEY));
}

/// A browser that, once the sign-in has saved its session (its last call),
/// either never answers again or panics.
struct Trap {
    inner: common::ScriptedDriver,
    sprung: bool,
    panics: bool,
}

impl Driver for Trap {
    async fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, CdpError> {
        if self.sprung {
            if self.panics {
                panic!("the browser driver broke mid-case");
            }
            std::future::pending::<()>().await;
        }
        if method == "Network.getAllCookies" {
            self.sprung = true;
        }
        self.inner.call(method, params).await
    }
    async fn wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError> {
        self.inner.wait_event(method, limit).await
    }
    fn forget_events(&mut self) {
        self.inner.forget_events()
    }
    fn take_dialogs(&mut self) -> Vec<String> {
        self.inner.take_dialogs()
    }
    fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.inner.set_deadline(deadline)
    }
}

/// A run of one case on `admin` whose browser is a `Trap`, as a task.
fn trapped_run(panics: bool) -> (tokio::task::JoinHandle<()>, tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    signing_in_root(&root);
    store::save_script(&root, &clicking(1, Some("admin"))).unwrap();
    let env = env_of(&root);
    let (inner, _) = common::stateful_app(false, None);
    let mut browsers = queue(vec![Trap { inner, sprung: false, panics }]);
    let job = tokio::spawn(async move {
        let mut run = new_run("run-x");
        let cancel = AtomicBool::new(false);
        let _ = run_cases(&mut browsers, &root, "Acme", "Web", &mut run, &[to_run(1)], None, false, &quick(), &cancel, &mut |_| {})
            .await;
    });
    (job, dir, env)
}

/// Stopping a run mid-case drops its future: the case's lease goes with it.
#[tokio::test]
async fn a_case_stopped_mid_case_lets_its_account_go() {
    let _l = crate::serial::account_leases();
    let (job, _dir, env) = trapped_run(false);
    let began = Instant::now();
    while !lease::is_held(&env, KEY) {
        assert!(began.elapsed() < Duration::from_secs(5), "the case never took its lease");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Stuck in its step, still holding the account.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(lease::is_held(&env, KEY));
    assert!(!job.is_finished());
    job.abort();
    assert!(job.await.unwrap_err().is_cancelled());
    assert!(!lease::is_held(&env, KEY), "a stopped case kept its account");
}

#[tokio::test]
async fn a_case_that_panics_lets_its_account_go() {
    let _l = crate::serial::account_leases();
    let (job, _dir, env) = trapped_run(true);
    let failed = job.await.unwrap_err();
    assert!(failed.is_panic(), "{failed:?}");
    assert!(!lease::is_held(&env, KEY), "a case that panicked kept its account");
}
