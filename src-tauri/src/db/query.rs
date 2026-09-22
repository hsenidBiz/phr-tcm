//! What the assistant's two database tools actually do, with the process
//! injected so all of it is testable without a server.
//!
//! The bridge routes in `ai_bridge.rs` are a thin shell over these: they
//! read the body, find the connection and the executable, and hand both
//! here. Everything that decides anything - which statements may run, what
//! is written to the log, what comes back when the server says no - lives
//! in this file, where a test can drive it with a fake `Runner`.
//!
//! The guard is not called here as a courtesy. `sqlcmd::run_sql` asks it
//! again itself, so the classification below is about ANSWERING well (the
//! switch is off; this connection cannot write) rather than about safety:
//! there is no path to the process that skips the gate, whatever this
//! file does.

use std::path::Path;

use super::guard::{self, Verdict};
use super::{schema, sqlcmd};
use sqlcmd::{Connection, Runner};

/// Said when no connection has been chosen. It names the card, because
/// that is the only thing the person reading it can act on.
pub const NO_CONNECTION: &str =
    "no database connection is chosen - pick one under Company database on the AI Bridge tab";

/// Said when the statement would write and the switch for that is off.
/// Separate from the guard's read-only sentence: one says "this connection
/// cannot", the other says "you have not allowed it yet", and telling a
/// person to switch connections when the switch is the problem sends them
/// to the wrong control.
pub const WRITES_OFF: &str =
    "create, update and delete are switched off - turn them on under Company database on the AI Bridge tab";

/// Tables a lookup returns when the call does not say.
pub const LOOKUP_DEFAULT: usize = 10;

/// And the most it will return however large a number is asked for.
pub const LOOKUP_MAX: usize = 30;

/// The `limit` a lookup body asked for, brought into range.
pub fn lookup_limit(asked: Option<i64>) -> usize {
    match asked {
        Some(n) => n.clamp(1, LOOKUP_MAX as i64) as usize,
        None => LOOKUP_DEFAULT,
    }
}

/// The line Settings -> Logs shows for one statement: what kind it was,
/// which server and database it went to, and the statement itself. A write
/// is written whole, because a write is the thing somebody may later need
/// to undo by hand; a read is cut at `READ_LOG_CHARS`, since a SELECT can
/// carry a page of values and the log is a trail, not a transcript.
///
/// The user and the password are never in it. Pure and separate from the
/// route so it can be asserted without a log file, the same shape as
/// `ai_bridge::describe_try`.
pub fn log_line(c: &Connection, verdict: &Verdict, sql: &str) -> String {
    let kind = if matches!(verdict, Verdict::Write) { "Write" } else { "Read" };
    // Flattened, because a multi-line statement would otherwise become
    // several log lines and only the first would look like one.
    let one_line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    let shown = if matches!(verdict, Verdict::Write) {
        one_line
    } else {
        cut(&one_line, READ_LOG_CHARS)
    };
    format!("db query ({kind}) on {}/{}: {shown}", c.server, c.database)
}

/// How much of a read's statement the log keeps.
const READ_LOG_CHARS: usize = 200;

fn cut(text: &str, at: usize) -> String {
    if text.chars().count() <= at {
        return text.to_string();
    }
    text.chars().take(at).collect()
}

/// The line Settings -> Logs shows for a statement that never ran at all -
/// refused by the guard, or shaped like a write and stopped at one of the
/// two write doors. Logged for the same reason a run is: a person looking
/// for why an assistant "did nothing" needs the attempt in the trail, not
/// just the ones that got through.
///
/// Always cut at `READ_LOG_CHARS`, whatever kind of statement it was - it
/// never reached sqlcmd, so there is no executed write to keep whole for.
fn refusal_log_line(c: &Connection, why: &str, sql: &str) -> String {
    let one_line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("db query refused on {}/{}: {why} - {}", c.server, c.database, cut(&one_line, READ_LOG_CHARS))
}

/// One statement from the assistant, if it may run at all.
///
/// The two write doors are both checked here, and both have to be open:
/// the person switched create/update/delete on in the app, AND the chosen
/// connection is one whose user may write. Either one shut is a 400 that
/// names which.
pub async fn run_query<R: Runner>(
    r: &R,
    exe: &Path,
    c: &Connection,
    writes_on: bool,
    sql: &str,
) -> Result<String, (u16, String)> {
    let verdict = guard::classify(sql);
    match &verdict {
        Verdict::Refused(why) => {
            crate::applog::info(refusal_log_line(c, why, sql));
            return Err((400, why.clone()));
        }
        Verdict::Write => {
            if !writes_on {
                crate::applog::info(refusal_log_line(c, WRITES_OFF, sql));
                return Err((400, WRITES_OFF.to_string()));
            }
            if guard::access_for_user(&c.user) != guard::Access::DevWrites {
                crate::applog::info(refusal_log_line(c, guard::READ_ONLY_SENTENCE, sql));
                return Err((400, guard::READ_ONLY_SENTENCE.to_string()));
            }
        }
        Verdict::Read => {}
    }

    // Logged before it runs, not after: a statement that hangs or takes
    // the connection down is exactly the one somebody needs to find.
    crate::applog::info(log_line(c, &verdict, sql));

    match sqlcmd::run_sql(r, exe, c, sql).await {
        Ok((text, capped)) => Ok(with_cap_note(&text, capped)),
        // Whatever the server said, redacted by `run_sql` on the way out.
        // 502 rather than 400: the statement was allowed and sent, and
        // something beyond this app answered.
        Err(said) => Err((502, format!("the database refused the statement: {said}"))),
    }
}

/// Puts the cap where an assistant reads it FIRST. `run_sql` says what it
/// had to cut after the rows, which is the right place for a person
/// scrolling and the wrong one for a reader that may stop early and
/// conclude it has the whole table.
///
/// `capped` comes from `sqlcmd::run_sql` itself, not from sniffing the
/// text for the word "capped" - a SELECT that happens to return that
/// literal in a value is not a cap, and treating it as one would fake a
/// notice nobody's row count agrees with.
fn with_cap_note(text: &str, capped: bool) -> String {
    if !capped {
        return text.to_string();
    }
    format!("rows: {} (capped)\n{text}", data_row_count(text))
}

/// The rows a reader would call data: not the header, not the dashes rule
/// sqlcmd draws under it, not a blank separator, not the "... " notice
/// this module or `sqlcmd::cap` appends, and not the "(N rows affected)"
/// footer sqlcmd signs off with - counting any of those as a row is what
/// let the old count run past the truth.
fn data_row_count(text: &str) -> usize {
    text.lines()
        .skip(1) // the header
        .filter(|l| {
            let trimmed = l.trim();
            !trimmed.is_empty()
                && !schema::is_rule(l)
                && !schema::is_footer(l)
                && !l.starts_with("... ")
        })
        .count()
}

/// The tables and columns behind some words.
///
/// A bare table name is answered with that table's whole column list; a
/// topic with the ranked lookup. Both are one SELECT, and a name that
/// turns out to be no table falls through to the ranking - so an
/// assistant can type either without knowing which it typed.
pub async fn run_lookup<R: Runner>(
    r: &R,
    exe: &Path,
    c: &Connection,
    query: &str,
    limit: usize,
) -> Result<String, (u16, String)> {
    let query = query.trim();
    crate::applog::info(format!(
        "db lookup on {}/{}: {}",
        c.server,
        c.database,
        cut(query, READ_LOG_CHARS)
    ));

    if let Some(sql) = schema::describe_sql(query) {
        let tsv = read(r, exe, c, &sql).await?;
        let described = schema::render_describe(&tsv);
        if !described.is_empty() {
            return Ok(described);
        }
    }
    let tsv = read(r, exe, c, &schema::lookup_sql(query, "", limit)).await?;
    Ok(schema::render_lookup(&tsv))
}

/// One of the lookup's own statements. Its failures read differently from
/// a statement the assistant wrote: nothing it sent is wrong, the database
/// simply could not be read.
async fn read<R: Runner>(
    r: &R,
    exe: &Path,
    c: &Connection,
    sql: &str,
) -> Result<String, (u16, String)> {
    sqlcmd::run_sql(r, exe, c, sql)
        .await
        // The lookup's own statements already cap themselves with `TOP`,
        // so whether sqlcmd's own row/character cap fired is not something
        // an assistant reading `render_lookup`/`render_describe` needs.
        .map(|(text, _capped)| text)
        .map_err(|said| (502, format!("the database could not be read: {said}")))
}
