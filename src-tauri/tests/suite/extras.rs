//! The optional extras switch: a machine-wide flag in one small file (beside
//! the old advanced flag, kept for the upgrade only), read once at start and
//! written through on every change.

use v2_lib::extras::{init, load, load_advanced, save, save_advanced, set_unlocked, unlocked};

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
    let _modules = crate::serial::modules();
    let modules_before = v2_lib::modules::current();
    assert!(!unlocked(), "locked until something says otherwise");
    // Must run before `init` anywhere in this process sets DIR - it is a
    // OnceLock, set at most once for the whole test binary.
    assert!(set_unlocked(true).is_err(), "nowhere to save before init runs");
    assert!(!unlocked(), "a refused save must not be published either");
    let dir = TempDir::new();
    save(dir.path(), true).unwrap();
    init(dir.path().to_path_buf());
    assert!(unlocked(), "init reads what was saved");
    assert!(!load_advanced(dir.path()), "and only what was saved");
    set_unlocked(false).unwrap();
    assert!(!unlocked());
    assert!(!load(dir.path()), "and a change is written to disk");
    set_unlocked(true).unwrap();
    assert!(unlocked());
    assert!(load(dir.path()));

    // Neither flag gates anything any more: the modules decide.
    set_unlocked(false).unwrap();
    assert_eq!(v2_lib::modules::current(), modules_before, "the extras switch moves no module");
    save_advanced(dir.path(), true).unwrap();
    assert!(load_advanced(dir.path()), "advanced is written to disk");
    assert!(!load(dir.path()), "without turning the extras switch back on");
    assert_eq!(v2_lib::modules::current(), modules_before, "Enable Advanced Features moves no module");
    set_unlocked(true).unwrap();
    assert!(load_advanced(dir.path()), "saving the extras switch kept advanced");
    assert_eq!(v2_lib::modules::current(), modules_before, "nor do both");

    // Leave the process as it started.
    save_advanced(dir.path(), false).unwrap();
    set_unlocked(false).unwrap();
}
