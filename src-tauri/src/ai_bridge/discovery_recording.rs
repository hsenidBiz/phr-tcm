//! A discovery's video: the assistant starts a recording just before its
//! checks and stops it right after, and the video is kept however the
//! discovery ends (stopped, ended by the assistant or the person, the browser
//! closed, or the ten-minute cap). A recording never fails or slows a
//! discovery: every recorder or store error is logged, then the work goes on.
//! Nothing logged here names a title, an address or a secret.

use super::*;
use crate::autorun::recordings::{self, RecordingEntry, Running};
use crate::browser::cdp::Driver as _;
use crate::browser::recording;
use std::path::Path;

pub const RECORDING_NO_DISCOVERY: &str =
    "No discovery is going: start one with start_autorun_discovery, then start the recording just before the checks.";
pub const RECORDING_NOT_AVAILABLE: &str =
    "Recording is not available: this browser version cannot record. Carry on with the checks without a recording.";
pub const ALREADY_RECORDING: &str =
    "A recording is already going in this browser: stop it with stop_autorun_recording before you start another.";
pub const NOT_RECORDING: &str = "No recording is going in this browser.";
pub const RECORDING_NOT_STARTED: &str = "The recording could not be started; carry on with the checks without one.";
pub const RECORDING_LOST: &str =
    "The recording stopped, but the browser gave no video, so none was kept; the checks are not affected.";

const WHO: &str = "Auto Test recording";

/// The start's answer.
pub fn recording_started(title: &str) -> String {
    format!(
        "Recording \"{title}\" started: stop it with stop_autorun_recording right after the checks. It stops by itself when the discovery ends or after 10 minutes."
    )
}

/// The stop's answer for a kept recording.
pub fn recording_kept(title: &str, duration_ms: u32) -> String {
    let secs = duration_ms / 1000;
    format!(
        "Recording kept: \"{title}\" ({}:{:02}). It is ready in the app under Auto Test \u{2192} Recordings, on the Past runs tab.",
        secs / 60,
        secs % 60
    )
}

/// Start recording the discovery in `slot`, called `{ "title": ... }` in `body`.
pub async fn recording_start_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &Path,
    body: &str,
    now_ms: u64,
) -> (u16, String) {
    const SHAPE: &str = "{ \"title\": \"<a short title, e.g. Bug 4521: leave balance fix>\" }";
    let raw = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => v.get("title").and_then(|t| t.as_str()).map(str::to_string),
        Err(_) => None,
    };
    let Some(raw) = raw else {
        return (400, format!("that is not a recording start. Expected {SHAPE}."));
    };
    let title = match recordings::clean_title(&raw) {
        Ok(t) => t,
        Err(why) => return (400, why),
    };
    let Some(browser) = slot.as_mut() else {
        return (409, RECORDING_NO_DISCOVERY.to_string());
    };
    let p = browser.parts();
    let Some(state) = p.discovery.as_mut().filter(|s| s.account.is_some()) else {
        return (409, RECORDING_NO_DISCOVERY.to_string());
    };
    if state.recording.is_some() {
        return (409, ALREADY_RECORDING.to_string());
    }
    if let recording::Support::No(why) = recording::support(p.driver).await {
        crate::applog::info(format!("{WHO}: not available: {why}"));
        return (409, RECORDING_NOT_AVAILABLE.to_string());
    }
    let Some(started) = recording::start(p.driver, WHO).await else {
        return (409, RECORDING_NOT_STARTED.to_string());
    };
    state.recording = Some(Running { recording: started, title: title.clone(), started_ms: now_ms, started: std::time::Instant::now(), root: root.to_path_buf() });
    crate::applog::info(format!("{WHO}: started"));
    (200, recording_started(&title))
}

/// Stop the recording of the discovery in `slot` and keep its video.
pub async fn recording_stop_in<B: DiscoveryBrowser>(slot: &mut Option<B>, now_ms: u64) -> (u16, String) {
    match take_and_keep(slot, now_ms).await {
        None => (409, NOT_RECORDING.to_string()),
        Some(Some(entry)) => (200, recording_kept(&entry.title, entry.duration_ms)),
        Some(None) => (200, RECORDING_LOST.to_string()),
    }
}

/// The safety stop: whatever recording `slot`'s discovery has is stopped and
/// kept, before the discovery or its browser goes. Every failure is logged.
pub async fn finish_recording<B: DiscoveryBrowser>(slot: &mut Option<B>, now_ms: u64) -> Option<RecordingEntry> {
    take_and_keep(slot, now_ms).await.flatten()
}

/// What is left of `total` once `elapsed` has gone: zero when it is spent.
pub fn budget_left(total: std::time::Duration, elapsed: std::time::Duration) -> std::time::Duration {
    total.saturating_sub(elapsed)
}

/// The safety stop when the app quits or updates: the recording is stopped
/// and read within `bound` (the browser's calls only), then written. A
/// recording the browser cannot hand over in time is lost, and said so. The
/// write itself is local and never timed out, so a video is never half kept.
pub async fn finish_recording_at_exit<B: DiscoveryBrowser>(slot: &mut Option<B>, now_ms: u64, bound: std::time::Duration) {
    let going = slot.as_mut().is_some_and(|b| b.parts().discovery.as_ref().is_some_and(|s| s.recording.is_some()));
    if !going {
        return;
    }
    match tokio::time::timeout(bound, take_and_read(slot, now_ms)).await {
        Ok(Some(Some(video))) => {
            keep_video(video).await;
        }
        Ok(_) => {}
        Err(_) => crate::applog::warn("Auto Test recording: lost at exit"),
    }
}

/// What a recording's cap timer does next (`cap_step_in`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapStep {
    /// Nothing left to wait for: the recording ended, or this timer's is not
    /// the one going now, or it was stopped at the cap.
    Done,
    /// Still going and not yet due: sleep this long, then look again.
    Wait(std::time::Duration),
}

/// One look of the cap timer spawned for the recording that started at
/// `started_ms`. A recording that is not that one (ended, or a newer one
/// began) is left alone. Due is measured on the monotonic clock.
pub async fn cap_step_in<B: DiscoveryBrowser>(slot: &mut Option<B>, started_ms: u64, now_ms: u64) -> CapStep {
    let age = slot
        .as_mut()
        .and_then(|b| b.parts().discovery.as_ref().and_then(|s| s.recording.as_ref()))
        .filter(|r| r.started_ms == started_ms)
        .map(|r| r.started.elapsed());
    let Some(age) = age else { return CapStep::Done };
    let left = budget_left(recordings::CAP, age);
    if !left.is_zero() {
        return CapStep::Wait(left);
    }
    crate::applog::info(format!("{WHO}: stopped at the 10-minute limit"));
    finish_recording(slot, now_ms).await;
    CapStep::Done
}

/// The ten-minute cap: stops the recording only when its own deadline has
/// come, so a later recording is never stopped by an earlier one's timer.
pub async fn stop_recording_over_cap_in<B: DiscoveryBrowser>(slot: &mut Option<B>, now_ms: u64) -> Option<RecordingEntry> {
    let due = slot
        .as_mut()
        .and_then(|b| b.parts().discovery.as_ref().and_then(|s| s.recording.as_ref().map(|r| now_ms >= r.deadline_ms())))
        .unwrap_or(false);
    if !due {
        return None;
    }
    crate::applog::info(format!("{WHO}: stopped at the 10-minute limit"));
    finish_recording(slot, now_ms).await
}

/// `None`: no recording was going. `Some(None)`: it stopped but nothing was
/// kept. `Some(Some(entry))`: kept.
async fn take_and_keep<B: DiscoveryBrowser>(slot: &mut Option<B>, now_ms: u64) -> Option<Option<RecordingEntry>> {
    match take_and_read(slot, now_ms).await? {
        Some(video) => Some(keep_video(video).await),
        None => Some(None),
    }
}

/// A recording's video, read, with what `recordings::add` needs.
struct Video {
    root: std::path::PathBuf,
    title: String,
    duration_ms: u32,
    bytes: Vec<u8>,
}

/// Take the running recording, stop it and read its video: the browser's
/// calls only. The stop and the read go out on the tab the recording started
/// on (its stream belongs to that tab's session); the assistant's own tab is
/// put back last, on every way out. `None`: none was going. `Some(None)`: no
/// video came.
async fn take_and_read<B: DiscoveryBrowser>(slot: &mut Option<B>, now_ms: u64) -> Option<Option<Video>> {
    let browser = slot.as_mut()?;
    let p = browser.parts();
    let running = p.discovery.as_mut()?.recording.take()?;
    let duration_ms = u32::try_from(now_ms.saturating_sub(running.started_ms).min(recordings::CAP_MS)).unwrap_or(u32::MAX);
    let Running { recording: handle, title, root, .. } = running;
    let before = p.driver.tab_name();
    let bytes = match recording::stop(p.driver, handle, WHO).await {
        Some(stopped) => recording::read_video(p.driver, stopped, WHO).await,
        None => None,
    };
    if p.driver.tab_name() != before {
        if let Err(e) = p.driver.switch_tab(&before).await {
            crate::applog::warn(format!("{WHO}: the browser's tab could not be put back: {e}"));
        }
    }
    Some(bytes.map(|bytes| Video { root, title, duration_ms, bytes }))
}

/// Write a read video into the recordings. Every failure is logged.
async fn keep_video(video: Video) -> Option<RecordingEntry> {
    let duration_ms = video.duration_ms;
    let kept = tokio::task::spawn_blocking(move || recordings::add(&video.root, &video.title, video.duration_ms, &video.bytes)).await;
    match kept {
        Ok(Ok(entry)) => {
            crate::applog::info(format!("{WHO}: kept ({}s)", duration_ms / 1000));
            Some(entry)
        }
        Ok(Err(why)) => {
            crate::applog::warn(format!("{WHO}: the video could not be kept: {why}"));
            None
        }
        Err(_) => {
            crate::applog::warn(format!("{WHO}: the video could not be kept"));
            None
        }
    }
}
