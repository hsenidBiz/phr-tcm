//! Module paths: the per-project file an unattended run reads to get from
//! the home page to a case's module screen, and the rules around it.

use serde_json::json;
use v2_lib::autorun::nav::{
    find_path, load_nav, module_key, nav_path, no_path, path_of, put_path, remove_path, route_for, same_page,
    save_nav, set_direct_urls, view, ModulePath, NavFile, NO_ACCOUNT, NO_MODULE,
};
use v2_lib::autorun::recipe::project_slug;

fn path(module: &str, arrived: &str) -> ModulePath {
    serde_json::from_value(json!({
        "module": module,
        "clicks": [
            { "role": "link", "name": "Leave", "exact": true },
            { "role": "link", "name": "Apply Leave", "exact": true }
        ],
        "arrived": arrived,
        "recorded": "2026-09-24T10:00:00Z"
    }))
    .unwrap()
}

fn with(modules: Vec<ModulePath>) -> NavFile {
    NavFile { direct_urls: true, modules }
}

#[test]
fn the_blocked_sentences_are_the_designs_own_words() {
    assert_eq!(NO_MODULE, "This case has no Module - set one in Azure DevOps, or record a path for it.");
    assert_eq!(NO_ACCOUNT, "Choose an account when starting the run, or set Runs as on the script.");
    assert_eq!(
        no_path(" Payroll "),
        "No menu path recorded for module \"Payroll\" - record one in Auto Run, Module paths."
    );
}

#[test]
fn no_file_and_a_file_without_the_switch_both_allow_addresses() {
    let dir = tempfile::tempdir().unwrap();
    let nav = load_nav(dir.path(), "Acme", "Web").unwrap();
    assert!(nav.direct_urls);
    assert!(nav.modules.is_empty());
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), "\u{feff}{ \"modules\": [] }").unwrap();
    assert!(load_nav(dir.path(), "Acme", "Web").unwrap().direct_urls, "absent means true");
}

#[test]
fn the_file_sits_beside_the_sign_in_recipe_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let nav = NavFile { direct_urls: false, modules: vec![path("Leave", "/hr/leave/apply")] };
    save_nav(dir.path(), "Acme", "Web", &nav).unwrap();
    let expected = dir.path().join("projects").join(format!("{}.nav.json", project_slug("Acme", "Web")));
    assert_eq!(nav_path(dir.path(), "Acme", "Web"), expected);
    assert!(expected.is_file());
    assert!(!dir.path().join("projects").join(format!("{}.nav.json.tmp", project_slug("Acme", "Web"))).exists());
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav);
}

#[test]
fn an_unreadable_file_says_so() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), "{ not json").unwrap();
    let err = load_nav(dir.path(), "Acme", "Web").unwrap_err();
    assert!(err.starts_with("the module paths file is not readable"), "{err}");
}

#[test]
fn two_paths_for_the_same_module_are_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let err = save_nav(dir.path(), "Acme", "Web", &with(vec![path("Leave", "/a"), path("  leave ", "/b")])).unwrap_err();
    assert_eq!(err, "module \"leave\" has two paths - keep one");
    assert!(!nav_path(dir.path(), "Acme", "Web").exists());
}

#[test]
fn a_path_needs_a_name_clicks_and_an_address_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut no_clicks = path("Leave", "/hr/leave/apply");
    no_clicks.clicks.clear();
    assert_eq!(
        save_nav(dir.path(), "Acme", "Web", &with(vec![no_clicks])).unwrap_err(),
        "module \"Leave\" has no clicks - record it again"
    );
    assert!(save_nav(dir.path(), "Acme", "Web", &with(vec![path("Leave", "hr/leave")])).is_err());
    assert!(save_nav(dir.path(), "Acme", "Web", &with(vec![path("  ", "/x")])).is_err());
    assert_eq!(
        save_nav(dir.path(), " ", "Web", &with(vec![])).unwrap_err(),
        "pick an organization and a project first"
    );
}

#[test]
fn a_module_matches_trimmed_and_ignoring_case() {
    let nav = with(vec![path("Leave", "/hr/leave/apply")]);
    assert!(find_path(&nav, "  LEAVE ").is_some());
    assert!(find_path(&nav, "Leav").is_none());
    assert!(find_path(&nav, "   ").is_none());
    assert_eq!(module_key(" Apply Leave "), "apply leave");
}

/// Review focus 2: Re-record, or the same module typed in another case,
/// replaces the path rather than being refused or kept twice.
#[test]
fn recording_a_module_again_replaces_its_path_whatever_its_case() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", path("Leave", "/hr/leave/old")).unwrap();
    let nav = put_path(dir.path(), "Acme", "Web", path(" leave ", "/hr/leave/apply")).unwrap();
    assert_eq!(nav.modules.len(), 1);
    assert_eq!(nav.modules[0].module, "leave");
    assert_eq!(nav.modules[0].arrived, "/hr/leave/apply");
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav);
}

#[test]
fn removing_a_path_and_turning_the_switch_change_only_their_own_part() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", path("Leave", "/hr/leave/apply")).unwrap();
    put_path(dir.path(), "Acme", "Web", path("Payroll", "/hr/payroll")).unwrap();
    let nav = set_direct_urls(dir.path(), "Acme", "Web", false).unwrap();
    assert!(!nav.direct_urls);
    assert_eq!(nav.modules.len(), 2);
    let nav = remove_path(dir.path(), "Acme", "Web", "LEAVE").unwrap();
    assert!(!nav.direct_urls, "removing a path leaves the switch alone");
    assert_eq!(nav.modules.iter().map(|m| m.module.as_str()).collect::<Vec<_>>(), vec!["Payroll"]);
    let again = remove_path(dir.path(), "Acme", "Web", "Leave").unwrap();
    assert_eq!(again, nav, "removing what is not there changes nothing");
}

#[test]
fn a_project_with_no_paths_needs_neither_a_module_nor_an_account() {
    assert_eq!(route_for(&NavFile::default(), None, None), Ok(None));
    assert_eq!(route_for(&NavFile::default(), Some("Leave"), Some("hr.admin")), Ok(None));
}

#[test]
fn with_paths_a_case_needs_its_module_a_path_for_it_and_an_account_in_that_order() {
    let nav = with(vec![path("Leave", "/hr/leave/apply")]);
    assert_eq!(route_for(&nav, None, None), Err(NO_MODULE.to_string()));
    assert_eq!(route_for(&nav, Some("  "), Some("hr.admin")), Err(NO_MODULE.to_string()));
    assert_eq!(route_for(&nav, Some("Payroll"), None), Err(no_path("Payroll")));
    assert_eq!(route_for(&nav, Some("leave"), None), Err(NO_ACCOUNT.to_string()));
    assert_eq!(route_for(&nav, Some("leave"), Some(" ")), Err(NO_ACCOUNT.to_string()));
    assert_eq!(route_for(&nav, Some(" Leave "), Some("hr.admin")).unwrap().map(|p| p.arrived.as_str()), Some("/hr/leave/apply"));
}

#[test]
fn only_the_path_of_an_address_counts_as_where_a_page_is() {
    assert_eq!(path_of("https://hr.example.internal/hr/leave/apply?tab=2#top"), "/hr/leave/apply");
    assert_eq!(path_of("https://hr.example.internal"), "/");
    assert_eq!(path_of("https://hr.example.internal/"), "/");
    assert_eq!(path_of("file:///C:/app/menu.html"), "/C:/app/menu.html");
    assert_eq!(path_of("/hr/x?y=1"), "/hr/x");
    let home = "https://hr.example.internal/hr/home/index";
    assert!(same_page("https://HR.example.internal/hr/home/index?from=login", home));
    assert!(!same_page("https://hr.example.internal/hr/welcome", home));
    assert!(!same_page("https://other.example/hr/home/index", home));
}

#[test]
fn the_dialog_view_reads_every_click_in_words() {
    let v = view(&with(vec![path("Leave", "/hr/leave/apply")]));
    assert!(v.direct_urls);
    assert_eq!(v.modules[0].module, "Leave");
    assert_eq!(v.modules[0].clicks, vec!["link \"Leave\"".to_string(), "link \"Apply Leave\"".to_string()]);
    assert_eq!(v.modules[0].arrived, "/hr/leave/apply");
}
