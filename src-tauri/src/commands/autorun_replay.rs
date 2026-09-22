//! The unattended run's IPC shell: real browsers for the replay engine,
//! progress as events, one run at a time, and a way to stop.
//!
//! NOTHING here calls Azure DevOps.

use crate::autorun::replay::{self, Browsers};
use crate::autorun::{sessions, store, LocalRun};
use crate::browser::cdp::Cdp;
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
/// per step within a case) by `autorun::replay::run_selection`.
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

/// The default timing, minus the point-and-pause highlight when nobody is
/// watching: a background browser has no screen for it to be seen on, and
/// waiting `highlight_ms` before every action would only slow the run down.
pub fn replay_timing(watch: bool) -> Timing {
    let t = Timing::default();
    if watch { t } else { Timing { highlight_ms: 0, ..t } }
}

/// A case from the frontend's selection: enough to run it (`case_id`) and
/// enough to report on it before its script has even loaded (`title`).
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct ReplayCase {
    pub case_id: i32,
    pub title: String,
}

/// The `Browsers` the command hands to `replay::run_selection`: a fresh
/// real browser per case, headless unless the person asked to watch.
struct RealBrowsers {
    which: Browser,
    watch: bool,
    current: Option<LaunchedBrowser>,
}

impl Browsers for RealBrowsers {
    type D = Cdp;

    async fn open(&mut self) -> Result<Cdp, String> {
        let extra = background_args();
        let browser = launch_with(self.which, if self.watch { &[] } else { &extra })?;
        // The debugging port answers when the browser is ready, which varies
        // with what else the machine is doing. Asking until it does beats a
        // fixed sleep that is either slow or flaky.
        let mut last = String::new();
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            match Cdp::connect(browser.port).await {
                Ok(cdp) => {
                    self.current = Some(browser);
                    return Ok(cdp);
                }
                Err(e) => last = e,
            }
        }
        super::autorun::close_browser(browser);
        Err(format!("it started but never answered: {last}"))
    }

    async fn close(&mut self, d: Cdp) {
        drop(d);
        if let Some(b) = self.current.take() {
            super::autorun::close_browser(b);
        }
    }
}

/// A panic or an early return out of `run_selection` must never leave a
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
/// arrives as `ReplayProgress` events while this is pending.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_replay(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    cases: Vec<ReplayCase>,
    browser_name: String,
    watch: bool,
) -> Result<LocalRun, String> {
    let _claim = OneAtATime::claim().ok_or_else(|| {
        "an unattended run is already going - wait for it, or stop it first".to_string()
    })?;
    CANCEL.store(false, Ordering::SeqCst);

    let root = super::autorun::root(&app)?;
    let mut run = LocalRun {
        id: store::new_run_id(),
        pbi_id,
        started_at: sessions::now_ms().to_string(),
        cases: vec![],
        mode: "unattended".to_string(),
        published: None,
    };

    let pairs: Vec<(i32, String)> = cases.iter().map(|c| (c.case_id, c.title.clone())).collect();
    let timing = replay_timing(watch);
    let mut browsers =
        RealBrowsers { which: Browser::from_name(&browser_name), watch, current: None };

    let outcome = replay::run_selection(
        &mut browsers,
        &root,
        &organization,
        &project,
        &mut run,
        &pairs,
        &timing,
        &CANCEL,
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
        pairs.len(),
    ));

    match outcome {
        Ok(()) => Ok(run),
        Err(e) => Err(format!("the run finished but could not be saved: {e}")),
    }
}

/// Ask the unattended run in progress to stop after the step it is on.
#[tauri::command]
#[specta::specta]
pub fn auto_run_replay_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}
