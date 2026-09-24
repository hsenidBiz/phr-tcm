//! A run's failed cases, read back as text an assistant can act on.
//!
//! This module never touches the browser, the filesystem beyond
//! `store::list_runs`, or Azure DevOps. It only turns what a run already
//! recorded into a shape an assistant can reason about: which step, which
//! action, what the page said - and, just as important, when it must
//! refuse to touch the script at all. `stop_reason` exists because a
//! failure is not always a script defect: a browser that stopped
//! answering, a sign-in that never got the case started, or a person who
//! already decided the case is Blocked are all reasons to leave the
//! script alone.

use super::edits::MAX_REPAIRS;
use super::nav;
use super::replay::{MODULE_STEP, SIGN_IN_STEP};
use super::{store, CaseRecord, CaseScript, LocalRun, StepRecord};
use crate::browser::actions::Action;
use std::path::Path;

/// Why an assistant must not try to repair this case. Checked in this
/// order because a case can match more than one at once (a Blocked case
/// whose sign-in also failed, say) and the sign-in / browser reasons are
/// the more specific, more actionable ones.
pub fn stop_reason(case: &CaseRecord) -> Option<String> {
    if let Some(step0) = case.steps.iter().find(|s| s.step_number == SIGN_IN_STEP) {
        if step0.outcomes.last().is_some_and(|o| !o.ok) {
            return Some(
                "the sign-in failed - fix the account or the recipe in the app, not the script"
                    .to_string(),
            );
        }
    }
    // A prefix, never a search: every reason the runner writes for a
    // browser that gave up begins "the browser" ("stopped answering...",
    // "did not open..."), while a Failed case's reason begins `step N:`
    // and quotes the page, which can say "did not answer" too.
    if case.reason.starts_with("the browser") {
        return Some("the browser stopped answering - rerun before changing anything".to_string());
    }
    if nav::is_setup_problem(&case.reason) {
        return Some(
            "the run could not take this case to its module screen - fix the module path or the case's Module in the app, not the script"
                .to_string(),
        );
    }
    if case.verdict == "Blocked" {
        return Some(
            "the person marked this case Blocked - a missing precondition is not a script defect"
                .to_string(),
        );
    }
    None
}

/// A case whose machine proposal or human verdict says it did not pass -
/// the set `describe_failures` reports on. A case nobody has judged either
/// way (both still "") is not a failure to act on, just an unfinished one.
fn is_failed(case: &CaseRecord) -> bool {
    matches!(case.proposed.as_str(), "Failed" | "Blocked") || matches!(case.verdict.as_str(), "Failed" | "Blocked")
}

/// A JSON value rendered on one line, with a space inside every pair of
/// braces so `{ "kind": "click" }` reads the way a person would write it
/// by hand - `serde_json::to_string`'s fully compact form is correct but
/// cramped, and `to_string_pretty`'s multi-line form does not belong in a
/// line-per-step report.
fn compact_json(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", serde_json::to_string(k).unwrap_or_default(), compact_json(v)))
                .collect();
            format!("{{ {} }}", inner.join(", "))
        }
        serde_json::Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(compact_json).collect();
            format!("[{}]", inner.join(", "))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// An action's JSON, for an assistant to read - except a `fill`'s value,
/// which never leaves this machine's memory a second time: the assistant
/// already knows what it wrote, and a run file is not the place to echo a
/// password or anything else a `fill` might have carried.
fn action_json(action: &Action) -> String {
    let mut v = serde_json::to_value(action).unwrap_or(serde_json::Value::Null);
    if let serde_json::Value::Object(map) = &mut v {
        if map.get("kind").and_then(|k| k.as_str()) == Some("fill") {
            map.insert("value".to_string(), serde_json::Value::String("...".to_string()));
        }
    }
    compact_json(&v)
}

/// What to print in place of an action's JSON, when the script on disk
/// cannot supply one: no script at all for this case, or - a script IS
/// there, but a person edited it after this run happened, and it no
/// longer has an action at this index. The two are told apart because
/// they call for different next steps (find the script vs. rerun first).
fn missing_action_text(script: Option<&CaseScript>, index: usize) -> String {
    match script {
        None => "script: not on this machine".to_string(),
        Some(_) => format!("script: no action {} on this machine (the script changed since the run)", index + 1),
    }
}

/// The script action at this step and action index, or `None` when there
/// is no such action - either no script matches this case, or one does
/// but no longer has an action here.
fn scripted_action<'a>(script: Option<&'a CaseScript>, step_number: i32, index: usize) -> Option<&'a Action> {
    script
        .and_then(|s| s.steps.iter().find(|st| st.step_number == step_number))
        .and_then(|st| st.actions.get(index))
}

/// One step's lines: `sign-in: <detail>` for step 0 when it failed, one
/// line for a step that never ran, or - for an ordinary step - one block
/// per failed action and one `  action K: not run (...)` line per action
/// skipped because an earlier action in the SAME step already failed.
fn describe_step(step: &StepRecord, script: Option<&CaseScript>, out: &mut Vec<String>) {
    if step.step_number == SIGN_IN_STEP {
        if let Some(o) = step.outcomes.last() {
            if !o.ok {
                out.push(format!("sign-in: {}", o.detail));
            }
        }
        return;
    }
    if step.step_number == MODULE_STEP {
        if let Some(o) = step.outcomes.last() {
            if !o.ok {
                out.push(format!("module: {}", o.detail));
            }
        }
        return;
    }

    let all_not_run = !step.outcomes.is_empty() && step.outcomes.iter().all(|o| o.detail.starts_with("not run:"));
    if all_not_run {
        let detail = &step.outcomes[0].detail;
        let why = detail.strip_prefix("not run: ").unwrap_or(detail);
        out.push(format!("step {}: not run ({why})", step.step_number));
        return;
    }

    for (i, outcome) in step.outcomes.iter().enumerate() {
        if outcome.ok {
            continue;
        }
        if let Some(why) = outcome.detail.strip_prefix("not run: ") {
            // A mixed step: this one action was skipped because another
            // action in the same step already failed. Shown, not dropped -
            // an assistant deciding what to fix needs to know the step had
            // MORE than the one failure it can see JSON for.
            out.push(format!("  action {}: not run ({why})", i + 1));
            continue;
        }
        let action_text = match scripted_action(script, step.step_number, i) {
            Some(action) => action_json(action),
            None => missing_action_text(script, i),
        };
        out.push(format!("step {}, action {}: {action_text}", step.step_number, i + 1));
        out.push(format!("  page said: {}", outcome.detail));
        if let Some(shot) = &outcome.screenshot {
            out.push(format!("  picture: {shot}"));
        }
    }
}

/// One failed case's whole block: header, account and repair count when
/// known, the stop reason when there is one, every failed or skipped
/// step, and the person's own note last.
fn describe_case(run_id: &str, case: &CaseRecord, script: Option<&CaseScript>) -> String {
    let mut lines: Vec<String> = Vec::new();

    // A supervised run never fills in `proposed` - only a person's own
    // verdict does - so an empty `proposed` is ordinary, not a gap to fill
    // with a blank; it is left out of the header rather than printed as
    // "proposed " with nothing after it.
    let mut header_parts = vec![format!("run {run_id}")];
    if !case.proposed.is_empty() {
        header_parts.push(format!("proposed {}", case.proposed));
    }
    if !case.verdict.is_empty() {
        header_parts.push(format!("verdict {}", case.verdict));
    }
    lines.push(format!("## Case {} \"{}\" ({})", case.case_id, case.title, header_parts.join(", ")));

    if let Some(account) = &case.account {
        lines.push(format!("account: {account}"));
    }
    if let Some(s) = script {
        lines.push(format!("repairs so far: {} of {MAX_REPAIRS}", s.repairs));
    }
    if let Some(reason) = stop_reason(case) {
        lines.push(format!("STOP: {reason}"));
    }

    for step in &case.steps {
        describe_step(step, script, &mut lines);
    }

    if !case.note.is_empty() {
        lines.push(format!("note from the person: {}", case.note));
    }

    lines.join("\n")
}

/// One run's failed cases, for an assistant: which step, which action (its
/// JSON), what the page said, and whether to stop. A case that passed, or
/// that nobody has judged either way yet, is left out entirely - this is a
/// worklist of what needs fixing, not a transcript of the whole run.
pub fn describe_failures(run: &LocalRun, scripts: &[CaseScript]) -> String {
    let blocks: Vec<String> = run
        .cases
        .iter()
        .filter(|c| is_failed(c))
        .map(|c| describe_case(&run.id, c, scripts.iter().find(|s| s.case_id == c.case_id)))
        .collect();

    if blocks.is_empty() {
        return format!("no failed case in run {}", run.id);
    }
    blocks.join("\n\n")
}

/// The newest run on this machine that holds this case, or - with no case
/// given - the newest run at all. `store::list_runs` already sorts newest
/// first, so the first match a linear scan finds is the one wanted.
pub fn latest_run(root: &Path, case_id: Option<i32>) -> Option<LocalRun> {
    let runs = store::list_runs(root);
    match case_id {
        Some(id) => runs.into_iter().find(|r| r.cases.iter().any(|c| c.case_id == id)),
        None => runs.into_iter().next(),
    }
}
