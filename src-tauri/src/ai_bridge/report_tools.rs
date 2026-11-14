//! The report tools: opening, reading, changing, checking and previewing the report the screen shows.

use super::*;

/// Said when the report library panicked inside a call.
pub(crate) const CRYSTAL_STOPPED: &str = "The report library stopped unexpectedly while doing that. Nothing was saved unless the report says otherwise.";

/// Run `work` on a blocking thread: `Err((400, sentence))` when it refuses,
/// `Err((500, ..))` when it panicked, and what it answers otherwise.
pub(crate) async fn crystal_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, (u16, String)> {
    match tokio::task::spawn_blocking(work).await {
        Ok(Ok(answer)) => Ok(answer),
        Ok(Err(why)) => Err((400, why)),
        Err(_) => Err((500, CRYSTAL_STOPPED.to_string())),
    }
}

/// A 200 with `value` as JSON text, or the refusal.
pub(crate) fn crystal_answer(done: Result<serde_json::Value, (u16, String)>) -> (u16, String) {
    match done {
        Ok(value) => (200, serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())),
        Err(refused) => refused,
    }
}

/// A crystal route's body as an object. An empty body is no arguments, for
/// the routes whose arguments are all optional.
pub(crate) fn crystal_args(body: &str) -> Result<serde_json::Value, (u16, String)> {
    if body.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) if v.is_object() => Ok(v),
        Ok(_) => Err((400, "the arguments must be a JSON object.".to_string())),
        Err(e) => Err((400, format!("that is not readable JSON: {e}."))),
    }
}

/// A text argument, trimmed; `None` when it is missing, null or blank.
pub(crate) fn crystal_text(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// A text argument the call cannot do without.
pub(crate) fn crystal_required(args: &serde_json::Value, key: &str) -> Result<String, (u16, String)> {
    crystal_text(args, key).ok_or_else(|| (400, format!("this call needs a \"{key}\".")))
}

/// `ops` and `plan` arrive as JSON, but an assistant that wrapped them in a
/// string (as it does for several other tools) is read the same way.
pub(crate) fn crystal_json_arg(args: &serde_json::Value, key: &str) -> Option<serde_json::Value> {
    match args.get(key) {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(text)) => {
            Some(serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::String(text.clone())))
        }
        Some(found) => Some(found.clone()),
    }
}

pub(crate) async fn crystal_open(body: &str) -> (u16, String) {
    let path = match crystal_args(body).and_then(|args| crystal_required(&args, "path")) {
        Ok(path) => path,
        Err(refused) => return refused,
    };
    let done = crystal_blocking(move || crate::crystal::open(&path)).await;
    // A refused open leaves the old report open; only a new one changes the screen.
    if done.is_ok() {
        crystal_changed().await;
    }
    crystal_answer(done)
}

pub(crate) async fn crystal_read(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let filter =
        crate::crystal::ReadFilter { section: crystal_text(&args, "section"), object: crystal_text(&args, "object") };
    crystal_answer(crystal_blocking(move || crate::crystal::read(&filter)).await)
}

/// Apply one batch. A batch the bundle refuses saves nothing, and the
/// answer is the refusal followed by the report as it now stands, so the
/// assistant has fresh ids to try again with.
pub(crate) async fn crystal_change(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let (ops, plan) = (crystal_json_arg(&args, "ops"), crystal_json_arg(&args, "plan"));
    let done = crystal_blocking(move || {
        crate::crystal::edit(ops, plan).map_err(|why| {
            let now = crate::crystal::read(&crate::crystal::ReadFilter::default())
                .ok()
                .and_then(|ir| serde_json::to_string_pretty(&ir).ok());
            match now {
                Some(ir) => format!("{why}\n\n{ir}"),
                None => why,
            }
        })
    })
    .await;
    // A refused batch tells the screen too: the report is re-read from disk.
    crystal_changed().await;
    crystal_answer(done)
}

pub(crate) async fn crystal_compare(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let (a, b) = match (crystal_required(&args, "a"), crystal_required(&args, "b")) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(refused), _) | (_, Err(refused)) => return refused,
    };
    let done = crystal_blocking(move || crate::crystal::compare(&a, &b)).await;
    crystal_answer(done.map(|lines| serde_json::json!(lines)))
}

pub(crate) async fn crystal_check(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let path = crystal_text(&args, "path");
    crystal_answer(crystal_blocking(move || crate::crystal::check(path.as_deref())).await)
}

pub(crate) async fn crystal_new(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let path = match crystal_required(&args, "path") {
        Ok(path) => path,
        Err(refused) => return refused,
    };
    let opts = rpt_core::blank::NewReportOptions {
        title: crystal_text(&args, "title"),
        author: crystal_text(&args, "author"),
        comments: crystal_text(&args, "comments"),
        ..Default::default()
    };
    let done = crystal_blocking(move || crate::crystal::new_report(&path, opts)).await;
    if done.is_ok() {
        crystal_changed().await;
    }
    crystal_answer(done)
}

pub(crate) async fn crystal_preview(body: &str) -> (u16, String) {
    let params = match crystal_args(body).and_then(|args| crystal_params(&args)) {
        Ok(params) => params,
        Err(refused) => return refused,
    };
    let done = crystal_blocking(move || crate::crystal::preview_saved_with(params)).await;
    // A render that failed is kept as the preview too, so the screen shows why.
    crystal_changed().await;
    crystal_answer(done.and_then(|info| serde_json::to_value(info).map_err(|e| (500, e.to_string()))))
}

/// The open report's parameters, in the order it declares them.
pub(crate) async fn crystal_parameters() -> (u16, String) {
    let done = crystal_blocking(crate::crystal::parameters).await;
    crystal_answer(done.and_then(|list| serde_json::to_value(list).map_err(|e| (500, e.to_string()))))
}

/// Find text across the open report and its subreports. The text keeps its
/// spaces: unlike the other routes' arguments it is not trimmed.
pub(crate) async fn crystal_find(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let Some(text) = args.get("text").and_then(|v| v.as_str()).map(str::to_string) else {
        return (400, "this call needs a \"text\".".to_string());
    };
    let match_case = args.get("match_case").and_then(|v| v.as_bool()).unwrap_or(false);
    let done = crystal_blocking(move || crate::crystal::find(&text, match_case)).await;
    crystal_answer(done.and_then(|hits| serde_json::to_value(hits).map_err(|e| (500, e.to_string()))))
}

/// Replace text across the open report and its subreports; `dry_run` lists
/// the changes and saves nothing. `find` and `replace` keep their spaces and
/// `replace` may be empty (it removes the text).
pub(crate) async fn crystal_replace(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let Some(find) = args.get("find").and_then(|v| v.as_str()).map(str::to_string) else {
        return (400, "this call needs a \"find\".".to_string());
    };
    let Some(replace) = args.get("replace").and_then(|v| v.as_str()).map(str::to_string) else {
        return (400, "this call needs a \"replace\" (it may be empty, to remove the text).".to_string());
    };
    let match_case = args.get("match_case").and_then(|v| v.as_bool()).unwrap_or(false);
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
    let done = crystal_blocking(move || crate::crystal::replace(&find, &replace, match_case, dry_run)).await;
    // A refused replace re-read the file, and an applied one changed it.
    crystal_changed().await;
    crystal_answer(done.and_then(|out| serde_json::to_value(out).map_err(|e| (500, e.to_string()))))
}

/// The two doors of a live preview that need no connection to check:
/// Database Access on (the route's own guard is Crystal Reports'), and the
/// person's switch for database writes - running a report's stored
/// procedure is a procedure call, which TCM treats as a write. `on` is
/// explicit so both are testable.
pub fn live_preview_gate(
    on: &std::collections::BTreeSet<crate::modules::Module>,
    writes_on: bool,
) -> Result<(), (u16, String)> {
    use crate::modules::Module;
    if !on.contains(&Module::DbAccess) {
        return Err((404, Module::DbAccess.off_sentence()));
    }
    if !writes_on {
        return Err((400, crate::db::query::WRITES_OFF.to_string()));
    }
    Ok(())
}

/// The report's parameters out of `params` (name to value). A number or a
/// true/false is taken as its text, which the bundle reads against the
/// parameter's declared kind.
pub(crate) fn crystal_params(args: &serde_json::Value) -> Result<Vec<(String, String)>, (u16, String)> {
    use serde_json::Value;
    match args.get("params") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(name, value)| match value {
                Value::String(text) => Ok((name.clone(), text.clone())),
                Value::Number(n) => Ok((name.clone(), n.to_string())),
                Value::Bool(b) => Ok((name.clone(), b.to_string())),
                _ => Err((400, format!("params.{name} must be text, a number or true/false."))),
            })
            .collect(),
        Some(_) => Err((
            400,
            "\"params\" must be an object of parameter name to value, e.g. { \"performance_cycle_id\": \"42\" }."
                .to_string(),
        )),
    }
}

/// The chosen database by the name the AI Bridge tab shows it under -
/// what a live preview says it came from, never the host.
pub(crate) fn chosen_database_name(ctx: &BridgeContext) -> String {
    ctx.db_secrets
        .as_deref()
        .zip(ctx.db_id.as_deref())
        .and_then(|(store, id)| crate::db::credentials::databases(store).into_iter().find(|d| d.id == id.trim()))
        .map(|d| d.label)
        .unwrap_or_else(|| "the chosen database".to_string())
}

/// Draw the open report with fresh rows from its stored procedure. Every
/// door is checked before anything connects: Database Access and writes
/// (`live_preview_gate`), a chosen connection, and the Dev login on it;
/// then `crystal::preview_live` checks the procedure and the parameters.
/// The bundle blocks on a runtime of its own, so the call runs on a
/// blocking thread (`crystal_blocking`), never on this one.
///
/// `on` is the modules that are on and `fetch` the bundle's
/// fetch-and-render (`preview_live_report`); both are parameters so a test
/// can show that a refused call never reaches the fetch.
pub async fn crystal_preview_live(
    ctx: &BridgeContext,
    body: &str,
    on: &std::collections::BTreeSet<crate::modules::Module>,
    fetch: impl FnOnce(&rpt_core::rpt_reader::Rpt, &rpt_core::LiveDb) -> Result<rpt_core::PreviewOutput, String>
        + Send
        + 'static,
) -> (u16, String) {
    let params = match crystal_args(body).and_then(|args| crystal_params(&args)) {
        Ok(params) => params,
        Err(refused) => return refused,
    };
    if let Err(refused) = live_preview_gate(on, ctx.db_writes) {
        return refused;
    }
    let conn = match crate::db::query::connection(ctx.db_secrets.as_deref(), ctx.db_id.as_deref()) {
        Ok(conn) => conn,
        Err(why) => return (409, why),
    };
    if crate::db::access_for_user(&conn.user) == crate::db::Access::ReadOnly {
        return (400, crate::db::READ_ONLY_SENTENCE.to_string());
    }
    let name = chosen_database_name(ctx);
    let done = crystal_blocking(move || crate::crystal::preview_live_with(&conn, &name, params, fetch)).await;
    // Past the doors a failed draw is kept as the preview (as with the saved one); a refusal is
    // only a re-read.
    crystal_changed().await;
    crystal_answer(done.and_then(|info| serde_json::to_value(info).map_err(|e| (500, e.to_string()))))
}

/// How to work with reports. The guide is a constant, so nothing here can
/// block or panic.
pub(crate) fn crystal_guide() -> (u16, String) {
    (200, crate::crystal::guide().to_string())
}

