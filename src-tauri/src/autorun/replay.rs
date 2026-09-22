//! Running whole cases with nobody pressing a button per step.
//!
//! Pure: it is handed its browsers, its clock is `Instant`, and the only
//! thing it writes is the run file. It never calls Azure DevOps, and it
//! never fills in a verdict: what it thinks is `proposed`, and a person
//! confirms or changes it afterwards.

use super::runner::{self, as_action_outcome};
use super::signin;
use super::{store, CaseRecord, CaseScript, LocalRun, StepRecord, StepScript};
use crate::browser::actions::ActionOutcome;
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;
use crate::events::ReplayProgress;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub const SIGN_IN_STEP: i32 = 0;
const AFTER_FAILED_STEP: &str = "not run: an earlier step of this case failed";
const AFTER_FAILED_SIGN_IN: &str = "not run: the sign-in failed";
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

/// `signed_in`: None when the script names no account, Some(ok) otherwise.
pub fn propose(script: &CaseScript, steps: &[StepRecord], signed_in: Option<bool>, stopped: bool) -> Proposal {
    let ran = || {
        steps.iter().flat_map(|s| s.outcomes.iter().map(move |o| (s.step_number, o))).filter(|(_, o)| was_run(o))
    };
    if let Some((n, o)) = ran().find(|(_, o)| !o.ok && o.harness) {
        let at = if n == SIGN_IN_STEP { "while signing in".to_string() } else { format!("at step {n}") };
        return Proposal { verdict: "Blocked", reason: format!("the browser stopped answering {at}: {}", o.detail) };
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
    if let Some((n, o)) = ran().find(|(_, o)| !o.ok) {
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

/// Run one whole case: an optional sign-in as step `SIGN_IN_STEP`, then
/// every scripted step in order, stopping the case (but not the run) after
/// the first step that fails or once `cancel` is set.
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
    let began = Instant::now();
    let mut steps: Vec<StepRecord> = Vec::with_capacity(script.steps.len() + 1);
    let mut signed_in = None;
    let mut account = None;

    if let Some(key) = &script.account {
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

    // Why the rest of the case is not being run, once something decided that.
    let mut skip: Option<&'static str> = (signed_in == Some(false)).then_some(AFTER_FAILED_SIGN_IN);
    let mut stopped = false;
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
        let outcomes = match runner::run_step(d, root, organization, project, step, timing, &mut account).await {
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
        account: script.account.clone(),
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

/// Run a whole selection of cases, one fresh browser each, saving the run
/// after every case so a crash or a stop loses nothing.
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
    let total = cases.len() as u32;
    let run_id = run.id.clone();
    let mut save_error: Option<String> = None;

    for (i, (case_id, title)) in cases.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let index = i as u32;
        let record = match store::load_script(root, *case_id) {
            Err(why) => unrun(*case_id, title, "", format!("the script could not be read: {why}")),
            Ok(None) => unrun(*case_id, title, "", "this case has no script on this machine".into()),
            Ok(Some(script)) => {
                let count = script.steps.len() as u32;
                progress(tell(&run_id, index, total, *case_id, title, "opening", 0, count, ""));
                match browsers.open().await {
                    Err(why) => unrun(*case_id, title, "Blocked", format!("the browser did not open: {why}")),
                    Ok(mut d) => {
                        let mut on_step = |n: i32| {
                            let phase = if n == SIGN_IN_STEP { "signing_in" } else { "step" };
                            progress(tell(&run_id, index, total, *case_id, title, phase, n, count, ""));
                        };
                        let rec =
                            run_case(&mut d, root, organization, project, &script, timing, cancel, &mut on_step)
                                .await;
                        browsers.close(d).await;
                        rec
                    }
                }
            }
        };
        let proposed = record.proposed.clone();
        let steps_done = record.steps.len() as u32;
        run.cases.push(record);
        if let Err(e) = store::save_run(root, run) {
            save_error.get_or_insert(e);
        }
        progress(tell(&run_id, index, total, *case_id, title, "done", 0, steps_done, &proposed));
    }

    save_error.map_or(Ok(()), Err)
}
