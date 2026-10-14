//! The Playwright tab: reading the team's repo from its clone folder, and
//! the health line. The fixture `tests/fixtures/pw_repo` is a cut-down clone
//! with made-up accounts; tests that change it work on a copy.

use std::fs;
use std::path::{Path, PathBuf};
use v2_lib::pw_repo::structure::{case_headings, node_on_path, read, structure_for, HealthIssue, RepoFeature};
use v2_lib::pw_repo::NOT_OFFERED;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pw_repo")
}

pub fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// A copy of the fixture with everything a healthy clone has.
fn healthy_copy() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    copy_dir(&fixture(), d.path());
    fs::create_dir_all(d.path().join("node_modules/@playwright/test")).unwrap();
    fs::write(d.path().join("qa-options.json"), "{}\n").unwrap();
    d
}

#[test]
fn the_fixture_repo_reads_as_its_features_ids_and_keys() {
    let d = healthy_copy();
    let s = read(d.path(), true).unwrap();
    assert_eq!(s.health, Vec::<HealthIssue>::new());
    assert_eq!(
        s.features,
        vec![
            RepoFeature {
                country: "sl".into(),
                side: "admin".into(),
                module: "performance".into(),
                feature: "rating-methods".into(),
                spec_count: 1,
                case_count: 3,
                case_ids: vec![135152, 135179, 135180],
            },
            RepoFeature {
                country: "sl".into(),
                side: "self".into(),
                module: "smoke".into(),
                feature: "framework-smoke".into(),
                spec_count: 2,
                case_count: 0,
                case_ids: vec![],
            },
        ],
        "_generated, common-components and an empty country are not features"
    );
    assert_eq!(s.generated_ids, vec![135152, 135179, 135190]);
    assert_eq!(s.user_keys, vec!["FixtureSL.performance.general", "FixtureSL.performance.supervisor"]);
    assert_eq!(s.navigation_keys, vec!["performance/rating-methods"]);
}

#[test]
fn the_structure_never_carries_a_password() {
    let d = healthy_copy();
    let s = read(d.path(), true).unwrap();
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("not-a-real-password"), "{json}");
    assert!(!json.contains("password"), "{json}");
}

#[test]
fn a_folder_that_is_not_the_repo_says_so_and_reads_nothing() {
    let d = tempfile::tempdir().unwrap();
    let s = read(d.path(), true).unwrap();
    assert_eq!(s.health, vec![HealthIssue::NotTheRepo]);
    assert!(s.features.is_empty() && s.user_keys.is_empty());

    let s = read(Path::new(""), true).unwrap();
    assert_eq!(s.health, vec![HealthIssue::NotTheRepo], "no folder chosen yet");
}

#[test]
fn a_clone_without_node_modules_says_so() {
    let d = healthy_copy();
    fs::remove_dir_all(d.path().join("node_modules")).unwrap();
    assert_eq!(read(d.path(), true).unwrap().health, vec![HealthIssue::NoNodeModules]);
}

#[test]
fn no_node_on_path_says_so() {
    let d = healthy_copy();
    assert_eq!(read(d.path(), false).unwrap().health, vec![HealthIssue::NoNode]);
    let e = tempfile::tempdir().unwrap();
    assert_eq!(
        read(e.path(), false).unwrap().health,
        vec![HealthIssue::NotTheRepo, HealthIssue::NoNode],
        "Node is looked for even without the repo"
    );
}

#[test]
fn a_missing_qa_options_file_says_so() {
    let d = healthy_copy();
    fs::remove_file(d.path().join("qa-options.json")).unwrap();
    assert_eq!(read(d.path(), true).unwrap().health, vec![HealthIssue::NoQaOptions]);
}

#[test]
fn every_issue_at_once_in_order() {
    let d = tempfile::tempdir().unwrap();
    copy_dir(&fixture(), d.path());
    assert_eq!(
        read(d.path(), false).unwrap().health,
        vec![HealthIssue::NoNodeModules, HealthIssue::NoNode, HealthIssue::NoQaOptions]
    );
}

#[test]
fn a_malformed_repo_file_is_an_error_that_names_it() {
    let d = healthy_copy();
    fs::write(d.path().join("src/navigation.json"), "[]").unwrap();
    let e = read(d.path(), true).unwrap_err();
    assert!(e.contains("src/navigation.json"), "{e}");
}

#[test]
fn refused_with_advanced_features_off() {
    let d = healthy_copy();
    let path = d.path().to_string_lossy().to_string();
    assert_eq!(structure_for(false, &path, true).unwrap_err(), NOT_OFFERED);
    assert!(structure_for(true, &path, true).is_ok());
}

#[test]
fn case_headings_take_level_two_numeric_headings_only() {
    let text = "# Set\r\n## 1 - one\r\n### 2 - deeper\n## Amendment - 2026\n## 3\u{2014}joined\n## 4 \u{2014} em dash\n##5 no space\n## 1 - again\n";
    assert_eq!(case_headings(text), vec![1, 4, 1]);
}

#[test]
fn node_is_found_on_path_by_looking_only() {
    let d = tempfile::tempdir().unwrap();
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    let empty = tempfile::tempdir().unwrap();
    let path = std::env::join_paths([empty.path(), d.path()]).unwrap();
    assert!(!node_on_path(Some(&path), Some(".COM;.EXE".as_ref())));
    fs::write(d.path().join(name), "").unwrap();
    assert!(node_on_path(Some(&path), Some(".COM;.EXE".as_ref())));
    assert!(node_on_path(Some(&path), None), "PATHEXT unset falls back to the usual extensions");
    assert!(!node_on_path(None, None));
}
