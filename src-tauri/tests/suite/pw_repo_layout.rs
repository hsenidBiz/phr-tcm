//! The folder rules of the Playwright repo, in one place: what a feature
//! folder is, how deep it may sit, and what its navigation key and project are.

use std::fs;
use v2_lib::pw_repo::layout::{self, SpecLocation};

fn s(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

#[test]
fn a_spec_location_is_the_folders_above_specs_at_any_depth() {
    assert_eq!(
        layout::spec_location("sl/admin/performance/definition-wizard/competencies/specs/x.spec.ts"),
        Some(SpecLocation { path: s(&["sl", "admin", "performance", "definition-wizard", "competencies"]), stem: "x".into() })
    );
    assert_eq!(
        layout::spec_location(r"suites\sl\admin\performance\rating-methods\specs\x.spec.ts"),
        Some(SpecLocation { path: s(&["sl", "admin", "performance", "rating-methods"]), stem: "x".into() })
    );
    assert_eq!(
        layout::spec_location("common-components/employee-search/specs/e.spec.ts"),
        Some(SpecLocation { path: s(&["common-components", "employee-search"]), stem: "e".into() })
    );
    for bad in ["_generated/a.spec.ts", "../x/specs/a.spec.ts", "auth.setup.ts", "sl/admin/x.spec.ts", "specs/a.spec.ts", "_x/y/specs/a.spec.ts"] {
        assert_eq!(layout::spec_location(bad), None, "{bad}");
    }
}

#[test]
fn the_navigation_key_is_the_last_two_folders() {
    assert_eq!(layout::navigation_key(&s(&["sl", "admin", "performance", "definition-wizard", "competencies"])), "definition-wizard/competencies");
    assert_eq!(layout::navigation_key(&s(&["sl", "admin", "performance", "rating-methods"])), "performance/rating-methods");
    assert_eq!(layout::navigation_key(&s(&["common-components", "employee-search"])), "common-components/employee-search");
}

#[test]
fn the_first_folder_names_the_project() {
    assert_eq!(layout::owning_project(&s(&["sl", "admin", "x", "y"])), "SL");
    assert_eq!(layout::owning_project(&s(&["ph", "admin", "x", "y"])), "PH");
    assert_eq!(layout::owning_project(&s(&["common-components", "x"])), "SL");
}

#[test]
fn folder_names_are_kebab_case_and_some_are_skipped() {
    assert!(layout::valid_name("rating-methods") && layout::valid_name("a1"));
    for bad in ["", "-a", "a-", "a--b", "A", "a_b", "a b"] {
        assert!(!layout::valid_name(bad), "{bad}");
    }
    assert!(layout::skipped("_generated") && layout::skipped(".git") && layout::skipped("node_modules"));
    assert!(!layout::skipped("sl"));
}

#[test]
fn a_group_folder_is_not_a_feature() {
    let d = tempfile::tempdir().unwrap();
    let mk = |rel: &str| fs::create_dir_all(d.path().join(rel)).unwrap();
    mk("sl/admin/performance/definition-wizard/competencies/specs");
    mk("sl/admin/performance/rating-methods/test-cases");
    mk("common-components/employee-search/pages");
    mk("_generated/specs");
    mk(".git/data");
    assert_eq!(
        layout::feature_dirs(d.path()),
        vec![
            s(&["common-components", "employee-search"]),
            s(&["sl", "admin", "performance", "definition-wizard", "competencies"]),
            s(&["sl", "admin", "performance", "rating-methods"]),
        ]
    );
}

#[test]
fn the_walk_goes_below_a_feature_but_not_into_its_content() {
    let d = tempfile::tempdir().unwrap();
    let mk = |rel: &str| fs::create_dir_all(d.path().join(rel)).unwrap();
    mk("sl/admin/m/f/specs/inner/test-cases");
    mk("sl/admin/m/f/sub/pages");
    assert_eq!(layout::feature_dirs(d.path()), vec![s(&["sl", "admin", "m", "f"]), s(&["sl", "admin", "m", "f", "sub"])]);
}

#[test]
fn a_test_case_file_is_recognised_at_any_depth() {
    assert_eq!(
        layout::test_case_file("suites/sl/admin/performance/definition-wizard/competencies/test-cases/competencies.md"),
        Some(s(&["sl", "admin", "performance", "definition-wizard", "competencies"]))
    );
    assert_eq!(
        layout::test_case_file("suites/sl/admin/performance/rating-methods/test-cases/rating-methods.md"),
        Some(s(&["sl", "admin", "performance", "rating-methods"]))
    );
    for bad in ["suites/_generated/test-cases/a.md", "suites/sl/test-cases/x.txt", "suites/test-cases/a.md", "src/sl/a/test-cases/a.md"] {
        assert_eq!(layout::test_case_file(bad), None, "{bad}");
    }
}

#[test]
fn the_navigation_key_of_a_short_path_is_what_there_is() {
    assert_eq!(layout::navigation_key(&s(&["only"])), "only");
    assert_eq!(layout::navigation_key(&[]), "");
}

#[test]
fn the_walk_does_not_follow_links() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("sl/admin/m/f/specs")).unwrap();
    let link = d.path().join("sl/admin/m/again");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(d.path().join("sl/admin/m/f"), &link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(d.path().join("sl/admin/m/f"), &link);
    if made.is_err() {
        eprintln!("skipped: this account cannot create a directory link");
        return;
    }
    assert_eq!(layout::feature_dirs(d.path()), vec![s(&["sl", "admin", "m", "f"])]);
}
