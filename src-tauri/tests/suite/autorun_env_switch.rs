//! The switch guard: a write from before a switch is refused and lands
//! nowhere, a flat layout is never refused, a non-active environment can be
//! written only while it is explicitly allowed, a switch is refused while a
//! browser holds an account, and the per-environment stores write only
//! through `autorun::roots`.

use std::path::Path;
use v2_lib::autorun::lease::{self, Holder};
use v2_lib::autorun::roots;
use v2_lib::autorun::store::{self, SaveScriptsError};
use v2_lib::commands::environments::{refuse_switch, SWITCH_BROWSER_SIGNED_IN};
use v2_lib::environments::{self, Environment};

/// Leaves the process-wide layout on a throw-away flat folder, so a later
/// test never sees a path that is gone.
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        store::set_root(std::env::temp_dir().join("tcm-autorun-switch-restored"));
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

/// A shared root with two environments; the first is selected. Returns the
/// two ids.
fn two_envs(t: &Path) -> (String, String) {
    store::set_shared_root(t.to_path_buf());
    let a = environments::load_or_init(t, Some("dev-read")).unwrap().active;
    let b = environments::save_env(t, env_named("QA"), &["dev-read".to_string()])
        .unwrap()
        .environments
        .into_iter()
        .find(|e| e.name == "QA")
        .unwrap()
        .id;
    store::select_environment(&a).unwrap();
    (a, b)
}

#[test]
fn a_save_with_a_root_from_before_a_switch_is_refused() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    let ra = store::configured_root().unwrap();
    assert_eq!(ra, roots::env_root(t.path(), &a));
    store::select_environment(&b).unwrap();

    let err = store::save_scripts_atomically(&ra, &[crate::common::component_script(11)]).unwrap_err();
    assert!(matches!(err, SaveScriptsError::EnvironmentSwitched), "{err:?}");
    assert_eq!(err.to_string(), roots::ENVIRONMENT_SWITCHED);
    assert!(!roots::env_root(t.path(), &a).join("scripts").join("case-11.json").exists());
    assert!(!roots::env_root(t.path(), &b).join("scripts").join("case-11.json").exists());
    // The other writes of a store are refused the same way.
    assert_eq!(store::save_order(&ra, 5, &[1, 2]), Err(roots::ENVIRONMENT_SWITCHED.to_string()));
    assert!(!ra.join("orders").exists());
}

#[test]
fn a_clear_with_a_root_from_before_a_switch_removes_nothing() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    let ra = store::configured_root().unwrap();
    store::save_scripts_atomically(&ra, &[crate::common::component_script(11)]).unwrap();
    let file = roots::env_root(t.path(), &a).join("scripts").join("case-11.json");
    assert!(file.is_file());
    store::select_environment(&b).unwrap();

    assert_eq!(store::clear_scripts(&ra, &[11]), Err(roots::ENVIRONMENT_SWITCHED.to_string()));
    assert!(file.is_file(), "the script of the environment that was left is still there");
}

#[test]
fn a_save_in_the_active_environment_goes_through() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (a, _b) = two_envs(t.path());
    let ra = store::configured_root().unwrap();
    store::save_scripts_atomically(&ra, &[crate::common::component_script(12)]).unwrap();
    assert!(roots::env_root(t.path(), &a).join("scripts").join("case-12.json").is_file());
}

#[test]
fn flat_layout_saves_are_never_refused() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_root(t.path().to_path_buf());
    store::save_order(t.path(), 5, &[1, 2]).unwrap();
    store::save_scripts_atomically(t.path(), &[crate::common::component_script(13)]).unwrap();
    assert!(t.path().join("scripts").join("case-13.json").is_file());
}

#[test]
fn allowed_target_accepts_writes_while_held() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (_a, b) = two_envs(t.path());
    let rb = roots::env_root(t.path(), &b);
    std::fs::create_dir_all(&rb).unwrap();

    assert_eq!(store::save_order(&rb, 5, &[1]), Err(roots::ENVIRONMENT_SWITCHED.to_string()));
    let allowed = roots::allow_writes_to(&rb);
    store::save_order(&rb, 5, &[1]).unwrap();
    assert!(rb.join("orders").is_dir());
    // A second hold on the same root keeps it open until both are gone.
    let again = roots::allow_writes_to(&rb);
    drop(allowed);
    store::save_order(&rb, 5, &[1, 2]).unwrap();
    drop(again);
    assert_eq!(store::save_order(&rb, 5, &[1]), Err(roots::ENVIRONMENT_SWITCHED.to_string()));
}

#[test]
fn a_guard_holds_the_switch_lock_for_reading() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (_a, _b) = two_envs(t.path());
    let ra = store::configured_root().unwrap();
    let held = roots::guard(&ra).unwrap();
    // Safe whatever else runs: a held read lock always blocks a writer.
    assert!(roots::switch_lock().try_write().is_err(), "a switch must wait for a write in flight");
    drop(held);
}

#[test]
fn a_switch_waits_for_a_bundle_and_the_bundle_lands_whole_or_not_at_all() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    let ra = store::configured_root().unwrap();
    let bundle: Vec<_> = (21..26).map(crate::common::component_script).collect();

    // A switch is under way (it holds the write lock): the bundle waits for it,
    // then finds its root is no longer the active one and writes nothing.
    let switching = roots::switch_lock().write().unwrap();
    let saver = {
        let (ra, bundle) = (ra.clone(), bundle.clone());
        std::thread::spawn(move || store::save_scripts_atomically(&ra, &bundle))
    };
    std::thread::sleep(std::time::Duration::from_millis(200));
    store::select_environment(&b).unwrap();
    drop(switching);
    let err = saver.join().unwrap().unwrap_err();
    assert!(matches!(err, SaveScriptsError::EnvironmentSwitched), "{err:?}");
    for id in 21..26 {
        assert!(!roots::env_root(t.path(), &a).join("scripts").join(format!("case-{id}.json")).exists());
        assert!(!roots::env_root(t.path(), &b).join("scripts").join(format!("case-{id}.json")).exists());
    }
    assert!(!roots::env_root(t.path(), &a).join("scripts").exists(), "a refused bundle makes no folder");
}

#[test]
fn a_bundle_is_written_under_one_guard() {
    // The pass that writes the files must hold ONE guard for all of them: a
    // switch between two files would land half a bundle in the old environment.
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/autorun/store.rs")).unwrap().replace("\r\n", "\n");
    let start = text.find("fn write_scripts_held(").unwrap();
    let body = &text[start..];
    let body = &body[..body.find("
}
").unwrap()];
    assert!(!body.contains("roots::write_file("), "a per-file guard inside the bundle write:
{body}");
    assert!(body.contains("roots::write_file_held("));
}

#[test]
fn switch_is_refused_while_a_browser_holds_an_account() {
    // The locks a switch test takes, in the suite's order.
    let _claims = crate::serial::autorun();
    let _slot = crate::serial::api_template_run();
    let _held = crate::serial::held_browsers();
    let _leases = crate::serial::account_leases();
    assert_eq!(refuse_switch(false), Ok(()));
    let browser = lease::try_acquire("env-switchtst", "admin", Holder::Browser { slot: 0 }).unwrap();
    assert_eq!(refuse_switch(false), Err(SWITCH_BROWSER_SIGNED_IN.to_string()));
    drop(browser);
    assert_eq!(refuse_switch(false), Ok(()));
    // A template run's lease is not a browser a person has to close.
    let template = lease::try_acquire("env-switchtst", "admin", Holder::Template).unwrap();
    assert_eq!(refuse_switch(false), Ok(()));
    drop(template);
}

#[test]
fn what_is_on_screen_is_judged_by_position() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (a, b) = two_envs(t.path());
    let ra = roots::env_root(t.path(), &a);
    let rb = roots::env_root(t.path(), &b);
    // Per-environment layout.
    assert!(roots::is_on_screen(&ra));
    assert!(roots::is_on_screen(&ra.join("projects")));
    assert!(roots::is_on_screen(t.path()), "the shared root");
    assert!(roots::is_on_screen(&t.path().join("test-files").join("acme-hr")), "test files are shared");
    assert!(roots::is_on_screen(&t.path().join("proposals")));
    assert!(!roots::is_on_screen(&rb), "another environment");
    assert!(!roots::is_on_screen(&rb.join("scripts")));
    assert!(!roots::is_on_screen(&std::env::temp_dir().join("tcm-elsewhere-x")));
    // Flat layout: everything under the one folder.
    let flat = tempfile::tempdir().unwrap();
    store::set_root(flat.path().to_path_buf());
    assert!(roots::is_on_screen(flat.path()));
    assert!(roots::is_on_screen(&flat.path().join("test-files").join("acme-hr")));
    assert!(!roots::is_on_screen(&t.path().join("test-files")));
}

#[test]
fn export_records_and_stores_refuse_a_stale_root_and_make_no_folder() {
    use v2_lib::autorun::components::{self, Component};
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (_a, b) = two_envs(t.path());
    let ra = store::configured_root().unwrap();
    store::select_environment(&b).unwrap();

    let map = v2_lib::pw_export::mapping::ExportMap::default();
    let p = v2_lib::pw_export::mapping::path(&ra, "Acme", "HR");
    assert_eq!(v2_lib::pw_export::mapping::save_at(&ra, &p, &map), Err(roots::ENVIRONMENT_SWITCHED.to_string()));
    assert!(!ra.join("projects").exists(), "no folder in the old environment");

    let c: Component = serde_json::from_value(serde_json::json!({
        "name": "Pick", "description": "", "inputs": [], "actions": []
    }))
    .unwrap();
    assert!(components::put(&ra, "Acme", "HR", c).is_err());
    assert!(!ra.join("projects").exists(), "no folder in the old environment");
}

#[test]
fn removals_refuse_a_stale_root() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let (_a, b) = two_envs(t.path());
    let ra = store::configured_root().unwrap();
    store::save_order(&ra, 5, &[1, 2]).unwrap();
    store::select_environment(&b).unwrap();
    assert_eq!(store::clear_order(&ra, 5), Err(roots::ENVIRONMENT_SWITCHED.to_string()));
    assert!(ra.join("orders").read_dir().unwrap().count() > 0, "the order stayed");
}

/// What this scan CANNOT see: it reads the listed owner files line by line for
/// the `std::fs` calls below. It does not see `commands/*.rs` (the report
/// page write), `browser/cdp.rs` (a download rename; runs refuse a switch),
/// a path built into a variable first, `OpenOptions` beyond the two listed in
/// `ALLOWED`, or anything in a file that is not listed. It is a tripwire for
/// the common way of adding a bypass, not proof.
///
/// A call that is meant to bypass the guard is named in `ALLOWED` as
/// (file, text on the line, why).
#[test]
fn per_env_stores_write_only_through_roots() {
    const ALLOWED: &[(&str, &str, &str)] = &[
        ("autorun/store.rs", "LAST_BROWSER", "the shared root, a machine file"),
        ("autorun/store.rs", "create_dir_all(&shared)", "the shared root"),
        ("autorun/store.rs", "create_dir_all(&root)", "make_env_root: makes the folder a switch is about to select"),
        ("autorun/store.rs", "create_dir_all(dir).map_err(|e| not_saved", "save_order, guarded just above"),
        ("autorun/store.rs", "create_dir_all(&dir).map_err(|e| SaveScriptsError", "the bundle save, guarded just above"),
        ("autorun/store.rs", "std::fs::create_dir_all(&dir).map_err(|e| e.to_string())", "save_shot, guard held across"),
        ("autorun/store.rs", "std::fs::rename(&from, &to)", "delete_script, guard held across"),
        ("autorun/store.rs", "remove_file(dir.join(old))", "prune_shots: maintenance of the root it was given, not a save"),
        ("autorun/store.rs", "match std::fs::remove_file(&path) {", "clear_scripts / sweep_files: person-initiated or maintenance, on the root captured at the start"),
        ("autorun/store.rs", "remove_file(entry.path())", "purge_deleted_scripts: maintenance over every environment"),
        ("autorun/components.rs", "std::fs::rename(", "reset_components, guard held across"),
        ("autorun/discovery_map.rs", "std::fs::rename(", "reset_map, guard held across"),
        ("autorun/explore.rs", "remove_file(session_path", "undoes begin's own new file"),
        ("pw_export/mapping.rs", "std::fs::create_dir_all(dir)", "prepare: always called after the guard"),
        ("autorun/store.rs", "OpenOptions", "save_shot, guard held across"),
        ("autorun/explore.rs", "OpenOptions", "begin: guarded at its top, cleaned up if refused"),
        ("autorun/explore.rs", "create_dir_all(&dir)", "begin: guarded at its top"),
    ];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // The per-environment owner files of the classification table.
    let owners = [
        "autorun/store.rs",
        "autorun/nav.rs",
        "autorun/components.rs",
        "autorun/quirks.rs",
        "autorun/setups.rs",
        "autorun/discovery_map.rs",
        "autorun/divergences.rs",
        "autorun/explore.rs",
        "autorun/mapping_summary.rs",
        "autorun/recipe.rs",
        "autorun/replay_record.rs",
        "autorun/capture_tools.rs",
        "api_templates/store.rs",
        "api_templates/fixture_store.rs",
        "api_templates/flow_store.rs",
        "pw_export/mapping.rs",
    ];
    let calls = [
        "ai_tools::atomic_write(",
        "fs::write(",
        "fs::create_dir_all(",
        "fs::remove_file(",
        "fs::remove_dir_all(",
        "fs::rename(",
        "OpenOptions",
    ];
    let mut seen = 0;
    let mut found: Vec<String> = Vec::new();
    for rel in owners {
        let path = src.join(rel);
        assert!(path.is_file(), "{rel} moved - update this list");
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            seen += 1;
            let code = line.trim_start();
            if code.starts_with("//") || !calls.iter().any(|c| code.contains(c)) {
                continue;
            }
            if ALLOWED.iter().any(|(f, text, _)| *f == rel && code.contains(text)) {
                continue;
            }
            found.push(format!("{rel}:{}: {code}", n + 1));
        }
    }
    assert!(seen > 5000, "the scan looked at too little ({seen} lines)");
    assert!(found.is_empty(), "writes that bypass roots:: (or add to ALLOWED with a reason):
{}", found.join("
"));
}
