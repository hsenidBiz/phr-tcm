//! The removed area: removing an environment moves its Auto Run folder to
//! `removed-envs/<id>-<ms>` for 30 days, lists and purges those folders by
//! age, never resolves a key that is not listed, and never lets a normal
//! store write into one.

use std::path::Path;
use v2_lib::autorun::roots::{self, REMOVED_DIR};
use v2_lib::autorun::store::{self, DAY_MS};
use v2_lib::environments::{self, Environment};

/// Leaves the process-wide layout on a throw-away flat folder, so a later
/// test never sees a path that is gone.
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        store::set_root(std::env::temp_dir().join("tcm-autorun-removed-restored"));
    }
}

fn env_named(name: &str) -> Environment {
    Environment {
        id: String::new(),
        name: name.into(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: "dev-read".into(),
        test_environment: false,
        test_prefix: "AUTOTEST".into(),
        db_auto_approve: false,
    }
}

/// A shared root with two environments, and a script saved in each one's
/// folder. Returns the two ids.
fn two_envs_with_scripts(t: &Path) -> (String, String) {
    let a = environments::load_or_init(t, Some("dev-read")).unwrap().active;
    let b = environments::save_env(t, env_named("QA"), &["dev-read".to_string()])
        .unwrap()
        .environments
        .into_iter()
        .find(|e| e.name == "QA")
        .unwrap()
        .id;
    for id in [&a, &b] {
        let scripts = roots::env_root(t, id).join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("case-1.json"), format!("{{\"for\":\"{id}\"}}")).unwrap();
    }
    (a, b)
}

const NOW: u64 = 5_000_000_000_000;

#[test]
fn removing_an_environment_keeps_its_autorun_folder_in_the_removed_area() {
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs_with_scripts(t.path());
    environments::remove_env(t.path(), &b).unwrap();
    assert!(!roots::env_root(t.path(), &b).exists(), "the live folder is gone");
    assert!(roots::env_root(t.path(), &a).exists(), "the other environment is untouched");
    let listed = roots::removed_folders(t.path(), v2_lib::autorun::sessions::now_ms());
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].env_id, b);
    assert_eq!(listed[0].name, "QA");
    assert_eq!(listed[0].reason, "removed");
    let kept = t.path().join(REMOVED_DIR).join(&listed[0].key);
    let script = std::fs::read_to_string(kept.join("scripts").join("case-1.json")).unwrap();
    assert!(script.contains(&b));
}

#[test]
fn removed_folders_lists_only_the_last_30_days() {
    let t = tempfile::tempdir().unwrap();
    let (_a, b) = two_envs_with_scripts(t.path());
    roots::retire(t.path(), &b, "QA", "removed", NOW - 31 * DAY_MS).unwrap().unwrap();
    // A second folder for the same id, as when the same id comes back.
    std::fs::create_dir_all(roots::env_root(t.path(), &b)).unwrap();
    roots::retire(t.path(), &b, "QA", "replaced", NOW - DAY_MS).unwrap().unwrap();
    let listed = roots::removed_folders(t.path(), NOW);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].reason, "replaced");
    assert_eq!(listed[0].removed_at_ms, NOW - DAY_MS);
}

#[test]
fn purge_removes_folders_older_than_30_days_only() {
    let t = tempfile::tempdir().unwrap();
    let (_a, b) = two_envs_with_scripts(t.path());
    let old = roots::retire(t.path(), &b, "QA", "removed", NOW - 31 * DAY_MS).unwrap().unwrap();
    std::fs::create_dir_all(roots::env_root(t.path(), &b)).unwrap();
    let recent = roots::retire(t.path(), &b, "QA", "removed", NOW - DAY_MS).unwrap().unwrap();
    // A folder that is not a retired one is left alone.
    std::fs::create_dir_all(t.path().join(REMOVED_DIR).join("notes")).unwrap();
    assert_eq!(roots::purge_removed(t.path(), NOW), 1);
    assert!(!t.path().join(REMOVED_DIR).join(old).exists());
    assert!(t.path().join(REMOVED_DIR).join(recent).exists());
    assert!(t.path().join(REMOVED_DIR).join("notes").exists());
}

#[test]
fn a_removed_environment_without_a_folder_is_removed_cleanly() {
    let t = tempfile::tempdir().unwrap();
    let (_a, b) = two_envs_with_scripts(t.path());
    std::fs::remove_dir_all(roots::env_root(t.path(), &b)).unwrap();
    let file = environments::remove_env(t.path(), &b).unwrap();
    assert_eq!(file.environments.len(), 1);
    assert!(roots::removed_folders(t.path(), NOW).is_empty());
    assert!(!t.path().join(REMOVED_DIR).exists(), "nothing was made for it");
}

#[test]
fn removed_root_refuses_unknown_and_expired_keys() {
    let t = tempfile::tempdir().unwrap();
    let (_a, b) = two_envs_with_scripts(t.path());
    let old = roots::retire(t.path(), &b, "QA", "removed", NOW - 31 * DAY_MS).unwrap().unwrap();
    std::fs::create_dir_all(roots::env_root(t.path(), &b)).unwrap();
    let recent = roots::retire(t.path(), &b, "QA", "removed", NOW - DAY_MS).unwrap().unwrap();
    assert_eq!(roots::removed_root(t.path(), "../x", NOW), None);
    assert_eq!(roots::removed_root(t.path(), &old, NOW), None, "past its 30 days");
    assert_eq!(roots::removed_root(t.path(), "env-0000000b-1", NOW), None);
    assert_eq!(roots::removed_root(t.path(), &recent, NOW), Some(t.path().join(REMOVED_DIR).join(&recent)));
}

#[test]
fn the_active_or_a_busy_environment_is_neither_removed_nor_retired() {
    let _lock = crate::serial::autorun();
    let _held = crate::serial::held_browsers();
    let _leases = crate::serial::account_leases();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    let (a, b) = two_envs_with_scripts(t.path());
    store::select_environment(&b).unwrap();
    // Active: retire is refused and the folder stays.
    let err = roots::retire(t.path(), &b, "QA", "removed", NOW).unwrap_err();
    assert!(err.contains("active"), "{err}");
    assert!(roots::env_root(t.path(), &b).join("scripts").exists());
    assert!(!t.path().join(REMOVED_DIR).exists());
    // Busy (a copy task is writing into it): removal is refused before the
    // environment leaves the list, and retire is refused too.
    store::select_environment(&a).unwrap();
    let busy = roots::allow_writes_to(&roots::env_root(t.path(), &b));
    let err = environments::remove_env(t.path(), &b).unwrap_err();
    assert!(err.contains("being used"), "{err}");
    assert!(environments::load_or_init(t.path(), None).unwrap().environments.iter().any(|e| e.id == b));
    assert!(roots::retire(t.path(), &b, "QA", "removed", NOW).is_err());
    assert!(roots::env_root(t.path(), &b).join("scripts").exists());
    drop(busy);
    environments::remove_env(t.path(), &b).unwrap();
    assert!(!roots::env_root(t.path(), &b).exists());
}

#[test]
fn nothing_writes_into_a_removed_folder_through_the_normal_stores() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    let (_a, b) = two_envs_with_scripts(t.path());
    let key = roots::retire(t.path(), &b, "QA", "removed", NOW).unwrap().unwrap();
    let kept = t.path().join(REMOVED_DIR).join(&key);
    let file = kept.join("scripts").join("late.json");
    let err = roots::write_file(&kept, &file, "{}").unwrap_err();
    assert_eq!(err, roots::REMOVED_READ_ONLY);
    assert!(roots::guard(&kept).is_err());
    assert!(roots::create_dir_all(&kept, kept.join("x")).is_err());
    assert!(!file.exists());
    assert!(!kept.join("x").exists());
    // A change there is not something the person is looking at.
    assert!(!roots::is_on_screen(&kept));
}
