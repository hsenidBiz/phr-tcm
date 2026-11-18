//! The app-wide Simple mode: its start-up default and the switch-on.

use std::collections::BTreeSet;

use v2_lib::app_settings::{AppSettings, AutorunMode};
use v2_lib::modules::{Module, HIDDEN_UNTIL_UNLOCKED};
use v2_lib::simple_mode::{is_new_install, modules_to_switch_on, settle_on_start_in, settle_simple_mode, set_app_simple_mode_with, switch_all_on_with};

fn on(ms: &[Module]) -> BTreeSet<Module> {
    ms.iter().copied().collect()
}

#[test]
fn settle_simple_mode_covers_all_four_inputs() {
    assert_eq!(settle_simple_mode(None, true), Some(true));
    assert_eq!(settle_simple_mode(None, false), Some(false));
    assert_eq!(settle_simple_mode(Some(true), false), Some(true));
    assert_eq!(settle_simple_mode(Some(false), true), Some(false));
}

#[test]
fn modules_to_switch_on_is_parent_first_and_never_the_held_back_one() {
    assert_eq!(
        modules_to_switch_on(&BTreeSet::new()),
        vec![Module::DbAccess, Module::WritingStyle, Module::AutoRun, Module::ApiTemplates, Module::Playwright]
    );
    assert!(!modules_to_switch_on(&BTreeSet::new()).contains(&HIDDEN_UNTIL_UNLOCKED));
    assert_eq!(modules_to_switch_on(&Module::ALL.into_iter().collect()), Vec::<Module>::new());
    assert_eq!(modules_to_switch_on(&on(&[Module::DbAccess, Module::AutoRun])), vec![Module::WritingStyle, Module::ApiTemplates, Module::Playwright]);
}

#[test]
fn switching_on_brings_every_module_on_and_leaves_the_api_templates_mark() {
    let d = tempfile::tempdir().unwrap();
    let mut st = BTreeSet::new();
    let mut s = AppSettings::default();
    let failed = switch_all_on_with(&mut st, &mut s, d.path());
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(st, on(&[Module::DbAccess, Module::WritingStyle, Module::AutoRun, Module::ApiTemplates, Module::Playwright]));
    assert!(!s.api_templates_set);
    assert!(!st.contains(&HIDDEN_UNTIL_UNLOCKED));
    // Auto Run's recommended features, on a machine with no Auto Run work.
    assert_eq!(s.autorun_mode, Some(AutorunMode::Simple));
    assert!(s.autorun_failed_video);
    // Saved, not only held.
    assert_eq!(v2_lib::modules::saved(d.path()).unwrap(), st);
    assert!(!v2_lib::app_settings::load(d.path()).api_templates_set);
}

#[test]
fn switching_on_keeps_an_api_templates_choice_a_person_made() {
    let d = tempfile::tempdir().unwrap();
    let mut st = BTreeSet::new();
    let mut s = AppSettings { api_templates_set: true, ..AppSettings::default() };
    assert!(switch_all_on_with(&mut st, &mut s, d.path()).is_empty());
    assert!(s.api_templates_set);
}

#[test]
fn turning_it_off_after_on_leaves_the_modules_on() {
    let d = tempfile::tempdir().unwrap();
    let mut st = BTreeSet::new();
    let mut s = AppSettings::default();
    let a = set_app_simple_mode_with(&mut st, &mut s, d.path(), true).unwrap();
    assert!(a.not_switched_on.is_empty());
    assert_eq!(a.settings.app_simple_mode, Some(true));
    let modules_before = st.clone();
    assert_eq!(modules_before.len(), 5);
    let b = set_app_simple_mode_with(&mut st, &mut s, d.path(), false).unwrap();
    assert_eq!(b.settings.app_simple_mode, Some(false));
    assert!(b.not_switched_on.is_empty());
    assert_eq!(st, modules_before);
    assert_eq!(v2_lib::modules::saved(d.path()).unwrap(), modules_before);
    assert_eq!(v2_lib::app_settings::load(d.path()).app_simple_mode, Some(false));
}

#[test]
fn any_other_sign_of_an_earlier_launch_is_an_existing_install() {
    for marker in ["cache.json", "databases.json", "extras.json", "writing-style.json"] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(marker), "{}").unwrap();
        assert!(!is_new_install(d.path()), "{marker}");
    }
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("autorun")).unwrap();
    assert!(!is_new_install(d.path()));

    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("cache.json"), "{}").unwrap();
    let new_install = is_new_install(d.path());
    let mut st = BTreeSet::new();
    let mut s = AppSettings::default();
    settle_on_start_in(d.path(), new_install, &mut st, &mut s);
    assert_eq!(s.app_simple_mode, Some(false));
    assert!(st.is_empty());
}

#[test]
fn an_empty_dir_is_a_new_install_and_gets_simple_mode_with_everything_on() {
    let d = tempfile::tempdir().unwrap();
    assert!(is_new_install(d.path()));
    let mut st = BTreeSet::new();
    let mut s = AppSettings::default();
    let failed = settle_on_start_in(d.path(), true, &mut st, &mut s);
    assert!(failed.is_empty());
    assert_eq!(s.app_simple_mode, Some(true));
    assert!(s.switch_markers_migrated);
    assert_eq!(st, on(&[Module::DbAccess, Module::WritingStyle, Module::AutoRun, Module::ApiTemplates, Module::Playwright]));
    let saved = v2_lib::app_settings::load(d.path());
    assert_eq!(saved.app_simple_mode, Some(true));
    assert!(saved.switch_markers_migrated);
}

#[test]
fn an_existing_modules_file_or_settings_file_is_an_existing_install() {
    let a = tempfile::tempdir().unwrap();
    std::fs::write(a.path().join("modules.json"), r#"{"on":["db_access"]}"#).unwrap();
    assert!(!is_new_install(a.path()));
    let b = tempfile::tempdir().unwrap();
    std::fs::write(b.path().join("app-settings.json"), "{}").unwrap();
    assert!(!is_new_install(b.path()));

    let new_install = is_new_install(a.path());
    let mut st = v2_lib::modules::load_or_create(a.path(), false);
    let mut s = AppSettings::default();
    let failed = settle_on_start_in(a.path(), new_install, &mut st, &mut s);
    assert!(failed.is_empty());
    assert_eq!(s.app_simple_mode, Some(false));
    assert!(!s.switch_markers_migrated);
    assert_eq!(st, on(&[Module::DbAccess]));
}

#[test]
fn a_saved_value_is_never_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let mut st = BTreeSet::new();
    let mut s = AppSettings { app_simple_mode: Some(false), ..AppSettings::default() };
    settle_on_start_in(d.path(), true, &mut st, &mut s);
    assert_eq!(s.app_simple_mode, Some(false));
    assert!(!s.switch_markers_migrated);
    assert!(st.is_empty());
}

const SIMPLE_MODE_NOTE: &str =
    "If the app is in Simple mode, do that part yourself or ask the person to turn Simple mode off in Settings.";

fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every guide that sends a person to a screen Simple mode hides says what to
/// do when that screen is not there.
#[test]
fn every_guide_that_names_a_hidden_screen_says_what_to_do_in_simple_mode() {
    let guides = [
        ("autorun/guide.rs", flat(&v2_lib::autorun::guide::autorun_guide())),
        ("api_templates/guide.rs", flat(&v2_lib::api_templates::guide::text(&[], None))),
        ("ai_bridge/guide_routes.rs", flat(include_str!("../../src/ai_bridge/guide_routes.rs"))),
    ];
    for (name, text) in &guides {
        assert!(text.contains(SIMPLE_MODE_NOTE), "{name} does not carry the Simple mode line");
    }
}

/// Simple mode is the person's switch. No assistant tool and no bridge route
/// may set it: the bridge only carries it, read-only, in /current-pbi.
#[test]
fn no_tool_or_route_sets_the_app_simple_mode() {
    let sources = [
        ("mcp.rs", include_str!("../../src/mcp.rs")),
        ("ai_tools.rs", include_str!("../../src/ai_tools.rs")),
        ("ai_bridge/mod.rs", include_str!("../../src/ai_bridge/mod.rs")),
        ("ai_bridge/server.rs", include_str!("../../src/ai_bridge/server.rs")),
        ("ai_bridge/guide_routes.rs", include_str!("../../src/ai_bridge/guide_routes.rs")),
        ("ai_bridge/template_routes.rs", include_str!("../../src/ai_bridge/template_routes.rs")),
    ];
    for (name, src) in sources {
        for (n, line) in src.lines().enumerate() {
            let writes = line.contains("set_app_simple_mode")
                || line.contains("setAppSimpleMode")
                || line.contains("simple_mode::")
                || line.contains("app_simple_mode =")
                || line.contains("app_simple_mode:") && !line.contains("Option<bool>")
                    && !line.contains("switches.app_simple_mode");
            assert!(!writes, "{name}:{}: {line}", n + 1);
        }
    }
}
