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
use std::time::Instant;

use serde_json::json;

use crate::activity_log::{self, Kind};
use super::guard::{self, Verdict};
use super::{schema, sqlcmd};
use sqlcmd::{Connection, Runner};

/// Said when no connection has been chosen. It names the card, because
/// that is the only thing the person reading it can act on.
pub const NO_CONNECTION: &str =
    "no database connection is chosen - pick one under Database Read Access on the AI Bridge tab";

/// Said when Your own database is chosen but no login is saved for it. The
/// choice has been made, so NO_CONNECTION's "pick one" would point at the
/// wrong control: what is missing is the login.
pub const NO_LOGIN_SAVED: &str =
    "no login is saved for this database yet - save one with its Edit button under Database Read Access on the AI Bridge tab";

/// Said when the statement would write and the switch for that is off.
/// Separate from the guard's read-only sentence: one says "this connection
/// cannot", the other says "you have not allowed it yet", and telling a
/// person to switch connections when the switch is the problem sends them
/// to the wrong control.
pub const WRITES_OFF: &str =
    "create, update and delete are switched off - turn them on under Database Read Access on the AI Bridge tab";

/// The connection the database `id` stands for, resolved from `store` now
/// (a login saved a moment ago is the one used), and a sqlcmd to run it
/// with. Each refusal is a sentence a person can act on: nothing chosen
/// (`NO_CONNECTION`, also for an id this build does not know, or no store
/// at all), no login saved, a store that cannot be read, a connection that
/// names no server or database, or no sqlcmd. None carries a credential.
pub fn ready(
    store: Option<&dyn super::SecretStore>,
    id: Option<&str>,
) -> Result<(Connection, std::path::PathBuf), String> {
    let nothing_chosen = || NO_CONNECTION.to_string();
    let store = store.ok_or_else(nothing_chosen)?;
    let id = id
        .map(str::trim)
        .filter(|id| super::credentials::is_known(store, id))
        .ok_or_else(nothing_chosen)?;
    // `own` with nothing saved HAS been chosen, so it gets its own sentence
    // - "pick one" would send the person back to a choice they already
    // made. A store that cannot be read keeps its own sentence too.
    let chosen = super::credentials::resolve(store, id)?.ok_or_else(|| NO_LOGIN_SAVED.to_string())?;
    // The error names the missing key and nothing else - the rest of that
    // string is a credential, and this sentence is shown to a person.
    let connection = sqlcmd::parse_connection(&chosen)
        .map_err(|why| format!("{why} - choose a connection under Database Read Access on the AI Bridge tab"))?;
    let exe = sqlcmd::sqlcmd_path().ok_or_else(|| sqlcmd::NOT_INSTALLED.to_string())?;
    Ok((connection, exe))
}

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

/// `{server}/{database}`, the way every activity record and app-log
/// summary below names the connection - never the user or the password.
fn conn_label(c: &Connection) -> String {
    format!("{}/{}", c.server, c.database)
}

/// A statement that never ran at all - refused by the guard, or shaped
/// like a write and stopped at one of the two write doors. Recorded for
/// the same reason a run is: a person looking for why an assistant "did
/// nothing" needs the attempt in the trail, not just the ones that got
/// through. The full statement goes to the activity log; `applog` gets a
/// short summary with no SQL in it at all.
fn record_refusal(c: &Connection, why: &str, sql: &str) {
    let conn = conn_label(c);
    activity_log::record(
        Kind::Db,
        json!({ "connection": conn, "verdict": "refused", "why": why, "sql": sql }),
    );
    crate::applog::info(format!("db query refused on {conn}: {why}"));
}

/// Every `(N row affected)` / `(N rows affected)` footer sqlcmd prints,
/// summed - a batch's own wrapper can print more than one. Separate from
/// `sqlcmd::BatchRun::rows`, which reads the wrapper's own markers rather
/// than sqlcmd's prose, because a plain `run_query` has no wrapper to read
/// them from.
pub fn rows_affected(stdout: &str) -> Option<i64> {
    let mut total = 0i64;
    let mut found = false;
    for line in stdout.lines() {
        let Some(inner) =
            line.trim().strip_prefix('(').and_then(|s| s.strip_suffix(" affected)"))
        else {
            continue;
        };
        let Some(n) = inner.split_whitespace().next().and_then(|s| s.parse::<i64>().ok()) else {
            continue;
        };
        total += n;
        found = true;
    }
    found.then_some(total)
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
            record_refusal(c, why, sql);
            return Err((400, why.clone()));
        }
        Verdict::Write => {
            if !writes_on {
                record_refusal(c, WRITES_OFF, sql);
                return Err((400, WRITES_OFF.to_string()));
            }
            if guard::access_for_user(&c.user) != guard::Access::DevWrites {
                record_refusal(c, guard::READ_ONLY_SENTENCE, sql);
                return Err((400, guard::READ_ONLY_SENTENCE.to_string()));
            }
        }
        Verdict::Read => {}
    }

    let (verdict_str, kind_title) =
        if matches!(verdict, Verdict::Write) { ("write", "Write") } else { ("read", "Read") };
    let conn = conn_label(c);

    // Recorded after the process returns, not before: the full statement
    // now only ever goes to the activity log, and that record needs to say
    // how the run ended, not just that it was attempted.
    let started = Instant::now();
    let result = sqlcmd::run_sql(r, exe, c, sql).await;
    let duration_ms = started.elapsed().as_millis() as u64;

    match result {
        Ok((text, capped)) => {
            let rows = rows_affected(&text);
            activity_log::record(
                Kind::Db,
                json!({
                    "connection": conn, "verdict": verdict_str, "sql": sql,
                    "ok": true, "rows": rows, "duration_ms": duration_ms,
                }),
            );
            let rows_note = rows.map(|n| format!(", {n} rows")).unwrap_or_default();
            crate::applog::info(format!("db query ({kind_title}) on {conn}: ok{rows_note}, {duration_ms} ms"));
            Ok(with_cap_note(&text, capped))
        }
        // Whatever the server said, redacted by `run_sql` on the way out.
        // 502 rather than 400: the statement was allowed and sent, and
        // something beyond this app answered.
        Err(said) => {
            activity_log::record(
                Kind::Db,
                json!({ "connection": conn, "verdict": verdict_str, "sql": sql, "ok": false, "duration_ms": duration_ms }),
            );
            crate::applog::info(format!("db query ({kind_title}) on {conn}: failed, {duration_ms} ms"));
            Err((502, format!("the database refused the statement: {said}")))
        }
    }
}

/// Several statements as one all-or-nothing transaction - see `db::batch`.
///
/// The same two write doors as `run_query`, asked of the batch as a whole:
/// one write anywhere in it makes it a write. Refusals, the writes switch
/// and the read-only connection answer exactly as they do for one
/// statement, with the refused statement named by its position.
///
/// The log keeps a batch that writes whole, every statement of it, and
/// says whether it was a dry run - a batch that ran for real is the thing
/// somebody may later need to find and undo.
pub async fn run_batch_query<R: Runner>(
    r: &R,
    exe: &Path,
    c: &Connection,
    writes_on: bool,
    statements: &[super::batch::BatchStatement],
    dry_run: bool,
) -> Result<String, (u16, String)> {
    use super::batch;
    let joined = || {
        statements
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{}) {}", i + 1, s.sql))
            .collect::<Vec<_>>()
            .join(" ")
    };
    // Classified with write access first, so the answer can tell "this
    // statement is refused everywhere" apart from the two write doors.
    let verdict = match batch::validate(statements, guard::Access::DevWrites) {
        Ok(v) => v,
        Err(why) => {
            record_refusal(c, &why, &joined());
            return Err((400, why));
        }
    };
    if verdict == Verdict::Write {
        if !writes_on {
            record_refusal(c, WRITES_OFF, &joined());
            return Err((400, WRITES_OFF.to_string()));
        }
        if guard::access_for_user(&c.user) != guard::Access::DevWrites {
            record_refusal(c, guard::READ_ONLY_SENTENCE, &joined());
            return Err((400, guard::READ_ONLY_SENTENCE.to_string()));
        }
    }

    let dry = if dry_run { ", dry run" } else { "" };
    let conn = conn_label(c);
    let n = statements.len();

    let started = Instant::now();
    let result = sqlcmd::run_batch(r, exe, c, statements, dry_run).await;
    let duration_ms = started.elapsed().as_millis() as u64;

    match result {
        Ok(run) => {
            let rows: i64 = run.rows.iter().flatten().sum();
            activity_log::record(
                Kind::Db,
                json!({
                    "connection": conn, "verdict": "batch", "sql": joined(), "dry_run": dry_run,
                    "ok": true, "rows": rows, "duration_ms": duration_ms,
                }),
            );
            crate::applog::info(format!("db batch ({n} statements{dry}) on {conn}: ok, {duration_ms} ms"));
            Ok(batch_answer(statements, &run, dry_run))
        }
        Err(said) => {
            activity_log::record(
                Kind::Db,
                json!({
                    "connection": conn, "verdict": "batch", "sql": joined(), "dry_run": dry_run,
                    "ok": false, "duration_ms": duration_ms,
                }),
            );
            crate::applog::info(format!("db batch ({n} statements{dry}) on {conn}: failed, {duration_ms} ms"));
            Err((502, said))
        }
    }
}

/// What a batch that ran to its end says back: how it ended first, then
/// each statement's row count, then whatever its SELECTs returned.
fn batch_answer(
    statements: &[super::batch::BatchStatement],
    run: &sqlcmd::BatchRun,
    dry_run: bool,
) -> String {
    use super::batch::Ended;
    let n = statements.len();
    let head = match (run.ended, dry_run) {
        (Some(Ended::RolledBack), true) => format!(
            "Dry run: all {n} statements ran and were rolled back - nothing was saved. Send the same statements without dry_run to save them."
        ),
        (Some(Ended::Saved), false) => format!("Saved: all {n} statements ran in one transaction."),
        // The wrapper always prints how it ended; a batch that exited
        // cleanly without saying so is reported as exactly that, never
        // guessed at.
        _ => "The batch finished, but its last line did not say whether it was saved - check the data before running it again.".to_string(),
    };
    let mut out = format!("{head}\n{}", super::batch::row_lines(statements, &run.rows));
    if !run.text.trim().is_empty() {
        out.push_str("\n\nOutput:\n");
        out.push_str(&with_cap_note(&run.text, run.capped));
    }
    out
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
/// topic with the ranked lookup. A name that turns out to be no table falls
/// through to the ranking - so an assistant can type either without knowing
/// which it typed.
///
/// The ranking looks in the `PeoplesHR` schema first: the HR databases also
/// hold `PeoplesHRDAP` copies of many tables, and `PeoplesHR` is the right
/// one. Only when nothing matches
/// there is every schema searched. Then one more statement reads the
/// details for the tables it picked - see `schema` for why that is two
/// round trips and not one.
pub async fn run_lookup<R: Runner>(
    r: &R,
    exe: &Path,
    c: &Connection,
    query: &str,
    limit: usize,
) -> Result<String, (u16, String)> {
    let query = query.trim();
    let conn = conn_label(c);

    // Timed and recorded once, after every read the lookup makes has
    // returned - not before, like the refusal-only line this replaced.
    // `run_lookup_inner` does the actual work and hands back how many rows
    // or tables it found, wherever that count is directly available.
    let started = Instant::now();
    let result = run_lookup_inner(r, exe, c, query, limit).await;
    let duration_ms = started.elapsed().as_millis() as u64;

    match &result {
        Ok((_, rows)) => {
            activity_log::record(
                Kind::Db,
                json!({
                    "connection": conn, "verdict": "lookup", "sql": query,
                    "ok": true, "rows": rows, "duration_ms": duration_ms,
                }),
            );
            crate::applog::info(format!("db lookup on {conn}: ok, {duration_ms} ms"));
        }
        Err(_) => {
            activity_log::record(
                Kind::Db,
                json!({ "connection": conn, "verdict": "lookup", "sql": query, "ok": false, "duration_ms": duration_ms }),
            );
            crate::applog::info(format!("db lookup on {conn}: failed, {duration_ms} ms"));
        }
    }

    result.map(|(text, _rows)| text)
}

/// The lookup's own work, split out so `run_lookup` can time and record it
/// exactly once regardless of which of the three answers below it hits.
/// The `Option<i64>` alongside the text is the row or table count for that
/// answer, wherever this function can say it directly - never guessed at.
async fn run_lookup_inner<R: Runner>(
    r: &R,
    exe: &Path,
    c: &Connection,
    query: &str,
    limit: usize,
) -> Result<(String, Option<i64>), (u16, String)> {
    if let Some(sql) = schema::describe_sql(query) {
        let tsv = read(r, exe, c, &sql).await?;
        let described = schema::render_describe(&tsv);
        if !described.is_empty() {
            // The described table's own column count.
            return Ok((described, Some(data_row_count(&tsv) as i64)));
        }
    }
    let preferred = crate::db_defaults::DEFAULT_SCHEMA_FILTER;
    let mut picked =
        schema::parse_ranked(&read(r, exe, c, &schema::lookup_sql(query, preferred, limit)).await?);
    if picked.is_empty() {
        picked = schema::parse_ranked(&read(r, exe, c, &schema::lookup_sql(query, "", limit)).await?);
    }
    if picked.is_empty() {
        // Zero tables matched - still a directly known count, not a guess.
        return Ok((schema::render_lookup(""), Some(0)));
    }
    let tsv = read(r, exe, c, &schema::detail_sql(query, &picked)).await?;
    // The number of tables the ranking picked, not the detail statement's
    // own row count (one table can carry several columns/foreign keys).
    Ok((schema::render_lookup(&tsv), Some(picked.len() as i64)))
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
