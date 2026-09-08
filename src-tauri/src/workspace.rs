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
    std::fs::write(&target, &bytes)
        .map_err(|e| format!("could not write {}: {e}", target.display()))?;
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

/// What `exclude_locally` actually managed to do. Not a bool: two of the
/// three outcomes leave a password where git can carry it away, and the
/// caller has to be able to say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    /// The file is untracked and now sits in this checkout's exclude list.
    Excluded,
    /// git already TRACKS the file: `.git/info/exclude` does nothing for it,
    /// and the next commit takes the contents with it.
    Tracked,
    /// Not a git checkout at all - there was nothing to exclude.
    NotGit,
}

/// Run `git` inside `root`. `None` when git is missing or could not start;
/// no console window flashes, as everywhere else this app shells out.
fn git_in(root: &Path, args: &[&str]) -> Option<std::process::Output> {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.output().ok()
}

/// Keep `rel` out of `git status` for THIS checkout only, via the exclude
/// file git itself names - the repo's `.gitignore` is the user's and is not
/// edited.
///
/// Everything here goes through the git CLI rather than looking for a
/// `.git` DIRECTORY: in a worktree or a submodule `.git` is a FILE pointing
/// elsewhere, so the directory test called those "not a checkout" and
/// excluded nothing, silently. `git rev-parse --git-path` answers correctly
/// in all three layouts.
///
/// And an exclude line only ever affects UNTRACKED files. When the config
/// is already in the index the line is written anyway (harmless, and it
/// starts working the moment the file leaves the index) but the answer is
/// `Tracked`, because the caller has a password to warn about.
pub fn exclude_locally(root: &Path, rel: &str) -> Result<Exclusion, String> {
    let Some(out) = git_in(root, &["rev-parse", "--git-path", "info/exclude"]) else {
        return Ok(Exclusion::NotGit);
    };
    if !out.status.success() {
        return Ok(Exclusion::NotGit);
    }
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if printed.is_empty() {
        return Ok(Exclusion::NotGit);
    }
    // git prints the path relative to the repository root it was run in
    // (absolute for a worktree/submodule), so resolve it against `root`.
    let printed = Path::new(&printed);
    let file = if printed.is_absolute() { printed.to_path_buf() } else { root.join(printed) };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if !existing.lines().any(|l| l.trim() == rel) {
        let mut next = existing;
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(rel);
        next.push('\n');
        std::fs::write(&file, next)
            .map_err(|e| format!("could not write {}: {e}", file.display()))?;
    }

    // Exit 0 = the path is in the index; anything else = it is not.
    let tracked = git_in(root, &["ls-files", "--error-unmatch", rel])
        .map(|o| o.status.success())
        .unwrap_or(false);
    Ok(if tracked { Exclusion::Tracked } else { Exclusion::Excluded })
}
