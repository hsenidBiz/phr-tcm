//! New folder: a feature folder made in the clone from the app, so an
//! export (or the repo's own `/gen-test`) has somewhere to go.
//!
//! A feature is a folder under `suites/` that holds `specs/`, `test-cases/`,
//! `pages/` or `data/` (`layout`). Git keeps no empty folder, and the repo
//! keeps its own empty ones with a `.gitkeep`, so a new feature gets
//! `test-cases/.gitkeep` and `specs/.gitkeep`: it is in the Overview and
//! offered as an export destination at once, and survives a commit.
//!
//! Writes only those files and the folders above them, never over anything,
//! and runs no git, npm or Playwright.

use super::layout::CONTENT_DIRS;
use crate::pw_export::mapping::Placement;
use std::path::Path;

/// The content folders a new feature starts with, each holding a `.gitkeep`.
pub const STARTED_WITH: [&str; 2] = ["test-cases", "specs"];

/// What `create_feature` wrote.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct NewFolder {
    /// The feature's path under `suites/`, as it was made.
    pub path: String,
    /// The files written, relative to the clone, forward slashes.
    pub files: Vec<String>,
}

/// Makes the feature folder `path` (folders under `suites/`, `/` between
/// them, a trailing slash or space ignored) in the clone at `clone`.
/// Refused, with nothing written, for a path an export would refuse
/// (`Placement::validate`), a folder named like a content folder, a folder
/// that differs from one already there only in capitals, anything in the
/// way that is not a plain folder, and a feature that is already there.
pub fn create_feature(clone: &Path, path: &str) -> Result<NewFolder, String> {
    let path = path.trim().trim_end_matches(['/', ' ']).to_string();
    Placement { path: path.clone() }.validate()?;
    let folders: Vec<&str> = path.split('/').collect();
    if let Some(f) = folders.iter().find(|f| CONTENT_DIRS.contains(f)) {
        return Err(format!(
            "The path \"{path}\" cannot be used: \"{f}\" is one of the folders inside a feature ({}).",
            CONTENT_DIRS.join(", ")
        ));
    }
    let suites = clone.join("suites");
    if !suites.is_dir() {
        return Err("The clone has no suites folder, so it is not the Playwright repo.".into());
    }

    let mut dir = suites.clone();
    for f in &folders {
        if let Some(other) = sibling_in_other_case(&dir, f) {
            return Err(format!(
                "\"{other}\" is already there, and \"{f}\" differs from it only in capitals. Use \"{other}\", or another name."
            ));
        }
        dir = dir.join(f);
        match std::fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(format!("\"{f}\" in \"{path}\" is already there and is not a folder.")),
            Err(_) => {}
        }
    }
    if CONTENT_DIRS.iter().any(|c| dir.join(c).exists()) {
        return Err(format!("\"{path}\" is already a feature folder."));
    }

    let mut files = Vec::new();
    for c in STARTED_WITH {
        let sub = dir.join(c);
        std::fs::create_dir_all(&sub).map_err(|e| format!("Could not create suites/{path}/{c}: {e}"))?;
        let keep = sub.join(".gitkeep");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&keep)
            .map_err(|e| format!("Could not write suites/{path}/{c}/.gitkeep: {e}"))?;
        files.push(format!("suites/{path}/{c}/.gitkeep"));
    }
    Ok(NewFolder { path, files })
}

/// A folder in `dir` named like `name` but for capitals, if any: on Windows
/// the two are the same folder, and the repo would hold one name or the other.
fn sibling_in_other_case(dir: &Path, name: &str) -> Option<String> {
    let rd = std::fs::read_dir(dir).ok()?;
    rd.flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .find(|n| n != name && n.eq_ignore_ascii_case(name))
}

