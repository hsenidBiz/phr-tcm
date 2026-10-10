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
    start_with(&["--headless=new"]).await
}

/// The same, with these switches (none: a visible window, as the
/// supervised browser and the recorder have).
async fn start_with(extra: &[&str]) -> LaunchedBrowser {
    let b = launch_with(Browser::Edge, extra).expect("Edge did not start");
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
    // Every one of them is told for the browser's own: the watcher, a
    // round later, has found nothing else in the job.
    std::thread::sleep(tree::WATCH_EVERY + Duration::from_millis(1500));
    assert_eq!(b.kills_on_close(), Some(true), "a browser process was taken for a program the person opened");
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

/// A program the person opens from a visible app browser (a download in
/// Excel, a link in their own Edge) is started by the browser and so lands
/// in its job. Closing the browser ends only the browser's own processes,
/// and the watcher takes kill-on-close off the job, so a crash of the app
/// would not end that program either. A harmless `ping` stands in for the
/// program, put in the job through the test seam; the test ends it itself.
#[tokio::test]
#[ignore = "starts a real Edge window"]
async fn a_program_opened_from_the_browser_is_left_running_when_the_browser_closes() {
    let _held = crate::serial::held_browsers();
    let _tail = crate::serial::log_tail();
    let b = start_with(&[]).await;
    // A visible browser alone, its helpers included, is all its own: a
    // watcher round later, kill-on-close is still on.
    std::thread::sleep(tree::WATCH_EVERY + Duration::from_millis(1500));
    assert_eq!(b.kills_on_close(), Some(true), "a process of the visible browser was taken for a program the person opened");
    let mut program = std::process::Command::new("ping")
        .args(["-n", "60", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("ping starts");
    let ended_it = scopeguard_kill(program.id());
    b.put_in_job(&program).expect("the program joins the browser's job");
    assert!(b.holds(program.id()));

    assert!(
        eventually(tree::WATCH_EVERY * 3, || b.kills_on_close() == Some(false)),
        "the watcher never took kill-on-close off the job"
    );
    let warned: Vec<String> = v2_lib::applog::recent(500)
        .into_iter()
        .map(|l| l.message)
        .filter(|m| m.contains("a program opened from the browser"))
        .collect();
    assert!(warned.iter().any(|m| m.to_ascii_lowercase().ends_with(": ping.exe")), "{warned:?}");
    assert!(warned.iter().all(|m| !m.to_ascii_lowercase().contains("system32")), "only the file name: {warned:?}");

    let profile = b.profile_dir.clone();
    assert!(b.close().is_ok(), "the browser's own processes ended, the program's presence notwithstanding");
    assert!(processes_on(&profile).is_empty(), "nothing of the browser is left");
    assert!(!profile.exists());
    assert!(program.try_wait().unwrap().is_none(), "the program the person opened is still running");
    drop(ended_it);
    let _ = program.wait();
}

/// Ends the test's own `ping` however the test ends, by its pid, through
/// its own handle: never any other process.
fn scopeguard_kill(pid: u32) -> impl Drop {
    struct KillOnDrop(u32);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = std::process::Command::new("taskkill").args(["/PID", &self.0.to_string(), "/F"]).output();
        }
    }
    KillOnDrop(pid)
}

/// The held browser's liveness (`held_browser_alive`) against a real Edge:
/// alive while it answers; gone once its main page is closed although its
/// processes still run (the report: the window closed, the app kept the
/// browser as held); gone once its processes have ended.
#[tokio::test]
#[ignore = "starts a real Edge"]
async fn a_held_browser_is_alive_only_while_its_own_page_answers() {
    use v2_lib::commands::autorun::held_browser_alive;
    let _held = crate::serial::held_browsers();
    let mut b = start().await;
    // A page of its own to drive: the first page Edge lists can be one of
    // its own dialogs.
    let mut cdp = Cdp::connect_browser(b.port).await.expect("connected");
    cdp.drive_new_page().await.expect("a page to drive");
    assert!(held_browser_alive(&mut b, &mut cdp).await, "a browser that answers was taken for gone");

    // Another tab keeps the browser running; the page the app drives closes.
    let mut other = Cdp::connect_browser(b.port).await.expect("a second connection");
    other.call("Target.createTarget", json!({ "url": "about:blank" })).await.expect("a second tab");
    // The page the app drives is closed from outside, as the person
    // closing its tab would.
    let main = cdp.current().expect("a main tab").target_id.clone();
    other.call("Target.closeTarget", json!({ "targetId": main })).await.expect("closed");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(b.processes().unwrap() > 0, "the browser itself should still run");
    assert!(
        !held_browser_alive(&mut b, &mut cdp).await,
        "a browser whose page closed is still taken for alive while its processes linger"
    );

    // Its processes ended: gone, without asking.
    let mut fresh = start().await;
    let mut fresh_cdp = Cdp::connect(fresh.port).await.expect("connected");
    assert!(fresh.end(), "ended");
    assert!(!held_browser_alive(&mut fresh, &mut fresh_cdp).await);
    drop(other);
    assert!(b.close().is_ok());
}

/// Release Auto Run browser against real processes: a browser the app
/// started is ended through its job, and a program outside the app's jobs
/// (a harmless `ping` standing in for the person's own Edge) is left
/// running.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "starts a real Edge"]
async fn release_closes_the_apps_browsers_and_nothing_else() {
    let _a = crate::serial::autorun();
    let _t = crate::serial::api_template_run();
    let _held = crate::serial::held_browsers();
    let b = start().await;
    let profile = b.profile_dir.clone();
    assert!(!processes_on(&profile).is_empty());
    let mut outside = std::process::Command::new("ping")
        .args(["-n", "60", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("ping starts");
    let ended_it = scopeguard_kill(outside.id());
    assert!(!b.holds(outside.id()));

    let said = v2_lib::commands::autorun::release_autorun_browsers().await.expect("release refused");
    assert!(said.contains("1 of the app's own browsers was closed"), "{said}");
    assert!(eventually(Duration::from_secs(8), || processes_on(&profile).is_empty()), "{:?}", processes_on(&profile));
    assert!(outside.try_wait().unwrap().is_none(), "a process outside the app's jobs was ended");
    drop(ended_it);
    let _ = outside.wait();
    drop(b);
}

/// The clear a sign-in makes before the next account (`session::clear`),
/// against a real Edge: a cookie of the account before, on the site's
/// domain, is gone afterwards. Only cookie names are compared.
#[tokio::test]
#[ignore = "starts a real Edge"]
async fn the_sign_in_clear_leaves_no_cookie_of_the_account_before() {
    let _held = crate::serial::held_browsers();
    let b = start().await;
    let mut cdp = Cdp::connect(b.port).await.expect("connected");
    cdp.call(
        "Network.setCookies",
        json!({ "cookies": [
            { "name": "ehrm85", "value": "previous-user", "domain": "hr.example.internal", "path": "/" },
            { "name": ".ASPXAUTH", "value": "previous-user", "domain": "hr.example.internal", "path": "/", "httpOnly": true }
        ] }),
    )
    .await
    .expect("cookies set");
    let names = |v: &serde_json::Value| -> Vec<String> {
        v["cookies"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str().map(str::to_string)).collect()
    };
    let before = cdp.call("Network.getAllCookies", json!({})).await.unwrap();
    assert!(names(&before).contains(&"ehrm85".to_string()), "{:?}", names(&before));
    v2_lib::browser::session::clear(&mut cdp, &["https://hr.example.internal".to_string()]).await.expect("cleared");
    let after = cdp.call("Network.getAllCookies", json!({})).await.unwrap();
    assert!(names(&after).is_empty(), "a cookie survived the clear: {:?}", names(&after));
    assert!(b.close().is_ok());
}
