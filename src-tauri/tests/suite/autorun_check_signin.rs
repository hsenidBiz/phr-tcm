//! Check sign-in (backlog 40 part 2 and 53): each account signed in once, in
//! a browser of its own, and walked as far as a chosen area. The browser is
//! a fake (`common::menu_app`); nothing here opens a real one.

use crate::common::{self, account, menu_recipe, ScriptedDriver, PASSWORD};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use v2_lib::autorun::accounts::{save_accounts, Account};
use v2_lib::autorun::lease::{self, Holder};
use v2_lib::autorun::nav::{save_nav, ModulePath, NavFile};
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::replay::Browsers;
use v2_lib::autorun::setup_find::RowSource;
use v2_lib::autorun::sign_in_check::{
    cancel, check_accounts, check_rank, expiring, going_sentence, is_stale, load_marks, mark_phrase, precheck,
    prefer_reaching, proposal_rank, summary, AreaReach, Mark, Precheck, SignInCheck, CANCELLED, EXPIRY_NOT_CHECKED,
    NOT_TRIED, NO_OTHER_SIGN_IN_CHECK,
};
use v2_lib::browser::timing::Timing;

const ORG: &str = "acme";
const PROJECT: &str = "Web";

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 150, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn area(module: &str, name: &str, arrived: &str) -> ModulePath {
    serde_json::from_value(json!({
        "module": module,
        "area": name,
        "clicks": [ { "role": "link", "name": name, "exact": true } ],
        "arrived": arrived,
        "recorded": "2026-10-05T10:00:00Z"
    }))
    .unwrap()
}

fn project(root: &Path) {
    let areas = vec![area("Leave", "Leave", "/hr/leave"), area("Settings", "Definition Wizard", "/hr/wizard")];
    save_nav(root, ORG, PROJECT, &NavFile { direct_urls: false, modules: areas, save_words: vec![], protected: vec![] })
        .unwrap();
    save_recipe(root, ORG, PROJECT, &menu_recipe()).unwrap();
}

fn accounts(root: &Path, keys: &[&str]) {
    let list: Vec<Account> = keys.iter().map(|k| Account { key: k.to_string(), ..account() }).collect();
    save_accounts(root, &list).unwrap();
}

fn menu(entries: &[(&'static str, &'static str, &'static str)]) -> ScriptedDriver {
    common::menu_app(entries, "/hr/home/index", 0).0
}

/// Hands out a fresh fake browser per `open` and counts how many are open
/// at once.
struct Fake {
    make: Box<dyn FnMut() -> ScriptedDriver>,
    opened: usize,
    closed: usize,
    most_at_once: usize,
    refuse: Option<String>,
}

impl Fake {
    fn new(make: impl FnMut() -> ScriptedDriver + 'static) -> Self {
        Fake { make: Box::new(make), opened: 0, closed: 0, most_at_once: 0, refuse: None }
    }
}

impl Browsers for Fake {
    type D = ScriptedDriver;
    async fn open(&mut self) -> Result<ScriptedDriver, String> {
        if let Some(why) = &self.refuse {
            return Err(why.clone());
        }
        self.opened += 1;
        self.most_at_once = self.most_at_once.max(self.opened - self.closed);
        Ok((self.make)())
    }
    async fn close(&mut self, _d: ScriptedDriver) {
        self.closed += 1;
    }
}

fn keys(list: &[&str]) -> Vec<String> {
    list.iter().map(|k| k.to_string()).collect()
}

fn full_menu() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![("link", "Leave", "/hr/leave"), ("link", "Definition Wizard", "/hr/wizard")]
}

fn plain(account: &str) -> SignInCheck {
    SignInCheck {
        account: account.into(),
        signed_in: true,
        landed: Some("/hr/home/index".into()),
        stopped: None,
        password_due: None,
        note: None,
        area: None,
    }
}

async fn run(
    fake: &mut Fake,
    root: &Path,
    who: &[&str],
    area: Option<&str>,
    pre: &BTreeMap<String, Precheck>,
) -> Vec<SignInCheck> {
    check_accounts(fake, root, ORG, PROJECT, &keys(who), area, pre, &quick()).await
}

// ---- The words ---------------------------------------------------------------

#[test]
fn summary_reaches_area() {
    let c = SignInCheck { area: Some(AreaReach::Reached { area: "Definition Wizard".into() }), ..plain("a") };
    assert_eq!(summary(&c), "signed in, landed on /hr/home/index; reaches Definition Wizard");
}

#[test]
fn summary_stops_on_the_way_with_walk_sentence() {
    let c = SignInCheck {
        area: Some(AreaReach::Stopped {
            area: "Definition Wizard".into(),
            at: "click 2, link \"Definition Wizard\": not found - stopped at /hr/home (page title: Self Service)".into(),
        }),
        ..plain("a")
    };
    assert_eq!(
        summary(&c),
        "signed in, landed on /hr/home/index; stopped on the way to Definition Wizard: click 2, link \"Definition Wizard\": not found - stopped at /hr/home (page title: Self Service)"
    );
}

#[test]
fn summary_password_reset_page() {
    let c = SignInCheck {
        account: "a".into(),
        signed_in: false,
        landed: None,
        stopped: Some(format!(
            "{} - the browser ended on /security/PasswordReset (page title: Reset password)",
            v2_lib::autorun::signin::PASSWORD_EXPIRED
        )),
        password_due: None,
        note: None,
        area: None,
    };
    assert_eq!(summary(&c), "stopped on the password reset page");
}

#[test]
fn summary_names_a_failed_sign_in_and_one_not_tried() {
    let failed = SignInCheck {
        signed_in: false,
        landed: None,
        stopped: Some("the signed-in marker did not show".into()),
        ..plain("a")
    };
    assert_eq!(summary(&failed), "sign-in failed: the signed-in marker did not show");
    let not_tried = SignInCheck { stopped: Some(format!("{NOT_TRIED}password expired 2026-10-29")), ..failed };
    assert_eq!(summary(&not_tried), format!("{NOT_TRIED}password expired 2026-10-29"));
}

#[test]
fn summary_names_path_only() {
    let c = SignInCheck {
        landed: Some(v2_lib::browser::save_guard::path_of("https://hr.example.internal/hr/home/index?token=abc")),
        area: Some(AreaReach::Reached { area: "Leave".into() }),
        ..plain("a")
    };
    let text = summary(&c);
    assert!(!text.contains("example.internal") && !text.contains("token"), "{text}");
    assert!(text.contains("/hr/home/index"), "{text}");
}

#[test]
fn summary_adds_the_password_words_when_known() {
    let c = SignInCheck { password_due: Some("password due 2026-11-02, 3 days".into()), ..plain("a") };
    assert_eq!(summary(&c), "signed in, landed on /hr/home/index (password due 2026-11-02, 3 days)");
    let noted = SignInCheck { note: Some(NO_OTHER_SIGN_IN_CHECK.into()), ..c };
    assert_eq!(
        summary(&noted),
        "signed in, landed on /hr/home/index (password due 2026-11-02, 3 days) - not checked for other sign-ins"
    );
}

// ---- Before a browser opens --------------------------------------------------

struct Answers {
    expiry: Result<String, String>,
    active: Result<String, String>,
}

impl RowSource for Answers {
    async fn select(&self, sql: &str) -> Result<String, String> {
        if sql.contains("reset_due") { self.expiry.clone() } else { self.active.clone() }
    }
}

const TODAY: &str = "2026-10-30";
const EXPIRY_Q: &str = "SELECT u AS user_id, d AS reset_due, c AS prompt_change FROM t WHERE u IN ({users})";
const ACTIVE_Q: &str = "SELECT s AS since, l AS logged_to FROM a WHERE u = {user}";

fn expiry(rows: &[&str]) -> Result<String, String> {
    Ok(format!("user_id\treset_due\tprompt_change\n-\t-\t-\n{}\n\n({} rows affected)\n", rows.join("\n"), rows.len()))
}

fn active(rows: &[&str]) -> Result<String, String> {
    Ok(format!("since\tlogged_to\n-\t-\n{}\n\n({} rows affected)\n", rows.join("\n"), rows.len()))
}

#[tokio::test]
async fn precheck_stops_an_expired_password_and_a_session_elsewhere() {
    let db = Answers { expiry: expiry(&["kim\t2026-10-01\t0"]), active: active(&[]) };
    let pre = precheck(Some(&db), EXPIRY_Q, ACTIVE_Q, "admin", "kim", false, TODAY).await;
    assert!(pre.expired.as_deref().is_some_and(|w| w.contains("2026-10-01")), "{pre:?}");

    let db = Answers { expiry: expiry(&["kim\t2027-02-02\t0"]), active: active(&["2026-10-30 09:40:12.000\tehrm"]) };
    let pre = precheck(Some(&db), EXPIRY_Q, ACTIVE_Q, "admin", "kim", false, TODAY).await;
    assert!(pre.elsewhere.as_deref().is_some_and(|w| w.contains("signed in elsewhere")), "{pre:?}");
    // The app's own session is not someone else's.
    let pre = precheck(Some(&db), EXPIRY_Q, ACTIVE_Q, "admin", "kim", true, TODAY).await;
    assert!(pre.elsewhere.is_none() && pre.blind.is_none(), "{pre:?}");
    assert_eq!(pre.password_due.as_deref(), Some("password due 2027-02-02, 95 days"));
}

#[tokio::test]
async fn an_active_session_query_that_cannot_be_asked_stops_the_account() {
    let none: Option<&Answers> = None;
    // Database reading off or not set up.
    let pre = precheck(none, EXPIRY_Q, ACTIVE_Q, "admin", "kim", false, TODAY).await;
    let blind = pre.blind.expect("not signed in blind");
    assert!(
        blind.starts_with("could not check whether someone else is signed in as admin (")
            && blind.ends_with(") - fix the query or turn on database read access"),
        "{blind}"
    );
    // The database does not answer.
    let down = Answers { expiry: Err("Login timeout expired".into()), active: Err("Login timeout expired".into()) };
    let pre = precheck(Some(&down), EXPIRY_Q, ACTIVE_Q, "admin", "kim", false, TODAY).await;
    assert!(pre.blind.as_deref().is_some_and(|b| b.contains("did not answer")), "{pre:?}");
    assert!(pre.notes.contains(&EXPIRY_NOT_CHECKED.to_string()), "{pre:?}");
    // The query itself fails.
    let broken = Answers { expiry: expiry(&["kim\t2027-02-02\t0"]), active: Ok("a\tb\n-\t-\nx\ty\n\n(1 rows affected)\n".into()) };
    let pre = precheck(Some(&broken), EXPIRY_Q, ACTIVE_Q, "admin", "kim", false, TODAY).await;
    assert!(pre.blind.is_some(), "{pre:?}");
}

#[tokio::test]
async fn with_no_active_session_query_the_check_goes_on_and_says_it_did_not_look() {
    let none: Option<&Answers> = None;
    let pre = precheck(none, "", "", "admin", "kim", false, TODAY).await;
    assert!(pre.blind.is_none() && pre.expired.is_none() && pre.elsewhere.is_none());
    assert_eq!(pre.notes, [NO_OTHER_SIGN_IN_CHECK]);
    // An expiry query that fails goes on too, saying so.
    let db = Answers { expiry: Err("Login timeout expired".into()), active: active(&[]) };
    let pre = precheck(Some(&db), EXPIRY_Q, "", "admin", "kim", false, TODAY).await;
    assert!(pre.blind.is_none() && pre.notes.contains(&EXPIRY_NOT_CHECKED.to_string()), "{pre:?}");
}

#[tokio::test]
async fn the_run_window_lists_accounts_expired_or_due_within_a_week() {
    let db = Answers { expiry: expiry(&["kim\t2026-10-01\t0", "lee\t2026-11-02\t0", "pat\t2027-02-02\t0"]), active: active(&[]) };
    let acct = |key: &str, user: &str| Account { key: key.into(), username: user.into(), ..account() };
    let list = vec![acct("admin", "kim"), acct("sup", "lee"), acct("boss", "pat")];
    let lines = expiring(&db, EXPIRY_Q, &list, TODAY).await;
    assert_eq!(lines, ["admin: password expired 2026-10-01", "sup: password due 2026-11-02, 3 days"]);
    assert!(expiring(&db, "", &list, TODAY).await.is_empty());
    let down = Answers { expiry: Err("Login timeout expired".into()), active: active(&[]) };
    assert!(expiring(&down, EXPIRY_Q, &list, TODAY).await.is_empty());
}

// ---- The walk ----------------------------------------------------------------

#[tokio::test]
async fn check_walks_to_area_and_releases_the_browser_and_the_account() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    let mut fake = Fake::new(|| menu(&full_menu()));
    let out = run(&mut fake, dir.path(), &["admin"], Some("Definition Wizard"), &BTreeMap::new()).await;
    assert_eq!(out.len(), 1);
    let c = &out[0];
    assert!(c.signed_in, "{c:?}");
    assert_eq!(c.landed.as_deref(), Some("/hr/home/index"));
    assert_eq!(c.area, Some(AreaReach::Reached { area: "Definition Wizard".into() }));
    assert_eq!(summary(c), "signed in, landed on /hr/home/index; reaches Definition Wizard");
    // Its own browser, closed after; the account is free again.
    assert_eq!((fake.opened, fake.closed), (1, 1));
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    assert!(!lease::is_held(&env, "admin"), "the account was still held");
}

#[tokio::test]
async fn check_without_an_area_only_signs_in() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    let mut fake = Fake::new(|| menu(&full_menu()));
    let out = run(&mut fake, dir.path(), &["admin"], None, &BTreeMap::new()).await;
    assert!(out[0].signed_in && out[0].area.is_none(), "{:?}", out[0]);
    assert_eq!(summary(&out[0]), "signed in, landed on /hr/home/index");
}

#[tokio::test]
async fn check_says_where_it_stopped_when_the_menu_lacks_the_area() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    let mut fake = Fake::new(|| menu(&[("link", "Leave", "/hr/leave")]));
    let out = run(&mut fake, dir.path(), &["admin"], Some("Definition Wizard"), &BTreeMap::new()).await;
    let c = &out[0];
    assert!(c.signed_in);
    let Some(AreaReach::Stopped { area, at }) = &c.area else { panic!("{c:?}") };
    assert_eq!(area, "Definition Wizard");
    assert!(at.contains("click 1") && at.contains("stopped at /hr/home/index"), "{at}");
    assert_eq!((fake.opened, fake.closed), (1, 1));
}

#[tokio::test]
async fn check_names_an_unrecorded_area_without_signing_in() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    let mut fake = Fake::new(|| menu(&full_menu()));
    let out = run(&mut fake, dir.path(), &["admin"], Some("Payroll Setup"), &BTreeMap::new()).await;
    assert!(!out[0].signed_in);
    assert!(out[0].stopped.as_deref().is_some_and(|s| s.starts_with(NOT_TRIED) && s.contains("Payroll Setup")), "{:?}", out[0]);
    assert_eq!(fake.opened, 0, "a browser was opened for an area that is not recorded");
}

#[tokio::test]
async fn check_goes_one_account_at_a_time() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin", "sup", "boss"]);
    let mut fake = Fake::new(|| menu(&full_menu()));
    let out = run(&mut fake, dir.path(), &["admin", "sup", "boss"], Some("Leave"), &BTreeMap::new()).await;
    assert_eq!(out.iter().map(|c| c.account.as_str()).collect::<Vec<_>>(), ["admin", "sup", "boss"]);
    assert!(out.iter().all(|c| c.signed_in), "{out:?}");
    assert_eq!(fake.opened, 3);
    assert_eq!(fake.most_at_once, 1, "two sign-ins were open at once");
}

#[tokio::test]
async fn check_stops_early_for_an_expired_or_busy_account_without_a_browser() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin", "sup"]);
    let mut pre = BTreeMap::new();
    pre.insert("admin".to_string(), Precheck { expired: Some("password expired 2026-10-29".into()), ..Default::default() });
    pre.insert(
        "sup".to_string(),
        Precheck {
            elsewhere: Some("sup is signed in elsewhere since 2026-10-30 09:40 - signing in will log that session out".into()),
            ..Default::default()
        },
    );
    let mut fake = Fake::new(|| menu(&full_menu()));
    let out = run(&mut fake, dir.path(), &["admin", "sup"], Some("Leave"), &pre).await;
    assert_eq!(fake.opened, 0, "a sign-in was tried");
    assert!(summary(&out[0]).contains("password expired 2026-10-29"), "{}", summary(&out[0]));
    assert!(summary(&out[1]).contains("signed in elsewhere"), "{}", summary(&out[1]));
    assert!(out.iter().all(|c| !c.signed_in && c.stopped.as_deref().is_some_and(|s| s.starts_with(NOT_TRIED))));
}

#[tokio::test]
async fn check_types_the_password_once_and_hides_it() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let _t = crate::serial::log_tail();
    let dir = tempfile::tempdir().unwrap();
    save_nav(dir.path(), ORG, PROJECT, &NavFile { direct_urls: false, modules: vec![], save_words: vec![], protected: vec![] })
        .unwrap();
    save_recipe(dir.path(), ORG, PROJECT, &common::recipe()).unwrap();
    accounts(dir.path(), &["admin"]);
    // The go button is never found: the sign-in fails after the password.
    let typed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = typed.clone();
    let mut fake = Fake::new(move || {
        let (d, _state) = common::stateful_app(false, Some("#go"));
        let seen = seen.clone();
        d.answering_first(move |method, params| {
            if method == "Input.insertText" && params["text"] == PASSWORD {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            None
        })
    });
    let out = run(&mut fake, dir.path(), &["admin"], None, &BTreeMap::new()).await;
    assert!(!out[0].signed_in, "{:?}", out[0]);
    assert_eq!(typed.load(std::sync::atomic::Ordering::SeqCst), 1, "the password was typed more than once");
    let said = serde_json::to_string(&out).unwrap() + &summary(&out[0]);
    assert!(!said.contains(PASSWORD), "{said}");
    let log: String = v2_lib::applog::recent(500).iter().map(|l| format!("{l:?}")).collect();
    assert!(!log.contains(PASSWORD), "the password reached the log");
}

#[tokio::test]
async fn check_leaves_an_account_someone_else_holds_alone() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    let _held = lease::try_acquire(&env, "admin", Holder::Browser { slot: 0 }).unwrap();
    let mut fake = Fake::new(|| menu(&full_menu()));
    let out = run(&mut fake, dir.path(), &["admin"], Some("Leave"), &BTreeMap::new()).await;
    assert!(!out[0].signed_in);
    assert!(out[0].stopped.as_deref().is_some_and(|s| s.starts_with(NOT_TRIED)), "{:?}", out[0]);
    assert_eq!(fake.opened, 0, "a browser opened for an account that is held");
    // The other holder still has it.
    assert!(lease::is_held(&env, "admin"));
}

#[tokio::test]
async fn check_says_a_browser_that_would_not_open() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    let mut fake = Fake::new(|| menu(&full_menu()));
    fake.refuse = Some("it started but never answered".into());
    let out = run(&mut fake, dir.path(), &["admin"], None, &BTreeMap::new()).await;
    assert!(!out[0].signed_in);
    assert_eq!(summary(&out[0]), "sign-in failed: it started but never answered");
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    assert!(!lease::is_held(&env, "admin"));
}

#[tokio::test]
async fn check_an_account_that_does_not_exist_is_said_not_tried() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    let mut fake = Fake::new(|| menu(&full_menu()));
    let out = run(&mut fake, dir.path(), &["ghost"], None, &BTreeMap::new()).await;
    assert!(out[0].stopped.as_deref().is_some_and(|s| s.starts_with(NOT_TRIED) && s.contains("ghost")), "{:?}", out[0]);
    assert_eq!(fake.opened, 0);
}

// ---- The mark on the account -------------------------------------------------

#[tokio::test]
async fn check_marks_the_account_with_when_and_how_it_went() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin", "sup"]);
    let mut n = 0;
    let mut fake = Fake::new(move || {
        n += 1;
        // The second account's menu lacks the wizard.
        if n == 1 { menu(&full_menu()) } else { menu(&[("link", "Leave", "/hr/leave")]) }
    });
    run(&mut fake, dir.path(), &["admin", "sup"], Some("Definition Wizard"), &BTreeMap::new()).await;
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    let marks = load_marks(dir.path(), &env);
    assert!(marks["admin"].reached && marks["admin"].checked_ms > 0.0, "{:?}", marks["admin"]);
    assert_eq!(marks["admin"].area.as_deref(), Some("Definition Wizard"));
    assert!(!marks["sup"].reached, "{:?}", marks["sup"]);
    assert!(!serde_json::to_string(&marks).unwrap().contains(PASSWORD));
}

fn mark(area: Option<&str>, signed_in: bool, reached: bool, at: f64) -> Mark {
    Mark { checked_ms: at, signed_in, reached, area: area.map(str::to_string), outcome: "x".into() }
}

const NOW: u64 = 1_800_000_000_000;

#[test]
fn accounts_rank_by_the_area_asked() {
    let at = NOW as f64 - 1000.0;
    let mut marks = BTreeMap::new();
    marks.insert("reaches".to_string(), mark(Some("Leave"), true, true, at));
    marks.insert("stopped".to_string(), mark(Some("Leave"), true, false, at));
    marks.insert("other".to_string(), mark(Some("Definition Wizard"), true, true, at));
    marks.insert("signs".to_string(), mark(None, true, false, at));
    marks.insert("refused".to_string(), mark(None, false, false, at));
    let all = keys(&["never", "stopped", "other", "reaches"]);
    // For Leave: reached first, then no mark for Leave (never, other), then stopped.
    assert_eq!(prefer_reaching(&all, &marks, Some("leave"), NOW), keys(&["reaches", "never", "other", "stopped"]));
    // For the wizard: only "other" reached it.
    assert_eq!(check_rank(&marks, "other", Some("Definition Wizard"), NOW), 0);
    assert_eq!(check_rank(&marks, "reaches", Some("Definition Wizard"), NOW), 1);
    // A sign-in alone counts as signing in, not as reaching an area.
    assert_eq!(check_rank(&marks, "signs", Some("Leave"), NOW), 1);
    assert_eq!(check_rank(&marks, "signs", None, NOW), 0);
    assert_eq!(check_rank(&marks, "refused", None, NOW), 2);
    assert_eq!(prefer_reaching(&keys(&[]), &marks, None, NOW), keys(&[]));
}

#[test]
fn a_mark_over_a_week_old_is_stale_and_does_not_rank() {
    let week = 7.0 * 24.0 * 3600.0 * 1000.0;
    let old = mark(Some("Leave"), true, true, NOW as f64 - week - 1.0);
    let fresh = mark(Some("Leave"), true, true, NOW as f64 - week + 60_000.0);
    assert!(is_stale(&old, NOW) && !is_stale(&fresh, NOW));
    let mut marks = BTreeMap::new();
    marks.insert("a".to_string(), old.clone());
    assert_eq!(check_rank(&marks, "a", Some("Leave"), NOW), 1);
    assert!(mark_phrase(&old, NOW).ends_with("checked over 7 days ago"), "{}", mark_phrase(&old, NOW));
    assert_eq!(mark_phrase(&fresh, NOW), "reaches Leave");
}

#[test]
fn one_ranking_puts_the_logins_marks_before_the_checks() {
    let mut marks = BTreeMap::new();
    marks.insert("good".to_string(), mark(Some("Leave"), true, true, NOW as f64));
    let expired = vec!["expired".to_string()];
    let due = vec!["due soon".to_string()];
    let none: Vec<String> = vec![];
    assert!(proposal_rank(&none, &marks, "never", Some("Leave"), NOW) > proposal_rank(&none, &marks, "good", Some("Leave"), NOW));
    // A login that reached the area but is expired still comes after an unmarked one never checked.
    assert!(proposal_rank(&expired, &marks, "good", Some("Leave"), NOW) > proposal_rank(&none, &marks, "never", Some("Leave"), NOW));
    assert!(proposal_rank(&expired, &marks, "good", None, NOW) > proposal_rank(&due, &marks, "good", None, NOW));
}

#[tokio::test]
async fn only_outcomes_about_the_account_are_kept() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin", "sup", "boss", "held", "blind"]);
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    // The account is busy: nothing is learned about it.
    let _busy = lease::try_acquire(&env, "held", Holder::Browser { slot: 0 }).unwrap();
    // A blind sign-in is not tried.
    let mut pre = BTreeMap::new();
    pre.insert("blind".to_string(), Precheck { blind: Some("could not check".into()), ..Default::default() });
    pre.insert("boss".to_string(), Precheck { expired: Some("password expired 2026-10-01".into()), ..Default::default() });
    let mut fake = Fake::new(|| menu(&full_menu()));
    run(&mut fake, dir.path(), &["held", "blind", "boss", "admin"], Some("Leave"), &pre).await;
    let marks = load_marks(dir.path(), &env);
    assert!(!marks.contains_key("held") && !marks.contains_key("blind"), "{marks:?}");
    assert!(marks["boss"].area.as_deref() == Some("Leave") && !marks["boss"].signed_in, "{:?}", marks["boss"]);
    assert!(marks["admin"].reached && marks["admin"].signed_in && marks["admin"].area.as_deref() == Some("Leave"));
    // A browser that would not start says nothing about the account either.
    let mut broken = Fake::new(|| menu(&full_menu()));
    broken.refuse = Some("it started but never answered".into());
    run(&mut broken, dir.path(), &["sup"], None, &BTreeMap::new()).await;
    assert!(!load_marks(dir.path(), &env).contains_key("sup"));
    // A sign-in alone is a mark that signs in and reaches nothing.
    let mut again = Fake::new(|| menu(&full_menu()));
    run(&mut again, dir.path(), &["sup"], None, &BTreeMap::new()).await;
    let marks = load_marks(dir.path(), &env);
    assert!(marks["sup"].signed_in && !marks["sup"].reached && marks["sup"].area.is_none(), "{:?}", marks["sup"]);
}

#[tokio::test]
async fn cancel_ends_the_check_between_accounts_and_every_browser_is_closed() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin", "sup", "boss"]);
    let none: Option<&Answers> = None;
    let mut n = 0;
    let mut fake = Fake::new(move || {
        n += 1;
        // While the check goes, every gate names it.
        let said = going_sentence("an unattended run is going");
        assert!(said.starts_with("a sign-in check is running (") && said.ends_with(") - wait for it or cancel it"), "{said}");
        assert!(v2_lib::commands::autorun_check_signin::refusal().unwrap().contains("sign-in check is running"));
        if n == 1 {
            cancel();
        }
        menu(&full_menu())
    });
    let out = check_with(&mut fake, none, ("", ""), dir.path(), ORG, PROJECT, &keys(&["admin", "sup", "boss"]), Some("Leave"))
        .await
        .unwrap();
    // Opened for the first only; the browser it had was closed.
    assert_eq!((fake.opened, fake.closed), (1, 1));
    assert_eq!(out.len(), 3);
    assert!(out[1..].iter().all(|c| c.stopped.as_deref() == Some(CANCELLED)), "{out:?}");
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    assert!(!lease::is_held(&env, "admin"));
    assert!(!load_marks(dir.path(), &env).contains_key("sup"), "a cancelled account was marked");
    // Nothing is going afterwards, and the sentence is the ordinary one.
    assert_eq!(going_sentence("an unattended run is going"), "an unattended run is going");
    assert!(v2_lib::commands::autorun_check_signin::refusal().is_none());
}

// ---- The assistant's tool ------------------------------------------------------

use v2_lib::ai_bridge::{check_sign_in_in, route, BridgeContext};

#[tokio::test]
async fn the_tool_answers_each_account_with_its_summary_and_no_password() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin", "sup"]);
    let mut n = 0;
    let mut fake = Fake::new(move || {
        n += 1;
        if n == 1 { menu(&full_menu()) } else { menu(&[("link", "Leave", "/hr/leave")]) }
    });
    let none: Option<&Answers> = None;
    let body = json!({ "accounts": ["admin", "sup"], "area": "Definition Wizard" }).to_string();
    let (status, text) = check_sign_in_in(&mut fake, none, dir.path(), ORG, PROJECT, &body).await;
    assert_eq!(status, 200, "{text}");
    assert!(!text.contains(PASSWORD), "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let rows = v["checked"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{text}");
    assert_eq!(rows[0]["account"], "admin");
    assert_eq!(
        rows[0]["summary"],
        "signed in, landed on /hr/home/index; reaches Definition Wizard - not checked for other sign-ins"
    );
    assert!(rows[1]["summary"].as_str().unwrap().contains("stopped on the way to Definition Wizard"), "{text}");
    assert_eq!((fake.opened, fake.closed), (2, 2));
}

#[tokio::test]
async fn the_tool_takes_the_accounts_it_is_given_and_refuses_a_stranger_first() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin", "sup"]);
    let mut fake = Fake::new(|| menu(&full_menu()));
    let none: Option<&Answers> = None;
    let (status, text) =
        check_sign_in_in(&mut fake, none, dir.path(), ORG, PROJECT, &json!({ "accounts": ["sup"] }).to_string()).await;
    assert_eq!(status, 200, "{text}");
    assert_eq!(fake.opened, 1);
    // `accounts` is required, and at most three are taken in one call.
    let (status, text) = check_sign_in_in(&mut fake, none, dir.path(), ORG, PROJECT, "{}").await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("needs `accounts`") && text.contains("at most 3"), "{text}");
    let four = json!({ "accounts": ["a", "b", "c", "d"] }).to_string();
    let (status, text) = check_sign_in_in(&mut fake, none, dir.path(), ORG, PROJECT, &four).await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("Split them across calls"), "{text}");
    assert_eq!(fake.opened, 1, "too many accounts still cost a sign-in");
    let (status, text) =
        check_sign_in_in(&mut fake, none, dir.path(), ORG, PROJECT, &json!({ "accounts": ["sup", "ghost"] }).to_string())
            .await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("ghost") && text.contains("get_accounts"), "{text}");
    assert_eq!(fake.opened, 1, "a stranger in the list still cost a sign-in");
    let (status, _) = check_sign_in_in(&mut fake, none, dir.path(), ORG, PROJECT, "{\"accounts\": 3}").await;
    assert_eq!(status, 400);
    let (status, _) = check_sign_in_in(&mut fake, none, dir.path(), ORG, PROJECT, "{\"password\": \"x\"}").await;
    assert_eq!(status, 400, "an argument it does not read is refused");
}

#[tokio::test]
async fn the_route_is_refused_while_an_unattended_run_goes() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    v2_lib::environments::load_or_init(dir.path(), Some("dev-read")).unwrap();
    v2_lib::autorun::store::set_root(dir.path().to_path_buf());
    accounts(dir.path(), &["admin"]);
    let _claim = OneAtATime::claim().expect("no run is going");
    let (status, text) = route(&BridgeContext::default(), None, "POST", "/autorun-check-sign-in", "{}", "1.0.0").await;
    assert_eq!(status, 409, "{text}");
    assert!(text.contains("unattended run"), "{text}");
}

#[tokio::test]
async fn a_proposal_lists_the_accounts_that_reached_the_area_asked_first() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    v2_lib::environments::load_or_init(dir.path(), Some("dev-read")).unwrap();
    v2_lib::autorun::store::set_root(dir.path().to_path_buf());
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    let now = v2_lib::autorun::sessions::now_ms() as f64;
    let mut marks = BTreeMap::new();
    marks.insert("hr.b".to_string(), mark(Some("Leave"), true, true, now));
    marks.insert("hr.c".to_string(), mark(Some("Leave"), true, false, now));
    marks.insert("hr.d".to_string(), mark(Some("Definition Wizard"), true, true, now));
    let marks_dir = v2_lib::autorun::roots::shared_of(dir.path()).join("sign_in_checks");
    std::fs::create_dir_all(&marks_dir).unwrap();
    std::fs::write(marks_dir.join(format!("{env}.json")), serde_json::to_string(&marks).unwrap()).unwrap();
    let p = |k: &str| json!({ "key": k, "label": k, "username": format!("{k}-user"), "no_password": "none" });
    let accounts = json!([p("hr.a"), p("hr.c"), p("hr.d"), p("hr.b")]);
    let order = |text: &str| {
        let _ = text;
        v2_lib::environments::load_proposals(dir.path(), &env).unwrap().into_iter().map(|p| p.key).collect::<Vec<_>>()
    };
    let body = json!({ "accounts": accounts, "area": "Leave" }).to_string();
    let (status, text) = route(&BridgeContext::default(), None, "POST", "/accounts-propose", &body, "1.0.0").await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("hr.b (reaches Leave)") && text.contains("hr.c (stopped on the way to Leave)"), "{text}");
    assert_eq!(order(&text), ["hr.b", "hr.a", "hr.d", "hr.c"]);
    // For another area the order follows that area.
    let body = json!({ "accounts": accounts, "area": "Definition Wizard" }).to_string();
    let (status, text) = route(&BridgeContext::default(), None, "POST", "/accounts-propose", &body, "1.0.0").await;
    assert_eq!(status, 200, "{text}");
    assert_eq!(order(&text), ["hr.d", "hr.a", "hr.c", "hr.b"]);
}

// ---- Who may ask, and when ---------------------------------------------------

use v2_lib::commands::autorun_check_signin::{check_with, refusal};
use v2_lib::commands::autorun_replay::OneAtATime;

#[tokio::test]
async fn check_refused_while_a_replay_runs() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    let claim = OneAtATime::claim().expect("no run is going");
    assert!(refusal().is_some());
    let mut fake = Fake::new(|| menu(&full_menu()));
    let none: Option<&Answers> = None;
    let refused = check_with(&mut fake, none, ("", ""), dir.path(), ORG, PROJECT, &keys(&["admin"]), Some("Leave")).await;
    assert!(refused.unwrap_err().contains("unattended run"), "a run was going");
    assert_eq!(fake.opened, 0);
    drop(claim);
    assert!(refusal().is_none());
}

#[tokio::test]
async fn check_holds_the_browser_while_it_goes_and_lets_it_go() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    let mut fake = Fake::new(|| {
        // While a check goes, no run, replay or other check can start.
        assert!(OneAtATime::claim().is_none(), "the browser was free during a check");
        menu(&full_menu())
    });
    let none: Option<&Answers> = None;
    let out = check_with(&mut fake, none, ("", ""), dir.path(), ORG, PROJECT, &keys(&["admin"]), Some("Leave")).await.unwrap();
    assert!(out[0].signed_in);
    assert!(OneAtATime::claim().is_some(), "the check kept the browser");
}

#[tokio::test]
async fn check_asks_the_environments_queries_before_any_browser() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    accounts(dir.path(), &["admin"]);
    // The account's login is kim; the expiry query says it is expired.
    let db = Answers { expiry: expiry(&["kim\t2026-10-01\t0"]), active: active(&[]) };
    let mut fake = Fake::new(|| menu(&full_menu()));
    let out = check_with(&mut fake, Some(&db), (EXPIRY_Q, ACTIVE_Q), dir.path(), ORG, PROJECT, &keys(&["admin"]), Some("Leave"))
        .await
        .unwrap();
    assert_eq!(fake.opened, 0, "a sign-in was tried for an expired password");
    assert!(summary(&out[0]).starts_with(NOT_TRIED) && summary(&out[0]).contains("password expired"), "{}", summary(&out[0]));
}
