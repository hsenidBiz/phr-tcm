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
    assert!(roots::switch_lock().try_write().is_err(), "a switch must wait for a write in flight");
    drop(held);
    assert!(roots::switch_lock().try_write().is_ok());
}

#[test]
fn switch_is_refused_while_a_browser_holds_an_account() {
    // Other modules hold leases of their own without any lock, so only the
    // refusal while this test holds one can be asserted, never "free".
    let held = lease::try_acquire("env-switchtst", "admin", Holder::Browser { slot: 0 }).unwrap();
    assert!(lease::a_browser_is_held());
    assert_ne!(refuse_switch(false), Ok(()));
    let said = refuse_switch(false).unwrap_err();
    // Another test's own refusal (a run, a recording) may come first; a
    // browser's is the one named when nothing else is in the way.
    assert!(said == SWITCH_BROWSER_SIGNED_IN || said.starts_with("the environment cannot be switched"), "{said}");
    drop(held);
}

#[test]
fn per_env_stores_write_only_through_roots() {
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
    let mut seen = 0;
    let mut found: Vec<String> = Vec::new();
    for rel in owners {
        let path = src.join(rel);
        assert!(path.is_file(), "{rel} moved - update this list");
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            // A write outside the Auto Run root says so, on the line.
            if line.contains("// outside the Auto Run root") {
                continue;
            }
            if code.contains("ai_tools::atomic_write(") || code.contains("fs::write(") {
                found.push(format!("{rel}:{}: {code}", n + 1));
            }
            seen += 1;
        }
    }
    assert!(seen > 5000, "the scan looked at too little ({seen} lines)");
    assert!(found.is_empty(), "writes that bypass roots::write_file:\n{}", found.join("\n"));
}
