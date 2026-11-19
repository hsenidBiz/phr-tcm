//! The rule for when an Auto Test case may reuse the browser the case
//! before it signed in (`autorun::kept_browser`).

use v2_lib::autorun::kept_browser::{ended_from, may_keep, may_reuse, reused_detail, Ended, Fresh, Next};
use v2_lib::autorun::runner::AFTER_STOP;
use v2_lib::autorun::{CaseRecord, CaseScript};

fn before() -> Ended {
    Ended { account: Some("a".into()), proposed: "Passed".into(), stopped: false, signed_in_ok: true, script_signs_in: false, no_save: false }
}

fn next() -> Next<'static> {
    Next { account: Some("a"), has_route: true, pause_between: false }
}

#[test]
fn reuses_same_account_with_route() {
    assert_eq!(may_reuse(Some(&before()), &next()), Ok(()));
}

#[test]
fn no_browser_before() {
    assert_eq!(may_reuse(None, &next()), Err(Fresh::NoBrowser));
}

#[test]
fn account_changed() {
    assert_eq!(may_reuse(Some(&before()), &Next { account: Some("b"), ..next() }), Err(Fresh::AccountChanged));
}

#[test]
fn next_without_account_is_a_change() {
    assert_eq!(may_reuse(Some(&before()), &Next { account: None, ..next() }), Err(Fresh::AccountChanged));
}

#[test]
fn before_without_account_is_a_change() {
    let b = Ended { account: None, ..before() };
    assert!(!may_keep(&b));
    assert_eq!(may_reuse(Some(&b), &next()), Err(Fresh::AccountChanged));
}

#[test]
fn no_module_trip() {
    assert_eq!(may_reuse(Some(&before()), &Next { has_route: false, ..next() }), Err(Fresh::NoModuleTrip));
}

#[test]
fn before_not_passed() {
    let b = Ended { proposed: "Failed".into(), ..before() };
    assert_eq!(may_reuse(Some(&b), &next()), Err(Fresh::BeforeNotPassed));
}

#[test]
fn before_stopped() {
    let b = Ended { stopped: true, ..before() };
    assert_eq!(may_reuse(Some(&b), &next()), Err(Fresh::BeforeStopped));
}

#[test]
fn before_sign_in_failed() {
    let b = Ended { signed_in_ok: false, ..before() };
    assert_eq!(may_reuse(Some(&b), &next()), Err(Fresh::BeforeSignInFailed));
}

#[test]
fn before_signed_in_by_script() {
    let b = Ended { script_signs_in: true, ..before() };
    assert_eq!(may_reuse(Some(&b), &next()), Err(Fresh::BeforeSignedInByScript));
}

#[test]
fn before_no_save() {
    let b = Ended { no_save: true, ..before() };
    assert_eq!(may_reuse(Some(&b), &next()), Err(Fresh::BeforeNoSave));
}

#[test]
fn paused() {
    assert_eq!(may_reuse(Some(&before()), &Next { pause_between: true, ..next() }), Err(Fresh::Paused));
}

#[test]
fn stopped_case_reports_stopped_before_not_passed() {
    let b = Ended { stopped: true, proposed: String::new(), ..before() };
    assert_eq!(may_reuse(Some(&b), &next()), Err(Fresh::BeforeStopped));
}

fn record(steps: serde_json::Value, proposed: &str, reason: &str) -> CaseRecord {
    serde_json::from_value(serde_json::json!({
        "case_id": 1, "title": "t", "verdict": "", "note": "",
        "steps": steps, "proposed": proposed, "reason": reason
    }))
    .unwrap()
}

fn script(steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(serde_json::json!({ "case_id": 1, "title": "t", "account": "x", "steps": steps })).unwrap()
}

#[test]
fn run_account_wins() {
    let e = ended_from(&record(serde_json::json!([]), "Passed", ""), &script(serde_json::json!([])), Some("a"));
    assert_eq!(e.account, Some("a".to_string()));
}

#[test]
fn ended_from_reads_record() {
    let failed0 = serde_json::json!([{ "step_number": 0, "outcomes": [{ "ok": false, "detail": "no" }] }]);
    let e = ended_from(&record(failed0, "Failed", ""), &script(serde_json::json!([])), Some("a"));
    assert!(!e.signed_in_ok);
    let ok0 = serde_json::json!([{ "step_number": 0, "outcomes": [{ "ok": true, "detail": "" }] }]);
    assert!(ended_from(&record(ok0, "Passed", ""), &script(serde_json::json!([])), Some("a")).signed_in_ok);
    assert!(ended_from(&record(serde_json::json!([]), "Passed", ""), &script(serde_json::json!([])), Some("a")).signed_in_ok);

    let signs = script(serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "sign_in", "account": "b" }] }]));
    assert!(ended_from(&record(serde_json::json!([]), "Passed", ""), &signs, Some("a")).script_signs_in);
    assert!(!ended_from(&record(serde_json::json!([]), "Passed", ""), &script(serde_json::json!([])), Some("a")).script_signs_in);
}

#[test]
fn ended_from_reads_stop_and_no_save() {
    let s = serde_json::from_value::<CaseScript>(serde_json::json!({ "case_id": 1, "title": "t", "steps": [], "no_save": true })).unwrap();
    let e = ended_from(&record(serde_json::json!([]), "", AFTER_STOP), &s, Some("a"));
    assert!(e.stopped && e.no_save);
}

#[test]
fn reused_detail_wording() {
    assert_eq!(
        reused_detail(840),
        "used the browser the case before signed in (checked it was still signed in in 840 ms)"
    );
}
