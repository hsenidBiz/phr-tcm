//! Embeds `src-tauri/help/` (the built "How To Use" site - see
//! docs/superpowers/specs/2026-09-26-help-site-design.md) and stages it on
//! disk so `tauri_plugin_opener::open_path` has a real filesystem path to
//! hand the default browser - a webview cannot serve the embedded bytes and
//! the opener cannot open them directly.
//!
//! `write_help` never assumes what the site embeds beyond `index.html`:
//! Task 1 shipped the site with no images, a later task adds
//! `img/light|dark/*.jpg`, and this module walks whatever is actually
//! there.

use include_dir::{include_dir, Dir, DirEntry};
use std::path::{Path, PathBuf};

/// The built site, embedded at compile time. Missing `src-tauri/help/`
/// fails the build - there would be nothing to ship.
static HELP: Dir = include_dir!("$CARGO_MANIFEST_DIR/help");

/// `open_help`'s failure message. Names no URL or path, per the app's rule
/// that user-facing errors never do (see `ado/transport.rs`).
pub const OPEN_ERROR: &str = "Could not open the help pages. Settings, Logs has the details.";

/// Gives each temp folder in `write_help` a name distinct from every other
/// call in this process, not just every other process - two rapid clicks
/// on the same running app both go through the same pid, and colliding on
/// one temp folder would let them interleave.
static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many times the final rename is retried before `write_help` gives up.
const RENAME_ATTEMPTS: u32 = 5;

/// True when `root` already holds every file embedded in `dir` (recursed),
/// each the same byte size as its embedded copy. Extra files under `root`
/// do not fail this - a stray file left inside a version folder is
/// harmless, and `write_help` cleans up SIBLING version folders on its own.
fn is_complete(root: &Path, dir: &Dir) -> bool {
    dir.entries().iter().all(|entry| match entry {
        DirEntry::Dir(sub) => is_complete(root, sub),
        DirEntry::File(file) => std::fs::metadata(root.join(file.path()))
            .map(|meta| meta.len() == file.contents().len() as u64)
            .unwrap_or(false),
    })
}

/// Writes every embedded file under `root`, creating parent folders as
/// needed. Safe to call against a folder that already holds some or all of
/// these files - it always writes the same fixed bytes this build embeds,
/// so a redundant write changes nothing.
fn write_files(root: &Path, dir: &Dir) -> std::io::Result<()> {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(sub) => write_files(root, sub)?,
            DirEntry::File(file) => {
                let path = root.join(file.path());
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, file.contents())?;
            }
        }
    }
    Ok(())
}

/// The pid segment of a `<version>.tmp-<pid>-<seq>` folder name, if `name`
/// has that shape.
fn tmp_owner_pid(name: &str) -> Option<u32> {
    name.split_once(".tmp-")?.1.split('-').next()?.parse().ok()
}

/// Removes every entry directly under `dest_root` except `keep` - old
/// installed-version folders, and any `.tmp-` folder that is not this
/// PROCESS's own (the app is single-instance, so a `.tmp-<pid>-*` folder
/// whose pid is not this one belongs to a process that no longer exists -
/// there is never a second live process to still be using it). A `.tmp-`
/// folder that IS this process's own is left alone: it may be another
/// in-flight call's temp folder, and only that call should remove it.
/// Best-effort throughout: a file still open elsewhere is left for next
/// time rather than failing the call that just succeeded.
fn cleanup_other_versions(dest_root: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(dest_root) else { return };
    let this_pid = std::process::id();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == keep {
            continue;
        }
        if tmp_owner_pid(&name) == Some(this_pid) {
            continue;
        }
        let path = entry.path();
        let _ = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
    }
}

/// Ensures `<dest_root>/<version>` holds every file this build embeds, and
/// returns the path of its `index.html`.
///
/// Three cases:
/// - **Already complete** (the common case: this version was opened
///   before) - release builds only: nothing is written. Debug builds skip
///   this and always fall through to one of the cases below, because the
///   version string usually does not change between rebuilds during
///   development, and `is_complete`'s size-only check would otherwise
///   serve a stale site whenever a rebuild changed file CONTENTS without
///   changing their size.
/// - **Missing entirely**: built fully in a private `<version>.tmp-<pid>-<n>`
///   folder first, then renamed into place in one atomic step, so a second,
///   concurrent call for the SAME version never sees - or opens - a
///   half-written folder. Windows refuses to rename onto a folder that
///   already exists, so if another call's rename won that race first, this
///   one finds `version_dir` complete and treats that as success, removing
///   its own temp folder instead of erroring. The rename is also retried a
///   few times with a short sleep before that check, since an antivirus
///   scanner can briefly hold a handle open on a file just written
///   (`ERROR_ACCESS_DENIED`) well after the write itself succeeded.
/// - **Exists but incomplete** (someone deleted a file from it by hand
///   after an earlier complete write - the only way it changes once
///   created, since it is only ever populated by the atomic rename above -
///   or a debug build, per above): every embedded file is rewritten
///   straight into it, not only the missing ones - `write_files` has no
///   way to tell "missing" from "present but stale" apart, so it always
///   writes the fixed bytes this build embeds. This is safe even if
///   another call is doing the same repair at the same time, because both
///   would write the exact same bytes.
///
/// Old version folders under `dest_root` are removed once this version's
/// folder is confirmed complete. Every error return leaves no temp folder
/// behind.
pub fn write_help(dest_root: &Path, version: &str) -> std::io::Result<PathBuf> {
    write_help_trusting(dest_root, version, !cfg!(debug_assertions))
}

/// `write_help`'s implementation, with the release-only "already complete,
/// so skip" fast path exposed as `trust_complete` rather than hardcoded to
/// `!cfg!(debug_assertions)`, so a test can exercise that branch directly
/// instead of only ever seeing the always-rewrite debug behaviour that
/// `cargo test`'s own `debug_assertions` would otherwise force.
pub fn write_help_trusting(
    dest_root: &Path,
    version: &str,
    trust_complete: bool,
) -> std::io::Result<PathBuf> {
    let version_dir = dest_root.join(version);
    let index = version_dir.join("index.html");

    if trust_complete && is_complete(&version_dir, &HELP) {
        cleanup_other_versions(dest_root, version);
        return Ok(index);
    }

    if version_dir.is_dir() {
        write_files(&version_dir, &HELP)?;
        cleanup_other_versions(dest_root, version);
        return Ok(index);
    }

    let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_dir = dest_root.join(format!("{version}.tmp-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err(e);
    }
    if let Err(e) = write_files(&tmp_dir, &HELP) {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err(e);
    }

    let mut rename_result: std::io::Result<()> = Ok(());
    for attempt in 0..RENAME_ATTEMPTS {
        match std::fs::rename(&tmp_dir, &version_dir) {
            Ok(()) => {
                rename_result = Ok(());
                break;
            }
            Err(e) => {
                // Another concurrent call may have finished first - that
                // is success, not a failure to report, and there is no
                // point retrying a rename whose target already exists.
                if is_complete(&version_dir, &HELP) {
                    let _ = std::fs::remove_dir_all(&tmp_dir);
                    rename_result = Ok(());
                    break;
                }
                rename_result = Err(e);
                if attempt + 1 < RENAME_ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }
    if let Err(e) = rename_result {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err(e);
    }

    cleanup_other_versions(dest_root, version);
    Ok(index)
}
