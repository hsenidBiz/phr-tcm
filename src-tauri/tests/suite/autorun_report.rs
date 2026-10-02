//! The Auto Run report: one run as a single HTML page. Everything from the
//! run is escaped, the page carries no script, a failed case shows the step
//! that stopped it with its picture, a missing picture is a note rather than
//! a failure, and nothing outside the shots folder is ever read into it.

use base64::Engine;
use v2_lib::autorun::report::{bucket, build, counts, BUCKETS};
use v2_lib::autorun::store::{load_shot, save_run, save_script};
use v2_lib::autorun::{CaseScript, LocalRun};
use v2_lib::commands::autorun::{export_report_at, REPORT_NOT_A_FULL_PATH, REPORT_RUN_GONE};

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
        let dir = std::env::temp_dir().join(format!("tcm-autorun-report-{nanos}-{n}"));
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

/// A JPEG's first bytes and a little more - enough to be recognised.
const JPEG: [u8; 8] = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];
const SHOT: &str = "shot-1786000200000-000001.jpg";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Case 201 failed at step 2 action 1 (a click), with a picture; 202 was
/// proposed Passed and confirmed Blocked by the person; 203 has nothing
/// proposed; 204 passed.
fn run_of(shot: &str) -> LocalRun {
    serde_json::from_value(serde_json::json!({
        "id": "run-1786000200000",
        "pbi_id": 42,
        "started_at": "1786000200000",
        "mode": "unattended",
        "environment": "QA",
        "cases": [
            {
                "case_id": 201, "title": "Valid login", "verdict": "", "note": "",
                "proposed": "Failed", "reason": "step 2: button \"Save\" not found",
                "account": "tester1",
                "steps": [
                    { "step_number": 0, "outcomes": [{ "ok": true, "detail": "signed in as tester1" }] },
                    { "step_number": 1, "outcomes": [{ "ok": true, "detail": "filled textbox \"Email\"" }] },
                    { "step_number": 2, "outcomes": [
                        { "ok": false, "detail": "button \"Save\" not found", "screenshot": shot },
                        { "ok": false, "detail": "not run: an earlier action of this step failed" }
                    ] }
                ]
            },
            { "case_id": 202, "title": "Locked account", "verdict": "Blocked", "note": "No test user", "proposed": "Passed", "steps": [] },
            { "case_id": 203, "title": "Password reset", "verdict": "", "note": "", "steps": [] },
            { "case_id": 204, "title": "Logout", "verdict": "Passed", "note": "", "steps": [] }
        ]
    }))
    .unwrap()
}

fn script_201() -> CaseScript {
    serde_json::from_value(serde_json::json!({
        "case_id": 201,
        "title": "Valid login",
        "area": "Login page",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "fill", "selector": "#email", "value": "hunter2-typed-secret" }] },
            { "step_number": 2, "actions": [
                { "kind": "click", "selector": "text=Save" },
                { "kind": "check_text", "value": "Saved" }
            ] }
        ]
    }))
    .unwrap()
}

fn no_shots(_: &str) -> Result<Vec<u8>, String> {
    Err("gone".to_string())
}

#[test]
fn the_bucket_rule_matches_the_table_the_screen_is_held_to() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/verdict_buckets.json"),
    )
    .unwrap();
    let table: serde_json::Value = serde_json::from_str(&text).unwrap();
    let rows = table["cases"].as_array().unwrap();
    assert!(rows.len() > 10);
    for row in rows {
        let (v, p, want) = (
            row["verdict"].as_str().unwrap(),
            row["proposed"].as_str().unwrap(),
            row["bucket"].as_str().unwrap(),
        );
        assert_eq!(bucket(v, p), want, "verdict {v:?}, proposed {p:?}");
        assert!(BUCKETS.contains(&want));
    }
}

#[test]
fn counts_follow_the_confirmed_verdict_over_the_proposal() {
    let run = run_of(SHOT);
    // Passed, Failed, Blocked, Not run - 202's confirmed Blocked beats its
    // Passed proposal.
    assert_eq!(counts(&run), [1, 1, 1, 1]);
    let html = build(&run, &[], "2 Oct 2026, 09:30", &no_shots);
    assert!(html.contains("<td class=\"b-passed\">1</td><td class=\"b-failed\">1</td><td class=\"b-blocked\">1</td><td class=\"b-notrun\">1</td><td>4</td>"));
    // The case table marks how each result was decided.
    assert!(html.contains("<td class=\"b-blocked\">Blocked</td><td>Confirmed</td>"));
    assert!(html.contains("<td class=\"b-failed\">Failed</td><td>Proposed</td>"));
}

#[test]
fn every_value_is_escaped_and_the_page_carries_no_script() {
    let mut run = run_of(SHOT);
    run.cases[0].title = "<script>alert('x')</script> & \"quotes\"".to_string();
    run.cases[1].note = "<img src=x onerror=alert(1)>".to_string();
    run.environment = Some("<b>QA</b>".to_string());
    let html = build(&run, &[], "<script>when</script>", &no_shots);

    assert!(!html.to_lowercase().contains("<script"), "the page must carry no script");
    assert!(html.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; &quot;quotes&quot;"));
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(html.contains("&lt;b&gt;QA&lt;/b&gt;"));
    assert!(!html.contains("<img src=x"));
}

#[test]
fn the_header_says_when_how_where_and_whether_it_was_sent() {
    let mut run = run_of(SHOT);
    let html = build(&run, &[], "2 Oct 2026, 09:30", &no_shots);
    assert!(html.contains("<h1>Auto Run report</h1>"));
    assert!(html.contains("<dt>Ran</dt><dd>2 Oct 2026, 09:30</dd>"));
    assert!(html.contains("<dt>Mode</dt><dd>Unattended</dd>"));
    assert!(html.contains("<dt>Environment</dt><dd>QA</dd>"));
    assert!(html.contains("<dt>PBI</dt><dd>#42</dd>"));
    assert!(html.contains("Not sent"));

    run.mode = String::new();
    run.environment = None;
    run.published = Some(serde_json::from_value(serde_json::json!({
        "run_id": 77, "web_url": "https://dev.azure.com/acme/Web/_testManagement/runs?runId=77", "at": "1786000300000"
    })).unwrap());
    // No local time from the webview: UTC, from the run's own start.
    let html = build(&run, &[], "", &no_shots);
    assert!(html.contains("<dt>Mode</dt><dd>Supervised</dd>"));
    assert!(!html.contains("<dt>Environment</dt>"));
    assert!(html.contains("Sent as test run #77 - <a href=\"https://dev.azure.com/acme/Web/_testManagement/runs?runId=77\">"));
    assert!(html.contains(" UTC</dd>"));
}

#[test]
fn a_published_address_that_is_not_web_is_text_not_a_link() {
    let mut run = run_of(SHOT);
    run.published = Some(serde_json::from_value(serde_json::json!({
        "run_id": 1, "web_url": "javascript:alert(1)", "at": "1"
    })).unwrap());
    let html = build(&run, &[], "x", &no_shots);
    assert!(!html.contains("href=\"javascript:"));
}

#[test]
fn a_failed_case_shows_the_step_that_stopped_it_and_embeds_its_picture() {
    let run = run_of(SHOT);
    let html = build(&run, &[script_201()], "x", &|name: &str| {
        assert_eq!(name, SHOT);
        Ok(JPEG.to_vec())
    });

    assert!(html.contains("<h2>Failed and blocked cases</h2>"));
    assert!(html.contains("<dt>Stopped at</dt><dd>Step 2</dd>"));
    assert!(html.contains("<dt>Action</dt><dd>click text=Save</dd>"));
    assert!(html.contains("<dt>Message</dt><dd>button &quot;Save&quot; not found</dd>"));
    assert!(html.contains(&format!("src=\"data:image/jpeg;base64,{}\"", b64(&JPEG))));
    // The Blocked case is listed too, decided by the person with no failed step.
    assert!(html.contains("No test user"));
    assert!(html.contains("decided by the person"));
    // The account key and the script's area are in the case table.
    assert!(html.contains("<td>tester1</td><td>Login page</td>"));
    // Passed and Not run cases get no failure block.
    assert!(!html.contains("<span class=\"id\">#204</span>"));
}

#[test]
fn a_typed_value_never_reaches_the_report() {
    let mut run = run_of(SHOT);
    // Make the fill itself the action that stopped the case.
    run.cases[0].steps[1].outcomes[0].ok = false;
    run.cases[0].steps[1].outcomes[0].detail = "textbox \"Email\" not found".to_string();
    let html = build(&run, &[script_201()], "x", &no_shots);
    assert!(html.contains("<dt>Action</dt><dd>fill in #email</dd>"));
    assert!(!html.contains("hunter2-typed-secret"));
}

#[test]
fn a_missing_picture_is_noted_not_fatal() {
    let run = run_of(SHOT);
    let html = build(&run, &[script_201()], "x", &no_shots);
    assert!(html.contains(&format!("The picture {SHOT} is no longer on this machine.")));
    assert!(html.contains("<dt>Stopped at</dt><dd>Step 2</dd>"));
    assert!(html.ends_with("</html>\n"));
}

#[test]
fn a_picture_too_large_is_noted_and_left_out() {
    let run = run_of(SHOT);
    let big = {
        let mut v = JPEG.to_vec();
        v.resize(v2_lib::autorun::report::MAX_SHOT_BYTES + 1, 0);
        v
    };
    let html = build(&run, &[], "x", &move |_: &str| Ok(big.clone()));
    assert!(html.contains("was too large to include"));
    assert!(!html.contains("data:image/jpeg"));
}

#[test]
fn a_shot_name_that_climbs_out_of_the_shots_folder_is_refused_unread() {
    let dir = TempDir::new();
    let secret = [0xFF, 0xD8, 9, 9, 9, 9, 9, 9];
    std::fs::write(dir.path().join("secret.jpg"), secret).unwrap();
    std::fs::create_dir_all(dir.path().join("shots")).unwrap();
    let run = run_of("../secret.jpg");

    let read = std::cell::Cell::new(false);
    let html = build(&run, &[], "x", &|name: &str| {
        read.set(true);
        load_shot(dir.path(), name)
    });
    assert!(!read.get(), "the loader must not even be asked for a name the guard refuses");
    assert!(html.contains("../secret.jpg is not a screenshot name"));
    assert!(!html.contains(&b64(&secret)));
}

#[test]
fn export_writes_the_page_through_the_shots_guard_and_names_the_file() {
    let dir = TempDir::new();
    let root = dir.path().join("autorun");
    save_run(&root, &run_of(SHOT)).unwrap();
    save_script(&root, &script_201()).unwrap();
    std::fs::create_dir_all(root.join("shots")).unwrap();
    std::fs::write(root.join("shots").join(SHOT), JPEG).unwrap();
    let out = dir.path().join("auto-run-2026-10-02-0930.html");

    let name = export_report_at(true, &root, "run-1786000200000", &out, "2 Oct 2026, 09:30").unwrap();
    assert_eq!(name, "auto-run-2026-10-02-0930.html");
    let html = std::fs::read_to_string(&out).unwrap();
    assert!(html.contains("<dd>click text=Save</dd>"));
    assert!(html.contains(&b64(&JPEG)));
    assert!(!html.to_lowercase().contains("<script"));
    // Atomic: no temp file is left beside it.
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("tcm-tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn export_is_refused_where_auto_run_is_not_offered() {
    let dir = TempDir::new();
    let root = dir.path().join("autorun");
    save_run(&root, &run_of(SHOT)).unwrap();
    let out = dir.path().join("report.html");

    let err = export_report_at(false, &root, "run-1786000200000", &out, "x").unwrap_err();
    assert_eq!(err, "not available in this build");
    assert!(!out.exists());
}

#[test]
fn export_says_so_for_a_run_that_is_gone_or_a_path_that_is_not_full() {
    let dir = TempDir::new();
    let root = dir.path().join("autorun");
    let out = dir.path().join("report.html");
    assert_eq!(export_report_at(true, &root, "run-1", &out, "x").unwrap_err(), REPORT_RUN_GONE);
    assert_eq!(
        export_report_at(true, &root, "run-1", std::path::Path::new("report.html"), "x").unwrap_err(),
        REPORT_NOT_A_FULL_PATH
    );
    // An id that is not a filename is refused before the disk is touched.
    assert!(export_report_at(true, &root, "../run-1", &out, "x").is_err());
    assert!(!out.exists());
}
