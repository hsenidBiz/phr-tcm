//! What the first switch-on of Auto Run turns on for a person who has never
//! used it, and how an install that already runs it keeps its controls.
//!
//! Pure decisions live here; `commands::modules` applies them.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app_settings::{AppSettings, AutorunMode};
use crate::modules::Module;

/// Which recommended items a switch-on turned on now.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Recommended {
    pub api_templates: bool,
    pub video: bool,
    /// The webview applies this per environment that never set it.
    pub network_tools: bool,
    pub mode_simple: bool,
}

/// An install that already runs Auto Run and never picked a mode gets
/// Advanced, so nothing it has today is hidden. Anything else is unchanged.
pub fn settle_mode_on_upgrade(mode: Option<AutorunMode>, auto_run_on: bool) -> Option<AutorunMode> {
    match (mode, auto_run_on) {
        (None, true) => Some(AutorunMode::Advanced),
        (m, _) => m,
    }
}

/// What switching Auto Run on turns on. Nothing when Auto Run was used
/// before; otherwise each item a person has not already chosen for.
pub fn recommended_on_switch(auto_run_used_before: bool, s: &AppSettings) -> Recommended {
    if auto_run_used_before {
        return Recommended::default();
    }
    Recommended {
        api_templates: !s.api_templates_set,
        video: !s.autorun_failed_video_set && !s.autorun_failed_video,
        network_tools: true,
        mode_simple: s.autorun_mode.is_none(),
    }
}

/// Whether the Auto Run data folder holds any FILE anywhere under it. Empty
/// directories do not count: every start-up builds `envs/`.
pub fn autorun_dir_has_files(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| match e.file_type() {
        Ok(t) if t.is_dir() => autorun_dir_has_files(&e.path()),
        Ok(t) => t.is_file(),
        Err(_) => false,
    })
}

/// Once true, always true: Auto Run is on now, or has left files.
pub fn settle_ever_on(ever_on: bool, auto_run_on: bool, dir_has_files: bool) -> bool {
    ever_on || auto_run_on || dir_has_files
}

/// Settle the mode and "ever on" for an install, once, at start-up. Must run
/// BEFORE `roots::start_up`. Saved only when something changed.
pub fn settle_on_start(data_dir: &Path) {
    let now = crate::app_settings::current();
    let auto_run_on = crate::modules::on(Module::AutoRun);
    let mode = settle_mode_on_upgrade(now.autorun_mode, auto_run_on);
    let ever = settle_ever_on(now.auto_run_ever_on, auto_run_on, !now.auto_run_ever_on && autorun_dir_has_files(&data_dir.join("autorun")));
    if mode != now.autorun_mode || ever != now.auto_run_ever_on {
        if let Err(e) = crate::app_settings::update(|s| {
            s.autorun_mode = mode;
            s.auto_run_ever_on = ever;
        }) {
            crate::applog::warn(format!("settling the Auto Run settings failed: {e}"));
        }
    }
}

/// Apply `r` to `s`: the video switch and the mode. (API Templates is a
/// module; the caller switches it.)
pub fn apply_to_settings(r: &Recommended, s: &mut AppSettings) {
    if r.video {
        s.autorun_failed_video = true;
    }
    if r.mode_simple {
        s.autorun_mode = Some(AutorunMode::Simple);
    }
}
