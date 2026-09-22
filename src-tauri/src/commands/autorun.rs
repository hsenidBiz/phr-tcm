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
use crate::browser::cdp::Cdp;
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
pub(crate) async fn supervised_session_is_open() -> bool {
    SESSION.lock().await.is_some()
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_open_browser(browser_name: String) -> Result<(), String> {
    if crate::commands::autorun_replay::replay_is_running() {
        return Err("an unattended run is going - wait for it, or stop it first".to_string());
    }
    let mut slot = SESSION.lock().await;
    if let Some(old) = slot.take() {
        close_session(old);
    }
    let which = Browser::from_name(&browser_name);
    let browser = launch_in(which)?;
    // The browser needs a moment to bind its port before it will answer.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let cdp = match Cdp::connect(browser.port).await {
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
    *slot = Some(Session { browser, cdp, account: None });
    crate::applog::info(format!("Auto-run opened {}", which.label()));
    Ok(())
}

/// Kill the process and drop its throwaway profile. Shared with
/// `autorun_replay`, whose `RealBrowsers` closes one of these after every
/// case (and on the way out of a failed open) so a background browser can
/// never outlive the run that started it.
pub(crate) fn close_browser(mut browser: LaunchedBrowser) {
    let _ = browser.child.kill();
    let _ = std::fs::remove_dir_all(&browser.profile_dir);
}

fn close_session(s: Session) {
    close_browser(s.browser);
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_close_browser() -> Result<(), String> {
    if let Some(s) = SESSION.lock().await.take() {
        close_session(s);
        crate::applog::info("Auto-run browser closed");
    }
    Ok(())
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
    step: StepScript,
) -> Result<Vec<ActionOutcome>, String> {
    let root = root(&app)?;
    let mut slot = SESSION.lock().await;
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    crate::autorun::runner::run_step(
        &mut session.cdp,
        &root,
        &organization,
        &project,
        &step,
        &crate::browser::timing::Timing::default(),
        &mut session.account,
    )
    .await
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
pub fn auto_run_save_script(app: tauri::AppHandle, mut script: CaseScript) -> Result<(), String> {
    // A person saving from the editor is a fresh start for the assistant's
    // repair count, whatever the editor happened to send - and the reason
    // for the last one is no longer relevant once a person has looked.
    script.repairs = 0;
    script.last_repair = None;
    // Through the same helper the bundle paths use, as a bundle of one:
    // the script editor is a THIRD way in, and a case id of 0 or an empty
    // step list refused from a file but accepted from the editor would be
    // a rule that depends on which door you came through.
    store::save_scripts_atomically(&root(&app)?, std::slice::from_ref(&script))
        .map_err(|e| e.to_string())
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
#[tauri::command]
#[specta::specta]
pub fn auto_run_import_scripts(app: tauri::AppHandle, path: String) -> Result<Vec<i32>, String> {
    import_scripts_from_path(&root(&app)?, &path)
}

/// The pure half of [`auto_run_import_scripts`]: everything that does not
/// need an `AppHandle`, so it can be exercised directly in tests the same
/// way `autorun::store`'s functions are.
pub fn import_scripts_from_path(root: &std::path::Path, path: &str) -> Result<Vec<i32>, String> {
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
    store::save_scripts_atomically(root, &scripts).map_err(|e| e.to_string())?;
    let ids: Vec<i32> = scripts.iter().map(|sc| sc.case_id).collect();
    crate::applog::info(format!("Imported {} auto-run script(s)", ids.len()));
    Ok(ids)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_run(app: tauri::AppHandle, run: LocalRun) -> Result<(), String> {
    if !safe_run_id(&run.id) {
        return Err(format!("run id {:?} is not a safe filename", run.id));
    }
    store::save_run_guarded(&root(&app)?, &run)
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

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_quirks(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    quirks: Vec<crate::autorun::quirks::Quirk>,
) -> Result<(), String> {
    crate::autorun::quirks::save_quirks(&root(&app)?, &organization, &project, &quirks)?;
    crate::applog::info("Auto-run project quirks saved");
    Ok(())
}

/// Sign the named account in, in the open browser. Used before a case's
/// first step, and by the `sign_in` action in the middle of one.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_sign_in(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    account_key: String,
) -> Result<crate::autorun::signin::SignInOutcome, String> {
    let root = root(&app)?;
    let (recipe, account) = crate::autorun::signin::prepare(&root, &organization, &project, &account_key)?;
    let mut slot = SESSION.lock().await;
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    let out = crate::autorun::signin::sign_in(
        &mut session.cdp,
        &root,
        &recipe,
        &account,
        &crate::browser::timing::Timing::default(),
    )
    .await;
    session.account = out.ok.then(|| account.key.clone());
    crate::applog::info(format!(
        "Auto-run sign-in as {}: {}",
        account.key,
        if out.ok { "ok" } else { "failed" }
    ));
    Ok(out)
}

/// Throw a saved session away, so the next sign-in goes through the form.
#[tauri::command]
#[specta::specta]
pub fn auto_run_forget_session(app: tauri::AppHandle, account_key: String) -> Result<(), String> {
    crate::autorun::sessions::forget_session(&root(&app)?, &account_key);
    Ok(())
}
