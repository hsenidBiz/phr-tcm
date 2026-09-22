//! The declared-edit gate: what an assistant must say before it may change
//! an existing Auto Run script, and how many unsupervised repairs a script
//! may take before a person has to look at it again.
//!
//! Pure functions over `CaseScript` - no browser, no filesystem. Nothing
//! here judges whether an edit is a GOOD idea, only whether it was
//! honestly declared and never quietly removed an assertion.

use super::{CaseScript, StepScript};
use std::collections::{BTreeMap, BTreeSet};

/// What an assistant declares before it may change an existing script.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub case_id: i32,
    /// Every step whose actions change, are added or are removed.
    pub steps: Vec<i32>,
    /// One sentence: what was wrong and what this changes.
    pub why: String,
    /// Something learned about the application that the next script needs
    /// to know. Recorded as a project quirk.
    #[serde(default)]
    pub quirk: Option<String>,
}

/// How many times a script may be repaired by an assistant before a person
/// must open it in the app and save it there, which resets the count.
pub const MAX_REPAIRS: u32 = 3;

/// A step's actions as the text the gate compares. Order matters (a
/// reordered script is a different script); JSON formatting does not. The
/// `unchecked` reason is folded in too, so changing just the reason still
/// counts as a change to the step.
pub fn step_signature(step: &StepScript) -> String {
    format!(
        "{}|{}",
        serde_json::to_string(&step.actions).unwrap_or_default(),
        step.unchecked.as_deref().unwrap_or("")
    )
}

/// How many of a step's actions JUDGE the page, rather than drive or wait
/// for it.
fn checks(step: &StepScript) -> usize {
    step.actions.iter().filter(|a| a.is_check()).count()
}

/// Rule 1's sentence, for one or several undeclared steps together, sorted
/// ascending.
fn undeclared_sentence(nums: &BTreeSet<i32>) -> String {
    let list = nums.iter().map(|n| format!("step {n}")).collect::<Vec<_>>().join(", ");
    let verb = if nums.len() == 1 { "was" } else { "were" };
    format!(
        "{list} {verb} changed but not declared - name every step you change in \"edits\", or leave it as it was"
    )
}

/// Rule 2's sentence, for one or several declared-but-unchanged steps
/// together, sorted ascending.
fn declared_but_unchanged_sentence(nums: &BTreeSet<i32>) -> String {
    let list = nums.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    if nums.len() == 1 {
        format!("step {list} was declared but not changed")
    } else {
        format!("steps {list} were declared but not changed")
    }
}

/// The lowest step number that appears more than once in a script's steps,
/// if any. A duplicate step number is what let a weakened copy hide behind
/// an untouched one in the `BTreeMap` this module used to build straight
/// from `old.steps` / `new.steps` - the map keeps only the last entry, but
/// the runner still executes every entry in the `Vec`.
fn duplicate_step_number(steps: &[StepScript]) -> Option<i32> {
    let mut seen = std::collections::HashSet::new();
    let mut dupes = BTreeSet::new();
    for s in steps {
        if !seen.insert(s.step_number) {
            dupes.insert(s.step_number);
        }
    }
    dupes.into_iter().next()
}

/// Refuses an undeclared or weakening change. `old` is the script on disk.
pub fn check_edits(old: &CaseScript, new: &CaseScript, declared: Option<&Edit>) -> Result<(), String> {
    // A duplicated step number defeats every check below it (the map built
    // from the Vec would silently keep only one of the two entries while
    // the runner executes both), so it is refused before anything else is
    // even compared.
    if let Some(n) = duplicate_step_number(&old.steps).or_else(|| duplicate_step_number(&new.steps)) {
        return Err(format!("step {n} appears more than once in the script"));
    }

    // A repair never moves a script to a different case - that is what
    // `case_id` on disk means, and an assistant editing the wrong file is
    // exactly the mistake this whole module exists to catch.
    if old.case_id != new.case_id {
        return Err("a repair cannot change which test case a script belongs to".to_string());
    }
    if let Some(e) = declared {
        if e.case_id != old.case_id {
            return Err(format!(
                "the declaration names case {} but this script is case {}",
                e.case_id, old.case_id
            ));
        }
    }

    // Rule 7: a declaration with no reason is refused before anything else
    // about it is even looked at.
    if let Some(e) = declared {
        if e.why.trim().is_empty() {
            return Err("an edit needs a reason".to_string());
        }
    }

    // Rule 5: only a person, in the app, picks who a script signs in as -
    // never a repair, declared or not.
    if old.account != new.account {
        return Err(
            "the account a script runs as cannot be changed by a repair - a person picks it in the app"
                .to_string(),
        );
    }

    let old_map: BTreeMap<i32, &StepScript> = old.steps.iter().map(|s| (s.step_number, s)).collect();
    let new_map: BTreeMap<i32, &StepScript> = new.steps.iter().map(|s| (s.step_number, s)).collect();
    let all_numbers: BTreeSet<i32> = old_map.keys().chain(new_map.keys()).copied().collect();

    // `replay::run_case` runs `script.steps` in Vec order, so the ORDER of
    // the steps is behaviour, not just their content. Compared here as the
    // sequence of step numbers common to both sides, taken in each side's
    // own file order (not the maps' sorted order) - a step added or removed
    // is not a reorder, so only numbers present on both sides count. This
    // runs before rule 1 so a script whose steps were merely shuffled -
    // same signatures, different order - is caught here rather than
    // silently passing rule 1's signature comparison.
    let common: BTreeSet<i32> = old_map.keys().copied().filter(|n| new_map.contains_key(n)).collect();
    let old_order: Vec<i32> = old.steps.iter().map(|s| s.step_number).filter(|n| common.contains(n)).collect();
    let new_order: Vec<i32> = new.steps.iter().map(|s| s.step_number).filter(|n| common.contains(n)).collect();
    if old_order != new_order {
        return Err("the steps are in a different order - a repair does not reorder a script".to_string());
    }

    // Rule 1's basis: a step present in only one side, or whose signature
    // (actions plus unchecked reason) differs between the two.
    let mut changed: BTreeSet<i32> = BTreeSet::new();
    for n in &all_numbers {
        match (old_map.get(n), new_map.get(n)) {
            (Some(o), Some(nw)) if step_signature(o) == step_signature(nw) => {}
            _ => {
                changed.insert(*n);
            }
        }
    }

    let declared_steps: BTreeSet<i32> =
        declared.map(|e| e.steps.iter().copied().collect()).unwrap_or_default();

    // Rule 1 / Rule 9: every changed step must be named. With no
    // declaration at all, the whole sentence is prefixed with the case id
    // so it can be told apart from the case that IS named but incomplete.
    let undeclared: BTreeSet<i32> = changed.difference(&declared_steps).copied().collect();
    if !undeclared.is_empty() {
        let sentence = undeclared_sentence(&undeclared);
        return Err(match declared {
            None => format!("case {}: {sentence}", old.case_id),
            Some(_) => sentence,
        });
    }

    // Rule 2: a declaration that names a step nothing happened to is the
    // same blast-radius hazard from the other side.
    let over_declared: BTreeSet<i32> = declared_steps.difference(&changed).copied().collect();
    if !over_declared.is_empty() {
        return Err(declared_but_unchanged_sentence(&over_declared));
    }

    // Rule 3: a repair never removes or weakens an assertion, declared or
    // not.
    for n in &changed {
        let old_checks = old_map.get(n).map(|s| checks(s)).unwrap_or(0);
        let new_checks = new_map.get(n).map(|s| checks(s)).unwrap_or(0);
        if new_checks < old_checks {
            return Err(format!(
                "step {n} had {old_checks} checks and now has {new_checks} - an assertion is never removed or weakened by a repair"
            ));
        }
    }

    Ok(())
}

/// The repair count to write: one more than before, or a refusal once the
/// cap is reached.
pub fn next_repairs(old: &CaseScript) -> Result<u32, String> {
    if old.repairs >= MAX_REPAIRS {
        return Err(format!(
            "this script has been repaired {MAX_REPAIRS} times without a person looking at it - open it in the app, save it there, and the count starts again"
        ));
    }
    Ok(old.repairs + 1)
}
