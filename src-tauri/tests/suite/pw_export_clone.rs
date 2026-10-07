//! Playwright export: reading a PHR-PLAYWRIGHT-AUTOMATION clone.

use std::fs;
use std::path::Path;
use v2_lib::pw_export::clone::{index_with, open, NOT_A_CLONE};

fn build(root: &Path, users: &str, index: &str) {
    fs::create_dir_all(root.join("suites/_generated")).unwrap();
    fs::create_dir_all(root.join("src/users")).unwrap();
    fs::write(root.join("playwright.config.ts"), "export default {};").unwrap();
    fs::write(root.join("suites/_generated/index.json"), index).unwrap();
    fs::write(
        root.join("src/navigation.json"),
        r#"{ "$schema": "./navigation.schema.json", "entries": { "performance/rating-methods": {}, "admin/users": {} } }"#,
    )
    .unwrap();
    fs::write(root.join("src/users/users.json"), users).unwrap();
}

#[test]
fn a_real_looking_clone_opens() {
    let d = tempfile::tempdir().unwrap();
    build(
        d.path(),
        r#"{"B_KEY": {"username": "u", "password": "p"}, "A_KEY": {"username": "v", "password": "q"}}"#,
        "{\n  \"9\": \"z.spec.ts\",\n  \"1\": \"a.spec.ts\"\n}\n",
    );
    fs::write(d.path().join("suites/_generated/z.spec.ts"), "").unwrap();
    fs::write(d.path().join("suites/_generated/a.spec.ts"), "").unwrap();
    fs::write(d.path().join("suites/_generated/notes.txt"), "").unwrap();
    let c = open(d.path()).unwrap();
    assert_eq!(c.user_keys, vec!["B_KEY", "A_KEY"]);
    assert_eq!(c.navigation_keys, vec!["performance/rating-methods", "admin/users"]);
    assert_eq!(
        c.index,
        vec![("9".to_string(), "z.spec.ts".to_string()), ("1".to_string(), "a.spec.ts".to_string())]
    );
    let mut g = c.generated_files.clone();
    g.sort();
    assert_eq!(g, vec!["a.spec.ts", "z.spec.ts"]);
}

#[test]
fn user_values_are_never_read() {
    let d = tempfile::tempdir().unwrap();
    build(d.path(), r#"{"K": 42}"#, "{}");
    let c = open(d.path()).unwrap();
    assert_eq!(c.user_keys, vec!["K"]);
}

#[test]
fn a_folder_that_is_not_the_repo_is_refused() {
    let d = tempfile::tempdir().unwrap();
    build(d.path(), "{}", "{}");
    fs::remove_file(d.path().join("playwright.config.ts")).unwrap();
    assert_eq!(open(d.path()).unwrap_err(), NOT_A_CLONE);
}

#[test]
fn a_malformed_index_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    build(d.path(), "{}", "[]");
    let e = open(d.path()).unwrap_err();
    assert!(e.contains("suites/_generated/index.json"), "{e}");
}

#[test]
fn index_appends_and_keeps_order() {
    let out = index_with(&[("1".into(), "a.spec.ts".into())], 2, "b.spec.ts");
    assert_eq!(out, "{\n  \"1\": \"a.spec.ts\",\n  \"2\": \"b.spec.ts\"\n}\n");
}

#[test]
fn index_replaces_in_place() {
    let cur = vec![("1".to_string(), "a.spec.ts".to_string()), ("2".to_string(), "b.spec.ts".to_string())];
    let out = index_with(&cur, 1, "c.spec.ts");
    assert_eq!(out, "{\n  \"1\": \"c.spec.ts\",\n  \"2\": \"b.spec.ts\"\n}\n");
}
