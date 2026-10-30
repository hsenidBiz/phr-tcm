//! Notes the script save gives about a delete that can go wrong: one that
//! runs after a check (a failing check stops the step, so the record stays
//! for the next run), and one that picks its row by position on a list
//! other records share.
//!
//! Pure checks on a script. A note is a warning in the save's answer, never
//! a refusal.

use super::{CaseScript, StepScript};
use crate::browser::actions::Action;

/// The words that make a click a delete.
const DELETE_WORDS: [&str; 3] = ["delete", "remove", "discard"];

/// Whether `a` is a click whose target's name, text or css holds `delete`,
/// `remove` or `discard` as a word (case folded).
pub fn is_delete_click(a: &Action) -> bool {
    let Action::Click { selector } = a else { return false };
    selector.links().iter().any(|l| {
        [l.name.as_deref(), l.text.as_deref(), l.css.as_deref()]
            .into_iter()
            .flatten()
            .any(|t| {
                t.to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| DELETE_WORDS.contains(&w))
            })
    })
}

/// The `nth` a click's locator picks by, when any link has one.
fn click_nth(a: &Action) -> Option<i32> {
    let Action::Click { selector } = a else { return None };
    selector.links().iter().find_map(|l| l.nth)
}

fn listed(only_steps: Option<&[i32]>, step: i32) -> bool {
    only_steps.is_none_or(|o| o.contains(&step))
}

fn flat(step: &StepScript) -> Vec<&Action> {
    step.actions.iter().flat_map(|a| a.each()).collect()
}

/// For each case step (never the clean-up) with a delete click after a
/// check - in the step, or in an earlier step of the case - one note.
pub fn delete_after_check_notes(script: &CaseScript, only_steps: Option<&[i32]>) -> Vec<String> {
    let mut notes = Vec::new();
    let mut checked = false;
    for step in script.case_steps() {
        let mut delete_after_check = false;
        for a in flat(step) {
            if is_delete_click(a) && checked {
                delete_after_check = true;
            }
            if a.is_check() {
                checked = true;
            }
        }
        if delete_after_check && listed(only_steps, step.step_number) {
            notes.push(format!(
                "step {} deletes after a check: when the check fails the delete never runs and the record stays for the next run - move the delete into the clean-up step",
                step.step_number
            ));
        }
    }
    notes
}

/// For each step with a delete click that picks by `nth`, or a click that
/// picks by `nth` and is followed (in its step or the next) by a confirm
/// dialog, one note.
pub fn positional_delete_notes(script: &CaseScript, only_steps: Option<&[i32]>) -> Vec<String> {
    let mut notes = Vec::new();
    for (i, step) in script.steps.iter().enumerate() {
        if !listed(only_steps, step.step_number) {
            continue;
        }
        let actions = flat(step);
        let next_has_dialog = script
            .steps
            .get(i + 1)
            .is_some_and(|n| flat(n).iter().any(|a| matches!(a, Action::ExpectDialog { .. })));
        let picked = actions.iter().enumerate().find_map(|(j, a)| {
            let nth = click_nth(a)?;
            let dialog_follows = next_has_dialog
                || actions[j + 1..].iter().any(|b| matches!(b, Action::ExpectDialog { .. }));
            (is_delete_click(a) || dialog_follows).then_some(nth)
        });
        if let Some(nth) = picked {
            notes.push(format!(
                "step {}: the delete picks by position (nth {nth}) - on a shared list another record can sit there; find the row by its own name before deleting",
                step.step_number
            ));
        }
    }
    notes
}
