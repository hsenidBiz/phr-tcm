//! Flaky cases: a case that both passed and failed in its recent runs on one environment.
//!
//! Pure reading of the local run files - nothing here asks Azure DevOps for anything. The person's
//! "Flaky" mark lives on the case's script (`CaseScript::flaky`, written by `store::set_flaky`); this
//! module only finds the candidates and says which of them are marked.

use super::failures::outcome_of;
use super::{CaseRecord, LocalRun};

/// How many of a case's newest runs on one environment are looked at.
pub const FLAKY_WINDOW: usize = 10;

/// One case that passed and failed within its window of runs on one environment.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FlakyCase {
    pub case_id: i32,
    /// The title as the newest of those runs has it.
    pub title: String,
    /// The environment the runs were made in; `None` for runs saved before environments existed.
    pub environment: Option<String>,
    /// One entry per run that passed or failed the case, oldest first: true is a pass.
    pub pattern: Vec<bool>,
    /// What the newest failure said: the failing action's own words, else the case's note.
    pub last_failure: Option<String>,
    /// Epoch milliseconds as a string (that run's `started_at`).
    pub last_failed_at: String,
    /// Whether the person has marked the case flaky. Always false from [`flaky_cases`]; set by the command.
    pub marked: bool,
}

/// Some(true) for a pass, Some(false) for a fail, None for anything else (Blocked, not judged).
fn passed(case: &CaseRecord) -> Option<bool> {
    match outcome_of(case) {
        "Passed" => Some(true),
        "Failed" => Some(false),
        _ => None,
    }
}

/// The failing action's detail, else the case's note, if either says anything.
fn failure_words(case: &CaseRecord) -> Option<String> {
    let step_words = case
        .steps
        .iter()
        .flat_map(|s| s.outcomes.iter())
        .find(|o| !o.ok && !o.detail.trim().is_empty())
        .map(|o| o.detail.trim().to_string());
    step_words.or_else(|| Some(case.note.trim().to_string()).filter(|n| !n.is_empty()))
}

fn at_ms(at: &str) -> u64 {
    at.trim().parse().unwrap_or(0)
}

fn started(run: &LocalRun) -> u64 {
    at_ms(&run.started_at)
}

/// Every case that, among its newest `window` runs on one environment, has at least one pass and at least
/// one fail. A run where the case was Blocked or not judged counts toward the window but is neither.
/// Ordered by the time of the newest failure, latest first.
pub fn flaky_cases(runs: &[LocalRun], window: usize) -> Vec<FlakyCase> {
    let mut newest_first: Vec<&LocalRun> = runs.iter().collect();
    newest_first.sort_by(|a, b| started(b).cmp(&started(a)).then_with(|| b.started_at.cmp(&a.started_at)));

    // (environment, case id) in first-seen order, which is newest-first.
    let mut keys: Vec<(Option<String>, i32)> = Vec::new();
    for run in &newest_first {
        for case in &run.cases {
            let key = (run.environment.clone(), case.case_id);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }

    let mut out: Vec<FlakyCase> = Vec::new();
    for (environment, case_id) in keys {
        let seen: Vec<(&LocalRun, &CaseRecord)> = newest_first
            .iter()
            .filter(|r| r.environment == environment)
            .filter_map(|r| r.cases.iter().find(|c| c.case_id == case_id).map(|c| (*r, c)))
            .take(window)
            .collect();
        let judged: Vec<(&LocalRun, &CaseRecord, bool)> =
            seen.iter().filter_map(|(r, c)| passed(c).map(|p| (*r, *c, p))).collect();
        if !judged.iter().any(|j| j.2) || !judged.iter().any(|j| !j.2) {
            continue;
        }
        let (fail_run, fail_case, _) = judged.iter().find(|j| !j.2).expect("a failure was found above");
        out.push(FlakyCase {
            case_id,
            title: seen[0].1.title.clone(),
            environment,
            pattern: judged.iter().rev().map(|j| j.2).collect(),
            last_failure: failure_words(fail_case),
            last_failed_at: fail_run.started_at.clone(),
            marked: false,
        });
    }
    out.sort_by(|a, b| at_ms(&b.last_failed_at).cmp(&at_ms(&a.last_failed_at)).then(a.case_id.cmp(&b.case_id)));
    out
}

/// [`flaky_cases`] with each case's mark filled in from `marked` (the ids whose script carries it).
pub fn with_marks(mut cases: Vec<FlakyCase>, marked: &[i32]) -> Vec<FlakyCase> {
    for c in &mut cases {
        c.marked = marked.contains(&c.case_id);
    }
    cases
}
