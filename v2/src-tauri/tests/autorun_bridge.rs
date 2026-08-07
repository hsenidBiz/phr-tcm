//! The two bridge routes an AI assistant reaches through the MCP proxy:
//! read the action-script format, and save scripts back.
//!
//! Neither touches Azure DevOps, so neither needs a signed-in client -
//! the guide documents a format, and the scripts are local files.

use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::autorun::store::{load_script, set_root};

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
        let dir = std::env::temp_dir().join(format!("tcm-autorun-bridge-{nanos}-{n}"));
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

fn ctx() -> BridgeContext {
    BridgeContext::default()
}

/// The store root is process-wide state, and cargo runs tests in
/// parallel - every test that sets it holds this lock so one test's root
/// cannot swap out from under another mid-save.
static ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The guide is format documentation, not org data - it must answer
/// before anyone signs in, or an assistant cannot even learn the shape.
#[tokio::test]
async fn the_guide_answers_without_a_signed_in_client() {
    let (status, body) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert_eq!(status, 200);
    assert!(body.contains("check_text"), "not the action guide: {body}");
}

/// One file, many cases - the assistant writes a whole PBI's worth in a
/// single call rather than one round trip per case.
#[tokio::test]
async fn a_bundle_saves_every_case_it_carries() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());

    let body = serde_json::json!([
        {
            "case_id": 201,
            "title": "Valid login",
            "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "Dashboard" }] }]
        },
        {
            "case_id": 202,
            "title": "Locked account",
            "steps": [{ "step_number": 1, "actions": [{ "kind": "check_url", "contains": "/locked" }] }]
        }
    ])
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("201") && out.contains("202"), "reply names neither case: {out}");

    let a = load_script(dir.path(), 201).unwrap().unwrap();
    assert_eq!(a.title, "Valid login");
    assert_eq!(a.steps.len(), 1);
    let b = load_script(dir.path(), 202).unwrap().unwrap();
    assert_eq!(b.title, "Locked account");
}

/// A single script is the same call with one entry - the assistant does
/// not need a second tool for the one-case case.
#[tokio::test]
async fn a_single_script_is_just_a_bundle_of_one() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    let body = serde_json::json!([{
        "case_id": 7,
        "title": "Only one",
        "steps": [{ "step_number": 1, "actions": [] }]
    }])
    .to_string();

    let (status, _) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200);
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().title, "Only one");
}

/// An unknown action kind must be refused at the door. Saving it would
/// hand the tester a script that dies mid-run, in front of them, with
/// the browser already open.
#[tokio::test]
async fn an_unknown_action_kind_is_refused_and_nothing_is_written() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    let body = serde_json::json!([{
        "case_id": 9,
        "title": "Bad",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "teleport", "to": "the moon" }] }]
    }])
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(load_script(dir.path(), 9).unwrap().is_none(), "a bad script was written anyway");
}

/// Malformed JSON gets the parser's own complaint, not a shrug.
#[tokio::test]
async fn malformed_json_is_a_400_that_says_why() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", "{ not json", "1.0.0").await;
    assert_eq!(status, 400);
    assert!(!out.is_empty(), "a 400 with no explanation");
}

/// This is a PARSE-gate test, not an atomicity test: `"kind": "nope"` is
/// an action tag serde does not know, so `serde_json::from_str::<Vec
/// <CaseScript>>` fails on the whole body before the write loop ever
/// starts - the good case was never going to be written regardless of
/// whether that loop is atomic. It would pass even if
/// `store::save_scripts_atomically` wrote every entry it reached with no
/// rollback at all. See `a_write_failure_mid_bundle_leaves_nothing_behind`
/// below for a test that actually exercises atomicity.
#[tokio::test]
async fn a_bundle_that_fails_to_parse_writes_nothing() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    let body = serde_json::json!([
        { "case_id": 11, "title": "Fine", "steps": [{ "step_number": 1, "actions": [] }] },
        { "case_id": 12, "title": "Broken", "steps": [{ "step_number": 1, "actions": [{ "kind": "nope" }] }] }
    ])
    .to_string();

    let (status, _) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400);
    assert!(
        load_script(dir.path(), 11).unwrap().is_none(),
        "the good case was written even though the bundle failed to parse"
    );
}

/// The real atomicity claim: both cases here parse and validate fine, so
/// the write loop is reached for both - but case 12's target path is
/// already a directory, so its write can never succeed. If the loop wrote
/// case 11 before discovering that, this would be the "15 of 30 written"
/// bug the claim was supposed to rule out.
#[tokio::test]
async fn a_write_failure_mid_bundle_leaves_nothing_behind() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());

    // Occupy case 12's target filename with a directory before the save
    // is even attempted, so its write is doomed from the start.
    let scripts_dir = dir.path().join("scripts");
    std::fs::create_dir_all(&scripts_dir).unwrap();
    std::fs::create_dir_all(scripts_dir.join("case-12.json")).unwrap();

    let body = serde_json::json!([
        { "case_id": 11, "title": "Fine", "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }] },
        { "case_id": 12, "title": "Blocked", "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }] }
    ])
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 500, "{out}");
    assert!(
        load_script(dir.path(), 11).unwrap().is_none(),
        "case 11 was written even though case 12 in the same bundle could not be"
    );
}
