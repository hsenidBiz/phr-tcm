//! The unattended run's IPC shell: real browsers for the replay engine,
//! progress as events, one run at a time, and a way to stop.
//!
//! NOTHING here calls Azure DevOps.

use crate::api_templates::held::{HeldBrowser, Keeps};
use crate::autorun::one_browser::{Launcher, OneBrowser};
use crate::autorun::plan::Reset;
use crate::autorun::replay::{self, Browsers, CaseToRun};
use crate::autorun::reset_wait::{self, AppGate};
use crate::autorun::{sessions, store, LocalRun};
use crate::browser::cdp::{Cdp, WsTransport};
use crate::browser::launch::{background_args, launch_with, Browser, LaunchedBrowser};
use crate::browser::timing::Timing;
use crate::events::ReplayProgress;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri_specta::Event;

/// Whether an unattended run is currently going. `OneAtATime::claim` is the
/// only way to flip it on; dropping the claim is the only way to flip it
/// off, so a run that panics, errors out or is cancelled still frees the
/// slot the moment its stack unwinds.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Asked to stop by `auto_run_replay_cancel`; read once per case (and once
/// per step within a case) by `autorun::replay::run_cases`.
static CANCEL: AtomicBool = AtomicBool::new(false);

pub struct OneAtATime(());

impl OneAtATime {
    pub fn claim() -> Option<OneAtATime> {
        RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| OneAtATime(()))
    }
}

impl Drop for OneAtATime {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Whether an unattended run is going right now. `auto_run_open_browser`
/// checks this before it will open a supervised browser (F7): the two
/// kinds of session must never run at once, and this is the pure half of
/// that check - testable without an `AppHandle`, unlike the SESSION side
/// of the exclusion (see `auto_run_replay` below).
pub fn replay_is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// The default timing, minus the point-and-pause highlight when nobody is
/// watching: a background browser has no screen for it to be seen on, and
/// waiting `highlight_ms` before every action would only slow the run down.
pub fn replay_timing(watch: bool) -> Timing {
    Timing::watched(watch && crate::app_settings::current().autorun_highlight)
}

/// A case from the frontend's selection: enough to run it (`case_id`),
/// enough to report on it before its script has even loaded (`title`), and
/// its Module field for the module paths.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct ReplayCase {
    pub case_id: i32,
    pub title: String,
    /// Absent or blank when the test case has no Module.
    #[serde(default)]
    pub module: Option<String>,
}

/// What a person reads when a browser started and never answered.
const NEVER_ANSWERED: &str =
    "it started but never answered - try again, and see Settings, Logs if it keeps happening";

/// Start a browser and wait until its DevTools port answers. Used by the
/// module recorder and by `RealBrowsers`.
pub(crate) async fn open_real(which: Browser, visible: bool) -> Result<(Cdp, LaunchedBrowser), String> {
    let extra = background_args();
    let browser = launch_with(which, if visible { &[] } else { &extra })?;
    // The debugging port answers when the browser is ready, which varies
    // with what else the machine is doing. Asking until it does beats a
    // fixed sleep that is either slow or flaky.
    let mut last = String::new();
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        match Cdp::connect(browser.port).await {
            Ok(cdp) => return Ok((cdp, browser)),
            Err(e) => last = e,
        }
    }
    super::autorun::close_browser(browser);
    // The connection error names the local DevTools address, which is
    // nothing a person can act on: it goes to the log instead.
    crate::applog::warn(format!("Auto-run browser never answered: {last}"));
    Err(NEVER_ANSWERED.to_string())
}

/// How an unattended run starts its one browser (`OneBrowser`): headless
/// unless the person asked to watch, with a throwaway profile that is
/// deleted when the browser is closed.
pub(crate) struct RealLauncher {
    which: Browser,
    watch: bool,
}

impl Launcher for RealLauncher {
    type Process = LaunchedBrowser;
    type T = WsTransport;

    async fn launch(&mut self) -> Result<LaunchedBrowser, String> {
        let extra = background_args();
        let browser = launch_with(self.which, if self.watch { &[] } else { &extra })?;
        // Asked until it answers, as `open_real` does.
        let mut last = String::new();
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            match Cdp::answers(browser.port).await {
                Ok(()) => return Ok(browser),
                Err(e) => last = e,
            }
        }
        super::autorun::close_browser(browser);
        crate::applog::warn(format!("Auto-run browser never answered: {last}"));
        Err(NEVER_ANSWERED.to_string())
    }

    async fn connect(&mut self, p: &LaunchedBrowser) -> Result<Cdp<WsTransport>, String> {
        Cdp::connect_browser(p.port).await
    }

    fn alive(&mut self, p: &mut LaunchedBrowser) -> bool {
        matches!(p.child.try_wait(), Ok(None))
    }

    fn close(&mut self, p: LaunchedBrowser) {
        super::autorun::close_browser(p);
    }
}

/// The `Browsers` an unattended run hands to `replay::run_cases`: one real
/// browser for the run, a fresh context per case.
pub(crate) fn run_browsers(which: Browser, watch: bool) -> OneBrowser<RealLauncher> {
    OneBrowser::new(RealLauncher { which, watch })
}

/// A fresh real browser per `open`, headless unless asked to watch. The API
/// template runner and a supervised case's setup open their browser
/// through this.
pub(crate) struct RealBrowsers {
    which: Browser,
    watch: bool,
    current: Option<LaunchedBrowser>,
}

impl RealBrowsers {
    pub(crate) fn new(which: Browser, watch: bool) -> Self {
        RealBrowsers { which, watch, current: None }
    }
}

impl Browsers for RealBrowsers {
    type D = Cdp;

    async fn open(&mut self) -> Result<Cdp, String> {
        let (mut cdp, browser) = open_real(self.which, self.watch).await?;
        self.current = Some(browser);
        // So a case that cannot reach its module can say what the page was
        // doing. Losing that is no reason not to run the case.
        if let Err(e) = crate::browser::page_log::watch(&mut cdp).await {
            crate::applog::warn(format!("unattended run: the page log could not be switched on: {e}"));
        }
        Ok(cdp)
    }

    async fn close(&mut self, d: Cdp) {
        drop(d);
        if let Some(b) = self.current.take() {
            super::autorun::close_browser(b);
        }
    }
}

/// A browser kept signed in between API template runs
/// (`api_templates::held`): the connection with the process it drives, so
/// it outlives the `RealBrowsers` that opened it.
pub(crate) struct KeptBrowser {
    cdp: Cdp,
    browser: LaunchedBrowser,
    /// Edge or Chrome: reused only by a run that asked for the same.
    which: Browser,
}

impl HeldBrowser for KeptBrowser {
    fn close(self) {
        drop(self.cdp);
        super::autorun::close_browser(self.browser);
    }
}

impl Keeps for RealBrowsers {
    type Kept = KeptBrowser;

    /// Takes the process out of `current`, so dropping this value no
    /// longer ends it. One that has already ended stays to be closed.
    fn keep(&mut self, d: Cdp) -> Result<KeptBrowser, Cdp> {
        let alive = self.current.as_mut().is_some_and(|b| matches!(b.child.try_wait(), Ok(None)));
        match self.current.take() {
            Some(browser) if alive => Ok(KeptBrowser { cdp: d, browser, which: self.which }),
            other => {
                self.current = other;
                Err(d)
            }
        }
    }

    /// Puts the process back in `current`: from here on `close`, or a
    /// panic dropping this value, ends it like one this value opened. One
    /// whose process has ended while kept, or another kind of browser than
    /// this value opens, is handed back to be closed.
    fn adopt(&mut self, mut kept: KeptBrowser) -> Result<Cdp, KeptBrowser> {
        if !self.same_kind(&kept) || !matches!(kept.browser.child.try_wait(), Ok(None)) {
            return Err(kept);
        }
        if let Some(old) = self.current.replace(kept.browser) {
            super::autorun::close_browser(old);
        }
        Ok(kept.cdp)
    }

    fn same_kind(&self, kept: &KeptBrowser) -> bool {
        kept.which == self.which
    }
}

/// A panic or an early return out of `run_cases` must never leave a
/// browser process behind: if `current` is still `Some` when this value
/// drops, nothing else is ever going to close it.
impl Drop for RealBrowsers {
    fn drop(&mut self) {
        if let Some(b) = self.current.take() {
            super::autorun::close_browser(b);
        }
    }
}

/// Run the selection unattended and return the finished run. Progress
/// arrives as `ReplayProgress` events while this is pending. `account`
/// signs in every case, over the account a script names (null leaves each
/// script to its own); it must be a key in the Accounts list, or the run
/// does not start. `retry_transient` runs a case whose failure looked
/// transient once more, in a fresh browser context (`autorun::transient`).
/// The run keeps one browser, with a context per case (`one_browser`).
/// `db_read_access` is the AI Bridge tab's Database Read Access switch:
/// while it is off no precondition is checked (`autorun::preconditions`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn auto_run_replay(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    cases: Vec<ReplayCase>,
    account: Option<String>,
    browser_name: String,
    watch: bool,
    retry_transient: bool,
    db_read_access: bool,
) -> Result<LocalRun, String> {
    let _claim = OneAtATime::claim().ok_or_else(|| {
        "an unattended run is already going - wait for it, or stop it first".to_string()
    })?;
    if let Some(busy) = super::autorun::open_session_refusal().await {
        return Err(busy);
    }
    super::autorun_record::refuse_while_recording()?;
    CANCEL.store(false, Ordering::SeqCst);

    let root = super::autorun::root(&app)?;
    // The browser picked in the dialog, remembered for a replay to a step
    // that finds no supervised browser open.
    store::remember_browser(&root, &browser_name);
    let run_account = crate::autorun::accounts::account_for_run(&root, account.as_deref())?;
    let mut run = LocalRun {
        id: store::new_run_id(),
        pbi_id,
        started_at: sessions::now_ms().to_string(),
        cases: vec![],
        mode: "unattended".to_string(),
        published: None,
        // A switch is refused while this run holds its slot, so this is the
        // environment every case of it signs in to.
        environment: crate::environments::active(&root).ok().map(|e| e.name),
        resets: vec![],
    };

    let sent: Vec<CaseToRun> = cases
        .iter()
        .map(|c| CaseToRun { case_id: c.case_id, title: c.title.clone(), module: c.module.clone() })
        .collect();
    // The order and its reset points, worked out again here from the
    // scripts on disk and the PBI's own order: the reset points are never
    // taken on the webview's word.
    let (list, resets) = planned_cases(&root, pbi_id, &sent);
    let timing = replay_timing(watch);
    let mut browsers = run_browsers(Browser::from_name(&browser_name), watch);
    // Where the cases' preconditions are asked: nowhere while Database
    // Read Access is off, otherwise the active environment's database,
    // resolved once for the run. A case without preconditions never looks
    // at it, so one that is not set up stops no other case.
    let precondition_db = {
        use tauri::Manager;
        let secrets = std::sync::Arc::clone(&app.state::<crate::db::DbSecrets>().0);
        crate::autorun::preconditions::for_run(&root, Some(secrets.as_ref()), db_read_access)
    };

    let notify = |n: &crate::events::AutorunResetNeeded| {
        let _ = n.clone().emit(&app);
    };
    let mut gate = AppGate { waits: reset_wait::waits(), run_id: run.id.clone(), notify: &notify };
    let outcome = replay::run_cases_planned(
        &mut browsers,
        &root,
        &organization,
        &project,
        &mut run,
        &list,
        run_account.as_deref(),
        retry_transient,
        &timing,
        &CANCEL,
        &precondition_db,
        &resets,
        &mut gate,
        &mut |p: ReplayProgress| {
            let _ = p.emit(&app);
        },
    )
    .await;

    // Counts only - never a case title or a failure detail. See house
    // rules: a password or page-specific detail never reaches the log at
    // this level, and neither does anything that would make this line grow
    // without bound for a big selection.
    let passed = run.cases.iter().filter(|c| c.proposed == "Passed").count();
    let failed = run.cases.iter().filter(|c| c.proposed == "Failed").count();
    let blocked = run.cases.iter().filter(|c| c.proposed == "Blocked").count();
    crate::applog::info(format!(
        "Auto-run unattended: {} cases, proposed {passed} passed / {failed} failed / {blocked} blocked",
        list.len(),
    ));

    // `run_cases` words its own errors: one that stopped the run before it
    // began is not one where every case ran and only the save failed.
    outcome.map(|()| run)
}

/// Ask the unattended run in progress to stop after the step it is on.
#[tauri::command]
#[specta::specta]
pub fn auto_run_replay_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// The selection in the order to run it, and the reset points in that
/// order: the plan worked out from the scripts on disk and the PBI's own
/// order (`autorun::auto_run_plan`'s `plan_at`), whatever order the cases
/// were sent in. Where the two orders differ the plan wins, and the
/// difference is logged (ids only).
pub fn planned_cases(root: &std::path::Path, pbi_id: i32, sent: &[CaseToRun]) -> (Vec<CaseToRun>, Vec<Reset>) {
    let ids: Vec<i32> = sent.iter().map(|c| c.case_id).collect();
    let plan = super::autorun::plan_at(root, pbi_id, &ids, None);
    if plan.order != ids {
        crate::applog::info(format!(
            "Auto-run unattended: the planned order {:?} replaces the order sent {:?}",
            plan.order, ids
        ));
    }
    let list = plan.order.iter().filter_map(|id| sent.iter().find(|c| c.case_id == *id).cloned()).collect();
    (list, plan.resets)
}

/// The person's answer at the reset point run `run_id` is waiting at:
/// Continue (`continue_run`) runs the next phase, Stop ends the run there.
/// Refused when that run is not waiting at a reset point.
#[tauri::command]
#[specta::specta]
pub fn auto_run_answer_reset(run_id: String, continue_run: bool) -> Result<(), String> {
    reset_wait::waits().answer(&run_id, continue_run)
}

/// The reset point the unattended run is waiting at, if any, as the Reset
/// needed panel shows it. A screen opened after the pause began (the person
/// left Auto Run and came back) asks this to show the panel again.
#[tauri::command]
#[specta::specta]
pub fn auto_run_waiting_reset() -> Option<crate::events::AutorunResetNeeded> {
    reset_wait::waits().waiting()
}

/// How long the app's exit waits for a run stopped at a reset point to
/// save itself.
const EXIT_SAVE_WAIT: Duration = Duration::from_secs(3);

/// The app is closing. A run waiting at a reset point is answered Stop, and
/// the exit waits (bounded) for it to record the rest as not run and save
/// itself, so its file is never left mid-run.
pub fn stop_paused_run_on_exit() {
    if !reset_wait::waits().stop_for_exit() {
        return;
    }
    crate::applog::info("Auto-run unattended: the app closed at a reset point - the run was stopped");
    let until = std::time::Instant::now() + EXIT_SAVE_WAIT;
    while replay_is_running() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(25));
    }
}
