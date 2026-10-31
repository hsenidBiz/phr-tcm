//! Check sign-in: each account signed in once, in a browser of its own, and
//! walked as far as a chosen area (backlog 40 part 2 and 53). It says, per
//! account, that it reaches the area, or where it stopped, so a person or an
//! assistant knows which logins can script a screen before any case is
//! written for it.
//!
//! It is the normal sign-in (`signin::sign_in_leased`, saved session first,
//! the recipe's steps otherwise, the password typed at most once) and the
//! walk a run makes (`nav::go_to_module`, menu paths for other menu shapes
//! included). One account at a time, each in a browser of its own that is
//! closed, and its account lease let go, before the next: never two
//! sign-ins at once, and never the person's browser.
//!
//! An account the environment's queries say cannot sign in (an expired
//! password, a session someone else holds) is not signed in at all. So is
//! one whose "signed in elsewhere" query is set but cannot be asked: a
//! sign-in would log that session out blind.
//!
//! A check can be cancelled (`cancel`): it stops between accounts and ends
//! the account it is on, closing that browser.
//!
//! Every word that leaves here is a path (`save_guard::path_of`: no host, no
//! query), a page title, or a sentence the sign-in and the walk already say;
//! no password is in any of them.

use super::accounts::Account;
use super::active_login::{active_rows, signed_in_elsewhere};
use super::lease::Held;
use super::nav::{self, Route, TripFrom};
use super::password_expiry::{card_words, check_users, CheckFailed, Expiry, EXPIRED_MARK};
use super::replay::Browsers;
use super::setup_find::RowSource;
use super::signin::{is_password_expired, redact, sign_in_leased, where_it_stands};
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// How a stop that happened before any sign-in began is worded: nothing was
/// typed, the account's session was not touched.
pub const NOT_TRIED: &str = "sign-in not tried - ";

/// A check the person or an assistant ended.
pub const CANCELLED: &str = "the check was cancelled";

/// Said beside a result when the environment has no "signed in elsewhere"
/// query, so nobody could be told about.
pub const NO_OTHER_SIGN_IN_CHECK: &str = "not checked for other sign-ins";

/// Said beside a result when the expiry query exists but could not be asked.
pub const EXPIRY_NOT_CHECKED: &str = "expiry not checked";

/// Marks older than this are shown as stale and do not rank an account.
pub const STALE_DAYS: u64 = 7;

/// The most accounts one call to the assistant's tool may name.
pub const MOST_ACCOUNTS: usize = 3;

// ---- Which check is going ----------------------------------------------------

fn going() -> &'static Mutex<Option<String>> {
    static GOING: Mutex<Option<String>> = Mutex::new(None);
    &GOING
}

static CANCEL: AtomicBool = AtomicBool::new(false);

/// The account a check is on, or `None` when no check is going.
pub fn running_account() -> Option<String> {
    going().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn now_on(account: &str) {
    *going().lock().unwrap_or_else(|e| e.into_inner()) = Some(account.to_string());
}

/// The check going moves on to `account`; nothing changes when none is.
fn moves_on_to(account: &str) {
    if let Some(on) = going().lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        *on = account.to_string();
    }
}

/// While this lives a check is going; dropping it (an end, an error, a
/// panic) says none is.
pub struct Running(());

/// A check begins with `first`: it is going from here, and a cancel asked
/// before it began is forgotten.
pub fn begin(first: &str) -> Running {
    CANCEL.store(false, Ordering::SeqCst);
    now_on(first);
    Running(())
}

impl Drop for Running {
    fn drop(&mut self) {
        *going().lock().unwrap_or_else(|e| e.into_inner()) = None;
        // A cancel asked of this check ends with it.
        CANCEL.store(false, Ordering::SeqCst);
    }
}

/// End the check going, if any: it stops between accounts, and the account
/// it is on is ended and its browser closed.
pub fn cancel() {
    if running_account().is_some() {
        CANCEL.store(true, Ordering::SeqCst);
    }
}

fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

/// What every gate says while a check is going, in place of "an unattended
/// run is going".
pub fn running_sentence() -> Option<String> {
    running_account().map(|a| format!("a sign-in check is running ({a}) - wait for it or cancel it"))
}

/// `default`, unless a check is the one going.
pub fn going_sentence(default: &str) -> String {
    running_sentence().unwrap_or_else(|| default.to_string())
}

// ---- The words ---------------------------------------------------------------

/// Whether the account's menu leads to the area that was asked for.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AreaReach {
    /// The walk arrived where the area was recorded to arrive.
    Reached { area: String },
    /// The walk stopped: `at` is the walk's own sentence and where the page
    /// stood (its path and title).
    Stopped { area: String, at: String },
}

/// One account's check.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SignInCheck {
    /// The account's key, never its login.
    pub account: String,
    pub signed_in: bool,
    /// The path the browser stood on after the sign-in.
    pub landed: Option<String>,
    /// Why there was no sign-in: a sentence, the password reset page and a
    /// stop before any attempt included.
    pub stopped: Option<String>,
    /// What the expiry query said about the password's due date, when it
    /// was asked and knew ("password due 2026-11-02, 3 days"). Words, never
    /// a password.
    pub password_due: Option<String>,
    /// What could not be checked, said beside the result.
    pub note: Option<String>,
    /// Only when an area was asked for and the sign-in arrived.
    pub area: Option<AreaReach>,
}

/// The words for a check: one line, for the dialog and the assistant.
pub fn summary(c: &SignInCheck) -> String {
    if !c.signed_in {
        let why = c.stopped.clone().unwrap_or_else(|| "no reason was given".to_string());
        if why.starts_with(NOT_TRIED) || why == CANCELLED {
            return why;
        }
        if is_password_expired(&why) {
            return "stopped on the password reset page".to_string();
        }
        return format!("sign-in failed: {why}");
    }
    let mut said = "signed in".to_string();
    if let Some(path) = c.landed.as_deref().filter(|p| !p.is_empty()) {
        said.push_str(&format!(", landed on {path}"));
    }
    match &c.area {
        Some(AreaReach::Reached { area }) => said.push_str(&format!("; reaches {area}")),
        Some(AreaReach::Stopped { area, at }) => said.push_str(&format!("; stopped on the way to {area}: {at}")),
        None => {}
    }
    if let Some(p) = c.password_due.as_deref().filter(|p| !p.is_empty()) {
        said.push_str(&format!(" ({p})"));
    }
    if let Some(n) = c.note.as_deref().filter(|n| !n.is_empty()) {
        said.push_str(&format!(" - {n}"));
    }
    said
}

// ---- Before a browser opens --------------------------------------------------

/// What the environment's queries said about an account before any browser
/// opens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Precheck {
    /// The password is expired (or a change is forced): the site would send
    /// the sign-in to its reset page. The words of the date.
    pub expired: Option<String>,
    /// Someone else is signed in as the account; a sign-in would log that
    /// session out. The sentence.
    pub elsewhere: Option<String>,
    /// The environment asks whether someone else is signed in, and that
    /// could not be asked: a sign-in would be blind. The sentence.
    pub blind: Option<String>,
    /// The expiry query's words for a password that is known.
    pub password_due: Option<String>,
    /// What was not checked, said beside the result.
    pub notes: Vec<String>,
}

fn why_not_answered(failed: &CheckFailed) -> String {
    match failed {
        CheckFailed::DidNotAnswer => "the database is off or did not answer".to_string(),
        CheckFailed::QueryFailed(why) => why.clone(),
    }
}

fn blind_sentence(key: &str, why: &str) -> String {
    format!(
        "could not check whether someone else is signed in as {key} ({why}) - fix the query or turn on database read access"
    )
}

/// Asks the environment's two queries about one account (`username` is the
/// login, `key` what a script calls it). `rows` is the database, `None` when
/// reading it is off or it is not set up. `ours` is whether the session the
/// site shows is this app's own (`active_login::session_is_ours`).
///
/// An expiry query that cannot be asked is a note ("expiry not checked").
/// An active session query that is set and cannot be asked stops the
/// account (`blind`): signing in could log someone out unseen. With no such
/// query the check goes on and says it did not look.
pub async fn precheck<S: RowSource>(
    rows: Option<&S>,
    expiry_query: &str,
    active_query: &str,
    key: &str,
    username: &str,
    ours: bool,
    today: &str,
) -> Precheck {
    let mut pre = Precheck::default();
    let mut down = false;
    if !expiry_query.trim().is_empty() {
        match rows {
            None => pre.notes.push(EXPIRY_NOT_CHECKED.to_string()),
            Some(rows) => match check_users(rows, expiry_query, &[username.to_string()], today).await {
                Ok(found) => {
                    for (_, e) in found {
                        match &e {
                            Expiry::Expired { .. } | Expiry::ForcedChange => {
                                pre.expired = Some(card_words(&e));
                                pre.password_due = Some(card_words(&e));
                            }
                            Expiry::Ok { .. } | Expiry::DueSoon { .. } => pre.password_due = Some(card_words(&e)),
                            Expiry::Unknown(_) => {}
                        }
                    }
                }
                Err(failed) => {
                    down = failed == CheckFailed::DidNotAnswer;
                    pre.notes.push(EXPIRY_NOT_CHECKED.to_string());
                }
            },
        }
    }
    if active_query.trim().is_empty() {
        pre.notes.push(NO_OTHER_SIGN_IN_CHECK.to_string());
    } else {
        match rows {
            None => {
                pre.blind =
                    Some(blind_sentence(key, "database read access is off or the database is not set up"));
            }
            Some(_) if down => {
                pre.blind = Some(blind_sentence(key, "the database did not answer"));
            }
            Some(rows) => match active_rows(rows, active_query, username).await {
                Ok(found) => pre.elsewhere = signed_in_elsewhere(key, &found, ours),
                Err(failed) => pre.blind = Some(blind_sentence(key, &why_not_answered(&failed))),
            },
        }
    }
    pre
}

/// The accounts among `accounts` whose password the expiry query says is
/// expired, being changed at the next sign-in, or due within a week: one
/// line each, for the run window. Empty when the query says nothing of them.
pub async fn expiring<S: RowSource>(rows: &S, expiry_query: &str, accounts: &[Account], today: &str) -> Vec<String> {
    if expiry_query.trim().is_empty() || accounts.is_empty() {
        return vec![];
    }
    let logins: Vec<String> = accounts.iter().map(|a| a.username.clone()).collect();
    let Ok(found) = check_users(rows, expiry_query, &logins, today).await else { return vec![] };
    accounts
        .iter()
        .zip(found)
        .filter_map(|(a, (_, e))| match e {
            Expiry::Expired { .. } | Expiry::ForcedChange | Expiry::DueSoon { .. } => {
                Some(format!("{}: {}", a.key, card_words(&e)))
            }
            Expiry::Ok { .. } | Expiry::Unknown(_) => None,
        })
        .collect()
}

fn not_tried(key: &str, why: impl AsRef<str>, pre: &Precheck) -> SignInCheck {
    SignInCheck {
        account: key.to_string(),
        signed_in: false,
        landed: None,
        stopped: Some(format!("{NOT_TRIED}{}", why.as_ref())),
        password_due: pre.password_due.clone(),
        note: None,
        area: None,
    }
}

fn cancelled_check(key: &str) -> SignInCheck {
    SignInCheck {
        account: key.to_string(),
        signed_in: false,
        landed: None,
        stopped: Some(CANCELLED.to_string()),
        password_due: None,
        note: None,
        area: None,
    }
}

// ---- The mark on the account -------------------------------------------------

/// What the last check of an account found, kept on this machine beside the
/// accounts so a later proposal can prefer the logins that reach an area.
/// Words only: no password, no host.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Mark {
    /// Milliseconds since the epoch; a float because specta refuses a
    /// 64-bit integer across IPC.
    pub checked_ms: f64,
    /// The sign-in worked.
    #[serde(default)]
    pub signed_in: bool,
    /// An area was asked for and the walk arrived: only then.
    pub reached: bool,
    /// The area the check was for; none for a sign-in alone.
    pub area: Option<String>,
    /// The check's summary line.
    pub outcome: String,
}

fn marks_path(root: &Path, env_id: &str) -> PathBuf {
    super::roots::shared_of(root).join("sign_in_checks").join(format!("{env_id}.json"))
}

/// The last check of each account of one environment, by key. A file that is
/// missing or unreadable is no marks at all: they are derived, never needed.
pub fn load_marks(root: &Path, env_id: &str) -> BTreeMap<String, Mark> {
    std::fs::read_to_string(marks_path(root, env_id))
        .ok()
        .and_then(|text| serde_json::from_str(text.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

fn note_check(root: &Path, env_id: &str, c: &SignInCheck, asked_area: Option<&str>) {
    let mut marks = load_marks(root, env_id);
    marks.insert(
        c.account.clone(),
        Mark {
            checked_ms: super::sessions::now_ms() as f64,
            signed_in: c.signed_in,
            reached: matches!(c.area, Some(AreaReach::Reached { .. })),
            area: asked_area.map(str::to_string),
            outcome: summary(c),
        },
    );
    let path = marks_path(root, env_id);
    let wrote = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&path, serde_json::to_string_pretty(&marks).unwrap_or_default()));
    if let Err(e) = wrote {
        // The error's words would name the path on this machine.
        crate::applog::warn(format!("Auto Run sign-in check: the result could not be kept ({:?})", e.kind()));
    }
}

/// A mark older than `STALE_DAYS`.
pub fn is_stale(m: &Mark, now_ms: u64) -> bool {
    (now_ms as f64) - m.checked_ms > (STALE_DAYS * 24 * 3600 * 1000) as f64
}

/// How far down the list an account goes, for the area asked (or for a
/// sign-in alone when none is): the ones whose last check reached the area
/// (or signed in) first, the ones with no mark for it next - never checked,
/// checked for another area, or checked too long ago - the ones that stopped
/// last.
pub fn check_rank(marks: &BTreeMap<String, Mark>, key: &str, area: Option<&str>, now_ms: u64) -> u8 {
    let Some(m) = marks.get(key).filter(|m| !is_stale(m, now_ms)) else { return 1 };
    match area.map(str::trim).filter(|a| !a.is_empty()) {
        Some(asked) => {
            if m.area.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(asked)) {
                if m.reached { 0 } else { 2 }
            } else {
                1
            }
        }
        None => {
            if m.signed_in { 0 } else { 2 }
        }
    }
}

/// The one ranking a proposal is listed in: first what Task 9's marks say of
/// the login (none, some, expired), then what the last check found for the
/// area asked.
pub fn proposal_rank(
    problems: &[String],
    marks: &BTreeMap<String, Mark>,
    key: &str,
    area: Option<&str>,
    now_ms: u64,
) -> (u8, u8) {
    let problem = if problems.is_empty() {
        0
    } else if problems.iter().any(|m| m == EXPIRED_MARK) {
        2
    } else {
        1
    };
    (problem, check_rank(marks, key, area, now_ms))
}

/// `keys` with the accounts that reached the area first, then the ones with
/// no mark for it, then the ones that stopped. Stable within a group.
pub fn prefer_reaching(keys: &[String], marks: &BTreeMap<String, Mark>, area: Option<&str>, now_ms: u64) -> Vec<String> {
    let mut out = keys.to_vec();
    out.sort_by_key(|k| check_rank(marks, k, area, now_ms));
    out
}

/// A mark in a few words, for a proposal's answer and the dialog.
pub fn mark_phrase(m: &Mark, now_ms: u64) -> String {
    let said = match (m.signed_in, m.reached, &m.area) {
        (false, _, _) => "could not sign in".to_string(),
        (true, true, Some(area)) => format!("reaches {area}"),
        (true, false, Some(area)) => format!("stopped on the way to {area}"),
        (true, _, None) => "signs in".to_string(),
    };
    if is_stale(m, now_ms) { format!("{said}, checked over {STALE_DAYS} days ago") } else { said }
}

// ---- The check ---------------------------------------------------------------

/// One account, before any browser opens: its account and its area's route.
struct Ready {
    account: Account,
    route: Option<(String, Route)>,
}

/// A check finished before any browser opened: whether it says something
/// about the account (so a mark is kept) and the area asked.
struct Stop {
    check: SignInCheck,
    keep: bool,
    area: Option<String>,
}

fn ready(
    root: &Path,
    org: &str,
    project: &str,
    key: &str,
    area: Option<&str>,
    pre: &Precheck,
) -> Result<Ready, Stop> {
    let plain = |why: String| Stop { check: not_tried(key, why, pre), keep: false, area: None };
    let (recipe, account) = super::signin::prepare(root, org, project, key).map_err(plain)?;
    let route = match area.map(str::trim).filter(|a| !a.is_empty()) {
        None => None,
        Some(name) => {
            let file = nav::load_nav(root, org, project).map_err(plain)?;
            match nav::route_for(&file, Some(name), None, Some(key)) {
                Ok(Some(path)) => Some((path.name().to_string(), Route::new(&recipe, path.clone()))),
                Ok(None) => None,
                Err(why) => return Err(plain(why)),
            }
        }
    };
    let asked = route.as_ref().map(|(name, _)| name.clone());
    if let Some(words) = &pre.expired {
        return Err(Stop { check: not_tried(key, words, pre), keep: true, area: asked });
    }
    if let Some(sentence) = &pre.elsewhere {
        return Err(Stop { check: not_tried(key, sentence, pre), keep: true, area: asked });
    }
    // Nothing about the account was learned: not kept.
    if let Some(sentence) = &pre.blind {
        return Err(Stop { check: not_tried(key, sentence, pre), keep: false, area: asked });
    }
    Ok(Ready { account, route })
}

/// Signs in once in `d` and walks to the area, if there is one. The flag
/// says whether the outcome is about the account (a browser that stopped
/// answering says nothing of it).
#[allow(clippy::too_many_arguments)]
async fn sign_in_and_walk<D: Driver>(
    d: &mut D,
    root: &Path,
    org: &str,
    project: &str,
    ready: &Ready,
    pre: &Precheck,
    lease: &mut Held,
    timing: &Timing,
) -> (SignInCheck, bool) {
    let key = ready.account.key.as_str();
    let mut signed_in = None;
    let out = match sign_in_leased(d, root, org, project, key, lease, &mut signed_in, timing).await {
        Ok(out) => out,
        Err(why) => {
            let failed = SignInCheck {
                account: key.to_string(),
                signed_in: false,
                landed: None,
                stopped: Some(redact(&why, &ready.account)),
                password_due: pre.password_due.clone(),
                note: None,
                area: None,
            };
            return (failed, false);
        }
    };
    let mut c = SignInCheck {
        account: key.to_string(),
        signed_in: out.ok,
        landed: out.landed.clone(),
        stopped: (!out.ok).then(|| redact(&out.detail, &ready.account)),
        password_due: pre.password_due.clone(),
        note: (!pre.notes.is_empty()).then(|| pre.notes.join("; ")),
        area: None,
    };
    let mut about_the_account = !(out.harness && !out.ok);
    if let (true, Some((name, route))) = (out.ok, &ready.route) {
        c.area = Some(match nav::go_to_module(d, route, TripFrom::SignIn, timing).await {
            Ok(_) => AreaReach::Reached { area: name.clone() },
            Err(failure) => {
                about_the_account = !failure.harness;
                let walk = redact(&failure.for_dialog(), &ready.account);
                let at = match where_it_stands(d).await {
                    Some(stands) => format!("{walk} - {stands}"),
                    None => walk,
                };
                AreaReach::Stopped { area: name.clone(), at }
            }
        });
    }
    (c, about_the_account)
}

/// The log's line for a check: the account's key and how it went, in a
/// few words, with nothing the page or the person typed.
fn log_words(c: &SignInCheck) -> &'static str {
    match (&c.area, c.signed_in) {
        (_, false) if c.stopped.as_deref() == Some(CANCELLED) => "cancelled",
        (_, false) if c.stopped.as_deref().is_some_and(|s| s.starts_with(NOT_TRIED)) => "not tried",
        (_, false) if c.stopped.as_deref().is_some_and(is_password_expired) => "password expired",
        (_, false) => "sign-in failed",
        (Some(AreaReach::Reached { .. }), true) => "reaches the area",
        (Some(AreaReach::Stopped { .. }), true) => "stopped on the way to the area",
        (None, true) => "signed in",
    }
}

/// Checks each of `keys` in turn, in `root`'s active environment: its
/// account taken first, a browser from `browsers` opened for it alone, the
/// sign-in made once, the walk to `area` made if one was named, and the
/// browser closed and the account let go before the next. `pre` carries
/// what the queries said (`precheck`), by key; an account without an entry
/// is not held back. The result is kept on each account whose outcome is
/// about the account (`Mark`): not a busy account, a browser that would not
/// start, one that stopped answering, or a check that was cancelled.
///
/// `cancel` ends it: accounts not yet begun are answered as cancelled, and
/// the one going is ended and its browser closed.
#[allow(clippy::too_many_arguments)]
pub async fn check_accounts<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    keys: &[String],
    area: Option<&str>,
    pre: &BTreeMap<String, Precheck>,
    timing: &Timing,
) -> Vec<SignInCheck> {
    let env_id = crate::environments::active_id(root).unwrap_or_default();
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        if cancelled() {
            out.push(cancelled_check(key));
            continue;
        }
        moves_on_to(key);
        let pre = pre.get(key).cloned().unwrap_or_default();
        let (check, keep, asked) = match ready(root, org, project, key, area, &pre) {
            Err(stop) => (stop.check, stop.keep, stop.area),
            Ok(ready) => {
                let asked = ready.route.as_ref().map(|(name, _)| name.clone());
                let (check, keep) = one_account(browsers, root, org, project, &ready, &pre, timing).await;
                (check, keep, asked)
            }
        };
        crate::applog::info(format!("Auto Run sign-in check as {key}: {}", log_words(&check)));
        if keep && !env_id.is_empty() {
            note_check(root, &env_id, &check, asked.as_deref());
        }
        out.push(check);
    }
    out
}

async fn one_account<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    ready: &Ready,
    pre: &Precheck,
    timing: &Timing,
) -> (SignInCheck, bool) {
    let key = ready.account.key.as_str();
    // The account first: one held by something else is said before a browser
    // is started for it.
    let mut lease = Held::setup();
    if let Err(why) = lease.hold(root, key).await {
        return (not_tried(key, redact(&why, &ready.account), pre), false);
    }
    let mut d = match browsers.open().await {
        Ok(d) => d,
        Err(why) => {
            let failed = SignInCheck {
                account: key.to_string(),
                signed_in: false,
                landed: None,
                stopped: Some(redact(&why, &ready.account)),
                password_due: pre.password_due.clone(),
                note: None,
                area: None,
            };
            return (failed, false);
        }
    };
    let done = {
        let work = sign_in_and_walk(&mut d, root, org, project, ready, pre, &mut lease, timing);
        tokio::pin!(work);
        let asked_to_stop = async {
            while !cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        };
        tokio::select! {
            done = &mut work => Some(done),
            _ = asked_to_stop => None,
        }
    };
    browsers.close(d).await;
    lease.let_go();
    done.unwrap_or_else(|| (cancelled_check(key), false))
}

/// Said when a check is asked for while the browser is in use.
pub const BROWSER_IN_USE: &str =
    "an unattended run, a replay or a discovery is using the Auto Run browser - wait for it, or stop it first";
