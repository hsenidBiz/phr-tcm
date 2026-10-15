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

#[test]
fn any_earlier_app_data_makes_an_existing_install() {
    for marker in ["extras.json", "app-settings.json", "cache.json", "databases.json", "writing-style.json"] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(marker), "{}").unwrap();
        assert!(v2_lib::modules::existing_install(d.path()), "{marker}");
    }
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("autorun")).unwrap();
    assert!(v2_lib::modules::existing_install(d.path()));
}

#[test]
fn a_dir_with_only_a_cache_keeps_database_access_and_writing_style() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("cache.json"), "{}").unwrap();
    assert_eq!(load_or_create(d.path(), false), set(&[Module::DbAccess, Module::WritingStyle]));
}

#[test]
fn an_empty_dir_is_a_fresh_install() {
    let d = tempfile::tempdir().unwrap();
    assert!(!v2_lib::modules::existing_install(d.path()));
    assert!(load_or_create(d.path(), false).is_empty());
}

#[test]
fn a_broken_tree_is_mended_on_load() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("modules.json"), r#"{"on":["playwright","auto_run"]}"#).unwrap();
    assert!(load_or_create(d.path(), false).is_empty());
    std::fs::write(d.path().join("modules.json"), r#"{"on":["playwright","auto_run","db_access","writing_style"]}"#).unwrap();
    assert_eq!(load_or_create(d.path(), false).len(), 4);
}

/// A switch whose save fails changes nothing and says why: the pure half
/// of `set`, on a local state, so it runs whatever this process's own
/// choice is and never touches it.
#[test]
fn a_set_whose_save_fails_leaves_the_state_unchanged() {
    let d = tempfile::tempdir().unwrap();
    // A directory where the file goes: the save cannot succeed.
    std::fs::create_dir(d.path().join("modules.json")).unwrap();
    let mut state: BTreeSet<Module> = Module::ALL.into_iter().collect();
    let before = state.clone();
    let r = v2_lib::modules::set_in(&mut state, d.path(), Module::WritingStyle, false);
    assert!(r.is_err(), "{r:?}");
    assert_eq!(state, before, "a switch that was not saved is not made");
}

/// The same switch into a dir that can take it is made and saved.
#[test]
fn a_set_that_saves_changes_the_state_and_the_file() {
    let d = tempfile::tempdir().unwrap();
    let mut state: BTreeSet<Module> = Module::ALL.into_iter().collect();
    let next = v2_lib::modules::set_in(&mut state, d.path(), Module::AutoRun, false).unwrap();
    assert_eq!(next, set(&[Module::DbAccess, Module::WritingStyle]));
    assert_eq!(state, next);
    assert_eq!(load_or_create(d.path(), false), next);
    // Locked: refused, nothing changed.
    let err = v2_lib::modules::set_in(&mut state, d.path(), Module::Playwright, true).unwrap_err();
    assert_eq!(err, "\"Playwright\" needs \"Auto Run\" switched on first.");
    assert_eq!(state, next);
}

// ---- every gate asks its module ------------------------------------------

/// Before setup has run (the test binary, the `--mcp` proxy), a development
/// build reads every module on and a release build none.
#[test]
fn before_setup_a_development_build_reads_everything_on() {
    assert_eq!(v2_lib::modules::before_setup(true), Module::ALL.into_iter().collect::<BTreeSet<_>>());
    assert!(v2_lib::modules::before_setup(false).is_empty());
}

/// The stored names round-trip, and an unknown one is no module.
#[test]
fn each_module_has_its_stored_name() {
    let keys: Vec<&str> = Module::ALL.iter().map(|m| m.key()).collect();
    assert_eq!(keys, ["db_access", "writing_style", "auto_run", "api_templates", "playwright"]);
    for m in Module::ALL {
        assert_eq!(Module::from_key(m.key()), Some(m));
        assert_eq!(serde_json::to_value(m).unwrap(), serde_json::json!(m.key()), "serde agrees");
    }
    assert_eq!(Module::from_key("advanced"), None);
}

/// Every gate refuses with the same sentence for its module.
#[test]
fn the_refusal_names_the_module() {
    for (m, said) in [
        (Module::DbAccess, "Database Access is switched off in Modules"),
        (Module::WritingStyle, "Writing Style is switched off in Modules"),
        (Module::AutoRun, "Auto Run is switched off in Modules"),
        (Module::ApiTemplates, "API Templates is switched off in Modules"),
        (Module::Playwright, "Playwright is switched off in Modules"),
    ] {
        assert_eq!(m.off_sentence(), said);
        assert_eq!(v2_lib::modules::require(m, false).unwrap_err(), said);
        assert!(v2_lib::modules::require(m, true).is_ok());
    }
    assert_eq!(v2_lib::pw_repo::NOT_OFFERED, Module::Playwright.off_sentence());
}

/// Each IPC command group refuses while its module is off, with that
/// module's sentence, and touches nothing.
#[test]
fn each_ipc_command_group_refuses_while_its_module_is_off() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        v2_lib::commands::api_templates::remove_flow_at(false, root.path(), "acme", "Web", "flow-1").unwrap_err(),
        Module::ApiTemplates.off_sentence()
    );
    assert_eq!(
        v2_lib::commands::autorun::download_sizes_at(false, root.path(), "run-1").unwrap_err(),
        Module::AutoRun.off_sentence()
    );
    assert_eq!(
        v2_lib::commands::test_files::list_at(false, root.path(), "acme", "Web").unwrap_err(),
        Module::AutoRun.off_sentence()
    );
    assert_eq!(v2_lib::pw_repo::ensure_offered(false).unwrap_err(), Module::Playwright.off_sentence());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0, "nothing was touched");
}

/// Each IPC command reads its own module, never another's or the old flag.
#[test]
fn each_ipc_command_reads_its_own_module() {
    let read = |p: &str| std::fs::read_to_string(format!("{}/src/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    for (file, module) in [
        ("commands/api_templates.rs", "Module::ApiTemplates"),
        ("commands/autorun.rs", "Module::AutoRun"),
        ("commands/test_files.rs", "Module::AutoRun"),
        ("commands/autorun_replay.rs", "Module::AutoRun"),
        ("commands/pw_repo.rs", "Module::Playwright"),
        ("commands/pw_export.rs", "Module::Playwright"),
    ] {
        let src = read(file);
        assert!(src.contains(module), "{file} does not ask {module}");
        assert!(!src.contains("autorun_offered") && !src.contains("features_on"), "{file} still reads the old gate");
    }
    let export = read("commands/pw_export.rs");
    assert_eq!(export.matches("ensure_offered()?;").count(), 3, "every Export to Playwright command is gated");
}

/// The writing guide carries the person's style only while Writing Style
/// is on: off, it is the standard guide whatever is saved.
#[test]
fn the_writing_guide_drops_the_custom_style_with_writing_style_off() {
    let _style = crate::serial::writing_style();
    let d = tempfile::tempdir().unwrap();
    v2_lib::writing_style::set_dir(Some(d.path().to_path_buf()));
    let saved = v2_lib::writing_style::WritingStyle { enabled: true, text: "Name every screen in bold.".into() };
    v2_lib::writing_style::save(d.path(), &saved).unwrap();
    let on = v2_lib::ai_bridge::custom_style_when(true);
    let off = v2_lib::ai_bridge::custom_style_when(false);
    v2_lib::writing_style::set_dir(None);
    assert!(on.expect("on, the style is carried").contains("Name every screen in bold."));
    assert_eq!(off, None, "off, the guide is the standard one");
    let bridge = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ai_bridge.rs")).unwrap();
    assert!(
        bridge.contains("custom_style_when(crate::modules::on(crate::modules::Module::WritingStyle))"),
        "the guide asks Writing Style"
    );
}

/// Preconditions are read only while the AI Bridge tab's Database Read
/// Access switch is on AND the Database Access module is on.
#[test]
fn preconditions_need_database_access_as_well_as_the_switch() {
    use v2_lib::autorun::preconditions::read_access_with;
    assert!(read_access_with(true, true));
    assert!(!read_access_with(true, false), "Database Access off");
    assert!(!read_access_with(false, true), "the switch off");
    assert!(!read_access_with(false, false));
    // Every entry point that takes the switch passes it through the module.
    let read = |p: &str| std::fs::read_to_string(format!("{}/src/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    assert_eq!(read("commands/autorun.rs").matches("preconditions::read_access(db_read_access)").count(), 2);
    assert_eq!(read("commands/autorun_replay.rs").matches("preconditions::read_access(db_read_access)").count(), 1);
    assert!(read("ai_bridge.rs").contains("crate::autorun::preconditions::read_access(!ctx.disabled_tools"));
}
