//! The one-time copy of the flat Auto Run folder into a folder per
//! environment, and the seed a first environment made afterwards takes.

use std::path::{Path, PathBuf};
use std::cell::RefCell;
use v2_lib::autorun::roots::{self, MigrationOps, MigrationReport};
use v2_lib::autorun::store;
use v2_lib::autorun::LocalRun;
use v2_lib::environments::{self, Environment};

const A: &str = "env-0000000a";
const B: &str = "env-0000000b";

fn env_named(id: &str, name: &str) -> Environment {
    Environment {
        id: id.into(),
        name: name.into(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: "dev-read".into(),
        test_environment: false,
        test_prefix: "AUTOTEST".into(),
        db_auto_approve: false,
    }
}

fn put(shared: &Path, rel: &str, body: &str) {
    let p = shared.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// A shared root whose `environments.json` lists A (active) and B.
fn two_envs(shared: &Path) {
    let file = environments::EnvFile {
        active: A.into(),
        environments: vec![env_named(A, "One"), env_named(B, "Two")],
        auto_approve_adopted: false,
        playwright_export: None,
    };
    put(shared, "environments.json", &serde_json::to_string(&file).unwrap());
}

fn at(shared: &Path, id: &str, rel: &str) -> PathBuf {
    roots::env_root(shared, id).join(rel)
}

#[test]
fn migration_copies_every_item_into_each_environment() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    put(t.path(), "projects/p.nav.json", "N");
    put(t.path(), "templates/x/t.json", "T");
    let r = roots::migrate_once(t.path()).unwrap().unwrap();
    assert_eq!(r.environments, 2);
    for id in [A, B] {
        assert_eq!(std::fs::read_to_string(at(t.path(), id, "scripts/case-1.json")).unwrap(), "S");
        assert_eq!(std::fs::read_to_string(at(t.path(), id, "projects/p.nav.json")).unwrap(), "N");
        assert_eq!(std::fs::read_to_string(at(t.path(), id, "templates/x/t.json")).unwrap(), "T");
    }
    assert_eq!(r.items, 3);
}

#[test]
fn migration_puts_history_only_in_the_active_environment() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "runs/run-1.json", "R");
    put(t.path(), "shots/a/b.png", "P");
    roots::migrate_once(t.path()).unwrap().unwrap();
    assert_eq!(std::fs::read_to_string(at(t.path(), A, "runs/run-1.json")).unwrap(), "R");
    assert_eq!(std::fs::read_to_string(at(t.path(), A, "shots/a/b.png")).unwrap(), "P");
    assert!(!at(t.path(), B, "runs").exists());
    assert!(!at(t.path(), B, "shots").exists());
}

#[test]
fn migration_puts_export_records_only_in_the_active_environment() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "projects/p.pwexport.json", "X");
    put(t.path(), "projects/p.json", "recipe");
    roots::migrate_once(t.path()).unwrap().unwrap();
    assert!(at(t.path(), A, "projects/p.pwexport.json").is_file());
    assert!(!at(t.path(), B, "projects/p.pwexport.json").exists());
    assert!(at(t.path(), B, "projects/p.json").is_file());
}

#[test]
fn migration_without_environments_keeps_a_seed() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "scripts/case-1.json", "S");
    put(t.path(), "runs/run-1.json", "R");
    roots::migrate_once(t.path()).unwrap().unwrap();
    assert!(at(t.path(), "_seed", "scripts/case-1.json").is_file());
    assert!(at(t.path(), "_seed", "runs/run-1.json").is_file());
    let names: Vec<_> = std::fs::read_dir(t.path().join("envs")).unwrap().flatten().collect();
    assert_eq!(names.len(), 1);
}

#[test]
fn migration_leaves_the_originals_untouched() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    put(t.path(), "shots/a.png", "P");
    roots::migrate_once(t.path()).unwrap().unwrap();
    assert_eq!(std::fs::read_to_string(t.path().join("scripts/case-1.json")).unwrap(), "S");
    assert_eq!(std::fs::read_to_string(t.path().join("shots/a.png")).unwrap(), "P");
    // History is linked, not a second copy: editing one shows in the other
    // only where the file system links; either way the original is intact.
    assert!(t.path().join("environments.json").is_file());
}

#[test]
fn migration_runs_once() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    assert!(roots::migrate_once(t.path()).unwrap().is_some());
    put(t.path(), "scripts/case-2.json", "S2");
    assert!(roots::migrate_once(t.path()).unwrap().is_none());
    assert!(!at(t.path(), A, "scripts/case-2.json").exists());
}

#[test]
fn a_failed_migration_leaves_no_envs_folder_and_retries() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "scripts/case-1.json", "S");
    put(t.path(), ".envs-staging-1/junk.txt", "j");
    put(t.path(), "environments.json", "{ not json");
    assert!(roots::migrate_once(t.path()).is_err());
    assert!(!t.path().join("envs").exists());
    assert_eq!(std::fs::read_to_string(t.path().join("environments.json")).unwrap(), "{ not json");
    two_envs(t.path());
    assert!(roots::migrate_once(t.path()).unwrap().is_some());
    assert!(t.path().join("envs").is_dir());
    assert!(!t.path().join(".envs-staging-1").exists());
    assert!(at(t.path(), B, "scripts/case-1.json").is_file());
}

#[test]
fn fresh_install_gets_an_empty_envs_folder() {
    let t = tempfile::tempdir().unwrap();
    let r = roots::migrate_once(t.path()).unwrap().unwrap();
    assert_eq!(r, MigrationReport { environments: 0, items: 0 });
    assert!(t.path().join("envs").is_dir());
    assert_eq!(std::fs::read_dir(t.path().join("envs")).unwrap().count(), 0);
}

#[test]
fn first_environment_made_after_the_update_takes_the_seed() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "scripts/case-1.json", "S");
    roots::migrate_once(t.path()).unwrap().unwrap();
    let file = environments::load_or_init(t.path(), None).unwrap();
    assert_eq!(std::fs::read_to_string(at(t.path(), &file.active, "scripts/case-1.json")).unwrap(), "S");
    assert!(!at(t.path(), "_seed", "").exists());
}

#[test]
fn a_second_new_environment_starts_empty() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "scripts/case-1.json", "S");
    roots::migrate_once(t.path()).unwrap().unwrap();
    environments::load_or_init(t.path(), None).unwrap();
    let file = environments::save_env(t.path(), env_named("", "Second"), &["dev-read".to_string()]).unwrap();
    let new = file.environments.iter().find(|e| e.name == "Second").unwrap();
    assert!(!at(t.path(), &new.id, "scripts").exists());
}

#[test]
fn migration_log_line_names_no_path() {
    let line = roots::migration_log_line(&MigrationReport { environments: 2, items: 5 });
    assert!(line.contains("2 environment(s)") && line.contains("5 item(s)"), "{line}");
    assert!(!line.contains(':') || line.starts_with("Auto Run:"), "{line}");
    assert!(!line.contains('\\') &&!line.contains('/'), "{line}");
}

fn no_sleep(_: std::time::Duration) {}

#[test]
fn seed_into_refuses_a_target_holding_data_and_keeps_the_seed() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    put(t.path(), &format!("envs/{A}/scripts/b.json"), "B");
    assert!(roots::seed_into(t.path(), A).is_err());
    assert!(at(t.path(), "_seed", "scripts/a.json").is_file());
    assert!(at(t.path(), A, "scripts/b.json").is_file());
}

#[test]
fn seed_into_replaces_an_empty_target_folder() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    std::fs::create_dir_all(at(t.path(), A, "")).unwrap();
    assert!(roots::seed_into(t.path(), A).unwrap());
    assert!(at(t.path(), A, "scripts/a.json").is_file());
    assert!(!at(t.path(), "_seed", "").exists());
}

#[test]
fn a_busy_seed_rename_is_retried_then_succeeds() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    let tries = RefCell::new(0);
    let r = roots::seed_into_with(
        t.path(),
        A,
        &mut |a, b| {
            *tries.borrow_mut() += 1;
            if *tries.borrow() < 3 {
                Err(std::io::Error::from_raw_os_error(32))
            } else {
                std::fs::rename(a, b)
            }
        },
        &mut no_sleep,
    );
    assert!(r.unwrap());
    assert_eq!(*tries.borrow(), 3);
    assert!(at(t.path(), A, "scripts/a.json").is_file());
}

#[test]
fn a_failed_seed_rename_loses_nothing_and_never_fails_the_list() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    // Another process holding a file inside the seed makes the folder
    // rename fail on Windows; elsewhere a rename never fails this way.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(at(t.path(), "_seed", "scripts/a.json"))
            .unwrap();
        let file = environments::load_or_init(t.path(), None).expect("the list failed because of the seed");
        assert_eq!(file.environments.len(), 1);
        assert!(at(t.path(), "_seed", "scripts/a.json").is_file());
        assert!(!at(t.path(), &file.active, "scripts/a.json").exists());
    }
}

#[test]
fn a_new_environment_that_fails_to_save_leaves_the_seed() {
    let t = tempfile::tempdir().unwrap();
    environments::load_or_init(t.path(), None).unwrap();
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    // Default is called "Default": the same name again is refused.
    let r = environments::save_env(t.path(), env_named("", "Default"), &["dev-read".to_string()]);
    assert!(r.is_err());
    assert!(at(t.path(), "_seed", "scripts/a.json").is_file());
}

fn assert_flat_and_no_envs(t: &Path) {
    assert!(!t.join("envs").exists(), "a half-built envs/ was left");
    assert!(std::fs::read_dir(t)
        .unwrap()
        .flatten()
        .all(|e| !e.file_name().to_string_lossy().starts_with(".envs-staging-")));
}

#[test]
fn a_failure_during_the_copy_leaves_no_envs_and_no_staging() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    // A symbolic link-free way to fail mid-copy: the copy seam errors after
    // it has written part of the staging folder.
    let r = roots::migrate_with(
        t.path(),
        MigrationOps {
            fan_out: &|flat, staging, ids, active| {
                roots::fan_out(flat, staging, ids, active)?;
                Err("disk full".to_string())
            },
            rename: &mut |a, b| std::fs::rename(a, b),
            sleep: &mut no_sleep,
        },
    );
    assert!(r.is_err());
    assert_flat_and_no_envs(t.path());
    assert!(t.path().join("scripts/case-1.json").is_file());
}

/// The staging folders (not their marks) left under `t`.
fn stagings(t: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(t)
        .unwrap()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with(".envs-staging-") && !n.ends_with(".complete")
        })
        .map(|e| e.path())
        .collect()
}

#[test]
fn a_failed_final_rename_keeps_the_finished_copy() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    let r = roots::migrate_with(
        t.path(),
        MigrationOps {
            fan_out: &roots::fan_out,
            rename: &mut |_, _| Err(std::io::Error::from_raw_os_error(5)),
            sleep: &mut no_sleep,
        },
    );
    assert!(r.is_err());
    assert!(!t.path().join("envs").exists(), "a half-built envs/ was left");
    let kept = stagings(t.path());
    assert_eq!(kept.len(), 1, "the finished copy was not kept");
    assert!(kept[0].join(B).join("scripts/case-1.json").is_file());
    assert!(t.path().join("scripts/case-1.json").is_file());
}

#[test]
fn the_next_start_only_renames_the_kept_copy() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    let first = roots::migrate_with(
        t.path(),
        MigrationOps {
            fan_out: &roots::fan_out,
            rename: &mut |_, _| Err(std::io::Error::from_raw_os_error(5)),
            sleep: &mut no_sleep,
        },
    );
    assert!(first.is_err());
    let copies = RefCell::new(0);
    let r = roots::migrate_with(
        t.path(),
        MigrationOps {
            fan_out: &|f, s, i, a| {
                *copies.borrow_mut() += 1;
                roots::fan_out(f, s, i, a)
            },
            rename: &mut |a, b| std::fs::rename(a, b),
            sleep: &mut no_sleep,
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(*copies.borrow(), 0, "the copy was built again");
    assert_eq!(r.environments, 2);
    assert!(r.items >= 1);
    assert_eq!(std::fs::read_to_string(at(t.path(), B, "scripts/case-1.json")).unwrap(), "S");
    assert!(stagings(t.path()).is_empty());
    assert!(std::fs::read_dir(t.path())
        .unwrap()
        .flatten()
        .all(|e| !e.file_name().to_string_lossy().ends_with(".complete")));
}

fn fail_rename_once(t: &Path) {
    let first = roots::migrate_with(
        t,
        MigrationOps {
            fan_out: &roots::fan_out,
            rename: &mut |_, _| Err(std::io::Error::from_raw_os_error(5)),
            sleep: &mut no_sleep,
        },
    );
    assert!(first.is_err());
}

fn counted_retry(t: &Path) -> i32 {
    let copies = RefCell::new(0);
    roots::migrate_with(
        t,
        MigrationOps {
            fan_out: &|f, s, i, a| {
                *copies.borrow_mut() += 1;
                roots::fan_out(f, s, i, a)
            },
            rename: &mut |a, b| std::fs::rename(a, b),
            sleep: &mut no_sleep,
        },
    )
    .unwrap()
    .unwrap();
    let n = *copies.borrow();
    n
}

#[test]
fn a_kept_copy_is_rebuilt_when_the_flat_folder_was_used_meanwhile() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    fail_rename_once(t.path());
    std::thread::sleep(std::time::Duration::from_millis(50));
    put(t.path(), "scripts/case-1.json", "EDITED");
    assert!(counted_retry(t.path()) > 0, "the stale copy was reused");
    assert_eq!(std::fs::read_to_string(at(t.path(), B, "scripts/case-1.json")).unwrap(), "EDITED");
    assert!(stagings(t.path()).is_empty());
}

#[test]
fn a_kept_copy_nothing_changed_since_is_only_renamed() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    std::thread::sleep(std::time::Duration::from_millis(50));
    fail_rename_once(t.path());
    assert_eq!(counted_retry(t.path()), 0);
    assert_eq!(std::fs::read_to_string(at(t.path(), A, "scripts/case-1.json")).unwrap(), "S");
}

#[test]
fn a_staging_without_its_mark_is_rebuilt() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    put(t.path(), ".envs-staging-5/env-0000000a/scripts/partial.json", "p");
    let copies = RefCell::new(0);
    let r = roots::migrate_with(
        t.path(),
        MigrationOps {
            fan_out: &|f, s, i, a| {
                *copies.borrow_mut() += 1;
                roots::fan_out(f, s, i, a)
            },
            rename: &mut |a, b| std::fs::rename(a, b),
            sleep: &mut no_sleep,
        },
    );
    assert!(r.unwrap().is_some());
    assert_eq!(*copies.borrow(), 1);
    assert!(!at(t.path(), A, "scripts/partial.json").exists());
}

#[test]
fn a_busy_migration_rename_is_retried() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    let tries = RefCell::new(0);
    let r = roots::migrate_with(
        t.path(),
        MigrationOps {
            fan_out: &roots::fan_out,
            rename: &mut |a, b| {
                *tries.borrow_mut() += 1;
                if *tries.borrow() < 3 {
                    Err(std::io::Error::from_raw_os_error(5))
                } else {
                    std::fs::rename(a, b)
                }
            },
            sleep: &mut no_sleep,
        },
    );
    assert!(r.unwrap().is_some());
    assert_eq!(*tries.borrow(), 3);
}

#[test]
fn an_item_that_cannot_be_listed_fails_the_migration() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    // A folder held open without sharing cannot be listed on Windows; the
    // copy must stop rather than skip it.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(t.path().join("scripts/case-1.json"))
            .unwrap();
        let r = roots::migrate_once(t.path());
        assert!(r.is_err(), "an unreadable file was skipped: {r:?}");
        assert_flat_and_no_envs(t.path());
    }
}

#[test]
fn a_link_in_place_of_an_item_fails_the_migration() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    let real = t.path().join("elsewhere");
    std::fs::create_dir_all(&real).unwrap();
    put(&real, "case-1.json", "S");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&real, t.path().join("scripts"));
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&real, t.path().join("scripts"));
    if made.is_err() {
        eprintln!("skipped: this account may not create symbolic links here");
        return;
    }
    let r = roots::migrate_once(t.path());
    assert_eq!(r.unwrap_err(), roots::ITEM_IS_A_LINK);
    assert_flat_and_no_envs(t.path());
}

fn a_run(id: &str) -> LocalRun {
    LocalRun {
        id: id.to_string(),
        pbi_id: 1,
        started_at: "1".to_string(),
        cases: vec![],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    }
}

#[test]
fn saving_a_migrated_run_leaves_the_flat_original_unchanged() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    store::save_run(t.path(), &a_run("run-1")).unwrap();
    let flat = std::fs::read(t.path().join("runs/run-1.json")).unwrap();
    roots::migrate_once(t.path()).unwrap().unwrap();
    let env = roots::env_root(t.path(), A);
    // Not the selected environment: writes into it are allowed on purpose.
    let _allow = roots::allow_writes_to(&env);
    let mut changed = a_run("run-1");
    changed.mode = "published-later".to_string();
    store::save_run(&env, &changed).unwrap();
    assert_eq!(std::fs::read(t.path().join("runs/run-1.json")).unwrap(), flat);
    assert!(store::load_run(&env, "run-1").unwrap().unwrap().mode == "published-later");
}

#[test]
fn runs_are_copied_but_shots_are_linked() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "runs/run-1.json", "R");
    put(t.path(), "shots/a.jpg", "P");
    roots::migrate_once(t.path()).unwrap().unwrap();
    // A write through the original shows through a link and not through a copy.
    for rel in ["runs/run-1.json", "shots/a.jpg"] {
        let mut f = std::fs::OpenOptions::new().append(true).open(t.path().join(rel)).unwrap();
        std::io::Write::write_all(&mut f, b"+").unwrap();
    }
    assert_eq!(std::fs::read_to_string(at(t.path(), A, "runs/run-1.json")).unwrap(), "R");
    assert_eq!(std::fs::read_to_string(at(t.path(), A, "shots/a.jpg")).unwrap(), "P+");
}

#[test]
fn a_development_build_migrates_only_when_asked() {
    assert!(roots::migration_allowed(false, None));
    assert!(roots::migration_allowed(true, Some("1")));
    assert!(!roots::migration_allowed(true, None));
    assert!(!roots::migration_allowed(true, Some("0")));
    assert!(roots::migration_allowed(false, Some("0")));
}

#[test]
fn start_up_skips_the_copy_in_a_development_build_but_uses_an_existing_envs() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    let s = roots::start_up(t.path(), false);
    assert!(s.skipped && !s.per_env && s.report.is_none());
    assert!(!t.path().join("envs").exists());
    std::fs::create_dir_all(t.path().join("envs")).unwrap();
    assert!(roots::start_up(t.path(), false).per_env);
}

#[test]
fn start_up_falls_back_to_the_flat_folder_when_the_copy_fails() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "scripts/case-1.json", "S");
    put(t.path(), "environments.json", "{ not json");
    let s = roots::start_up(t.path(), true);
    assert!(!s.per_env && s.report.is_none());
    let said = s.failed.expect("the failure was not reported");
    assert!(roots::path_free(&said), "{said}");
    two_envs(t.path());
    let s = roots::start_up(t.path(), true);
    assert!(s.per_env && s.report.is_some() && s.failed.is_none());
}

#[test]
fn a_seed_left_behind_is_handed_over_at_the_next_start() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    let file = environments::load_or_init(t.path(), None).unwrap();
    let id = file.active.clone();
    // The handover that happened with the list failed: put the seed back.
    std::fs::rename(at(t.path(), &id, ""), at(t.path(), "_seed", "")).unwrap();
    assert!(at(t.path(), "_seed", "scripts/a.json").is_file());
    let s = roots::start_up(t.path(), false);
    assert!(s.per_env);
    assert!(at(t.path(), &id, "scripts/a.json").is_file());
    assert!(!at(t.path(), "_seed", "").exists());
}

#[test]
fn the_migration_rename_waits_much_longer_than_an_ordinary_one() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "scripts/case-1.json", "S");
    let waited = RefCell::new(0u64);
    let r = roots::migrate_with(
        t.path(),
        MigrationOps {
            fan_out: &roots::fan_out,
            rename: &mut |_, _| Err(std::io::Error::from_raw_os_error(5)),
            sleep: &mut |d| *waited.borrow_mut() += d.as_millis() as u64,
        },
    );
    assert!(r.is_err());
    let ms = *waited.borrow();
    assert!((14_000..=20_000).contains(&ms), "waited {ms} ms");
    assert!(!t.path().join("envs").exists());
}

#[test]
fn failed_starts_are_counted_and_cleared_on_success() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "scripts/case-1.json", "S");
    put(t.path(), "environments.json", "{ not json");
    assert_eq!(roots::start_up(t.path(), true).failed_starts, 1);
    assert_eq!(roots::start_up(t.path(), true).failed_starts, 2);
    assert_eq!(roots::start_up(t.path(), true).failed_starts, 3);
    two_envs(t.path());
    let ok = roots::start_up(t.path(), true);
    assert!(ok.per_env && ok.failed_starts == 0);
    // Cleared: a later failure starts again from one.
    std::fs::remove_dir_all(t.path().join("envs")).unwrap();
    put(t.path(), "environments.json", "{ not json");
    assert_eq!(roots::start_up(t.path(), true).failed_starts, 1);
}

#[test]
fn the_warning_explains_itself_from_the_third_failed_start() {
    let early = roots::failure_warning("Access is denied.", 2);
    assert!(!early.contains("in a row"), "{early}");
    let late = roots::failure_warning("Access is denied.", 3);
    assert!(late.contains("3 starts in a row") && late.contains("antivirus"), "{late}");
    assert!(roots::path_free(&late));
}

#[test]
fn a_left_behind_seed_moves_into_an_empty_first_environment() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    put(t.path(), "envs/_seed/orders/o.json", "O");
    let r = roots::retry_seed(t.path());
    assert_eq!((r.moved, r.kept), (2, 0));
    assert!(at(t.path(), A, "scripts/a.json").is_file() && at(t.path(), A, "orders/o.json").is_file());
    assert!(!at(t.path(), "_seed", "").exists());
}

#[test]
fn a_left_behind_seed_fills_only_what_the_first_environment_lacks() {
    let t = tempfile::tempdir().unwrap();
    two_envs(t.path());
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    put(t.path(), "envs/_seed/orders/o.json", "O");
    put(t.path(), &format!("envs/{A}/orders/own.json"), "OWN");
    let r = roots::retry_seed(t.path());
    assert_eq!((r.moved, r.kept), (1, 1));
    assert!(at(t.path(), A, "scripts/a.json").is_file());
    assert_eq!(std::fs::read_to_string(at(t.path(), A, "orders/own.json")).unwrap(), "OWN");
    assert!(!at(t.path(), A, "orders/o.json").exists());
    // What was kept aside stays in the seed.
    assert!(at(t.path(), "_seed", "orders/o.json").is_file());
    assert!(!at(t.path(), "_seed", "scripts").exists());
}

#[test]
fn a_new_environment_gets_nothing_from_a_left_behind_seed() {
    let t = tempfile::tempdir().unwrap();
    environments::load_or_init(t.path(), None).unwrap();
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    let file = environments::save_env(t.path(), env_named("", "Second"), &["dev-read".to_string()]).unwrap();
    let new = file.environments.iter().find(|e| e.name == "Second").unwrap();
    assert!(!at(t.path(), &new.id, "scripts").exists());
    assert!(at(t.path(), "_seed", "scripts/a.json").is_file());
}
