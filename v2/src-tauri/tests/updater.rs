//! The byte figure shown next to the update progress bar.
//!
//! The download itself needs a Velopack install to exercise, so what is
//! pinned here is the arithmetic the user actually reads: "X of Y".

use v2_lib::updater::bytes_at;

#[test]
fn the_ends_are_exact() {
    // A size that is deliberately not a round hundred: `total / 100 * p`
    // silently drops the remainder, so a finished download would report
    // one byte short of the size it just told the user it was fetching.
    let total = 25_000_001;
    assert_eq!(bytes_at(0, total), 0);
    assert_eq!(bytes_at(100, total), total, "100% must be the whole package");
}

#[test]
fn the_middle_is_the_floor_not_a_rounding() {
    // 50% of 25,000,001 is 12,500,000.5 - claiming the extra byte would be
    // claiming a byte that has not arrived.
    assert_eq!(bytes_at(50, 25_000_001), 12_500_000);
    assert_eq!(bytes_at(5, 24_800_000), 1_240_000);
    assert_eq!(bytes_at(95, 24_800_000), 23_560_000);
}

#[test]
fn a_percentage_out_of_range_is_clamped() {
    // Velopack should only ever send 0-100, but this feeds a progress bar
    // and a byte count: out-of-range must land on an end, never wrap or
    // read as more bytes than the package holds.
    assert_eq!(bytes_at(-1, 24_800_000), 0);
    assert_eq!(bytes_at(-32_768, 24_800_000), 0);
    assert_eq!(bytes_at(101, 24_800_000), 24_800_000);
    assert_eq!(bytes_at(32_767, 24_800_000), 24_800_000);
}

#[test]
fn an_absurd_size_does_not_overflow() {
    assert_eq!(bytes_at(100, u64::MAX), u64::MAX);
    assert_eq!(bytes_at(50, u64::MAX), u64::MAX / 2);
}

#[test]
fn an_unknown_size_stays_zero() {
    // The feed always gives a size, but a zero must not become a division
    // by zero or a bar that fills from nothing.
    assert_eq!(bytes_at(0, 0), 0);
    assert_eq!(bytes_at(50, 0), 0);
    assert_eq!(bytes_at(100, 0), 0);
}
