//! Copying a whole environment's Auto Run data: what comes across, what
//! never does, what a replace keeps aside, and the sources offered.

use std::path::Path;
use v2_lib::autorun::env_copy::{
    copy_choices, copy_environment, is_empty_env, source_root, COPY_NEEDS_CONFIRM, COPY_ONTO_ITSELF,
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
    let again = tauri::async_runtime::block_on(v2_lib::commands::env_copy::copy_with(t.path(), &b, &b, true));
    assert_eq!(again.unwrap_err(), COPY_ONTO_ITSELF);
}
