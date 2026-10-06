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

/// Judge the page errors since the previous step on the step's outcomes,
/// as `mode` says. Returns how many were counted for a `flag` (0 for a
/// `fail` or none).
pub fn judge_step<D: Driver>(
    d: &mut D,
    mode: Option<PageErrors>,
    ignore: &[String],
    outcomes: &mut [ActionOutcome],
) -> u32 {
    let taken = d.page_error_book().map(|b| b.take()).unwrap_or_default();
    let Some(mode) = mode else {
        return 0;
    };
    let errors = counted(taken, ignore);
    let Some(said) = summary(&errors) else {
        return 0;
    };
    match mode {
        PageErrors::Fail => {
            if let Some(failed) = outcomes.iter_mut().find(|o| !o.ok) {
                failed.detail.push_str(&format!("{}{said})", crate::browser::page_errors::NOTE));
            } else if let Some(last) = outcomes.last_mut() {
                *last = ActionOutcome::failed(said);
            }
            0
        }
        PageErrors::Flag => {
            if let Some(last) = outcomes.last_mut() {
                last.detail.push_str(&note(&errors));
            }
            errors.len() as u32
        }
    }
}
