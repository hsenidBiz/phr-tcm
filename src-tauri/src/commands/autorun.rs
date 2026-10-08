//! The supervised runner's IPC surface.
//!
//! NOTHING here calls Azure DevOps - sending a reviewed run there is
//! `commands::autorun_publish`, a separate command reached only by the
//! Send button. The browser session lives for as long as the screen keeps
//! it open; steps run against it one at a time, driven by the human
//! clicking through.

use crate::autorun::store;
use crate::autorun::{CaseScript, LocalRun, StepScript};
use crate::browser::actions::ActionOutcome;
use crate::browser::cdp::{Cdp, Driver};
use crate::browser::launch::{launch_in, Browser, LaunchedBrowser};
use base64::Engine;
use std::path::PathBuf;
use tauri::Manager;

/// The one live session. A second Open replaces the first, so a stray
/// browser can never leave the screen permanently stuck.
static SESSION: tokio::sync::Mutex<Option<Session>> = tokio::sync::Mutex::const_new(None);

pub(crate) struct Session {
    pub(crate) browser: LaunchedBrowser,
    pub(crate) cdp: Cdp,
    /// The account last signed in as, in THIS browser. `None` until a
    /// sign-in succeeds. Written on every sign-in; nothing reads it yet -
    /// unattended replay (a later phase) is what will.
    pub(crate) account: Option<String>,
    /// The account this browser holds (`autorun::lease`), from a sign-in
    /// (one that failed too) until it signs in as another account. It goes with the
    /// session: closing the browser, a second Open replacing it, or the
    /// app exiting all drop it.
    pub(crate) lease: crate::autorun::lease::Held,
    /// The case whose no-save guard this browser holds, if any
    /// (`guard_for_case`).
    pub(crate) guarded_case: Option<i32>,
    /// The Auto Run folder whose `downloads/supervised` this browser saves
    /// into, emptied once it closes. `None` when downloads could not be
    /// switched on.
    pub(crate) downloads_root: Option<PathBuf>,
    /// The case this browser's tabs belong to (`runner::tabs_for_case`):
    /// a step of another case closes every tab but `main` first.
    pub(crate) tabs_case: Option<i32>,
    /// The discovery under way in this browser, if any: what the page
    /// routes record is filed under its area and stamps that area explored.
    pub(crate) discovery: Option<DiscoveryState>,
}

/// A discovery under way in the supervised browser: the area it explores
/// and the account KEY it explores as (never a login). Both are `None` from
/// the moment the browser opens for a discovery until its sign-in arrives.
pub struct DiscoveryState {
    pub area: Option<String>,
    pub account: Option<String>,
    /// When the discovery started (milliseconds since the epoch): a page it
    /// reads again keeps the locators matched since then.
    pub started_at: u64,
}

/// The supervised session, for the bridge's page routes. Whoever locks
/// it holds the browser: keep the critical section to one protocol job.
/// `ai_bridge`'s `/autorun-page`, `/autorun-probe` and `/autorun-try`
/// are its callers - the person's own browser, borrowed for one job.
pub(crate) fn supervised() -> &'static tokio::sync::Mutex<Option<Session>> {
    &SESSION
}

/// Said when a step arrives with no browser behind it. Pulled out so a
/// test can hold the wording to account - "no session" would tell the
/// person nothing about what to do.
pub fn describe_session_error() -> String {
    "no browser is open - press Open browser first".to_string()
}

/// Where scripts and runs live: beside the app's other data.
///
/// Prefers the value app setup published process-wide, because the AI
/// bridge writes through that same value and has no `AppHandle` to derive
/// one from. Two independent derivations of "the autorun directory" is
/// exactly how an assistant's saved script ends up somewhere this screen
/// never looks. The handle is the fallback for any path that runs before
/// setup.
pub fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Some(configured) = store::configured_root() {
        return Ok(configured);
    }
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("autorun"))
}

/// Guards a run id from escaping the runs directory. The rule itself lives
/// in `store::safe_run_id` - `load_run` needs it too, and there is exactly
/// one copy - re-exported here because this command module is the id's
/// first IPC-facing entry point, and existing callers still import it from
/// here.
pub use store::safe_run_id;

/// Whether the one supervised session is currently open. Peeked by
/// `auto_run_replay` (F7): an unattended run and a supervised session
/// must never be open at once, and this is the mutex side of that
/// exclusion - the other direction (`auto_run_open_browser` refusing
/// while an unattended run is going) uses the pure, AppHandle-free
/// `autorun_replay::replay_is_running` instead.
pub async fn supervised_session_is_open() -> bool {
    SESSION.lock().await.is_some()
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_open_browser(app: tauri::AppHandle, browser_name: String) -> Result<(), String> {
    if crate::commands::autorun_replay::replay_is_running() {
        return Err(UNATTENDED_GOING.to_string());
    }
    // A discovery's browser is never replaced: heard before a replay is
    // stopped, and again under the lock. No replay goes beside a discovery
    // (`replay_supervised` refuses), so the lock is free to take.
    if auto_run_discovery_active() {
        return Err(busy_browser_sentence(true).to_string());
    }
    // A replay going in the browser this replaces ends first: it is heard
    // without the session's lock, which the replay holds.
    crate::autorun::replay_to::stop();
    let mut slot = SESSION.lock().await;
    crate::ai_bridge::refuse_while_discovering(&mut slot)?;
    let opened = open_into(root(&app), &mut slot, Browser::from_name(&browser_name)).await;
    // A discovery this replaced is over, whether or not the new one opened.
    publish_discovery(&slot);
    opened?;
    // The person's choice, once it opened, for a replay that finds no
    // browser. Best effort: the browser is open either way.
    match root(&app) {
        Ok(root) => store::remember_browser(&root, &browser_name),
        Err(e) => crate::applog::warn(format!("auto-run: the browser choice could not be remembered: {e}")),
    }
    Ok(())
}

/// Said when a supervised browser is asked for while an unattended run is
/// going.
const UNATTENDED_GOING: &str = "an unattended run is going - wait for it, or stop it first";

/// The supervised session, opened with the browser last chosen
/// (`store::last_browser`) when none is open - the same opening as Open
/// browser. Called with the session lock held.
async fn open_if_none(app: &tauri::AppHandle, slot: &mut Option<Session>) -> Result<(), String> {
    if slot.is_some() {
        return Ok(());
    }
    if crate::commands::autorun_replay::replay_is_running() {
        return Err(UNATTENDED_GOING.to_string());
    }
    let which = Browser::from_name(&store::last_browser(&root(app)?));
    open_into(root(app), slot, which).await
}

/// Said by `open_for_discovery` when the app set up no data directory.
const NO_DATA_DIRECTORY: &str = "the app could not set up its data directory this session - restart the app";

/// What a discovery is refused for before anything is looked at under the
/// session's lock: an unattended run, a recording or check, a replay to a
/// step. A replay holds the lock while it goes, so it is heard here, not
/// waited for.
pub(crate) fn refuse_discovery_while_busy() -> Result<(), String> {
    if crate::commands::autorun_replay::replay_is_running() {
        return Err(UNATTENDED_GOING.to_string());
    }
    crate::commands::autorun_record::refuse_while_recording()?;
    if crate::autorun::replay_to::is_running() {
        return Err(crate::autorun::replay_to::ALREADY_RUNNING.to_string());
    }
    Ok(())
}

/// Open the Auto Run browser for the assistant's discovery, in the browser
/// named (`edge` or `chrome`). Refused while anything else holds the
/// browser - a supervised browser already open included, which a
/// discovery never replaces. The session is marked a discovery from the
/// moment it opens; its area and account are set once the sign-in arrives.
pub(crate) async fn open_for_discovery(browser_name: &str) -> Result<(), String> {
    refuse_discovery_while_busy()?;
    let mut slot = SESSION.lock().await;
    if let Some(open) = slot.as_ref() {
        return Err(busy_browser_sentence(open.discovery.is_some()).to_string());
    }
    let root = store::configured_root().ok_or_else(|| NO_DATA_DIRECTORY.to_string())?;
    open_into(Ok(root), &mut slot, Browser::from_name(browser_name)).await?;
    if let Some(session) = slot.as_mut() {
        session.discovery =
            Some(DiscoveryState { area: None, account: None, started_at: crate::autorun::sessions::now_ms() });
    }
    publish_discovery(&slot);
    Ok(())
}

/// End the discovery under way, closing its browser. A browser the person
/// opened is left alone, and with no discovery going this does nothing.
pub(crate) async fn end_discovery() -> (u16, String) {
    let mut slot = SESSION.lock().await;
    let answer = crate::ai_bridge::end_discovery_in(&mut slot);
    publish_discovery(&slot);
    answer
}

/// End Discovery on Auto Run's Setup card: ends the discovery under way the
/// way the assistant's `end_autorun_discovery` does, closing its browser,
/// and says so to the window. A browser the person opened is left alone,
/// and with no discovery going this does nothing. Nothing is lost: the map
/// keeps what was seen.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_end_discovery() -> Result<(), String> {
    end_discovery().await;
    Ok(())
}

/// Whether a discovery holds the Auto Run browser, as last published. Kept
/// beside the session rather than read from it, so asking never waits on
/// the session lock a replay holds while it goes.
static DISCOVERY_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The window `autorun_discovery_changed` goes to. Set once during app
/// setup: the bridge, which starts and ends discoveries, has no
/// `AppHandle` of its own. Unset in the test binaries, where publishing
/// only moves the flag.
static DISCOVERY_EVENTS: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// Called once during app setup.
pub fn set_discovery_events(app: tauri::AppHandle) {
    let _ = DISCOVERY_EVENTS.set(app);
}

/// Say whether `slot` now holds a discovery, to the window too when that
/// changed. Called, with the session lock held, wherever a discovery can
/// start or end: its open, its end, a failed sign-in closing it, Close
/// browser, and Open browser replacing the session.
pub(crate) fn publish_discovery(slot: &Option<Session>) {
    use std::sync::atomic::Ordering;
    use tauri_specta::Event as _;
    let active = slot.as_ref().is_some_and(|s| s.discovery.is_some());
    if DISCOVERY_ACTIVE.swap(active, Ordering::SeqCst) != active {
        if let Some(app) = DISCOVERY_EVENTS.get() {
            let _ = crate::events::AutorunDiscoveryChanged { active }.emit(app);
        }
    }
}

/// Whether the assistant is exploring the app in the Auto Run browser:
/// Auto Run's runs and Open browser wait while it is.
#[tauri::command]
#[specta::specta]
pub fn auto_run_discovery_active() -> bool {
    DISCOVERY_ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
}

/// One area of the discovery map, as the Discovery card shows it.
#[derive(serde::Serialize, specta::Type, Clone, Debug, PartialEq)]
pub struct AreaView {
    /// `""` is the bucket for what belongs to no area.
    pub area: String,
    /// Milliseconds since the epoch; a JavaScript number holds it exactly.
    #[specta(type = Option<f64>)]
    pub explored_at: Option<u64>,
    pub account: Option<String>,
    pub stale: bool,
    /// Why it is stale, as a sentence; `None` when it is not.
    pub stale_reason: Option<String>,
    pub pages: u32,
    pub elements: u32,
    pub writes: Vec<crate::autorun::discovery_map::WriteEntry>,
}

#[derive(serde::Serialize, specta::Type, Clone, Debug, PartialEq)]
pub struct MapView {
    pub areas: Vec<AreaView>,
}

/// The project's discovery map at `now`, area by area. The bucket for what
/// belongs to no area is never stale: there is nothing to explore again.
pub fn map_view(root: &std::path::Path, org: &str, project: &str, now: u64) -> Result<MapView, String> {
    use crate::autorun::discovery_map::{load_map, stale_reason};
    let map = load_map(root, org, project)?;
    let areas = map
        .areas
        .into_iter()
        .map(|a| {
            let why = if a.area.is_empty() { None } else { stale_reason(&a, now) };
            AreaView {
                stale: why.is_some(),
                stale_reason: why.map(sentence_case),
                pages: a.pages.len() as u32,
                elements: a.pages.iter().map(|p| p.elements.len() as u32).sum(),
                explored_at: a.explored_at,
                account: a.account,
                writes: a.writes,
                area: a.area,
            }
        })
        .collect();
    Ok(MapView { areas })
}

/// `s` with its first letter capitalised.
fn sentence_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What discovery has mapped of this project, for the Discovery card.
#[tauri::command]
#[specta::specta]
pub fn auto_run_load_map(app: tauri::AppHandle, organization: String, project: String) -> Result<MapView, String> {
    map_view(&root(&app)?, &organization, &project, crate::autorun::sessions::now_ms())
}

/// Forget what discovery mapped of `area`. Saved scripts keep running;
/// new saves there need the area explored again.
#[tauri::command]
#[specta::specta]
pub fn auto_run_forget_map_area(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    area: String,
) -> Result<(), String> {
    crate::autorun::discovery_map::forget_area(&root(&app)?, &organization, &project, &area)
}

/// Reset map in the Discovery window, offered when the map cannot be read:
/// the damaged file is moved aside (never deleted) and discovery starts an
/// empty map. Hands back where it was moved, project-relative.
#[tauri::command]
#[specta::specta]
pub fn auto_run_reset_map(app: tauri::AppHandle, organization: String, project: String) -> Result<Option<String>, String> {
    crate::autorun::discovery_map::reset_map(&root(&app)?, &organization, &project, crate::autorun::sessions::now_ms())
}

/// Said when something wants the browser a session holds. A discovery's
/// names the assistant's own way out.
pub fn busy_browser_sentence(discovering: bool) -> &'static str {
    if discovering {
        "the assistant is exploring the app in the Auto Run browser - end it with end_autorun_discovery, or press End discovery in Auto Run, Setup, first"
    } else {
        "close the supervised browser first"
    }
}

/// The sentence for a browser a session holds, or `None` when none is
/// open.
pub async fn open_session_refusal() -> Option<String> {
    SESSION.lock().await.as_ref().map(|s| busy_browser_sentence(s.discovery.is_some()).to_string())
}

impl crate::ai_bridge::DiscoveryBrowser for Session {
    type D = Cdp;
    fn parts(&mut self) -> crate::ai_bridge::DiscoveryParts<'_, Cdp> {
        crate::ai_bridge::DiscoveryParts {
            driver: &mut self.cdp,
            lease: &mut self.lease,
            signed_in: &mut self.account,
            discovery: &mut self.discovery,
        }
    }
    fn close(self) {
        close_session(self);
    }
}

/// Open the supervised browser into `slot`, closing one already there.
/// `root` is the Auto Run folder its downloads go under. Called with the
/// session lock held.
async fn open_into(root: Result<PathBuf, String>, slot: &mut Option<Session>, which: Browser) -> Result<(), String> {
    // Looked at with the session lock held: a recording claims its slot and
    // then waits on this lock to look for a session, so the two can never
    // both miss each other.
    crate::commands::autorun_record::refuse_while_recording()?;
    if let Some(old) = slot.take() {
        close_session(old);
    }
    let browser = launch_in(which)?;
    // The browser needs a moment to bind its port before it will answer.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let mut cdp = match Cdp::connect(browser.port).await {
        Ok(cdp) => cdp,
        Err(e) => {
            // Connect failed after the process was already spawned and its
            // temp profile created. Without this, `browser` is dropped here
            // (Child has no kill-on-drop) and `*slot` is never assigned, so
            // `auto_run_close_browser` has nothing to take - the msedge.exe
            // and its %TEMP% profile dir leak for the rest of the app's
            // lifetime, and every retry leaks another pair.
            close_browser(browser);
            return Err(e);
        }
    };
    // Network and console events on from the start, as an unattended run's
    // browser has them: a step that checks a request reads the network
    // record, and a failure can say what the page was doing. Losing them is
    // no reason not to open the browser.
    if let Err(e) = crate::browser::page_log::watch(&mut cdp).await {
        crate::applog::warn(format!("auto-run: the page log could not be switched on: {e}"));
    }
    // Downloads are kept while this browser is open, in a folder emptied
    // as it opens and as it closes: they belong to no run. Losing them is
    // no reason not to open the browser either.
    let downloads_root = match keep_supervised_downloads(root, &mut cdp).await {
        Ok(root) => Some(root),
        Err(e) => {
            crate::applog::warn(format!("auto-run: downloads could not be switched on: {e}"));
            None
        }
    };
    *slot = Some(Session {
        browser,
        cdp,
        account: None,
        lease: crate::autorun::lease::Held::supervised(),
        guarded_case: None,
        downloads_root,
        tabs_case: None,
        discovery: None,
    });
    // A tab the page opens waits, paused, until this connection reads that
    // it opened and sets it up: read between commands too, or a popup a
    // person opens by hand would sit blank until the next step.
    answer_between_commands();
    crate::applog::info(format!("Auto-run opened {}", which.label()));
    Ok(())
}

/// Kill the process and drop its throwaway profile. Shared with
/// `autorun_replay`, whose `RealBrowsers` closes one of these after every
/// case (and on the way out of a failed open) so a background browser can
/// never outlive the run that started it. It waits for the process to be
/// gone first: a browser still shutting down holds files in its profile,
/// and removing the folder under it fails.
pub(crate) fn close_browser(mut browser: LaunchedBrowser) {
    let _ = browser.child.kill();
    let _ = browser.child.wait();
    if let Err(e) = std::fs::remove_dir_all(&browser.profile_dir) {
        let name = browser.profile_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        crate::applog::info(format!("auto-run: could not remove browser profile {name}: {e}"));
    }
}

/// Point the supervised browser at `downloads/supervised`, emptied first
/// of whatever an earlier session left there. Returns the Auto Run folder
/// it lives in, for the close to empty it again.
async fn keep_supervised_downloads(root: Result<PathBuf, String>, cdp: &mut Cdp) -> Result<PathBuf, String> {
    let root = root?;
    if let Err(e) = store::empty_supervised_downloads(&root) {
        crate::applog::warn(format!("auto-run: an earlier browser's downloads could not all be removed: {e}"));
    }
    cdp.enable_downloads(&store::supervised_downloads_dir(&root)).await.map_err(|e| e.to_string())?;
    Ok(root)
}

/// The browser goes first: one still running holds the files it saved.
fn close_session(s: Session) {
    close_browser(s.browser);
    if let Some(root) = s.downloads_root {
        if let Err(e) = store::empty_supervised_downloads(&root) {
            crate::applog::warn(format!("auto-run: the closed browser's downloads could not all be removed: {e}"));
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_close_browser() -> Result<(), String> {
    // A replay going in this browser is stopped first, without the lock it
    // holds: it ends at its next look, and the browser closes after it. A
    // case's setup still running is stopped between its template steps.
    crate::autorun::replay_to::stop();
    crate::autorun::setup::stop();
    // A discovery going in it ends with it: its state lives in the session.
    let mut slot = SESSION.lock().await;
    if crate::ai_bridge::close_browser_in(&mut slot) {
        crate::applog::info("Auto-run browser closed");
    }
    publish_discovery(&slot);
    Ok(())
}

/// Auto Run's browsers go with the app. A recording - or a Start, a check or
/// a Try still going - is ended the way Cancel ends it, which closes the
/// recording browser; then the supervised browser is closed. Each takes its
/// throwaway profile with it (`close_browser`). Called from the app's exit
/// hook in lib.rs and from an update restart, both of which bound it.
///
/// A Try or a check after Stop only sees the cancel at its next 250 ms poll
/// (`CANCEL_POLL`), and its claim is dropped only once its own browser is
/// already closed - so waiting here for the recorder to be let go is what
/// makes "the browser is gone" true for the caller, not just "a cancel was
/// asked for".
pub async fn close_autorun_browsers() {
    let _ = crate::commands::autorun_record::auto_run_record_cancel().await;
    while crate::commands::autorun_record::recording_is_going() {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    crate::autorun::replay_to::stop();
    crate::autorun::setup::stop();
    if let Some(s) = SESSION.lock().await.take() {
        close_session(s);
        crate::applog::info("Auto-run browser closed as the app exits");
    }
}

/// What a supervised step answers: one outcome per action, the tab the
/// step ran in when that was not `main` (`runner::InRun::tab`), and the
/// browser dialog it met, if any (`runner::InRun::dialog`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct StepRun {
    pub outcomes: Vec<ActionOutcome>,
    pub tab: Option<String>,
    pub dialog: Option<crate::autorun::StepDialog>,
    /// How many page errors a `"page_errors": "flag"` script counted in the
    /// step (`runner::InRun::page_errors_seen`).
    pub page_errors_seen: u32,
}

/// Run one step's actions in order and report every outcome. Actions after
/// an ordinary failure still run: the watcher learns more from "the click
/// worked, the check did not" than from a run that stops at the first red.
/// A failed `sign_in` is the exception - see `autorun::runner::run_step`,
/// which does the actual work; this command is just the IPC-facing shell
/// around it.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_step(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    step: StepScript,
) -> Result<StepRun, String> {
    let root = root(&app)?;
    // The step's fixture values: shared drafts' current outputs, and what
    // the case's setup gave at its start. The step the pane holds is never
    // changed; a value still missing refuses the step with its sentence.
    let step = crate::autorun::setup::resolve_step(&root, &organization, &project, case_id, &step)?;
    let mut slot = SESSION.lock().await;
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    // Another case's tabs do not carry over into this one.
    // The saved script's own choices about dialogs nobody expected and the
    // page's own errors, and which of its steps comes first.
    let saved = crate::autorun::store::load_script(&root, case_id).ok().flatten();
    let first_step = saved.as_ref().and_then(|s| s.steps.iter().map(|s| s.step_number).min()) == Some(step.step_number);
    crate::autorun::runner::supervised_step_begins(&mut session.cdp, &mut session.tabs_case, case_id, first_step).await;
    guard_supervised(session, &root, &organization, &project, case_id, true).await?;
    // A watched run makes no trip of its own: a bare `return_to_area` builds
    // the case's route from its saved script, and one that names an area
    // that area's route - each only when the step has one.
    use crate::autorun::runner::{area_route, area_routes, named_areas, AreaRoute};
    let wants_area = step.actions.iter().flat_map(|a| a.each()).any(|a| {
        matches!(a, crate::browser::actions::Action::ReturnToArea { .. }) && a.area_named().is_none()
    });
    let resolved = if wants_area { Some(area_route(&root, &organization, &project, case_id)) } else { None };
    let area = match &resolved {
        Some(Ok(r)) => AreaRoute::To(r),
        Some(Err(why)) => AreaRoute::Unknown(why),
        None => AreaRoute::Unknown(crate::autorun::runner::NEEDS_SCRIPT_AREA),
    };
    let areas = area_routes(&root, &organization, &project, &named_areas(&step.actions));
    let mut run = crate::autorun::runner::InRun {
        areas: Some(&areas),
        fail_on_unexpected_dialog: saved.as_ref().is_some_and(|s| s.fail_on_unexpected_dialog),
        page_errors: saved.as_ref().and_then(|s| s.page_errors),
        ignore_page_errors: saved.map(|s| s.ignore_page_errors).unwrap_or_default(),
        ..Default::default()
    };
    let outcomes = crate::autorun::runner::run_step_in_run(
        &mut session.cdp,
        &root,
        &organization,
        &project,
        &step,
        &crate::browser::timing::Timing::default(),
        &mut session.account,
        &mut session.lease,
        None,
        area,
        &mut run,
    )
    .await?;
    Ok(StepRun { outcomes, tab: run.tab.take(), dialog: run.dialog.take(), page_errors_seen: run.page_errors_seen })
}

/// Replay case `case_id`'s saved steps 1 to `step` - 1 in the supervised
/// browser and stop before `step` (`autorun::replay_to`), for the person's
/// Replay to step button. The browser open is used; with none, the one last
/// chosen is opened first, as Open browser opens it. One replay at a time.
/// The pane hears `AutorunReplayProgress` before each step. Refusals and
/// the end come back as the `ReplayEnd` with its sentence, which the pane
/// shows as it is; `Err` is a browser that would not open.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_replay_to_step(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    step: i32,
    db_read_access: bool,
) -> Result<crate::autorun::replay_to::ReplayAnswer, String> {
    let req = crate::autorun::replay_to::ReplayRequest { case_id, step, db_read_access };
    // The person's own replay, as their own step: it may lift an earlier
    // case's guard. The tabs its steps ran in go to the pane beside them.
    let (end, tabs) = replay_supervised(&app, &organization, &project, req, true).await?;
    Ok(crate::autorun::replay_to::ReplayAnswer::with_tabs(end, tabs))
}

/// A replay to a step in the supervised browser, for the person's button
/// and the assistant's tool alike (`ai_bridge`'s `/autorun-replay`). One
/// at a time; refused before anything opens when the checks fail; the
/// browser open is used, else the one last chosen is opened. `may_lift` is
/// `guard_for_case`'s: true for the person, false for the assistant, whose
/// replay may switch the no-save guard on but never lifts one held for
/// another case. With the end come the tabs its steps ran in outside
/// `main` (`replay_to_traced`). `Err` is a browser that would not open.
pub(crate) async fn replay_supervised(
    app: &tauri::AppHandle,
    organization: &str,
    project: &str,
    req: crate::autorun::replay_to::ReplayRequest,
    may_lift: bool,
) -> Result<(crate::autorun::replay_to::ReplayEnd, Vec<crate::autorun::replay_to::ReplayedTab>), String> {
    use crate::autorun::replay_to::{self, OneReplay, ReplayEnd};
    use tauri_specta::Event;
    let _one = match OneReplay::claim() {
        Ok(one) => one,
        Err(why) => return Ok((ReplayEnd::Refused(why), Vec::new())),
    };
    let root = root(app)?;
    // Refused before anything opens.
    if let Err(why) = replay_to::check(&root, organization, project, &req) {
        return Ok((ReplayEnd::Refused(why), Vec::new()));
    }
    let (case_id, step, db_read_access) = (req.case_id, req.step, req.db_read_access);
    let secrets = std::sync::Arc::clone(&app.state::<crate::db::DbSecrets>().0);
    let mut slot = SESSION.lock().await;
    // A Close that took the lock first is respected: no browser is opened
    // again for a replay the person has already stopped.
    if let Some(stopped) = replay_to::stopped_before_opening(&replay_to::CANCEL) {
        return Ok((stopped, Vec::new()));
    }
    // A discovery's browser is the assistant's: a replay never runs in it.
    if let Err(why) = crate::ai_bridge::refuse_while_discovering(&mut slot) {
        return Ok((ReplayEnd::Refused(why), Vec::new()));
    }
    let had_browser = slot.is_some();
    open_if_none(app, &mut slot).await?;
    // The panes hear of a browser this replay opened, and of the account
    // it leaves the browser signed in as, before the lock lets them in.
    if let Some(changed) = replay_to::opened_event(had_browser) {
        let _ = changed.emit(app);
    }
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    let account_before = session.account.clone();
    // The replay starts the case afresh, closing every tab but `main`
    // first (`replay_to_checked`): the tabs left are this case's.
    session.tabs_case = Some(case_id);
    // A case's setup runs in a browser of its own, the one last chosen.
    let mut setup_browsers = crate::commands::autorun_replay::RealBrowsers::new(
        crate::browser::launch::Browser::from_name(&store::last_browser(&root)),
        false,
    );
    let mut tabs = Vec::new();
    let end = replay_to::replay_to_traced(
        &mut session.cdp,
        &mut setup_browsers,
        &root,
        organization,
        project,
        &req,
        &mut session.account,
        &mut session.lease,
        &mut session.guarded_case,
        may_lift,
        &crate::browser::timing::Timing::default(),
        &replay_to::CANCEL,
        || crate::autorun::preconditions::for_run(&root, Some(secrets.as_ref()), db_read_access),
        |k, of| {
            let _ = crate::events::AutorunReplayProgress { case_id, step: k, of }.emit(app);
        },
        &mut tabs,
    )
    .await;
    if let Some(changed) = replay_to::signed_in_event(&account_before, &session.account) {
        let _ = changed.emit(app);
    }
    if session.cdp.is_guarding_saves() {
        answer_between_commands();
    }
    let how = match &end {
        ReplayEnd::Ready { .. } => "ready".to_string(),
        ReplayEnd::StoppedAt { phase: replay_to::ReplayPhase::SignIn, .. } => "stopped while signing in".to_string(),
        ReplayEnd::StoppedAt { phase: replay_to::ReplayPhase::Area, .. } => "stopped going to the area".to_string(),
        ReplayEnd::StoppedAt { step, .. } => format!("stopped at step {step}"),
        ReplayEnd::Blocked(_) => "blocked before signing in".to_string(),
        ReplayEnd::Stopped { step } => format!("stopped before step {step} finished"),
        ReplayEnd::Refused(_) => "refused".to_string(),
    };
    let by = if may_lift { "" } else { " for the assistant" };
    crate::applog::info(format!("Auto Run replay of case {case_id} to step {step}{by}: {how}"));
    Ok((end, tabs))
}

/// The replay's stop control: the replay going, if any, ends at its next
/// look and says where it stopped.
#[tauri::command]
#[specta::specta]
pub fn auto_run_stop_replay() {
    crate::autorun::replay_to::stop();
}

/// The person's Allow or Deny on the assistant's request to replay a
/// must-not-save script (`autorun::replay_ask`). Refused with
/// `that replay request is no longer waiting` for a request that already
/// timed out or was answered: a late Allow starts nothing.
#[tauri::command]
#[specta::specta]
pub fn auto_run_answer_replay_request(id: String, allow: bool) -> Result<(), String> {
    let answered = crate::autorun::replay_ask::asks().answer(&id, allow);
    if answered.is_ok() {
        let what = if allow { "allowed" } else { "declined" };
        crate::applog::info(format!("Auto Run: the person {what} the assistant's replay"));
    }
    answered
}

/// A supervised case's preconditions, checked where the case starts and
/// before its sign-in. `blocked`: the case is Blocked, with the sentence
/// as its reason, and the pane never signs it in. `notice`: said before
/// step 1, and the case goes on (the checks were skipped while
/// `db_read_access`, the AI Bridge tab's Database Read Access switch, is
/// off). Neither: the case goes on. The active environment's database is
/// looked up only when the case's script has preconditions. Once they are
/// met, the case's fixtures are prepared as an unattended case's are
/// (`autorun::setup::check_supervised`): a setup not approved, a shared
/// fixture never built or a failed setup run is `blocked` with its
/// sentence, and what the setup gave is kept for the case's steps.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_check_preconditions(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    db_read_access: bool,
) -> Result<crate::autorun::preconditions::PreconditionCheck, String> {
    let root = root(&app)?;
    // A watched case starts here: the last case's tabs are closed, even
    // when it is the same case started again.
    if let Some(session) = SESSION.lock().await.as_mut() {
        session.cdp.close_other_tabs().await;
        session.tabs_case = Some(case_id);
    }
    let secrets = std::sync::Arc::clone(&app.state::<crate::db::DbSecrets>().0);
    let mut checked = crate::autorun::preconditions::check_script(&root, &organization, &project, case_id, || {
        crate::autorun::preconditions::for_run(&root, Some(secrets.as_ref()), db_read_access)
    })
    .await
    .inspect_err(|_| crate::autorun::setup::forget(case_id))?;
    if checked.blocked.is_some() {
        // A start that ends Blocked leaves no setup values behind.
        crate::autorun::setup::forget(case_id);
        return Ok(checked);
    }
    let mut browsers = crate::commands::autorun_replay::RealBrowsers::new(
        crate::browser::launch::Browser::from_name(&store::last_browser(&root)),
        false,
    );
    let timing = crate::commands::autorun_replay::replay_timing(false);
    let blocked = supervised_setup(
        &mut browsers,
        &root,
        &organization,
        &project,
        case_id,
        &timing,
        crate::api_templates::runner::RUN_LIMIT,
        &crate::api_templates::runner::RETRY_PAUSES,
    )
    .await;
    if let Some(why) = blocked {
        // A precondition "checks skipped" notice is still said.
        checked.blocked = Some(why);
    }
    Ok(checked)
}

/// The supervised browser making way for a setup that signs in as `key`
/// (`setup::make_way`). `SESSION` is held for that alone, never while the
/// setup runs.
pub async fn make_way_for_setup(key: String, timing: &crate::browser::timing::Timing) {
    let mut slot = SESSION.lock().await;
    if let Some(s) = slot.as_mut() {
        crate::autorun::setup::make_way(&mut s.cdp, &mut s.account, &mut s.lease, &key, timing).await;
    }
}

/// A supervised start's setup (`setup::check_supervised`), with the
/// supervised browser making way only once the setup will run. Nothing
/// here holds `SESSION` while the setup runs, so Close, a step, Open
/// browser and an unattended Start all still answer; Close stops the
/// setup between its template steps. `Some` is the Blocked sentence.
#[allow(clippy::too_many_arguments)]
pub async fn supervised_setup<B: crate::autorun::replay::Browsers>(
    browsers: &mut B,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    timing: &crate::browser::timing::Timing,
    limit: std::time::Duration,
    retry_pauses: &[std::time::Duration],
) -> Option<String> {
    crate::autorun::setup::check_supervised(
        browsers,
        root,
        organization,
        project,
        case_id,
        timing,
        |key| make_way_for_setup(key, timing),
        limit,
        retry_pauses,
    )
    .await
}

/// Put the supervised browser's no-save guard where this case needs it
/// (`guard_for_case`), for a step or an assistant's try alike.
pub(crate) async fn guard_supervised(
    session: &mut Session,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    may_lift: bool,
) -> Result<(), String> {
    let out =
        guard_for_case(&mut session.cdp, &mut session.guarded_case, root, organization, project, case_id, may_lift).await;
    if session.cdp.is_guarding_saves() {
        answer_between_commands();
    }
    out
}

/// Put a browser's no-save guard where this case needs it. A case whose
/// script on this machine is marked `no_save` is guarded, with the
/// project's save words read afresh, so an edit on the Setup tab counts
/// from the next step. Any other case has an earlier case's guard lifted -
/// but only when `may_lift`: a person's own step. An assistant's try may add
/// a guard and never take one away, so naming the wrong case (or one with
/// no script) can never let a supervised no-save case's draft be saved.
/// A save stopped for the case that held the guard before, and not yet
/// reported, is that case's: it is written to the application log under
/// that case and never carried into this one. `Err` is the step or try
/// refused: a no-save case never runs unguarded.
pub async fn guard_for_case<D: Driver>(
    d: &mut D,
    guarded_case: &mut Option<i32>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    may_lift: bool,
) -> Result<(), String> {
    let no_save = store::load_script(root, case_id)?.is_some_and(|s| s.no_save);
    if !no_save && !may_lift {
        return Ok(());
    }
    if let Some(before) = guarded_case.filter(|c| *c != case_id) {
        if let Some(sentence) = d.take_save_blocked() {
            crate::applog::warn(format!("Auto Run, case {before}: {sentence}"));
        }
    }
    if no_save {
        let guarded = match crate::autorun::nav::load_nav(root, organization, project) {
            Err(why) => Err(why),
            Ok(nav) => d.guard_saves(&nav.save_words).await.map_err(|e| e.to_string()),
        };
        guarded.map_err(|why| crate::browser::save_guard::setup_failed(&why))?;
        *guarded_case = Some(case_id);
    } else if d.is_guarding_saves() {
        // Lifted only once the browser says so; until then it keeps
        // answering every paused request as a guarded browser does.
        match d.stop_guarding_saves().await {
            Ok(()) => *guarded_case = None,
            Err(e) => crate::applog::warn(format!("Auto Run: the no-save guard could not be lifted yet: {e}")),
        }
    } else {
        *guarded_case = None;
    }
    Ok(())
}

/// How often the supervised browser is read between commands while
/// something waits on it (`Cdp::wants_reading`), and for how long each
/// time.
const ANSWER_EVERY: std::time::Duration = std::time::Duration::from_millis(50);
const ANSWER_FOR: std::time::Duration = std::time::Duration::from_millis(10);

/// Whether the task that answers the supervised browser between commands
/// is running. Only ever changed with the session lock held, so starting
/// one and the last one ending can never miss each other.
static ANSWERING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A guarded browser pauses every request until it is answered, and the
/// client only reads the socket while something calls it. Between two
/// commands - a person reading the page before pressing the next step -
/// a task reads it (`keep_answering`), so the page is never held up
/// waiting. Called with the session lock held.
fn answer_between_commands() {
    if ANSWERING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(keep_answering(&SESSION, &ANSWERING, |s: &mut Session| &mut s.cdp, ANSWER_EVERY, ANSWER_FOR));
}

/// Every `every`, read the browser in `slot` for `read_for` while it wants
/// reading (`Cdp::wants_reading`: it is guarded, or it drives the whole
/// browser, where a tab the page opens waits to be set up); a command
/// holding the slot reads it itself and is never waited on. Ends, clearing
/// `running` with the slot still locked, once the slot is empty or its
/// browser no longer wants reading.
pub async fn keep_answering<S, T: crate::browser::cdp::Transport>(
    slot: &tokio::sync::Mutex<Option<S>>,
    running: &std::sync::atomic::AtomicBool,
    cdp_of: fn(&mut S) -> &mut Cdp<T>,
    every: std::time::Duration,
    read_for: std::time::Duration,
) {
    loop {
        tokio::time::sleep(every).await;
        let Ok(mut held) = slot.try_lock() else { continue };
        match held.as_mut().map(cdp_of) {
            Some(cdp) if cdp.wants_reading() => cdp.pump(read_for).await,
            _ => {
                running.store(false, std::sync::atomic::Ordering::SeqCst);
                return;
            }
        }
    }
}

/// One failure screenshot as a data URL the webview can show. The name is
/// checked in `store::load_shot`; nothing outside the shots folder can be
/// read through here.
#[tauri::command]
#[specta::specta]
pub fn auto_run_shot(app: tauri::AppHandle, name: String) -> Result<String, String> {
    let bytes = store::load_shot(&root(&app)?, &name)?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_load_script(
    app: tauri::AppHandle,
    case_id: i32,
) -> Result<Option<CaseScript>, String> {
    store::load_script(&root(&app)?, case_id)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_script(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    script: CaseScript,
) -> Result<(), String> {
    save_script_from_editor(&root(&app)?, &organization, &project, script)
}

/// Said by a save or an import that names no organization or project.
pub const NO_PROJECT: &str = "choose an organization and a project first - scripts follow that project's rules";

/// Whether a script may open pages by address is the project's own rule,
/// read from a file found by organization and project. With either blank
/// that file is one no project has, which reads as "allowed" - so a save
/// with no project would skip the rule instead of applying it.
fn require_project(organization: &str, project: &str) -> Result<(), String> {
    if organization.trim().is_empty() || project.trim().is_empty() {
        return Err(NO_PROJECT.to_string());
    }
    Ok(())
}

/// The pure half of [`auto_run_save_script`], so a test can reach it
/// without an `AppHandle`.
pub fn save_script_from_editor(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    mut script: CaseScript,
) -> Result<(), String> {
    require_project(organization, project)?;
    // A person saving from the editor is a fresh start for the assistant's
    // repair count, whatever the editor happened to send - and the reason
    // for the last one is no longer relevant once a person has looked.
    script.repairs = 0;
    script.last_repair = None;
    // The editor owns what it shows: the steps, title, account, area, Must
    // not save, preconditions and the shared-state marks. Everything else
    // is the stored script's, whatever the editor sent - so a setting the
    // editor has no control for survives a save from it, and a stale
    // editor can never write an old value back. Spelt out field by field,
    // so a new field cannot be added without saying whose it is.
    let stored = store::load_script(root, script.case_id)?.unwrap_or_else(|| CaseScript {
        case_id: script.case_id,
        title: String::new(),
        account: None,
        area: None,
        steps: Vec::new(),
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: Vec::new(),
        setup: None,
        changes: Vec::new(),
        needs_unchanged: Vec::new(),
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None,
        ignore_page_errors: Vec::new(),
    });
    let CaseScript {
        // The editor's own.
        case_id: _,
        title: _,
        account: _,
        area: _,
        steps: _,
        no_save: _,
        preconditions: _,
        changes: _,
        needs_unchanged: _,
        // Reset above: a person's save starts the repair count afresh.
        repairs: _,
        last_repair: _,
        // The store keeps the mark on disk whatever a save sends
        // (`store::save_scripts_atomically`).
        suspected_defect: _,
        // Every save stamps its own time.
        saved_at: _,
        // The stored script's. Only the assistant writes a setup, so the
        // webview can never change a fixture or write an old one back over
        // an approval.
        setup,
        fail_on_unexpected_dialog,
        page_errors,
        ignore_page_errors,
    } = stored;
    script.setup = setup;
    script.fail_on_unexpected_dialog = fail_on_unexpected_dialog;
    script.page_errors = page_errors;
    script.ignore_page_errors = ignore_page_errors;
    // The project's rules - no address while that is switched off, only
    // recorded areas - the same ones the import and the assistant's save
    // apply.
    crate::autorun::nav::check_project_rules(root, organization, project, std::slice::from_ref(&script))?;
    // Through the same helper the bundle paths use, as a bundle of one:
    // the script editor is a THIRD way in, and a case id of 0 or an empty
    // step list refused from a file but accepted from the editor would be
    // a rule that depends on which door you came through.
    store::save_scripts_atomically(root, std::slice::from_ref(&script)).map_err(|e| e.to_string())
}

/// A person's Clear on a case's suspected-defect mark: they have looked and
/// decided it is not, or no longer, the application's fault. Removes the
/// mark and nothing else - the script's actions and repair count stay.
#[tauri::command]
#[specta::specta]
pub fn auto_run_clear_suspected_defect(app: tauri::AppHandle, case_id: i32) -> Result<(), String> {
    clear_suspected_defect(&root(&app)?, case_id)
}

/// The pure half of [`auto_run_clear_suspected_defect`], so a test can
/// reach it without an `AppHandle`. A case with no mark is left as it is.
pub fn clear_suspected_defect(root: &std::path::Path, case_id: i32) -> Result<(), String> {
    if store::clear_suspected_defect_any(root, case_id)? {
        crate::applog::info(format!("Auto Run: the suspected defect on case {case_id} was cleared from the app"));
    }
    Ok(())
}

/// The plan for a selection, as the screen shows it: the order, the order
/// split into phases at each reset point, and the reset points. `counts`
/// is `(this order's resets, the suggested order's resets)`, only when
/// Auto Run's own order for the PBI needs more resets than the suggestion.
/// `saved` says whether the PBI has an order of its own on this machine.
/// Titles are the screen's: it has the case list.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct PlanView {
    pub order: Vec<i32>,
    pub phases: Vec<Vec<i32>>,
    pub resets: Vec<crate::autorun::plan::Reset>,
    pub counts: Option<(u32, u32)>,
    pub saved: bool,
}

/// The plan for the cases selected on the Auto Run tab, in list order.
/// Each case's marks come from its saved script; a case with no script
/// has none. Scripts and orders are kept by work item id, so the
/// organization and project name the selection's source and nothing more.
/// `preview_order` stands in for the saved order for this one call (the
/// Execution order dialog asks about a list the person has moved but not
/// saved); nothing is written.
#[tauri::command]
#[specta::specta]
pub fn auto_run_plan(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    case_ids: Vec<i32>,
    preview_order: Option<Vec<i32>>,
) -> Result<PlanView, String> {
    let _ = (organization, project);
    Ok(plan_at(&root(&app)?, pbi_id, &case_ids, preview_order.as_deref()))
}

/// The pure half of [`auto_run_plan`], so a test can reach it without an
/// `AppHandle`. A script that cannot be read counts as no marks (and is
/// logged), so one bad file never stops the plan.
pub fn plan_at(root: &std::path::Path, pbi_id: i32, case_ids: &[i32], preview_order: Option<&[i32]>) -> PlanView {
    use crate::autorun::plan::{plan_for, Marks};
    let selected: Vec<(i32, Marks)> = case_ids
        .iter()
        .map(|&id| {
            let marks = match store::load_script(root, id) {
                Ok(Some(script)) => Marks::of(&script),
                Ok(None) => Marks::default(),
                Err(e) => {
                    crate::applog::warn(format!("auto-run: case {id}'s script could not be read for the plan: {e}"));
                    Marks::default()
                }
            };
            (id, marks)
        })
        .collect();
    let saved = match preview_order {
        Some(p) => Some(p.to_vec()),
        None => store::load_order(root, pbi_id),
    };
    let (plan, counts) = plan_for(&selected, saved.as_deref());
    PlanView {
        order: plan.order,
        phases: plan.phases,
        resets: plan.resets,
        counts: counts.map(|(a, b)| (a as u32, b as u32)),
        saved: preview_order.is_none() && saved.is_some(),
    }
}

/// Save Auto Run's own execution order for a PBI on this machine. Only
/// the cases given move: the rest of an order already saved keeps its
/// places (`store::merge_order`). Run Tests' order is separate and is not
/// changed.
#[tauri::command]
#[specta::specta]
pub fn auto_run_save_order(app: tauri::AppHandle, pbi_id: i32, case_ids: Vec<i32>) -> Result<(), String> {
    // The dialog may order only the cases ticked now: the rest of the saved
    // order is kept.
    store::save_order_merged(&root(&app)?, pbi_id, &case_ids)
}

/// "Use suggested order": forget the PBI's own order, so the suggested
/// order is used again.
#[tauri::command]
#[specta::specta]
pub fn auto_run_clear_order(app: tauri::AppHandle, pbi_id: i32) -> Result<(), String> {
    store::clear_order(&root(&app)?, pbi_id)
}

/// Import a BUNDLE of scripts from one file - the shape an assistant
/// writes for a whole PBI, and the shape the Auto Run screen's Import
/// button reads back.
///
/// Takes a PATH, not the file's contents: the frontend used to read the
/// file itself and hand over base64 text, but `atob` decodes base64 to a
/// latin-1 binary string, so any non-ASCII byte (an accent, a curly
/// quote, an em dash) came out mojibake, and a UTF-8 BOM made the JSON
/// look corrupt before it ever reached the parser. Reading here, in Rust,
/// with the same BOM handling `import_parser` already uses, sidesteps
/// both.
///
/// All or nothing, via `store::save_scripts_atomically`: every entry is
/// validated and serialised before a single file is written, so a bad
/// entry - or a filesystem error partway through a big bundle - never
/// leaves the tester unable to tell which cases are current. Returns the
/// case ids that landed, so the screen can say what changed rather than
/// just "done".
///
/// Every script is checked against the live app as an assistant's save is
/// (`seen_check`), in full, before anything is written, so the test cases
/// are read from Azure DevOps first: a case's own words may name what a
/// check looks for. With no way to read them the import is refused, never
/// let through unchecked.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_import_scripts(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    path: String,
) -> Result<Vec<i32>, String> {
    let root = root(&app)?;
    require_project(&organization, &project)?;
    let scripts = read_import_file(&path)?;
    let cases = import_case_texts(&app, &organization, &scripts).await?;
    import_scripts(&root, &organization, &project, scripts, cases.as_ref())
}

/// The test cases' own text by case id: each step's action and expected
/// result, which the seen check reads.
pub type CaseTexts = std::collections::HashMap<i32, Vec<String>>;

/// Said when an import cannot read its scripts' test cases.
pub const IMPORT_NEEDS_CASES: &str =
    "Imported scripts are checked against the live app and their test cases; sign in and try again.";

/// Closes the list of what an import named that was never seen.
pub const IMPORT_UNSEEN_THEN: &str =
    "Explore the area with discovery so its map holds what these scripts use, then import again.";

/// The text of every case `scripts` are for, read from Azure DevOps (read
/// only). `None` when they cannot be read: signed out or offline.
async fn import_case_texts(
    app: &tauri::AppHandle,
    organization: &str,
    scripts: &[CaseScript],
) -> Result<Option<CaseTexts>, String> {
    let Ok(token) = crate::state::get_fresh_token(app).await else { return Ok(None) };
    let mut ids: Vec<i32> = scripts.iter().map(|s| s.case_id).collect();
    ids.sort_unstable();
    ids.dedup();
    match crate::ado::AdoClient::new(token).get_test_cases_by_ids(organization, &ids, None, None).await {
        Ok(cases) => Ok(Some(
            cases
                .into_iter()
                .map(|c| (c.id, c.steps.into_iter().flat_map(|s| [s.action, s.expected]).collect()))
                .collect(),
        )),
        // Azure DevOps refuses the whole batch when any id is not a work
        // item, so it cannot say which: the ids are named back instead.
        Err(crate::ado::AdoError::NotFound) => Err(format!(
            "Azure DevOps has no work item for at least one of {}. An imported script is checked against its own test case, so nothing was imported.",
            ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
        )),
        Err(e) => {
            crate::applog::warn(format!("Import scripts: the test cases could not be read: {e:?}"));
            Ok(None)
        }
    }
}

/// The pure half of [`auto_run_import_scripts`]: everything that does not
/// need an `AppHandle`, so it can be exercised directly in tests the same
/// way `autorun::store`'s functions are. `cases` is the scripts' test
/// cases' text, `None` when it could not be read.
pub fn import_scripts_from_path(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    path: &str,
    cases: Option<&CaseTexts>,
) -> Result<Vec<i32>, String> {
    require_project(organization, project)?;
    let scripts = read_import_file(path)?;
    import_scripts(root, organization, project, scripts, cases)
}

/// The scripts an import file holds; refused when it holds none.
fn read_import_file(path: &str) -> Result<Vec<CaseScript>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Could not read {path}: {e}"))?;
    let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
    let scripts: Vec<CaseScript> = serde_json::from_str(content).map_err(|e| {
        format!(
            "that file is not a list of action scripts: {e}. Expected an array of {{ case_id, title, steps }}."
        )
    })?;
    if scripts.is_empty() {
        return Err("that file has no scripts in it".to_string());
    }
    Ok(scripts)
}

/// Checks and writes the scripts an import file held: all or nothing.
fn import_scripts(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    mut scripts: Vec<CaseScript>,
    cases: Option<&CaseTexts>,
) -> Result<Vec<i32>, String> {
    crate::autorun::nav::check_project_rules(root, organization, project, &scripts)?;
    // An import can mark a script Must not save, never unmark one: only a
    // person saving from the editor turns the flag off.
    for sc in scripts.iter_mut() {
        if !sc.no_save && store::load_script(root, sc.case_id)?.is_some_and(|old| old.no_save) {
            sc.no_save = true;
        }
    }
    check_imported_seen(root, organization, project, &scripts, cases)?;
    store::save_scripts_atomically(root, &scripts).map_err(|e| e.to_string())?;
    let ids: Vec<i32> = scripts.iter().map(|sc| sc.case_id).collect();
    crate::applog::info(format!("Imported {} auto-run script(s)", ids.len()));
    Ok(ids)
}

/// The save route's seen check, over every step of every imported script.
/// Every failure is listed, script by script, then what to do.
fn check_imported_seen(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    scripts: &[CaseScript],
    cases: Option<&CaseTexts>,
) -> Result<(), String> {
    let cases = cases.ok_or_else(|| IMPORT_NEEDS_CASES.to_string())?;
    let map = crate::autorun::discovery_map::load_map(root, organization, project)?;
    let mut lines: Vec<String> = Vec::new();
    for sc in scripts {
        let Some(text) = cases.get(&sc.case_id) else {
            lines.push(format!("Case {}: Azure DevOps has no test case with this id.", sc.case_id));
            continue;
        };
        for u in crate::autorun::seen_check::check_seen_all(&map, sc, text, None) {
            lines.push(format!("Case {}, step {}: {} was never seen on the live app.", sc.case_id, u.step, u.locator));
        }
    }
    if lines.is_empty() {
        return Ok(());
    }
    crate::applog::info(format!("Import scripts refused: {} unseen locator(s) or missing case(s)", lines.len()));
    lines.push(IMPORT_UNSEEN_THEN.to_string());
    Err(lines.join("\n"))
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_run(app: tauri::AppHandle, run: LocalRun) -> Result<(), String> {
    save_run_at(&root(&app)?, run)
}

/// `auto_run_save_run` for a given data root. The first save of a run - a
/// supervised one, which the screen builds - records the active
/// environment's name; a run already on disk keeps whatever it has, so an
/// old run reviewed later is never stamped with today's environment.
pub fn save_run_at(root: &std::path::Path, mut run: LocalRun) -> Result<(), String> {
    if !safe_run_id(&run.id) {
        return Err(format!("run id {:?} is not a safe filename", run.id));
    }
    if run.environment.is_none() && matches!(store::load_run(root, &run.id), Ok(None)) {
        run.environment = crate::environments::active(root).ok().map(|e| e.name);
    }
    store::save_run_guarded(root, &run)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_list_runs(app: tauri::AppHandle) -> Vec<LocalRun> {
    match root(&app) {
        Ok(r) => store::list_runs(&r),
        Err(_) => vec![],
    }
}

/// `Ok(None)` for a run id nobody has saved - the review screen uses this
/// to load a run for the Send-to-Azure-DevOps screen without pulling every
/// run in the list.
#[tauri::command]
#[specta::specta]
pub fn auto_run_load_run(app: tauri::AppHandle, run_id: String) -> Result<Option<LocalRun>, String> {
    store::load_run(&root(&app)?, &run_id)
}

/// Said when a report is asked for a run this machine no longer has.
pub const REPORT_RUN_GONE: &str = "this run is no longer on this machine";

/// The pure half of [`auto_run_open_report`]: one run as a single HTML page
/// (`autorun::report`), written atomically to `<root>/reports/<run id>.html`
/// - the same file, overwritten, on every open. The reports folder sits
/// beside the shots folder, which is how the page links its pictures
/// (`../shots/<name>`). "Is Auto Run offered here" is passed in, the way the
/// Test files commands take it, so a locked build's refusal is testable.
/// The run id is checked with `store::safe_run_id` before anything touches
/// the disk, since it becomes a file name. The scripts on this machine
/// supply a failed action's words and a case's area. Returns the file's
/// path, for the opener.
///
/// `ran_at` is the run's start as the webview shows it (the person's own
/// locale); the page escapes it and prints it as given, and blank falls
/// back to the run's start in UTC.
pub fn write_report_at(
    offered: bool,
    root: &std::path::Path,
    run_id: &str,
    ran_at: &str,
) -> Result<std::path::PathBuf, String> {
    crate::commands::api_templates::refuse_unless(offered)?;
    if !safe_run_id(run_id) {
        return Err(format!("run id {run_id:?} is not a safe filename"));
    }
    let run = store::load_run(root, run_id)?.ok_or_else(|| REPORT_RUN_GONE.to_string())?;
    let scripts: Vec<CaseScript> = run
        .cases
        .iter()
        .filter_map(|c| store::load_script(root, c.case_id).ok().flatten())
        .collect();
    let html = crate::autorun::report::build_with_downloads(
        &run,
        &scripts,
        ran_at,
        &|name| store::shot_exists(root, name),
        &|name| store::download_size(root, run_id, name),
    );
    let dir = store::reports_dir(root);
    let path = dir.join(format!("{run_id}.html"));
    std::fs::create_dir_all(&dir).map_err(|e| {
        crate::applog::warn(format!("auto run report: the reports folder could not be made: {e}"));
        REPORT_NOT_WRITTEN.to_string()
    })?;
    crate::ai_tools::atomic_write(&path, &html).map_err(|e| {
        crate::applog::warn(format!("auto run report for {run_id} could not be written: {e}"));
        REPORT_NOT_WRITTEN.to_string()
    })?;
    crate::applog::info(format!("auto run report for {run_id} written"));
    Ok(path)
}

/// Said when the report page could not be written to disk.
pub const REPORT_NOT_WRITTEN: &str = "the report could not be written - see Settings, Logs";

/// Said when the browser could not be asked to open the report.
pub const REPORT_NOT_OPENED: &str = "the report could not be opened in your browser - see Settings, Logs";

/// Writes one run's report and opens it in the default browser. `ran_at` is
/// the run's start time as the webview shows it. Async, with
/// the write on a blocking thread: reading a run's scripts and writing the
/// page must not hold the main thread, which would freeze the window.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_open_report(app: tauri::AppHandle, run_id: String, ran_at: String) -> Result<(), String> {
    let offered = crate::ai_tools::autorun_offered();
    let root = root(&app)?;
    let path = tauri::async_runtime::spawn_blocking(move || write_report_at(offered, &root, &run_id, &ran_at))
        .await
        .map_err(|e| {
            crate::applog::warn(format!("auto run report: the writer stopped: {e}"));
            REPORT_NOT_WRITTEN.to_string()
        })??;
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("the auto run report could not be opened: {e}"));
        REPORT_NOT_OPENED.to_string()
    })
}

/// The pure half of [`auto_run_open_download`]: the one path Open may hand
/// to the default app, a plain name in the run's own download folder
/// (`store::download_file`). "Is Auto Run offered here" is passed in, as the
/// report takes it.
pub fn download_path_at(
    offered: bool,
    root: &std::path::Path,
    run_id: &str,
    name: &str,
) -> Result<std::path::PathBuf, String> {
    crate::commands::api_templates::refuse_unless(offered)?;
    store::download_file(root, run_id, name)
}

/// The pure half of [`auto_run_download_sizes`].
pub fn download_sizes_at(
    offered: bool,
    root: &std::path::Path,
    run_id: &str,
) -> Result<Vec<crate::autorun::DownloadFile>, String> {
    crate::commands::api_templates::refuse_unless(offered)?;
    Ok(store::download_files(root, run_id))
}

/// Said when the default app could not be asked to open a download.
pub const DOWNLOAD_NOT_OPENED: &str = "the file could not be opened - see Settings, Logs";

/// Opens one of a run's downloads with the system's default app. Only a
/// plain name in that run's own download folder is opened; anything else
/// is refused with `that file is not one of this run's downloads`.
#[tauri::command]
#[specta::specta]
pub fn auto_run_open_download(app: tauri::AppHandle, run_id: String, name: String) -> Result<(), String> {
    let path = download_path_at(crate::ai_tools::autorun_offered(), &root(&app)?, &run_id, &name)?;
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("a run's download could not be opened: {e}"));
        DOWNLOAD_NOT_OPENED.to_string()
    })
}

/// The files in a run's download folder with their sizes, read from disk
/// now: a name the run recorded that is missing here is a file that is no
/// longer on this machine.
#[tauri::command]
#[specta::specta]
pub fn auto_run_download_sizes(
    app: tauri::AppHandle,
    run_id: String,
) -> Result<Vec<crate::autorun::DownloadFile>, String> {
    download_sizes_at(crate::ai_tools::autorun_offered(), &root(&app)?, &run_id)
}

/// A run id the frontend can stamp on a new session.
#[tauri::command]
#[specta::specta]
pub fn auto_run_new_id() -> String {
    store::new_run_id()
}

/// The tester's accounts, passwords included: the Accounts dialog edits
/// them in place. This is the one IPC call that carries a password, by the
/// owner's decision (see `autorun::accounts`). It is never logged.
#[tauri::command]
#[specta::specta]
pub fn auto_run_list_accounts(
    app: tauri::AppHandle,
) -> Result<Vec<crate::autorun::accounts::Account>, String> {
    crate::autorun::accounts::load_accounts(&root(&app)?)
}

/// Replace the accounts list. Returns the keys whose saved session was
/// dropped, so the screen can say so.
#[tauri::command]
#[specta::specta]
pub fn auto_run_save_accounts(
    app: tauri::AppHandle,
    accounts: Vec<crate::autorun::accounts::Account>,
) -> Result<Vec<String>, String> {
    let dropped = crate::autorun::accounts::save_accounts(&root(&app)?, &accounts)?;
    crate::applog::info(format!("Auto-run accounts saved ({} account(s))", accounts.len()));
    Ok(dropped)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_load_recipe(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Option<crate::autorun::recipe::SignInRecipe>, String> {
    crate::autorun::recipe::load_recipe(&root(&app)?, &organization, &project)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_recipe(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    recipe: crate::autorun::recipe::SignInRecipe,
) -> Result<(), String> {
    crate::autorun::recipe::save_recipe(&root(&app)?, &organization, &project, &recipe)?;
    crate::applog::info("Auto-run sign-in recipe saved");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_load_quirks(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    crate::autorun::quirks::load_quirks(&root(&app)?, &organization, &project)
}

/// What a supervised run just saved says about the project's quirks: the
/// run pane calls this once, after its save; the review screen never does.
#[tauri::command]
#[specta::specta]
pub fn auto_run_count_evidence(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    run_id: String,
) -> Result<bool, String> {
    crate::autorun::quirks::count_saved_run(
        &root(&app)?,
        &organization,
        &project,
        &run_id,
        crate::autorun::sessions::now_ms(),
    )
}

// The Known quirks list's own changes, one per button. Each is saved the
// moment it is made and answers with the whole list as saved, so the
// dialog never shows a list the file does not hold.

/// A note the person types in, added as theirs. Past the active cap it is
/// refused with the notes worth retiring.
#[tauri::command]
#[specta::specta]
pub fn auto_run_add_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    text: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    use crate::autorun::quirks::{record_in, update_quirks, FROM_AUTORUN};
    let now = crate::autorun::sessions::now_ms();
    let (_, list) = update_quirks(&root(&app)?, &organization, &project, |l| {
        record_in(l, &text, "person", FROM_AUTORUN, Vec::new(), now)
    })?;
    crate::applog::info("Auto Run: a project quirk was added");
    Ok(list)
}

/// A note's text, changed - whoever wrote it.
#[tauri::command]
#[specta::specta]
pub fn auto_run_edit_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
    text: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    let (_, list) = crate::autorun::quirks::update_quirks(&root(&app)?, &organization, &project, |l| {
        crate::autorun::quirks::edit_in(l, &id, &text)
    })?;
    crate::applog::info("Auto Run: a project quirk was edited");
    Ok(list)
}

/// Retired by the person - any note, theirs or an assistant's, with an
/// optional reason. It stays in the file and can be restored.
#[tauri::command]
#[specta::specta]
pub fn auto_run_retire_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
    reason: Option<String>,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    let now = crate::autorun::sessions::now_ms();
    let (_, list) = crate::autorun::quirks::update_quirks(&root(&app)?, &organization, &project, |l| {
        crate::autorun::quirks::retire_in(l, &id, reason.as_deref(), None, false, now)
    })?;
    crate::applog::info("Auto Run: a project quirk was retired");
    Ok(list)
}

/// A retired note, back on the active list - refused while that is full.
#[tauri::command]
#[specta::specta]
pub fn auto_run_restore_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    let (_, list) = crate::autorun::quirks::update_quirks(&root(&app)?, &organization, &project, |l| {
        crate::autorun::quirks::restore_in(l, &id)
    })?;
    crate::applog::info("Auto Run: a project quirk was restored");
    Ok(list)
}

/// A note removed from the file altogether.
#[tauri::command]
#[specta::specta]
pub fn auto_run_delete_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    let (_, list) = crate::autorun::quirks::update_quirks(&root(&app)?, &organization, &project, |l| {
        crate::autorun::quirks::delete_in(l, &id)
    })?;
    crate::applog::info("Auto Run: a project quirk was deleted");
    Ok(list)
}

/// The project's areas and its address switch, as the Areas dialog shows
/// them. A project with no file reads as no areas, switch on.
#[tauri::command]
#[specta::specta]
pub fn auto_run_load_nav(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::load_nav(&root(&app)?, &organization, &project)?;
    Ok(crate::autorun::nav::view(&nav))
}

/// The project's own save words (Setup, Save words), replaced as a whole
/// list. The built-in words are not in it and cannot be removed.
#[tauri::command]
#[specta::specta]
pub fn auto_run_set_save_words(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    words: Vec<String>,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::set_save_words(&root(&app)?, &organization, &project, &words)?;
    crate::applog::info(format!("Auto-run: the project has {} save words of its own", nav.save_words.len()));
    Ok(crate::autorun::nav::view(&nav))
}

/// "Scripts may open pages by address", saved the moment it is flipped.
#[tauri::command]
#[specta::specta]
pub fn auto_run_set_direct_urls(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    allowed: bool,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::set_direct_urls(&root(&app)?, &organization, &project, allowed)?;
    crate::applog::info(format!(
        "Auto-run: scripts may open pages by address: {}",
        if allowed { "on" } else { "off" }
    ));
    Ok(crate::autorun::nav::view(&nav))
}

/// Forget one area, by name. The module's other areas stay. The dialog
/// asks first.
#[tauri::command]
#[specta::specta]
pub fn auto_run_remove_module_path(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    area: String,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::remove_path(&root(&app)?, &organization, &project, &area)?;
    crate::applog::info("Auto-run area removed");
    Ok(crate::autorun::nav::view(&nav))
}

/// Sign the named account in, in the open browser. Used before a case's
/// first step, and by the `sign_in` action in the middle of one. An account
/// anything else holds (`autorun::lease`) is refused at once, with the
/// sentence that says who has it, and the browser is left as it was.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_sign_in(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    account_key: String,
) -> Result<crate::autorun::signin::SignInOutcome, String> {
    let root = root(&app)?;
    // Refused before the browser is waited for: no account, no recipe.
    crate::autorun::signin::prepare(&root, &organization, &project, &account_key)?;
    let mut slot = SESSION.lock().await;
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    crate::autorun::signin::sign_in_leased(
        &mut session.cdp,
        &root,
        &organization,
        &project,
        &account_key,
        &mut session.lease,
        &mut session.account,
        &crate::browser::timing::Timing::default(),
    )
    .await
}

/// Throw a saved session away, so the next sign-in goes through the form.
#[tauri::command]
#[specta::specta]
pub fn auto_run_forget_session(app: tauri::AppHandle, account_key: String) -> Result<(), String> {
    crate::autorun::sessions::forget_session(&root(&app)?, &account_key);
    Ok(())
}

/// Both `auto_run_clear_scripts` and `auto_run_clear_runs` refuse while a
/// run is going - an unattended run reads scripts as it goes, and a
/// supervised session writes them (Script) and reads them back (Run) - so
/// "clear" during either would race a job already using the very files it
/// is about to remove. The sentences match the ones `auto_run_open_browser`
/// and `auto_run_replay` already use for the same two exclusions, so a
/// person who has seen one has seen both.
///
/// `pub` rather than crate-private so a test can exercise the guard
/// directly - it takes no `AppHandle`, and building a real supervised
/// browser session just to see this message would be much more test than
/// the message deserves.
pub async fn refuse_while_a_run_is_going() -> Result<(), String> {
    if crate::commands::autorun_replay::replay_is_running() {
        return Err("an unattended run is going - wait for it, or stop it first".to_string());
    }
    if let Some(busy) = open_session_refusal().await {
        return Err(busy);
    }
    Ok(())
}

/// Remove the named cases' scripts from this machine, for the "Clear
/// scripts" button on the (development-only) Auto Run toolbar. Nothing in
/// Azure DevOps is touched - scripts never lived there.
///
/// Returns `u32`, not the `usize` `store::clear_scripts` itself returns -
/// specta refuses to export `usize` to TypeScript at all ("BigInt-style
/// types... to avoid precision loss"), and a count of files removed from a
/// handful of cases never comes close to needing 64 bits.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_clear_scripts(app: tauri::AppHandle, case_ids: Vec<i32>) -> Result<u32, String> {
    refuse_while_a_run_is_going().await?;
    let removed = store::clear_scripts(&root(&app)?, &case_ids)?;
    Ok(removed as u32)
}

/// Remove every saved run and screenshot from this machine, for the
/// "Clear results" button on the (development-only) Auto Run toolbar.
/// Runs already sent to Azure DevOps are removed too - the record there
/// is the durable one, and the confirm the screen shows says so.
///
/// Returns `u32` for the same reason `auto_run_clear_scripts` does.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_clear_runs(app: tauri::AppHandle) -> Result<u32, String> {
    refuse_while_a_run_is_going().await?;
    let removed = store::clear_runs(&root(&app)?)?;
    Ok(removed as u32)
}
