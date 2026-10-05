//! The optional extras switch and Enable Advanced Features: two
//! machine-wide flags in one small file, read once at start and written
//! through on every change.

use v2_lib::ai_tools::autorun_offered_for;
use v2_lib::extras::{
    advanced, features_on, init, load, load_advanced, save, save_advanced, set_advanced, set_unlocked, unlocked,
};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-extras-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_missing_or_unreadable_file_means_locked() {
    let dir = TempDir::new();
    assert!(!load(dir.path()), "nothing saved yet");
    std::fs::write(dir.path().join("extras.json"), "{not json").unwrap();
    assert!(!load(dir.path()), "garbage");
    std::fs::write(dir.path().join("extras.json"), r#"{"unlocked":"yes"}"#).unwrap();
    assert!(!load(dir.path()), "the wrong shape");
    std::fs::write(dir.path().join("extras.json"), "{}").unwrap();
    assert!(!load(dir.path()), "no field");
}

#[test]
fn saving_round_trips_and_unknown_fields_are_ignored() {
    let dir = TempDir::new();
    save(dir.path(), true).unwrap();
    assert!(load(dir.path()));
    save(dir.path(), false).unwrap();
    assert!(!load(dir.path()));
    std::fs::write(dir.path().join("extras.json"), r#"{"unlocked":true,"later":1}"#).unwrap();
    assert!(load(dir.path()), "a field a later version adds must not relock");
}

#[test]
fn saving_creates_the_folder() {
    let dir = TempDir::new();
    let nested = dir.path().join("a").join("b");
    save(&nested, true).unwrap();
    assert!(load(&nested));
}

#[test]
fn an_older_file_without_advanced_loads_it_as_off() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("extras.json"), r#"{"unlocked":true}"#).unwrap();
    assert!(load(dir.path()), "the extras switch still reads as saved");
    assert!(!load_advanced(dir.path()), "a file from before the switch existed reads as off");
    assert!(!load_advanced(TempDir::new().path()), "nothing saved yet");
}

#[test]
fn saving_one_flag_keeps_the_other() {
    let dir = TempDir::new();
    save(dir.path(), true).unwrap();
    save_advanced(dir.path(), true).unwrap();
    assert!(load(dir.path()), "saving advanced must not wipe the extras switch");
    assert!(load_advanced(dir.path()));
    save(dir.path(), false).unwrap();
    assert!(load_advanced(dir.path()), "saving the extras switch must not wipe advanced");
    assert!(!load(dir.path()));
    save(dir.path(), true).unwrap();
    save_advanced(dir.path(), false).unwrap();
    assert!(load(dir.path()), "turning advanced off leaves the extras switch on");
    assert!(!load_advanced(dir.path()));
}

/// The one test that touches the process-wide flags and the folder `init`
/// sets once for the whole binary - so everything that reads or writes them
/// is here, in order, under the shared lock.
#[test]
fn the_switch_is_read_at_start_and_written_through() {
    let _lock = crate::serial::extras();
    assert!(!unlocked(), "locked until something says otherwise");
    assert!(!advanced(), "off until something says otherwise");
    assert!(!features_on());
    // Must run before `init` anywhere in this process sets DIR - it is a
    // OnceLock, set at most once for the whole test binary.
    assert!(set_unlocked(true).is_err(), "nowhere to save before init runs");
    assert!(set_advanced(true).is_err(), "nowhere to save before init runs");
    assert!(!unlocked(), "a refused save must not be published either");
    assert!(!advanced(), "a refused save must not be published either");
    let dir = TempDir::new();
    save(dir.path(), true).unwrap();
    init(dir.path().to_path_buf());
    assert!(unlocked(), "init reads what was saved");
    assert!(!advanced(), "and only what was saved");
    set_unlocked(false).unwrap();
    assert!(!unlocked());
    assert!(!load(dir.path()), "and a change is written to disk");
    set_unlocked(true).unwrap();
    assert!(unlocked());
    assert!(load(dir.path()));

    // features_on: either flag turns them on.
    assert!(features_on(), "the extras switch alone");
    set_unlocked(false).unwrap();
    assert!(!features_on(), "neither");
    assert!(!autorun_offered_for(false, features_on()), "a release build with neither does not offer the tools");
    set_advanced(true).unwrap();
    assert!(advanced());
    assert!(load_advanced(dir.path()), "advanced is written to disk");
    assert!(!load(dir.path()), "without turning the extras switch back on");
    assert!(features_on(), "advanced alone");
    assert!(autorun_offered_for(false, features_on()), "a release build with advanced on offers the tools");
    set_unlocked(true).unwrap();
    assert!(features_on(), "both");
    assert!(load_advanced(dir.path()), "saving the extras switch kept advanced");

    // Leave the process as it started.
    set_advanced(false).unwrap();
    set_unlocked(false).unwrap();
    assert!(!features_on());
}
