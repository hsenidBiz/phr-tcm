//! The module tree and its stored choice.

use std::collections::BTreeSet;
use v2_lib::modules::{apply, initial, load_or_create, save, saved, Module};

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
    assert_eq!(Module::CrystalReports.parent(), None);
    assert_eq!(Module::DbAccess.below(), vec![Module::AutoRun, Module::ApiTemplates, Module::Playwright]);
    assert_eq!(Module::AutoRun.children(), vec![Module::ApiTemplates, Module::Playwright]);
    assert_eq!(Module::ApiTemplates.name(), "API Templates");
    assert_eq!(Module::CrystalReports.name(), "Crystal Reports");
    assert_eq!(Module::ALL.last(), Some(&Module::CrystalReports));
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
    assert_eq!(next, set(&[Module::WritingStyle, Module::CrystalReports]));
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
    let all: BTreeSet<Module> = set(&[Module::DbAccess, Module::WritingStyle, Module::AutoRun, Module::ApiTemplates, Module::Playwright]);
    assert_eq!(initial(true, true, false, false), all);
    assert_eq!(initial(true, false, true, false), all);
}

/// Crystal Reports is new: no install gets it from an old flag, so it is on
/// only in a development build until a person switches it on themselves.
#[test]
fn crystal_reports_starts_off_everywhere_but_a_development_build() {
    for (existing, advanced, unlocked) in [(false, false, false), (true, false, false), (true, true, false), (true, false, true), (true, true, true)] {
        assert!(!initial(existing, advanced, unlocked, false).contains(&Module::CrystalReports), "{existing} {advanced} {unlocked}");
    }
    assert!(initial(false, false, false, true).contains(&Module::CrystalReports));
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
    let all = set(&[Module::DbAccess, Module::WritingStyle, Module::AutoRun, Module::ApiTemplates, Module::Playwright]);
    assert_eq!(load_or_create(d.path(), false), all);
    // and it was saved, so the next read is the file
    assert_eq!(load_or_create(d.path(), false), all);
}

/// A newer app's module this one does not know is dropped; every module it
/// does know stays - the file is not thrown away and rebuilt from old flags.
#[test]
fn a_file_naming_an_unknown_module_keeps_every_known_one() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("modules.json"), r#"{"on":["db_access","future_thing"]}"#).unwrap();
    assert_eq!(load_or_create(d.path(), false), set(&[Module::DbAccess]));
}

/// What the `--mcp` proxy reads when the app cannot be asked: the saved
/// file, mended; nothing when there is none or it cannot be read.
#[test]
fn the_saved_choice_is_read_without_creating_anything() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(saved(d.path()), None);
    assert!(!d.path().join("modules.json").exists(), "reading must not write");
    std::fs::write(d.path().join("modules.json"), "not json").unwrap();
    assert_eq!(saved(d.path()), None);
    std::fs::write(d.path().join("modules.json"), r#"{"on":["auto_run","writing_style"]}"#).unwrap();
    assert_eq!(saved(d.path()), Some(set(&[Module::WritingStyle])), "mended: auto_run needs db_access");
}

/// The proxy finds the file where the app writes it: Tauri's app data dir
/// is the roaming app-data folder plus the bundle identifier.
#[test]
fn the_proxy_looks_in_the_apps_data_folder() {
    let conf = include_str!("../../tauri.conf.json");
    let v: serde_json::Value = serde_json::from_str(conf).unwrap();
    assert_eq!(v["identifier"].as_str().unwrap(), v2_lib::modules::APP_IDENTIFIER);
    let dir = v2_lib::modules::data_dir_in(Some(std::ffi::OsString::from("C:/Roaming"))).unwrap();
    assert!(dir.ends_with(v2_lib::modules::APP_IDENTIFIER), "{dir:?}");
    assert_eq!(v2_lib::modules::data_dir_in(None), None);
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
    assert_eq!(next, set(&[Module::DbAccess, Module::WritingStyle, Module::CrystalReports]));
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
    assert_eq!(keys, ["db_access", "writing_style", "auto_run", "api_templates", "playwright", "crystal_reports"]);
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

/// While the held-back module is locked no sentence names it; unlocked it
/// is refused like any other module. Every other module reads the same
/// either way.
#[test]
fn the_held_back_module_is_not_named_while_locked() {
    let m = Module::CrystalReports;
    assert_eq!(m.off_sentence_for(false), "That module is not available on this machine.");
    assert_eq!(m.off_sentence_for(false), v2_lib::modules::NOT_AVAILABLE);
    assert!(!m.off_sentence_for(false).contains("Crystal"));
    assert_eq!(m.off_sentence_for(true), "Crystal Reports is switched off in Modules");
    for other in Module::ALL.into_iter().filter(|o| *o != m) {
        assert_eq!(other.off_sentence_for(false), other.off_sentence_for(true));
    }
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
    assert_eq!(export.matches("ensure_offered()?;").count(), 5, "every Export to Playwright command is gated");
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

mod commands {
    use super::set;
    use v2_lib::commands::modules::{modules_set_with, AUTO_RUN_BROWSER_OPEN, AUTO_RUN_BUSY, PLAYWRIGHT_BUSY, SAVE_FAILED};
    use v2_lib::modules::Module;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn a_busy_run_refuses_switching_auto_run_off() {
        let d = dir();
        let mut st = set(&[Module::DbAccess, Module::AutoRun]);
        let err = modules_set_with(&mut st, d.path(), Module::AutoRun, false, true, false, false).unwrap_err();
        assert_eq!(err, AUTO_RUN_BUSY);
        assert!(st.contains(&Module::AutoRun));
    }

    #[test]
    fn a_busy_repo_command_refuses_switching_playwright_off() {
        let d = dir();
        let mut st = set(&[Module::DbAccess, Module::AutoRun, Module::Playwright]);
        let err = modules_set_with(&mut st, d.path(), Module::Playwright, false, false, false, true).unwrap_err();
        assert_eq!(err, PLAYWRIGHT_BUSY);
        assert!(st.contains(&Module::Playwright));
    }

    #[test]
    fn a_cascade_that_would_switch_off_a_busy_auto_run_is_refused() {
        let d = dir();
        let mut st = set(&[Module::DbAccess, Module::AutoRun]);
        let err = modules_set_with(&mut st, d.path(), Module::DbAccess, false, true, false, false).unwrap_err();
        assert_eq!(err, AUTO_RUN_BUSY);
        assert_eq!(st, set(&[Module::DbAccess, Module::AutoRun]));
    }

    #[test]
    fn busy_checks_do_not_block_switching_on() {
        let d = dir();
        let mut st = set(&[Module::DbAccess]);
        let now = modules_set_with(&mut st, d.path(), Module::AutoRun, true, true, false, true).unwrap();
        assert_eq!(now, vec![Module::DbAccess, Module::AutoRun]);
    }

    #[test]
    fn a_locked_module_is_refused() {
        let d = dir();
        let mut st = set(&[]);
        let err = modules_set_with(&mut st, d.path(), Module::AutoRun, true, false, false, false).unwrap_err();
        assert_eq!(err, "\"Auto Run\" needs \"Database Access\" switched on first.");
    }

    #[test]
    fn a_cascade_answers_with_what_is_left_on() {
        let d = dir();
        let all: Vec<Module> = Module::ALL.to_vec();
        let mut st = set(&all);
        let now = modules_set_with(&mut st, d.path(), Module::DbAccess, false, false, false, false).unwrap();
        assert_eq!(now, vec![Module::WritingStyle, Module::CrystalReports]);
        assert_eq!(st, set(&[Module::WritingStyle, Module::CrystalReports]));
    }

    #[test]
    fn an_open_auto_run_browser_refuses_switching_auto_run_off() {
        let d = dir();
        let mut st = set(&[Module::DbAccess, Module::AutoRun]);
        let err = modules_set_with(&mut st, d.path(), Module::AutoRun, false, false, true, false).unwrap_err();
        assert_eq!(err, "Auto Run can't be switched off while its browser is open - close it first.");
        assert_eq!(err, AUTO_RUN_BROWSER_OPEN);
        // by cascade too, and switching on is never blocked by it
        let err = modules_set_with(&mut st, d.path(), Module::DbAccess, false, false, true, false).unwrap_err();
        assert_eq!(err, AUTO_RUN_BROWSER_OPEN);
        assert_eq!(st, set(&[Module::DbAccess, Module::AutoRun]));
        // Auto Run already off: nothing for the browser to block
        let mut off = set(&[Module::DbAccess]);
        assert!(modules_set_with(&mut off, d.path(), Module::DbAccess, false, false, true, false).is_ok());
    }

    /// The states behind `auto_run_browser_open`: a setup browser (a lease
    /// that only a person closes) and a held API template browser.
    #[test]
    fn the_browser_states_are_read_from_their_owners() {
        use v2_lib::autorun::lease::{try_acquire, Holder};
        let _h = crate::serial::held_browsers();
        let _g = crate::serial::account_leases();
        assert!(!v2_lib::commands::modules::auto_run_browser_open());
        let lease = try_acquire("env-modules-test", "acct", Holder::Setup).unwrap();
        assert!(v2_lib::commands::modules::auto_run_browser_open(), "a setup browser is open");
        drop(lease);
        assert!(!v2_lib::commands::modules::auto_run_browser_open());
        let lease = try_acquire("env-modules-test", "acct", Holder::Browser { slot: 0 }).unwrap();
        assert!(v2_lib::commands::modules::auto_run_browser_open(), "the supervised browser holds an account");
        drop(lease);
        // A run's own lease (a case) is the run, not an open browser.
        let lease = try_acquire("env-modules-test", "acct", Holder::Case { run: "r".into() }).unwrap();
        assert!(!v2_lib::commands::modules::auto_run_browser_open());
        drop(lease);
    }

    #[test]
    fn a_failed_save_says_so_without_a_path() {
        // A file where the data folder should be: the save cannot work.
        let d = dir();
        let file = d.path().join("blocked");
        std::fs::write(&file, "x").unwrap();
        let mut st = set(&[Module::DbAccess]);
        let err = modules_set_with(&mut st, &file, Module::WritingStyle, true, false, false, false).unwrap_err();
        assert_eq!(err, SAVE_FAILED);
        assert!(!err.contains(['/', char::from(92), ':']) && !err.contains("blocked"), "{err}");
        assert_eq!(st, set(&[Module::DbAccess]), "and nothing was changed");
        // a refusal from the tree is still its own sentence
        let err = modules_set_with(&mut st, &file, Module::Playwright, true, false, false, false).unwrap_err();
        assert!(err.contains("needs"), "{err}");
    }
}

// ---- the module held back until this machine unlocks it -------------------

#[test]
fn a_locked_machine_is_not_offered_the_held_back_module_and_nothing_else_changes() {
    use v2_lib::modules::effective;
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    let locked = effective(&all, true);
    assert_eq!(locked, all, "unlocked keeps every module");
    let mut expect = all.clone();
    expect.remove(&Module::CrystalReports);
    assert_eq!(effective(&all, false), expect, "locked drops only that module");
    let others = set(&[Module::DbAccess, Module::AutoRun]);
    assert_eq!(effective(&others, false), others);
    assert!(effective(&BTreeSet::new(), false).is_empty());
}

#[test]
fn switching_the_held_back_module_on_while_locked_is_refused_and_saves_nothing() {
    use v2_lib::modules::{refuse_if_locked, set_in};
    let d = tempfile::tempdir().unwrap();
    let mut state = set(&[Module::DbAccess]);
    save(d.path(), &state).unwrap();
    let before = std::fs::read_to_string(d.path().join("modules.json")).unwrap();
    // The refusal comes before any save, as `set` orders it.
    let err = refuse_if_locked(Module::CrystalReports, true, false).unwrap_err();
    assert_eq!(err, "That module is not available on this machine.");
    assert_eq!(std::fs::read_to_string(d.path().join("modules.json")).unwrap(), before);
    assert_eq!(state, set(&[Module::DbAccess]));
    // Unlocked, or switching it off, or any other module: not refused.
    assert!(refuse_if_locked(Module::CrystalReports, true, true).is_ok());
    assert!(refuse_if_locked(Module::CrystalReports, false, false).is_ok());
    assert!(refuse_if_locked(Module::AutoRun, true, false).is_ok());
    assert!(set_in(&mut state, d.path(), Module::CrystalReports, true).is_ok());
}

#[test]
fn a_saved_on_choice_survives_locking_and_unlocking() {
    use v2_lib::modules::effective;
    let d = tempfile::tempdir().unwrap();
    let saved_on = set(&[Module::DbAccess, Module::CrystalReports]);
    save(d.path(), &saved_on).unwrap();
    let on_disk = load_or_create(d.path(), false);
    assert!(!effective(&on_disk, false).contains(&Module::CrystalReports));
    assert_eq!(load_or_create(d.path(), false), saved_on, "the saved choice is untouched while locked");
    assert!(effective(&on_disk, true).contains(&Module::CrystalReports));
}
