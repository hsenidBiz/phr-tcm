//! Whether a script counts as checked (`CaseScript::checked`): proven against the live app and not
//! red since. A run or replay of the case that fails or is Blocked clears it; a pass of the whole
//! case sets it again; a case with a divergence waiting for a ruling is never checked.

use super::replay_to::ReplayEnd;
use super::{divergences, store, CaseRecord};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Green,
    Red,
}

/// A run's case record: `proposed`, else the person's `verdict`: "Passed" -> Green; "Failed" or
/// "Blocked" -> Red; else None.
pub fn outcome_of_record(record: &CaseRecord) -> Option<Outcome> {
    let word = if record.proposed.is_empty() { record.verdict.as_str() } else { record.proposed.as_str() };
    match word {
        "Passed" => Some(Outcome::Green),
        "Failed" | "Blocked" => Some(Outcome::Red),
        _ => None,
    }
}

/// A replay's end: `StoppedAt` (any phase), `BrowserGone` with a `why`, and `Blocked` -> Red; `Ready`
/// with `whole: Some(n)` and `step > n` (the whole case ran) -> Green; anything else None.
pub fn outcome_of_replay(end: &ReplayEnd) -> Option<Outcome> {
    match end {
        ReplayEnd::StoppedAt { .. } | ReplayEnd::Blocked(_) => Some(Outcome::Red),
        ReplayEnd::BrowserGone { why: Some(_), .. } => Some(Outcome::Red),
        ReplayEnd::Ready { step, whole: Some(n), .. } if step > n => Some(Outcome::Green),
        _ => None,
    }
}

/// No divergence of the case blocks it; an unreadable file blocks nothing.
pub fn may_be_checked(root: &Path, org: &str, project: &str, case_id: i32) -> bool {
    divergences::needs_ruling(root, org, project, case_id).is_none()
}

/// After a case ended: Red clears the flag; Green sets it only when the script is stamped and
/// `may_be_checked`. Never fails the caller: an error is logged, naming the case id only.
pub fn note_outcome(root: &Path, case_id: i32, outcome: Outcome) {
    let result = match outcome {
        Outcome::Red => store::set_checked(root, case_id, false),
        Outcome::Green => match store::load_script(root, case_id) {
            Ok(Some(script)) => match (script.organization.as_deref(), script.project.as_deref()) {
                (Some(org), Some(project)) if may_be_checked(root, org, project, case_id) => {
                    store::set_checked(root, case_id, true)
                }
                _ => Ok(()),
            },
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        },
    };
    if result.is_err() {
        crate::applog::warn(format!("Auto Run: could not update whether case {case_id}'s script counts as checked"));
    }
}
