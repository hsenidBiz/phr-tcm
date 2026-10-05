//! A run's downloads as the person reaches them: Open hands a file to the
//! default app only when it is a plain name in that run's own download
//! folder (Review Focus 5), and the sizes Past runs and the review show are
//! read from that folder.

use v2_lib::autorun::store::{download_files, downloads_dir, DOWNLOAD_GONE, NOT_THIS_RUNS_DOWNLOAD};
use v2_lib::commands::autorun::{download_path_at, download_sizes_at};

fn run_folder_with(root: &std::path::Path, run_id: &str, files: &[(&str, usize)]) -> std::path::PathBuf {
    let dir = downloads_dir(root, run_id);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, len) in files {
        std::fs::write(dir.join(name), vec![b'x'; *len]).unwrap();
    }
    dir
}

#[test]
fn a_file_in_the_runs_own_folder_resolves_to_that_file() {
    let root = tempfile::tempdir().unwrap();
    let dir = run_folder_with(root.path(), "run-1", &[("Template.xlsx", 10), ("x (2).csv", 3)]);
    let got = download_path_at(true, root.path(), "run-1", "Template.xlsx").unwrap();
    // The plain path, never the verbatim `\\?\` form canonicalising gives on
    // Windows, which the shell's opener handles unreliably.
    assert_eq!(got, dir.join("Template.xlsx"));
    assert!(!got.to_string_lossy().starts_with(r"\\?\"), "{}", got.display());
    // A numbered name is an ordinary name.
    assert!(download_path_at(true, root.path(), "run-1", "x (2).csv").is_ok());
}

#[test]
fn two_dots_inside_a_name_open_but_dot_and_dot_dot_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let dir = run_folder_with(root.path(), "run-1", &[("report..csv", 4)]);
    assert_eq!(download_path_at(true, root.path(), "run-1", "report..csv").unwrap(), dir.join("report..csv"));
    for name in ["..", "."] {
        assert_eq!(download_path_at(true, root.path(), "run-1", name).unwrap_err(), NOT_THIS_RUNS_DOWNLOAD, "{name:?}");
    }
}

#[test]
fn a_name_with_path_parts_is_refused_even_when_the_file_exists() {
    let root = tempfile::tempdir().unwrap();
    run_folder_with(root.path(), "run-1", &[("a.csv", 1)]);
    run_folder_with(root.path(), "run-2", &[("secret.csv", 1)]);
    std::fs::write(root.path().join("runs.json"), b"{}").unwrap();
    for name in [
        "..\\run-2\\secret.csv",
        "../run-2/secret.csv",
        "..",
        "..\\..\\runs.json",
        "sub/a.csv",
        "sub\\a.csv",
        "C:\\Windows\\win.ini",
        "C:a.csv",
        "a.csv:stream",
        "/etc/passwd",
        "\\\\server\\share\\a.csv",
        "",
    ] {
        assert_eq!(
            download_path_at(true, root.path(), "run-1", name).unwrap_err(),
            NOT_THIS_RUNS_DOWNLOAD,
            "{name:?} must be refused"
        );
    }
}

#[test]
fn another_runs_file_or_an_unsafe_run_id_is_refused() {
    let root = tempfile::tempdir().unwrap();
    run_folder_with(root.path(), "run-2", &[("secret.csv", 1)]);
    run_folder_with(root.path(), "unnamed-run", &[("secret.csv", 1)]);
    run_folder_with(root.path(), "supervised", &[("secret.csv", 1)]);
    for run_id in ["run-1", "../run-2", "..", "supervised", "SUPERVISED", ""] {
        let err = download_path_at(true, root.path(), run_id, "secret.csv").unwrap_err();
        assert!(err == NOT_THIS_RUNS_DOWNLOAD || err == DOWNLOAD_GONE, "{run_id:?}: {err}");
    }
    // An unsafe id never reaches the shared fallback folder.
    assert_eq!(download_path_at(true, root.path(), "../run-2", "secret.csv").unwrap_err(), NOT_THIS_RUNS_DOWNLOAD);
}

#[test]
fn a_file_that_is_gone_says_so() {
    let root = tempfile::tempdir().unwrap();
    run_folder_with(root.path(), "run-1", &[]);
    assert_eq!(download_path_at(true, root.path(), "run-1", "gone.csv").unwrap_err(), DOWNLOAD_GONE);
    // No folder at all reads the same.
    assert_eq!(download_path_at(true, root.path(), "run-9", "gone.csv").unwrap_err(), DOWNLOAD_GONE);
}

#[test]
fn a_folder_in_the_runs_folder_is_not_a_download() {
    let root = tempfile::tempdir().unwrap();
    let dir = run_folder_with(root.path(), "run-1", &[]);
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    assert_eq!(download_path_at(true, root.path(), "run-1", "nested").unwrap_err(), NOT_THIS_RUNS_DOWNLOAD);
    assert!(download_files(root.path(), "run-1").is_empty());
}

/// A link in the run's folder that leads out of it is refused. A file
/// symlink needs a privilege an ordinary account may not have, so it is
/// checked where it can be made; a directory junction to an outside folder
/// needs none, and is always made and checked.
#[cfg(windows)]
#[test]
fn a_link_out_of_the_runs_folder_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let dir = run_folder_with(root.path(), "run-1", &[]);
    let outside = root.path().join("outside.txt");
    std::fs::write(&outside, b"x").unwrap();
    if std::os::windows::fs::symlink_file(&outside, dir.join("link.txt")).is_ok() {
        assert_eq!(download_path_at(true, root.path(), "run-1", "link.txt").unwrap_err(), NOT_THIS_RUNS_DOWNLOAD);
    }

    let outside_dir = root.path().join("outside-folder");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("a.csv"), b"x").unwrap();
    let junction = dir.join("junction");
    let made = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&junction)
        .arg(&outside_dir)
        .output()
        .unwrap();
    assert!(made.status.success(), "mklink /J failed: {}", String::from_utf8_lossy(&made.stderr));
    assert!(std::fs::symlink_metadata(&junction).is_ok(), "the junction was not made");
    assert_eq!(download_path_at(true, root.path(), "run-1", "junction").unwrap_err(), NOT_THIS_RUNS_DOWNLOAD);
    assert!(download_files(root.path(), "run-1").iter().all(|f| f.name != "junction"));
}

#[test]
fn a_locked_build_opens_nothing() {
    let root = tempfile::tempdir().unwrap();
    run_folder_with(root.path(), "run-1", &[("a.csv", 1)]);
    assert!(download_path_at(false, root.path(), "run-1", "a.csv").is_err());
    assert!(download_sizes_at(false, root.path(), "run-1").is_err());
}

#[test]
fn the_sizes_are_the_files_in_the_runs_folder_by_name() {
    let root = tempfile::tempdir().unwrap();
    run_folder_with(root.path(), "run-1", &[("b.csv", 1126), ("a.xlsx", 5427)]);
    run_folder_with(root.path(), "run-2", &[("other.csv", 1)]);
    let got = download_sizes_at(true, root.path(), "run-1").unwrap();
    let pairs: Vec<(String, u32)> = got.into_iter().map(|f| (f.name, f.size)).collect();
    assert_eq!(pairs, vec![("a.xlsx".to_string(), 5427), ("b.csv".to_string(), 1126)]);
    // A run with no folder has none; an unsafe id lists nothing.
    assert!(download_sizes_at(true, root.path(), "run-9").unwrap().is_empty());
    run_folder_with(root.path(), "unnamed-run", &[("x.csv", 1)]);
    assert!(download_sizes_at(true, root.path(), "../run-2").unwrap().is_empty());
}
