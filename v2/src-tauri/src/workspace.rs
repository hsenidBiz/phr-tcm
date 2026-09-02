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

/// Copy `source` into the cases folder and return where it landed. A
/// file already inside is returned as is. Never overwrites: identical
/// bytes reuse the existing file, different bytes take `name-2.json`,
/// `name-3.json`, ...
pub fn copy_into_cases(root: &Path, source: &Path) -> Result<PathBuf, String> {
    let dir = ensure_cases_dir(root)?;
    if is_inside(&dir, source) {
        return Ok(source.to_path_buf());
    }
    let name = source
        .file_name()
        .ok_or_else(|| format!("not a file: {}", source.display()))?;
    let bytes =
        std::fs::read(source).map_err(|e| format!("could not read {}: {e}", source.display()))?;
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "test-cases".to_string());
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut n = 1u32;
    loop {
        let file = if n == 1 { format!("{stem}{ext}") } else { format!("{stem}-{n}{ext}") };
        let candidate = dir.join(file);
        match std::fs::read(&candidate) {
            Ok(existing) if existing == bytes => return Ok(candidate),
            Ok(_) => n += 1,
            Err(_) => {
                std::fs::write(&candidate, &bytes)
                    .map_err(|e| format!("could not write {}: {e}", candidate.display()))?;
                return Ok(candidate);
            }
        }
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

/// Keep `rel` out of `git status` for THIS checkout only, via
/// `.git/info/exclude` - the repo's `.gitignore` is the user's and is not
/// edited. `Ok(false)` when the root is not a git checkout.
pub fn exclude_locally(root: &Path, rel: &str) -> Result<bool, String> {
    if !root.join(".git").is_dir() {
        return Ok(false);
    }
    let info = root.join(".git").join("info");
    std::fs::create_dir_all(&info).map_err(|e| format!("could not create {}: {e}", info.display()))?;
    let file = info.join("exclude");
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == rel) {
        return Ok(true);
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(rel);
    next.push('\n');
    std::fs::write(&file, next).map_err(|e| format!("could not write {}: {e}", file.display()))?;
    Ok(true)
}
