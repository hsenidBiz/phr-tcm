//! The header pill's look at the clone: how many spec files `suites/` holds
//! and a fingerprint that moves when they do. Everything here works on a
//! temp folder, never on a real clone.

use std::fs;
use std::path::Path;
use v2_lib::autorun::pw_rules::{READ_LIMIT, SNAPSHOT};
use v2_lib::pw_repo::specs_status::{pipeline_files, read, CLONE_MISSING, SUITES_MISSING};

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// A clone with two specs (one `.ts`, one `.js`) and files that are not specs.
fn clone() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "suites/sl/admin/rating/specs/a.spec.ts", "test('a')");
    put(d.path(), "suites/sl/admin/rating/specs/deep/b.spec.js", "test('b')");
    put(d.path(), "suites/sl/admin/rating/pages/page.ts", "export {}");
    put(d.path(), "suites/sl/admin/rating/test-cases/c.md", "## 1");
    put(d.path(), "suites/sl/admin/rating/specs/helper.spec.ts.bak", "x");
    d
}

#[test]
fn it_counts_ts_and_js_specs_and_nothing_else() {
    let d = clone();
    let s = read(d.path()).unwrap();
    assert_eq!(s.count, 2);
    assert_eq!(s.fingerprint.len(), 64);
    assert!(s.fingerprint.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn the_same_files_give_the_same_fingerprint() {
    let d = clone();
    assert_eq!(read(d.path()).unwrap(), read(d.path()).unwrap());
}

#[test]
fn folders_a_run_leaves_behind_and_dot_folders_are_skipped() {
    let d = clone();
    let before = read(d.path()).unwrap();
    for skipped in ["node_modules", ".hidden", "test-results", "playwright-report", "allure-results", "blob-report"] {
        put(d.path(), &format!("suites/sl/admin/rating/{skipped}/x.spec.ts"), "test('x')");
        put(d.path(), &format!("suites/{skipped}/y.spec.js"), "test('y')");
    }
    assert_eq!(read(d.path()).unwrap(), before);
}

#[test]
fn a_spec_outside_suites_is_not_counted() {
    let d = clone();
    put(d.path(), "src/elsewhere.spec.ts", "test('z')");
    assert_eq!(read(d.path()).unwrap().count, 2);
}

#[test]
fn adding_a_spec_changes_the_count_and_the_fingerprint() {
    let d = clone();
    let before = read(d.path()).unwrap();
    put(d.path(), "suites/sl/self/smoke/specs/new.spec.ts", "test('n')");
    let after = read(d.path()).unwrap();
    assert_eq!(after.count, before.count + 1);
    assert_ne!(after.fingerprint, before.fingerprint);
}

#[test]
fn removing_a_spec_changes_the_count_and_the_fingerprint() {
    let d = clone();
    let before = read(d.path()).unwrap();
    fs::remove_file(d.path().join("suites/sl/admin/rating/specs/a.spec.ts")).unwrap();
    let after = read(d.path()).unwrap();
    assert_eq!(after.count, before.count - 1);
    assert_ne!(after.fingerprint, before.fingerprint);
}

#[test]
fn editing_a_spec_changes_the_fingerprint_but_not_the_count() {
    let d = clone();
    let before = read(d.path()).unwrap();
    // A longer file: the size differs even when the clock does not tick.
    put(d.path(), "suites/sl/admin/rating/specs/a.spec.ts", "test('a'); test('a2');");
    let after = read(d.path()).unwrap();
    assert_eq!(after.count, before.count);
    assert_ne!(after.fingerprint, before.fingerprint);
}

#[test]
fn touching_a_spec_without_changing_its_size_changes_the_fingerprint() {
    let d = clone();
    let file = d.path().join("suites/sl/admin/rating/specs/a.spec.ts");
    let before = read(d.path()).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    fs::OpenOptions::new().write(true).open(&file).unwrap().set_modified(later).unwrap();
    assert_ne!(read(d.path()).unwrap().fingerprint, before.fingerprint);
}

#[test]
fn renaming_a_spec_changes_the_fingerprint() {
    let d = clone();
    let before = read(d.path()).unwrap();
    let from = d.path().join("suites/sl/admin/rating/specs/a.spec.ts");
    fs::rename(&from, from.with_file_name("renamed.spec.ts")).unwrap();
    let after = read(d.path()).unwrap();
    assert_eq!(after.count, before.count);
    assert_ne!(after.fingerprint, before.fingerprint);
}

#[test]
fn a_suites_folder_with_no_specs_counts_zero() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("suites/sl")).unwrap();
    assert_eq!(read(d.path()).unwrap().count, 0);
}

#[test]
fn a_missing_suites_folder_is_an_error_that_names_no_path() {
    let d = tempfile::tempdir().unwrap();
    let e = read(d.path()).unwrap_err();
    assert_eq!(e, SUITES_MISSING);
    // The webview shows exactly this sentence as "No specs found" (src/lib/pwSpecs.ts).
    assert_eq!(SUITES_MISSING, "The clone has no suites folder.");
    assert!(!e.contains(&d.path().to_string_lossy().to_string()));
}

#[test]
fn a_missing_clone_is_an_error_that_names_no_path() {
    let d = tempfile::tempdir().unwrap();
    let gone = d.path().join("not-here");
    let e = read(&gone).unwrap_err();
    assert_eq!(e, CLONE_MISSING);
    assert!(!e.contains(&gone.to_string_lossy().to_string()));
}

#[test]
fn a_suites_that_is_a_file_is_not_a_suites_folder() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "suites", "not a folder");
    assert_eq!(read(d.path()).unwrap_err(), SUITES_MISSING);
}

// --- The pipeline's rule files -----------------------------------------

/// A clone holding every rule file the app ships a copy of.
fn clone_with_rules() -> tempfile::TempDir {
    let d = clone();
    for (rel, text) in SNAPSHOT {
        put(d.path(), rel, text);
    }
    d
}

#[test]
fn every_shipped_rule_file_in_the_clone_is_found_and_none_missing() {
    let d = clone_with_rules();
    let p = read(d.path()).unwrap().pipeline;
    let mut shipped: Vec<String> = SNAPSHOT.iter().map(|(k, _)| k.to_string()).collect();
    shipped.sort();
    assert_eq!(p.found, shipped);
    assert!(p.missing.is_empty());
    assert_eq!(p.fingerprint.len(), 64);
    // The test specs are not rule files, and are still counted on their own.
    assert_eq!(read(d.path()).unwrap().count, 2);
}

#[test]
fn a_rule_file_the_clone_lacks_is_missing_by_its_repo_path() {
    let d = clone_with_rules();
    fs::remove_file(d.path().join(".claude/agents/test-refactorer.md")).unwrap();
    let p = read(d.path()).unwrap().pipeline;
    assert_eq!(p.missing, vec![".claude/agents/test-refactorer.md".to_string()]);
    assert!(!p.found.contains(&".claude/agents/test-refactorer.md".to_string()));
    // Names no clone path.
    assert!(p.missing.iter().all(|m| !m.contains(&d.path().to_string_lossy().to_string())));
}

#[test]
fn a_clone_with_no_rule_files_has_every_shipped_one_missing() {
    let d = clone();
    let p = read(d.path()).unwrap().pipeline;
    assert!(p.found.is_empty());
    assert_eq!(p.missing.len(), SNAPSHOT.len());
}

#[test]
fn a_skill_of_the_clones_own_is_found_and_one_without_skill_md_is_not_missing() {
    let d = clone_with_rules();
    put(d.path(), ".claude/skills/our-own/SKILL.md", "# ours");
    fs::create_dir_all(d.path().join(".claude/skills/empty-one")).unwrap();
    let p = read(d.path()).unwrap().pipeline;
    assert!(p.found.contains(&".claude/skills/our-own/SKILL.md".to_string()));
    assert!(!p.found.iter().chain(&p.missing).any(|f| f.contains("empty-one")));
    assert!(p.missing.is_empty());
}

#[test]
fn a_rule_file_too_big_to_be_one_is_missing() {
    let d = clone_with_rules();
    put(d.path(), "CLAUDE.md", &"x".repeat(READ_LIMIT as usize + 1));
    assert_eq!(read(d.path()).unwrap().pipeline.missing, vec!["CLAUDE.md".to_string()]);
}

#[test]
fn editing_a_rule_file_moves_only_the_pipeline_fingerprint() {
    let d = clone_with_rules();
    let before = read(d.path()).unwrap();
    put(d.path(), ".claude/commands/gen-test.md", "a different, longer gen-test command");
    let after = read(d.path()).unwrap();
    assert_ne!(after.pipeline.fingerprint, before.pipeline.fingerprint);
    // The Overview's re-read follows the test specs alone.
    assert_eq!(after.fingerprint, before.fingerprint);
    assert_eq!(pipeline_files(d.path()), after.pipeline);
}

#[test]
fn editing_a_spec_leaves_the_pipeline_fingerprint_alone() {
    let d = clone_with_rules();
    let before = read(d.path()).unwrap();
    put(d.path(), "suites/sl/admin/rating/specs/a.spec.ts", "test('a'); test('a2');");
    assert_eq!(read(d.path()).unwrap().pipeline, before.pipeline);
}
