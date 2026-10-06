//! A replay to a step: the supervised browser carried through a case's
//! saved steps 1 to N-1 and stopped before step N, so a person (or an
//! assistant) can heal step N where it failed.
//!
//! In the design's order: the checks before anything opens, the case's
//! preconditions, its no-save guard, its sign-in through the supervised
//! browser's lease, the trip to its area the unattended run makes, then the
//! steps. Only the saved script's own steps run, never an edit. Nothing here
//! opens the browser or calls Azure DevOps: the command hands it the open
//! supervised session.

use super::lease::Held;
use super::nav::{Route, TripFrom};
use super::preconditions::{self, NoDb, PreconditionDb};
use super::runner::{self, AreaRoute, InRun, AFTER_STOP};
use super::replay::Browsers;
use super::setup::{self, NoBrowsers};
use super::{replay, signin, store, CaseScript};
use crate::api_templates::gate::StageDb;
use crate::browser::actions::ActionOutcome;
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::timing::Timing;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Said when a replay is asked for while one is already going.
pub const ALREADY_RUNNING: &str = "a replay is already running - wait for it to finish";

/// What a replay is asked to do: carry case `case_id` to the page before
/// `step` runs. `db_read_access` is the AI Bridge tab's Database Read Access
/// switch, as for a supervised case's preconditions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRequest {
    pub case_id: i32,
    pub step: i32,
    pub db_read_access: bool,
}

/// Where a replay that stopped on a failure was: signing the case in,
/// going to its area, or running one of its steps. The sign-in and the
/// trip are not step 1: a record keeps their outcomes under the sign-in
/// step and the module step, as an unattended run's does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReplayPhase {
    SignIn,
    Area,
    Step,
}

/// How a replay ended. `sentence` says it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ReplayEnd {
    /// Every step before `step` ran: the browser is on the page before it.
    /// `notice` is said beside it: the preconditions were not checked.
    Ready { case_id: i32, step: i32, notice: Option<String> },
    /// The replay failed in `phase`: step `step` when it is `Step`; the
    /// case's sign-in or its trip to the area before step 1 otherwise (then
    /// `step` is 1, the step it never reached). `why` is the failure's
    /// sentence, and `outcomes` what ran, a failure carrying its screenshot.
    StoppedAt { phase: ReplayPhase, step: i32, why: String, outcomes: Vec<ActionOutcome> },
    /// A precondition of the case is not met, or its fixtures could not
    /// give their values (`setup::prepare_case`): nothing was signed in,
    /// and the case is Blocked with this sentence as its reason, as a
    /// watched start blocks it.
    Blocked(String),
    /// The stop control, or the browser closing, ended the replay before
    /// step `step` finished.
    Stopped { step: i32 },
    /// Nothing was replayed: the sentence says why.
    Refused(String),
}

impl ReplayEnd {
    /// What the pane, and the assistant, are told.
    pub fn sentence(&self) -> String {
        match self {
            ReplayEnd::Ready { case_id, step, .. } => {
                format!("replayed case {case_id} to step {step} - the browser is on the page before step {step} runs")
            }
            ReplayEnd::StoppedAt { phase: ReplayPhase::SignIn, why, .. } => {
                format!("replay stopped while signing in: {why}")
            }
            ReplayEnd::StoppedAt { phase: ReplayPhase::Area, why, .. } => {
                format!("replay stopped while going to the case's area: {why}")
            }
            ReplayEnd::StoppedAt { phase: ReplayPhase::Step, step, why, .. } => {
                format!("replay stopped at step {step}: {why}")
            }
            ReplayEnd::Blocked(why) => why.clone(),
            ReplayEnd::Stopped { step } => format!("the replay was stopped at step {step}"),
            ReplayEnd::Refused(why) => why.clone(),
        }
    }
}

/// What the panes hear once a replay has opened the supervised browser:
/// nothing when one was open already (`had_browser`).
pub fn opened_event(had_browser: bool) -> Option<crate::events::AutorunSessionChanged> {
    (!had_browser).then_some(crate::events::AutorunSessionChanged { opened: true, account: None })
}

/// What the panes hear once a replay has signed the browser in: the
/// account key it holds now, when that is not the one it held `before`.
pub fn signed_in_event(
    before: &Option<String>,
    after: &Option<String>,
) -> Option<crate::events::AutorunSessionChanged> {
    (after.is_some() && after != before)
        .then(|| crate::events::AutorunSessionChanged { opened: true, account: after.clone() })
}

/// Why a replay stopped when its page tried to save while the browser was
/// guarding case `other`'s draft (an assistant's replay never lifts that
/// guard).
pub fn guarding_other_case(other: i32) -> String {
    format!("the Auto Run browser is guarding case {other}'s draft, and the page tried to save")
}

/// A replay's end with the sentence that says it, as the person's command
/// answers: the pane shows `sentence` as it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ReplayAnswer {
    pub end: ReplayEnd,
    pub sentence: String,
}

impl From<ReplayEnd> for ReplayAnswer {
    fn from(end: ReplayEnd) -> Self {
        let sentence = end.sentence();
        ReplayAnswer { end, sentence }
    }
}

/// A stop asked for before the browser is opened for a replay - a Close
/// that won the session's lock first - ends it there: nothing opens.
pub fn stopped_before_opening(cancel: &AtomicBool) -> Option<ReplayEnd> {
    cancel.load(Ordering::SeqCst).then_some(ReplayEnd::Stopped { step: 1 })
}

/// Whether a replay is going. Only `OneReplay` flips it.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// The replay's stop control: set by `stop`, and by anything that closes
/// the supervised browser. Read before each part of the replay and by the
/// step's own waits, never under the session's lock, so a stop is heard
/// while the replay holds the browser.
pub static CANCEL: AtomicBool = AtomicBool::new(false);

/// Ask the replay that is going, if any, to stop.
pub fn stop() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// The one replay going. Taking it clears an earlier stop; dropping it - an
/// end, an error or a panic - frees the slot.
pub struct OneReplay(());

impl OneReplay {
    pub fn claim() -> Result<OneReplay, String> {
        RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| ALREADY_RUNNING.to_string())?;
        CANCEL.store(false, Ordering::SeqCst);
        Ok(OneReplay(()))
    }
}

impl Drop for OneReplay {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Whether a replay to a step is going right now.
pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// What the checks before anything opens found: the case's script and the
/// route to its area.
pub struct Checked {
    pub script: CaseScript,
    pub route: Route,
}

/// The checks before anything opens: the case has a saved script, `step`
/// is from 1 to its last step plus 1 (every step), and its area can be
/// reached the way a watched run's `return_to_area` reaches it
/// (`runner::area_route`). `Err` is the refusal's sentence.
pub fn check(root: &Path, organization: &str, project: &str, req: &ReplayRequest) -> Result<Checked, String> {
    let id = req.case_id;
    let script = store::load_script(root, id)?.ok_or_else(|| format!("case {id} has no saved script"))?;
    let last = script.steps.iter().map(|s| s.step_number).max().unwrap_or(0);
    if req.step < 1 || req.step > last + 1 {
        return Err(format!("step {} is not in case {id}'s script (it has steps 1 to {last})", req.step));
    }
    let route = runner::area_route(root, organization, project, id)?;
    Ok(Checked { script, route })
}

/// How the application log names a replay.
fn who(case_id: i32) -> String {
    format!("Auto Run replay, case {case_id}")
}

/// Whether this outcome is the browser having gone away.
fn closed(o: &ActionOutcome) -> bool {
    o.harness && o.detail.contains(&CdpError::Closed.to_string())
}

/// `replay_to_checked` as the person's own replay (it may lift an earlier
/// case's guard), with the default timing, no earlier guard to carry, no
/// database to ask and no browser for a setup: a case with preconditions
/// is refused with `preconditions::NEED_DB` while Database Read Access is
/// on, and carries the notice while it is off; an approved setup fails to
/// open its browser (`setup::NoBrowsers`).
#[allow(clippy::too_many_arguments)]
pub async fn replay_to<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    req: &ReplayRequest,
    account: &mut Option<String>,
    lease: &mut Held,
    cancel: &AtomicBool,
    progress: impl FnMut(i32, i32),
) -> ReplayEnd {
    let mut guarded_case = None;
    let db_read_access = req.db_read_access;
    let db = move || -> PreconditionDb<NoDb> {
        if db_read_access {
            PreconditionDb::Missing(preconditions::NEED_DB.to_string())
        } else {
            PreconditionDb::ReadingOff
        }
    };
    replay_to_checked(
        d,
        &mut NoBrowsers::<D>::default(),
        root,
        organization,
        project,
        req,
        account,
        lease,
        &mut guarded_case,
        true,
        &Timing::default(),
        cancel,
        db,
        progress,
    )
    .await
}

/// Replay case `req.case_id` in the supervised browser `d` up to the page
/// before step `req.step`. `account`, `lease` and `guarded_case` are the
/// supervised session's own, as `auto_run_step` uses them. `may_lift` is
/// `guard_for_case`'s: true for the person's replay; false for an
/// assistant's, which may switch the guard on for a no-save case but never
/// lifts one held for another case, as a try never does. `db` gives the
/// place preconditions are asked (`preconditions::for_run`), looked at only
/// when the script has some. `setup_browsers` gives the browser a case's
/// setup runs in (`setup::prepare_case`), after the preconditions and
/// before anything is signed in; what the setup gave is kept for the steps
/// the person runs after the replay (`setup::remember`). `progress(k, total)` is called before each
/// step runs, `total` being `req.step - 1`. `cancel` is the stop control.
#[allow(clippy::too_many_arguments)]
pub async fn replay_to_checked<D: Driver, P: StageDb, B: Browsers>(
    d: &mut D,
    setup_browsers: &mut B,
    root: &Path,
    organization: &str,
    project: &str,
    req: &ReplayRequest,
    account: &mut Option<String>,
    lease: &mut Held,
    guarded_case: &mut Option<i32>,
    may_lift: bool,
    timing: &Timing,
    cancel: &AtomicBool,
    db: impl FnOnce() -> PreconditionDb<P>,
    mut progress: impl FnMut(i32, i32),
) -> ReplayEnd {
    let Checked { script, route } = match check(root, organization, project, req) {
        Ok(c) => c,
        Err(why) => return ReplayEnd::Refused(why),
    };
    let (id, n) = (req.case_id, req.step);
    let stopped = || cancel.load(Ordering::SeqCst);
    if stopped() {
        return ReplayEnd::Stopped { step: 1 };
    }
    // The case starts afresh: no tab an earlier case or try opened carries
    // over, and the tabs steps 1 to N-1 open are opened again by running
    // them.
    d.close_other_tabs().await;

    // What an earlier start's setup gave is let go: a replay that ends
    // before its own setup has run leaves nothing stale for the steps.
    setup::forget(id);

    // Preconditions, exactly as a supervised case starts: one not met
    // stops the replay before anything is signed in.
    let checked = match preconditions::check_script(root, organization, project, id, db).await {
        Ok(c) => c,
        Err(why) => return ReplayEnd::Refused(why),
    };
    if let Some(why) = checked.blocked {
        return ReplayEnd::Blocked(why);
    }

    // The case's fixtures, as an unattended case's: the setup needs its
    // approval, and its run makes the case's own draft in a browser of its
    // own, closed before this one signs in. The steps below run from the
    // copy with the values in; the saved script is never changed.
    if stopped() {
        return ReplayEnd::Stopped { step: 1 };
    }
    // A setup that signs in as the account this browser holds: once the
    // setup will run, this browser's session is ended and its lease let
    // go first (`setup::make_way`). Close stops the setup between its
    // template steps.
    let (way_d, way_account, way_lease) = (&mut *d, &mut *account, &mut *lease);
    let make_way = |key: String| async move { setup::make_way(way_d, way_account, way_lease, &key, timing).await };
    let prepared =
        setup::prepare_case_hooked(setup_browsers, root, organization, project, &script, timing, cancel, make_way).await;
    let script = match prepared {
        Ok(prepared) => {
            setup::remember(id, prepared.setup_outputs);
            prepared.script
        }
        Err(why) => return ReplayEnd::Blocked(why),
    };
    if stopped() {
        return ReplayEnd::Stopped { step: 1 };
    }

    // The no-save guard, through the same path as a person's step.
    if let Err(why) =
        crate::commands::autorun::guard_for_case(d, guarded_case, root, organization, project, id, may_lift).await
    {
        return ReplayEnd::Refused(why);
    }
    // A guard still held for another case: an assistant's replay never
    // lifts it, so a save this case's page tries is stopped for that case.
    let guarding_other = guarded_case.filter(|c| *c != id);

    // The case's own account, through the supervised browser's lease.
    let mut signed_now = false;
    if let Some(key) = script.account.as_deref() {
        if stopped() {
            return ReplayEnd::Stopped { step: 1 };
        }
        let out = match signin::sign_in_leased(d, root, organization, project, key, lease, account, timing).await {
            Ok(out) => out,
            Err(why) => return ReplayEnd::Refused(why),
        };
        let mut one = runner::as_action_outcome(&out);
        if closed(&one) {
            return ReplayEnd::Stopped { step: 1 };
        }
        if !one.ok {
            if !one.harness {
                one.screenshot = runner::picture(d, root).await;
            }
            return ReplayEnd::StoppedAt { phase: ReplayPhase::SignIn, step: 1, why: one.detail.clone(), outcomes: vec![one] };
        }
        signed_now = true;
    }

    // The trip to the case's area, the one the unattended run makes.
    if stopped() {
        return ReplayEnd::Stopped { step: 1 };
    }
    let from = if signed_now { TripFrom::SignIn } else { TripFrom::Elsewhere };
    let went = replay::trip_to_module(d, root, &route, from, timing, &who(id)).await;
    if closed(&went) {
        return ReplayEnd::Stopped { step: 1 };
    }
    if !went.ok {
        return ReplayEnd::StoppedAt { phase: ReplayPhase::Area, step: 1, why: went.detail.clone(), outcomes: vec![went] };
    }

    // Steps 1 to N-1, in the script's order. The other areas their
    // `return_to_area` actions name are read once, before the first.
    let names = runner::named_areas(script.steps.iter().filter(|s| s.step_number < n).flat_map(|s| s.actions.iter()));
    let areas = runner::area_routes(root, organization, project, &names);
    for step in script.steps.iter().filter(|s| s.step_number < n) {
        let k = step.step_number;
        if stopped() {
            return ReplayEnd::Stopped { step: k };
        }
        progress(k, n - 1);
        let mut in_run = InRun {
            cancel: Some(cancel),
            areas: Some(&areas),
            fail_on_unexpected_dialog: script.fail_on_unexpected_dialog,
            ..Default::default()
        };
        let ran = runner::run_step_in_run(
            d,
            root,
            organization,
            project,
            step,
            timing,
            account,
            lease,
            Some(&route),
            AreaRoute::To(&route),
            &mut in_run,
        )
        .await;
        let mut outcomes = match ran {
            Ok(o) => o,
            Err(why) => return ReplayEnd::StoppedAt { phase: ReplayPhase::Step, step: k, why, outcomes: Vec::new() },
        };
        if outcomes.iter().any(|o| o.detail == AFTER_STOP || closed(o)) {
            return ReplayEnd::Stopped { step: k };
        }
        // A save the page sent after the step's last action passed is
        // still this step's.
        if outcomes.iter().all(|o| o.ok) {
            if let Some(sentence) = d.take_save_blocked() {
                if let Some(last) = outcomes.last_mut() {
                    *last = ActionOutcome::failed(sentence);
                    last.screenshot = runner::picture(d, root).await;
                }
            }
        }
        if let Some(mut why) = outcomes.iter().find(|o| !o.ok).map(|o| o.detail.clone()) {
            if let Some(other) = guarding_other.filter(|_| crate::browser::save_guard::is_blocked(&why)) {
                why = guarding_other_case(other);
            }
            return ReplayEnd::StoppedAt { phase: ReplayPhase::Step, step: k, why, outcomes };
        }
    }
    ReplayEnd::Ready { case_id: id, step: n, notice: checked.notice }
}
