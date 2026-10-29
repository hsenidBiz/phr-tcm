//! The one-time copy of the flat Auto Run folder into a folder per
//! environment, and the seed a first environment made afterwards takes.

use std::path::{Path, PathBuf};
use v2_lib::autorun::roots::{self, MigrationReport};
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
fn seed_into_leaves_a_seed_alone_when_the_target_has_data() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "envs/_seed/scripts/a.json", "S");
    put(t.path(), &format!("envs/{A}/scripts/b.json"), "B");
    assert!(!roots::seed_into(t.path(), A).unwrap());
    assert!(at(t.path(), "_seed", "scripts/a.json").is_file());
    assert!(at(t.path(), A, "scripts/b.json").is_file());
}

#[test]
fn migration_log_line_names_no_path() {
    let line = roots::migration_log_line(&MigrationReport { environments: 2, items: 5 });
    assert!(line.contains("2 environment(s)") && line.contains("5 item(s)"), "{line}");
    assert!(!line.contains(':') || line.starts_with("Auto Run:"), "{line}");
    assert!(!line.contains('\\') &&!line.contains('/'), "{line}");
}
