//! The pacer's starting level. Its own test binary, so no other test has
//! moved the process-wide interval before this one reads it.

use v2_lib::ado::throttle;

/// Full speed is the default: until the Settings choice is pushed down (and
/// for anyone who never picked one), requests go out with no gap.
#[test]
fn the_pacer_starts_at_full_speed_and_unknown_levels_stay_throttled() {
    assert_eq!(throttle::current_interval_ms(), 0, "the app starts unthrottled");
    assert_eq!(throttle::interval_for("full"), 0);
    assert_eq!(throttle::interval_for("balanced"), 200);
    assert_eq!(throttle::interval_for("gentle"), 800);
    // A value the app does not know is never read as "no limit": a typo in
    // storage must not quietly take the brakes off.
    assert_eq!(throttle::interval_for("fulll"), 200);
    assert_eq!(throttle::interval_for(""), 200);
}
