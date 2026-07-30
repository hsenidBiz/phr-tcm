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
///
/// # Why this asks the feed again instead of trusting `pending`
///
/// `RELEASES_URL` ends in `/releases/latest/download/`, which is a MOVING
/// target: it serves the assets of whatever release is newest right now.
/// The stored `UpdateInfo` names an exact file - `...-1.18.3-full.nupkg` -
/// and the moment a newer release is published, that file is no longer
/// under `latest` and the download 404s. Measured, not guessed: with 1.18.4
/// published, `latest/download/...1.18.4-full.nupkg` answers 200 and
/// `...1.18.3-full.nupkg` answers 404.
///
/// That was survivable when the app only checked at launch, because the
/// banner appeared and was clicked within about the same minute. It stopped
/// being survivable when the check moved to hourly: the banner now sits
/// there until someone notices it, so the info behind it can be an hour old
/// - and two releases half an hour apart is enough to break it. Re-asking
/// costs one request and removes the whole class of staleness.
pub fn download_and_apply(state: &UpdateState) -> Result<(), String> {
    let um = manager().ok_or("not a Velopack install")?;

    let info = match um.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(fresh)) => {
            let fresh = *fresh;
            *state.pending.lock().unwrap() = Some(fresh.clone());
            fresh
        }
        // Already current - someone updated this install another way, or
        // the release was pulled. Saying so is better than downloading
        // nothing and calling it a failure.
        Ok(_) => {
            *state.pending.lock().unwrap() = None;
            return Err("This build is already up to date.".into());
        }
        // A re-check that could not run is not itself a reason to refuse.
        // Fall back to what the banner was built from and let the download
        // report its own problem.
        Err(e) => {
            crate::applog::warn(format!("re-check before update failed: {e}"));
            state
                .pending
                .lock()
                .unwrap()
                .clone()
                .ok_or("no update pending - check first")?
        }
    };

    // Name the version in both failures. "http 404" on its own cannot tell
    // you whether the feed is unreachable or whether it moved out from
    // under a stale banner, and that distinction is the whole bug above.
    let version = info.TargetFullRelease.Version.clone();
    um.download_updates(&info, None)
        .map_err(|e| format!("could not download {version}: {e}"))?;
    um.apply_updates_and_restart(&info.TargetFullRelease)
        .map_err(|e| format!("could not apply {version}: {e}"))
}
