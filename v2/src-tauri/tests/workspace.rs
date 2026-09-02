//! The working repository's path rules: the `.test-cases` folder, copying a
//! picked file into it without ever overwriting, and the checks the intake
//! and the importer both defer to.

use std::path::{Path, PathBuf};
use v2_lib::workspace::{
    cases_dir, copy_into_cases, default_output_path, ensure_cases_dir, exclude_locally,
    is_inside, resolve_output, slug, CASES_DIR,
};

fn temp_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tcm-ws-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_cases_folder_is_created_once_and_named() {
    let root = temp_root("ensure");
    let dir = ensure_cases_dir(&root).unwrap();
    assert!(dir.is_dir());
    assert_eq!(dir.file_name().unwrap().to_string_lossy(), CASES_DIR);
    assert_eq!(ensure_cases_dir(&root).unwrap(), dir, "idempotent");
    assert_eq!(cases_dir(&root), dir);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ensuring_inside_a_missing_root_is_refused_not_created() {
    let ghost = std::env::temp_dir().join("tcm-ws-no-such-root-9f3a");
    assert!(ensure_cases_dir(&ghost).is_err());
    assert!(!ghost.exists(), "a typo in the root must not become a folder");
}

#[test]
fn a_picked_file_is_copied_in_and_picking_it_again_reuses_the_copy() {
    let root = temp_root("copy");
    let src = root.join("elsewhere").join("login.json");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, r#"{"test_cases":[]}"#).unwrap();

    let copied = copy_into_cases(&root, &src).unwrap();
    assert!(is_inside(&cases_dir(&root), &copied), "{}", copied.display());
    assert_eq!(std::fs::read(&copied).unwrap(), std::fs::read(&src).unwrap());
    assert!(src.exists(), "the original is copied, never moved");

    let again = copy_into_cases(&root, &src).unwrap();
    assert_eq!(again, copied, "same bytes already there - no second file");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_different_file_with_the_same_name_gets_a_suffix_not_an_overwrite() {
    let root = temp_root("suffix");
    let dir = ensure_cases_dir(&root).unwrap();
    std::fs::write(dir.join("login.json"), "old").unwrap();
    let src = root.join("in").join("login.json");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, "new").unwrap();

    let copied = copy_into_cases(&root, &src).unwrap();
    assert_eq!(copied.file_name().unwrap().to_string_lossy(), "login-2.json");
    assert_eq!(std::fs::read_to_string(dir.join("login.json")).unwrap(), "old");
    assert_eq!(std::fs::read_to_string(&copied).unwrap(), "new");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_already_in_the_folder_is_not_copied() {
    let root = temp_root("inside");
    let dir = ensure_cases_dir(&root).unwrap();
    let inside = dir.join("x.json");
    std::fs::write(&inside, "{}").unwrap();
    assert_eq!(copy_into_cases(&root, &inside).unwrap(), inside);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no copy of a copy");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn is_inside_is_case_insensitive_and_exact_about_prefixes() {
    let root = temp_root("inside-check");
    let dir = ensure_cases_dir(&root).unwrap();
    assert!(is_inside(&dir, &dir.join("a.json")), "a file not yet written still counts");
    let shouted = PathBuf::from(dir.to_string_lossy().to_uppercase()).join("a.json");
    assert!(is_inside(&dir, &shouted), "Windows paths compare case-insensitively");
    let sibling = root.join(".test-cases-extra").join("a.json");
    assert!(!is_inside(&dir, &sibling), "a sibling sharing the prefix is outside");
    assert!(!is_inside(&dir, &root.join("a.json")));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn output_names_derive_from_the_feature() {
    assert_eq!(slug("Login & Session flow"), "login-session-flow");
    assert_eq!(slug("   "), "test-cases");
    let p = default_output_path(Path::new("D:/repo"), "Login flow");
    assert!(p.ends_with("login-flow.json"), "{p}");
    assert!(p.contains(CASES_DIR), "{p}");
}

#[test]
fn a_bare_name_resolves_into_the_folder_and_a_path_is_left_alone() {
    let root = Path::new("D:/repo");
    let resolved = resolve_output(root, "login.json");
    assert_eq!(resolved, cases_dir(root).join("login.json").to_string_lossy());
    assert_eq!(resolve_output(root, "D:/x/y.json"), "D:/x/y.json");
    assert_eq!(resolve_output(root, "  "), "");
}

#[test]
fn exclude_writes_once_and_only_in_a_git_checkout() {
    let plain = temp_root("plain");
    assert_eq!(exclude_locally(&plain, ".mcp.json").unwrap(), false);
    assert!(!plain.join(".git").exists());

    let repo = temp_root("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    assert_eq!(exclude_locally(&repo, ".mcp.json").unwrap(), true);
    assert_eq!(exclude_locally(&repo, ".mcp.json").unwrap(), true);
    let text = std::fs::read_to_string(repo.join(".git").join("info").join("exclude")).unwrap();
    assert_eq!(text.matches(".mcp.json").count(), 1, "{text}");
    let _ = std::fs::remove_dir_all(&plain);
    let _ = std::fs::remove_dir_all(&repo);
}
