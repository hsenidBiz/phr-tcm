//! Velopack auto-update. The app checks the v2 releases repo on launch and
//! applies updates only when the user asks (restart-to-update), mirroring
//! v1's non-intrusive update prompt. In dev (not Velopack-installed),
//! UpdateManager::new fails and everything here quietly reports "no update".

use std::sync::Mutex;
use velopack::{sources, UpdateCheck, UpdateInfo, UpdateManager};

/// v2 has its own releases repo so v1's and v2's "latest release" (which is
/// what Velopack's HttpSource reads) can never fight over the update feed.
const RELEASES_URL: &str =
    "https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases/releases/latest/download/";

#[derive(Default)]
pub struct UpdateState {
    pub pending: Mutex<Option<UpdateInfo>>,
}

fn manager() -> Option<UpdateManager> {
    let source = sources::HttpSource::new(RELEASES_URL);
    // Errors here mean "not a Velopack install" (dev build) - no update UX.
    UpdateManager::new(source, None, None).ok()
}

/// The outcome of an update check - all THREE of them.
///
/// This used to be an Option, so "a newer version exists", "you are up to
/// date", "this build cannot update itself" and "the feed was unreachable"
/// collapsed into two answers. The app told the last two "You are on the
/// latest version", which is a claim it had not checked and could not make.
#[derive(Debug, Default, Clone, serde::Serialize, specta::Type)]
pub struct UpdateStatus {
    /// The newer version, when there is one.
    pub available: Option<String>,
    /// Why no check happened. When this is set, `available` being None
    /// means "unknown", NOT "up to date".
    pub blocked: Option<String>,
}

/// Looks for a newer release, storing the UpdateInfo for apply.
pub fn check(state: &UpdateState) -> UpdateStatus {
    let Some(um) = manager() else {
        return UpdateStatus {
            available: None,
            blocked: Some("This build does not update itself - it was not installed by the installer.".into()),
        };
    };
    match um.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(info)) => {
            let version = info.TargetFullRelease.Version.clone();
            *state.pending.lock().unwrap() = Some(*info);
            UpdateStatus { available: Some(version), blocked: None }
        }
        Ok(_) => UpdateStatus::default(),
        Err(e) => {
            crate::applog::warn(format!("update check failed: {e}"));
            UpdateStatus {
                available: None,
                blocked: Some(format!("Could not reach the update feed: {e}")),
            }
        }
    }
}

/// Download the pending update and restart into it.
pub fn download_and_apply(state: &UpdateState) -> Result<(), String> {
    let info = state
        .pending
        .lock()
        .unwrap()
        .clone()
        .ok_or("no update pending - check first")?;
    let um = manager().ok_or("not a Velopack install")?;
    um.download_updates(&info, None).map_err(|e| e.to_string())?;
    um.apply_updates_and_restart(&info.TargetFullRelease)
        .map_err(|e| e.to_string())
}
