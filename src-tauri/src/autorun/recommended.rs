//! What the first switch-on of Auto Test turns on for a person who has never
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

/// An install that already runs Auto Test and never picked a mode gets
/// Advanced, so nothing it has today is hidden. Anything else is unchanged.
pub fn settle_mode_on_upgrade(mode: Option<AutorunMode>, auto_run_on: bool) -> Option<AutorunMode> {
    match (mode, auto_run_on) {
        (None, true) => Some(AutorunMode::Advanced),
        (m, _) => m,
    }
}

/// What switching Auto Test on turns on. Nothing when Auto Test was used
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

/// Whether `dir` or anything under it holds a file.
fn holds_a_file(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| match e.file_type() {
        Ok(t) if t.is_dir() => holds_a_file(&e.path()),
        Ok(t) => t.is_file(),
        Err(_) => false,
    })
}

/// Whether Auto Test's OWN work exists under the shared Auto Test folder: at
/// least one saved script or one run record, in an environment folder or in
/// the older flat layout. Nothing else counts - the environments file, the
/// proposals, the last-fill notes, a migration or failed-start note and empty
/// folders are all written whether or not Auto Test was ever on. The seed
/// folder is a template, not work.
pub fn autorun_has_own_work(dir: &Path) -> bool {
    let mut roots = vec![dir.to_path_buf()];
    roots.extend(crate::autorun::roots::env_roots(dir).into_iter().filter(|r| r.file_name().and_then(|n| n.to_str()) != Some(crate::autorun::roots::SEED_ID)));
    roots.iter().any(|r| ["scripts", "runs"].iter().any(|item| holds_a_file(&r.join(item))))
}

/// Once true, always true: Auto Test is on now, or has saved scripts or runs.
pub fn settle_ever_on(ever_on: bool, auto_run_on: bool, has_own_work: bool) -> bool {
    ever_on || auto_run_on || has_own_work
}

/// The mode and "ever on" this install settles to at start-up.
pub fn settled_on_start(now: &AppSettings, auto_run_on: bool, autorun_dir: &Path) -> (Option<AutorunMode>, bool) {
    let mode = settle_mode_on_upgrade(now.autorun_mode, auto_run_on);
    let ever = settle_ever_on(now.auto_run_ever_on, auto_run_on, !now.auto_run_ever_on && autorun_has_own_work(autorun_dir));
    (mode, ever)
}

/// Settle the mode and "ever on" for an install, once, at start-up. Must run
/// BEFORE `roots::start_up`. Saved only when something changed.
pub fn settle_on_start(data_dir: &Path) {
    let now = crate::app_settings::current();
    let (mode, ever) = settled_on_start(&now, crate::modules::on(Module::AutoRun), &data_dir.join("autorun"));
    if mode != now.autorun_mode || ever != now.auto_run_ever_on {
        if let Err(e) = crate::app_settings::update(|s| {
            s.autorun_mode = mode;
            s.auto_run_ever_on = ever;
        }) {
            crate::applog::warn(format!("settling the Auto Test settings failed: {e}"));
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
