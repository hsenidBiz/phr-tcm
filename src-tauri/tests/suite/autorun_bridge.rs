//! The bridge routes an AI assistant reaches through the MCP proxy: read
//! the action-script format, look at the page in the browser the person
//! opened, try a locator or a single action, read a run's failures,
//! record a quirk - and save scripts back under the expected-result
//! floor, the declared-edit gate and the repair cap.
//!
//! Only one of these routes reaches Azure DevOps, and only to read: the
//! save route looks the test cases up so the floor has something to
//! check the script against. Driving the browser never does.

use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{autorun_guard_for, autorun_route_guard, describe_try, route, BridgeContext};
use v2_lib::autorun::guide::autorun_guide;
use v2_lib::autorun::quirks::load_quirks;
use v2_lib::autorun::store::{load_script, save_run, save_scripts_atomically, set_root};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepRecord};
use v2_lib::browser::actions::{Action, ActionOutcome};
use v2_lib::steps_xml::{build_steps_xml, Step};
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

/// A real organization and project: the save route looks its cases up
/// under `ctx.org`, and a quirk is filed under the pair.
fn ctx() -> BridgeContext {
    BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() }
}

// The store root is process-wide state, and cargo runs tests in parallel -
// every test that sets it holds `crate::serial::autorun()` so one test's
// root cannot swap out from under another mid-save. The same lock covers
// the run and recording claims other modules take, which the page routes
// answer from.

/// A signed-in client standing in for Azure DevOps. Each entry is a case
/// id, its title, and one expected result per step - all the floor reads.
/// The work-items-by-ids call is the ONLY request the save route makes.
async fn client_with_cases(cases: &[(i32, &str, &[&str])]) -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let value: Vec<serde_json::Value> = cases
        .iter()
        .map(|(id, title, expected)| {
            let steps: Vec<Step> = expected
                .iter()
                .enumerate()
                .map(|(i, e)| Step { action: format!("Step {}", i + 1), expected: (*e).to_string(), shared: None })
                .collect();
            serde_json::json!({
                "id": id,
                "fields": {
                    "System.Title": title,
                    "Microsoft.VSTS.TCM.Steps": build_steps_xml(&steps),
                }
            })
        })
        .collect();
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": value })),
        )
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

/// Case 7 as every save test below writes it: a navigation step with
/// nothing to assert, then a step that clicks Save and checks the toast.
fn case_7(selector: &str, value: &str) -> serde_json::Value {
    serde_json::json!([{
        "case_id": 7,
        "title": "Save a rating",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings" }] },
            { "step_number": 2, "actions": [
                { "kind": "click", "selector": "#save" },
                { "kind": "expect_contains_text", "selector": selector, "value": value }
            ]}
        ]
    }])
}

/// The declaration that goes with a change to case 7's step 2.
fn edit_step_2(why: &str) -> serde_json::Value {
    serde_json::json!({ "case_id": 7, "steps": [2], "why": why })
}

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
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(201, "Valid login", &[""]), (202, "Locked account", &[""])]).await;

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

    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
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
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Only one", &[""])]).await;
    let body = serde_json::json!([{
        "case_id": 7,
        "title": "Only one",
        "steps": [{ "step_number": 1, "actions": [] }]
    }])
    .to_string();

    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().title, "Only one");
}

/// An unknown action kind must be refused at the door. Saving it would
/// hand the tester a script that dies mid-run, in front of them, with
/// the browser already open. Parsing is the FIRST gate, before the
/// sign-in check, which is why this one needs no client.
#[tokio::test]
async fn an_unknown_action_kind_is_refused_and_nothing_is_written() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
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
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", "{ not json", "1.0.0").await;
    assert_eq!(status, 400);
    assert!(!out.is_empty(), "a 400 with no explanation");
}

/// This is a PARSE-gate test, not an atomicity test: `"kind": "nope"` is
/// an action tag serde does not know, so the whole body fails to read
/// before the write loop ever starts - the good case was never going to
/// be written regardless of whether that loop is atomic. It would pass
/// even if `store::save_scripts_atomically` wrote every entry it reached
/// with no rollback at all. See
/// `a_write_failure_mid_bundle_leaves_nothing_behind` below for a test
/// that actually exercises atomicity.
#[tokio::test]
async fn a_bundle_that_fails_to_parse_writes_nothing() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
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
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(11, "Fine", &[""]), (12, "Blocked", &[""])]).await;

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

    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 500, "{out}");
    assert!(
        load_script(dir.path(), 11).unwrap().is_none(),
        "case 11 was written even though case 12 in the same bundle could not be"
    );
}

// --------------------------------------------------- the floor on a save

/// A brand new script declares nothing - there is no earlier version to
/// declare a change to - but it still has to check what its case says it
/// should, or say why it does not.
#[tokio::test]
async fn a_new_script_needs_no_declaration_but_must_meet_the_floor() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let checks_nothing = serde_json::json!([{
        "case_id": 7,
        "title": "Save a rating",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings" }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": "#save" }] }
        ]
    }])
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &checks_nothing, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("step 2 expects"), "{out}");
    assert!(load_script(dir.path(), 7).unwrap().is_none(), "a script below the floor was written");

    // `repairs` is never the sender's to set: a bundle claiming 99 lands
    // as a new script at 0, and a declared repair lands at one more than
    // the file had - not at whatever the body asked for. A sender that
    // could write this field could also write it back to zero, which is
    // the whole of what the cap prevents.
    let mut body = case_7("#toast", "Saved");
    body[0]["repairs"] = serde_json::json!(99);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body.to_string(), "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out.lines().next().unwrap(), "saved 1 script(s): case 7 (new)");
    let saved = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(saved.repairs, 0, "a new script has taken no repairs");

    let mut repaired = case_7(".toast", "Saved");
    repaired[0]["repairs"] = serde_json::json!(99);
    let declared = serde_json::json!({
        "scripts": repaired,
        "edits": [edit_step_2("the toast has no id, only a class")],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &declared, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out, "saved 1 script(s): case 7 (repaired, 1 of 3 used)");
    assert_eq!(
        load_script(dir.path(), 7).unwrap().unwrap().repairs,
        1,
        "the count came from the file, not the body"
    );
}

/// Saying WHY a step cannot be checked is the other way past the floor -
/// it leaves the gap visible in the file instead of silently absent.
#[tokio::test]
async fn an_unchecked_step_with_a_reason_passes_the_floor() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let body = serde_json::json!([{
        "case_id": 7,
        "title": "Save a rating",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings" }] },
            {
                "step_number": 2,
                "actions": [{ "kind": "click", "selector": "#save" }],
                "unchecked": "the toast vanishes too fast to read"
            }
        ]
    }])
    .to_string();

    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let saved = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(saved.steps[1].unchecked.as_deref(), Some("the toast vanishes too fast to read"));
}

/// The floor needs the test case, and the test case comes from Azure
/// DevOps - so with nobody signed in there is nothing to check against
/// and the save is refused before a single byte is written. 503, like
/// every other route that needs a signed-in client: nothing about the
/// bundle is wrong, the app just cannot check it yet.
#[tokio::test]
async fn saving_without_signing_in_is_refused_before_anything_is_written() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let body = case_7("#toast", "Saved").to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 503, "{out}");
    assert!(out.contains("sign in to Test Case Manager first"), "{out}");
    assert!(out.contains("checked against its test case"), "{out}");
    assert!(load_script(dir.path(), 7).unwrap().is_none(), "a script was written anyway");
}

/// A bundle bigger than the same cap `/test-cases` already applies to its
/// own `ids` list is refused before the floor even looks anything up -
/// disk or Azure DevOps.
#[tokio::test]
async fn a_bundle_over_the_case_cap_is_refused_before_any_lookup() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let too_many: Vec<serde_json::Value> = (1..=201)
        .map(|id| {
            serde_json::json!({
                "case_id": id,
                "title": "t",
                "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }]
            })
        })
        .collect();
    let body = serde_json::Value::Array(too_many).to_string();
    // No client at all - if this reached the floor's lookup it would 503
    // instead, so a 400 here proves the cap runs first.
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "a bundle can carry at most 200 scripts");
    assert!(load_script(dir.path(), 1).unwrap().is_none(), "a script was written anyway");
}

/// A case id the organization does not know is a mistake worth naming -
/// a script saved against it would never be runnable.
#[tokio::test]
async fn a_case_the_organization_does_not_have_is_named() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    // The lookup answers with nothing at all for case 7.
    let (_server, client) = client_with_cases(&[]).await;

    let body = case_7("#toast", "Saved").to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "case 7 is not a test case in this organization");
    assert!(load_script(dir.path(), 7).unwrap().is_none());
}

// ------------------------------------------ the declared-edit gate on a save

/// Changing a script that already exists is a REPAIR: every step touched
/// has to be named, an assertion may never go, and the count of repairs
/// taken since a person last saved it goes up by one.
#[tokio::test]
async fn an_edit_must_be_declared_and_a_check_may_not_go() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let first = case_7("#toast", "Saved").to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    // Step 2's locator moved, and nothing said so.
    let undeclared = serde_json::json!({ "scripts": case_7(".toast", "Saved") }).to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &undeclared, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("step 2 was changed but not declared"), "{out}");
    assert_eq!(
        load_script(dir.path(), 7).unwrap().unwrap().repairs,
        0,
        "a refused repair must not have counted"
    );

    // The same change, declared.
    let declared = serde_json::json!({
        "scripts": case_7(".toast", "Saved"),
        "edits": [edit_step_2("the toast has no id, only a class")],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &declared, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out.lines().next().unwrap(), "saved 1 script(s): case 7 (repaired, 1 of 3 used)");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 1);

    // Declared or not, the check itself may never be dropped.
    let weakened = serde_json::json!({
        "scripts": [{
            "case_id": 7,
            "title": "Save a rating",
            "steps": [
                { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings" }] },
                { "step_number": 2, "actions": [{ "kind": "click", "selector": "#save" }] }
            ]
        }],
        "edits": [edit_step_2("the toast never appears in my run")],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &weakened, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("an assertion is never removed"), "{out}");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 1, "still one repair in");
}

/// A repair's `why` is persisted on the script itself, not just in the
/// applog, so a person opening the editor can see it without hunting
/// through Settings -> Logs. An unchanged re-send keeps the last repair's
/// reason exactly as it was.
#[tokio::test]
async fn a_repairs_reason_is_persisted_and_survives_an_unchanged_resend() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let first = case_7("#toast", "Saved").to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().last_repair, None, "a new script has no repair yet");

    let declared = serde_json::json!({
        "scripts": case_7(".toast", "Saved"),
        "edits": [edit_step_2("the toast has no id, only a class")],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &declared, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(
        load_script(dir.path(), 7).unwrap().unwrap().last_repair.as_deref(),
        Some("the toast has no id, only a class")
    );

    // Re-sending the same script unchanged keeps the reason exactly as it
    // was - it did not repair anything this time.
    let resend = case_7(".toast", "Saved").to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &resend, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(
        load_script(dir.path(), 7).unwrap().unwrap().last_repair.as_deref(),
        Some("the toast has no id, only a class")
    );
}

/// Three repairs without a person looking is the cap. Saving the script
/// from the app's own editor (which writes `repairs: 0`) is what starts
/// the count again.
#[tokio::test]
async fn the_fourth_repair_is_refused_until_a_person_saves() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let (status, out) = route(
        &ctx(),
        Some(&client),
        "POST",
        "/autorun-script",
        &case_7("#toast", "v0").to_string(),
        "1.0.0",
    )
    .await;
    assert_eq!(status, 200, "{out}");

    for (n, value) in [(1u32, "v1"), (2, "v2"), (3, "v3")] {
        let body = serde_json::json!({
            "scripts": case_7("#toast", value),
            "edits": [edit_step_2("the toast wording changed again")],
        })
        .to_string();
        let (status, out) =
            route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
        assert_eq!(status, 200, "repair {n}: {out}");
        assert!(out.contains(&format!("repaired, {n} of 3 used")), "repair {n}: {out}");
        assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, n);
    }

    let fourth = serde_json::json!({
        "scripts": case_7("#toast", "v4"),
        "edits": [edit_step_2("one more try")],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &fourth, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("repaired 3 times without a person looking at it"), "{out}");
    let on_disk = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(on_disk.repairs, 3, "the refused fourth repair changed nothing");
    assert_eq!(on_disk.steps[1].actions.len(), 2);

    // What the app's editor does when the person saves: the same script,
    // written straight to the store with the count back at zero.
    let reset = CaseScript { repairs: 0, ..on_disk };
    save_scripts_atomically(dir.path(), &[reset]).unwrap();

    let again = serde_json::json!({
        "scripts": case_7("#toast", "v5"),
        "edits": [edit_step_2("the toast wording changed once more")],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &again, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("repaired, 1 of 3 used"), "{out}");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 1);
}

/// What an assistant learned while repairing is worth more than the
/// repair: it travels with the declaration and lands in the project's
/// quirks, attributed, so the next script does not rediscover it.
#[tokio::test]
async fn a_quirk_travels_with_the_edit() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let (status, out) = route(
        &ctx(),
        Some(&client),
        "POST",
        "/autorun-script",
        &case_7("#toast", "Saved").to_string(),
        "1.0.0",
    )
    .await;
    assert_eq!(status, 200, "{out}");

    let body = serde_json::json!({
        "scripts": case_7(".toast", "Saved"),
        "edits": [{
            "case_id": 7,
            "steps": [2],
            "why": "the toast has no id, only a class",
            "quirk": "the toast is rendered into a portal at the end of the body",
        }],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let quirks = load_quirks(dir.path(), "acme", "Web").unwrap();
    assert!(
        out.contains(&format!(
            "quirk recorded as {}: the toast is rendered into a portal at the end of the body",
            quirks[0].id
        )),
        "{out}"
    );
    assert_eq!(quirks.len(), 1);
    assert_eq!(quirks[0].text, "the toast is rendered into a portal at the end of the body");
    assert_eq!(quirks[0].by, "assistant");
    // It remembers the repair it came with, so later runs can say whether
    // it helped. No run of case 7 is on this machine: no class to keep.
    assert_eq!(
        quirks[0].sources,
        vec![v2_lib::autorun::quirks::QuirkSource { case_id: 7, steps: vec![2], class: None }]
    );

    // The same quirk a second time is not written twice, and the report
    // says so rather than pretending something new was learned.
    let repeat = serde_json::json!({
        "scripts": case_7("#toast", "Saved"),
        "edits": [{
            "case_id": 7,
            "steps": [2],
            "why": "the id came back",
            "quirk": "The toast is rendered into a portal at the end of the body",
        }],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &repeat, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("quirk already known, as "), "{out}");
    assert_eq!(load_quirks(dir.path(), "acme", "Web").unwrap().len(), 1);
}

/// Case 9, the second case of the two-case bundle below. Its expected
/// result is checked, so it clears the floor on its own.
fn case_9(value: &str) -> serde_json::Value {
    serde_json::json!({
        "case_id": 9,
        "title": "Delete a rating",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings" }] },
            { "step_number": 2, "actions": [
                { "kind": "click", "selector": "#delete" },
                { "kind": "expect_contains_text", "selector": "#toast", "value": value }
            ]}
        ]
    })
}

/// Saving a whole PBI again after fixing ONE case sends every other case
/// back exactly as it was. That is not a repair of those cases, so it
/// costs nothing from their count and can never run into the cap - which
/// would otherwise lock a bundle after three fixes to any case in it.
#[tokio::test]
async fn an_unchanged_script_costs_no_repair() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[
        (7, "Save a rating", &["", "A toast says Saved"]),
        (9, "Delete a rating", &["", "A toast says Deleted"]),
    ])
    .await;

    let both = |seven: &str, nine: &str| {
        serde_json::json!([case_7("#toast", seven)[0].clone(), case_9(nine)])
    };
    let (status, out) = route(
        &ctx(),
        Some(&client),
        "POST",
        "/autorun-script",
        &both("Saved", "Deleted").to_string(),
        "1.0.0",
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out, "saved 2 script(s): case 7 (new), case 9 (new)");

    // Case 7 is repaired; case 9 comes back word for word.
    let body = serde_json::json!({
        "scripts": both("Saved!", "Deleted"),
        "edits": [edit_step_2("the toast gained an exclamation mark")],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(
        out,
        "saved 2 script(s): case 7 (repaired, 1 of 3 used), case 9 (unchanged)"
    );
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 1);
    assert_eq!(load_script(dir.path(), 9).unwrap().unwrap().repairs, 0, "case 9 paid nothing");

    // Declaring an edit for a case nothing happened to is still refused -
    // that is rule 2, and skipping the gate must not skip it too.
    let over_declared = serde_json::json!({
        "scripts": both("Saved!", "Deleted"),
        "edits": [{ "case_id": 9, "steps": [2], "why": "I thought I changed this" }],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &over_declared, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("declared but not changed"), "{out}");

    // Case 7 all the way to the cap, then four identical re-sends of it.
    for value in ["Saved!!", "Saved!!!"] {
        let body = serde_json::json!({
            "scripts": case_7("#toast", value),
            "edits": [edit_step_2("the toast wording moved again")],
        })
        .to_string();
        let (status, out) =
            route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
        assert_eq!(status, 200, "{out}");
    }
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 3, "at the cap");

    for round in 1..=4 {
        let (status, out) = route(
            &ctx(),
            Some(&client),
            "POST",
            "/autorun-script",
            &case_7("#toast", "Saved!!!").to_string(),
            "1.0.0",
        )
        .await;
        assert_eq!(status, 200, "re-send {round}: {out}");
        assert_eq!(out, "saved 1 script(s): case 7 (unchanged)", "re-send {round}");
        assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 3, "re-send {round}");
    }
}

/// A declaration naming a case the bundle is not saving usually means a
/// case id was typed wrong into one of the two lists. Ignoring it would
/// still file its quirk while changing nothing, so it is named back and
/// the whole call is refused.
#[tokio::test]
async fn an_edit_for_a_case_outside_the_bundle_is_refused() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let body = serde_json::json!({
        "scripts": case_7("#toast", "Saved"),
        "edits": [{
            "case_id": 11,
            "steps": [2],
            "why": "fixing the locator",
            "quirk": "the toast is rendered into a portal",
        }],
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "edits names case 11, which is not in this bundle");
    assert!(load_script(dir.path(), 7).unwrap().is_none());
    assert!(load_quirks(dir.path(), "acme", "Web").unwrap().is_empty(), "a stray edit filed a quirk");

    let two = serde_json::json!({
        "scripts": case_7("#toast", "Saved"),
        "edits": [
            { "case_id": 13, "steps": [2], "why": "one" },
            { "case_id": 11, "steps": [2], "why": "two" },
        ],
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &two, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "edits names cases 11, 13, which are not in this bundle");
}

/// A repair may change what a step does; it may never change the ORDER
/// the runner executes them in - `replay::run_case` runs `steps` in Vec
/// order, so a reorder is a behaviour change the signature comparison
/// alone would miss. Refused before anything is written, declared or not.
#[tokio::test]
async fn reordering_the_steps_writes_nothing() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let first = case_7("#toast", "Saved").to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let before = load_script(dir.path(), 7).unwrap().unwrap();

    // Same two steps, same content, just written in the opposite order.
    let reordered = serde_json::json!([{
        "case_id": 7,
        "title": "Save a rating",
        "steps": [
            { "step_number": 2, "actions": [
                { "kind": "click", "selector": "#save" },
                { "kind": "expect_contains_text", "selector": "#toast", "value": "Saved" }
            ]},
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings" }] }
        ]
    }])
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &reordered, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "the steps are in a different order - a repair does not reorder a script");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap(), before, "a refused reorder wrote something");

    // Declaring both steps does not excuse it either - a reorder is
    // refused outright, not something `edits` can sign off on.
    let declared = serde_json::json!({
        "scripts": [{
            "case_id": 7,
            "title": "Save a rating",
            "steps": [
                { "step_number": 2, "actions": [
                    { "kind": "click", "selector": "#save" },
                    { "kind": "expect_contains_text", "selector": "#toast", "value": "Saved" }
                ]},
                { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings" }] }
            ]
        }],
        "edits": [{ "case_id": 7, "steps": [1, 2], "why": "reordered for readability" }],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &declared, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "the steps are in a different order - a repair does not reorder a script");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap(), before, "a refused reorder wrote something");
}

/// A declaration whose `steps` list is empty carries only a quirk, no
/// repair. Against a script that did not change, that is exactly what it
/// claims to be and goes through as `(unchanged)`; against one that DID
/// change, rule 1 still refuses it - an empty list cannot cover a real
/// change.
#[tokio::test]
async fn a_quirk_only_declaration_is_pinned_for_unchanged_and_refused_for_changed() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let first = case_7("#toast", "Saved").to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    // Unchanged script, quirk-only declaration: goes through as
    // unchanged, repairs untouched, quirk recorded.
    let unchanged = serde_json::json!({
        "scripts": case_7("#toast", "Saved"),
        "edits": [{
            "case_id": 7,
            "steps": [],
            "why": "nothing changed, just noting this",
            "quirk": "the toast always says exactly Saved",
        }],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &unchanged, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out.lines().next().unwrap(), "saved 1 script(s): case 7 (unchanged)");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 0);
    let quirks = load_quirks(dir.path(), "acme", "Web").unwrap();
    assert_eq!(quirks.len(), 1);
    assert_eq!(quirks[0].text, "the toast always says exactly Saved");

    // Changed script, same empty-steps declaration: rule 1 refuses it -
    // an empty `edits.steps` names nothing, so the real change is left
    // undeclared.
    let changed = serde_json::json!({
        "scripts": case_7(".toast", "Saved"),
        "edits": [{
            "case_id": 7,
            "steps": [],
            "why": "the locator moved",
            "quirk": "a new quirk",
        }],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &changed, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("step 2 was changed but not declared"), "{out}");
}

/// A declaration for a case with no script on disk is a mistake, not a
/// no-op: the assistant thinks it is repairing something that is not
/// there.
#[tokio::test]
async fn declaring_an_edit_for_a_case_with_no_script_is_refused() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let body = serde_json::json!({
        "scripts": case_7("#toast", "Saved"),
        "edits": [edit_step_2("fixing the locator")],
    })
    .to_string();
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "case 7 has no script yet - \"edits\" is for changing one that exists");
    assert!(load_script(dir.path(), 7).unwrap().is_none());
}

/// A body key nobody reads is named back, not dropped in silence - the
/// same rule `transform_cases` follows. A misspelled "edits" that was
/// quietly ignored would save an undeclared repair.
#[tokio::test]
async fn unknown_body_keys_are_named_not_ignored() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let body = serde_json::json!({
        "scripts": case_7("#toast", "Saved"),
        "edit": [edit_step_2("misspelled")],
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("\"edit\""), "the key is not named: {out}");
    assert!(out.contains("scripts") && out.contains("edits"), "{out}");
    assert!(load_script(dir.path(), 7).unwrap().is_none());
}

// ------------------------------------------------------- the page routes

/// Every route that touches the browser needs one the PERSON opened.
/// These tests open none, so the session slot is empty and all three
/// answer the same way - which is exactly what an assistant sees when it
/// reaches for the page before anyone has opened a browser.
#[tokio::test]
async fn page_routes_need_the_supervised_browser() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let expected = "no supervised browser is open - the person opens one with Open browser on the Auto Run tab";

    let (status, out) = route(&ctx(), None, "GET", "/autorun-page", "", "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out, expected);

    let probe = serde_json::json!({ "selector": "#save" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-probe", &probe, "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out, expected);

    let try_body =
        serde_json::json!({ "action": { "kind": "click", "selector": "#save" } }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-try", &try_body, "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out, expected);
}

/// A locator that could never match anything is refused before the
/// browser is asked - the same sentence `Target::validate` gives a script
/// at save time.
#[tokio::test]
async fn probe_refuses_a_locator_that_cannot_be_read() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let body = serde_json::json!({ "selector": "" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-probe", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "a selector is empty");

    let (status, out) = route(&ctx(), None, "POST", "/autorun-probe", "{}", "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("selector"), "{out}");
}

/// Signing in is the person's job: it needs their accounts and the
/// project's recipe, and no assistant ever drives it. An action the
/// runner could not carry out is refused before the browser is asked too.
#[tokio::test]
async fn try_refuses_a_sign_in_and_an_invalid_action() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let sign_in =
        serde_json::json!({ "action": { "kind": "sign_in", "account": "tester" } }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-try", &sign_in, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "sign_in is not a thing an assistant does - the person signs in");

    let bad_url =
        serde_json::json!({ "action": { "kind": "navigate", "url": "javascript:alert(1)" } })
            .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-try", &bad_url, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("navigate needs an http"), "{out}");

    let unknown = serde_json::json!({ "action": { "kind": "teleport" } }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-try", &unknown, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("teleport"), "{out}");
}

/// The applog line for a tried action never carries a `fill`'s VALUE -
/// only its selector, the same as every other selector-carrying kind.
/// Pure, so this is provable without a browser at all.
#[test]
fn describe_try_never_carries_a_fills_value() {
    let action = Action::Fill { selector: "#password".into(), value: "hunter2".to_string() };
    let out = describe_try(&action, true);
    assert!(!out.contains("hunter2"), "{out}");
    assert_eq!(out, "AI tried fill #password in the supervised browser: ok");
}

#[test]
fn describe_try_drops_an_api_checks_query_string_and_fragment() {
    let watch: Action = serde_json::from_value(serde_json::json!({
        "kind": "expect_response", "url_contains": "/hr/Cycle/Save?access_token=abc#top"
    }))
    .unwrap();
    assert_eq!(describe_try(&watch, true), "AI tried expect_response /hr/Cycle/Save in the supervised browser: ok");
    let ask: Action = serde_json::from_value(serde_json::json!({ "kind": "api_request", "path": "/api/me?token=abc#x" })).unwrap();
    assert_eq!(describe_try(&ask, false), "AI tried api_request /api/me in the supervised browser: failed");
}

#[test]
fn describe_try_names_the_kind_the_target_and_whether_it_worked() {
    let navigate = Action::Navigate { url: "https://app.example/ratings".to_string() };
    assert_eq!(
        describe_try(&navigate, true),
        "AI tried navigate https://app.example/ratings in the supervised browser: ok"
    );

    let click = Action::Click { selector: serde_json::from_value(serde_json::json!({ "role": "button", "name": "Save" })).unwrap() };
    assert_eq!(
        describe_try(&click, false),
        "AI tried click button \"Save\" in the supervised browser: failed"
    );
}

/// A saved script may navigate to `file://` (the live fixture is a local
/// file), but a TRIED action runs against the person's real, open browser
/// - sending it to a local file is never something a rehearsal should do,
/// however the case is written or trimmed.
#[tokio::test]
async fn try_refuses_a_file_navigate() {
    for url in ["file:///etc/passwd", "  FILE://C:/secrets.txt"] {
        let body = serde_json::json!({ "action": { "kind": "navigate", "url": url } }).to_string();
        let (status, out) = route(&ctx(), None, "POST", "/autorun-try", &body, "1.0.0").await;
        assert_eq!(status, 400, "{out}");
        assert_eq!(out, "a tried navigate goes to http or https only");
    }
}

// --------------------------------------------------------- the read routes

fn failed_run(id: &str, case_id: i32) -> LocalRun {
    LocalRun {
        id: id.to_string(),
        pbi_id: 42,
        started_at: "1700000000000".to_string(),
        cases: vec![CaseRecord {
            case_id,
            title: "Save a rating".to_string(),
            verdict: "Failed".to_string(),
            note: String::new(),
            steps: vec![StepRecord {
                step_number: 2,
                outcomes: vec![ActionOutcome::failed("nothing on the page answers to #toast")],
                screenshot: None,
            }],
            proposed: String::new(),
            reason: String::new(),
            duration_ms: None,
            account: None,
        }],
        mode: String::new(),
        published: None,
        environment: None,
    }
}

/// An assistant asks by case and gets the newest run that holds it; a
/// case nobody has ever run is a 404 that says so rather than an empty
/// report that reads like "nothing failed".
#[tokio::test]
async fn failures_are_read_from_the_latest_run() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    save_run(dir.path(), &failed_run("run-1700000000000", 7)).unwrap();

    let (status, out) =
        route(&ctx(), None, "GET", "/autorun-failures?case_id=7", "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("## Case 7"), "{out}");
    assert!(out.contains("nothing on the page answers to #toast"), "{out}");

    let (status, out) =
        route(&ctx(), None, "GET", "/autorun-failures?case_id=999", "", "1.0.0").await;
    assert_eq!(status, 404, "{out}");
    assert_eq!(out, "no run on this machine has case 999");

    let (status, out) = route(
        &ctx(),
        None,
        "GET",
        "/autorun-failures?run_id=run-1700000000000",
        "",
        "1.0.0",
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("## Case 7"), "{out}");

    let (status, out) =
        route(&ctx(), None, "GET", "/autorun-failures?run_id=run-9", "", "1.0.0").await;
    assert_eq!(status, 404, "{out}");
    assert_eq!(out, "no run run-9");

    // A case_id that was sent but is not a number is a mistake worth
    // naming: read as "no case given" it would answer about the newest
    // run instead, which is a different question with a plausible answer.
    let (status, out) =
        route(&ctx(), None, "GET", "/autorun-failures?case_id=seven", "", "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "case_id must be a number");

    // A run file that cannot be read is not a run that is not there, and
    // the store's own message for it names the file's path - which is
    // nothing the reader can act on.
    std::fs::write(dir.path().join("runs").join("run-bad.json"), "{ not a run").unwrap();
    let (status, out) =
        route(&ctx(), None, "GET", "/autorun-failures?run_id=run-bad", "", "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "the run file could not be read");
}

/// A quirk can be recorded on its own, not only alongside a repair - and
/// one already on the list is not written twice. The answer names its id.
#[tokio::test]
async fn a_quirk_can_be_recorded_on_its_own() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let body = serde_json::json!({ "text": "the grid paginates at 25 rows" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let quirks = load_quirks(dir.path(), "acme", "Web").unwrap();
    assert_eq!(out, format!("recorded as {}", quirks[0].id));

    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out, format!("already known, as {}", quirks[0].id));

    let quirks = load_quirks(dir.path(), "acme", "Web").unwrap();
    assert_eq!(quirks.len(), 1);
    assert_eq!(quirks[0].by, "assistant");
    assert_eq!(quirks[0].from, "autorun");

    let empty = serde_json::json!({ "text": "   " }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &empty, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(!out.is_empty());

    let bad_from = serde_json::json!({ "text": "x", "from": "elsewhere" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &bad_from, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
}

/// The guide's constant only says a quirks section exists - the route is
/// what actually appends the real one, read fresh from what this project
/// has recorded. With nothing recorded, the route adds nothing at all.
#[tokio::test]
async fn the_route_appends_the_projects_quirks() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let (status, body) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert_eq!(status, 200);
    // Nothing recorded adds no quirks - only the active environment, which is
    // the app's and not the project's.
    assert!(body.starts_with(&autorun_guide()), "{body}");
    assert!(!body.contains("## Known quirks of this application\n\n"), "an empty quirks list must add nothing: {body}");

    let saved = serde_json::json!({ "text": "the grid paginates at 25 rows" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &saved, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let id = load_quirks(dir.path(), "acme", "Web").unwrap()[0].id.clone();

    let (status, body) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert_eq!(status, 200);
    assert!(body.starts_with(&autorun_guide()), "the guide's own text must survive unchanged");
    assert!(body.contains("## Known quirks of this application\n\n"), "{body}");
    assert!(
        body.contains(&format!("- [{id}] (assistant) the grid paginates at 25 rows\n")),
        "the recorded quirk never reached the guide: {body}"
    );
}

/// The retire route: an assistant's own note goes, with its reason, and
/// leaves both guides; a replacement takes its place and its sources; a
/// person's note is refused in so many words.
#[tokio::test]
async fn the_assistant_retires_its_own_quirk_and_never_a_persons() {
    use v2_lib::autorun::quirks::{save_quirks, Quirk, QuirkSource, PERSON_NOTE};
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let mut mine = Quirk::new("the grid needs a second click", "assistant", "autorun", 1);
    mine.sources = vec![QuirkSource { case_id: 7, steps: vec![2], class: Some("not_found".into()) }];
    let theirs = Quirk::new("the search box debounces 400ms", "person", "autorun", 2);
    save_quirks(dir.path(), "acme", "Web", &[mine, theirs]).unwrap();
    let list = load_quirks(dir.path(), "acme", "Web").unwrap();
    let (mine_id, theirs_id) = (list[0].id.clone(), list[1].id.clone());

    let refused = serde_json::json!({ "id": theirs_id, "reason": "no longer true" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk-retire", &refused, "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, PERSON_NOTE));

    let no_reason = serde_json::json!({ "id": mine_id }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk-retire", &no_reason, "1.0.0").await;
    assert_eq!(status, 400, "{out}");

    let body = serde_json::json!({
        "id": mine_id,
        "reason": "it was the spinner, not the grid",
        "replacement": "a spinner covers the grid while it loads",
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk-retire", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let list = load_quirks(dir.path(), "acme", "Web").unwrap();
    let new = list.iter().find(|q| q.text == "a spinner covers the grid while it loads").unwrap();
    assert_eq!(out, format!("retired {mine_id}, replaced by {}", new.id));
    assert_eq!(new.sources, vec![QuirkSource { case_id: 7, steps: vec![2], class: Some("not_found".into()) }]);
    let old = list.iter().find(|q| q.id == mine_id).unwrap();
    assert_eq!(old.status, "retired");
    assert_eq!(old.retired_reason.as_deref(), Some("it was the spinner, not the grid"));

    // Out of both guides; the replacement and the person's note stay.
    let (_, autorun) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    let (_, api) = route(&ctx(), None, "GET", "/api-template-guide", "", "1.0.0").await;
    for guide in [&autorun, &api] {
        assert!(!guide.contains("the grid needs a second click"), "{guide}");
        assert!(guide.contains("a spinner covers the grid while it loads"), "{guide}");
        assert!(guide.contains("the search box debounces 400ms"), "{guide}");
    }
}

/// One list per project, read by both assistants: the API templates
/// guide ends with the same section as the Auto Run guide, and a note
/// filed from API template work says so.
#[tokio::test]
async fn the_api_template_guide_ends_with_the_same_quirks_section() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let (_, before) = route(&ctx(), None, "GET", "/api-template-guide", "", "1.0.0").await;
    assert!(!before.contains("## Known quirks of this application\n\n"), "{before}");

    let body = serde_json::json!({ "text": "the leave handler wants a CSRF header", "from": "api" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let quirks = load_quirks(dir.path(), "acme", "Web").unwrap();
    assert_eq!(quirks[0].from, "api");

    let (status, api) = route(&ctx(), None, "GET", "/api-template-guide", "", "1.0.0").await;
    assert_eq!(status, 200);
    let line = format!("- [{}] (assistant, API) the leave handler wants a CSRF header\n", quirks[0].id);
    assert!(api.contains("## Known quirks of this application\n\n") && api.contains(&line), "{api}");
    let (_, autorun) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert!(autorun.contains(&line), "the same line in the Auto Run guide: {autorun}");
    assert!(api.contains("`record_app_quirk") && api.contains("`retire_app_quirk"), "the guide names its own tools: {api}");
}

/// Names the active environment `name` (a test environment or not) in the
/// store at `dir`.
fn name_the_active_environment(dir: &std::path::Path, name: &str, test_environment: bool) {
    let mut env = v2_lib::environments::active(dir).unwrap();
    env.name = name.into();
    env.test_environment = test_environment;
    let known = vec![env.db_id.clone()];
    v2_lib::environments::save_env(dir, env, &known).unwrap();
}

/// The Auto Run guide explains environments, and the route names the
/// active one - with no project open too, since an environment is the
/// app's, not a project's.
#[tokio::test]
async fn the_autorun_guide_explains_environments_and_names_the_active_one() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    name_the_active_environment(dir.path(), "Local QA", false);

    for context in [ctx(), BridgeContext::default()] {
        let (status, body) = route(&context, None, "GET", "/autorun-guide", "", "1.0.0").await;
        assert_eq!(status, 200);
        assert!(body.contains("## Environments"), "{body}");
        assert!(body.contains("Local QA"), "the active environment's name: {body}");
        assert!(body.contains("get_accounts") && body.contains("propose_accounts"), "{body}");
        assert!(body.contains("never invent a password"), "{body}");
        assert!(body.contains("read-only") && body.contains("never write"), "{body}");
        assert!(body.contains("not marked as a test environment"), "{body}");
    }

    name_the_active_environment(dir.path(), "Staging", true);
    let (_, body) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert!(body.contains("Staging") && !body.contains("Local QA"), "{body}");
    assert!(!body.contains("not marked as a test environment"), "{body}");
    assert!(body.contains("marked as a test environment"), "{body}");
}

/// The API templates guide says templates run against the active
/// environment, by name.
#[tokio::test]
async fn the_api_template_guide_says_templates_run_against_the_active_environment() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    name_the_active_environment(dir.path(), "Local QA", false);

    let (status, api) = route(&ctx(), None, "GET", "/api-template-guide", "", "1.0.0").await;
    assert_eq!(status, 200);
    assert!(api.contains("## Environments"), "{api}");
    assert!(api.contains("run against the active environment, \"Local QA\""), "{api}");
}

/// A full list refuses one more through the bridge, naming the tool and
/// the candidates.
#[tokio::test]
async fn a_full_list_refuses_through_the_bridge_with_candidates() {
    use v2_lib::autorun::quirks::{save_quirks, Quirk, MAX_QUIRKS};
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let list: Vec<Quirk> =
        (0..MAX_QUIRKS).map(|i| Quirk::new(&format!("note {i}"), "assistant", "autorun", 100 + i as u64)).collect();
    save_quirks(dir.path(), "acme", "Web", &list).unwrap();
    let body = serde_json::json!({ "text": "one more" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("retire_autorun_quirk"), "{out}");
    assert!(out.contains("\"note 0\"") && out.contains("\"note 1\"") && out.contains("\"note 2\""), "{out}");
    assert!(!out.contains("\"note 3\""), "{out}");
}

// ----------------------------------------------------------- the dev gate
//
// Every route above runs through `route()`, which reads its own build's
// `dev_build()` - always true for this test binary, since `cargo test`
// compiles with debug assertions on. The release rule (a release build
// refuses outright) can only be proven through the guard's explicit
// `dev: bool` seam, exercised directly here.

/// Outside a development build, the guard refuses unconditionally with
/// the exact sentence the autorun routes are meant to answer.
#[test]
fn the_guard_refuses_outside_a_development_build() {
    let refused = autorun_route_guard(false);
    let (status, body) = refused.expect("a release build must be refused");
    assert_eq!(status, 404);
    assert_eq!(body, "not available in this build");
}

/// Inside a development build, the guard lets the request through - the
/// caller falls through to the route's own handling.
#[test]
fn the_guard_lets_a_development_build_through() {
    assert!(autorun_route_guard(true).is_none());
}

/// The guard is applied by PATH, once, before the router's match - so a
/// route added later is covered by the shape of its name rather than by
/// somebody remembering to repeat the check. Every Auto Run route is
/// refused outside a development build; nothing else is touched.
#[test]
fn the_guard_still_holds_for_every_new_route() {
    let autorun = [
        "/autorun-guide",
        "/autorun-script",
        "/autorun-page",
        "/autorun-probe",
        "/autorun-try",
        "/autorun-failures",
        "/autorun-quirk",
        "/autorun-quirk-retire",
        "/autorun-defect",
    ];
    for path in autorun {
        let (status, body) =
            autorun_guard_for(path, false).unwrap_or_else(|| panic!("{path} was not refused"));
        assert_eq!(status, 404, "{path}");
        assert_eq!(body, "not available in this build", "{path}");
        assert!(autorun_guard_for(path, true).is_none(), "{path} refused in a dev build");
    }
    for path in ["/ping", "/guide", "/test-cases", "/tools"] {
        assert!(autorun_guard_for(path, false).is_none(), "{path} is not an Auto Run route");
    }
}

#[tokio::test]
async fn with_addresses_switched_off_a_bundle_with_a_navigate_is_refused_before_anything_else() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    v2_lib::autorun::nav::set_direct_urls(dir.path(), "acme", "Web", false).unwrap();
    let body = case_7("#toast", "Saved").to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, format!("case 7: {}", v2_lib::autorun::nav::no_address(1)));
    assert!(load_script(dir.path(), 7).unwrap().is_none());
}

#[tokio::test]
async fn the_guide_says_a_run_starts_on_the_module_screen_only_while_addresses_are_off() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_, on) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert!(!on.contains("## This project's runs start on the module screen"), "{on}");
    v2_lib::autorun::nav::set_direct_urls(dir.path(), "acme", "Web", false).unwrap();
    let (status, off) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert_eq!(status, 200);
    assert!(off.contains("## This project's runs start on the module screen"), "{off}");
    assert!(!off.contains('\u{2014}'));
}

/// `cases` ties a quirk recorded on its own to steps that failed in their
/// case's newest run - a step that did not fail there is refused and
/// nothing is written - and a line a person retired is not brought back by
/// the assistant.
#[tokio::test]
async fn a_quirk_on_its_own_names_failed_steps_and_never_overrides_a_person() {
    use v2_lib::autorun::quirks::{save_quirks, update_quirks, retire_in, QuirkSource};
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let mut run: LocalRun = serde_json::from_value(serde_json::json!({
        "id": "run-1700000000000", "pbi_id": 1, "started_at": "1700000000000", "cases": [], "mode": "unattended"
    }))
    .unwrap();
    let mut case: CaseRecord = serde_json::from_value(serde_json::json!({
        "case_id": 7, "title": "case 7", "verdict": "", "note": "", "steps": [], "proposed": "Failed"
    }))
    .unwrap();
    case.steps.push(StepRecord { step_number: 1, outcomes: vec![ActionOutcome::passed("ok")], screenshot: None });
    case.steps.push(StepRecord {
        step_number: 2,
        outcomes: vec![ActionOutcome::failed("waited 5000ms: button \"Save\" not found")],
        screenshot: None,
    });
    run.cases.push(case);
    save_run(dir.path(), &run).unwrap();

    let wrong = serde_json::json!({ "text": "the save button loads late", "cases": [{ "case_id": 7, "steps": [1] }] }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &wrong, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("case 7 step 1 did not fail"), "{out}");
    assert!(load_quirks(dir.path(), "acme", "Web").unwrap().is_empty());

    let right = serde_json::json!({ "text": "the save button loads late", "cases": [{ "case_id": 7, "steps": [2] }] }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &right, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let quirks = load_quirks(dir.path(), "acme", "Web").unwrap();
    assert_eq!(quirks[0].sources, vec![QuirkSource { case_id: 7, steps: vec![2], class: Some("not_found".into()) }]);

    // The person retires it in the app; the assistant cannot bring it back.
    let id = quirks[0].id.clone();
    update_quirks(dir.path(), "acme", "Web", |l| retire_in(l, &id, Some("it was the network"), None, false, 5)).unwrap();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &right, "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, "a person retired this note (it was the network) - ask them to restore it"));

    // One the assistant retired itself comes back, with its old reason.
    save_quirks(dir.path(), "acme", "Web", &[]).unwrap();
    let body = serde_json::json!({ "text": "dates render as dd/mm" }).to_string();
    route(&ctx(), None, "POST", "/autorun-quirk", &body, "1.0.0").await;
    let id = load_quirks(dir.path(), "acme", "Web").unwrap()[0].id.clone();
    let retire = serde_json::json!({ "id": id, "reason": "the locale changed" }).to_string();
    route(&ctx(), None, "POST", "/autorun-quirk-retire", &retire, "1.0.0").await;
    let (status, out) = route(&ctx(), None, "POST", "/autorun-quirk", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.starts_with(&format!("{id} had been retired (\"the locale changed\")")), "{out}");
}

// ------------------------------------------------------- a script's area

/// Case 7 with an area, or without one.
fn case_7_in(area: Option<&str>, selector: &str) -> serde_json::Value {
    let mut sc = case_7(selector, "Saved");
    if let Some(a) = area {
        sc[0]["area"] = serde_json::json!(a);
    }
    sc
}

/// Two recorded areas under one module, for the bridge's project.
fn record_two_areas(root: &std::path::Path) {
    let nav: v2_lib::autorun::nav::NavFile = serde_json::from_value(serde_json::json!({
        "modules": [
            { "area": "Cycle Setup", "module": "PMS", "clicks": [{ "role": "link", "name": "Setup" }], "arrived": "/pms/setup", "recorded": "2026-10-01T10:00:00Z" },
            { "area": "Manage Cycle", "module": "PMS", "clicks": [{ "role": "link", "name": "Manage" }], "arrived": "/pms/manage", "recorded": "2026-10-01T10:00:00Z" }
        ]
    }))
    .unwrap();
    v2_lib::autorun::nav::save_nav(root, "acme", "Web", &nav).unwrap();
}

/// Review of Task 8, finding 1: where a case starts is part of the script.
/// A re-send that only moves it to another area is a change - refused
/// undeclared, counted against the repair cap once declared - never
/// "(unchanged)".
#[tokio::test]
async fn a_resend_that_only_changes_the_area_is_a_repair() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    record_two_areas(dir.path());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let first = case_7_in(Some("Cycle Setup"), "#toast").to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    let moved = case_7_in(Some("Manage Cycle"), "#toast").to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &moved, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("the area changed from \"Cycle Setup\" to \"Manage Cycle\" but was not declared"), "{out}");
    let on_disk = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(on_disk.area.as_deref(), Some("Cycle Setup"));
    assert_eq!(on_disk.repairs, 0);

    let declared = serde_json::json!({
        "scripts": case_7_in(Some("Manage Cycle"), "#toast"),
        "edits": [{ "case_id": 7, "steps": [], "area": true, "why": "the case is about managing a cycle, not setting one up" }],
    })
    .to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &declared, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out.lines().next().unwrap(), "saved 1 script(s): case 7 (repaired, 1 of 3 used)");
    let on_disk = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(on_disk.area.as_deref(), Some("Manage Cycle"));
    assert_eq!(on_disk.repairs, 1);
}

/// A repair to a step that leaves `area` out does not erase the area the
/// saved script has: declaring the step is not declaring the area, so the
/// save is refused and the area stays on disk.
#[tokio::test]
async fn a_repair_that_leaves_the_area_out_does_not_erase_it() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    record_two_areas(dir.path());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;

    let first = case_7_in(Some("Manage Cycle"), "#toast").to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    let forgot = serde_json::json!({
        "scripts": case_7_in(None, ".toast"),
        "edits": [edit_step_2("the toast has no id, only a class")],
    })
    .to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &forgot, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("the area changed from \"Manage Cycle\" to the case's Module but was not declared"), "{out}");
    let on_disk = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(on_disk.area.as_deref(), Some("Manage Cycle"));
    assert_eq!(on_disk.repairs, 0);

    // Sending the area back with the same repair is the repair alone.
    let kept = serde_json::json!({
        "scripts": case_7_in(Some("Manage Cycle"), ".toast"),
        "edits": [edit_step_2("the toast has no id, only a class")],
    })
    .to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &kept, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().area.as_deref(), Some("Manage Cycle"));
}
