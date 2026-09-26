//! The working repository's path rules: the `.test-cases` folder, copying a
//! picked file into it without ever overwriting, and the checks the intake
//! and the importer both defer to.

use std::path::{Path, PathBuf};
use v2_lib::workspace::{
    cases_dir, copy_into_cases, default_output_path, ensure_cases_dir, exclude_locally,
    is_inside, resolve_output, slug, Exclusion, CASES_DIR,
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

    let (copied, _) = copy_into_cases(&root, &src).unwrap();
    assert!(is_inside(&cases_dir(&root), &copied), "{}", copied.display());
    assert_eq!(std::fs::read(&copied).unwrap(), std::fs::read(&src).unwrap());
    assert!(src.exists(), "the original is copied, never moved");

    let (again, _) = copy_into_cases(&root, &src).unwrap();
    assert_eq!(again, copied, "same bytes already there - no second file");
    let _ = std::fs::remove_dir_all(&root);
}

/// Round 8 §11. The old rule - never overwrite, newer bytes take `-2`,
/// `-3` - left the OLDEST content under the canonical name, and on an
/// id-carrying set importing the obvious file silently reverted fifteen
/// corrected work items. The picked file is the one the user wants: it
/// takes the canonical name, and what it displaces goes to `.history`.
#[test]
fn a_different_file_with_the_same_name_replaces_the_copy_and_keeps_the_old_one() {
    let root = temp_root("displace");
    let dir = ensure_cases_dir(&root).unwrap();
    std::fs::write(dir.join("login.json"), "old").unwrap();
    let src = root.join("in").join("login.json");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, "new").unwrap();

    let (copied, displaced) = copy_into_cases(&root, &src).unwrap();
    assert_eq!(copied, dir.join("login.json"), "the obvious name is the newest");
    assert_eq!(std::fs::read_to_string(&copied).unwrap(), "new");
    let displaced = displaced.expect("the old bytes were kept somewhere");
    assert!(is_inside(&dir.join(".history"), &displaced), "{}", displaced.display());
    assert!(displaced.file_name().unwrap().to_string_lossy().starts_with("login."));
    assert_eq!(std::fs::read_to_string(&displaced).unwrap(), "old", "nothing is ever lost");
    assert!(!dir.join("login-2.json").exists(), "no more numbered copies");

    // Picking it again with the same bytes: no second displacement.
    let (again, none) = copy_into_cases(&root, &src).unwrap();
    assert_eq!(again, copied);
    assert!(none.is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_already_in_the_folder_is_not_copied() {
    let root = temp_root("inside");
    let dir = ensure_cases_dir(&root).unwrap();
    let inside = dir.join("x.json");
    std::fs::write(&inside, "{}").unwrap();
    assert_eq!(copy_into_cases(&root, &inside).unwrap().0, inside);
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

// ------------------------------------------------------------ git exclusion
//
// The connection string for the database MCP server lands in a project
// config file, so "is this file going to be committed?" is a security
// question, not a tidiness one. These go through the real `git` CLI - the
// same thing that decides the answer on the user's machine.

fn git_is_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_in(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("could not run git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn an_untracked_config_is_excluded_and_the_line_is_written_once() {
    if !git_is_available() {
        eprintln!("skipped: git is not on PATH");
        return;
    }
    let repo = temp_root("git-untracked");
    git_in(&repo, &["init", "-q"]);
    std::fs::write(repo.join(".mcp.json"), "{}").unwrap();

    assert_eq!(exclude_locally(&repo, ".mcp.json").unwrap(), Exclusion::Excluded);
    assert_eq!(exclude_locally(&repo, ".mcp.json").unwrap(), Exclusion::Excluded);
    let text = std::fs::read_to_string(repo.join(".git").join("info").join("exclude")).unwrap();
    assert_eq!(text.matches(".mcp.json").count(), 1, "{text}");
    let _ = std::fs::remove_dir_all(&repo);
}

/// The reason this went through the CLI at all: `.git/info/exclude` has no
/// effect on a file git already TRACKS, so the password would show up as a
/// plain modification and be committed with the next `git add -A`.
#[test]
fn a_tracked_config_is_reported_as_tracked() {
    if !git_is_available() {
        eprintln!("skipped: git is not on PATH");
        return;
    }
    let repo = temp_root("git-tracked");
    git_in(&repo, &["init", "-q"]);
    git_in(&repo, &["config", "user.email", "tcm@example.test"]);
    git_in(&repo, &["config", "user.name", "TCM Tests"]);
    std::fs::write(repo.join(".mcp.json"), "{}").unwrap();
    git_in(&repo, &["add", ".mcp.json"]);
    git_in(&repo, &["commit", "-q", "-m", "add config"]);

    assert_eq!(exclude_locally(&repo, ".mcp.json").unwrap(), Exclusion::Tracked);
    // The line is still written - harmless, and it starts working the
    // moment the file leaves the index.
    let text = std::fs::read_to_string(repo.join(".git").join("info").join("exclude")).unwrap();
    assert_eq!(text.matches(".mcp.json").count(), 1, "{text}");
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn a_folder_that_is_not_a_checkout_says_so_and_gets_no_git_dir() {
    let plain = temp_root("git-plain");
    assert_eq!(exclude_locally(&plain, ".mcp.json").unwrap(), Exclusion::NotGit);
    assert!(!plain.join(".git").exists(), "nothing is created in a plain folder");
    let _ = std::fs::remove_dir_all(&plain);
}
