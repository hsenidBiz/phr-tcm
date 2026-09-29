//! The gate: a template that performs one stage of a flow runs only when
//! every stage before it is done for the record, and "done" is asked of the
//! database - one read-only check per stage. See the design doc "API
//! template flows" §5 (the gate) and §7 (the progress states).
//!
//! The database sits behind `StageDb` so the logic here is testable without
//! a server; `SqlcmdStageDb` is the real one, on the same `sqlcmd` read path
//! `db_query` uses.
//!
//! Three answers a check can give, and they are never confused: done, not
//! done, and *could not run* - a database error, a timeout, an answer with
//! no row count. A check that could not run refuses the run in its own
//! words; it is never taken for "not done" (which would send the assistant
//! off to redo work) nor for "done" (which would let a run through). The
//! raw error goes to the activity log only.

use super::flow::{creating_stage, required_before, substitute_check, Flow, Stage};
use super::store::SavedTemplate;
use crate::activity_log::{self, Kind};
use crate::db::query::rows_affected;
use crate::db::{sqlcmd, Connection, Runner};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long one check may take before it counts as one that could not run.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Where a stage's check is asked.
pub trait StageDb {
    /// `server/database`, for the log.
    fn label(&self) -> String;

    /// Runs one read. `Ok(true)` when it returned at least one row.
    fn read(&self, sql: &str) -> impl std::future::Future<Output = Result<bool, String>>;
}

/// The real database: `sqlcmd`, through `run_sql`, which asks the guard
/// again itself.
pub struct SqlcmdStageDb<R: Runner> {
    pub runner: R,
    pub exe: PathBuf,
    pub conn: Connection,
}

impl<R: Runner> StageDb for SqlcmdStageDb<R> {
    fn label(&self) -> String {
        format!("{}/{}", self.conn.server, self.conn.database)
    }

    async fn read(&self, sql: &str) -> Result<bool, String> {
        let (text, _capped) = sqlcmd::run_sql(&self.runner, &self.exe, &self.conn, sql).await?;
        // The answer is the row count sqlcmd prints, not the rows: a check
        // that says `SET NOCOUNT ON`, or whose output was cut before the
        // footer, has no count, and guessing from the text would turn "the
        // answer was lost" into "done" or "not done".
        match rows_affected(&text) {
            Some(n) => Ok(n > 0),
            None => Err("the check's answer carried no row count".to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageState {
    Done,
    NotDone,
    CouldNotRun,
}

/// Why a check is being run - recorded with it.
pub struct CheckFor<'a> {
    /// `"gate"`, `"prove"`, `"progress"` or `"save"`.
    pub purpose: &'static str,
    pub template: Option<&'a str>,
}

/// One stage's check for `value`, with `CHECK_TIMEOUT` to answer.
pub async fn stage_state<D: StageDb>(
    db: &D,
    flow: &Flow,
    stage: &Stage,
    value: &Value,
    why: &CheckFor<'_>,
) -> StageState {
    stage_state_within(db, flow, stage, value, why, CHECK_TIMEOUT).await
}

/// `stage_state` with the time limit given, so a test need not wait 15
/// seconds to see one expire.
pub async fn stage_state_within<D: StageDb>(
    db: &D,
    flow: &Flow,
    stage: &Stage,
    value: &Value,
    why: &CheckFor<'_>,
    limit: Duration,
) -> StageState {
    let label = db.label();
    let started = Instant::now();

    let substituted = substitute_check(&stage.check, &flow.subject, value);
    let sql = match &substituted {
        Ok(sql) => sql.clone(),
        Err(_) => stage.check.clone(),
    };
    let answer: Result<bool, String> = match substituted {
        Ok(sql) => match tokio::time::timeout(limit, db.read(&sql)).await {
            Ok(r) => r,
            Err(_) => Err(format!("the check did not answer within {} seconds", limit.as_secs())),
        },
        Err(why) => Err(why),
    };
    let duration_ms = started.elapsed().as_millis() as u64;

    let state = match &answer {
        Ok(true) => StageState::Done,
        Ok(false) => StageState::NotDone,
        Err(_) => StageState::CouldNotRun,
    };

    let mut entry = json!({
        "verdict": "flow check",
        "connection": label,
        "sql": sql,
        "flow": flow.id,
        "stage": stage.id,
        "purpose": why.purpose,
        "template": why.template,
        "ok": answer.is_ok(),
        "done": state == StageState::Done,
        "duration_ms": duration_ms,
    });
    if let Err(e) = &answer {
        entry["error"] = json!(e);
    }
    activity_log::record(Kind::Db, entry);

    // No SQL here, and no error text: the activity log has both.
    let said = match state {
        StageState::Done => "done",
        StageState::NotDone => "not done",
        StageState::CouldNotRun => "could not run",
    };
    crate::applog::info(format!("db flow check on {label}: {}/{} {said}", flow.id, stage.id));
    state
}

/// The saved templates that perform `stage` of `flow`, in the order given.
pub fn templates_on<'a>(templates: &'a [SavedTemplate], flow: &str, stage: &str) -> Vec<&'a SavedTemplate> {
    templates
        .iter()
        .filter(|t| t.template.stage.as_ref().is_some_and(|r| r.flow == flow && r.id == stage))
        .collect()
}

/// The subject's value as a person reads it in a sentence.
fn shown(value: &Value) -> String {
    match value.as_str() {
        Some(s) => s.to_string(),
        None => value.to_string(),
    }
}

/// Refuses a value of the wrong type before any statement runs: the same
/// sentence `substitute_check` gives, from the first check of the flow.
fn validate_value(flow: &Flow, value: &Value) -> Result<(), String> {
    let Some(first) = flow.stages.first() else { return Ok(()) };
    substitute_check(&first.check, &flow.subject, value).map(|_| ())
}

/// May a template performing `stage_id` run for `value`? Every stage before
/// it is checked, in flow order, all of them - the answer names everything
/// that is missing, not just the first thing found.
///
/// The stage that creates the record is never gated (there is no record
/// yet), and asks nothing of the database.
pub async fn gate<D: StageDb>(
    db: &D,
    flow: &Flow,
    stage_id: &str,
    value: &Value,
    templates: &[SavedTemplate],
    template_id: &str,
) -> Result<(), String> {
    if creating_stage(flow).is_some_and(|s| s.id == stage_id) {
        return Ok(());
    }
    let required = required_before(flow, stage_id);
    if required.is_empty() {
        return Ok(());
    }
    validate_value(flow, value)?;

    let why = CheckFor { purpose: "gate", template: Some(template_id) };
    let mut states: Vec<(&Stage, StageState)> = Vec::with_capacity(required.len());
    for s in required {
        states.push((s, stage_state(db, flow, s, value, &why).await));
    }

    if let Some((s, _)) = states.iter().find(|(_, st)| *st == StageState::CouldNotRun) {
        return Err(format!(
            "the check for {} could not be run - see the activity folder in Settings, Logs",
            s.title
        ));
    }

    let done: HashMap<&str, bool> = states.iter().map(|(s, st)| (s.id.as_str(), *st == StageState::Done)).collect();
    let missing: Vec<&Stage> = states.iter().filter(|(_, st)| *st == StageState::NotDone).map(|(s, _)| *s).collect();
    if missing.is_empty() {
        return Ok(());
    }
    let (earliest, later): (Vec<&Stage>, Vec<&Stage>) = missing
        .into_iter()
        .partition(|s| s.requires.iter().all(|r| done.get(r.as_str()).copied().unwrap_or(false)));

    let subject = &flow.subject.name;
    let shown_value = shown(value);
    let mut said: Vec<String> = earliest
        .iter()
        .map(|s| {
            let on = templates_on(templates, &flow.id, &s.id);
            let how = if on.is_empty() {
                format!("no template performs {} yet: prove one first", s.title)
            } else {
                let names: Vec<String> =
                    on.iter().map(|t| format!("{} ({})", t.template.id, t.template.title)).collect();
                format!("do it first with {}", names.join(" or "))
            };
            format!("{} is not done for {subject} {shown_value} - {how}.", s.title)
        })
        .collect();
    if !later.is_empty() {
        let titles: Vec<&str> = later.iter().map(|s| s.title.as_str()).collect();
        said.push(format!("Then: {}.", titles.join(", ")));
    }
    Err(said.join(" "))
}

/// One stage as `get_api_flow_progress` reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StageProgress {
    pub id: String,
    pub title: String,
    /// `"done"`, `"next"`, `"blocked"`, `"skippable"` or `"could_not_check"`.
    pub state: &'static str,
    pub optional: bool,
    /// Ids of the saved templates that perform this stage.
    pub templates: Vec<String>,
}

/// Every stage of the flow for `value`, each checked once.
///
/// `next`: not done and every stage it requires is done. `skippable`: the
/// same, but optional. `blocked`: something it requires is not done - or
/// could not be checked. `could_not_check`: its own check could not run.
pub async fn progress<D: StageDb>(
    db: &D,
    flow: &Flow,
    value: &Value,
    templates: &[SavedTemplate],
) -> Result<Vec<StageProgress>, String> {
    validate_value(flow, value)?;

    let why = CheckFor { purpose: "progress", template: None };
    let mut states: HashMap<&str, StageState> = HashMap::new();
    for s in &flow.stages {
        states.insert(s.id.as_str(), stage_state(db, flow, s, value, &why).await);
    }

    Ok(flow
        .stages
        .iter()
        .map(|s| {
            let state = match states[s.id.as_str()] {
                StageState::Done => "done",
                StageState::CouldNotRun => "could_not_check",
                StageState::NotDone => {
                    let open = s
                        .requires
                        .iter()
                        .all(|r| states.get(r.as_str()).is_some_and(|st| *st == StageState::Done));
                    match (open, s.optional) {
                        (false, _) => "blocked",
                        (true, true) => "skippable",
                        (true, false) => "next",
                    }
                }
            };
            StageProgress {
                id: s.id.clone(),
                title: s.title.clone(),
                state,
                optional: s.optional,
                templates: templates_on(templates, &flow.id, &s.id).iter().map(|t| t.template.id.clone()).collect(),
            }
        })
        .collect())
}
