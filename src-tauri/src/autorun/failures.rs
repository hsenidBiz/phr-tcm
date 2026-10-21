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

use super::components::{ran_actions, ComponentFile, Ran};
use super::nav;
use super::replay::{MODULE_STEP, SIGN_IN_STEP};
use super::{store, CaseRecord, CaseScript, LocalRun, StepRecord};
use crate::browser::actions::Action;
use std::path::Path;

/// The `STOP:` line for a case the run could not take to its area
/// ("Could not reach module ...").
pub const UNREACHED_STOP: &str = "the run could not reach this case's area - repair the area's path with save_autorun_area and its repair argument (the guide's areas section), not the script";

/// The `STOP:` line for a case that needs a person's ruling on a divergence.
pub const NEEDS_RULING_STOP: &str = "the case waits for a person's ruling on a divergence - leave the script as the case says";

/// How a failure is classified before anything is changed. The guide repeats
/// it word for word (a test holds the two together).
pub const CLASSIFY_FIRST: &str = "Classify each failure before you change anything: the locator, timing, the environment, data or a missing precondition, or the application not doing what the case expects.";

/// The bounds of a repair, ending the failures answer when a case failed or
/// was Blocked, and repeated word for word (less its heading) in the guide.
/// Guidance for the assistant: the app refuses none of the cycle limits, only
/// the check floor and the case's values at the save.
pub const REPAIR_BOUNDS: &str = "## Bounds of a repair

Classify each failure before you change anything: the locator, timing, the environment, data or a missing precondition, or the application not doing what the case expects. A check whose value differs from what the page shows is data, a missing precondition or the application, never a script fix: call report_case_divergence and keep the case's value. A missing precondition is reported, never repaired: a healer pointed at a missing precondition will keep rewriting assertions and locators until an empty or disabled page satisfies them, which produces a script that passes and proves nothing.

- At most 3 fix-run cycles per case: a fix, then a replay or run of it.
- Stop at cycle 2 when the failure is identical to cycle 1's: the same step, the same action, the same words.
- Stop at once on a login page, a missing precondition or a product defect: a real bug must surface as a failure.
- A pass after a change that changed nothing is run 2 more times: 2 green of 3 is a flake, not a fix. Report it as a flake.
- A repair keeps the script's number of checks unless its edit names each check given up and why in gives_up, and never changes a check's value away from the case; the save refuses both otherwise.
";

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
    // Blocked because another holder had the account: the reason is the
    // lease's own sentence, exactly.
    if super::lease::is_in_use(&case.reason) {
        return Some("the account was in use elsewhere - rerun the case once it is free, and leave the script alone".to_string());
    }
    // A trip that could not reach the area: its menu path may have moved,
    // and the assistant repairs the area (`area_repair`), never the script.
    if case.reason.starts_with(nav::UNREACHED_PREFIX) {
        return Some(UNREACHED_STOP.to_string());
    }
    if nav::is_setup_problem(&case.reason) {
        return Some(
            "the run could not take this case to its module screen - fix the module path or the case's Module in the app, not the script"
                .to_string(),
        );
    }
    // A divergence a person has not ruled on: the script stays as the case
    // says. A `waits for a repair:` reason is not a stop - it is the
    // signal to repair.
    if case.reason.starts_with(super::divergences::NEEDS_RULING_PREFIX) {
        return Some(NEEDS_RULING_STOP.to_string());
    }
    if case.verdict == "Blocked" {
        return Some(
            "the person marked this case Blocked - a missing precondition is not a script defect"
                .to_string(),
        );
    }
    None
}

/// A failure, as the bounds compare two: the first step with a failed action, that action's index in the
/// step, and the first line of its words, lowercased, every digit run read as "#", whitespace collapsed,
/// at most 160 characters.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FailureSignature {
    pub step: i32,
    pub action: usize,
    pub words: String,
}

/// None when no action of the case failed.
pub fn signature(case: &CaseRecord) -> Option<FailureSignature> {
    for step in &case.steps {
        let failed = step.outcomes.iter().enumerate().find(|(_, o)| !o.ok && !o.detail.starts_with("not run:"));
        if let Some((action, outcome)) = failed {
            let first = outcome.detail.lines().next().unwrap_or("").to_lowercase();
            let mut masked = String::new();
            let mut in_digits = false;
            for c in first.chars() {
                if c.is_ascii_digit() {
                    if !in_digits {
                        masked.push('#');
                    }
                    in_digits = true;
                } else {
                    in_digits = false;
                    masked.push(c);
                }
            }
            let words: String = masked.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect();
            return Some(FailureSignature { step: step.step_number, action, words });
        }
    }
    None
}

/// The `STOP:` line for a failure that came back unchanged after the script was repaired.
pub const SAME_FAILURE_STOP: &str = "the same failure as before the last repair: the same step, the same action, the same words - the repair did not reach the cause, so stop on this case and report each failure, what the repair changed and your diagnosis to the person";

fn iso_ms(ms: u64) -> String {
    crate::applog::iso_of(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms))
}

/// SAME_FAILURE_STOP when `case` (read at `read_at_ms`) fails as the newest earlier failure of the same case
/// did and the script was repaired in between: `script.repairs > 0` and `script.saved_at` later than that
/// earlier failure and no later than `read_at_ms` (both as `applog::iso_of`, whole seconds). The earlier
/// failure is the newest, before `read_at_ms`, of: each run of `store::list_runs(root)` holding the case
/// failed (`is_failed`) at `started_at`; the case's kept replay (`replay_record::load`) at its `at`; and that
/// replay's `previous`.
pub fn same_failure_stop(root: &Path, case: &CaseRecord, script: Option<&CaseScript>, read_at_ms: u64) -> Option<&'static str> {
    let now = signature(case)?;
    let script = script.filter(|s| s.repairs > 0)?;
    let saved = script.saved_at.as_deref()?;
    let mut earlier: Vec<(u64, FailureSignature)> = Vec::new();
    for run in store::list_runs(root) {
        let Ok(at) = run.started_at.parse::<u64>() else { continue };
        if let Some(c) = run.cases.iter().find(|c| c.case_id == case.case_id && is_failed(c)) {
            if let Some(sig) = signature(c) {
                earlier.push((at, sig));
            }
        }
    }
    if let Some(rec) = super::replay_record::load(root, case.case_id) {
        if let Some(sig) = rec.as_run_case().and_then(|c| signature(&c)) {
            earlier.push((rec.at, sig));
        }
        if let Some(prev) = rec.previous {
            earlier.push((prev.at, prev.signature));
        }
    }
    let (at, sig) = earlier.into_iter().filter(|(at, _)| *at < read_at_ms).max_by_key(|(at, _)| *at)?;
    (sig == now && saved > iso_ms(at).as_str() && saved <= iso_ms(read_at_ms).as_str()).then_some(SAME_FAILURE_STOP)
}

/// A case whose machine proposal or human verdict says it did not pass -
/// the set `describe_failures` reports on. A case nobody has judged either
/// way (both still "") is not a failure to act on, just an unfinished one.
pub fn is_failed(case: &CaseRecord) -> bool {
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
/// password or anything else a `fill` might have carried. A
/// `use_component`'s text inputs are typed values too, and are masked the
/// same way; its target inputs are locators, and stay. A `when_visible`'s
/// guarded actions are masked as well.
fn action_json(action: &Action) -> String {
    let mut v = serde_json::to_value(action).unwrap_or(serde_json::Value::Null);
    mask_typed(&mut v);
    compact_json(&v)
}

fn mask_typed(v: &mut serde_json::Value) {
    let masked = || serde_json::Value::String("...".to_string());
    let serde_json::Value::Object(map) = v else { return };
    match map.get("kind").and_then(|k| k.as_str()) {
        Some("fill") => {
            map.insert("value".to_string(), masked());
        }
        Some("use_component") => {
            if let Some(serde_json::Value::Object(inputs)) = map.get_mut("inputs") {
                for value in inputs.values_mut() {
                    // A locator is an object or a list of them; anything
                    // else is text.
                    if !(value.is_object() || value.is_array()) {
                        *value = masked();
                    }
                }
            }
        }
        _ => {}
    }
    if let Some(serde_json::Value::Array(then)) = map.get_mut("then") {
        then.iter_mut().for_each(mask_typed);
    }
}

/// What to print in place of an action's JSON for an action a component
/// ran, when the component on disk no longer expands to what ran.
const CHANGED_COMPONENT: &str = "component: this action is not in the component on this machine (the component changed since the run)";

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

/// What each of a step's outcomes ran (`components::ran_actions`): the
/// script's actions with its components expanded. Empty when no script
/// matches this case or it no longer has this step.
fn ran_in_step(script: Option<&CaseScript>, step: &StepRecord, components: &ComponentFile) -> Vec<Ran> {
    match script.and_then(|s| s.steps.iter().find(|st| st.step_number == step.step_number)) {
        Some(st) => ran_actions(&st.actions, &step.outcomes, components, &step.components),
        None => Vec::new(),
    }
}

/// One step's lines: `sign-in: <detail>` for step 0 when it failed, one
/// line for a step that never ran, or - for an ordinary step - one block
/// per failed action and one `  action K: not run (...)` line per action
/// skipped because an earlier action in the SAME step already failed. A
/// step that saved files then names them (`push_downloads`). An action a
/// component ran names the component.
fn describe_step(step: &StepRecord, script: Option<&CaseScript>, components: &ComponentFile, root: Option<&Path>, out: &mut Vec<String>) {
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
        push_downloads(step, out);
        return;
    }

    describe_failed_actions(step, &format!("step {}", step.step_number), script, components, root, out);
    push_downloads(step, out);
}

/// How a failure report names the clean-up step.
const CLEANUP_NAME: &str = "clean-up (it never fails the case)";

/// The first line of a case listed only because its clean-up failed.
pub const CLEANUP_ONLY: &str = "the case passed; only its clean-up failed";

/// One block per failed action of `step` (named `name`): its JSON, what the
/// page said and its picture, and one `  action K: not run (...)` line per
/// action skipped because an earlier one in the same step failed.
fn describe_failed_actions(
    step: &StepRecord,
    name: &str,
    script: Option<&CaseScript>,
    components: &ComponentFile,
    root: Option<&Path>,
    out: &mut Vec<String>,
) {
    let ran = ran_in_step(script, step, components);
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
        let here = ran.get(i);
        let action_text = match here {
            Some(Ran { action: Some(action), .. }) => action_json(action),
            Some(Ran { action: None, component: Some(_) }) => CHANGED_COMPONENT.to_string(),
            _ => missing_action_text(script, i),
        };
        match here.and_then(|r| r.component.as_deref()) {
            Some(c) => out.push(format!("{name}, {c}, action {}: {action_text}", i + 1)),
            None => out.push(format!("{name}, action {}: {action_text}", i + 1)),
        }
        out.push(format!("  page said: {}", outcome.detail));
        if let Some(shot) = &outcome.screenshot {
            // The full path when the store is known: a bare name sends an
            // assistant searching the disk for the file.
            match root {
                Some(root) => out.push(format!("  picture: {}", store::shot_path(root, shot).display())),
                None => out.push(format!("  picture: {shot}")),
            }
        }
    }
}

/// `step N downloads: a.xlsx, b.csv` for a step that saved files: the names
/// the run keeps them under, never their folder or their contents.
fn push_downloads(step: &StepRecord, out: &mut Vec<String>) {
    if !step.downloads.is_empty() {
        out.push(format!("step {} downloads: {}", step.step_number, step.downloads.join(", ")));
    }
}

/// Whether a run id is a replay's read back as a run (`replay_record::as_run`):
/// a saved run's id is never words.
fn run_mode_is_replay(run_id: &str) -> bool {
    run_id.starts_with("a replay to step ")
}

/// One failed case's whole block: header, account and area when known,
/// the stop reason when there is one, every failed or skipped
/// step, and the person's own note last.
fn describe_case(run_id: &str, case: &CaseRecord, script: Option<&CaseScript>, components: &ComponentFile, root: Option<&Path>, started: Option<u64>) -> String {
    let mut lines: Vec<String> = Vec::new();

    // A supervised run never fills in `proposed` - only a person's own
    // verdict does - so an empty `proposed` is ordinary, not a gap to fill
    // with a blank; it is left out of the header rather than printed as
    // "proposed " with nothing after it.
    // A replay read back as a run (`replay_record::as_run`) says so: its id
    // is "a replay to step N".
    let origin = if run_mode_is_replay(run_id) { format!("from {run_id}") } else { format!("run {run_id}") };
    let mut header_parts = vec![origin];
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
    if let Some(area) = script.and_then(CaseScript::area_name) {
        lines.push(format!("area: {area}"));
    }
    if let Some(m) = script.and_then(|s| s.suspected_defect.as_ref()) {
        lines.push(format!("suspected defect at step {}: {}", m.step_number, m.note));
    }
    let stop = stop_reason(case).or_else(|| match (root, started) {
        (Some(root), Some(at)) => same_failure_stop(root, case, script, at).map(str::to_string),
        _ => None,
    });
    if let Some(reason) = stop {
        lines.push(format!("STOP: {reason}"));
    }

    // A case listed only for its clean-up says so before anything else.
    if !is_failed(case) && case.cleanup_line().is_some() {
        lines.push(CLEANUP_ONLY.to_string());
    }
    for step in &case.steps {
        describe_step(step, script, components, root, &mut lines);
    }
    // The clean-up is no step of the case: what failed in it is said as a
    // step's failure is, under its own name.
    if let Some(tidy) = case.cleanup.as_ref().filter(|_| case.cleanup_line().is_some()) {
        describe_failed_actions(tidy, CLEANUP_NAME, script, components, root, &mut lines);
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
    describe_failures_with(run, scripts, &ComponentFile::default())
}

/// [`describe_failures`], with the project's components: an action a
/// component ran is shown as it expands from them. Without them it is
/// named by its component only.
pub fn describe_failures_with(run: &LocalRun, scripts: &[CaseScript], components: &ComponentFile) -> String {
    describe_failures_in(None, run, scripts, components)
}

/// [`describe_failures_with`], naming each picture by its full path under
/// the store `root`'s shots folder when one is given, never just its name.
pub fn describe_failures_in(root: Option<&Path>, run: &LocalRun, scripts: &[CaseScript], components: &ComponentFile) -> String {
    let blocks: Vec<String> = run
        .cases
        .iter()
        // A case that passed is listed too when its clean-up failed: a
        // broken clean-up leaves data behind for the next run.
        .filter(|c| is_failed(c) || c.cleanup_line().is_some())
        .map(|c| describe_case(&run.id, c, scripts.iter().find(|s| s.case_id == c.case_id), components, root, run.started_at.parse::<u64>().ok()))
        .collect();

    if blocks.is_empty() {
        return format!("no failed case in run {}", run.id);
    }
    let mut out = blocks.join("\n\n");
    // The same failure in more than one case, last: read after the cases
    // it summarises, and absent altogether when nothing repeats.
    let patterns = super::patterns::patterns_section(&super::patterns::find_patterns_with(run, scripts, components));
    if !patterns.is_empty() {
        out.push_str("\n\n");
        out.push_str(patterns.trim_end());
    }
    out
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

/// How many of the newest other runs `recent_failing` looks at, and how
/// far back: the smaller of the two windows.
pub const RECENT_RUNS: usize = 5;
pub const RECENT_MS: u64 = 24 * 60 * 60 * 1000;

/// A case's word on itself: the person's verdict, else the machine's
/// proposal.
fn outcome_of(case: &CaseRecord) -> &str {
    if case.verdict.is_empty() {
        &case.proposed
    } else {
        &case.verdict
    }
}

/// How many of `run`'s cases failed, and how many were Blocked.
pub fn failed_and_blocked(run: &LocalRun) -> (usize, usize) {
    let failed = run.cases.iter().filter(|c| outcome_of(c) == "Failed").count();
    let blocked = run.cases.iter().filter(|c| outcome_of(c) == "Blocked").count();
    (failed, blocked)
}

/// When `run` started, as ISO 8601 UTC; `None` for a start that does not
/// read.
fn started(run: &LocalRun) -> Option<String> {
    let ms = run.started_at.trim().parse::<u64>().ok()?;
    Some(crate::applog::iso_of(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms)))
}

/// `N case(s)`.
fn cases_word(n: usize) -> String {
    format!("{n} {}", if n == 1 { "case" } else { "cases" })
}

/// One run named for an assistant: its id, when it started, its PBI, how
/// many cases it held, and the areas its cases start in when their scripts
/// name any (`run run-1 (started 2026-10-14T09:12:00Z, PBI #5, 4 cases,
/// area Leave)`). A replay read as a run is named by its own words.
pub fn run_label(run: &LocalRun, scripts: &[CaseScript]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(at) = started(run) {
        parts.push(format!("started {at}"));
    }
    if run.pbi_id > 0 {
        parts.push(format!("PBI #{}", run.pbi_id));
    }
    parts.push(cases_word(run.cases.len()));
    let mut areas: Vec<&str> = Vec::new();
    for c in &run.cases {
        if let Some(a) = scripts.iter().find(|s| s.case_id == c.case_id).and_then(CaseScript::area_name) {
            if !areas.contains(&a) {
                areas.push(a);
            }
        }
    }
    if !areas.is_empty() {
        parts.push(format!("{} {}", if areas.len() == 1 { "area" } else { "areas" }, areas.join(", ")));
    }
    let name = if run_mode_is_replay(&run.id) { run.id.clone() } else { format!("run {}", run.id) };
    format!("{name} ({})", parts.join(", "))
}

/// Of `runs` (newest first, as `store::list_runs` gives them), the other
/// recent ones with a failed or Blocked case: of the `RECENT_RUNS` newest
/// that are not `read_id`, those that started within `RECENT_MS` of
/// `now_ms`, whichever leaves fewer.
pub fn recent_failing<'a>(runs: &'a [LocalRun], read_id: &str, now_ms: u64) -> Vec<&'a LocalRun> {
    runs.iter()
        .filter(|r| r.id != read_id)
        .take(RECENT_RUNS)
        .filter(|r| r.started_at.trim().parse::<u64>().is_ok_and(|at| now_ms.saturating_sub(at) <= RECENT_MS))
        .filter(|r| failed_and_blocked(r) != (0, 0))
        .collect()
}

/// What `get_autorun_failures` asked with neither a case nor a run says
/// around the failures it read: first which run that was, then the other
/// recent runs here with failed or Blocked cases (`recent_failing`), each
/// with its counts and how to read it. `None` for the second when there
/// are none. `scripts` are this machine's, for the areas each run's cases
/// start in.
pub fn newest_run_context(read: &LocalRun, scripts: &[CaseScript], runs: &[LocalRun], now_ms: u64) -> (String, Option<String>) {
    let head = format!(
        "Read {}: the newest on this machine. Ask with run_id to read another.",
        run_label(read, scripts)
    );
    let others = recent_failing(runs, &read.id, now_ms);
    if others.is_empty() {
        return (head, None);
    }
    let mut lines = vec!["Other recent runs on this machine with failed or blocked cases (read one with run_id):".to_string()];
    for r in others {
        let (failed, blocked) = failed_and_blocked(r);
        lines.push(format!("- {}: {failed} failed, {blocked} blocked - run_id={}", run_label(r, scripts), r.id));
    }
    (head, Some(lines.join("\n")))
}
