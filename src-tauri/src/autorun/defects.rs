//! The suspected application defect: an assistant's finding that a case's
//! script is right and the application did not do what the case expects.
//!
//! Pure checks only - no browser, no filesystem. The mark lives on the
//! case's script (`CaseScript::suspected_defect`) and is written by
//! `store::set_suspected_defect`; this module decides whether a mark may be
//! set at all, and makes its note safe to keep. A mark is not a repair: it
//! never changes the script's actions and never counts toward the cap.

use super::{failures, quirks, CaseScript, LocalRun, SuspectedDefect};

/// The longest note a mark keeps, in characters.
pub const MAX_NOTE: usize = 300;

/// Said when the case's newest run failed for one of the `STOP:` reasons
/// `failures::stop_reason` gives.
pub const STOP_REFUSAL: &str = "a STOP failure is not an application defect";

/// Said for a note with nothing in it.
pub const BLANK_NOTE: &str = "a suspected defect needs a note saying what the application did";

/// Said for a note over `MAX_NOTE` characters.
pub const LONG_NOTE: &str = "the note is longer than 300 characters";

/// The note as it is kept: trimmed, then through the scrub API-check
/// excerpts get - an address cut to its path, a query dropped, bearer
/// tokens and JWTs hidden - so a note quoting what the page showed cannot
/// carry a host or a token into a run's reason or a result comment.
pub fn scrub_note(note: &str) -> String {
    super::api_checks::shown_body(note.trim())
}

/// Whether `step_number` of case `case_id` may be marked as a suspected
/// application defect, and the mark to store when it may.
///
/// `run` is the newest run on this machine that holds the case, `script`
/// the case's script on disk. Refused, in this order, when the case has no
/// script, the step is not in it, the newest run's failure for the case is
/// a `STOP:` one (a sign-in that failed, a browser that stopped answering,
/// a case the person marked Blocked), the step did not fail in that run
/// (`quirks::source_from_run`'s rule and sentence), or the note is blank or
/// over 300 characters (measured on the trimmed note, before the scrub).
pub fn check_mark(
    run: Option<&LocalRun>,
    script: Option<&CaseScript>,
    case_id: i32,
    step_number: i32,
    note: &str,
    now_ms: u64,
) -> Result<SuspectedDefect, String> {
    let Some(script) = script else {
        return Err(format!("case {case_id} has no script to mark"));
    };
    if !script.steps.iter().any(|s| s.step_number == step_number) {
        return Err(format!("step {step_number} is not in the script"));
    }
    let Some(case) = run.and_then(|r| r.cases.iter().rev().find(|c| c.case_id == case_id)) else {
        return Err(format!("no run on this machine has case {case_id}"));
    };
    // Before the step check: a sign-in that failed never reaches the step
    // at all, and "it did not fail" would send the assistant the wrong way.
    if failures::stop_reason(case).is_some() {
        return Err(STOP_REFUSAL.to_string());
    }
    quirks::source_from_run(run, Some(script), case_id, &[step_number])?;
    let trimmed = note.trim();
    if trimmed.is_empty() {
        return Err(BLANK_NOTE.to_string());
    }
    if trimmed.chars().count() > MAX_NOTE {
        return Err(LONG_NOTE.to_string());
    }
    Ok(SuspectedDefect { step_number, note: scrub_note(trimmed), marked_at: now_ms.to_string() })
}
