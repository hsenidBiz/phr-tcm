//! Auto Run's mode and what the first switch-on turns on.

use std::collections::BTreeSet;

use v2_lib::app_settings::{AppSettings, AutorunMode};
use v2_lib::autorun::recommended::{auto_run_used_before, recommended_on_switch, settle_mode_on_upgrade};
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
fn auto_run_used_before_looks_for_the_autorun_folder() {
    let d = tempfile::tempdir().unwrap();
    assert!(!auto_run_used_before(d.path()));
    std::fs::write(d.path().join("cache.json"), "{}").unwrap();
    assert!(!auto_run_used_before(d.path()));
    std::fs::create_dir(d.path().join("autorun")).unwrap();
    assert!(auto_run_used_before(d.path()));
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
}

#[test]
fn switching_auto_run_on_where_it_was_used_before_turns_nothing_on() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("autorun")).unwrap();
    let mut st = on(&[Module::DbAccess]);
    let mut s = AppSettings::default();
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
