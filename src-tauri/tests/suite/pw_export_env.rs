//! The Playwright export environment: one environment is named (the active
//! one at first), Export and Sync are refused in any other with one
//! sentence, it cannot be removed, and the export records move with it.

use std::path::{Path, PathBuf};
use v2_lib::autorun::roots;
use v2_lib::autorun::store;
use v2_lib::environments::{self, EnvFile, Environment};
use v2_lib::pw_export::mapping::{self, ExportMap, Placement};

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

/// Leaves the process-wide layout on a throw-away flat folder.
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        store::set_root(std::env::temp_dir().join("tcm-pw-export-env-restored"));
    }
}

/// A shared root with `Default` (active) and `Automation`, and both
/// environments' folders made. Returns (default id, automation id).
fn two_envs(t: &Path) -> (String, String) {
    let a = environments::load_or_init(t, Some("dev-read")).unwrap().active;
    let b = environments::save_env(t, env_named("Automation"), &["dev-read".to_string()])
        .unwrap()
        .environments
        .into_iter()
        .find(|e| e.name == "Automation")
        .unwrap()
        .id;
    for id in [&a, &b] {
        std::fs::create_dir_all(roots::env_root(t, id).join("projects")).unwrap();
    }
    (a, b)
}

fn put(root: &Path, rel: &str, text: &str) -> PathBuf {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, text).unwrap();
    p
}

fn file_on_disk(t: &Path) -> String {
    std::fs::read_to_string(t.join("environments.json")).unwrap()
}

/// The file with no export environment named, as an older version left it.
fn unname_export(t: &Path) -> EnvFile {
    let mut file: EnvFile = serde_json::from_str(&file_on_disk(t)).unwrap();
    file.playwright_export = None;
    std::fs::write(t.join("environments.json"), serde_json::to_string(&file).unwrap()).unwrap();
    file
}

fn has_path_char(s: &str) -> bool {
    s.chars().any(|c| c == '/' || c == '\\')
}

#[test]
fn the_export_environment_starts_as_the_active_one() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, _b) = two_envs(t.path());
    let mut file = unname_export(t.path());
    assert!(!file_on_disk(t.path()).contains("playwright_export"));
    assert_eq!(environments::export_env(t.path()).unwrap().id, a);
    assert!(file_on_disk(t.path()).contains("playwright_export"), "the choice is saved");
    // A name that is not listed is the active one again.
    file.playwright_export = Some("env-deadbeef".into());
    std::fs::write(t.path().join("environments.json"), serde_json::to_string(&file).unwrap()).unwrap();
    assert_eq!(environments::export_env(t.path()).unwrap().id, a);
}

#[test]
fn a_first_environment_is_made_the_export_environment() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let file = environments::load_or_init(t.path(), Some("dev-read")).unwrap();
    assert_eq!(file.playwright_export.as_deref(), Some(file.active.as_str()));
}

#[test]
fn export_is_refused_outside_the_export_environment() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    environments::set_export_env(t.path(), &b).unwrap();
    let want = "Export and Sync run only in Automation, the Playwright export environment - switch to Automation on the AI Bridge tab first";
    assert_eq!(environments::not_export_env("Automation"), want);
    assert_eq!(environments::check_export_env(&roots::env_root(t.path(), &a)).unwrap_err(), want);
    // The flat layout's one folder is judged by the active environment (a).
    assert_eq!(environments::check_export_env(t.path()).unwrap_err(), want);
    assert!(environments::check_export_env(&roots::env_root(t.path(), &b)).is_ok());
    environments::set_export_env(t.path(), &a).unwrap();
    assert!(environments::check_export_env(&roots::env_root(t.path(), &a)).is_ok());
    assert!(environments::check_export_env(t.path()).is_ok());
}

#[test]
fn the_check_makes_no_file_when_there_is_none() {
    let t = tempfile::tempdir().unwrap();
    assert!(environments::check_export_env(t.path()).is_ok());
    assert!(!t.path().join("environments.json").exists());
}

#[test]
fn removing_the_export_environment_is_refused() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    environments::set_export_env(t.path(), &b).unwrap();
    assert_eq!(environments::remove_env(t.path(), &b).unwrap_err(), environments::REMOVE_EXPORT_ENV);
    assert!(environments::listed_ids(t.path()).unwrap().contains(&b), "nothing was removed");
    // Named elsewhere, it can go.
    environments::set_export_env(t.path(), &a).unwrap();
    environments::remove_env(t.path(), &b).unwrap();
}

#[test]
fn changing_the_export_environment_moves_its_records() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    let (ra, rb) = (roots::env_root(t.path(), &a), roots::env_root(t.path(), &b));
    put(&ra, "projects/p.pwexport.json", "{\"from\":\"a-p\"}");
    put(&ra, "projects/q.pwexport.json", "{\"from\":\"a-q\"}");
    put(&ra, "projects/p.nav.json", "nav");
    put(&rb, "projects/p.pwexport.json", "{\"from\":\"b-p\"}");
    let file = environments::set_export_env(t.path(), &b).unwrap();
    assert_eq!(file.playwright_export.as_deref(), Some(b.as_str()));
    assert_eq!(environments::export_env(t.path()).unwrap().id, b);
    assert_eq!(std::fs::read_to_string(rb.join("projects/p.pwexport.json")).unwrap(), "{\"from\":\"a-p\"}", "replaced");
    assert_eq!(std::fs::read_to_string(rb.join("projects/q.pwexport.json")).unwrap(), "{\"from\":\"a-q\"}");
    assert!(!ra.join("projects/p.pwexport.json").exists());
    assert!(!ra.join("projects/q.pwexport.json").exists());
    assert!(ra.join("projects/p.nav.json").exists(), "only the export records move");
    assert!(
        std::fs::read_dir(rb.join("projects")).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".moving")),
        "no staging copy is left"
    );
    // Naming it again changes nothing.
    environments::set_export_env(t.path(), &b).unwrap();
    assert!(rb.join("projects/q.pwexport.json").exists());
}

#[test]
fn naming_an_unlisted_environment_changes_nothing() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, _b) = two_envs(t.path());
    let ra = roots::env_root(t.path(), &a);
    put(&ra, "projects/p.pwexport.json", "x");
    assert_eq!(environments::set_export_env(t.path(), "env-deadbeef").unwrap_err(), environments::EXPORT_ENV_UNKNOWN);
    assert!(ra.join("projects/p.pwexport.json").exists());
    assert_eq!(environments::export_env(t.path()).unwrap().id, a);
}

#[test]
fn a_move_that_cannot_finish_leaves_the_records_where_they_were() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    let (ra, rb) = (roots::env_root(t.path(), &a), roots::env_root(t.path(), &b));
    put(&ra, "projects/p.pwexport.json", "keep");
    // The destination's `projects` is a file: nothing can be put in it.
    std::fs::remove_dir_all(rb.join("projects")).unwrap();
    put(&rb, "projects", "in the way");
    let e = environments::set_export_env(t.path(), &b).unwrap_err();
    assert_eq!(e, environments::EXPORT_ENV_NOT_MOVED);
    assert!(!has_path_char(&e), "the sentence names no path");
    assert_eq!(std::fs::read_to_string(ra.join("projects/p.pwexport.json")).unwrap(), "keep");
    assert_eq!(environments::export_env(t.path()).unwrap().id, a, "still the old export environment");
}

#[test]
fn renaming_a_repo_folder_moves_placements_in_the_export_environment_while_another_is_active() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    environments::set_export_env(t.path(), &b).unwrap();
    store::set_shared_root(t.path().to_path_buf());
    store::select_environment(&a).unwrap();
    let rb = roots::env_root(t.path(), &b);

    let clone = tempfile::tempdir().unwrap();
    put(clone.path(), "suites/sl/admin/performance/specs/p.spec.ts", "export {};\n");
    put(clone.path(), "src/helpers.ts", "export const h = 1;\n");
    let sync = tempfile::tempdir().unwrap();
    let rec = v2_lib::pw_repo::sync::record_path(sync.path(), &clone.path().to_string_lossy(), "org", "proj");
    std::fs::write(rec, "{}").unwrap();
    let mut map = ExportMap::default();
    map.areas.insert("Perf".into(), Placement { path: "sl/admin/performance".into() });
    // Saved while `b` is allowed, as the commands do.
    {
        let _allowed = roots::allow_writes_to(&rb);
        mapping::save(&rb, "org", "proj", &map).unwrap();
    }
    // Without the hold the active environment is `a`: the write is refused.
    assert!(mapping::save(&rb, "org", "proj", &map).is_err());

    let _allowed = roots::allow_writes_to(&rb);
    let done = v2_lib::pw_repo::rename_folder::rename(
        clone.path(),
        Some(&rb),
        Some(sync.path()),
        "sl/admin/performance",
        "sl/admin/perf",
        true,
    )
    .unwrap();
    assert_eq!(done.placements_updated, 1);
    assert!(done.warning.is_none(), "{:?}", done.warning);
    let moved = mapping::load(&rb, "org", "proj").unwrap();
    assert_eq!(moved.areas["Perf"].path, "sl/admin/perf");
}

#[test]
fn adopt_export_env_makes_no_environments_file() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    assert!(!environments::adopt_export_env(t.path()).unwrap());
    assert!(!t.path().join("environments.json").exists());
    // With a file that names none: the active one, once.
    let (a, _b) = two_envs(t.path());
    unname_export(t.path());
    assert!(environments::adopt_export_env(t.path()).unwrap());
    assert!(!environments::adopt_export_env(t.path()).unwrap());
    assert_eq!(environments::export_env(t.path()).unwrap().id, a);
}

#[test]
fn the_removal_check_reads_the_export_name_without_the_environments_lock() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (_a, b) = two_envs(t.path());
    environments::set_export_env(t.path(), &b).unwrap();
    assert!(roots::is_export_environment(t.path(), &b));
    assert!(!roots::is_export_environment(t.path(), "env-00000000"));
    assert_eq!(roots::removal_refusal(t.path(), &b), Some(roots::REMOVE_EXPORT_ENVIRONMENT));
}

// ---- fix round 1 -----------------------------------------------------------

const MA: &str = "env-0000000a";
const MB: &str = "env-0000000b";

/// A flat root (records and scripts at the top) whose environments file lists
/// A (active) and B, with `export` named as the export environment.
fn flat_with_export(t: &Path, export: Option<&str>) {
    let mut a = env_named("One");
    a.id = MA.into();
    let mut b = env_named("Two");
    b.id = MB.into();
    let file = EnvFile {
        active: MA.into(),
        environments: vec![a, b],
        auto_approve_adopted: false,
        playwright_export: export.map(str::to_string),
    };
    put(t, "environments.json", &serde_json::to_string(&file).unwrap());
    put(t, "scripts/case-1.json", "{}");
    put(t, "projects/p.pwexport.json", "{\"rec\":1}");
}

#[test]
fn the_update_puts_the_export_records_in_the_named_export_environment() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    flat_with_export(t.path(), Some(MB));
    roots::migrate_once(t.path()).unwrap().unwrap();
    assert!(roots::env_root(t.path(), MB).join("projects/p.pwexport.json").is_file());
    assert!(!roots::env_root(t.path(), MA).join("projects/p.pwexport.json").exists());
    assert!(roots::env_root(t.path(), MA).join("scripts/case-1.json").is_file(), "everything else goes to both");
}

#[test]
fn the_update_falls_back_to_the_active_environment_when_the_name_is_unusable() {
    let _lock = crate::serial::autorun();
    for export in [None, Some("env-deadbeef")] {
        let t = tempfile::tempdir().unwrap();
        flat_with_export(t.path(), export);
        roots::migrate_once(t.path()).unwrap().unwrap();
        assert!(roots::env_root(t.path(), MA).join("projects/p.pwexport.json").is_file(), "{export:?}");
        assert!(!roots::env_root(t.path(), MB).join("projects/p.pwexport.json").exists(), "{export:?}");
    }
}

#[test]
fn in_the_flat_layout_naming_the_export_environment_only_records_the_name() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    flat_with_export(t.path(), Some(MA));
    let file = environments::set_export_env(t.path(), MB).unwrap();
    assert_eq!(file.playwright_export.as_deref(), Some(MB));
    assert!(!t.path().join("envs").exists(), "nothing under envs/ is made");
    assert!(t.path().join("projects/p.pwexport.json").is_file(), "the one folder keeps its record");
}

#[test]
fn a_read_never_makes_the_first_environment() {
    let t = tempfile::tempdir().unwrap();
    assert!(environments::export_env(t.path()).is_err());
    assert!(!t.path().join("environments.json").exists());
}

#[test]
fn a_leftover_record_the_old_environment_does_not_have_is_set_aside() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    let (ra, rb) = (roots::env_root(t.path(), &a), roots::env_root(t.path(), &b));
    put(&ra, "projects/p.pwexport.json", "new");
    put(&rb, "projects/gone.pwexport.json", "stale placements");
    environments::set_export_env(t.path(), &b).unwrap();
    assert_eq!(std::fs::read_to_string(rb.join("projects/p.pwexport.json")).unwrap(), "new");
    assert!(!rb.join("projects/gone.pwexport.json").exists(), "it is not current");
    assert_eq!(std::fs::read_to_string(rb.join("projects/gone.pwexport.json.stale")).unwrap(), "stale placements");
    assert!(mapping::files(&rb).iter().all(|p| !p.to_string_lossy().contains("gone")));
}

#[test]
fn staging_files_are_swept_and_never_copied_to_another_environment() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    let (ra, rb) = (roots::env_root(t.path(), &a), roots::env_root(t.path(), &b));
    put(&ra, "projects/p.pwexport.json.moving", "half");
    put(&ra, "projects/q.pwexport.json.stale", "old");
    put(&ra, "scripts/case-1.json", "{}");
    assert!(roots::is_export_record(Path::new("projects/p.pwexport.json.moving")));
    assert!(roots::is_export_record(Path::new("projects/q.pwexport.json.stale")));
    assert_eq!(roots::sweep_export_staging(t.path()), 1);
    assert!(!ra.join("projects/p.pwexport.json.moving").exists());
    assert!(ra.join("projects/q.pwexport.json.stale").exists(), "a set-aside record is kept");
    // A copy into the other environment leaves both families behind.
    put(&ra, "projects/r.pwexport.json.moving", "half");
    v2_lib::autorun::env_copy::copy_environment(t.path(), &ra, &b, "Two", true, 1_000).unwrap();
    assert!(rb.join("scripts/case-1.json").is_file());
    assert!(!rb.join("projects/r.pwexport.json.moving").exists());
    assert!(!rb.join("projects/q.pwexport.json.stale").exists());
}

#[test]
fn naming_the_export_environment_waits_for_a_records_write_in_flight() {
    let _lock = crate::serial::autorun();
    let t = tempfile::tempdir().unwrap();
    let (_a, b) = two_envs(t.path());
    let held = mapping::records_lock().lock().unwrap_or_else(|e| e.into_inner());
    let (root, id) = (t.path().to_path_buf(), b.clone());
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    let worker = std::thread::spawn(move || {
        environments::set_export_env(&root, &id).unwrap();
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!done.load(std::sync::atomic::Ordering::SeqCst), "it waits while a records write holds the lock");
    drop(held);
    worker.join().unwrap();
    assert!(done.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn every_script_carrying_command_starts_from_the_export_root() {
    let read = |p: &str| std::fs::read_to_string(format!("{}/src/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let repo = read("commands/pw_repo.rs");
    assert_eq!(repo.matches("autorun::root(").count(), 0, "pw_repo.rs reads the active root only through export_root");
    assert_eq!(repo.matches("pw_export::export_root(&app)?").count(), 4, "states, handoff, add_new, update");
    let export = read("commands/pw_export.rs");
    assert_eq!(export.matches("export_root(&app)?").count(), 3, "preview, save_map, write");
    // The only other readers of the active root are the export-environment views.
    assert_eq!(export.matches("autorun::root(").count(), 4, "export_root, export_env_root, pw_export_env, pw_set_export_env");
}

/// The webview tells a refusal from a failure by how its sentence BEGINS
/// (src/screens/Playwright/Sync.tsx, src/screens/Settings.tsx,
/// src/lib/copyRefusal.ts). Rewording one of these without changing the
/// matching text there would turn a calm reason into a red failure.
#[test]
fn the_sentences_the_webview_matches_by_their_beginning_still_begin_so() {
    use v2_lib::autorun::{backup_import, env_copy};
    use v2_lib::commands::environments as ce;
    assert!(environments::not_export_env("X").starts_with("Export and Sync run only in "));
    assert!(backup_import::IMPORT_PENDING.starts_with("an earlier import has not finished"));
    assert!(backup_import::IMPORT_UNFINISHED.starts_with("the import did not finish"));
    assert!(env_copy::CASE_COPY_SWITCHED.starts_with("the environment was switched"));
    assert!(env_copy::COPY_NEEDS_CONFIRM.contains("confirm to replace it"));
    assert!(env_copy::CASE_COPY_NEEDS_CONFIRM.contains("confirm to replace it"));
    for s in [ce::SWITCH_BUSY, ce::SWITCH_TEMPLATE_RUNNING, ce::SWITCH_SUPERVISED_OPEN, ce::SWITCH_BROWSER_SIGNED_IN] {
        assert!(s.starts_with("the environment cannot be switched"), "{s}");
    }
}
