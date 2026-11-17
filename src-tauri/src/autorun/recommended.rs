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

/// Whether the app data folder already holds Auto Run's folder.
pub fn auto_run_used_before(data_dir: &Path) -> bool {
    data_dir.join("autorun").exists()
}

/// Settle the mode for an install that was already running Auto Run, once,
/// at start-up. Saved only when it changed.
pub fn settle_on_start() {
    let now = crate::app_settings::current();
    let settled = settle_mode_on_upgrade(now.autorun_mode, crate::modules::on(Module::AutoRun));
    if settled != now.autorun_mode {
        if let Err(e) = crate::app_settings::update(|s| s.autorun_mode = settled) {
            crate::applog::warn(format!("settling the Auto Run mode failed: {e}"));
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
