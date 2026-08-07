//! The session's own rule: a step cannot run without an open browser,
//! and the failure says how to fix it rather than panicking on an
//! absent session.

use v2_lib::commands::autorun::{describe_session_error, safe_run_id};

#[test]
fn stepping_without_a_browser_says_to_open_one() {
    let msg = describe_session_error();
    assert!(msg.to_lowercase().contains("open"), "unhelpful: {msg}");
    assert!(msg.to_lowercase().contains("browser"), "unhelpful: {msg}");
}

/// `store::save_run` writes the id straight into a filename with no
/// sanitisation of its own; `auto_run_save_run` is the id's first
/// IPC-facing entry point, so a hostile or buggy frontend value must be
/// rejected before it ever reaches the filesystem.
#[test]
fn run_ids_that_could_escape_the_runs_directory_are_rejected() {
    assert!(safe_run_id("run-1738972800000"));
    assert!(!safe_run_id(""));
    assert!(!safe_run_id("../../evil"));
    assert!(!safe_run_id("nested/path"));
    assert!(!safe_run_id("back\\slash"));
    assert!(!safe_run_id("."));
    assert!(!safe_run_id(".."));
}
