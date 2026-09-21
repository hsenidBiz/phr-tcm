//! The session's own rule: a step cannot run without an open browser,
//! and the failure says how to fix it rather than panicking on an
//! absent session.

use v2_lib::autorun::store::{load_script, save_run, save_run_guarded};
use v2_lib::autorun::{LocalRun, PublishedRun};
use v2_lib::commands::autorun::{describe_session_error, import_scripts_from_path, safe_run_id};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-autorun-import-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stepping_without_a_browser_says_to_open_one() {
    let msg = describe_session_error();
    assert!(msg.to_lowercase().contains("open"), "unhelpful: {msg}");
    assert!(msg.to_lowercase().contains("browser"), "unhelpful: {msg}");
}

/// `store::save_run` writes the id straight into a filename with no
/// sanitisation of its own; `auto_run_save_run` is the id's first
/// IPC-facing entry point, so a hostile or buggy frontend value must be
/// rejected before it ever reaches the filesystem.
#[test]
fn run_ids_that_could_escape_the_runs_directory_are_rejected() {
    assert!(safe_run_id("run-1738972800000"));
    assert!(!safe_run_id(""));
    assert!(!safe_run_id("../../evil"));
    assert!(!safe_run_id("nested/path"));
    assert!(!safe_run_id("back\\slash"));
    assert!(!safe_run_id("."));
    assert!(!safe_run_id(".."));
}

/// Once a run has been sent to Azure DevOps, a stale review screen saving
/// an older copy of it - one that never saw the send - must never erase
/// the record that it happened. Saving the SAME run back, `published`
/// block and all (a note edited after sending), is still allowed.
#[test]
fn saving_over_a_published_run_with_an_unpublished_copy_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let published = LocalRun {
        id: "run-3".to_string(),
        pbi_id: 1,
        started_at: "1".to_string(),
        cases: vec![],
        mode: String::new(),
        published: Some(PublishedRun {
            run_id: 555,
            web_url: "https://dev.azure.com/org/proj/_workitems/edit/555".to_string(),
            at: "1700000000000".to_string(),
        }),
    };
    save_run(dir.path(), &published).unwrap();

    let stale = LocalRun { published: None, ..published.clone() };
    let err = save_run_guarded(dir.path(), &stale).expect_err("a stale unpublished copy was accepted");
    assert!(err.contains("already been sent"), "{err}");
    let reloaded = v2_lib::autorun::store::load_run(dir.path(), "run-3").unwrap().unwrap();
    assert!(reloaded.published.is_some(), "the guard let the published record be erased");

    let edited_note = LocalRun { cases: vec![], ..published.clone() };
    assert!(save_run_guarded(dir.path(), &edited_note).is_ok());
}

/// The frontend used to read the picked file itself and hand Rust base64
/// text, decoded with `atob` - which produces a latin-1 binary string, so
/// any non-ASCII byte came out mojibake, and a leading UTF-8 BOM made the
/// JSON look corrupt before it ever reached the parser. Importing now
/// reads the file Rust-side instead, the same way `import_parser` does,
/// so both a BOM and non-ASCII text must survive the round trip intact.
#[test]
fn importing_a_utf8_file_with_a_bom_keeps_non_ascii_text_intact() {
    let dir = TempDir::new();
    let root = dir.path().join("data");

    let value = "Caf\u{e9} \u{2014} \u{201c}smart quotes\u{201d} and a non\u{a0}breaking space";
    let bundle = serde_json::json!([{
        "case_id": 301,
        "title": "Caf\u{e9} accents in the title too",
        "steps": [{
            "step_number": 1,
            "actions": [{ "kind": "check_text", "value": value }]
        }]
    }]);

    // A real UTF-8 BOM (EF BB BF) prepended to otherwise-normal UTF-8
    // JSON bytes - exactly what a text editor commonly writes.
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(bundle.to_string().as_bytes());
    let file = dir.path().join("bundle.json");
    std::fs::write(&file, &bytes).unwrap();

    let ids = import_scripts_from_path(&root, file.to_str().unwrap()).expect("import failed");
    assert_eq!(ids, vec![301]);

    let loaded = load_script(&root, 301).unwrap().unwrap();
    assert_eq!(loaded.title, "Caf\u{e9} accents in the title too");
    match &loaded.steps[0].actions[0] {
        v2_lib::browser::actions::Action::CheckText { value: got } => {
            assert_eq!(got, value, "non-ASCII text was corrupted on import");
        }
        other => panic!("unexpected action: {other:?}"),
    }
}
