//! The `/check-coverage` bridge route: joins a draft's `Spec:` citations
//! against one or more real spec files on disk. No Azure DevOps involved,
//! so it answers without a signed-in client - and it must, since a
//! developer wants this before they've even opened the app's config
//! screen.

use v2_lib::ai_bridge::{route, BridgeContext};

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
        let dir = std::env::temp_dir().join(format!("tcm-speccov-bridge-{nanos}-{n}"));
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

/// A real markdown spec file with two sections, and a draft (sent inline,
/// not via `path`) whose one case cites only the first. The second section
/// must come back in `uncovered` - the whole point of the tool is that a
/// gap in a real document is found, not just in a hand-built fixture.
fn spec_and_draft(dir: &std::path::Path) -> (std::path::PathBuf, String) {
    let spec_path = dir.join("spec.md");
    std::fs::write(
        &spec_path,
        "## 7.1 List loads\n\nThe list loads on page open.\n\n## 7.4 Export button\n\nAn export button is offered.\n",
    )
    .unwrap();

    let draft = serde_json::json!([
        {
            "title": "List loads correctly",
            "reviewer_notes": "Checks the list loads.\nSpec: spec.md 7.1",
            "steps": [{ "action": "Open the list", "expected": "It loads" }],
        }
    ])
    .to_string();

    (spec_path, draft)
}

#[tokio::test]
async fn coverage_answers_without_a_signed_in_client() {
    let dir = TempDir::new();
    let (spec_path, draft) = spec_and_draft(dir.path());

    let body = serde_json::json!({
        "spec_paths": [spec_path.to_string_lossy()],
        "json": draft,
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/check-coverage", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("\"uncovered\""), "no uncovered key: {out}");
    // 7.1 was cited, so only 7.4 is a gap.
    assert!(out.contains("7.4"), "the real gap in the document did not surface: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let uncovered: Vec<&str> = v["uncovered"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert_eq!(uncovered, vec!["7.4"], "{out}");
    assert!(v["covered"]["7.1"].as_array().unwrap().len() == 1, "{out}");
}

#[tokio::test]
async fn a_missing_spec_file_is_a_400_naming_the_path() {
    let dir = TempDir::new();
    let missing = dir.path().join("does-not-exist.md");

    let body = serde_json::json!({
        "spec_paths": [missing.to_string_lossy()],
        "json": "[]",
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/check-coverage", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    // JSON-escapes the path's backslashes, so compare the file name rather
    // than the raw OS path string.
    assert!(
        out.contains("does-not-exist.md"),
        "error does not name the missing path: {out}"
    );
}

#[tokio::test]
async fn path_and_json_together_is_a_400() {
    let dir = TempDir::new();
    let (spec_path, draft) = spec_and_draft(dir.path());
    let draft_path = dir.path().join("draft.json");
    std::fs::write(&draft_path, &draft).unwrap();

    let body = serde_json::json!({
        "spec_paths": [spec_path.to_string_lossy()],
        "path": draft_path.to_string_lossy(),
        "json": draft,
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/check-coverage", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.to_lowercase().contains("path") && out.to_lowercase().contains("json"), "{out}");
}

/// Round-5 item 10's other half: naming NEITHER source is refused the same
/// way as naming both. An empty body must never read as "score an empty
/// draft" - that would silently report every section as covered by
/// nothing being wrong, when actually nothing was ever sent to check.
#[tokio::test]
async fn neither_path_nor_json_is_a_400() {
    let dir = TempDir::new();
    let (spec_path, _draft) = spec_and_draft(dir.path());

    let body = serde_json::json!({ "spec_paths": [spec_path.to_string_lossy()] }).to_string();

    let (status, out) = route(&ctx(), None, "POST", "/check-coverage", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
}

/// The findings register carries exactly seven keys, and `warnings` is
/// never one of them - see `speccov::check_coverage`'s doc comment. A
/// `warnings` key here would suggest something to fix, when what this
/// route reports is coverage state to read and account for.
#[tokio::test]
async fn the_report_carries_no_warnings_key() {
    let dir = TempDir::new();
    let (spec_path, draft) = spec_and_draft(dir.path());

    let body = serde_json::json!({
        "spec_paths": [spec_path.to_string_lossy()],
        "json": draft,
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/check-coverage", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(!out.contains("\"warnings\""), "findings register must never carry a warnings key: {out}");
}
