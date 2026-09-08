//! The app's own log: a rolling file per day plus an in-memory tail the
//! Settings screen can show.
//!
//! This exists for bug reports. When something misbehaves the useful
//! question is "what did the app just do", and until now the answer lived
//! only in a dev console nobody has in a release build. So: append to a
//! file under the OS app-log directory, and keep the recent lines in
//! memory so the viewer is instant and never re-reads a growing file.
//!
//! Deliberately dependency-free (no log/tracing/chrono): a handful of
//! call sites and one screen do not justify a logging framework, and the
//! timestamp maths below is a fixed, well-known algorithm.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Lines kept in memory for the viewer. Older lines stay in the files.
/// Sized for the request trail: one bulk create of 50 cases is already a
/// few hundred lines, and the tail has to still hold what came BEFORE the
/// thing that went wrong.
const TAIL: usize = 6000;

/// Files older than this are pruned at startup.
const KEEP_DAYS: u64 = 7;

/// One line, as the viewer renders it.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct LogLine {
    /// "YYYY-MM-DD HH:MM:SS" in UTC.
    pub at: String,
    /// "debug" | "info" | "warn" | "error".
    pub level: String,
    pub message: String,
}

fn tail() -> &'static Mutex<VecDeque<LogLine>> {
    static T: OnceLock<Mutex<VecDeque<LogLine>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(VecDeque::with_capacity(TAIL)))
}

fn dir_cell() -> &'static Mutex<Option<PathBuf>> {
    static D: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(None))
}

/// Days since the epoch -> (year, month, day). Howard Hinnant's civil_from_days.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn now_parts() -> (i64, u32, u32, u32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil(days);
    (y, m, d, (rem / 3600) as u32, ((rem % 3600) / 60) as u32, (rem % 60) as u32)
}

/// UTC "YYYY-MM-DD HH:MM:SS". Public because backup export stamps its
/// `exported_at` with the same clock the log lines use.
pub fn stamp() -> String {
    let (y, mo, d, h, mi, s) = now_parts();
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// The same clock as `stamp`, in a form a file name can carry:
/// "YYYYMMDD-HHMMSS".
pub fn file_stamp() -> String {
    let (y, mo, d, h, mi, s) = now_parts();
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

fn today_file() -> Option<PathBuf> {
    let dir = dir_cell().lock().ok()?.clone()?;
    let (y, mo, d, ..) = now_parts();
    Some(dir.join(format!("tcm-{y:04}-{mo:02}-{d:02}.log")))
}

/// Points the logger at a directory and prunes old files. Called once from
/// the Tauri setup hook; logging before this still fills the memory tail.
pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut cell) = dir_cell().lock() {
        *cell = Some(dir.clone());
    }
    prune(&dir);
}

fn prune(dir: &PathBuf) {
    let cutoff = SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(KEEP_DAYS * 86_400))
        .unwrap_or(UNIX_EPOCH);
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("tcm-") || !name.ends_with(".log") {
            continue; // never touch files this app did not write
        }
        if e.metadata()
            .and_then(|m| m.modified())
            .map(|m| m < cutoff)
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Records one line: memory tail first (so the viewer always has it), then
/// best-effort append to today's file. Never panics and never propagates a
/// logging failure into the caller's result.
pub fn log(level: &str, message: impl Into<String>) {
    let line = LogLine { at: stamp(), level: level.to_string(), message: message.into() };

    if let Ok(mut buf) = tail().lock() {
        if buf.len() == TAIL {
            buf.pop_front();
        }
        buf.push_back(line.clone());
    }

    if let Some(path) = today_file() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{} [{}] {}", line.at, line.level.to_uppercase(), line.message);
        }
    }
}

/// The fine-grained trail: every request, every step of a long job.
///
/// Separate from `info` because it is a firehose - a bulk create makes a
/// request per case, plus the suite and classification lookups - and the
/// viewer defaults to hiding it. It is still written to the file, which is
/// the copy that ends up in a bug report.
pub fn debug(msg: impl Into<String>) {
    log("debug", msg);
}
pub fn info(msg: impl Into<String>) {
    log("info", msg);
}
pub fn warn(msg: impl Into<String>) {
    log("warn", msg);
}
pub fn error(msg: impl Into<String>) {
    log("error", msg);
}

/// Newest lines last, capped at `limit`.
pub fn recent(limit: usize) -> Vec<LogLine> {
    let Ok(buf) = tail().lock() else { return vec![] };
    let skip = buf.len().saturating_sub(limit);
    buf.iter().skip(skip).cloned().collect()
}

pub fn directory() -> String {
    dir_cell()
        .lock()
        .ok()
        .and_then(|d| d.clone())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_match_known_values() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_723), (2024, 1, 1)); // leap-year boundary
        assert_eq!(civil(20_644), (2026, 7, 10));
    }

    #[test]
    fn tail_keeps_the_newest_lines_and_records_the_level() {
        for i in 0..(TAIL + 10) {
            log("info", format!("line {i}"));
        }
        let lines = recent(5);
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4].message, format!("line {}", TAIL + 9));
        assert_eq!(lines[4].level, "info");
        // The buffer is bounded, so the oldest lines fell out.
        assert!(recent(TAIL + 100).len() <= TAIL);
    }
}
