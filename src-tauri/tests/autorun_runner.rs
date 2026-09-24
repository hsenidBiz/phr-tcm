//! The pure step loop: what `auto_run_step` used to do inline, now
//! testable directly. `sign_in` is carried out here (the executor only
//! validates it), and a failed `sign_in` stops the rest of the step - the
//! actions after it would run as the wrong person, or as nobody.

mod common;

use common::{account, quick, recipe, stateful_app, ScriptedDriver};
use serde_json::json;
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::recipe::{save_recipe, SignInRecipe};
use v2_lib::autorun::runner::{as_action_outcome, policy_for, run_step};
use v2_lib::autorun::signin::SignInOutcome;
use v2_lib::autorun::StepScript;
use v2_lib::browser::actions::Action;
use v2_lib::browser::cdp::CdpError;

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
    // No recipe saved for this project.
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
    assert!(!out[0].ok && out[0].detail.contains("sign-in recipe"), "{:?}", out[0]);
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
        &v2_lib::autorun::nav::NavFile { direct_urls: false, modules: vec![] },
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
