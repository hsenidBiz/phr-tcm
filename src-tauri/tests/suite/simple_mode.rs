//! The app-wide Simple mode: its start-up default and the switch-on.

use std::collections::BTreeSet;

use v2_lib::app_settings::{AppSettings, AutorunMode};
use v2_lib::modules::{Module, HIDDEN_UNTIL_UNLOCKED};
use v2_lib::simple_mode::{is_new_install, set_autorun_mode_with, SIMPLE_NO_ADVANCED, modules_to_switch_on, settle_on_start_in, settle_simple_mode, set_app_simple_mode_with, switch_all_on_with};

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
    // Auto Test's recommended features, on a machine with no Auto Test work.
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

/// The plan's line, for a part the assistant can do itself; and the form for a
/// part only the person can do (an import, removing a template).
const NOTE_FULL: &str =
    "If the app is in Simple mode, do that part yourself or ask the person to turn Simple mode off in Settings.";
const NOTE_ASK: &str = "If the app is in Simple mode, ask the person to turn Simple mode off in Settings.";

fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn src_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every file under `dir` with one of `exts`.
fn files_under(dir: &std::path::Path, exts: &[&str], out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files_under(&p, exts, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)) {
            out.push(p);
        }
    }
}

/// The text of a source file without its `//` comment lines.
fn without_comments(src: &str) -> String {
    src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

/// Every guide text the assistant receives: the guide sources, the tool
/// descriptions, the bridge's route modules and the role files. Wherever one
/// names a screen Simple mode hides, it also says what to do when that screen
/// is not there. (A tool name such as `list_api_templates` is not a screen.)
#[test]
fn every_guide_that_names_a_hidden_screen_says_what_to_do_in_simple_mode() {
    const SCREENS: [&str; 7] = [
        "Playwright tab",
        "Import Test Cases",
        "Update Test Cases",
        "Search Suites",
        "Suite Management",
        "API Templates",
        "Settings, Modules",
    ];
    let root = src_dir();
    let mut files = vec![
        root.join("autorun/guide.rs"),
        root.join("api_templates/guide.rs"),
        root.join("writing_style.rs"),
        root.join("ai_tools.rs"),
        root.join("mcp.rs"),
    ];
    files_under(&root.join("ai_bridge"), &["rs"], &mut files);
    files_under(&root.join("autorun/roles"), &["md"], &mut files);

    let mut named = 0;
    for f in &files {
        let raw = std::fs::read_to_string(f).unwrap();
        let text = flat(&if f.extension().unwrap() == "rs" { without_comments(&raw) } else { raw });
        let Some(screen) = SCREENS.iter().find(|s| text.contains(**s)) else { continue };
        named += 1;
        assert!(
            text.contains(NOTE_FULL) || text.contains(NOTE_ASK),
            "{} names {screen} but has no Simple mode line",
            f.display()
        );
    }
    assert!(named >= 4, "the scan found only {named} guide sources naming a hidden screen");

    // The two forms are used where they fit: a part only the person can do
    // never tells the assistant to do it itself.
    for rel in ["ai_bridge/guide_routes.rs", "api_templates/guide.rs", "writing_style.rs"] {
        let text = flat(&std::fs::read_to_string(root.join(rel)).unwrap());
        assert!(!text.contains(NOTE_FULL), "{rel} tells the assistant to do the person's part");
        assert!(text.contains(NOTE_ASK), "{rel} lacks the ask form");
    }
}

/// Simple mode is the person's switch. No assistant tool and no bridge route
/// may set it: the bridge only carries it, read-only, in /current-pbi.
#[test]
fn no_tool_or_route_sets_the_app_simple_mode() {
    let root = src_dir();
    let mut files = vec![root.join("mcp.rs"), root.join("ai_tools.rs")];
    files_under(&root.join("ai_bridge"), &["rs"], &mut files);
    assert!(files.len() > 20, "the scan found only {} files", files.len());
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        for (n, line) in src.lines().enumerate() {
            let writes = line.contains("set_app_simple_mode")
                || line.contains("setAppSimpleMode")
                || line.contains("simple_mode::")
                || line.contains("app_simple_mode =")
                || line.contains("app_simple_mode:") && !line.contains("Option<bool>")
                    && !line.contains("switches.app_simple_mode");
            assert!(!writes, "{}:{}: {line}", f.display(), n + 1);
        }
    }
}

#[test]
fn turning_simple_mode_on_holds_auto_run_in_simple_even_with_the_modules_already_on() {
    let d = tempfile::tempdir().unwrap();
    let mut st: BTreeSet<Module> = Module::ALL.into_iter().filter(|m| *m != HIDDEN_UNTIL_UNLOCKED).collect();
    let mut s = AppSettings { autorun_mode: Some(AutorunMode::Advanced), ..AppSettings::default() };
    let a = set_app_simple_mode_with(&mut st, &mut s, d.path(), true).unwrap();
    assert_eq!(a.settings.autorun_mode, Some(AutorunMode::Simple));
    assert_eq!(v2_lib::app_settings::load(d.path()).autorun_mode, Some(AutorunMode::Simple));
}

#[test]
fn a_new_install_starts_with_auto_run_in_simple() {
    let d = tempfile::tempdir().unwrap();
    let mut st = BTreeSet::new();
    let mut s = AppSettings { autorun_mode: Some(AutorunMode::Advanced), ..AppSettings::default() };
    settle_on_start_in(d.path(), true, &mut st, &mut s);
    assert_eq!(s.autorun_mode, Some(AutorunMode::Simple));
    assert_eq!(v2_lib::app_settings::load(d.path()).autorun_mode, Some(AutorunMode::Simple));
}

#[test]
fn an_existing_install_keeps_its_auto_run_mode_at_start_up() {
    let d = tempfile::tempdir().unwrap();
    let mut st = BTreeSet::new();
    let mut s = AppSettings { autorun_mode: Some(AutorunMode::Advanced), ..AppSettings::default() };
    settle_on_start_in(d.path(), false, &mut st, &mut s);
    assert_eq!(s.autorun_mode, Some(AutorunMode::Advanced));
}

#[test]
fn advanced_is_refused_while_simple_mode_is_on_and_allowed_after_it_is_off() {
    let d = tempfile::tempdir().unwrap();
    let mut st = BTreeSet::new();
    let mut s = AppSettings::default();
    set_app_simple_mode_with(&mut st, &mut s, d.path(), true).unwrap();
    let e = set_autorun_mode_with(&mut s, d.path(), AutorunMode::Advanced).unwrap_err();
    assert_eq!(e, SIMPLE_NO_ADVANCED);
    assert_eq!(s.autorun_mode, Some(AutorunMode::Simple));
    // Simple itself is always fine.
    assert_eq!(set_autorun_mode_with(&mut s, d.path(), AutorunMode::Simple).unwrap().autorun_mode, Some(AutorunMode::Simple));
    // Off leaves Simple; Advanced can then be picked.
    let off = set_app_simple_mode_with(&mut st, &mut s, d.path(), false).unwrap();
    assert_eq!(off.settings.autorun_mode, Some(AutorunMode::Simple));
    let adv = set_autorun_mode_with(&mut s, d.path(), AutorunMode::Advanced).unwrap();
    assert_eq!(adv.autorun_mode, Some(AutorunMode::Advanced));
    assert_eq!(v2_lib::app_settings::load(d.path()).autorun_mode, Some(AutorunMode::Advanced));
}
