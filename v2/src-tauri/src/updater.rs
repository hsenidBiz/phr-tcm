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

/// Returns the available version string, storing the UpdateInfo for apply.
pub fn check(state: &UpdateState) -> Option<String> {
    let um = manager()?;
    match um.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(info)) => {
            let version = info.TargetFullRelease.Version.clone();
            *state.pending.lock().unwrap() = Some(*info);
            Some(version)
        }
        _ => None,
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
