//! Velopack auto-update. The app checks the v2 releases repo on launch and
//! applies updates only when the user asks (restart-to-update), mirroring
//! v1's non-intrusive update prompt. In dev (not Velopack-installed),
//! UpdateManager::new fails and everything here quietly reports "no update".

use std::sync::Mutex;
use velopack::{sources, UpdateCheck, UpdateInfo, UpdateManager};

pub mod ado;

/// v2 has its own releases repo so v1's and v2's "latest release" (which is
/// what Velopack's HttpSource reads) can never fight over the update feed.
pub const REPO_URL: &str = "https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases";

/// The `latest/download/` mirror. Kept only as a fallback - see `sources`.
pub const RELEASES_URL: &str =
    "https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases/releases/latest/download/";

#[derive(Default)]
pub struct UpdateState {
    pub pending: Mutex<Option<UpdateInfo>>,
}

/// Where to look for releases, in the order they are tried.
///
/// # Why the GitHub API comes first
///
/// `latest/download/` is a MOVING pointer, and a download takes two
/// requests through it: one for the feed, one for the package. Those two
/// can disagree. Measured on 2026-08-01, minutes after 1.18.10 was
/// published: a client fetched a feed still naming 1.18.9, then asked
/// `latest/download/...1.18.9-full.nupkg` and got a 404, because `latest`
/// had already moved on and 1.18.9's asset only exists under 1.18.9's own
/// release. Re-checking immediately before the download (1.18.4) narrowed
/// that window but could not close it - both requests still go through the
/// same moving pointer, and the feed is served through a cache.
///
/// `GithubSource` reads the releases list from the API and downloads each
/// asset from ITS OWN release's url, which never moves. A feed one release
/// behind then downloads a file that still exists instead of 404ing.
///
/// The old mirror stays as a second try: it needs only github.com, so a
/// network that allows the site but blocks `api.github.com` keeps working
/// exactly as well as it did before.
fn sources() -> Vec<(&'static str, Box<dyn sources::UpdateSource>)> {
    vec![
        (
            "github api",
            // No token: the releases repo is public, and this ships to
            // machines we do not control - there is nothing safe to embed.
            Box::new(sources::GithubSource::new(REPO_URL, None, false)),
        ),
        ("latest/download", Box::new(sources::HttpSource::new(RELEASES_URL))),
    ]
}

/// One manager per reachable source. Empty means "not a Velopack install"
/// (a dev build), which is the app's cue to offer no update UX at all.
fn managers() -> Vec<(&'static str, UpdateManager)> {
    sources()
        .into_iter()
        .filter_map(|(name, src)| UpdateManager::new_boxed(src, None, None).ok().map(|um| (name, um)))
        .collect()
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
    /// A version a previous "Restart to update" tried and failed to reach.
    ///
    /// The apply happens after this process has exited, so the only way
    /// the app can know it failed is forensically: `note_attempt` records
    /// the target version just before the hand-off, and the next launch
    /// finds itself still on the old version. Without this the failure is
    /// invisible - the app restarts, the banner comes back, and the user
    /// is left to wonder whether clicking it did anything at all.
    pub failed_attempt: Option<String>,
}

/// The file that remembers what the last "Restart to update" aimed for.
const ATTEMPT_FILE: &str = "update-attempt.txt";

/// Records that this process is about to hand off to Update.exe to become
/// `version`. Written just before the hand-off; read by the NEXT launch.
pub fn note_attempt(data_dir: &std::path::Path, version: &str) {
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::fs::write(data_dir.join(ATTEMPT_FILE), version);
}

/// The version the previous apply tried and failed to reach, if any.
///
/// "Failed" is judged by outcome, not by error codes: the marker names a
/// version strictly newer than the one now running, so the hand-off to
/// Update.exe cannot have worked. A marker the app has caught up with (or
/// one that does not parse) is deleted; a failed one is KEPT, so the
/// explanation survives further restarts until an update actually lands -
/// the next `note_attempt` overwrites it anyway.
pub fn failed_attempt(data_dir: &std::path::Path, running: &str) -> Option<String> {
    let path = data_dir.join(ATTEMPT_FILE);
    let target = std::fs::read_to_string(&path).ok()?;
    let target = target.trim().to_string();
    match (parse_version(&target), parse_version(running)) {
        (Some(t), Some(r)) if t > r => Some(target),
        _ => {
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// "1.20.8" as an orderable triple. Anything else is None - a marker this
/// cannot read is a marker not worth alarming anyone over.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().split('.').map(|p| p.parse::<u64>().ok());
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Some(a)), Some(Some(b)), Some(Some(c)), None) => Some((a, b, c)),
        _ => None,
    }
}

/// Looks for a newer release, storing the UpdateInfo for apply.
///
/// A source that ANSWERS settles it, whichever way it answers - "you are up
/// to date" is a real answer and the fallback is not asked to second-guess
/// it. Only a source that could not be reached moves on to the next.
pub fn check(state: &UpdateState) -> UpdateStatus {
    let mans = managers();
    if mans.is_empty() {
        return UpdateStatus {
            blocked: Some("This build does not update itself - it was not installed by the installer.".into()),
            ..UpdateStatus::default()
        };
    }
    let mut last = None;
    for (name, um) in mans {
        match um.check_for_updates() {
            Ok(UpdateCheck::UpdateAvailable(info)) => {
                let version = info.TargetFullRelease.Version.clone();
                *state.pending.lock().unwrap() = Some(*info);
                return UpdateStatus { available: Some(version), ..UpdateStatus::default() };
            }
            Ok(_) => return UpdateStatus::default(),
            Err(e) => {
                crate::applog::warn(format!("update check failed via {name}: {e}"));
                last = Some(e.to_string());
            }
        }
    }
    UpdateStatus {
        blocked: Some(format!(
            "Could not reach the update feed: {}",
            last.unwrap_or_else(|| "no source answered".into())
        )),
        ..UpdateStatus::default()
    }
}

/// How far a download has got, as the app reports it to the user.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    /// 0-100, straight from Velopack.
    pub percent: i16,
    /// Bytes, DERIVED from `percent` (see `bytes_at`) - not a byte counter.
    pub downloaded: u64,
    /// Bytes, exact: the size the release feed gives for the package.
    pub total: u64,
}

/// The byte figure behind a percentage of a known total.
///
/// Velopack's downloader counts real bytes but only reports whole percent
/// floored to the nearest 5 (`download.rs`), so this is a lower bound that
/// can lag the true count by up to 5% of the package. The TOTAL is exact -
/// it comes from the feed, not from this - which is what makes the pair
/// worth showing: "12.5 MB of 24.8 MB" is honest about the destination even
/// when the numerator is stepping.
pub fn bytes_at(percent: i16, total: u64) -> u64 {
    let p = percent.clamp(0, 100) as u64;
    // Multiply before dividing: `total / 100 * p` throws away the remainder
    // on every package whose size is not a multiple of 100.
    (total / 100).saturating_mul(p) + (total % 100) * p / 100
}

/// Download the pending update and restart into it.
///
/// `on_progress` is called from the download thread as well as from this
/// one, so it has to be cheap and thread-safe - emitting a Tauri event is
/// both. It is always called at least twice: once at 0% (which is how the
/// UI learns the size before a single byte lands) and once at 100% before
/// the restart, so a package that was already on disk still resolves the
/// bar instead of leaving it stuck at zero.
///
/// # Why this asks the feed again instead of trusting `pending`
///
/// The stored `UpdateInfo` can be an hour old - the check runs hourly and
/// the banner then sits there until someone notices it - so the version it
/// names may already have been superseded. Re-asking costs one request.
///
/// It is no longer load-bearing the way it was in 1.18.4, when it was the
/// only defence against `latest/download/` moving out from under a stale
/// banner. `sources` explains why that defence could never be complete and
/// what replaced it.
///
/// # Why each source gets its own attempt
///
/// A source that can find the feed but not the package is exactly the 404
/// that started all this. Trying the next source with the same
/// `UpdateInfo` costs one more request and turns that into a download that
/// works, because the two sources resolve the same filename differently.
pub fn download_and_apply(
    state: &UpdateState,
    data_dir: Option<std::path::PathBuf>,
    on_progress: impl Fn(Progress) + Send + Sync + 'static,
) -> Result<(), String> {
    let mans = managers();
    if mans.is_empty() {
        return Err("not a Velopack install".into());
    }

    let report = std::sync::Arc::new(on_progress);
    let mut last = String::new();
    for (name, um) in mans {
        match try_source(&um, state, data_dir.as_deref(), &report) {
            Ok(()) => return Ok(()), // never returns: the app restarts
            Err(Refusal::UpToDate) => {
                *state.pending.lock().unwrap() = None;
                return Err("This build is already up to date.".into());
            }
            Err(Refusal::Failed(e)) => {
                crate::applog::warn(format!("update via {name} failed: {e}"));
                last = e;
            }
        }
    }
    Err(last)
}

/// Why one source did not produce a running new version.
enum Refusal {
    /// The feed says there is nothing newer. Every source would say the
    /// same, so this stops the loop instead of continuing it.
    UpToDate,
    Failed(String),
}

fn try_source(
    um: &UpdateManager,
    state: &UpdateState,
    data_dir: Option<&std::path::Path>,
    report: &std::sync::Arc<impl Fn(Progress) + Send + Sync + 'static>,
) -> Result<(), Refusal> {
    let info = match um.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(fresh)) => {
            let fresh = *fresh;
            *state.pending.lock().unwrap() = Some(fresh.clone());
            fresh
        }
        // Already current - someone updated this install another way, or
        // the release was pulled. Saying so is better than downloading
        // nothing and calling it a failure.
        Ok(_) => return Err(Refusal::UpToDate),
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
                .ok_or_else(|| Refusal::Failed(format!("no update pending - check first ({e})")))?
        }
    };

    // Name the version in both failures. "http 404" on its own cannot tell
    // you whether the feed is unreachable or whether it moved out from
    // under a stale banner, and that distinction is the whole bug above.
    let version = info.TargetFullRelease.Version.clone();
    let total = info.TargetFullRelease.Size;

    report(Progress { percent: 0, downloaded: 0, total });

    // Velopack sends percentages synchronously from inside the read loop, so
    // anything slow on the receiving end would throttle the download itself.
    // Draining on our own thread keeps the two apart.
    let (tx, rx) = std::sync::mpsc::channel::<i16>();
    let pump = {
        let report = std::sync::Arc::clone(&report);
        std::thread::spawn(move || {
            for percent in rx {
                report(Progress { percent, downloaded: bytes_at(percent, total), total });
            }
        })
    };

    // `download_updates` owns the sender and drops it on return, which ends
    // the loop above - so the join cannot outlive the download.
    let downloaded = um.download_updates(&info, Some(tx));
    let _ = pump.join();
    downloaded.map_err(|e| Refusal::Failed(format!("could not download {version}: {e}")))?;

    report(Progress { percent: 100, downloaded: total, total });
    // The last thing written before the hand-off: if the next launch is
    // still older than this version, the apply below must have failed - a
    // fact that process can learn no other way, because this one is about
    // to exit and Update.exe reports its failures only to its own log.
    if let Some(dir) = data_dir {
        note_attempt(dir, &version);
    }
    // The package is on disk now, so a failure here is not something the
    // next source could fix - but it is still reported as one failure among
    // the sources rather than specially, because the caller's job is only to
    // say what went wrong.
    um.apply_updates_and_restart(&info.TargetFullRelease)
        .map_err(|e| Refusal::Failed(format!("could not apply {version}: {e}")))
}
