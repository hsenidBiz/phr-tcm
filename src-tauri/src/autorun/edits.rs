//! The declared-edit gate: what an assistant must say before it may change
//! an existing Auto Run script.
//!
//! Pure functions over `CaseScript` - no browser, no filesystem. Nothing
//! here judges whether an edit is a GOOD idea, only whether it was
//! honestly declared: a declared step may change in any way, a check
//! dropped included, and an undeclared one stays exactly as it was.

use super::nav::module_key;
use super::{CaseScript, Precondition, PreconditionOp, PreconditionRepair, StepScript};
use std::collections::{BTreeMap, BTreeSet};

/// What an assistant declares before it may change an existing script.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub case_id: i32,
    /// Every step whose actions change, are added or are removed. Absent
    /// reads as none: an edit of a precondition alone names no step.
    #[serde(default)]
    pub steps: Vec<i32>,
    /// One sentence: what was wrong and what this changes. An edit of a
    /// precondition alone may leave it out: its `reason` says why.
    #[serde(default)]
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
    /// The one precondition this repair adds, removes or changes, with its
    /// reason. At most one per case per save.
    #[serde(default)]
    pub precondition: Option<PreconditionEdit>,
}

/// One precondition a repair adds, removes or changes. A saved one is
/// named by its `flow` and `stage` (and its `value` when two share them);
/// `to` is the precondition as it should be, for an add or a change.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreconditionEdit {
    pub op: PreconditionOp,
    #[serde(default)]
    pub flow: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub to: Option<Precondition>,
    /// One line: for a removal, which steps now create that state, or why
    /// the case no longer needs it.
    #[serde(default)]
    pub reason: String,
}

/// Said when a repair would turn `no_save` off.
pub const NO_SAVE_KEPT: &str =
    "this script is marked Must not save, and a repair cannot turn that off - send \"no_save\": true, or ask a person to change it in the app";

/// Said when a repair drops or changes a precondition it did not declare.
pub const PRECONDITIONS_KEPT: &str = "a precondition was removed or changed but not declared - add one \"precondition\" edit ({ \"op\": \"remove\" or \"change\", \"flow\", \"stage\", \"to\" for a change, \"reason\" }) to the case's \"edits\" entry, or send every saved precondition back as it is";

/// Said when the preconditions sent differ from the saved ones by more
/// than the one precondition edit declared.
pub const PRECONDITIONS_UNDECLARED: &str = "the preconditions changed in more ways than the case's \"precondition\" edit declares - a repair changes one precondition per save; send the others as saved";

/// Said when a precondition edit has no reason.
pub const PRECONDITION_NEEDS_REASON: &str = "a precondition edit needs a \"reason\": for a removal, say which steps now create that state, or why the case no longer needs it";

/// The longest reason a precondition edit may give, in characters.
pub const PRECONDITION_REASON_MAX: usize = 200;

/// Said when a precondition edit's reason is more than one short line.
pub const PRECONDITION_REASON_LONG: &str = "a precondition edit's \"reason\" is one line of at most 200 characters";

/// Said when one case's entry carries more than one precondition edit.
pub const ONE_PRECONDITION_EDIT: &str = "a repair changes at most one precondition per save - send one \"precondition\" edit now and the next in a later save";

/// The saved preconditions with `edit` applied, and the record the script
/// keeps of it. Refused: no reason, or one longer than a line; a saved
/// precondition that is not there, or not the only one named; an add or a
/// change with no `to`; an add of one the script has; a change to the
/// same precondition. Validating the precondition itself against the
/// project's flows is the save's (`preconditions::check_saved`), as for
/// any save.
pub fn apply_precondition_edit(
    saved: &[Precondition],
    edit: &PreconditionEdit,
) -> Result<(Vec<Precondition>, PreconditionRepair), String> {
    let reason = edit.reason.trim();
    if reason.is_empty() {
        return Err(PRECONDITION_NEEDS_REASON.to_string());
    }
    if reason.contains(['\n', '\r']) || reason.chars().count() > PRECONDITION_REASON_MAX {
        return Err(PRECONDITION_REASON_LONG.to_string());
    }
    let names_one = edit.flow.is_some() || edit.stage.is_some() || edit.value.is_some();
    let record = |before: Option<Precondition>, after: Option<Precondition>| PreconditionRepair {
        op: edit.op,
        before,
        after,
        reason: reason.to_string(),
    };
    if edit.op == PreconditionOp::Add {
        if names_one {
            return Err(
                "an \"add\" precondition edit takes the new precondition in \"to\" and names no saved one".to_string(),
            );
        }
        let Some(to) = edit.to.clone() else {
            return Err(
                "an \"add\" precondition edit needs \"to\": the precondition to add, { flow, stage, value, why }"
                    .to_string(),
            );
        };
        if saved.iter().any(|p| p.flow == to.flow && p.stage == to.stage && p.value == to.value) {
            return Err("the script already has that precondition".to_string());
        }
        let mut after = saved.to_vec();
        after.push(to.clone());
        return Ok((after, record(None, Some(to))));
    }
    let op = if edit.op == PreconditionOp::Remove { "remove" } else { "change" };
    let (Some(flow), Some(stage)) = (edit.flow.as_deref(), edit.stage.as_deref()) else {
        return Err(format!(
            "a \"{op}\" precondition edit names the saved precondition by its \"flow\" and \"stage\""
        ));
    };
    let named: Vec<usize> = saved
        .iter()
        .enumerate()
        .filter(|(_, p)| p.flow == flow && p.stage == stage && edit.value.as_ref().is_none_or(|v| &p.value == v))
        .map(|(i, _)| i)
        .collect();
    let at = match named.as_slice() {
        [one] => *one,
        [] => {
            let value = edit.value.as_ref().map(|v| format!(" for {}", crate::api_templates::gate::shown(v)));
            return Err(format!("the script has no precondition {flow} / {stage}{}", value.unwrap_or_default()));
        }
        _ => {
            return Err(format!(
                "the script has more than one precondition {flow} / {stage} - name its \"value\" too"
            ))
        }
    };
    let mut after = saved.to_vec();
    let before = after[at].clone();
    if edit.op == PreconditionOp::Remove {
        if edit.to.is_some() {
            return Err("a \"remove\" precondition edit takes no \"to\"".to_string());
        }
        after.remove(at);
        return Ok((after, record(Some(before), None)));
    }
    let Some(to) = edit.to.clone() else {
        return Err("a \"change\" precondition edit needs \"to\": the precondition as it should be".to_string());
    };
    if to == before {
        return Err("the precondition edit changes nothing - \"to\" is the precondition as it is".to_string());
    }
    after[at] = to.clone();
    Ok((after, record(Some(before), Some(to))))
}

/// Said when a repair would add, change or remove a script's setup.
pub const SETUP_KEPT: &str =
    "a repair cannot add, change or remove a script's setup - save the script itself to change it";

/// A step's actions as the text the gate compares. Order matters (a
/// reordered script is a different script); JSON formatting does not. The
/// `unchecked` reason is folded in too, so changing just the reason still
/// counts as a change to the step, and so is whether it is the clean-up.
pub fn step_signature(step: &StepScript) -> String {
    format!(
        "{}|{}{}",
        serde_json::to_string(&step.actions).unwrap_or_default(),
        step.unchecked.as_deref().unwrap_or(""),
        if step.cleanup { "|cleanup" } else { "" }
    )
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
/// if any. A duplicate step number is what let a changed copy hide behind
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

/// Refuses an undeclared change. `old` is the script on disk.
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
    // about it is even looked at. An edit of a precondition alone gives
    // its reason in the precondition edit itself (rule 12).
    if let Some(e) = declared {
        let precondition_only = e.precondition.is_some() && e.steps.is_empty() && !e.area;
        if e.why.trim().is_empty() && !precondition_only {
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
    // means, not something a repair tunes until the case runs. Without a
    // precondition edit a repair keeps every precondition the saved script
    // has, exactly (flow, stage, value and why), and may add new ones:
    // adding only blocks a case earlier, dropping or changing one weakens
    // it. With one, declared with its reason, the preconditions are the
    // saved ones with exactly that one add, removal or change - the reason
    // is what tells a person reading the editor why. Either way an added
    // or changed one is validated like any save (`nav::check_project_rules`).
    match declared.and_then(|e| e.precondition.as_ref()) {
        Some(edit) => {
            let (expected, _) = apply_precondition_edit(&old.preconditions, edit)?;
            if new.preconditions != expected {
                return Err(PRECONDITIONS_UNDECLARED.to_string());
            }
        }
        None => {
            if old.preconditions.iter().any(|p| !new.preconditions.contains(p)) {
                return Err(PRECONDITIONS_KEPT.to_string());
            }
        }
    }

    // Rule 13: a setup writes data in the application on every run of its
    // case. A repair may not add one, change it or remove it: only the
    // editor or an import can.
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

    Ok(())
}
