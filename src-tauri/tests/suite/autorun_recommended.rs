//! Auto Run's mode and what the first switch-on turns on.

use std::collections::BTreeSet;

use v2_lib::app_settings::{AppSettings, AutorunMode};
use v2_lib::autorun::recommended::{autorun_dir_has_files, recommended_on_switch, settle_ever_on, settle_mode_on_upgrade};
use v2_lib::commands::modules::modules_set_with;
use v2_lib::modules::Module;

fn on(ms: &[Module]) -> BTreeSet<Module> {
    ms.iter().copied().collect()
}

#[test]
fn an_install_with_auto_run_on_and_no_mode_settles_to_advanced() {
    assert_eq!(settle_mode_on_upgrade(None, true), Some(AutorunMode::Advanced));
}

#[test]
fn no_mode_and_auto_run_off_stays_unpicked() {
    assert_eq!(settle_mode_on_upgrade(None, false), None);
}

#[test]
fn a_picked_mode_is_kept_on_upgrade() {
    assert_eq!(settle_mode_on_upgrade(Some(AutorunMode::Simple), true), Some(AutorunMode::Simple));
}

#[test]
fn an_install_with_auto_run_on_settles_to_advanced_and_gets_nothing_switched_on() {
    let r = recommended_on_switch(true, &AppSettings::default());
    assert!(!r.api_templates && !r.video && !r.network_tools && !r.mode_simple);
}

#[test]
fn a_machine_that_never_used_auto_run_gets_everything() {
    let r = recommended_on_switch(false, &AppSettings::default());
    assert!(r.api_templates && r.video && r.network_tools && r.mode_simple);
}

#[test]
fn a_person_set_switch_survives_auto_run_off_and_on() {
    let s = AppSettings { api_templates_set: true, autorun_failed_video_set: true, ..AppSettings::default() };
    let r = recommended_on_switch(false, &s);
    assert!(!r.api_templates);
    assert!(!r.video);
}

#[test]
fn a_picked_mode_is_never_overwritten() {
    let s = AppSettings { autorun_mode: Some(AutorunMode::Advanced), ..AppSettings::default() };
    assert!(!recommended_on_switch(false, &s).mode_simple);
}

#[test]
fn a_folder_holding_only_empty_directories_is_not_use() {
    let d = tempfile::tempdir().unwrap();
    assert!(!autorun_dir_has_files(&d.path().join("autorun")));
    std::fs::create_dir_all(d.path().join("autorun").join("envs")).unwrap();
    assert!(!autorun_dir_has_files(&d.path().join("autorun")));
}

#[test]
fn a_file_anywhere_under_the_folder_is_use() {
    let d = tempfile::tempdir().unwrap();
    let deep = d.path().join("autorun").join("envs").join("one");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("script.json"), "{}").unwrap();
    assert!(autorun_dir_has_files(&d.path().join("autorun")));
}

#[test]
fn the_start_up_settle_with_auto_run_on_says_ever_on() {
    assert!(settle_ever_on(false, true, false));
    assert!(settle_ever_on(false, false, true));
    assert!(!settle_ever_on(false, false, false));
    assert!(settle_ever_on(true, false, false), "once true it never goes back");
}

#[test]
fn switching_auto_run_on_for_the_first_time_turns_the_recommended_features_on() {
    let d = tempfile::tempdir().unwrap();
    let mut st = on(&[Module::DbAccess]);
    let mut s = AppSettings::default();
    let a = modules_set_with(&mut st, &mut s, d.path(), Module::AutoRun, true, false, false, false).unwrap();
    assert!(a.on.contains(&Module::ApiTemplates));
    assert!(s.autorun_failed_video);
    assert_eq!(s.autorun_mode, Some(AutorunMode::Simple));
    let r = a.recommended.unwrap();
    assert!(r.api_templates && r.video && r.network_tools && r.mode_simple);
    // Saved, not only held.
    assert_eq!(v2_lib::app_settings::load(d.path()).autorun_mode, Some(AutorunMode::Simple));
    assert!(s.auto_run_ever_on);
    assert!(v2_lib::app_settings::load(d.path()).auto_run_ever_on);

    // A second off-to-on gets nothing.
    st.remove(&Module::AutoRun);
    let b = modules_set_with(&mut st, &mut s, d.path(), Module::AutoRun, true, false, false, false).unwrap();
    assert_eq!(b.recommended, Some(Default::default()));
}

#[test]
fn switching_auto_run_on_where_it_was_used_before_turns_nothing_on() {
    let d = tempfile::tempdir().unwrap();
    let mut st = on(&[Module::DbAccess]);
    let mut s = AppSettings { auto_run_ever_on: true, ..AppSettings::default() };
    let a = modules_set_with(&mut st, &mut s, d.path(), Module::AutoRun, true, false, false, false).unwrap();
    assert!(!a.on.contains(&Module::ApiTemplates));
    assert!(!s.autorun_failed_video);
    assert_eq!(s.autorun_mode, None);
    assert_eq!(a.recommended, Some(Default::default()));
}

#[test]
fn recommended_is_none_when_auto_run_was_already_on() {
    let d = tempfile::tempdir().unwrap();
    let mut st = on(&[Module::DbAccess, Module::AutoRun]);
    let mut s = AppSettings::default();
    let a = modules_set_with(&mut st, &mut s, d.path(), Module::Playwright, true, false, false, false).unwrap();
    assert_eq!(a.recommended, None);
    assert_eq!(s.autorun_mode, None);
}

#[test]
fn switching_api_templates_yourself_is_remembered_either_way() {
    let d = tempfile::tempdir().unwrap();
    let mut st = on(&[Module::DbAccess, Module::AutoRun, Module::ApiTemplates]);
    let mut s = AppSettings::default();
    modules_set_with(&mut st, &mut s, d.path(), Module::ApiTemplates, false, false, false, false).unwrap();
    assert!(s.api_templates_set);
}
