//! Per-command timing for the Playwright tab: one debug line per command,
//! its name and milliseconds only.

use std::time::Duration;
use v2_lib::pw_repo::{timed, timing_line};

#[test]
fn the_timing_line_is_the_name_and_the_milliseconds_only() {
    assert_eq!(timing_line("pw_list", Duration::from_millis(1234)), "pw-timing: pw_list took 1234 ms");
}

#[test]
fn a_dropped_guard_logs_its_line_at_debug_level() {
    let _g = crate::serial::log_tail();
    drop(timed("pw_reports"));
    let line = v2_lib::applog::recent(50)
        .into_iter()
        .rev()
        .find(|l| l.message.starts_with("pw-timing: pw_reports took "))
        .expect("the timing line");
    assert_eq!(line.level, "debug");
}
