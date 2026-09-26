//! `help::write_help` - staging the bundled "How To Use" site under the
//! app's local data dir before `open_help` hands it to the browser. See
//! `src-tauri/src/help.rs` for the write-then-rename design this exercises.
//!
//! The site ships with only `index.html` today (Task 1) - a later task
//! adds `img/light|dark/*.jpg` - so every test here walks whatever the
//! build actually embeds rather than hardcoding a file list, by embedding
//! the same folder a second time here and recursing over it.

use v2_lib::help::{write_help, write_help_trusting, OPEN_ERROR};

static HELP: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/help");

fn collect_paths(dir: &include_dir::Dir, out: &mut Vec<std::path::PathBuf>) {
    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(sub) => collect_paths(sub, out),
            include_dir::DirEntry::File(file) => out.push(file.path().to_path_buf()),
        }
    }
}

/// Every relative path the embedded site ships, at least `index.html`.
fn embedded_paths() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    collect_paths(&HELP, &mut out);
    assert!(!out.is_empty(), "the embedded help site has no files");
    out
}

#[test]
fn writes_every_embedded_file_and_returns_index_html() {
    let dest_root = tempfile::tempdir().unwrap();
    let index = write_help(dest_root.path(), "9.9.9").unwrap();
    assert_eq!(index, dest_root.path().join("9.9.9").join("index.html"));
    assert!(index.is_file());
    for rel in embedded_paths() {
        let written = dest_root.path().join("9.9.9").join(&rel);
        assert!(written.is_file(), "{} was not written", rel.display());
    }
}

/// A second call for the same version must never disturb anything else in
/// the folder - a marker `write_help` never creates itself must still be
/// there afterwards. (Release builds skip writing entirely once the
/// folder is confirmed complete; debug builds always rewrite the embedded
/// files themselves - see `write_help`'s doc comment and the
/// `a_debug_build_rewrites...` test below - but `write_files` never
/// touches a path it does not embed, so the marker survives either way.)
#[test]
fn a_second_call_for_the_same_version_does_not_rewrite() {
    let dest_root = tempfile::tempdir().unwrap();
    write_help(dest_root.path(), "9.9.9").unwrap();
    let version_dir = dest_root.path().join("9.9.9");
    let marker = version_dir.join("marker.txt");
    std::fs::write(&marker, "kept").unwrap();

    let index = write_help(dest_root.path(), "9.9.9").unwrap();
    assert_eq!(index, version_dir.join("index.html"));
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "kept");
}

#[test]
fn a_partially_deleted_folder_is_completed() {
    let dest_root = tempfile::tempdir().unwrap();
    write_help(dest_root.path(), "9.9.9").unwrap();
    let version_dir = dest_root.path().join("9.9.9");
    std::fs::remove_file(version_dir.join("index.html")).unwrap();

    let index = write_help(dest_root.path(), "9.9.9").unwrap();
    assert!(index.is_file());
    for rel in embedded_paths() {
        assert!(version_dir.join(&rel).is_file(), "{} was not restored", rel.display());
    }
}

#[test]
fn an_older_version_folder_is_removed() {
    let dest_root = tempfile::tempdir().unwrap();
    write_help(dest_root.path(), "1.0.0").unwrap();
    assert!(dest_root.path().join("1.0.0").is_dir());

    write_help(dest_root.path(), "2.0.0").unwrap();
    assert!(!dest_root.path().join("1.0.0").exists(), "the old version was not cleaned up");
    assert!(dest_root.path().join("2.0.0").join("index.html").is_file());
}

/// The concurrency case Review Focus calls out: two "clicks" landing at
/// once inside the SAME running app - so the same process id - must not
/// interleave writes to the point where either fails or the folder ends up
/// incomplete.
#[test]
fn concurrent_calls_for_the_same_version_both_succeed() {
    let dest_root = tempfile::tempdir().unwrap();
    let root: std::path::PathBuf = dest_root.path().to_path_buf();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || write_help(&root, "9.9.9"))
        })
        .collect();
    for h in handles {
        h.join().unwrap().unwrap();
    }
    let version_dir = root.join("9.9.9");
    for rel in embedded_paths() {
        assert!(version_dir.join(&rel).is_file(), "{} missing after concurrent writes", rel.display());
    }
}

#[test]
fn the_open_error_names_no_url() {
    assert!(!OPEN_ERROR.to_lowercase().contains("http"), "{OPEN_ERROR}");
}

/// A `.tmp-<pid>-<n>` folder whose pid is not this test process's own is
/// left over from a process that no longer exists (the app is
/// single-instance) - `write_help` sweeps it up the same as an old
/// version folder, rather than leaving `.tmp-` folders forever.
#[test]
fn a_stale_tmp_folder_from_a_dead_process_is_removed() {
    let dest_root = tempfile::tempdir().unwrap();
    let stale = dest_root.path().join("1.2.3.tmp-999999999-0");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("index.html"), b"stale").unwrap();

    write_help(dest_root.path(), "9.9.9").unwrap();
    assert!(!stale.exists(), "the stale temp folder from a dead process was not removed");
}

/// A failure partway through the fresh-write path (here: the destination
/// cannot hold any subfolder at all) must not leave a temp folder behind
/// for `write_help` to have to clean up next time.
#[test]
fn a_failing_write_leaves_no_temp_folder_behind() {
    let base = tempfile::tempdir().unwrap();
    // An ancestor component that is a plain FILE, not a directory - no
    // path under it can ever be created, regardless of the unpredictable
    // pid/sequence-numbered temp name `write_help` picks.
    let blocker = base.path().join("not-a-directory");
    std::fs::write(&blocker, b"x").unwrap();
    let dest_root = blocker.join("help");

    assert!(write_help(&dest_root, "9.9.9").is_err());
    // Nothing was left behind anywhere under `base` - not even an empty
    // temp folder - because the one path that could have held it
    // (`dest_root`, under the file `blocker`) can never exist.
    assert!(!dest_root.exists());
}

/// Debug builds always take the rewrite path (see `write_help`'s doc
/// comment) so a rebuild under an UNCHANGED version is never served stale.
/// `is_complete`'s check is size-only, so same-size-different-content is
/// exactly the case a release build's fast path would miss - this proves
/// the debug build does not. `cargo test` compiles with `debug_assertions`
/// on, so this exercises that branch directly, without needing a release
/// build.
#[test]
fn a_debug_build_rewrites_even_when_only_the_size_matches() {
    let dest_root = tempfile::tempdir().unwrap();
    write_help(dest_root.path(), "9.9.9").unwrap();
    let index_path = dest_root.path().join("9.9.9").join("index.html");
    let real = std::fs::read(&index_path).unwrap();

    std::fs::write(&index_path, vec![b'x'; real.len()]).unwrap();

    write_help(dest_root.path(), "9.9.9").unwrap();
    assert_eq!(
        std::fs::read(&index_path).unwrap(),
        real,
        "stale content under an unchanged version was not rewritten"
    );
}

/// The release-only fast path, exercised directly through
/// `write_help_trusting(.., trust_complete: true)` rather than relying on a
/// release build: a complete folder is left alone - same-size-different-
/// content included - when the caller trusts completeness, the mirror image
/// of `a_debug_build_rewrites_even_when_only_the_size_matches` above.
#[test]
fn a_complete_folder_is_not_rewritten_when_trusting_it() {
    let dest_root = tempfile::tempdir().unwrap();
    write_help_trusting(dest_root.path(), "9.9.9", true).unwrap();
    let index_path = dest_root.path().join("9.9.9").join("index.html");
    let real = std::fs::read(&index_path).unwrap();

    let tampered = vec![b'x'; real.len()];
    std::fs::write(&index_path, &tampered).unwrap();

    write_help_trusting(dest_root.path(), "9.9.9", true).unwrap();
    assert_eq!(
        std::fs::read(&index_path).unwrap(),
        tampered,
        "a complete folder was rewritten even though the caller trusted it"
    );
}
