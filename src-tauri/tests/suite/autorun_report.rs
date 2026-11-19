//! The Auto Test report: one run as an HTML page opened in the browser.
//! Everything from the run is escaped, the page's only scripts are its own
//! inline ones (the new views' tests are in `autorun_report_smart`), every
//! case has its own section with every step, a failed case shows the step
//! that stopped it, pictures are linked from the shots folder beside the
//! reports folder (never embedded), a missing picture is a note rather than
//! a failure, and a name that is not a screenshot is never linked.

use v2_lib::autorun::report::{action_words, bucket, build, build_with_downloads, counts, scrub_urls, BUCKETS};
use v2_lib::autorun::store::{downloads_dir, save_run, save_script};
use v2_lib::autorun::{CaseScript, LocalRun};
use v2_lib::commands::autorun::{write_report_at, REPORT_RUN_GONE};

/// Every `<script` in the page is one of the page's own: inline (no `src`),
/// either its data blob or a plain script - and there are exactly the three
/// it writes. Anything a case wrote that looked like a tag was escaped.
pub fn only_the_pages_own_scripts(html: &str) {
    let lower = html.to_lowercase();
    let opens: Vec<usize> = lower.match_indices("<script").map(|(i, _)| i).collect();
    assert_eq!(opens.len(), 3, "the page writes three scripts and nothing else may open one");
    for i in opens {
        let tag_end = lower[i..].find('>').map(|n| i + n).unwrap();
        let tag = &lower[i..=tag_end];
        assert!(tag == "<script>" || tag == "<script type=\"application/json\" id=\"report-data\">", "unexpected script tag {tag}");
    }
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
    // Each case's detail marks how its result was decided, and its status.
    let blocked = article_of(&html, 202);
    assert!(blocked.contains("data-status=\"blocked\"") && blocked.contains("Decided by: Confirmed"), "{blocked}");
    let failed = article_of(&html, 201);
    assert!(failed.contains("data-status=\"failed\"") && failed.contains("Decided by: Proposed"), "{failed}");
}

#[test]
fn every_value_is_escaped_and_the_page_carries_no_script() {
    let mut run = run_of(SHOT);
    run.cases[0].title = "<script>alert('x')</script> & \"quotes\"".to_string();
    run.cases[1].note = "<img src=x onerror=alert(1)>".to_string();
    run.environment = Some("<b>QA</b>".to_string());
    let html = build(&run, &[], "<script>when</script>", &no_shots);

    only_the_pages_own_scripts(&html);
    assert!(html.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; &quot;quotes&quot;"));
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(html.contains("&lt;b&gt;QA&lt;/b&gt;"));
    assert!(!html.contains("<img src=x"));
}

#[test]
fn the_header_says_when_how_where_and_whether_it_was_sent() {
    let mut run = run_of(SHOT);
    let html = build(&run, &[], "2 Oct 2026, 09:30", &no_shots);
    assert!(html.contains("<span class=\"topbar__brand\">Auto Test report</span>"));
    assert!(html.contains("<dt>Ran</dt><dd><time data-ts=\"1786000200000\">2 Oct 2026, 09:30</time></dd>"));
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
    assert!(html.contains(" UTC</time></dd>"));
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

    assert!(html.contains("Stopped at</span> Step 2 \u{b7} click text=Save</p>"));
    assert!(html.contains("<span class=\"error-text__head\">button &quot;Save&quot; not found</span>"));
    assert!(html.contains(&format!("src=\"../shots/{SHOT}\"")));
    // The Blocked case is listed too, decided by the person with no failed step.
    assert!(html.contains("No test user"));
    assert!(html.contains("decided by the person"));
    // The account key and the script's area are in the case's detail.
    assert!(html.contains("<span>Login page</span>") && html.contains("as tester1</span>"));
}

/// One case's whole detail, from its opening tag to its close.
pub fn article_of(html: &str, case_id: i32) -> &str {
    let at = html
        .find(&format!("<article class=\"dt detail\" id=\"case-{case_id}\""))
        .unwrap_or_else(|| panic!("no detail for case {case_id}"));
    let rest = &html[at..];
    &rest[..rest.find("</article>").unwrap()]
}

#[test]
fn every_case_has_a_detail_and_only_failed_and_blocked_print_in_full() {
    let html = build(&run_of(SHOT), &[script_201()], "x", &all_shots);
    assert_eq!(html.matches("<article class=\"dt detail\"").count(), 4);
    assert!(article_of(&html, 201).contains("data-print=\"1\""), "Failed prints in full");
    assert!(article_of(&html, 202).contains("data-print=\"1\""), "Blocked prints in full");
    assert!(article_of(&html, 203).contains("data-print=\"0\""), "Not run does not");
    assert!(article_of(&html, 204).contains("data-print=\"0\""), "Passed does not");
    // The page still needs no script for any of that.
    only_the_pages_own_scripts(&html);
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
    assert!(html.contains("<span class=\"step__title\">Sign in</span>"));
    assert!(html.contains("<span class=\"step__title\">Step 1</span>"));
    assert!(html.contains("<span class=\"step__title\">Step 2</span>"));
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
    only_the_pages_own_scripts(&html);
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
    assert!(html.contains("Stopped at</span> Step 2"));
    assert!(html.ends_with("</html>\n"));
}

#[test]
fn a_typed_value_never_reaches_the_report() {
    let mut run = run_of(SHOT);
    // Make the fill itself the action that stopped the case.
    run.cases[0].steps[1].outcomes[0].ok = false;
    run.cases[0].steps[1].outcomes[0].detail = "textbox \"Email\" not found".to_string();
    let html = build(&run, &[script_201()], "x", &all_shots);
    assert!(html.contains("\u{b7} fill in #email</p>"));
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

    let path = write_report_at(true, &root, "Acme", "Web", "run-1786000200000", "2 Oct 2026, 09:30").unwrap();
    assert_eq!(path, root.join("reports").join("run-1786000200000.html"));
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("\u{b7} click text=Save</p>"));
    // The time the webview sent, as given - not UTC.
    assert!(html.contains("<dt>Ran</dt><dd><time data-ts=\"1786000200000\">2 Oct 2026, 09:30</time></dd>"));
    // The header says the time as given; only the history's own times fall back to UTC (the
    // page's script shows those in the reader's locale).
    let top = &html[html.find("<header class=\"topbar\">").unwrap()..html.find("</header>").unwrap()];
    assert!(top.contains("2 Oct 2026, 09:30") && !top.contains(" UTC"), "{top}");
    // The picture on disk is linked; the one that is not is a note.
    assert!(html.contains(&format!("src=\"../shots/{SHOT}\"")));
    assert!(html.contains(&format!("The picture {PASSED_SHOT} was not found")));
    assert!(!html.contains("data:image"));
    only_the_pages_own_scripts(&html);
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
    let first = write_report_at(true, &root, "Acme", "Web", "run-1786000200000", "2 Oct 2026, 09:30").unwrap();
    std::fs::write(&first, "stale").unwrap();
    // A picture that has since arrived is linked on the next open.
    std::fs::write(root.join("shots").join(PASSED_SHOT), JPEG).unwrap();

    let second = write_report_at(true, &root, "Acme", "Web", "run-1786000200000", "2 Oct 2026, 09:30").unwrap();
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

    let err = write_report_at(false, &root, "Acme", "Web", "run-1786000200000", "x").unwrap_err();
    assert_eq!(err, "Auto Test is switched off in Modules");
    assert!(!root.join("reports").exists());
}

#[test]
fn opening_says_so_for_a_run_that_is_gone_and_refuses_an_unsafe_id() {
    let dir = TempDir::new();
    let root = dir.path().join("autorun");
    assert_eq!(write_report_at(true, &root, "Acme", "Web", "run-1", "x").unwrap_err(), REPORT_RUN_GONE);
    for bad in ["../run-1", "a/b", "a\\b", "", "run 1", "run-1.html"] {
        let err = write_report_at(true, &root, "Acme", "Web", bad, "x").unwrap_err();
        assert!(err.contains("not a safe filename"), "{bad:?}: {err}");
    }
    assert!(!root.join("reports").exists());
    assert!(!dir.path().join("run-1.html").exists());
}

/// The text of every script the page runs (the data blob is not one).
pub fn page_scripts(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("<script>") {
        let body = &rest[at + "<script>".len()..];
        let end = body.find("</script>").unwrap();
        out.push(body[..end].to_string());
        rest = &body[end..];
    }
    out
}

/// The Content-Security-Policy the page declares.
pub fn page_csp(html: &str) -> String {
    let open = "<meta http-equiv=\"Content-Security-Policy\" content=\"";
    let at = html.find(open).expect("a policy") + open.len();
    html[at..at + html[at..].find('"').unwrap()].to_string()
}

#[test]
fn the_policy_loads_nothing_and_names_the_pages_own_scripts_by_hash() {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let html = build(&run_of(SHOT), &[], "x", &no_shots);
    let csp = page_csp(&html);
    for part in ["default-src 'none'", "img-src file:", "media-src file:", "style-src 'unsafe-inline'", "base-uri 'none'", "form-action 'none'"] {
        assert!(csp.contains(part), "{part} in {csp}");
    }
    let script_src = csp.split("; ").find(|p| p.starts_with("script-src")).expect("a script-src");
    assert!(!script_src.contains("'unsafe-inline'") && !script_src.contains("'unsafe-eval'"), "{script_src}");
    let scripts = page_scripts(&html);
    assert_eq!(scripts.len(), 2, "the head script and the page's own");
    for body in &scripts {
        let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(body.as_bytes()));
        assert!(script_src.contains(&format!("'sha256-{hash}'")), "no hash for a script in {script_src}");
    }
    // A script that is not one of those is not covered: one hash per script, no more.
    assert_eq!(script_src.matches("'sha256-").count(), 2);
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
fn a_timed_request_check_says_its_timing_and_the_measured_start_reaches_the_report() {
    let said = |v: serde_json::Value| action_words(&serde_json::from_value::<v2_lib::browser::actions::Action>(v).unwrap());
    assert_eq!(
        said(serde_json::json!({
            "kind": "expect_response", "method": "GET", "url_contains": "/api/cards",
            "not_before_ms": 600, "timeout_ms": 2000
        })),
        "expect a GET request to /api/cards to answer 200, starting at least 600 ms after the action before it"
    );
    assert_eq!(
        said(serde_json::json!({
            "kind": "expect_response", "method": "GET", "url_contains": "/api/cards",
            "started_within_ms": 300, "same_document": true
        })),
        "expect a GET request to /api/cards to answer 200, starting within 300 ms of the action before it, with no page reload"
    );
    let mut run = run_of(SHOT);
    run.cases[3].steps[0].outcomes[0].detail = "GET /api/cards answered 200, started 812 ms after action 1 (fill): the step's matching request 1 of 1, sent after action 1 (fill) began".to_string();
    let html = build(&run, &[], "x", &no_shots);
    assert!(html.contains("started 812 ms after action 1 (fill)"));
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
    assert!(html.contains("\u{b7} go to https://app.example.test/login</p>"));
    assert!(!html.contains("s3cr3t-value"));
}

#[test]
fn opening_escapes_the_time_it_is_given() {
    let dir = TempDir::new();
    let root = seeded_root(&dir);
    let path = write_report_at(true, &root, "Acme", "Web", "run-1786000200000", "<script>x</script>").unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("<dt>Ran</dt><dd><time data-ts=\"1786000200000\">&lt;script&gt;x&lt;/script&gt;</time></dd>"));
    only_the_pages_own_scripts(&html);
}

#[test]
fn an_address_in_a_recorded_sentence_loses_its_query_string_and_fragment() {
    let mut run = run_of(SHOT);
    // A passed outcome, a failed one's message, and the run's own reason.
    run.cases[3].steps[0].outcomes[0].detail = "loaded https://x/y?token=s3cr3t#frag".to_string();
    run.cases[0].steps[2].outcomes[0].detail = "url is http://h/p?a=s3cr3t2 not the expected one".to_string();
    run.cases[0].reason = "moved to file:///C:/a/b.html?k=s3cr3t3#top and stopped".to_string();
    let html = build(&run, &[], "x", &no_shots);
    assert!(html.contains("<li class=\"ok\">\u{2713} loaded https://x/y</li>"));
    assert!(html.contains("<span class=\"error-text__head\">url is http://h/p not the expected one</span>"));
    assert!(html.contains("moved to file:///C:/a/b.html and stopped"));
    for secret in ["s3cr3t", "s3cr3t2", "s3cr3t3", "#frag", "#top"] {
        assert!(!html.contains(secret), "{secret} leaked into the page");
    }
}

#[test]
fn scrubbing_keeps_the_rest_of_the_sentence_and_every_address_in_it() {
    assert_eq!(scrub_urls("no address here: 100% ok? yes #1"), "no address here: 100% ok? yes #1");
    assert_eq!(
        scrub_urls("from HTTPS://a/b?x=1 to http://c/d#e then (https://f/g?h=2) done"),
        "from HTTPS://a/b to http://c/d then (https://f/g) done"
    );
    assert_eq!(scrub_urls("moved to \"https://x/y?t=1\" ok"), "moved to \"https://x/y\" ok");
    assert_eq!(scrub_urls("https://x/y?t=1"), "https://x/y");
    assert_eq!(scrub_urls(""), "");
}

/// One case's whole detail.
fn section_of(html: &str, case_id: i32) -> &str {
    article_of(html, case_id)
}

/// Spec §6: a case that ran a second time after a transient failure says
/// so beside its result, and its section names the first try's failure.
#[test]
fn a_retried_case_is_labelled_and_names_its_first_try() {
    let mut run = run_of(SHOT);
    run.cases[3].proposed = "Passed".into();
    run.cases[3].retried = Some("step 1: GET /hr/api/<x> answered 503, expected 200".into());
    let html = build(&run, &[script_201()], "x", &all_shots);
    let section = section_of(&html, 204);
    assert!(section.contains("<span class=\"signal\">Retried</span>"), "{section}");
    assert!(
        section.contains("<p><strong>Retried:</strong> the first try failed: step 1: GET /hr/api/&lt;x&gt; answered 503, expected 200</p>"),
        "{section}"
    );
    // A case that was not retried carries neither.
    let other = section_of(&html, 201);
    assert!(!other.contains("Retried"), "{other}");
}

/// A case whose preconditions were skipped while Database Read Access was
/// off says so in its section; a case without a notice says nothing.
#[test]
fn a_case_with_a_notice_shows_it_in_its_section() {
    let mut run = run_of(SHOT);
    run.cases[3].notice = Some(v2_lib::autorun::preconditions::NOT_CHECKED.into());
    let html = build(&run, &[script_201()], "x", &all_shots);
    let section = section_of(&html, 204);
    assert!(
        section.contains(
            "<p><strong>Notice:</strong> preconditions were not checked: the Database Access module is off in Settings</p>"
        ),
        "{section}"
    );
    assert!(!section_of(&html, 201).contains("Notice"));
}

/// `run_of`, with case 201's step 1 and step 2 each saving a file, and a
/// name a page could have made hostile on case 204.
fn run_with_downloads() -> LocalRun {
    let mut run = run_of(SHOT);
    run.cases[0].steps[1].downloads = vec!["Template.xlsx".into()];
    run.cases[0].steps[2].downloads = vec!["errors.csv".into()];
    run.cases[3].steps[0].downloads = vec!["<b>x</b>&.csv".into()];
    run
}

fn sizes(name: &str) -> Option<u64> {
    match name {
        "Template.xlsx" => Some(5427),
        "errors.csv" => Some(1126),
        _ => None,
    }
}

#[test]
fn a_case_with_downloads_has_one_downloads_line_naming_each_with_its_size() {
    let html = build_with_downloads(&run_with_downloads(), &[script_201()], "x", &no_shots, &sizes);
    assert!(
        html.contains("<p><strong>Downloads:</strong> Template.xlsx (5.3 KB), errors.csv (1.1 KB)</p>"),
        "{html}"
    );
    assert_eq!(html.matches("<strong>Downloads:</strong>").count(), 2, "one line per case with downloads");
    // Names only: never a link to the file, never its folder.
    assert!(!html.contains("downloads/"));
    assert!(!html.contains("href=\"Template.xlsx"));
}

#[test]
fn a_download_name_is_escaped_and_a_gone_file_says_so() {
    let html = build_with_downloads(&run_with_downloads(), &[], "x", &no_shots, &sizes);
    assert!(
        html.contains("<strong>Downloads:</strong> &lt;b&gt;x&lt;/b&gt;&amp;.csv (no longer on this machine)</p>"),
        "{html}"
    );
    assert!(!html.contains("<b>x</b>"));
}

#[test]
fn a_case_with_no_downloads_has_no_downloads_line() {
    let html = build(&run_of(SHOT), &[script_201()], "x", &no_shots);
    assert!(!html.contains("Downloads:"));
}

#[test]
fn the_written_report_reads_each_downloads_size_from_the_runs_folder() {
    let dir = TempDir::new();
    let root = dir.path().join("autorun");
    save_run(&root, &run_with_downloads()).unwrap();
    let folder = downloads_dir(&root, "run-1786000200000");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Template.xlsx"), vec![0u8; 5427]).unwrap();
    std::fs::write(folder.join("errors.csv"), vec![0u8; 1126]).unwrap();

    let path = write_report_at(true, &root, "Acme", "Web", "run-1786000200000", "x").unwrap();
    let html = std::fs::read_to_string(path).unwrap();
    assert!(html.contains("Template.xlsx (5.3 KB), errors.csv (1.1 KB)"), "{html}");
    assert!(html.contains("&lt;b&gt;x&lt;/b&gt;&amp;.csv (no longer on this machine)"));
}

/// `build` reads each download's size from the run's own folder under the
/// app's data root, the way the written report does - it never just says a
/// file that is there is gone.
#[test]
fn build_reads_each_downloads_size_from_the_data_root() {
    let _serial = crate::serial::autorun();
    let dir = TempDir::new();
    let root = dir.path().join("autorun");
    let folder = downloads_dir(&root, "run-1786000200000");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Template.xlsx"), vec![0u8; 5427]).unwrap();
    v2_lib::autorun::store::set_root(root.clone());
    let html = build(&run_with_downloads(), &[script_201()], "x", &no_shots);
    assert!(html.contains("Template.xlsx (5.3 KB), errors.csv (no longer on this machine)"), "{html}");
}

// ---- components ----

#[test]
fn a_report_of_a_component_step_lines_up() {
    use crate::common::{component_case, component_run, component_script, pick_a_date_file, COMPONENT_TYPED};
    use v2_lib::autorun::components::{expand, ComponentFile};
    use v2_lib::autorun::report::build_with_components;
    let run = component_run(vec![component_case(7, "#start cannot be typed into")]);
    let typed = expand(
        &crate::common::pick_a_date(),
        serde_json::json!({ "field": { "css": "#start" }, "day": COMPONENT_TYPED }).as_object().unwrap(),
    )
    .unwrap();
    let html = build_with_components(&run, &[component_script(7)], &pick_a_date_file(), "x", &no_shots, &|_| None);
    assert!(
        html.contains(&format!("\u{b7} Pick a date: {}</p>", v2_lib::html_escape::esc_attr(&action_words(&typed[1])))),
        "{html}"
    );
    // Every outcome is listed, those the component ran named by it.
    assert!(html.contains("\u{2713} clicked #new</li>"), "{html}");
    assert!(html.contains("\u{2713} Pick a date: clicked #start</li>"), "{html}");
    assert!(html.contains("\u{2717} Pick a date: #start cannot be typed into</li>"), "{html}");
    assert!(html.contains("\u{2717} page does NOT contain Saved</li>"), "{html}");
    assert!(!html.contains(COMPONENT_TYPED));
    assert!(!html.contains("script on this machine has changed"), "{html}");

    // The component no longer there: named by the component, not called a
    // changed script.
    let html = build_with_components(&run, &[component_script(7)], &ComponentFile::default(), "x", &no_shots, &|_| None);
    assert!(
        html.contains("\u{b7} Pick a date: action 3 (the component on this machine has changed since the run)</p>"),
        "{html}"
    );
    assert!(!html.contains("script on this machine has changed"), "{html}");
}

/// The report reads the components of the project it is opened for: the
/// same name saved in another project is not the one that ran.
#[test]
fn the_report_reads_the_components_of_its_own_project() {
    use crate::common::{component_case, component_run, component_script, pick_a_date, COMPONENT_TYPED};
    use v2_lib::autorun::components::{expand, put};
    let dir = TempDir::new();
    let root = dir.path().join("autorun");
    save_run(&root, &component_run(vec![component_case(7, "#start cannot be typed into")])).unwrap();
    save_script(&root, &component_script(7)).unwrap();
    put(&root, "Acme", "Web", pick_a_date()).unwrap();
    let mut other = pick_a_date();
    other.actions.truncate(1);
    put(&root, "Other", "Project", other).unwrap();

    let path = write_report_at(true, &root, "Acme", "Web", "run-1786000200000", "x").unwrap();
    let html = std::fs::read_to_string(path).unwrap();
    let typed = expand(
        &pick_a_date(),
        serde_json::json!({ "field": { "css": "#start" }, "day": COMPONENT_TYPED }).as_object().unwrap(),
    )
    .unwrap();
    assert!(
        html.contains(&format!("\u{b7} Pick a date: {}</p>", v2_lib::html_escape::esc_attr(&action_words(&typed[1])))),
        "{html}"
    );
    assert!(!html.contains("has changed since the run"), "{html}");

    // Opened for the other project, its own (shorter) component does not
    // expand to what ran.
    let path = write_report_at(true, &root, "Other", "Project", "run-1786000200000", "x").unwrap();
    let html = std::fs::read_to_string(path).unwrap();
    assert!(
        html.contains("\u{b7} Pick a date: action 3 (the component on this machine has changed since the run)</p>"),
        "{html}"
    );
}

#[test]
fn the_report_shows_how_long_a_case_and_its_steps_took() {
    let run: LocalRun = serde_json::from_value(serde_json::json!({
        "id": "run-1", "pbi_id": 1, "started_at": "1786000200000", "mode": "unattended",
        "cases": [{
            "case_id": 301, "title": "Timed", "verdict": "", "note": "", "proposed": "Passed",
            "phases": { "open_ms": 1100, "sign_in_ms": 12400, "area_ms": 3000, "steps_ms": 23900, "close_ms": 800, "total_ms": 41200 },
            "steps": [{ "step_number": 1, "outcomes": [{ "ok": true, "detail": "clicked" }], "duration_ms": 2300 }]
        }]
    }))
    .unwrap();
    let html = build(&run, &[], "x", &|_: &str| false);
    assert!(html.contains("Took 41.2 s: open 1.1, sign-in 12.4, area 3.0, steps 23.9, close 0.8"), "{html}");
    assert!(html.contains("2.3 s"), "{html}");
}

/// A style check is said with its property and its comparison.
#[test]
fn a_style_check_is_said_in_words() {
    let say = |v: serde_json::Value| action_words(&serde_json::from_value(v).unwrap());
    assert_eq!(say(serde_json::json!({ "kind": "expect_style", "selector": "#s", "property": "color", "equals": "red" })), "expect #s's color to be \"red\"");
    assert_eq!(say(serde_json::json!({ "kind": "expect_style", "selector": "#s", "property": "box-shadow", "contains": "2px" })), "expect #s's box-shadow to contain \"2px\"");
    assert_eq!(say(serde_json::json!({ "kind": "expect_style", "selector": "#s", "property": "display", "not_equals": "none" })), "expect #s's display not to be \"none\"");
}

/// #46: a case's clean-up is shown as its own block, named Clean-up, and a
/// failed one is said under the case.
#[test]
fn a_clean_up_is_its_own_block_and_its_failure_its_own_line() {
    let run: LocalRun = serde_json::from_value(serde_json::json!({
        "id": "run-1786000200000", "pbi_id": 42, "started_at": "1786000200000", "mode": "unattended",
        "cases": [{
            "case_id": 301, "title": "Add a rating", "verdict": "", "note": "",
            "proposed": "Passed", "reason": "every action of 1 steps passed, but its clean-up failed",
            "steps": [{ "step_number": 1, "outcomes": [{ "ok": true, "detail": "clicked button \"Save\"" }] }],
            "cleanup": { "step_number": 2, "outcomes": [{ "ok": false, "detail": "button \"Delete\" not found" }] }
        }]
    }))
    .unwrap();
    let html = build(&run, &[], "", &|_| false);
    assert!(html.contains("<span class=\"step__title\">Clean-up</span>"), "{html}");
    assert!(html.contains("<p><strong>Clean-up:</strong> button &quot;Delete&quot; not found</p>"), "{html}");
    assert!(!html.contains("<span class=\"step__title\">Step 2</span>"), "{html}");
}


#[test]
fn the_new_check_options_have_words() {
    let say = |v: serde_json::Value| action_words(&serde_json::from_value(v).unwrap());
    let sel = serde_json::json!({ "css": "#n" });
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_text", "selector": sel, "matches": "^Showing" })),
        "expect #n to match /^Showing/"
    );
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_attribute", "selector": sel, "name": "aria-label", "matches": "^View " })),
        "expect #n's aria-label to match /^View /"
    );
    assert_eq!(say(serde_json::json!({ "kind": "expect_count", "selector": sel, "at_least": 2 })), "expect at least 2 of #n");
    assert_eq!(say(serde_json::json!({ "kind": "expect_count", "selector": sel, "at_most": 5 })), "expect at most 5 of #n");
    assert_eq!(say(serde_json::json!({ "kind": "expect_count", "selector": sel, "equals": 3 })), "expect 3 of #n");
}

#[test]
fn an_each_check_has_words() {
    let say = |v: serde_json::Value| action_words(&serde_json::from_value(v).unwrap());
    let list = serde_json::json!({ "css": "div.card" });
    let h3 = serde_json::json!({ "css": "h3" });
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_each", "selector": list, "child": h3, "matches": "^AUTOTEST " })),
        "expect h3 in each of div.card to match /^AUTOTEST /"
    );
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_each", "selector": list, "visible": true, "child": h3, "min": 2 })),
        "expect h3 in each of div.card to be visible (at least 2)"
    );
    assert_eq!(say(serde_json::json!({ "kind": "expect_each", "selector": list, "equals": "Draft" })), "expect each of div.card to read \"Draft\"");
    assert_eq!(say(serde_json::json!({ "kind": "expect_each", "selector": list, "contains": "Draft", "min": 1 })), "expect each of div.card to contain \"Draft\"");
    assert_eq!(say(serde_json::json!({ "kind": "expect_each", "selector": list, "not_empty": true })), "expect each of div.card to have text");
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_each", "selector": list, "attribute": "href", "equals": "/a" })),
        "expect each of div.card to have href=\"/a\""
    );
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_each", "selector": list, "attribute": "href", "contains": "a" })),
        "expect each of div.card to have href containing \"a\""
    );
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_each", "selector": list, "attribute": "aria-label", "matches": "^View " })),
        "expect each of div.card to have aria-label matching /^View /"
    );
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_each", "selector": list, "attribute": "title", "not_empty": true })),
        "expect each of div.card to have a title"
    );
}

#[test]
fn an_order_check_and_a_position_check_have_words() {
    let say = |v: serde_json::Value| action_words(&serde_json::from_value(v).unwrap());
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_order", "selector": { "css": "li" }, "texts": ["All", "Draft", "Published"] })),
        "expect li to list \"All\", \"Draft\", \"Published\" in that order"
    );
    let bar = serde_json::json!({ "css": "#bar" });
    let new = serde_json::json!({ "css": "#new" });
    for (relation, words) in [
        ("left_of", "left of"),
        ("right_of", "right of"),
        ("above", "above"),
        ("below", "below"),
        ("inside", "inside"),
        ("in_top_left", "in the top-left of"),
        ("in_top_right", "in the top-right of"),
        ("in_bottom_left", "in the bottom-left of"),
        ("in_bottom_right", "in the bottom-right of"),
    ] {
        assert_eq!(
            say(serde_json::json!({ "kind": "expect_position", "selector": new, "relative_to": bar, "relation": relation })),
            format!("expect #new to be {words} #bar")
        );
    }
}

#[test]
fn a_forced_click_that_expects_nothing_has_words() {
    let say = |v: serde_json::Value| action_words(&serde_json::from_value(v).unwrap());
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_no_change", "selector": { "css": "#save" } })),
        "click #save anyway and expect nothing to happen"
    );
    assert_eq!(
        say(serde_json::json!({ "kind": "expect_no_change", "selector": { "css": "#save" }, "region": { "css": "#cards" }, "within_ms": 2000 })),
        "click #save anyway and expect nothing to happen to #cards"
    );
}

/// A case's whole detail.
fn product_section_of(html: &str, case_id: i32) -> String {
    article_of(html, case_id).to_string()
}

fn with_bugs(run: &mut LocalRun, bugs: &[(i32, &[&str])]) {
    for (case_id, list) in bugs {
        let c = run.cases.iter_mut().find(|c| c.case_id == *case_id).unwrap();
        c.product_bugs = list.iter().map(|s| s.to_string()).collect();
    }
}

#[test]
fn a_product_bug_is_listed_at_the_top_of_the_report_and_on_its_case() {
    let mut run = run_of(SHOT);
    with_bugs(&mut run, &[(201, &["q1: Save <twice> makes a duplicate"]), (203, &["q1: Save <twice> makes a duplicate", "q2: dates drop the year"])]);
    let html = build(&run, &[], "2 Oct 2026", &no_shots);
    let top = html.find("<h2>Product bugs worked around</h2>").expect("the section");
    assert!(top < html.find("<h2>Summary</h2>").unwrap(), "above the summary");
    assert!(html.contains("<li>q1: Save &lt;twice&gt; makes a duplicate - cases #201, #203</li>"), "{html}");
    assert!(html.contains("<li>q2: dates drop the year - cases #203</li>"));
    assert!(product_section_of(&html, 201).contains("<p><strong>Product bug:</strong> q1: Save &lt;twice&gt; makes a duplicate</p>"));
    assert!(!product_section_of(&html, 204).contains("Product bug:"));
}

#[test]
fn the_top_list_stops_at_ten() {
    let mut run = run_of(SHOT);
    let bugs: Vec<String> = (1..=13).map(|i| format!("q{i}: bug {i}")).collect();
    run.cases[0].product_bugs = bugs;
    let html = build(&run, &[], "2 Oct 2026", &no_shots);
    let top = &html[html.find("<ul class=\"bugs\">").unwrap()..];
    let top = &top[..top.find("</ul>").unwrap()];
    assert_eq!(top.matches("<li>").count(), 11, "{top}");
    assert!(top.contains("<li>and 3 more</li>"), "{top}");
    assert!(top.contains("q10: bug 10") && !top.contains("q11: bug 11"));
}

#[test]
fn a_run_with_no_product_bug_or_divergence_has_no_new_section() {
    let html = build(&run_of(SHOT), &[], "2 Oct 2026", &no_shots);
    assert!(!html.contains("Product bugs worked around") && !html.contains("<ul class=\"bugs\">"));
    assert!(!html.contains("Divergence:") && !html.contains("Product bug:"));
}

#[test]
fn a_divergence_shows_on_its_case_in_the_report() {
    use v2_lib::autorun::divergences::{Divergence, DivergenceKind, DivergenceStatus, Ruling};
    use v2_lib::autorun::report::{build_with_extras, ReportExtras};
    let run = run_of(SHOT);
    let started: i64 = run.started_at.parse().unwrap();
    let d = |case_id: i32, status: DivergenceStatus, ruling: Option<Ruling>, ruled_at: Option<i64>| Divergence {
        id: "d1".into(),
        case_id,
        step: 2,
        case_says: "4 levels".into(),
        app_shows: "6 levels".into(),
        kind: DivergenceKind::Data,
        reported_at: started.to_string(),
        status,
        ruling,
        ruled_at: ruled_at.map(|t| t.to_string()),
        case_fp: None,
        step_action: None,
        ruled_by: None,
        said: None,
        note: None,
        proposal: None,
        pbi: None,
    };
    let day = 24 * 60 * 60 * 1000;
    let list = vec![
        d(201, DivergenceStatus::Open, None, None),
        d(203, DivergenceStatus::AwaitingRepair, None, None),
        d(204, DivergenceStatus::Ruled, Some(Ruling::DataFixed), Some(started - day)),
        d(202, DivergenceStatus::Ruled, Some(Ruling::CaseStands), Some(started - 30 * day)),
    ];
    let html = build_with_extras(
        &run,
        &[],
        &Default::default(),
        "2 Oct 2026",
        &no_shots,
        &|_| None,
        &ReportExtras { divergences: &list },
    );
    assert!(product_section_of(&html, 201).contains(
        "<p><strong>Divergence:</strong> step 2: the case says &quot;4 levels&quot;, the app shows &quot;6 levels&quot; (data, needs a ruling)</p>"
    ) || product_section_of(&html, 201).contains("step 2: the case says"), "{}", section_of(&html, 201));
    assert!(product_section_of(&html, 201).contains("(data, needs a ruling)"));
    assert!(product_section_of(&html, 203).contains("(data, waits for a repair)"));
    assert!(product_section_of(&html, 204).contains("(data, ruled: you fixed the data)"));
    assert!(!product_section_of(&html, 202).contains("Divergence:"), "ruled long ago");
}

#[test]
fn the_address_checks_read_in_the_report() {
    let said = |v: serde_json::Value| action_words(&serde_json::from_value::<v2_lib::browser::actions::Action>(v).unwrap());
    assert_eq!(
        said(serde_json::json!({ "kind": "check_url", "equals": "/hr/cycles" })),
        "expect the address to be \"/hr/cycles\""
    );
    assert_eq!(
        said(serde_json::json!({ "kind": "check_url", "equals": "https://app.example.test/hr/cycles?token=abc" })),
        "expect the address to be \"/hr/cycles\""
    );
    assert_eq!(
        said(serde_json::json!({ "kind": "check_url", "same_as": "before" })),
        "expect the address to be the one noted as \"before\""
    );
    assert_eq!(said(serde_json::json!({ "kind": "remember_url", "as": "before" })), "note the address as \"before\"");
    assert_eq!(said(serde_json::json!({ "kind": "check_url", "contains": "/home" })), "check the address contains \"/home\"");
}

#[test]
fn page_until_reads_in_the_report() {
    let said = |v: serde_json::Value| action_words(&serde_json::from_value::<v2_lib::browser::actions::Action>(v).unwrap());
    assert_eq!(
        said(serde_json::json!({
            "kind": "page_until", "next": { "role": "button", "name": "Next" }, "until": { "text": "Published" }, "max_pages": 40
        })),
        "page with button \"Next\" until text \"Published\" show, at most 40 pages"
    );
    assert_eq!(
        said(serde_json::json!({
            "kind": "page_until", "next": { "role": "button", "name": "Next" },
            "until_all": [{ "text": "Published" }, { "text": "Unpublished" }], "max_pages": 5
        })),
        "page with button \"Next\" until text \"Published\" and text \"Unpublished\" show, at most 5 pages"
    );
}

#[test]
fn the_report_names_an_assistant_ruling_and_the_persons_words() {
    use v2_lib::autorun::divergences::{Divergence, DivergenceKind, DivergenceStatus, RuledBy, Ruling};
    use v2_lib::autorun::report::{build_with_extras, ReportExtras};
    let run = run_of(SHOT);
    let started: i64 = run.started_at.parse().unwrap();
    let list = vec![Divergence {
        id: "d1".into(),
        case_id: 201,
        step: 2,
        case_says: "4 levels".into(),
        app_shows: "6 levels".into(),
        kind: DivergenceKind::Data,
        reported_at: started.to_string(),
        status: DivergenceStatus::Ruled,
        ruling: Some(Ruling::CaseStands),
        ruled_at: Some(started.to_string()),
        case_fp: None,
        step_action: None,
        ruled_by: Some(RuledBy::Assistant),
        said: Some("a different cycle name is fine".into()),
        note: None,
        proposal: None,
        pbi: None,
    }];
    let html = build_with_extras(
        &run,
        &[],
        &Default::default(),
        "2 Oct 2026",
        &no_shots,
        &|_| None,
        &ReportExtras { divergences: &list },
    );
    let section = product_section_of(&html, 201);
    assert!(section.contains("ruled by the assistant"), "{section}");
    assert!(section.contains("a different cycle name is fine"), "{section}");
    assert!(section.contains("ruled: the app is wrong"), "{section}");
}

/// A run of the given cases (id, verdict), each with the given steps' outcomes empty.
fn run_with(cases: &[(i32, &str)]) -> LocalRun {
    let list: Vec<serde_json::Value> = cases
        .iter()
        .map(|(id, verdict)| {
            serde_json::json!({ "case_id": id, "title": format!("Case {id}"), "verdict": verdict, "note": "", "steps": [] })
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": "run-1786000300000", "pbi_id": 42, "started_at": "1786000300000", "mode": "unattended",
        "cases": list
    }))
    .unwrap()
}

/// A script whose step 1 lists `entries` as unchecked (none: no key).
fn script_with_unchecked(case_id: i32, entries: serde_json::Value) -> CaseScript {
    let mut step = serde_json::json!({ "step_number": 1, "actions": [{ "kind": "check_text", "value": "Saved" }] });
    if !entries.is_null() {
        step["unchecked"] = entries;
    }
    serde_json::from_value(serde_json::json!({ "case_id": case_id, "title": "t", "steps": [step] })).unwrap()
}

fn two_entries() -> serde_json::Value {
    serde_json::json!([
        { "item": "no lock icon", "why": "no check reads an icon" },
        { "item": "no denied message", "why": "no check reads words", "stand_in": "expect_hidden" }
    ])
}

#[test]
fn a_passed_case_with_unchecked_entries_says_how_many() {
    let run = run_with(&[(301, "Passed")]);
    let html = build(&run, &[script_with_unchecked(301, two_entries())], "x", &no_shots);
    assert!(html.contains("<span class=\"tone-pass\">Passed, 2 expectations unchecked</span>"), "{html}");
    assert!(html.contains("<p><strong>Unchecked:</strong> step 1: no lock icon - no check reads an icon</p>"), "{html}");
    assert!(
        html.contains("<p><strong>Unchecked:</strong> step 1: no denied message - no check reads words (stand-in: expect_hidden)</p>"),
        "{html}"
    );
}

#[test]
fn a_failed_case_shows_its_unchecked_items_but_no_count() {
    let run = run_with(&[(302, "Failed")]);
    let html = build(&run, &[script_with_unchecked(302, two_entries())], "x", &no_shots);
    assert!(html.contains("<span class=\"tone-fail\">Failed</span>"), "{html}");
    assert!(!html.contains("Failed, "), "{html}");
    assert_eq!(html.matches("<p><strong>Unchecked:</strong>").count(), 2, "{html}");
    assert!(!html.contains("class=\"unchecked\""), "{html}");
}

#[test]
fn the_summary_counts_unchecked_expectations_in_passed_cases() {
    let run = run_with(&[(303, "Passed"), (304, "Passed"), (305, "Passed"), (306, "Failed")]);
    let scripts = [
        script_with_unchecked(303, two_entries()),
        script_with_unchecked(304, serde_json::json!("the PDF cannot be read")),
        script_with_unchecked(305, serde_json::Value::Null),
        script_with_unchecked(306, two_entries()),
    ];
    let html = build(&run, &scripts, "x", &no_shots);
    assert!(html.contains("<p class=\"unchecked\">3 expectations unchecked in 2 passed cases</p>"), "{html}");
    let one = build(&run_with(&[(303, "Passed")]), &[script_with_unchecked(303, serde_json::json!("why"))], "x", &no_shots);
    assert!(one.contains("<p class=\"unchecked\">1 expectation unchecked in 1 passed case</p>"), "{one}");
}

#[test]
fn a_run_with_none_unchecked_reads_as_before() {
    let run = run_with(&[(307, "Passed"), (308, "Failed")]);
    let html = build(&run, &[script_with_unchecked(307, serde_json::Value::Null), script_with_unchecked(308, serde_json::Value::Null)], "x", &no_shots);
    assert!(!html.to_lowercase().contains("unchecked"), "{html}");
    assert!(html.contains("<span class=\"tone-pass\">Passed</span>"), "{html}");
}

#[test]
fn a_report_for_an_environment_that_was_left_is_refused_and_makes_no_folder() {
    use v2_lib::autorun::{roots, store};
    use v2_lib::environments::{self, Environment};
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            store::set_root(std::env::temp_dir().join("tcm-autorun-report-restored"));
        }
    }
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let dir = TempDir::new();
    store::set_shared_root(dir.path().to_path_buf());
    let a = environments::load_or_init(dir.path(), Some("dev-read")).unwrap().active;
    let qa = Environment {
        id: String::new(),
        name: "QA".into(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: "dev-read".into(),
        test_environment: false,
        test_prefix: "AUTOTEST".into(),
        db_auto_approve: false,
        password_expiry_query: String::new(),
        active_session_query: String::new(),
        reset_expired_passwords: false,
        account_lookup: Default::default(),
    };
    let b = environments::save_env(dir.path(), qa, &["dev-read".to_string()])
        .unwrap()
        .environments
        .into_iter()
        .find(|e| e.name == "QA")
        .unwrap()
        .id;
    store::select_environment(&a).unwrap();
    let ra = store::configured_root().unwrap();
    save_run(&ra, &run_of(SHOT)).unwrap();
    store::select_environment(&b).unwrap();

    let err = write_report_at(true, &ra, "Acme", "Web", "run-1786000200000", "x").unwrap_err();
    assert_eq!(err, roots::ENVIRONMENT_SWITCHED);
    assert!(!roots::env_root(dir.path(), &a).join("reports").exists());
}

/// A failure of a case whose script is marked flaky is counted as flaky, not
/// as failed, the way Past runs and the review count it. It stays a failure
/// in every other way: the case's result says Failed, its section is open
/// and shows the step that stopped it.
#[test]
fn a_flaky_marked_failure_is_counted_as_flaky_and_keeps_its_details() {
    use v2_lib::autorun::report::flaky_count;
    let run = run_of(SHOT);
    let mut marked = script_201();
    marked.flaky = Some(v2_lib::autorun::FlakyMark { marked_at: "1786000000000".into() });

    // Not marked: as before - one failed, no Flaky column.
    let plain = build(&run, &[script_201()], "", &no_shots);
    assert_eq!(flaky_count(&run, &[script_201()]), 0);
    let summary = |h: &str| h[h.find("<table class=\"summary\">").unwrap()..].split("</table>").next().unwrap().to_string();
    assert!(!summary(&plain).contains("<th>Flaky</th>") && !plain.contains("data-status=\"flaky\""), "{plain}");
    assert!(plain.contains("<td class=\"b-failed\">1</td>"), "{plain}");

    // Marked: the Failed column drops to 0 and the Flaky column holds the case.
    assert_eq!(flaky_count(&run, &[marked.clone()]), 1);
    let html = build(&run, &[marked], "", &no_shots);
    assert!(html.contains("<th>Not run</th><th>Flaky</th><th>Total</th>"), "{html}");
    assert!(html.contains("<td class=\"b-failed\">0</td>"), "{html}");
    assert!(html.contains("<td class=\"b-flaky\">1</td>"), "{html}");
    assert!(html.contains("<td>4</td></tr></tbody></table>"), "the total is every case: {html}");
    assert!(html.contains("marked as flaky, so it is not counted as failed above"), "{html}");
    // The case still reads Failed, flagged, printed in full, with its raw failure.
    let detail = article_of(&html, 201);
    assert!(detail.contains("<span class=\"tone-flaky\">Failed</span>"), "{detail}");
    assert!(detail.contains("<span class=\"signal\">Flaky</span>") && detail.contains("data-print=\"1\""), "{detail}");
    assert!(html.contains("button &quot;Save&quot; not found"), "{html}");
    // Only a failure is flaky: a mark on a passing case changes nothing.
    let mut mark_204: CaseScript =
        serde_json::from_value(serde_json::json!({ "case_id": 204, "title": "Logout", "steps": [] })).unwrap();
    mark_204.flaky = Some(v2_lib::autorun::FlakyMark { marked_at: "1".into() });
    assert_eq!(flaky_count(&run, &[mark_204]), 0);
}

/// Raise bug keeps the bug it filed on the run's case, so the review and
/// Past runs show it after the review closes. A record saved before the
/// field reads as having none, and the field is left out when empty.
#[test]
fn a_filed_bug_is_kept_on_the_cases_record() {
    use v2_lib::commands::autorun::record_bug_at;
    let tmp = TempDir::new();
    let run = run_of(SHOT);
    save_run(tmp.path(), &run).unwrap();

    // Nothing is written for a case with no bug: old files stay as they were.
    let before = std::fs::read_to_string(tmp.path().join("runs").join("run-1786000200000.json")).unwrap();
    assert!(!before.contains("\"bugs\""), "{before}");

    record_bug_at(tmp.path(), "run-1786000200000", 201, 777, "org", "Proj").unwrap();
    // Filing it again records it once; another bug is added.
    record_bug_at(tmp.path(), "run-1786000200000", 201, 777, "org", "Proj").unwrap();
    record_bug_at(tmp.path(), "run-1786000200000", 201, 778, "org", "Proj").unwrap();

    let back = v2_lib::autorun::store::load_run(tmp.path(), "run-1786000200000").unwrap().unwrap();
    let case = back.cases.iter().find(|c| c.case_id == 201).unwrap();
    let ids: Vec<i32> = case.bugs.iter().map(|b| b.id).collect();
    assert_eq!(ids, vec![777, 778]);
    assert_eq!((case.bugs[0].org.as_str(), case.bugs[0].project.as_str()), ("org", "Proj"));
    assert!(back.cases.iter().filter(|c| c.case_id != 201).all(|c| c.bugs.is_empty()));
    // Everything else about the run is as it was.
    assert_eq!(back.cases.len(), run.cases.len());
    assert_eq!(back.cases[0].proposed, "Failed");

    // The saved record reads back through the listing the screens use.
    let listed = v2_lib::autorun::store::list_runs(tmp.path());
    assert_eq!(listed[0].cases[0].bugs.len(), 2);

    // A record from before the field has no `bugs` key and reads as none.
    let old: LocalRun = serde_json::from_str(&before).unwrap();
    assert!(old.cases.iter().all(|c| c.bugs.is_empty()));

    // A run that is gone, a case it does not hold, and a nonsense number are refused.
    assert!(record_bug_at(tmp.path(), "run-1", 201, 1, "o", "p").is_err());
    assert!(record_bug_at(tmp.path(), "run-1786000200000", 999, 1, "o", "p").is_err());
    assert!(record_bug_at(tmp.path(), "run-1786000200000", 201, 0, "o", "p").is_err());
}

/// A run already sent to Azure DevOps still takes the bug, and stays sent.
#[test]
fn a_bug_can_be_recorded_on_a_run_that_was_already_sent() {
    use v2_lib::commands::autorun::record_bug_at;
    let tmp = TempDir::new();
    let mut run = run_of(SHOT);
    run.published = Some(serde_json::from_value(serde_json::json!({
        "run_id": 9, "web_url": "https://dev.azure.com/o/p/_testManagement/runs?runId=9", "at": "1786000300000"
    })).unwrap());
    save_run(tmp.path(), &run).unwrap();
    record_bug_at(tmp.path(), "run-1786000200000", 201, 31, "o", "p").unwrap();
    let back = v2_lib::autorun::store::load_run(tmp.path(), "run-1786000200000").unwrap().unwrap();
    assert!(back.published.is_some());
    assert_eq!(back.cases[0].bugs.len(), 1);
}

/// The flaky rule is held to one table, tests/fixtures/flaky_buckets.json, which the Auto Test
/// screens' test reads too (`isFlakyFailure`), so the report and the screens count alike.
#[test]
fn the_flaky_rule_matches_the_table_the_screens_are_held_to() {
    use v2_lib::autorun::report::{case_bucket, is_flaky_failure};
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flaky_buckets.json"),
    )
    .unwrap();
    let table: serde_json::Value = serde_json::from_str(&text).unwrap();
    let rows = table["cases"].as_array().unwrap();
    assert!(rows.len() > 5);
    for row in rows {
        let case: v2_lib::autorun::CaseRecord = serde_json::from_value(serde_json::json!({
            "case_id": 1, "title": "t", "note": "", "steps": [],
            "verdict": row["verdict"], "proposed": row["proposed"],
        }))
        .unwrap();
        let mut script: CaseScript =
            serde_json::from_value(serde_json::json!({ "case_id": 1, "title": "t", "steps": [] })).unwrap();
        if row["marked"].as_bool().unwrap() {
            script.flaky = Some(v2_lib::autorun::FlakyMark { marked_at: "1".into() });
        }
        let got = if is_flaky_failure(&case, Some(&script)) { "flaky".to_string() } else { case_bucket(&case).to_lowercase() };
        assert_eq!(got, row["counts_as"].as_str().unwrap(), "{row}");
    }
}

#[test]
fn a_passed_cases_unchecked_count_leaves_out_the_steps_done_by_auto_run() {
    let script: CaseScript = serde_json::from_value(serde_json::json!({ "case_id": 309, "title": "t", "steps": [
        { "step_number": 1, "actions": [], "by_auto_run": true, "unchecked": "Auto Test opens the app" },
        { "step_number": 2, "actions": [], "unchecked": "the PDF cannot be read" }
    ] }))
    .unwrap();
    let html = build(&run_with(&[(309, "Passed")]), &[script], "x", &no_shots);
    assert!(html.contains("<span class=\"tone-pass\">Passed, 1 expectation unchecked</span>"), "{html}");
    assert!(!html.contains("Auto Test opens the app"), "{html}");
    assert!(html.contains("<p class=\"unchecked\">1 expectation unchecked in 1 passed case</p>"), "{html}");
}
