//! Several statements as one all-or-nothing transaction.
//!
//! One statement per call cannot change four tables safely: the second
//! UPDATE can fail after the first has landed, and "check the row count,
//! then run the next" is a promise an assistant keeps by hand. A batch is
//! the same statements sent together, wrapped BY THE APP in a transaction
//! that aborts on any error, with each statement's row count checked
//! against what the caller expected before the next one runs. A dry run is
//! the whole thing, then rolled back.
//!
//! The guard does not change for this. Every statement is classified on
//! its own, exactly as a single `db_query` statement is, so an assistant
//! still can never send BEGIN, COMMIT, DECLARE or a second statement
//! itself: the only transaction control in a batch is the fixed wrapper
//! `compose` writes around statements that have already passed. Two
//! checks are added for statements that share a batch - see `validate`.
//!
//! `sqlcmd::run_batch` is the only caller that runs one, and it validates
//! again itself, the same way `run_sql` asks the guard itself.

use super::guard::{self, Access, Verdict};

/// The most statements one batch may hold.
pub const MAX_BATCH_STATEMENTS: usize = 50;

/// What the wrapper prints to report a statement's row count and how the
/// batch ended. Chosen so no result set is likely to produce it.
pub const MARK: &str = "tcm-batch";

/// The variables the wrapper declares. A statement that names one is
/// refused, so nothing but the wrapper ever sets them.
const RESERVED_PREFIX: &str = "@tcm_";

/// One statement in a batch, and how many rows it must affect for the
/// batch to go on. None means any number is fine.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchStatement {
    pub sql: String,
    pub expect_rows: Option<i64>,
}

/// Checks every statement and says what the batch is: a Write if any
/// statement writes, a Read otherwise. The first problem found is the
/// answer, naming the statement by its position, and nothing runs.
///
/// On top of the guard's own verdict, a statement in a batch must be
/// lexically closed (see `guard::lexically_closed`) and must not name the
/// wrapper's variables. The whole batch is held to the guard's size limit,
/// not just each statement.
pub fn validate(statements: &[BatchStatement], access: Access) -> Result<Verdict, String> {
    if statements.is_empty() {
        return Err("the batch has no statements".to_string());
    }
    if statements.len() > MAX_BATCH_STATEMENTS {
        return Err(format!(
            "a batch holds at most {MAX_BATCH_STATEMENTS} statements: split this one"
        ));
    }
    let total: usize = statements.iter().map(|s| s.sql.chars().count()).sum();
    if total > guard::MAX_SQL_CHARS {
        return Err(format!(
            "the batch is longer than {} characters in all: send a smaller one",
            guard::MAX_SQL_CHARS
        ));
    }
    let mut writes = false;
    for (i, s) in statements.iter().enumerate() {
        let n = i + 1;
        match guard::allowed(&s.sql, access) {
            Ok(Verdict::Write) => writes = true,
            Ok(_) => {}
            Err(why) => return Err(format!("statement {n}: {why}")),
        }
        if !guard::lexically_closed(&s.sql) {
            return Err(format!(
                "statement {n} ends inside a string, a quoted name or a comment: close it"
            ));
        }
        if s.sql.to_ascii_lowercase().contains(RESERVED_PREFIX) {
            return Err(format!(
                "statement {n} names a variable starting with {RESERVED_PREFIX}, which the batch keeps for itself"
            ));
        }
        if matches!(s.expect_rows, Some(n) if n < 0) {
            return Err(format!("statement {n}: expect_rows cannot be negative"));
        }
    }
    Ok(if writes { Verdict::Write } else { Verdict::Read })
}

/// The one batch sqlcmd is given, and the line each statement starts on
/// (1-based), so a server error's line number can be put back in terms of
/// the statement that caused it.
///
/// `SET XACT_ABORT ON` makes any error - a constraint, a conversion, the
/// row check's own THROW - abort the batch and roll the transaction back.
/// Each statement is followed at once by its row count: nothing may run
/// between a statement and `@@ROWCOUNT`, or the count is of something else.
pub fn compose(statements: &[BatchStatement], dry_run: bool) -> (String, Vec<usize>) {
    let mut sql = String::from(
        "SET NOCOUNT ON;\nSET XACT_ABORT ON;\nDECLARE @tcm_rows INT, @tcm_msg NVARCHAR(400);\nBEGIN TRANSACTION;\n",
    );
    let mut starts = Vec::with_capacity(statements.len());
    for (i, s) in statements.iter().enumerate() {
        let n = i + 1;
        starts.push(sql.lines().count() + 1);
        // Its own terminator comes off and the batch's goes on, on a line
        // of its own, so a trailing `--` comment cannot swallow it.
        let body = s.sql.trim().trim_end_matches(|c: char| c == ';' || c.is_whitespace());
        sql.push_str(body);
        sql.push_str("\n;\n");
        sql.push_str(&format!(
            "SET @tcm_rows = @@ROWCOUNT;\nPRINT CONCAT(N'{MARK}|{n}|', @tcm_rows);\n"
        ));
        if let Some(want) = s.expect_rows {
            sql.push_str(&format!(
                "IF @tcm_rows <> {want} BEGIN SET @tcm_msg = CONCAT(N'statement {n} affected ', @tcm_rows, N' rows, expected {want}'); THROW 50000, @tcm_msg, 1; END;\n"
            ));
        }
    }
    sql.push_str(if dry_run { "ROLLBACK TRANSACTION;\n" } else { "COMMIT TRANSACTION;\n" });
    sql.push_str(&format!(
        "PRINT N'{MARK}|end|{}';",
        if dry_run { "rolled-back" } else { "saved" }
    ));
    (sql, starts)
}

/// How a batch ended, as its own last line said.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ended {
    Saved,
    RolledBack,
}

/// What sqlcmd printed, split into the wrapper's report and everything
/// else (the result sets of any SELECTs in the batch, in order).
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// Each statement's row count, by position; None for one that never ran.
    pub rows: Vec<Option<i64>>,
    pub ended: Option<Ended>,
    pub rest: String,
}

pub fn read_report(stdout: &str, count: usize) -> Report {
    let mut rows = vec![None; count];
    let mut ended = None;
    let mut rest: Vec<&str> = Vec::new();
    let prefix = format!("{MARK}|");
    for line in stdout.lines() {
        let Some(tail) = line.trim().strip_prefix(&prefix) else {
            rest.push(line);
            continue;
        };
        match tail.split_once('|') {
            Some(("end", "saved")) => ended = Some(Ended::Saved),
            Some(("end", "rolled-back")) => ended = Some(Ended::RolledBack),
            Some((n, value)) => {
                if let (Ok(n), Ok(value)) = (n.parse::<usize>(), value.trim().parse::<i64>()) {
                    if (1..=count).contains(&n) {
                        rows[n - 1] = Some(value);
                    }
                }
            }
            None => rest.push(line),
        }
    }
    let rest = rest.join("\n").trim_matches('\n').trim_end().to_string();
    Report { rows, ended, rest }
}

/// A server error's "Line N" is a line of the composed batch, which nobody
/// wrote. Said as the statement it falls in instead.
pub fn name_the_statement(said: &str, starts: &[usize]) -> String {
    said.lines()
        .map(|line| {
            let Some(at) = line.find(", Line ") else {
                return line.to_string();
            };
            let digits: String =
                line[at + 7..].chars().take_while(|c| c.is_ascii_digit()).collect();
            let Ok(n) = digits.parse::<usize>() else {
                return line.to_string();
            };
            match starts.iter().rposition(|&s| s <= n) {
                Some(k) => format!(
                    "{}, in statement {}{}",
                    &line[..at],
                    k + 1,
                    &line[at + 7 + digits.len()..]
                ),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lines that say what each statement did, one per statement.
pub fn row_lines(statements: &[BatchStatement], rows: &[Option<i64>]) -> String {
    statements
        .iter()
        .zip(rows)
        .enumerate()
        .map(|(i, (s, got))| {
            let what = match (got, s.expect_rows) {
                (Some(got), Some(want)) if *got == want => format!("{got} rows (expected {want})"),
                (Some(got), Some(want)) => format!("{got} rows - expected {want}"),
                (Some(got), None) => format!("{got} rows"),
                (None, _) => "did not run".to_string(),
            };
            format!("{}. {what} - {}", i + 1, preview(&s.sql))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn preview(sql: &str) -> String {
    let one_line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= 80 {
        return one_line;
    }
    format!("{}...", one_line.chars().take(80).collect::<String>())
}
