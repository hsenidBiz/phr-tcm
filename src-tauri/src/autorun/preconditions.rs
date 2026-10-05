//! Preconditions: the records a case relies on, checked before step 1.
//!
//! A script names each one as a stage of an API template flow that must be
//! done for a value (`Precondition`). Before a case signs in, the run asks
//! the active environment's database whether each stage is done, through
//! the flow machinery (`api_templates::gate::stage_state`), and a case
//! whose record is not there is Blocked before anything happens in the
//! browser. The app runs these checks itself, so they do not need the
//! assistant's Database Read Access switch; they do need a database.
//!
//! Every save path (the Script editor, an import, the assistant's save)
//! validates the preconditions through `check_saved`, so a script naming a
//! flow, a stage or a value that is not there is refused, never saved to
//! fail at run time.
//!
//! No sentence here carries SQL, a server name or a connection detail: the
//! activity log the gate writes is the one place those go.

use super::{CaseScript, Precondition};
use crate::api_templates::flow::{Flow, Subject, SubjectType};
use crate::api_templates::flow_store;
use crate::api_templates::gate::{self, CheckFor, SqlcmdStageDb, StageDb, StageState};
use serde_json::Value;
use std::path::Path;

/// Said when a case has preconditions and no database is chosen for the
/// active environment.
pub const NEED_DB: &str = "preconditions need a database chosen on the AI Bridge tab";

/// What the activity log records as the reason for each check.
pub const PURPOSE: &str = "precondition";

/// The front of every Blocked sentence for a check that could not be made.
const COULD_NOT: &str = "precondition could not be checked";

/// Said for a precondition whose value is missing, or not the type the
/// flow's checks take. Counted from 1.
fn no_value(n: usize) -> String {
    format!("precondition {n}: give the value the flow's checks take")
}

/// Whether `value` is one the flow's checks can take: a whole number of 0
/// or more for a number subject, a non-blank string for a string one -
/// the same types `flow::substitute_check` accepts.
fn fits(subject: &Subject, value: &Value) -> bool {
    match subject.kind {
        SubjectType::Number => value.as_u64().is_some(),
        SubjectType::String => value.as_str().is_some_and(|s| !s.trim().is_empty()),
    }
}

/// Every problem with these preconditions against the project's flows, one
/// sentence each, in order. Empty means they may be saved.
pub fn problems(flows: &[Flow], preconditions: &[Precondition]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, p) in preconditions.iter().enumerate() {
        let n = i + 1;
        let Some(flow) = flows.iter().find(|f| f.id == p.flow) else {
            out.push(format!("precondition {n}: no flow {}", p.flow));
            continue;
        };
        if !flow.stages.iter().any(|s| s.id == p.stage) {
            out.push(format!("precondition {n}: flow {} has no stage {}", flow.title, p.stage));
        }
        if !fits(&flow.subject, &p.value) {
            out.push(no_value(n));
        }
    }
    out
}

/// The save-time check every save path makes: each script's preconditions
/// against the project's flows, all of them. The flows are read only when
/// a script has any. A refusal names the case, then every problem.
pub fn check_saved(root: &Path, org: &str, project: &str, scripts: &[CaseScript]) -> Result<(), String> {
    if scripts.iter().all(|s| s.preconditions.is_empty()) {
        return Ok(());
    }
    let flows = flow_store::list(root, org, project).map_err(|e| {
        crate::applog::warn(format!("Auto Run preconditions: the flows could not be read: {e}"));
        "the flows could not be read to check the preconditions - see Settings, Logs".to_string()
    })?;
    let refused: Vec<String> = scripts
        .iter()
        .filter_map(|s| {
            let found = problems(&flows, &s.preconditions);
            (!found.is_empty()).then(|| format!("case {}: {}", s.case_id, found.join("; ")))
        })
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(refused.join("; "))
    }
}

/// The Blocked sentence for a precondition whose stage is not done.
pub fn not_met(stage_title: &str, value: &Value, flow_title: &str, why: Option<&str>) -> String {
    let mut out = format!("precondition not met: {stage_title} for {} ({flow_title})", gate::shown(value));
    if let Some(why) = why.map(str::trim).filter(|w| !w.is_empty()) {
        out.push_str(" - ");
        out.push_str(why);
    }
    out
}

/// Asks each precondition's stage check, in order, and stops at the first
/// that is not met or could not run. `Err` is the Blocked sentence. The
/// value goes into each check through the flow's own typed substitution
/// (`flow::substitute_check`), never pasted.
pub async fn check_all<D: StageDb>(db: &D, flows: &[Flow], preconditions: &[Precondition]) -> Result<(), String> {
    let found = problems(flows, preconditions);
    if !found.is_empty() {
        return Err(format!("{COULD_NOT}: {}", found.join("; ")));
    }
    let why = CheckFor { purpose: PURPOSE, template: None };
    for p in preconditions {
        // Both found: `problems` above refused anything else.
        let Some(flow) = flows.iter().find(|f| f.id == p.flow) else { continue };
        let Some(stage) = flow.stages.iter().find(|s| s.id == p.stage) else { continue };
        match gate::stage_state(db, flow, stage, &p.value, &why).await {
            StageState::Done => {}
            StageState::NotDone => return Err(not_met(&stage.title, &p.value, &flow.title, p.why.as_deref())),
            StageState::CouldNotRun => return Err(format!("{COULD_NOT}: {}", gate::could_not_run(stage))),
        }
    }
    Ok(())
}

/// One case's preconditions, as the run checks them before its sign-in.
/// Nothing is asked, and the database is never looked at, when the case
/// has none. `db` is the database, or the Blocked sentence that says why
/// there is none (`environment_db`).
pub async fn check_case<D: StageDb>(
    db: &Result<D, String>,
    root: &Path,
    org: &str,
    project: &str,
    preconditions: &[Precondition],
) -> Result<(), String> {
    if preconditions.is_empty() {
        return Ok(());
    }
    let db = db.as_ref().map_err(String::clone)?;
    let flows = flow_store::list(root, org, project).map_err(|e| {
        crate::applog::warn(format!("Auto Run preconditions: the flows could not be read: {e}"));
        format!("{COULD_NOT}: the flows could not be read - see Settings, Logs")
    })?;
    check_all(db, &flows, preconditions).await
}

/// The supervised run's check for one case, by id, before its sign-in.
/// `Ok(None)`: the case may go on (it has no script, or no preconditions,
/// or every one is met). `Ok(Some(sentence))`: it is Blocked. The database
/// is resolved only when the script has preconditions.
pub async fn check_script<D: StageDb>(
    root: &Path,
    org: &str,
    project: &str,
    case_id: i32,
    db: impl FnOnce() -> Result<D, String>,
) -> Result<Option<String>, String> {
    let Some(script) = super::store::load_script(root, case_id)? else { return Ok(None) };
    if script.preconditions.is_empty() {
        return Ok(None);
    }
    let db = db();
    Ok(check_case(&db, root, org, project, &script.preconditions).await.err())
}

/// The active environment's database, as the place preconditions are
/// asked, or the Blocked sentence that says why there is none: `NEED_DB`
/// when none is chosen (or the one chosen is not set up any more), and the
/// database setup's own sentence after "precondition could not be
/// checked" for anything else (no login saved, no sqlcmd).
pub fn environment_db(
    root: &Path,
    store: Option<&dyn crate::db::SecretStore>,
) -> Result<SqlcmdStageDb<crate::db::RealRunner>, String> {
    let env = crate::environments::active(root).map_err(|e| {
        crate::applog::warn(format!("Auto Run preconditions: the active environment could not be read: {e}"));
        format!("{COULD_NOT}: the environments could not be read - see Settings, Logs")
    })?;
    let (conn, exe) = crate::db::query::ready(store, Some(&env.db_id)).map_err(|why| {
        if why == crate::db::query::NO_CONNECTION {
            NEED_DB.to_string()
        } else {
            format!("{COULD_NOT}: {why}")
        }
    })?;
    Ok(SqlcmdStageDb { runner: crate::db::RealRunner, exe, conn })
}

/// A database that is never asked: the stand-in for callers that have no
/// database to give, alongside the sentence that says so.
pub struct NoDb;

impl StageDb for NoDb {
    fn label(&self) -> String {
        "none".to_string()
    }

    async fn read(&self, _sql: &str) -> Result<bool, String> {
        Err(NEED_DB.to_string())
    }
}
