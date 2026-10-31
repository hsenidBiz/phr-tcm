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
//! password, a session someone else holds) is not signed in at all.
//!
//! Every word that leaves here is a path (`save_guard::path_of`: no host, no
//! query), a page title, or a sentence the sign-in and the walk already say;
//! no password is in any of them.

use super::accounts::Account;
use super::active_login::{active_rows, failure_note, signed_in_elsewhere};
use super::lease::Held;
use super::nav::{self, Route, TripFrom};
use super::password_expiry::{card_words, check_users, Expiry};
use super::replay::Browsers;
use super::setup_find::RowSource;
use super::signin::{is_password_expired, redact, sign_in_leased, where_it_stands};
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How a stop that happened before any sign-in began is worded: nothing was
/// typed, the account's session was not touched.
pub const NOT_TRIED: &str = "sign-in not tried - ";

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
    /// What the expiry query said about the password, when it was asked and
    /// knew ("password due 2026-11-02, 3 days").
    pub password: Option<String>,
    /// Only when an area was asked for and the sign-in arrived.
    pub area: Option<AreaReach>,
}

/// The words for a check: one line, for the dialog and the assistant.
pub fn summary(c: &SignInCheck) -> String {
    if !c.signed_in {
        let why = c.stopped.clone().unwrap_or_else(|| "no reason was given".to_string());
        if why.starts_with(NOT_TRIED) {
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
    if let Some(p) = c.password.as_deref().filter(|p| !p.is_empty()) {
        said.push_str(&format!(" ({p})"));
    }
    said
}

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
    /// The expiry query's words for a password that is known.
    pub password: Option<String>,
    /// A check that could not be made, said once.
    pub note: Option<String>,
}

/// Asks the environment's two queries about one account (`username` is the
/// login, `key` what a script calls it). `ours` is whether the session the
/// site shows is this app's own (`active_login::session_is_ours`). A query
/// left empty is not asked; a database that does not answer stops nothing.
pub async fn precheck<S: RowSource>(
    rows: &S,
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
        match check_users(rows, expiry_query, &[username.to_string()], today).await {
            Ok(found) => {
                for (_, e) in found {
                    match &e {
                        Expiry::Expired { .. } | Expiry::ForcedChange => {
                            pre.expired = Some(card_words(&e));
                            pre.password = Some(card_words(&e));
                        }
                        Expiry::Ok { .. } | Expiry::DueSoon { .. } => pre.password = Some(card_words(&e)),
                        Expiry::Unknown(_) => {}
                    }
                }
            }
            Err(failed) => {
                down = failed == super::password_expiry::CheckFailed::DidNotAnswer;
                pre.note = Some(failure_note("password expiry query", &failed));
            }
        }
    }
    if !active_query.trim().is_empty() && !down {
        match active_rows(rows, active_query, username).await {
            Ok(found) => pre.elsewhere = signed_in_elsewhere(key, &found, ours),
            Err(failed) => {
                pre.note.get_or_insert_with(|| failure_note("active session query", &failed));
            }
        }
    }
    pre
}

fn not_tried(key: &str, why: impl AsRef<str>, pre: &Precheck) -> SignInCheck {
    SignInCheck {
        account: key.to_string(),
        signed_in: false,
        landed: None,
        stopped: Some(format!("{NOT_TRIED}{}", why.as_ref())),
        password: pre.password.clone(),
        area: None,
    }
}

/// What the last check of an account found, kept on this machine beside the
/// accounts so a later proposal can prefer the logins that reach an area.
/// Words only: no password, no host.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Mark {
    /// Milliseconds since the epoch; a float because specta refuses a
    /// 64-bit integer across IPC.
    pub checked_ms: f64,
    /// Signed in, and arrived at the area when one was asked for.
    pub reached: bool,
    /// The area asked for, when one was.
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

fn note_check(root: &Path, env_id: &str, c: &SignInCheck) {
    let mut marks = load_marks(root, env_id);
    marks.insert(
        c.account.clone(),
        Mark {
            checked_ms: super::sessions::now_ms() as f64,
            reached: c.signed_in && !matches!(c.area, Some(AreaReach::Stopped { .. })),
            area: match &c.area {
                Some(AreaReach::Reached { area }) | Some(AreaReach::Stopped { area, .. }) => Some(area.clone()),
                None => None,
            },
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

/// A mark in a few words, for a proposal's answer: `reaches Leave`,
/// `signed in`, or `stopped`.
pub fn mark_phrase(m: &Mark) -> String {
    match (m.reached, &m.area) {
        (true, Some(area)) => format!("reaches {area}"),
        (true, None) => "signed in".to_string(),
        (false, _) => "stopped".to_string(),
    }
}

/// How far down the list an account goes in a proposal: the ones whose last
/// check reached their area first, the ones never checked next, the ones
/// whose last check stopped last.
pub fn check_rank(marks: &BTreeMap<String, Mark>, key: &str) -> u8 {
    match marks.get(key) {
        Some(m) if m.reached => 0,
        None => 1,
        Some(_) => 2,
    }
}

/// `keys` with the accounts that reached their area first, then the ones
/// never checked, then the ones that stopped. Stable within a group.
pub fn prefer_reaching(keys: &[String], marks: &BTreeMap<String, Mark>) -> Vec<String> {
    let mut out = keys.to_vec();
    out.sort_by_key(|k| check_rank(marks, k));
    out
}

/// One account, before any browser opens: its account, its area's route, its
/// lease. `Err` is the finished check when it cannot be begun, and whether
/// that says something about the account (a mark is kept) or only about what
/// was asked (it is not).
struct Ready {
    account: Account,
    route: Option<(String, Route)>,
}

fn ready(
    root: &Path,
    org: &str,
    project: &str,
    key: &str,
    area: Option<&str>,
    pre: &Precheck,
) -> Result<Ready, (SignInCheck, bool)> {
    let (recipe, account) = super::signin::prepare(root, org, project, key).map_err(|why| (not_tried(key, why, pre), false))?;
    let route = match area.map(str::trim).filter(|a| !a.is_empty()) {
        None => None,
        Some(name) => {
            let file = nav::load_nav(root, org, project).map_err(|why| (not_tried(key, why, pre), false))?;
            match nav::route_for(&file, Some(name), None, Some(key)) {
                Ok(Some(path)) => Some((path.name().to_string(), Route::new(&recipe, path.clone()))),
                Ok(None) => None,
                Err(why) => return Err((not_tried(key, why, pre), false)),
            }
        }
    };
    if let Some(words) = &pre.expired {
        return Err((not_tried(key, words, pre), true));
    }
    if let Some(sentence) = &pre.elsewhere {
        return Err((not_tried(key, sentence, pre), true));
    }
    Ok(Ready { account, route })
}

/// Signs in once in `d` and walks to the area, if there is one.
async fn sign_in_and_walk<D: Driver>(
    d: &mut D,
    root: &Path,
    org: &str,
    project: &str,
    ready: &Ready,
    pre: &Precheck,
    lease: &mut Held,
    timing: &Timing,
) -> SignInCheck {
    let key = ready.account.key.as_str();
    let mut signed_in = None;
    let out = match sign_in_leased(d, root, org, project, key, lease, &mut signed_in, timing).await {
        Ok(out) => out,
        Err(why) => return not_tried(key, redact(&why, &ready.account), pre),
    };
    let mut c = SignInCheck {
        account: key.to_string(),
        signed_in: out.ok,
        landed: out.landed.clone(),
        stopped: (!out.ok).then(|| redact(&out.detail, &ready.account)),
        password: pre.password.clone(),
        area: None,
    };
    if let (true, Some((name, route))) = (out.ok, &ready.route) {
        c.area = Some(match nav::go_to_module(d, route, TripFrom::SignIn, timing).await {
            Ok(_) => AreaReach::Reached { area: name.clone() },
            Err(failure) => {
                let walk = redact(&failure.for_dialog(), &ready.account);
                let at = match where_it_stands(d).await {
                    Some(stands) => format!("{walk} - {stands}"),
                    None => walk,
                };
                AreaReach::Stopped { area: name.clone(), at }
            }
        });
    }
    c
}

/// The log's line for a check: the account's key and how it went, in a
/// few words, with nothing the page or the person typed.
fn log_words(c: &SignInCheck) -> &'static str {
    match (&c.area, c.signed_in) {
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
/// is not held back. The result is kept on each account that was really
/// looked at (`Mark`).
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
        let pre = pre.get(key).cloned().unwrap_or_default();
        let (check, keep) = match ready(root, org, project, key, area, &pre) {
            Err(done) => done,
            Ok(ready) => (one_account(browsers, root, org, project, &ready, &pre, timing).await, true),
        };
        crate::applog::info(format!("Auto Run sign-in check as {key}: {}", log_words(&check)));
        if keep && !env_id.is_empty() {
            note_check(root, &env_id, &check);
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
) -> SignInCheck {
    let key = ready.account.key.as_str();
    // The account first: one held by something else is said before a browser
    // is started for it.
    let mut lease = Held::setup();
    if let Err(why) = lease.hold(root, key).await {
        return not_tried(key, redact(&why, &ready.account), pre);
    }
    let mut d = match browsers.open().await {
        Ok(d) => d,
        Err(why) => {
            return SignInCheck {
                account: key.to_string(),
                signed_in: false,
                landed: None,
                stopped: Some(redact(&why, &ready.account)),
                password: pre.password.clone(),
                area: None,
            };
        }
    };
    let c = sign_in_and_walk(&mut d, root, org, project, ready, pre, &mut lease, timing).await;
    browsers.close(d).await;
    lease.let_go();
    c
}

/// Said when a check is asked for while the browser is in use.
pub const BROWSER_IN_USE: &str =
    "an unattended run, a replay or a discovery is using the Auto Run browser - wait for it, or stop it first";
