//! The byte figure shown next to the update progress bar.
//!
//! The download itself needs a Velopack install to exercise, so what is
//! pinned here is the arithmetic the user actually reads: "X of Y".

use v2_lib::updater::{bytes_at, REPO_URL, RELEASES_URL};

/// Both urls point at the SAME repo. They drifted apart once already - the
/// feed was read from one place and this is what stops the package being
/// fetched from another.
#[test]
fn both_urls_name_the_v2_releases_repo() {
    assert!(RELEASES_URL.starts_with(REPO_URL), "{RELEASES_URL} is not under {REPO_URL}");
    assert!(REPO_URL.ends_with("azure-devops-test-case-manager-v2-releases"));
    // v1's repo is a different one, and pointing v2 at it would have the
    // app offer its users the wrong application entirely.
    assert!(!REPO_URL.contains("v2-releases/v"), "REPO_URL must be the repo root, not a release");
}

/// The download that failed for real, reproduced against the live repo.
///
/// `latest/download/` serves whatever release is newest, so the moment
/// 1.18.10 was published, `latest/download/...1.18.9-full.nupkg` started
/// 404ing while 1.18.9's own release still held the file. Ignored by
/// default because it needs the network; run with
/// `cargo test --test updater -- --ignored`.
#[test]
#[ignore = "hits github.com"]
fn a_superseded_version_is_still_downloadable_from_its_own_release() {
    let client = reqwest::blocking::Client::builder()
        .user_agent("tcm-v2-test")
        .build()
        .expect("client");
    let status = |url: String| client.head(&url).send().expect("request failed").status().as_u16();
    let file = "AzureDevOpsTestCaseManager.V2-1.18.9-full.nupkg";
    assert_eq!(status(format!("{RELEASES_URL}{file}")), 404, "latest/ should have moved on");
    assert_eq!(
        status(format!("{REPO_URL}/releases/download/v1.18.9/{file}")),
        200,
        "the per-release url is the one that does not move"
    );
}

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
