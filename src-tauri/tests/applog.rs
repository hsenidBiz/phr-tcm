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
