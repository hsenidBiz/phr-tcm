//! The steps Auto Run performs itself: launching the app, signing in,
//! reaching the area - what runs before step 1, so a step whose whole
//! expected result is that has nothing for the script to do or check.
//! Pure. The TypeScript mirror is `src/screens/AutoRun/byAutoRun.ts`, held
//! to the same fixture (`tests/fixtures/by_auto_run.json`).

use std::collections::BTreeSet;

use super::{CaseScript, Unchecked};

/// The step numbers Auto Run performs itself.
///
/// A step counts when it has `by_auto_run: true` and sits in the leading
/// run: every case step before it has the mark too. When no step of the
/// script sets the field (a script saved before it existed), the leading
/// run of case steps from step 1 with no actions and a whole-step
/// `unchecked` sentence counts instead. The clean-up never counts.
pub fn done_by_auto_run(script: &CaseScript) -> BTreeSet<i32> {
    let mut out = BTreeSet::new();
    if script.steps.iter().any(|s| s.by_auto_run) {
        for step in script.case_steps() {
            if !step.by_auto_run {
                break;
            }
            out.insert(step.step_number);
        }
        return out;
    }
    for (i, step) in script.case_steps().enumerate() {
        let whole_step = matches!(&step.unchecked, Some(Unchecked::Step(s)) if !s.trim().is_empty());
        let opening = i != 0 || step.step_number == 1;
        if !step.actions.is_empty() || !whole_step || !opening {
            break;
        }
        out.insert(step.step_number);
    }
    out
}

/// What the save refuses about the marks, one sentence per step, in step
/// order: a marked step with actions, and a marked step that is not one of
/// the opening steps (one after a step without the mark, or the clean-up).
pub fn mark_problems(script: &CaseScript) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    let mut leading = true;
    for step in &script.steps {
        let n = step.step_number;
        if step.cleanup {
            if step.by_auto_run {
                out.push((n, format!("step {n} is marked as done by Auto Run but is not one of the opening steps")));
            }
            continue;
        }
        if !step.by_auto_run {
            leading = false;
            continue;
        }
        if !leading {
            out.push((n, format!("step {n} is marked as done by Auto Run but is not one of the opening steps")));
        } else if !step.actions.is_empty() {
            out.push((n, format!("step {n} is marked as done by Auto Run but has actions - drop one or the other")));
        }
    }
    out
}
