//! A script's `page_errors`: what a step does with the page's own errors -
//! uncaught script errors and 5xx answers, from every tab of the run, since
//! the previous step was judged (`browser::page_errors`).
//!
//! - **`fail`**: the step fails with the first error's sentence, and
//!   `(and <n> more)` for the rest. A step that already failed keeps its
//!   own failure, with the errors said after it as a note.
//! - **`flag`**: the step is judged as usual; the errors are listed after
//!   its last action's words, and the case counts them
//!   (`CaseRecord::page_errors_seen`).
//! - **Absent**: nothing changes - they are read and let go.
//!
//! Errors from before step 1 (the sign-in, the trip to the module) are not
//! any step's: the run drops them before step 1, and every sign-in drops
//! what it met itself (`signin`). The run's own `api_request` answers are
//! named as its own where they are sent (`runner`). A setup fixture runs in
//! a browser of its own, so its requests never reach the case's.

use super::{CaseScript, PageErrors};
use crate::browser::actions::ActionOutcome;
use crate::browser::cdp::Driver;
use crate::browser::page_errors::{counted, note, summary};

/// The most phrases `ignore_page_errors` holds.
pub const MAX_PHRASES: usize = 10;
/// The longest phrase.
pub const MAX_PHRASE_CHARS: usize = 120;
pub const TOO_MANY: &str = "ignore_page_errors holds at most 10 phrases";
pub const EMPTY_PHRASE: &str = "ignore_page_errors: a phrase cannot be empty";
pub const LONG_PHRASE: &str = "ignore_page_errors: a phrase is at most 120 characters";

/// What is wrong with one script's ignore phrases, if anything.
pub fn check_phrases(phrases: &[String]) -> Result<(), String> {
    if phrases.len() > MAX_PHRASES {
        return Err(TOO_MANY.to_string());
    }
    if phrases.iter().any(|p| p.trim().is_empty()) {
        return Err(EMPTY_PHRASE.to_string());
    }
    if phrases.iter().any(|p| p.chars().count() > MAX_PHRASE_CHARS) {
        return Err(LONG_PHRASE.to_string());
    }
    Ok(())
}

/// The save-time check every save path makes. A refusal names the case.
pub fn check_saved(scripts: &[CaseScript]) -> Result<(), String> {
    let refused: Vec<String> = scripts
        .iter()
        .filter_map(|s| check_phrases(&s.ignore_page_errors).err().map(|why| format!("case {}: {why}", s.case_id)))
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(refused.join("; "))
    }
}

/// Forget every page error read so far: what came before step 1.
pub fn drop_all<D: Driver>(d: &mut D) {
    if let Some(book) = d.page_error_book() {
        book.take();
    }
}

/// What judging a step's page errors did.
#[derive(Debug, Default, PartialEq)]
pub struct Judged {
    /// How many a `flag` counted (0 for a `fail`, or none).
    pub seen: u32,
    /// The outcome a `fail` turned into the step's failure, by index - for
    /// the caller to picture, as it pictures any failed action.
    pub failed: Option<usize>,
}

/// Judge the page errors since the previous step on the step's outcomes,
/// as `mode` says. A step with no outcomes that fails is given one, so the
/// failure is said.
pub fn judge_step<D: Driver>(
    d: &mut D,
    mode: Option<PageErrors>,
    ignore: &[String],
    outcomes: &mut Vec<ActionOutcome>,
) -> Judged {
    let taken = d.page_error_book().map(|b| b.take()).unwrap_or_default();
    let Some(mode) = mode else {
        return Judged::default();
    };
    let errors = counted(taken, ignore);
    let Some(said) = summary(&errors) else {
        return Judged::default();
    };
    match mode {
        PageErrors::Fail => {
            if let Some(failed) = outcomes.iter_mut().find(|o| !o.ok) {
                failed.detail.push_str(&format!("{}{said})", crate::browser::page_errors::NOTE));
                return Judged::default();
            }
            match outcomes.last_mut() {
                Some(last) => *last = ActionOutcome::failed(said),
                None => outcomes.push(ActionOutcome::failed(said)),
            }
            Judged { seen: 0, failed: Some(outcomes.len() - 1) }
        }
        PageErrors::Flag => {
            if let Some(last) = outcomes.last_mut() {
                last.detail.push_str(&note(&errors));
            }
            Judged { seen: errors.len() as u32, failed: None }
        }
    }
}
