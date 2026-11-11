//! A sign-in trial reports itself (A2 F9).

use v2_lib::autorun::signin_trial::{self, SignInTrial};

// The signin_proposal_bridge tests start trials without this lock, so these
// check only their own environment name.
fn mine(name: &str) -> bool {
    signin_trial::current().is_some_and(|t| t.environment == name)
}

#[test]
fn a_trial_shows_while_it_runs_and_goes_after() {
    let _g = crate::serial::sign_in_trial();
    let name = "Staging-trial-test";
    assert!(!mine(name));
    let m = signin_trial::start(name);
    assert!(mine(name));
    drop(m);
    assert!(!mine(name));
}

#[test]
fn a_trial_carries_no_address_or_login() {
    let v = serde_json::to_value(SignInTrial { environment: "Staging".into() }).unwrap();
    assert_eq!(v, serde_json::json!({ "environment": "Staging" }));
}
