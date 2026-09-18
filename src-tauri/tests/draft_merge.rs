//! Rewriting a draft PATCHES it: cases the importer skipped stay where they
//! were, keys the app does not model survive, and every field keeps the
//! spelling the author used (review: rust-data #1).

use serde_json::{json, Value};
use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::import_parser::{apply_draft_edits, merge_cases_into_draft, parse_json_text};
use v2_lib::model::{DraftEdit, TestCase};

fn edit(before: TestCase, after: Option<TestCase>) -> DraftEdit {
    DraftEdit { before, after }
}

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir()
            .join(format!("tcm-draft-merge-{nanos}-{}", N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A draft in `<repo>/.test-cases`, where in-place writes are allowed (A9).
fn repo_draft(dir: &TempDir, name: &str, doc: &Value) -> (BridgeContext, std::path::PathBuf) {
    let folder = dir.0.join(".test-cases");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(doc).unwrap()).unwrap();
    let ctx = BridgeContext {
        working_dir: Some(dir.0.to_string_lossy().to_string()),
        ..Default::default()
    };
    (ctx, path)
}

fn on_disk(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn titles(doc: &Value) -> Vec<String> {
    doc["test_cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c.get("title").or_else(|| c.get("name")).and_then(Value::as_str).unwrap_or("").to_string()
        })
        .collect()
}

async fn transform_in_place(ctx: &BridgeContext, path: &std::path::Path, ops: Value) {
    let body = json!({ "path": path.to_string_lossy(), "in_place": true, "operations": ops })
        .to_string();
    let (status, out) = route(ctx, None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
}

/// The draft from the finding: two cases are still being written, and the
/// importer skips both.
fn half_written() -> Value {
    json!({
        "specs": ["Spec.md"],
        "comments": "whole-set note",
        "test_cases": [
            { "title": "First", "steps": [{ "action": "Open.", "expected": "Opens." }] },
            { "title": "Still being written", "steps": [] },
            { "title": "Blank step", "steps": [{ "action": "", "expected": "Something." }] },
            { "title": "Last", "steps": [{ "action": "Close.", "expected": "Closes." }] }
        ]
    })
}

#[test]
fn a_parsed_case_knows_which_entry_it_came_from() {
    let parsed = parse_json_text(&half_written().to_string()).unwrap();
    let sources: Vec<Option<usize>> = parsed.cases.iter().map(|c| c.source.0).collect();
    assert_eq!(sources, vec![Some(0), Some(3)]);
}

#[tokio::test]
async fn an_in_place_transform_keeps_the_cases_the_importer_skipped() {
    let dir = TempDir::new();
    let (ctx, path) = repo_draft(&dir, "draft.json", &half_written());
    transform_in_place(&ctx, &path, json!([{ "op": "add_tags", "value": "smoke" }])).await;

    let doc = on_disk(&path);
    assert_eq!(titles(&doc), ["First", "Still being written", "Blank step", "Last"], "{doc}");
    let cases = doc["test_cases"].as_array().unwrap();
    assert_eq!(cases[0]["tags"], "smoke");
    assert_eq!(cases[3]["tags"], "smoke");
    assert_eq!(cases[1], half_written()["test_cases"][1], "a skipped case is kept verbatim");
    assert_eq!(cases[2], half_written()["test_cases"][2]);
    assert_eq!(doc["specs"], json!(["Spec.md"]));
    assert_eq!(doc["comments"], "whole-set note");
}

#[tokio::test]
async fn a_transform_keeps_unknown_keys_and_the_authors_spellings() {
    let dir = TempDir::new();
    let draft = json!({ "test_cases": [{
        "name": "Sign in",
        "author": "avin",
        "notes": "check on Friday",
        "prerequisites": "Signed out",
        "tags": ["auth", "smoke"],
        "steps": [
            { "step": "Open the login page.", "expected_result": "It loads.", "screenshot": "a.png" },
            { "step": "Sign in.", "expected_result": "The home page opens." }
        ]
    }]});
    let (ctx, path) = repo_draft(&dir, "aliases.json", &draft);
    transform_in_place(
        &ctx,
        &path,
        json!([
            { "op": "replace_in_steps", "find": "Sign in.", "replace": "Sign in with SSO." },
            { "op": "add_tags", "value": "sso" }
        ]),
    )
    .await;

    let doc = on_disk(&path);
    let case = &doc["test_cases"][0];
    assert_eq!(case["name"], "Sign in");
    assert!(case.get("title").is_none(), "the title keeps its spelling: {case}");
    assert_eq!(case["author"], "avin");
    assert_eq!(case["notes"], "check on Friday");
    assert!(case.get("comment").is_none(), "{case}");
    assert_eq!(case["prerequisites"], "Signed out");
    assert!(case.get("preconditions").is_none(), "{case}");
    assert_eq!(case["tags"], json!(["auth", "smoke", "sso"]), "a list of tags stays a list");
    assert_eq!(case["steps"][0], draft["test_cases"][0]["steps"][0], "an untouched step is verbatim");
    assert_eq!(case["steps"][1]["step"], "Sign in with SSO.");
    assert_eq!(case["steps"][1]["expected_result"], "The home page opens.");
    assert!(case["steps"][1].get("action").is_none(), "{case}");
}

#[tokio::test]
async fn an_id_the_importer_refused_is_still_in_the_file_afterwards() {
    let dir = TempDir::new();
    let draft = json!({ "test_cases": [
        { "id": "abc", "title": "Bad id", "steps": [{ "action": "Go.", "expected": "" }] },
        { "id": 0, "title": "Zero id", "steps": [{ "action": "Go.", "expected": "" }] }
    ]});
    let (ctx, path) = repo_draft(&dir, "ids.json", &draft);
    transform_in_place(&ctx, &path, json!([{ "op": "add_tags", "value": "smoke" }])).await;
    let doc = on_disk(&path);
    assert_eq!(doc["test_cases"][0]["id"], "abc", "{doc}");
    assert_eq!(doc["test_cases"][1]["id"], 0, "{doc}");
}

#[tokio::test]
async fn an_in_place_optimize_keeps_the_cases_the_importer_skipped() {
    let dir = TempDir::new();
    let (ctx, path) = repo_draft(&dir, "opt.json", &half_written());
    let target = format!(
        "/optimize?in_place=true&path={}",
        path.to_string_lossy().replace('\\', "%5C")
    );
    let (status, out) = route(&ctx, None, "POST", &target, "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let doc = on_disk(&path);
    let t = titles(&doc);
    assert_eq!(t.len(), 4, "{doc}");
    assert!(t.contains(&"Still being written".to_string()), "{doc}");
    assert!(t.contains(&"Blank step".to_string()), "{doc}");
    assert_eq!(doc["test_cases"][1], half_written()["test_cases"][1]);
}

#[tokio::test]
async fn remove_and_insert_in_place_touch_only_their_own_cases() {
    let dir = TempDir::new();
    let (ctx, path) = repo_draft(&dir, "edit.json", &half_written());
    transform_in_place(
        &ctx,
        &path,
        json!([
            { "op": "remove_cases", "where": { "title_contains": "Last" } },
            { "op": "insert_cases", "at_index": 1,
              "cases": [{ "title": "Inserted", "steps": [{ "action": "Do.", "expected": "Done." }] }] }
        ]),
    )
    .await;
    // The skipped entries stay right after "First", the entry they followed.
    assert_eq!(
        titles(&on_disk(&path)),
        ["First", "Still being written", "Blank step", "Inserted"]
    );
}

/// save_draft_cases: the queue renamed one of the file's cases and removed
/// another. The file keeps the case it skipped, the case an assistant added
/// after the last sync, and every key it had.
#[test]
fn a_bulk_edit_patches_its_cases_and_leaves_the_rest_of_the_file_alone() {
    let old = json!({ "comments": "note", "test_cases": [
        { "title": "Rename me", "author": "avin", "steps": [{ "action": "A.", "expected": "" }] },
        { "title": "Remove me", "steps": [{ "action": "B.", "expected": "" }] },
        { "title": "Half written", "steps": [] },
        { "title": "Added after the last sync", "steps": [{ "action": "C.", "expected": "" }] }
    ]})
    .to_string();
    let parsed = parse_json_text(&old).unwrap().cases;
    // What the queue holds has come through IPC, so it has no source.
    let row = |i: usize| TestCase { source: Default::default(), ..parsed[i].clone() };
    let renamed = TestCase { title: "Renamed".into(), ..row(0) };

    let out = apply_draft_edits(&old, &[edit(row(0), Some(renamed)), edit(row(1), None)]).unwrap();
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(titles(&doc), ["Renamed", "Half written", "Added after the last sync"], "{out}");
    assert_eq!(doc["test_cases"][0]["author"], "avin");
    assert_eq!(doc["comments"], "note");
}

/// The upload write-back: the created id lands on the right one of two
/// same-titled drafts, and nothing else about the entry changes.
#[test]
fn an_upload_stamps_the_new_id_onto_the_case_it_came_from() {
    let old = json!({ "test_cases": [
        { "title": "Same title", "steps": [{ "step": "One.", "expected": "" }] },
        { "title": "Same title", "steps": [{ "step": "Two.", "expected": "" }] }
    ]})
    .to_string();
    let parsed = parse_json_text(&old).unwrap().cases;
    let row = |i: usize| TestCase { source: Default::default(), ..parsed[i].clone() };
    let created = TestCase { update_id: Some(501), ..row(1) };

    let out =
        apply_draft_edits(&old, &[edit(row(0), Some(row(0))), edit(row(1), Some(created))]).unwrap();
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert!(doc["test_cases"][0].get("id").is_none(), "{out}");
    assert_eq!(doc["test_cases"][1]["id"], 501);
    assert_eq!(doc["test_cases"][1]["steps"][0]["step"], "Two.", "the step keeps its spelling");
}

/// The Nth "X" in the queue is the Nth "X" in the file. Removing the first
/// of two same-titled drafts removes the FIRST entry; the second keeps
/// every key it had.
#[test]
fn removing_the_first_of_two_same_titled_drafts_removes_the_first_entry() {
    let first = json!({ "title": "X", "author": "a", "steps": [{ "action": "A.", "expected": "" }] });
    let second =
        json!({ "title": "X", "steps": [{ "action": "B.", "expected": "", "screenshot": "b.png" }] });
    let old = json!({ "test_cases": [first, second.clone()] }).to_string();
    let parsed = parse_json_text(&old).unwrap().cases;
    let row = |i: usize| TestCase { source: Default::default(), ..parsed[i].clone() };

    // Queue order: Q1 (= the first entry) removed, Q2 kept as it was.
    let out = apply_draft_edits(&old, &[edit(row(0), None), edit(row(1), Some(row(1)))]).unwrap();
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["test_cases"], json!([second]), "{out}");
}

#[test]
fn a_bare_array_keeps_its_skipped_entries_in_the_wrapper() {
    let old = r#"[{"title":"Draft","steps":[]},{"title":"Real","steps":[{"action":"Go."}]}]"#;
    let parsed = parse_json_text(old).unwrap();
    let out = merge_cases_into_draft(old, &parsed.cases).unwrap();
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["format"], "azure-devops-test-cases");
    assert_eq!(doc["test_cases"][0], json!({ "title": "Draft", "steps": [] }));
    assert_eq!(
        doc["test_cases"][1],
        json!({ "title": "Real", "steps": [{ "action": "Go." }] }),
        "an unchanged case is written back exactly as it was"
    );
}
