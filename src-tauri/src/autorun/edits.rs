//! The declared-edit gate: what an assistant must say before it may change
//! an existing Auto Run script, and how many unsupervised repairs a script
//! may take before a person has to look at it again.
//!
//! Pure functions over `CaseScript` - no browser, no filesystem. Nothing
//! here judges whether an edit is a GOOD idea, only whether it was
//! honestly declared and never quietly removed an assertion.

use super::nav::module_key;
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
    /// The repair moves the case to another area (or back to its module's
    /// default area, by leaving `area` out). Where a case starts
    /// is part of the script, so this is declared like a changed step.
    #[serde(default)]
    pub area: bool,
}

/// Said when a repair would turn `no_save` off.
pub const NO_SAVE_KEPT: &str =
    "this script is marked Must not save, and a repair cannot turn that off - send \"no_save\": true, or ask a person to change it in the app";

/// Said when a repair would change a script's preconditions.
pub const PRECONDITIONS_KEPT: &str =
    "a repair cannot change a script's preconditions - save the script itself to change them";

/// Said when a repair would add, change or remove a script's setup.
pub const SETUP_KEPT: &str =
    "a repair cannot add, change or remove a script's setup - save the script itself to change it";

/// The end of Rule 3's refusal - the save route looks for it to decide
/// whether the test case's own history could excuse the change.
pub const NEVER_WEAKENED: &str = "an assertion is never removed or weakened by a repair";

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

/// The test case's steps that changed between two readings of it: the
/// position (1-based, as a script numbers its steps) of every step whose
/// action or expected result differs, and of every step `before` had that
/// `now` no longer has. A step only `now` has changes nothing a script
/// already checked, so it is not counted.
pub fn steps_changed_by_case(before: &[crate::steps_xml::Step], now: &[crate::steps_xml::Step]) -> BTreeSet<i32> {
    before
        .iter()
        .enumerate()
        .filter(|(i, b)| now.get(*i) != Some(*b))
        .map(|(i, _)| i as i32 + 1)
        .collect()
}

/// Refuses an undeclared or weakening change. `old` is the script on disk.
pub fn check_edits(old: &CaseScript, new: &CaseScript, declared: Option<&Edit>) -> Result<(), String> {
    check_edits_following_case(old, new, declared, &BTreeSet::new())
}

/// `check_edits`, for a repair that follows its test case: `changed_by_case`
/// is the steps the case itself changed or dropped since the script was
/// saved (`steps_changed_by_case`). Those steps may lose checks - the case
/// no longer asks for them - and every other rule still holds: each one is
/// still declared, and the save's expected-result floor still checks the
/// script against the case as it is now.
pub fn check_edits_following_case(
    old: &CaseScript,
    new: &CaseScript,
    declared: Option<&Edit>,
    changed_by_case: &BTreeSet<i32>,
) -> Result<(), String> {
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

    // Rule 11: a script that must not save keeps that promise through every
    // repair - leaving `no_save` out of a repair IS turning it off. Only a
    // person saving from the editor can. Turning it on is always allowed.
    if old.no_save && !new.no_save {
        return Err(NO_SAVE_KEPT.to_string());
    }

    // Rule 12: the records a case relies on are part of what the case
    // means, not something a repair tunes until the case runs. A repair
    // keeps every precondition the saved script has, exactly (flow, stage,
    // value and why), and may add new ones: adding only blocks a case
    // earlier, dropping or changing one weakens it. Only the editor or an
    // import can drop or change one. An added one is validated like any
    // save (`nav::check_project_rules`).
    if old.preconditions.iter().any(|p| !new.preconditions.contains(p)) {
        return Err(PRECONDITIONS_KEPT.to_string());
    }

    // Rule 13: a setup writes data in the application on every run of its
    // case, once a person has approved it. A repair may not add one,
    // change it or remove it: only the editor or an import can.
    if old.setup != new.setup {
        return Err(SETUP_KEPT.to_string());
    }

    // Rule 10: where the case starts is part of the script. A repair that
    // changes the area - and leaving out an area the saved script has IS a
    // change, never a silent erase, as leaving out `account` is - must say
    // `"area": true`; one that says so and changes nothing is refused, as a
    // step declared but not changed is. A blank area is no area, and case
    // does not tell two area names apart.
    let (old_area, new_area) = (old.area_name(), new.area_name());
    let area_changed = old_area.map(module_key) != new_area.map(module_key);
    let area_declared = declared.is_some_and(|e| e.area);
    if area_changed && !area_declared {
        let say = |a: Option<&str>| a.map_or("the case's Module".to_string(), |a| format!("\"{a}\""));
        return Err(format!(
            "case {}: the area changed from {} to {} but was not declared - add \"area\": true to the case's \"edits\" entry, or leave the area as it was",
            old.case_id,
            say(old_area),
            say(new_area)
        ));
    }
    if area_declared && !area_changed {
        return Err("the area was declared but not changed".to_string());
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
    // not - unless the test case itself changed or dropped that step since
    // the script was saved: following the case is not weakening it.
    for n in changed.iter().filter(|n| !changed_by_case.contains(n)) {
        let old_checks = old_map.get(n).map(|s| checks(s)).unwrap_or(0);
        let new_checks = new_map.get(n).map(|s| checks(s)).unwrap_or(0);
        if new_checks < old_checks {
            return Err(format!(
                "step {n} had {old_checks} checks and now has {new_checks} - {NEVER_WEAKENED}"
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
