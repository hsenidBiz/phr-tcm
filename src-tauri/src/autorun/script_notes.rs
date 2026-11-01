//! Notes the script save gives about a delete that can go wrong: a delete of
//! what the case made that runs after a check (a failing check stops the
//! step, so the record stays for the next run), and a delete that picks its
//! row by position on a list other records share.
//!
//! Pure checks on a script. A note is a warning in the save's answer, never
//! a refusal. A delete that is the case's own subject, and the confirm
//! button of a modal the step just opened, are not noted.

use super::components::{expand_actions, ComponentFile};
use super::{CaseScript, StepScript};
use crate::browser::actions::Action;
use crate::browser::locator::LocatorStep;

/// What makes a click a delete: a word of its name or text, or a part of
/// its css (a class, an id or an aria-label).
const DELETE_WORDS: [&str; 4] = ["delete", "remove", "discard", "trash"];

/// Words that make a "remove" or "discard" something other than a delete of
/// a record: "Remove filter", "Discard changes", "Remove selection".
const NOT_A_DELETE: [&str; 6] = ["filter", "filters", "selection", "selected", "changes", "change"];

fn words(t: &str) -> Vec<String> {
    t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// Whether one link of a locator names a delete.
fn link_is_delete(l: &LocatorStep) -> bool {
    let spoken: Vec<String> = [l.name.as_deref(), l.text.as_deref()].into_iter().flatten().flat_map(words).collect();
    if spoken.iter().any(|w| NOT_A_DELETE.contains(&w.as_str())) {
        return false;
    }
    if spoken.iter().any(|w| DELETE_WORDS.contains(&w.as_str())) {
        return true;
    }
    l.css.as_deref().map(str::to_lowercase).is_some_and(|c| {
        !NOT_A_DELETE.iter().any(|n| c.contains(n)) && DELETE_WORDS.iter().any(|d| c.contains(d))
    })
}

/// Whether `a` is a click that deletes: its target's name, text or css holds
/// `delete`, `remove`, `discard` or `trash`.
pub fn is_delete_click(a: &Action) -> bool {
    let Action::Click { selector } = a else { return false };
    selector.links().iter().any(link_is_delete)
}

/// The name, text or has_text of the links that name the record (not the delete
/// button itself), folded.
fn record_names(a: &Action) -> Vec<String> {
    let Action::Click { selector } = a else { return Vec::new() };
    selector
        .links()
        .iter()
        .flat_map(|l| {
            // has_text names the row even on the delete button's own link.
            let own = if link_is_delete(l) { None } else { Some([l.name.clone(), l.text.clone()]) };
            own.into_iter().flatten().chain([l.has_text.clone()])
        })
        .flatten()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// The `nth` a click picks by, when nothing in its locator names the row
/// (a name or text of a link that is not itself the delete).
fn positional_nth(a: &Action) -> Option<i32> {
    let Action::Click { selector } = a else { return None };
    if !record_names(a).is_empty() {
        return None;
    }
    selector.links().iter().find_map(|l| l.nth)
}

fn listed(only_steps: Option<&[i32]>, step: i32) -> bool {
    only_steps.is_none_or(|o| o.contains(&step))
}

/// A step's actions as they run, `when_visible` bodies and components
/// expanded (a component that cannot be expanded is read as written).
fn flat(step: &StepScript, components: Option<&ComponentFile>) -> Vec<Action> {
    let written: Vec<Action> = step.actions.iter().flat_map(|a| a.judging()).cloned().collect();
    match components.map(|f| expand_actions(f, &step.actions)) {
        Some(Ok((expanded, _))) => expanded.iter().flat_map(|a| a.judging()).cloned().collect(),
        _ => written,
    }
}

fn click_words(a: &Action) -> Vec<String> {
    let Action::Click { selector } = a else { return Vec::new() };
    selector
        .links()
        .iter()
        .flat_map(|l| [l.name.clone(), l.text.clone(), l.css.clone()])
        .flatten()
        .flat_map(|t| words(&t))
        .collect()
}

/// Whether `a` creates or saves something.
fn makes_something(a: &Action) -> bool {
    !is_delete_click(a)
        && click_words(a).iter().any(|w| {
            crate::browser::save_guard::SAVE_WORDS.contains(&w.as_str())
                || crate::browser::save_guard::WORD_SAVES.contains(&w.as_str())
                || crate::browser::save_guard::CLICK_WORDS.contains(&w.as_str())
                || w == "new"
        })
}

/// Whether `a` shows or checks a dialog (a modal the step opened).
fn is_dialogish(a: &Action) -> bool {
    match a {
        Action::ExpectDialog { .. } => true,
        _ if a.is_check() => a
            .targets()
            .iter()
            .any(|t| t.links().iter().any(|l| matches!(l.role.as_deref(), Some("dialog") | Some("alertdialog")))),
        _ => false,
    }
}

/// For each case step (never the clean-up) with a delete click of something
/// the case made earlier, after a check: one note. A delete in a step that
/// first opened its own confirm (a delete click, a dialog) is the confirm
/// button and is left alone, and so is a delete when the script made
/// nothing before it - then the delete is what the case tests.
pub fn delete_after_check_notes(
    script: &CaseScript,
    components: Option<&ComponentFile>,
    only_steps: Option<&[i32]>,
) -> Vec<String> {
    let mut notes = Vec::new();
    let mut checked = false;
    let mut made = script.setup.is_some();
    let mut filled: Vec<String> = Vec::new();
    for step in script.case_steps() {
        let actions = flat(step, components);
        let mut noted = false;
        for (j, a) in actions.iter().enumerate() {
            if let Action::Fill { value, .. } = a {
                let v = value.trim().to_lowercase();
                if v.chars().count() >= 3 {
                    filled.push(v);
                }
            }
            if is_delete_click(a) {
                let confirm = actions[..j].iter().any(|b| is_delete_click(b) || is_dialogish(b))
                    || actions[j + 1..].iter().any(is_dialogish);
                let names = record_names(a);
                let ours = names.is_empty()
                    || names.iter().any(|n| filled.iter().any(|f| n.contains(f.as_str()) || f.contains(n.as_str())));
                if checked && made && ours && !confirm {
                    noted = true;
                }
            }
            if makes_something(a) {
                made = true;
            }
            if a.is_check() {
                checked = true;
            }
        }
        if noted && listed(only_steps, step.step_number) {
            notes.push(format!(
                "step {} deletes what the case made after a check; put it in the clean-up step so it runs even when an earlier check fails, unless the case asks for the delete: then keep it in its step with its check and add a guarded fallback to the clean-up",
                step.step_number
            ));
        }
    }
    notes
}

/// The positional pick in one run of actions: the `nth` and whether the
/// click is a delete, for a click that picks by position and either is a
/// delete or is followed, before any other click, by a confirm dialog.
fn positional_picks(actions: &[Action]) -> Option<(i32, bool)> {
    actions.iter().enumerate().find_map(|(j, a)| {
        let nth = positional_nth(a)?;
        let dialog_next = actions[j + 1..]
            .iter()
            .take_while(|b| !matches!(b, Action::Click { .. }))
            .any(|b| matches!(b, Action::ExpectDialog { .. }));
        let delete = is_delete_click(a);
        (delete || dialog_next).then_some((nth, delete))
    })
}

fn picks_sentence(pick: (i32, bool)) -> String {
    let (nth, delete) = pick;
    let what = if delete { "the delete" } else { "the click before the delete's confirm" };
    format!(
        "{what} picks by position (nth {nth}) - on a shared list another record can sit there; find the row by its own name (has_text) before deleting"
    )
}

/// For each step with a click that picks by position and deletes (or opens
/// a delete's confirm dialog in the same step): one note.
pub fn positional_delete_notes(
    script: &CaseScript,
    components: Option<&ComponentFile>,
    only_steps: Option<&[i32]>,
) -> Vec<String> {
    script
        .steps
        .iter()
        .filter(|s| listed(only_steps, s.step_number))
        .filter_map(|s| {
            positional_picks(&flat(s, components)).map(|p| format!("step {}: {}", s.step_number, picks_sentence(p)))
        })
        .collect()
}

/// The same note for a component's actions, which have no steps.
pub fn positional_delete_notes_for_actions(actions: &[Action]) -> Vec<String> {
    let flat: Vec<Action> = actions.iter().flat_map(|a| a.judging()).cloned().collect();
    positional_picks(&flat).map(picks_sentence).into_iter().collect()
}

/// CSS that picks by place in a list, not by what the row holds.
const POSITIONAL_CSS: [&str; 5] = [":first-child", ":last-child", ":nth-child(", ":first-of-type", ":nth-of-type("];

/// The steps (by number) with a locator that picks by position: an `nth`
/// on any link, or CSS naming a place. Guards and components are read as
/// they run.
fn positional_steps(script: &CaseScript, components: Option<&ComponentFile>) -> Vec<i32> {
    script
        .case_steps()
        .filter(|s| {
            flat(s, components).iter().flat_map(|a| a.each()).any(|a| {
                a.targets().iter().flat_map(|t| t.links()).any(|l| {
                    l.nth.is_some()
                        || l.css.as_deref().map(str::to_lowercase).is_some_and(|c| POSITIONAL_CSS.iter().any(|p| c.contains(p)))
                })
            })
        })
        .map(|s| s.step_number)
        .collect()
}

/// How many names an order note lists before "and N more".
const ORDER_NOTE_NAMES: usize = 10;

fn capped(mut names: Vec<String>) -> String {
    let more = names.len().saturating_sub(ORDER_NOTE_NAMES);
    names.truncate(ORDER_NOTE_NAMES);
    let mut out = names.join(", ");
    if more > 0 {
        out.push_str(&format!(", and {more} more"));
    }
    out
}

fn area_of(s: &CaseScript) -> String {
    s.area_name().unwrap_or("the default area").to_string()
}

/// Notes when one script changes shared state in an area and another picks
/// a row of that area by position: a row deleted and re-applied goes to the
/// end of its list, so the position moves. `saved` holds the other scripts
/// of the same project; only those in `saving`'s area are read (`changes`
/// names no list, so the area is the join). Never a refusal.
pub fn order_notes(saving: &CaseScript, saved: &[CaseScript]) -> Vec<String> {
    order_notes_with(saving, saved, None)
}

/// `order_notes`, with the components a script uses expanded.
pub fn order_notes_with(saving: &CaseScript, saved: &[CaseScript], components: Option<&ComponentFile>) -> Vec<String> {
    let area = area_of(saving);
    let area_ref = &area;
    let others = || {
        saved.iter().filter(move |o| o.case_id != saving.case_id && area_of(o).eq_ignore_ascii_case(area_ref))
    };
    let mut notes = Vec::new();
    if !saving.changes.is_empty() {
        let pickers: Vec<String> = others()
            .flat_map(|o| positional_steps(o, components).into_iter().map(move |st| format!("case {} step {st}", o.case_id)))
            .collect();
        if !pickers.is_empty() {
            notes.push(format!(
                "case {} changes {} in area {area}; these scripts pick a row there by position and may break when a row is deleted and re-applied (it goes to the end): {}",
                saving.case_id,
                saving.changes.join(", "),
                capped(pickers)
            ));
        }
    }
    let steps = positional_steps(saving, components);
    if !steps.is_empty() {
        let changers: Vec<String> =
            others().filter(|o| !o.changes.is_empty()).map(|o| format!("case {} ({})", o.case_id, o.changes.join(", "))).collect();
        if !changers.is_empty() {
            let at: Vec<String> = steps.iter().map(|s| s.to_string()).collect();
            notes.push(format!(
                "case {} picks a row by position in area {area} (step {}); these scripts change state there and a row deleted and re-applied goes to the end, which moves the position: {}",
                saving.case_id,
                at.join(", "),
                capped(changers)
            ));
        }
    }
    notes
}

/// What the save says when a delete moves out of a case step: the case asked
/// for it, so it stays in its step with its check.
pub const KEEP_THE_CASES_DELETE: &str =
    "a delete the case asks for stays in its step with its check; the clean-up holds a guarded fallback";

/// An action as the script compares it: its kind and its target.
fn identity(a: &Action) -> (String, String) {
    (a.kind(), a.targets().first().map(|t| t.describe()).unwrap_or_default())
}

fn judged(actions: &[Action]) -> usize {
    actions.iter().flat_map(|a| a.judging()).filter(|a| a.is_check()).count()
}

/// One note per case step whose action went to the clean-up step while the
/// step lost a check in the same save. Never a refusal. Empty when there is
/// no old script or the new one has no clean-up step.
pub fn moved_with_check(old: &CaseScript, new: &CaseScript) -> Vec<String> {
    let Some(cleanup) = new.cleanup_step() else { return Vec::new() };
    let in_cleanup: Vec<(String, String)> =
        cleanup.actions.iter().flat_map(|a| a.each()).map(identity).collect();
    let mut notes = Vec::new();
    for old_step in old.case_steps() {
        let Some(new_step) = new.case_steps().find(|s| s.step_number == old_step.step_number) else { continue };
        if judged(&new_step.actions) >= judged(&old_step.actions) {
            continue;
        }
        let kept: Vec<(String, String)> = new_step.actions.iter().flat_map(|a| a.each()).map(identity).collect();
        let moved = old_step
            .actions
            .iter()
            .flat_map(|a| a.each())
            .filter(|a| !a.is_check())
            .map(|a| identity(a))
            .find(|id| !kept.contains(id) && in_cleanup.contains(id));
        if let Some((kind, target)) = moved {
            let what = if target.is_empty() { kind } else { format!("{kind} on {target}") };
            notes.push(format!(
                "step {}: {what} moved to the clean-up and the step lost a check - {KEEP_THE_CASES_DELETE}",
                old_step.step_number
            ));
        }
    }
    notes
}
