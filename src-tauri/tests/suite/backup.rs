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

/// Accounts are kept per environment now: restoring any one environment's
/// list must drop the saved sessions just as restoring `accounts.json` does.
#[test]
fn restoring_an_environments_accounts_drops_the_old_sessions() {
    let dst = tmpdir("restore-env-drops-sessions");
    std::fs::create_dir_all(dst.join("autorun/sessions/env-0a1b2c3d")).unwrap();
    std::fs::write(dst.join("autorun/sessions/env-0a1b2c3d/admin.json"), "{}").unwrap();

    let b64 = base64::engine::general_purpose::STANDARD;
    // Mixed case in the folder below autorun, as Windows would read it.
    let files = vec![BackupFile { path: "autorun/Accounts/env-0a1b2c3d.json".into(), b64: b64.encode(b"[]") }];
    restore_files(&dst, &files).unwrap();

    assert!(dst.join("autorun/accounts/env-0a1b2c3d.json").is_file(), "the list was not restored");
    assert!(
        !dst.join("autorun/sessions").exists(),
        "a restore that writes an environment's accounts must drop the old sessions folder"
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

/// Windows reads "autorun/Accounts.json" as the same file, so the old
/// sessions must go for that spelling too.
#[test]
fn accounts_json_in_any_letter_case_drops_the_old_sessions() {
    let dst = tmpdir("restore-drops-sessions-anycase");
    std::fs::create_dir_all(dst.join("autorun/sessions")).unwrap();
    std::fs::write(dst.join("autorun/sessions/admin.json"), "{}").unwrap();

    let b64 = base64::engine::general_purpose::STANDARD;
    let files = vec![BackupFile { path: "autorun/Accounts.json".into(), b64: b64.encode(b"[]") }];
    restore_files(&dst, &files).unwrap();

    assert!(!dst.join("autorun/sessions").exists());
    let _ = std::fs::remove_dir_all(&dst);
}

/// The logins were replaced the moment accounts.json was written. A later
/// entry failing must not leave yesterday's cookies under today's names.
#[test]
fn a_restore_that_fails_after_accounts_json_has_still_dropped_the_sessions() {
    let dst = tmpdir("restore-fails-late");
    std::fs::create_dir_all(dst.join("autorun/sessions")).unwrap();
    std::fs::write(dst.join("autorun/sessions/admin.json"), "{}").unwrap();

    let b64 = base64::engine::general_purpose::STANDARD;
    let files = vec![
        BackupFile { path: "autorun/accounts.json".into(), b64: b64.encode(b"[]") },
        BackupFile { path: "autorun/scripts/case-1.json".into(), b64: "this is not base64 !!".into() },
    ];
    assert!(restore_files(&dst, &files).is_err());

    assert!(!dst.join("autorun/sessions").exists());
    let _ = std::fs::remove_dir_all(&dst);
}

const OLD_LIST: &str = r#"[{"key":"hr.sup","label":"Supervisor","username":"sup1","password":"pw-Zq9"}]"#;

fn usernames(root: &std::path::Path) -> Vec<String> {
    v2_lib::autorun::accounts::load_accounts(root).unwrap().into_iter().map(|a| a.username).collect()
}

/// A backup made before environments carries the one machine-wide
/// `autorun/accounts.json` and no `autorun/environments.json`. By the time
/// anyone imports it the app has already made Default (the title bar reads
/// the active environment at startup), so that list would land in a file
/// nothing reads. It must become the ACTIVE environment's list instead.
#[test]
fn an_old_backups_accounts_land_in_the_active_environment() {
    let dst = tmpdir("restore-old-accounts");
    let root = dst.join("autorun");
    std::fs::create_dir_all(&root).unwrap();
    let env_file = v2_lib::environments::load_or_init(&root, Some("dev-read")).unwrap();
    std::fs::create_dir_all(root.join("sessions").join(&env_file.active)).unwrap();
    std::fs::write(root.join("sessions").join(&env_file.active).join("hr.sup.json"), "{}").unwrap();

    let b64 = base64::engine::general_purpose::STANDARD;
    let files = vec![BackupFile { path: "autorun/accounts.json".into(), b64: b64.encode(OLD_LIST) }];
    restore_files(&dst, &files).unwrap();

    assert_eq!(usernames(&root), ["sup1"], "the restored list is the active environment's");
    assert_eq!(v2_lib::environments::active_id(&root).unwrap(), env_file.active, "no new environment was made");
    assert!(!root.join("sessions").exists(), "sessions are dropped as for any accounts restore");
    assert!(!root.join("accounts.json").exists(), "the frozen machine-wide copy is not left behind");
    let _ = std::fs::remove_dir_all(&dst);
}

/// The same backup restored before Default exists: Default is made from it.
#[test]
fn an_old_backup_on_a_fresh_machine_becomes_defaults_accounts() {
    let dst = tmpdir("restore-old-accounts-fresh");
    let b64 = base64::engine::general_purpose::STANDARD;
    let files = vec![BackupFile { path: "autorun/accounts.json".into(), b64: b64.encode(OLD_LIST) }];
    restore_files(&dst, &files).unwrap();

    let root = dst.join("autorun");
    assert_eq!(v2_lib::environments::active(&root).unwrap().name, "Default");
    assert_eq!(usernames(&root), ["sup1"]);
    let _ = std::fs::remove_dir_all(&dst);
}

/// A backup that carries its own environments keeps them as they were: a
/// stray machine-wide list in it must not overwrite an environment's.
#[test]
fn a_backup_with_environments_keeps_its_own_accounts() {
    let dst = tmpdir("restore-new-accounts");
    let b64 = base64::engine::general_purpose::STANDARD;
    let envs = r#"{"active":"env-0a1b2c3d","environments":[{"id":"env-0a1b2c3d","name":"Default","start_url":"","allowed_origins":[],"db_id":"dev-read","test_environment":false}]}"#;
    let own = r#"[{"key":"hr.emp","label":"Employee","username":"emp-new","password":"pw-new"}]"#;
    let files = vec![
        BackupFile { path: "autorun/accounts.json".into(), b64: b64.encode(OLD_LIST) },
        BackupFile { path: "autorun/environments.json".into(), b64: b64.encode(envs) },
        BackupFile { path: "autorun/accounts/env-0a1b2c3d.json".into(), b64: b64.encode(own) },
    ];
    restore_files(&dst, &files).unwrap();

    assert_eq!(usernames(&dst.join("autorun")), ["emp-new"]);
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

/// A person's own databases travel by name (the list holds no login), and
/// come back as a MERGE: what this machine has stays, the backup's are added
/// with no login, a clashing name gets " 2", retired ids are combined - and
/// an environment restored beside them still means the same database.
#[test]
fn own_databases_travel_and_merge_into_this_machines_list() {
    use v2_lib::db::credentials::{
        add_custom, databases, is_known, remove_custom, DbCredentialsForm, MemoryStore, SecretStore, WithList,
    };
    let form = DbCredentialsForm {
        server: "sql.staging".into(),
        port: None,
        database: "HR".into(),
        user: "tester".into(),
        password: Some("S3cret-XYZ".into()),
        trust_cert: false,
    };

    // The machine the backup is made on: two own databases, one removed
    // before (retired), and an environment that uses one of them.
    let src = tmpdir("dbs-src");
    let theirs = WithList { store: MemoryStore::default(), list: src.join("databases.json") };
    let staging = add_custom(&theirs, "Staging", &form).unwrap();
    let qa = add_custom(&theirs, "QA box", &form).unwrap();
    let gone = add_custom(&theirs, "Old one", &form).unwrap();
    remove_custom(&theirs, &gone.id).unwrap();
    let known: Vec<String> = databases(&theirs).into_iter().map(|d| d.id).collect();
    let root = src.join("autorun");
    let mut default = v2_lib::environments::load_or_init(&root, Some(&staging.id)).unwrap().environments.remove(0);
    default.name = "Staging env".into();
    v2_lib::environments::save_env(&root, default, &known).unwrap();

    let (files, _) = collect_files(&src);
    assert!(files.iter().any(|(p, _)| p == "databases.json"), "the list is not in the backup");
    let carried = String::from_utf8(files.iter().find(|(p, _)| p == "databases.json").unwrap().1.clone()).unwrap();
    assert!(!carried.contains("S3cret-XYZ") && !carried.contains("sql.staging"), "{carried}");
    let doc = build_doc("2.0.9", "2026-10-02T00:00:00Z", BTreeMap::new(), files);

    // The machine it is restored on: its own "Staging" (a different
    // database, with a login), and one id it retired itself.
    let dst = tmpdir("dbs-dst");
    let ours = WithList { store: MemoryStore::default(), list: dst.join("databases.json") };
    let mine = add_custom(&ours, "Staging", &form).unwrap();
    let mine_gone = add_custom(&ours, "Scratch", &form).unwrap();
    remove_custom(&ours, &mine_gone.id).unwrap();

    restore_files(&dst, &doc.files).unwrap();

    let after: Vec<(String, String)> = databases(&ours).into_iter().filter(|d| !d.shipped).map(|d| (d.id, d.label)).collect();
    assert_eq!(
        after,
        vec![
            ("own".to_string(), "Your own database".to_string()),
            (mine.id.clone(), "Staging".to_string()),
            (staging.id.clone(), "Staging 2".to_string()),
            (qa.id.clone(), "QA box".to_string()),
        ]
    );
    // This machine's own login is untouched; the restored ones have none.
    assert!(ours.get(&format!("tcm-v2/db/{}", mine.id)).unwrap().is_some());
    let restored = databases(&ours).into_iter().find(|d| d.id == staging.id).unwrap();
    assert!(restored.server.is_empty() && !restored.has_password);
    // Both sides' retired ids stay retired, and neither comes back.
    let text = std::fs::read_to_string(dst.join("databases.json")).unwrap();
    assert!(text.contains(&gone.id) && text.contains(&mine_gone.id), "{text}");
    assert!(!is_known(&ours, &gone.id) && !is_known(&ours, &mine_gone.id));
    // The restored environment still names a database this machine knows.
    let env = v2_lib::environments::active(&dst.join("autorun")).unwrap();
    assert_eq!(env.db_id, staging.id);
    assert!(is_known(&ours, &env.db_id));

    // Restoring the same backup again adds nothing twice.
    restore_files(&dst, &doc.files).unwrap();
    assert_eq!(databases(&ours).into_iter().filter(|d| !d.shipped).count(), 4);

    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

/// A backup whose list cannot be read restores everything else and leaves
/// this machine's list as it was.
#[test]
fn an_unreadable_list_in_a_backup_is_skipped() {
    use v2_lib::db::credentials::{add_custom, databases, DbCredentialsForm, MemoryStore, WithList};
    let dst = tmpdir("dbs-bad");
    let ours = WithList { store: MemoryStore::default(), list: dst.join("databases.json") };
    let form = DbCredentialsForm { server: "h".into(), port: None, database: "d".into(), user: "u".into(), password: Some("p".into()), trust_cert: false };
    add_custom(&ours, "Staging", &form).unwrap();
    let before = std::fs::read_to_string(dst.join("databases.json")).unwrap();
    let b64 = base64::engine::general_purpose::STANDARD;
    let files = vec![
        BackupFile { path: "databases.json".into(), b64: b64.encode(b"{ not json") },
        BackupFile { path: "cache.json".into(), b64: b64.encode(b"{}") },
    ];
    restore_files(&dst, &files).unwrap();
    assert_eq!(std::fs::read_to_string(dst.join("databases.json")).unwrap(), before);
    assert!(dst.join("cache.json").exists());
    assert_eq!(databases(&ours).into_iter().filter(|d| !d.shipped).count(), 2);
    let _ = std::fs::remove_dir_all(&dst);
}

/// This machine's own list unreadable: the merge is skipped and logged,
/// and the rest of the restore still completes - environments included -
/// with the local list left exactly as it was.
#[test]
fn an_unreadable_local_list_skips_the_merge_and_the_restore_completes() {
    let dst = tmpdir("dbs-local-bad");
    std::fs::write(dst.join("databases.json"), "{ not json").unwrap();
    let b64 = base64::engine::general_purpose::STANDARD;
    let envs = r#"{"active":"env-0000000a","environments":[{"id":"env-0000000a","name":"Restored","start_url":"","allowed_origins":[],"db_id":"custom-1a2b3c4d","test_environment":false}]}"#;
    let list = r#"{"databases":[{"id":"custom-1a2b3c4d","label":"Staging"}],"retired":[]}"#;
    let files = vec![
        BackupFile { path: "databases.json".into(), b64: b64.encode(list) },
        BackupFile { path: "autorun/environments.json".into(), b64: b64.encode(envs) },
        BackupFile { path: "cache.json".into(), b64: b64.encode(b"{}") },
    ];
    let restored = restore_files(&dst, &files).expect("the restore completes");
    assert_eq!(restored, 2, "the two other files land; the list merge is skipped");
    assert_eq!(std::fs::read_to_string(dst.join("databases.json")).unwrap(), "{ not json");
    let env = v2_lib::environments::active(&dst.join("autorun")).unwrap();
    assert_eq!((env.name.as_str(), env.db_id.as_str()), ("Restored", "custom-1a2b3c4d"));
    assert!(dst.join("cache.json").exists());
    let _ = std::fs::remove_dir_all(&dst);
}

// ---- the per-environment layout ---------------------------------------------

const ENV_A: &str = "env-0000000a";
const ENV_B: &str = "env-0000000b";

fn env_json(active: &str, export: Option<&str>) -> String {
    let one = |id: &str, name: &str| {
        format!(r#"{{"id":"{id}","name":"{name}","start_url":"","allowed_origins":[],"db_id":"dev-read","test_environment":false}}"#)
    };
    let export = export.map(|e| format!(r#","playwright_export":"{e}""#)).unwrap_or_default();
    format!(
        r#"{{"active":"{active}","environments":[{},{}]{export}}}"#,
        one(ENV_A, "Alpha"),
        one(ENV_B, "Beta")
    )
}

fn put(root: &std::path::Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn read(root: &std::path::Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

fn entry(path: &str, text: &str) -> BackupFile {
    BackupFile { path: path.into(), b64: base64::engine::general_purpose::STANDARD.encode(text) }
}

/// An updated machine: two environments, each with its own scripts, and the
/// first with history.
fn updated_machine(tag: &str) -> PathBuf {
    let dst = tmpdir(tag);
    let a = dst.join("autorun");
    put(&a, "environments.json", &env_json(ENV_A, None));
    put(&a, &format!("envs/{ENV_A}/scripts/case-1.json"), "machine-a");
    put(&a, &format!("envs/{ENV_A}/runs/run-9.json"), "machine-run");
    put(&a, &format!("envs/{ENV_B}/scripts/case-2.json"), "machine-b");
    dst
}

fn no_staging_left(autorun: &std::path::Path, prefix: &str) -> bool {
    std::fs::read_dir(autorun).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(prefix))
}

#[test]
fn a_backup_carries_every_environment_and_the_seed_but_not_shots_or_the_removed_area() {
    let src = updated_machine("carry-envs");
    let a = src.join("autorun");
    put(&a, "envs/_seed/scripts/case-3.json", "seed");
    put(&a, &format!("envs/{ENV_A}/shots/s1.png"), "png");
    put(&a, &format!("envs/.copying-{ENV_A}-5/scripts/x.json"), "half");
    put(&a, "removed-envs/env-00000001-5/scripts/old.json", "old");
    put(&a, ".envs-staging-5/env-0000000a/scripts/y.json", "stage");
    put(&a, ".import-5/flat/scripts/z.json", "import");
    let (files, _) = collect_files(&src);
    let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    for want in [
        format!("autorun/envs/{ENV_A}/scripts/case-1.json"),
        format!("autorun/envs/{ENV_A}/runs/run-9.json"),
        format!("autorun/envs/{ENV_B}/scripts/case-2.json"),
        "autorun/envs/_seed/scripts/case-3.json".to_string(),
    ] {
        assert!(paths.contains(&want.as_str()), "{want} missing from {paths:?}");
    }
    for bad in ["shots", "removed-envs", ".copying-", ".envs-staging-", ".import-"] {
        assert!(!paths.iter().any(|p| p.contains(bad)), "{bad} travelled: {paths:?}");
    }
    // And an import refuses them too, in any letter case or separator.
    assert!(!safe_relative_path(&format!("autorun/envs/{ENV_A}/shots/s1.png")));
    assert!(!safe_relative_path(&format!("AUTORUN\\envs\\{ENV_A}\\SHOTS\\s1.png")));
    assert!(!safe_relative_path("autorun/removed-envs/env-00000001-5/removed.json"));
    assert!(!safe_relative_path("autorun/.import-5/flat/scripts/z.json"));
    assert!(safe_relative_path(&format!("autorun/envs/{ENV_A}/scripts/case-1.json")));
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn a_backup_leaves_out_the_legacy_originals() {
    let src = updated_machine("legacy-originals");
    let a = src.join("autorun");
    put(&a, "scripts/case-1.json", "legacy");
    put(&a, "projects/p.json", "legacy");
    put(&a, "test-made.json", "[]");
    let (files, _) = collect_files(&src);
    let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    assert!(!paths.contains(&"autorun/scripts/case-1.json") && !paths.contains(&"autorun/projects/p.json"), "{paths:?}");
    assert!(paths.contains(&"autorun/test-made.json"), "shared files still travel");
    // Before the update (no envs/) the same files are the data and travel.
    let flat = tmpdir("legacy-flat");
    put(&flat.join("autorun"), "scripts/case-1.json", "legacy");
    let (files, _) = collect_files(&flat);
    assert!(files.iter().any(|(p, _)| p == "autorun/scripts/case-1.json"));
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&flat);
}

#[test]
fn a_pre_update_backup_fills_every_environment() {
    let dst = updated_machine("pre-update");
    let a = dst.join("autorun");
    let files = vec![
        entry("autorun/environments.json", &env_json(ENV_A, None)),
        entry("autorun/scripts/case-5.json", "from-backup"),
        entry("autorun/runs/run-1.json", "backup-run"),
        entry("autorun/accounts/env-0000000a.json", "[]"),
    ];
    assert!(v2_lib::backup::is_pre_update(&files));
    restore_files(&dst, &files).unwrap();

    for id in [ENV_A, ENV_B] {
        assert_eq!(read(&a, &format!("envs/{id}/scripts/case-5.json")).as_deref(), Some("from-backup"), "{id}");
    }
    assert_eq!(read(&a, &format!("envs/{ENV_A}/runs/run-1.json")).as_deref(), Some("backup-run"));
    assert!(read(&a, &format!("envs/{ENV_B}/runs/run-1.json")).is_none(), "history goes to the active environment only");
    // What the machine had is not in the new folders, and nothing is at the flat path.
    assert!(read(&a, &format!("envs/{ENV_A}/scripts/case-1.json")).is_none());
    assert!(!a.join("scripts").exists() && !a.join("runs").exists(), "nothing lands at the flat path");
    assert!(read(&a, "accounts/env-0000000a.json").is_some(), "shared files restore where they are");
    // The replaced folders wait in the removed area, with their data.
    let removed = v2_lib::autorun::roots::removed_folders(&a, v2_lib::cache::now_ms());
    let mut kept: Vec<(String, String)> = removed.iter().map(|r| (r.env_id.clone(), r.reason.clone())).collect();
    kept.sort();
    assert_eq!(
        kept,
        vec![
            (ENV_A.to_string(), "replaced by an import".to_string()),
            (ENV_B.to_string(), "replaced by an import".to_string())
        ]
    );
    let old_a = removed.iter().find(|r| r.env_id == ENV_A).unwrap();
    assert_eq!(read(&a, &format!("removed-envs/{}/scripts/case-1.json", old_a.key)).as_deref(), Some("machine-a"));
    assert_eq!(read(&a, &format!("removed-envs/{}/runs/run-9.json", old_a.key)).as_deref(), Some("machine-run"));
    assert!(no_staging_left(&a, ".import-"));
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn a_pre_update_backups_export_record_goes_to_the_export_environment() {
    let dst = updated_machine("pre-update-export");
    let a = dst.join("autorun");
    let files = vec![
        entry("autorun/environments.json", &env_json(ENV_A, Some(ENV_B))),
        entry("autorun/projects/p.pwexport.json", "{}"),
        entry("autorun/projects/p.json", "recipe"),
    ];
    restore_files(&dst, &files).unwrap();
    assert!(read(&a, &format!("envs/{ENV_B}/projects/p.pwexport.json")).is_some());
    assert!(read(&a, &format!("envs/{ENV_A}/projects/p.pwexport.json")).is_none());
    for id in [ENV_A, ENV_B] {
        assert!(read(&a, &format!("envs/{id}/projects/p.json")).is_some());
    }
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn a_post_update_backup_restores_the_folders_as_they_are() {
    let dst = updated_machine("post-update");
    let a = dst.join("autorun");
    let files = vec![
        entry("autorun/environments.json", &env_json(ENV_A, None)),
        entry(&format!("autorun/envs/{ENV_A}/scripts/case-1.json"), "from-backup"),
        entry(&format!("autorun/envs/{ENV_B}/scripts/case-7.json"), "b7"),
    ];
    assert!(!v2_lib::backup::is_pre_update(&files));
    restore_files(&dst, &files).unwrap();
    assert_eq!(read(&a, &format!("envs/{ENV_A}/scripts/case-1.json")).as_deref(), Some("from-backup"));
    assert_eq!(read(&a, &format!("envs/{ENV_B}/scripts/case-7.json")).as_deref(), Some("b7"));
    assert_eq!(read(&a, &format!("envs/{ENV_B}/scripts/case-2.json")).as_deref(), Some("machine-b"));
    assert!(!a.join("removed-envs").exists(), "nothing was set aside");
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn an_invalid_environment_folder_in_a_backup_is_skipped() {
    let dst = updated_machine("bad-env-folder");
    let a = dst.join("autorun");
    let files = vec![
        entry("autorun/envs/..evil/x.json", "x"),
        entry("autorun/envs/NOPE/x.json", "x"),
        entry("autorun/envs/ENV-0000000A/x.json", "x"),
        entry("autorun/envs/x.json", "x"),
        entry(&format!("autorun/envs/{ENV_B}/scripts/ok.json"), "ok"),
    ];
    let n = restore_files(&dst, &files).unwrap();
    assert_eq!(n, 1);
    assert!(!a.join("envs/..evil").exists() && !a.join("envs/NOPE").exists() && !a.join("envs/x.json").exists());
    assert!(read(&a, &format!("envs/{ENV_B}/scripts/ok.json")).is_some());
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn a_post_update_backup_on_a_machine_without_the_layout_builds_envs_whole() {
    let dst = tmpdir("post-update-fresh");
    let a = dst.join("autorun");
    put(&a, "scripts/case-1.json", "flat-original");
    let files = vec![
        entry("autorun/environments.json", &env_json(ENV_A, None)),
        entry(&format!("autorun/envs/{ENV_A}/scripts/case-1.json"), "from-backup"),
    ];
    restore_files(&dst, &files).unwrap();
    assert_eq!(read(&a, &format!("envs/{ENV_A}/scripts/case-1.json")).as_deref(), Some("from-backup"));
    assert!(no_staging_left(&a, ".envs-staging-"));
    assert_eq!(read(&a, "scripts/case-1.json").as_deref(), Some("flat-original"), "the flat originals stay as they are");
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn a_pre_update_backup_on_a_machine_without_the_layout_lands_flat() {
    let dst = tmpdir("pre-update-flat");
    let a = dst.join("autorun");
    let files = vec![entry("autorun/scripts/case-5.json", "from-backup")];
    restore_files(&dst, &files).unwrap();
    assert_eq!(read(&a, "scripts/case-5.json").as_deref(), Some("from-backup"));
    assert!(!a.join("envs").exists(), "the next start does the update's copy");
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn a_busy_environment_refuses_an_import_before_anything_moves() {
    let dst = updated_machine("busy-env");
    let a = dst.join("autorun");
    let _busy = v2_lib::autorun::roots::allow_writes_to(&v2_lib::autorun::roots::env_root(&a, ENV_B));
    let files = vec![entry("autorun/environments.json", &env_json(ENV_A, None)), entry("autorun/scripts/case-5.json", "x")];
    let err = restore_files(&dst, &files).unwrap_err();
    assert_eq!(err, v2_lib::autorun::roots::REMOVE_BUSY);
    assert_eq!(read(&a, &format!("envs/{ENV_A}/scripts/case-1.json")).as_deref(), Some("machine-a"));
    assert_eq!(read(&a, &format!("envs/{ENV_B}/scripts/case-2.json")).as_deref(), Some("machine-b"));
    assert!(!a.join("removed-envs").exists());
    assert!(no_staging_left(&a, ".import-"));
    let _ = std::fs::remove_dir_all(&dst);
}

/// The swap stopping part-way (the second environment's folder will not move
/// in) leaves the note and the built folders; the next start finishes it and
/// nothing the machine had is lost.
#[test]
fn an_import_that_stops_part_way_is_finished_at_the_next_start() {
    use v2_lib::autorun::{backup_import, roots};
    let dst = updated_machine("import-stops");
    let a = dst.join("autorun");
    let staging = backup_import::staging_dir(&a, 4242);
    put(&backup_import::flat_dir(&staging), "scripts/case-5.json", "from-backup");
    let ids = vec![ENV_A.to_string(), ENV_B.to_string()];
    let names = |id: &str| if id == ENV_A { "Alpha".to_string() } else { "Beta".to_string() };
    let mut fail_b = |from: &std::path::Path, to: &std::path::Path| {
        if to.ends_with(ENV_B) && from.to_string_lossy().contains("built") {
            Err(std::io::Error::other("held"))
        } else {
            std::fs::rename(from, to)
        }
    };
    {
        let held = roots::switch_lock().write().unwrap();
        let err = backup_import::replace_environments(&held, &a, &staging, &ids, ENV_A, &names, 4242, &mut fail_b, &[])
            .unwrap_err();
        assert_eq!(err, backup_import::IMPORT_UNFINISHED);
    }
    assert_eq!(read(&a, &format!("envs/{ENV_A}/scripts/case-5.json")).as_deref(), Some("from-backup"));
    assert!(staging.is_dir(), "the note and the built folders wait");
    assert_eq!(backup_import::recover(&a), 1);
    assert_eq!(read(&a, &format!("envs/{ENV_B}/scripts/case-5.json")).as_deref(), Some("from-backup"));
    assert!(!staging.exists());
    let removed = roots::removed_folders(&a, 4242);
    let old_b = removed.iter().find(|r| r.env_id == ENV_B).expect("B's old folder is kept");
    assert_eq!(read(&a, &format!("removed-envs/{}/scripts/case-2.json", old_b.key)).as_deref(), Some("machine-b"));
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn an_import_that_never_started_putting_data_in_place_is_cleared() {
    use v2_lib::autorun::backup_import;
    let dst = updated_machine("import-unmarked");
    let a = dst.join("autorun");
    let staging = backup_import::staging_dir(&a, 77);
    put(&backup_import::flat_dir(&staging), "scripts/case-5.json", "copy");
    assert_eq!(backup_import::recover(&a), 0);
    assert!(!staging.exists());
    assert_eq!(read(&a, &format!("envs/{ENV_A}/scripts/case-1.json")).as_deref(), Some("machine-a"), "live data untouched");
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn the_preview_names_the_environments_a_pre_update_import_replaces() {
    let dst = updated_machine("preview");
    let mut doc = build_doc("2.0.0", "2026-10-01", BTreeMap::new(), vec![]);
    doc.files = vec![entry("autorun/environments.json", &env_json(ENV_A, None)), entry("autorun/scripts/c.json", "x")];
    let p = v2_lib::backup::preview(&dst, &doc);
    assert!(p.pre_update);
    assert_eq!(p.replaces, ["Alpha", "Beta"]);
    // Without a list in the backup, this machine's environments.
    doc.files = vec![entry("autorun/scripts/c.json", "x")];
    assert_eq!(v2_lib::backup::preview(&dst, &doc).replaces, ["Alpha", "Beta"]);
    doc.files = vec![entry(&format!("autorun/envs/{ENV_A}/scripts/c.json"), "x")];
    let p = v2_lib::backup::preview(&dst, &doc);
    assert!(!p.pre_update && p.replaces.is_empty());
    let _ = std::fs::remove_dir_all(&dst);
}

/// After an import replaced the list, the Auto Run folder in use is the new
/// active environment's, and an import that left no environment at all makes
/// none (Default is the list's job, with the chosen database).
#[test]
fn the_app_follows_the_imported_active_environment() {
    use v2_lib::autorun::{roots, store};
    let _lock = crate::serial::autorun();
    let dst = updated_machine("settle");
    let a = dst.join("autorun");
    store::set_shared_root(a.clone());
    assert_eq!(store::select_environment(ENV_A).unwrap(), roots::env_root(&a, ENV_A));
    // The import replaced the list: B is active now.
    put(&a, "environments.json", &env_json(ENV_B, None));
    let secrets = v2_lib::db::credentials::MemoryStore::default();
    let held = roots::switch_lock().write().unwrap();
    let settled = v2_lib::commands::environments::settle_after_import(&a, &secrets, None).unwrap();
    drop(held);
    assert_eq!(settled, Some((ENV_B.to_string(), false)));
    assert_eq!(store::selected_root(), Some(roots::env_root(&a, ENV_B)));

    // No environments file at all: nothing is made, nothing is moved.
    let empty = tmpdir("settle-empty");
    let e = empty.join("autorun");
    std::fs::create_dir_all(e.join("envs")).unwrap();
    store::set_shared_root(e.clone());
    let held = roots::switch_lock().write().unwrap();
    assert_eq!(v2_lib::commands::environments::settle_after_import(&e, &secrets, None).unwrap(), None);
    drop(held);
    assert!(!e.join("environments.json").exists());
    store::set_shared_root(dst.join("elsewhere"));
    let _ = std::fs::remove_dir_all(&dst);
    let _ = std::fs::remove_dir_all(&empty);
}
