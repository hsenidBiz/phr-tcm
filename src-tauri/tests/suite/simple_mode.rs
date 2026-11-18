//! The app-wide Simple mode: its start-up default and the switch-on.

use std::collections::BTreeSet;

use v2_lib::app_settings::{AppSettings, AutorunMode};
use v2_lib::modules::{Module, HIDDEN_UNTIL_UNLOCKED};
use v2_lib::simple_mode::{is_new_install, modules_to_switch_on, settle_on_start_in, settle_simple_mode, switch_all_on_with};

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
    switch_all_on_with(&mut st, &mut s, d.path());
    let modules_before = st.clone();
    // "Off" only changes the saved choice (the command saves it and touches no module).
    s.app_simple_mode = Some(false);
    v2_lib::app_settings::save(d.path(), &s).unwrap();
    assert_eq!(st, modules_before);
    assert_eq!(v2_lib::modules::saved(d.path()).unwrap(), modules_before);
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
