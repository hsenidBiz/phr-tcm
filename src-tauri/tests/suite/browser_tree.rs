//! The pieces of `browser::tree` that need no browser: the start-up sweep
//! of leftover profile folders, and the line logged when a browser's first
//! process is found gone. The live half is `browser_live_tree`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use v2_lib::browser::tree::{self, Ends};

/// A tree some browser of this app is running on right now.
struct Running(PathBuf);

impl Ends for Running {
    fn terminate(&self) {}
    fn end(&self) -> bool {
        true
    }
    fn profile_dir(&self) -> &Path {
        &self.0
    }
}

#[test]
fn the_dead_browser_log_line_names_pid_job_count_and_devtools() {
    let line = v2_lib::browser::launch::pid_gone_line(4242, Some(7), true);
    assert!(line.contains("pid 4242") && line.contains("7 processes") && line.contains("DevTools answered"), "{line}");
    let line = v2_lib::browser::launch::pid_gone_line(4242, Some(0), false);
    assert!(line.contains("0 processes") && line.contains("did not answer"), "{line}");
    assert!(!line.contains("127.0.0.1") && !line.contains("http") && !line.contains('?'), "{line}");
}

#[test]
fn startup_sweep_removes_only_tcm_autorun_digit_folders() {
    let _held = crate::serial::held_browsers();
    let temp = tempfile::tempdir().unwrap();
    let make = |name: &str| {
        let d = temp.path().join(name);
        std::fs::create_dir_all(d.join("Default")).unwrap();
        std::fs::write(d.join("Default").join("Preferences"), "{}").unwrap();
        d
    };
    let gone = make("tcm-autorun-123");
    let kept: Vec<PathBuf> = [
        "tcm-autorun-bridge-1-2",
        "tcm-autorun-discovery-1-3",
        "tcm-autorun-defects-4",
        "tcm-autorun-",
        "tcm-autorun-12a",
        "xtcm-autorun-5",
        "tcm-autorun-5.old",
    ]
    .iter()
    .map(|n| make(n))
    .collect();
    std::fs::write(temp.path().join("tcm-autorun-77"), "a file, not a profile").unwrap();
    // One a browser of this app is running on right now.
    let live_dir = make("tcm-autorun-456");
    let live = Arc::new(Running(live_dir.clone()));
    tree::register(&live);

    let removed = tree::sweep_leftover_profiles(temp.path());
    assert_eq!(removed, vec!["tcm-autorun-123".to_string()]);
    assert!(!gone.exists());
    for d in &kept {
        assert!(d.join("Default").join("Preferences").is_file(), "{} was touched", d.display());
    }
    assert!(temp.path().join("tcm-autorun-77").is_file());
    assert!(live_dir.join("Default").join("Preferences").is_file(), "a live browser's profile is left alone");
    assert!(tree::is_profile_name("tcm-autorun-0"));
    assert!(!tree::is_profile_name("tcm-autorun-bridge-1"));
    drop(live);
}

/// A folder something still has a file open in is skipped whole, never
/// half removed under a browser still running on it.
#[cfg(windows)]
#[test]
fn a_folder_in_use_is_left_alone_by_the_sweep() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("tcm-autorun-9001");
    std::fs::create_dir_all(dir.join("Default")).unwrap();
    std::fs::write(dir.join("Default").join("Preferences"), "{}").unwrap();
    std::fs::write(dir.join("lockfile"), "").unwrap();
    // Opened as a running browser holds its lock file: shared with no one.
    let held = std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(dir.join("lockfile")).unwrap();

    let removed = tree::sweep_leftover_profiles(temp.path());
    assert!(removed.is_empty(), "{removed:?}");
    assert!(dir.join("Default").join("Preferences").is_file(), "nothing in it was removed");
    drop(held);
    assert_eq!(tree::sweep_leftover_profiles(temp.path()), vec!["tcm-autorun-9001".to_string()]);
}
