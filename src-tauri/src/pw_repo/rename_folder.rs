//! Rename folder: a folder under the clone's `suites/` renamed from the
//! Overview, with a plain warning of what the rename would break.
//!
//! The app never rewrites the repo's source files to mend an import. It
//! renames the folder (`std::fs::rename`, nothing deleted) and says, before
//! it does, which files point into the folder or out of it by a relative
//! path, which of `playwright.config.*` and `package.json` mention its path,
//! and which of the app's own Export to Playwright settings name it - those
//! the app does update itself.
//!
//! Files are read as text, never through git, and only the kinds a relative
//! import can sit in (`.ts .js .mjs .cjs .json`), skipping `node_modules`,
//! `.git` and build output. Every name shown is relative to the clone.

use super::layout::{self, CONTENT_DIRS};
use crate::pw_export::mapping::{self, Placement};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The kinds of file that can import something by a relative path.
const SCANNED: [&str; 5] = ["ts", "js", "mjs", "cjs", "json"];

/// Folders never read: dependencies, git's own data and build output.
const SKIPPED_DIRS: [&str; 9] =
    ["node_modules", ".git", "dist", "build", "out", "playwright-report", "test-results", "coverage", ".next"];

/// A file bigger than this is a report or a lock file, not source.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// One thing a rename would break.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct RenameBreak {
    /// The file, relative to the clone, forward slashes.
    pub file: String,
    /// A short plain sentence saying what it does with the folder.
    pub what: String,
}

/// What renaming a folder would break, and what the app updates itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct RenameCheck {
    /// The folder's path under `suites/`, as it is now.
    pub from: String,
    /// The path it would have.
    pub to: String,
    /// Files in the repo that would stop working until they are fixed.
    pub breaks: Vec<RenameBreak>,
    /// What the app itself changes with the folder, one sentence each.
    pub tcm_updates: Vec<String>,
}

/// What `rename` did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct RenameDone {
    pub from: String,
    pub to: String,
    /// How many Export to Playwright placements were moved with the folder.
    pub placements_updated: u32,
}

/// The folders of a path under `suites/`, checked: no way out of the tree,
/// every name one the app can write (`layout::name_problem`).
fn parts(raw: &str) -> Result<Vec<String>, String> {
    let p = raw.trim().trim_end_matches(['/', ' ']);
    if p.is_empty() {
        return Ok(vec![]);
    }
    let escapes = p.starts_with('/') || p.starts_with('\\') || p.contains(':') || p.contains('\\');
    let folders: Vec<&str> = p.split('/').collect();
    if escapes || folders.iter().any(|f| *f == ".." || *f == ".") {
        return Err(format!("The path \"{p}\" is not a folder under suites."));
    }
    for f in &folders {
        if let Some(why) = layout::name_problem(f) {
            return Err(format!("The path \"{p}\" cannot be used: {why}."));
        }
    }
    Ok(folders.into_iter().map(str::to_string).collect())
}

/// The folder at `rel` under `suites`, if every step of it is a real folder
/// (not a link, not a file).
fn real_dir(suites: &Path, rel: &[String]) -> bool {
    let mut at = suites.to_path_buf();
    for f in rel {
        at.push(f);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            _ => return false,
        }
    }
    true
}

/// The folder paths of `from` and `to`, once the rename is allowed at all.
/// Nothing is written.
fn validate(clone: &Path, from: &str, to: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let suites = clone.join("suites");
    if !suites.is_dir() {
        return Err("The clone has no suites folder, so it is not the Playwright repo.".into());
    }
    let from_p = parts(from)?;
    let to_p = parts(to)?;
    if from_p.is_empty() {
        return Err("The suites folder itself cannot be renamed.".into());
    }
    if to_p.is_empty() {
        return Err("The new name is empty.".into());
    }
    let (from_s, to_s) = (from_p.join("/"), to_p.join("/"));
    if let Some(f) = from_p.iter().find(|f| CONTENT_DIRS.contains(&f.as_str())) {
        return Err(format!(
            "\"{f}\" is one of the folders inside a feature ({}), so \"{from_s}\" is not renamed here.",
            CONTENT_DIRS.join(", ")
        ));
    }
    if let Some(f) = to_p.iter().find(|f| CONTENT_DIRS.contains(&f.as_str())) {
        return Err(format!(
            "The path \"{to_s}\" cannot be used: \"{f}\" is one of the folders inside a feature ({}).",
            CONTENT_DIRS.join(", ")
        ));
    }
    if from_p.len() >= 2 && to_p.len() < 2 {
        return Err(format!("\"{from_s}\" cannot move to the top level of suites."));
    }
    let inside = to_p.len() > from_p.len() && to_p.iter().zip(&from_p).all(|(a, b)| a.eq_ignore_ascii_case(b));
    if inside {
        return Err(format!("\"{from_s}\" cannot be moved into itself."));
    }
    if !real_dir(&suites, &from_p) {
        return Err(format!("\"{from_s}\" is not a folder in the clone (any more). Press Refresh and try again."));
    }
    if from_s == to_s {
        return Err("The new name is the same as the old one.".into());
    }
    // Where it would land: that folder must be there, and no other thing may
    // already hold the name (a name differing only in capitals is the same
    // folder on Windows, unless it is this folder itself being recapitalised).
    let (to_parent, to_name) = to_p.split_at(to_p.len() - 1);
    let to_name = &to_name[0];
    if !real_dir(&suites, to_parent) {
        return Err(format!("There is no folder \"{}\" to put \"{to_name}\" in.", to_parent.join("/")));
    }
    let mut dir = suites.clone();
    for f in to_parent {
        dir.push(f);
    }
    let same_parent = from_p[..from_p.len() - 1] == *to_parent;
    let from_name = from_p.last().cloned().unwrap_or_default();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n == *to_name {
                return Err(format!("\"{to_s}\" is already there, so \"{from_s}\" cannot be renamed to it."));
            }
            if n.eq_ignore_ascii_case(to_name) && !(same_parent && n == from_name) {
                return Err(format!(
                    "\"{n}\" is already there, and \"{to_name}\" differs from it only in capitals. Use another name."
                ));
            }
        }
    }
    Ok((from_p, to_p))
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// `parts` joined, lowercased: the key a path is compared by (capitals do not
/// tell two folders apart on Windows).
fn key(parts: &[String]) -> String {
    lower(&parts.join("/"))
}

/// `base` (a folder, as parts relative to the clone) followed by the relative
/// `spec` (`./x`, `../y/z`), settled lexically. `None` when it leaves the
/// clone.
fn resolve(base: &[String], spec: &str) -> Option<Vec<String>> {
    let mut out: Vec<String> = base.to_vec();
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            s => out.push(s.to_string()),
        }
    }
    Some(out)
}

fn is_relative(spec: &str) -> bool {
    spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../")
}

/// Whether `target` is `folder` or something inside it.
fn within(target: &[String], folder: &[String]) -> bool {
    target.len() >= folder.len() && target.iter().zip(folder).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// `at` with the leading `from` swapped for `to`.
fn moved(at: &[String], from: &[String], to: &[String]) -> Vec<String> {
    let mut out = to.to_vec();
    out.extend_from_slice(&at[from.len()..]);
    out
}

/// The relative paths a source file imports or requires, and a JSON file's
/// string values that look like relative paths.
fn specifiers(text: &str, json: bool) -> Vec<String> {
    static CODE: OnceLock<regex::Regex> = OnceLock::new();
    static STRING: OnceLock<regex::Regex> = OnceLock::new();
    let re = if json {
        STRING.get_or_init(|| regex::Regex::new(r#""([^"\r\n]*)""#).expect("a valid pattern"))
    } else {
        CODE.get_or_init(|| {
            regex::Regex::new(r#"(?:\bfrom|\bimport|\brequire)\s*\(?\s*['"`]([^'"`\r\n]+)['"`]"#).expect("a valid pattern")
        })
    };
    re.captures_iter(text).filter_map(|c| c.get(1)).map(|m| m.as_str().to_string()).filter(|s| is_relative(s)).collect()
}

/// Whether `text` names the folder: `suites/<path>`, or for a path of two
/// folders or more the path itself, each as a whole name (not a prefix of a
/// longer one).
fn mentions(text: &str, from: &[String]) -> bool {
    let text = lower(&text.replace("\\\\", "/").replace('\\', "/"));
    let path = key(from);
    let name_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    let found = |needle: &str, dotted_before: bool| {
        text.match_indices(needle).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + needle.len()..].chars().next();
            let before_ok = before.is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '-')) && (dotted_before || c != '.'));
            before_ok && after.is_none_or(|c| !name_char(c))
        })
    };
    found(&format!("suites/{path}"), true) || (from.len() >= 2 && found(&path, false))
}

/// Every file under `dir` a rename could break, with its path as parts
/// relative to the clone.
fn files(clone: &Path) -> Vec<(Vec<String>, PathBuf)> {
    fn walk(dir: &Path, rel: &mut Vec<String>, out: &mut Vec<(Vec<String>, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(t) = e.file_type() else { continue };
            if t.is_symlink() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if t.is_dir() {
                if SKIPPED_DIRS.contains(&name.as_str()) {
                    continue;
                }
                rel.push(name);
                walk(&e.path(), rel, out);
                rel.pop();
            } else if t.is_file() {
                let ext = name.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_default();
                if SCANNED.contains(&ext.as_str()) && name != "package-lock.json" {
                    let mut at = rel.clone();
                    at.push(name);
                    out.push((at, e.path()));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(clone, &mut Vec::new(), &mut out);
    out
}

/// The app's own settings that name `from` (or something in it), one
/// sentence each, with the Export to Playwright files holding them.
fn placements_in(maps_root: Option<&Path>, from: &[String]) -> Vec<(PathBuf, mapping::ExportMap)> {
    let Some(root) = maps_root else { return vec![] };
    mapping::files(root)
        .into_iter()
        .filter_map(|p| match mapping::load_at(&p) {
            Ok(m) => Some((p, m)),
            Err(_) => {
                // The error names the file's full path: kept out of the log.
                crate::applog::warn("pw-repo: an export settings file could not be read for a rename");
                None
            }
        })
        .filter(|(_, m)| {
            m.areas.values().chain(m.cases.values()).any(|p| within(&split(&p.path), from))
        })
        .collect()
}

fn split(path: &str) -> Vec<String> {
    path.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// What renaming `from` to `to` (both paths under `suites/`) would break in
/// the clone at `clone`, and what the app updates itself; `maps_root` is the
/// folder of the app's Auto Run files, where the export settings are kept.
/// Refused, with nothing read beyond the folder names, for a path the rename
/// itself would refuse. Read only.
pub fn check(clone: &Path, maps_root: Option<&Path>, from: &str, to: &str) -> Result<RenameCheck, String> {
    let (from_p, to_p) = validate(clone, from, to)?;
    let (from_s, to_s) = (from_p.join("/"), to_p.join("/"));
    let folder = {
        let mut f = vec!["suites".to_string()];
        f.extend(from_p.iter().cloned());
        f
    };
    let target = {
        let mut f = vec!["suites".to_string()];
        f.extend(to_p.iter().cloned());
        f
    };

    let mut breaks = Vec::new();
    for (rel, path) in files(clone) {
        let name = rel.last().cloned().unwrap_or_default();
        let json = name.to_ascii_lowercase().ends_with(".json");
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let shown = rel.join("/");
        let base = &rel[..rel.len() - 1];
        let file_inside = within(&rel, &folder);
        let file_after = if file_inside { moved(&rel, &folder, &target) } else { rel.clone() };
        let base_after = &file_after[..file_after.len() - 1];

        // (a) and (b): a relative path that would no longer lead where it did.
        let (mut into, mut out_of) = (false, false);
        for spec in specifiers(&text, json) {
            let Some(was) = resolve(base, &spec) else { continue };
            let target_inside = within(&was, &folder);
            if !file_inside && !target_inside {
                continue;
            }
            let should = if target_inside { moved(&was, &folder, &target) } else { was.clone() };
            let now = resolve(base_after, &spec);
            if now.as_deref().map(key) == Some(key(&should)) {
                continue;
            }
            if file_inside {
                out_of = true;
            } else {
                into = true;
            }
        }
        if into {
            breaks.push(RenameBreak { file: shown.clone(), what: "Imports something inside the folder by a relative path.".into() });
        }
        if out_of {
            breaks.push(RenameBreak {
                file: shown.clone(),
                what: "Imports something outside the folder by a relative path that would stop leading there.".into(),
            });
        }

        // (c) the config and package.json, by the folder's path.
        let lowered = name.to_ascii_lowercase();
        if (lowered.starts_with("playwright.config.") || lowered == "package.json") && mentions(&text, &from_p) {
            breaks.push(RenameBreak { file: shown, what: "Mentions the folder's path.".into() });
        }
    }

    // (d) the app's own settings, which it moves along with the folder.
    let mut tcm_updates = Vec::new();
    for (_, map) in placements_in(maps_root, &from_p) {
        for (area, p) in &map.areas {
            if within(&split(&p.path), &from_p) {
                tcm_updates.push(format!("Export to Playwright places the area \"{area}\" in this folder."));
            }
        }
        for (id, p) in &map.cases {
            if within(&split(&p.path), &from_p) {
                tcm_updates.push(format!("Export to Playwright places case {id} in this folder."));
            }
        }
    }
    tcm_updates.sort();
    tcm_updates.dedup();
    Ok(RenameCheck { from: from_s, to: to_s, breaks, tcm_updates })
}

/// The refusal text for an unconfirmed rename: the sentence the webview's own
/// dialog opens with, then the list.
pub fn unconfirmed(check: &RenameCheck) -> String {
    let mut out = format!(
        "Renaming {} to {} will break these, which you will need to fix in the repository:",
        check.from, check.to
    );
    for b in &check.breaks {
        out.push_str(&format!("\n{} - {}", b.file, b.what));
    }
    out
}

/// Renames `from` to `to` on disk. With anything the check lists under
/// breaks and `confirmed` false it refuses with the list and moves nothing.
/// The app's own export placements in `maps_root` move with the folder; the
/// repo's files are never rewritten and nothing is deleted.
pub fn rename(clone: &Path, maps_root: Option<&Path>, from: &str, to: &str, confirmed: bool) -> Result<RenameDone, String> {
    let found = check(clone, maps_root, from, to)?;
    if !found.breaks.is_empty() && !confirmed {
        return Err(unconfirmed(&found));
    }
    let (from_p, to_p) = (split(&found.from), split(&found.to));
    let at = |p: &[String]| p.iter().fold(clone.join("suites"), |d, f| d.join(f));
    std::fs::rename(at(&from_p), at(&to_p)).map_err(|e| {
        format!("The folder could not be renamed: {e}. Close anything that has it open (an editor, a terminal) and try again.")
    })?;

    let mut updated = 0u32;
    let mut failed = false;
    for (path, mut map) in placements_in(maps_root, &from_p) {
        for p in map.areas.values_mut().chain(map.cases.values_mut()) {
            let now = split(&p.path);
            if within(&now, &from_p) {
                *p = Placement { path: moved(&now, &from_p, &to_p).join("/") };
                updated += 1;
            }
        }
        if mapping::save_at(&path, &map).is_err() {
            crate::applog::warn("pw-repo: export settings could not be saved after a rename");
            failed = true;
        }
    }
    if failed {
        return Err(format!(
            "The folder was renamed, but the app could not update its Export to Playwright settings for it. Check the placements of {} in Export to Playwright.",
            found.to
        ));
    }
    Ok(RenameDone { from: found.from, to: found.to, placements_updated: updated })
}
