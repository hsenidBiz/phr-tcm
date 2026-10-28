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
//!
//! The same look checks the rule files the script pipeline follows (the
//! agent files, `gen-test`, the skills and the repo's `CLAUDE.md`, as
//! `autorun::pw_rules` reads them): which the clone holds, which it lacks,
//! and a fingerprint of their own. Those are what the pill counts.

use crate::autorun::pw_rules::{plain_skill_name, READ_LIMIT, SKILLS_DIR, SKILL_FILE, SNAPSHOT};
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
    /// The rule files the script pipeline follows.
    pub pipeline: PipelineFiles,
}

/// The rule files the script pipeline follows, as the clone holds them.
/// Each is named by its path as the repo lays it out
/// (`.claude/agents/playwright-test-healer.md`, `CLAUDE.md`) - never a
/// clone path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct PipelineFiles {
    /// The ones the clone holds, sorted: every shipped rule file it has, and
    /// any skill of its own.
    pub found: Vec<String>,
    /// Shipped rule files the clone does not hold, sorted. The copy shipped
    /// with the app stands in for each.
    pub missing: Vec<String>,
    /// SHA-256 (hex) of each found file's path, size and modified time.
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

/// `<size>:<modified>` of `rel` in `clone` when it is a regular file no
/// bigger than a rule file can be (the size `pw_rules` reads up to), else
/// `None`. Metadata only: the file is not opened.
fn rule_file(clone: &Path, rel: &str) -> Option<String> {
    let m = std::fs::metadata(clone.join(rel)).ok()?;
    if !m.is_file() || m.len() > READ_LIMIT {
        return None;
    }
    let at = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some(format!("{}:{at}", m.len()))
}

/// Which of the pipeline's rule files `clone` holds: every file the app
/// ships a copy of, plus each skill folder of the clone's own.
pub fn pipeline_files(clone: &Path) -> PipelineFiles {
    let mut wanted: Vec<String> = SNAPSHOT.iter().map(|(k, _)| (*k).to_string()).collect();
    if let Ok(entries) = std::fs::read_dir(clone.join(SKILLS_DIR)) {
        for e in entries.flatten() {
            let Some(name) = e.file_name().to_str().map(str::to_string) else { continue };
            if plain_skill_name(&name) && e.path().is_dir() {
                wanted.push(format!("{SKILLS_DIR}/{name}/{SKILL_FILE}"));
            }
        }
    }
    wanted.sort();
    wanted.dedup();
    let (mut found, mut missing, mut lines) = (Vec::new(), Vec::new(), Vec::new());
    for rel in wanted {
        match rule_file(clone, &rel) {
            Some(meta) => {
                lines.push(format!("{rel}:{meta}"));
                found.push(rel);
            }
            // A skill of the clone's own with no SKILL.md is no rule file;
            // only a shipped one is missing.
            None if SNAPSHOT.iter().any(|(k, _)| *k == rel) => missing.push(rel),
            None => {}
        }
    }
    let fingerprint = Sha256::digest(lines.join("\n").as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    PipelineFiles { found, missing, fingerprint }
}

/// Counts and fingerprints the specs under `clone/suites`, and checks the
/// pipeline's rule files.
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
    Ok(SpecsStatus { count: lines.len() as u32, fingerprint, pipeline: pipeline_files(clone) })
}
