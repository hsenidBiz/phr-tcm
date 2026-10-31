//! Password expiry: reading when a login's password falls due, from the
//! environment's own "password expiry query".
//!
//! The date is not a column in PeoplesHR: it is worked out in SQL, so the
//! query belongs to the environment's settings and not to the code. It is
//! one SELECT, set like a setup's find rule:
//!
//! - `{users}` is its one placeholder. The app writes it as a comma list of
//!   quoted literals (`N'a', N'b'`, any quote doubled), so a query reads
//!   `... WHERE USRACC_USER_ID IN ({users})`. A `{users}` written inside
//!   quotes is refused. No other input value is ever put into SQL.
//! - the statement must be one SELECT the database guard classifies as a
//!   read, and it runs with the write switch off whatever the AI Bridge says;
//! - it answers the columns `user_id` and `reset_due` (a date, or a date and
//!   time), and may answer `prompt_change` (1 forces a change at the next
//!   sign-in whatever the date says).
//!
//! A database that is off or does not answer never blocks anything: the
//! check is skipped (`Expiry::Unknown`) and said once by the caller. No
//! sentence here carries SQL, a server name or a password.

use super::setup_find::{placeholder_inside_quotes, quoted_literal, starts_with_select, RowSource};
use crate::db::{guard, query};
use chrono::NaiveDate;

/// The placeholder for the logins being asked about.
pub const USERS: &str = "{users}";

/// A login whose password falls due in fewer days than this is warned about.
pub const DUE_SOON_DAYS: i64 = 7;

const WHAT: &str = "password expiry query";
const NO_LOGINS: &str = "there are no logins to ask about";

/// The sentence for a table the app could not read an answer from. Said in
/// place of the database's own words.
const UNREADABLE: &str = "the query's answer could not be read";

/// One login's row from the expiry query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiryRow {
    pub user: String,
    /// The date and perhaps time the password falls due, as the database
    /// printed it; `None` when it printed nothing (NULL).
    pub reset_due: Option<String>,
    /// The site forces a change at the next sign-in.
    pub prompt_change: bool,
}

/// What the expiry query says about one login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expiry {
    Ok { due: String, days_left: i64 },
    DueSoon { due: String, days_left: i64 },
    Expired { due: String },
    /// The site sends the login to its reset page at the next sign-in.
    ForcedChange,
    /// Not known: no row, no date, or the database could not say. Never
    /// treated as expired.
    Unknown(String),
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    let head: String = text.trim().chars().take(10).collect();
    NaiveDate::parse_from_str(&head, "%Y-%m-%d").ok()
}

/// What `row` says on the day `today` (`YYYY-MM-DD`). A forced change wins
/// over any date; a row without a usable date is `Unknown`, never expired.
pub fn judge(row: Option<&ExpiryRow>, today: &str) -> Expiry {
    let Some(row) = row else {
        return Expiry::Unknown("the expiry query has no row for this login".to_string());
    };
    if row.prompt_change {
        return Expiry::ForcedChange;
    }
    let Some(due) = row.reset_due.as_deref().and_then(parse_date) else {
        return Expiry::Unknown("the expiry query gave no date for this login".to_string());
    };
    let Some(now) = parse_date(today) else {
        return Expiry::Unknown("today's date could not be read".to_string());
    };
    let days_left = (due - now).num_days();
    let due = due.format("%Y-%m-%d").to_string();
    if days_left < 0 {
        Expiry::Expired { due }
    } else if days_left < DUE_SOON_DAYS {
        Expiry::DueSoon { due, days_left }
    } else {
        Expiry::Ok { due, days_left }
    }
}

fn days_words(n: i64) -> String {
    if n == 1 { "1 day".to_string() } else { format!("{n} days") }
}

/// The words the account card and the run window show.
pub fn card_words(e: &Expiry) -> String {
    match e {
        Expiry::Ok { due, days_left } | Expiry::DueSoon { due, days_left } => {
            format!("password due {due}, {}", days_words(*days_left))
        }
        Expiry::Expired { due } => format!("password expired {due}"),
        Expiry::ForcedChange => "password change forced at the next sign-in".to_string(),
        Expiry::Unknown(_) => "password expiry not known".to_string(),
    }
}

/// The mark a proposed login carries, when it has one. A forced change
/// reaches the reset page like an expired password does.
pub fn mark_of(e: &Expiry) -> Option<&'static str> {
    match e {
        Expiry::Expired { .. } | Expiry::ForcedChange => Some(EXPIRED_MARK),
        Expiry::DueSoon { .. } => Some(DUE_SOON_MARK),
        Expiry::Ok { .. } | Expiry::Unknown(_) => None,
    }
}

pub const EXPIRED_MARK: &str = "expired";
pub const DUE_SOON_MARK: &str = "due soon";

/// The statement as it will run: `placeholder` replaced by `value` (already
/// SQL), checked to be one SELECT the guard accepts as a read. Shared with
/// the active session query.
pub(crate) fn read_only_with(sql: &str, placeholder: &str, value: &str, what: &str) -> Result<String, String> {
    let one_select = || format!("A {what} is one SELECT statement.");
    if !starts_with_select(sql) {
        return Err(one_select());
    }
    if !sql.contains(placeholder) {
        return Err(format!("A {what} uses {placeholder} for the login it asks about."));
    }
    if placeholder_inside_quotes(sql, placeholder) {
        return Err(format!("Write {placeholder} outside quotes: the app puts the login in as a quoted value."));
    }
    if sql.replace(placeholder, "").contains(['{', '}']) {
        return Err(format!("A {what} takes no placeholder but {placeholder}."));
    }
    let out = sql.replace(placeholder, value);
    match guard::classify(&out) {
        guard::Verdict::Read => Ok(out),
        guard::Verdict::Refused(why) => Err(why),
        _ => Err(one_select()),
    }
}

/// Whether a query may be saved: empty (not checked), or one read that uses
/// its placeholder. The sentence says what is wrong.
pub(crate) fn check_template(sql: &str, placeholder: &str, what: &str) -> Result<(), String> {
    if sql.trim().is_empty() {
        return Ok(());
    }
    read_only_with(sql, placeholder, "N'x'", what).map(|_| ())
}

/// [`check_template`] for the expiry query.
pub fn check_query(sql: &str) -> Result<(), String> {
    check_template(sql, USERS, WHAT)
}

/// The query as it will run for `users`.
pub fn query_with_users(sql: &str, users: &[String]) -> Result<String, String> {
    if users.is_empty() {
        return Err(NO_LOGINS.to_string());
    }
    let list = users.iter().map(|u| quoted_literal(u)).collect::<Vec<_>>().join(", ");
    read_only_with(sql, USERS, &list, WHAT)
}

/// A sqlcmd answer as its header and rows (cells trimmed). Reads what
/// `setup_find::first_test_record` reads: an optional `rows: N (capped)`
/// line, a header, a dash line, rows, and a footer with the row count.
pub(crate) fn parse_table(text: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let first = text.lines().next().map(str::trim).unwrap_or("");
    let capped = first.starts_with("rows:") && first.ends_with("(capped)");
    let text = if capped { text.split_once('\n').map_or("", |(_, rest)| rest) } else { text };
    if !capped && query::rows_affected(text).is_none() {
        return Err(UNREADABLE.to_string());
    }
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
    let header: Vec<String> = match lines.next() {
        Some(h) => h.split('\t').map(|c| c.trim().to_lowercase()).collect(),
        None => return Err(UNREADABLE.to_string()),
    };
    let mut rows = vec![];
    for (i, line) in lines.enumerate() {
        let trimmed = line.trim();
        if i == 0 && !trimmed.is_empty() && trimmed.chars().all(|c| c == '-' || c == '\t' || c == ' ') {
            continue;
        }
        if trimmed.is_empty() || query::rows_affected(trimmed).is_some() {
            continue;
        }
        rows.push(line.split('\t').map(|c| c.trim().to_string()).collect());
    }
    Ok((header, rows))
}

pub(crate) fn column(header: &[String], name: &str) -> Option<usize> {
    header.iter().position(|c| c == name)
}

/// The cell, or `None` when it is empty or the database's NULL.
pub(crate) fn cell(row: &[String], at: usize) -> Option<String> {
    row.get(at).map(|c| c.trim()).filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("NULL")).map(str::to_string)
}

/// Reads the expiry query's answer.
pub fn parse_rows(text: &str) -> Result<Vec<ExpiryRow>, String> {
    let (header, rows) = parse_table(text)?;
    let missing = |name: &str| format!("the password expiry query's answer has no column {name}");
    let user = column(&header, "user_id").ok_or_else(|| missing("user_id"))?;
    let due = column(&header, "reset_due").ok_or_else(|| missing("reset_due"))?;
    let prompt = column(&header, "prompt_change");
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some(ExpiryRow {
                user: cell(r, user)?,
                reset_due: cell(r, due),
                prompt_change: prompt.and_then(|p| cell(r, p)).is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
            })
        })
        .collect())
}

/// Asks the expiry query about `users` and judges each, in the order given.
/// `Err` is a sentence saying the check could not be made (the database did
/// not answer, or its answer could not be read): the caller skips the check
/// and says so once, and never blocks on it.
pub async fn check_users<S: RowSource>(
    rows: &S,
    sql: &str,
    users: &[String],
    today: &str,
) -> Result<Vec<(String, Expiry)>, String> {
    let asked = query_with_users(sql, users)?;
    let answer = match rows.select(&asked).await {
        Ok(a) => a,
        Err(said) => {
            crate::applog::warn(format!("Auto Run: the password expiry query did not run: {said}"));
            return Err("the database did not answer".to_string());
        }
    };
    let found = parse_rows(&answer).inspect_err(|why| {
        crate::applog::warn(format!("Auto Run: the password expiry query's answer was not read: {why}"));
    })?;
    Ok(users
        .iter()
        .map(|u| {
            let row = found.iter().find(|r| r.user.eq_ignore_ascii_case(u.trim()));
            (u.clone(), judge(row, today))
        })
        .collect())
}

/// Today's date as `judge` reads it, on this machine.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}
