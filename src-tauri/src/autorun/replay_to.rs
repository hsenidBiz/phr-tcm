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
    /// `whole` is how many steps the case has, when `step` is past its last:
    /// the whole case ran, and the browser is on the page its last step left.
    /// `cleanup`, beside a `whole`, is the number of the script's clean-up
    /// step (`None` without one): it ran too when `step` is past it.
    Ready {
        case_id: i32,
        step: i32,
        notice: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        whole: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cleanup: Option<i32>,
    },
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
    /// The stop control (Stop, Close browser, Open browser replacing it,
    /// Release Auto Run browser or the app exiting) ended the replay before
    /// step `step` finished.
    Stopped { step: i32 },
    /// The browser closed under the replay before step `step` finished: its
    /// window was closed, or it crashed or stopped answering. The replay
    /// lets it go (`commands::autorun::replay_supervised`), so the next
    /// replay or discovery opens a new one. A replay that had stopped on a
    /// failure first (`StoppedAt`) keeps that failure's `why` and
    /// `outcomes` here, so the pane still shows the failed step's rows.
    BrowserGone { step: i32, why: Option<String>, outcomes: Vec<ActionOutcome> },
    /// Nothing was replayed: the sentence says why.
    Refused(String),
}

impl ReplayEnd {
    /// What the pane, and the assistant, are told.
    pub fn sentence(&self) -> String {
        self.sentence_in(None)
    }

    /// `sentence`, for a case whose clean-up is step `cleanup`: a replay
    /// that ended there says "the clean-up", as the pane, the report and
    /// the hand-over name it, never a step number the case does not have.
    pub fn sentence_in(&self, cleanup: Option<i32>) -> String {
        let at = |k: &i32| step_said(*k, cleanup);
        match self {
            ReplayEnd::Ready { case_id, step, whole: Some(all), cleanup, .. } => {
                let steps = if *all == 1 { "its 1 step".to_string() } else { format!("all {all} steps") };
                match cleanup {
                    Some(c) if step > c => format!(
                        "replayed the whole of case {case_id} ({steps} and its clean-up) - the browser is on the page its clean-up left"
                    ),
                    Some(c) => format!(
                        "replayed the whole of case {case_id} ({steps}; its clean-up did not run - replay to step {} to run it too) - the browser is on the page its last step left",
                        c + 1
                    ),
                    None => format!("replayed the whole of case {case_id} ({steps}) - the browser is on the page its last step left"),
                }
            }
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
                format!("replay stopped at {}: {why}", at(step))
            }
            ReplayEnd::Blocked(why) => why.clone(),
            ReplayEnd::Stopped { step } => format!("the replay was stopped at {}: {ASKED_TO_STOP}", at(step)),
            ReplayEnd::BrowserGone { step, why: None, .. } => format!("replay stopped at {}: {BROWSER_CLOSED}", at(step)),
            ReplayEnd::BrowserGone { step, why: Some(why), .. } => {
                format!("replay stopped at {}: {why}; then {BROWSER_CLOSED}", at(step))
            }
            ReplayEnd::Refused(why) => why.clone(),
        }
    }
}

/// Step `k` as a replay's sentences say it: "the clean-up" when `k` is the
/// case's clean-up step (`cleanup`), else "step k".
pub fn step_said(k: i32, cleanup: Option<i32>) -> String {
    match cleanup {
        Some(c) if c == k => "the clean-up".to_string(),
        _ => format!("step {k}"),
    }
}

/// Why a replay the stop control ended stopped.
pub const ASKED_TO_STOP: &str = "it was asked to stop (Stop, Close browser or Release Auto Run browser)";

/// Why a replay whose browser closed under it stopped.
pub const BROWSER_CLOSED: &str = "the Auto Run browser closed before the step finished (its window was closed, or it stopped answering), so the app let it go - replay again and a new browser opens";

/// What the panes hear once a replay has opened the supervised browser:
/// nothing when one was open already (`had_browser`).
pub fn opened_event(had_browser: bool) -> Option<crate::events::AutorunSessionChanged> {
    (!had_browser).then_some(crate::events::AutorunSessionChanged { opened: true, account: None })
}

/// What the panes hear once a replay (or a try) has signed the browser in:
/// the account key it holds now, when that is not the one it held `before`.
/// A sign-in a step's `expire_session` ended is NOT news: the steps after it
/// expect no session, and a pane told so would sign in before them.
pub fn signed_in_event(
    before: &Option<String>,
    after: &Option<String>,
) -> Option<crate::events::AutorunSessionChanged> {
    (after.is_some() && after != before)
        .then(|| crate::events::AutorunSessionChanged { opened: true, account: after.clone() })
}

/// What the panes hear once the assistant's reset found the browser's
/// sign-in gone and forgot it: the browser is open with nobody signed in,
/// so a pane signs in again before its next step. Only the reset says this.
pub fn sign_in_forgotten_event(
    before: &Option<String>,
    after: &Option<String>,
) -> Option<crate::events::AutorunSessionChanged> {
    (before.is_some() && after.is_none()).then_some(crate::events::AutorunSessionChanged { opened: true, account: None })
}

/// Why a replay stopped when its page tried to save while the browser was
/// guarding case `other`'s draft with a guard the replay may not lift (the
/// person's, for the assistant's replay: `commands::autorun::ReplayBy::may_lift`).
pub fn guarding_other_case(other: i32) -> String {
    format!("the Auto Run browser is guarding case {other}'s draft, and the page tried to save")
}

/// The supervised browser a replay leaves open: the account KEY it holds,
/// if any, and whether the assistant opened it (`commands::autorun::OpenedBy`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOpen {
    pub account: Option<String>,
    pub by_assistant: bool,
}

/// The line an assistant's replay answer gains when the browser stays
/// open: what that blocks, and how it is freed.
pub fn left_open_line(left: &LeftOpen) -> String {
    if left.by_assistant {
        // A discovery taking it over keeps its sign-in, and so the account's
        // lease: template runs as that account wait for the release.
        let (signed, templates) = match &left.account {
            Some(key) => (
                ", signed in",
                format!(", and template runs as {key} until release_autorun_browser frees it (a discovery keeps its sign-in)"),
            ),
            None => ("", String::new()),
        };
        return format!(
            "The Auto Run browser stays open on this step{signed}: discovery actions and component saves are refused until start_autorun_discovery takes it over{templates}. Do not release it between replays; release_autorun_browser is for a broken browser."
        );
    }
    let blocked = match &left.account {
        Some(key) => format!("discovery actions, component saves and template runs as {key}"),
        None => "discovery actions and component saves".to_string(),
    };
    format!("The person's Auto Run browser stays open on this step: {blocked} are refused until the person closes it.")
}

/// A step the replay ran outside `main`: the tab it ran in, as a live
/// step's `tab` says it (`InRun::tab`). A step that stayed in `main` has
/// none.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct ReplayedTab {
    pub step: i32,
    pub tab: String,
}

/// A replay's end with the sentence that says it, as the person's command
/// answers: the pane shows `sentence` as it is, and `tabs` beside the
/// steps the replay ran outside `main`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ReplayAnswer {
    pub end: ReplayEnd,
    pub sentence: String,
    pub tabs: Vec<ReplayedTab>,
}

impl ReplayAnswer {
    /// The answer for `end`, with the tabs its steps ran in, for a case
    /// whose clean-up is step `cleanup` (`ReplayEnd::sentence_in`).
    pub fn with_tabs(end: ReplayEnd, tabs: Vec<ReplayedTab>, cleanup: Option<i32>) -> Self {
        let sentence = end.sentence_in(cleanup);
        ReplayAnswer { end, sentence, tabs }
    }
}

impl From<ReplayEnd> for ReplayAnswer {
    fn from(end: ReplayEnd) -> Self {
        ReplayAnswer::with_tabs(end, Vec::new(), None)
    }
}

/// A stop asked for before the browser is opened for a replay - a Close
/// that won the session's lock first - ends it there: nothing opens.
pub fn stopped_before_opening(cancel: &AtomicBool) -> Option<ReplayEnd> {
    cancel.load(Ordering::SeqCst).then_some(ReplayEnd::Stopped { step: 1 })
}

/// Whether a replay is going, and whose: `NOBODY`, `PERSON` or
/// `ASSISTANT`. Only `OneReplay` sets it.
static RUNNING: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(NOBODY);
const NOBODY: u8 = 0;
const PERSON: u8 = 1;
const ASSISTANT: u8 = 2;

/// The replay's stop control: set by `stop`, and by anything that closes
/// the supervised browser. Read before each part of the replay and by the
/// step's own waits, never under the session's lock, so a stop is heard
/// while the replay holds the browser.
pub static CANCEL: AtomicBool = AtomicBool::new(false);

/// Ask the replay that is going, if any, to stop.
pub fn stop() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Whether the replay going was asked to stop by the assistant's
/// `stop_autorun_replay` (`stop_on_request`), not by the person's Stop,
/// Close or Release. Cleared by a claim, and when the assistant starts a
/// replay (`forget_stop_request`).
static ON_REQUEST: AtomicBool = AtomicBool::new(false);

/// The assistant's `stop_autorun_replay`: the same stop as `stop`, noted
/// as asked for, so the replay's answer says it was stopped on request.
pub fn stop_on_request() {
    ON_REQUEST.store(true, Ordering::SeqCst);
    stop();
}

/// Whether the replay that ended last was asked to stop by the assistant.
pub fn stop_was_requested() -> bool {
    ON_REQUEST.load(Ordering::SeqCst)
}

/// A new replay of the assistant's starts with no stop asked of it.
pub fn forget_stop_request() {
    ON_REQUEST.store(false, Ordering::SeqCst);
}

/// What the assistant's replay that `stop_autorun_replay` stopped answers:
/// where the stop was heard, from the step it was on (`at`, 0 before step
/// 1) and the last action of that step to begin (`action`, from 1; 0 when
/// none had): "at" that action when the stop ended it inside its own wait
/// (`inside`), "after" it when it had finished. Said alone, with no page.
/// The case's clean-up (`cleanup`, its step number) is said as such.
pub fn stopped_on_request(case_id: i32, at: i32, action: usize, inside: bool, cleanup: Option<i32>) -> String {
    let front = format!("the replay of case {case_id} was stopped on request");
    let step = step_said(at, cleanup);
    match (at, action) {
        (at, 0) if at > 0 => format!("{front} before {step}'s first action"),
        (at, n) if at > 0 && inside => format!("{front} at action {n} of {step}, inside its wait"),
        (at, n) if at > 0 => format!("{front} after action {n} of {step}"),
        _ => format!(
            "{front} before step 1, while the case was made ready (its preconditions, sign-in or trip to its area)"
        ),
    }
}

/// The one replay going. Taking it clears an earlier stop; dropping it - an
/// end, an error or a panic - frees the slot.
pub struct OneReplay(());

impl OneReplay {
    /// The person's replay (Replay to step in the pane) takes the slot.
    pub fn claim() -> Result<OneReplay, String> {
        Self::claim_as(PERSON)
    }

    /// The assistant's replay (`replay_autorun_to_step`) takes the slot:
    /// the only replay `stop_autorun_replay` may stop
    /// (`assistants_replay_running`).
    pub fn claim_for_assistant() -> Result<OneReplay, String> {
        Self::claim_as(ASSISTANT)
    }

    fn claim_as(owner: u8) -> Result<OneReplay, String> {
        RUNNING
            .compare_exchange(NOBODY, owner, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| ALREADY_RUNNING.to_string())?;
        CANCEL.store(false, Ordering::SeqCst);
        ON_REQUEST.store(false, Ordering::SeqCst);
        PROGRESS.store(0, Ordering::SeqCst);
        CLEANUP_AT.store(0, Ordering::SeqCst);
        ACTIONS.reset();
        Ok(OneReplay(()))
    }
}

impl Drop for OneReplay {
    fn drop(&mut self) {
        RUNNING.store(NOBODY, Ordering::SeqCst);
    }
}

/// Where the replay going is: the step it is running and how many steps it
/// runs in all, packed as `(step << 32) | of`. Zero before step 1 (signing
/// in, going to the area). Set by `note_progress`, cleared by a claim.
static PROGRESS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The replay going is about to run step `step` of `of`: kept for whoever
/// asks how far it has got (`progress`).
pub fn note_progress(step: i32, of: i32) {
    let packed = (u64::from(step.max(0) as u32) << 32) | u64::from(of.max(0) as u32);
    PROGRESS.store(packed, Ordering::SeqCst);
    ACTIONS.reset();
}

/// The step number of the clean-up of the case the replay going (or ended
/// last) runs, 0 when it has none: noted as its steps start
/// (`run_steps`), cleared by a claim, so its sentences say "the clean-up".
static CLEANUP_AT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// The case the replay runs has its clean-up at step `step` (`None`: none).
pub fn note_cleanup(step: Option<i32>) {
    CLEANUP_AT.store(step.unwrap_or(0).max(0), Ordering::SeqCst);
}

/// The clean-up step of the case the replay going (or ended last) runs.
pub fn cleanup_at() -> Option<i32> {
    let k = CLEANUP_AT.load(Ordering::SeqCst);
    (k > 0).then_some(k)
}

/// The action the replay going is on, within its step (`runner::StepProgress`).
pub static ACTIONS: runner::StepProgress = runner::StepProgress::new();

/// The action the replay going is on, and how many of its step's actions
/// have failed: none before its first step's first action.
pub fn action_now() -> runner::ActionNow {
    ACTIONS.now()
}

/// The step the replay going is running and how many it runs, `(0, 0)`
/// before its first step.
pub fn progress() -> (i32, i32) {
    let packed = PROGRESS.load(Ordering::SeqCst);
    ((packed >> 32) as i32, (packed & 0xFFFF_FFFF) as i32)
}

/// Whether a replay to a step is going right now.
pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst) != NOBODY
}

/// Whether the replay going right now is the assistant's own
/// (`OneReplay::claim_for_assistant`). False while nothing runs - before
/// its replay has claimed the slot, or once it has ended and its page is
/// still being read - and while the person's replay runs.
pub fn assistants_replay_running() -> bool {
    RUNNING.load(Ordering::SeqCst) == ASSISTANT
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
/// on, and carries the notice while it is off; a setup fails to open its
/// browser (`setup::NoBrowsers`).
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
        &Timing::supervised(),
        cancel,
        db,
        progress,
    )
    .await
}

/// Replay case `req.case_id` in the supervised browser `d` up to the page
/// before step `req.step`. `account`, `lease` and `guarded_case` are the
/// supervised session's own, as `auto_run_step` uses them. `may_lift` is
/// `guard_for_case`'s, from who asked and whose guard is on
/// (`commands::autorun::ReplayBy::may_lift`): true for the person's replay,
/// and for the assistant's over a guard a call of its own left; false over
/// the person's guard or a no_save discovery's, which the replay keeps
/// while it may still switch the guard on for a no-save case. `db` gives the
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
    progress: impl FnMut(i32, i32),
) -> ReplayEnd {
    let mut tabs = Vec::new();
    replay_to_traced(
        d,
        setup_browsers,
        root,
        organization,
        project,
        req,
        account,
        lease,
        guarded_case,
        may_lift,
        timing,
        cancel,
        db,
        progress,
        &mut tabs,
        &mut Reuse::default(),
    )
    .await
}

/// `replay_to_checked`, also learning back the tab each step it ran was
/// in when that was not `main` (`tabs`, in step order) - the failing step's
/// too - so the pane says `in tab <name>` beside them as it does for a
/// step the person ran; and, in `reuse`, what it made of a browser already
/// signed in as the case's account, for its answer to say (`Reuse::said`).
///
/// That browser - signed in by the app as the script's account, holding
/// its lease in this environment, still holding the HttpOnly cookies that
/// sign-in left (`Held::same_sign_in`), its page not refused and the recipe's
/// signed-in marker showing (`signin::already_signed_in`) - keeps its
/// sign-in and goes to the area from where it is (`TripFrom::Elsewhere`).
/// If that trip fails, the case is signed in as it always is and goes
/// again, once; a save the guard stopped on that trip is kept for the
/// answer. Any other browser signs in first.
#[allow(clippy::too_many_arguments)]
pub async fn replay_to_traced<D: Driver, P: StageDb, B: Browsers>(
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
    tabs: &mut Vec<ReplayedTab>,
    reuse: &mut Reuse,
) -> ReplayEnd {
    *reuse = Reuse::default();
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

    // The case's fixtures, as an unattended case's: the setup's run makes
    // the case's own draft in a browser of its own, closed before this one
    // signs in. The steps below run from the
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
    // A guard still held for another case: one this replay may not lift
    // (the person's), so a save this case's page tries is stopped for that
    // case.
    let guarding_other = guarded_case.filter(|c| *c != id);

    // A browser already signed in as the case's account keeps its sign-in
    // and goes to the area from where it is. A trip that fails then signs in
    // below, as any other browser does, and goes again.
    if let Some(key) = script.account.as_deref() {
        if stopped() {
            return ReplayEnd::Stopped { step: 1 };
        }
        if signed_in_here(d, root, organization, project, key, account, lease, timing).await {
            let went = replay::trip_to_module(d, root, &route, TripFrom::Elsewhere, timing, &who(id)).await;
            if closed(&went) {
                return ReplayEnd::BrowserGone { step: 1, why: None, outcomes: Vec::new() };
            }
            let area = route.path.name();
            if went.ok {
                crate::applog::info(format!("{}: signed in already as {key}, went to {area}", who(id)));
                reuse.went = Some(area.to_string());
            } else {
                crate::applog::info(format!(
                    "{}: signed in already as {key}, but the trip to {area} failed - signing in again",
                    who(id)
                ));
                if crate::browser::save_guard::is_blocked(&went.detail) {
                    reuse.save_stopped = Some(save_stopped_line(area, &went.detail));
                }
            }
        }
    }

    // The case's own account, through the supervised browser's lease.
    let mut signed_now = false;
    if let Some(key) = script.account.as_deref().filter(|_| reuse.went.is_none()) {
        if stopped() {
            return ReplayEnd::Stopped { step: 1 };
        }
        let out = match signin::sign_in_leased(d, root, organization, project, key, lease, account, timing).await {
            Ok(out) => out,
            Err(why) => return ReplayEnd::Refused(why),
        };
        let mut one = runner::as_action_outcome(&out);
        if closed(&one) {
            return ReplayEnd::BrowserGone { step: 1, why: None, outcomes: Vec::new() };
        }
        if !one.ok {
            if !one.harness {
                one.screenshot = runner::picture(d, root).await;
            }
            return ReplayEnd::StoppedAt { phase: ReplayPhase::SignIn, step: 1, why: one.detail.clone(), outcomes: vec![one] };
        }
        signed_now = true;
    }

    // The trip to the case's area, the one the unattended run makes - made
    // already by a browser that was signed in.
    if stopped() {
        return ReplayEnd::Stopped { step: 1 };
    }
    if reuse.went.is_none() {
        let from = if signed_now { TripFrom::SignIn } else { TripFrom::Elsewhere };
        let went = replay::trip_to_module(d, root, &route, from, timing, &who(id)).await;
        if closed(&went) {
            return ReplayEnd::BrowserGone { step: 1, why: None, outcomes: Vec::new() };
        }
        if !went.ok {
            return ReplayEnd::StoppedAt { phase: ReplayPhase::Area, step: 1, why: went.detail.clone(), outcomes: vec![went] };
        }
    }

    // Steps 1 to N-1, in the script's order.
    if let Some(stopped) =
        run_steps(d, root, organization, project, &script, &route, 1, n, account, lease, timing, cancel, &mut progress, tabs, guarding_other)
            .await
    {
        return stopped;
    }
    ReplayEnd::Ready { case_id: id, step: n, notice: checked.notice, whole: whole_case(&script, n), cleanup: cleanup_ran(&script, n) }
}

/// Is the supervised browser `d` already signed in as `key`, the case's
/// account? Its session is `key`'s (`account`), it holds `key`'s lease in
/// the active environment (`Held::holds`), it still holds the HttpOnly
/// cookies the app's own sign-in left (`Held::sign_in_print`,
/// `Held::same_sign_in`: not a login a step typed or the person made by
/// hand; a cookie a page's script rewrote is not the sign-in's, and one
/// that changed is named in the app log), and the page says
/// so (`signin::already_signed_in`). Only looked at, never changed. A
/// project whose recipe cannot be read is signed in as always, which says
/// why.
#[allow(clippy::too_many_arguments)]
pub async fn signed_in_here<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    key: &str,
    account: &Option<String>,
    lease: &Held,
    timing: &Timing,
) -> bool {
    if account.as_deref() != Some(key) || !lease.holds(root, key) {
        return false;
    }
    if lease.sign_in_print().is_none() {
        return false;
    }
    let Ok(recipe) = super::recipe::load_effective_recipe(root, organization, project) else { return false };
    // The assistant's test cookies are left out (`Held::same_sign_in`).
    if !lease.same_sign_in(d, &recipe.origins()).await {
        return false;
    }
    signin::already_signed_in(d, &recipe, account.as_deref(), key, timing).await
}

/// What a replay from the beginning made of a browser already signed in as
/// its case's account (`replay_to_traced`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reuse {
    /// The area it went to without signing in.
    pub went: Option<String>,
    /// A save the no-save guard stopped while it went there, said as
    /// `save_stopped_line`: the case then signed in again and went once more.
    pub save_stopped: Option<String>,
}

impl Reuse {
    /// Said after where the replay started, in place of a sign-in; nothing
    /// when the browser signed in as it always did.
    pub fn said(&self) -> Option<String> {
        match (&self.went, &self.save_stopped) {
            (Some(area), _) => Some(signed_in_already_line(area)),
            (None, Some(line)) => Some(line.clone()),
            (None, None) => None,
        }
    }
}

/// Said when a browser already signed in stopped a save (`sentence`, the
/// guard's) on its way to `area`, before the case signed in again.
pub fn save_stopped_line(area: &str, sentence: &str) -> String {
    // One dot, whether or not the sentence brought its own.
    let sentence = sentence.trim_end().trim_end_matches('.');
    format!("Signed in already, but on the way to {area}: {sentence}. Signed in again and went once more.")
}

/// What a replay's answer says, after where it started, when it went to
/// the case's area `area` in a browser already signed in as its account,
/// in place of a sign-in.
pub fn signed_in_already_line(area: &str) -> String {
    format!("Signed in already: went to {area}.")
}

/// How many steps of the case `script` has when `step` is past its last, so
/// a replay to it ran the whole case; `None` otherwise. The clean-up step is
/// not one of them.
pub fn whole_case(script: &CaseScript, step: i32) -> Option<i32> {
    let last = script.case_steps().map(|s| s.step_number).max().unwrap_or(0);
    (step > last).then(|| i32::try_from(script.case_steps().count()).unwrap_or(i32::MAX))
}

/// The number of the clean-up step, beside a replay to `step` that ran the
/// whole case: it ran too when `step` is past it. `None` when the replay
/// did not run the whole case or the script has no clean-up.
pub fn cleanup_ran(script: &CaseScript, step: i32) -> Option<i32> {
    let tidy = script.cleanup_step()?;
    whole_case(script, step)?;
    Some(tidy.step_number)
}

/// Run case `script`'s steps `from` to `n` - 1, in its order, in the
/// browser `d`, as a replay runs them: `Some` is where it stopped, `None`
/// that every one passed. `guarding_other` is a no-save guard held for
/// another case (`guarding_other_case`).
#[allow(clippy::too_many_arguments)]
async fn run_steps<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    script: &CaseScript,
    route: &Route,
    from: i32,
    n: i32,
    account: &mut Option<String>,
    lease: &mut Held,
    timing: &Timing,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(i32, i32),
    tabs: &mut Vec<ReplayedTab>,
    guarding_other: Option<i32>,
) -> Option<ReplayEnd> {
    let stopped = || cancel.load(Ordering::SeqCst);
    let in_range = |k: i32| k >= from && k < n;
    // The other areas their `return_to_area` actions name are read once,
    // before the first.
    let names = runner::named_areas(script.in_run_order().filter(|s| in_range(s.step_number)).flat_map(|s| s.actions.iter()));
    let areas = runner::area_routes(root, organization, project, &names);
    // The recipe and the areas file each step looks at: read once, and
    // again only when one changed.
    let files = super::run_files::RunFiles::default();
    // What the page met before the first of them is no step's error.
    super::page_errors::drop_all(d);
    // The last action that changed the page, carried from step to step: a
    // failure names what the page answered the step before. A replay keeps
    // no step records, so what came late is said only there.
    let mut noted = runner::Noted::default();
    noted.in_case(script.area.as_deref());
    // Its sentences name the clean-up as such.
    note_cleanup(script.cleanup_step().map(|s| s.step_number));
    // The case's steps, then its clean-up: a replay past the clean-up runs
    // it last, and only once every step before it passed.
    for (at, step) in script.in_run_order().filter(|s| in_range(s.step_number)).enumerate() {
        let k = step.step_number;
        if stopped() {
            return Some(ReplayEnd::Stopped { step: k });
        }
        note_progress(k, n - 1);
        progress(k, n - 1);
        noted.step = at;
        noted.take_late();
        let mut in_run = InRun {
            cancel: Some(cancel),
            stop_before_each_action: true,
            areas: Some(&areas),
            files: Some(&files),
            fail_on_unexpected_dialog: script.fail_on_unexpected_dialog,
            page_errors: script.page_errors,
            ignore_page_errors: script.ignore_page_errors.clone(),
            progress: Some(&ACTIONS),
            noted: Some(&mut noted),
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
            Some(route),
            AreaRoute::To(route),
            &mut in_run,
        )
        .await;
        if let Some(tab) = in_run.tab.take() {
            tabs.push(ReplayedTab { step: k, tab });
        }
        let mut outcomes = match ran {
            Ok(o) => o,
            Err(why) => return Some(ReplayEnd::StoppedAt { phase: ReplayPhase::Step, step: k, why, outcomes: Vec::new() }),
        };
        if outcomes.iter().any(|o| o.detail == AFTER_STOP) {
            return Some(ReplayEnd::Stopped { step: k });
        }
        if outcomes.iter().any(closed) {
            return Some(ReplayEnd::BrowserGone { step: k, why: None, outcomes: Vec::new() });
        }
        // What the step's components waited for and read is filed as seen
        // in the case's area, as a probe would file it: a toast is gone
        // before anyone can probe it.
        crate::ai_bridge::file_component_finds(d, root, organization, project, script.area.clone(), &step.actions, &outcomes)
            .await;
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
        // A step that failed teaches the application's rules, under the
        // area the case is in, with what came late for the step before and
        // the earlier action its failure line names.
        if outcomes.iter().any(|o| !o.ok) {
            let late = noted.take_late();
            let answers = noted.with_earlier(&late);
            let area = noted.area();
            runner::remember_failed_step(root, organization, project, step, &outcomes, &answers, area, script.case_id);
        }
        if let Some(mut why) = outcomes.iter().find(|o| !o.ok).map(|o| o.detail.clone()) {
            if let Some(other) = guarding_other.filter(|_| crate::browser::save_guard::is_blocked(&why)) {
                why = guarding_other_case(other);
            }
            return Some(ReplayEnd::StoppedAt { phase: ReplayPhase::Step, step: k, why, outcomes });
        }
    }
    None
}

/// Where an assistant's replay left its own browser: on the page whose
/// address path is `path`, before step `before` of case `case_id`, every
/// step before it run, at `at_ms` (milliseconds since the epoch). Kept on
/// the supervised session while nothing else has used that browser since
/// (no step, try, sign-in, start or other replay). `script` is a
/// fingerprint of the script that ran (`run_fingerprint`), so a script
/// changed since in anything that shapes the run is never carried on from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayStand {
    pub case_id: i32,
    pub before: i32,
    pub script: u64,
    pub path: String,
    pub at_ms: u64,
}

impl ReplayStand {
    /// The stand of a replay of `script` (the copy that ran) that reached
    /// step `before`, on the page at `path`, at `at_ms`.
    pub fn of(script: &CaseScript, before: i32, path: &str, at_ms: u64) -> Self {
        ReplayStand {
            case_id: script.case_id,
            before,
            script: run_fingerprint(script, before),
            path: path.to_string(),
            at_ms,
        }
    }
}

/// How long a stand is carried on from: past this the application's own
/// session may have timed out, so a replay starts from the beginning.
pub const STAND_FRESH_MS: u64 = 10 * 60 * 1000;

/// A fingerprint of everything in `script` that shapes a replay to step
/// `before`: its steps before `before`, and every script-level setting -
/// `no_save`, the account, the area, the setup, the preconditions, the
/// dialog and page-error choices. Only the bookkeeping a save or a run
/// writes (repairs, the last repair, the precondition repairs, when it
/// was saved, a suspected defect) is left out.
pub fn run_fingerprint(script: &CaseScript, before: i32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut shaped = script.clone();
    shaped.steps.retain(|s| s.step_number < before);
    shaped.repairs = 0;
    shaped.last_repair = None;
    shaped.precondition_repairs = Vec::new();
    shaped.saved_at = None;
    shaped.suspected_defect = None;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&shaped).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// Where an assistant's replay starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayFrom {
    /// From the beginning: preconditions, setup, sign-in, the area and
    /// step 1 on.
    Beginning,
    /// From step K: the browser already stood before it, so only steps K
    /// to N - 1 run.
    CarriedOn(i32),
    /// The browser already stands before the step asked for: nothing runs.
    AlreadyThere,
}

impl ReplayFrom {
    /// Said before the replay's own sentence.
    pub fn said(self, case_id: i32, step: i32) -> String {
        match self {
            ReplayFrom::Beginning => "Started from the beginning.".to_string(),
            ReplayFrom::CarriedOn(k) => format!("Carried on from step {k}."),
            // A reset takes the browser over (`DiscoveryBrowser::taken_over`
            // in `commands::autorun::reset_in_for_assistant`), which drops
            // the stand, so the next replay starts from the beginning.
            ReplayFrom::AlreadyThere => format!(
                "The browser is already standing before step {step} of case {case_id}: nothing ran. To run it again from the beginning, call reset_autorun_browser first."
            ),
        }
    }
}

/// Where a replay of case `case_id` to `step` starts, from the stand the
/// browser's last replay left (`None` when it left none, or something used
/// the browser since). `script` is the case's saved script now, `here` the
/// address path the browser is on and `now_ms` the time. The same case,
/// stood before step K with the script unchanged in anything that shapes
/// the run (`run_fingerprint`), the browser still on that page and the
/// stand younger than `STAND_FRESH_MS`: before step `step` already,
/// nothing runs; K earlier, it carries on from K. Anything else - another
/// case, a K past `step`, a script changed since, a browser on another
/// page, an old stand - starts from the beginning.
pub fn carry_from(stand: Option<&ReplayStand>, script: &CaseScript, step: i32, here: &str, now_ms: u64) -> ReplayFrom {
    match stand {
        Some(s)
            if s.case_id == script.case_id
                && s.before <= step
                && s.script == run_fingerprint(script, s.before)
                && !s.path.is_empty()
                && s.path == here
                && now_ms.saturating_sub(s.at_ms) < STAND_FRESH_MS =>
        {
            if s.before == step {
                ReplayFrom::AlreadyThere
            } else {
                ReplayFrom::CarriedOn(s.before)
            }
        }
        _ => ReplayFrom::Beginning,
    }
}

/// Carry case `req.case_id` on in `d` from the page before step `from`,
/// where its last replay left it, to the page before step `req.step`: only
/// steps `from` to `req.step` - 1 run, each with its fixture values filled
/// in as a person's step is (`setup::resolve_step`). The case's no-save
/// guard is armed first, as at the start of every replay (`guard_for_case`,
/// never lifting one held for another case). No preconditions,
/// setup, sign-in or trip to the area: the browser is already past them.
#[allow(clippy::too_many_arguments)]
pub async fn carry_on<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    req: &ReplayRequest,
    from: i32,
    account: &mut Option<String>,
    lease: &mut Held,
    guarded_case: &mut Option<i32>,
    timing: &Timing,
    cancel: &AtomicBool,
    mut progress: impl FnMut(i32, i32),
    tabs: &mut Vec<ReplayedTab>,
) -> ReplayEnd {
    let Checked { script, route } = match check(root, organization, project, req) {
        Ok(c) => c,
        Err(why) => return ReplayEnd::Refused(why),
    };
    let (id, n) = (req.case_id, req.step);
    let mut filled = script.clone();
    for step in filled.steps.iter_mut().filter(|s| s.step_number >= from && s.step_number < n) {
        match setup::resolve_step(root, organization, project, id, step) {
            Ok(resolved) => *step = resolved,
            Err(why) => return ReplayEnd::Refused(why),
        }
    }
    // The no-save guard, as every replay arms it: a script marked Must not
    // save is guarded however it was when the browser got here.
    if let Err(why) =
        crate::commands::autorun::guard_for_case(d, guarded_case, root, organization, project, id, false).await
    {
        return ReplayEnd::Refused(why);
    }
    let guarding_other = guarded_case.filter(|c| *c != id);
    if let Some(stopped) = run_steps(
        d,
        root,
        organization,
        project,
        &filled,
        &route,
        from,
        n,
        account,
        lease,
        timing,
        cancel,
        &mut progress,
        tabs,
        guarding_other,
    )
    .await
    {
        return stopped;
    }
    ReplayEnd::Ready { case_id: id, step: n, notice: None, whole: whole_case(&script, n), cleanup: cleanup_ran(&script, n) }
}
