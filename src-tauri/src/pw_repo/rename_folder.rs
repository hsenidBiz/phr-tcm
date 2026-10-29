//! Rename folder: a folder under the clone's `suites/` renamed from the
//! Overview, with a plain warning of what the rename would break.
//!
//! The app never rewrites the repo's source files to mend an import. It
//! renames the folder (`std::fs::rename`, nothing deleted) and says, before
//! it does, which files point into the folder or out of it by a relative
//! path or a `tsconfig` alias, which of `playwright.config.*`, `package.json`
//! and `tsconfig*.json` mention its path, and which of the app's own Export
//! to Playwright settings name it - those the app does update itself.
//!
//! Files are read as text, never through git, and only the kinds an import
//! can sit in (`.ts .js .mjs .cjs .json`), skipping `node_modules`, every
//! dot-folder (`.git`, `.auth`), build and report output, `src/users` and
//! any JSON file named like a secret. Every name shown is relative to the
//! clone.
//!
//! An import is followed three ways: a relative path, a `paths` alias of the
//! clone's `tsconfig*.json` (and what it extends in the clone), and a bare
//! path from its `baseUrl`. A string anywhere that holds the folder's path
//! (`suites/<path>`) is listed too, as are `src/navigation.json` entries the
//! specs find by their folder names.
//!
//! The app's own Export to Playwright settings are moved only when they
//! belong to this clone: the project has a Sync record for it.

use super::layout::{self, CONTENT_DIRS, SHARED};
use crate::pw_export::mapping::{self, ExportMap, Placement};
use crate::pw_repo::sync;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The kinds of file that can import something by a relative path.
const SCANNED: [&str; 5] = ["ts", "js", "mjs", "cjs", "json"];

/// Folders never read: dependencies and build or report output. (Every
/// folder starting with a dot is skipped too: git's data, saved sessions.)
const SKIPPED_DIRS: [&str; 9] = [
    "node_modules",
    "dist",
    "build",
    "out",
    "playwright-report",
    "test-results",
    "coverage",
    "allure-results",
    "blob-report",
];

/// Words in a JSON file's name that mark it as holding secrets: never read.
const SECRET_WORDS: [&str; 8] =
    ["secret", "password", "passwd", "credential", "token", "apikey", "api-key", "private-key"];

/// What a refused rename of the shared tree says.
pub const SHARED_REFUSED: &str =
    "common-components is a name the app and the repository rely on; it cannot be renamed here.";

/// The one item a rename that only changes capitals always lists.
pub const CAPITALS_ONLY: &str =
    "Changing only the capitals: git on Windows may not record it, and imports spelled the old way fail on a case-sensitive system.";

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
    /// Set when the folder moved but the app could not update some of its
    /// own export settings: the sentence says so and names the files.
    pub warning: Option<String>,
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

/// Whether `p` is the shared tree itself (`common-components`, any capitals).
fn is_shared_root(p: &[String]) -> bool {
    p.len() == 1 && p[0].to_lowercase() == SHARED
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
    if is_shared_root(&from_p) || is_shared_root(&to_p) {
        return Err(SHARED_REFUSED.into());
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
    if to_p.len() > from_p.len() && within(&to_p, &from_p) {
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
    // Capitals are folded as the file system folds them, Unicode included.
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
            if n.to_lowercase() == to_name.to_lowercase() && !(same_parent && n == from_name) {
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
/// `spec` (`./x`, `../y/z`), settled lexically, with the length of the
/// leading part of `base` that was kept (what lies at or after that index
/// was spelled by `spec`). `None` when it leaves the clone.
fn resolve_low(base: &[String], spec: &str) -> Option<(Vec<String>, usize)> {
    let mut out: Vec<String> = base.to_vec();
    let mut low = out.len();
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
                low = low.min(out.len());
            }
            s => out.push(s.to_string()),
        }
    }
    Some((out, low))
}

fn resolve(base: &[String], spec: &str) -> Option<Vec<String>> {
    resolve_low(base, spec).map(|(p, _)| p)
}

fn is_relative(spec: &str) -> bool {
    spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../")
}

/// Whether `target` is `folder` or something inside it.
fn within(target: &[String], folder: &[String]) -> bool {
    target.len() >= folder.len() && target.iter().zip(folder).all(|(a, b)| lower(a) == lower(b))
}

/// `at` with the leading `from` swapped for `to`.
fn moved(at: &[String], from: &[String], to: &[String]) -> Vec<String> {
    let mut out = to.to_vec();
    out.extend_from_slice(&at[from.len()..]);
    out
}

/// The paths a source file imports or requires (a JSON file: its string
/// values that look like relative paths).
fn specifiers(text: &str, json: bool) -> Vec<String> {
    static CODE: OnceLock<regex::Regex> = OnceLock::new();
    static STRING: OnceLock<regex::Regex> = OnceLock::new();
    let re = if json {
        STRING.get_or_init(|| regex::Regex::new(r#""([^"\r\n]*)""#).expect("a valid pattern"))
    } else {
        CODE.get_or_init(|| {
            regex::Regex::new(r#"(?:\bfrom|\bimport|\brequire(?:\.resolve)?)\s*\(?\s*['"`]([^'"`\r\n]+)['"`]"#)
                .expect("a valid pattern")
        })
    };
    re.captures_iter(text)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .filter(|s| !json || is_relative(s))
        .collect()
}

/// Every string literal of a file (single, double or backtick quoted).
fn literals(text: &str) -> Vec<String> {
    static LITERAL: OnceLock<regex::Regex> = OnceLock::new();
    let re = LITERAL
        .get_or_init(|| regex::Regex::new(r#"'([^'\r\n]*)'|"([^"\r\n]*)"|`([^`]*)`"#).expect("a valid pattern"));
    re.captures_iter(text)
        .filter_map(|c| c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)))
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Whether `text` names the folder: `suites/<path>`, or with `bare` and a
/// path of two folders or more the path itself, each as a whole name (not a
/// prefix of a longer one). Backslashes and escaped slashes (`suites\/x`)
/// count as slashes.
fn mentions(text: &str, from: &[String], bare: bool) -> bool {
    let text = lower(&text.replace("\\/", "/").replace("\\\\", "/").replace('\\', "/"));
    let path = key(from);
    let name_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    let found = |needle: &str, dotted_before: bool| {
        text.match_indices(needle).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + needle.len()..].chars().next();
            let before_ok =
                before.is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '-')) && (dotted_before || c != '.'));
            before_ok && after.is_none_or(|c| !name_char(c))
        })
    };
    found(&format!("suites/{path}"), true) || (bare && from.len() >= 2 && found(&path, false))
}

/// Whether a JSON file's name marks it as holding secrets.
fn secret_named(file: &str) -> bool {
    let n = file.to_lowercase();
    n == "users.json" || SECRET_WORDS.iter().any(|w| n.contains(w))
}

/// Every file under `clone` a rename could break, with its path as parts
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
                if name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_str()) {
                    continue;
                }
                rel.push(name);
                // The test accounts' file lives here: its values are passwords.
                let users = rel.len() == 2 && rel[0] == "src" && rel[1] == "users";
                if !users {
                    walk(&e.path(), rel, out);
                }
                rel.pop();
            } else if t.is_file() {
                let ext = name.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_default();
                if SCANNED.contains(&ext.as_str())
                    && name != "package-lock.json"
                    && !(ext == "json" && secret_named(&name))
                {
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

// ---- tsconfig path aliases --------------------------------------------------

/// `//` and `/* */` comments and trailing commas removed from a tsconfig,
/// which is JSON with both allowed.
fn strip_jsonc(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut bare = String::with_capacity(s.len());
    let (mut i, mut in_str) = (0, false);
    while i < c.len() {
        if in_str {
            bare.push(c[i]);
            if c[i] == '\\' && i + 1 < c.len() {
                bare.push(c[i + 1]);
                i += 2;
                continue;
            }
            if c[i] == '"' {
                in_str = false;
            }
            i += 1;
        } else if c[i] == '"' {
            in_str = true;
            bare.push('"');
            i += 1;
        } else if c[i] == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if c[i] == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else {
            bare.push(c[i]);
            i += 1;
        }
    }
    let c: Vec<char> = bare.chars().collect();
    let mut out = String::with_capacity(c.len());
    let (mut i, mut in_str) = (0, false);
    while i < c.len() {
        if in_str {
            out.push(c[i]);
            if c[i] == '\\' && i + 1 < c.len() {
                out.push(c[i + 1]);
                i += 2;
                continue;
            }
            if c[i] == '"' {
                in_str = false;
            }
        } else if c[i] == '"' {
            in_str = true;
            out.push('"');
        } else if c[i] == ',' {
            let next = c[i + 1..].iter().find(|x| !x.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(',');
            }
        } else {
            out.push(c[i]);
        }
        i += 1;
    }
    out
}

/// What one tsconfig (with what it extends inside the clone) says.
#[derive(Default)]
struct Cfg {
    base: Option<Vec<String>>,
    base_file: String,
    paths: Vec<(String, Vec<String>)>,
    paths_dir: Vec<String>,
    paths_file: String,
}

fn read_cfg(clone: &Path, rel: &[String], depth: usize) -> Option<Cfg> {
    if depth > 8 {
        return None;
    }
    let path = rel.iter().fold(clone.to_path_buf(), |d, f| d.join(f));
    if std::fs::metadata(&path).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&strip_jsonc(&text)).ok()?;
    let dir = &rel[..rel.len() - 1];
    let file = rel.join("/");
    let mut cfg = Cfg::default();
    let extends: Vec<String> = match value.get("extends") {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => vec![],
    };
    for e in extends.iter().filter(|e| is_relative(e)) {
        let Some(mut target) = resolve(dir, e) else { continue };
        if !target.last().is_some_and(|l| l.to_lowercase().ends_with(".json")) {
            if let Some(l) = target.last_mut() {
                l.push_str(".json");
            }
        }
        if let Some(parent) = read_cfg(clone, &target, depth + 1) {
            if parent.base.is_some() {
                cfg.base = parent.base;
                cfg.base_file = parent.base_file;
            }
            if !parent.paths.is_empty() {
                cfg.paths = parent.paths;
                cfg.paths_dir = parent.paths_dir;
                cfg.paths_file = parent.paths_file;
            }
        }
    }
    if let Some(opts) = value.get("compilerOptions") {
        if let Some(b) = opts.get("baseUrl").and_then(|b| b.as_str()) {
            let b = if is_relative(b) { b.to_string() } else { format!("./{b}") };
            if let Some(p) = resolve(dir, &b) {
                cfg.base = Some(p);
                cfg.base_file = file.clone();
            }
        }
        if let Some(serde_json::Value::Object(m)) = opts.get("paths") {
            cfg.paths = m
                .iter()
                .map(|(k, v)| {
                    let targets = v.as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect());
                    (k.clone(), targets.unwrap_or_default())
                })
                .collect();
            cfg.paths_dir = dir.to_vec();
            cfg.paths_file = file;
        }
    }
    Some(cfg)
}

/// One alias: its pattern, where it leads (as parts relative to the clone,
/// a `*` standing for the matched part) and the file that declares it.
struct Alias {
    pattern: String,
    targets: Vec<Vec<String>>,
    file: String,
}

/// The aliases of one root tsconfig: its `baseUrl` and its `paths`.
struct Aliases {
    base: Option<Vec<String>>,
    base_file: String,
    list: Vec<Alias>,
}

impl Aliases {
    /// Every path an import `spec` (not relative) may lead to, as parts
    /// relative to the clone.
    fn resolve(&self, spec: &str) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        for a in &self.list {
            let cap = match a.pattern.split_once('*') {
                Some((pre, suf)) => {
                    if spec.len() >= pre.len() + suf.len() && spec.starts_with(pre) && spec.ends_with(suf) {
                        Some(spec[pre.len()..spec.len() - suf.len()].to_string())
                    } else {
                        None
                    }
                }
                None => (spec == a.pattern).then(String::new),
            };
            let Some(cap) = cap else { continue };
            for t in &a.targets {
                out.push(
                    t.join("/").replace('*', &cap).split('/').filter(|s| !s.is_empty()).map(str::to_string).collect(),
                );
            }
        }
        if let Some(base) = &self.base {
            if let Some(p) = resolve(base, spec) {
                out.push(p);
            }
        }
        out
    }
}

/// The aliases of every `tsconfig*.json` at the clone's root.
fn aliases(clone: &Path) -> Vec<Aliases> {
    let Ok(rd) = std::fs::read_dir(clone) else { return vec![] };
    let mut names: Vec<String> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("tsconfig") && n.ends_with(".json"))
        .collect();
    names.sort();
    let mut out = Vec::new();
    for n in names {
        let Some(cfg) = read_cfg(clone, &[n], 0) else { continue };
        let at = cfg.base.clone().unwrap_or_else(|| cfg.paths_dir.clone());
        let list = cfg
            .paths
            .iter()
            .map(|(pattern, targets)| Alias {
                pattern: pattern.clone(),
                targets: targets.iter().filter_map(|t| resolve(&at, t)).collect(),
                file: cfg.paths_file.clone(),
            })
            .collect();
        out.push(Aliases { base: cfg.base, base_file: cfg.base_file, list });
    }
    out
}

// ---- the app's own export settings ------------------------------------------

/// The export settings that belong to this clone.
struct Settings {
    found: Vec<(PathBuf, ExportMap)>,
    /// File names of settings that could not be read.
    unreadable: Vec<String>,
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// The Export to Playwright files in `maps_root` of the projects that have a
/// Sync record for this clone in `sync_dir`: the others belong to another
/// clone and are left alone.
fn own_settings(clone: &Path, maps_root: Option<&Path>, sync_dir: Option<&Path>) -> Settings {
    let mut out = Settings { found: vec![], unreadable: vec![] };
    let (Some(root), Some(sync_dir)) = (maps_root, sync_dir) else { return out };
    let key = sync::clone_key(&clone.to_string_lossy());
    for p in mapping::files(root) {
        let name = file_name(&p);
        let slug = name.strip_suffix(".pwexport.json").unwrap_or(&name);
        if !sync_dir.join(format!("{key}-{slug}.json")).is_file() {
            continue;
        }
        match mapping::load_at(&p) {
            Ok(m) => out.found.push((p, m)),
            Err(_) => {
                // The error names the file's full path: kept out of the log.
                crate::applog::warn("pw-repo: an export settings file could not be read for a rename");
                out.unreadable.push(name);
            }
        }
    }
    out
}

fn split(path: &str) -> Vec<String> {
    path.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The common component a rename touches: the folder name it has now, and
/// the one it will have when it stays a common component.
fn component_change(from: &[String], to: &[String]) -> Option<(String, Option<String>)> {
    if from.len() != 2 || from[0].to_lowercase() != SHARED {
        return None;
    }
    let new = (to.len() == 2 && to[0].to_lowercase() == SHARED).then(|| to[1].clone());
    Some((from[1].clone(), new))
}

fn names_component(map: &ExportMap, comp: &Option<(String, Option<String>)>) -> bool {
    comp.as_ref().is_some_and(|(old, _)| map.components.values().any(|v| lower(v) == lower(old)))
}

fn touches(map: &ExportMap, from: &[String], comp: &Option<(String, Option<String>)>) -> bool {
    map.areas.values().chain(map.cases.values()).any(|p| within(&split(&p.path), from)) || names_component(map, comp)
}

fn push_unique(breaks: &mut Vec<RenameBreak>, file: &str, what: &str) {
    if !breaks.iter().any(|b| b.file == file && b.what == what) {
        breaks.push(RenameBreak { file: file.to_string(), what: what.to_string() });
    }
}

/// What renaming `from` to `to` (both paths under `suites/`) would break in
/// the clone at `clone`, and what the app updates itself; `maps_root` is the
/// folder of the app's Auto Run files, where the export settings are kept,
/// and `sync_dir` the folder of the Sync records that say which of them
/// belong to this clone. Refused, with nothing read beyond the folder names,
/// for a path the rename itself would refuse. Read only.
pub fn check(
    clone: &Path,
    maps_root: Option<&Path>,
    sync_dir: Option<&Path>,
    from: &str,
    to: &str,
) -> Result<RenameCheck, String> {
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
    let caps_only = lower(&from_s) == lower(&to_s);
    let idx = folder.len() - 1;
    let old_last = folder[idx].clone();
    // A path that leads into the folder spelled as it is now: all that fails
    // after a rename of capitals alone.
    let old_spelled = |p: &[String]| within(p, &folder) && p[idx] == old_last;
    let hits = |p: &[String]| if caps_only { old_spelled(p) } else { within(p, &folder) };

    let mut breaks: Vec<RenameBreak> = Vec::new();
    if caps_only {
        push_unique(&mut breaks, &format!("suites/{from_s}"), CAPITALS_ONLY);
    }

    // The tsconfigs: an alias that leads into the folder.
    let aliased = aliases(clone);
    for a in &aliased {
        if a.base.as_ref().is_some_and(|b| hits(b)) {
            push_unique(&mut breaks, &a.base_file, "Has a path alias that points into the folder.");
        }
        for al in &a.list {
            let literal = |t: &Vec<String>| {
                let cut = t.iter().position(|s| s.contains('*')).unwrap_or(t.len());
                t[..cut].to_vec()
            };
            if al.targets.iter().any(|t| hits(&literal(t))) {
                push_unique(&mut breaks, &al.file, "Has a path alias that points into the folder.");
            }
        }
    }

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

        // An import that would no longer lead where it did.
        let (mut into, mut out_of, mut alias, mut spelled) = (false, false, false, false);
        for spec in specifiers(&text, json) {
            if is_relative(&spec) {
                let Some((was, low)) = resolve_low(base, &spec) else { continue };
                let target_inside = within(&was, &folder);
                if caps_only && target_inside && low <= idx && was[idx] == old_last {
                    spelled = true;
                }
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
            } else if !json {
                for a in &aliased {
                    if a.resolve(&spec).iter().any(|p| hits(p)) {
                        if caps_only {
                            spelled = true;
                        } else {
                            alias = true;
                        }
                    }
                }
            }
        }
        if into {
            push_unique(&mut breaks, &shown, "Imports something inside the folder by a relative path.");
        }
        if out_of {
            push_unique(
                &mut breaks,
                &shown,
                "Imports something outside the folder by a relative path that would stop leading there.",
            );
        }
        if alias {
            push_unique(&mut breaks, &shown, "Imports something inside the folder through a path alias.");
        }
        if spelled {
            push_unique(&mut breaks, &shown, "Imports something inside the folder spelled with its old capitals.");
        }

        // The folder's path written out: in the config, package.json and
        // tsconfigs by name, in any other file as a string.
        let lowered = name.to_ascii_lowercase();
        let config = lowered.starts_with("playwright.config.")
            || lowered == "package.json"
            || (lowered.starts_with("tsconfig") && lowered.ends_with(".json"));
        let mention = if config {
            mentions(&text, &from_p, true)
        } else if text.to_lowercase().contains("suites") {
            literals(&text).iter().any(|l| mentions(l, &from_p, false))
        } else {
            false
        };
        // A JSON file already listed for a path says it once.
        let already = breaks.iter().any(|b| b.file == shown && (json || b.what.contains("path alias that points")));
        if mention && !already {
            push_unique(&mut breaks, &shown, "Mentions the folder's path.");
        }
    }

    // src/navigation.json: the specs find their entry by folder names.
    if let Ok(text) = std::fs::read_to_string(clone.join("src/navigation.json")) {
        if let Ok(nav) = serde_json::from_str::<serde_json::Value>(&text) {
            let keys: Vec<String> = match nav.get("entries") {
                Some(serde_json::Value::Object(m)) => m.keys().cloned().collect(),
                _ => vec![],
            };
            for feat in layout::feature_dirs(&clone.join("suites")).iter().filter(|f| within(f, &from_p)) {
                let old = layout::navigation_key(feat);
                let new = layout::navigation_key(&moved(feat, &from_p, &to_p));
                if old != new && keys.contains(&old) {
                    push_unique(
                        &mut breaks,
                        "src/navigation.json",
                        &format!("Has the navigation entry \"{old}\", which the specs find by their folder names."),
                    );
                }
            }
            fn named(v: &serde_json::Value, from: &[String]) -> bool {
                match v {
                    serde_json::Value::String(s) => mentions(s, from, true),
                    serde_json::Value::Array(a) => a.iter().any(|x| named(x, from)),
                    serde_json::Value::Object(m) => m.iter().any(|(k, x)| mentions(k, from, true) || named(x, from)),
                    _ => false,
                }
            }
            if named(&nav, &from_p) {
                push_unique(&mut breaks, "src/navigation.json", "Mentions the folder's path.");
            }
        }
    }

    // The app's own settings, which it moves along with the folder.
    let own = own_settings(clone, maps_root, sync_dir);
    let comp = component_change(&from_p, &to_p);
    let mut tcm_updates = Vec::new();
    for (path, map) in &own.found {
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
        if let Some((old, new)) = &comp {
            for (name, v) in &map.components {
                if lower(v) != lower(old) {
                    continue;
                }
                if new.is_some() {
                    tcm_updates
                        .push(format!("Export to Playwright maps the common component \"{name}\" to this folder."));
                } else {
                    push_unique(
                        &mut breaks,
                        &file_name(path),
                        &format!(
                            "Maps the common component \"{name}\" to this folder, which would no longer be in common-components."
                        ),
                    );
                }
            }
        }
    }
    for name in &own.unreadable {
        push_unique(&mut breaks, name, &format!("Could not update TCM's export settings in {name}; fix them by hand."));
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
/// The app's own export placements (those of projects with a Sync record for
/// this clone) move with the folder; the repo's files are never rewritten
/// and nothing is deleted. Settings that cannot be updated after the move
/// are named in the result's `warning`: the folder has moved by then.
pub fn rename(
    clone: &Path,
    maps_root: Option<&Path>,
    sync_dir: Option<&Path>,
    from: &str,
    to: &str,
    confirmed: bool,
) -> Result<RenameDone, String> {
    let found = check(clone, maps_root, sync_dir, from, to)?;
    if !found.breaks.is_empty() && !confirmed {
        return Err(unconfirmed(&found));
    }
    let (from_p, to_p) = (split(&found.from), split(&found.to));
    let at = |p: &[String]| p.iter().fold(clone.join("suites"), |d, f| d.join(f));
    std::fs::rename(at(&from_p), at(&to_p)).map_err(|e| {
        format!("The folder could not be renamed: {e}. Close anything that has it open (an editor, a terminal) and try again.")
    })?;

    let mut updated = 0u32;
    let own = own_settings(clone, maps_root, sync_dir);
    let mut failed: Vec<String> = own.unreadable.clone();
    let comp = component_change(&from_p, &to_p);
    for (path, mut map) in own.found {
        if !touches(&map, &from_p, &comp) {
            continue;
        }
        for p in map.areas.values_mut().chain(map.cases.values_mut()) {
            let now = split(&p.path);
            if within(&now, &from_p) {
                *p = Placement { path: moved(&now, &from_p, &to_p).join("/") };
                updated += 1;
            }
        }
        if let Some((old, Some(new))) = &comp {
            for v in map.components.values_mut() {
                if lower(v) == lower(old) {
                    *v = new.clone();
                    updated += 1;
                }
            }
        }
        let saved = match maps_root {
            Some(root) => mapping::save_at(root, &path, &map),
            None => Err(String::new()),
        };
        if saved.is_err() {
            crate::applog::warn("pw-repo: export settings could not be saved after a rename");
            failed.push(file_name(&path));
        }
    }
    failed.sort();
    failed.dedup();
    let warning = (!failed.is_empty()).then(|| {
        format!(
            "The folder was renamed, but TCM could not update its export settings in {}; fix them by hand.",
            failed.join(", ")
        )
    });
    Ok(RenameDone { from: found.from, to: found.to, placements_updated: updated, warning })
}
