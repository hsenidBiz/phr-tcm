//! Recording a project's sign-in recipe: a visible browser the person
//! signs in with by hand, a draft of what they did, and a recipe saved
//! only once it has signed in on its own in a fresh browser.
//!
//! It shares the module recorder's one slot (`RecorderClaim`) and its
//! Cancel: one recording or check at a time, never alongside an unattended
//! run or the supervised browser. NOTHING here calls Azure DevOps, and no
//! typed value is ever read - the only text a saved recipe holds that the
//! person wrote is the fixed text they give in the review.

use super::autorun_record::{claim_the_recorder, open_browser, take_cancel_pending, unless_cancelled, RecorderClaim};
use crate::autorun::accounts::{find_account, Account};
use crate::autorun::recipe::{check_start_url, load_recipe, save_recipe, SignInRecipe};
use crate::autorun::recorder::{self, Ended};
use crate::autorun::signin::{redact, sign_in_fresh};
use crate::autorun::signin_recorder::{self as rec, Captured, Draft, FieldChoice, Seen, SignInDraftView, Step};
use crate::browser::cdp::Driver;
use crate::browser::launch::Browser;
use crate::browser::timing::Timing;
use crate::events::RecordingEvent;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri_specta::Event;

/// The listening task: it owns the recording browser and the recorder's
/// claim, and lets both go when it ends.
pub type Listening = tokio::task::JoinHandle<Captured>;

/// An open sign-in recording.
pub struct SignInRecording {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    pick: Arc<AtomicBool>,
    task: Listening,
    about: SignInFor,
}

impl SignInRecording {
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    /// Cancel: the task closes the browser and frees the recorder.
    pub async fn end(self) {
        self.cancel.store(true, Ordering::SeqCst);
        let _ = self.task.await;
    }
}

/// What a sign-in recording is for.
#[derive(Debug, Clone, PartialEq)]
pub struct SignInFor {
    pub organization: String,
    pub project: String,
    pub start_url: String,
}

pub(crate) static CURRENT: tokio::sync::Mutex<Option<SignInRecording>> = tokio::sync::Mutex::const_new(None);

/// The finished recording, between Finish and a save that works. Only
/// locators and the start address - nothing a person typed.
static DRAFT: std::sync::Mutex<Option<Draft>> = std::sync::Mutex::new(None);

fn draft_slot() -> std::sync::MutexGuard<'static, Option<Draft>> {
    DRAFT.lock().unwrap_or_else(|p| p.into_inner())
}

/// The draft a save would use, if there is one.
pub fn current_draft() -> Option<Draft> {
    draft_slot().clone()
}

/// Kept for the save. Stop puts one here; tests put one directly.
pub fn keep_draft(draft: Draft) {
    *draft_slot() = Some(draft);
}

pub fn forget_draft() {
    *draft_slot() = None;
}

pub const CHECK_CANCELLED: &str = "the check was cancelled - the recipe was not saved";
pub const NO_DRAFT: &str = "there is no recorded sign-in to save - record one first";
pub const OTHER_PROJECT: &str = "this sign-in was recorded for another project - record it again";
const NOTHING_RECORDING: &str = "the sign-in is not being recorded";
const RECORDER_FELL_OVER: &str =
    "the recorder stopped unexpectedly - nothing was saved; see Settings, Logs for the details";

/// A sign-in recording whose browser was closed has ended on its own (and
/// freed the recorder); its leftovers must not stand in for a new one.
async fn drop_a_finished_recording() {
    let mut slot = CURRENT.lock().await;
    if slot.as_ref().is_some_and(SignInRecording::is_finished) {
        *slot = None;
    }
}

/// Whether a sign-in recording is open (its browser may have closed since).
pub async fn sign_in_recording_is_open() -> bool {
    CURRENT.lock().await.is_some()
}

/// Start's last step, once its browser is at the start address and
/// listening. A Cancel that came in meanwhile wins: `start_listening` is
/// never called and is dropped with whatever it owns, which closes the
/// browser - and so is the claim. Decided under this slot's lock, which
/// Cancel holds too while it decides.
pub async fn open_the_recording(
    claim: RecorderClaim,
    about: SignInFor,
    start_listening: impl FnOnce(RecorderClaim, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>) -> Listening,
) -> Result<(), String> {
    let mut slot = CURRENT.lock().await;
    if take_cancel_pending() {
        crate::applog::info("Auto-run sign-in recording cancelled while it was opening");
        return Err(recorder::CANCELLED.to_string());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    let pick = Arc::new(AtomicBool::new(false));
    let task = start_listening(claim, stop.clone(), cancel.clone(), pick.clone());
    *slot = Some(SignInRecording { stop, cancel, pick, task, about });
    Ok(())
}

fn event(kind: &str, index: u32, readable: String, detail: String, password: bool) -> RecordingEvent {
    RecordingEvent { kind: kind.into(), index, readable, detail, password }
}

/// The recording itself, once its browser is listening: every step, the
/// marker and every note is told to `tell` as a `RecordingEvent` (locator
/// words only), until Stop, Cancel or a closed browser - which is told too.
pub async fn listen<D: Driver>(
    d: &mut D,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    pick: &AtomicBool,
    tell: &mut (dyn FnMut(RecordingEvent) + Send),
) -> Captured {
    let captured = rec::capture(d, stop, cancel, pick, &mut |seen| {
        tell(match seen {
            Seen::Step(n, Step::Click(t)) => event("click", n, t.describe(), String::new(), false),
            Seen::Step(n, Step::Field { target, password }) => {
                event("field", n, target.describe(), String::new(), *password)
            }
            Seen::Marker(t) => event("marker", 0, t.describe(), String::new(), false),
            Seen::Unreadable(why) => event("unreadable", 0, String::new(), why.to_string(), false),
        })
    })
    .await;
    if captured.ended == Ended::Closed {
        tell(event("closed", 0, String::new(), recorder::BROWSER_CLOSED.to_string(), false));
    }
    captured
}

/// Sign in as `who` with the recorded recipe, through its own steps only.
/// `Err` is the dialog's sentence; the full words go to the log.
pub async fn check_sign_in<D: Driver>(
    d: &mut D,
    root: &Path,
    recipe: &SignInRecipe,
    who: &Account,
    timing: &Timing,
) -> Result<(), String> {
    let out = sign_in_fresh(d, root, recipe, who, timing).await;
    if out.ok {
        return Ok(());
    }
    let detail = redact(&out.detail, who);
    crate::applog::warn(format!("sign-in recording: the check as {} did not sign in: {detail}", who.key));
    Err(redact(&rec::check_failure(&detail, out.harness), who))
}

/// The recipe a save would write: the draft, the review's choices, and the
/// settings kept from the project's own recipe. `Err` is the sentence.
pub fn build_recipe(
    root: &Path,
    organization: &str,
    project: &str,
    fields: &[FieldChoice],
) -> Result<SignInRecipe, String> {
    let draft = current_draft().ok_or_else(|| NO_DRAFT.to_string())?;
    if draft.organization != organization || draft.project != project {
        return Err(OTHER_PROJECT.to_string());
    }
    let existing = load_recipe(root, organization, project)
        .map_err(|e| format!("{e} - so its other settings cannot be kept; fix it with Edit first"))?;
    draft.recipe(fields, existing.as_ref())
}

/// Open a visible browser with nobody signed in, go to `start_url`, and
/// listen. Each step arrives as a `RecordingEvent`.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_sign_in_start(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    start_url: String,
    browser_name: String,
) -> Result<(), String> {
    let start_url = start_url.trim().to_string();
    check_start_url(&start_url)?;
    if organization.trim().is_empty() || project.trim().is_empty() {
        return Err("pick an organization and a project first".to_string());
    }
    drop_a_finished_recording().await;
    let claim = claim_the_recorder().await?;
    forget_draft();
    let which = Browser::from_name(&browser_name);
    let (mut cdp, browser) = open_browser(which, true).await?;
    rec::prepare(&mut cdp, &start_url, &Timing::default()).await?;

    let about = SignInFor { organization, project, start_url };
    open_the_recording(claim, about, move |claim, stop, cancel, pick| {
        tokio::spawn(async move {
            let captured = listen(&mut cdp, &stop, &cancel, &pick, &mut |ev| {
                let _ = ev.emit(&app);
            })
            .await;
            drop(cdp);
            // `Owned`: the recording browser is killed here, before the
            // recorder is free for anything else.
            drop(browser);
            drop(claim);
            captured
        })
    })
    .await?;
    crate::applog::info("Auto-run sign-in recording started");
    Ok(())
}

/// Pick mode: the next click in the recording browser is the signed-in
/// check, and is not carried out. Again picks again.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_sign_in_pick() -> Result<(), String> {
    let slot = CURRENT.lock().await;
    match slot.as_ref() {
        Some(r) if !r.is_finished() => {
            r.pick.store(true, Ordering::SeqCst);
            Ok(())
        }
        _ => Err(NOTHING_RECORDING.to_string()),
    }
}

/// Close the recording browser and hand back the draft in words. The
/// locators stay here for the save.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_sign_in_stop() -> Result<SignInDraftView, String> {
    let recording = CURRENT.lock().await.take().ok_or_else(|| NOTHING_RECORDING.to_string())?;
    recording.stop.store(true, Ordering::SeqCst);
    let SignInRecording { task, about, .. } = recording;
    let captured = task.await.map_err(|e| {
        crate::applog::warn(format!("sign-in recording: the recorder stopped unexpectedly: {e}"));
        RECORDER_FELL_OVER.to_string()
    })?;
    let (steps, marker) = rec::finish(captured)?;
    let draft =
        Draft { organization: about.organization, project: about.project, start_url: about.start_url, steps, marker };
    let view = draft.view();
    crate::applog::info(format!(
        "Auto-run sign-in recorded ({} steps, {})",
        draft.steps.len(),
        if draft.marker.is_some() { "with a signed-in check" } else { "no signed-in check yet" }
    ));
    keep_draft(draft);
    Ok(view)
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SignInSaveResult {
    pub saved: bool,
    /// Why nothing was saved; empty when `saved`.
    pub failure: String,
}

/// Build the recipe from the draft and the review's field choices, sign in
/// with it as `account` in a fresh background browser, and save it only if
/// that works. A failed check keeps the draft, so the choices can change
/// and be saved again without recording again.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_sign_in_save(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    account: String,
    browser_name: String,
    fields: Vec<FieldChoice>,
) -> Result<SignInSaveResult, String> {
    let root = super::autorun::root(&app)?;
    let out = save_checked(&root, &organization, &project, &account, &browser_name, &fields).await;
    Ok(match out {
        Ok(()) => SignInSaveResult { saved: true, failure: String::new() },
        Err(failure) => SignInSaveResult { saved: false, failure },
    })
}

async fn save_checked(
    root: &Path,
    organization: &str,
    project: &str,
    account: &str,
    browser_name: &str,
    fields: &[FieldChoice],
) -> Result<(), String> {
    let recipe = build_recipe(root, organization, project, fields)?;
    let who = find_account(root, account)?.ok_or_else(|| {
        format!("there is no account \"{account}\" on this machine - add it in Auto Run, Accounts")
    })?;
    drop_a_finished_recording().await;
    let _claim = claim_the_recorder().await?;
    let which = Browser::from_name(browser_name);
    let (mut cdp, browser) = open_browser(which, false).await?;
    let out = unless_cancelled(
        check_sign_in(&mut cdp, root, &recipe, &who, &super::autorun_replay::replay_timing(false)),
        CHECK_CANCELLED,
    )
    .await;
    drop(cdp);
    // `Owned`: the check's browser is killed here whichever way it ended.
    drop(browser);
    if let Err(why) = out {
        crate::applog::info(format!("Auto-run recorded sign-in not saved: {}", redact(&why, &who)));
        return Err(why);
    }
    save_recipe(root, organization, project, &recipe)?;
    forget_draft();
    crate::applog::info(format!("Auto-run sign-in recipe recorded and saved ({} steps)", recipe.steps.len()));
    Ok(())
}
