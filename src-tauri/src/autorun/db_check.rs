//! The `expect_database` step: what a check may ask, how its placeholders
//! become quoted literals, how the answer text is read, how a verdict is
//! reached, and (`run_check`) the asking and asking again until it is right.

use crate::api_templates::gate::{self, SqlcmdStageDb, StageDb};
use crate::autorun::preconditions::NoDb;
use crate::db::RealRunner;
use crate::autorun::capture_redact::{redact_text, secret_name};
use crate::autorun::setup_find::quoted_literal;
use crate::browser::actions::{Action, ActionOutcome, HIDDEN_VALUE};
use crate::browser::expect::NOTED_SECRET;
use crate::browser::table;
use crate::db::guard::{self, Verdict as GuardVerdict};
use crate::db::query;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// May this action ask the database: an `expect_database`, or a component,
/// which may hold one? The one test every path that resolves the database
/// lazily reads.
pub fn may_ask(a: &Action) -> bool {
    matches!(a, Action::ExpectDatabase { .. } | Action::UseComponent { .. })
}

/// `sql` error text without the server's name: sqlcmd's `Msg 207, Level 16,
/// State 1, Server NAME, Line 1` header keeps its numbers and loses `Server`
/// and `Procedure` parts.
pub fn without_server(said: &str) -> String {
    match regex::Regex::new(r"(?i),\s*(?:Server|Procedure)\s+[^,\r\n]+") {
        Ok(re) => re.replace_all(said, "").into_owned(),
        Err(_) => said.to_string(),
    }
}

/// Said when the database refused the login or would not open the database:
/// sqlcmd's own words name the login and the database, so they are only logged.
pub const LOGIN_REFUSED: &str =
    "The database refused this step's sign-in or could not be opened. Settings → Logs has the details.";
/// Said when the login may not read what a check asks.
pub const READ_DENIED: &str =
    "The database login may not read what this step asks. Settings → Logs has the details.";

/// The fixed sentence for an error that names a login or a database, `None`
/// for an ordinary SQL error (which names only the query's own columns).
fn refused_sentence(said: &str) -> Option<&'static str> {
    let l = said.to_lowercase();
    let msg = |n: &str| l.contains(&format!("msg {n},"));
    if l.contains("login failed for user")
        || l.contains("cannot open database")
        || l.contains("not able to access the database")
        || msg("18456")
        || msg("4060")
        || msg("916")
    {
        Some(LOGIN_REFUSED)
    } else if l.contains("permission was denied") || msg("229") || msg("262") {
        Some(READ_DENIED)
    } else {
        None
    }
}

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
const OPAQUE_PLACEHOLDER: &str =
    "expect_database has a placeholder inside a comment or a quoted name - write it bare, like WHERE no = {{noted.req}}";
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
    /// `sensitive`: the wanted text came from a secret placeholder, so no
    /// sentence prints it, as for a secret column.
    Column { name: String, equals: Option<String>, matches: Option<String>, sensitive: bool },
}

/// The expectation an `expect_database` action states; `None` for any other
/// kind, an `Err` sentence when it states none or more than one.
pub fn try_expectation(a: &Action) -> Option<Result<Expect, String>> {
    let Action::ExpectDatabase { exists, count, at_least, at_most, column, equals, matches, sensitive, .. } = a else {
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
                        sensitive: *sensitive,
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

/// Where a placeholder sits in the SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Code,
    /// Inside a `'...'` literal.
    Literal,
    /// Inside a `--` or `/* */` comment.
    Comment,
    /// Inside a `[bracket]` or `"double-quoted"` identifier.
    Ident,
}

/// What `sql[..pos]` leaves open at `pos`.
fn ctx_at(sql: &str, pos: usize) -> Ctx {
    let mut ctx = Ctx::Code;
    let mut close = ' ';
    let mut chars = sql[..pos].chars().peekable();
    let mut last = ' ';
    while let Some(c) = chars.next() {
        match ctx {
            Ctx::Code => match c {
                '\'' => ctx = Ctx::Literal,
                '-' if chars.peek() == Some(&'-') => {
                    chars.next();
                    ctx = Ctx::Comment;
                    close = '\n';
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    ctx = Ctx::Comment;
                    close = '/';
                    last = ' ';
                }
                '[' => {
                    ctx = Ctx::Ident;
                    close = ']';
                }
                '"' => {
                    ctx = Ctx::Ident;
                    close = '"';
                }
                _ => {}
            },
            Ctx::Literal => {
                if c == '\'' {
                    ctx = Ctx::Code;
                }
            }
            Ctx::Comment => {
                if close == '\n' {
                    if c == '\n' {
                        ctx = Ctx::Code;
                    }
                } else if last == '*' && c == '/' {
                    ctx = Ctx::Code;
                    last = ' ';
                } else {
                    last = c;
                }
            }
            Ctx::Ident => {
                if c == close {
                    if chars.peek() == Some(&close) {
                        chars.next();
                    } else {
                        ctx = Ctx::Code;
                    }
                }
            }
        }
    }
    ctx
}

/// Every `{{...}}` in `sql` that names a setup, fixture or noted value:
/// byte range, the full name (`noted.req`) and where it sits.
fn located(sql: &str) -> Vec<(usize, usize, String, Ctx)> {
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
            out.push((start, end, name.to_string(), ctx_at(sql, start)));
            from = end;
        } else {
            // Not one of ours: its `{{` may be a stray one, ahead of a real
            // placeholder that the span just read has swallowed.
            from = start + 2;
        }
    }
    out
}

/// A component's check: its text inputs go into `sql` as quoted literals, so
/// none may be written inside a quote, a comment or a quoted name.
pub fn refuse_quoted_inputs(sql: &str, inputs: &[&str]) -> Result<(), String> {
    let mut from = 0;
    while let Some(open) = sql[from..].find("{{") {
        let start = from + open;
        let Some(close) = sql[start + 2..].find("}}") else { break };
        let end = start + 2 + close + 2;
        if inputs.contains(&sql[start + 2..end - 2].trim()) {
            match ctx_at(sql, start) {
                Ctx::Literal => return Err(QUOTED_PLACEHOLDER.to_string()),
                Ctx::Comment | Ctx::Ident => return Err(OPAQUE_PLACEHOLDER.to_string()),
                Ctx::Code => {}
            }
        }
        from = start + 2;
    }
    Ok(())
}

/// The placeholders a run binds: every one but those inside a quoted
/// literal, which is text (a saved check has none: `validate` refuses them).
fn placeholders(sql: &str) -> Vec<(usize, usize, String)> {
    located(sql).into_iter().filter(|f| f.3 != Ctx::Literal).map(|(s, e, n, _)| (s, e, n)).collect()
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

/// `sql` without its `--` line comments and `/* */` block comments, those
/// inside a quoted literal being text, not comments.
fn without_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut in_quote = false;
    while let Some(c) = chars.next() {
        if in_quote {
            out.push(c);
            in_quote = c != '\'';
        } else if c == '\'' {
            in_quote = true;
            out.push(c);
        } else if c == '-' && chars.peek() == Some(&'-') {
            for n in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut last = ' ';
            for n in chars.by_ref() {
                if last == '*' && n == '/' {
                    break;
                }
                last = n;
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
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
    // ORDER BY is read without the comments: `-- order by x` orders nothing.
    let bare = without_comments(sql);
    let all = located(sql);
    for (_, _, name, ctx) in &all {
        match ctx {
            Ctx::Literal => return Err(QUOTED_PLACEHOLDER.to_string()),
            Ctx::Comment | Ctx::Ident => return Err(OPAQUE_PLACEHOLDER.to_string()),
            Ctx::Code => {}
        }
        if !name.starts_with("noted.") && secret_name(name) {
            return Err(format!(
                "expect_database cannot put {{{{{name}}}}} in its SQL - it holds a secret and the SQL is logged"
            ));
        }
    }
    let mut probe = String::with_capacity(sql.len());
    let mut at = 0;
    for (start, end, _, _) in &all {
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
        if !re.is_match(&without_literals(&bare)) {
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
        Expect::Column { name, equals, matches, sensitive } => {
            let secret = *sensitive || secret_name(name);
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
                // The rows kept already pass the limit: more than that exist.
                if let (Some(w), None, None) = (at_most, equals, at_least) {
                    if n > *w as usize {
                        return Verdict::Fail(format!("expected {words}, found more than {n}"));
                    }
                }
                return Verdict::Fail(CAPPED_COUNT.to_string());
            }
            if ok {
                Verdict::Pass(format!("found {}", rows_word(n)))
            } else {
                Verdict::Fail(format!("expected {words}, found {n}"))
            }
        }
        Expect::Column { name, equals, matches, sensitive } => {
            judge_column(name, equals.as_deref(), matches.as_deref(), *sensitive, rows)
        }
    }
}

fn judge_column(name: &str, equals: Option<&str>, matches: Option<&str>, sensitive: bool, rows: &Rows) -> Verdict {
    let wanted_name = name.to_lowercase();
    let Some(at) = rows.columns.iter().position(|c| c.to_lowercase() == wanted_name) else {
        return Verdict::Fail(format!("no column named '{name}'; the query returned {}", rows.columns.join(", ")));
    };
    let printed = rows.columns[at].as_str();
    let secret = sensitive || secret_name(printed) || secret_name(name);
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

/// The database a check asks: whatever can answer a `SELECT` with the rows
/// as `sqlcmd` prints them, in a future that can move between threads. Not a
/// blanket impl over `StageDb`: `Runner::run`'s future is not provably `Send`
/// for every runner, so each database a run really asks says so itself.
pub trait DbRows: Send + Sync {
    fn select<'a>(&'a self, sql: &'a str) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;
}

/// The real database, asked through `sqlcmd`.
impl DbRows for SqlcmdStageDb<RealRunner> {
    fn select<'a>(&'a self, sql: &'a str) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(StageDb::select(self, sql))
    }
}

/// No database: every ask is refused with the sentence for none.
impl DbRows for NoDb {
    fn select<'a>(&'a self, sql: &'a str) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(StageDb::select(self, sql))
    }
}

/// Where a run's `expect_database` steps are asked.
#[derive(Clone)]
pub enum DbSource<'a> {
    /// The database to ask.
    Ready(&'a dyn DbRows),
    /// No database to ask: the sentence that says why.
    Missing(String),
    /// The Database Access module is off.
    ReadingOff,
}

/// A run that was given no database says so (`NO_DATABASE`).
impl Default for DbSource<'_> {
    fn default() -> Self {
        DbSource::Missing(NO_DATABASE.to_string())
    }
}

/// Said when a check runs and the active environment has no database.
pub const NO_DATABASE: &str =
    "This step checks the database, but no database is chosen for this environment on the AI Bridge tab";
/// Said when a check runs while the Database Access module is off.
pub const READING_OFF: &str = "This step checks the database, but Database Access is turned off";
const SECRET_NOTED: &str = "a value read from a password field cannot be used in a database check";

/// Carry out one `expect_database`: bind its `{{noted.*}}` placeholders from
/// `noted` as quoted literals, then ask, judge and ask again every
/// `POLL_MS` until the answer is right, `timeout_ms` has run out or `cancel`
/// is set. A query the database refuses fails at once; one that could not
/// reach it ends the wait early with the network sentence.
pub async fn run_check(
    src: &DbSource<'_>,
    a: &Action,
    noted: &BTreeMap<String, String>,
    timeout_ms: u32,
    cancel: Option<&AtomicBool>,
) -> ActionOutcome {
    let db = match src {
        DbSource::Ready(db) => *db,
        DbSource::Missing(why) => return ActionOutcome::failed(why.clone()),
        DbSource::ReadingOff => return ActionOutcome::failed(READING_OFF),
    };
    let Action::ExpectDatabase { sql, .. } = a else {
        return ActionOutcome::failed("this is not a database check");
    };
    let expect = match try_expectation(a) {
        Some(Ok(e)) => e,
        Some(Err(why)) => return ActionOutcome::failed(why),
        None => return ActionOutcome::failed("this is not a database check"),
    };
    for name in noted_names(sql) {
        if noted.get(&name).is_some_and(|v| v.starts_with(NOTED_SECRET)) {
            return ActionOutcome::failed(SECRET_NOTED);
        }
    }
    let bound = match bind(sql, |full| full.strip_prefix("noted.").and_then(|n| noted.get(n).cloned())) {
        Ok(b) => b,
        Err(why) => return ActionOutcome::failed(why),
    };
    let left = unbound(&bound);
    if !left.is_empty() {
        return ActionOutcome::failed(format!(
            "expect_database has no value yet for {}",
            left.join(", ")
        ));
    }
    let began = Instant::now();
    let limit = Duration::from_millis(u64::from(timeout_ms.min(MAX_TIMEOUT_MS)));
    loop {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return ActionOutcome::failed(crate::autorun::runner::AFTER_STOP);
        }
        let last = match db.select(&bound).await {
            Err(said) => {
                if said == query::DB_UNREACHABLE || query::is_unreachable(&said) || gate::unreachable(&said) {
                    return ActionOutcome::failed(query::DB_UNREACHABLE);
                }
                if let Some(sentence) = refused_sentence(&said) {
                    crate::applog::warn(format!("Auto Test: the database check was refused: {}", redact_text(&said)));
                    return ActionOutcome::failed(sentence);
                }
                return ActionOutcome::failed(without_server(&redact_text(&said)));
            }
            Ok(text) => match parse(&text) {
                Err(why) => return ActionOutcome::failed(why),
                Ok(rows) => match judge(&expect, &rows) {
                    Verdict::Pass(detail) => return ActionOutcome::passed(detail),
                    Verdict::Fail(detail) => detail,
                },
            },
        };
        let spent = began.elapsed();
        if spent >= limit {
            return ActionOutcome::failed(last);
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS).min(limit - spent)).await;
    }
}
