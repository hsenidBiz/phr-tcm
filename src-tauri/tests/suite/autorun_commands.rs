//! The session's own rule: a step cannot run without an open browser,
//! and the failure says how to fix it rather than panicking on an
//! absent session.

use v2_lib::autorun::nav::{no_address, save_nav, NavFile};
use v2_lib::autorun::store::{load_run, load_script, save_run, save_run_guarded};
use v2_lib::autorun::CaseScript;
use v2_lib::autorun::{LocalRun, PublishedRun};
use v2_lib::commands::autorun::{
    describe_session_error, import_scripts_from_path, refuse_while_a_run_is_going, safe_run_id, save_script_from_editor,
    CaseTexts, IMPORT_NEEDS_CASES, IMPORT_UNSEEN_THEN, NO_PROJECT,
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
        environment: None,
        resets: vec![],
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
        environment: None,
        resets: vec![],
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
        environment: None,
        resets: vec![],
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

    let cases = case_texts(&[(301, &[])]);
    let ids = import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap(), Some(&cases)).expect("import failed");
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
/// tests in this binary in parallel by default. Other modules claim it
/// too, so the claim is taken under the suite's autorun lock.
#[test]
fn only_one_unattended_run_at_a_time_and_the_claim_is_given_back() {
    let _claims = crate::serial::autorun();
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
    let _claims = crate::serial::autorun();
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
    save_nav(&root, "acme", "Web", &NavFile { direct_urls: false, modules: vec![], save_words: vec![] }).unwrap();
    let script: CaseScript = serde_json::from_value(with_navigate(7)).unwrap();

    let err = save_script_from_editor(&root, "acme", "Web", script.clone()).unwrap_err();
    assert_eq!(err, format!("case 7: {}", no_address(2)));
    assert!(load_script(&root, 7).unwrap().is_none());

    let file = dir.path().join("bundle.json");
    std::fs::write(&file, serde_json::Value::Array(vec![with_navigate(8)]).to_string()).unwrap();
    let err = import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap(), None).unwrap_err();
    assert_eq!(err, format!("case 8: {}", no_address(2)));
    assert!(load_script(&root, 8).unwrap().is_none());

    // Another project, whose switch was never touched, takes the same script.
    save_script_from_editor(&root, "acme", "Other", script).unwrap();
    assert!(load_script(&root, 7).unwrap().is_some());
}

/// Whether a script may open pages by address is a project's own rule, so
/// a save or an import that names no project is refused before anything
/// else - including a script with no navigate at all - and writes nothing.
#[test]
fn a_save_or_an_import_with_no_organization_or_project_is_refused_and_writes_nothing() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    let script: CaseScript = serde_json::from_value(with_navigate(7)).unwrap();
    let file = dir.path().join("bundle.json");
    std::fs::write(&file, serde_json::Value::Array(vec![with_navigate(8)]).to_string()).unwrap();

    for (org, project) in [("", "Web"), ("acme", ""), ("  ", "Web"), ("acme", " ")] {
        let err = save_script_from_editor(&root, org, project, script.clone()).unwrap_err();
        assert_eq!(err, NO_PROJECT, "{org:?} / {project:?}");
        let err = import_scripts_from_path(&root, org, project, file.to_str().unwrap(), None).unwrap_err();
        assert_eq!(err, NO_PROJECT, "{org:?} / {project:?}");
    }
    assert!(!err_names_an_address(NO_PROJECT), "{NO_PROJECT}");
    assert!(!root.exists(), "nothing was written anywhere under the data folder");
}

fn err_names_an_address(s: &str) -> bool {
    s.contains("://") || s.contains("dev.azure.com")
}

/// A script of case 7 with `setup` as given, through the editor's own save.
fn with_setup(setup: Option<&str>) -> CaseScript {
    let mut v = serde_json::json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ] });
    if let Some(f) = setup {
        v["setup"] = serde_json::json!({ "fixture": f });
    }
    serde_json::from_value(v).unwrap()
}

/// Puts fixtures on disk as the store reads them, so a script may name one.
/// Written as files: saving through the store would need its templates too,
/// which this test is not about.
fn put_fixtures(root: &std::path::Path, ids: &[&str]) {
    let dir = v2_lib::api_templates::fixture_store::fixtures_dir(root, "acme", "Web");
    std::fs::create_dir_all(&dir).unwrap();
    for id in ids {
        let f = serde_json::json!({ "id": id, "name": id, "account": "hr.admin", "steps": [] });
        std::fs::write(dir.join(format!("{id}.json")), f.to_string()).unwrap();
    }
}

fn stored_setup(root: &std::path::Path) -> Option<String> {
    load_script(root, 7).unwrap().unwrap().setup.map(|s| s.fixture)
}

#[test]
fn an_editor_save_with_a_different_setup_keeps_the_stored_one() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    put_fixtures(&root, &["draft-cycle", "another-fixture"]);
    v2_lib::autorun::store::save_scripts_atomically(&root, &[with_setup(Some("draft-cycle"))]).unwrap();
    save_script_from_editor(&root, "acme", "Web", with_setup(Some("another-fixture"))).unwrap();
    assert_eq!(stored_setup(&root).as_deref(), Some("draft-cycle"));
}

#[test]
fn an_editor_save_with_no_setup_keeps_the_stored_one() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    put_fixtures(&root, &["draft-cycle", "another-fixture"]);
    v2_lib::autorun::store::save_scripts_atomically(&root, &[with_setup(Some("draft-cycle"))]).unwrap();
    save_script_from_editor(&root, "acme", "Web", with_setup(None)).unwrap();
    assert_eq!(stored_setup(&root).as_deref(), Some("draft-cycle"));
}

/// The editor owns what it shows. Every other setting is the stored
/// script's, so a save from an editor that never sent them keeps them all.
#[test]
fn an_editor_save_keeps_every_setting_it_does_not_show() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    put_fixtures(&root, &["draft-cycle"]);
    let mut stored = with_setup(Some("draft-cycle"));
    stored.fail_on_unexpected_dialog = true;
    stored.page_errors = Some(v2_lib::autorun::PageErrors::Flag);
    stored.ignore_page_errors = vec!["ResizeObserver".to_string()];
    v2_lib::autorun::store::save_scripts_atomically(&root, &[stored]).unwrap();

    // What the editor sends: its own fields, none of these.
    let mut edited = with_setup(None);
    edited.title = "renamed".to_string();
    save_script_from_editor(&root, "acme", "Web", edited).unwrap();

    let kept = load_script(&root, 7).unwrap().unwrap();
    assert_eq!(kept.title, "renamed", "the editor's own field is the editor's");
    assert!(kept.fail_on_unexpected_dialog);
    assert_eq!(kept.page_errors, Some(v2_lib::autorun::PageErrors::Flag));
    assert_eq!(kept.ignore_page_errors, ["ResizeObserver"]);
    assert_eq!(kept.setup.map(|s| s.fixture).as_deref(), Some("draft-cycle"));

    // And a stale editor cannot turn them on for a script that has none.
    let dir = TempDir::new();
    let root = dir.path().join("data");
    let mut sneaky = with_setup(None);
    sneaky.page_errors = Some(v2_lib::autorun::PageErrors::Fail);
    sneaky.fail_on_unexpected_dialog = true;
    save_script_from_editor(&root, "acme", "Web", sneaky).unwrap();
    let kept = load_script(&root, 7).unwrap().unwrap();
    assert_eq!(kept.page_errors, None);
    assert!(!kept.fail_on_unexpected_dialog);
}

#[test]
fn an_editor_save_on_a_script_with_no_setup_stays_without_one() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    // Nothing stored yet, then a stored script that has none: a setup the
    // editor sends is ignored both times.
    save_script_from_editor(&root, "acme", "Web", with_setup(Some("sneaked-in"))).unwrap();
    assert_eq!(stored_setup(&root), None);
    save_script_from_editor(&root, "acme", "Web", with_setup(Some("sneaked-in"))).unwrap();
    assert_eq!(stored_setup(&root), None);
}

// ---- Imports get the same seen check as a save ---------------------------

/// The test cases' own text by id, as the import fetches it.
fn case_texts(cases: &[(i32, &[&str])]) -> CaseTexts {
    cases.iter().map(|(id, text)| (*id, text.iter().map(|t| t.to_string()).collect())).collect()
}

fn button(name: &str) -> serde_json::Value {
    serde_json::json!({ "role": "button", "name": name })
}

/// `root`'s map holds a Save button on the ratings page, and the page itself.
fn seen_ratings(root: &std::path::Path) {
    use v2_lib::autorun::discovery_map::record_matched;
    let save: v2_lib::browser::locator::Target = serde_json::from_value(button("Save")).unwrap();
    record_matched(root, "acme", "Web", None, "/ratings", &save, 1).unwrap();
}

fn ratings_script(case_id: i32, steps: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "case_id": case_id, "title": "t", "steps": steps })
}

#[test]
fn an_import_with_an_unseen_locator_is_refused_listing_every_one_and_writes_nothing() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    seen_ratings(&root);
    let bundle = serde_json::json!([
        ratings_script(7, serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": button("Save") }] }])),
        ratings_script(8, serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": button("Publish") }] },
            { "step_number": 2, "actions": [{ "kind": "navigate", "url": "/elsewhere" }] }
        ])),
        ratings_script(9, serde_json::json!([{ "step_number": 3, "actions": [{ "kind": "click", "selector": button("Archive") }] }])),
        ratings_script(10, serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": button("Save") }] }]))
    ]);
    let file = dir.path().join("bundle.json");
    std::fs::write(&file, bundle.to_string()).unwrap();
    let cases = case_texts(&[(7, &[]), (8, &[]), (9, &[])]);
    let err = import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap(), Some(&cases)).unwrap_err();
    assert_eq!(
        err,
        [
            "Case 8, step 1: button \"Publish\" was never seen on the live app.",
            "Case 8, step 2: /elsewhere was never seen on the live app.",
            "Case 9, step 3: button \"Archive\" was never seen on the live app.",
            "Case 10: Azure DevOps has no test case with this id.",
            IMPORT_UNSEEN_THEN,
        ]
        .join("\n")
    );
    for id in [7, 8, 9, 10] {
        assert!(load_script(&root, id).unwrap().is_none(), "case {id} was written");
    }
}

#[test]
fn an_import_of_seen_scripts_is_written() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    seen_ratings(&root);
    v2_lib::autorun::discovery_map::record_seen(&root, "acme", "Web", None, "/ratings/123/edit", "", &[], None, None, 1)
        .unwrap();
    let bundle = serde_json::json!([ratings_script(7, serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "/ratings/123/edit" }] },
        { "step_number": 2, "actions": [
            { "kind": "click", "selector": button("Save") },
            // Not on the map, but the case says it: a check may look for it.
            { "kind": "expect_visible", "selector": { "text": "Rating saved" } }
        ] }
    ]))]);
    let file = dir.path().join("bundle.json");
    std::fs::write(&file, bundle.to_string()).unwrap();
    let cases = case_texts(&[(7, &["Press Save", "Rating saved appears"])]);
    let ids = import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap(), Some(&cases)).unwrap();
    assert_eq!(ids, vec![7]);
    assert!(load_script(&root, 7).unwrap().is_some());
}

#[test]
fn an_import_without_its_test_cases_is_refused() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    seen_ratings(&root);
    let bundle = serde_json::json!([ratings_script(7, serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": button("Save") }] }
    ]))]);
    let file = dir.path().join("bundle.json");
    std::fs::write(&file, bundle.to_string()).unwrap();
    let err = import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap(), None).unwrap_err();
    assert_eq!(err, IMPORT_NEEDS_CASES);
    assert_eq!(IMPORT_NEEDS_CASES, "Imported scripts are checked against the live app and their test cases; sign in and try again.");
    assert!(load_script(&root, 7).unwrap().is_none());

    // A map nothing can read refuses the import as it refuses a save.
    let map = v2_lib::autorun::discovery_map::map_path(&root, "acme", "Web");
    std::fs::write(&map, "{ not a map").unwrap();
    let cases = case_texts(&[(7, &[])]);
    let err = import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap(), Some(&cases)).unwrap_err();
    assert_eq!(err, v2_lib::autorun::discovery_map::load_map(&root, "acme", "Web").unwrap_err());
    assert!(load_script(&root, 7).unwrap().is_none());
}
