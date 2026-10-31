//! Finding and launching the browser the tests will be watched in. The
//! pure parts - where Edge lives, what arguments it gets, picking a port
//! - are unit tested here; actually starting a browser is an ignored
//! test, like the updater's live download check.

use v2_lib::browser::launch::{
    args_with, background_args, edge_candidates, free_port, launch_args, window_position_switch,
};
use v2_lib::browser::viewport::{device_metrics_params, DEVICE_SCALE, PAGE_HEIGHT, PAGE_WIDTH};
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

/// Chromium throttles a minimised or covered window's timers to once a
/// minute and stops painting it. The app under test is what suffers: its
/// debounces and toasts crawl, and a run that passed while somebody
/// watched fails the moment the window goes behind another. These three
/// switches are the only thing that keeps a background run honest, so
/// they are pinned here rather than left to whoever edits the list next.
#[test]
fn the_page_under_test_is_never_throttled_for_being_in_the_background() {
    let args = launch_args(9555, Path::new(r"C:\tmp\p"));
    for flag in [
        "--disable-background-timer-throttling",
        "--disable-backgrounding-occluded-windows",
        "--disable-renderer-backgrounding",
    ] {
        assert!(args.contains(&flag.to_string()), "{flag} is missing from {args:?}");
    }
}

/// Edge signs a new profile into the Windows account and syncs it, so the
/// person's extensions arrive in the throwaway profile and one opens a tab
/// over the page under test, which then stops drawing and stops answering
/// role lookups (`browser_live::role_lookups_keep_answering_...`).
#[test]
fn the_throwaway_profile_is_never_synced() {
    let args = launch_args(9666, Path::new(r"C:\tmp\p"));
    assert!(args.contains(&"--disable-sync".to_string()), "{args:?}");
}

/// Extra switches are for the live tests only (they run headless so they
/// do not take over the desktop). The app's own arguments must stay
/// exactly as they were, with the start page last.
#[test]
fn extra_switches_go_before_the_start_page() {
    let dir = PathBuf::from("C:/tmp/p");
    let args = args_with(9222, &dir, &["--headless=new"]);
    assert_eq!(args.last().map(String::as_str), Some("about:blank"));
    assert!(args.contains(&"--headless=new".to_string()));
    assert_eq!(args_with(9222, &dir, &[]), launch_args(9222, &dir));
}

/// An extra assistant's browser opens offset from the main window, so two
/// windows are never exactly on top of each other: the position goes through
/// the extra switches (the path `launch_in_at` takes), before the start page.
/// Main asks for none, so its launch is exactly what it was.
#[test]
fn a_positioned_launch_names_its_window_position() {
    let dir = PathBuf::from("C:/tmp/p");
    assert_eq!(window_position_switch(None), None);
    assert_eq!(window_position_switch(Some((60, 60))).as_deref(), Some("--window-position=60,60"));
    assert_eq!(window_position_switch(Some((120, 120))).as_deref(), Some("--window-position=120,120"));
    let args = args_with(9222, &dir, &["--window-position=60,60"]);
    assert!(args.contains(&"--window-position=60,60".to_string()), "{args:?}");
    assert_eq!(args.last().map(String::as_str), Some("about:blank"));
    // The switch is added to the app's own arguments, none removed.
    assert_eq!(args.len(), launch_args(9222, &dir).len() + 1);
    assert!(!args.contains(&"--headless=new".to_string()), "a positioned window is a visible one");
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
    let b = v2_lib::browser::launch::launch().unwrap();
    let url = format!("http://127.0.0.1:{}/json/version", b.port);
    let body = reqwest::blocking::get(&url).unwrap().text().unwrap();
    assert!(body.contains("webSocketDebuggerUrl"), "got: {body}");
    let _ = b.close();
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

// ---- Unattended replay runs in the background --------------------------

/// An unattended run has nobody to watch it, so it opens headless in the
/// window every browser the app drives starts with (a 1280 x 720 page and
/// the browser's frame around it, from the `viewport` constants) - a
/// watched run (supervised, or unattended with "watch" asked for) is not
/// headless.
#[test]
fn a_background_browser_is_headless_in_the_window_the_page_size_asks_for() {
    let extra = background_args();
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    let args = args_with(9333, Path::new("C:/tmp/p"), &extra);
    assert!(args.iter().any(|a| a == "--headless=new"));
    let want = format!("--window-size={},{}", PAGE_WIDTH + 16, PAGE_HEIGHT + 140);
    assert!(args.iter().any(|a| *a == want), "{want} is missing from {args:?}");
    assert!(!args.iter().any(|a| a == "--window-size=1366,900"), "{args:?}");
    // Big enough for the page with the frame around it, never smaller.
    let (w, h) = want["--window-size=".len()..].split_once(',').unwrap();
    assert!(w.parse::<u32>().unwrap() > PAGE_WIDTH && h.parse::<u32>().unwrap() > PAGE_HEIGHT);
    // And a watched one has neither.
    let shown = args_with(9333, Path::new("C:/tmp/p"), &[]);
    assert!(!shown.iter().any(|a| a.starts_with("--headless")));
}

// ---- A 1280 x 720 page at scale 1 --------------------------------------

/// A high-DPI screen must not scale the page, and a headed window must be
/// big enough to hold the page with the browser's own frame around it.
#[test]
fn every_browser_launches_at_device_scale_one_in_a_window_that_holds_the_page() {
    let args = launch_args(9333, Path::new(r"C:	mp\p"));
    assert!(args.contains(&"--force-device-scale-factor=1".to_string()), "{args:?}");
    let size = args.iter().find_map(|a| a.strip_prefix("--window-size=")).expect("no window size");
    let (w, h) = size.split_once(',').unwrap();
    assert!(w.parse::<u32>().unwrap() > PAGE_WIDTH && h.parse::<u32>().unwrap() > PAGE_HEIGHT, "{size}");
    // Switches only: the start page stays last.
    assert_eq!(args.last().map(String::as_str), Some("about:blank"));
}

/// The numbers are Playwright's default, and sit in one place.
#[test]
fn the_page_area_is_1280_by_720_at_scale_one() {
    assert_eq!((PAGE_WIDTH, PAGE_HEIGHT, DEVICE_SCALE), (1280, 720, 1.0));
    assert_eq!(
        device_metrics_params(),
        serde_json::json!({ "width": 1280, "height": 720, "deviceScaleFactor": 1.0, "mobile": false })
    );
}

// ---- password prompts off (backlog 44) ----

use v2_lib::browser::launch::{
    close_hung, password_prefs, prefs_failure_is_logged_only, write_password_prefs, PASSWORD_FEATURES_OFF,
};
use v2_lib::browser::tree::{Orphan, Owner};

fn disable_features(args: &[String]) -> Vec<&String> {
    args.iter().filter(|a| a.starts_with("--disable-features=")).collect()
}

#[test]
fn launch_args_disable_password_leak_detection_once() {
    let args = launch_args(9222, Path::new(r"C:\tmp\p"));
    let found = disable_features(&args);
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("PasswordLeakDetection"));
    assert_eq!(PASSWORD_FEATURES_OFF, "PasswordLeakDetection");
    assert_eq!(args.last().map(String::as_str), Some("about:blank"));
}

#[test]
fn args_with_folds_extra_disable_features_into_one_switch() {
    let args = args_with(9222, Path::new(r"C:\tmp\p"), &["--disable-features=Foo"]);
    let found = disable_features(&args);
    assert_eq!(found, vec![&"--disable-features=PasswordLeakDetection,Foo".to_string()]);
    assert_eq!(args.last().map(String::as_str), Some("about:blank"));
}

fn three_off(v: &serde_json::Value) {
    assert_eq!(v["credentials_enable_service"], false);
    assert_eq!(v["profile"]["password_manager_enabled"], false);
    assert_eq!(v["profile"]["password_manager_leak_detection"], false);
}

#[test]
fn password_prefs_on_empty_profile() {
    three_off(&password_prefs(None));
}

#[test]
fn password_prefs_merge_keeps_existing_keys() {
    let v = password_prefs(Some(
        r#"{"other":1,"profile":{"name":"x","password_manager_enabled":true},"credentials_enable_service":true}"#,
    ));
    assert_eq!(v["other"], 1);
    assert_eq!(v["profile"]["name"], "x");
    three_off(&v);
}

#[test]
fn password_prefs_replace_unreadable_file() {
    three_off(&password_prefs(Some("not json")));
    three_off(&password_prefs(Some("[1,2]")));
    three_off(&password_prefs(Some(r#"{"profile":5}"#)));
}

#[test]
fn write_password_prefs_creates_default_preferences() {
    let dir = tempfile::tempdir().unwrap();
    write_password_prefs(dir.path()).unwrap();
    let file = dir.path().join("Default").join("Preferences");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    three_off(&v);
    let mut kept = v.clone();
    kept["signed_in_marker"] = serde_json::json!("keep");
    std::fs::write(&file, kept.to_string()).unwrap();
    write_password_prefs(dir.path()).unwrap();
    let again: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(again["signed_in_marker"], "keep");
    three_off(&again);
}

#[test]
fn launch_goes_on_when_prefs_cannot_be_written() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Default"), "a file where the folder should be").unwrap();
    let e = write_password_prefs(dir.path()).unwrap_err();
    let line = prefs_failure_is_logged_only(&e);
    assert!(line.contains("password prompts"));
    assert!(!line.contains(&dir.path().to_string_lossy().to_string()));
}

fn orphan(pid: u32, port: Option<u16>) -> Orphan {
    Orphan { pid, profile: format!("tcm-autorun-{}", port.unwrap_or(0)), port, age_ms: Some(600_000), owner: None }
}

#[test]
fn close_hung_ends_only_silent_ones() {
    let orphans = vec![
        orphan(1, Some(1)),
        orphan(2, Some(2)),
        Orphan { pid: 3, profile: "tcm-autorun-3".into(), port: None, age_ms: Some(600_000), owner: None },
    ];
    let mut ended = Vec::new();
    let hung = close_hung(orphans, &|port| port == 2, &mut |o: &Orphan| {
        ended.push(o.profile.clone());
        true
    });
    assert_eq!(ended, vec!["tcm-autorun-1".to_string()]);
    assert_eq!(hung.closed, vec!["tcm-autorun-1".to_string()]);
    assert!(hung.stuck.is_empty());
}

#[test]
fn close_hung_keeps_an_answering_orphan() {
    let mut ended = 0;
    let hung = close_hung(vec![orphan(2, Some(2))], &|_| true, &mut |_: &Orphan| {
        ended += 1;
        true
    });
    assert_eq!(ended, 0);
    assert!(hung.closed.is_empty() && hung.stuck.is_empty());
}

#[test]
fn close_hung_names_one_that_would_not_end() {
    let hung = close_hung(vec![orphan(1, Some(1))], &|_| false, &mut |_: &Orphan| false);
    assert!(hung.closed.is_empty());
    assert_eq!(hung.stuck, vec!["tcm-autorun-1".to_string()]);
}

#[test]
fn an_unendable_orphan_is_remembered_and_not_tried_again() {
    use v2_lib::browser::launch::{forget_unendable, remember_unendable, still_to_try};
    let name = "tcm-autorun-unendable-test-1";
    let mut o = orphan(1, Some(1));
    o.profile = name.to_string();
    assert_eq!(still_to_try(vec![o.clone()]).len(), 1);
    remember_unendable(name);
    assert!(still_to_try(vec![o.clone()]).is_empty());
    forget_unendable(name);
    assert_eq!(still_to_try(vec![o]).len(), 1);
}

fn gone(_: u32) -> Option<u64> {
    None
}

#[test]
fn a_young_browser_is_never_ended() {
    use v2_lib::browser::tree::may_end;
    let mut o = orphan(1, Some(1));
    o.age_ms = Some(89_999);
    assert!(!may_end(&o, 10, &gone));
    o.age_ms = None;
    assert!(!may_end(&o, 10, &gone));
    o.age_ms = Some(90_000);
    assert!(may_end(&o, 10, &gone));
}

#[test]
fn a_browser_owned_by_another_running_app_is_left() {
    use v2_lib::browser::tree::may_end;
    let mut o = orphan(1, Some(1));
    o.owner = Some(Owner { pid: 500, started_ms: 1234 });
    assert!(!may_end(&o, 10, &|pid| (pid == 500).then_some(1234)));
}

#[test]
fn a_browser_whose_owner_is_gone_or_reused_is_ended() {
    use v2_lib::browser::tree::may_end;
    let mut o = orphan(1, Some(1));
    o.owner = Some(Owner { pid: 500, started_ms: 1234 });
    assert!(may_end(&o, 10, &gone));
    // The pid runs again, but it is another program that started later.
    assert!(may_end(&o, 10, &|_| Some(9999)));
}

#[test]
fn a_browser_with_no_marker_is_judged_on_age_alone() {
    use v2_lib::browser::tree::may_end;
    assert!(may_end(&orphan(1, Some(1)), 10, &gone));
}

#[test]
fn a_leftover_of_this_very_app_is_ended() {
    use v2_lib::browser::tree::may_end;
    let mut o = orphan(1, Some(1));
    o.owner = Some(Owner { pid: 10, started_ms: 77 });
    assert!(may_end(&o, 10, &|pid| (pid == 10).then_some(77)));
}

#[test]
fn the_owner_marker_round_trips_and_garbage_is_nothing() {
    use v2_lib::browser::tree::{owner_marker_text, parse_owner_marker};
    let owner = Owner { pid: 42, started_ms: 1_700_000_000_000 };
    assert_eq!(parse_owner_marker(&owner_marker_text(&owner)), Some(owner));
    assert_eq!(parse_owner_marker("not json"), None);
    assert_eq!(parse_owner_marker("{}"), None);
}

#[test]
fn launch_writes_an_owner_marker_into_the_profile() {
    use v2_lib::browser::launch::write_owner_marker;
    let dir = tempfile::tempdir().unwrap();
    write_owner_marker(dir.path()).unwrap();
    let text = std::fs::read_to_string(dir.path().join("tcm-owner.json")).unwrap();
    let owner = v2_lib::browser::tree::parse_owner_marker(&text).unwrap();
    assert_eq!(owner.pid, std::process::id());
}

#[test]
fn the_liveness_probe_is_the_apps_own_rule() {
    use v2_lib::browser::launch::{ANSWER_TRIES, ANSWER_WITHIN};
    assert_eq!(ANSWER_TRIES, 2);
    assert_eq!(ANSWER_WITHIN, std::time::Duration::from_secs(3));
}
