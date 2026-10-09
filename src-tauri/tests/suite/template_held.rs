//! The held signed-in browser store (`api_templates::held`): one per
//! (environment, account), handed back only to a run with the same sign-in
//! and only while nothing else has taken the account since; and the lease
//! generation (`autorun::lease::generation`) that tells it so.
//!
//! The store and the lease table are process-wide, so every test here takes
//! `serial::held_browsers` and then `serial::account_leases`, in that order.

use crate::common;
use std::time::{Duration, Instant};
use v2_lib::api_templates::held::{self, HeldEntry, Taken, HELD_IDLE};
use v2_lib::api_templates::runner::Session;
use v2_lib::autorun::lease::{self, Holder};

/// Stands in for a browser: the store never drives one, only keeps it.
#[derive(Debug, PartialEq)]
struct Tag(u32);

const ENV: &str = "env-held";

fn session() -> Session {
    Session::new(common::recipe(), common::account(), "https://hr.example.internal".into(), Vec::new())
}

fn fingerprint() -> u64 {
    held::fingerprint(&common::recipe(), &common::account())
}

fn entry(tag: u32, env: &str, key: &str) -> HeldEntry<Tag> {
    HeldEntry {
        driver: Tag(tag),
        session: session(),
        fingerprint: fingerprint(),
        generation: lease::generation(env, key),
        page: Some("/Home/Index".into()),
    }
}

fn reused(t: Taken<Tag>) -> Option<HeldEntry<Tag>> {
    match t {
        Taken::Reuse(e) => Some(e),
        _ => None,
    }
}

#[test]
fn take_after_put_returns_the_entry_once() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.once";
    assert!(held::put(ENV, key, entry(1, ENV, key)).is_none());

    let got = reused(held::take::<Tag>(ENV, key, fingerprint())).expect("the held browser was not handed back");
    assert_eq!(got.driver, Tag(1));
    assert_eq!(got.page.as_deref(), Some("/Home/Index"));
    assert!(matches!(held::take::<Tag>(ENV, key, fingerprint()), Taken::Nothing), "handed back twice");
}

#[test]
fn a_case_taking_the_account_makes_the_held_browser_give_way() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.given-way";
    held::put(ENV, key, entry(2, ENV, key));
    let before = lease::generation(ENV, key);
    drop(lease::try_acquire(ENV, key, Holder::Case { run: "run-1".into() }).unwrap());
    assert_eq!(lease::generation(ENV, key), before + 1);

    match held::take::<Tag>(ENV, key, fingerprint()) {
        Taken::Close(e) => assert_eq!(e.driver, Tag(2)),
        _ => panic!("a browser that gave way was not handed back for closing"),
    }
    assert!(matches!(held::take::<Tag>(ENV, key, fingerprint()), Taken::Nothing), "a stale entry stayed");
}

#[test]
fn the_supervised_browser_and_setup_also_bump_the_generation() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.others";
    assert_eq!(lease::generation(ENV, key), 0, "a key never taken starts at 0");
    drop(lease::try_acquire(ENV, key, Holder::Browser).unwrap());
    drop(lease::try_acquire(ENV, key, Holder::Setup).unwrap());
    assert_eq!(lease::generation(ENV, key), 2);
    assert_eq!(lease::generation("another-env", key), 0, "a generation is per environment");
}

#[test]
fn a_template_lease_does_not_bump_the_generation() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.template";
    held::put(ENV, key, entry(3, ENV, key));
    let before = lease::generation(ENV, key);
    drop(lease::try_acquire(ENV, key, Holder::Template).unwrap());
    assert_eq!(lease::generation(ENV, key), before);

    let got = reused(held::take::<Tag>(ENV, key, fingerprint())).expect("a template lease made it give way");
    assert_eq!(got.driver, Tag(3));
}

#[test]
fn a_refused_take_does_not_bump_the_generation() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.refused";
    let _template = lease::try_acquire(ENV, key, Holder::Template).unwrap();
    let before = lease::generation(ENV, key);
    assert!(lease::try_acquire(ENV, key, Holder::Browser).is_err());
    assert_eq!(lease::generation(ENV, key), before, "a lease that was never taken took the account");
}

#[test]
fn a_changed_recipe_is_not_reused() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.changed";
    held::put(ENV, key, entry(4, ENV, key));

    let mut recipe = common::recipe();
    recipe.start_url = "https://hr.example.internal/other".into();
    let changed = held::fingerprint(&recipe, &common::account());
    assert_ne!(changed, fingerprint());
    match held::take::<Tag>(ENV, key, changed) {
        Taken::Close(e) => assert_eq!(e.driver, Tag(4)),
        _ => panic!("a browser signed in with another recipe was not handed back for closing"),
    }
    assert!(matches!(held::take::<Tag>(ENV, key, fingerprint()), Taken::Nothing));
}

#[test]
fn a_changed_login_changes_the_fingerprint() {
    let mut account = common::account();
    account.username = "someone-else".into();
    assert_ne!(held::fingerprint(&common::recipe(), &account), fingerprint());
    let mut account = common::account();
    account.key = "another.key".into();
    assert_ne!(held::fingerprint(&common::recipe(), &account), fingerprint());
    assert_eq!(fingerprint(), fingerprint(), "the same sign-in gave two fingerprints");
}

#[test]
fn expired_drains_only_entries_past_their_deadline() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let now = Instant::now();
    let (old, young) = ("held.old", "held.young");
    held::put_at(ENV, old, entry(5, ENV, old), now);
    held::put_at(ENV, young, entry(6, ENV, young), now + Duration::from_secs(60));

    assert!(held::expired_at::<Tag>(now + HELD_IDLE - Duration::from_secs(1)).is_empty(), "drained too early");
    let gone = held::expired_at::<Tag>(now + HELD_IDLE + Duration::from_secs(1));
    assert_eq!(gone.into_iter().map(|e| e.driver).collect::<Vec<_>>(), vec![Tag(5)]);

    let got = reused(held::take::<Tag>(ENV, young, fingerprint())).expect("the younger entry was drained");
    assert_eq!(got.driver, Tag(6));
}

#[test]
fn put_replacing_an_entry_hands_the_old_one_back() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.replace";
    assert!(held::put(ENV, key, entry(7, ENV, key)).is_none());
    let old = held::put(ENV, key, entry(8, ENV, key)).expect("the replaced entry was not handed back");
    assert_eq!(old.driver, Tag(7));
    let got = reused(held::take::<Tag>(ENV, key, fingerprint())).unwrap();
    assert_eq!(got.driver, Tag(8));
}

#[test]
fn drain_all_hands_back_every_entry() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    held::drain_all::<Tag>();
    held::put(ENV, "held.all-1", entry(9, ENV, "held.all-1"));
    held::put("env-other", "held.all-2", entry(10, "env-other", "held.all-2"));
    let mut tags: Vec<u32> = held::drain_all::<Tag>().into_iter().map(|e| e.driver.0).collect();
    tags.sort();
    assert_eq!(tags, vec![9, 10]);
    assert!(held::drain_all::<Tag>().is_empty());
}

#[test]
fn the_sweep_closes_a_browser_that_gave_way_before_its_idle_time_runs_out() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let now = Instant::now();
    let (moved, still) = ("held.swept", "held.kept");
    held::put_at(ENV, moved, entry(11, ENV, moved), now);
    held::put_at(ENV, still, entry(12, ENV, still), now);
    drop(lease::try_acquire(ENV, moved, Holder::Browser).unwrap());

    let gone = held::expired_at::<Tag>(now);
    assert_eq!(gone.into_iter().map(|e| e.driver).collect::<Vec<_>>(), vec![Tag(11)]);
    let got = reused(held::take::<Tag>(ENV, still, fingerprint())).expect("the sweep took an entry that had not given way");
    assert_eq!(got.driver, Tag(12));
}

// --- Single runs that keep their browser (`runner::run_template_within`) ---
//
// These drive the runner with the API template runner's fake page
// (`api_templates_runner::App`) and a `Browsers` that keeps it. Each rig
// has its own data root, so its own environment id: entries never meet
// another test's. Locks, where taken: `autorun`, `api_template_run`,
// `activity_log`, then `held_browsers` and `account_leases`.

use crate::api_templates_runner::{
    answer, empty_400, prove, request, rig, template, App, Rig, LOGIN, PAGE, QUICK_PAUSES, TOKEN,
};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use v2_lib::api_templates::held::{HeldBrowser, Keeps};
use v2_lib::api_templates::runner::{run_template_within, RunReport, REUSED, RUN_LIMIT};
use v2_lib::api_templates::ApiTemplate;
use v2_lib::autorun::replay::Browsers;

/// A kept fake page; closing it only counts.
struct KeptApp {
    app: App,
    closed: Arc<AtomicUsize>,
}

impl HeldBrowser for KeptApp {
    fn close(self) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

/// Hands out prepared pages and keeps whatever a run leaves alive. `closed`
/// counts its own closes; `kept_closed` the closes of pages it kept, made
/// later by the store's helpers.
struct Keeping {
    next: VecDeque<App>,
    opened: usize,
    closed: usize,
    kept_closed: Arc<AtomicUsize>,
}

impl Keeping {
    fn new(pages: Vec<App>, kept_closed: &Arc<AtomicUsize>) -> Self {
        Keeping { next: pages.into(), opened: 0, closed: 0, kept_closed: kept_closed.clone() }
    }
}

impl Browsers for Keeping {
    type D = App;

    async fn open(&mut self) -> Result<App, String> {
        self.opened += 1;
        self.next.pop_front().ok_or_else(|| "no browser left".to_string())
    }

    async fn close(&mut self, _d: App) {
        self.closed += 1;
    }
}

impl Keeps for Keeping {
    type Kept = KeptApp;

    fn keep(&mut self, d: App) -> Result<KeptApp, App> {
        Ok(KeptApp { app: d, closed: self.kept_closed.clone() })
    }

    fn adopt(&mut self, kept: KeptApp) -> App {
        kept.app
    }
}

/// Empties the store of fake pages when the test ends, however it ends.
struct Clean;

impl Drop for Clean {
    fn drop(&mut self) {
        held::drain_all::<KeptApp>();
    }
}

fn first_page(r: &mut Rig) -> App {
    r.browsers.next.take().expect("the rig's page")
}

/// A second browser on the same application, with its own sign-in count.
fn fresh_page(r: &Rig) -> (App, Arc<AtomicUsize>) {
    let (inner, state) = crate::common::stateful_app(false, None);
    (App { inner, script: r.script.clone() }, state.clicks)
}

async fn run_in(r: &Rig, b: &mut Keeping, t: ApiTemplate, limit: Duration) -> RunReport {
    run_template_within(b, r.root.path(), &request(t, prove()), &crate::common::quick(), limit, &QUICK_PAUSES).await
}

async fn run(r: &Rig, b: &mut Keeping, t: ApiTemplate) -> RunReport {
    run_in(r, b, t, RUN_LIMIT).await
}

fn ok_answers() -> Vec<serde_json::Value> {
    vec![answer(200, json!({ "success": true, "cycleId": 274 })), answer(200, json!({ "success": true }))]
}

fn twice<T: Clone>(v: Vec<T>) -> Vec<T> {
    v.iter().chain(v.iter()).cloned().collect()
}

/// Every fake page held now, taken out of the store.
fn held_pages() -> Vec<App> {
    held::drain_all::<KeptApp>().into_iter().map(|e| e.driver.app).collect()
}

fn navigations_to(app: &App, page: &str) -> usize {
    app.inner.calls_to("Page.navigate").iter().filter(|p| p["url"].as_str().unwrap_or("").ends_with(page)).count()
}

#[tokio::test]
async fn a_second_run_within_two_minutes_opens_no_browser_and_signs_in_once() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(twice(ok_answers()), None);
    let kept_closed = Arc::new(AtomicUsize::new(0));

    let mut first = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    let report = run(&r, &mut first, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!((first.opened, first.closed), (1, 0), "the browser is kept, not closed");

    let mut second = Keeping::new(vec![], &kept_closed);
    let report = run(&r, &mut second, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(second.opened, 0, "the kept browser was reused");
    assert_eq!(r.sign_ins(), 1, "the second run signed in again");
    assert_eq!(r.fetched().len(), 4);
    assert_eq!(kept_closed.load(Ordering::SeqCst), 0);
    assert_eq!(held_pages().len(), 1, "kept again for the next run");
}

#[tokio::test]
async fn the_token_page_is_not_reloaded_when_already_on_it() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(twice(ok_answers()), None);
    let kept_closed = Arc::new(AtomicUsize::new(0));

    let mut first = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    assert!(run(&r, &mut first, template()).await.ok);
    let report = run(&r, &mut Keeping::new(vec![], &kept_closed), template()).await;
    assert!(report.ok, "{report:?}");

    let pages = held_pages();
    assert_eq!(navigations_to(&pages[0], PAGE), 1, "the second run loaded the token page it was already on");
    // The token was still read from the page, and sent.
    assert_eq!(r.fetched()[2][1], json!(TOKEN), "the second run sent no token");
}

#[tokio::test]
async fn a_different_token_page_is_loaded() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(twice(ok_answers()), None);
    let kept_closed = Arc::new(AtomicUsize::new(0));

    let mut first = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    assert!(run(&r, &mut first, template()).await.ok);
    let mut other = template();
    other.antiforgery.page = "/hr/pmsv10/cyclelist".into();
    let report = run(&r, &mut Keeping::new(vec![], &kept_closed), other).await;
    assert!(report.ok, "{report:?}");

    let pages = held_pages();
    assert_eq!(navigations_to(&pages[0], PAGE), 1);
    assert_eq!(navigations_to(&pages[0], "/hr/pmsv10/cyclelist"), 1, "the other token page was not loaded");
    assert_eq!(r.sign_ins(), 1);
}

#[tokio::test]
async fn a_dropped_session_on_a_reused_browser_signs_in_again_once() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(ok_answers(), None);
    let kept_closed = Arc::new(AtomicUsize::new(0));
    let mut first = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    assert!(run(&r, &mut first, template()).await.ok);

    // An empty 400 to the first request.
    r.script.lock().unwrap().responses.extend([empty_400()].into_iter().chain(ok_answers()));
    let (page, clicks) = fresh_page(&r);
    let mut second = Keeping::new(vec![page], &kept_closed);
    let report = run(&r, &mut second, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!((second.opened, second.closed), (1, 1), "the reused browser is closed and one fresh one opens");
    assert_eq!(clicks.load(Ordering::SeqCst), 1, "the fresh browser signs in once");
    assert_eq!(r.fetched().len(), 2 + 3, "no retry of the refused request in the kept browser");
    assert_eq!(report.steps.len(), 2, "the template ran from its first step: {report:?}");

    // The token page sent to the sign-in page.
    r.script.lock().unwrap().responses.extend(ok_answers());
    r.script.lock().unwrap().hrefs.push_back(LOGIN.to_string());
    let (page, clicks) = fresh_page(&r);
    let mut third = Keeping::new(vec![page], &kept_closed);
    let report = run(&r, &mut third, template()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!((third.opened, third.closed), (1, 1));
    assert_eq!(clicks.load(Ordering::SeqCst), 1);
    assert_eq!(kept_closed.load(Ordering::SeqCst), 0, "a kept browser is closed by the run, through its Browsers");
    assert_eq!(held_pages().len(), 1);
}

#[tokio::test]
async fn an_empty_400_on_a_fresh_browser_fails_as_before() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(vec![empty_400(); 4], None);
    let kept_closed = Arc::new(AtomicUsize::new(0));
    let mut b = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    let report = run(&r, &mut b, template()).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("Cycle setup"));
    let detail = &report.steps.last().unwrap().detail;
    assert!(detail.contains("refused the same way on all 4 tries"), "{detail}");
    assert_eq!(b.opened, 1, "a fresh browser is not opened again");
    assert_eq!(r.sign_ins(), 1);
}

#[tokio::test]
async fn a_400_with_a_body_on_a_reused_browser_is_the_templates_own_answer() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(ok_answers(), None);
    let kept_closed = Arc::new(AtomicUsize::new(0));
    let mut first = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    assert!(run(&r, &mut first, template()).await.ok);

    r.script.lock().unwrap().responses.push_back(answer(400, json!({ "errors": { "CycleName": ["taken"] } })));
    let mut second = Keeping::new(vec![], &kept_closed);
    let report = run(&r, &mut second, template()).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("Cycle setup"));
    assert_eq!(report.steps.last().unwrap().status, Some(400));
    assert_eq!(second.opened, 0, "a 400 with a body did not start the run again");
    assert_eq!(r.fetched().len(), 3, "nor was it sent again");
    assert_eq!(r.sign_ins(), 1);
}

#[tokio::test]
async fn a_run_that_timed_out_keeps_nothing() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().hang_fetch = true;
    let kept_closed = Arc::new(AtomicUsize::new(0));
    let mut b = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    let report = run_in(&r, &mut b, template(), Duration::from_millis(800)).await;
    assert!(!report.ok);
    assert!(report.message().contains("longer than 3 minutes"), "{}", report.message());
    assert_eq!(b.closed, 1, "the browser of a run that timed out is closed");
    assert!(held_pages().is_empty(), "and never kept");
}

#[tokio::test]
async fn a_failed_sign_in_keeps_nothing() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(vec![], Some("#go"));
    let kept_closed = Arc::new(AtomicUsize::new(0));
    let mut b = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    let report = run(&r, &mut b, template()).await;
    assert_eq!(report.failed.as_deref(), Some("Sign in"), "{report:?}");
    assert_eq!(b.closed, 1);
    assert!(held_pages().is_empty());
}

#[tokio::test]
async fn the_idle_close_never_closes_a_browser_in_use() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(ok_answers(), None);
    let kept_closed = Arc::new(AtomicUsize::new(0));
    let mut first = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    assert!(run(&r, &mut first, template()).await.ok);

    // The second run reuses it and is stuck on its first request; the
    // sweep runs then, as if the idle time had long run out.
    r.script.lock().unwrap().hang_fetch = true;
    let mut second = Keeping::new(vec![], &kept_closed);
    let sweep = async {
        let sent = async {
            while r.fetched().len() < 3 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        let in_use = tokio::time::timeout(Duration::from_secs(5), sent).await.is_ok();
        held::sweep_at::<KeptApp>(Instant::now() + HELD_IDLE * 10);
        (in_use, kept_closed.load(Ordering::SeqCst))
    };
    let (report, (in_use, closed_by_sweep)) =
        tokio::join!(run_in(&r, &mut second, template(), Duration::from_millis(800)), sweep);
    assert!(in_use, "the second run never sent its request in the kept browser: {report:?}");
    assert_eq!(closed_by_sweep, 0, "the sweep closed the browser a run was using");
    assert!(!report.ok);
    assert_eq!((second.opened, second.closed), (0, 1), "the run closed it itself when it timed out");

    // Kept and idle, it is the sweep's.
    r.script.lock().unwrap().hang_fetch = false;
    r.script.lock().unwrap().responses.extend(ok_answers());
    let (page, _) = fresh_page(&r);
    assert!(run(&r, &mut Keeping::new(vec![page], &kept_closed), template()).await.ok);
    held::sweep_at::<KeptApp>(Instant::now());
    assert_eq!(kept_closed.load(Ordering::SeqCst), 0, "not yet idle for two minutes");
    held::sweep_at::<KeptApp>(Instant::now() + HELD_IDLE);
    assert_eq!(kept_closed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn quitting_changing_environment_or_signing_out_closes_the_held_browser() {
    let _claims = crate::serial::autorun();
    let _slot = crate::serial::api_template_run();
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let mut r = rig(twice(twice(ok_answers())), None);
    let kept_closed = Arc::new(AtomicUsize::new(0));
    let mut b = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    let closed = || kept_closed.load(Ordering::SeqCst);

    // Quitting (and an update restart): Auto Run's exit close.
    assert!(run(&r, &mut b, template()).await.ok);
    tokio::time::timeout(Duration::from_secs(5), v2_lib::commands::autorun::close_autorun_browsers())
        .await
        .expect("closing on exit is bounded");
    assert_eq!(closed(), 1, "quitting left the held browser open");

    // Signing out: what `commands::auth::sign_out` calls.
    let (page, _) = fresh_page(&r);
    assert!(run(&r, &mut Keeping::new(vec![page], &kept_closed), template()).await.ok);
    held::close_all();
    assert_eq!(closed(), 2, "signing out left the held browser open");

    // Changing the active environment.
    let (page, _) = fresh_page(&r);
    assert!(run(&r, &mut Keeping::new(vec![page], &kept_closed), template()).await.ok);
    let root = r.root.path();
    let current = v2_lib::environments::active(root).unwrap();
    let mut qa = current.clone();
    (qa.id, qa.name) = (String::new(), "QA".into());
    let saved = v2_lib::environments::save_env(root, qa, &[current.db_id.clone()]).unwrap();
    let qa = saved.environments.iter().find(|e| e.name == "QA").unwrap().id.clone();
    let store = v2_lib::db::credentials::MemoryStore::default();
    v2_lib::commands::environments::set_active_with(root, &store, &qa).await.unwrap();
    assert_eq!(closed(), 3, "changing environment left the held browser open");
    assert!(held_pages().is_empty());
}

#[tokio::test]
async fn a_reused_run_reports_its_sign_in_as_reused() {
    let _act = crate::serial::activity_log();
    let (_h, _l, _c) = (crate::serial::held_browsers(), crate::serial::account_leases(), Clean);
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());
    let mut r = rig(twice(ok_answers()), None);
    let kept_closed = Arc::new(AtomicUsize::new(0));
    let mut first = Keeping::new(vec![first_page(&mut r)], &kept_closed);
    assert!(run(&r, &mut first, template()).await.ok);
    assert!(run(&r, &mut Keeping::new(vec![], &kept_closed), template()).await.ok);

    let pages: Vec<_> = crate::common::activity_records(activity.path(), "api")
        .into_iter()
        .filter(|r| r["event"] == "token_page")
        .collect();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0]["sign_ins"][0]["via"], "sign-in recipe");
    assert_eq!(pages[1]["sign_ins"], json!([{ "via": REUSED, "appeared": [] }]));
    assert_eq!(REUSED, "Signed in earlier, reused");
}
