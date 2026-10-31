//! `needs_records`: the kept records a script relies on and did not make.
//!
//! A script names each one by a saved setup (`setups`) whose database find
//! rule looks for it. Before the case signs in, the run asks that rule
//! (read only, through the active environment's database); a record that is
//! not there makes the case Blocked with one sentence saying what is
//! missing and which setup builds it, instead of failing halfway through
//! the steps. They follow the Database Access module as preconditions do:
//! while it is off nothing is asked and the case goes on, saying so.
//!
//! Every save path validates the declaration through `check_saved`: a name
//! that is not a saved setup, or a setup with no database find rule, is
//! refused. A script that relies on a record without declaring it is never
//! refused; the assistant's save adds a note
//! (`preconditions::undeclared_record_note`).
//!
//! No sentence here carries SQL, a server name or a connection detail.

use super::preconditions::{PreconditionCheck, PreconditionDb};
use super::setup_find::{find_in_database, DbOutcome, RowSource};
use super::setups::{self, SetupFile};
use super::{CaseScript, NeededRecord};
use crate::api_templates::gate::StageDb;
use std::path::Path;

/// Said on the case, never blocking, when the Database Access module is off
/// and its records could not be asked.
pub const NOT_CHECKED: &str = "needs_records not checked: database read access is off";

/// What asking for a case's needed records decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeedsOutcome {
    /// Every record is there.
    Met,
    /// The Blocked sentence: a record is not there, or could not be looked for.
    Missing(String),
    /// Nothing was asked (`NOT_CHECKED`); the case goes on.
    NotChecked(String),
}

/// Every problem with a script's declaration against the project's saved
/// setups, one sentence each. Empty means it may be saved.
pub fn check_declared(needed: &[NeededRecord], setups: &SetupFile) -> Vec<String> {
    needed
        .iter()
        .filter_map(|n| {
            let Some(setup) = setups::find_setup(setups, &n.setup) else {
                return Some(format!(
                    "needs_records names \"{}\", which is not a saved setup - save it with save_setup first",
                    n.setup.trim()
                ));
            };
            setup.find.database.is_none().then(|| {
                format!(
                    "needs_records names \"{}\", which has no database find rule, so a run cannot look for its record before signing in",
                    setup.name
                )
            })
        })
        .collect()
}

/// The save-time check every save path makes: each script's declaration
/// against the project's saved setups. The setups are read only when a
/// script declares any. A refusal names the case, then every problem.
pub fn check_saved(root: &Path, org: &str, project: &str, scripts: &[CaseScript]) -> Result<(), String> {
    if scripts.iter().all(|s| s.needs_records.is_empty()) {
        return Ok(());
    }
    let file = setups::load_setups(root, org, project)?;
    let refused: Vec<String> = scripts
        .iter()
        .filter_map(|s| {
            let found = check_declared(&s.needs_records, &file);
            (!found.is_empty()).then(|| format!("case {}: {}", s.case_id, found.join("; ")))
        })
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(refused.join("; "))
    }
}

/// Asks each needed record's setup find rule, in order, and stops at the
/// first that is not there. `database_ready` is whether the database may be
/// read at all; when it may not, nothing is asked.
pub async fn check_needed<S: RowSource>(
    rows: &S,
    needed: &[NeededRecord],
    setups: &SetupFile,
    prefix: &str,
    database_ready: bool,
) -> NeedsOutcome {
    if needed.is_empty() {
        return NeedsOutcome::Met;
    }
    if !database_ready {
        return NeedsOutcome::NotChecked(NOT_CHECKED.to_string());
    }
    for n in needed {
        // Both can have changed since the script was saved: a setup deleted
        // or its rule dropped is said, never taken for a record that is there.
        let Some(setup) = setups::find_setup(setups, &n.setup) else {
            return NeedsOutcome::Missing(format!(
                "needs_records could not be checked: \"{}\" is not a saved setup any more",
                n.setup.trim()
            ));
        };
        let Some(db) = &setup.find.database else {
            return NeedsOutcome::Missing(format!(
                "needs_records could not be checked: setup \"{}\" has no database find rule",
                setup.name
            ));
        };
        match find_in_database(rows, db, prefix).await {
            DbOutcome::Found(_) => {}
            DbOutcome::NotFound => {
                return NeedsOutcome::Missing(format!(
                    "Blocked: the case needs {} (setup \"{}\"), which is not there - run_setup {} builds it",
                    n.why.trim(),
                    setup.name,
                    setup.name
                ));
            }
            DbOutcome::CouldNotRun(said) => {
                crate::applog::warn(format!("Auto Run needs_records: the find rule of setup {} did not run: {said}", setup.name));
                return NeedsOutcome::Missing(format!(
                    "needs_records could not be checked: the find rule of setup \"{}\" did not run - see Settings, Logs",
                    setup.name
                ));
            }
        }
    }
    NeedsOutcome::Met
}

/// A run's database as a place a find rule's SELECT is asked.
struct Via<'a, D>(&'a D);

impl<D: StageDb> RowSource for Via<'_, D> {
    async fn select(&self, sql: &str) -> Result<String, String> {
        self.0.select(sql).await
    }
}

/// One case's needed records, as the run checks them before its sign-in.
/// Nothing is asked, the database is never looked at, and nothing is said,
/// when the case declares none. While reading is off nothing is asked and
/// the case goes on with `NOT_CHECKED`.
pub async fn check_case_needs<D: StageDb>(
    db: &PreconditionDb<D>,
    root: &Path,
    org: &str,
    project: &str,
    needed: &[NeededRecord],
) -> PreconditionCheck {
    if needed.is_empty() {
        return PreconditionCheck::default();
    }
    let db = match db {
        PreconditionDb::ReadingOff => {
            return PreconditionCheck { blocked: None, notice: Some(NOT_CHECKED.to_string()) }
        }
        PreconditionDb::Missing(why) => return PreconditionCheck { blocked: Some(why.clone()), notice: None },
        PreconditionDb::Ready(db) => db,
    };
    let file = match setups::load_setups(root, org, project) {
        Ok(f) => f,
        Err(why) => return PreconditionCheck { blocked: Some(format!("needs_records could not be checked: {why}")), notice: None },
    };
    let prefix = crate::environments::active(root)
        .map(|e| e.test_prefix)
        .unwrap_or_else(|_| crate::environments::default_test_prefix());
    match check_needed(&Via(db), needed, &file, &prefix, true).await {
        NeedsOutcome::Met => PreconditionCheck::default(),
        NeedsOutcome::Missing(why) => PreconditionCheck { blocked: Some(why), notice: None },
        NeedsOutcome::NotChecked(said) => PreconditionCheck { blocked: None, notice: Some(said) },
    }
}

/// A script's preconditions, then its needed records: what the run asks
/// before the case signs in. The first that blocks decides; the notices of
/// both are kept.
pub async fn check_script_case<D: StageDb>(
    db: &PreconditionDb<D>,
    root: &Path,
    org: &str,
    project: &str,
    script: &CaseScript,
) -> PreconditionCheck {
    let mut checked = super::preconditions::check_case(db, root, org, project, &script.preconditions).await;
    if checked.blocked.is_some() {
        return checked;
    }
    let needs = check_case_needs(db, root, org, project, &script.needs_records).await;
    checked.blocked = needs.blocked;
    checked.notice = match (checked.notice, needs.notice) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (a, b) => a.or(b),
    };
    checked
}
