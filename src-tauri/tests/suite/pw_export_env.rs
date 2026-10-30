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
