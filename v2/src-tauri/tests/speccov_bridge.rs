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

/// The `path` branch - a draft too large to inline gets written to a local
/// file and scored the same way as `json`, mirroring the happy path above.
#[tokio::test]
async fn coverage_scores_a_draft_given_by_path() {
    let dir = TempDir::new();
    let (spec_path, draft) = spec_and_draft(dir.path());
    let draft_path = dir.path().join("draft.json");
    std::fs::write(&draft_path, &draft).unwrap();

    let body = serde_json::json!({
        "spec_paths": [spec_path.to_string_lossy()],
        "path": draft_path.to_string_lossy(),
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/check-coverage", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let uncovered: Vec<&str> = v["uncovered"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert_eq!(uncovered, vec!["7.4"], "the gap did not surface from a path-given draft: {out}");
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

// ---------------------------------------------------------------------
// `/merge-cases` - the other half of a fan-out: `check_spec_coverage`
// finds gaps in a single draft, this is how a spec too large for one
// writer gets back to being a single draft at all.
// ---------------------------------------------------------------------

fn case_json(title: &str) -> serde_json::Value {
    serde_json::json!({
        "title": title,
        "steps": [{ "action": "Open the list", "expected": "It loads" }],
    })
}

/// Two real slice files, merged in the order given, must land as one
/// draft with both the total and the per-file breakdown right - the
/// whole point of a merge is that the pieces can be trusted to add up.
#[tokio::test]
async fn merge_preserves_order_and_reports_per_file_counts() {
    let dir = TempDir::new();
    let slice_a = dir.path().join("slice-a.json");
    let slice_b = dir.path().join("slice-b.json");
    std::fs::write(
        &slice_a,
        serde_json::json!([case_json("Case A1"), case_json("Case A2")]).to_string(),
    )
    .unwrap();
    std::fs::write(&slice_b, serde_json::json!([case_json("Case B1")]).to_string()).unwrap();
    let output_path = dir.path().join("merged.json");

    let body = serde_json::json!({
        "paths": [slice_a.to_string_lossy(), slice_b.to_string_lossy()],
        "output_path": output_path.to_string_lossy(),
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/merge-cases", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["cases"], 3, "{out}");
    let per_file = v["per_file"].as_array().unwrap();
    assert_eq!(per_file.len(), 2, "{out}");
    assert_eq!(per_file[0]["cases"], 2, "{out}");
    assert_eq!(per_file[1]["cases"], 1, "{out}");

    // Slice order is preserved on disk too - A's two cases before B's one.
    let written = std::fs::read_to_string(&output_path).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&written).unwrap();
    let titles: Vec<&str> = doc["test_cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["Case A1", "Case A2", "Case B1"], "{written}");
}

/// A slice that cannot be read fails the WHOLE merge, naming the path -
/// and nothing gets written, because a merge silently missing one slice
/// is worse than no merge at all.
#[tokio::test]
async fn an_unreadable_slice_fails_the_whole_merge_and_writes_nothing() {
    let dir = TempDir::new();
    let slice_a = dir.path().join("slice-a.json");
    std::fs::write(&slice_a, serde_json::json!([case_json("Case A1")]).to_string()).unwrap();
    let missing = dir.path().join("does-not-exist-slice.json");
    let output_path = dir.path().join("merged.json");

    let body = serde_json::json!({
        "paths": [slice_a.to_string_lossy(), missing.to_string_lossy()],
        "output_path": output_path.to_string_lossy(),
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/merge-cases", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(
        out.contains("does-not-exist-slice.json"),
        "error does not name the unreadable slice: {out}"
    );
    assert!(!output_path.exists(), "a failed merge must write nothing: {out}");
}

/// An `output_path` that already exists is refused, not silently
/// overwritten - the caller picks a new name rather than this route
/// guessing whether the existing file was meant to survive.
#[tokio::test]
async fn an_existing_output_path_is_refused_rather_than_overwritten() {
    let dir = TempDir::new();
    let slice_a = dir.path().join("slice-a.json");
    std::fs::write(&slice_a, serde_json::json!([case_json("Case A1")]).to_string()).unwrap();
    let output_path = dir.path().join("already-here.json");
    std::fs::write(&output_path, "not a merge result").unwrap();

    let body = serde_json::json!({
        "paths": [slice_a.to_string_lossy()],
        "output_path": output_path.to_string_lossy(),
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/merge-cases", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(
        out.contains("already-here.json"),
        "error does not name the existing output path: {out}"
    );
    assert_eq!(
        std::fs::read_to_string(&output_path).unwrap(),
        "not a merge result",
        "refused, not overwritten"
    );
}

/// Warnings from every slice are aggregated, each still carrying the name
/// of the slice it came from - lose that and a warning three slices deep
/// in a fan-out cannot be traced back to the file that needs fixing.
#[tokio::test]
async fn aggregated_warnings_carry_the_slice_file_name() {
    let dir = TempDir::new();
    let slice_a = dir.path().join("slice-a.json");
    let slice_b = dir.path().join("slice-b.json");
    std::fs::write(&slice_a, serde_json::json!([case_json("Case A1")]).to_string()).unwrap();
    // id 0 is not a valid work item id - the importer keeps the case but
    // warns, which is exactly the kind of per-slice warning that must not
    // get lost once several slices are concatenated together.
    std::fs::write(
        &slice_b,
        serde_json::json!([{
            "title": "Case B1",
            "id": 0,
            "steps": [{ "action": "Open", "expected": "Opens" }],
        }])
        .to_string(),
    )
    .unwrap();
    let output_path = dir.path().join("merged.json");

    let body = serde_json::json!({
        "paths": [slice_a.to_string_lossy(), slice_b.to_string_lossy()],
        "output_path": output_path.to_string_lossy(),
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/merge-cases", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let warnings: Vec<&str> = v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("slice-b.json") && w.contains("not a valid work item")),
        "warning does not carry its slice's file name: {warnings:?}"
    );
}

/// The atomic guarantee: if the final write cannot land (here, because the
/// parent directory does not exist), `output_path` is left exactly as it
/// was before the call - never a half-written file, and no stray temp
/// file either.
#[tokio::test]
async fn a_failed_write_leaves_no_file_at_output_path() {
    let dir = TempDir::new();
    let slice_a = dir.path().join("slice-a.json");
    std::fs::write(&slice_a, serde_json::json!([case_json("Case A1")]).to_string()).unwrap();
    let output_path = dir.path().join("no-such-subdir").join("merged.json");

    let body = serde_json::json!({
        "paths": [slice_a.to_string_lossy()],
        "output_path": output_path.to_string_lossy(),
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/merge-cases", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(!output_path.exists(), "a failed write must leave nothing at output_path: {out}");
    let tmp_path = dir.path().join("no-such-subdir").join("merged.json.tmp");
    assert!(!tmp_path.exists(), "no stray temp file should remain either: {out}");
}

/// Round 6 §4: a fan-out makes title collisions likely because no slice
/// writer sees another's output. The merge names them - dedupe would
/// silently delete a real case, and a human has to disambiguate instead.
#[tokio::test]
async fn merge_warns_when_a_title_appears_in_more_than_one_slice() {
    let dir = TempDir::new();
    let slice_a = dir.path().join("ga01.json");
    let slice_b = dir.path().join("ga04.json");
    std::fs::write(
        &slice_a,
        serde_json::json!([case_json("No Notification When Cycle Inactive"), case_json("Only A")])
            .to_string(),
    )
    .unwrap();
    std::fs::write(
        &slice_b,
        serde_json::json!([case_json("No Notification When Cycle Inactive")]).to_string(),
    )
    .unwrap();
    let output_path = dir.path().join("merged.json");

    let body = serde_json::json!({
        "paths": [slice_a.to_string_lossy(), slice_b.to_string_lossy()],
        "output_path": output_path.to_string_lossy(),
    })
    .to_string();

    let (status, out) = route(&ctx(), None, "POST", "/merge-cases", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();

    // Both copies are KEPT - the merge reports, it does not decide.
    assert_eq!(v["cases"], 3, "{out}");
    let warnings = v["warnings"].as_array().unwrap();
    let dup = warnings
        .iter()
        .filter_map(|w| w.as_str())
        .find(|w| w.contains("duplicate title"))
        .expect("a duplicate-title warning");
    assert!(dup.contains("no notification when cycle inactive"), "{dup}");
    assert!(dup.contains("ga01.json") && dup.contains("ga04.json"), "{dup}");
    // The unique title stays unmentioned.
    assert!(!warnings.iter().any(|w| w.as_str().unwrap().contains("Only A")), "{out}");
}
