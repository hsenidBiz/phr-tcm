//! A launched browser as a whole process tree, against a REAL Edge.
//! Ignored by default, as `browser_live` is: each test starts and ends
//! its own headless Edge, and looks at it from outside. Run on purpose:
//!
//!   cargo test --test suite browser_live_tree:: -- --ignored --test-threads=1
//!
//! Processes are only ever looked at here (their command lines, read with
//! Get-CimInstance), never ended from outside: a browser is ended only
//! through the job the app put it in. The person's own Edge, which is in
//! none of these jobs, is never touched.

#![cfg(windows)]

use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};
use v2_lib::autorun::replay::Browsers;
use v2_lib::browser::actions::{execute_with, Action, ActionOutcome};
use v2_lib::browser::cdp::Cdp;
use v2_lib::browser::launch::{launch_with, Browser, LaunchedBrowser};
use v2_lib::browser::page;
use v2_lib::browser::timing::Timing;
use v2_lib::browser::tree;

/// Every process on the machine running on this profile folder, by pid,
/// read from the process list (read only).
fn processes_on(profile: &Path) -> Vec<u32> {
    let name = profile.file_name().unwrap().to_string_lossy().into_owned();
    let script = format!(
        "Get-CimInstance Win32_Process -Filter \"Name='msedge.exe'\" | Where-Object {{ $_.CommandLine -match '{name}(\\\\|\\s|\"|$)' }} | ForEach-Object {{ $_.ProcessId }}"
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .expect("powershell runs");
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()
}

/// Wait until `f` holds, up to `within`.
fn eventually(within: Duration, mut f: impl FnMut() -> bool) -> bool {
    let began = Instant::now();
    loop {
        if f() {
            return true;
        }
        if began.elapsed() >= within {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

async fn answers(port: u16) -> bool {
    matches!(tokio::time::timeout(Duration::from_secs(3), Cdp::answers(port)).await, Ok(Ok(())))
}

/// A headless Edge, once its DevTools port answers, and a moment more for
/// its gpu, utility and renderer processes to start.
async fn start() -> LaunchedBrowser {
    let b = launch_with(Browser::Edge, &["--headless=new"]).expect("Edge did not start");
    for _ in 0..60 {
        if answers(b.port).await {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            return b;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let _ = b.close();
    panic!("Edge never answered");
}

#[tokio::test]
#[ignore = "starts a real Edge"]
async fn a_launched_browser_and_all_its_children_are_in_its_job() {
    let _held = crate::serial::held_browsers();
    let b = start().await;
    let pids = processes_on(&b.profile_dir);
    assert!(pids.len() > 1, "a browser is many processes: {pids:?}");
    for pid in &pids {
        assert!(b.holds(*pid), "process {pid} on this profile is outside the browser's job");
    }
    assert!(b.processes().unwrap() as usize >= pids.len(), "{:?} vs {pids:?}", b.processes());
    b.close().map_err(|_| ()).expect("closed");
}

#[tokio::test]
#[ignore = "starts a real Edge"]
async fn closing_a_browser_leaves_no_process_and_removes_its_profile() {
    let _held = crate::serial::held_browsers();
    let _tail = crate::serial::log_tail();
    let b = start().await;
    let profile = b.profile_dir.clone();
    assert!(!processes_on(&profile).is_empty());
    let name = profile.file_name().unwrap().to_string_lossy().into_owned();
    assert!(b.close().is_ok(), "every process ended");
    assert!(processes_on(&profile).is_empty(), "nothing left on the profile");
    assert!(!profile.exists(), "the profile is gone");
    let complaints: Vec<String> = v2_lib::applog::recent(500)
        .into_iter()
        .map(|l| l.message)
        .filter(|m| m.contains(&name) || m.contains("os error 32"))
        .collect();
    assert!(complaints.is_empty(), "{complaints:?}");
}

#[tokio::test]
#[ignore = "starts a real Edge"]
async fn closing_one_browser_leaves_another_browser_running() {
    let _held = crate::serial::held_browsers();
    let first = start().await;
    let other = start().await;
    assert!(first.close().is_ok());
    assert!(other.processes().unwrap() > 0, "the other browser's job still has processes");
    assert!(answers(other.port).await, "and it still answers");
    // Its renderers come and go on their own; its first process is the one
    // that would have gone with a kill aimed wrongly.
    let still: Vec<u32> = processes_on(&other.profile_dir);
    assert!(still.contains(&other.pid()), "the other browser's first process {} is gone: {still:?}", other.pid());
    assert!(other.close().is_ok());
}

/// The backstop: a browser the app forgets (or an app that crashes) closes
/// the job's last handle, and Windows ends the whole tree.
#[tokio::test]
#[ignore = "starts a real Edge"]
async fn dropping_the_job_handle_kills_the_tree() {
    let _held = crate::serial::held_browsers();
    let b = start().await;
    let profile = b.profile_dir.clone();
    assert!(!processes_on(&profile).is_empty());
    drop(b);
    assert!(eventually(Duration::from_secs(8), || processes_on(&profile).is_empty()), "{:?}", processes_on(&profile));
    assert!(tree::remove_profile(&profile).is_ok());
}

fn fixture_url() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/autorun-live.html").replace('\\', "/");
    format!("file:///{}", path.trim_start_matches('/').replace(' ', "%20"))
}

async fn act(d: &mut Cdp, action: serde_json::Value) -> ActionOutcome {
    let timing = Timing { action_ms: 4000, expect_ms: 3000, nav_ms: 15000, poll_ms: 100, highlight_ms: 0, lease_wait_ms: 300 };
    let action: Action = serde_json::from_value(action).expect("a valid action");
    execute_with(d, &action, &timing).await
}

/// The real unattended run's browser (`run_browsers`): a case that loads a
/// page, reloads it, opens a tab and closes it leaves the browser fit for
/// the next case, which runs in the same browser rather than a new one.
#[tokio::test]
#[ignore = "starts a real Edge"]
async fn a_reload_and_a_tab_open_close_keep_the_browser_for_the_next_case() {
    let _held = crate::serial::held_browsers();
    let before = tree::held_profiles();
    let mut run = v2_lib::commands::autorun_replay::run_browsers(Browser::Edge, false);

    let mut d = run.open().await.expect("a page for case 1");
    let started: Vec<_> = tree::held_profiles().into_iter().filter(|p| !before.contains(p)).collect();
    assert_eq!(started.len(), 1, "one browser for case 1: {started:?}");
    for action in [
        json!({ "kind": "navigate", "url": fixture_url() }),
        json!({ "kind": "reload" }),
        json!({ "kind": "open_tab", "name": "second", "url": fixture_url() }),
        json!({ "kind": "close_tab", "name": "second" }),
    ] {
        let out = act(&mut d, action.clone()).await;
        assert!(out.ok, "{action}: {out:?}");
    }
    run.close(d).await;

    let mut d = run.open().await.expect("a page for case 2");
    let now: Vec<_> = tree::held_profiles().into_iter().filter(|p| !before.contains(p)).collect();
    assert_eq!(now, started, "case 2 runs in the same browser");
    assert_eq!(page::eval_value(&mut d, "1 + 1").await.unwrap(), json!(2));
    run.close(d).await;

    drop(run);
    let profile = &started[0];
    assert!(processes_on(profile).is_empty(), "the run's browser ended with the run");
    assert!(!profile.exists());
}
