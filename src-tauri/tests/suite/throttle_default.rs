//! The pacer's starting level. This used to be a test binary of its own so
//! that no other test could have moved the process-wide interval before
//! this one read it; in the one-binary suite the pacer lock does that job.
//! Every test that changes the level holds the same lock and puts back the
//! level it found before letting go (throttle_backoff's `Gate`), so what
//! this reads is still the level the process started with.

use v2_lib::ado::throttle;

/// Full speed is the default: until the Settings choice is pushed down (and
/// for anyone who never picked one), requests go out with no gap.
#[test]
fn the_pacer_starts_at_full_speed_and_unknown_levels_stay_throttled() {
    let _pacer = crate::serial::pacer();
    assert_eq!(throttle::current_interval_ms(), 0, "the app starts unthrottled");
    assert_eq!(throttle::interval_for("full"), 0);
    assert_eq!(throttle::interval_for("balanced"), 200);
    assert_eq!(throttle::interval_for("gentle"), 800);
    // A value the app does not know is never read as "no limit": a typo in
    // storage must not quietly take the brakes off.
    assert_eq!(throttle::interval_for("fulll"), 200);
    assert_eq!(throttle::interval_for(""), 200);
}
