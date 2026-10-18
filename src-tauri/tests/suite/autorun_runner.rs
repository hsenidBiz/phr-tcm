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
use std::sync::atomic::Ordering;
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

/// What each action after a step's first failure records: it is never
/// carried out, so it never waits for something that cannot appear.
const AFTER_ACTION: &str = "not run: an earlier action in this step failed";

fn click(css: &str) -> Action {
    serde_json::from_value(json!({ "kind": "click", "selector": { "css": css } })).unwrap()
}

#[tokio::test]
async fn a_failed_action_stops_the_rest_of_its_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, Some("#broken"));
    let mut acc: Option<String> = None;
    let out = run_step(
        &mut d,
        dir.path(),
        "Acme",
        "Web",
        &step(vec![click("#save"), click("#broken"), click("#save")]),
        &quick(),
        &mut acc,
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 3, "one outcome per action, as always: {out:?}");
    assert!(out[0].ok, "{:?}", out[0]);
    // The failure is the second action's own, pictured as before.
    assert!(!out[1].ok && out[1].detail.contains("#broken"), "{:?}", out[1]);
    assert!(!out[1].detail.starts_with("not run:"), "{:?}", out[1]);
    // The third is never carried out.
    assert!(!out[2].ok, "{:?}", out[2]);
    assert_eq!(out[2].detail, AFTER_ACTION);
    assert!(out[2].screenshot.is_none(), "a skipped action is not pictured: {:?}", out[2]);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 1, "only the first #save was clicked");
}

#[tokio::test]
async fn a_guard_whose_target_never_shows_does_not_stop_the_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let mut acc: Option<String> = None;
    let guard: Action = serde_json::from_value(json!({ "kind": "when_visible", "selector": { "css": "#other-session" },
        "within_ms": 40, "then": [ { "kind": "click", "selector": { "css": "#other-session" } } ] }))
    .unwrap();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![click("#save"), guard, click("#save")]), &quick(), &mut acc)
        .await
        .unwrap();
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(state.clicks.load(Ordering::SeqCst), 2, "both #save clicks ran");
}

#[tokio::test]
async fn a_failing_guarded_action_stops_the_step_too() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, Some("#broken"));
    let mut acc: Option<String> = None;
    let guard: Action = serde_json::from_value(json!({ "kind": "when_visible", "selector": { "css": "#banner" },
        "within_ms": 200, "then": [ { "kind": "click", "selector": { "css": "#broken" } } ] }))
    .unwrap();
    let out =
        run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard, click("#save")]), &quick(), &mut acc).await.unwrap();
    assert_eq!(out.len(), 2);
    assert!(!out[0].ok && out[0].detail.contains("#broken"), "{:?}", out[0]);
    assert_eq!(out[1].detail, AFTER_ACTION);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_step_whose_actions_all_pass_runs_every_one() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let mut acc: Option<String> = None;
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![click("#a"), click("#b"), click("#c")]), &quick(), &mut acc)
        .await
        .unwrap();
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|o| o.ok && !o.detail.starts_with("not run:")), "{out:?}");
    assert!(out.iter().all(|o| o.screenshot.is_none()), "{out:?}");
    assert_eq!(state.clicks.load(Ordering::SeqCst), 3);
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
        &v2_lib::autorun::nav::NavFile { direct_urls: false, modules: vec![], save_words: vec![], protected: vec![] },
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
    // Each in a step of its own: a failed action stops the rest of its step.
    let mut d = yes_page();
    let mut out = Vec::new();
    for a in [
        json!({ "kind": "expect_tab", "name": "report", "within_ms": 10000 }),
        json!({ "kind": "switch_tab", "name": "report" }),
        json!({ "kind": "close_tab", "name": "report" }),
        json!({ "kind": "expect_tab_closed", "name": "report" }),
    ] {
        let (o, _) = run_tab_step(&mut d, &tab_step(json!([a]))).await;
        out.extend(o);
    }
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
    save_nav(dir.path(), "Acme", "Web", &NavFile { direct_urls: false, modules: vec![], save_words: vec![], protected: vec![] }).unwrap();
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
            components: used, duration_ms: None,
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
    async fn a_failure_inside_a_component_stops_the_step() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "Acme", "Web", made("Confirm the save", 1, json!([]), json!([
            { "kind": "check_text", "value": "Saved" },
            { "kind": "click", "selector": { "css": "#close" } }
        ]))).unwrap();
        let s = step_of(json!([
            { "kind": "use_component", "component": "Confirm the save", "inputs": {} },
            { "kind": "click", "selector": { "css": "#next" } }
        ]));
        let mut d = FakePage { body_has_text: false, ..FakePage::default() }.driver();
        let (out, _) = run_in(&mut d, dir.path(), &s).await;
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(!out[0].ok && !out[0].detail.starts_with("not run:"), "{:?}", out[0]);
        for i in [1, 2] {
            assert_eq!(out[i].detail, super::AFTER_ACTION, "{:?}", out[i]);
        }
        assert_eq!(out[1].component.as_deref(), Some("Confirm the save"));
        assert!(d.calls_to("Input.dispatchMouseEvent").is_empty(), "a click ran: {:?}", d.calls);
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

/// Every action kind, and whether it waits for the step before's picture:
/// anything that may change the page waits; checks and reads do not.
#[test]
fn every_action_kind_says_whether_it_changes_the_page() {
    use v2_lib::autorun::runner::changes_the_page;
    let css = json!({ "css": "#x" });
    let table: Vec<(serde_json::Value, bool)> = vec![
        (json!({ "kind": "navigate", "url": "/a" }), true),
        (json!({ "kind": "use_component", "component": "c" }), true),
        (json!({ "kind": "click", "selector": css }), true),
        (json!({ "kind": "fill", "selector": css, "value": "v" }), true),
        (json!({ "kind": "sign_in", "account": "a" }), true),
        (json!({ "kind": "upload", "selector": css, "file": "f.txt" }), true),
        (json!({ "kind": "api_request", "path": "/api/x" }), true),
        (json!({ "kind": "when_visible", "selector": css, "then": [] }), true),
        (json!({ "kind": "repeat_while_visible", "selector": css, "max": 2, "then": [] }), true),
        (json!({ "kind": "reload" }), true),
        (json!({ "kind": "expire_session" }), true),
        (json!({ "kind": "return_to_area" }), true),
        (json!({ "kind": "press_key", "key": "Enter" }), true),
        (json!({ "kind": "drag", "from": css, "to": css }), true),
        (json!({ "kind": "open_tab", "name": "t", "url": "/a" }), true),
        (json!({ "kind": "switch_tab", "name": "t" }), true),
        (json!({ "kind": "close_tab", "name": "t" }), true),
        (json!({ "kind": "wait_for", "selector": css, "timeout_ms": 10 }), false),
        (json!({ "kind": "check_text", "value": "v" }), false),
        (json!({ "kind": "check_url", "contains": "v" }), false),
        (json!({ "kind": "expect_visible", "selector": css }), false),
        (json!({ "kind": "expect_hidden", "selector": css }), false),
        (json!({ "kind": "expect_text", "selector": css, "equals": "v" }), false),
        (json!({ "kind": "expect_contains_text", "selector": css, "value": "v" }), false),
        (json!({ "kind": "expect_count", "selector": css, "equals": 1 }), false),
        (json!({ "kind": "expect_attribute", "selector": css, "name": "n", "equals": "v" }), false),
        (json!({ "kind": "expect_enabled", "selector": css }), false),
        (json!({ "kind": "expect_disabled", "selector": css }), false),
        (json!({ "kind": "expect_checked", "selector": css, "checked": true }), false),
        (json!({ "kind": "expect_response", "url_contains": "/api" }), false),
        (json!({ "kind": "expect_focused", "selector": css }), false),
        (json!({ "kind": "expect_download", "name": "*.csv" }), false),
        (json!({ "kind": "expect_tab", "name": "t" }), false),
        (json!({ "kind": "expect_tab_closed", "name": "t" }), false),
        (json!({ "kind": "expect_dialog", "answer": "accept" }), false),
        (json!({ "kind": "expect_row", "table": css, "cells": { "Name": "a" } }), false),
        (json!({ "kind": "expect_no_row", "table": css, "cells": { "Name": "a" } }), false),
        (json!({ "kind": "expect_sorted", "table": css, "column": "Name", "order": "ascending" }), false),
        (json!({ "kind": "expect_row_count", "table": css, "equals": 1 }), false),
    ];
    let mut kinds = std::collections::BTreeSet::new();
    for (raw, changes) in &table {
        let action: Action = serde_json::from_value(raw.clone()).unwrap_or_else(|e| panic!("{raw}: {e}"));
        assert_eq!(changes_the_page(&action), *changes, "{raw}");
        kinds.insert(action.kind());
    }
    for kind in v2_lib::autorun::guide::ACTION_KINDS {
        assert!(kinds.contains(*kind), "{kind} is missing from the table");
    }
    assert_eq!(kinds.len(), table.len(), "a kind is in the table twice");
}

/// A step tells its progress, action by action: the action it is on, its
/// kind and target, and how many of its actions failed. A failed action
/// never stops the step and is never tried again.
#[tokio::test]
async fn a_step_tells_which_action_it_is_on_and_how_many_failed() {
    use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, StepProgress, NEEDS_SCRIPT_AREA};
    let dir = tempfile::tempdir().unwrap();
    let mut d = common::FakePage { body_has_text: false, ..common::FakePage::default() }.driver();
    let s = tab_step(json!([
        { "kind": "expect_visible", "selector": { "css": "#save" } },
        { "kind": "check_text", "value": "a" },
        { "kind": "check_text", "value": "b" }
    ]));
    let progress = StepProgress::new();
    let (mut acc, mut lease) = (None, v2_lib::autorun::lease::Held::supervised());
    let mut run = InRun { progress: Some(&progress), ..Default::default() };
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
    assert_eq!(out.iter().map(|o| o.ok).collect::<Vec<_>>(), [true, false, false], "{out:?}");
    // The failed check stopped the step: the one after it never began.
    assert_eq!(out[2].detail, AFTER_ACTION);
    let now = progress.now();
    assert_eq!((now.index, now.of, now.kind.as_str()), (2, 3, "check_text"));
    assert_eq!(now.failed, 1);
    assert!(now.since.is_some());
    progress.reset();
    assert_eq!(progress.now().index, 0);
}

// ---- What the page said, and what the application refused ------------------

const CLICKED: &str = "Input.dispatchMouseEvent";

/// An ASP.NET problem body, as a refused save answers.
const ASPNET_400: &str = r#"{"title":"One or more validation errors occurred.","errors":{"Weightage":["Weightage must total 100."]},"traceId":"00-4bf9-00"}"#;

fn net_event(method: &str, params: serde_json::Value) -> v2_lib::browser::cdp::Event {
    v2_lib::browser::cdp::Event { method: method.to_string(), params }
}

/// One request the page sends its own server, answered `status`.
fn page_request(id: &str, method: &str, url: &str, kind: &str, status: u16) -> Vec<v2_lib::browser::cdp::Event> {
    vec![
        net_event(
            "Network.requestWillBeSent",
            json!({ "requestId": id, "type": kind, "documentURL": "https://hr.example/hr/page",
                    "request": { "url": url, "method": method } }),
        ),
        net_event(
            "Network.responseReceived",
            json!({ "requestId": id, "response": { "status": status, "url": url, "mimeType": "application/json" } }),
        ),
        net_event("Network.loadingFinished", json!({ "requestId": id })),
    ]
}

/// A page whose message reads answer `reads` in turn (the last repeats),
/// whose refused requests answer `ASPNET_400`, and - with `gone_after_click`
/// - on which nothing is found once something was clicked.
fn said_page(reads: Vec<Vec<&'static str>>, gone_after_click: bool) -> ScriptedDriver {
    let page = common::FakePage::default();
    let mut n = 0usize;
    let mut clicked = false;
    ScriptedDriver::new(move |method, params| {
        if method == CLICKED {
            clicked = true;
        }
        match method {
            "Runtime.evaluate" if params["expression"] == v2_lib::browser::page_messages::READ_JS => {
                let i = n.min(reads.len() - 1);
                n += 1;
                let messages: Vec<serde_json::Value> =
                    reads[i].iter().map(|t| json!({ "text": t, "where": "page" })).collect();
                Ok(json!({ "result": { "value": { "messages": messages, "step": null, "path": "/hr/eval/42/rules" } } }))
            }
            "Network.getResponseBody" => Ok(json!({ "body": ASPNET_400, "base64Encoded": false })),
            "Runtime.getProperties" if gone_after_click && clicked => Ok(json!({ "result": [] })),
            _ => page.answer(method, params),
        }
    })
    .with_net_record()
}

/// The click makes the page send a save that the application refuses.
fn click_is_refused(d: &mut ScriptedDriver) {
    for e in page_request("r1", "POST", "https://hr.example/api/eval-rules?x=1", "XHR", 400) {
        d.on_call_events.push((CLICKED.to_string(), e));
    }
}

/// One step, run as a try (`refusals_fail`) or as a run.
async fn run_said(
    d: &mut ScriptedDriver,
    actions: serde_json::Value,
    refusals_fail: bool,
) -> Vec<v2_lib::browser::actions::ActionOutcome> {
    use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
    let dir = tempfile::tempdir().unwrap();
    let (mut acc, mut lease) = (None, v2_lib::autorun::lease::Held::supervised());
    let mut run = InRun { refusals_fail, ..Default::default() };
    run_step_in_run(
        d,
        dir.path(),
        "Acme",
        "Web",
        &tab_step(actions),
        &quick(),
        &mut acc,
        &mut lease,
        None,
        AreaRoute::Unknown(NEEDS_SCRIPT_AREA),
        &mut run,
    )
    .await
    .unwrap()
}

fn said(text: &str) -> v2_lib::browser::page_messages::PageMessage {
    v2_lib::browser::page_messages::PageMessage { text: text.to_string(), place: "page".to_string() }
}

#[tokio::test]
async fn a_try_fails_on_a_refused_request() {
    let mut d = said_page(vec![vec![]], false);
    click_is_refused(&mut d);
    let out = run_said(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), true).await;
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.starts_with(r#"the app refused: "One or more validation errors occurred.""#), "{}", out[0].detail);
    assert!(out[0].detail.ends_with("trace id 00-4bf9-00)"), "{}", out[0].detail);
    assert!(!out[0].detail.contains("x=1"), "no query: {}", out[0].detail);
    assert_eq!(out[0].refused.len(), 1);
}

#[tokio::test]
async fn a_try_that_is_refused_also_says_what_the_page_showed() {
    let mut d = said_page(vec![vec![], vec!["Weightage must total 100."]], false);
    click_is_refused(&mut d);
    let out = run_said(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), true).await;
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(
        out[0].detail.ends_with(r#"trace id 00-4bf9-00); the page showed: "Weightage must total 100.""#),
        "{}",
        out[0].detail
    );
}

#[tokio::test]
async fn a_run_reports_but_does_not_fail_on_a_refusal() {
    let mut d = said_page(vec![vec![], vec!["Weightage must total 100."]], false);
    click_is_refused(&mut d);
    let out = run_said(
        &mut d,
        json!([
            { "kind": "click", "selector": { "css": "#save" } },
            { "kind": "expect_visible", "selector": { "css": "#weightage-error" } }
        ]),
        false,
    )
    .await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[0].refused.len(), 1, "{out:?}");
    assert_eq!(out[0].page_said, vec![said("Weightage must total 100.")]);
    assert!(out[1].refused.is_empty() && out[1].page_said.is_empty(), "a check changes nothing: {:?}", out[1]);
}

#[tokio::test]
async fn a_run_step_that_fails_after_a_refusal_names_it() {
    use v2_lib::autorun::patterns::{classify, ErrorClass};
    let mut d = said_page(vec![vec![]], true);
    click_is_refused(&mut d);
    let out = run_said(
        &mut d,
        json!([
            { "kind": "click", "selector": { "css": "#save" } },
            { "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }
        ]),
        false,
    )
    .await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(!out[1].ok, "{:?}", out[1]);
    assert!(out[1].detail.contains(" - after click"), "{}", out[1].detail);
    assert!(out[1].detail.contains("the app refused: "), "{}", out[1].detail);
    assert_eq!(classify(&out[1].detail, None), ErrorClass::AppRefused);
}

#[tokio::test]
async fn a_run_step_that_fails_after_a_message_names_it_and_keeps_its_class() {
    use v2_lib::autorun::patterns::{classify, ErrorClass};
    let mut d = said_page(vec![vec![], vec!["At least one component must be enabled."]], true);
    let out = run_said(
        &mut d,
        json!([
            { "kind": "click", "selector": { "css": "#save" } },
            { "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }
        ]),
        false,
    )
    .await;
    assert!(!out[1].ok, "{:?}", out[1]);
    assert!(out[1].detail.contains(" - after click "), "{}", out[1].detail);
    assert!(
        out[1].detail.ends_with(r#"the page showed: "At least one component must be enabled.""#),
        "{}",
        out[1].detail
    );
    assert_eq!(classify(&out[1].detail, None), ErrorClass::NotFound);
}

#[tokio::test]
async fn a_message_after_a_click_is_reported_not_failed() {
    let mut d = said_page(vec![vec![], vec!["Saved."]], false);
    let out = run_said(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), true).await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].page_said, vec![said("Saved.")]);
    assert!(out[0].refused.is_empty());
}

#[tokio::test]
async fn a_late_message_goes_to_the_action_that_caused_it() {
    let mut d = said_page(vec![vec![], vec![], vec!["Name is required"]], false);
    let out = run_said(
        &mut d,
        json!([
            { "kind": "click", "selector": { "css": "#save" } },
            { "kind": "click", "selector": { "css": "#next" } }
        ]),
        false,
    )
    .await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[0].page_said, vec![said("Name is required")]);
    assert!(out[1].page_said.is_empty(), "{:?}", out[1]);
}

#[tokio::test]
async fn the_runs_own_api_request_is_not_a_refusal() {
    use v2_lib::autorun::api_checks::GET_FN;
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" if params["functionDeclaration"] == GET_FN => Ok(json!({ "result": { "value": {
            "status": 400, "text": "", "sameOrigin": true, "redirected": false, "finalPath": "/api/cycles/42"
        } } })),
        "Network.getResponseBody" => Ok(json!({ "body": ASPNET_400, "base64Encoded": false })),
        _ => Ok(json!({})),
    })
    .with_net_record();
    for e in page_request("own-1", "GET", "https://hr.example/api/cycles/42", "Fetch", 400) {
        d.on_call_events.push(("Runtime.callFunctionOn".to_string(), e));
    }
    let out =
        run_said(&mut d, json!([{ "kind": "api_request", "path": "/api/cycles/42", "expect": { "status": 400 } }]), true)
            .await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(out.iter().all(|o| o.refused.is_empty()), "{out:?}");
}

#[test]
fn a_page_note_quotes_each_message() {
    use v2_lib::autorun::runner::{page_showed_note, PAGE_SHOWED};
    assert_eq!(page_showed_note(&[said("a"), said("b")]), r#"the page showed: "a"; "b""#);
    assert!(page_showed_note(&[said("a")]).starts_with(PAGE_SHOWED));
}

#[test]
fn an_outcome_with_nothing_said_saves_as_before() {
    let o = v2_lib::browser::actions::ActionOutcome::passed("clicked");
    let v = serde_json::to_value(&o).unwrap();
    assert!(v.get("page_said").is_none() && v.get("refused").is_none(), "{v}");
    let back: v2_lib::browser::actions::ActionOutcome =
        serde_json::from_value(json!({ "ok": true, "detail": "clicked" })).unwrap();
    assert_eq!(back, o);
}

// ---- Answers that come after the action ------------------------------------

/// The click's save starts at once and is answered `after_ms` of waiting
/// later.
fn click_is_refused_after(d: &mut ScriptedDriver, after_ms: u64) {
    let mut events = page_request("r1", "POST", "https://hr.example/api/eval-rules", "XHR", 400);
    d.on_call_events.push((CLICKED.to_string(), events.remove(0)));
    for e in events {
        d.on_idle_after_call.push((CLICKED.to_string(), after_ms, e));
    }
}

/// The click's save is sent, and answered, only `after_ms` of waiting
/// after the click: after the click's own reads are done.
fn click_saves_late(d: &mut ScriptedDriver, after_ms: u64) {
    for e in page_request("r1", "POST", "https://hr.example/api/eval-rules", "XHR", 400) {
        d.on_idle_after_call.push((CLICKED.to_string(), after_ms, e));
    }
}

fn fake_clock(d: &mut ScriptedDriver) -> std::sync::Arc<std::sync::atomic::AtomicU64> {
    let clock = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    d.idle_clock = Some(clock.clone());
    clock
}

#[tokio::test]
async fn a_refusal_answered_after_the_click_is_reported_and_fails_a_try() {
    let mut d = said_page(vec![vec![]], false);
    click_is_refused_after(&mut d, 150);
    let out = run_said(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), true).await;
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.starts_with("the app refused: "), "{}", out[0].detail);
    assert_eq!(out[0].refused.len(), 1);
    // A run reports it.
    let mut d = said_page(vec![vec![]], false);
    click_is_refused_after(&mut d, 150);
    let out = run_said(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), false).await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].refused.len(), 1, "{out:?}");
}

#[tokio::test]
async fn a_request_that_never_answers_is_waited_for_three_seconds_and_fails_nothing() {
    let click = json!([{ "kind": "click", "selector": { "css": "#save" } }]);
    let mut d = said_page(vec![vec![]], false);
    let quiet = fake_clock(&mut d);
    run_said(&mut d, click.clone(), true).await;
    let mut d = said_page(vec![vec![]], false);
    let waiting = fake_clock(&mut d);
    let sent = page_request("r1", "POST", "https://hr.example/api/eval-rules", "XHR", 400).remove(0);
    d.on_call_events.push((CLICKED.to_string(), sent));
    let out = run_said(&mut d, click, true).await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(out[0].refused.is_empty());
    let extra = waiting.load(Ordering::SeqCst) - quiet.load(Ordering::SeqCst);
    assert_eq!(extra, v2_lib::autorun::runner::ANSWER_WAIT_MS, "waited {extra} ms for the answer");
}

#[tokio::test]
async fn a_request_answered_with_the_click_costs_no_wait() {
    let click = json!([{ "kind": "click", "selector": { "css": "#save" } }]);
    let mut d = said_page(vec![vec![]], false);
    let quiet = fake_clock(&mut d);
    run_said(&mut d, click.clone(), false).await;
    let mut d = said_page(vec![vec![]], false);
    let answered = fake_clock(&mut d);
    for e in page_request("r1", "POST", "https://hr.example/api/eval-rules", "XHR", 200) {
        d.on_call_events.push((CLICKED.to_string(), e));
    }
    run_said(&mut d, click, false).await;
    assert_eq!(answered.load(Ordering::SeqCst), quiet.load(Ordering::SeqCst));
}

/// Runs `actions` as step `at` of a case whose last page change `noted`
/// carries.
async fn run_noted(
    d: &mut ScriptedDriver,
    actions: serde_json::Value,
    noted: &mut v2_lib::autorun::runner::Noted,
    at: usize,
) -> Vec<v2_lib::browser::actions::ActionOutcome> {
    use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
    let dir = tempfile::tempdir().unwrap();
    let (mut acc, mut lease) = (None, v2_lib::autorun::lease::Held::supervised());
    noted.step = at;
    let mut run = InRun { noted: Some(noted), ..Default::default() };
    run_step_in_run(
        d,
        dir.path(),
        "Acme",
        "Web",
        &tab_step(actions),
        &quick(),
        &mut acc,
        &mut lease,
        None,
        AreaRoute::Unknown(NEEDS_SCRIPT_AREA),
        &mut run,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_step_that_fails_names_what_the_step_before_caused() {
    use v2_lib::autorun::patterns::{classify, ErrorClass};
    use v2_lib::autorun::runner::{credit_late, Noted};
    // Step 3's click; the save, its 400 and the toast come only once the
    // step has ended.
    let mut d = said_page(vec![vec![], vec![], vec!["Weightage must total 100."]], true);
    click_saves_late(&mut d, 20);
    let mut noted = Noted::default();
    let three = run_noted(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), &mut noted, 0).await;
    assert!(three[0].ok && three[0].refused.is_empty(), "{three:?}");
    let four = run_noted(
        &mut d,
        json!([{ "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }]),
        &mut noted,
        1,
    )
    .await;
    assert!(!four[0].ok, "{:?}", four[0]);
    assert!(four[0].detail.contains(" - after click "), "{}", four[0].detail);
    assert!(four[0].detail.contains("the app refused: \"One or more validation errors occurred.\""), "{}", four[0].detail);
    assert!(four[0].detail.ends_with(r#"the page showed: "Weightage must total 100.""#), "{}", four[0].detail);
    assert_eq!(classify(&four[0].detail, None), ErrorClass::AppRefused);
    // What came late is put on step 3's own record.
    let late = noted.take_late();
    assert_eq!(late.len(), 1, "{late:?}");
    assert_eq!((late[0].step, late[0].action), (0, 0));
    let mut steps = vec![v2_lib::autorun::StepRecord {
        step_number: 3,
        outcomes: three,
        screenshot: None,
        downloads: vec![],
        tab: None,
        dialog: None,
        components: Vec::new(),
        duration_ms: None,
    }];
    credit_late(&mut steps, late);
    assert_eq!(steps[0].outcomes[0].refused.len(), 1);
    assert_eq!(steps[0].outcomes[0].page_said, vec![said("Weightage must total 100.")]);
}

#[tokio::test]
async fn a_wait_that_fails_names_what_the_click_before_it_caused_late() {
    use v2_lib::autorun::patterns::{classify, ErrorClass};
    let mut d = said_page(vec![vec![], vec![], vec!["Weightage must total 100."]], true);
    click_saves_late(&mut d, 20);
    let out = run_said(
        &mut d,
        json!([
            { "kind": "click", "selector": { "css": "#save" } },
            { "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }
        ]),
        false,
    )
    .await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].refused.len(), 1, "credited to the click: {out:?}");
    assert_eq!(out[0].page_said, vec![said("Weightage must total 100.")]);
    assert!(!out[1].ok, "{:?}", out[1]);
    assert!(out[1].detail.contains(" - after click ") && out[1].detail.contains("the app refused: "), "{}", out[1].detail);
    assert_eq!(classify(&out[1].detail, None), ErrorClass::AppRefused);
}

#[tokio::test]
async fn twenty_clicks_never_push_out_the_steps_own_mark() {
    let mut d = said_page(vec![vec![]], false).with_mark_ring();
    // A request from before the step: never the step's.
    for e in page_request("old", "GET", "https://hr.example/api/list", "XHR", 200) {
        d.net.as_mut().unwrap().observe(&e);
    }
    let mut actions: Vec<serde_json::Value> =
        (0..20).map(|i| json!({ "kind": "click", "selector": { "css": format!("#b{i}") } })).collect();
    actions.push(json!({ "kind": "expect_response", "url_contains": "/api/list", "timeout_ms": 100 }));
    let out = run_said(&mut d, serde_json::Value::Array(actions), false).await;
    assert!(out[..20].iter().all(|o| o.ok), "{out:?}");
    assert!(!out[20].ok, "an earlier step's request was taken for this one's: {:?}", out[20]);
}

#[tokio::test]
async fn a_new_page_is_not_what_a_navigation_said() {
    let mut d = said_page(vec![vec![], vec!["Welcome back"]], false);
    d.on_every_call_events.push((
        "Page.navigate".into(),
        v2_lib::browser::cdp::Event {
            method: "Page.lifecycleEvent".into(),
            params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }),
        },
    ));
    let out = run_said(&mut d, json!([{ "kind": "navigate", "url": "https://hr.example/home" }]), false).await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(out[0].page_said.is_empty(), "{:?}", out[0]);
}

#[tokio::test]
async fn a_try_that_is_refused_keeps_the_dialog_it_met() {
    let mut d = said_page(vec![vec![]], false);
    click_is_refused(&mut d);
    d.dialogs_on_call.push((CLICKED.to_string(), "confirm".to_string(), "Leave?".to_string()));
    let out = run_said(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), true).await;
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.starts_with("the app refused: "), "{}", out[0].detail);
    assert!(out[0].detail.ends_with(r#" (a confirm dialog was accepted: "Leave?")"#), "{}", out[0].detail);
}

/// A watched step clicks; then something else uses the browser (a replay,
/// a try, a sign-in), which forgets the watched run's last page change. A
/// later watched step that fails names nothing from before that use.
#[tokio::test]
async fn a_watched_step_after_another_use_names_nothing_from_before_it() {
    use v2_lib::autorun::runner::Noted;
    let mut d = said_page(vec![vec![], vec![], vec!["Weightage must total 100."]], true);
    click_saves_late(&mut d, 20);
    let mut noted = Noted::default();
    let two = run_noted(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), &mut noted, 0).await;
    assert!(two[0].ok, "{two:?}");
    // The replay, try or sign-in in between.
    noted.forget(&d);
    let five = run_noted(
        &mut d,
        json!([{ "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }]),
        &mut noted,
        1,
    )
    .await;
    assert!(!five[0].ok, "{:?}", five[0]);
    assert!(!five[0].detail.contains(" - after "), "{}", five[0].detail);
    assert!(noted.take_late().is_empty());
}

/// A step that fails in a run teaches the application's rules: the
/// refusal and the message the step before's save got are kept under the
/// case's area, as a run's - and a step that passed teaches nothing.
#[tokio::test]
async fn a_failed_step_remembers_what_the_step_before_was_refused() {
    use v2_lib::autorun::discovery_map::{load_map, rules_of, RuleSource};
    use v2_lib::autorun::runner::{remember_failed_step, Noted};
    let dir = tempfile::tempdir().unwrap();
    let mut d = said_page(vec![vec![], vec![], vec!["Weightage must total 100."]], true);
    click_saves_late(&mut d, 20);
    let mut noted = Noted::default();
    noted.in_case(Some("Eval Rules"));
    let save = json!([{ "kind": "click", "selector": { "css": "#save" } }]);
    let three = run_noted(&mut d, save.clone(), &mut noted, 0).await;
    let late = noted.take_late();
    remember_failed_step(dir.path(), "Acme", "Web", &tab_step(save), &three, &late, noted.area(), 7);
    assert!(load_map(dir.path(), "Acme", "Web").unwrap().areas.is_empty(), "a step that passed was remembered");
    let wait = json!([{ "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }]);
    let four = run_noted(&mut d, wait.clone(), &mut noted, 1).await;
    assert!(!four[0].ok, "{:?}", four[0]);
    let late = noted.take_late();
    assert_eq!(late.len(), 1, "{late:?}");
    remember_failed_step(dir.path(), "Acme", "Web", &tab_step(wait), &four, &late, noted.area(), 7);
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let rules = rules_of(&map, "Eval Rules");
    let mut messages: Vec<&str> = rules.iter().map(|r| r.message.as_str()).collect();
    messages.sort();
    assert_eq!(messages, ["One or more validation errors occurred.", "Weightage must total 100."], "{rules:?}");
    let click = v2_lib::ai_bridge::describe_action(&serde_json::from_value(json!({ "kind": "click", "selector": { "css": "#save" } })).unwrap());
    assert!(rules.iter().all(|r| r.after == click && r.source == RuleSource::Run && r.case_id == Some(7)), "{rules:?}");
    assert!(rules.iter().all(|r| r.page == "/hr/eval/:id/rules"), "{rules:?}");
    let file = std::fs::read_to_string(v2_lib::autorun::discovery_map::map_path(dir.path(), "Acme", "Web")).unwrap();
    assert!(!file.contains("00-4bf9-00") && !file.contains("eval-rules"), "{file}");
}

/// With no area known, a failed step is not remembered anywhere.
#[tokio::test]
async fn a_failed_step_with_no_area_is_not_remembered() {
    use v2_lib::autorun::runner::{remember_failed_step, Noted};
    let dir = tempfile::tempdir().unwrap();
    let mut d = said_page(vec![vec![]], true);
    click_is_refused(&mut d);
    let mut noted = Noted::default();
    noted.in_case(None);
    let steps = json!([
        { "kind": "click", "selector": { "css": "#save" } },
        { "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }
    ]);
    let out = run_noted(&mut d, steps.clone(), &mut noted, 0).await;
    assert!(out[0].ok && !out[0].refused.is_empty() && !out[1].ok, "{out:?}");
    remember_failed_step(dir.path(), "Acme", "Web", &tab_step(steps), &out, &[], noted.area(), 7);
    assert!(!v2_lib::autorun::discovery_map::map_path(dir.path(), "Acme", "Web").exists());
}

/// A late refusal of step N's save is filed under the page the save's own
/// read was on, though the next step moved to another page before the
/// failure.
#[tokio::test]
async fn a_late_refusal_is_filed_under_the_page_of_the_action_it_answered() {
    use v2_lib::autorun::discovery_map::{load_map, rules_of};
    use v2_lib::autorun::runner::{remember_failed_step, Noted};
    let page = common::FakePage::default();
    let mut clicks = 0usize;
    let mut d = ScriptedDriver::new(move |method, params| {
        if method == CLICKED && params["type"] == "mouseReleased" {
            clicks += 1;
        }
        match method {
            "Runtime.evaluate" if params["expression"] == v2_lib::browser::page_messages::READ_JS => {
                let path = if clicks <= 1 { "/hr/eval/42/rules" } else { "/hr/goals" };
                Ok(json!({ "result": { "value": { "messages": [], "step": null, "path": path } } }))
            }
            "Network.getResponseBody" => Ok(json!({ "body": ASPNET_400, "base64Encoded": false })),
            "Runtime.getProperties" if clicks >= 2 => Ok(json!({ "result": [] })),
            _ => page.answer(method, params),
        }
    })
    .with_net_record();
    click_saves_late(&mut d, 20);
    let dir = tempfile::tempdir().unwrap();
    let mut noted = Noted::default();
    noted.in_case(Some("Eval Rules"));
    let save = run_noted(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), &mut noted, 0).await;
    assert!(save[0].ok && save[0].refused.is_empty(), "{save:?}");
    // The save is sent and refused only once the step has ended.
    v2_lib::browser::cdp::Driver::idle(&mut d, std::time::Duration::from_millis(50)).await;
    noted.take_late();
    let next = json!([
        { "kind": "click", "selector": { "css": "#goals-tab" } },
        { "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }
    ]);
    let out = run_noted(&mut d, next.clone(), &mut noted, 1).await;
    assert!(out[0].ok && !out[1].ok, "{out:?}");
    let late = noted.take_late();
    assert_eq!(late.len(), 1, "{late:?}");
    remember_failed_step(dir.path(), "Acme", "Web", &tab_step(next), &out, &late, noted.area(), 7);
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let rules = rules_of(&map, "Eval Rules");
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert_eq!(rules[0].page, "/hr/eval/:id/rules");
}

/// A failure TCM made itself - a save the guard stopped, a Stop, a browser
/// that stopped answering - is no evidence for the message before it; a
/// refused request is kept all the same.
#[test]
fn a_failure_tcm_made_is_no_evidence_for_a_message() {
    use v2_lib::autorun::discovery_map::{load_map, map_path, rules_of};
    use v2_lib::autorun::runner::{remember_failed_step, AFTER_STEP_BEFORE_FAILED, AFTER_STOP};
    use v2_lib::browser::actions::ActionOutcome;
    let refusal = v2_lib::autorun::refusals::RefusedRequest {
        method: "POST".into(),
        path: "/api/eval-rules".into(),
        status: 400,
        message: "Weightage must total 100.".into(),
        trace_id: None,
        from_body: true,
    };
    let clicked = |refused: bool| {
        let mut o = ActionOutcome::passed("clicked #save");
        o.page_said = vec![said("Name is required")];
        o.page_path = Some("/hr/eval/42/rules".into());
        if refused {
            o.refused = vec![refusal.clone()];
        }
        o
    };
    let mut gone = ActionOutcome::failed("the browser did not answer");
    gone.harness = true;
    let failures = [
        ActionOutcome::failed(v2_lib::browser::save_guard::blocked("POST", "https://hr.example/api/x?y=1")),
        ActionOutcome::failed(AFTER_STOP),
        ActionOutcome::failed(AFTER_STEP_BEFORE_FAILED),
        ActionOutcome::failed(v2_lib::browser::cdp::TAB_HELD_UNGUARDED),
        ActionOutcome::failed(v2_lib::browser::cdp::TAB_OPEN_UNGUARDED),
        gone,
    ];
    let step = tab_step(json!([
        { "kind": "click", "selector": { "css": "#save" } },
        { "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }
    ]));
    for failure in failures {
        let dir = tempfile::tempdir().unwrap();
        remember_failed_step(dir.path(), "Acme", "Web", &step, &[clicked(false), failure.clone()], &[], Some("A"), 7);
        assert!(!map_path(dir.path(), "Acme", "Web").exists(), "{failure:?} was taken for evidence");
        remember_failed_step(dir.path(), "Acme", "Web", &step, &[clicked(true), failure.clone()], &[], Some("A"), 7);
        let map = load_map(dir.path(), "Acme", "Web").unwrap();
        let kept: Vec<String> = rules_of(&map, "A").iter().map(|r| r.message.clone()).collect();
        assert_eq!(kept, ["Weightage must total 100."], "{failure:?}");
    }
    // A failure the page gave is evidence.
    let dir = tempfile::tempdir().unwrap();
    let check = ActionOutcome::failed("waited 100ms and never saw #goals");
    remember_failed_step(dir.path(), "Acme", "Web", &step, &[clicked(false), check], &[], Some("A"), 7);
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(rules_of(&map, "A").len(), 1);
}

/// A mark the whole browser no longer holds reads nothing, never the
/// record from its start: a request refused long before is not taken for
/// one since the mark.
#[tokio::test]
async fn a_mark_no_longer_held_finds_no_refusal() {
    use v2_lib::autorun::refusals::refused_since;
    use v2_lib::browser::cdp::{Driver, MAX_NET_MARKS};
    let mut d = said_page(vec![vec![]], false).with_mark_ring();
    for e in page_request("old", "POST", "https://hr.example/api/eval-rules", "XHR", 400) {
        d.net.as_mut().unwrap().observe(&e);
    }
    let mark = d.net_mark();
    // Another address: the same one twice is taken for a poll.
    for e in page_request("new", "POST", "https://hr.example/api/goals", "XHR", 400) {
        d.net.as_mut().unwrap().observe(&e);
    }
    assert_eq!(refused_since(&mut d, mark, &[]).await.len(), 1, "a mark held reads what came after it");
    for _ in 0..MAX_NET_MARKS {
        d.net_mark();
    }
    assert!(refused_since(&mut d, mark, &[]).await.is_empty(), "a mark pushed out read the whole record");
    assert!(d.net_since_held(mark).is_none());
}

/// The case's last page change whose mark was pushed out is put nothing
/// more on: a later failure names no refusal from before it.
#[tokio::test]
async fn a_page_change_whose_mark_is_gone_is_credited_nothing() {
    use v2_lib::autorun::runner::Noted;
    use v2_lib::browser::cdp::{Driver, MAX_NET_MARKS};
    let mut d = said_page(vec![vec![]], true).with_mark_ring();
    for e in page_request("old", "POST", "https://hr.example/api/eval-rules", "XHR", 400) {
        d.net.as_mut().unwrap().observe(&e);
    }
    let mut noted = Noted::default();
    let three = run_noted(&mut d, json!([{ "kind": "click", "selector": { "css": "#save" } }]), &mut noted, 0).await;
    assert!(three[0].ok && three[0].refused.is_empty(), "{three:?}");
    for _ in 0..MAX_NET_MARKS {
        d.net_mark();
    }
    let four = run_noted(
        &mut d,
        json!([{ "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }]),
        &mut noted,
        1,
    )
    .await;
    assert!(!four[0].ok, "{:?}", four[0]);
    assert!(!four[0].detail.contains("the app refused"), "{}", four[0].detail);
    assert!(noted.take_late().is_empty());
}

/// An unattended case: step 3's save is refused within the click's own
/// reads, so the step passes; step 4 then fails, and its failure line names
/// that refusal. The run keeps it as a rule of the case's area.
#[tokio::test]
async fn a_run_remembers_the_refusal_its_failure_names_from_the_step_before() {
    use v2_lib::autorun::discovery_map::{load_map, rules_of, RuleSource};
    let dir = tempfile::tempdir().unwrap();
    let mut d = said_page(vec![vec![]], true);
    click_is_refused(&mut d);
    let case: v2_lib::autorun::CaseScript = serde_json::from_value(json!({
        "case_id": 7,
        "title": "case 7",
        "area": "Eval Rules",
        "steps": [
            { "step_number": 3, "actions": [{ "kind": "click", "selector": { "css": "#save" } }] },
            { "step_number": 4, "actions": [{ "kind": "wait_for", "selector": { "css": "#goals" }, "timeout_ms": 100 }] }
        ]
    }))
    .unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let rec = v2_lib::autorun::replay::run_case(&mut d, dir.path(), "Acme", "Web", &case, &quick(), &cancel, &mut |_| {}).await;
    let three = &rec.steps[0].outcomes[0];
    assert!(three.ok && three.refused.len() == 1, "{three:?}");
    let four = &rec.steps[1].outcomes[0];
    assert!(!four.ok && four.detail.contains(" - after click "), "{four:?}");
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let rules = rules_of(&map, "Eval Rules");
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert_eq!(rules[0].message, "One or more validation errors occurred.");
    assert_eq!((rules[0].source, rules[0].case_id), (RuleSource::Run, Some(7)));
}
