//! The app-wide Simple mode: its start-up default and the switch-on of every
//! module. Simple mode hides screens; it never switches anything off.
//!
//! Pure decisions live here; `commands::app_settings` applies them.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use crate::app_settings::AppSettings;
use crate::modules::{Module, HIDDEN_UNTIL_UNLOCKED};

/// What `set_app_simple_mode` answers: the settings as saved, and the names
/// of the modules that could not be switched on (empty when all came on).
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct SimpleModeAnswer {
    pub settings: AppSettings,
    pub not_switched_on: Vec<String>,
}

/// A new install starts in Simple mode; an install that already had data
/// keeps the screens it has. A saved choice is never overwritten.
pub fn settle_simple_mode(saved: Option<bool>, new_install: bool) -> Option<bool> {
    saved.or(Some(new_install))
}

/// The modules missing from `on`, parents first, never the held-back one.
pub fn modules_to_switch_on(on: &BTreeSet<Module>) -> Vec<Module> {
    Module::ALL.into_iter().filter(|m| *m != HIDDEN_UNTIL_UNLOCKED && !on.contains(m)).collect()
}

/// Whether `dir` is a first launch. Must be asked BEFORE anything at
/// start-up writes into `dir`: any data an earlier launch left (the same
/// test the modules use, `modules::existing_install`) or a saved module
/// choice or saved settings mean an earlier launch.
pub fn is_new_install(dir: &Path) -> bool {
    !crate::modules::existing_install(dir) && !dir.join("modules.json").exists() && !dir.join("app-settings.json").exists()
}

/// What settling writes, or `None` when a value is already saved: the
/// Simple mode choice and `switch_markers_migrated` (a new install has no old
/// markers to migrate). The one decision both start-up paths use.
pub fn settled_values(saved: Option<bool>, new_install: bool) -> Option<(Option<bool>, bool)> {
    saved.is_none().then(|| (settle_simple_mode(None, new_install), new_install))
}

/// Switch each missing module on through `set` (which answers the modules on
/// after the switch), returning the names that failed. A failure does not
/// stop the rest; a module whose parent failed fails too and is listed.
pub fn switch_all_on_via(
    mut on: BTreeSet<Module>,
    mut set: impl FnMut(Module) -> Result<Vec<Module>, String>,
) -> Vec<String> {
    let mut failed = Vec::new();
    for m in modules_to_switch_on(&on) {
        if on.contains(&m) {
            continue; // an earlier switch brought it on (Auto Run's recommended items)
        }
        match set(m) {
            Ok(now) => on = now.into_iter().collect(),
            Err(e) => {
                crate::applog::warn(format!("Simple mode: could not switch {} on: {e}", m.name()));
                failed.push(m.name().to_string());
            }
        }
    }
    failed
}

/// The testable half of the switch-on: every module on in `state`, saved
/// into `dir`, with the Auto Run recommended features applied to `settings`
/// and the API Templates mark left as it was.
pub fn switch_all_on_with(state: &mut BTreeSet<Module>, settings: &mut AppSettings, dir: &Path) -> Vec<String> {
    let start = state.clone();
    // `state` is the one the set path updates; `set` gets the answer from it.
    switch_all_on_via(start, |m| {
        crate::commands::modules::modules_set_with_by(state, settings, dir, m, true, false, false, false, false).map(|a| a.on)
    })
}

/// The start-up settle against `dir` (the testable half): decide the saved
/// value once and, for a new install, switch every module on. `new_install`
/// comes from `is_new_install`, asked before anything wrote into `dir`.
pub fn settle_on_start_in(dir: &Path, new_install: bool, state: &mut BTreeSet<Module>, settings: &mut AppSettings) -> Vec<String> {
    let Some((mode, migrated)) = settled_values(settings.app_simple_mode, new_install) else {
        return Vec::new();
    };
    settings.app_simple_mode = mode;
    settings.switch_markers_migrated = migrated;
    if let Err(e) = crate::app_settings::save(dir, settings) {
        crate::applog::warn(format!("settling Simple mode failed: {e}"));
    }
    if new_install {
        switch_all_on_with(state, settings, dir)
    } else {
        Vec::new()
    }
}

/// Settle Simple mode for this process, once, at start-up (after the modules
/// and settings are loaded). `new_install` is read before `cache::init`.
pub fn settle_on_start(new_install: bool) {
    let Some((mode, migrated)) = settled_values(crate::app_settings::current().app_simple_mode, new_install) else {
        return;
    };
    if let Err(e) = crate::app_settings::update(|s| {
        s.app_simple_mode = mode;
        s.switch_markers_migrated = migrated;
    }) {
        crate::applog::warn(format!("settling Simple mode failed: {e}"));
        return;
    }
    if new_install {
        switch_all_on();
    }
}

/// Switch every module on in this process; the names that failed.
pub fn switch_all_on() -> Vec<String> {
    switch_all_on_via(crate::modules::current(), |m| {
        crate::commands::modules::modules_set_now(m, true, false).map(|a| a.on)
    })
}

/// The one path `set_app_simple_mode` and its testable twin both take, over
/// a context `ctx` of the caller's own: `save` stores the choice, then (on)
/// `switch` brings every module on, answering the names that failed. A
/// failure to switch leaves the choice saved; off changes no module.
pub fn set_app_simple_mode_via<C>(
    ctx: &mut C,
    on: bool,
    save: impl FnOnce(&mut C, bool) -> Result<(), String>,
    switch: impl FnOnce(&mut C) -> Vec<String>,
) -> Result<Vec<String>, String> {
    save(ctx, on)?;
    Ok(if on { switch(ctx) } else { Vec::new() })
}

/// The testable half of `set_app_simple_mode`, against a state and
/// directory of the caller's own.
pub fn set_app_simple_mode_with(
    state: &mut BTreeSet<Module>,
    settings: &mut AppSettings,
    dir: &Path,
    on: bool,
) -> Result<SimpleModeAnswer, String> {
    let not_switched_on = set_app_simple_mode_via(
        &mut (state, &mut *settings),
        on,
        |(_, s), on| {
            s.app_simple_mode = Some(on);
            crate::app_settings::save(dir, s)
        },
        |(st, s)| switch_all_on_with(st, s, dir),
    )?;
    Ok(SimpleModeAnswer { settings: settings.clone(), not_switched_on })
}
