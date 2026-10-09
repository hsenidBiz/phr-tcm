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

use super::components::{ran_actions, ComponentFile, Ran};
use super::nav::{self, Route};
use super::runner::{self, as_action_outcome};
use super::lease::{Held, Holder};
use super::{preconditions, recipe, setup, signin, transient};
use super::plan::Reset;
use super::{store, CasePhases, CaseRecord, CaseScript, LocalRun, ResetRecord, StepRecord, StepScript, RESET_CONTINUED, RESET_STOPPED};
use crate::api_templates::gate::StageDb;
use crate::browser::actions::{Action, ActionOutcome};
use crate::browser::cdp::Driver;
use crate::browser::save_guard;
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
use super::runner::AFTER_STOP;

/// Added to a failed trip's sentence when the page log had something to
/// say, so the person knows where to look.
pub const PAGE_LOG_NOTE: &str = " What the page was doing then is in Settings, Logs.";

/// A case that never reached its module says only what it could not
/// find; the page's failed and unfinished requests and console errors say
/// why. They go to the application log - what a bug report ships - and
/// not into the run, where a long list would bury the sentence the person
/// reads (PeoplesHR, 2026-10-02: a first case stuck on a spinner).
fn log_the_page<D: Driver>(d: &D, who: &str, out: &mut ActionOutcome) {
    if nav::log_page(d, who, &out.detail) {
        out.detail.push_str(PAGE_LOG_NOTE);
    }
}

/// The run's own trip to the case's module before step 1, as the one
/// outcome its "Go to X" line records: `nav::reach_module` from where the
/// browser is (`from`), then what the page did on the way. A save the
/// module's page sent as it opened is the outcome, with a picture; a trip
/// that failed on the page gets a picture, and the page log goes to the
/// application log under `who`. Shared by the unattended run and the
/// supervised browser's replay to a step (`replay_to`), so the two travel
/// the same way.
pub async fn trip_to_module<D: Driver>(
    d: &mut D,
    root: &Path,
    route: &Route,
    from: nav::TripFrom,
    timing: &Timing,
    who: &str,
) -> ActionOutcome {
    let mut out = nav::reach_module(d, route, from, timing, who).await;
    // A save the module's page sent as it opened fails the case here,
    // before step 1 acts on it.
    if let Some(sentence) = d.take_save_blocked() {
        out = ActionOutcome::failed(sentence);
        out.screenshot = runner::picture(d, root).await;
    } else if !out.ok && !out.harness {
        out.screenshot = runner::picture(d, root).await;
        // Read after the picture: taking it read every event the page had
        // sent by then - a save among them is the reason.
        match d.take_save_blocked() {
            Some(sentence) => {
                let shot = out.screenshot.take();
                out = ActionOutcome::failed(sentence);
                out.screenshot = shot;
            }
            None => log_the_page(d, who, &mut out),
        }
    }
    out
}

/// How the application log names a case of an unattended run.
fn who(case_id: i32) -> String {
    format!("unattended run, case {case_id}")
}

/// Where each case's fresh browser comes from: for an unattended run, a
/// fresh context in the run's one browser (`one_browser`). The tests give
/// fakes.
pub trait Browsers {
    type D: Driver;
    fn open(&mut self) -> impl std::future::Future<Output = Result<Self::D, String>>;
    fn close(&mut self, d: Self::D) -> impl std::future::Future<Output = ()>;
}

/// The reason each case left when a run ends at a reset point is recorded
/// with, and the outcome of each of its steps.
pub const STOPPED_AT_RESET: &str = "not run: the run stopped at a reset point";

/// Where an unattended run pauses at a reset point. The command gives one
/// that asks the person in the app (`reset_wait::AppGate`); the tests give
/// one that answers as told.
pub trait ResetGate {
    /// Wait at `reset` until a person answers: `true` carries on with the
    /// next phase, `false` ends the run there. `remaining` is every case
    /// still to run, the next one first. A Stop (`cancel`) while waiting
    /// answers `false`. There is no time limit.
    fn wait(&mut self, reset: &Reset, remaining: &[i32], cancel: &AtomicBool) -> impl std::future::Future<Output = bool>;
}

/// A gate for a run with no reset points: it is never asked, and would
/// carry on if it were.
pub struct NeverPauses;

impl ResetGate for NeverPauses {
    async fn wait(&mut self, _reset: &Reset, _remaining: &[i32], _cancel: &AtomicBool) -> bool {
        true
    }
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

/// Whole milliseconds of a span.
fn ms(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn not_run(step: &StepScript, why: &str) -> StepRecord {
    StepRecord {
        step_number: step.step_number,
        outcomes: step.actions.iter().map(|_| ActionOutcome::failed(why)).collect(),
        screenshot: None,
        downloads: Vec::new(),
        tab: None,
        dialog: None,
        components: Vec::new(), duration_ms: None,
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
fn unreached<'a>(action: Option<&Action>, n: i32, o: &'a ActionOutcome) -> Option<&'a str> {
    if n == MODULE_STEP {
        return Some(o.detail.as_str());
    }
    match action {
        // Another holder had the account: the case was never signed in as
        // it, so the run could not carry it out - Blocked, not Failed.
        Some(Action::SignIn { .. }) if super::lease::is_in_use(&o.detail) => Some(o.detail.as_str()),
        Some(Action::SignIn { .. }) => nav::unreached_after_sign_in(&o.detail),
        // The runner's own refusal (runner.rs, the `Navigate if
        // !direct_urls` arm) is written without going through
        // `execute_in`, so no page dialog is ever appended to it: it is
        // exactly this sentence, for this step.
        Some(Action::Navigate { .. } | Action::OpenTab { .. }) if o.detail == nav::no_address(n) => Some(o.detail.as_str()),
        // The same refusal, met inside a guard: said after what the guard
        // had already done.
        Some(Action::WhenVisible { .. }) if o.detail.ends_with(&nav::no_address(n)) => Some(o.detail.as_str()),
        _ => None,
    }
}

/// `signed_in`: None when no account applies to the case, Some(ok) otherwise.
pub fn propose(script: &CaseScript, steps: &[StepRecord], signed_in: Option<bool>, stopped: bool) -> Proposal {
    propose_with(script, steps, signed_in, stopped, &ComponentFile::default())
}

/// [`propose`], with the project's components: an outcome a component ran
/// is read against the action it ran (`components::ran_actions`), so a
/// refusal inside a component is told apart the same way.
pub fn propose_with(
    script: &CaseScript,
    steps: &[StepRecord],
    signed_in: Option<bool>,
    stopped: bool,
    components: &ComponentFile,
) -> Proposal {
    let paired: Vec<(i32, Vec<Ran>)> = steps
        .iter()
        .map(|s| {
            let ran = script
                .steps
                .iter()
                .find(|st| st.step_number == s.step_number)
                .map(|st| ran_actions(&st.actions, &s.outcomes, components, &s.components))
                .unwrap_or_default();
            (s.step_number, ran)
        })
        .collect();
    let action_at = |n: i32, i: usize| {
        paired.iter().find(|(m, _)| *m == n).and_then(|(_, r)| r.get(i)).and_then(|r| r.action.as_ref())
    };
    let ran = || {
        steps
            .iter()
            .flat_map(|s| s.outcomes.iter().enumerate().map(move |(i, o)| (s.step_number, i, o)))
            .filter(|(_, _, o)| was_run(o))
    };
    // A no-save script whose page tried to save failed, wherever that
    // happened - even on the way to the module, which would otherwise read
    // as a run that could not start the case. Only the runner writes this
    // sentence, and only for a no-save script.
    if script.no_save {
        if let Some((n, _, o)) = ran().find(|(_, _, o)| !o.ok && save_guard::is_blocked(&o.detail)) {
            let at = match n {
                SIGN_IN_STEP => "while signing in".to_string(),
                MODULE_STEP => "while going to the module".to_string(),
                _ => format!("step {n}"),
            };
            return Proposal { verdict: "Failed", reason: format!("{at}: {}", o.detail) };
        }
    }
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
    if let Some(why) = ran().filter(|(_, _, o)| !o.ok).find_map(|(n, i, o)| unreached(action_at(n, i), n, o)) {
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
        let usual = format!("step {n}: {}", o.detail);
        // The case's script says this step's failure is the application's:
        // say so in front of what the page showed.
        let reason = match &script.suspected_defect {
            Some(mark) if mark.step_number == n => format!("{} - {usual}", super::defects::label(mark)),
            _ => usual,
        };
        return Proposal { verdict: "Failed", reason };
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
    let mut lease = Held::new(Holder::Case { run: String::new() }, timing.lease_wait());
    let account = script.account.as_deref();
    run_case_as(d, root, organization, project, &mut lease, script, account, None, timing, cancel, on_step).await
}

/// Run one whole case: an optional sign-in as `account` (step
/// `SIGN_IN_STEP`), then, with a `route`, the trip to the module
/// (`MODULE_STEP`), then every scripted step in order, stopping the case
/// (but not the run) after the first step that fails or once `cancel` is
/// set.
///
/// The case signs in only once it holds its account in `lease`, which
/// waits for a case or a template run up to `timing`'s lease wait; a case
/// that cannot get it is Blocked with the sentence that says who had it,
/// and never signs in. The caller owns `lease` and lets it go once the
/// case's browser is closed (`one_go`).
///
/// However the case ends - passed, failed, stopped, or never started -
/// every tab but `main` is closed before this returns: no tab carries over
/// into the next case.
#[allow(clippy::too_many_arguments)]
pub async fn run_case_as<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    lease: &mut Held,
    script: &CaseScript,
    account: Option<&str>,
    route: Option<&Route>,
    timing: &Timing,
    cancel: &AtomicBool,
    on_step: &mut (dyn FnMut(i32) + Send),
) -> CaseRecord {
    let record = run_case_in(d, root, organization, project, lease, script, account, route, timing, cancel, on_step).await;
    d.close_other_tabs().await;
    record
}

#[allow(clippy::too_many_arguments)]
async fn run_case_in<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    lease: &mut Held,
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
    let (mut sign_in_took, mut area_took) = (0u64, 0u64);

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

    // A no-save script never runs unguarded: the guard goes on before
    // anything happens in the browser, or the case does not run at all.
    if skip.is_none() && script.no_save {
        let words = nav::load_nav(root, organization, project).map(|n| n.save_words);
        let guarded = match words {
            Err(why) => Err(why),
            Ok(words) => d.guard_saves(&words).await.map_err(|e| e.to_string()),
        };
        if let Err(why) = guarded {
            let mut record = blocked_before_start(script, account, save_guard::setup_failed(&why));
            record.duration_ms = i32::try_from(began.elapsed().as_millis()).ok();
            return record;
        }
    }

    if skip.is_none() {
        if let Some(key) = account {
            on_step(SIGN_IN_STEP);
            // The account is this case's from before its sign-in to its
            // end, whichever way the sign-in goes: one that fails partway
            // may still have signed the account in.
            // A Stop pressed while the case waits for its account ends the
            // wait; one that lands as the account comes free still stops
            // the case before it signs in.
            let held = tokio::select! {
                held = lease.hold(root, key) => Some(held),
                () = stop_asked(cancel) => None,
            };
            match held {
                Some(Ok(())) if !cancel.load(Ordering::SeqCst) => {}
                Some(Err(why)) if !cancel.load(Ordering::SeqCst) => {
                    let mut record = blocked_before_start(script, account, why);
                    record.duration_ms = i32::try_from(began.elapsed().as_millis()).ok();
                    return record;
                }
                _ => {
                    stopped = true;
                    skip = Some(AFTER_STOP);
                }
            }
        }
    }
    if skip.is_none() {
        if let Some(key) = account {
            let signing_in = Instant::now();
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
            sign_in_took = ms(signing_in.elapsed());
            steps.push(StepRecord { step_number: SIGN_IN_STEP, outcomes: out, screenshot: None, downloads: Vec::new(), tab: None, dialog: None, components: Vec::new(), duration_ms: Some(sign_in_took) });
        }
        skip = (signed_in == Some(false)).then_some(AFTER_FAILED_SIGN_IN);
    }

    if let (None, Some(r)) = (skip, route) {
        if cancel.load(Ordering::SeqCst) {
            skip = Some(AFTER_STOP);
            stopped = true;
        } else {
            on_step(MODULE_STEP);
            // The case's own sign-in just above, when it had one; otherwise
            // the browser comes as it was left.
            let from = if signed_in == Some(true) { nav::TripFrom::SignIn } else { nav::TripFrom::Elsewhere };
            let reaching = Instant::now();
            let out = trip_to_module(d, root, r, from, timing, &who(script.case_id)).await;
            area_took = ms(reaching.elapsed());
            if save_guard::is_blocked(&out.detail) {
                skip = Some(AFTER_FAILED_STEP);
            } else if !out.ok {
                skip = Some(AFTER_UNREACHED);
            }
            steps.push(StepRecord {
                step_number: MODULE_STEP,
                outcomes: vec![out],
                screenshot: None,
                downloads: Vec::new(),
                tab: None,
                dialog: None,
                components: Vec::new(), duration_ms: Some(area_took),
            });
        }
    }

    // Where each step that ran began, by its index in `steps`: the files
    // the browser saved are put on the step they started in once the case
    // is over, so one still arriving as its step ended is not left out.
    let mut step_began: Vec<(usize, Instant)> = Vec::new();
    // The other areas the script's `return_to_area` actions name, read once
    // before step 1, as the case's own route was.
    let names = runner::named_areas(script.steps.iter().flat_map(|s| s.actions.iter()));
    let areas = runner::area_routes(root, organization, project, &names);
    // What the page met before step 1 - the sign-in, the trip to the
    // module - is no step's error.
    super::page_errors::drop_all(d);
    let mut page_errors_seen = 0u32;
    let stepping = Instant::now();
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
        let asked_at = Instant::now();
        // A bare `return_to_area` goes where the run went before step 1; one
        // that names an area, to that area.
        let area = match route {
            Some(r) => runner::AreaRoute::To(r),
            None => runner::AreaRoute::Unknown(runner::NO_AREA_IN_RUN),
        };
        let mut in_run = runner::InRun {
            cancel: Some(cancel),
            areas: Some(&areas),
            fail_on_unexpected_dialog: script.fail_on_unexpected_dialog,
            page_errors: script.page_errors,
            ignore_page_errors: script.ignore_page_errors.clone(),
            ..Default::default()
        };
        let outcomes = match runner::run_step_in_run(
            d,
            root,
            organization,
            project,
            step,
            timing,
            &mut current,
            lease,
            route,
            area,
            &mut in_run,
        )
        .await
        {
            Ok(o) => o,
            Err(why) => step.actions.iter().map(|_| ActionOutcome::failed(why.clone())).collect(),
        };
        // The step's own start, the one its `expect_download` used.
        step_began.push((steps.len(), in_run.began.unwrap_or(asked_at)));
        let mut outcomes = outcomes;
        // A Stop that ended a wait inside the step: the case stops here, as
        // it would have before the next step, with no picture to wait for.
        let stopped_inside = outcomes.iter().any(|o| o.detail == AFTER_STOP);
        let harness = outcomes.iter().any(|o| !o.ok && o.harness);
        let screenshot = if harness || stopped_inside { None } else { runner::picture(d, root).await };
        // A save the page sent after the step's last action had already
        // passed (read while the picture was taken) is still this step's.
        if let Some(sentence) = d.take_save_blocked() {
            if outcomes.iter().all(|o| o.ok) {
                if let Some(last) = outcomes.last_mut() {
                    *last = ActionOutcome::failed(sentence);
                }
            }
        }
        if stopped_inside {
            stopped = true;
            skip = Some(AFTER_STOP);
        } else if outcomes.iter().any(|o| !o.ok) {
            skip = Some(AFTER_FAILED_STEP);
        }
        steps.push(StepRecord { step_number: step.step_number, outcomes, screenshot, downloads: Vec::new(), tab: in_run.tab, dialog: in_run.dialog, components: in_run.components, duration_ms: Some(ms(asked_at.elapsed())) });
        page_errors_seen += in_run.page_errors_seen;
    }

    // The case's one wait for a download still arriving (`one_go` does not
    // wait again), then each file is put on the step it started in.
    let took = began.elapsed();
    let steps_took = ms(stepping.elapsed());
    settle_downloads(d, cancel).await;
    if !step_began.is_empty() {
        // Every tab's: a step's file is the step's whichever tab saved it.
        let all = d.all_downloads();
        for (i, &(at, from)) in step_began.iter().enumerate() {
            let until = step_began.get(i + 1).map(|&(_, next)| next);
            steps[at].downloads = runner::saved_between(&all, from, until);
        }
    }

    // A file that does not read leaves a component's outcomes unpaired.
    let components = super::components::load_components(root, organization, project).unwrap_or_default();
    let p = propose_with(script, &steps, signed_in, stopped, &components);
    CaseRecord {
        case_id: script.case_id,
        title: script.title.clone(),
        verdict: String::new(),
        note: String::new(),
        steps,
        proposed: p.verdict.to_string(),
        reason: p.reason,
        duration_ms: i32::try_from(took.as_millis()).ok(),
        account: account.map(str::to_string),
        retried: None,
        notice: None,
        page_errors_seen,
        phases: Some(CasePhases { sign_in_ms: sign_in_took, area_ms: area_took, steps_ms: steps_took, total_ms: ms(took), ..Default::default() }),
    }
}

/// Returns once `cancel` is set, looking every `STOP_POLL`.
async fn stop_asked(cancel: &AtomicBool) {
    while !cancel.load(Ordering::SeqCst) {
        tokio::time::sleep(STOP_POLL).await;
    }
}

/// How often a case waiting for its account looks for a Stop.
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(50);

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
        retried: None,
        notice: None,
        page_errors_seen: 0, phases: None,
    }
}

/// A case left when the run ended at a reset point: not run, nothing
/// proposed, and each step of its script (when it has one) not run for the
/// same reason.
fn stopped_at_reset(root: &Path, case: &CaseToRun) -> CaseRecord {
    let mut record = unrun(case.case_id, &case.title, "", STOPPED_AT_RESET.to_string());
    if let Ok(Some(script)) = store::load_script(root, case.case_id) {
        record.steps = script.steps.iter().map(|s| not_run(s, STOPPED_AT_RESET)).collect();
    }
    record
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
        retried: None,
        notice: None,
        page_errors_seen: 0, phases: None,
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

/// Where a case sits in the run, for its progress events.
struct Place<'a> {
    run_id: &'a str,
    index: u32,
    total: u32,
    case_id: i32,
    title: &'a str,
    count: u32,
}

/// What one go at a case needs.
struct Go<'a> {
    root: &'a Path,
    organization: &'a str,
    project: &'a str,
    script: &'a CaseScript,
    account: Option<&'a str>,
    route: Option<&'a Route>,
    timing: &'a Timing,
    cancel: &'a AtomicBool,
}

/// How long a case's browser stays open, once its steps are done, for a
/// download still on its way.
const DOWNLOAD_SETTLE: std::time::Duration = std::time::Duration::from_secs(5);

/// A download the last step started may still be arriving as the case
/// ends. Its browser is kept a little while it does, so the file is kept
/// under its own name rather than left half written under its guid. A case
/// with nothing in progress closes at once, and a Stop ends the wait within
/// `STOP_POLL`.
pub async fn settle_downloads<D: Driver>(d: &mut D, cancel: &AtomicBool) {
    use crate::browser::downloads::DownloadState;
    let until = Instant::now() + DOWNLOAD_SETTLE;
    while !cancel.load(Ordering::SeqCst)
        && Instant::now() < until
        && d.all_downloads().iter().any(|e| e.state == DownloadState::InProgress)
    {
        d.idle(STOP_POLL).await;
    }
}

/// One go at a case in a fresh browser: opened, the case run, and the
/// browser given back. `Err` is why the browser did not open: the case
/// never ran, so the caller decides what record that leaves.
async fn one_go<B: Browsers>(
    browsers: &mut B,
    go: &Go<'_>,
    at: &Place<'_>,
    progress: &mut (dyn FnMut(ReplayProgress) + Send),
) -> Result<CaseRecord, String> {
    let (run_id, index, total, case_id, title, count) = (at.run_id, at.index, at.total, at.case_id, at.title, at.count);
    progress(tell(run_id, index, total, case_id, title, "opening", 0, count, ""));
    let opening = Instant::now();
    match browsers.open().await {
        Err(why) => Err(why),
        Ok(mut d) => {
            let open_ms = ms(opening.elapsed());
            // The case's downloads are kept with the run. A browser that
            // will not save them still runs the case: a step that checks a
            // download then says none came.
            if let Err(e) = d.enable_downloads(&store::downloads_dir(go.root, run_id)).await {
                crate::applog::warn(format!("{}: downloads could not be switched on: {e}", who(case_id)));
            }
            let mut on_step = |n: i32| {
                let phase = match n {
                    SIGN_IN_STEP => "signing_in",
                    MODULE_STEP => "module",
                    _ => "step",
                };
                progress(tell(run_id, index, total, case_id, title, phase, n, count, ""));
            };
            // The case's account, held until its browser is closed: a local
            // here, so an end, a stop (this future dropped) and a panic all
            // let it go too.
            let mut lease = Held::new(Holder::Case { run: run_id.to_string() }, go.timing.lease_wait());
            let mut rec = run_case_as(
                &mut d,
                go.root,
                go.organization,
                go.project,
                &mut lease,
                go.script,
                go.account,
                go.route,
                go.timing,
                go.cancel,
                &mut on_step,
            )
            .await;
            // `run_case_as` has already waited for a download still
            // arriving, once, so the browser closes now.
            let closing = Instant::now();
            browsers.close(d).await;
            drop(lease);
            let close_ms = ms(closing.elapsed());
            // Open to close. The case's own parts are measured inside it, so
            // a case that never got as far as its steps still says how long
            // the browser took.
            let phases = rec.phases.get_or_insert_with(CasePhases::default);
            phases.open_ms = open_ms;
            phases.close_ms = close_ms;
            phases.total_ms = ms(opening.elapsed());
            crate::applog::info(format!("{}: {}", who(case_id), phases.line()));
            Ok(rec)
        }
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
    run_cases(browsers, root, organization, project, run, &cases, None, false, timing, cancel, progress).await
}

/// `run_cases_checked` with no database for preconditions: a case that
/// has any is Blocked with `preconditions::NEED_DB`, and every other case
/// runs as it always has.
#[allow(clippy::too_many_arguments)]
pub async fn run_cases<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    organization: &str,
    project: &str,
    run: &mut LocalRun,
    cases: &[CaseToRun],
    run_account: Option<&str>,
    retry_transient: bool,
    timing: &Timing,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(ReplayProgress) + Send),
) -> Result<(), String> {
    let no_db: preconditions::PreconditionDb<preconditions::NoDb> =
        preconditions::PreconditionDb::Missing(preconditions::NEED_DB.to_string());
    run_cases_checked(
        browsers,
        root,
        organization,
        project,
        run,
        cases,
        run_account,
        retry_transient,
        timing,
        cancel,
        &no_db,
        progress,
    )
    .await
}

/// Run a whole selection, one fresh browser each, saving the run after
/// every case so a crash or a stop loses nothing. `run_account`, the
/// account the person picked for the run, signs in every case - over the
/// account a script names, which only decides when nothing was picked. A
/// `sign_in` step inside a script still changes to the account it names.
/// The module paths file
/// is read once, first: an unreadable one stops the run before any
/// browser opens. With `retry_transient`, a case whose failure looked
/// transient (`transient::is_transient`) runs once more in a fresh browser.
/// A case with preconditions has them checked against `precondition_db`
/// before its browser opens; one not met Blocks the case with its sentence
/// (`preconditions::check_case`), it never signs in, and the run goes on.
/// While Database Read Access is off (`PreconditionDb::ReadingOff`) none
/// is checked, and such a case runs carrying the `notice` that says so.
/// After the preconditions and before its browser opens, a case that uses
/// fixtures is prepared (`setup::prepare_case`): a setup not approved, a
/// shared fixture never built or a setup run that failed Blocks it with
/// that sentence, and its setup's browser comes from `browsers` too.
#[allow(clippy::too_many_arguments)]
pub async fn run_cases_checked<B: Browsers, P: StageDb>(
    browsers: &mut B,
    root: &Path,
    organization: &str,
    project: &str,
    run: &mut LocalRun,
    cases: &[CaseToRun],
    run_account: Option<&str>,
    retry_transient: bool,
    timing: &Timing,
    cancel: &AtomicBool,
    precondition_db: &preconditions::PreconditionDb<P>,
    progress: &mut (dyn FnMut(ReplayProgress) + Send),
) -> Result<(), String> {
    run_cases_planned(
        browsers,
        root,
        organization,
        project,
        run,
        cases,
        run_account,
        retry_transient,
        timing,
        cancel,
        precondition_db,
        &[],
        &mut NeverPauses,
        progress,
    )
    .await
}

/// [`run_cases_checked`] with the plan's reset points (design §3). Before a
/// case that a reset in `resets` names, once the case before it has ended
/// and its browser has closed, the run pauses at `gate`. Continue runs the
/// next phase; Stop (or the run's own Stop while paused) ends the run
/// there, with every case left recorded as not run
/// (`STOPPED_AT_RESET`). Each pause is kept in `run.resets` with how long
/// it lasted and how it ended. `cases` are in the order to run them.
#[allow(clippy::too_many_arguments)]
pub async fn run_cases_planned<B: Browsers, P: StageDb, G: ResetGate>(
    browsers: &mut B,
    root: &Path,
    organization: &str,
    project: &str,
    run: &mut LocalRun,
    cases: &[CaseToRun],
    run_account: Option<&str>,
    retry_transient: bool,
    timing: &Timing,
    cancel: &AtomicBool,
    precondition_db: &preconditions::PreconditionDb<P>,
    resets: &[Reset],
    gate: &mut G,
    progress: &mut (dyn FnMut(ReplayProgress) + Send),
) -> Result<(), String> {
    // Each error says for itself where the run got to, so the command can
    // pass it on as it is: this one stops the run before any case.
    let nav_file = nav::load_nav(root, organization, project).map_err(|e| format!("the run did not start: {e}"))?;
    let sign_in_recipe = if nav_file.modules.is_empty() {
        None
    } else {
        // An unreadable recipe gives no route here; the case's own sign-in
        // (step 0, `signin::prepare`) then fails with the read error itself.
        recipe::load_effective_recipe(root, organization, project).ok()
    };
    let total = cases.len() as u32;
    let run_id = run.id.clone();
    let mut save_error: Option<String> = None;
    // Where this call's cases start, so the evidence below counts only
    // what THIS run did, even into a run record that already held some.
    let first = run.cases.len();

    for (i, case) in cases.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        // A reset point before this case: the case before it has ended and
        // its browser is closed, so the person can put things back now.
        if let Some(reset) = resets.iter().filter(|_| i > 0).find(|r| r.before_case_id == case.case_id) {
            let remaining: Vec<i32> = cases[i..].iter().map(|c| c.case_id).collect();
            crate::applog::info(format!("Auto-run unattended: paused before case {} for a reset", case.case_id));
            let paused = Instant::now();
            let go_on = gate.wait(reset, &remaining, cancel).await && !cancel.load(Ordering::SeqCst);
            let waited_ms = u32::try_from(paused.elapsed().as_millis()).unwrap_or(u32::MAX);
            run.resets.push(ResetRecord {
                before_case_id: reset.before_case_id,
                names: reset.names.clone(),
                changed_by: reset.changed_by.clone(),
                waited_ms,
                outcome: if go_on { RESET_CONTINUED } else { RESET_STOPPED }.to_string(),
            });
            crate::applog::info(format!(
                "Auto-run unattended: the reset before case {} {} after {waited_ms} ms",
                case.case_id,
                if go_on { "continued" } else { "stopped the run" },
            ));
            if !go_on {
                for (k, rest) in cases.iter().enumerate().skip(i) {
                    let record = stopped_at_reset(root, rest);
                    let count = record.steps.len() as u32;
                    run.cases.push(record);
                    progress(tell(&run_id, k as u32, total, rest.case_id, &rest.title, "done", 0, count, ""));
                }
                if let Err(e) = store::save_run(root, run) {
                    save_error.get_or_insert(e);
                }
                break;
            }
            // The pause is on disk before the next case starts.
            if let Err(e) = store::save_run(root, run) {
                save_error.get_or_insert(e);
            }
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
                let account = run_account.or(script.account.as_deref());
                // Where the case starts, then the records it relies on -
                // both before its browser opens, so a case that cannot
                // start never signs in.
                let mut notice = None;
                let ready = match nav::route_for(&nav_file, script.area.as_deref(), case.module.as_deref(), account) {
                    Err(why) => Err(why),
                    Ok(path) => {
                        let checked =
                            preconditions::check_case(precondition_db, root, organization, project, &script.preconditions)
                                .await;
                        notice = checked.notice;
                        match checked.blocked {
                            Some(why) => Err(why),
                            // Then its fixtures: the setup's approval, the
                            // shared drafts' values and the setup's own run,
                            // in a browser of its own that is closed, and its
                            // lease let go, before the case's browser opens.
                            None => setup::prepare_case(browsers, root, organization, project, &script, timing, cancel)
                                .await
                                .map(|prepared| (path, prepared.script)),
                        }
                    }
                };
                let mut record = match ready {
                    Err(why) => blocked_before_start(&script, account, why),
                    // The case runs from the copy with its fixture values in;
                    // the saved script is never changed.
                    Ok((path, ready)) => {
                        // A path but no recipe: the sign-in fails first and
                        // says what to add, so no route is needed.
                        let route = path.zip(sign_in_recipe.as_ref()).map(|(p, r)| Route::new(r, p.clone()));
                        let at = Place { run_id: &run_id, index, total, case_id, title, count };
                        let go = Go { root, organization, project, script: &ready, account, route: route.as_ref(), timing, cancel };
                        let first = one_go(browsers, &go, &at, progress)
                            .await
                            .unwrap_or_else(|why| unrun(case_id, title, "Blocked", format!("the browser did not open: {why}")));
                        // One more go, from sign-in in a fresh browser, for a
                        // failure that looked transient - once, never again,
                        // and never after a stop. It gets a fresh draft: the
                        // setup runs again first (the first go may have
                        // changed its draft), and a setup that cannot run
                        // keeps the first go's record.
                        let looked_transient = if retry_transient { {
                            let components = super::components::load_components(root, organization, project).unwrap_or_default();
                            transient::is_transient_with(&first, Some(&ready), &components)
                        } } else { None };
                        match looked_transient {
                            Some(why) if !cancel.load(Ordering::SeqCst) => {
                                match setup::prepare_case(browsers, root, organization, project, &script, timing, cancel).await {
                                    Err(not) => transient::retry_not_started(first, &not),
                                    Ok(again) => {
                                        let go = Go { script: &again.script, ..go };
                                        match one_go(browsers, &go, &at, progress).await {
                                            Ok(second) => transient::after_retry(why, first.duration_ms, second),
                                            // No second go to keep: the first go's
                                            // steps and evidence stay the record.
                                            Err(open) => transient::retry_not_started(
                                                first,
                                                &format!("the browser did not open: {open}"),
                                            ),
                                        }
                                    }
                                }
                            }
                            _ => first,
                        }
                    }
                };
                // Said on the record whichever way the case went, so the
                // review and the report show the checks were skipped.
                record.notice = notice;
                record
            }
        };
        let mut record = record;
        // A marked step that passed this time: the mark goes, and the
        // case's own record says so. Bookkeeping - it never fails the run.
        if let Some(step_number) = super::defects::clear_if_passed(root, &record) {
            record.reason = super::defects::append_cleared(&record.reason, step_number);
        }
        let proposed = record.proposed.clone();
        run.cases.push(record);
        if let Err(e) = store::save_run(root, run) {
            save_error.get_or_insert(e);
        }
        progress(tell(&run_id, index, total, case_id, title, "done", 0, count, &proposed));
    }

    // Old pictures go once, now every case is saved - after a Stop or a
    // failed case too, since the loop above ends the same way for all.
    store::prune_old_shots(root);

    // What the run says about the project's quirks: a note filed with a
    // repair is confirmed by its steps passing, or doubted by them failing
    // the same way again. Bookkeeping on text an assistant reads - it never
    // changes a script, and a quirks file it cannot write never fails the
    // run (it is logged).
    super::quirks::record_run_evidence(root, organization, project, &run.cases[first..], super::sessions::now_ms());

    // Every case ran by now: only the save failed, and the words say so.
    save_error.map_or(Ok(()), |e| Err(format!("the run finished but could not be saved: {e}")))
}
