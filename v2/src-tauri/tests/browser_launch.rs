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

// ---- Chrome as well as Edge --------------------------------------------

use v2_lib::browser::launch::{browser_candidates, Browser};

/// Chrome installs to the same two Program Files roots, under Google.
#[test]
fn chrome_is_looked_for_where_its_installers_put_it() {
    let found = browser_candidates(
        Browser::Chrome,
        r"C:\Program Files",
        r"C:\Program Files (x86)",
    );
    assert_eq!(
        found,
        vec![
            PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
        ]
    );
}

/// Asking for Edge still gets Edge - the existing behaviour is the
/// default, not something Chrome support quietly replaced.
#[test]
fn edge_is_still_reachable_through_the_same_door() {
    let found = browser_candidates(Browser::Edge, r"C:\PF", r"C:\PF86");
    assert_eq!(
        found,
        edge_candidates(r"C:\PF", r"C:\PF86"),
        "the enum path and the original helper must agree"
    );
}

/// Both browsers are Chromium, so both take the same switches - the
/// arguments must not have quietly become Edge-specific.
#[test]
fn both_browsers_take_the_same_debugging_switches() {
    let args = launch_args(9444, Path::new(r"C:	mp\p"));
    assert!(args.contains(&"--remote-debugging-port=9444".to_string()));
    assert!(!args.iter().any(|a| a.to_lowercase().contains("edge")));
}

/// A name from settings, mapped once. An unknown value falls back to
/// Edge rather than failing to launch anything at all - the app is
/// Windows-first and Edge is the one browser guaranteed present.
#[test]
fn a_browser_name_maps_to_its_enum_and_falls_back_to_edge() {
    assert_eq!(Browser::from_name("chrome"), Browser::Chrome);
    assert_eq!(Browser::from_name("Chrome"), Browser::Chrome);
    assert_eq!(Browser::from_name("edge"), Browser::Edge);
    assert_eq!(Browser::from_name("firefox"), Browser::Edge);
    assert_eq!(Browser::from_name(""), Browser::Edge);
}
