//! The browser's own screen recorder, used to keep a video of a failed
//! Auto Test case.
//!
//! Chromium 153 added `Page.startScreenRecording` and
//! `Page.stopScreenRecording`: the browser itself encodes the tab into an
//! MP4 (AV1) in a separate process and hands the file back as an
//! `IO.StreamHandle`, read with `IO.read` and let go with `IO.close`. Nothing
//! here encodes anything, and nothing is bundled.
//!
//! A recording must never fail or slow a run. Every call is bounded by its
//! own timeout, every error is logged (never shown as the case's result) and
//! simply means no video, and a browser too old to offer the command is
//! never asked for it (`support`).

use super::cdp::{CdpError, Driver};
use crate::autorun::store;
use base64::Engine;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// The call that starts recording the page.
pub const START: &str = "Page.startScreenRecording";
/// The call that stops it and answers with the stream the video is read from.
pub const STOP: &str = "Page.stopScreenRecording";
/// Reads a chunk of a stream.
pub const READ: &str = "IO.read";
/// Lets a stream go.
pub const CLOSE: &str = "IO.close";
/// Names the browser and its version.
pub const VERSION: &str = "Browser.getVersion";

/// The first Chromium (and so Edge) major version that has the recorder.
pub const FIRST_MAJOR: u32 = 153;

/// What the case is recorded at: no larger than this, no faster than this.
/// Kept small on purpose - a video of a failed case is evidence, not a film.
pub const MAX_WIDTH: u32 = 1280;
pub const MAX_HEIGHT: u32 = 720;
pub const FRAME_RATE: u32 = 30;

/// How long a start or a stop may take before the recording is given up on.
const CALL_MS: u64 = 5_000;
/// How long reading the finished video back may take, all of it.
const READ_MS: u64 = 60_000;
/// The size asked for per `IO.read`.
const CHUNK: u64 = 1024 * 1024;

/// Said once on the run (`LocalRun::video_unavailable`) when "Keep a video of failed cases" is on and
/// the browser cannot record.
pub const NOT_AVAILABLE: &str = "Video was not recorded: this browser version cannot record.";

/// What `start` asks the browser for.
pub fn start_params() -> Value {
    json!({ "frameRate": FRAME_RATE, "maxWidth": MAX_WIDTH, "maxHeight": MAX_HEIGHT, "audio": false })
}

/// Whether the connected browser can record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Support {
    Yes,
    /// No, and why (for the log; never shown).
    No(String),
}

impl Support {
    pub fn is_yes(&self) -> bool {
        matches!(self, Support::Yes)
    }
}

/// The major version in a `Browser.getVersion` product string:
/// `Edg/157.0.3456.78` -> 157, `HeadlessChrome/153.0.1.2` -> 153.
pub fn major_of(product: &str) -> Option<u32> {
    let (_, version) = product.rsplit_once('/')?;
    version.split('.').next()?.trim().parse().ok()
}

/// Whether the browser the driver is connected to has the recorder: its
/// version is asked for, once, and read. A browser that does not answer, or
/// answers with something unreadable, is taken not to - a probe that fails
/// cleanly is the same as an old browser, and the run goes on without video.
pub async fn support<D: Driver>(d: &mut D) -> Support {
    let reply = match tokio::time::timeout(Duration::from_millis(CALL_MS), d.call(VERSION, json!({}))).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Support::No(format!("the browser's version could not be read: {e}")),
        Err(_) => return Support::No("the browser did not tell its version in time".to_string()),
    };
    let product = reply["product"].as_str().unwrap_or_default();
    match major_of(product) {
        Some(m) if m >= FIRST_MAJOR => Support::Yes,
        Some(m) => Support::No(format!("the browser is version {m}, and recording needs {FIRST_MAJOR}")),
        None => Support::No("the browser's version could not be understood".to_string()),
    }
}

/// A recording that is running.
#[derive(Debug)]
pub struct Recording {
    /// The tab the recording was started on; it is stopped on the same one.
    tab: String,
}

/// A recording that was stopped and whose video is waiting in the browser to
/// be read (`keep`) or let go (`discard`).
#[derive(Debug)]
pub struct Stopped {
    handle: String,
}

/// Start recording the page. `None` when the browser would not (logged): the
/// case runs on without a video.
pub async fn start<D: Driver>(d: &mut D, who: &str) -> Option<Recording> {
    let tab = d.tab_name();
    match d.call_within(START, start_params(), Duration::from_millis(CALL_MS)).await {
        Ok(_) => Some(Recording { tab }),
        Err(e) => {
            crate::applog::warn(format!("{who}: the video of this case could not be started: {e}"));
            None
        }
    }
}

/// Stop recording, on the tab it was started on: a case that ended in another
/// tab goes back to that one first, and one that cannot gives no video. `None` when the browser
/// gave no video (logged).
pub async fn stop<D: Driver>(d: &mut D, recording: Recording, who: &str) -> Option<Stopped> {
    if d.tab_name() != recording.tab {
        if let Err(e) = d.switch_tab(&recording.tab).await {
            crate::applog::warn(format!("{who}: the video of this case could not be stopped, its tab is not reachable: {e}"));
            return None;
        }
    }
    let reply = match d.call_within(STOP, json!({}), Duration::from_millis(CALL_MS)).await {
        Ok(r) => r,
        Err(e) => {
            crate::applog::warn(format!("{who}: the video of this case could not be stopped: {e}"));
            return None;
        }
    };
    match reply["stream"].as_str().filter(|s| !s.is_empty()) {
        Some(handle) => Some(Stopped { handle: handle.to_string() }),
        None => {
            crate::applog::warn(format!("{who}: the browser gave no video for this case"));
            None
        }
    }
}

/// Let the video go unread: the case passed, so it is not kept.
pub async fn discard<D: Driver>(d: &mut D, stopped: Stopped) {
    let _ = d.call_within(CLOSE, json!({ "handle": stopped.handle }), Duration::from_millis(CALL_MS)).await;
}

/// Read the video and keep it beside the run's pictures: its name there, or
/// `None` when it could not be read or written (logged). The stream is let
/// go either way.
pub async fn keep<D: Driver>(d: &mut D, stopped: Stopped, root: &Path, who: &str) -> Option<String> {
    let read = tokio::time::timeout(Duration::from_millis(READ_MS), read_all(d, &stopped.handle)).await;
    let _ = d.call_within(CLOSE, json!({ "handle": stopped.handle }), Duration::from_millis(CALL_MS)).await;
    let bytes = match read {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            crate::applog::warn(format!("{who}: the video of this case could not be read: {e}"));
            return None;
        }
        Err(_) => {
            crate::applog::warn(format!("{who}: the video of this case took too long to read"));
            return None;
        }
    };
    if bytes.is_empty() {
        crate::applog::warn(format!("{who}: the video of this case was empty"));
        return None;
    }
    let root = root.to_path_buf();
    let len = bytes.len();
    match tokio::task::spawn_blocking(move || store::save_video(&root, &bytes)).await {
        Ok(Ok(name)) => {
            crate::applog::info(format!("{who}: kept a video of the case ({:.1} MB)", len as f64 / 1_048_576.0));
            Some(name)
        }
        Ok(Err(e)) => {
            crate::applog::warn(format!("{who}: the video of this case could not be saved: {e}"));
            None
        }
        Err(_) => None,
    }
}

/// Every chunk of a stream, until its end. Refuses a video larger than
/// `store::MAX_VIDEO_BYTES` rather than hold it.
async fn read_all<D: Driver>(d: &mut D, handle: &str) -> Result<Vec<u8>, CdpError> {
    let mut out: Vec<u8> = Vec::new();
    loop {
        let r = d.call_within(READ, json!({ "handle": handle, "size": CHUNK }), Duration::from_millis(CALL_MS)).await?;
        let data = r["data"].as_str().unwrap_or_default();
        if r["base64Encoded"].as_bool().unwrap_or(false) {
            let chunk = base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| CdpError::Protocol {
                method: READ.to_string(),
                message: format!("a piece of the video could not be decoded: {e}"),
            })?;
            out.extend_from_slice(&chunk);
        } else {
            out.extend_from_slice(data.as_bytes());
        }
        if out.len() as u64 > store::MAX_VIDEO_BYTES {
            return Err(CdpError::Protocol { method: READ.to_string(), message: "the video is too large to keep".to_string() });
        }
        if r["eof"].as_bool().unwrap_or(true) {
            return Ok(out);
        }
    }
}
