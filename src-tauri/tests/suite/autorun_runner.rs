//! The pure step loop: what `auto_run_step` used to do inline, now
//! testable directly. `sign_in` is carried out here (the executor only
//! validates it), and a failed `sign_in` stops the rest of the step - the
//! actions after it would run as the wrong person, or as nobody.

use crate::common;

use common::{account, quick, recipe, stateful_app, ScriptedDriver};
use serde_json::json;
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::recipe::{save_recipe, SignInRecipe};
use v2_lib::autorun::runner::{as_action_outcome, policy_for, run_step};
use v2_lib::autorun::signin::SignInOutcome;
use v2_lib::autorun::StepScript;
use v2_lib::browser::actions::Action;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::downloads::{DownloadEntry, DownloadState};
use std::time::{Duration, Instant};

#[test]
fn a_project_with_no_recipe_runs_unrestricted_and_one_with_a_recipe_does_not() {
    assert!(policy_for(None).allows("https://anywhere.example/"));
    let recipe: SignInRecipe = serde_json::from_value(serde_json::json!({
        "start_url": "https://hr.example.internal/", "steps": [{ "kind": "check_url", "contains": "/" }],
        "signed_in": { "css": "#m" }, "allowed_origins": ["https://sso.example.internal"]
    }))
    .unwrap();
    let p = policy_for(Some(&recipe));
    assert!(p.allows("https://hr.example.internal/x") && p.allows("https://sso.example.internal/y"));
    assert!(!p.allows("https://anywhere.example/"));
}

#[test]
fn a_sign_in_reads_as_one_action_outcome() {
    let out = SignInOutcome {
        ok: false,
        detail: "sign-in stopped at step 2: not found".into(),
        used_saved_session: false,
        steps: vec![],
        harness: false,
        appeared: vec![],
    };
    let a = as_action_outcome(&out);
    assert!(!a.ok && a.detail.contains("step 2") && a.screenshot.is_none());
    assert!(!a.harness);

    // A harness failure carries over too - `run_step` must not then ask a
    // browser that already failed to answer for a screenshot.
    let harness_out = SignInOutcome {
        ok: false,
        detail: "the browser did not answer: closed".into(),
        used_saved_session: false,
        steps: vec![],
        harness: true,
        appeared: vec![],
    };
    assert!(as_action_outcome(&harness_out).harness);
}

fn step(actions: Vec<Action>) -> StepScript {
    StepScript { step_number: 1, actions, unchecked: None }
}

#[tokio::test]
async fn with_no_recipe_saved_a_step_of_ordinary_actions_runs_and_navigate_goes_anywhere() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Page.navigate" => Ok(json!({ "frameId": "F", "loaderId": "L" })),
        _ => Ok(json!({})),
    });
    d.on_call_events.push((
        "Page.navigate".into(),
        v2_lib::browser::cdp::Event {
            method: "Page.lifecycleEvent".into(),
            params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }),
        },
    ));
    let mut acc: Option<String> = None;
    let out = run_step(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![Action::Navigate { url: "https://anywhere.example/".into() }]),
        &quick(),
        &mut acc,
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].ok, "{:?}", out[0]);
}

#[tokio::test]
async fn with_a_recipe_saved_navigate_outside_its_origins_is_refused_and_never_reaches_the_browser() {
    let dir = tempfile::tempdir().unwrap();
    save_recipe(dir.path(), "Acme", "Web", &recipe()).unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let mut acc: Option<String> = None;
    let out = run_step(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![Action::Navigate { url: "https://anywhere.example/".into() }]),
        &quick(),
        &mut acc,
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 1);
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(d.calls_to("Page.navigate").is_empty());
}

#[tokio::test]
async fn a_sign_in_for_an_unknown_account_fails_and_stops_the_rest_of_the_step() {
    let dir = tempfile::tempdir().unwrap();
    save_recipe(dir.path(), "Acme", "Web", &recipe()).unwrap();
    // No accounts saved at all.
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let mut acc: Option<String> = Some("someone".into());
    let out = run_step(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![
            Action::SignIn { account: "ghost".into() },
            Action::CheckText { value: "ok".into() },
        ]),
        &quick(),
        &mut acc,
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 2);
    assert!(!out[0].ok && out[0].detail.contains("Accounts"), "{:?}", out[0]);
    assert!(!out[1].ok && out[1].detail.contains("not run"), "{:?}", out[1]);
    assert_eq!(acc, None);
    // The failed sign_in's own outcome may still take a failure screenshot
    // (the browser is still there); the check_text after it must not touch
    // the driver at all, since it was never run.
    assert!(
        d.calls.iter().all(|(m, _)| m == "Page.captureScreenshot"),
        "the driver must never be touched for a check_text after a failed sign_in: {:?}",
        d.calls
    );
}

#[tokio::test]
async fn a_sign_in_with_no_recipe_saved_fails_and_stops_the_rest_of_the_step() {
    let dir = tempfile::tempdir().unwrap();
    // No recipe saved for this project, and no site address for the
    // built-in one.
    save_accounts(dir.path(), &[account()]).unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let mut acc: Option<String> = None;
    let out = run_step(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![
            Action::SignIn { account: "admin".into() },
            Action::CheckText { value: "ok".into() },
        ]),
        &quick(),
        &mut acc,
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 2);
    assert!(!out[0].ok && out[0].detail.contains("set the site address first"), "{:?}", out[0]);
    assert!(!out[1].ok && out[1].detail.contains("not run"), "{:?}", out[1]);
    assert_eq!(acc, None);
    assert!(
        d.calls.iter().all(|(m, _)| m == "Page.captureScreenshot"),
        "the driver must never be touched for a check_text after a failed sign_in: {:?}",
        d.calls
    );
}

#[tokio::test]
async fn a_successful_sign_in_sets_the_account_and_the_actions_after_it_run() {
    let dir = tempfile::tempdir().unwrap();
    save_recipe(dir.path(), "Acme", "Web", &recipe()).unwrap();
    save_accounts(dir.path(), &[account()]).unwrap();
    let (mut d, _) = stateful_app(false, None);
    let mut acc: Option<String> = None;
    let out = run_step(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![
            Action::SignIn { account: "admin".into() },
            Action::CheckText { value: "ok".into() },
        ]),
        &quick(),
        &mut acc,
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 2);
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(acc.as_deref(), Some("admin"));
    // The action after the sign_in actually ran - it did not get the
    // "not run" placeholder.
    assert!(!out[1].detail.contains("not run"), "{:?}", out[1]);
}

#[tokio::test]
async fn an_ordinary_failed_action_does_not_stop_the_actions_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Page.navigate" => Ok(json!({ "frameId": "F", "loaderId": "L" })),
        _ => Ok(json!({})),
    });
    d.on_call_events.push((
        "Page.navigate".into(),
        v2_lib::browser::cdp::Event {
            method: "Page.lifecycleEvent".into(),
            params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }),
        },
    ));
    let mut acc: Option<String> = None;
    let out = run_step(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![
            Action::CheckText { value: "definitely not on the page".into() },
            Action::Navigate { url: "https://anywhere.example/".into() },
        ]),
        &quick(),
        &mut acc,
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 2);
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(!out[1].detail.contains("not run"), "{:?}", out[1]);
    assert!(!d.calls_to("Page.navigate").is_empty(), "the navigate after the failure still ran");
}

#[tokio::test]
async fn a_failed_page_action_gets_a_screenshot_and_a_harness_failure_gets_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Page.captureScreenshot" => Ok(json!({ "data": "/9j/4AAQ" })),
        _ => Ok(json!({})),
    });
    let mut acc: Option<String> = None;
    let out = run_step(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![Action::CheckText { value: "not on the page".into() }]),
        &quick(),
        &mut acc,
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 1);
    assert!(!out[0].ok);
    let name = out[0].screenshot.as_ref().expect("a page failure should carry a screenshot");
    assert!(dir.path().join("shots").join(name).is_file());

    // A harness failure (the transport itself failing) gets no screenshot.
    let mut d2 = ScriptedDriver::new(|method, _| match method {
        "Runtime.evaluate" => Err(v2_lib::browser::cdp::CdpError::Closed),
        _ => Ok(json!({})),
    });
    let mut acc2: Option<String> = None;
    let out2 = run_step(
        &mut d2,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![Action::CheckText { value: "x".into() }]),
        &quick(),
        &mut acc2,
    )
    .await
    .unwrap();
    assert_eq!(out2.len(), 1);
    assert!(!out2[0].ok);
    assert!(out2[0].screenshot.is_none(), "{:?}", out2[0]);
}

/// A `sign_in` that fails because the browser stopped answering (here, the
/// very first `clear` call) must carry `harness` through
/// `as_action_outcome`, or `run_step` tries to screenshot a browser that
/// is not there any more.
#[tokio::test]
async fn a_sign_in_whose_clear_fails_gets_no_screenshot() {
    let dir = tempfile::tempdir().unwrap();
    save_recipe(dir.path(), "Acme", "Web", &recipe()).unwrap();
    save_accounts(dir.path(), &[account()]).unwrap();
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Network.clearBrowserCookies" => Err(CdpError::Closed),
        _ => Ok(json!({})),
    });
    let mut acc: Option<String> = None;
    let out = run_step(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![Action::SignIn { account: "admin".into() }]),
        &quick(),
        &mut acc,
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 1);
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(
        d.calls_to("Page.captureScreenshot").is_empty(),
        "a browser that is not answering must never be asked for a screenshot: {:?}",
        d.calls
    );
}

#[tokio::test]
async fn with_addresses_switched_off_a_saved_navigate_fails_with_the_projects_sentence_and_stops_the_step() {
    let dir = tempfile::tempdir().unwrap();
    v2_lib::autorun::nav::save_nav(
        dir.path(),
        "Acme",
        "Web",
        &v2_lib::autorun::nav::NavFile { direct_urls: false, modules: vec![], save_words: vec![] },
    )
    .unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let mut acc: Option<String> = None;
    let step = StepScript {
        step_number: 4,
        actions: vec![Action::Navigate { url: "/hr/leave/apply".into() }, Action::CheckText { value: "Leave".into() }],
        unchecked: None,
    };
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step, &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, v2_lib::autorun::nav::no_address(4));
    assert_eq!(out[1].detail, "not run: this step opened a page by address, which this project does not allow");
    assert!(d.calls_to("Page.navigate").is_empty());
}

// ------------------------------------------------------------ expect_download

const EXPORT: &str = "Employee No,Name\r\nE001,Ada\r\n";

/// A download the browser followed: `name` is the page's suggested name,
/// kept at `file` (which may be numbered), started `offset` from now - a
/// positive one is during the step the test is about to run.
fn followed(file: &std::path::Path, name: &str, offset: i64, state: DownloadState) -> DownloadEntry {
    let now = Instant::now();
    let started_at = if offset >= 0 {
        now + Duration::from_millis(offset as u64)
    } else {
        now - Duration::from_millis(offset.unsigned_abs())
    };
    DownloadEntry {
        guid: format!("g-{name}-{offset}"),
        name: name.to_string(),
        path: file.to_path_buf(),
        started_at,
        state,
        bytes: std::fs::metadata(file).map(|m| m.len()).unwrap_or(0),
    }
}

fn download_step(check: serde_json::Value) -> StepScript {
    step(vec![serde_json::from_value(check).unwrap()])
}

#[tokio::test]
async fn a_download_from_this_step_is_checked_once_it_completes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report.csv");
    std::fs::write(&file, EXPORT).unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    d.downloads.push(followed(&file, "report.csv", 5000, DownloadState::Completed));
    let mut acc: Option<String> = None;
    let s = download_step(json!({ "kind": "expect_download", "name": "report*.csv", "within_ms": 2000,
        "headers": { "exact": ["Employee No", "Name"] }, "cells": [{ "ref": "B2", "text": "Ada" }] }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &s, &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{}", out[0].detail);
    assert_eq!(out[0].detail, "downloaded \"report.csv\" (28 bytes), headers match, B2 is \"Ada\"");
}

/// Review Focus 3: a download an earlier step started is never this
/// step's, finished or not.
#[tokio::test]
async fn a_download_that_started_before_the_step_is_not_this_steps() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report.csv");
    std::fs::write(&file, EXPORT).unwrap();
    for state in [DownloadState::Completed, DownloadState::InProgress] {
        let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
        d.downloads.push(followed(&file, "report.csv", -1000, state));
        let mut acc: Option<String> = None;
        let s = download_step(json!({ "kind": "expect_download", "name": "report.csv", "within_ms": 300 }));
        let out = run_step(&mut d, dir.path(), "Acme", "Web", &s, &quick(), &mut acc).await.unwrap();
        assert!(!out[0].ok);
        assert_eq!(out[0].detail, "no download started within 0.3 s");
    }
    // Whole seconds read as whole seconds.
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let mut acc: Option<String> = None;
    let s = download_step(json!({ "kind": "expect_download", "name": "report.csv", "within_ms": 1000 }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &s, &quick(), &mut acc).await.unwrap();
    assert_eq!(out[0].detail, "no download started within 1 s");
}

#[tokio::test]
async fn a_canceled_or_unfinished_download_fails_with_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report.csv");
    std::fs::write(&file, EXPORT).unwrap();
    let cases = [
        (DownloadState::Canceled, "the download \"report.csv\" was canceled"),
        (DownloadState::InProgress, "the download \"report.csv\" did not finish within 0.3 s"),
    ];
    for (state, want) in cases {
        let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
        d.downloads.push(followed(&file, "report.csv", 5000, state));
        let mut acc: Option<String> = None;
        let s = download_step(json!({ "kind": "expect_download", "name": "report.csv", "within_ms": 300 }));
        let out = run_step(&mut d, dir.path(), "Acme", "Web", &s, &quick(), &mut acc).await.unwrap();
        assert!(!out[0].ok);
        assert_eq!(out[0].detail, want);
    }
}

/// The first download of the step is the one checked: a wrong name fails,
/// it does not wait for another.
#[tokio::test]
async fn a_download_with_the_wrong_name_fails_and_says_both_names() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("other.csv");
    std::fs::write(&file, EXPORT).unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    d.downloads.push(followed(&file, "other.csv", 5000, DownloadState::Completed));
    let mut acc: Option<String> = None;
    let s = download_step(json!({ "kind": "expect_download", "name": "Template*.xlsx", "within_ms": 2000 }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &s, &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, "got \"other.csv\", expected a file named \"Template*.xlsx\"");
}

/// The step's record names the files saved during it, as they are kept on
/// disk (numbered when the name was taken); an earlier step's download
/// and a canceled one are not among them.
#[tokio::test]
async fn the_files_saved_during_a_step_are_on_its_record() {
    let dir = tempfile::tempdir().unwrap();
    let earlier = dir.path().join("report.csv");
    let this = dir.path().join("report (2).csv");
    std::fs::write(&earlier, EXPORT).unwrap();
    std::fs::write(&this, EXPORT).unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    d.downloads.push(followed(&earlier, "report.csv", -1000, DownloadState::Completed));
    d.downloads.push(followed(&this, "report.csv", 5000, DownloadState::Completed));
    d.downloads.push(followed(&dir.path().join("gone"), "late.csv", 5001, DownloadState::Canceled));
    let script: v2_lib::autorun::CaseScript = serde_json::from_value(json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "expect_download", "name": "report.csv", "within_ms": 2000 }] }
    ] }))
    .unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let rec = v2_lib::autorun::replay::run_case(&mut d, dir.path(), "Acme", "Web", &script, &quick(), &cancel, &mut |_| {})
        .await;
    let step = rec.steps.iter().find(|s| s.step_number == 1).unwrap();
    assert!(step.outcomes[0].ok, "{}", step.outcomes[0].detail);
    assert_eq!(step.downloads, ["report (2).csv"]);
    // Written only when there are some, so older run files read the same.
    let json = serde_json::to_value(step).unwrap();
    assert_eq!(json["downloads"], json!(["report (2).csv"]));
    let mut none = step.clone();
    none.downloads.clear();
    assert!(serde_json::to_value(&none).unwrap().get("downloads").is_none());
}

/// A Stop pressed while an unattended step waits for its download ends the
/// wait at the next look: the case stops there, as a stopped case does.
#[tokio::test]
async fn a_stop_ends_a_download_wait_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let script: v2_lib::autorun::CaseScript = serde_json::from_value(json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 1, "actions": [
            { "kind": "expect_download", "name": "report.csv", "within_ms": 10000 },
            { "kind": "check_text", "value": "Saved" }
        ] },
        { "step_number": 2, "actions": [{ "kind": "check_text", "value": "Done" }] }
    ] }))
    .unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let started = Instant::now();
    let stop = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    };
    let timing = quick();
    let mut on_step = |_| {};
    let (rec, ()) = tokio::join!(
        v2_lib::autorun::replay::run_case(&mut d, dir.path(), "Acme", "Web", &script, &timing, &cancel, &mut on_step),
        stop
    );
    assert!(started.elapsed() < Duration::from_secs(1), "took {:?}", started.elapsed());
    let stopped = "not run: the run was stopped";
    assert_eq!(rec.steps[0].outcomes[0].detail, stopped);
    assert_eq!(rec.steps[0].outcomes[1].detail, stopped);
    assert!(rec.steps[0].outcomes[0].screenshot.is_none() && rec.steps[0].screenshot.is_none());
    assert_eq!(rec.steps[1].outcomes[0].detail, stopped);
    assert_eq!((rec.proposed.as_str(), rec.reason.as_str()), ("", "stopped before it finished"));
}

/// Review follow-up 3: a download the browser was heard to start while the
/// step read what had already arrived (its settle) is an earlier step's.
/// The step's check and its record agree: it belongs to neither.
#[tokio::test]
async fn a_download_heard_during_the_settle_belongs_to_neither_the_check_nor_the_record() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report.csv");
    std::fs::write(&file, EXPORT).unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    d.downloads_on_call
        .push(("Runtime.evaluate".into(), followed(&file, "report.csv", 0, DownloadState::Completed)));
    let script: v2_lib::autorun::CaseScript = serde_json::from_value(json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "expect_download", "name": "report.csv", "within_ms": 300 }] }
    ] }))
    .unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let rec = v2_lib::autorun::replay::run_case(&mut d, dir.path(), "Acme", "Web", &script, &quick(), &cancel, &mut |_| {})
        .await;
    assert_eq!(d.downloads.len(), 1, "the settle heard the download");
    assert_eq!(rec.steps[0].outcomes[0].detail, "no download started within 0.3 s");
    assert!(rec.steps[0].downloads.is_empty(), "{:?}", rec.steps[0].downloads);
}

/// A watched run or a try has no Stop to hear: its wait runs its course.
#[tokio::test]
async fn a_watched_step_has_no_stop_to_end_its_wait() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let mut acc: Option<String> = None;
    let s = download_step(json!({ "kind": "expect_download", "name": "report.csv", "within_ms": 300 }));
    let started = Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &s, &quick(), &mut acc).await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(out[0].detail, "no download started within 0.3 s");
}

/// A watched run or a try has no Stop, so its wait is capped: a script
/// asking for two minutes waits the cap, and says the capped time.
#[tokio::test]
async fn a_watched_download_wait_is_capped_whatever_within_ms_says() {
    use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA, WATCHED_DOWNLOAD_WAIT_MS};
    assert_eq!(WATCHED_DOWNLOAD_WAIT_MS, 30_000);
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let mut acc: Option<String> = None;
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    let s = download_step(json!({ "kind": "expect_download", "name": "report.csv", "within_ms": 120000 }));
    let mut run = InRun { watched_cap_ms: Some(300), ..Default::default() };
    let started = Instant::now();
    let out = run_step_in_run(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &s,
        &quick(),
        &mut acc,
        &mut lease,
        None,
        AreaRoute::Unknown(NEEDS_SCRIPT_AREA),
        &mut run,
    )
    .await
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
    assert_eq!(out[0].detail, "no download started within 0.3 s");
}

// ---- Tabs ------------------------------------------------------------------

/// A page whose `check_text` holds for the words `yes` and nothing else,
/// and which can take a picture.
fn yes_page() -> ScriptedDriver {
    ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" if params["expression"] == "document" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" => {
            let arg = params["arguments"][0]["value"].as_str().unwrap_or("");
            Ok(json!({ "result": { "value": arg == "yes" } }))
        }
        "Page.captureScreenshot" => Ok(json!({ "data": "/9j/4AAQ" })),
        _ => Ok(json!({})),
    })
}

fn tab_step(actions: serde_json::Value) -> StepScript {
    StepScript { step_number: 1, actions: serde_json::from_value(actions).unwrap(), unchecked: None }
}

/// Runs one step the way a run does, and hands back what it learned.
async fn run_tab_step(
    d: &mut ScriptedDriver,
    s: &StepScript,
) -> (Vec<v2_lib::browser::actions::ActionOutcome>, Option<String>) {
    use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
    let dir = tempfile::tempdir().unwrap();
    let mut acc: Option<String> = None;
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    let mut run = InRun::default();
    let out = run_step_in_run(
        d,
        dir.path(),
        "Acme",
        "Web",
        s,
        &quick(),
        &mut acc,
        &mut lease,
        None,
        AreaRoute::Unknown(NEEDS_SCRIPT_AREA),
        &mut run,
    )
    .await
    .unwrap();
    (out, run.tab)
}

#[tokio::test]
async fn a_step_acts_in_the_tab_it_switched_to_and_says_which() {
    let mut d = yes_page();
    d.tabs.unnamed = 1;
    let s = tab_step(json!([
        { "kind": "expect_tab", "name": "report" },
        { "kind": "switch_tab", "name": "report" },
        { "kind": "check_text", "value": "yes" }
    ]));
    let (out, tab) = run_tab_step(&mut d, &s).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert!(d.tabs.called_in("report").iter().any(|m| m == "Runtime.callFunctionOn"), "{:?}", d.tabs.calls);
    assert!(!d.tabs.called_in("main").iter().any(|m| m == "Runtime.callFunctionOn"), "{:?}", d.tabs.calls);
    assert_eq!(tab.as_deref(), Some("report"));
    assert_eq!(d.tabs.steps_begun, 1, "the step did not say it began");
}

#[tokio::test]
async fn a_step_that_stays_in_main_names_no_tab_and_one_that_returns_to_main_still_does() {
    let mut d = yes_page();
    let (_, tab) = run_tab_step(&mut d, &tab_step(json!([{ "kind": "check_text", "value": "yes" }]))).await;
    assert_eq!(tab, None);
    d.tabs.unnamed = 1;
    let s = tab_step(json!([
        { "kind": "expect_tab", "name": "report" },
        { "kind": "switch_tab", "name": "report" },
        { "kind": "check_text", "value": "yes" },
        { "kind": "close_tab", "name": "report" }
    ]));
    let (out, tab) = run_tab_step(&mut d, &s).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[3].detail, "closed the \"report\" tab; main is the current tab now");
    assert_eq!(tab.as_deref(), Some("report"));
}

/// Review Focus 5: a failure's picture is of the tab the step is in.
#[tokio::test]
async fn a_failure_in_another_tab_is_pictured_in_that_tab() {
    let mut d = yes_page();
    d.tabs.unnamed = 1;
    let s = tab_step(json!([
        { "kind": "expect_tab", "name": "report" },
        { "kind": "switch_tab", "name": "report" },
        { "kind": "check_text", "value": "no" }
    ]));
    let (out, _) = run_tab_step(&mut d, &s).await;
    assert!(!out[2].ok);
    assert!(out[2].screenshot.is_some(), "{out:?}");
    assert!(d.tabs.called_in("report").iter().any(|m| m == "Page.captureScreenshot"), "{:?}", d.tabs.calls);
    assert!(!d.tabs.called_in("main").iter().any(|m| m == "Page.captureScreenshot"));
}

/// Review Focus 3: a print preview that closes itself while it is the
/// current tab. The next action fails with its name at once, with no
/// picture and not as the browser having stopped; the tab actions still
/// run, and the step goes on in `main`.
#[tokio::test]
async fn a_tab_that_closes_itself_fails_the_next_action_with_its_name() {
    let mut d = yes_page();
    d.tabs.unnamed = 1;
    d.tabs.closes_on = Some(("Runtime.callFunctionOn".into(), "preview".into()));
    let s = tab_step(json!([
        { "kind": "expect_tab", "name": "preview" },
        { "kind": "switch_tab", "name": "preview" },
        { "kind": "check_text", "value": "yes" },
        { "kind": "check_text", "value": "yes" },
        { "kind": "expect_tab_closed", "name": "preview" },
        { "kind": "check_text", "value": "yes" }
    ]));
    let (out, tab) = run_tab_step(&mut d, &s).await;
    assert!(out[2].ok, "{out:?}");
    assert!(!out[3].ok && !out[3].harness, "{out:?}");
    assert_eq!(out[3].detail, "there is no tab preview");
    assert!(out[3].screenshot.is_none());
    assert!(out[4].ok, "{out:?}");
    assert!(out[5].ok, "{out:?}");
    assert_eq!(d.tabs.calls.last().map(|(t, _)| t.as_str()), Some("main"));
    assert_eq!(tab.as_deref(), Some("preview"));
}

#[tokio::test]
async fn a_tab_no_step_expects_does_not_fail_the_step() {
    let mut d = yes_page();
    d.tabs.opens_on = Some("Runtime.callFunctionOn".into());
    let (out, tab) = run_tab_step(&mut d, &tab_step(json!([{ "kind": "check_text", "value": "yes" }]))).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(d.tabs.unnamed, 1);
    assert_eq!(tab, None);
}

#[tokio::test]
async fn a_tab_action_fails_with_the_spec_sentences() {
    let mut d = yes_page();
    let s = tab_step(json!([
        { "kind": "expect_tab", "name": "report", "within_ms": 10000 },
        { "kind": "switch_tab", "name": "report" },
        { "kind": "close_tab", "name": "report" },
        { "kind": "expect_tab_closed", "name": "report" }
    ]));
    let (out, _) = run_tab_step(&mut d, &s).await;
    let said: Vec<&str> = out.iter().map(|o| o.detail.as_str()).collect();
    assert_eq!(
        said,
        ["no new tab opened within 10 seconds", "there is no tab report", "there is no tab report", "there is no tab report"]
    );
    assert!(out.iter().all(|o| !o.ok && !o.harness));
}

/// `open_tab` opens a page by address, which a project that refuses
/// `navigate` refuses the same way.
#[tokio::test]
async fn open_tab_is_refused_where_navigate_is() {
    use v2_lib::autorun::nav::{no_address, save_nav, NavFile};
    let dir = tempfile::tempdir().unwrap();
    save_nav(dir.path(), "Acme", "Web", &NavFile { direct_urls: false, modules: vec![], save_words: vec![] }).unwrap();
    let mut d = yes_page();
    let mut acc: Option<String> = None;
    let s = tab_step(json!([{ "kind": "open_tab", "name": "second", "url": "/hr/home" }]));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &s, &quick(), &mut acc).await.unwrap();
    assert_eq!(out[0].detail, no_address(1));
    assert!(d.tabs.open.is_empty());
}

/// The supervised browser: a step of another case closes the last case's
/// tabs first; a step of the same case keeps them.
#[tokio::test]
async fn a_supervised_step_of_another_case_closes_the_last_cases_tabs() {
    use v2_lib::autorun::runner::tabs_for_case;
    let mut d = yes_page();
    let mut case = None;
    tabs_for_case(&mut d, &mut case, 7).await;
    assert_eq!(d.tabs.closed_others, 1);
    d.tabs.open.push("report".into());
    tabs_for_case(&mut d, &mut case, 7).await;
    assert_eq!(d.tabs.closed_others, 1);
    assert_eq!(d.tabs.open, ["report"]);
    tabs_for_case(&mut d, &mut case, 8).await;
    assert_eq!(d.tabs.closed_others, 2);
    assert!(d.tabs.open.is_empty());
    assert_eq!(case, Some(8));
}

/// An assistant's try for another case than the one the supervised browser
/// last ran starts in `main`: the other case's tabs are closed first, even
/// one it left current.
#[tokio::test]
async fn a_try_for_another_case_runs_in_main_and_closes_the_last_cases_tabs() {
    use v2_lib::ai_bridge::try_for_case;
    let dir = tempfile::tempdir().unwrap();
    let mut d = yes_page();
    d.tabs.open.push("report".into());
    d.tabs.current = "report".into();
    let mut tabs_case = Some(1);
    let mut account = None;
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    let check: Action = serde_json::from_value(json!({ "kind": "check_text", "value": "yes" })).unwrap();
    let (status, text) =
        try_for_case(&mut d, &mut tabs_case, &mut account, &mut lease, dir.path(), "Acme", "Web", 2, None, &check).await;
    assert_eq!(status, 200, "{text}");
    assert_eq!(d.tabs.closed_others, 1);
    assert!(d.tabs.open.is_empty(), "the last case's tab is still open");
    assert!(d.tabs.called_in("main").iter().any(|m| m == "Runtime.callFunctionOn"), "{:?}", d.tabs.calls);
    assert!(d.tabs.called_in("report").is_empty(), "{:?}", d.tabs.calls);
    assert_eq!(tabs_case, Some(2));
}

// ---- components ----

mod components_in_a_step {
    use super::*;
    use common::FakePage;
    use v2_lib::autorun::components::{put, Component, ComponentUse};
    use v2_lib::autorun::lease::Held;
    use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
    use v2_lib::browser::actions::ActionOutcome;

    fn made(name: &str, version: u32, inputs: serde_json::Value, actions: serde_json::Value) -> Component {
        serde_json::from_value(json!({
            "name": name, "description": "d", "inputs": inputs, "actions": actions, "version": version
        }))
        .expect("a component")
    }

    fn step_of(actions: serde_json::Value) -> StepScript {
        StepScript { step_number: 1, actions: serde_json::from_value(actions).unwrap(), unchecked: None }
    }

    async fn run_in(
        d: &mut ScriptedDriver,
        root: &std::path::Path,
        s: &StepScript,
    ) -> (Vec<ActionOutcome>, Vec<ComponentUse>) {
        let mut account = None;
        let mut held = Held::supervised();
        let mut r = InRun::default();
        let out = run_step_in_run(
            d,
            root,
            "Acme",
            "Web",
            s,
            &quick(),
            &mut account,
            &mut held,
            None,
            AreaRoute::Unknown(NEEDS_SCRIPT_AREA),
            &mut r,
        )
        .await
        .unwrap();
        (out, r.components)
    }

    #[tokio::test]
    async fn an_unknown_component_fails_the_step_and_runs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = FakePage::default().driver();
        let s = step_of(json!([
            { "kind": "click", "selector": { "role": "button", "name": "New" } },
            { "kind": "use_component", "component": "Ghost", "inputs": {} },
            { "kind": "check_text", "value": "Saved" }
        ]));
        let (out, used) = run_in(&mut d, dir.path(), &s).await;
        assert!(d.calls.is_empty(), "an action ran: {:?}", d.calls);
        assert!(used.is_empty());
        assert_eq!(out.len(), 3, "{out:?}");
        // The sentence is on the use; the click before it and the check
        // after it are not run.
        assert!(!out[1].ok && out[1].detail == "Ghost is not saved in this project", "{:?}", out[1]);
        for i in [0, 2] {
            assert!(!out[i].ok && out[i].detail.starts_with("not run:"), "{:?}", out[i]);
        }
        assert!(out.iter().all(|o| o.screenshot.is_none()));

        // A missing input fails it the same way.
        put(dir.path(), "Acme", "Web", made("Pick", 1, json!([{ "name": "day", "kind": "text", "description": "" }]),
            json!([{ "kind": "check_text", "value": "{{day}}" }]))).unwrap();
        let s = step_of(json!([{ "kind": "use_component", "component": "pick", "inputs": {} }]));
        let (out, _) = run_in(&mut d, dir.path(), &s).await;
        assert!(d.calls.is_empty(), "an action ran: {:?}", d.calls);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].detail, "Pick needs day");
    }

    #[tokio::test]
    async fn a_run_records_the_version_it_used() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "Acme", "Web", made("Close the toast", 1, json!([]),
            json!([{ "kind": "check_text", "value": "Saved" }]))).unwrap();
        let s = step_of(json!([
            { "kind": "check_text", "value": "Saved" },
            { "kind": "use_component", "component": "close the  TOAST", "inputs": {} }
        ]));
        let mut d = FakePage::default().driver();
        let (out, used) = run_in(&mut d, dir.path(), &s).await;
        assert_eq!(used, vec![ComponentUse { name: "Close the toast".into(), version: 1 }]);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out.iter().all(|o| o.ok), "{out:?}");
        assert_eq!(out[0].component, None);
        assert_eq!(out[1].component.as_deref(), Some("Close the toast"));

        // Changed after the script was saved: the run uses the version
        // there now, all of its actions, and says which it was.
        put(dir.path(), "Acme", "Web", made("Close the toast", 2, json!([]), json!([
            { "kind": "check_text", "value": "Saved" },
            { "kind": "click", "selector": { "css": "#close" } }
        ]))).unwrap();
        let mut d = FakePage::default().driver();
        let (out, used) = run_in(&mut d, dir.path(), &s).await;
        assert_eq!(used, vec![ComponentUse { name: "Close the toast".into(), version: 2 }]);
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(out[2].ok && out[2].detail.starts_with("clicked"), "{:?}", out[2]);
        assert_eq!(out[1].component.as_deref(), Some("Close the toast"));
        assert_eq!(out[2].component.as_deref(), Some("Close the toast"));

        // The step's record keeps the use, and a run file without it reads.
        let rec = v2_lib::autorun::StepRecord {
            step_number: 1,
            outcomes: out,
            screenshot: None,
            downloads: vec![],
            tab: None,
            dialog: None,
            components: used,
        };
        let v = serde_json::to_value(&rec).unwrap();
        assert_eq!(v["components"], json!([{ "name": "Close the toast", "version": 2 }]));
        assert_eq!(v["outcomes"][2]["component"], json!("Close the toast"));
        assert!(v["outcomes"][0].get("component").is_none(), "{v}");
        let old: v2_lib::autorun::StepRecord =
            serde_json::from_value(json!({ "step_number": 1, "outcomes": [{ "ok": true, "detail": "x" }] })).unwrap();
        assert!(old.components.is_empty() && old.outcomes[0].component.is_none());
    }

    #[tokio::test]
    async fn a_failure_inside_a_component_never_logs_a_typed_value() {
        let _log = crate::serial::log_tail();
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "Acme", "Web", made("Enter a reason", 1,
            json!([{ "name": "field", "kind": "target", "description": "" }, { "name": "reason", "kind": "text", "description": "" }]),
            json!([
                { "kind": "fill", "selector": { "input": "field" }, "value": "{{reason}}" },
                { "kind": "check_text", "value": "Reason saved" }
            ]))).unwrap();
        let secret = "s3cret-typed-7f2";
        let s = step_of(json!([{ "kind": "use_component", "component": "Enter a reason",
            "inputs": { "field": { "css": "#reason" }, "reason": secret } }]));
        let mut d = FakePage { body_has_text: false, ..FakePage::default() }.driver();
        let (out, _) = run_in(&mut d, dir.path(), &s).await;
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[0].ok && !out[1].ok, "{out:?}");
        // It was typed...
        assert!(format!("{:?}", d.calls_to("Input.insertText")).contains(secret), "the value was never typed");
        // ...and is nowhere in what the step says or the log keeps.
        assert!(!format!("{out:?}").contains(secret), "{out:?}");
        assert!(!serde_json::to_string(&out).unwrap().contains(secret));
        let logged = v2_lib::applog::recent(6000);
        assert!(logged.iter().all(|l| !l.message.contains(secret)), "a log line holds the typed value");
    }
}
