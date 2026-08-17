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

/// Chrome installs to the two Program Files roots under Google - and,
/// when the installer ran without elevation, to the user's own profile.
/// That third path is not a nicety: on a locked-down work machine it is
/// the ONLY place Chrome ever lands.
#[test]
fn chrome_is_looked_for_where_its_installers_put_it() {
    let found = browser_candidates(
        Browser::Chrome,
        r"C:\Program Files",
        r"C:\Program Files (x86)",
        r"C:\Users\t\AppData\Local",
    );
    assert_eq!(
        found,
        vec![
            PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
            PathBuf::from(r"C:\Users\t\AppData\Local\Google\Chrome\Application\chrome.exe"),
        ]
    );
}

/// Edge is machine-wide by construction, so the per-user root is not
/// searched for it - a stat that could never succeed, on every launch.
#[test]
fn edge_is_not_looked_for_in_the_per_user_root() {
    let found = browser_candidates(
        Browser::Edge,
        r"C:\PF",
        r"C:\PF86",
        r"C:\Users\t\AppData\Local",
    );
    assert_eq!(
        found,
        vec![
            PathBuf::from(r"C:\PF\Microsoft\Edge\Application\msedge.exe"),
            PathBuf::from(r"C:\PF86\Microsoft\Edge\Application\msedge.exe"),
        ],
        "Edge picked up a per-user candidate it can never be installed at"
    );
}

/// The two browsers must not resolve to the same exe - a "run it in
/// Chrome" that quietly started Edge would produce a green result for a
/// browser nobody tested.
#[test]
fn the_two_browsers_resolve_to_different_executables() {
    let edge = browser_candidates(Browser::Edge, r"C:\PF", r"C:\PF86", r"C:\LAD");
    let chrome = browser_candidates(Browser::Chrome, r"C:\PF", r"C:\PF86", r"C:\LAD");
    assert!(
        edge.iter().all(|e| !chrome.contains(e)),
        "Edge and Chrome share a candidate path: {edge:?} vs {chrome:?}"
    );
    assert!(chrome.iter().all(|c| c.to_string_lossy().contains("chrome.exe")));
    assert!(edge.iter().all(|e| e.to_string_lossy().contains("msedge.exe")));
}

/// Both browsers are Chromium and share one `launch_args`, so the
/// switches must stay browser-neutral - an Edge-only flag here would
/// break every Chrome run at once.
#[test]
fn the_debugging_switches_name_no_particular_browser() {
    let args = launch_args(9444, Path::new(r"C:\tmp\p"));
    assert!(args.contains(&"--remote-debugging-port=9444".to_string()));
    assert!(args.iter().any(|a| a.starts_with("--user-data-dir=")));
    assert!(!args
        .iter()
        .any(|a| { let l = a.to_lowercase(); l.contains("edge") || l.contains("chrome") }));
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
