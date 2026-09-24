//! The session's own rule: a step cannot run without an open browser,
//! and the failure says how to fix it rather than panicking on an
//! absent session.

use v2_lib::autorun::nav::{no_address, save_nav, NavFile};
use v2_lib::autorun::store::{load_run, load_script, save_run, save_run_guarded};
use v2_lib::autorun::CaseScript;
use v2_lib::autorun::{LocalRun, PublishedRun};
use v2_lib::commands::autorun::{
    describe_session_error, import_scripts_from_path, refuse_while_a_run_is_going, safe_run_id, save_script_from_editor,
};
use v2_lib::commands::autorun_replay::{replay_is_running, replay_timing, OneAtATime};

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
    let reloaded = load_run(dir.path(), "run-3").unwrap().unwrap();
    assert!(reloaded.published.is_some(), "the guard let the published record be erased");

    let edited_note = LocalRun { cases: vec![], ..published.clone() };
    assert!(save_run_guarded(dir.path(), &edited_note).is_ok());
}

/// A run file that exists but cannot be parsed (corrupt JSON, a partial
/// write) must never be treated as "no existing run" - that would let the
/// guard fail open and silently overwrite whatever was really on disk,
/// published or not. The guarded save refuses instead, and the garbage
/// file itself is left exactly as it was.
#[test]
fn a_run_file_that_cannot_be_read_is_never_overwritten_by_a_guarded_save() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("runs")).unwrap();
    std::fs::write(dir.path().join("runs").join("run-x.json"), "{ not json").unwrap();

    let run = LocalRun {
        id: "run-x".to_string(),
        pbi_id: 1,
        started_at: "1".to_string(),
        cases: vec![],
        mode: String::new(),
        published: None,
    };
    let err = save_run_guarded(dir.path(), &run).expect_err("a corrupt existing file was overwritten");
    assert!(err.contains("could not be read"), "{err}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("runs").join("run-x.json")).unwrap(),
        "{ not json",
        "the guarded save must not touch the file it could not read"
    );
}

/// `save_run_guarded` is a second IPC-adjacent write path into the runs
/// directory, same as `save_run` behind the command - it must apply the
/// same id rule itself rather than relying on a caller to have checked
/// first.
#[test]
fn save_run_guarded_rejects_an_unsafe_run_id() {
    let dir = tempfile::tempdir().unwrap();
    let run = LocalRun {
        id: "../escape".to_string(),
        pbi_id: 1,
        started_at: "1".to_string(),
        cases: vec![],
        mode: String::new(),
        published: None,
    };
    assert!(save_run_guarded(dir.path(), &run).is_err());
    assert!(
        std::fs::read_dir(dir.path()).map(|mut d| d.next().is_none()).unwrap_or(true),
        "an unsafe id must write nothing"
    );
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

    let ids = import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap()).expect("import failed");
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

// ---- Unattended replay's IPC shell --------------------------------------

/// Only one unattended run may be going at a time; the second caller is
/// refused until the first one's claim is dropped. `replay_is_running`
/// (what `auto_run_open_browser` will check, per F7) has to track the
/// exact same state - asserted in the SAME test, not a separate one,
/// because both touch the one process-wide `RUNNING` flag and cargo runs
/// tests in this binary in parallel by default.
#[test]
fn only_one_unattended_run_at_a_time_and_the_claim_is_given_back() {
    assert!(!replay_is_running(), "nothing is running yet");
    let first = OneAtATime::claim().expect("nothing is running");
    assert!(replay_is_running());
    assert!(OneAtATime::claim().is_none(), "a second run must be refused while the first is going");
    drop(first);
    assert!(!replay_is_running(), "a finished run must free the slot");
    assert!(OneAtATime::claim().is_some(), "a finished run must free the slot");
}

/// Nobody is watching a background run, so it does not pause to highlight
/// where it is about to click - a watched one still does.
#[test]
fn nobody_is_watching_a_background_run_so_it_does_not_pause_to_point() {
    assert_eq!(replay_timing(false).highlight_ms, 0);
    assert!(replay_timing(true).highlight_ms > 0);
    assert_eq!(replay_timing(false).action_ms, v2_lib::browser::timing::Timing::default().action_ms);
}

// ---- Clearing scripts and results (dev-only Auto Run toolbar) ----------

/// `auto_run_clear_scripts` and `auto_run_clear_runs` share this guard
/// with `auto_run_open_browser`'s own refusal while an unattended run is
/// going: a run in progress reads scripts and writes runs, so clearing
/// either while one is live would race the very files it is using. The
/// sentence must read exactly the same wherever a person sees it - this
/// pins it to `auto_run_open_browser`'s own wording rather than letting
/// the two drift apart.
#[tokio::test]
async fn clearing_is_refused_while_an_unattended_run_is_going_with_open_browsers_own_sentence() {
    assert!(refuse_while_a_run_is_going().await.is_ok(), "nothing is running yet");
    let claim = OneAtATime::claim().expect("nothing is running");
    let err = refuse_while_a_run_is_going().await.expect_err("a run is going");
    assert_eq!(err, "an unattended run is going - wait for it, or stop it first");
    drop(claim);
    assert!(refuse_while_a_run_is_going().await.is_ok(), "the claim was freed - clearing is allowed again");
}

fn with_navigate(case_id: i32) -> serde_json::Value {
    serde_json::json!({ "case_id": case_id, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] },
        { "step_number": 2, "actions": [{ "kind": "navigate", "url": "/hr/leave/apply" }] }
    ] })
}

#[test]
fn with_addresses_switched_off_the_editor_and_an_import_refuse_a_navigate_and_write_nothing() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    save_nav(&root, "acme", "Web", &NavFile { direct_urls: false, modules: vec![] }).unwrap();
    let script: CaseScript = serde_json::from_value(with_navigate(7)).unwrap();

    let err = save_script_from_editor(&root, "acme", "Web", script.clone()).unwrap_err();
    assert_eq!(err, format!("case 7: {}", no_address(2)));
    assert!(load_script(&root, 7).unwrap().is_none());

    let file = dir.path().join("bundle.json");
    std::fs::write(&file, serde_json::Value::Array(vec![with_navigate(8)]).to_string()).unwrap();
    let err = import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap()).unwrap_err();
    assert_eq!(err, format!("case 8: {}", no_address(2)));
    assert!(load_script(&root, 8).unwrap().is_none());

    // Another project, whose switch was never touched, takes the same script.
    save_script_from_editor(&root, "acme", "Other", script).unwrap();
    assert!(load_script(&root, 7).unwrap().is_some());
}
