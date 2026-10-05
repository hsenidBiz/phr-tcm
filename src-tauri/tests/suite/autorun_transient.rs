//! Which failed unattended cases are worth one more go (spec §6): an API
//! check or the page's own request answered 502, 503 or 504, or 400 with
//! an empty body; a request the step waited for failed at the network
//! level (`net::ERR_*`); or the browser stopped answering. Never a text
//! that did not match, a 400 that said why, or a 404.

use serde_json::json;
use v2_lib::autorun::replay::{MODULE_STEP, SIGN_IN_STEP};
use v2_lib::autorun::transient::is_transient;
use v2_lib::autorun::{CaseRecord, CaseScript, StepRecord};
use v2_lib::browser::actions::ActionOutcome;

/// Step 1 clicks, step 2 is the one under test, step 3 checks text.
fn script(step2: serde_json::Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 7, "title": "Save a cycle", "steps": [
        { "step_number": 1, "actions": [{ "kind": "click", "selector": { "css": "#new" } }] },
        { "step_number": 2, "actions": [step2] },
        { "step_number": 3, "actions": [{ "kind": "check_text", "value": "Saved" }] }
    ] }))
    .unwrap()
}

fn expect_save() -> serde_json::Value {
    json!({ "kind": "expect_response", "method": "POST", "url_contains": "/Save", "status": 200 })
}

fn api_get() -> serde_json::Value {
    json!({ "kind": "api_request", "path": "/hr/api/cycles/42" })
}

/// A case that stopped at step 2 with `failed` as that step's one outcome.
fn failed_at_2(proposed: &str, failed: ActionOutcome) -> CaseRecord {
    let reason = format!("step 2: {}", failed.detail);
    CaseRecord {
        case_id: 7,
        title: "Save a cycle".into(),
        verdict: String::new(),
        note: String::new(),
        steps: vec![
            StepRecord { step_number: 1, outcomes: vec![ActionOutcome::passed("clicked #new")], screenshot: None },
            StepRecord { step_number: 2, outcomes: vec![failed], screenshot: None },
            StepRecord {
                step_number: 3,
                outcomes: vec![ActionOutcome::failed("not run: an earlier step of this case failed")],
                screenshot: None,
            },
        ],
        proposed: proposed.into(),
        reason,
        duration_ms: Some(900),
        account: None,
        retried: None,
        notice: None,
    }
}

#[test]
fn a_gateway_error_on_an_api_check_is_transient_and_says_its_first_sentence() {
    for status in [502, 503, 504] {
        let detail = format!("POST /hr/Cycle/Save answered {status}, expected 200");
        let case = failed_at_2("Failed", ActionOutcome::failed(detail.clone()));
        assert_eq!(is_transient(&case, Some(&script(expect_save()))), Some(format!("step 2: {detail}")));

        // With the body it read, too: the status alone decides.
        let with_body = format!("GET /hr/api/cycles/42 answered {status}, expected 200 - the response began: <html>Bad gateway</html>");
        let case = failed_at_2("Failed", ActionOutcome::failed(with_body));
        assert!(is_transient(&case, Some(&script(api_get()))).is_some(), "{status}");
    }
}

#[test]
fn a_400_is_transient_only_with_an_empty_body() {
    let empty = failed_at_2("Failed", ActionOutcome::failed("POST /hr/Cycle/Save answered 400, expected 200"));
    assert!(is_transient(&empty, Some(&script(expect_save()))).is_some());

    let said_why = failed_at_2(
        "Failed",
        ActionOutcome::failed("POST /hr/Cycle/Save answered 400, expected 200 - the response began: {\"error\":\"Name is required\"}"),
    );
    assert_eq!(is_transient(&said_why, Some(&script(expect_save()))), None);
}

#[test]
fn a_404_or_another_status_is_not_transient() {
    for detail in [
        "GET /hr/api/cycles/42 answered 404, expected 200",
        "GET /hr/api/cycles/42 answered 500, expected 200",
        "GET /hr/api/cycles/42 answered 200, expected 201",
    ] {
        let case = failed_at_2("Failed", ActionOutcome::failed(detail));
        assert_eq!(is_transient(&case, Some(&script(api_get()))), None, "{detail}");
    }
}

#[test]
fn a_network_failure_on_a_request_the_step_waited_for_is_transient() {
    let reset = failed_at_2("Failed", ActionOutcome::failed("POST /hr/Cycle/Save failed: net::ERR_CONNECTION_RESET"));
    assert!(is_transient(&reset, Some(&script(expect_save()))).is_some());

    let nav = json!({ "kind": "navigate", "url": "https://hr.example.internal/hr/cycles" });
    let refused = failed_at_2(
        "Failed",
        ActionOutcome::failed("https://hr.example.internal/hr/cycles would not load: net::ERR_CONNECTION_REFUSED"),
    );
    assert!(is_transient(&refused, Some(&script(nav))).is_some());

    // The page calling its own request off is not the network failing.
    let cancelled = failed_at_2("Failed", ActionOutcome::failed("POST /hr/Cycle/Save was cancelled by the page"));
    assert_eq!(is_transient(&cancelled, Some(&script(expect_save()))), None);
    // Nor is a page that never sent it.
    let none = failed_at_2(
        "Failed",
        ActionOutcome::failed("no request matching \"/Save\" in 10 s (this step made 3 requests)"),
    );
    assert_eq!(is_transient(&none, Some(&script(expect_save()))), None);
}

#[test]
fn the_browser_stopping_is_transient_wherever_it_happened() {
    let mut silent = ActionOutcome::failed("the browser did not respond in time");
    silent.harness = true;
    let mut case = failed_at_2("Blocked", silent.clone());
    case.reason = "the browser stopped answering at step 2: the browser did not respond in time".into();
    assert_eq!(is_transient(&case, Some(&script(expect_save()))), Some(case.reason.clone()));
    // Even with no script on this machine to say what the step was.
    assert!(is_transient(&case, None).is_some());

    // While going to the module, too.
    let mut module = case.clone();
    module.steps = vec![
        StepRecord { step_number: SIGN_IN_STEP, outcomes: vec![ActionOutcome::passed("signed in")], screenshot: None },
        StepRecord { step_number: MODULE_STEP, outcomes: vec![silent], screenshot: None },
    ];
    assert!(is_transient(&module, Some(&script(expect_save()))).is_some());
}

#[test]
fn an_assertion_or_a_text_mismatch_is_never_transient() {
    let check = json!({ "kind": "check_text", "value": "Saved" });
    let case = failed_at_2("Failed", ActionOutcome::failed("text \"Saved\" not found"));
    assert_eq!(is_transient(&case, Some(&script(check))), None);

    // The page's words can say anything: a check whose value reads like a
    // gateway error is still the page failing the test.
    let lookalike = json!({ "kind": "check_text", "value": "answered 503" });
    let case = failed_at_2("Failed", ActionOutcome::failed("POST /x answered 503, expected 200"));
    assert_eq!(is_transient(&case, Some(&script(lookalike))), None);
}

#[test]
fn only_the_first_failure_counts() {
    // An ordinary failure first, then a gateway error later in the step.
    let mut case = failed_at_2("Failed", ActionOutcome::failed("text \"Saved\" not found"));
    case.steps[1].outcomes.push(ActionOutcome::failed("POST /hr/Cycle/Save answered 503, expected 200"));
    let sc: CaseScript = serde_json::from_value(json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "click", "selector": { "css": "#new" } }] },
        { "step_number": 2, "actions": [{ "kind": "check_text", "value": "Saved" }, expect_save()] }
    ] }))
    .unwrap();
    assert_eq!(is_transient(&case, Some(&sc)), None);
}

#[test]
fn a_case_that_passed_was_stopped_or_needs_its_script_is_never_transient() {
    let passed = CaseRecord { proposed: "Passed".into(), ..failed_at_2("", ActionOutcome::failed("x")) };
    assert_eq!(is_transient(&passed, Some(&script(expect_save()))), None);

    let gateway = ActionOutcome::failed("POST /hr/Cycle/Save answered 503, expected 200");
    let stopped = failed_at_2("", gateway.clone());
    assert_eq!(is_transient(&stopped, Some(&script(expect_save()))), None, "a stopped case proposes nothing");

    // Without the script the failure's words alone decide nothing.
    assert_eq!(is_transient(&failed_at_2("Failed", gateway), None), None);
}

/// An `api_request` is sent by the page's own `fetch`, which never says
/// `net::ERR_*`: a dropped connection comes back as "Failed to fetch".
#[test]
fn an_api_request_the_network_dropped_is_transient_but_its_timeout_is_not() {
    let dropped = failed_at_2("Failed", ActionOutcome::failed("GET /hr/api/cycles/42 failed: TypeError: Failed to fetch"));
    assert!(is_transient(&dropped, Some(&script(api_get()))).is_some());

    // The page's own limit ran out: a slow server, not a dropped line.
    let slow = failed_at_2("Failed", ActionOutcome::failed("GET /hr/api/cycles/42 failed: timeout"));
    assert_eq!(is_transient(&slow, Some(&script(api_get()))), None);
    // A page that could not send it at all is not the network either.
    let unsent = failed_at_2("Failed", ActionOutcome::failed("GET /hr/api/cycles/42 failed: the page could not send it"));
    assert_eq!(is_transient(&unsent, Some(&script(api_get()))), None);
    // Only an api_request's own sentence: a check reading those words is the page.
    let check = json!({ "kind": "check_text", "value": "Failed to fetch" });
    assert_eq!(is_transient(&dropped, Some(&script(check))), None);
}

/// A case whose sign-in is the first thing that failed.
fn sign_in_failed(outcomes: Vec<ActionOutcome>) -> CaseRecord {
    let last = outcomes.last().unwrap().detail.clone();
    CaseRecord {
        steps: vec![StepRecord { step_number: SIGN_IN_STEP, outcomes, screenshot: None }],
        proposed: "Blocked".into(),
        reason: format!("while signing in: {last}"),
        ..failed_at_2("Blocked", ActionOutcome::failed("x"))
    }
}

/// The sign-in page not loading at all (a server restarting) is the most
/// common transient there is.
#[test]
fn a_sign_in_page_that_would_not_load_is_transient() {
    let url = "https://hr.example.internal/";
    let refused = sign_in_failed(vec![
        ActionOutcome::failed(format!("{url} would not load: net::ERR_CONNECTION_REFUSED")),
        ActionOutcome::failed(format!("the sign-in page did not open: {url} would not load: net::ERR_CONNECTION_REFUSED")),
    ]);
    assert_eq!(is_transient(&refused, Some(&script(expect_save()))), Some(refused.reason.clone()));
    // Even with no script: the sign-in is the runner's, not the script's.
    assert!(is_transient(&refused, None).is_some());

    // The page calling its own load off is not the network.
    let aborted = sign_in_failed(vec![ActionOutcome::failed(format!("{url} would not load: net::ERR_ABORTED"))]);
    assert_eq!(is_transient(&aborted, Some(&script(expect_save()))), None);
    // A sign-in that loaded and then did not work is never transient.
    let wrong = sign_in_failed(vec![ActionOutcome::failed(
        "the signed-in marker #home never appeared - check the username and password for \"admin\"",
    )]);
    assert_eq!(is_transient(&wrong, Some(&script(expect_save()))), None);
    // Nor a recipe check whose words read like a failed load.
    let lookalike = sign_in_failed(vec![ActionOutcome::failed(
        "text \"x would not load: net::ERR_CONNECTION_RESET\" not found",
    )]);
    assert_eq!(is_transient(&lookalike, Some(&script(expect_save()))), None);
}

#[test]
fn a_mid_script_sign_in_whose_page_would_not_load_is_transient() {
    let sign_in = json!({ "kind": "sign_in", "account": "admin" });
    let refused = failed_at_2(
        "Blocked",
        ActionOutcome::failed("the sign-in page did not open: https://hr.example.internal/ would not load: net::ERR_CONNECTION_RESET"),
    );
    assert!(is_transient(&refused, Some(&script(sign_in.clone()))).is_some());
    let wrong = failed_at_2("Blocked", ActionOutcome::failed("sign-in stopped at step 2: #password is not on the page"));
    assert_eq!(is_transient(&wrong, Some(&script(sign_in))), None);
}
