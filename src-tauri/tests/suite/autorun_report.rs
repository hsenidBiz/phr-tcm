//! The Auto Run report: one run as an HTML page opened in the browser.
//! Everything from the run is escaped, the page carries no script, every
//! case has its own section with every step, a failed case shows the step
//! that stopped it, pictures are linked from the shots folder beside the
//! reports folder (never embedded), a missing picture is a note rather than
//! a failure, and a name that is not a screenshot is never linked.

use v2_lib::autorun::report::{action_words, bucket, build, counts, BUCKETS, CSP};
use v2_lib::autorun::store::{save_run, save_script};
use v2_lib::autorun::{CaseScript, LocalRun};
use v2_lib::commands::autorun::{write_report_at, REPORT_RUN_GONE};

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

/// A JPEG's first bytes and a little more - enough to be a file.
const JPEG: [u8; 8] = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];
const SHOT: &str = "shot-1786000200000-000001.jpg";
/// The picture the passed step of case 204 took.
const PASSED_SHOT: &str = "shot-1786000200000-000002.jpg";

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
            { "case_id": 204, "title": "Logout", "verdict": "Passed", "note": "", "steps": [
                { "step_number": 1, "outcomes": [{ "ok": true, "detail": "clicked button \"Log out\"" }], "screenshot": "shot-1786000200000-000002.jpg" }
            ] }
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

fn no_shots(_: &str) -> bool {
    false
}

fn all_shots(_: &str) -> bool {
    true
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
fn a_failed_case_shows_the_step_that_stopped_it_and_links_its_picture() {
    let run = run_of(SHOT);
    let html = build(&run, &[script_201()], "x", &|name: &str| {
        assert!(name == SHOT || name == PASSED_SHOT);
        true
    });

    assert!(html.contains("<dt>Stopped at</dt><dd>Step 2</dd>"));
    assert!(html.contains("<dt>Action</dt><dd>click text=Save</dd>"));
    assert!(html.contains("<dt>Message</dt><dd>button &quot;Save&quot; not found</dd>"));
    assert!(html.contains(&format!("src=\"../shots/{SHOT}\"")));
    // The Blocked case is listed too, decided by the person with no failed step.
    assert!(html.contains("No test user"));
    assert!(html.contains("decided by the person"));
    // The account key and the script's area are in the case table.
    assert!(html.contains("<td>tester1</td><td>Login page</td>"));
}

/// The opening tag of the `<details>` section of one case.
fn details_of(html: &str, case_id: i32) -> String {
    let marker = format!("<span class=\"id\">#{case_id}</span>");
    let at = html
        .match_indices("<details")
        .map(|(i, _)| i)
        .find(|i| html[*i..].find("</summary>").is_some_and(|e| html[*i..*i + e].contains(&marker)))
        .unwrap_or_else(|| panic!("no <details> section for case {case_id}"));
    let end = html[at..].find('>').unwrap();
    html[at..at + end + 1].to_string()
}

#[test]
fn every_case_has_a_details_section_open_for_failed_and_blocked_only() {
    let html = build(&run_of(SHOT), &[script_201()], "x", &all_shots);
    assert_eq!(html.matches("<details").count(), 4);
    assert!(details_of(&html, 201).contains(" open"), "Failed opens");
    assert!(details_of(&html, 202).contains(" open"), "Blocked opens");
    assert!(!details_of(&html, 203).contains(" open"), "Not run stays closed");
    assert!(!details_of(&html, 204).contains(" open"), "Passed stays closed");
    // The page still needs no script for any of that.
    assert!(!html.to_lowercase().contains("<script"));
}

#[test]
fn every_step_and_action_is_listed_marked_and_in_order() {
    let html = build(&run_of(SHOT), &[script_201()], "x", &no_shots);
    let signed = html.find("<li class=\"ok\">\u{2713} signed in as tester1</li>").unwrap();
    let step1 = html.find("<li class=\"ok\">\u{2713} filled textbox &quot;Email&quot;</li>").unwrap();
    let step2 = html.find("<li class=\"bad\">\u{2717} button &quot;Save&quot; not found</li>").unwrap();
    let skipped = html
        .find("<li class=\"bad\">\u{2717} not run: an earlier action of this step failed</li>")
        .unwrap();
    assert!(signed < step1 && step1 < step2 && step2 < skipped);
    // The steps carry their labels.
    assert!(html.contains("<h4>Sign in</h4>"));
    assert!(html.contains("<h4>Step 1</h4>"));
    assert!(html.contains("<h4>Step 2</h4>"));
    // A passed case's step is there too, inside its own closed section.
    assert!(html.contains("clicked button &quot;Log out&quot;"));
}

#[test]
fn a_passed_steps_picture_is_linked_relative_to_the_shots_folder() {
    let html = build(&run_of(SHOT), &[], "x", &all_shots);
    assert!(html.contains(&format!("<img loading=\"lazy\" src=\"../shots/{PASSED_SHOT}\" alt=\"Step 1\">")));
    // The failed action's own picture is "Step 2, action 1".
    assert!(html.contains(&format!("<img loading=\"lazy\" src=\"../shots/{SHOT}\" alt=\"Step 2, action 1\">")));
}

#[test]
fn the_page_embeds_nothing_and_a_picture_is_never_a_data_url() {
    let html = build(&run_of(SHOT), &[script_201()], "x", &all_shots);
    assert!(!html.contains("data:image"));
    assert!(!html.contains("base64"));
    assert!(!html.to_lowercase().contains("<script"));
}

#[test]
fn a_name_with_a_slash_or_dots_is_not_linked_and_never_looked_up() {
    for bad in ["../secret.jpg", "..\\secret.jpg", "shots/shot-1-1.jpg", "shot-1-1.jpg/../../x", "x.png"] {
        let mut run = run_of(SHOT);
        run.cases[0].steps[2].outcomes[0].screenshot = Some(bad.to_string());
        run.cases[0].steps[2].screenshot = None;
        run.cases[3].steps[0].screenshot = None;
        let asked = std::cell::Cell::new(false);
        let html = build(&run, &[], "x", &|_: &str| {
            asked.set(true);
            true
        });
        assert!(!asked.get(), "{bad:?}: the lookup must not be asked about a name the guard refuses");
        assert!(!html.contains("<img"), "{bad:?} must not be linked");
        assert!(html.contains("is not a screenshot name"), "{bad:?}");
    }
}

#[test]
fn a_missing_picture_is_noted_not_fatal() {
    let run = run_of(SHOT);
    let html = build(&run, &[script_201()], "x", &no_shots);
    assert!(html.contains(&format!("The picture {SHOT} was not found")));
    assert!(!html.contains("<img"));
    assert!(html.contains("<dt>Stopped at</dt><dd>Step 2</dd>"));
    assert!(html.ends_with("</html>\n"));
}

#[test]
fn a_typed_value_never_reaches_the_report() {
    let mut run = run_of(SHOT);
    // Make the fill itself the action that stopped the case.
    run.cases[0].steps[1].outcomes[0].ok = false;
    run.cases[0].steps[1].outcomes[0].detail = "textbox \"Email\" not found".to_string();
    let html = build(&run, &[script_201()], "x", &all_shots);
    assert!(html.contains("<dt>Action</dt><dd>fill in #email</dd>"));
    assert!(!html.contains("hunter2-typed-secret"));
}

fn seeded_root(dir: &TempDir) -> std::path::PathBuf {
    let root = dir.path().join("autorun");
    save_run(&root, &run_of(SHOT)).unwrap();
    save_script(&root, &script_201()).unwrap();
    std::fs::create_dir_all(root.join("shots")).unwrap();
    std::fs::write(root.join("shots").join(SHOT), JPEG).unwrap();
    root
}

#[test]
fn opening_writes_the_page_to_reports_beside_the_shots_and_links_them() {
    let dir = TempDir::new();
    let root = seeded_root(&dir);

    let path = write_report_at(true, &root, "run-1786000200000", "2 Oct 2026, 09:30").unwrap();
    assert_eq!(path, root.join("reports").join("run-1786000200000.html"));
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("<dd>click text=Save</dd>"));
    // The time the webview sent, as given - not UTC.
    assert!(html.contains("<dt>Ran</dt><dd>2 Oct 2026, 09:30</dd>"));
    assert!(!html.contains(" UTC"));
    // The picture on disk is linked; the one that is not is a note.
    assert!(html.contains(&format!("src=\"../shots/{SHOT}\"")));
    assert!(html.contains(&format!("The picture {PASSED_SHOT} was not found")));
    assert!(!html.contains("data:image"));
    assert!(!html.to_lowercase().contains("<script"));
    // Atomic: no temp file is left beside it.
    let leftovers: Vec<_> = std::fs::read_dir(root.join("reports"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("tcm-tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn opening_again_overwrites_the_same_file() {
    let dir = TempDir::new();
    let root = seeded_root(&dir);
    let first = write_report_at(true, &root, "run-1786000200000", "2 Oct 2026, 09:30").unwrap();
    std::fs::write(&first, "stale").unwrap();
    // A picture that has since arrived is linked on the next open.
    std::fs::write(root.join("shots").join(PASSED_SHOT), JPEG).unwrap();

    let second = write_report_at(true, &root, "run-1786000200000", "2 Oct 2026, 09:30").unwrap();
    assert_eq!(first, second);
    let html = std::fs::read_to_string(&second).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains(&format!("src=\"../shots/{PASSED_SHOT}\"")));
    assert_eq!(std::fs::read_dir(root.join("reports")).unwrap().flatten().count(), 1);
}

#[test]
fn opening_is_refused_where_auto_run_is_not_offered() {
    let dir = TempDir::new();
    let root = seeded_root(&dir);

    let err = write_report_at(false, &root, "run-1786000200000", "x").unwrap_err();
    assert_eq!(err, "not available in this build");
    assert!(!root.join("reports").exists());
}

#[test]
fn opening_says_so_for_a_run_that_is_gone_and_refuses_an_unsafe_id() {
    let dir = TempDir::new();
    let root = dir.path().join("autorun");
    assert_eq!(write_report_at(true, &root, "run-1", "x").unwrap_err(), REPORT_RUN_GONE);
    for bad in ["../run-1", "a/b", "a\\b", "", "run 1", "run-1.html"] {
        let err = write_report_at(true, &root, bad, "x").unwrap_err();
        assert!(err.contains("not a safe filename"), "{bad:?}: {err}");
    }
    assert!(!root.join("reports").exists());
    assert!(!dir.path().join("run-1.html").exists());
}

#[test]
fn the_page_declares_a_policy_that_runs_nothing_and_loads_nothing() {
    let html = build(&run_of(SHOT), &[], "x", &no_shots);
    assert_eq!(CSP, "default-src 'none'; img-src file:; style-src 'unsafe-inline'");
    assert!(html.contains(&format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">")));
}

#[test]
fn an_api_checks_address_is_reported_without_its_query_string() {
    let watch: v2_lib::browser::actions::Action = serde_json::from_value(serde_json::json!({
        "kind": "expect_response", "method": "post", "url_contains": "/hr/Cycle/Save?access_token=abc#top"
    }))
    .unwrap();
    assert_eq!(action_words(&watch), "expect a POST request to /hr/Cycle/Save to answer 200");
    let ask: v2_lib::browser::actions::Action =
        serde_json::from_value(serde_json::json!({ "kind": "api_request", "path": "/api/me?token=abc#x" })).unwrap();
    assert_eq!(action_words(&ask), "ask /api/me and expect 200");
}

#[test]
fn a_navigate_address_is_reported_without_its_query_string() {
    let go: v2_lib::browser::actions::Action = serde_json::from_value(serde_json::json!({
        "kind": "navigate", "url": "https://app.example.test/login?token=s3cr3t-value&x=1#frag"
    }))
    .unwrap();
    assert_eq!(action_words(&go), "go to https://app.example.test/login");

    // And in the page, when that navigate is what stopped the case.
    let mut run = run_of(SHOT);
    run.cases[0].steps[2].outcomes[0].detail = "the page did not finish loading".to_string();
    let script: CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": 201, "title": "Valid login",
        "steps": [{ "step_number": 2, "actions": [
            { "kind": "navigate", "url": "https://app.example.test/login?token=s3cr3t-value" }
        ] }]
    }))
    .unwrap();
    let html = build(&run, &[script], "x", &no_shots);
    assert!(html.contains("<dd>go to https://app.example.test/login</dd>"));
    assert!(!html.contains("s3cr3t-value"));
}

#[test]
fn opening_escapes_the_time_it_is_given() {
    let dir = TempDir::new();
    let root = seeded_root(&dir);
    let path = write_report_at(true, &root, "run-1786000200000", "<script>x</script>").unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("<dt>Ran</dt><dd>&lt;script&gt;x&lt;/script&gt;</dd>"));
    assert!(!html.to_lowercase().contains("<script"));
}
