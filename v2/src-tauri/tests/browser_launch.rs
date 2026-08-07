//! Finding and launching the browser the tests will be watched in. The
//! pure parts - where Edge lives, what arguments it gets, picking a port
//! - are unit tested here; actually starting a browser is an ignored
//! test, like the updater's live download check.

use v2_lib::browser::launch::{edge_candidates, free_port, launch_args};
use std::path::{Path, PathBuf};

/// Both Program Files roots are searched, 64-bit first: an Edge in
/// "Program Files" is the current install, the x86 one is the legacy
/// location that some machines still carry.
#[test]
fn edge_is_looked_for_in_both_program_files_roots() {
    let found = edge_candidates(r"C:\Program Files", r"C:\Program Files (x86)");
    assert_eq!(
        found,
        vec![
            PathBuf::from(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
        ]
    );
}

/// The launch has to be debuggable, isolated, and VISIBLE. A headless
/// flag here would defeat the whole feature - the human is the oracle.
#[test]
fn the_launch_is_debuggable_isolated_and_never_headless() {
    let args = launch_args(9333, Path::new(r"C:\tmp\profile-1"));
    assert!(args.contains(&"--remote-debugging-port=9333".to_string()));
    assert!(args.contains(&r"--user-data-dir=C:\tmp\profile-1".to_string()));
    assert!(args.contains(&"--no-first-run".to_string()));
    assert!(
        !args.iter().any(|a| a.contains("headless")),
        "the run must be watchable: {args:?}"
    );
}

/// A port nobody else holds, so two runs (or a stale browser) cannot
/// collide.
#[test]
fn free_port_returns_a_usable_port() {
    let a = free_port().unwrap();
    let b = free_port().unwrap();
    assert!(a > 1024, "expected a high port, got {a}");
    assert!(b > 1024, "expected a high port, got {b}");
}

#[test]
#[ignore = "starts a real Edge window"]
fn launch_starts_a_browser_that_answers_on_its_port() {
    let mut b = v2_lib::browser::launch::launch().unwrap();
    let url = format!("http://127.0.0.1:{}/json/version", b.port);
    let body = reqwest::blocking::get(&url).unwrap().text().unwrap();
    assert!(body.contains("webSocketDebuggerUrl"), "got: {body}");
    let _ = b.child.kill();
}
