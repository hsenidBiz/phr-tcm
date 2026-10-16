//! New folder (`pw_repo::new_folder`): a feature folder made in the clone,
//! kept in git by a `.gitkeep` in each folder it starts with.

use std::fs;
use std::path::Path;
use v2_lib::pw_repo::layout::feature_dirs;
use v2_lib::pw_repo::new_folder::create_feature;

fn clone() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("suites/sl/admin/performance/competencies/specs")).unwrap();
    fs::write(d.path().join("suites/sl/admin/.gitkeep"), "").unwrap();
    d
}

fn files_under(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn a_new_feature_gets_test_cases_and_specs_kept_in_git_and_is_listed() {
    let c = clone();
    let made = create_feature(c.path(), "  sl/admin/performance/definition-wizard/DW-competencies/ ").unwrap();
    assert_eq!(made.path, "sl/admin/performance/definition-wizard/DW-competencies");
    assert_eq!(
        made.files,
        vec![
            "suites/sl/admin/performance/definition-wizard/DW-competencies/test-cases/.gitkeep",
            "suites/sl/admin/performance/definition-wizard/DW-competencies/specs/.gitkeep",
        ]
    );
    for f in &made.files {
        assert_eq!(fs::read(c.path().join(f)).unwrap(), b"");
    }
    let features: Vec<String> = feature_dirs(&c.path().join("suites")).iter().map(|f| f.join("/")).collect();
    assert!(features.contains(&made.path), "{features:?}");
}

#[test]
fn a_refused_folder_writes_nothing() {
    let c = clone();
    let before = files_under(c.path());
    for (path, says) in [
        ("sl", "needs at least a country and a feature folder"),
        ("sl/_x", "starts with _ or ."),
        ("sl/a:b", "has :"),
        ("sl/../x", "is not a folder name"),
        ("sl/admin/specs/x", "is one of the folders inside a feature"),
        ("sl/admin/performance/competencies", "is already a feature folder"),
        ("sl/Admin/new", "differs from it only in capitals"),
    ] {
        let e = create_feature(c.path(), path).unwrap_err();
        assert!(e.contains(says), "{path}: {e}");
    }
    // A file where a folder would go.
    fs::write(c.path().join("suites/sl/admin/notes"), "x").unwrap();
    let e = create_feature(c.path(), "sl/admin/notes/x").unwrap_err();
    assert!(e.contains("is not a folder"), "{e}");
    fs::remove_file(c.path().join("suites/sl/admin/notes")).unwrap();
    assert_eq!(files_under(c.path()), before);
}

#[test]
fn a_folder_with_no_suites_is_not_the_repo() {
    let d = tempfile::tempdir().unwrap();
    let e = create_feature(d.path(), "sl/admin/x").unwrap_err();
    assert!(e.contains("no suites folder"), "{e}");
    assert!(!d.path().join("suites").exists());
}
