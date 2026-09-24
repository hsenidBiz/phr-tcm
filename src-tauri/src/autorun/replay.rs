//! Running whole cases with nobody pressing a button per step.
//!
//! Pure: it is handed its browsers, its clock is `Instant`, and the only
//! thing it writes is the run file. It never calls Azure DevOps, and it
//! never fills in a verdict: what it thinks is `proposed`, and a person
//! confirms or changes it afterwards.
//!
//! In a project with recorded module paths a case goes the way a tester
//! goes: sign in, click through the menu to the case's module, check it
//! arrived, then run the steps. A project with none runs as it always has.

use super::nav::{self, Route};
use super::runner::{self, as_action_outcome};
use super::{recipe, signin};
use super::{store, CaseRecord, CaseScript, LocalRun, StepRecord, StepScript};
use crate::browser::actions::{Action, ActionOutcome};
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;
use crate::events::ReplayProgress;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub const SIGN_IN_STEP: i32 = 0;
/// The runner's own "Go to X" line: after the sign-in, before step 1.
pub const MODULE_STEP: i32 = -1;
const AFTER_FAILED_STEP: &str = "not run: an earlier step of this case failed";
const AFTER_FAILED_SIGN_IN: &str = "not run: the sign-in failed";
const AFTER_UNREACHED: &str = "not run: the module screen was not reached";
const AFTER_STOP: &str = "not run: the run was stopped";

/// Where a fresh browser per case comes from. The command gives real ones;
/// the tests give fakes.
pub trait Browsers {
    type D: Driver;
    fn open(&mut self) -> impl std::future::Future<Output = Result<Self::D, String>>;
    fn close(&mut self, d: Self::D) -> impl std::future::Future<Output = ()>;
}

pub struct Proposal {
    pub verdict: &'static str,
    pub reason: String,
}

/// One case of a selection, as the frontend knows it before its script is
/// read. `module` is the test case's Module field.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseToRun {
    pub case_id: i32,
    pub title: String,
    pub module: Option<String>,
}

fn not_run(step: &StepScript, why: &str) -> StepRecord {
    StepRecord {
        step_number: step.step_number,
        outcomes: step.actions.iter().map(|_| ActionOutcome::failed(why)).collect(),
        screenshot: None,
    }
}

fn was_run(o: &ActionOutcome) -> bool {
    !o.detail.starts_with("not run:")
}

/// The runner's sentence for a failed trip to the module, when this failed
/// outcome is one the runner wrote: the "Go to X" line itself, or a
/// `sign_in`'s trip back. Decided by where the outcome sits, never by its
/// words alone - a page's dialog or a script's `check_text` value can say
/// the same thing, and that is still the page failing the test.
fn unreached<'a>(script: &CaseScript, n: i32, i: usize, o: &'a ActionOutcome) -> Option<&'a str> {
    if n == MODULE_STEP {
        return Some(o.detail.as_str());
    }
    let action = script.steps.iter().find(|s| s.step_number == n).and_then(|s| s.actions.get(i));
    match action {
        Some(Action::SignIn { .. }) => nav::unreached_after_sign_in(&o.detail),
        _ => None,
    }
}

/// `signed_in`: None when no account applies to the case, Some(ok) otherwise.
pub fn propose(script: &CaseScript, steps: &[StepRecord], signed_in: Option<bool>, stopped: bool) -> Proposal {
    let ran = || {
        steps
            .iter()
            .flat_map(|s| s.outcomes.iter().enumerate().map(move |(i, o)| (s.step_number, i, o)))
            .filter(|(_, _, o)| was_run(o))
    };
    if let Some((n, _, o)) = ran().find(|(_, _, o)| !o.ok && o.harness) {
        let at = match n {
            SIGN_IN_STEP => "while signing in".to_string(),
            MODULE_STEP => "while going to the module".to_string(),
            _ => format!("at step {n}"),
        };
        return Proposal { verdict: "Blocked", reason: format!("the browser stopped answering {at}: {}", o.detail) };
    }
    // The run could not put the case where its steps begin: that is not
    // the application failing the test.
    if let Some(why) = ran().filter(|(_, _, o)| !o.ok).find_map(|(n, i, o)| unreached(script, n, i, o)) {
        return Proposal { verdict: "Blocked", reason: why.to_string() };
    }
    if signed_in == Some(false) {
        let why = steps
            .iter()
            .find(|s| s.step_number == SIGN_IN_STEP)
            .and_then(|s| s.outcomes.last())
            .map(|o| o.detail.clone())
            .unwrap_or_default();
        return Proposal { verdict: "Blocked", reason: why };
    }
    if let Some((n, _, o)) = ran().find(|(_, _, o)| !o.ok) {
        return Proposal { verdict: "Failed", reason: format!("step {n}: {}", o.detail) };
    }
    if stopped {
        return Proposal { verdict: "", reason: "stopped before it finished".into() };
    }
    if !script.steps.iter().flat_map(|s| &s.actions).any(|a| a.is_check()) {
        return Proposal { verdict: "", reason: "this script checks nothing, so there is nothing to propose".into() };
    }
    Proposal { verdict: "Passed", reason: format!("every action of {} steps passed", script.steps.len()) }
}

/// Run one whole case as the script's own account, with no module step.
/// Kept for callers that have no route; `run_case_as` is the full form.
pub async fn run_case<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    script: &CaseScript,
    timing: &Timing,
    cancel: &AtomicBool,
    on_step: &mut (dyn FnMut(i32) + Send),
) -> CaseRecord {
    run_case_as(d, root, organization, project, script, script.account.as_deref(), None, timing, cancel, on_step).await
}

/// Run one whole case: an optional sign-in as `account` (step
/// `SIGN_IN_STEP`), then, with a `route`, the trip to the module
/// (`MODULE_STEP`), then every scripted step in order, stopping the case
/// (but not the run) after the first step that fails or once `cancel` is
/// set.
#[allow(clippy::too_many_arguments)]
pub async fn run_case_as<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    script: &CaseScript,
    account: Option<&str>,
    route: Option<&Route>,
    timing: &Timing,
    cancel: &AtomicBool,
    on_step: &mut (dyn FnMut(i32) + Send),
) -> CaseRecord {
    let began = Instant::now();
    let mut steps: Vec<StepRecord> = Vec::with_capacity(script.steps.len() + 2);
    let mut signed_in = None;
    let mut current = None;
    let mut stopped = false;

    // Why the rest of the case is not being run, once something decided
    // that. Checked here too, before the sign-in - a stop asked for while
    // this case was still only "opening" must leave it completely
    // untouched, not just cut short after already having signed in.
    let mut skip: Option<&'static str> = if cancel.load(Ordering::SeqCst) {
        stopped = true;
        Some(AFTER_STOP)
    } else {
        None
    };

    if skip.is_none() {
        if let Some(key) = account {
            on_step(SIGN_IN_STEP);
            let out = match signin::prepare(root, organization, project, key) {
                Err(why) => vec![ActionOutcome::failed(why)],
                Ok((recipe, who)) => {
                    let signed = signin::sign_in(d, root, &recipe, &who, timing).await;
                    let mut all = signed.steps.clone();
                    all.push(as_action_outcome(&signed));
                    all
                }
            };
            let ok = out.last().is_some_and(|o| o.ok);
            signed_in = Some(ok);
            steps.push(StepRecord { step_number: SIGN_IN_STEP, outcomes: out, screenshot: None });
        }
        skip = (signed_in == Some(false)).then_some(AFTER_FAILED_SIGN_IN);
    }

    if let (None, Some(r)) = (skip, route) {
        if cancel.load(Ordering::SeqCst) {
            skip = Some(AFTER_STOP);
            stopped = true;
        } else {
            on_step(MODULE_STEP);
            let mut out = nav::reached(&r.path.module, nav::go_to_module(d, r, timing).await);
            if !out.ok && !out.harness {
                out.screenshot = runner::picture(d, root).await;
            }
            if !out.ok {
                skip = Some(AFTER_UNREACHED);
            }
            steps.push(StepRecord { step_number: MODULE_STEP, outcomes: vec![out], screenshot: None });
        }
    }

    for step in &script.steps {
        if skip.is_none() && cancel.load(Ordering::SeqCst) {
            skip = Some(AFTER_STOP);
            stopped = true;
        }
        if let Some(why) = skip {
            steps.push(not_run(step, why));
            continue;
        }
        on_step(step.step_number);
        let outcomes =
            match runner::run_step_routed(d, root, organization, project, step, timing, &mut current, route).await {
                Ok(o) => o,
                Err(why) => step.actions.iter().map(|_| ActionOutcome::failed(why.clone())).collect(),
            };
        let harness = outcomes.iter().any(|o| !o.ok && o.harness);
        let screenshot = if harness { None } else { runner::picture(d, root).await };
        if outcomes.iter().any(|o| !o.ok) {
            skip = Some(AFTER_FAILED_STEP);
        }
        steps.push(StepRecord { step_number: step.step_number, outcomes, screenshot });
    }

    let p = propose(script, &steps, signed_in, stopped);
    CaseRecord {
        case_id: script.case_id,
        title: script.title.clone(),
        verdict: String::new(),
        note: String::new(),
        steps,
        proposed: p.verdict.to_string(),
        reason: p.reason,
        duration_ms: i32::try_from(began.elapsed().as_millis()).ok(),
        account: account.map(str::to_string),
    }
}

fn unrun(case_id: i32, title: &str, proposed: &str, reason: String) -> CaseRecord {
    CaseRecord {
        case_id,
        title: title.to_string(),
        verdict: String::new(),
        note: String::new(),
        steps: vec![],
        proposed: proposed.to_string(),
        reason,
        duration_ms: None,
        account: None,
    }
}

/// A case that cannot start (design §5): every step shown as not run, the
/// verdict proposed is Blocked, and no browser was opened for it.
fn blocked_before_start(script: &CaseScript, account: Option<&str>, reason: String) -> CaseRecord {
    let why = format!("not run: {reason}");
    CaseRecord {
        case_id: script.case_id,
        title: script.title.clone(),
        verdict: String::new(),
        note: String::new(),
        steps: script.steps.iter().map(|s| not_run(s, &why)).collect(),
        proposed: "Blocked".to_string(),
        reason,
        duration_ms: None,
        account: account.map(str::to_string),
    }
}

/// One `ReplayProgress`, built from plain values rather than a closure so
/// it never has to borrow `run` or `progress` - both are busy elsewhere in
/// the loop this is called from.
#[allow(clippy::too_many_arguments)]
fn tell(
    run_id: &str,
    index: u32,
    total: u32,
    case_id: i32,
    title: &str,
    phase: &str,
    step_number: i32,
    steps: u32,
    proposed: &str,
) -> ReplayProgress {
    ReplayProgress {
        run_id: run_id.to_string(),
        index,
        total,
        case_id,
        title: title.to_string(),
        phase: phase.to_string(),
        step_number,
        steps,
        proposed: proposed.to_string(),
    }
}

/// Run a selection with no module or run account per case - the shape
/// every caller used before module paths.
#[allow(clippy::too_many_arguments)]
pub async fn run_selection<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    organization: &str,
    project: &str,
    run: &mut LocalRun,
    cases: &[(i32, String)],
    timing: &Timing,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(ReplayProgress) + Send),
) -> Result<(), String> {
    let cases: Vec<CaseToRun> =
        cases.iter().map(|(case_id, title)| CaseToRun { case_id: *case_id, title: title.clone(), module: None }).collect();
    run_cases(browsers, root, organization, project, run, &cases, None, timing, cancel, progress).await
}

/// Run a whole selection, one fresh browser each, saving the run after
/// every case so a crash or a stop loses nothing. `run_account` signs in
/// every script that names no account of its own. The module paths file
/// is read once, first: an unreadable one stops the run before any
/// browser opens.
#[allow(clippy::too_many_arguments)]
pub async fn run_cases<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    organization: &str,
    project: &str,
    run: &mut LocalRun,
    cases: &[CaseToRun],
    run_account: Option<&str>,
    timing: &Timing,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(ReplayProgress) + Send),
) -> Result<(), String> {
    let nav_file = nav::load_nav(root, organization, project)?;
    let sign_in_recipe = if nav_file.modules.is_empty() {
        None
    } else {
        // An unreadable recipe gives no route here; the case's own sign-in
        // (step 0, `signin::prepare`) then fails with the read error itself.
        recipe::load_recipe(root, organization, project).ok().flatten()
    };
    let total = cases.len() as u32;
    let run_id = run.id.clone();
    let mut save_error: Option<String> = None;

    for (i, case) in cases.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let index = i as u32;
        let (case_id, title) = (case.case_id, case.title.as_str());
        // The script's own step count, not what the case actually ran -
        // it must read the same on every phase of a case, including
        // "done", whether the browser opened or the case has a script at
        // all. 0 only when there is genuinely no script to count.
        let mut count = 0u32;
        let record = match store::load_script(root, case_id) {
            Err(why) => unrun(case_id, title, "", format!("the script could not be read: {why}")),
            Ok(None) => unrun(case_id, title, "", "this case has no script on this machine".into()),
            Ok(Some(script)) => {
                count = script.steps.len() as u32;
                let account = script.account.as_deref().or(run_account);
                match nav::route_for(&nav_file, case.module.as_deref(), account) {
                    Err(why) => blocked_before_start(&script, account, why),
                    Ok(path) => {
                        // A path but no recipe: the sign-in fails first and
                        // says what to add, so no route is needed.
                        let route = path.zip(sign_in_recipe.as_ref()).map(|(p, r)| Route::new(r, p.clone()));
                        progress(tell(&run_id, index, total, case_id, title, "opening", 0, count, ""));
                        match browsers.open().await {
                            Err(why) => unrun(case_id, title, "Blocked", format!("the browser did not open: {why}")),
                            Ok(mut d) => {
                                let mut on_step = |n: i32| {
                                    let phase = match n {
                                        SIGN_IN_STEP => "signing_in",
                                        MODULE_STEP => "module",
                                        _ => "step",
                                    };
                                    progress(tell(&run_id, index, total, case_id, title, phase, n, count, ""));
                                };
                                let rec = run_case_as(
                                    &mut d,
                                    root,
                                    organization,
                                    project,
                                    &script,
                                    account,
                                    route.as_ref(),
                                    timing,
                                    cancel,
                                    &mut on_step,
                                )
                                .await;
                                browsers.close(d).await;
                                rec
                            }
                        }
                    }
                }
            }
        };
        let proposed = record.proposed.clone();
        run.cases.push(record);
        if let Err(e) = store::save_run(root, run) {
            save_error.get_or_insert(e);
        }
        progress(tell(&run_id, index, total, case_id, title, "done", 0, count, &proposed));
    }

    save_error.map_or(Ok(()), Err)
}
