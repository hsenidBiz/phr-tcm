//! Export / import of the app's local state (`v2_lib::backup`), exercised
//! through its public API only.

use base64::Engine;
use std::collections::BTreeMap;
use std::path::PathBuf;
use v2_lib::backup::{
    build_doc, collect_files, read_doc, restore_files, safe_relative_path, write_doc, BackupFile,
    FORMAT,
};

/// Mirrors the private per-file cap (`MAX_FILE_BYTES`) in `backup.rs`.
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

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

/// Backups must carry the app's one cache file (`cache.json`): tags,
/// resolved suites and the assigned-items baseline all live in it now, so a
/// backup that drops it silently loses that data on restore.
#[test]
fn backups_carry_the_app_cache_file() {
    let dir = std::env::temp_dir().join(format!(
        "tcm-backup-carries-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cache.json"), r#"{"owner":null,"entries":{}}"#).unwrap();

    let (files, _skipped) = v2_lib::backup::collect_files(&dir);
    assert!(
        files.iter().any(|(path, _)| path == "cache.json"),
        "collect_files must carry cache.json: {files:?}"
    );
    assert!(v2_lib::backup::safe_relative_path("cache.json"));

    let _ = std::fs::remove_dir_all(dir);
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
    if let Some(out_dir) = out.parent() {
        let _ = std::fs::remove_dir_all(out_dir);
    }
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
        "logs/app.log",    // outside the roots
        "autorunx/a.json", // prefix trick: not the autorun root
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

/// Session files hold live cookies and the screenshots folder can hold
/// hundreds of images. Neither belongs in an export, and neither may be
/// written by an import.
#[test]
fn sessions_and_screenshots_stay_out_of_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let auto = dir.path().join("autorun");
    std::fs::create_dir_all(auto.join("sessions")).unwrap();
    std::fs::create_dir_all(auto.join("shots")).unwrap();
    std::fs::create_dir_all(auto.join("scripts")).unwrap();
    std::fs::write(auto.join("sessions/admin.json"), "{}").unwrap();
    std::fs::write(auto.join("shots/shot-1-000001.jpg"), [0xFFu8, 0xD8]).unwrap();
    std::fs::write(auto.join("scripts/case-1.json"), "{}").unwrap();
    std::fs::write(auto.join("accounts.json"), "[]").unwrap();

    let (kept, _) = v2_lib::backup::collect_files(dir.path());
    let names: Vec<&str> = kept.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"autorun/scripts/case-1.json"), "{names:?}");
    assert!(names.contains(&"autorun/accounts.json"), "accounts travel with a backup: {names:?}");
    assert!(!names.iter().any(|n| n.starts_with("autorun/sessions") || n.starts_with("autorun/shots")), "{names:?}");

    assert!(v2_lib::backup::safe_relative_path("autorun/scripts/case-1.json"));
    assert!(!v2_lib::backup::safe_relative_path("autorun/sessions/admin.json"));
    assert!(!v2_lib::backup::safe_relative_path("autorun/shots/shot-1-000001.jpg"));

    // Windows splits a path on `\` just as it does `/`, so an entry
    // spelled with backslashes still names the excluded folder.
    assert!(!v2_lib::backup::safe_relative_path("autorun/sessions\\admin.json"));
    assert!(!v2_lib::backup::safe_relative_path("autorun\\shots\\x.jpg"));
    // The Windows file system is case-insensitive: `autorun/Sessions` IS
    // `autorun/sessions` on disk, whatever case a backup entry spells it in.
    assert!(!v2_lib::backup::safe_relative_path("AutoRun/Sessions/admin.json"));
    assert!(!v2_lib::backup::safe_relative_path("autorun/SESSIONS/admin.json"));
}

/// `save_accounts` drops a saved session the moment the login behind it
/// changes; a backup restore writes `autorun/accounts.json` straight to
/// disk and must make the same guarantee, or the next run restores the
/// OLD person's cookies under whatever account the backup's admin now is.
#[test]
fn restoring_accounts_json_drops_the_old_sessions() {
    let dst = tmpdir("restore-drops-sessions");
    std::fs::create_dir_all(dst.join("autorun/sessions")).unwrap();
    std::fs::write(dst.join("autorun/sessions/admin.json"), "{}").unwrap();

    let b64 = base64::engine::general_purpose::STANDARD;
    let files = vec![BackupFile { path: "autorun/accounts.json".into(), b64: b64.encode(b"[]") }];
    restore_files(&dst, &files).unwrap();

    assert!(
        !dst.join("autorun/sessions").exists(),
        "a restore that writes accounts.json must drop the old sessions folder"
    );
    let _ = std::fs::remove_dir_all(&dst);
}

/// A backup with no accounts.json in it must leave saved sessions alone -
/// there is no new login for them to disagree with.
#[test]
fn a_restore_without_accounts_json_leaves_sessions_alone() {
    let dst = tmpdir("restore-keeps-sessions");
    std::fs::create_dir_all(dst.join("autorun/sessions")).unwrap();
    std::fs::write(dst.join("autorun/sessions/admin.json"), "{}").unwrap();

    let b64 = base64::engine::general_purpose::STANDARD;
    let files = vec![BackupFile { path: "autorun/scripts/case-1.json".into(), b64: b64.encode(b"{}") }];
    restore_files(&dst, &files).unwrap();

    assert!(
        dst.join("autorun/sessions/admin.json").is_file(),
        "sessions must be left alone when the backup carries no accounts.json"
    );
    let _ = std::fs::remove_dir_all(&dst);
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
