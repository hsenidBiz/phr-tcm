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
    let first = lease::try_acquire("env-aa000001", KEY, case_of("run-1")).unwrap();
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
        (Holder::Template, case_of("run-1"), "an API template run"),
        (case_of("run-1"), case_of("run-1"), "another case in this run"),
        (case_of("run-1"), case_of("run-2"), "an unattended run"),
        (case_of("run-1"), Holder::Template, "an unattended run"),
        (case_of("run-1"), Holder::Setup, "an unattended run"),
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

/// The supervised browser and Auto Run setup hold an account until a person
/// closes them: a waiter is refused at once, with no polling.
#[tokio::test]
async fn nothing_waits_on_an_account_a_person_holds() {
    let _l = crate::serial::account_leases();
    let env = "env-aa00000a";
    for (holder, said) in [(Holder::Browser { slot: 0 }, "the Auto Run browser"), (Holder::Setup, "Auto Run setup")] {
        let held = lease::try_acquire(env, KEY, holder).unwrap();
        for asking in [case_of("run-1"), Holder::Template, Holder::Browser { slot: 0 }, Holder::Setup] {
            let began = Instant::now();
            let refused = lease::acquire(env, KEY, asking, Duration::from_secs(5)).await.unwrap_err();
            assert!(began.elapsed() < Duration::from_millis(100), "it waited on {said}: {:?}", began.elapsed());
            assert_eq!(refused, format!("the account admin was in use by {said} - try again when it is free"));
        }
        drop(held);
    }
    assert!(!lease::is_held(env, KEY));
}

#[tokio::test]
async fn different_accounts_never_block_each_other() {
    let _l = crate::serial::account_leases();
    let a = lease::try_acquire("env-aa000003", "admin", Holder::Browser { slot: 0 }).unwrap();
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
    let again = lease::try_acquire("env-aa000005", KEY, Holder::Browser { slot: 0 });
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
    assert!(lease::try_acquire("env-aa000008", KEY, Holder::Browser { slot: 0 }).is_ok());
}

/// The supervised browser never waits: asked to sign in as an account
/// someone else holds, it is refused at once.
#[tokio::test]
async fn a_supervised_sign_in_on_a_held_account_is_refused_at_once() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let env = env_of(dir.path());
    let _template = lease::try_acquire(&env, KEY, Holder::Template).unwrap();
    let mut browser = Held::supervised();
    let began = Instant::now();
    let refused = browser.hold(dir.path(), KEY).await.expect_err("a held account was not refused");
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
    let out = run_step_routed(&mut d, root, "Acme", "Web", &step, &quick(), &mut account, &mut held, None, v2_lib::autorun::runner::AreaRoute::Unknown(v2_lib::autorun::runner::NEEDS_SCRIPT_AREA)).await.unwrap();
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, "the account admin was in use by an unattended run - try again when it is free");
    assert!(!out[1].ok, "the rest of the step ran after a refused sign-in: {out:?}");
    assert_eq!(state.clicks.load(std::sync::atomic::Ordering::SeqCst), 0, "the sign-in ran");
    assert!(!d.methods().iter().any(|m| m == "Page.navigate"), "the sign-in page was opened");
    assert_eq!(account.as_deref(), Some("manager"), "a refused sign-in changes nothing in the browser");
}

/// The supervised browser holds its account from a sign-in until it signs
/// in as another account (or is dropped with its session). A refusal
/// leaves what it held.
#[tokio::test]
async fn the_supervised_browser_holds_its_account_until_it_signs_in_as_another() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let env = env_of(root);
    let mut browser = Held::supervised();

    browser.hold(root, "admin").await.unwrap();
    assert_eq!(browser.account(), Some("admin"));
    assert!(lease::is_held(&env, "admin"));

    // The same account again keeps the lease it has: no refusal from itself.
    browser.hold(root, "admin").await.unwrap();
    assert!(lease::is_held(&env, "admin"));

    // Refused for an account a case holds: what was held stays held.
    let case = lease::try_acquire(&env, "manager", case_of("run-1")).unwrap();
    assert!(browser.hold(root, "manager").await.is_err());
    assert_eq!(browser.account(), Some("admin"));
    assert!(lease::is_held(&env, "admin"));
    drop(case);

    // Another account: the old one is let go.
    browser.hold(root, "manager").await.unwrap();
    assert_eq!(browser.account(), Some("manager"));
    assert!(!lease::is_held(&env, "admin"));
    assert!(lease::is_held(&env, "manager"));

    drop(browser);
    assert!(!lease::is_held(&env, "manager"), "a closed browser still held its account");
}

/// A `sign_in` that fails may still have signed the account in: the
/// browser keeps that account's lease, in place of the one before.
#[tokio::test]
async fn a_sign_in_step_that_fails_keeps_its_account() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    signing_in_root(root);
    let env = env_of(root);
    let step = script(1, None, serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "sign_in", "account": "admin" }
    ] }]))
    .steps
    .remove(0);
    // The recipe's button is not on this page: the sign-in fails.
    let (mut d, _) = common::stateful_app(false, Some("#go"));
    let mut account = Some("manager".to_string());
    let mut held = Held::supervised();
    held.hold(root, "manager").await.unwrap();
    let out = run_step_routed(&mut d, root, "Acme", "Web", &step, &quick(), &mut account, &mut held, None, v2_lib::autorun::runner::AreaRoute::Unknown(v2_lib::autorun::runner::NEEDS_SCRIPT_AREA)).await.unwrap();
    assert!(!out[0].ok, "{out:?}");
    assert_eq!(held.account(), Some("admin"));
    assert!(lease::is_held(&env, "admin"), "a failed sign-in let its account go");
    assert!(!lease::is_held(&env, "manager"));
}

// ---- the unattended run ----------------------------------------------

/// Hands out its drivers in turn and keeps each one given back. With
/// `watch`, it notes at each close whether that account was still held.
struct Queue<D> {
    drivers: VecDeque<D>,
    returned: Vec<D>,
    watch: Option<(String, String)>,
    held_at_close: Vec<bool>,
}

impl<D: Driver> Browsers for Queue<D> {
    type D = D;
    async fn open(&mut self) -> Result<D, String> {
        self.drivers.pop_front().ok_or_else(|| "no browser left".to_string())
    }
    async fn close(&mut self, d: D) {
        if let Some((env, key)) = &self.watch {
            self.held_at_close.push(lease::is_held(env, key));
        }
        self.returned.push(d);
    }
}

fn queue<D>(drivers: Vec<D>) -> Queue<D> {
    Queue { drivers: drivers.into(), returned: vec![], watch: None, held_at_close: vec![] }
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
        resets: vec![],
        network_pauses: vec![],
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
    let _browser = lease::try_acquire(&env_of(root), KEY, Holder::Browser { slot: 0 }).unwrap();
    let (first, first_state) = common::stateful_app(false, None);
    let (second, _) = common::stateful_app(false, None);
    let mut browsers = queue(vec![first, second]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    // A long wait it must not use: the browser holds the account until a
    // person closes it.
    let patient = Timing { lease_wait_ms: 5_000, ..quick() };
    let began = Instant::now();
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1), to_run(2)], None, false, &patient, &cancel, &mut |_| {})
        .await
        .unwrap();
    assert!(began.elapsed() < Duration::from_secs(2), "the case waited on the Auto Run browser: {:?}", began.elapsed());

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
    browsers.watch = Some((env_of(root), KEY.to_string()));
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
    // Each case let its account go only once its browser was closed.
    assert_eq!(browsers.held_at_close, vec![true, true]);
    // The run is over: the account is free.
    assert!(!lease::is_held(&env_of(root), KEY));
}

/// A case of another run holds the account: the case waits for it, then
/// signs in.
#[tokio::test]
async fn an_unattended_case_waits_for_another_runs_case_then_signs_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    signing_in_root(root);
    store::save_script(root, &clicking(1, Some("admin"))).unwrap();
    let other = lease::try_acquire(&env_of(root), KEY, case_of("run-other")).unwrap();
    let ends = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(other);
    });
    let (d, state) = common::stateful_app(false, None);
    let mut browsers = queue(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    let patient = Timing { lease_wait_ms: 5_000, ..quick() };
    let began = Instant::now();
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1)], None, false, &patient, &cancel, &mut |_| {})
        .await
        .unwrap();
    ends.await.unwrap();
    assert!(began.elapsed() >= Duration::from_millis(250), "it did not wait for the other run's case: {:?}", began.elapsed());
    assert!(signed_in_and_ran(&run.cases[0]), "{:?}", run.cases[0]);
    assert!(state.clicks.load(std::sync::atomic::Ordering::SeqCst) > 0);
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
    browsers.watch = Some((env_of(root), KEY.to_string()));
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1)], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert!(!signed_in_and_ran(&run.cases[0]), "{:?}", run.cases[0]);
    assert!(run.cases[0].steps[0].outcomes.iter().any(|o| !o.ok), "{:?}", run.cases[0].steps[0]);
    assert_eq!(browsers.held_at_close, vec![true], "a failed sign-in let its account go before its browser closed");
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

// ---- Auto Run setup ---------------------------------------------------

fn leave_path() -> v2_lib::autorun::nav::ModulePath {
    serde_json::from_value(serde_json::json!({
        "module": "Leave",
        "clicks": [ { "role": "link", "name": "Leave", "exact": true }, { "role": "link", "name": "Apply Leave", "exact": true } ],
        "arrived": "/hr/leave/apply",
        "recorded": "2026-09-24T10:00:00Z"
    }))
    .unwrap()
}

/// The module-path check (and Try), the module-path recording and the
/// recorded sign-in's check each clear their sign-in with the account
/// lease: a held account is refused at once, before the browser is asked
/// anything.
#[tokio::test]
async fn every_setup_sign_in_is_refused_at_once_on_a_held_account() {
    use v2_lib::autorun::nav::check_path;
    use v2_lib::commands::autorun_record::prepare_to_record;
    use v2_lib::commands::autorun_record_signin::check_sign_in;
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let _case = lease::try_acquire(&env_of(root), KEY, case_of("run-1")).unwrap();
    let sentence = "the account admin was in use by an unattended run - try again when it is free";
    let patient = Timing { lease_wait_ms: 5_000, ..quick() };

    let (mut d, _) = common::stateful_app(false, None);
    let began = Instant::now();
    let err = check_path(&mut d, root, &common::menu_recipe(), &common::account(), &leave_path(), &patient, &mut Held::setup())
        .await
        .unwrap_err();
    assert_eq!(err, sentence, "the module-path check");
    assert!(d.calls.is_empty(), "the module-path check used the browser: {:?}", d.methods());

    let (mut d, _) = common::stateful_app(false, None);
    let err = prepare_to_record(&mut d, root, &common::menu_recipe(), &common::account(), &patient, &mut Held::setup())
        .await
        .unwrap_err();
    assert_eq!(err, sentence, "the module-path recording");
    assert!(d.calls.is_empty(), "the module-path recording used the browser: {:?}", d.methods());

    let (mut d, _) = common::stateful_app(false, None);
    let err = check_sign_in(&mut d, root, &common::recipe(), &common::account(), &patient, &mut Held::setup())
        .await
        .unwrap_err();
    assert_eq!(err, sentence, "the recorded sign-in's check");
    assert!(d.calls.is_empty(), "the recorded sign-in's check used the browser: {:?}", d.methods());

    assert!(began.elapsed() < Duration::from_millis(500), "a setup sign-in waited: {:?}", began.elapsed());
}

/// A setup sign-in holds its account until its own lease is dropped, which
/// its command does once its browser is closed; anything else asking for
/// the account meanwhile is refused at once with `Auto Run setup`.
#[tokio::test]
async fn a_setup_sign_in_holds_its_account_until_it_ends() {
    use v2_lib::autorun::nav::check_path;
    use v2_lib::commands::autorun_record_signin::check_sign_in;
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let env = env_of(root);

    // The recorded sign-in's check, signing in.
    let (mut d, _) = common::stateful_app(false, None);
    let mut setup = Held::setup();
    check_sign_in(&mut d, root, &common::recipe(), &common::account(), &quick(), &mut setup).await.unwrap();
    assert!(lease::is_held(&env, KEY));
    let began = Instant::now();
    let refused = lease::acquire(&env, KEY, Holder::Template, Duration::from_secs(5)).await.unwrap_err();
    assert!(began.elapsed() < Duration::from_millis(100), "a template run waited on Auto Run setup");
    assert_eq!(refused, "the account admin was in use by Auto Run setup - try again when it is free");
    drop(setup);
    assert!(!lease::is_held(&env, KEY));

    // The module-path check, whose sign-in fails: the account stays held
    // until the check ends all the same.
    let (mut d, _) = common::stateful_app(false, Some("#go"));
    let mut setup = Held::setup();
    let err = check_path(&mut d, root, &common::recipe(), &common::account(), &leave_path(), &quick(), &mut setup).await;
    assert!(err.is_err());
    assert!(lease::is_held(&env, KEY), "a failed setup sign-in let its account go");
    drop(setup);
    assert!(!lease::is_held(&env, KEY));
}


#[test]
fn a_lease_refusal_is_told_by_its_exact_sentence() {
    for holder in ["the Auto Run browser", "an API template run", "another case in this run", "an unattended run", "Auto Run setup"] {
        assert!(lease::is_in_use(&format!("the account manager was in use by {holder} - try again when it is free")), "{holder}");
    }
    assert!(!lease::is_in_use("the account manager was in use by somebody - try again when it is free"));
    assert!(!lease::is_in_use("the account  was in use by an API template run - try again when it is free"));
    assert!(!lease::is_in_use("the account a b was in use by an API template run - try again when it is free"));
    assert!(!lease::is_in_use("step 3: the account manager was in use by an API template run - try again when it is free"));
    assert!(!lease::is_in_use("the account manager was in use by an API template run"));
}

/// Review fix 1: a `sign_in` in the middle of a case refused its account
/// is Blocked with the lease's sentence - never Failed, which would be
/// published as a test the application failed.
#[tokio::test]
async fn a_mid_case_sign_in_refused_its_account_blocks_the_case() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    signing_in_root(root);
    store::save_script(
        root,
        &script(1, Some("admin"), serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "css": "#go" } }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "css": "#go" } }] },
            { "step_number": 3, "actions": [
                { "kind": "sign_in", "account": "manager" },
                { "kind": "click", "selector": { "css": "#go" } }
            ] }
        ])),
    )
    .unwrap();
    let _template = lease::try_acquire(&env_of(root), "manager", Holder::Template).unwrap();
    let (d, _) = common::stateful_app(false, None);
    let mut browsers = queue(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1)], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    let sentence = "the account manager was in use by an API template run - try again when it is free";
    let case = &run.cases[0];
    assert_eq!(case.proposed, "Blocked", "{case:?}");
    assert_eq!(case.reason, sentence);
    let failed = v2_lib::autorun::failures::stop_reason(case);
    assert!(failed.is_some(), "the case is offered for repair");
}

/// Review fix 3: Stop pressed while a case waits for its account ends the
/// wait, and the case never signs in.
#[tokio::test]
async fn stop_during_a_lease_wait_stops_the_case_before_its_sign_in() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    signing_in_root(root);
    store::save_script(root, &clicking(1, Some("admin"))).unwrap();
    let _template = lease::try_acquire(&env_of(root), KEY, Holder::Template).unwrap();
    let (d, state) = common::stateful_app(false, None);
    let mut browsers = queue(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    let patient = Timing { lease_wait_ms: 5_000, ..quick() };
    let began = Instant::now();
    let stop = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    };
    let cases = [to_run(1)];
    let mut progress = |_| {};
    let (ran, ()) = tokio::join!(
        run_cases(&mut browsers, root, "Acme", "Web", &mut run, &cases, None, false, &patient, &cancel, &mut progress),
        stop
    );
    ran.unwrap();
    assert!(began.elapsed() < Duration::from_secs(2), "the wait outlived the Stop: {:?}", began.elapsed());
    assert_eq!(state.clicks.load(std::sync::atomic::Ordering::SeqCst), 0, "the stopped case ran");
    assert!(!browsers.returned[0].methods().iter().any(|m| m == "Page.navigate"), "the stopped case signed in");
    let case = &run.cases[0];
    assert!(case.steps.iter().all(|s| s.step_number != 0), "a sign-in was recorded: {:?}", case.steps);
    assert!(case.steps.iter().flat_map(|s| &s.outcomes).all(|o| o.detail == "not run: the run was stopped"), "{:?}", case.steps);
    assert_eq!(case.proposed, "");
    assert_eq!(case.reason, "stopped before it finished");
}

/// A browser holding its account again keeps the lease it has: no new take,
/// so the account's generation (`lease::generation`) does not move and a
/// held template browser does not give way to it. Another account does take.
#[tokio::test]
async fn a_re_hold_does_not_bump_the_generation() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let env = env_of(root);
    let mut browser = Held::supervised();

    browser.hold(root, "admin").await.unwrap();
    let once = lease::generation(&env, "admin");
    browser.hold(root, "admin").await.unwrap();
    assert_eq!(lease::generation(&env, "admin"), once, "a re-hold counted as a new sign-in");

    let manager = lease::generation(&env, "manager");
    browser.hold(root, "manager").await.unwrap();
    assert_eq!(lease::generation(&env, "manager"), manager + 1);
}

// ---------------------------------------------------------------------------
// Accounts across browsers: a refusal names the assistant whose browser holds
// the account, and nothing ever waits for one.
// ---------------------------------------------------------------------------

mod across_browsers {
    use super::*;
    use v2_lib::ai_bridge::{in_use_by_words, with_holder_named};
    use v2_lib::autorun::agents::{self, AgentHeaders, AgentKey};
    use v2_lib::autorun::slots::SlotNo;
    use v2_lib::commands::autorun_slots as slots;

    const ID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// An agent first seen at `now`: labelled by the order they register in.
    fn register(id: &str, now: u64) -> AgentKey {
        let h = AgentHeaders { id: Some(id.to_string()), pid: Some(4242), client: Some("claude-code".into()), name: None };
        agents::note_call(&h.caller(), &h, now);
        AgentKey { id: id.to_string(), name: None }
    }

    fn fresh() {
        agents::clear();
        slots::clear();
    }

    fn named_by(label: &str) -> String {
        format!(". {label} is signed in as it: work on cases that sign in as another account, or wait until {label} is done.")
    }

    #[test]
    fn an_account_held_by_another_slot_is_refused_naming_its_label() {
        let _a = crate::serial::agents();
        let _l = crate::serial::account_leases();
        fresh();
        let env = "env-ab000001";
        register(ID_A, 1_000);
        let b = register(ID_B, 2_000);
        assert_eq!(agents::info(&b).unwrap().label, "Assistant 2");
        slots::bind(SlotNo(1), &b);

        let held = lease::try_acquire(env, "hr_admin", Holder::Browser { slot: 1 }).unwrap();
        let sentence = lease::try_acquire(env, "hr_admin", Holder::Browser { slot: 0 }).unwrap_err();
        assert_eq!(sentence, "the account hr_admin was in use by the Auto Run browser - try again when it is free");

        let named = with_holder_named((409, sentence.clone()), SlotNo::MAIN, env);
        assert_eq!(named, (409, format!("{sentence}{}", named_by("Assistant 2"))));

        // Words after the sentence stay after it, and a full stop already
        // there is not doubled.
        let within = format!("Could not sign in: {sentence}.");
        let (status, text) = with_holder_named((409, within), SlotNo::MAIN, env);
        assert_eq!(status, 409);
        assert_eq!(text, format!("Could not sign in: {sentence}{}", named_by("Assistant 2")));
        assert!(!text.ends_with(".."), "{text}");
        let within = format!("{sentence} (and more words)");
        let text = with_holder_named((409, within), SlotNo::MAIN, env).1;
        assert_eq!(text, format!("{sentence}{} (and more words)", named_by("Assistant 2")));

        // The holder asking is told nothing more; neither is an answer that
        // holds no refusal, nor a refusal of an account nobody now holds.
        assert_eq!(with_holder_named((409, sentence.clone()), SlotNo(1), env).1, sentence);
        assert_eq!(with_holder_named((200, "all went well".into()), SlotNo::MAIN, env).1, "all went well");
        assert_eq!(with_holder_named((409, sentence.clone()), SlotNo::MAIN, "env-ab0000ff").1, sentence);

        // A slot nobody is bound to is named by nobody.
        slots::unbind(SlotNo(1));
        assert_eq!(with_holder_named((409, sentence.clone()), SlotNo::MAIN, env).1, sentence);
        drop(held);
        fresh();
    }

    /// An assistant on main is named too, to one on an extra browser; a gone
    /// assistant is not.
    #[test]
    fn the_assistant_on_main_is_named_to_one_on_an_extra_browser() {
        let _a = crate::serial::agents();
        let _l = crate::serial::account_leases();
        fresh();
        let env = "env-ab000002";
        let a = register(ID_A, 1_000);
        let b = register(ID_B, 2_000);
        slots::bind(SlotNo::MAIN, &a);
        slots::bind(SlotNo(1), &b);
        let held = lease::try_acquire(env, "hr_admin", Holder::Browser { slot: 0 }).unwrap();
        let sentence = lease::try_acquire(env, "hr_admin", Holder::Browser { slot: 1 }).unwrap_err();
        assert_eq!(
            with_holder_named((409, sentence.clone()), SlotNo(1), env).1,
            format!("{sentence}{}", named_by("Assistant 1"))
        );
        agents::note_bye(ID_A, 3_000);
        assert_eq!(with_holder_named((409, sentence.clone()), SlotNo(1), env).1, sentence, "a gone assistant was named");
        drop(held);
        fresh();
    }

    #[test]
    fn is_in_use_still_matches_the_browser_refusal() {
        let _l = crate::serial::account_leases();
        for slot in 0..3u8 {
            let env = format!("env-ab00001{slot}");
            let held = lease::try_acquire(&env, KEY, Holder::Browser { slot }).unwrap();
            for asking in [Holder::Browser { slot: (slot + 1) % 3 }, Holder::Template, Holder::Setup, case_of("run-1")] {
                let refused = lease::try_acquire(&env, KEY, asking).unwrap_err();
                assert!(lease::is_in_use(&refused), "{refused}");
                assert_eq!(refused, lease::in_use_by_browser(KEY));
            }
            drop(held);
        }
        assert!(lease::is_in_use("the account admin was in use by the Auto Run browser - try again when it is free"));
        assert!(!lease::is_in_use("the account admin was in use by Assistant 2 - try again when it is free"));
    }

    /// A browser holds an account until a person (or the app) closes it, so
    /// nothing waits on one, whichever slot holds it.
    #[tokio::test]
    async fn a_browser_holder_is_never_waited_for() {
        let _l = crate::serial::account_leases();
        let env = "env-ab000003";
        let held = lease::try_acquire(env, KEY, Holder::Browser { slot: 1 }).unwrap();
        for asking in [Holder::Browser { slot: 0 }, Holder::Browser { slot: 2 }, Holder::Template, Holder::Setup, case_of("run-9")] {
            let began = Instant::now();
            let refused = lease::acquire(env, KEY, asking, lease::WAIT).await;
            assert!(began.elapsed() < Duration::from_millis(100), "it waited on a browser: {:?}", began.elapsed());
            assert_eq!(refused.unwrap_err(), lease::in_use_by_browser(KEY));
        }
        drop(held);
        assert!(!lease::is_held(env, KEY));
    }

    #[test]
    fn who_holds_an_account_can_be_looked_up() {
        let _l = crate::serial::account_leases();
        let env = "env-ab000004";
        assert_eq!(lease::holder_of(env, "one"), None);
        assert!(lease::holders_in(env).is_empty());
        let one = lease::try_acquire(env, "one", Holder::Browser { slot: 2 }).unwrap();
        let two = lease::try_acquire(env, "two", case_of("run-3")).unwrap();
        let other_env = lease::try_acquire("env-ab000005", "three", Holder::Template).unwrap();
        assert_eq!(lease::holder_of(env, "one"), Some(Holder::Browser { slot: 2 }));
        assert_eq!(lease::holder_of(env, "two"), Some(case_of("run-3")));
        let mut in_env = lease::holders_in(env);
        in_env.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(in_env, vec![("one".to_string(), Holder::Browser { slot: 2 }), ("two".to_string(), case_of("run-3"))]);
        drop((one, two, other_env));
        assert_eq!(lease::holder_of(env, "one"), None);
    }

    #[test]
    fn a_refusal_is_found_inside_an_answer() {
        let sentence = |key: &str, by: &str| format!("the account {key} was in use by {by} - try again when it is free");
        assert_eq!(
            lease::find_in_use(&sentence("hr_admin", "the Auto Run browser")),
            Some(("hr_admin".to_string(), "the Auto Run browser".to_string()))
        );
        let inside = format!("Step 1 failed: {}. Nothing else ran.", sentence("manager", "an unattended run"));
        assert_eq!(lease::find_in_use(&inside), Some(("manager".to_string(), "an unattended run".to_string())));
        // The first one, not the last.
        let two = format!("{} / {}", sentence("a", "Auto Run setup"), sentence("b", "an API template run"));
        assert_eq!(lease::find_in_use(&two).unwrap().0, "a");
        // Not a refusal: another holder, a key with a space, no ending.
        assert_eq!(lease::find_in_use(&sentence("a", "somebody")), None);
        assert_eq!(lease::find_in_use("the account a b was in use by an unattended run - try again when it is free"), None);
        assert_eq!(lease::find_in_use("the account a was in use by an unattended run"), None);
        assert_eq!(lease::find_in_use("nothing here"), None);
        // A text that has a bad start before a good one finds the good one.
        let late = format!("the account was fine, the account x y z. {}", sentence("c", "the Auto Run browser"));
        assert_eq!(lease::find_in_use(&late).unwrap().0, "c");
    }

    #[test]
    fn a_browser_of_any_slot_counts_as_a_browser_held() {
        let _l = crate::serial::account_leases();
        let env = "env-ab000006";
        for holder in [Holder::Browser { slot: 0 }, Holder::Browser { slot: 1 }, Holder::Browser { slot: 2 }, Holder::Setup] {
            let held = lease::try_acquire(env, KEY, holder).unwrap();
            assert!(lease::a_browser_is_held());
            drop(held);
        }
    }

    /// A browser opened in a slot holds its accounts as that slot, and the
    /// person's own paths (`supervised`) as main.
    #[tokio::test]
    async fn a_browser_holds_its_account_as_its_own_slot() {
        let _l = crate::serial::account_leases();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let env = env_of(root);
        let mut extra = Held::supervised_in(SlotNo(2));
        extra.hold(root, "admin").await.unwrap();
        assert_eq!(lease::holder_of(&env, "admin"), Some(Holder::Browser { slot: 2 }));
        let mut main = Held::supervised();
        main.hold(root, "manager").await.unwrap();
        assert_eq!(lease::holder_of(&env, "manager"), Some(Holder::Browser { slot: 0 }));
        // The main browser asking for the extra one's account is refused at once.
        let refused = Held::supervised().hold(root, "admin").await.unwrap_err();
        assert_eq!(refused, lease::in_use_by_browser("admin"));
        drop((extra, main));
        assert!(!lease::is_held(&env, "admin") && !lease::is_held(&env, "manager"));
    }

    #[test]
    fn in_use_by_words_names_each_holder_as_the_asker_reads_it() {
        let _a = crate::serial::agents();
        fresh();
        let a = register(ID_A, 1_000);
        let b = register(ID_B, 2_000);
        let browser = |slot| Holder::Browser { slot };
        // The asker's own browser.
        assert_eq!(in_use_by_words(&browser(1), Some(SlotNo(1)), false), "your browser");
        assert_eq!(in_use_by_words(&browser(0), Some(SlotNo::MAIN), true), "your browser");
        // Another assistant's, by label.
        slots::bind(SlotNo(1), &b);
        assert_eq!(in_use_by_words(&browser(1), Some(SlotNo::MAIN), false), "Assistant 2's browser");
        assert_eq!(in_use_by_words(&browser(1), None, false), "Assistant 2's browser");
        slots::bind(SlotNo::MAIN, &a);
        assert_eq!(in_use_by_words(&browser(0), Some(SlotNo(1)), true), "Assistant 1's browser");
        // Main, bound to nobody: the person's when they opened it.
        slots::unbind(SlotNo::MAIN);
        assert_eq!(in_use_by_words(&browser(0), Some(SlotNo(1)), true), "the person's browser");
        assert_eq!(in_use_by_words(&browser(0), Some(SlotNo(1)), false), "the main Auto Run browser");
        // A gone assistant is not named.
        agents::note_bye(ID_B, 3_000);
        assert_eq!(in_use_by_words(&browser(1), Some(SlotNo::MAIN), true), "another Auto Run browser");
        // The other holders.
        assert_eq!(in_use_by_words(&case_of("run-1"), Some(SlotNo::MAIN), false), "an unattended run");
        assert_eq!(in_use_by_words(&Holder::Template, None, false), "an API template run");
        assert_eq!(in_use_by_words(&Holder::Setup, None, false), "Auto Run setup");
        fresh();
    }

    /// The browser a slot opens is made with that slot's holder, and the
    /// refusals are named by the bridge.
    #[test]
    fn open_into_holds_as_its_slot_and_the_bridge_names_the_holder() {
        let commands = include_str!("../../src/commands/autorun.rs");
        assert!(commands.contains("lease: crate::autorun::lease::Held::supervised_in(n),"));
        assert!(!commands.contains("lease: crate::autorun::lease::Held::supervised(),"));
        let bridge = include_str!("../../src/ai_bridge.rs");
        assert!(bridge.contains("with_holder_named("));
        // One wrapper around every arm of `route_as`, after the match, not an arm at a time.
        let route_as = &bridge[bridge.find("pub async fn route_as(").unwrap()..];
        let route_as = &route_as[..route_as.find("\n}").unwrap()];
        assert!(route_as.contains("let answer = match (method, path) {"));
        assert!(route_as.trim_end().ends_with("name_the_holder(answer, caller, method, path)"));
        assert_eq!(route_as.matches("name_the_holder(").count(), 1);
        assert!(bridge.contains("accounts_read(ctx, crate::commands::autorun_slots::slot_if_any(caller))"));
    }
}
