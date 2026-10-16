//! The folder rules of the team's Playwright repo, in one place.
//!
//! A **feature** is any folder under `suites/` that directly holds `specs/`,
//! `test-cases/`, `pages/` or `data/`; its path is the folders from `suites/`
//! down to it, any length. The first folder names the Playwright project
//! (uppercased; `common-components` runs under `SL`). Folders starting with
//! `_` or `.`, and `node_modules`, are never part of the tree.

use std::path::Path;

/// The shared tree's folder name.
pub const SHARED: &str = "common-components";

/// The folders that make their parent a feature.
pub const CONTENT_DIRS: [&str; 4] = ["specs", "test-cases", "pages", "data"];

/// Characters a folder or file name never has: Windows refuses all but the
/// backtick and quote, which would break the Finish in the repo prompt.
const REFUSED: [char; 11] = ['/', '\\', '<', '>', ':', '"', '|', '?', '*', '`', '\''];

/// Names Windows reserves for devices, with or without an extension.
const DEVICES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2",
    "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Why `s` cannot be a folder (or file stem) the app writes into the repo
/// and names in a prompt, or `None` when it can. Any name the repo itself
/// could hold is fine - capitals, digits, `_`, `.`, spaces - except one
/// the tree never reads (`skipped`) or that Windows refuses or alters.
pub fn name_problem(s: &str) -> Option<String> {
    if s.is_empty() {
        return Some("a folder name is empty".into());
    }
    if s == "." || s == ".." {
        return Some(format!("\"{s}\" is not a folder name"));
    }
    if skipped(s) {
        return Some(format!("\"{s}\" starts with _ or . (or is node_modules), so the repo's tree never reads it"));
    }
    if s.starts_with(' ') || s.ends_with(' ') || s.ends_with('.') {
        return Some(format!("\"{s}\" starts or ends with a space, or ends with a dot, which Windows drops"));
    }
    if let Some(c) = s.chars().find(|c| REFUSED.contains(c) || c.is_control()) {
        let shown = if c.is_control() { "a control character".to_string() } else { format!("{c}") };
        return Some(format!("\"{s}\" has {shown}, which a folder name cannot have"));
    }
    let base = s.split('.').next().unwrap_or(s).trim_end().to_ascii_lowercase();
    if DEVICES.contains(&base.as_str()) {
        return Some(format!("\"{s}\" is a name Windows reserves"));
    }
    None
}

/// Whether `s` is a valid folder name (`name_problem` has nothing to say).
pub fn valid_name(s: &str) -> bool {
    name_problem(s).is_none()
}

/// Whether a folder of this name is never part of the tree.
pub fn skipped(name: &str) -> bool {
    name.starts_with('_') || name.starts_with('.') || name == "node_modules"
}

/// Where a spec file sits: the feature folder's path and the file's stem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecLocation {
    pub path: Vec<String>,
    pub stem: String,
}

fn parts(file: &str) -> Vec<&str> {
    file.split(['/', '\\']).filter(|p| !p.is_empty()).collect()
}

fn path_ok(path: &[&str]) -> bool {
    !path.is_empty() && path.iter().all(|p| *p != ".." && *p != "." && !skipped(p))
}

/// Reads `file` (relative to `suites/` or to the clone; a leading `suites/`
/// is dropped; `/` or `\`) as `<folders>/specs/<anything>/<stem>.spec.ts`:
/// the LAST `specs` folder is the one, so a spec may sit in folders below it.
pub fn spec_location(file: &str) -> Option<SpecLocation> {
    let mut p = parts(file);
    if p.first() == Some(&"suites") {
        p.remove(0);
    }
    let name = p.pop()?;
    let stem = name.strip_suffix(".spec.ts").filter(|s| !s.is_empty())?;
    let at = p.iter().rposition(|s| *s == "specs")?;
    p.truncate(at);
    if !path_ok(&p) {
        return None;
    }
    Some(SpecLocation { path: p.iter().map(|s| s.to_string()).collect(), stem: stem.to_string() })
}

/// The last two folders of a feature's path, joined with `/`: the key of its
/// `navigation.json` entry.
pub fn navigation_key(path: &[String]) -> String {
    let from = path.len().saturating_sub(2);
    path[from..].join("/")
}

/// The Playwright project a feature runs under: its first folder uppercased,
/// `SL` for the shared tree.
pub fn owning_project(path: &[String]) -> String {
    match path.first().map(String::as_str) {
        None | Some(SHARED) => "SL".to_string(),
        Some(first) => first.to_uppercase(),
    }
}

/// Every feature folder's path under `suites`, sorted. A content folder is
/// never walked into, nor a skipped one or a link; folders below a feature are.
pub fn feature_dirs(suites: &Path) -> Vec<Vec<String>> {
    fn walk(dir: &Path, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        if !path.is_empty() && names.iter().any(|n| CONTENT_DIRS.contains(&n.as_str())) {
            out.push(path.clone());
        }
        for n in names {
            if skipped(&n) || CONTENT_DIRS.contains(&n.as_str()) {
                continue;
            }
            path.push(n.clone());
            walk(&dir.join(&n), path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(suites, &mut Vec::new(), &mut out);
    out.sort();
    out
}

/// For `suites/<path>/test-cases/<name>.md` (relative to the clone, `/`),
/// the feature path; `None` for anything else.
pub fn test_case_file(rel: &str) -> Option<Vec<String>> {
    let mut p: Vec<&str> = rel.split('/').collect();
    if p.first() != Some(&"suites") {
        return None;
    }
    p.remove(0);
    let name = p.pop()?;
    if !name.ends_with(".md") || p.pop()? != "test-cases" || !path_ok(&p) {
        return None;
    }
    Some(p.iter().map(|s| s.to_string()).collect())
}
