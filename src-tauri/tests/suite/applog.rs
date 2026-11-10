//! The app log: the civil-date maths behind every stamp, and the bounded
//! in-memory tail the Settings viewer reads.

use v2_lib::applog::{civil, log, recent};

/// Mirrors the private in-memory tail size (`TAIL`) in `applog.rs`.
const TAIL: usize = 6000;

#[test]
fn civil_dates_match_known_values() {
    assert_eq!(civil(0), (1970, 1, 1));
    assert_eq!(civil(19_723), (2024, 1, 1)); // leap-year boundary
    assert_eq!(civil(20_644), (2026, 7, 10));
}

/// The tail is the whole process's, and in the one-binary suite other
/// modules log into it while this runs - so this test's own lines carry a
/// prefix and are picked out by it, rather than assumed to be the last
/// lines written. The log lock keeps this flood from pushing out a line
/// another test is about to read back.
#[test]
fn tail_keeps_the_newest_lines_and_records_the_level() {
    let _log = crate::serial::log_tail();
    let prefix = "applog-tail-test line";
    for i in 0..(TAIL + 10) {
        log("info", format!("{prefix} {i}"));
    }
    assert_eq!(recent(5).len(), 5);
    let mine: Vec<_> = recent(TAIL).into_iter().filter(|l| l.message.starts_with(prefix)).collect();
    let newest = mine.last().expect("the newest line is in the tail");
    assert_eq!(newest.message, format!("{prefix} {}", TAIL + 9));
    assert_eq!(newest.level, "info");
    // The buffer is bounded, so the oldest lines fell out.
    assert!(!mine.iter().any(|l| l.message == format!("{prefix} 0")), "the oldest line is still held");
    assert!(recent(TAIL + 100).len() <= TAIL);
}

/// One day's file stops growing at its cap: one line says so, and the
/// lines after it are kept in memory only.
#[test]
fn a_full_log_file_stops_growing_and_says_why() {
    let _log = crate::serial::log_tail();
    let dir = tempfile::tempdir().unwrap();
    v2_lib::applog::init(dir.path().to_path_buf());
    v2_lib::applog::set_file_cap(400);
    for i in 0..50 {
        log("debug", format!("applog-cap-test line {i} with some length to it"));
    }
    let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
    assert_eq!(files.len(), 1, "one file for the day");
    let text = std::fs::read_to_string(files[0].path()).unwrap();
    v2_lib::applog::set_file_cap(v2_lib::applog::MAX_FILE_BYTES);
    assert!(text.len() < 400 + 300, "the file stopped near its cap: {} bytes", text.len());
    assert_eq!(text.matches("reached its size limit").count(), 1);
    // The memory tail still has every line.
    let tail: Vec<_> = recent(6000).into_iter().filter(|l| l.message.starts_with("applog-cap-test")).collect();
    assert_eq!(tail.len(), 50);
}
