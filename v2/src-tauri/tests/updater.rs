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

/// The process must not keep the install's `current\` as its working
/// directory.
///
/// Velopack launches the app with cwd = `current\`, and every child the
/// app starts without an explicit cwd - the browser behind "View in
/// Browser", Auto Run's Edge, `claude mcp add` - inherits it. A process's
/// cwd pins that directory against rename, and renaming `current\` is the
/// first thing Update.exe does when applying an update. On 2026-08-21 a
/// user's 1.20.2 -> 1.20.5 update failed three times with "os error 32"
/// because Edge, opened from the app that morning, still sat in
/// `current\`. Update.exe kills processes whose EXE is under the install
/// root, but a browser's exe is not, so only the app can prevent this - by
/// leaving the directory before anything can inherit it.
#[test]
fn the_process_leaves_the_install_dir_so_children_cannot_pin_it() {
    let root = std::env::temp_dir().join(format!("tcm-leave-{}", std::process::id()));
    let current = root.join("current");
    std::fs::create_dir_all(&current).unwrap();
    let was = std::env::current_dir().unwrap();
    std::env::set_current_dir(&current).unwrap();

    v2_lib::leave_install_dir();

    let now = std::env::current_dir().unwrap();
    assert!(!now.starts_with(&root), "still inside the install dir: {}", now.display());
    // The point of leaving: Update.exe can now rename `current\`.
    let moved = root.join("current.bak");
    std::fs::rename(&current, &moved).expect("the current dir should be renameable once nothing sits in it");

    std::env::set_current_dir(&was).unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

/// The forensic marker behind "your last update didn't finish".
///
/// The apply runs after the app has exited and reports its failure only to
/// Velopack's own log, so the app restarting on the OLD version was
/// indistinguishable from never having clicked at all - the banner just
/// came back. `note_attempt` + `failed_attempt` close that gap: aim is
/// recorded before the hand-off, outcome is judged on the next launch.
mod update_attempt_marker {
    use v2_lib::updater::{failed_attempt, note_attempt};

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tcm-attempt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn still_on_the_old_version_means_the_apply_failed_and_the_marker_survives() {
        let d = dir("failed");
        note_attempt(&d, "1.20.8");
        assert_eq!(failed_attempt(&d, "1.20.7"), Some("1.20.8".into()));
        // Kept: the explanation must survive further restarts of the old
        // version, not vanish after being shown once.
        assert_eq!(failed_attempt(&d, "1.20.7"), Some("1.20.8".into()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reaching_or_passing_the_target_clears_the_marker() {
        let d = dir("landed");
        note_attempt(&d, "1.20.8");
        assert_eq!(failed_attempt(&d, "1.20.8"), None, "the update landed - nothing failed");
        assert!(!d.join("update-attempt.txt").exists(), "a resolved marker must not linger");

        // Overshot (hand-copied files, a skipped release): also not a failure.
        note_attempt(&d, "1.20.8");
        assert_eq!(failed_attempt(&d, "1.21.0"), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn no_marker_or_an_unreadable_one_alarms_no_one() {
        let d = dir("noise");
        assert_eq!(failed_attempt(&d, "1.20.7"), None);
        std::fs::write(d.join("update-attempt.txt"), "not-a-version").unwrap();
        assert_eq!(failed_attempt(&d, "1.20.7"), None);
        assert!(!d.join("update-attempt.txt").exists(), "garbage must be cleaned up, not re-read forever");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_newer_attempt_overwrites_the_old_aim() {
        let d = dir("overwrite");
        note_attempt(&d, "1.20.8");
        note_attempt(&d, "1.20.9");
        assert_eq!(failed_attempt(&d, "1.20.7"), Some("1.20.9".into()));
        let _ = std::fs::remove_dir_all(&d);
    }
}

use v2_lib::updater::{resolve, sources, Attempt, UpdateState};

/// Signed out, DevOps is not even tried: the token is what makes it
/// answerable, and a launch-time check with no session must behave
/// exactly as it did before DevOps existed.
#[test]
fn without_a_token_devops_is_not_in_the_list() {
    let (list, denied) = sources(None, false);
    let names: Vec<_> = list.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["github api", "latest/download"]);
    assert!(denied.is_none());
}

#[test]
fn with_a_token_devops_is_tried_first() {
    let (list, denied) = sources(Some("tok".into()), false);
    let names: Vec<_> = list.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["ado", "github api", "latest/download"]);
    assert!(denied.is_some());
}

/// The Settings switch that exists to prove DevOps works on its own: with
/// it on, GitHub is not merely tried last - it is not tried at all.
#[test]
fn with_github_switched_off_devops_is_the_only_source() {
    let (list, _) = sources(Some("tok".into()), true);
    let names: Vec<_> = list.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["ado"]);
    // ...and signed out there is nothing left to ask. `check` turns this
    // into a "sign in" message rather than the not-an-install one.
    let (list, _) = sources(None, true);
    assert!(list.is_empty());
}

fn info(version: &str) -> Box<velopack::UpdateInfo> {
    let mut i = velopack::UpdateInfo::default();
    i.TargetFullRelease.Version = version.into();
    Box::new(i)
}

/// DevOps said no, GitHub served the update: the user gets the update AND
/// is told access is missing - both are true, neither hides the other.
#[test]
fn no_access_and_an_update_from_github_are_both_reported() {
    let state = UpdateState::default();
    let s = resolve(
        vec![("ado", Attempt::Failed("403".into())), ("github api", Attempt::Available(info("1.23.0")))],
        true,
        &state,
    );
    assert_eq!(s.available.as_deref(), Some("1.23.0"));
    assert!(s.no_access);
    assert!(s.blocked.is_none());
    assert!(state.pending.lock().unwrap().is_some(), "the pending info is kept for the download");
}

#[test]
fn no_access_and_no_fallback_is_blocked_and_no_access() {
    let state = UpdateState::default();
    let s = resolve(
        vec![("ado", Attempt::Failed("403".into())), ("github api", Attempt::Failed("timeout".into()))],
        true,
        &state,
    );
    assert!(s.available.is_none());
    assert!(s.no_access);
    assert!(s.blocked.as_deref().unwrap().contains("timeout"), "the LAST failure is the one named");
}

#[test]
fn up_to_date_from_the_first_source_is_a_plain_up_to_date() {
    let state = UpdateState::default();
    let s = resolve(vec![("ado", Attempt::UpToDate)], false, &state);
    assert!(s.available.is_none() && s.blocked.is_none() && !s.no_access);
}

#[test]
fn no_attempts_at_all_means_this_build_cannot_update() {
    let state = UpdateState::default();
    let s = resolve(vec![], false, &state);
    assert!(s.blocked.as_deref().unwrap().contains("does not update itself"));
}
