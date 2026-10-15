//! The module tree and its stored choice.

use std::collections::BTreeSet;
use v2_lib::modules::{apply, initial, load_or_create, save, Module};

fn set(ms: &[Module]) -> BTreeSet<Module> {
    ms.iter().copied().collect()
}

#[test]
fn the_tree_has_the_designed_parents() {
    assert_eq!(Module::DbAccess.parent(), None);
    assert_eq!(Module::WritingStyle.parent(), None);
    assert_eq!(Module::AutoRun.parent(), Some(Module::DbAccess));
    assert_eq!(Module::ApiTemplates.parent(), Some(Module::AutoRun));
    assert_eq!(Module::Playwright.parent(), Some(Module::AutoRun));
    assert_eq!(Module::DbAccess.below(), vec![Module::AutoRun, Module::ApiTemplates, Module::Playwright]);
    assert_eq!(Module::AutoRun.children(), vec![Module::ApiTemplates, Module::Playwright]);
    assert_eq!(Module::ApiTemplates.name(), "API Templates");
}

#[test]
fn a_locked_module_cannot_be_switched_on() {
    let err = apply(&set(&[]), Module::AutoRun, true).unwrap_err();
    assert_eq!(err, "\"Auto Run\" needs \"Database Access\" switched on first.");
    assert!(apply(&set(&[Module::DbAccess]), Module::AutoRun, true).unwrap().contains(&Module::AutoRun));
}

#[test]
fn switching_a_parent_off_switches_everything_below_it_off() {
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    let next = apply(&all, Module::DbAccess, false).unwrap();
    assert_eq!(next, set(&[Module::WritingStyle]));
}

#[test]
fn a_fresh_install_starts_with_nothing_on() {
    assert!(initial(false, true, true, false).is_empty());
}

#[test]
fn an_existing_install_keeps_database_access_and_writing_style() {
    assert_eq!(initial(true, false, false, false), set(&[Module::DbAccess, Module::WritingStyle]));
}

#[test]
fn an_advanced_or_unlocked_install_keeps_auto_run_api_templates_and_playwright() {
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    assert_eq!(initial(true, true, false, false), all);
    assert_eq!(initial(true, false, true, false), all);
}

#[test]
fn a_dev_build_starts_with_everything_on() {
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    assert_eq!(initial(false, false, false, true), all);
}

#[test]
fn an_unreadable_file_on_an_existing_install_is_rebuilt_from_the_old_flags() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("extras.json"), r#"{"advanced":true}"#).unwrap();
    std::fs::write(d.path().join("modules.json"), "not json").unwrap();
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    assert_eq!(load_or_create(d.path(), false), all);
    // and it was saved, so the next read is the file
    assert_eq!(load_or_create(d.path(), false), all);
}

#[test]
fn the_choice_round_trips_through_modules_json() {
    let d = tempfile::tempdir().unwrap();
    let on = set(&[Module::DbAccess, Module::AutoRun, Module::Playwright]);
    save(d.path(), &on).unwrap();
    assert_eq!(load_or_create(d.path(), false), on);
    let text = std::fs::read_to_string(d.path().join("modules.json")).unwrap();
    assert!(text.contains("db_access") && text.contains("auto_run") && text.contains("playwright"));
}
