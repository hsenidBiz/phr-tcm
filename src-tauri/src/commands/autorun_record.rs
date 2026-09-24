//! Recording a module's menu path: a visible browser the person clicks
//! through, a check in a fresh browser, and the path saved only if that
//! replay lands where the recording did.
//!
//! One recording at a time, never alongside an unattended run or the
//! supervised browser. NOTHING here calls Azure DevOps.

use crate::autorun::accounts::Account;
use crate::autorun::nav::{self, ModulePath};
use crate::autorun::recipe::SignInRecipe;
use crate::autorun::recorder::{self, Captured, Ended};
use crate::autorun::signin;
use crate::browser::cdp::Driver;
use crate::browser::launch::{Browser, LaunchedBrowser};
use crate::browser::timing::Timing;
use crate::events::RecordingEvent;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri_specta::Event;

static RECORDING: AtomicBool = AtomicBool::new(false);

/// Held for as long as a recording (or a Try) is going. Dropping it is the
/// only way to free the slot, so an error or a panic frees it too.
pub struct RecorderClaim(());

impl RecorderClaim {
    pub fn claim() -> Option<RecorderClaim> {
        RECORDING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| RecorderClaim(()))
    }
}

impl Drop for RecorderClaim {
    fn drop(&mut self) {
        RECORDING.store(false, Ordering::SeqCst);
    }
}

pub fn recording_is_going() -> bool {
    RECORDING.load(Ordering::SeqCst)
}

pub const ALREADY_RECORDING: &str = "a module path is already being recorded - finish or cancel it first";
/// Said by an unattended run and the supervised browser while recording.
pub const RECORDING_BUSY: &str = "a module path is being recorded - finish or cancel it first";

const WOULD_NOT_LISTEN: &str =
    "the browser would not report clicks - try again, and see Settings, Logs if it keeps happening";
const RECORDER_FELL_OVER: &str =
    "the recorder stopped unexpectedly - nothing was saved; see Settings, Logs for the details";

pub fn refuse_while_recording() -> Result<(), String> {
    if recording_is_going() {
        Err(RECORDING_BUSY.to_string())
    } else {
        Ok(())
    }
}

/// The run and the supervised browser. The sentences match the ones those
/// two already use for each other.
async fn refuse_other_sessions() -> Result<(), String> {
    if crate::commands::autorun_replay::replay_is_running() {
        return Err("an unattended run is going - wait for it, or stop it first".to_string());
    }
    if crate::commands::autorun::supervised_session_is_open().await {
        return Err("close the supervised browser first".to_string());
    }
    Ok(())
}

/// Whether a recording (or a Try) may start now.
pub async fn refuse_to_record_now() -> Result<(), String> {
    refuse_other_sessions().await?;
    if recording_is_going() {
        return Err(ALREADY_RECORDING.to_string());
    }
    Ok(())
}

/// The slot first, then the others: the run claims its slot and then looks
/// for a recording, so taking ours before looking for it means two that
/// start at the same moment cannot both miss each other.
async fn claim_the_recorder() -> Result<RecorderClaim, String> {
    let claim = RecorderClaim::claim().ok_or_else(|| ALREADY_RECORDING.to_string())?;
    refuse_other_sessions().await?;
    Ok(claim)
}

/// A browser this module started, killed when this drops - on the way out
/// of an error or a panic as much as at the end.
struct Owned(Option<LaunchedBrowser>);

impl Drop for Owned {
    fn drop(&mut self) {
        if let Some(b) = self.0.take() {
            super::autorun::close_browser(b);
        }
    }
}

/// `open_real`'s words name no address (the one that would goes to the
/// log there), and "Edge is not installed" is worth passing on as it is.
async fn open_browser(which: Browser, visible: bool) -> Result<(crate::browser::cdp::Cdp, Owned), String> {
    let (cdp, browser) =
        super::autorun_replay::open_real(which, visible).await.map_err(|e| format!("the browser did not open: {e}"))?;
    Ok((cdp, Owned(Some(browser))))
}

struct Recording {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    /// The claim comes back with the clicks unless the browser was closed.
    task: tokio::task::JoinHandle<(Captured, Option<RecorderClaim>)>,
    organization: String,
    project: String,
    module: String,
    account: String,
    which: Browser,
}

static CURRENT: tokio::sync::Mutex<Option<Recording>> = tokio::sync::Mutex::const_new(None);

/// A recording whose browser was closed has ended on its own (and already
/// freed the slot); its leftovers must not stand in for a new one.
async fn drop_a_finished_recording() {
    let mut slot = CURRENT.lock().await;
    if slot.as_ref().is_some_and(|r| r.task.is_finished()) {
        *slot = None;
    }
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ModuleRecordResult {
    pub saved: bool,
    pub module: String,
    /// Why nothing was saved; empty when `saved`.
    pub failure: String,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ModuleTryResult {
    pub ok: bool,
    pub detail: String,
}

fn now_iso() -> String {
    crate::commands::queue::iso_utc((crate::autorun::sessions::now_ms() / 1000) as i64)
}

/// Sign in, go home, and start listening. The sign-in's own words can name
/// the application's address, so the person gets the same fixed sentences
/// the check gives and the words go to the log.
pub async fn prepare_to_record<D: Driver>(
    d: &mut D,
    root: &Path,
    recipe: &SignInRecipe,
    who: &Account,
    timing: &Timing,
) -> Result<(), String> {
    let signed = signin::sign_in(d, root, recipe, who, timing).await;
    if !signed.ok {
        crate::applog::warn(format!(
            "module recording: signing in as {} did not work: {}",
            who.key,
            signin::redact(&signed.detail, who)
        ));
        return Err(if signed.harness { nav::SIGN_IN_BROWSER_SILENT } else { nav::SIGN_IN_FAILED }.to_string());
    }
    let home = nav::go_home(d, &recipe.start_url, &recipe.origins(), timing).await;
    if !home.ok {
        // `go_home` words its failures without the address.
        return Err(format!("the home page did not open: {}", home.detail));
    }
    recorder::arm(d).await.map_err(|e| {
        crate::applog::warn(format!("module recording: the listener could not be added: {e}"));
        WOULD_NOT_LISTEN.to_string()
    })
}

/// The recording itself, once its browser is listening: every click the
/// page reports is told to `tell` as a `RecordingEvent` (locator words
/// only), until Stop, Cancel or a closed browser. A closed browser ends the
/// recording with nothing to save, so its claim is dropped here and the
/// slot is free at once; otherwise the claim comes back for the caller to
/// hold through the check.
pub async fn listen<D: Driver>(
    d: &mut D,
    claim: RecorderClaim,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    tell: &mut (dyn FnMut(RecordingEvent) + Send),
) -> (Captured, Option<RecorderClaim>) {
    let mut index = 0u32;
    let captured = recorder::capture(d, stop, cancel, &mut |seen| {
        tell(match seen {
            Ok(t) => {
                index += 1;
                RecordingEvent { kind: "click".into(), index, readable: t.describe(), detail: String::new() }
            }
            Err(why) => RecordingEvent { kind: "unreadable".into(), index: 0, readable: String::new(), detail: why.to_string() },
        })
    })
    .await;
    if captured.ended == Ended::Closed {
        drop(claim);
        tell(RecordingEvent {
            kind: "closed".into(),
            index: 0,
            readable: String::new(),
            detail: recorder::BROWSER_CLOSED.to_string(),
        });
        return (captured, None);
    }
    (captured, Some(claim))
}

/// Sign in fresh in a background browser and walk the path.
async fn check_in_fresh_browser(
    root: &Path,
    organization: &str,
    project: &str,
    account: &str,
    which: Browser,
    path: &ModulePath,
) -> Result<String, String> {
    let (recipe, who) = signin::prepare(root, organization, project, account)?;
    let (mut cdp, browser) = open_browser(which, false).await?;
    let out = nav::check_path(&mut cdp, root, &recipe, &who, path, &super::autorun_replay::replay_timing(false)).await;
    drop(cdp);
    drop(browser);
    out
}

/// Open a visible browser, sign in as `account`, go home, and start
/// listening. Each captured click arrives as a `RecordingEvent`.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_start(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    module: String,
    account: String,
    browser_name: String,
) -> Result<(), String> {
    let module = module.trim().to_string();
    if module.is_empty() {
        return Err("name the module first".to_string());
    }
    drop_a_finished_recording().await;
    let claim = claim_the_recorder().await?;
    let root = super::autorun::root(&app)?;
    let (recipe, who) = signin::prepare(&root, &organization, &project, &account)?;
    let which = Browser::from_name(&browser_name);
    let (mut cdp, browser) = open_browser(which, true).await?;
    prepare_to_record(&mut cdp, &root, &recipe, &who, &Timing::default()).await?;

    let stop = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    let (stop_seen, cancel_seen, events_to) = (stop.clone(), cancel.clone(), app.clone());
    let task = tokio::spawn(async move {
        let out = listen(&mut cdp, claim, &stop_seen, &cancel_seen, &mut |ev| {
            let _ = ev.emit(&events_to);
        })
        .await;
        drop(cdp);
        drop(browser);
        out
    });
    *CURRENT.lock().await = Some(Recording { stop, cancel, task, organization, project, module, account, which });
    crate::applog::info("Auto-run module recording started");
    Ok(())
}

/// Stop, close the recording browser, replay the path in a fresh signed-in
/// browser, and save it only if every click found its one element and the
/// page ended where the recording did.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_stop(app: tauri::AppHandle) -> Result<ModuleRecordResult, String> {
    let rec = CURRENT.lock().await.take().ok_or_else(|| "nothing is being recorded".to_string())?;
    rec.stop.store(true, Ordering::SeqCst);
    let Recording { task, organization, project, module, account, which, .. } = rec;
    let (captured, _claim) = task.await.map_err(|e| {
        crate::applog::warn(format!("module recording: the recorder stopped unexpectedly: {e}"));
        RECORDER_FELL_OVER.to_string()
    })?;
    let path = match recorder::finish(&module, captured, &now_iso()) {
        Ok(p) => p,
        Err(failure) => return Ok(ModuleRecordResult { saved: false, module, failure }),
    };
    let root = super::autorun::root(&app)?;
    match check_in_fresh_browser(&root, &organization, &project, &account, which, &path).await {
        Ok(_) => {
            let clicks = path.clicks.len();
            nav::put_path(&root, &organization, &project, path)?;
            crate::applog::info(format!("Auto-run module path saved ({clicks} clicks)"));
            Ok(ModuleRecordResult { saved: true, module, failure: String::new() })
        }
        Err(failure) => Ok(ModuleRecordResult { saved: false, module, failure }),
    }
}

/// Close the recording browser and save nothing.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_cancel() -> Result<(), String> {
    let rec = CURRENT.lock().await.take();
    if let Some(rec) = rec {
        rec.cancel.store(true, Ordering::SeqCst);
        // The task closes the browser before it gives the claim back, and
        // the claim is dropped with this result.
        let _ = rec.task.await;
    }
    Ok(())
}

/// The same check a recording must pass, on a saved path.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_try_module_path(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    module: String,
    account: String,
    browser_name: String,
) -> Result<ModuleTryResult, String> {
    drop_a_finished_recording().await;
    let _claim = claim_the_recorder().await?;
    let root = super::autorun::root(&app)?;
    let nav_file = nav::load_nav(&root, &organization, &project)?;
    let path = nav::find_path(&nav_file, &module).cloned().ok_or_else(|| nav::no_path(&module))?;
    Ok(
        match check_in_fresh_browser(&root, &organization, &project, &account, Browser::from_name(&browser_name), &path).await {
            Ok(arrived) => ModuleTryResult { ok: true, detail: format!("reached {arrived}") },
            Err(detail) => ModuleTryResult { ok: false, detail },
        },
    )
}
