//! Whether the clone's spec files are there, and a fingerprint that changes
//! when they do.
//!
//! The Playwright header's pill asks this every minute while the tab is
//! shown, so it only looks: a walk of `suites/` for `*.spec.ts` and
//! `*.spec.js`, reading each file's size and modified time and nothing else.
//! No process is started and no file is opened.
//!
//! The fingerprint is a SHA-256 over every spec's path (relative to
//! `suites/`), size and modified time, sorted. Adding, removing or editing a
//! spec changes it; nothing else does. When it differs from the one the
//! webview last processed, the webview reads the clone again.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Said when the clone folder itself is gone. Names no path.
pub const CLONE_MISSING: &str = "The clone folder could not be found. Check that it is still there, then press Refresh.";

/// Said when the clone has no `suites` folder. Names no path. The webview
/// shows this one as "No specs found" (`SUITES_MISSING` in lib/pwSpecs.ts).
pub const SUITES_MISSING: &str = "The clone has no suites folder.";

/// Said when a folder under `suites` could not be read. Names no path.
pub const UNREADABLE: &str = "The spec files could not be read. Check that the clone is not locked by another program, then press Refresh.";

/// Folders the walk never enters (besides those starting with `.`): the
/// installed packages and what a run leaves behind.
const SKIPPED: [&str; 5] = ["node_modules", "test-results", "playwright-report", "allure-results", "blob-report"];

/// What the walk found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct SpecsStatus {
    /// How many `*.spec.ts` / `*.spec.js` files `suites/` holds.
    pub count: u32,
    /// SHA-256 (hex) of each spec's relative path, size and modified time.
    pub fingerprint: String,
}

fn is_spec(name: &str) -> bool {
    name.ends_with(".spec.ts") || name.ends_with(".spec.js")
}

fn skipped(name: &str) -> bool {
    name.starts_with('.') || SKIPPED.contains(&name)
}

/// One line per spec under `dir`, each `<relative path>:<size>:<modified>`.
/// A link is never followed. Any folder that cannot be read fails the walk.
fn walk(dir: &Path, rel: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|_| UNREADABLE.to_string())?;
    for e in entries {
        let e = e.map_err(|_| UNREADABLE.to_string())?;
        let name = e.file_name().to_string_lossy().to_string();
        let kind = e.file_type().map_err(|_| UNREADABLE.to_string())?;
        let path: PathBuf = e.path();
        if kind.is_dir() {
            if !skipped(&name) {
                walk(&path, &rel.join(&name), out)?;
            }
        } else if kind.is_file() && is_spec(&name) {
            let m = e.metadata().map_err(|_| UNREADABLE.to_string())?;
            let at = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            out.push(format!("{}:{}:{at}", rel.join(&name).to_string_lossy().replace('\\', "/"), m.len()));
        }
    }
    Ok(())
}

/// Counts and fingerprints the specs under `clone/suites`.
pub fn read(clone: &Path) -> Result<SpecsStatus, String> {
    if !clone.is_dir() {
        return Err(CLONE_MISSING.to_string());
    }
    let suites = clone.join("suites");
    if !suites.is_dir() {
        return Err(SUITES_MISSING.to_string());
    }
    let mut lines = Vec::new();
    walk(&suites, Path::new(""), &mut lines)?;
    lines.sort();
    let fingerprint = Sha256::digest(lines.join("\n").as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    Ok(SpecsStatus { count: lines.len() as u32, fingerprint })
}
