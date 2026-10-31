//! Copying a whole environment's Auto Run data: what comes across, what
//! never does, what a replace keeps aside, and the sources offered.

use std::path::Path;
use v2_lib::autorun::env_copy::{
    copy_choices, copy_environment, copy_environment_with, is_empty_env, recover, source_root, COPY_FAILED,
    COPY_NEEDS_CONFIRM, COPY_NOTHING_TO_COPY, COPY_ONTO_ITSELF, COPY_SOURCE_MISSING,
};
use v2_lib::autorun::roots::{self, REMOVED_DIR};
use v2_lib::commands::environments::SWITCH_TEMPLATE_RUNNING;
use v2_lib::environments::{self, EnvFile, Environment};

const NOW: u64 = 5_000_000_000_000;

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

/// A shared root with environments A (active), B and C. Returns their ids.
fn three_envs(t: &Path) -> (String, String, String) {
    let a = environments::load_or_init(t, Some("dev-read")).unwrap().active;
    let id_of = |name: &str| {
        environments::save_env(t, env_named(name), &["dev-read".to_string()])
            .unwrap()
            .environments
            .into_iter()
            .find(|e| e.name == name)
            .unwrap()
            .id
    };
    let b = id_of("B");
    let c = id_of("C");
    (a, b, c)
}

fn put(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(root: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

fn envs(t: &Path) -> EnvFile {
    environments::load_or_init(t, None).unwrap()
}

fn no_staging_left(t: &Path) -> bool {
    std::fs::read_dir(t.join("envs"))
        .unwrap()
        .flatten()
        .all(|e| !e.file_name().to_string_lossy().starts_with(".copying-"))
}

/// Environment `a`'s describing data.
fn fill_source(t: &Path, a: &str) {
    let ra = roots::env_root(t, a);
    put(&ra, "scripts/case-1.json", "A-script");
    put(&ra, "projects/p.nav.json", "A-nav");
    put(&ra, "templates/t.json", "A-template");
}

#[test]
fn copy_into_an_empty_environment_brings_every_script_and_area() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let (ra, rb) = (roots::env_root(t.path(), &a), roots::env_root(t.path(), &b));
    assert!(is_empty_env(&rb));
    let done = copy_environment(t.path(), &ra, &b, "B", false, NOW).unwrap();
    assert_eq!((done.items, done.kept_aside), (3, false));
    assert_eq!(read(&rb, "scripts/case-1.json").as_deref(), Some("A-script"));
    assert_eq!(read(&rb, "projects/p.nav.json").as_deref(), Some("A-nav"));
    assert_eq!(read(&rb, "templates/t.json").as_deref(), Some("A-template"));
    assert_eq!(read(&ra, "scripts/case-1.json").as_deref(), Some("A-script"), "the source is untouched");
    assert!(roots::removed_folders(t.path(), NOW).is_empty(), "nothing was replaced");
    assert!(no_staging_left(t.path()), "the staging folder is gone");
}

#[test]
fn copy_never_brings_history_or_export_records() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let ra = roots::env_root(t.path(), &a);
    put(&ra, "runs/run-1.json", "A-run");
    put(&ra, "shots/s.png", "A-shot");
    put(&ra, "projects/p.pwexport.json", "A-export");
    put(&ra, "removed.json", "not an item");
    copy_environment(t.path(), &ra, &b, "B", false, NOW).unwrap();
    let rb = roots::env_root(t.path(), &b);
    for gone in ["runs/run-1.json", "shots/s.png", "projects/p.pwexport.json", "removed.json"] {
        assert!(read(&rb, gone).is_none(), "{gone} must not be copied");
    }
    assert_eq!(read(&rb, "projects/p.nav.json").as_deref(), Some("A-nav"));
}

#[test]
fn copy_onto_a_non_empty_environment_asks_first() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let rb = roots::env_root(t.path(), &b);
    put(&rb, "scripts/case-9.json", "B-script");
    let err = copy_environment(t.path(), &roots::env_root(t.path(), &a), &b, "B", false, NOW).unwrap_err();
    assert_eq!(err, COPY_NEEDS_CONFIRM);
    assert_eq!(read(&rb, "scripts/case-9.json").as_deref(), Some("B-script"));
    assert!(read(&rb, "scripts/case-1.json").is_none(), "nothing was copied");
    assert!(roots::removed_folders(t.path(), NOW).is_empty());
    assert!(no_staging_left(t.path()));
}

#[test]
fn replacing_keeps_the_old_contents_aside_and_the_targets_history() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let rb = roots::env_root(t.path(), &b);
    put(&rb, "scripts/case-9.json", "B-script");
    put(&rb, "runs/run-1.json", "B-run");
    put(&rb, "projects/p.pwexport.json", "B-export");
    let done = copy_environment(t.path(), &roots::env_root(t.path(), &a), &b, "B", true, NOW).unwrap();
    assert_eq!((done.items, done.kept_aside), (3, true));
    assert_eq!(read(&rb, "scripts/case-1.json").as_deref(), Some("A-script"));
    assert!(read(&rb, "scripts/case-9.json").is_none(), "the old script is replaced");
    assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("B-run"), "history stays");
    assert_eq!(read(&rb, "projects/p.pwexport.json").as_deref(), Some("B-export"), "the export record stays");
    let listed = roots::removed_folders(t.path(), NOW);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].env_id, b);
    assert_eq!(listed[0].reason, "replaced");
    assert_eq!(listed[0].name, "B");
    let kept = t.path().join(REMOVED_DIR).join(&listed[0].key);
    assert_eq!(read(&kept, "scripts/case-9.json").as_deref(), Some("B-script"));
    assert!(read(&kept, "runs/run-1.json").is_none(), "history is not duplicated into the backup");
    // And the backup is a source for a later copy.
    let choices = copy_choices(t.path(), &envs(t.path()), &b, NOW).unwrap();
    assert!(choices.sources.iter().any(|s| s.removed && s.key == format!("removed:{}", listed[0].key)));
}

#[test]
fn a_removed_folder_is_a_source() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (_a, b, c) = three_envs(t.path());
    put(&roots::env_root(t.path(), &c), "scripts/case-7.json", "C-script");
    let now = v2_lib::autorun::sessions::now_ms();
    environments::remove_env(t.path(), &c).unwrap();
    let file = envs(t.path());
    let choices = copy_choices(t.path(), &file, &b, now).unwrap();
    let removed: Vec<_> = choices.sources.iter().filter(|s| s.removed).collect();
    assert_eq!(removed.len(), 1);
    assert!(removed[0].key.starts_with(&format!("removed:{c}-")));
    assert_eq!(removed[0].name, "C");
    assert_eq!(removed[0].days_left, Some(30));
    let source = source_root(t.path(), &file, &removed[0].key, now).unwrap();
    let done = copy_environment(t.path(), &source, &b, "B", false, now + 1).unwrap();
    assert_eq!(done.items, 1);
    let rb = roots::env_root(t.path(), &b);
    assert_eq!(read(&rb, "scripts/case-7.json").as_deref(), Some("C-script"));
    assert!(read(&rb, "removed.json").is_none(), "the retire record is not copied");
}

#[test]
fn copy_choices_say_whether_the_target_is_empty_and_leave_the_target_out() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, c) = three_envs(t.path());
    let file = envs(t.path());
    let rb = roots::env_root(t.path(), &b);
    put(&rb, "runs/run-1.json", "history only");
    put(&rb, "projects/p.pwexport.json", "export only");
    let choices = copy_choices(t.path(), &file, &b, NOW).unwrap();
    assert!(choices.target_empty, "history and an export record do not count");
    let keys: Vec<_> = choices.sources.iter().map(|s| s.key.clone()).collect();
    assert!(keys.contains(&a) && keys.contains(&c) && !keys.contains(&b));
    put(&rb, "scripts/case-1.json", "x");
    assert!(!copy_choices(t.path(), &file, &b, NOW).unwrap().target_empty);
    assert!(copy_choices(t.path(), &file, "env-ffffffff", NOW).is_err());
}

#[test]
fn copying_an_environment_onto_itself_is_refused() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, _b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let ra = roots::env_root(t.path(), &a);
    assert_eq!(copy_environment(t.path(), &ra, &a, "A", true, NOW).unwrap_err(), COPY_ONTO_ITSELF);
    assert_eq!(read(&ra, "scripts/case-1.json").as_deref(), Some("A-script"));
}

#[test]
fn an_unknown_source_key_is_refused() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    three_envs(t.path());
    let file = envs(t.path());
    for key in ["removed:../x", "removed:env-aaaaaaaa-1", "env-ffffffff", "../x", "", "removed:", "_seed"] {
        let err = source_root(t.path(), &file, key, NOW).unwrap_err();
        assert!(!err.contains(['/', '\\']), "no path in {err:?}");
    }
}

#[test]
fn a_failed_copy_leaves_the_target_as_it_was() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let rb = roots::env_root(t.path(), &b);
    put(&rb, "scripts/case-9.json", "B-script");
    put(&rb, "runs/run-1.json", "B-run");
    // The replace cannot be set aside: its removed-area folder is taken.
    std::fs::create_dir_all(t.path().join(REMOVED_DIR).join(format!("{b}-{NOW}"))).unwrap();
    let err = copy_environment(t.path(), &roots::env_root(t.path(), &a), &b, "B", true, NOW).unwrap_err();
    assert!(err.contains("did not finish") && !err.contains(['/', '\\']), "{err}");
    assert_eq!(read(&rb, "scripts/case-9.json").as_deref(), Some("B-script"));
    assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("B-run"), "history is back in place");
    assert!(read(&rb, "scripts/case-1.json").is_none());
    assert!(no_staging_left(t.path()), "the staging folder is removed");
}

#[test]
fn a_target_something_is_writing_into_is_refused() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let rb = roots::env_root(t.path(), &b);
    let hold = roots::allow_writes_to(&rb);
    let err = copy_environment(t.path(), &roots::env_root(t.path(), &a), &b, "B", false, NOW).unwrap_err();
    assert_eq!(err, roots::REMOVE_BUSY);
    drop(hold);
    assert!(copy_environment(t.path(), &roots::env_root(t.path(), &a), &b, "B", false, NOW).is_ok());
}

#[test]
fn the_command_is_refused_while_a_template_runs() {
    let _lock = crate::serial::autorun();
    let _slot = crate::serial::api_template_run();
    let _held = crate::serial::held_browsers();
    let _leases = crate::serial::account_leases();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let claim = v2_lib::api_templates::runner::claim().unwrap();
    let err = tauri::async_runtime::block_on(v2_lib::commands::env_copy::copy_with(t.path(), &b, &a, false)).unwrap_err();
    assert_eq!(err, SWITCH_TEMPLATE_RUNNING);
    drop(claim);
    let (done, active) =
        tauri::async_runtime::block_on(v2_lib::commands::env_copy::copy_with(t.path(), &b, &a, false)).unwrap();
    assert_eq!((done.items, active), (3, false));
    let again = tauri::async_runtime::block_on(v2_lib::commands::env_copy::copy_with(t.path(), &b, &b, true));
    assert_eq!(again.unwrap_err(), COPY_ONTO_ITSELF);
}

// ---- fix round 1: nothing is deleted that could not be put back -------------

fn staging_of(t: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(t.join("envs"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".copying-"))
        .map(|e| e.path())
        .collect()
}

/// B holds a script, run history and an export record that is the ONLY file
/// in its `projects/` folder; the set-aside of B is forced to fail.
fn blocked_replace(t: &Path, a: &str, b: &str) {
    fill_source(t, a);
    let rb = roots::env_root(t, b);
    put(&rb, "scripts/case-9.json", "B-script");
    put(&rb, "runs/run-1.json", "B-run");
    put(&rb, "projects/p.pwexport.json", "B-export");
    std::fs::create_dir_all(t.join(REMOVED_DIR).join(format!("{b}-{NOW}"))).unwrap();
}

#[test]
fn an_export_record_alone_in_projects_survives_a_failed_set_aside() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    blocked_replace(t.path(), &a, &b);
    let rb = roots::env_root(t.path(), &b);
    let err = copy_environment(t.path(), &roots::env_root(t.path(), &a), &b, "B", true, NOW).unwrap_err();
    assert_eq!(err, COPY_FAILED);
    assert_eq!(read(&rb, "projects/p.pwexport.json").as_deref(), Some("B-export"));
    assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("B-run"));
    assert_eq!(read(&rb, "scripts/case-9.json").as_deref(), Some("B-script"));
    assert!(staging_of(t.path()).is_empty());
}

#[test]
fn staging_is_kept_when_an_item_cannot_be_put_back_and_the_next_start_puts_it_back() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    blocked_replace(t.path(), &a, &b);
    let rb = roots::env_root(t.path(), &b);
    // Every move out of staging is held (as an antivirus scan does).
    let mut held = |from: &Path, to: &Path| -> std::io::Result<()> {
        if from.to_string_lossy().contains(".copying-") && to.starts_with(&rb) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        std::fs::rename(from, to)
    };
    let err =
        copy_environment_with(t.path(), &roots::env_root(t.path(), &a), &b, "B", true, NOW, &mut held, &[]).unwrap_err();
    assert_eq!(err, COPY_FAILED);
    let staging = staging_of(t.path());
    assert_eq!(staging.len(), 1, "staging is kept");
    assert_eq!(read(&staging[0], "runs/run-1.json").as_deref(), Some("B-run"));
    assert_eq!(read(&staging[0], "projects/p.pwexport.json").as_deref(), Some("B-export"));
    // The next start gives the environment its history back and deletes nothing.
    assert_eq!(recover(t.path()), 1);
    assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("B-run"));
    assert_eq!(read(&rb, "projects/p.pwexport.json").as_deref(), Some("B-export"));
    assert_eq!(read(&rb, "scripts/case-9.json").as_deref(), Some("B-script"));
    assert_eq!(staging_of(t.path()).len(), 1, "staging is left aside, not deleted");
}

#[test]
fn a_failed_placement_with_nothing_set_aside_keeps_history_and_export_record() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let rb = roots::env_root(t.path(), &b);
    put(&rb, "runs/run-1.json", "B-run");
    put(&rb, "projects/p.pwexport.json", "B-export");
    let mut deny = |from: &Path, to: &Path| -> std::io::Result<()> {
        if from.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".copying-")) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        std::fs::rename(from, to)
    };
    let err =
        copy_environment_with(t.path(), &roots::env_root(t.path(), &a), &b, "B", false, NOW, &mut deny, &[]).unwrap_err();
    assert_eq!(err, COPY_FAILED);
    assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("B-run"));
    assert_eq!(read(&rb, "projects/p.pwexport.json").as_deref(), Some("B-export"));
    assert!(staging_of(t.path()).is_empty());
}

#[test]
fn a_copy_cut_short_between_set_aside_and_placement_is_finished_at_the_next_start() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let rb = roots::env_root(t.path(), &b);
    put(&rb, "scripts/case-9.json", "B-script");
    put(&rb, "runs/run-1.json", "B-run");
    put(&rb, "projects/p.pwexport.json", "B-export");
    // Nothing can be renamed into B any more: placement fails and so does the restore.
    let mut deny = |from: &Path, to: &Path| -> std::io::Result<()> {
        if to == rb {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        std::fs::rename(from, to)
    };
    let err =
        copy_environment_with(t.path(), &roots::env_root(t.path(), &a), &b, "B", true, NOW, &mut deny, &[]).unwrap_err();
    assert_eq!(err, COPY_FAILED);
    assert!(!rb.exists(), "the target is missing, as after a crash");
    assert_eq!(staging_of(t.path()).len(), 1);
    assert_eq!(recover(t.path()), 1);
    assert_eq!(read(&rb, "scripts/case-1.json").as_deref(), Some("A-script"), "the copy the person asked for");
    assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("B-run"));
    assert_eq!(read(&rb, "projects/p.pwexport.json").as_deref(), Some("B-export"));
    assert!(read(&rb, ".copying.json").is_none(), "the note is gone");
    assert!(staging_of(t.path()).is_empty());
    assert_eq!(roots::removed_folders(t.path(), NOW).len(), 1, "the replaced data is still kept");
}

/// A staging folder as a crash leaves it.
fn leftover(t: &Path, target: &str, note: Option<&str>) -> std::path::PathBuf {
    let staging = t.join("envs").join(format!(".copying-{target}-{NOW}"));
    put(&staging, "scripts/case-1.json", "NEW");
    put(&staging, "runs/run-1.json", "H-run");
    put(&staging, "projects/p.pwexport.json", "H-export");
    if let Some(step) = note {
        put(&staging, ".copying.json", &format!("{{\"target\":\"{target}\",\"source\":\"x\",\"step\":\"{step}\"}}"));
    }
    staging
}

#[test]
fn recover_finishes_a_copy_when_the_target_is_missing_or_holds_no_file() {
    let _lock = crate::serial::autorun();
    for empty_folders in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (_a, b, _c) = three_envs(t.path());
        leftover(t.path(), &b, Some("kept"));
        let rb = roots::env_root(t.path(), &b);
        if empty_folders {
            std::fs::create_dir_all(rb.join("scripts")).unwrap();
        }
        assert_eq!(recover(t.path()), 1);
        assert_eq!(read(&rb, "scripts/case-1.json").as_deref(), Some("NEW"));
        assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("H-run"));
        assert_eq!(read(&rb, "projects/p.pwexport.json").as_deref(), Some("H-export"));
        assert!(staging_of(t.path()).is_empty());
    }
}

#[test]
fn recover_never_finishes_a_copy_into_a_removed_environment() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (_a, b, _c) = three_envs(t.path());
    let staging = leftover(t.path(), &b, Some("kept"));
    // `b` leaves the list while its copy waits.
    let mut file = environments::load_or_init(t.path(), None).unwrap();
    file.environments.retain(|e| e.id != b);
    std::fs::write(t.path().join("environments.json"), serde_json::to_string(&file).unwrap()).unwrap();
    assert_eq!(recover(t.path()), 0);
    assert!(!roots::env_root(t.path(), &b).exists(), "the removed environment's folder was made again");
    assert!(staging.is_dir(), "nothing is deleted");
}

#[test]
fn recover_puts_history_back_only_where_the_target_has_none_and_deletes_nothing() {
    let _lock = crate::serial::autorun();
    for step in ["kept", "abandoned"] {
        let t = tempfile::tempdir().unwrap();
        let (_a, b, _c) = three_envs(t.path());
        let staging = leftover(t.path(), &b, Some(step));
        let rb = roots::env_root(t.path(), &b);
        put(&rb, "scripts/case-9.json", "OLD");
        put(&rb, "runs/run-2.json", "own-run");
        assert_eq!(recover(t.path()), 1);
        assert_eq!(read(&rb, "scripts/case-9.json").as_deref(), Some("OLD"), "the target is not touched");
        assert_eq!(read(&rb, "runs/run-2.json").as_deref(), Some("own-run"), "its own history wins");
        assert!(read(&rb, "runs/run-1.json").is_none());
        assert_eq!(read(&rb, "projects/p.pwexport.json").as_deref(), Some("H-export"));
        assert_eq!(read(&staging, "runs/run-1.json").as_deref(), Some("H-run"), "what stayed is kept");
        assert_eq!(read(&staging, "scripts/case-1.json").as_deref(), Some("NEW"));
    }
}

#[test]
fn abandoned_with_a_missing_target_gets_its_history_back() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (_a, b, _c) = three_envs(t.path());
    leftover(t.path(), &b, Some("abandoned"));
    assert_eq!(recover(t.path()), 1);
    let rb = roots::env_root(t.path(), &b);
    assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("H-run"));
    assert!(read(&rb, "scripts/case-1.json").is_none(), "an abandoned copy is not applied");
    assert_eq!(staging_of(t.path()).len(), 1);
}

#[test]
fn staging_folders_are_never_listed_as_environments_or_retired() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (_a, b, _c) = three_envs(t.path());
    let staging = leftover(t.path(), &b, Some("kept"));
    assert!(roots::env_roots(t.path()).iter().all(|r| !r.to_string_lossy().contains(".copying-")));
    assert_eq!(roots::retire_unlisted(t.path(), NOW), 0);
    assert!(staging.exists());
}

#[test]
fn a_missing_or_empty_source_is_refused_and_the_target_keeps_its_data() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, c) = three_envs(t.path());
    let rb = roots::env_root(t.path(), &b);
    put(&rb, "scripts/case-9.json", "B-script");
    // C never had a folder.
    assert_eq!(
        copy_environment(t.path(), &roots::env_root(t.path(), &c), &b, "B", true, NOW).unwrap_err(),
        COPY_SOURCE_MISSING
    );
    // A has a folder with history only.
    put(&roots::env_root(t.path(), &a), "runs/run-1.json", "A-run");
    assert_eq!(
        copy_environment(t.path(), &roots::env_root(t.path(), &a), &b, "B", true, NOW).unwrap_err(),
        COPY_NOTHING_TO_COPY
    );
    assert_eq!(read(&rb, "scripts/case-9.json").as_deref(), Some("B-script"));
    assert!(roots::removed_folders(t.path(), NOW).is_empty());
}

// ---- fix round 2 -------------------------------------------------------------

#[test]
fn a_copy_cut_short_in_step_two_has_no_note_and_the_next_start_restores_the_history() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (_a, b, _c) = three_envs(t.path());
    // As a kill during the moves leaves it: some history in staging, no note.
    let staging = leftover(t.path(), &b, None);
    let rb = roots::env_root(t.path(), &b);
    put(&rb, "scripts/case-9.json", "OLD");
    put(&rb, "runs/run-2.json", "own-run");
    assert_eq!(recover(t.path()), 1);
    assert_eq!(read(&rb, "projects/p.pwexport.json").as_deref(), Some("H-export"));
    assert_eq!(read(&rb, "runs/run-2.json").as_deref(), Some("own-run"), "never overwritten");
    assert_eq!(read(&rb, "scripts/case-9.json").as_deref(), Some("OLD"));
    assert!(staging.exists(), "the folder is left aside");
    assert!(read(&staging, "scripts/case-1.json").is_some());
    // And a target that is missing altogether gets its history too.
    let t2 = tempfile::tempdir().unwrap();
    let (_a2, b2, _c2) = three_envs(t2.path());
    leftover(t2.path(), &b2, None);
    assert_eq!(recover(t2.path()), 1);
    let rb2 = roots::env_root(t2.path(), &b2);
    assert_eq!(read(&rb2, "runs/run-1.json").as_deref(), Some("H-run"));
    assert!(read(&rb2, "scripts/case-1.json").is_none(), "an unnoted copy is never applied");
}

#[test]
fn a_note_that_cannot_be_written_stops_the_copy_and_loses_nothing() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b, _c) = three_envs(t.path());
    fill_source(t.path(), &a);
    let rb = roots::env_root(t.path(), &b);
    put(&rb, "scripts/case-9.json", "B-script");
    put(&rb, "runs/run-1.json", "B-run");
    put(&rb, "projects/p.pwexport.json", "B-export");
    let no_note = |_: &Path, _: &str, _: &Path, step: &str| -> Result<(), String> {
        if step == "kept" {
            Err("the disk is full".to_string())
        } else {
            Ok(())
        }
    };
    let err = v2_lib::autorun::env_copy::copy_environment_full(
        t.path(),
        &roots::env_root(t.path(), &a),
        &b,
        "B",
        true,
        NOW,
        &mut |x, y| std::fs::rename(x, y),
        &[],
        &no_note,
    )
    .unwrap_err();
    assert_eq!(err, COPY_FAILED);
    assert_eq!(read(&rb, "scripts/case-9.json").as_deref(), Some("B-script"), "not set aside");
    assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("B-run"));
    assert_eq!(read(&rb, "projects/p.pwexport.json").as_deref(), Some("B-export"));
    assert!(roots::removed_folders(t.path(), NOW).is_empty());
    assert!(staging_of(t.path()).is_empty());
}

#[test]
fn only_the_newest_copy_for_a_target_is_finished() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (_a, b, _c) = three_envs(t.path());
    let old = t.path().join("envs").join(format!(".copying-{b}-{}", NOW - 5));
    put(&old, "scripts/case-1.json", "OLD-COPY");
    put(&old, "runs/run-1.json", "old-run");
    put(&old, ".copying.json", &format!("{{\"target\":\"{b}\",\"source\":\"x\",\"step\":\"kept\"}}"));
    let new = leftover(t.path(), &b, Some("kept"));
    assert!(new.exists());
    recover(t.path());
    let rb = roots::env_root(t.path(), &b);
    assert_eq!(read(&rb, "scripts/case-1.json").as_deref(), Some("NEW"), "the newest copy wins");
    assert_eq!(read(&rb, "runs/run-1.json").as_deref(), Some("H-run"));
    assert!(old.exists(), "the older one is left aside");
}

#[test]
fn start_up_settles_a_cut_short_copy_before_the_seed_is_handed_over() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, _b, _c) = three_envs(t.path());
    // `a` is the first environment, the one a pending seed would fill.
    std::fs::create_dir_all(t.path().join("envs")).unwrap();
    put(&roots::env_root(t.path(), roots::SEED_ID), "scripts/case-1.json", "SEED");
    leftover(t.path(), &a, Some("kept"));
    let started = roots::start_up(t.path(), false);
    assert_eq!(started.settled_copies, 1);
    let ra = roots::env_root(t.path(), &a);
    assert_eq!(read(&ra, "scripts/case-1.json").as_deref(), Some("NEW"), "the copy, not the seed");
    assert_eq!(read(&ra, "runs/run-1.json").as_deref(), Some("H-run"));
}

// ---- copying one case ------------------------------------------------------

mod one_case {
    use super::*;
    use serde_json::{json, Value};
    use v2_lib::autorun::env_copy::{
        copy_case, plan_case_copy, CASE_COPY_NEEDS_CONFIRM, NO_SCRIPT_TO_COPY,
    };
    use v2_lib::autorun::nav::{self, ModulePath, NavFile};
    use v2_lib::autorun::{accounts, components, recipe, store, CaseScript};

    const ORG: &str = "org";
    const PROJECT: &str = "proj";

    fn script(case_id: i32, area: Option<&str>, steps: Value) -> CaseScript {
        serde_json::from_value(json!({
            "case_id": case_id, "title": "Apply for leave", "account": "admin",
            "area": area, "steps": steps,
            "suspected_defect": { "step_number": 1, "note": "n", "marked_at": "1" },
        }))
        .unwrap()
    }

    fn uses(name: &str) -> Value {
        json!([{ "step_number": 1, "actions": [{ "kind": "use_component", "component": name, "inputs": {} }] }])
    }

    fn comp(name: &str, desc: &str, actions: Value) -> components::Component {
        serde_json::from_value(json!({ "name": name, "description": desc, "inputs": [], "actions": actions })).unwrap()
    }

    fn area(name: &str, arrived: &str) -> ModulePath {
        serde_json::from_value(json!({
            "module": "Leave", "area": name,
            "clicks": [{ "role": "link", "name": name, "exact": true }],
            "arrived": arrived, "recorded": "2026-10-05T10:00:00Z"
        }))
        .unwrap()
    }

    fn recipe_json() -> recipe::SignInRecipe {
        serde_json::from_value(json!({
            "start_url": "https://hr.example.internal/",
            "steps": [{ "kind": "click", "selector": { "css": "#login" } }],
            "after_sign_in": [],
            "signed_in": { "css": "#home" }
        }))
        .unwrap()
    }

    /// Roots of the source (A) and the target (B), with writes into both let
    /// through (neither is the active store root in a test).
    struct Pair {
        t: tempfile::TempDir,
        a: std::path::PathBuf,
        b: std::path::PathBuf,
        b_id: String,
        _allow: (roots::AllowedTarget, roots::AllowedTarget),
    }

    fn pair() -> Pair {
        let t = tempfile::tempdir().unwrap();
        let (a_id, b_id, _c) = three_envs(t.path());
        let (a, b) = (roots::env_root(t.path(), &a_id), roots::env_root(t.path(), &b_id));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let allow = (roots::allow_writes_to(&a), roots::allow_writes_to(&b));
        Pair { t, a, b, b_id, _allow: allow }
    }

    fn nav_of(areas: Vec<ModulePath>) -> NavFile {
        NavFile { direct_urls: false, modules: areas, save_words: vec![], protected: vec![] }
    }

    #[test]
    fn copying_a_case_brings_its_components_area_and_recipe() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, Some("Leave"), uses("Login form"))).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Login form", "d", json!([{ "kind": "check_text", "value": "ok" }]))).unwrap();
        nav::save_nav(&p.a, ORG, PROJECT, &nav_of(vec![area("Leave", "/hr/leave")])).unwrap();
        recipe::save_recipe(&p.a, ORG, PROJECT, &recipe_json()).unwrap();

        let plan = plan_case_copy(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7).unwrap();
        assert_eq!(plan.components_added, vec!["Login form".to_string()]);
        assert_eq!(plan.areas_added, vec!["Leave".to_string()]);
        assert!(plan.recipe_added && !plan.target_has_script && !plan.uses_default_area);
        assert!(store::load_script(&p.b, 7).unwrap().is_none(), "the plan writes nothing");
        assert!(components::load_components(&p.b, ORG, PROJECT).unwrap().components.is_empty());

        let done = copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, false).unwrap();
        assert_eq!(done, plan);
        let copied = store::load_script(&p.b, 7).unwrap().expect("the script");
        assert!(copied.suspected_defect.is_none() && copied.cleared_defect.is_none());
        assert_eq!(copied.title, "Apply for leave");
        assert!(components::find(&components::load_components(&p.b, ORG, PROJECT).unwrap(), "Login form").is_some());
        assert!(nav::find_area(&nav::load_nav(&p.b, ORG, PROJECT).unwrap(), "Leave").is_some());
        assert!(recipe::load_recipe(&p.b, ORG, PROJECT).unwrap().is_some());
        assert!(store::load_script(&p.a, 7).unwrap().unwrap().suspected_defect.is_some(), "the source keeps its mark");
    }

    #[test]
    fn copying_a_case_never_overwrites_the_targets_component_or_area() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, Some("Leave"), uses("Login form"))).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Login form", "source", json!([{ "kind": "check_text", "value": "a" }]))).unwrap();
        components::put(&p.b, ORG, PROJECT, comp("Login form", "target", json!([{ "kind": "check_text", "value": "b" }]))).unwrap();
        nav::save_nav(&p.a, ORG, PROJECT, &nav_of(vec![area("Leave", "/hr/leave")])).unwrap();
        nav::save_nav(&p.b, ORG, PROJECT, &nav_of(vec![area("Leave", "/other/leave")])).unwrap();
        recipe::save_recipe(&p.a, ORG, PROJECT, &recipe_json()).unwrap();
        let mut other = recipe_json();
        other.start_url = "https://other.example.internal/".into();
        recipe::save_recipe(&p.b, ORG, PROJECT, &other).unwrap();
        let before = (
            std::fs::read(components::components_path(&p.b, ORG, PROJECT)).unwrap(),
            std::fs::read(nav::nav_path(&p.b, ORG, PROJECT)).unwrap(),
        );

        let plan = copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, false).unwrap();
        assert_eq!(plan.components_kept, vec!["Login form".to_string()]);
        assert!(plan.components_added.is_empty() && plan.areas_added.is_empty() && !plan.recipe_added);
        assert_eq!(plan.areas_kept, vec!["Leave".to_string()]);
        assert_eq!(std::fs::read(components::components_path(&p.b, ORG, PROJECT)).unwrap(), before.0);
        assert_eq!(std::fs::read(nav::nav_path(&p.b, ORG, PROJECT)).unwrap(), before.1);
        assert_eq!(recipe::load_recipe(&p.b, ORG, PROJECT).unwrap().unwrap().start_url, "https://other.example.internal/");
        assert!(store::load_script(&p.b, 7).unwrap().is_some());
    }

    #[test]
    fn nested_components_are_brought_too() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, None, uses("Outer"))).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Outer", "d", json!([
            { "kind": "when_visible", "selector": { "css": "#x" }, "then": [{ "kind": "use_component", "component": "Inner", "inputs": {} }] }
        ]))).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Inner", "d", json!([{ "kind": "check_text", "value": "ok" }]))).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Unrelated", "d", json!([{ "kind": "check_text", "value": "ok" }]))).unwrap();
        let plan = copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, false).unwrap();
        let mut added = plan.components_added.clone();
        added.sort();
        assert_eq!(added, vec!["Inner".to_string(), "Outer".to_string()]);
        assert!(plan.uses_default_area);
        let there = components::load_components(&p.b, ORG, PROJECT).unwrap();
        assert!(components::find(&there, "Inner").is_some() && components::find(&there, "Unrelated").is_none());
    }

    #[test]
    fn a_case_already_scripted_in_the_target_asks_first() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, Some("Leave"), uses("Login form"))).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Login form", "d", json!([{ "kind": "check_text", "value": "ok" }]))).unwrap();
        let mut old = script(7, None, uses("Old"));
        old.title = "Targets own".into();
        store::save_script(&p.b, &old).unwrap();

        assert!(plan_case_copy(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7).unwrap().target_has_script);
        let err = copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, false).unwrap_err();
        assert_eq!(err, CASE_COPY_NEEDS_CONFIRM);
        assert_eq!(store::load_script(&p.b, 7).unwrap().unwrap().title, "Targets own");
        assert!(components::load_components(&p.b, ORG, PROJECT).unwrap().components.is_empty(), "nothing written");

        copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, true).unwrap();
        assert_eq!(store::load_script(&p.b, 7).unwrap().unwrap().title, "Apply for leave");
        let kept: Vec<_> = std::fs::read_dir(p.b.join("deleted-scripts")).unwrap().flatten().collect();
        assert_eq!(kept.len(), 1, "the replaced script is kept aside");
    }

    #[test]
    fn a_missing_account_and_missing_api_data_are_named_not_blocking() {
        let _lock = crate::serial::autorun();
        let p = pair();
        let mut s = script(7, None, uses("X"));
        s.setup = serde_json::from_value(json!({ "fixture": "fx-1" })).unwrap();
        s.preconditions = serde_json::from_value(json!([
            { "flow": "flow-2", "stage": "s", "value": null },
            { "flow": "flow-2", "stage": "t", "value": null }
        ]))
        .unwrap();
        store::save_script(&p.a, &s).unwrap();
        let plan = copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, false).unwrap();
        assert_eq!(plan.missing_accounts, vec!["admin".to_string()]);
        assert_eq!(plan.missing_api, vec!["fx-1".to_string(), "flow-2".to_string()]);
        assert!(store::load_script(&p.b, 7).unwrap().is_some());

        // With the account there, it is not named.
        let acct: accounts::Account = serde_json::from_value(
            json!({ "key": "admin", "label": "Admin", "username": "u", "password": "p" }),
        )
        .unwrap();
        accounts::save_accounts_for(&p.b, &p.b_id, &[acct]).unwrap();
        let plan = plan_case_copy(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7).unwrap();
        assert!(plan.missing_accounts.is_empty());
    }

    #[test]
    fn a_case_with_no_script_is_refused() {
        let _lock = crate::serial::autorun();
        let p = pair();
        assert_eq!(plan_case_copy(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7).unwrap_err(), NO_SCRIPT_TO_COPY);
        assert_eq!(copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, true).unwrap_err(), NO_SCRIPT_TO_COPY);
    }

    #[test]
    fn a_failure_before_the_script_leaves_no_script_in_the_target() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, Some("Leave"), uses("Login form"))).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Login form", "d", json!([{ "kind": "check_text", "value": "ok" }]))).unwrap();
        // The target's components file is a folder: reading it fails, so
        // nothing at all is written.
        std::fs::create_dir_all(components::components_path(&p.b, ORG, PROJECT)).unwrap();
        let err = copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, false).unwrap_err();
        assert!(!err.contains(p.b.to_string_lossy().as_ref()), "{err}");
        assert!(store::load_script(&p.b, 7).unwrap().is_none());
    }

    #[test]
    fn a_target_that_is_not_an_environment_folder_is_refused() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, None, uses("X"))).unwrap();
        let elsewhere = p.t.path().join("not-an-env");
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert!(copy_case(&p.a, &elsewhere, &p.b_id, ORG, PROJECT, 7, true).is_err());
        assert!(copy_case(&p.a, &p.a, &p.b_id, ORG, PROJECT, 7, true).is_err());
    }

    // ---- fix round 1 -------------------------------------------------------

    #[test]
    fn the_flat_layout_is_refused_and_never_gets_an_envs_folder() {
        let _lock = crate::serial::autorun();
        let t = tempfile::tempdir().unwrap();
        let (_a, b, _c) = three_envs(t.path());
        // The flat layout: the root is the shared folder itself, and a
        // script sits in it. Environment folders do not exist yet.
        let flat = tempfile::tempdir().unwrap();
        put(flat.path(), "scripts/case-7.json", &serde_json::to_string(&script(7, None, uses("X"))).unwrap());
        let target = roots::env_root(flat.path(), &b);
        let err = plan_case_copy(flat.path(), &target, &b, ORG, PROJECT, 7).unwrap_err();
        assert_eq!(err, v2_lib::autorun::env_copy::CASE_COPY_NEEDS_LAYOUT);
        assert!(copy_case(flat.path(), &target, &b, ORG, PROJECT, 7, true).is_err());
        assert!(!flat.path().join("envs").exists(), "nothing started the new layout");
        // And the other way: the flat folder as the target.
        let p = pair();
        store::save_script(&p.a, &script(7, None, uses("X"))).unwrap();
        assert!(copy_case(&p.a, flat.path(), &b, ORG, PROJECT, 7, true).is_err());
    }

    #[test]
    fn the_seed_and_a_removed_folder_are_not_environments_to_copy_between() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, None, uses("X"))).unwrap();
        let seed = roots::env_root(p.t.path(), roots::SEED_ID);
        std::fs::create_dir_all(&seed).unwrap();
        assert!(plan_case_copy(&p.a, &seed, roots::SEED_ID, ORG, PROJECT, 7).is_err());
        let removed = p.t.path().join(REMOVED_DIR).join("gone-123");
        std::fs::create_dir_all(&removed).unwrap();
        assert!(plan_case_copy(&p.a, &removed, "gone", ORG, PROJECT, 7).is_err());
        assert!(plan_case_copy(&removed, &p.b, &p.b_id, ORG, PROJECT, 7).is_err());
    }

    #[test]
    fn a_case_is_pulled_into_the_active_environment_under_the_normal_guard() {
        let _lock = crate::serial::autorun();
        let t = tempfile::tempdir().unwrap();
        let (a_id, b_id, _c) = three_envs(t.path());
        let (a, b) = (roots::env_root(t.path(), &a_id), roots::env_root(t.path(), &b_id));
        std::fs::create_dir_all(&b).unwrap();
        v2_lib::autorun::store::set_shared_root(t.path().to_path_buf());
        let active = v2_lib::autorun::store::select_environment(&a_id).unwrap();
        assert_eq!(active, a);
        {
            let _allow = roots::allow_writes_to(&b);
            store::save_script(&b, &script(7, Some("Leave"), uses("Login form"))).unwrap();
            components::put(&b, ORG, PROJECT, comp("Login form", "d", json!([{ "kind": "check_text", "value": "ok" }]))).unwrap();
            nav::save_nav(&b, ORG, PROJECT, &nav_of(vec![area("Leave", "/hr/leave")])).unwrap();
        }
        // Pull: the source is B, the target is the active A; no allowance is held.
        let plan = plan_case_copy(&b, &a, &a_id, ORG, PROJECT, 7).unwrap();
        assert_eq!(plan.components_added, vec!["Login form".to_string()]);
        assert!(store::load_script(&a, 7).unwrap().is_none());
        copy_case(&b, &a, &a_id, ORG, PROJECT, 7, false).unwrap();
        assert!(store::load_script(&a, 7).unwrap().is_some());
        assert!(nav::find_area(&nav::load_nav(&a, ORG, PROJECT).unwrap(), "Leave").is_some());
        // The script the active environment now has asks before it is replaced.
        assert_eq!(
            copy_case(&b, &a, &a_id, ORG, PROJECT, 7, false).unwrap_err(),
            CASE_COPY_NEEDS_CONFIRM
        );
        // A replay of the case refuses the replace before anything is written.
        let _replay = store::replaying(7);
        let before = std::fs::read(a.join("scripts/case-7.json")).unwrap();
        let err = copy_case(&b, &a, &a_id, ORG, PROJECT, 7, true).unwrap_err();
        assert!(err.contains("case 7") && err.contains("replay"), "{err}");
        assert_eq!(std::fs::read(a.join("scripts/case-7.json")).unwrap(), before);
        assert!(!a.join("deleted-scripts").exists());
    }

    #[test]
    fn a_replayed_case_refuses_a_push_replace_before_any_write() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, Some("Leave"), uses("Login form"))).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Login form", "d", json!([{ "kind": "check_text", "value": "ok" }]))).unwrap();
        store::save_script(&p.b, &script(7, None, uses("Old"))).unwrap();
        let _replay = store::replaying(7);
        let err = copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, true).unwrap_err();
        assert!(err.contains("case 7") && err.contains("replay"), "{err}");
        assert!(components::load_components(&p.b, ORG, PROJECT).unwrap().components.is_empty(), "nothing written first");
    }

    #[test]
    fn every_area_and_account_a_script_and_its_components_visit_is_brought_or_named() {
        let _lock = crate::serial::autorun();
        let p = pair();
        let steps = json!([
            { "step_number": 1, "actions": [
                { "kind": "return_to_area", "area": "Payroll" },
                { "kind": "sign_in", "account": "approver" },
                { "kind": "use_component", "component": "Outer", "inputs": {} }
            ] }
        ]);
        store::save_script(&p.a, &script(7, Some("Leave"), steps)).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Outer", "d", json!([
            { "kind": "return_to_area", "area": "Reports" },
            { "kind": "sign_in", "account": "auditor" }
        ]))).unwrap();
        nav::save_nav(&p.a, ORG, PROJECT, &nav_of(vec![
            area("Leave", "/hr/leave"), area("Payroll", "/hr/pay"), area("Reports", "/hr/rep"), area("Other", "/hr/o"),
        ])).unwrap();
        // The target already has Leave, but a different one.
        nav::save_nav(&p.b, ORG, PROJECT, &nav_of(vec![area("Leave", "/else")])).unwrap();
        let acct = |k: &str| -> accounts::Account {
            serde_json::from_value(json!({ "key": k, "label": k, "username": "u", "password": "p" })).unwrap()
        };
        accounts::save_accounts_for(&p.b, &p.b_id, &[acct("approver")]).unwrap();

        let plan = plan_case_copy(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7).unwrap();
        let mut added = plan.areas_added.clone();
        added.sort();
        assert_eq!(added, vec!["Payroll".to_string(), "Reports".to_string()]);
        assert_eq!(plan.areas_kept, vec!["Leave".to_string()]);
        let mut missing = plan.missing_accounts.clone();
        missing.sort();
        assert_eq!(missing, vec!["admin".to_string(), "auditor".to_string()], "approver exists there");
        copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7, false).unwrap();
        let there = nav::load_nav(&p.b, ORG, PROJECT).unwrap();
        assert!(nav::find_area(&there, "Payroll").is_some() && nav::find_area(&there, "Reports").is_some());
        assert!(nav::find_area(&there, "Other").is_none(), "only what the case visits");
    }

    #[test]
    fn a_copied_recipe_says_which_site_address_it_brings() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, None, uses("X"))).unwrap();
        recipe::save_recipe(&p.a, ORG, PROJECT, &recipe_json()).unwrap();
        let plan = plan_case_copy(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7).unwrap();
        assert!(plan.recipe_added);
        assert_eq!(plan.recipe_address.as_deref(), Some("https://hr.example.internal/"));
    }

    #[test]
    fn a_component_or_area_that_appears_before_the_write_is_not_overwritten() {
        let _lock = crate::serial::autorun();
        let p = pair();
        components::put(&p.b, ORG, PROJECT, comp("Login form", "target", json!([{ "kind": "check_text", "value": "b" }]))).unwrap();
        let saved = components::put_if_absent(
            &p.b, ORG, PROJECT, comp("login form", "source", json!([{ "kind": "check_text", "value": "a" }])),
        )
        .unwrap();
        assert!(!saved);
        let file = components::load_components(&p.b, ORG, PROJECT).unwrap();
        assert_eq!(components::find(&file, "Login form").unwrap().description, "target");
    }

    #[test]
    fn the_case_commands_refuse_flat_a_stale_screen_and_the_wrong_direction() {
        use v2_lib::commands::env_copy::{case_copy_with, case_plan_with, CaseWay};
        let _lock = crate::serial::autorun();
        let t = tempfile::tempdir().unwrap();
        let (a, b, _c) = three_envs(t.path());
        // Flat layout: refused, and no envs folder is made beside it.
        let flat = tempfile::tempdir().unwrap();
        environments::load_or_init(flat.path(), Some("dev-read")).unwrap();
        let flat_envs = environments::load_or_init(flat.path(), None).unwrap();
        let some_id = flat_envs.environments[0].id.clone();
        let _ = std::fs::remove_dir_all(flat.path().join("envs"));
        v2_lib::autorun::store::set_root(flat.path().to_path_buf());
        let err = tauri::async_runtime::block_on(case_copy_with(
            flat.path(), ORG, PROJECT, 7, &some_id, &b, CaseWay::Push, true,
        ))
        .unwrap_err();
        assert_eq!(err, v2_lib::autorun::env_copy::CASE_COPY_NEEDS_LAYOUT);
        assert!(!flat.path().join("envs").exists());

        v2_lib::autorun::store::set_shared_root(t.path().to_path_buf());
        v2_lib::autorun::store::select_environment(&a).unwrap();
        let plan = |active: &str, other: &str, way| {
            tauri::async_runtime::block_on(case_plan_with(t.path(), ORG, PROJECT, 7, active, other, way))
        };
        assert_eq!(plan(&a, &a, CaseWay::Push).unwrap_err(), v2_lib::autorun::env_copy::COPY_TO_ACTIVE);
        assert_eq!(plan(&a, &a, CaseWay::Pull).unwrap_err(), v2_lib::autorun::env_copy::COPY_FROM_ACTIVE);
        assert_eq!(plan(&b, &a, CaseWay::Push).unwrap_err(), v2_lib::autorun::env_copy::CASE_COPY_SWITCHED);
        assert_eq!(plan(&a, &b, CaseWay::Pull).unwrap_err(), NO_SCRIPT_TO_COPY);
    }

    #[test]
    fn a_component_neither_environment_has_is_named_in_both_directions() {
        let _lock = crate::serial::autorun();
        let p = pair();
        store::save_script(&p.a, &script(7, None, uses("Ghost"))).unwrap();
        components::put(&p.a, ORG, PROJECT, comp("Real", "d", json!([{ "kind": "use_component", "component": "Deeper ghost", "inputs": {} }]))).unwrap();
        let mut s = script(8, None, uses("Real"));
        s.case_id = 8;
        store::save_script(&p.a, &s).unwrap();
        let plan = plan_case_copy(&p.a, &p.b, &p.b_id, ORG, PROJECT, 7).unwrap();
        assert_eq!(plan.missing_components, vec!["Ghost".to_string()]);
        let plan = copy_case(&p.a, &p.b, &p.b_id, ORG, PROJECT, 8, false).unwrap();
        assert_eq!(plan.components_added, vec!["Real".to_string()]);
        assert_eq!(plan.missing_components, vec!["Deeper ghost".to_string()], "still copied");
        assert!(store::load_script(&p.b, 8).unwrap().is_some());
        // The other way: a script in B whose component is in neither.
        let a_id = roots::env_id_of(&p.a).unwrap();
        store::save_script(&p.b, &script(9, None, uses("Ghost two"))).unwrap();
        let back = plan_case_copy(&p.b, &p.a, &a_id, ORG, PROJECT, 9).unwrap();
        assert_eq!(back.missing_components, vec!["Ghost two".to_string()]);
    }
}
