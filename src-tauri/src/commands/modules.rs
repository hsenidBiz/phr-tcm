//! Reading and switching modules from the webview. See `crate::modules`.

use std::collections::BTreeSet;
use std::path::Path;

use crate::app_settings::AppSettings;
use crate::autorun::recommended::{self, Recommended};
use crate::modules::{self, Module};

/// The answer to a switch: every module now on, and - only when this call
/// turned Auto Test from off to on - what that turned on besides.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ModulesAnswer {
    pub on: Vec<Module>,
    pub recommended: Option<Recommended>,
}

pub const AUTO_RUN_BUSY: &str = "Auto Test can't be switched off while a run is going - stop the run first.";
pub const PLAYWRIGHT_BUSY: &str = "Playwright can't be switched off while a repo command is running - stop it first.";

pub const AUTO_RUN_BROWSER_OPEN: &str = "Auto Test can't be switched off while its browser is open - close it first.";
/// What a failed save of `modules.json` says; the raw reason goes to the log.
pub const SAVE_FAILED: &str = "Could not save this setting. The app log in Settings has the details.";

/// Whether any Auto Test browser is open: an assistant discovery, a
/// supervised browser (any of the assistants' too), an Auto Test setup
/// browser, or a held API template browser. Switching Auto Test off would
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
/// (its cascade counts: Database Access off takes Auto Test with it).
///
/// The slash commands and "always allow" entries that follow the switch are
/// files (and a PATH search), so they are written on a blocking thread.
#[tauri::command]
#[specta::specta]
pub async fn modules_set(app: tauri::AppHandle, module: Module, on: bool) -> Result<ModulesAnswer, String> {
    let before = modules::current();
    let answer = modules_set_now(module, on, true)?;
    // A module off takes its slash commands with it (`effective_disabled`);
    // re-sync from the bridge's last-known toggles so they vanish/appear now.
    if modules::current() != before {
        resync_commands_blocking(&app).await;
    }
    Ok(answer)
}

/// Re-sync the assistant's files on a blocking thread, after a switch.
pub async fn resync_commands_blocking(app: &tauri::AppHandle) {
    let again = app.clone();
    if let Err(e) = tauri::async_runtime::spawn_blocking(move || resync_commands(&again)).await {
        crate::applog::warn(format!("re-syncing the AI tools' files after a module switch stopped early: {e}"));
    }
}

/// One module switch on this process's state, without the file re-sync.
/// `by_person` is false for a switch the person did not choose themselves
/// (Simple mode's switch-on): it leaves the "a person switched API Templates"
/// mark as it was.
pub fn modules_set_now(module: Module, on: bool, by_person: bool) -> Result<ModulesAnswer, String> {
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
    // What the switch recommends, worked out on a copy: the live settings
    // are changed below inside the one save, never written back whole.
    let was = crate::app_settings::current();
    let mut preview = was.clone();
    let rec = settle_switch(&before, module, on, by_person, &mut preview);
    let mut next = modules::set(module, on).map_err(save_failed)?;
    if rec.as_ref().is_some_and(|r| r.api_templates) {
        next = modules::set(Module::ApiTemplates, true).map_err(save_failed)?;
    }
    if preview != was {
        crate::app_settings::update(|c| {
            settle_switch(&before, module, on, by_person, c);
        })
        .map_err(settings_save_failed)?;
    }
    Ok(ModulesAnswer { on: in_order(&next), recommended: rec })
}

/// The settings half of a switch, shared by `modules_set` and
/// `modules_set_with`: what an off-to-on Auto Test switch recommends (`Some`
/// only then) applied to `s`, the mark that a person switched API Templates
/// themselves, and "Auto Test has been on". The caller switches API Templates
/// when the answer says so, and saves `s` when it changed.
fn settle_switch(before: &BTreeSet<Module>, module: Module, on: bool, by_person: bool, s: &mut AppSettings) -> Option<Recommended> {
    let rec = (module == Module::AutoRun && on && !before.contains(&Module::AutoRun))
        .then(|| recommended::recommended_on_switch(s.auto_run_ever_on, s));
    if let Some(r) = &rec {
        recommended::apply_to_settings(r, s);
        s.auto_run_ever_on = true;
    }
    if module == Module::ApiTemplates && by_person {
        s.api_templates_set = true;
    }
    rec
}

fn settings_save_failed(e: String) -> String {
    crate::applog::warn(format!("saving the app settings after a module switch failed: {e}"));
    SAVE_FAILED.to_string()
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
        // Auto Test and API template tools: a module off takes its rules out.
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
/// directory of the caller's own (so a test needs no process-wide lock), and
/// the recommended features on `settings` (saved into `dir` when changed).
#[allow(clippy::too_many_arguments)]
pub fn modules_set_with(
    state: &mut BTreeSet<Module>,
    settings: &mut AppSettings,
    dir: &Path,
    module: Module,
    on: bool,
    auto_run_busy: bool,
    browser_open: bool,
    playwright_busy: bool,
) -> Result<ModulesAnswer, String> {
    modules_set_with_by(state, settings, dir, module, on, true, auto_run_busy, browser_open, playwright_busy)
}

/// `modules_set_with` with `by_person` (see `modules_set_now`).
#[allow(clippy::too_many_arguments)]
pub fn modules_set_with_by(
    state: &mut BTreeSet<Module>,
    settings: &mut AppSettings,
    dir: &Path,
    module: Module,
    on: bool,
    by_person: bool,
    auto_run_busy: bool,
    browser_open: bool,
    playwright_busy: bool,
) -> Result<ModulesAnswer, String> {
    refuse_if_busy(state, module, on, auto_run_busy, browser_open, playwright_busy)?;
    modules::apply(state, module, on)?;
    let before = state.clone();
    let was = settings.clone();
    let mut updated = was.clone();
    let rec = settle_switch(&before, module, on, by_person, &mut updated);
    let mut next = modules::set_in(state, dir, module, on).map_err(save_failed)?;
    if rec.as_ref().is_some_and(|r| r.api_templates) {
        next = modules::set_in(state, dir, Module::ApiTemplates, true).map_err(save_failed)?;
    }
    if updated != was {
        crate::app_settings::save(dir, &updated).map_err(settings_save_failed)?;
        *settings = updated;
    }
    Ok(ModulesAnswer { on: in_order(&next), recommended: rec })
}

/// Log the raw reason (it names a path) and answer with the sentence.
fn save_failed(raw: String) -> String {
    crate::applog::warn(format!("modules: could not save modules.json: {raw}"));
    SAVE_FAILED.to_string()
}

/// The busy refusals: switching `module` off (and everything under it that is
/// on) must not stop a run or a repo command that is going, nor strand an open
/// Auto Test browser.
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
