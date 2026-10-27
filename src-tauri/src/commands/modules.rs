//! Reading and switching modules from the webview. See `crate::modules`.

use std::collections::BTreeSet;
use std::path::Path;

use crate::modules::{self, Module};

pub const AUTO_RUN_BUSY: &str = "Auto Run can't be switched off while a run is going - stop the run first.";
pub const PLAYWRIGHT_BUSY: &str = "Playwright can't be switched off while a repo command is running - stop it first.";

pub const AUTO_RUN_BROWSER_OPEN: &str = "Auto Run can't be switched off while its browser is open - close it first.";
/// What a failed save of `modules.json` says; the raw reason goes to the log.
pub const SAVE_FAILED: &str = "Could not save this setting. The app log in Settings has the details.";

/// Whether any Auto Run browser is open: an assistant discovery, a
/// supervised browser (any of the assistants' too), an Auto Run setup
/// browser, or a held API template browser. Switching Auto Run off would
/// strand it.
pub fn auto_run_browser_open() -> bool {
    crate::commands::autorun::any_discovery_active()
        || crate::commands::autorun::any_session_open_now()
        || crate::autorun::lease::a_browser_is_held()
        || crate::api_templates::held::any()
}

/// The modules on now, in tree order.
#[tauri::command]
#[specta::specta]
pub fn modules_get() -> Vec<Module> {
    in_order(&modules::current())
}

fn in_order(on: &BTreeSet<Module>) -> Vec<Module> {
    Module::ALL.into_iter().filter(|m| on.contains(m)).collect()
}

/// Switch `module` on or off; the answer is every module now on. Refused
/// while switching it off would stop a run or a repo command that is going
/// (its cascade counts: Database Access off takes Auto Run with it).
///
/// The slash commands and "always allow" entries that follow the switch are
/// files (and a PATH search), so they are written on a blocking thread.
#[tauri::command]
#[specta::specta]
pub async fn modules_set(app: tauri::AppHandle, module: Module, on: bool) -> Result<Vec<Module>, String> {
    modules::refuse_if_locked(module, on, crate::extras::unlocked())?;
    let before = modules::current();
    refuse_if_busy(
        &before,
        module,
        on,
        crate::commands::autorun_replay::replay_is_running(),
        auto_run_browser_open(),
        crate::pw_repo::process::running_now().is_some(),
    )?;
    // The tree's own refusal ("needs ... first") is its sentence; only a
    // save that failed gets the generic one.
    modules::apply(&before, module, on)?;
    let next = modules::set(module, on).map_err(save_failed)?;
    // A module off takes its slash commands with it (`effective_disabled`);
    // re-sync from the bridge's last-known toggles so they vanish/appear now.
    if next != before {
        let again = app.clone();
        if let Err(e) = tauri::async_runtime::spawn_blocking(move || resync_commands(&again)).await {
            crate::applog::warn(format!("re-syncing the AI tools' files after a module switch stopped early: {e}"));
        }
    }
    Ok(in_order(&next))
}

/// Re-sync the assistant's slash commands from the bridge's last-known
/// toggles, so a module's commands vanish or appear now - after a module
/// switch and after the machine's unlock changes what is offered.
///
/// This reads and writes files and looks programs up on PATH, so it is called
/// off the main thread; and it holds neither of the bridge's locks while it
/// does (`resync_commands_with`).
pub fn resync_commands(app: &tauri::AppHandle) {
    use tauri::Manager;
    let handle = app.state::<crate::commands::ai_bridge::BridgeHandle>();
    let root = crate::commands::autorun::root(app).ok();
    resync_commands_with(&handle, |shared, disabled, working_dir| {
        crate::commands::ai_tools::sync_commands(disabled, working_dir);
        // The same switch decides the "always allow" the app writes for the
        // Auto Run and API template tools: a module off takes its rules out.
        if let Some(root) = &root {
            crate::commands::ai_tools::log_allow_outcomes(&crate::commands::ai_tools::sync_allow_files(
                root,
                working_dir,
                &crate::commands::ai_tools::ToolHome::real(),
                Some(&shared.ctx),
            ));
        }
    });
}

/// `resync_commands` on a handle of the caller's own, with `sync` given the
/// running bridge, its disabled tools and its working directory. The save has
/// worked, so a poisoned bridge lock must not turn that into a failure: the
/// guards are recovered. What is wanted is copied out and both locks let go
/// before `sync` runs. Nothing when the bridge is not running.
pub fn resync_commands_with(
    handle: &crate::commands::ai_bridge::BridgeHandle,
    sync: impl FnOnce(&crate::ai_bridge::SharedBridge, &[String], Option<&str>),
) {
    let wanted = {
        let guard = handle.running.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|(shared, _)| {
            let ctx = shared.ctx.lock().unwrap_or_else(|e| e.into_inner());
            (std::sync::Arc::clone(shared), ctx.disabled_tools.clone(), ctx.working_dir.clone())
        })
    };
    if let Some((shared, disabled, working_dir)) = wanted {
        sync(&shared, &disabled, working_dir.as_deref());
    }
}

/// Bring the generated agent files into line with the Playwright repo's rules
/// as they are now, from the bridge's last-known context as `resync_commands`
/// does. Nothing when the bridge is not running.
///
/// This reads and writes files, so it is called off the main thread; and it
/// holds neither of the bridge's locks while it does (`resync_agents_with`).
pub fn resync_agents(app: &tauri::AppHandle) {
    use tauri::Manager;
    let handle = app.state::<crate::commands::ai_bridge::BridgeHandle>();
    resync_agents_with(&handle, crate::commands::ai_tools::sync_agents);
}

/// `resync_agents` on a handle of the caller's own, with `sync` given the
/// bridge's disabled tools and working directory. Both are copied out and the
/// bridge's locks let go before `sync` runs, so file I/O never keeps the bridge
/// (every request it serves) waiting. Nothing when the bridge is not running.
pub fn resync_agents_with(
    handle: &crate::commands::ai_bridge::BridgeHandle,
    sync: impl FnOnce(&[String], Option<&str>),
) {
    let wanted = {
        let guard = handle.running.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|(shared, _)| {
            let ctx = shared.ctx.lock().unwrap_or_else(|e| e.into_inner());
            (ctx.disabled_tools.clone(), ctx.working_dir.clone())
        })
    };
    if let Some((disabled, working_dir)) = wanted {
        sync(&disabled, working_dir.as_deref());
    }
}

/// The testable half: the busy refusals, then `modules::set_in` on a state and
/// directory of the caller's own (so a test needs no process-wide lock).
pub fn modules_set_with(
    state: &mut BTreeSet<Module>,
    dir: &Path,
    module: Module,
    on: bool,
    auto_run_busy: bool,
    browser_open: bool,
    playwright_busy: bool,
) -> Result<Vec<Module>, String> {
    refuse_if_busy(state, module, on, auto_run_busy, browser_open, playwright_busy)?;
    modules::apply(state, module, on)?;
    modules::set_in(state, dir, module, on).map(|n| in_order(&n)).map_err(save_failed)
}

/// Log the raw reason (it names a path) and answer with the sentence.
fn save_failed(raw: String) -> String {
    crate::applog::warn(format!("modules: could not save modules.json: {raw}"));
    SAVE_FAILED.to_string()
}

/// The busy refusals: switching `module` off (and everything under it that is
/// on) must not stop a run or a repo command that is going, nor strand an open
/// Auto Run browser.
pub fn refuse_if_busy(
    state: &BTreeSet<Module>,
    module: Module,
    on: bool,
    auto_run_busy: bool,
    browser_open: bool,
    playwright_busy: bool,
) -> Result<(), String> {
    if !on {
        let mut going: Vec<Module> = vec![module];
        going.extend(module.below());
        let going: Vec<Module> = going.into_iter().filter(|m| state.contains(m)).collect();
        if auto_run_busy && going.contains(&Module::AutoRun) {
            return Err(AUTO_RUN_BUSY.to_string());
        }
        if browser_open && going.contains(&Module::AutoRun) {
            return Err(AUTO_RUN_BROWSER_OPEN.to_string());
        }
        if playwright_busy && going.contains(&Module::Playwright) {
            return Err(PLAYWRIGHT_BUSY.to_string());
        }
    }
    Ok(())
}
