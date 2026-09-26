//! The working repository: the one folder per-repo test-case files belong
//! to, and the path rules the intake, the importer and the registration
//! code all defer to. Pure functions over `std::fs` - the Tauri commands
//! in `commands/workspace.rs` are thin wrappers.

use std::path::{Path, PathBuf};

/// The per-repo home of test-case JSON. A dot-folder so it sits beside
/// `.claude/` and `.mcp.json` rather than among the repo's own sources.
pub const CASES_DIR: &str = ".test-cases";

pub fn cases_dir(root: &Path) -> PathBuf {
    root.join(CASES_DIR)
}

/// Create the folder if needed. The ROOT is never created: a mistyped
/// repository path must not quietly become a new folder somewhere.
pub fn ensure_cases_dir(root: &Path) -> Result<PathBuf, String> {
    if !root.is_dir() {
        return Err(format!("working repository does not exist: {}", root.display()));
    }
    let dir = cases_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// One comparable spelling of a path: canonical where it exists (so
/// `D:\repo` and `D:/REPO/` agree), the nearest existing parent joined
/// with the file name where it does not yet - an output path is checked
/// before the assistant has written the file. Lower-cased and
/// backslashed because the app is Windows-first and NTFS is
/// case-insensitive.
fn normalized(p: &Path) -> String {
    let full = match p.canonicalize() {
        Ok(c) => c,
        Err(_) => match (p.parent().and_then(|d| d.canonicalize().ok()), p.file_name()) {
            (Some(d), Some(f)) => d.join(f),
            _ => p.to_path_buf(),
        },
    };
    full.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Is `path` `dir` itself or somewhere beneath it? A sibling that merely
/// shares the prefix (`.test-cases-extra`) is outside.
pub fn is_inside(dir: &Path, path: &Path) -> bool {
    let d = normalized(dir);
    let p = normalized(path);
    p == d || p.starts_with(&format!("{d}\\"))
}

/// Copy `source` into the cases folder and return where it landed, plus
/// where anything it displaced went. A file already inside is returned as
/// is; identical bytes reuse the existing file.
///
/// Different bytes under the same name REPLACE the file - the pick is the
/// user's statement of which content they want - and the previous copy
/// moves to `.history/<stem>.<stamp>.json`. Nothing is ever deleted.
///
/// Round 8 §11: this used to refuse to overwrite and write `name-2.json`,
/// `name-3.json` instead, which kept the OLDEST bytes under the obvious
/// name. On a set carrying work item ids, importing that file silently
/// reverted fifteen corrected cases in Azure DevOps. Safety was the intent;
/// the naming had it backwards.
pub fn copy_into_cases(root: &Path, source: &Path) -> Result<(PathBuf, Option<PathBuf>), String> {
    let dir = ensure_cases_dir(root)?;
    if is_inside(&dir, source) {
        return Ok((source.to_path_buf(), None));
    }
    let name = source
        .file_name()
        .ok_or_else(|| format!("not a file: {}", source.display()))?;
    let bytes =
        std::fs::read(source).map_err(|e| format!("could not read {}: {e}", source.display()))?;
    let target = dir.join(name);
    let displaced = match std::fs::read(&target) {
        Ok(existing) if existing == bytes => return Ok((target, None)),
        Ok(_) => Some(displace(&dir, &target)?),
        Err(_) => None,
    };
    std::fs::write(&target, &bytes).map_err(|e| match &displaced {
        Some(d) => format!(
            "could not write {}: {e}. The previous copy is at {}",
            target.display(),
            d.display()
        ),
        None => format!("could not write {}: {e}", target.display()),
    })?;
    Ok((target, displaced))
}

/// Move `target` into `.history` under a stamped name and return the new
/// path. A clash within the same second takes `-2`, `-3`, ...
fn displace(dir: &Path, target: &Path) -> Result<PathBuf, String> {
    let history = dir.join(".history");
    std::fs::create_dir_all(&history)
        .map_err(|e| format!("could not create {}: {e}", history.display()))?;
    let stem = target.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "test-cases".into());
    let ext = target.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let stamp = crate::applog::file_stamp();
    let mut n = 1u32;
    loop {
        let file = if n == 1 { format!("{stem}.{stamp}{ext}") } else { format!("{stem}.{stamp}-{n}{ext}") };
        let candidate = history.join(file);
        if !candidate.exists() {
            std::fs::rename(target, &candidate)
                .map_err(|e| format!("could not move {} aside: {e}", target.display()))?;
            return Ok(candidate);
        }
        n += 1;
    }
}

/// A file-name-safe version of a feature name: lower-case, runs of
/// anything non-alphanumeric collapsed to one dash.
pub fn slug(feature: &str) -> String {
    let dashed: String = feature
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let joined = dashed.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if joined.is_empty() { "test-cases".to_string() } else { joined }
}

/// The path the intake suggests for a job: `<root>/.test-cases/<slug>.json`.
pub fn default_output_path(root: &Path, feature: &str) -> String {
    cases_dir(root).join(format!("{}.json", slug(feature))).to_string_lossy().to_string()
}

/// A bare file name means "in the cases folder"; anything carrying a
/// directory part is returned unchanged (and judged by `is_inside` later).
pub fn resolve_output(root: &Path, output_path: &str) -> String {
    let t = output_path.trim();
    if t.is_empty() || t.contains('/') || t.contains('\\') {
        return t.to_string();
    }
    cases_dir(root).join(t).to_string_lossy().to_string()
}
