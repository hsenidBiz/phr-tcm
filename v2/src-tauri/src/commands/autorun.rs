//! The supervised runner's IPC surface.
//!
//! NOTHING here calls Azure DevOps. The browser session lives for as
//! long as the screen keeps it open; steps run against it one at a time,
//! driven by the human clicking through.

use crate::autorun::store;
use crate::autorun::{CaseScript, LocalRun, StepScript};
use crate::browser::actions::{execute, ActionOutcome, Evaluator};
use crate::browser::cdp::Cdp;
use crate::browser::launch::{launch_in, Browser, LaunchedBrowser};
use std::path::PathBuf;
use tauri::Manager;

/// The one live session. A second Open replaces the first, so a stray
/// browser can never leave the screen permanently stuck.
static SESSION: tokio::sync::Mutex<Option<Session>> = tokio::sync::Mutex::const_new(None);

struct Session {
    browser: LaunchedBrowser,
    cdp: Cdp,
}

impl Evaluator for Cdp {
    async fn eval(&mut self, expression: &str) -> Result<serde_json::Value, String> {
        Cdp::eval(self, expression).await
    }
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

/// Guards `store::save_run` from a run id that could escape the runs
/// directory. `store::save_run` writes `run.id` straight into a filename
/// with no sanitisation of its own - harmless while `store::new_run_id()`
/// (an epoch-millis string) was the only producer, but this command is
/// the id's first IPC-facing entry point, so a frontend value like
/// `"../../evil"` or one containing a path separator must be rejected
/// here rather than trusted through to a filesystem write.
pub fn safe_run_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_open_browser(browser_name: String) -> Result<(), String> {
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
    *slot = Some(Session { browser, cdp });
    crate::applog::info(format!("Auto-run opened {}", which.label()));
    Ok(())
}

fn close_browser(mut browser: LaunchedBrowser) {
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

/// Run one step's actions in order and report every outcome. Actions
/// after a failure still run: the watcher learns more from "the click
/// worked, the check did not" than from a run that stops at the first
/// red.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_step(step: StepScript) -> Result<Vec<ActionOutcome>, String> {
    let mut slot = SESSION.lock().await;
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    let mut out = Vec::new();
    for action in &step.actions {
        out.push(execute(&mut session.cdp, action).await);
    }
    Ok(out)
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
pub fn auto_run_save_script(app: tauri::AppHandle, script: CaseScript) -> Result<(), String> {
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
    store::save_run(&root(&app)?, &run)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_list_runs(app: tauri::AppHandle) -> Vec<LocalRun> {
    match root(&app) {
        Ok(r) => store::list_runs(&r),
        Err(_) => vec![],
    }
}

/// A run id the frontend can stamp on a new session.
#[tauri::command]
#[specta::specta]
pub fn auto_run_new_id() -> String {
    store::new_run_id()
}
