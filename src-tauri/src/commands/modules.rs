//! Reading and switching modules from the webview. See `crate::modules`.

use std::collections::BTreeSet;
use std::path::Path;

use crate::modules::{self, Module};

pub const AUTO_RUN_BUSY: &str = "Auto Run can't be switched off while a run is going - stop the run first.";
pub const PLAYWRIGHT_BUSY: &str = "Playwright can't be switched off while a repo command is running - stop it first.";

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
#[tauri::command]
#[specta::specta]
pub fn modules_set(app: tauri::AppHandle, module: Module, on: bool) -> Result<Vec<Module>, String> {
    use tauri::Manager;
    let before = modules::current();
    refuse_if_busy(
        &before,
        module,
        on,
        crate::commands::autorun_replay::replay_is_running(),
        crate::pw_repo::process::running_now().is_some(),
    )?;
    let next = modules::set(module, on)?;
    // A module off takes its slash commands with it (`effective_disabled`);
    // re-sync from the bridge's last-known toggles so they vanish/appear now.
    if next != before {
        let handle = app.state::<crate::commands::ai_bridge::BridgeHandle>();
        let guard = handle.running.lock().unwrap();
        if let Some((shared, _)) = guard.as_ref() {
            let ctx = shared.ctx.lock().unwrap();
            crate::commands::ai_tools::sync_commands(&ctx.disabled_tools, ctx.working_dir.as_deref());
        }
    }
    Ok(in_order(&next))
}

/// The testable half: the busy refusals, then `modules::set_in` on a state and
/// directory of the caller's own (so a test needs no process-wide lock).
pub fn modules_set_with(
    state: &mut BTreeSet<Module>,
    dir: &Path,
    module: Module,
    on: bool,
    auto_run_busy: bool,
    playwright_busy: bool,
) -> Result<Vec<Module>, String> {
    refuse_if_busy(state, module, on, auto_run_busy, playwright_busy)?;
    modules::set_in(state, dir, module, on).map(|n| in_order(&n))
}

/// The busy refusals: switching `module` off (and everything under it that is
/// on) must not stop a run or a repo command that is going.
pub fn refuse_if_busy(
    state: &BTreeSet<Module>,
    module: Module,
    on: bool,
    auto_run_busy: bool,
    playwright_busy: bool,
) -> Result<(), String> {
    if !on {
        let mut going: Vec<Module> = vec![module];
        going.extend(module.below());
        let going: Vec<Module> = going.into_iter().filter(|m| state.contains(m)).collect();
        if auto_run_busy && going.contains(&Module::AutoRun) {
            return Err(AUTO_RUN_BUSY.to_string());
        }
        if playwright_busy && going.contains(&Module::Playwright) {
            return Err(PLAYWRIGHT_BUSY.to_string());
        }
    }
    Ok(())
}
