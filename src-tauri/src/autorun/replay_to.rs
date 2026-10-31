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

use super::lease::{CookieCheck, Held};
use super::nav::{Route, TripFrom};
use super::preconditions::{self, NoDb, PreconditionDb};
use super::runner::{self, AreaRoute, InRun, AFTER_STOP};
use super::recipe::SignInRecipe;
use super::replay::Browsers;
use super::setup::{self, NoBrowsers};
use super::{replay, signin, store, CaseScript};
use crate::api_templates::gate::StageDb;
use crate::browser::actions::{Action, ActionOutcome};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::timing::Timing;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use super::slots::{SlotNo, SLOT_COUNT};

/// Said when a replay is asked for while one is already going.
pub const ALREADY_RUNNING: &str = "a replay is already running - wait for it to finish";

/// What a replay is asked to do: carry case `case_id` to the page before
/// `step` runs. `db_read_access` is the Database Access module being on, as
/// for a supervised case's preconditions.
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
                format!(", and template runs as {key} until a release (reset_autorun_browser) frees it (a discovery keeps its sign-in)"),
            ),
            None => ("", String::new()),
        };
        return format!(
            "The Auto Run browser stays open on this step{signed}: discovery actions and component saves are refused until start_autorun_discovery takes it over{templates}. Do not release it between replays: reset_autorun_browser's release is for a broken browser."
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

const NOBODY: u8 = 0;
const PERSON: u8 = 1;
const ASSISTANT: u8 = 2;

/// One browser slot's replay: whether one is going and whose, its stop
/// control, how far it has got and the action it is on. Every slot has its
/// own (`state`), so a stop, a step or an action in one slot is never read
/// as another's. Coordination only, nothing a restart keeps.
pub struct ReplayState {
    /// Whether a replay is going, and whose: `NOBODY`, `PERSON` or
    /// `ASSISTANT`. Only `OneReplay` sets it.
    running: std::sync::atomic::AtomicU8,
    /// The replay's stop control: set by `stop`, and by anything that closes
    /// the slot's browser. Read before each part of the replay and by the
    /// step's own waits, never under the session's lock, so a stop is heard
    /// while the replay holds the browser.
    pub cancel: AtomicBool,
    /// Whether the replay going was asked to stop by the assistant's
    /// the replay tool's `stop` (`stop_on_request`), not by the person's Stop,
    /// Close or Release. Cleared by a claim, and when the assistant starts a
    /// replay (`forget_stop_request`).
    on_request: AtomicBool,
    /// Where the replay going is: the step it is running and how many steps
    /// it runs in all, packed as `(step << 32) | of`. Zero before step 1
    /// (signing in, going to the area). Set by `note_progress`, cleared by a
    /// claim.
    progress: std::sync::atomic::AtomicU64,
    /// The step number of the clean-up of the case the replay going (or
    /// ended last) runs, 0 when it has none: noted as its steps start
    /// (`run_steps`), cleared by a claim, so its sentences say "the clean-up".
    cleanup_at: std::sync::atomic::AtomicI32,
    /// The action the replay going is on, within its step
    /// (`runner::StepProgress`).
    pub actions: runner::StepProgress,
}

impl ReplayState {
    pub const fn new() -> ReplayState {
        ReplayState {
            running: std::sync::atomic::AtomicU8::new(NOBODY),
            cancel: AtomicBool::new(false),
            on_request: AtomicBool::new(false),
            progress: std::sync::atomic::AtomicU64::new(0),
            cleanup_at: std::sync::atomic::AtomicI32::new(0),
            actions: runner::StepProgress::new(),
        }
    }

    /// Ask the replay that is going, if any, to stop.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// The assistant's replay `stop`: the same stop as `stop`, noted
    /// as asked for, so the replay's answer says it was stopped on request.
    pub fn stop_on_request(&self) {
        self.on_request.store(true, Ordering::SeqCst);
        self.stop();
    }

    /// Whether the replay that ended last was asked to stop by the assistant.
    pub fn stop_was_requested(&self) -> bool {
        self.on_request.load(Ordering::SeqCst)
    }

    /// A new replay of the assistant's starts with no stop asked of it.
    pub fn forget_stop_request(&self) {
        self.on_request.store(false, Ordering::SeqCst);
    }

    /// The replay going is about to run step `step` of `of`: kept for whoever
    /// asks how far it has got (`progress`).
    pub fn note_progress(&self, step: i32, of: i32) {
        let packed = (u64::from(step.max(0) as u32) << 32) | u64::from(of.max(0) as u32);
        self.progress.store(packed, Ordering::SeqCst);
        self.actions.reset();
    }

    /// The case the replay runs has its clean-up at step `step` (`None`: none).
    pub fn note_cleanup(&self, step: Option<i32>) {
        self.cleanup_at.store(step.unwrap_or(0).max(0), Ordering::SeqCst);
    }

    /// The clean-up step of the case the replay going (or ended last) runs.
    pub fn cleanup_at(&self) -> Option<i32> {
        let k = self.cleanup_at.load(Ordering::SeqCst);
        (k > 0).then_some(k)
    }

    /// The action the replay going is on, and how many of its step's actions
    /// have failed: none before its first step's first action.
    pub fn action_now(&self) -> runner::ActionNow {
        self.actions.now()
    }

    /// The step the replay going is running and how many it runs, `(0, 0)`
    /// before its first step.
    pub fn progress(&self) -> (i32, i32) {
        let packed = self.progress.load(Ordering::SeqCst);
        ((packed >> 32) as i32, (packed & 0xFFFF_FFFF) as i32)
    }

    /// Whether a replay to a step is going in this slot right now.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst) != NOBODY
    }

    /// Whether the replay going in this slot right now is the assistant's
    /// own (`OneReplay::claim_for_assistant_in`). False while nothing runs -
    /// before its replay has claimed the slot, or once it has ended and its
    /// page is still being read - and while the person's replay runs.
    pub fn assistants_replay_running(&self) -> bool {
        self.running.load(Ordering::SeqCst) == ASSISTANT
    }
}

impl Default for ReplayState {
    fn default() -> Self {
        ReplayState::new()
    }
}

/// Every browser slot's replay state, indexed by `SlotNo`.
static REPLAYS: [ReplayState; SLOT_COUNT] = [const { ReplayState::new() }; SLOT_COUNT];

/// Slot `n`'s replay state.
pub fn state(n: SlotNo) -> &'static ReplayState {
    &REPLAYS[n.index()]
}

/// Whether a replay is going in any slot.
pub fn any_running() -> bool {
    REPLAYS.iter().any(ReplayState::is_running)
}

/// Ask the main slot's replay, if any, to stop.
pub fn stop() {
    state(SlotNo::MAIN).stop();
}

/// The main slot's `ReplayState::stop_on_request`.
pub fn stop_on_request() {
    state(SlotNo::MAIN).stop_on_request();
}

/// The main slot's `ReplayState::stop_was_requested`.
pub fn stop_was_requested() -> bool {
    state(SlotNo::MAIN).stop_was_requested()
}

/// The main slot's `ReplayState::forget_stop_request`.
pub fn forget_stop_request() {
    state(SlotNo::MAIN).forget_stop_request();
}

/// What the assistant's replay that its `stop` stopped answers:
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

/// The one replay going in a slot. Taking it clears an earlier stop; dropping
/// it - an end, an error or a panic - frees that slot, and only that slot.
pub struct OneReplay(SlotNo);

impl OneReplay {
    /// The person's replay (Replay to step in the pane) takes the main slot.
    pub fn claim() -> Result<OneReplay, String> {
        Self::claim_as(SlotNo::MAIN, PERSON)
    }

    /// The assistant's replay (`replay_autorun_to_step`) takes the main slot:
    /// the only replay the replay tool's `stop` may stop
    /// (`assistants_replay_running`).
    pub fn claim_for_assistant() -> Result<OneReplay, String> {
        Self::claim_as(SlotNo::MAIN, ASSISTANT)
    }

    /// The person's replay takes slot `n`.
    pub fn claim_in(n: SlotNo) -> Result<OneReplay, String> {
        Self::claim_as(n, PERSON)
    }

    /// The assistant's replay takes slot `n`.
    pub fn claim_for_assistant_in(n: SlotNo) -> Result<OneReplay, String> {
        Self::claim_as(n, ASSISTANT)
    }

    fn claim_as(n: SlotNo, owner: u8) -> Result<OneReplay, String> {
        let s = state(n);
        s.running
            .compare_exchange(NOBODY, owner, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| ALREADY_RUNNING.to_string())?;
        s.cancel.store(false, Ordering::SeqCst);
        s.on_request.store(false, Ordering::SeqCst);
        s.progress.store(0, Ordering::SeqCst);
        s.cleanup_at.store(0, Ordering::SeqCst);
        s.actions.reset();
        Ok(OneReplay(n))
    }
}

impl Drop for OneReplay {
    fn drop(&mut self) {
        state(self.0).running.store(NOBODY, Ordering::SeqCst);
    }
}

/// The main slot's `ReplayState::note_progress`.
pub fn note_progress(step: i32, of: i32) {
    state(SlotNo::MAIN).note_progress(step, of);
}

/// The main slot's `ReplayState::note_cleanup`.
pub fn note_cleanup(step: Option<i32>) {
    state(SlotNo::MAIN).note_cleanup(step);
}

/// The main slot's `ReplayState::cleanup_at`.
pub fn cleanup_at() -> Option<i32> {
    state(SlotNo::MAIN).cleanup_at()
}

/// The main slot's `ReplayState::action_now`.
pub fn action_now() -> runner::ActionNow {
    state(SlotNo::MAIN).action_now()
}

/// The main slot's `ReplayState::progress`.
pub fn progress() -> (i32, i32) {
    state(SlotNo::MAIN).progress()
}

/// Whether a replay to a step is going in the main slot right now.
pub fn is_running() -> bool {
    state(SlotNo::MAIN).is_running()
}

/// Whether the replay going in the main slot right now is the assistant's
/// own (`OneReplay::claim_for_assistant`).
pub fn assistants_replay_running() -> bool {
    state(SlotNo::MAIN).assistants_replay_running()
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
/// is refused with `preconditions::NEED_DB` while the Database Access module
/// is on, and carries the notice while it is off; a setup fails to open its
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
    replay: &ReplayState,
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
        replay,
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
/// step runs, `total` being `req.step - 1`. `replay` is the slot's replay
/// state: its stop control, how far it has got and the action it is on.
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
    replay: &ReplayState,
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
        replay,
        db,
        progress,
        &mut tabs,
        &mut Reuse::default(),
        &mut Marks::default(),
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
///
/// `marks` learns back where the page stood after each case step that
/// passed (`Checkpoint`), for the stand a replay that reaches its step
/// leaves.
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
    replay: &ReplayState,
    db: impl FnOnce() -> PreconditionDb<P>,
    mut progress: impl FnMut(i32, i32),
    tabs: &mut Vec<ReplayedTab>,
    reuse: &mut Reuse,
    marks: &mut Marks,
) -> ReplayEnd {
    *reuse = Reuse::default();
    marks.clear();
    let Checked { script, route } = match check(root, organization, project, req) {
        Ok(c) => c,
        Err(why) => return ReplayEnd::Refused(why),
    };
    let (id, n) = (req.case_id, req.step);
    let stopped = || replay.cancel.load(Ordering::SeqCst);
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
        setup::prepare_case_hooked(setup_browsers, root, organization, project, &script, timing, &replay.cancel, make_way).await;
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
        // Only a case whose account this browser is signed in as is asked
        // why not: any other account signs in as always.
        let reused = if account.as_deref() == Some(key) {
            match signed_in_here_why(d, root, organization, project, key, account, lease, timing).await {
                Ok(()) => true,
                Err(r) => {
                    crate::applog::info(format!("{}: not signed in already as {key}: {}", who(id), r.reason()));
                    false
                }
            }
        } else {
            false
        };
        if reused {
            let (went, menu) = replay::trip_to_module_noted(d, root, &route, TripFrom::Elsewhere, timing, &who(id), Some(key)).await;
            if !menu.is_empty() {
                reuse.menu.extend(menu);
                reuse.account = Some(key.to_string());
            }
            if closed(&went) {
                return ReplayEnd::BrowserGone { step: 1, why: None, outcomes: Vec::new() };
            }
            let area = route.path.name();
            if went.ok {
                crate::applog::info(format!("{}: signed in already as {key}, went to {area}", who(id)));
                reuse.went = Some(area.to_string());
                // The trip may have reloaded the home page, which can re-issue
                // a cookie of this same sign-in: the print is taken again.
                if let Err(r) = retake_sign_in_for(d, root, organization, project, lease, "the trip to the area").await {
                    crate::applog::info(format!(
                        "{}: the sign-in's cookies could not be taken again after the trip: {}",
                        who(id),
                        r.reason()
                    ));
                }
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
        let (went, menu) = replay::trip_to_module_noted(d, root, &route, from, timing, &who(id), script.account.as_deref()).await;
        if !menu.is_empty() {
            reuse.menu.extend(menu);
            if reuse.account.is_none() {
                reuse.account = script.account.clone();
            }
        }
        if closed(&went) {
            return ReplayEnd::BrowserGone { step: 1, why: None, outcomes: Vec::new() };
        }
        if !went.ok {
            return ReplayEnd::StoppedAt { phase: ReplayPhase::Area, step: 1, why: went.detail.clone(), outcomes: vec![went] };
        }
    }

    // Steps 1 to N-1, in the script's order.
    let ended =
        run_steps(d, root, organization, project, &script, &route, 1, n, account, lease, timing, replay, &mut progress, tabs, guarding_other, marks, false)
            .await;
    // Wherever the steps ended, the page they ran on may have re-issued the
    // sign-in's cookies: print them again, so a replay of the same case next
    // keeps the sign-in (a browser that is gone has nothing to print).
    if !matches!(ended, Some(ReplayEnd::BrowserGone { .. })) {
        retake_after_steps(d, root, organization, project, &script, n, account, lease, timing).await;
    }
    if let Some(stopped) = ended {
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
    signed_in_here_why(d, root, organization, project, key, account, lease, timing).await.is_ok()
}

/// `signed_in_here`, saying why not (`NotSignedIn`).
#[allow(clippy::too_many_arguments)]
pub async fn signed_in_here_why<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    key: &str,
    account: &Option<String>,
    lease: &Held,
    timing: &Timing,
) -> Result<(), NotSignedIn> {
    let recipe = sign_in_stands(d, root, organization, project, key, account, lease).await?;
    signin::already_signed_in_why(d, &recipe, account.as_deref(), key, timing, timing.expect_ms.min(signin::SIGNED_IN_LOOK_MS)).await
}

/// Why a browser is not (or no longer) signed in as the case's account.
/// `reason` is a clause of the app log's line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotSignedIn {
    /// The browser's session names no account.
    NoAccount,
    /// The browser's session is another account's.
    OtherAccount,
    /// This browser does not hold the account's lease in the active environment.
    LeaseNotHeld,
    /// The app's last sign-in left no print of its HttpOnly cookies.
    NoPrint,
    /// The sign-in recipe could not be read.
    NoRecipe,
    /// The sign-in's HttpOnly cookies changed or went (named on the line before).
    CookiesChanged,
    /// The browser did not answer while its cookies were read.
    CookiesUnread,
    /// The page says the application refused the session.
    SessionRefused,
    /// The recipe's signed-in marker did not show within this many ms.
    MarkerNotShown(u64),
    /// The home page opened but never showed it was signed in.
    HomeNeverSignedIn,
    /// No HttpOnly cookies could be read after the home page loaded.
    NoCookiesAfterHome,
}

impl NotSignedIn {
    pub fn reason(&self) -> String {
        match self {
            NotSignedIn::NoAccount => "no account is signed in in this browser".to_string(),
            NotSignedIn::OtherAccount => "the browser is signed in as another account".to_string(),
            NotSignedIn::LeaseNotHeld => {
                "this browser does not hold the account's lease in the active environment".to_string()
            }
            NotSignedIn::NoPrint => "the app's last sign-in left no print of its HttpOnly cookies".to_string(),
            NotSignedIn::NoRecipe => "the sign-in recipe could not be read".to_string(),
            NotSignedIn::CookiesChanged => "the sign-in's HttpOnly cookies changed (named on the line before)".to_string(),
            NotSignedIn::CookiesUnread => "the browser did not answer while its cookies were read".to_string(),
            NotSignedIn::SessionRefused => "the page says the application refused the session".to_string(),
            NotSignedIn::MarkerNotShown(ms) => format!("the signed-in marker did not show within {ms} ms"),
            NotSignedIn::HomeNeverSignedIn => crate::autorun::nav::HOME_NEVER_SIGNED_IN.to_string(),
            NotSignedIn::NoCookiesAfterHome => "no HttpOnly cookies could be read after the home page loaded".to_string(),
        }
    }
}

/// A sign-in that stood is taken again after the app's own home load or
/// trip ran: a page the app serves (its login page, on PeoplesHR) may
/// re-issue an HttpOnly cookie of that very sign-in, and the print taken
/// before would call it changed from then on. Callers have judged the
/// cookies just before (`sign_in_stands`), and only the app's own page
/// loads and menu clicks ran since. The cookies the load re-issued are
/// named in the app log (names only, never a value; `by` says what
/// loaded). When no cookie can be read the sign-in is forgotten
/// (`NoCookiesAfterHome`), so nothing is trusted that cannot be checked.
pub async fn retake_sign_in<D: Driver>(
    d: &mut D,
    origins: &[String],
    lease: &mut Held,
    by: &str,
) -> Result<(), NotSignedIn> {
    if let Some(old) = lease.sign_in_print() {
        if let Some(changed) = crate::browser::session::changed_cookies(d, origins, old, lease.ignored_in_print()).await {
            if !changed.is_empty() {
                crate::applog::info(format!("Auto Run: {by} re-issued the sign-in's cookies: {}", changed.join(", ")));
            }
        }
    }
    match crate::browser::session::cookie_print(d, origins).await {
        Some(print) => {
            lease.note_sign_in(Some(print));
            Ok(())
        }
        None => {
            lease.forget_sign_in();
            Err(NotSignedIn::NoCookiesAfterHome)
        }
    }
}

/// Whether a list of actions (a discovery call's, or a replay's steps) could have changed who is signed in:
/// a `sign_in`, a fill of a password-type input (or of the recipe's own
/// password field), or a navigation to the recipe's sign-in address. Says
/// which, in a few words.
pub fn touches_sign_in(
    actions: &[crate::browser::actions::Action],
    recipe: &crate::autorun::recipe::SignInRecipe,
) -> Option<String> {
    use crate::autorun::recipe::RecipeStep;
    use crate::browser::actions::Action;
    let text = |t: &crate::browser::locator::Target| serde_json::to_string(t).unwrap_or_default().to_lowercase();
    // The fields the recipe itself fills (its prompts' too): a script that
    // fills the same field logs in, whatever the field is called.
    let mut recipe_fields: Vec<String> = vec![];
    for st in &recipe.steps {
        match st {
            RecipeStep::Do(Action::Fill { selector, .. }) => recipe_fields.push(text(selector)),
            RecipeStep::WhenVisible(w) => {
                for a in &w.then {
                    if let Action::Fill { selector, .. } = a {
                        recipe_fields.push(text(selector));
                    }
                }
            }
            _ => {}
        }
    }
    let start = crate::autorun::discovery_map::path_only(&recipe.start_url);
    fn address_touches(url: &str, start: &str) -> Option<&'static str> {
        let path = crate::autorun::discovery_map::path_only(url);
        if path == start {
            Some("went to the sign-in address")
        } else if path.to_lowercase().contains("/home/switch") {
            Some("switched the side of the site")
        } else {
            None
        }
    }
    fn walk(
        actions: &[Action],
        start: &str,
        fields: &[String],
        text: &dyn Fn(&crate::browser::locator::Target) -> String,
    ) -> Option<String> {
        for a in actions {
            match a {
                Action::SignIn { .. } => return Some("signed in".to_string()),
                Action::Fill { selector, value } => {
                    let t = text(selector);
                    let v = value.to_lowercase();
                    if t.contains("password")
                        || t.contains("pwd")
                        || v.contains("{{password}}")
                        || v.contains("{{username}}")
                        || fields.contains(&t)
                    {
                        return Some("filled a login field".to_string());
                    }
                }
                Action::Navigate { url } | Action::OpenTab { url, .. } => {
                    if let Some(why) = address_touches(url, start) {
                        return Some(why.to_string());
                    }
                }
                Action::WhenVisible { then, .. } | Action::RepeatWhileVisible { then, .. } => {
                    if let Some(why) = walk(then, start, fields, text) {
                        return Some(why);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(actions, &start, &recipe_fields, &text)
}

/// Backlog 44b: a replay's own steps may make the app re-issue the sign-in's
/// session cookie as its pages load, which the print taken at sign-in then
/// calls changed: the next replay of the case would sign in again. Once the
/// steps have ended - stopped before a step, or the whole case - the print
/// is taken again, but only when the sign-in is provably the same one:
/// the lease is still held, the steps (components expanded, `when_visible`
/// and `repeat_while_visible` looked into) could not have changed the login
/// (`touches_sign_in`), the identity cookie the print holds (`ehrm85`, which
/// carries the user the server checks the sign-in against) has the value it
/// had, and the page still shows it is signed in. Otherwise the old print
/// stays, so the next replay judges the browser by it - and signs in again
/// if it changed. What was decided is logged, with no path and no value.
#[allow(clippy::too_many_arguments)]
async fn retake_after_steps<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    script: &CaseScript,
    n: i32,
    account: &Option<String>,
    lease: &mut Held,
    timing: &Timing,
) {
    let Some(key) = script.account.as_deref().filter(|k| account.as_deref() == Some(*k)) else { return };
    let said = |why: &str| crate::applog::info(format!("{}: the sign-in's cookies were not taken again: {why}", who(script.case_id)));
    if !lease.holds(root, key) {
        return;
    }
    let Ok(recipe) = super::recipe::load_effective_recipe(root, organization, project) else { return };
    // Every step the replay could have run: those before `n`, and `n` itself
    // (a replay stopped at a step may have run part of it), and the clean-up.
    let written: Vec<Action> = script
        .in_run_order()
        .filter(|s| s.step_number <= n || s.cleanup)
        .flat_map(|s| s.actions.iter().cloned())
        .collect();
    let ran = match super::components::load_components(root, organization, project)
        .and_then(|file| super::components::expand_actions(&file, &written).map(|(a, _)| a))
    {
        Ok(a) => a,
        Err(_) => return said("the steps' components could not be read"),
    };
    if let Some(why) = touches_sign_in(&ran, &recipe) {
        return said(&format!("the steps {why}"));
    }
    let origins = recipe.origins();
    let Some(print) = lease.sign_in_print() else { return };
    if !print.holds_cookie(IDENTITY_COOKIE) {
        return said("the sign-in left no identity cookie to compare");
    }
    let Some(changed) = crate::browser::session::changed_cookies(d, &origins, print, lease.ignored_in_print()).await else {
        return said("the browser did not answer while its cookies were read");
    };
    if changed.iter().any(|c| c.to_ascii_lowercase().starts_with(&format!("{IDENTITY_COOKIE} "))) {
        return said("the identity cookie is not the one the sign-in left");
    }
    let shown = signin::already_signed_in_why(d, &recipe, account.as_deref(), key, timing, timing.expect_ms.min(signin::SIGNED_IN_LOOK_MS)).await;
    if shown.is_ok() {
        let _ = retake_sign_in(d, &origins, lease, "the replay's steps").await;
    }
}

/// The cookie that carries the user the server checks the sign-in against
/// (PeoplesHR's), compared before a replay's print is taken again.
const IDENTITY_COOKIE: &str = "ehrm85";

/// `retake_sign_in` for the cookies of `root`'s project recipe (read again;
/// one that cannot be read forgets the sign-in).
pub async fn retake_sign_in_for<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    lease: &mut Held,
    by: &str,
) -> Result<(), NotSignedIn> {
    match super::recipe::load_effective_recipe(root, organization, project) {
        Ok(recipe) => retake_sign_in(d, &recipe.origins(), lease, by).await,
        Err(_) => {
            lease.forget_sign_in();
            Err(NotSignedIn::NoRecipe)
        }
    }
}

/// The part of `signed_in_here_why` that never looks at the page: the
/// browser's session is `key`'s (`account`), it holds `key`'s lease in the
/// active environment, the app's last sign-in left a print, the recipe can
/// be read, and the print's HttpOnly cookies are all still there with the
/// same values (`Held::check_sign_in`). Gives the recipe it read.
pub async fn sign_in_stands<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    key: &str,
    account: &Option<String>,
    lease: &Held,
) -> Result<SignInRecipe, NotSignedIn> {
    match account.as_deref() {
        None => return Err(NotSignedIn::NoAccount),
        Some(a) if a != key => return Err(NotSignedIn::OtherAccount),
        Some(_) => {}
    }
    if !lease.holds(root, key) {
        return Err(NotSignedIn::LeaseNotHeld);
    }
    if lease.sign_in_print().is_none() {
        return Err(NotSignedIn::NoPrint);
    }
    let Ok(recipe) = super::recipe::load_effective_recipe(root, organization, project) else {
        return Err(NotSignedIn::NoRecipe);
    };
    // The assistant's test cookies are left out (`Held::check_sign_in`).
    match lease.check_sign_in(d, &recipe.origins()).await {
        CookieCheck::Same => Ok(recipe),
        CookieCheck::NoPrint => Err(NotSignedIn::NoPrint),
        CookieCheck::Changed(_) => Err(NotSignedIn::CookiesChanged),
        CookieCheck::Unread => Err(NotSignedIn::CookiesUnread),
    }
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
    /// What the trips set up in the application's menu preferences on the
    /// way to the area (`menu_setup`): one note for each setting changed,
    /// or for a dialog that could not be checked.
    pub menu: Vec<String>,
    /// The case's account, whose menu `menu` is about.
    pub account: Option<String>,
    /// Why a replay of another case did not carry on from the one the
    /// browser last ran (`carry_across_why`): said in place of "Started
    /// from the beginning.".
    pub not_carried: Option<String>,
}

impl Reuse {
    /// Said after where the replay started, in place of a sign-in; nothing
    /// when the browser signed in as it always did and set nothing up.
    /// `key` is the account the menu was set up for.
    pub fn said(&self, key: Option<&str>) -> Option<String> {
        let first = match (&self.went, &self.save_stopped) {
            (Some(area), _) => Some(signed_in_already_line(area)),
            (None, Some(line)) => Some(line.clone()),
            (None, None) => None,
        };
        if self.menu.is_empty() {
            return first;
        }
        let menu = match key {
            Some(key) => format!("Menu set up for {key}: {}.", self.menu.join("; ")),
            None => format!("Menu set up: {}.", self.menu.join("; ")),
        };
        Some(match first {
            Some(first) => format!("{first} {menu}"),
            None => menu,
        })
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
/// another case (`guarding_other_case`). After each case step that passes,
/// where the page stands is pushed to `marks` (`Checkpoint`), `wrote_before`
/// being whether anything before `from` already saved.
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
    replay: &ReplayState,
    progress: &mut impl FnMut(i32, i32),
    tabs: &mut Vec<ReplayedTab>,
    guarding_other: Option<i32>,
    marks: &mut Marks,
    wrote_before: bool,
) -> Option<ReplayEnd> {
    // One mark of the network record for the whole run, let go on every way
    // out; and how many tabs the browser had had by then.
    let mark = d.net_mark();
    let tabs_before = d.tabs_opened();
    let ended = run_steps_marked(
        d, root, organization, project, script, route, from, n, account, lease, timing, replay, progress, tabs,
        guarding_other, marks, wrote_before, mark, tabs_before,
    )
    .await;
    d.net_unmark(mark);
    ended
}

#[allow(clippy::too_many_arguments)]
async fn run_steps_marked<D: Driver>(
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
    replay: &ReplayState,
    progress: &mut impl FnMut(i32, i32),
    tabs: &mut Vec<ReplayedTab>,
    guarding_other: Option<i32>,
    marks: &mut Marks,
    wrote_before: bool,
    mark: u64,
    tabs_before: u64,
) -> Option<ReplayEnd> {
    let stopped = || replay.cancel.load(Ordering::SeqCst);
    let in_range = |k: i32| k >= from && k < n;
    // A look-only replay (the script must not save, or the browser is
    // guarded for another case) cannot save: the guard stops every request
    // it takes for a save, so a write that got through is not called a save
    // (`Unclean::Posted`).
    let look_only = script.no_save || guarding_other.is_some() || d.is_guarding_saves();
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
    replay.note_cleanup(script.cleanup_step().map(|s| s.step_number));
    // The case's steps, then its clean-up: a replay past the clean-up runs
    // it last, and only once every step before it passed.
    let order: Vec<&super::StepScript> = script.in_run_order().collect();
    for (at, step) in script.in_run_order().filter(|s| in_range(s.step_number)).enumerate() {
        let k = step.step_number;
        if stopped() {
            return Some(ReplayEnd::Stopped { step: k });
        }
        replay.note_progress(k, n - 1);
        progress(k, n - 1);
        noted.step = at;
        noted.take_late();
        let mut in_run = InRun {
            cancel: Some(&replay.cancel),
            stop_before_each_action: true,
            areas: Some(&areas),
            files: Some(&files),
            fail_on_unexpected_dialog: script.fail_on_unexpected_dialog,
            page_errors: script.page_errors,
            ignore_page_errors: script.ignore_page_errors.clone(),
            progress: Some(&replay.actions),
            noted: Some(&mut noted),
            next_step_first: order
                .iter()
                .position(|s| s.step_number == k)
                .and_then(|i| order.get(i + 1))
                .filter(|n| !n.cleanup)
                .and_then(|n| n.actions.first()),
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
            let why = crate::browser::input::with_offscreen_hint(&why);
            return Some(ReplayEnd::StoppedAt { phase: ReplayPhase::Step, step: k, why, outcomes });
        }
        // The clean-up is no step of the case: nothing is carried on from
        // after it.
        if !step.cleanup {
            // Once a save is seen it stays seen: a record that later drops
            // the request must not turn it back into "nothing saved".
            let wrote = wrote_before
                || marks.checkpoints.last().is_some_and(|c| c.wrote)
                || d.net_wrote_since(mark);
            let (path, tab, dialogs) = page_state(d).await;
            marks.checkpoints.push(Checkpoint { k, prefix: steps_fingerprint(script, k), path, tab, dialogs, wrote });
        }
    }
    // Every step passed, the clean-up too: what the whole run did.
    marks.unclean = if !d.net_recording() {
        Some(Unclean::Unrecorded)
    } else if wrote_before && !look_only {
        Some(Unclean::Saved)
    } else if wrote_before || d.net_wrote_since(mark) {
        // A look-only script cannot have saved (its guard stops what it takes
        // for a save), but the guard is a list of words: a write it let
        // through may still have changed the page's state. Say so, by path.
        // An earlier part of the same case that posted (`wrote_before`) counts the same way.
        if look_only {
            crate::applog::info(format!(
                "Auto Run: a look-only replay sent a request the save guard let through: {}",
                d.net_posted_paths(mark).join(", ")
            ));
            Some(Unclean::Posted)
        } else {
            Some(Unclean::Saved)
        }
    } else if d.tabs_opened() > tabs_before {
        Some(Unclean::Tabs)
    } else {
        None
    };
    None
}

/// Why a finished replay's browser cannot be carried on from by another
/// case: something the replay did could have changed the state that case's
/// own first steps would leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unclean {
    /// A save went out (or the record can no longer rule one out): in the
    /// steps, after the shared ones, or in the clean-up.
    Saved,
    /// A look-only replay's page sent a write the save guard let through
    /// (it is a list of words): not a save, but the state may have changed.
    Posted,
    /// The network record was not on, so no save could have been seen.
    Unrecorded,
    /// A second tab was open at some point.
    Tabs,
}

impl Unclean {
    /// The one sentence an answer gives for starting from the beginning.
    /// `in_shared` is whether the save was already there at the shared
    /// steps (the best checkpoint's `wrote`).
    pub fn why(self, case_id: i32, in_shared: bool) -> String {
        match self {
            Unclean::Saved if in_shared => format!("case {case_id} saved something in the shared steps."),
            Unclean::Saved => format!("case {case_id} saved something after the shared steps."),
            Unclean::Posted => {
                format!("case {case_id}'s page sent a request the save guard let through, so its state may have changed.")
            }
            Unclean::Unrecorded => {
                format!("case {case_id} ran without a network record, so a save could not be ruled out.")
            }
            Unclean::Tabs => format!("case {case_id} opened another tab."),
        }
    }
}

/// What a replay learns back about the browser it left, for the stand
/// (`ReplayStand`): where the page stood after each case step that passed,
/// and, once every step passed, whether anything the whole run did could
/// have changed state (`Unclean`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    pub checkpoints: Vec<Checkpoint>,
    /// `None` until the run finishes, and for a run that changed nothing.
    pub unclean: Option<Unclean>,
}

impl Marks {
    pub fn clear(&mut self) {
        *self = Marks::default();
    }
}

/// Where the page stood after case step `k` of a replay passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub k: i32,
    /// `steps_fingerprint(script, k)`.
    pub prefix: u64,
    /// The address path then.
    pub path: String,
    /// `Driver::tab_name()` then.
    pub tab: String,
    /// `OPEN_DIALOGS_JS` then.
    pub dialogs: u32,
    /// Any write sent from the replay's start to here.
    pub wrote: bool,
}

/// Counts the visible dialogs on the page. An expression with no input in it.
pub const OPEN_DIALOGS_JS: &str = "Array.from(document.querySelectorAll('dialog[open], [role=dialog], [role=alertdialog], .modal.show')).filter(function (e) { var r = e.getBoundingClientRect(); var s = getComputedStyle(e); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; }).length";

/// The address path, the current tab's name and how many dialogs are open.
/// A count that cannot be read is `u32::MAX`, which never matches.
pub async fn page_state<D: Driver>(d: &mut D) -> (String, String, u32) {
    let (path, _) = crate::ai_bridge::current_page(d).await;
    let tab = d.tab_name();
    let dialogs = crate::browser::page::eval_value(d, OPEN_DIALOGS_JS)
        .await
        .ok()
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(u32::MAX);
    (path, tab, dialogs)
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
    /// `settings_fingerprint` of the script that ran.
    pub settings: u64,
    /// Where the page stood after each case step that passed.
    pub checkpoints: Vec<Checkpoint>,
    /// What the whole replay did that another case could not carry on over;
    /// `None` when it did nothing of the kind.
    pub unclean: Option<Unclean>,
}

impl ReplayStand {
    /// The stand of a replay of `script` (the copy that ran) that reached
    /// step `before`, on the page at `path`, at `at_ms`, with the
    /// `checkpoints` it passed.
    pub fn of(script: &CaseScript, before: i32, path: &str, at_ms: u64, checkpoints: Vec<Checkpoint>) -> Self {
        ReplayStand {
            case_id: script.case_id,
            before,
            script: run_fingerprint(script, before),
            path: path.to_string(),
            at_ms,
            settings: settings_fingerprint(script),
            checkpoints,
            unclean: None,
        }
    }

    /// The same stand with what its whole replay did (`Marks::unclean`).
    pub fn with_unclean(mut self, unclean: Option<Unclean>) -> Self {
        self.unclean = unclean;
        self
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

/// A fingerprint of the script-level settings alone, whatever the case: its
/// account, area, save mode, preconditions, setup, and the dialog and
/// page-error choices. The case, its title and its steps are left out, as
/// is the bookkeeping `run_fingerprint` leaves out.
pub fn settings_fingerprint(script: &CaseScript) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut shaped = script.clone();
    shaped.case_id = 0;
    shaped.title = String::new();
    shaped.steps = Vec::new();
    shaped.repairs = 0;
    shaped.last_repair = None;
    shaped.precondition_repairs = Vec::new();
    shaped.saved_at = None;
    shaped.suspected_defect = None;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&shaped).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// A fingerprint of the actions of the case's steps up to `k`, in run
/// order. Actions only: never a step's expected text.
pub fn steps_fingerprint(script: &CaseScript, k: i32) -> u64 {
    use std::hash::{Hash, Hasher};
    let steps: Vec<(i32, &Vec<crate::browser::actions::Action>)> =
        script.case_steps().filter(|s| s.step_number <= k).map(|s| (s.step_number, &s.actions)).collect();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&steps).unwrap_or_default().hash(&mut h);
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
    /// From step `k` of another case, `case_id`, whose first `k` steps are
    /// this case's too: the browser stood after them, so only steps `k + 1`
    /// to N - 1 run.
    CarriedOnFrom { case_id: i32, k: i32 },
}

impl ReplayFrom {
    /// Said before the replay's own sentence.
    pub fn said(self, case_id: i32, step: i32) -> String {
        match self {
            ReplayFrom::Beginning => "Started from the beginning.".to_string(),
            ReplayFrom::CarriedOn(k) => format!("Carried on from step {k}."),
            ReplayFrom::CarriedOnFrom { case_id, k } => {
                format!("Carried on from step {k} of case {case_id} (the same first {k} steps).")
            }
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

/// Where a replay of another case than the stand's starts: carried on from
/// the last checkpoint of the stand's replay whose first steps are the same
/// in every action, that saved nothing and left the page as it is now
/// (`now` is `page_state`); otherwise from the beginning. The two cases
/// must have the same settings (`settings_fingerprint`), the new one no
/// setup, and the stand be younger than `STAND_FRESH_MS`. The stand's whole
/// replay must also have changed nothing a carried-on case could meet
/// (`ReplayStand::unclean`).
pub fn carry_across(
    stand: Option<&ReplayStand>,
    script: &CaseScript,
    step: i32,
    now: &(String, String, u32),
    now_ms: u64,
) -> ReplayFrom {
    carry_across_why(stand, script, step, now, now_ms).0
}

/// `carry_across`, and, when the first steps matched and the page is as it
/// was but the stand's replay did something that could have changed state,
/// the sentence that says why the new replay starts from the beginning.
pub fn carry_across_why(
    stand: Option<&ReplayStand>,
    script: &CaseScript,
    step: i32,
    now: &(String, String, u32),
    now_ms: u64,
) -> (ReplayFrom, Option<String>) {
    let Some(s) = stand else {
        return (ReplayFrom::Beginning, None);
    };
    if s.case_id == script.case_id
        || script.setup.is_some()
        || s.settings != settings_fingerprint(script)
        || now_ms.saturating_sub(s.at_ms) >= STAND_FRESH_MS
    {
        return (ReplayFrom::Beginning, None);
    }
    let matching: Vec<&Checkpoint> = s
        .checkpoints
        .iter()
        .filter(|cp| {
            cp.k < step
                && cp.prefix == steps_fingerprint(script, cp.k)
                && (cp.path.as_str(), cp.tab.as_str(), cp.dialogs) == (now.0.as_str(), now.1.as_str(), now.2)
        })
        .collect();
    if let (Some(why), Some(best)) = (s.unclean, matching.iter().max_by_key(|cp| cp.k)) {
        return (ReplayFrom::Beginning, Some(why.why(s.case_id, best.wrote)));
    }
    let from = matching
        .iter()
        .filter(|cp| !cp.wrote)
        .map(|cp| cp.k)
        .max()
        .map_or(ReplayFrom::Beginning, |k| ReplayFrom::CarriedOnFrom { case_id: s.case_id, k });
    (from, None)
}

/// What a new stand inherits of the old one's `unclean`, for a replay that
/// started `from`: nothing from the beginning; everything otherwise (a
/// carry-on still has the earlier steps behind it).
pub fn unclean_kept(from: ReplayFrom, old: Option<&ReplayStand>) -> Option<Unclean> {
    match from {
        ReplayFrom::Beginning => None,
        _ => old.and_then(|s| s.unclean),
    }
}

/// The checkpoints of the old stand a replay that started `from` keeps for
/// its own new stand, before the ones it marked itself: none from the
/// beginning; those before K for a same-case carry from K; all of them when
/// nothing ran; those up to `k` for a carry across (their prefix is the new
/// case's too).
pub fn checkpoints_kept(from: ReplayFrom, old: Option<&ReplayStand>) -> Vec<Checkpoint> {
    let Some(s) = old else {
        return Vec::new();
    };
    match from {
        ReplayFrom::Beginning => Vec::new(),
        ReplayFrom::CarriedOn(k) => s.checkpoints.iter().filter(|c| c.k < k).cloned().collect(),
        ReplayFrom::AlreadyThere => s.checkpoints.clone(),
        ReplayFrom::CarriedOnFrom { k, .. } => s.checkpoints.iter().filter(|c| c.k <= k).cloned().collect(),
    }
}

/// Whether anything before step `from` of a same-case carry-on saved: the
/// last checkpoint the old stand has before it says. A first step has
/// nothing before it; a missing checkpoint anywhere else cannot tell, so it
/// says yes.
pub fn wrote_before(old: Option<&ReplayStand>, from: i32) -> bool {
    match old.and_then(|s| s.checkpoints.iter().filter(|c| c.k < from).max_by_key(|c| c.k)) {
        Some(c) => c.wrote,
        None => from > 1,
    }
}

/// Carry case `req.case_id` on in `d` from the page before step `from`,
/// where its last replay left it, to the page before step `req.step`: only
/// steps `from` to `req.step` - 1 run, each with its fixture values filled
/// in as a person's step is (`setup::resolve_step`). The case's no-save
/// guard is armed first, as at the start of every replay (`guard_for_case`,
/// never lifting one held for another case). No preconditions,
/// setup, sign-in or trip to the area: the browser is already past them.
/// `wrote_before` is whether anything before `from` saved, and `marks`
/// learns back the checkpoints of the steps it ran (`run_steps`).
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
    replay: &ReplayState,
    mut progress: impl FnMut(i32, i32),
    tabs: &mut Vec<ReplayedTab>,
    wrote_before: bool,
    marks: &mut Marks,
) -> ReplayEnd {
    marks.clear();
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
        replay,
        &mut progress,
        tabs,
        guarding_other,
        marks,
        wrote_before,
    )
    .await
    {
        return stopped;
    }
    ReplayEnd::Ready { case_id: id, step: n, notice: None, whole: whole_case(&script, n), cleanup: cleanup_ran(&script, n) }
}
