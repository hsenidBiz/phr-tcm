//! What the clone holds, and whether the tab can work with it.
//!
//! - The features: every folder under `suites/` that holds `specs/`,
//!   `test-cases/`, `pages/` or `data/`, at any depth (`layout`), with how many specs it has and the test case ids of the `## <id>`
//!   headings in its `test-cases/*.md`.
//! - The generated raw specs' ids, from `suites/_generated/index.json`.
//! - The KEYS of `src/users/users.json` (never its values, which hold
//!   passwords) and of `src/navigation.json`, both through
//!   `pw_export::clone::open`.
//! - The health line: anything that stops the rest of the tab working.
//!
//! Read only. Node is looked for on PATH; nothing is started.

use super::layout;
use crate::pw_export::clone::{self, NOT_A_CLONE};
use serde::Serialize;
use std::ffi::OsStr;
use std::path::Path;

/// One thing that stops the tab working, in the order the health line shows
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
pub enum HealthIssue {
    /// The folder is not a PHR-PLAYWRIGHT-AUTOMATION clone (or none is set).
    NotTheRepo,
    /// The clone has no `node_modules`: `npm install` has not been run there.
    NoNodeModules,
    /// No `node` on PATH.
    NoNode,
    /// No `qa-options.json` at the clone's root: the runner uses its defaults.
    NoQaOptions,
}

/// One feature folder under `suites/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct RepoFeature {
    /// The folders from `suites/` down to it.
    pub path: Vec<String>,
    /// The key of its `navigation.json` entry (`layout::navigation_key`).
    pub navigation_key: String,
    /// `*.spec.ts` files in its `specs/`.
    pub spec_count: u32,
    /// Test case sections (`## <id>` headings) in its `test-cases/*.md`.
    pub case_count: u32,
    /// Those sections' ids, in file order, each once.
    pub case_ids: Vec<i32>,
}

/// The clone as the Playwright tab shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct RepoStructure {
    /// Sorted by path.
    pub features: Vec<RepoFeature>,
    /// The ids `suites/_generated/index.json` maps to a raw spec, in its order.
    pub generated_ids: Vec<i32>,
    /// The account keys of `users.json`. Keys only.
    pub user_keys: Vec<String>,
    /// The entries of `navigation.json`.
    pub navigation_keys: Vec<String>,
    /// Empty when everything the tab needs is there.
    pub health: Vec<HealthIssue>,
}

/// Reads the clone at `root`. A folder that is not the repo is not an error:
/// it is answered with `NotTheRepo` and nothing else read. A repo whose own
/// files are malformed is an error that names the file.
/// `node_found` is `node_found()` in the app; tests pass it.
pub fn read(root: &Path, node_found: bool) -> Result<RepoStructure, String> {
    let mut out = RepoStructure {
        features: vec![],
        generated_ids: vec![],
        user_keys: vec![],
        navigation_keys: vec![],
        health: vec![],
    };
    let repo = if root.as_os_str().is_empty() {
        None
    } else {
        match clone::open(root) {
            Ok(r) => Some(r),
            Err(e) if e == NOT_A_CLONE => None,
            Err(e) => return Err(e),
        }
    };
    let Some(repo) = repo else {
        out.health.push(HealthIssue::NotTheRepo);
        if !node_found {
            out.health.push(HealthIssue::NoNode);
        }
        return Ok(out);
    };

    if !root.join("node_modules").is_dir() {
        out.health.push(HealthIssue::NoNodeModules);
    }
    if !node_found {
        out.health.push(HealthIssue::NoNode);
    }
    if !root.join("qa-options.json").is_file() {
        out.health.push(HealthIssue::NoQaOptions);
    }

    out.generated_ids = repo.index.iter().filter_map(|(id, _)| id.parse().ok()).collect();
    out.user_keys = repo.user_keys;
    out.navigation_keys = repo.navigation_keys;
    out.features = features(&root.join("suites"));
    Ok(out)
}

/// File names in `dir` ending in `suffix`, sorted.
fn files_ending(dir: &Path, suffix: &str) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.ends_with(suffix))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn features(suites: &Path) -> Vec<RepoFeature> {
    layout::feature_dirs(suites)
        .into_iter()
        .map(|path| {
            let f = path.iter().fold(suites.to_path_buf(), |p, part| p.join(part));
            let spec_count = files_ending(&f.join("specs"), ".spec.ts").len() as u32;
            let mut case_ids: Vec<i32> = Vec::new();
            let cases = f.join("test-cases");
            for name in files_ending(&cases, ".md") {
                let text = std::fs::read_to_string(cases.join(&name)).unwrap_or_default();
                for id in case_headings(&text) {
                    if !case_ids.contains(&id) {
                        case_ids.push(id);
                    }
                }
            }
            RepoFeature {
                navigation_key: layout::navigation_key(&path),
                path,
                spec_count,
                case_count: case_ids.len() as u32,
                case_ids,
            }
        })
        .collect()
}

/// Whether `rel` (relative to the clone, forward slashes) is a test-case
/// file the feature walk reads: `suites/<path>/test-cases/<name>.md`
/// (`layout::test_case_file`).
pub fn feature_test_cases(rel: &str) -> bool {
    layout::test_case_file(rel).is_some()
}

/// The ids of a test-case file's `## <id>` headings, in order: a level-two
/// heading whose first word is all digits. `## Amendment`, `## Steps` and
/// deeper headings are not test cases, and nor is `## Excluded <id>`
/// (`excluded_headings`): the summary counts the cases the repo tests.
pub fn case_headings(text: &str) -> Vec<i32> {
    headings(text, "")
}

/// The ids of a test-case file's `## Excluded <id>` headings, in order: the
/// cases the team left out of the repo on purpose. `Excluded` is the only
/// such word the repo uses.
pub fn excluded_headings(text: &str) -> Vec<i32> {
    headings(text, "Excluded ")
}

/// The ids of the level-two headings that start with `prefix` and then a
/// word that is all digits.
fn headings(text: &str, prefix: &str) -> Vec<i32> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim_end_matches('\r').strip_prefix("## ")?.strip_prefix(prefix)?;
            let word: &str = rest.split_whitespace().next()?;
            if word.is_empty() || !word.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            word.parse().ok()
        })
        .collect()
}

/// Whether a `node` executable is on `path` (a PATH value). On Windows the
/// name is tried with each of `pathext`'s extensions; elsewhere bare. Only
/// looks: nothing is run.
pub fn node_on_path(path: Option<&OsStr>, pathext: Option<&OsStr>) -> bool {
    let Some(path) = path else { return false };
    let names: Vec<String> = if cfg!(windows) {
        let exts = pathext
            .map(|p| p.to_string_lossy().to_string())
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
        exts.split(';').map(str::trim).filter(|e| !e.is_empty()).map(|e| format!("node{e}")).collect()
    } else {
        vec!["node".to_string()]
    };
    std::env::split_paths(path).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
}

/// `node_on_path` for this process's PATH and PATHEXT.
pub fn node_found() -> bool {
    node_on_path(std::env::var_os("PATH").as_deref(), std::env::var_os("PATHEXT").as_deref())
}

/// The command's body: refused where the tab is not offered, else `read`.
pub fn structure_for(offered: bool, clone: &str, node_found: bool) -> Result<RepoStructure, String> {
    super::ensure_offered(offered)?;
    read(Path::new(clone.trim()), node_found)
}
