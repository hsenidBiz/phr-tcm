//! Find rules for saved setups: how a setup looks for a test record that
//! already exists before it builds a new one.
//!
//! A rule is a database SELECT, a short browser check, or both. Either way it
//! can only ever match records named with the environment's test prefix:
//!
//! - `{prefix}` is the one placeholder in the SQL. The app writes it as a
//!   quoted literal (`N'<prefix>'`, any quote in the prefix doubled), and a
//!   `{prefix}` the caller wrote inside quotes is refused. No other input
//!   value is ever put into SQL.
//! - the statement must be one SELECT that the database guard classifies as a
//!   read, and it runs with the write switch off whatever the AI Bridge says;
//! - a row whose name column does not start with the prefix is skipped;
//! - a browser rule ends in a check that names `{prefix}`.
//!
//! The SQL is never logged from here: `db::query::run_query` writes it to the
//! activity log, as `db_query` does.

use crate::browser::actions::Action;
use crate::db::{guard, query, Connection, Runner};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const PREFIX: &str = "{prefix}";
pub const BROWSER_FIND_MAX: usize = 10;
pub const ONLY_TEST_RECORDS: &str =
    "A find rule must look only for records named with the test prefix.";
pub const DATABASE_OFF: &str = "This setup checks the database for an existing record, and Database Read Access is off on the AI Bridge tab. Ask the person to switch it on.";
pub const NEEDS_A_FIND_RULE: &str =
    "A setup needs a find rule: a database SELECT, a browser check, or both.";

const ONE_SELECT: &str = "A database find rule is one SELECT statement.";
const OUTSIDE_QUOTES: &str =
    "Write {prefix} outside quotes: the app puts it in as a quoted value.";
const NEEDS_ID_COLUMN: &str = "A database find rule names its id_column.";
const ONLY_PREFIX_PLACEHOLDER: &str = "A database find rule takes no placeholder but {prefix}.";
const BROWSER_ENDS_IN_A_CHECK: &str =
    "A browser find rule ends in a check: expect_visible, expect_text or expect_contains_text.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbFind {
    pub sql: String,
    pub id_column: String,
    #[serde(default)]
    pub name_column: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FindRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<DbFind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<Vec<Action>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FoundRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
}

/// Trimmed, case ignored, as the fixture run compares a record's name.
pub fn starts_with_prefix(name: &str, prefix: &str) -> bool {
    let prefix = prefix.trim().to_lowercase();
    !prefix.is_empty() && name.trim().to_lowercase().starts_with(&prefix)
}

/// Whether a `{prefix}` sits inside a quoted literal. A doubled quote stays
/// inside the literal.
fn prefix_inside_quotes(sql: &str) -> bool {
    placeholder_inside_quotes(sql, PREFIX)
}

/// Whether `placeholder` sits inside a quoted literal of `sql`. A doubled
/// quote stays inside the literal. Shared by the other read-only queries
/// that take a placeholder the app writes as a quoted value.
pub(crate) fn placeholder_inside_quotes(sql: &str, placeholder: &str) -> bool {
    let mut inside = false;
    let mut rest = sql;
    while let Some(c) = rest.chars().next() {
        if !inside || c != '\'' {
            if inside && rest.starts_with(placeholder) {
                return true;
            }
            if !inside && rest.starts_with(placeholder) {
                rest = &rest[placeholder.len()..];
                continue;
            }
        }
        if c == '\'' {
            inside = !inside;
        }
        rest = &rest[c.len_utf8()..];
    }
    false
}

pub(crate) fn starts_with_select(sql: &str) -> bool {
    let t = sql.trim_start();
    t.len() >= 6
        && t.is_char_boundary(6)
        && t[..6].eq_ignore_ascii_case("select")
        && t[6..].chars().next().map_or(true, |c| !(c.is_alphanumeric() || c == '_'))
}

pub(crate) fn quoted_literal(prefix: &str) -> String {
    format!("N'{}'", prefix.replace('\'', "''"))
}

/// Every problem with a rule, one sentence each. Empty when it may be saved.
pub fn check_find(rule: &FindRule) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    if rule.database.is_none() && rule.browser.is_none() {
        problems.push(NEEDS_A_FIND_RULE.to_string());
        return problems;
    }
    if let Some(db) = &rule.database {
        if !starts_with_select(&db.sql) {
            problems.push(ONE_SELECT.to_string());
        }
        if !db.sql.contains(PREFIX) || db.name_column.trim().is_empty() {
            problems.push(ONLY_TEST_RECORDS.to_string());
        }
        if prefix_inside_quotes(&db.sql) {
            problems.push(OUTSIDE_QUOTES.to_string());
        }
        if db.id_column.trim().is_empty() {
            problems.push(NEEDS_ID_COLUMN.to_string());
        }
        if db.sql.replace(PREFIX, "").contains(['{', '}']) {
            problems.push(ONLY_PREFIX_PLACEHOLDER.to_string());
        }
        if problems.is_empty() || !problems.iter().any(|p| p == ONE_SELECT) {
            // The statement as it will run must be a read the guard accepts.
            if let Err(said) = sql_with_prefix(&db.sql, "TESTPREFIX") {
                if !problems.contains(&said) {
                    problems.push(said);
                }
            }
        }
    }
    if let Some(actions) = &rule.browser {
        if actions.is_empty() {
            problems.push(BROWSER_ENDS_IN_A_CHECK.to_string());
        }
        if actions.len() > BROWSER_FIND_MAX {
            problems.push(format!(
                "A browser find rule is at most {BROWSER_FIND_MAX} actions."
            ));
        }
        if actions
            .iter()
            .any(|a| matches!(a, Action::SignIn { .. } | Action::UseComponent { .. }))
        {
            problems.push(
                "A browser find rule uses no sign_in and no use_component.".to_string(),
            );
        }
        if let Some(last) = actions.last() {
            let is_check = matches!(
                last,
                Action::ExpectVisible { .. }
                    | Action::ExpectText { .. }
                    | Action::ExpectContainsText { .. }
            );
            if !is_check {
                problems.push(BROWSER_ENDS_IN_A_CHECK.to_string());
            } else {
                let text = serde_json::to_string(last).unwrap_or_default();
                if !text.contains(PREFIX) {
                    problems.push(ONLY_TEST_RECORDS.to_string());
                }
            }
        }
    }
    problems.dedup();
    problems
}

/// The statement as it will run: every `{prefix}` becomes the prefix as a
/// quoted literal. The result must be a read.
pub fn sql_with_prefix(sql: &str, prefix: &str) -> Result<String, String> {
    if prefix_inside_quotes(sql) {
        return Err(OUTSIDE_QUOTES.to_string());
    }
    let out = sql.replace(PREFIX, &quoted_literal(prefix));
    match guard::classify(&out) {
        guard::Verdict::Read => Ok(out),
        guard::Verdict::Refused(why) => Err(why),
        _ => Err(ONE_SELECT.to_string()),
    }
}

fn replace_in(value: &mut Value, prefix: &str) {
    match value {
        Value::String(s) => {
            if s.contains(PREFIX) {
                *s = s.replace(PREFIX, prefix);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| replace_in(v, prefix)),
        Value::Object(map) => map.values_mut().for_each(|v| replace_in(v, prefix)),
        _ => {}
    }
}

/// `{prefix}` replaced in every string of every action.
pub fn browser_with_prefix(actions: &[Action], prefix: &str) -> Result<Vec<Action>, String> {
    actions
        .iter()
        .map(|a| {
            let mut value = serde_json::to_value(a)
                .map_err(|_| "the browser find rule could not be read".to_string())?;
            replace_in(&mut value, prefix);
            serde_json::from_value::<Action>(value)
                .map_err(|_| "the browser find rule could not be read".to_string())
        })
        .collect()
}

/// Reads sqlcmd's answer: a header line, a dash line, rows, a blank line and
/// the `(N rows affected)` footer. The first row whose name starts with the
/// prefix is the record.
pub fn first_test_record(
    text: &str,
    db: &DbFind,
    prefix: &str,
) -> Result<Option<FoundRecord>, String> {
    // An answer cut at the row cap opens with "rows: N (capped)" and has lost
    // its footer: the rows it kept are still read, and a record among them
    // is found.
    let first_line = text.lines().next().map(str::trim).unwrap_or("");
    let capped = first_line.starts_with("rows:") && first_line.ends_with("(capped)");
    let text = if capped { text.split_once('\n').map_or("", |(_, rest)| rest) } else { text };
    if !capped && query::rows_affected(text).is_none() {
        return Err("the find rule's answer carried no row count".to_string());
    }
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
    let header: Vec<String> = match lines.next() {
        Some(h) => h.split('\t').map(|c| c.trim().to_lowercase()).collect(),
        None => return Err("the find rule's answer carried no row count".to_string()),
    };
    let column = |name: &str| -> Result<usize, String> {
        header
            .iter()
            .position(|c| *c == name.trim().to_lowercase())
            .ok_or_else(|| format!("the find rule's answer has no column {}", name.trim()))
    };
    let id_at = column(&db.id_column)?;
    let name_at = column(&db.name_column)?;

    for (i, line) in lines.enumerate() {
        let trimmed = line.trim();
        if i == 0 && !trimmed.is_empty() && trimmed.chars().all(|c| c == '-' || c == '\t' || c == ' ')
        {
            continue;
        }
        // A blank line and the `(N rows affected)` footer are not rows; a
        // row that merely starts with "(" is one.
        if trimmed.is_empty() || query::rows_affected(trimmed).is_some() {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        let name = cells.get(name_at).map(|c| c.trim()).unwrap_or("");
        if name.is_empty() || name.eq_ignore_ascii_case("NULL") || !starts_with_prefix(name, prefix)
        {
            continue;
        }
        let id = cells.get(id_at).map(|c| c.trim().to_string());
        return Ok(Some(FoundRecord { id, name: name.to_string() }));
    }
    if capped {
        return Err("the find rule's answer was cut short before any test record - make its SELECT narrower".to_string());
    }
    Ok(None)
}

/// Where a find rule's SELECT is asked. Faked in tests.
pub trait RowSource {
    fn select(&self, sql: &str) -> impl std::future::Future<Output = Result<String, String>>;
}

/// The real one: `sqlcmd` through `run_query`, writes off.
pub struct SqlcmdRows<R: Runner> {
    pub runner: R,
    pub exe: PathBuf,
    pub conn: Connection,
}

impl<R: Runner> RowSource for SqlcmdRows<R> {
    async fn select(&self, sql: &str) -> Result<String, String> {
        query::run_query(&self.runner, &self.exe, &self.conn, false, sql)
            .await
            .map_err(|(_, said)| said)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DbOutcome {
    Found(FoundRecord),
    NotFound,
    CouldNotRun(String),
}

pub async fn find_in_database<S: RowSource>(rows: &S, db: &DbFind, prefix: &str) -> DbOutcome {
    let sql = match sql_with_prefix(&db.sql, prefix) {
        Ok(sql) => sql,
        Err(said) => return DbOutcome::CouldNotRun(said),
    };
    let text = match rows.select(&sql).await {
        Ok(text) => text,
        Err(said) => return DbOutcome::CouldNotRun(said),
    };
    match first_test_record(&text, db, prefix) {
        Ok(Some(found)) => DbOutcome::Found(found),
        Ok(None) => DbOutcome::NotFound,
        Err(said) => DbOutcome::CouldNotRun(said),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindChoice {
    Database,
    Browser,
    /// Only a database rule, and the database cannot be read: said as
    /// `DATABASE_OFF`.
    Refused,
}

pub fn choose(rule: &FindRule, database_ready: bool) -> FindChoice {
    if rule.database.is_some() && database_ready {
        FindChoice::Database
    } else if rule.browser.is_some() {
        FindChoice::Browser
    } else {
        FindChoice::Refused
    }
}
