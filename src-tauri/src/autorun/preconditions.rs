//! Preconditions: the records a case relies on, checked before step 1.
//!
//! A script names each one as a stage of an API template flow that must be
//! done for a value (`Precondition`). Before a case signs in, the run asks
//! the active environment's database whether each stage is done, through
//! the flow machinery (`api_templates::gate::stage_state`), and a case
//! whose record is not there is Blocked before anything happens in the
//! browser. They follow the Database Read Access switch: while it is off
//! no check runs and a case with preconditions goes on carrying
//! `NOT_CHECKED`; while it is on they need a database.
//!
//! Every save path (the Script editor, an import, the assistant's save)
//! validates the preconditions through `check_saved`, so a script naming a
//! flow, a stage or a value that is not there is refused, never saved to
//! fail at run time.
//!
//! No sentence here carries SQL, a server name or a connection detail: the
//! activity log the gate writes is the one place those go.

use super::{CaseScript, Precondition, PreconditionOp, PreconditionRepair};
use crate::browser::actions::Action;
use crate::browser::locator::Target;
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

/// What the assistant's save adds, after "names {records} (from step {n})",
/// when a script relies on a record it declares no precondition for. A note,
/// never a refusal.
pub const UNDECLARED_RECORD: &str = "but declares no precondition: on an environment without that record the case fails as a wrong result instead of being Blocked before step 1 - declare each record it relies on in \"preconditions\" (see \"Preconditions\" in the guide).";

/// The most records the note names before saying how many more.
const RECORDS_SHOWN: usize = 3;

/// A literal numeric id in a css attribute: `[id="42"]`, `[data-id="7"]`,
/// `[data-cycle-id="11171"]`. The attribute name is captured: one that only
/// ends in the letters (`data-grid`, `data-valid`) is not an id, and a
/// column's or a row's id is not a record's (`RECORD_ID_NOT`).
const RECORD_ID_CSS: &str = r#"(?i)\[\s*(id|data-(?:[a-z0-9_-]*[-_])?id)\s*=\s*["']?[0-9]+["']?\s*\]"#;

/// The parts of an id attribute's name that make it a column's or a row's.
const RECORD_ID_NOT: [&str; 3] = ["col", "column", "row"];

/// Does `css` hold a numeric id attribute of a record?
fn names_a_record_id(re: &regex::Regex, css: &str) -> bool {
    re.captures_iter(css).any(|c| {
        let name = c[1].to_lowercase();
        !name.split(['-', '_']).any(|part| RECORD_ID_NOT.contains(&part))
    })
}

/// Does the click in `action` press a save button: a name or text holding,
/// as whole words, a built-in save word (`browser::save_guard`) or one of
/// the project's own?
fn clicks_save(action: &Action, words: &[String]) -> bool {
    use crate::browser::locator::fold_name;
    // Only a click that saves. An `expect_no_change` on a save button is a
    // click the case says does nothing, so it makes no record.
    let Action::Click { selector } = action else { return false };
    selector.links().iter().any(|l| {
        let shown = match (&l.role, &l.name, &l.text) {
            (Some(_), Some(n), _) => n,
            (None, _, Some(t)) => t,
            _ => return false,
        };
        let shown = fold_name(shown);
        words.iter().any(|w| crate::autorun::seen_check::has_phrase(&shown, w))
    })
}

/// One note, or none, for a script with no preconditions and no setup whose
/// checked steps (`only_steps`, else all) name a record by hand: a name or
/// a text holding the environment's `test_prefix` as a whole word (and not
/// typed by the script itself), or a css attribute holding a literal record
/// id. "names {list} (from step {n}) {UNDECLARED_RECORD}", `n` being the
/// first one's step. Anything with a `{{` placeholder is not a record.
/// A name the script made itself is not either: a value it typed, holding
/// the prefix as a whole word (`{{prefix}}` is the prefix), in an earlier
/// step that also clicks a save button (the built-in save words and
/// `save_words`, the project's own). Typing into a search box and opening
/// the result is not making it.
pub fn undeclared_record_note(
    script: &CaseScript,
    only_steps: Option<&[i32]>,
    test_prefix: &str,
    save_words: &[String],
) -> Option<String> {
    use crate::browser::locator::fold_name;
    if !script.preconditions.is_empty() || script.setup.is_some() {
        return None;
    }
    let prefix = fold_name(test_prefix);
    let id_css = regex::Regex::new(RECORD_ID_CSS).ok()?;
    let words: Vec<String> = crate::browser::save_guard::SAVE_WORDS
        .iter()
        .chain(crate::browser::save_guard::WORD_SAVES.iter())
        .map(|w| w.to_string())
        .chain(save_words.iter().map(|w| w.trim().to_lowercase()))
        .filter(|w| !w.is_empty())
        .collect();
    // What the script made itself: test records it typed and saved.
    let mut typed: Vec<(i32, String)> = Vec::new();
    for step in &script.steps {
        if !step.actions.iter().flat_map(Action::each).any(|a| clicks_save(a, &words)) {
            continue;
        }
        for value in step.actions.iter().flat_map(Action::each).filter_map(|a| a.typed_value()) {
            let value = value.replace("{{prefix}}", test_prefix);
            let folded = fold_name(&value);
            if !value.contains("{{") && crate::autorun::seen_check::has_phrase(&folded, &prefix) {
                typed.push((step.step_number, folded));
            }
        }
    }
    let mut records: Vec<(i32, String)> = Vec::new();
    for step in script.steps.iter().filter(|s| only_steps.is_none_or(|only| only.contains(&s.step_number))) {
        for action in step.actions.iter().flat_map(Action::each) {
            for link in action.targets().into_iter().flat_map(Target::links) {
                let words = match (&link.role, &link.name, &link.text) {
                    (Some(_), Some(n), _) => Some(n),
                    (None, _, Some(t)) => Some(t),
                    _ => None,
                };
                let found = match (words, &link.css) {
                    (Some(w), _) if !prefix.is_empty() && !w.contains("{{") => {
                        let folded = fold_name(w);
                        let made = typed.iter().any(|(at, t)| *at < step.step_number && folded.contains(t.as_str()));
                        (crate::autorun::seen_check::has_phrase(&folded, &prefix) && !made).then(|| w.clone())
                    }
                    (None, Some(c)) if !c.contains("{{") && names_a_record_id(&id_css, c) => Some(c.clone()),
                    _ => None,
                };
                if let Some(record) = found {
                    if !records.iter().any(|(_, r)| *r == record) {
                        records.push((step.step_number, record));
                    }
                }
            }
        }
    }
    let first = records.first()?.0;
    let shown: Vec<String> = records.iter().take(RECORDS_SHOWN).map(|(_, r)| format!("{r:?}")).collect();
    let more = records.len().saturating_sub(RECORDS_SHOWN);
    let more = if more > 0 { format!(" and {more} more") } else { String::new() };
    Some(format!("names {}{more} (from step {first}) {UNDECLARED_RECORD}", shown.join(", ")))
}

/// A precondition as a person reads it: "<stage> for <value> (<flow>)",
/// by their titles when the project has them, by their ids otherwise.
pub fn describe(flows: &[Flow], p: &Precondition) -> String {
    let flow = flows.iter().find(|f| f.id == p.flow);
    let stage = flow
        .and_then(|f| f.stages.iter().find(|s| s.id == p.stage))
        .map_or(p.stage.as_str(), |s| s.title.as_str());
    let flow_title = flow.map_or(p.flow.as_str(), |f| f.title.as_str());
    format!("{stage} for {} ({flow_title})", gate::shown(&p.value))
}

/// What a repair's precondition edit did, in one line: the save's answer,
/// the script's last repair and the activity log all say it this way.
pub fn repair_line(flows: &[Flow], r: &PreconditionRepair) -> String {
    let said = |p: &Option<Precondition>| p.as_ref().map(|p| describe(flows, p)).unwrap_or_default();
    match r.op {
        PreconditionOp::Add => format!("precondition added: {} - {}", said(&r.after), r.reason),
        PreconditionOp::Remove => format!("precondition removed: {} - {}", said(&r.before), r.reason),
        PreconditionOp::Change => {
            format!("precondition changed: {} to {} - {}", said(&r.before), said(&r.after), r.reason)
        }
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

/// How long a check that could not be run because the database did not
/// answer (`gate::unreachable`: a VPN link that dropped for a moment)
/// waits before it is tried once more. A check that ran and said "not
/// done", or one that failed in a way that would fail again (a SQL error,
/// a login refused), is never tried again.
pub const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

/// Asks each precondition's stage check, in order, and stops at the first
/// that is not met or could not run. `Err` is the Blocked sentence. The
/// value goes into each check through the flow's own typed substitution
/// (`flow::substitute_check`), never pasted. A check the database did not
/// answer is tried once more after `RETRY_AFTER`, and that answer decides.
pub async fn check_all<D: StageDb>(db: &D, flows: &[Flow], preconditions: &[Precondition]) -> Result<(), String> {
    check_all_within(db, flows, preconditions, RETRY_AFTER).await
}

/// `check_all` with the wait before the second try given, so a test need
/// not wait 3 seconds to see one.
pub async fn check_all_within<D: StageDb>(
    db: &D,
    flows: &[Flow],
    preconditions: &[Precondition],
    retry_after: std::time::Duration,
) -> Result<(), String> {
    let found = problems(flows, preconditions);
    if !found.is_empty() {
        return Err(format!("{COULD_NOT}: {}", found.join("; ")));
    }
    let why = CheckFor { purpose: PURPOSE, template: None };
    for p in preconditions {
        // Both found: `problems` above refused anything else.
        let Some(flow) = flows.iter().find(|f| f.id == p.flow) else { continue };
        let Some(stage) = flow.stages.iter().find(|s| s.id == p.stage) else { continue };
        let (mut state, error) = gate::stage_answer(db, flow, stage, &p.value, &why).await;
        if state == StageState::CouldNotRun && error.as_deref().is_some_and(gate::unreachable) {
            crate::applog::info(format!(
                "Auto Run preconditions: the database did not answer the check for {} - trying once more in {} s",
                stage.title,
                retry_after.as_secs_f32()
            ));
            tokio::time::sleep(retry_after).await;
            state = gate::stage_state(db, flow, stage, &p.value, &why).await;
        }
        match state {
            StageState::Done => {}
            StageState::NotDone => return Err(not_met(&stage.title, &p.value, &flow.title, p.why.as_deref())),
            StageState::CouldNotRun => return Err(format!("{COULD_NOT}: {}", gate::could_not_run(stage))),
        }
    }
    Ok(())
}

/// Where a run's preconditions are asked.
pub enum PreconditionDb<D> {
    /// The database to ask.
    Ready(D),
    /// No database to ask: the Blocked sentence that says why
    /// (`environment_db`).
    Missing(String),
    /// Database Read Access is switched off: no check runs, and a case with
    /// preconditions goes on carrying `NOT_CHECKED`.
    ReadingOff,
}

impl<D> PreconditionDb<D> {
    /// The database `environment_db` found, or its sentence for none.
    pub fn from_result(db: Result<D, String>) -> Self {
        match db {
            Ok(d) => PreconditionDb::Ready(d),
            Err(why) => PreconditionDb::Missing(why),
        }
    }
}

/// The note a case with preconditions carries when they were not checked
/// because Database Read Access is switched off. Not a block: the case
/// goes on.
pub const NOT_CHECKED: &str = "preconditions were not checked: Database Read Access is off on the AI Bridge tab";

/// What the check before a case's sign-in decided.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct PreconditionCheck {
    /// The Blocked sentence: the case never signs in.
    pub blocked: Option<String>,
    /// Said, and the case goes on (`NOT_CHECKED`).
    pub notice: Option<String>,
}

impl PreconditionCheck {
    fn go() -> Self {
        Self::default()
    }
}

/// One case's preconditions, as the run checks them before its sign-in.
/// Nothing is asked, the database is never looked at, and nothing is said,
/// when the case has none. While reading is off nothing is asked either,
/// and the case goes on with `NOT_CHECKED`.
pub async fn check_case<D: StageDb>(
    db: &PreconditionDb<D>,
    root: &Path,
    org: &str,
    project: &str,
    preconditions: &[Precondition],
) -> PreconditionCheck {
    if preconditions.is_empty() {
        return PreconditionCheck::go();
    }
    let db = match db {
        PreconditionDb::ReadingOff => {
            return PreconditionCheck { blocked: None, notice: Some(NOT_CHECKED.to_string()) }
        }
        PreconditionDb::Missing(why) => return PreconditionCheck { blocked: Some(why.clone()), notice: None },
        PreconditionDb::Ready(db) => db,
    };
    let flows = match flow_store::list(root, org, project) {
        Ok(flows) => flows,
        Err(e) => {
            crate::applog::warn(format!("Auto Run preconditions: the flows could not be read: {e}"));
            return PreconditionCheck {
                blocked: Some(format!("{COULD_NOT}: the flows could not be read - see Settings, Logs")),
                notice: None,
            };
        }
    };
    PreconditionCheck { blocked: check_all(db, &flows, preconditions).await.err(), notice: None }
}

/// The supervised run's check for one case, by id, before its sign-in.
/// Neither field set: the case may go on (it has no script, or no
/// preconditions, or every one is met). The database is resolved only
/// when the script has preconditions.
pub async fn check_script<D: StageDb>(
    root: &Path,
    org: &str,
    project: &str,
    case_id: i32,
    db: impl FnOnce() -> PreconditionDb<D>,
) -> Result<PreconditionCheck, String> {
    let Some(script) = super::store::load_script(root, case_id)? else { return Ok(PreconditionCheck::go()) };
    if script.preconditions.is_empty() {
        return Ok(PreconditionCheck::go());
    }
    let db = db();
    Ok(check_case(&db, root, org, project, &script.preconditions).await)
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

/// Whether preconditions may be read: the AI Bridge tab's Database Read
/// Access switch (`switch`) AND the Database Access module (`db_on`). Both
/// explicit so every combination is testable.
pub fn read_access_with(switch: bool, db_on: bool) -> bool {
    switch && db_on
}

/// `read_access_with` for this process: the switch as the caller holds
/// it, and `modules::on(DbAccess)`. Every entry point that takes the switch
/// passes it through here before anything reads it.
pub fn read_access(switch: bool) -> bool {
    read_access_with(switch, crate::modules::on(crate::modules::Module::DbAccess))
}

/// Where a run started from the app asks its preconditions: nowhere while
/// Database Read Access is off (`db_read_access` is the AI Bridge tab's
/// switch, as the webview holds it: `db_query` not among the disabled
/// tools), otherwise the active environment's database.
pub fn for_run(
    root: &Path,
    store: Option<&dyn crate::db::SecretStore>,
    db_read_access: bool,
) -> PreconditionDb<SqlcmdStageDb<crate::db::RealRunner>> {
    if !db_read_access {
        return PreconditionDb::ReadingOff;
    }
    PreconditionDb::from_result(environment_db(root, store))
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
