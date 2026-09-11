//! Export / import of the app's local state, for moving to a new machine.
//!
//! One JSON file carries both halves of what the app remembers:
//! - the webview's `tcm-v2-*` localStorage (settings, theme, drafts, the
//!   local cache db, pins, watch paths) - collected by the frontend and
//!   handed in, since only the webview can read it;
//! - the disk stores under `app_data_dir` (the tag reference cache, Auto
//!   Run scripts and local runs, materialized shared drafts, AI findings).
//!
//! What deliberately never travels: credentials. Sign-in tokens live only
//! in memory, the AI-bridge token is re-minted per launch in the OS temp
//! dir, and the app log stays where it is - none of those are under the
//! roots this module reads.

use base64::Engine;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// The disk roots (relative to `app_data_dir`) a backup carries. An
/// allowlist, applied on BOTH sides: export never wanders into logs or
/// strangers' files, and import refuses to write anywhere else - a crafted
/// backup must not be able to drop files outside these folders.
const ROOTS: [&str; 4] = ["reference-cache.json", "autorun", "shared-drafts", "findings.json"];

/// A single file per entry is capped so one enormous stray artifact cannot
/// balloon the backup into something no one can email or copy around.
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(serde::Serialize, serde::Deserialize, specta::Type, Clone, Debug)]
pub struct BackupFile {
    /// Path relative to `app_data_dir`, forward slashes.
    pub path: String,
    /// File content, base64.
    pub b64: String,
}

#[derive(serde::Serialize, serde::Deserialize, specta::Type, Clone, Debug)]
pub struct BackupDoc {
    /// Sentinel + format version, so a random JSON file is refused with a
    /// clear message instead of half-imported.
    pub app: String,
    pub format: u32,
    pub exported_at: String,
    pub app_version: String,
    pub local_storage: BTreeMap<String, String>,
    pub files: Vec<BackupFile>,
}

#[derive(serde::Serialize, specta::Type, Clone, Debug)]
pub struct ExportSummary {
    pub path: String,
    pub keys: u32,
    pub files: u32,
    /// Files left out (too large), named so the export is honest about it.
    pub skipped: Vec<String>,
}

#[derive(serde::Serialize, specta::Type, Clone, Debug)]
pub struct BackupImportResult {
    pub local_storage: BTreeMap<String, String>,
    pub files_restored: u32,
    pub exported_at: String,
    pub app_version: String,
}

pub const APP_SENTINEL: &str = "tcm-v2-backup";
pub const FORMAT: u32 = 1;

/// Walk the allowlisted roots under `data_dir`. Returns kept files as
/// `(relative_path, bytes)` plus the relative paths skipped for size.
pub fn collect_files(data_dir: &Path) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let mut kept = vec![];
    let mut skipped = vec![];
    for root in ROOTS {
        walk(data_dir, &data_dir.join(root), &mut kept, &mut skipped);
    }
    kept.sort_by(|a, b| a.0.cmp(&b.0));
    skipped.sort();
    (kept, skipped)
}

fn walk(base: &Path, p: &Path, kept: &mut Vec<(String, Vec<u8>)>, skipped: &mut Vec<String>) {
    let Ok(meta) = std::fs::metadata(p) else { return };
    if meta.is_dir() {
        let Ok(entries) = std::fs::read_dir(p) else { return };
        for e in entries.flatten() {
            walk(base, &e.path(), kept, skipped);
        }
        return;
    }
    let rel = match p.strip_prefix(base) {
        Ok(r) => r.to_string_lossy().replace('\\', "/"),
        Err(_) => return,
    };
    if meta.len() > MAX_FILE_BYTES {
        skipped.push(rel);
        return;
    }
    if let Ok(bytes) = std::fs::read(p) {
        kept.push((rel, bytes));
    }
}

/// Is `rel` a path import may write? Relative, inside one of the roots,
/// and free of `..`/absolute/drive components (the zip-slip family).
pub fn safe_relative_path(rel: &str) -> bool {
    if rel.is_empty() || rel.len() > 500 {
        return false;
    }
    let p = Path::new(rel);
    if !p
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return false;
    }
    ROOTS
        .iter()
        .any(|root| rel == *root || rel.starts_with(&format!("{root}/")))
}

pub fn build_doc(
    app_version: &str,
    exported_at: &str,
    local_storage: BTreeMap<String, String>,
    files: Vec<(String, Vec<u8>)>,
) -> BackupDoc {
    let b64 = base64::engine::general_purpose::STANDARD;
    BackupDoc {
        app: APP_SENTINEL.into(),
        format: FORMAT,
        exported_at: exported_at.into(),
        app_version: app_version.into(),
        local_storage,
        files: files
            .into_iter()
            .map(|(path, bytes)| BackupFile { path, b64: b64.encode(bytes) })
            .collect(),
    }
}

/// Atomic write: temp sibling then rename, so a failed export never leaves
/// a truncated file that LOOKS like a backup.
pub fn write_doc(doc: &BackupDoc, path: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    let tmp: PathBuf = path.with_extension("tmp-export");
    std::fs::write(&tmp, json).map_err(|e| format!("could not write the backup: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not write the backup: {e}")
    })
}

pub fn read_doc(path: &Path) -> Result<BackupDoc, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read the file: {e}"))?;
    let doc: BackupDoc =
        serde_json::from_str(&text).map_err(|_| "this is not a Test Case Manager backup file".to_string())?;
    if doc.app != APP_SENTINEL {
        return Err("this is not a Test Case Manager backup file".into());
    }
    if doc.format > FORMAT {
        return Err(format!(
            "this backup was made by a newer version of the app (format {}) - update this copy first",
            doc.format
        ));
    }
    Ok(doc)
}

/// Restore the disk half of a backup. Unsafe paths are refused one by one
/// (the rest still land); returns how many files were written.
pub fn restore_files(data_dir: &Path, files: &[BackupFile]) -> Result<u32, String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut restored = 0u32;
    for f in files {
        if !safe_relative_path(&f.path) {
            crate::applog::warn(format!("Backup import skipped an unsafe path: {}", f.path));
            continue;
        }
        let bytes = b64
            .decode(&f.b64)
            .map_err(|_| format!("corrupt backup: {} is not valid base64", f.path))?;
        let target = data_dir.join(&f.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&target, bytes).map_err(|e| format!("could not restore {}: {e}", f.path))?;
        restored += 1;
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "tcm-backup-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_backup_round_trips_files_and_local_storage() {
        let src = tmpdir("src");
        std::fs::write(src.join("reference-cache.json"), b"{\"tags\":[]}").unwrap();
        std::fs::create_dir_all(src.join("autorun/runs")).unwrap();
        std::fs::write(src.join("autorun/runs/r1.json"), b"{}").unwrap();
        std::fs::create_dir_all(src.join("shared-drafts")).unwrap();
        std::fs::write(src.join("shared-drafts/shared-pbi-7.json"), b"[]").unwrap();
        // A file OUTSIDE the roots never travels.
        std::fs::write(src.join("secrets.txt"), b"nope").unwrap();

        let (files, skipped) = collect_files(&src);
        assert!(skipped.is_empty());
        let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            names,
            vec!["autorun/runs/r1.json", "reference-cache.json", "shared-drafts/shared-pbi-7.json"]
        );

        let mut ls = BTreeMap::new();
        ls.insert("tcm-v2-theme".to_string(), "dark".to_string());
        let doc = build_doc("1.20.4", "2026-08-19T00:00:00Z", ls, files);
        let out = tmpdir("out").join("backup.json");
        write_doc(&doc, &out).unwrap();

        let read = read_doc(&out).unwrap();
        assert_eq!(read.local_storage["tcm-v2-theme"], "dark");
        let dst = tmpdir("dst");
        let n = restore_files(&dst, &read.files).unwrap();
        assert_eq!(n, 3);
        assert_eq!(std::fs::read(dst.join("autorun/runs/r1.json")).unwrap(), b"{}");
        assert_eq!(
            std::fs::read(dst.join("reference-cache.json")).unwrap(),
            b"{\"tags\":[]}"
        );
        assert!(!dst.join("secrets.txt").exists());

        let _ = std::fs::remove_dir_all(src);
        let _ = std::fs::remove_dir_all(dst);
    }

    #[test]
    fn an_oversized_file_is_skipped_and_named() {
        let src = tmpdir("big");
        std::fs::create_dir_all(src.join("autorun")).unwrap();
        std::fs::write(src.join("autorun/huge.bin"), vec![0u8; (MAX_FILE_BYTES + 1) as usize])
            .unwrap();
        std::fs::write(src.join("autorun/ok.json"), b"{}").unwrap();
        let (files, skipped) = collect_files(&src);
        assert_eq!(files.len(), 1, "{files:?}");
        assert_eq!(skipped, vec!["autorun/huge.bin"]);
        let _ = std::fs::remove_dir_all(src);
    }

    #[test]
    fn unsafe_paths_are_refused_on_import() {
        for bad in [
            "../evil.txt",
            "autorun/../../evil.txt",
            "C:/Windows/evil.txt",
            "/etc/passwd",
            "logs/app.log",     // outside the roots
            "autorunx/a.json",  // prefix trick: not the autorun root
            "",
        ] {
            assert!(!safe_relative_path(bad), "{bad:?} must be refused");
        }
        for good in ["reference-cache.json", "autorun/scripts/tc-1.js", "shared-drafts/shared-pbi-2.json"] {
            assert!(safe_relative_path(good), "{good:?} must be allowed");
        }

        // And restore actually drops the unsafe one while keeping the rest.
        let dst = tmpdir("refuse");
        let b64 = base64::engine::general_purpose::STANDARD;
        let files = vec![
            BackupFile { path: "../evil.txt".into(), b64: b64.encode(b"x") },
            BackupFile { path: "autorun/ok.json".into(), b64: b64.encode(b"{}") },
        ];
        let n = restore_files(&dst, &files).unwrap();
        assert_eq!(n, 1);
        assert!(dst.join("autorun/ok.json").exists());
        assert!(!dst.parent().unwrap().join("evil.txt").exists());
        let _ = std::fs::remove_dir_all(dst);
    }

    #[test]
    fn foreign_json_and_newer_formats_are_refused() {
        let d = tmpdir("foreign");
        let not_ours = d.join("cases.json");
        std::fs::write(&not_ours, "[{\"title\":\"a test case draft\"}]").unwrap();
        let err = read_doc(&not_ours).unwrap_err();
        assert!(err.contains("not a Test Case Manager backup"), "{err}");

        let newer = d.join("newer.json");
        let mut doc = build_doc("9.9.9", "2027-01-01T00:00:00Z", BTreeMap::new(), vec![]);
        doc.format = FORMAT + 1;
        write_doc(&doc, &newer).unwrap();
        let err = read_doc(&newer).unwrap_err();
        assert!(err.contains("newer version"), "{err}");
        let _ = std::fs::remove_dir_all(d);
    }
}
