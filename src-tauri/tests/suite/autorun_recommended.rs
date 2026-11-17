//! Auto Run's mode and what the first switch-on turns on.

use std::collections::BTreeSet;

use v2_lib::app_settings::{AppSettings, AutorunMode};
use v2_lib::autorun::recommended::{autorun_has_own_work, recommended_on_switch, settle_ever_on, settle_mode_on_upgrade, settled_on_start};
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

fn put(path: &std::path::Path, name: &str) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join(name), "{}").unwrap();
}

#[test]
fn files_that_are_written_whether_or_not_auto_run_was_used_are_not_use() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("autorun");
    assert!(!autorun_has_own_work(&a));
    std::fs::create_dir_all(a.join("envs").join("env-0a1b2c3d").join("scripts")).unwrap();
    assert!(!autorun_has_own_work(&a), "empty folders");
    std::fs::write(a.join("environments.json"), "{}").unwrap();
    put(&a.join("proposals"), "p.json");
    put(&a.join("last-fill"), "f.json");
    put(&a, "migration-note.txt");
    put(&a.join("envs").join("env-0a1b2c3d"), "orders.json");
    put(&a.join("envs").join("_seed").join("scripts"), "s.json");
    assert!(!autorun_has_own_work(&a));
}

#[test]
fn a_saved_script_or_a_run_record_in_an_environment_is_use() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("autorun");
    put(&a.join("envs").join("env-0a1b2c3d").join("scripts"), "case-1.json");
    assert!(autorun_has_own_work(&a));
    let d2 = tempfile::tempdir().unwrap();
    let a2 = d2.path().join("autorun");
    put(&a2.join("envs").join("env-0a1b2c3d").join("runs"), "run-1.json");
    assert!(autorun_has_own_work(&a2));
}

#[test]
fn a_script_in_the_older_flat_layout_is_use() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("autorun");
    put(&a.join("scripts"), "case-1.json");
    assert!(autorun_has_own_work(&a));
}

#[test]
fn the_start_up_settle_with_auto_run_off_and_only_the_environments_file_stays_not_ever_on() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("autorun");
    std::fs::create_dir_all(a.join("envs")).unwrap();
    std::fs::write(a.join("environments.json"), "{}").unwrap();
    let mut s = AppSettings::default();
    for _ in 0..2 {
        let (mode, ever) = settled_on_start(&s, false, &a);
        assert_eq!(mode, None);
        assert!(!ever);
        s.autorun_mode = mode;
        s.auto_run_ever_on = ever;
    }
    put(&a.join("envs").join("env-0a1b2c3d").join("scripts"), "case-1.json");
    assert!(settled_on_start(&s, false, &a).1, "a saved script is use");
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
