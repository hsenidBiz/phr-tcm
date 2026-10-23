//! The header pill's look at the clone: how many spec files `suites/` holds
//! and a fingerprint that moves when they do. Everything here works on a
//! temp folder, never on a real clone.

use std::fs;
use std::path::Path;
use v2_lib::pw_repo::specs_status::{read, CLONE_MISSING, SUITES_MISSING};

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
