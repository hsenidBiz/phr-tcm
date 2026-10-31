//! Active sessions: whether a login is already signed in somewhere else,
//! from the environment's own "active session query".
//!
//! PeoplesHR allows one session per user, and signing in anyway ("Continue
//! here") logs the other session out: its next requests come back as empty
//! 400s mid-run. The site records every sign-in in a table, so the question
//! is one read-only SELECT set per environment, like the password expiry
//! query (`password_expiry`):
//!
//! - `{user}` is its one placeholder, written by the app as the login's
//!   name in lower case as a quoted literal (any quote doubled); a `{user}`
//!   written inside quotes is refused;
//! - the statement must be one SELECT the database guard classifies as a
//!   read, and it runs with the write switch off;
//! - it answers the columns `since` (when the session began) and
//!   `logged_to` (which site it signed in to), and returns only sessions
//!   that are signed in NOW - the query filters on the flag, never on the
//!   log-off time, which older rows leave empty.
//!
//! A session this app's own Auto Run browser holds does not count. A
//! database that is off or does not answer never blocks anything: the
//! check is skipped and said once.

use super::password_expiry::{
    card_words, cell, check_template, column, mark_of, parse_table, read_only_with, Expiry, EXPIRED_MARK,
};
use super::setup_find::{quoted_literal, RowSource};

/// The placeholder for the login being asked about.
pub const USER: &str = "{user}";

const WHAT: &str = "active session query";

/// The mark a proposed login carries when someone is signed in as it.
pub const SIGNED_ELSEWHERE_MARK: &str = "signed in elsewhere";

/// Said once when the checks could not be made.
pub const NOT_CHECKED_NOTE: &str =
    "password expiry and sessions elsewhere were not checked: the database is off or did not answer";

/// One signed-in session of a login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRow {
    pub since: String,
    pub logged_to: String,
}

/// A run's database as a place the two queries are asked.
pub struct StageRows<'a, D>(pub &'a D);

impl<D: crate::api_templates::gate::StageDb> RowSource for StageRows<'_, D> {
    async fn select(&self, sql: &str) -> Result<String, String> {
        self.0.select(sql).await
    }
}

/// Whether a query may be saved: empty (not checked), or one read that uses
/// `{user}`.
pub fn check_query(sql: &str) -> Result<(), String> {
    check_template(sql, USER, WHAT)
}

/// The query as it will run for `user` (lower case).
pub fn query_with_user(sql: &str, user: &str) -> Result<String, String> {
    read_only_with(sql, USER, &quoted_literal(&user.trim().to_lowercase()), WHAT)
}

/// Reads the active session query's answer.
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

/// The time of day of a database timestamp (`09:40`), or the text as it is
/// when it carries none.
fn time_of(since: &str) -> String {
    let t = since.trim();
    match t.split_once([' ', 'T']) {
        Some((_, time)) if time.len() >= 5 => time[..5].to_string(),
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
        time_of(&first.since)
    ))
}

/// The sessions a login has right now. `Err` is a sentence saying the check
/// could not be made.
pub async fn active_rows<S: RowSource>(rows: &S, sql: &str, user: &str) -> Result<Vec<ActiveRow>, String> {
    let asked = query_with_user(sql, user)?;
    let answer = match rows.select(&asked).await {
        Ok(a) => a,
        Err(said) => {
            crate::applog::warn(format!("Auto Run: the active session query did not run: {said}"));
            return Err("the database did not answer".to_string());
        }
    };
    parse_rows(&answer).inspect_err(|why| {
        crate::applog::warn(format!("Auto Run: the active session query's answer was not read: {why}"));
    })
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
/// before it opens a browser. `ours` is whether this app's own Auto Run
/// browser holds the account. Nothing is asked of a query left empty; a
/// database that does not answer skips the check and is said once.
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
    let mut skipped = false;
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
            Err(_) => skipped = true,
        }
    }
    if !active_query.trim().is_empty() {
        match active_rows(rows, active_query, username).await {
            Ok(found) => {
                if let Some(said) = signed_in_elsewhere(key, &found, ours) {
                    gate.refused = Some(format!("{said}. Pick another account, or tell the person."));
                }
            }
            Err(_) => skipped = true,
        }
    }
    if skipped {
        gate.warnings.push(NOT_CHECKED_NOTE.to_string());
    }
    gate
}

/// The marks of a set of proposed logins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    /// Each login's key, in the order given, with the marks it carries.
    pub marks: Vec<(String, Vec<String>)>,
    /// Said once when the checks could not be made.
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

/// Marks each of `logins` (key, login name): expired, due soon, or signed in
/// elsewhere. `ours(key)` is whether this app's own browser holds that
/// account. A database that does not answer marks nothing and says so once;
/// nothing is asked when both queries are empty. A mark never carries a
/// password.
pub async fn mark_logins<S: RowSource>(
    rows: &S,
    expiry_query: &str,
    active_query: &str,
    logins: &[(String, String)],
    ours: &(dyn Fn(&str) -> bool + Sync),
    today: &str,
) -> Marks {
    let mut marks: Vec<(String, Vec<String>)> = logins.iter().map(|(k, _)| (k.clone(), vec![])).collect();
    let mut skipped = false;
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
            Err(_) => skipped = true,
        }
    }
    if !active_query.trim().is_empty() && !skipped {
        for ((key, user), (_, m)) in logins.iter().zip(marks.iter_mut()) {
            match active_rows(rows, active_query, user).await {
                Ok(found) => {
                    if signed_in_elsewhere(key, &found, ours(key)).is_some() {
                        m.push(SIGNED_ELSEWHERE_MARK.to_string());
                    }
                }
                Err(_) => {
                    skipped = true;
                    break;
                }
            }
        }
    }
    Marks { marks, note: skipped.then(|| NOT_CHECKED_NOTE.to_string()) }
}
