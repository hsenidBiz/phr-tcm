//! The database tools (`db_lookup`, `db_query`) and the staging database the API flows use.

use super::*;

/// The two things both database routes need before they can do anything:
/// a connection the person chose, and a sqlcmd to run it with. Neither is
/// a failure of the call - both are 409, "the app is not set up for this
/// yet" - and each says which of the two is missing.
pub(crate) fn db_ready(
    ctx: &BridgeContext,
) -> Result<(crate::db::Connection, std::path::PathBuf), (u16, String)> {
    // Resolved now, not when the context was pushed: a login saved since
    // is the one this call signs in with. An id this build does not know -
    // one saved for a preset a later release removed - is nothing chosen
    // too: the person's next step is the same, pick a database.
    crate::db::query::ready(ctx.db_secrets.as_deref(), ctx.db_id.as_deref()).map_err(|why| (409, why))
}

/// Said when a template on a flow is proven or run with no database chosen:
/// its gate is a set of database checks, so without one it cannot run.
pub const FLOW_NEEDS_DB: &str =
    "this template belongs to a flow, and flow checks need a database: choose one on the AI Bridge tab";

/// Said when a flow check would run while the Database Access module is
/// off: every check is an assistant-written read of that database, so none
/// runs until the module is on again.
pub const FLOW_NEEDS_READING: &str =
    "flow checks read the company database: turn the Database Access module on in Settings";

/// The chosen database as the place flow checks are asked, with
/// `db_ready`'s refusals as they are - what saving a flow and a flow's
/// progress answer with. Refused first while the Database Access module is
/// off, whatever the disabled list says, before any connection is looked
/// at.
pub(crate) fn stage_db(
    ctx: &BridgeContext,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    stage_db_for(ctx, &crate::modules::current())
}

/// `stage_db` with the modules that are on passed in, so both answers are
/// testable. The module is the switch for reading the database; a
/// `db_query` entry among the disabled tools refuses too, as a second door.
pub fn stage_db_for(
    ctx: &BridgeContext,
    on: &std::collections::BTreeSet<crate::modules::Module>,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    if !on.contains(&crate::modules::Module::DbAccess) || ctx.disabled_tools.iter().any(|t| t == "db_query") {
        return Err((409, FLOW_NEEDS_READING.to_string()));
    }
    let (conn, exe) = db_ready(ctx)?;
    Ok(crate::api_templates::gate::SqlcmdStageDb { runner: crate::db::RealRunner, exe, conn })
}

/// The database a prove or a run of a template on a flow gates on. With
/// nothing chosen the answer says why a template needs one at all
/// (`FLOW_NEEDS_DB`), with `db_ready`'s own status; the Database Access
/// module being off is `stage_db`'s `FLOW_NEEDS_READING`; every other
/// refusal - no login saved, a store that cannot be read, no sqlcmd - is
/// `db_ready`'s, unchanged.
pub fn real_stage_db(
    ctx: &BridgeContext,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    real_stage_db_for(ctx, &crate::modules::current())
}

/// `real_stage_db` with the modules that are on passed in.
pub fn real_stage_db_for(
    ctx: &BridgeContext,
    on: &std::collections::BTreeSet<crate::modules::Module>,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    stage_db_for(ctx, on).map_err(|(status, why)| {
        if why == crate::db::query::NO_CONNECTION {
            (status, FLOW_NEEDS_DB.to_string())
        } else {
            (status, why)
        }
    })
}

// ---- Crystal Reports -----------------------------------------------------
//
// Every call below runs the report library on a blocking thread: it may
// render a PDF while holding the open report's lock, and a panic inside the
// bundle must come back as an answer, never take the bridge down.

/// A body's JSON, parsed once, or the refusal that names what could not be
/// read. Both database routes need more than one field out of the same
/// body (`db_lookup` reads `query` and `limit`), so parsing happens here
/// and every field after this is read from the one `Value`.
pub(crate) fn db_body(body: &str, shape: &str) -> Result<serde_json::Value, (u16, String)> {
    serde_json::from_str(body)
        .map_err(|e| (400, format!("that is not readable JSON: {e}. Expected {shape}.")))
}

/// One named string out of an already-parsed body, trimmed, or the
/// refusal that says what the body should have carried. Separate from
/// `body_field` because these two routes want the empty string and the
/// missing key to read the same way: neither is a question.
pub(crate) fn db_body_text(parsed: &serde_json::Value, key: &str, shape: &str) -> Result<String, (u16, String)> {
    let text = parsed.get(key).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err((400, format!("this call needs a \"{key}\". Expected {shape}.")));
    }
    Ok(text)
}

/// The tables and columns behind some words, or one table's own columns.
pub(crate) async fn db_lookup(ctx: &BridgeContext, body: &str) -> (u16, String) {
    const SHAPE: &str = "{ \"query\": \"leave request\", \"limit\": 10 }";
    let parsed = match db_body(body, SHAPE) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let query = match db_body_text(&parsed, "query", SHAPE) {
        Ok(q) => q,
        Err(refused) => return refused,
    };
    let limit = crate::db::query::lookup_limit(parsed.get("limit").and_then(|l| l.as_i64()));
    let (connection, exe) = match db_ready(ctx) {
        Ok(ready) => ready,
        Err(refused) => return refused,
    };
    // Kept here, not in `run_lookup`, so the lookup itself stays a plain
    // function of what sqlcmd answers - and only an answer is kept, never
    // a failure, so a server that was briefly unreachable is asked again.
    let key = crate::cache::keys::db_lookup(
        &connection.server,
        &connection.database,
        limit,
        &query.to_lowercase(),
    );
    if let Some(hit) = crate::cache::session_fresh::<String>(&key, crate::cache::keys::DB_LOOKUP_TTL) {
        return (200, hit);
    }
    match crate::db::query::run_lookup(
        &crate::db::RealRunner,
        &exe,
        &connection,
        &query,
        limit,
    )
    .await
    {
        Ok(text) => {
            crate::cache::session_put(&key, text.clone());
            (200, text)
        }
        Err(refused) => refused,
    }
}

/// The statements of a batch body: each either `{ "sql": ..., "expect_rows":
/// N }` or a bare string. Anything else is named back rather than skipped -
/// a dropped statement would change what the transaction does.
pub(crate) fn batch_statements(list: &[serde_json::Value]) -> Result<Vec<crate::db::BatchStatement>, (u16, String)> {
    list.iter()
        .enumerate()
        .map(|(i, item)| {
            let n = i + 1;
            let (sql, expect_rows) = match item {
                serde_json::Value::String(s) => (s.clone(), None),
                serde_json::Value::Object(o) => {
                    let sql = o.get("sql").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let expect_rows = match o.get("expect_rows") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(v) => Some(v.as_i64().ok_or_else(|| {
                            (400, format!("statement {n}: expect_rows must be a whole number"))
                        })?),
                    };
                    (sql, expect_rows)
                }
                _ => {
                    return Err((
                        400,
                        format!("statement {n} must be an object with \"sql\" (and optionally \"expect_rows\")"),
                    ))
                }
            };
            if sql.trim().is_empty() {
                return Err((400, format!("statement {n} has no \"sql\"")));
            }
            Ok(crate::db::BatchStatement { sql: sql.trim().to_string(), expect_rows })
        })
        .collect()
}

/// One statement, or a batch of them as one transaction, if the guard and
/// the two write doors allow it.
pub(crate) async fn db_query(ctx: &BridgeContext, body: &str) -> (u16, String) {
    const SHAPE: &str = "{ \"sql\": \"SELECT TOP (10) * FROM dbo.LeaveRequest\" } or { \"statements\": [{ \"sql\": \"UPDATE ...\", \"expect_rows\": 2 }], \"dry_run\": true }";
    let parsed = match db_body(body, SHAPE) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let dry_run = parsed.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
    // A single `sql` with dry_run is a batch of one: ignoring the flag
    // would run for real what the caller asked only to try.
    let batch = match (parsed.get("statements"), parsed.get("sql")) {
        (Some(_), Some(_)) => {
            return (400, format!("send \"sql\" or \"statements\", not both. Expected {SHAPE}."))
        }
        (Some(list), None) => match list.as_array() {
            Some(list) => Some(list.clone()),
            None => return (400, format!("\"statements\" must be a list. Expected {SHAPE}.")),
        },
        (None, Some(sql)) if dry_run => Some(vec![sql.clone()]),
        _ => None,
    };
    if let Some(list) = batch {
        let statements = match batch_statements(&list) {
            Ok(s) => s,
            Err(refused) => return refused,
        };
        let (connection, exe) = match db_ready(ctx) {
            Ok(ready) => ready,
            Err(refused) => return refused,
        };
        return match crate::db::query::run_batch_query(
            &crate::db::RealRunner,
            &exe,
            &connection,
            ctx.db_writes,
            &statements,
            dry_run,
        )
        .await
        {
            Ok(text) => (200, text),
            Err(refused) => refused,
        };
    }
    let sql = match db_body_text(&parsed, "sql", SHAPE) {
        Ok(s) => s,
        Err(refused) => return refused,
    };
    let (connection, exe) = match db_ready(ctx) {
        Ok(ready) => ready,
        Err(refused) => return refused,
    };
    match crate::db::query::run_query(
        &crate::db::RealRunner,
        &exe,
        &connection,
        ctx.db_writes,
        &sql,
    )
    .await
    {
        Ok(text) => (200, text),
        Err(refused) => refused,
    }
}

