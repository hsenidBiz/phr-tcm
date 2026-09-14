//! Backups must carry the app's one cache file (`cache.json`): tags,
//! resolved suites and the assigned-items baseline all live in it now, so a
//! backup that drops it silently loses that data on restore.

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
