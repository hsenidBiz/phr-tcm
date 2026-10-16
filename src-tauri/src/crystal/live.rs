//! The live-data preview: the open report drawn with fresh rows from its
//! stored procedure, on the connection chosen in the AI Bridge tab.
//!
//! The bridge decides who may ask (Database Access on, writes switched on,
//! the Dev login); this file decides what runs. Only a stored procedure
//! runs, and only one of the connection's own database - checked on the
//! report before anything connects (`live_target`). The login becomes the
//! bundle's ADO connection string here, right before the call, and is
//! dropped after it; no answer, stored diagnostic or log line carries the
//! host, the password or the string (`scrub`).

use rpt_core::rpt_reader::model::Report;
use rpt_core::rpt_reader::Rpt;
use rpt_core::{LiveDb, PreviewOutput};
use rpt_db_mssql::TableFetch;

use super::{file_name, state, PreviewInfo, NOTHING_OPEN};
use crate::db::Connection;

const COMMAND_REFUSED: &str =
    "this report runs a SQL command, not a stored procedure: live preview only runs stored procedures";

/// The procedure a live preview of `report` runs, checked: the first table
/// the bundle can fetch must be a stored procedure of `own_db` (the
/// connection's database). A database-qualified name is accepted only for
/// `own_db`, and the qualifier is dropped from the answer; another
/// database, a linked server, an `sp_` or `xp_` procedure, a SQL command,
/// or anything that is not a plain name is refused with the reason.
pub fn live_target(report: &Report, own_db: &str) -> Result<String, String> {
    // Which procedure runs does not depend on the parameter values: they
    // only fill in its arguments.
    let no_values = rpt_core::coerce_params(report, &[])?;
    live_target_of(&rpt_db_mssql::plan_for_report(report, &no_values), own_db)
}

/// `live_target` over the bundle's fetch plan: the first table it can
/// fetch is the one it runs, so that is the one checked.
pub fn live_target_of(plans: &[TableFetch], own_db: &str) -> Result<String, String> {
    let first = plans.iter().find_map(|p| match p {
        TableFetch::StoredProcedure { rpc_name, .. } => Some(Ok(rpc_name.as_str())),
        TableFetch::Command { .. } => Some(Err(COMMAND_REFUSED.to_string())),
        TableFetch::NotSupported { .. } => None,
    });
    match first {
        Some(Ok(name)) => checked_procedure(name, own_db),
        Some(Err(why)) => Err(why),
        None => {
            let reasons: Vec<String> = plans
                .iter()
                .filter_map(|p| match p {
                    TableFetch::NotSupported { table, reason } => Some(format!("table {table}: {reason}")),
                    _ => None,
                })
                .collect();
            Err(if reasons.is_empty() {
                "this report reads no database table, so there is no procedure to run".to_string()
            } else {
                format!("no table of this report can be fetched live ({})", reasons.join("; "))
            })
        }
    }
}

/// `name` as `[database.]schema.procedure` parts, checked. The bundle runs
/// the name inside one pair of brackets, so a bracket, a quote, a space or
/// a `;` in it could end the name early: every part must be a plain name.
fn checked_procedure(name: &str, own_db: &str) -> Result<String, String> {
    let plain = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || "_@#$".contains(c));
    let parts: Vec<&str> = name.split('.').collect();
    // `db..procedure` leaves the schema out: the database's default one.
    let (database, schema, procedure) = match parts.as_slice() {
        [procedure] => (None, None, *procedure),
        [schema, procedure] => (None, Some(*schema), *procedure),
        [database, schema, procedure] => (Some(*database), Some(*schema).filter(|s| !s.is_empty()), *procedure),
        _ => {
            return Err(format!(
                "the report runs \"{name}\", which names a linked server or is not one procedure's name: live preview runs only a procedure of the connection's own database"
            ))
        }
    };
    if !plain(procedure) || !schema.is_none_or(plain) || !database.is_none_or(plain) {
        return Err(format!(
            "the report runs \"{name}\", which is not a plain procedure name (letters, digits and _ only, parts separated by dots)"
        ));
    }
    if let Some(other) = database.filter(|d| !d.eq_ignore_ascii_case(own_db)) {
        return Err(format!(
            "the report runs {name}, a procedure of the database {other} rather than the connection's own: live preview runs only procedures of the connection's own database"
        ));
    }
    let lower = procedure.to_ascii_lowercase();
    if lower.starts_with("sp_") || lower.starts_with("xp_") {
        return Err(format!(
            "the report runs {procedure}, a system or extended procedure (sp_ or xp_): live preview never runs those"
        ));
    }
    Ok(match schema {
        Some(schema) => format!("{schema}.{procedure}"),
        None => procedure.to_string(),
    })
}

/// The login as the bundle's ADO connection string. A value with a `;` in
/// it is refused rather than passed on: `;Server=` inside a password would
/// point the string at another host.
fn connection_string(conn: &Connection) -> Result<String, String> {
    if [&conn.server, &conn.database, &conn.user, &conn.password].iter().any(|v| v.contains(';')) {
        return Err(
            "the chosen connection's login has a ';' in it, which cannot be passed on to the report's data source - choose another connection on the AI Bridge tab"
                .to_string(),
        );
    }
    Ok(format!(
        "Server={};Database={};User Id={};Password={};TrustServerCertificate={}",
        conn.server,
        conn.database,
        conn.user,
        conn.password,
        if conn.trust_cert { "True" } else { "False" }
    ))
}

/// `text` with every occurrence of `needle` replaced by `with`, matching
/// ASCII letters in any case. A driver may report the host in another case
/// than it was given.
fn replace_any_case(text: &str, needle: &str, with: &str) -> String {
    // ASCII lowercasing keeps every byte offset, so a match in the lowered
    // copy is the same bytes in `text`.
    let (haystack, needle_lower) = (text.to_ascii_lowercase(), needle.to_ascii_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(at) = haystack[from..].find(&needle_lower) {
        out.push_str(&text[from..from + at]);
        out.push_str(with);
        from += at + needle.len();
    }
    out.push_str(&text[from..]);
    out
}

/// `text` with every `<marker><name><end>` cut down to `<marker><end>`,
/// matching `marker` in any case; with no `end` after it, the name runs to
/// the next space. How a server's own name is taken out of a driver
/// message whatever it is called.
fn drop_named(text: &str, marker: &str, end: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(at) = lower[from..].find(marker) {
        let name_at = from + at + marker.len();
        out.push_str(&text[from..from + at]);
        let name_len = match lower[name_at..].find(end) {
            Some(len) => len,
            None => lower[name_at..].find(char::is_whitespace).unwrap_or(lower.len() - name_at),
        };
        from = name_at + name_len;
    }
    out.push_str(&text[from..]);
    out
}

/// `text` with every IPv4 or IPv6 address (with or without a port)
/// replaced by `(address)`.
fn drop_addresses(text: &str) -> String {
    let in_run = |c: char| c.is_ascii_hexdigit() || c == '.' || c == ':';
    let is_address = |run: &str| {
        let core = run.trim_matches(['.', ':']);
        let with_port = core
            .rsplit_once(':')
            .filter(|(_, port)| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()));
        core.parse::<std::net::IpAddr>().is_ok()
            || with_port.is_some_and(|(head, _)| head.parse::<std::net::Ipv4Addr>().is_ok())
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(in_run) {
        out.push_str(&rest[..start]);
        let len = rest[start..].find(|c: char| !in_run(c)).unwrap_or(rest.len() - start);
        let run = &rest[start..start + len];
        if run.parse::<std::net::IpAddr>().is_ok() {
            // `::1` and the like begin with what would otherwise be trimmed.
            out.push_str("(address)");
        } else if is_address(run) {
            // Keep a sentence's own full stop or colon around it.
            let lead = run.len() - run.trim_start_matches(['.', ':']).len();
            let tail = run.len() - run.trim_end_matches(['.', ':']).len();
            out.push_str(&run[..lead]);
            out.push_str("(address)");
            out.push_str(&run[run.len() - tail..]);
        } else {
            out.push_str(run);
        }
        rest = &rest[start + len..];
    }
    out.push_str(rest);
    out
}

/// `text` with nothing of the login or the server left in it: the whole
/// connection string, any `Password=` the bundle's own redaction catches,
/// the password, the host - as given and without its `,port` or
/// `\instance` - and every server name the driver adds of its own. A
/// server's error names the server by its own name (`'...' on server NAME
/// executing ...`), which need not be the host the login gives, and a
/// redirect names an address; both are cut out whatever they say, and any
/// IP address left is replaced.
fn scrub(text: &str, conn: &Connection, connection_string: &str) -> String {
    let mut out = replace_any_case(text, connection_string, "(the connection string)");
    out = rpt_db_mssql::redact_connection(&out);
    let host = conn.server.split([',', '\\']).next().unwrap_or_default().trim();
    for (secret, with) in [(conn.password.as_str(), "(hidden)"), (conn.server.as_str(), "(server)"), (host, "(server)")] {
        if !secret.trim().is_empty() {
            out = replace_any_case(&out, secret, with);
        }
    }
    out = drop_named(&out, " on server ", " executing");
    out = drop_named(&out, "alternative address: `", "`");
    drop_addresses(&out)
}

/// A parameter name as a person writes it: `@name`, without the
/// `{?...}` a formula wraps it in.
fn shown(name: &str) -> &str {
    name.trim_start_matches('{').trim_start_matches('?').trim_end_matches('}')
}

/// The parameter values given for a live preview of `report`, checked
/// before anything connects: every name must be one of the report's
/// parameters (matched like the bundle matches them: in any case, with or
/// without the `@`), and every parameter the report needs a value for -
/// not optional and not allowed to be empty - must be given. A value saved
/// in the report does not count: the bundle binds only the values passed
/// here and NULL for the rest, never a saved one. Each refusal names the
/// parameters.
pub fn check_live_params(report: &Report, params: &[(String, String)]) -> Result<(), String> {
    let key = rpt_db_mssql::param_key;
    let fields: Vec<_> = report.data_definition.parameter_fields().collect();
    // The procedure's own parameters are names the bundle binds too.
    let declared = fields.iter().map(|(f, _)| shown(&f.name)).chain(
        report.database.tables.iter().flat_map(|t| t.parameters.iter().map(|p| p.name.as_str())),
    );
    let mut known: Vec<&str> = Vec::new();
    for name in declared {
        let k = key(name);
        if !k.is_empty() && !k.starts_with("return_value") && !known.iter().any(|n| key(n) == k) {
            known.push(name);
        }
    }
    let unknown: Vec<&str> =
        params.iter().map(|(n, _)| n.as_str()).filter(|n| !known.iter().any(|k| key(k) == key(n))).collect();
    if !unknown.is_empty() {
        let has = if known.is_empty() { "none".to_string() } else { known.join(", ") };
        return Err(format!("this report has no parameter named {} (its parameters: {has})", unknown.join(", ")));
    }
    let missing: Vec<&str> = fields
        .iter()
        .filter(|(_, p)| p.report_name.is_none() && !p.optional_prompt && !p.allow_null_value)
        .map(|(f, _)| shown(&f.name))
        .filter(|n| !params.iter().any(|(given, _)| key(given) == key(n)))
        .collect();
    if !missing.is_empty() {
        return Err(format!("the report needs a value for {}: give it in params", missing.join(", ")));
    }
    Ok(())
}

/// Render the open report from its stored procedure, run on `conn`, and
/// keep the result like a saved-data render, marked as live from
/// `connection_name`. The caller has already decided `conn` may run it;
/// this checks the procedure (`live_target`) and the parameter values
/// before anything connects.
pub fn preview_live(
    conn: &Connection,
    connection_name: &str,
    params: Vec<(String, String)>,
) -> Result<PreviewInfo, String> {
    preview_live_with(conn, connection_name, params, rpt_core::preview::preview_live_report)
}

/// `preview_live` with the bundle's fetch-and-render as a parameter, so a
/// test can stand in for the server. `fetch` blocks on the bundle's own
/// runtime: call this from a blocking thread, never on an async one.
pub fn preview_live_with(
    conn: &Connection,
    connection_name: &str,
    params: Vec<(String, String)>,
    fetch: impl FnOnce(&Rpt, &LiveDb) -> Result<PreviewOutput, String>,
) -> Result<PreviewInfo, String> {
    let mut slot = state();
    let open = slot.as_mut().ok_or(NOTHING_OPEN)?;
    let name = file_name(&open.path);
    let refused = |why: String| format!("{name} cannot be previewed with live data: {why}.");
    live_target(open.rpt.report(), &conn.database).map_err(refused)?;
    // A misspelt or missing parameter, or a value that does not fit its
    // kind, is refused here, before any connection: the bundle would run
    // the procedure with NULL for it. It coerces the same pairs again itself.
    check_live_params(open.rpt.report(), &params).map_err(refused)?;
    rpt_core::coerce_params(open.rpt.report(), &params).map_err(refused)?;
    let db = LiveDb { connection_string: connection_string(conn).map_err(refused)?, params };

    let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fetch(&open.rpt, &db)))
        .unwrap_or_else(|_| Err("the renderer stopped unexpectedly".to_string()));
    let clean = |text: &str| scrub(text, conn, &db.connection_string);
    let rendered = match rendered {
        Ok(out) => Ok(PreviewOutput { diagnostics: out.diagnostics.iter().map(|d| clean(d)).collect(), ..out }),
        Err(e) => Err(clean(&e)),
    };
    drop(db);

    let live = Some(connection_name.to_string());
    match rendered {
        Ok(out) => {
            let info = PreviewInfo {
                path: open.path.clone(),
                page_count: u32::try_from(out.page_count).unwrap_or(u32::MAX),
                diagnostics: out.diagnostics,
                live,
            };
            crate::applog::info(format!(
                "Crystal Reports: drew {name} with live data from {connection_name}, {} page(s)",
                info.page_count
            ));
            open.preview = Some((info.clone(), out.pdf));
            Ok(info)
        }
        Err(e) => {
            let why = format!("The live preview of {name} from {connection_name} could not be drawn: {e}.");
            crate::applog::info(format!("Crystal Reports: {why}"));
            let info = PreviewInfo { path: open.path.clone(), page_count: 0, diagnostics: vec![why.clone()], live };
            open.preview = Some((info, Vec::new()));
            Err(why)
        }
    }
}
