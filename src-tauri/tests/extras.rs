//! The optional extras switch: one machine-wide flag in its own small
//! file, read once at start and written through on every change.

use v2_lib::extras::{init, load, save, set_unlocked, unlocked};

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

/// The one test in this binary that touches the process-wide switch -
/// tests run in parallel threads and share statics, so everything that
/// reads or writes it is here, in order.
#[test]
fn the_switch_is_read_at_start_and_written_through() {
    assert!(!unlocked(), "locked until something says otherwise");
    let dir = TempDir::new();
    save(dir.path(), true).unwrap();
    init(dir.path().to_path_buf());
    assert!(unlocked(), "init reads what was saved");
    set_unlocked(false).unwrap();
    assert!(!unlocked());
    assert!(!load(dir.path()), "and a change is written to disk");
    set_unlocked(true).unwrap();
    assert!(unlocked());
    assert!(load(dir.path()));
}
