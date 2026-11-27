//! The pure half of the `expect_database` step: what a check may ask,
//! how its placeholders become quoted literals, how the answer text is read
//! and how a verdict is reached. The runner (not this module) owns the
//! connection, the polling and the clock.

use crate::autorun::capture_redact::secret_name;
use crate::autorun::setup_find::quoted_literal;
use crate::browser::actions::{Action, HIDDEN_VALUE};
use crate::browser::table;
use crate::db::guard::{self, Verdict as GuardVerdict};
use crate::db::query;

/// The longest a check may keep asking before it fails.
pub const MAX_TIMEOUT_MS: u32 = 60_000;
/// How long the runner waits between two asks.
pub const POLL_MS: u64 = 500;

const NEEDS_SQL: &str = "expect_database has an empty sql - write the SELECT to run";
const NO_EXPECTATION: &str =
    "expect_database says nothing to check - give exactly one of exists, count / at_least / at_most, or column with equals / matches";
const TWO_EXPECTATIONS: &str =
    "expect_database has two expectations - give exactly one of exists, count / at_least / at_most, or column with equals / matches";
const COLUMN_ONE: &str = "expect_database column takes exactly one of equals or matches";
const COLUMN_NAMED: &str = "expect_database equals / matches need a column to read";
const COUNT_ONE: &str = "expect_database takes one of count, at_least or at_most";
const QUOTED_PLACEHOLDER: &str =
    "expect_database has a placeholder inside quotes - write it bare, like WHERE no = {{noted.req}}; the runner quotes the value";
const NEEDS_ORDER_BY: &str =
    "expect_database reads the first row of its answer, so the SELECT needs an ORDER BY to make that row certain";
const READ_ONLY: &str = "expect_database only reads - the SQL must be one SELECT";
const CAPPED_COUNT: &str =
    "the answer was cut at the row cap, so the rows cannot be counted - make the SELECT a COUNT(*) and check its column";

/// What a check expects of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    Exists(bool),
    Count { equals: Option<u32>, at_least: Option<u32>, at_most: Option<u32> },
    Column { name: String, equals: Option<String>, matches: Option<String> },
}

/// The expectation an `expect_database` action states; `None` for any other
/// kind, an `Err` sentence when it states none or more than one.
pub fn try_expectation(a: &Action) -> Option<Result<Expect, String>> {
    let Action::ExpectDatabase { exists, count, at_least, at_most, column, equals, matches, .. } = a else {
        return None;
    };
    let has_count = count.is_some() || at_least.is_some() || at_most.is_some();
    let has_column = column.is_some() || equals.is_some() || matches.is_some();
    let groups = usize::from(exists.is_some()) + usize::from(has_count) + usize::from(has_column);
    Some(match groups {
        0 => Err(NO_EXPECTATION.to_string()),
        1 => {
            if let Some(e) = exists {
                Ok(Expect::Exists(*e))
            } else if has_count {
                table::row_count(*count, *at_least, *at_most)
                    .map_err(|_| COUNT_ONE.to_string())
                    .map(|_| Expect::Count { equals: *count, at_least: *at_least, at_most: *at_most })
            } else {
                match column {
                    None => Err(COLUMN_NAMED.to_string()),
                    Some(c) if c.trim().is_empty() => Err(COLUMN_NAMED.to_string()),
                    Some(c) if equals.is_some() != matches.is_some() => Ok(Expect::Column {
                        name: c.trim().to_string(),
                        equals: equals.clone(),
                        matches: matches.clone(),
                    }),
                    Some(_) => Err(COLUMN_ONE.to_string()),
                }
            }
        }
        _ => Err(TWO_EXPECTATIONS.to_string()),
    })
}

/// The expectation of an `expect_database` action, `None` for another kind
/// or one that states none or several (`validate` says why).
pub fn expectation(a: &Action) -> Option<Expect> {
    try_expectation(a).and_then(Result::ok)
}

/// Every `{{...}}` in `sql` that names a setup, fixture or noted value:
/// byte range and the full name (`noted.req`).
fn placeholders(sql: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = sql[from..].find("{{") {
        let start = from + open;
        let Some(close) = sql[start + 2..].find("}}") else { break };
        let end = start + 2 + close + 2;
        let name = sql[start + 2..end - 2].trim();
        let named = name.split_once('.').is_some_and(|(kind, rest)| {
            matches!(kind, "setup" | "fixture" | "noted") && !rest.is_empty()
        });
        if named {
            out.push((start, end, name.to_string()));
        }
        from = end;
    }
    out
}

/// The `{{noted.<name>}}` names, in the order they appear.
pub fn noted_names(sql: &str) -> Vec<String> {
    placeholders(sql).into_iter().filter_map(|(_, _, n)| n.strip_prefix("noted.").map(str::to_string)).collect()
}

/// `sql` with each placeholder `lookup` answers (by its full name) replaced
/// by the value as a quoted literal. One it does not answer stays as written.
pub fn bind(sql: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::with_capacity(sql.len());
    let mut at = 0;
    for (start, end, name) in placeholders(sql) {
        out.push_str(&sql[at..start]);
        match lookup(&name) {
            Some(v) => out.push_str(&quoted_literal(&v)),
            None => out.push_str(&sql[start..end]),
        }
        at = end;
    }
    out.push_str(&sql[at..]);
    Ok(out)
}

/// The placeholders still present, as written.
pub fn unbound(sql: &str) -> Vec<String> {
    placeholders(sql).into_iter().map(|(s, e, _)| sql[s..e].to_string()).collect()
}

/// `sql` without the text of its quoted literals.
fn without_literals(sql: &str) -> String {
    let mut in_quote = false;
    let mut out = String::new();
    for c in sql.chars() {
        if c == '\'' {
            in_quote = !in_quote;
        } else if !in_quote {
            out.push(c);
        }
    }
    out
}

/// Is a check's saved shape one the runner may carry out?
pub fn validate(sql: &str, e: Result<Expect, String>, timeout_ms: Option<u32>) -> Result<(), String> {
    if sql.trim().is_empty() {
        return Err(NEEDS_SQL.to_string());
    }
    let e = e?;
    if let Expect::Column { matches: Some(p), .. } = &e {
        crate::browser::pattern::check_pattern("expect_database", p)?;
        crate::browser::pattern::compiled(p)
            .map_err(|why| format!("expect_database \"matches\" is not a pattern: {why}"))?;
    }
    if timeout_ms.is_some_and(|t| t > MAX_TIMEOUT_MS) {
        return Err(format!("expect_database waits at most {MAX_TIMEOUT_MS} ms"));
    }
    let found = placeholders(sql);
    for (start, _, _) in &found {
        if sql[..*start].matches('\'').count() % 2 == 1 {
            return Err(QUOTED_PLACEHOLDER.to_string());
        }
    }
    let mut probe = String::with_capacity(sql.len());
    let mut at = 0;
    for (start, end, _) in &found {
        probe.push_str(&sql[at..*start]);
        probe.push_str("N'x'");
        at = *end;
    }
    probe.push_str(&sql[at..]);
    match guard::classify(&probe) {
        GuardVerdict::Read => {}
        GuardVerdict::Refused(why) => return Err(why),
        _ => return Err(READ_ONLY.to_string()),
    }
    if matches!(e, Expect::Column { .. }) {
        let re = regex::Regex::new(r"(?i)\border\s+by\b").map_err(|x| x.to_string())?;
        if !re.is_match(&without_literals(sql)) {
            return Err(NEEDS_ORDER_BY.to_string());
        }
    }
    Ok(())
}

/// The answer to a check's query, read from the `sqlcmd` text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// The answer was cut at the row cap: it holds fewer rows than exist.
    pub capped: bool,
}

/// Read the answer text as `setup_find::first_test_record` does: an optional
/// `rows: N (capped)` line, a tab-separated header, a dash line, the rows and
/// a `(N rows affected)` footer.
pub fn parse(text: &str) -> Result<Rows, String> {
    let first = text.lines().next().map(str::trim).unwrap_or("");
    let capped = first.starts_with("rows:") && first.ends_with("(capped)");
    let body = if capped { text.split_once('\n').map_or("", |(_, rest)| rest) } else { text };
    if !capped && query::rows_affected(body).is_none() {
        return Err("the database's answer carried no row count".to_string());
    }
    let mut lines = body.lines().skip_while(|l| l.trim().is_empty());
    let columns: Vec<String> = match lines.next() {
        Some(h) => h.split('\t').map(|c| c.trim().to_string()).collect(),
        None => return Err("the database's answer carried no row count".to_string()),
    };
    let mut rows = Vec::new();
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
    Ok(Rows { columns, rows, capped })
}

/// What a check made of the answer; each carries the detail sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass(String),
    Fail(String),
}

fn rows_word(n: usize) -> String {
    if n == 1 { "1 row".to_string() } else { format!("{n} rows") }
}

/// What the check wants, in words for the report. A secret column's wanted
/// value is never printed, and the SQL never is.
pub fn describe(e: &Expect) -> String {
    match e {
        Expect::Exists(true) => "a row".to_string(),
        Expect::Exists(false) => "no row".to_string(),
        Expect::Count { equals: Some(n), .. } => rows_word(*n as usize),
        Expect::Count { at_least: Some(n), .. } => format!("at least {}", rows_word(*n as usize)),
        Expect::Count { at_most: Some(n), .. } => format!("at most {}", rows_word(*n as usize)),
        Expect::Count { .. } => "a count".to_string(),
        Expect::Column { name, equals, matches } => {
            let secret = secret_name(name);
            match (equals, matches) {
                (Some(v), _) => format!("{name} is {}", if secret { HIDDEN_VALUE.to_string() } else { format!("'{v}'") }),
                (_, Some(p)) => {
                    format!("{name} matches {}", if secret { HIDDEN_VALUE.to_string() } else { format!("'{p}'") })
                }
                _ => name.clone(),
            }
        }
    }
}

/// Judge the answer against what the check expects.
pub fn judge(e: &Expect, rows: &Rows) -> Verdict {
    let n = rows.rows.len();
    match e {
        Expect::Exists(true) => {
            if n > 0 {
                Verdict::Pass("a row was found".into())
            } else {
                Verdict::Fail("expected a row, found none".into())
            }
        }
        Expect::Exists(false) => {
            if n == 0 {
                Verdict::Pass("no row was found".into())
            } else {
                Verdict::Fail(format!("expected no row, found {n}"))
            }
        }
        Expect::Count { equals, at_least, at_most } => {
            let (ok, words) = match (equals, at_least, at_most) {
                (Some(w), _, _) => (n == *w as usize, rows_word(*w as usize)),
                (_, Some(w), _) => (n >= *w as usize, format!("at least {}", rows_word(*w as usize))),
                (_, _, Some(w)) => (n <= *w as usize, format!("at most {}", rows_word(*w as usize))),
                _ => return Verdict::Fail(COUNT_ONE.to_string()),
            };
            if rows.capped && !(at_least.is_some() && ok) {
                return Verdict::Fail(CAPPED_COUNT.to_string());
            }
            if ok {
                Verdict::Pass(format!("found {}", rows_word(n)))
            } else {
                Verdict::Fail(format!("expected {words}, found {n}"))
            }
        }
        Expect::Column { name, equals, matches } => judge_column(name, equals.as_deref(), matches.as_deref(), rows),
    }
}

fn judge_column(name: &str, equals: Option<&str>, matches: Option<&str>, rows: &Rows) -> Verdict {
    let Some(at) = rows.columns.iter().position(|c| c.eq_ignore_ascii_case(name)) else {
        return Verdict::Fail(format!("no column named '{name}'; the query returned {}", rows.columns.join(", ")));
    };
    let printed = rows.columns[at].as_str();
    let secret = secret_name(printed) || secret_name(name);
    let Some(row) = rows.rows.first() else {
        return Verdict::Fail(if secret {
            format!("expected {printed} to hold {HIDDEN_VALUE}, found no row")
        } else {
            format!("expected {printed} {}, found no row", wanted(equals, matches))
        });
    };
    let cell = row.get(at).map(String::as_str).unwrap_or("");
    let ok = match (equals, matches) {
        (Some(w), _) => cell == w,
        (_, Some(p)) => crate::browser::pattern::compiled(p).map(|re| re.is_match(cell)).unwrap_or(false),
        _ => false,
    };
    if ok {
        return Verdict::Pass(if secret {
            format!("{printed} holds {HIDDEN_VALUE}")
        } else {
            format!("{printed} is {}", found(cell))
        });
    }
    Verdict::Fail(if secret {
        format!("{printed} does not hold {HIDDEN_VALUE}")
    } else {
        format!("expected {printed} {}, found {}", wanted(equals, matches), found(cell))
    })
}

fn wanted(equals: Option<&str>, matches: Option<&str>) -> String {
    match (equals, matches) {
        (Some(w), _) => format!("'{w}'"),
        (_, Some(p)) => format!("to match '{p}'"),
        _ => String::new(),
    }
}

fn found(cell: &str) -> String {
    if cell == "NULL" {
        "NULL".to_string()
    } else {
        format!("'{cell}'")
    }
}
