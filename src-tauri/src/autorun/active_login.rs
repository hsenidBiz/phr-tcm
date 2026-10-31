//! Active sessions: whether a login is already signed in somewhere else,
//! from the environment's own "active session query".
//!
//! PeoplesHR allows one session per user, and signing in anyway ("Continue
//! here") logs the other session out: its next requests come back as empty
//! 400s mid-run. The site records every sign-in in a table, so the question
//! is one read-only SELECT set per environment, like the password expiry
//! query (`password_expiry`). It takes ONE of two placeholders:
//!
//! - `{user}`: the login's name in lower case as a quoted literal (any quote
//!   doubled); the answer has the columns `since` (when the session began)
//!   and `logged_to` (which site it signed in to). Asked once per login.
//! - `{users}`: the comma list of quoted lower-case names, for one call for
//!   all the logins; the answer has `user_id` as well, one row per active
//!   session.
//!
//! A placeholder written inside quotes is refused, the statement must be one
//! SELECT the database guard classifies as a read, and it runs with the write
//! switch off. The query returns only sessions that are signed in NOW - it
//! filters on the flag, never on the log-off time, which older rows leave
//! empty.
//!
//! A session this app itself holds - its Auto Run browser has the account,
//! or it signed in as the account within the reuse window and still keeps
//! that session (`session_is_ours`) - does not count: the site's row for it
//! stays at flag 1 after the browser closes, and it is this app's own
//! earlier sign-in, not someone else's. A database that is off or does not
//! answer never blocks anything: the check is skipped and said once.

use super::password_expiry::{
    card_words, cell, check_template, column, mark_of, parse_table, read_only_with, run_failed, users_list_with,
    CheckFailed, Expiry, EXPIRED_MARK, USERS,
};
use super::setup_find::{quoted_literal, RowSource};
use std::path::Path;
use std::time::{Duration, Instant};

/// The placeholder for the one login being asked about.
pub const USER: &str = "{user}";

const WHAT: &str = "active session query";

/// The mark a proposed login carries when someone is signed in as it.
pub const SIGNED_ELSEWHERE_MARK: &str = "signed in elsewhere";

/// Said once when the database is off or did not answer.
pub const NOT_CHECKED_NOTE: &str =
    "password expiry and sessions elsewhere were not checked: the database is off or did not answer";

/// With a query that takes one login, at most this many are asked, one
/// after another.
pub const SINGLE_LOGIN_CAP: usize = 25;

/// And they stop when this long has gone, the rest marked not checked.
pub const SINGLE_BUDGET: Duration = Duration::from_secs(30);

/// One signed-in session of a login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRow {
    pub since: String,
    pub logged_to: String,
}

fn uses_list(sql: &str) -> bool {
    sql.contains(USERS)
}

/// Whether a query may be saved: empty (not checked), or one read that uses
/// `{user}` or `{users}`.
pub fn check_query(sql: &str) -> Result<(), String> {
    check_template(sql, if uses_list(sql) { USERS } else { USER }, WHAT)
}

/// The query as it will run for `user` (lower case).
pub fn query_with_user(sql: &str, user: &str) -> Result<String, String> {
    read_only_with(sql, USER, &quoted_literal(&user.trim().to_lowercase()), WHAT)
}

/// Reads the answer of the one-login form.
pub fn parse_rows(text: &str) -> Result<Vec<ActiveRow>, String> {
    let (header, rows) = parse_table(text)?;
    let missing = |name: &str| format!("the active session query's answer has no column {name}");
    let since = column(&header, "since").ok_or_else(|| missing("since"))?;
    let to = column(&header, "logged_to");
    Ok(rows
        .iter()
        .map(|r| ActiveRow {
            since: cell(r, since).unwrap_or_default(),
            logged_to: to.and_then(|t| cell(r, t)).unwrap_or_default(),
        })
        .collect())
}

/// Reads the answer of the list form: each row with the login it is for.
pub fn parse_user_rows(text: &str) -> Result<Vec<(String, ActiveRow)>, String> {
    let (header, rows) = parse_table(text)?;
    let missing = |name: &str| format!("the active session query's answer has no column {name}");
    let user = column(&header, "user_id").ok_or_else(|| missing("user_id"))?;
    let since = column(&header, "since").ok_or_else(|| missing("since"))?;
    let to = column(&header, "logged_to");
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some((
                cell(r, user)?,
                ActiveRow {
                    since: cell(r, since).unwrap_or_default(),
                    logged_to: to.and_then(|t| cell(r, t)).unwrap_or_default(),
                },
            ))
        })
        .collect())
}

/// A database timestamp as `2026-10-30 09:40`, or the text as it is when it
/// is not one. The date is always said: a row can be days old.
fn when_words(since: &str) -> String {
    let t = since.trim();
    match t.split_once([' ', 'T']) {
        Some((date, time)) if date.len() == 10 && time.len() >= 5 => format!("{date} {}", &time[..5]),
        _ => t.to_string(),
    }
}

/// The warning for a login someone else is signed in as, or `None` when
/// nobody is, or the only session is this app's own (`ours`).
pub fn signed_in_elsewhere(key: &str, rows: &[ActiveRow], ours: bool) -> Option<String> {
    if ours {
        return None;
    }
    let first = rows.first()?;
    let to = if first.logged_to.is_empty() { String::new() } else { format!(" ({})", first.logged_to) };
    Some(format!(
        "{key} is signed in elsewhere since {}{to} - signing in will log that session out",
        when_words(&first.since)
    ))
}

/// Whether the account's session is this app's own: an Auto Run browser
/// holds it now, or its saved session is within `session_minutes` - the
/// sign-in this app made earlier and still reuses, whose row the site keeps
/// at flag 1 after the browser closes.
pub fn session_is_ours(root: &Path, env_id: &str, key: &str, session_minutes: u32, now_ms: u64) -> bool {
    super::lease::holder_of(env_id, key).is_some()
        || super::sessions::load_fresh_session(root, key, session_minutes, now_ms).is_some()
}

/// A run's database as a place the two queries are asked.
pub struct StageRows<'a, D>(pub &'a D);

impl<D: crate::api_templates::gate::StageDb> RowSource for StageRows<'_, D> {
    async fn select(&self, sql: &str) -> Result<String, String> {
        self.0.select(sql).await
    }
}

async fn ask<S: RowSource>(rows: &S, asked: &str) -> Result<String, CheckFailed> {
    rows.select(asked).await.map_err(|said| {
        crate::applog::warn(format!("Auto Run: the active session query did not run: {said}"));
        run_failed(&said)
    })
}

fn unread(why: String) -> CheckFailed {
    crate::applog::warn(format!("Auto Run: the active session query's answer was not read: {why}"));
    CheckFailed::QueryFailed(why)
}

/// Every session of `users`, each with its login, in ONE call (the list
/// form).
async fn active_batch<S: RowSource>(
    rows: &S,
    sql: &str,
    users: &[String],
) -> Result<Vec<(String, ActiveRow)>, CheckFailed> {
    let lower: Vec<String> = users.iter().map(|u| u.trim().to_lowercase()).collect();
    let asked = users_list_with(sql, &lower, WHAT).map_err(CheckFailed::QueryFailed)?;
    parse_user_rows(&ask(rows, &asked).await?).map_err(unread)
}

/// The sessions one login has right now, whichever form the query is.
pub async fn active_rows<S: RowSource>(rows: &S, sql: &str, user: &str) -> Result<Vec<ActiveRow>, CheckFailed> {
    if uses_list(sql) {
        let found = active_batch(rows, sql, &[user.to_string()]).await?;
        return Ok(found
            .into_iter()
            .filter(|(u, _)| u.eq_ignore_ascii_case(user.trim()))
            .map(|(_, r)| r)
            .collect());
    }
    let asked = query_with_user(sql, user).map_err(CheckFailed::QueryFailed)?;
    parse_rows(&ask(rows, &asked).await?).map_err(unread)
}

/// What a skipped check says: the database is off or did not answer, or the
/// setting's query is wrong. Names the setting, never SQL.
fn failure_note(setting: &str, failed: &CheckFailed) -> String {
    match failed {
        CheckFailed::DidNotAnswer => NOT_CHECKED_NOTE.to_string(),
        CheckFailed::QueryFailed(why) => {
            format!("the {setting} could not run: {why} - see Settings, Logs, and the environment's setting")
        }
    }
}

fn add_note(notes: &mut Vec<String>, note: String) {
    if !notes.contains(&note) {
        notes.push(note);
    }
}

/// What the checks before a discovery's sign-in decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gate {
    /// The sentence a discovery is refused with: someone else is signed in
    /// as the account.
    pub refused: Option<String>,
    /// Said beside the answer, never blocking.
    pub warnings: Vec<String>,
}

/// The two checks a discovery makes for `key` (signing in as `username`)
/// before it opens a browser. `ours` is whether the session is this app's
/// own (`session_is_ours`). Nothing is asked of a query left empty; a
/// database that does not answer is asked no further and is said once.
pub async fn discovery_gate<S: RowSource>(
    rows: &S,
    expiry_query: &str,
    active_query: &str,
    key: &str,
    username: &str,
    ours: bool,
    today: &str,
) -> Gate {
    let mut gate = Gate::default();
    let mut down = false;
    if !expiry_query.trim().is_empty() {
        match super::password_expiry::check_users(rows, expiry_query, &[username.to_string()], today).await {
            Ok(found) => {
                for (_, e) in found {
                    match e {
                        Expiry::Expired { .. } => gate.warnings.push(format!(
                            "{key}: {} - the sign-in will land on the site's password reset page and fail as password expired",
                            card_words(&e)
                        )),
                        Expiry::DueSoon { .. } | Expiry::ForcedChange => {
                            gate.warnings.push(format!("{key}: {}", card_words(&e)))
                        }
                        Expiry::Ok { .. } | Expiry::Unknown(_) => {}
                    }
                }
            }
            Err(failed) => {
                down = failed == CheckFailed::DidNotAnswer;
                add_note(&mut gate.warnings, failure_note("password expiry query", &failed));
            }
        }
    }
    if !active_query.trim().is_empty() && !down {
        match active_rows(rows, active_query, username).await {
            Ok(found) => {
                if let Some(said) = signed_in_elsewhere(key, &found, ours) {
                    gate.refused = Some(format!(
                        "{said}. Pick another account, or sign that session out on the site, or wait for it to end; if this check is wrong for your site, clear the environment's active-session query in Environments."
                    ));
                }
            }
            Err(failed) => add_note(&mut gate.warnings, failure_note("active session query", &failed)),
        }
    }
    gate
}

/// The marks of a set of proposed logins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    /// Each login's key, in the order given, with the marks it carries.
    pub marks: Vec<(String, Vec<String>)>,
    /// Said once when a check could not be made or was cut short.
    pub note: Option<String>,
}

impl Marks {
    pub fn marks_of(&self, key: &str) -> Vec<String> {
        self.marks.iter().find(|(k, _)| k == key).map(|(_, m)| m.clone()).unwrap_or_default()
    }

    /// `keys` with the logins that can sign in first: no mark, then the ones
    /// due soon or signed in elsewhere, then the expired. Stable within a
    /// group.
    pub fn usable_first(&self, keys: &[String]) -> Vec<String> {
        let rank = |key: &str| {
            let marks = self.marks_of(key);
            if marks.iter().any(|m| m == EXPIRED_MARK) {
                2
            } else if marks.is_empty() {
                0
            } else {
                1
            }
        };
        let mut out = keys.to_vec();
        out.sort_by_key(|k| rank(k));
        out
    }
}

/// [`mark_logins_within`] with the single-login query's own time limit.
pub async fn mark_logins<S: RowSource>(
    rows: &S,
    expiry_query: &str,
    active_query: &str,
    logins: &[(String, String)],
    ours: &(dyn Fn(&str) -> bool + Sync),
    today: &str,
) -> Marks {
    mark_logins_within(rows, expiry_query, active_query, logins, ours, today, SINGLE_BUDGET).await
}

/// Marks each of `logins` (key, login name): expired, due soon, or signed in
/// elsewhere. `ours(key)` is whether that account's session is this app's
/// own. The expiry query and a `{users}` active session query are one call
/// each; a `{user}` one is asked login by login, at most `SINGLE_LOGIN_CAP`
/// of them and within `budget`, and the rest are said not checked. A
/// database that does not answer marks nothing, is asked no further and is
/// said once; nothing is asked when both queries are empty. A mark never
/// carries a password.
pub async fn mark_logins_within<S: RowSource>(
    rows: &S,
    expiry_query: &str,
    active_query: &str,
    logins: &[(String, String)],
    ours: &(dyn Fn(&str) -> bool + Sync),
    today: &str,
    budget: Duration,
) -> Marks {
    let mut marks: Vec<(String, Vec<String>)> = logins.iter().map(|(k, _)| (k.clone(), vec![])).collect();
    let mut notes: Vec<String> = vec![];
    let mut down = false;
    if !expiry_query.trim().is_empty() && !logins.is_empty() {
        let users: Vec<String> = logins.iter().map(|(_, u)| u.clone()).collect();
        match super::password_expiry::check_users(rows, expiry_query, &users, today).await {
            Ok(found) => {
                for ((_, e), (_, m)) in found.iter().zip(marks.iter_mut()) {
                    if let Some(mark) = mark_of(e) {
                        m.push(mark.to_string());
                    }
                }
            }
            Err(failed) => {
                down = failed == CheckFailed::DidNotAnswer;
                add_note(&mut notes, failure_note("password expiry query", &failed));
            }
        }
    }
    if !active_query.trim().is_empty() && !down && !logins.is_empty() {
        if uses_list(active_query) {
            let users: Vec<String> = logins.iter().map(|(_, u)| u.clone()).collect();
            match active_batch(rows, active_query, &users).await {
                Ok(found) => {
                    for ((key, user), (_, m)) in logins.iter().zip(marks.iter_mut()) {
                        let mine: Vec<ActiveRow> = found
                            .iter()
                            .filter(|(u, _)| u.eq_ignore_ascii_case(user.trim()))
                            .map(|(_, r)| r.clone())
                            .collect();
                        if signed_in_elsewhere(key, &mine, ours(key)).is_some() {
                            m.push(SIGNED_ELSEWHERE_MARK.to_string());
                        }
                    }
                }
                Err(failed) => add_note(&mut notes, failure_note("active session query", &failed)),
            }
        } else {
            let started = Instant::now();
            let mut asked = 0usize;
            let mut stopped = false;
            for ((key, user), (_, m)) in logins.iter().zip(marks.iter_mut()) {
                if asked >= SINGLE_LOGIN_CAP || started.elapsed() >= budget {
                    break;
                }
                asked += 1;
                match active_rows(rows, active_query, user).await {
                    Ok(found) => {
                        if signed_in_elsewhere(key, &found, ours(key)).is_some() {
                            m.push(SIGNED_ELSEWHERE_MARK.to_string());
                        }
                    }
                    Err(failed) => {
                        add_note(&mut notes, failure_note("active session query", &failed));
                        stopped = true;
                        break;
                    }
                }
            }
            let left = logins.len() - asked;
            if !stopped && left > 0 {
                let noun = if left == 1 { "login" } else { "logins" };
                add_note(
                    &mut notes,
                    format!(
                        "sessions elsewhere were not checked for {left} {noun} (a query for one login is asked for at most {SINGLE_LOGIN_CAP}, within {} s; a query with {{users}} covers them all in one call)",
                        SINGLE_BUDGET.as_secs()
                    ),
                );
            }
        }
    }
    Marks { marks, note: (!notes.is_empty()).then(|| notes.join("; ")) }
}
