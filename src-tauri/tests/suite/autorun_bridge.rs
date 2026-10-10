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

/// Through the assistant's save, a repair may add a precondition to a
/// saved script - validated like any save, and counted as a repair - but
/// never drop or change one it has.
#[tokio::test]
async fn an_assistant_repair_may_add_preconditions_but_never_drop_or_change_one() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let flow: v2_lib::api_templates::flow::Flow =
        serde_json::from_value(crate::common::cycle_flow_json()).unwrap();
    v2_lib::api_templates::flow_store::save(dir.path(), "acme", "Web", &flow).unwrap();
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let send = |pre: serde_json::Value| {
        let mut body = case_7("#toast", "Saved");
        body[0]["preconditions"] = pre;
        body.to_string()
    };
    let publish = serde_json::json!({ "flow": "pms-performance-cycle", "stage": "publish", "value": 274 });
    let rules = serde_json::json!({ "flow": "pms-performance-cycle", "stage": "rules", "value": 274 });

    see_scripts(dir.path(), &send(serde_json::json!([])));
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &send(serde_json::json!([])), "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    // Added to a saved script: a repair, and validated.
    let bad = serde_json::json!([{ "flow": "pms-performance-cycle", "stage": "published", "value": 274 }]);
    see_scripts(dir.path(), &send(bad.clone()));
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &send(bad), "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "case 7: precondition 1: flow Performance cycle wizard has no stage published");
    see_scripts(dir.path(), &send(serde_json::json!([publish])));
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &send(serde_json::json!([publish])), "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out.lines().next().unwrap(), "saved 1 script(s): case 7 (repaired, 1 of 3 used)");

    // Sent back as it is, with one more beside it: another repair.
    let (status, out) = route(
        &ctx(),
        Some(&client),
        "POST",
        "/autorun-script",
        &send(serde_json::json!([publish, rules])),
        "1.0.0",
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out.lines().next().unwrap(), "saved 1 script(s): case 7 (repaired, 2 of 3 used)");

    // Dropped, or changed: refused, and the saved script keeps both.
    let kept = v2_lib::autorun::edits::PRECONDITIONS_KEPT;
    see_scripts(dir.path(), &send(serde_json::json!([rules])));
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &send(serde_json::json!([rules])), "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, kept));
    let mut moved = publish.clone();
    moved["value"] = serde_json::json!(275);
    let (status, out) = route(
        &ctx(),
        Some(&client),
        "POST",
        "/autorun-script",
        &send(serde_json::json!([moved, rules])),
        "1.0.0",
    )
    .await;
    assert_eq!((status, out.as_str()), (400, kept));
    let saved = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(saved.preconditions.len(), 2);
    assert_eq!(saved.repairs, 2);
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
    see_scripts(dir.path(), &checks_nothing);
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
    see_scripts(dir.path(), &body.to_string());
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
    see_scripts(dir.path(), &declared);
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

    see_scripts(dir.path(), &body);
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
    see_scripts(dir.path(), &first);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    // Step 2's locator moved, and nothing said so.
    let undeclared = serde_json::json!({ "scripts": case_7(".toast", "Saved") }).to_string();
    see_scripts(dir.path(), &undeclared);
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
    see_scripts(dir.path(), &declared);
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
    see_scripts(dir.path(), &weakened);
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
    see_scripts(dir.path(), &first);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().last_repair, None, "a new script has no repair yet");

    let declared = serde_json::json!({
        "scripts": case_7(".toast", "Saved"),
        "edits": [edit_step_2("the toast has no id, only a class")],
    })
    .to_string();
    see_scripts(dir.path(), &declared);
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
    see_scripts(dir.path(), &resend);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &resend, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(
        load_script(dir.path(), 7).unwrap().unwrap().last_repair.as_deref(),
        Some("the toast has no id, only a class")
    );
}

// ------------------------------------- the tool's payload shapes, end to end

/// The body `save_autorun_script` posts for these tool arguments, taken
/// from the REAL MCP dispatch - the same function the stdio proxy runs.
fn tool_body(arguments: serde_json::Value) -> String {
    let posted = std::cell::RefCell::new(None);
    let call = |method: &str, path: &str, body: &str| {
        if (method, path) == ("POST", "/autorun-script") {
            *posted.borrow_mut() = Some(body.to_string());
        }
        Ok((200, r#"{"autorun": true, "disabled": []}"#.to_string()))
    };
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "save_autorun_script", "arguments": arguments },
    });
    v2_lib::mcp::handle_message(&req.to_string(), "1.0.0", &call).unwrap();
    posted.into_inner().expect("the tool posted nothing to /autorun-script")
}

/// Every shape an assistant sends a repair in reaches the route WITH its
/// edits: the repair lands. Owner's report: "save_autorun_script through
/// the tools drops the edits list". Each shape starts from a fresh store
/// holding case 7, and changes step 2's locator.
#[tokio::test]
async fn every_payload_shape_of_a_repair_keeps_its_edits() {
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let scripts = case_7(".toast", "Saved");
    let edits = serde_json::json!([edit_step_2("the toast has no id, only a class")]);
    let bundle = serde_json::json!({ "scripts": scripts, "edits": edits });
    let mut nested = scripts.clone();
    nested[0]["edits"] = edit_step_2("the toast has no id, only a class");
    let mut nested_list = scripts.clone();
    nested_list[0]["edits"] = edits.clone();
    let mut nested_no_id = scripts.clone();
    nested_no_id[0]["edits"] = serde_json::json!({ "steps": [2], "why": "the toast has no id, only a class" });

    let shapes: Vec<(&str, serde_json::Value)> = vec![
        ("array + array", serde_json::json!({ "scripts": scripts, "edits": edits })),
        ("array + string", serde_json::json!({ "scripts": scripts, "edits": edits.to_string() })),
        ("string + array", serde_json::json!({ "scripts": scripts.to_string(), "edits": edits })),
        ("string + string", serde_json::json!({ "scripts": scripts.to_string(), "edits": edits.to_string() })),
        ("bundle in a scripts string", serde_json::json!({ "scripts": bundle.to_string() })),
        ("bundle object as scripts", serde_json::json!({ "scripts": bundle })),
        ("bundle in a scripts string + edits null", serde_json::json!({ "scripts": bundle.to_string(), "edits": null })),
        ("bundle in a scripts string + the same edits", serde_json::json!({ "scripts": bundle.to_string(), "edits": edits })),
        ("edits nested in the script", serde_json::json!({ "scripts": nested })),
        ("edits nested as a list", serde_json::json!({ "scripts": nested_list.to_string() })),
        ("edits nested without a case id", serde_json::json!({ "scripts": nested_no_id })),
    ];
    let _root = crate::serial::autorun();
    let mut lost = vec![];
    for (name, arguments) in shapes {
        let dir = TempDir::new();
        set_root(dir.path().to_path_buf());
        let first = case_7("#toast", "Saved").to_string();
        see_scripts(dir.path(), &first);
        let (status, out) =
            route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
        assert_eq!(status, 200, "{name}: {out}");

        let body = tool_body(arguments);
        see_scripts(dir.path(), &body);
        let (status, out) =
            route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
        if status != 200 || !out.contains("case 7 (repaired, 1 of 3 used)") {
            lost.push(format!("{name}: {status} {out}\n    body: {body}"));
        }
    }
    assert!(lost.is_empty(), "shapes that lost their edits:\n{}", lost.join("\n"));
}

/// `edits` sent as null on a NEW script is no declaration - there is
/// nothing to declare - and the script saves.
#[tokio::test]
async fn edits_null_on_a_new_script_is_no_declaration() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let body = tool_body(serde_json::json!({ "scripts": case_7("#toast", "Saved"), "edits": null }));
    see_scripts(dir.path(), &body);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("case 7 (new)"), "{out}");
}

/// A repair sent with no `edits` at all is refused, and the sentence says
/// `edits` is missing rather than only that a step changed.
#[tokio::test]
async fn a_repair_without_edits_says_edits_is_missing() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let first = case_7("#toast", "Saved").to_string();
    see_scripts(dir.path(), &first);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    let body = tool_body(serde_json::json!({ "scripts": case_7(".toast", "Saved") }));
    see_scripts(dir.path(), &body);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("step 2 was changed but not declared"), "{out}");
    assert!(
        out.contains("case 7 already has a script, so this save is a repair, and \"edits\" is missing."),
        "{out}"
    );
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 0);
}

/// When the bundle declares OTHER cases, a changed case left out of
/// `edits` gets the per-case sentence: the list is there, this case's
/// entry is what is missing.
#[tokio::test]
async fn with_other_cases_declared_an_undeclared_case_is_named_not_missing_edits() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[
        (7, "Save a rating", &["", "A toast says Saved"]),
        (9, "Delete a rating", &["", "A toast says Deleted"]),
    ])
    .await;
    let first = serde_json::json!([case_7("#toast", "Saved")[0].clone(), case_9("Deleted")]).to_string();
    see_scripts(dir.path(), &first);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    let body = serde_json::json!({
        "scripts": [case_7(".toast", "Saved")[0].clone(), case_9("Removed")],
        "edits": [edit_step_2("the toast has no id, only a class")],
    })
    .to_string();
    see_scripts(dir.path(), &body);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(
        out,
        "case 9: step 2 was changed but not declared - name every step you change in \"edits\", or leave it as it was"
    );
}

/// Two DIFFERENT declarations for one case are refused - the gate could
/// only ever read one of them - whether both are top-level entries or one
/// is nested in the script. The same declaration sent twice is one.
#[tokio::test]
async fn two_different_declarations_for_one_case_are_refused() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let first = case_7("#toast", "Saved").to_string();
    see_scripts(dir.path(), &first);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    let refusal = "case 7 is declared twice in \"edits\", with different contents - send one entry per case";
    let top_level = serde_json::json!({
        "scripts": case_7(".toast", "Saved"),
        "edits": [edit_step_2("the toast has no id"), edit_step_2("the toast moved")],
    });
    let mut nested = case_7(".toast", "Saved");
    nested[0]["edits"] = edit_step_2("the toast has no id");
    let nested_and_top = serde_json::json!({ "scripts": nested, "edits": [edit_step_2("the toast moved")] });
    for body in [top_level, nested_and_top] {
        see_scripts(dir.path(), &body.to_string());
        let (status, out) =
            route(&ctx(), Some(&client), "POST", "/autorun-script", &body.to_string(), "1.0.0").await;
        assert_eq!((status, out.as_str()), (400, refusal), "{body}");
    }
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 0, "nothing was written");

    let same_twice = serde_json::json!({ "scripts": nested, "edits": [edit_step_2("the toast has no id")] });
    see_scripts(dir.path(), &same_twice.to_string());
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &same_twice.to_string(), "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("case 7 (repaired, 1 of 3 used)"), "{out}");
}

/// A top-level `edits` that is one object is a one-entry list, the same
/// as one object nested in a script.
#[tokio::test]
async fn a_single_top_level_edit_object_is_a_one_entry_list() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) =
        client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let first = case_7("#toast", "Saved").to_string();
    see_scripts(dir.path(), &first);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    let body = tool_body(serde_json::json!({
        "scripts": case_7(".toast", "Saved"),
        "edits": edit_step_2("the toast has no id, only a class"),
    }));
    see_scripts(dir.path(), &body);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("case 7 (repaired, 1 of 3 used)"), "{out}");

    // Anything else that is not a list is still refused.
    let body = serde_json::json!({ "scripts": case_7("#toast", "Saved"), "edits": "two steps" }).to_string();
    see_scripts(dir.path(), &body);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.starts_with("that is not a list of declared edits"), "{out}");
}

/// The guide's whole body sent as `scripts`, with more `edits` beside it:
/// the bundle's own declarations are merged with them when they are a
/// list or one entry, and when they are anything else the call is refused
/// before anything reaches the app - the edits beside it are never
/// silently dropped.
#[tokio::test]
async fn a_bundles_own_edits_that_are_not_a_list_refuse_the_edits_beside_it() {
    let scripts = case_7(".toast", "Saved");
    let beside = serde_json::json!([edit_step_2("the toast moved")]);
    for own in [serde_json::json!(42), serde_json::json!("two steps"), serde_json::json!(true)] {
        let posted = std::cell::RefCell::new(false);
        let call = |method: &str, path: &str, _body: &str| {
            if (method, path) == ("POST", "/autorun-script") {
                *posted.borrow_mut() = true;
            }
            Ok((200, r#"{"autorun": true, "disabled": []}"#.to_string()))
        };
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "save_autorun_script", "arguments": {
                "scripts": serde_json::json!({ "scripts": scripts, "edits": own }).to_string(),
                "edits": beside,
            } },
        });
        let resp = v2_lib::mcp::handle_message(&req.to_string(), "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], true, "{own}: {resp}");
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("the bundle sent as \"scripts\" has an \"edits\" that is not a list, and more \"edits\" were sent beside it - send one \"edits\" list beside \"scripts\", one entry per case"),
            "{own}: {text}"
        );
        assert!(!*posted.borrow(), "{own}: nothing may reach the app");
    }

    // One entry of its own, and the same entry beside it: one declaration.
    let body = tool_body(serde_json::json!({
        "scripts": { "scripts": scripts, "edits": edit_step_2("the toast moved") },
        "edits": beside,
    }));
    let sent: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent["edits"], beside, "{body}");
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

    see_scripts(dir.path(), &case_7("#toast", "v0").to_string());
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
        see_scripts(dir.path(), &body);
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
    see_scripts(dir.path(), &fourth);
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
    see_scripts(dir.path(), &again);
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

    see_scripts(dir.path(), &case_7("#toast", "Saved").to_string());
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
    see_scripts(dir.path(), &body);
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
    see_scripts(dir.path(), &repeat);
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
    see_scripts(dir.path(), &both("Saved", "Deleted").to_string());
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
    see_scripts(dir.path(), &body);
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
    see_scripts(dir.path(), &over_declared);
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
        see_scripts(dir.path(), &body);
        let (status, out) =
            route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
        assert_eq!(status, 200, "{out}");
    }
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 3, "at the cap");

    for round in 1..=4 {
        see_scripts(dir.path(), &case_7("#toast", "Saved!!!").to_string());
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
    see_scripts(dir.path(), &first);
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
    see_scripts(dir.path(), &reordered);
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
    see_scripts(dir.path(), &declared);
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
    see_scripts(dir.path(), &first);
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
    see_scripts(dir.path(), &unchanged);
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
    see_scripts(dir.path(), &changed);
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
        serde_json::json!({ "action": { "kind": "click", "selector": "#save" }, "case_id": 7 }).to_string();
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

/// A try names the case it is for, so that case's no-save guard applies
/// (run safety §1): without one, or with one that is not a number, it is
/// refused before the browser is asked.
#[tokio::test]
async fn try_refuses_a_body_that_names_no_case() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());

    let none = serde_json::json!({ "action": { "kind": "click", "selector": "#save" } }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-try", &none, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "name the case this try is for (case_id), so its no-save guard applies");

    let words = serde_json::json!({ "action": { "kind": "click", "selector": "#save" }, "case_id": "seven" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-try", &words, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "case_id must be a number");
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
        "AI tried navigate /ratings in the supervised browser: ok"
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

/// The same refusal reaches inside a guard: a `when_visible` whose `then`
/// would send the person's browser to a local file is refused before it
/// looks for anything.
#[tokio::test]
async fn try_refuses_a_file_navigate_inside_a_when_visible() {
    for url in ["file:///etc/passwd", "  FILE://C:/secrets.txt"] {
        let body = serde_json::json!({ "action": {
            "kind": "when_visible",
            "selector": { "role": "button", "name": "Accept" },
            "then": [
                { "kind": "click", "selector": { "role": "button", "name": "Accept" } },
                { "kind": "navigate", "url": url }
            ]
        } })
        .to_string();
        let (status, out) = route(&ctx(), None, "POST", "/autorun-try", &body, "1.0.0").await;
        assert_eq!(status, 400, "{out}");
        assert_eq!(out, "a tried navigate goes to http or https only");
    }
}

// ------------------------------------------ what the page routes record

use v2_lib::ai_bridge::{probe_page, read_page, recording_area, try_in, Sighting};
use v2_lib::autorun::discovery_map::{load_map, AreaMap};
use v2_lib::browser::snapshot::{snapshot_with_lines, DEFAULT_LIMIT, PROBE_SUMMARY_JS};

fn sighting(root: &std::path::Path, area: Option<&str>) -> Sighting {
    Sighting {
        root: root.to_path_buf(),
        org: "acme".into(),
        project: "Web".into(),
        area: area.map(str::to_string),
        account: None,
        discovering: None,
        policy: v2_lib::browser::actions::Policy::open(),
    }
}

/// The map's area by name, `""` being the unattributed bucket.
fn mapped_area(root: &std::path::Path, area: &str) -> Option<AreaMap> {
    load_map(root, "acme", "Web").unwrap().areas.into_iter().find(|a| a.area == area)
}

/// A rating form with a Name field and a Save button, at
/// `/hr/ratings?id=42`, titled Ratings.
fn ratings_page() -> crate::common::ScriptedDriver {
    crate::common::ScriptedDriver::new(|method, params| match method {
        "Accessibility.getFullAXTree" => Ok(serde_json::json!({ "nodes": [
            { "nodeId": "1", "ignored": false, "role": { "value": "form" }, "name": { "value": "Rating" },
              "childIds": ["2", "3"] },
            { "nodeId": "2", "ignored": false, "role": { "value": "textbox" }, "name": { "value": "Name" },
              "childIds": [] },
            { "nodeId": "3", "ignored": false, "role": { "value": "button" }, "name": { "value": "Save" },
              "childIds": [] }
        ] })),
        "Runtime.evaluate" if params["expression"] == "location.href" => {
            Ok(serde_json::json!({ "result": { "value": "https://app.example/hr/ratings?id=42#top" } }))
        }
        "Runtime.evaluate" if params["expression"] == "document.title" => {
            Ok(serde_json::json!({ "result": { "value": "Ratings" } }))
        }
        other => panic!("unexpected {other} {params}"),
    })
}

/// A page on which every locator finds `found` elements, at `/hr/ratings`.
fn probed_page(found: usize) -> crate::common::ScriptedDriver {
    crate::common::ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" if params["expression"] == "document" => {
                Ok(serde_json::json!({ "result": { "objectId": "doc" } }))
            }
            "Runtime.evaluate" if params["expression"] == "location.href" => {
                Ok(serde_json::json!({ "result": { "value": "https://app.example/hr/ratings" } }))
            }
            "Runtime.evaluate" if params["expression"] == "document.title" => {
                Ok(serde_json::json!({ "result": { "value": "Ratings" } }))
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::locator::VISIBLE_JS => {
                Ok(serde_json::json!({ "result": { "value": true } }))
            }
            "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => Ok(serde_json::json!({
                "result": { "value": { "tag": "button", "text": "Archive", "rect": [10.0, 20.0, 80.0, 24.0] } }
            })),
            "Runtime.callFunctionOn" => Ok(serde_json::json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => Ok(serde_json::json!({ "result": (0..found)
                .map(|i| serde_json::json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } }))
                .collect::<Vec<_>>() })),
            other => panic!("unexpected {other} {params}"),
        }
    })
}

fn archive() -> v2_lib::browser::locator::Target {
    v2_lib::browser::locator::Target::from("#archive")
}

fn click_save() -> Action {
    Action::Click { selector: "#save".into() }
}

/// Does `page` hold the CSS locator `css`?
fn holds_css(page: &v2_lib::autorun::discovery_map::PageMap, css: &str) -> bool {
    page.elements.iter().any(|e| e.key == v2_lib::browser::locator::SeenKey::Css(css.to_string()))
}

/// A page read files every locator it printed under the page's path and
/// title, and hands the assistant the same text it always did. Only a
/// discovery stamps the area as explored, by the account it ran as.
#[tokio::test]
async fn a_page_read_records_its_locators_in_the_map() {
    let dir = TempDir::new();
    let (status, text) =
        read_page(&mut ratings_page(), DEFAULT_LIMIT, Some(&sighting(dir.path(), Some("Ratings")))).await;
    assert_eq!(status, 200, "{text}");
    let (expected, _) = snapshot_with_lines(&mut ratings_page(), DEFAULT_LIMIT).await.unwrap();
    assert_eq!(text, expected, "the page text changed");

    let area = mapped_area(dir.path(), "Ratings").expect("nothing was recorded");
    assert_eq!(area.explored_at, None, "a page read is not a discovery");
    assert_eq!(area.account, None);
    assert_eq!(area.pages.len(), 1, "{:?}", area.pages);
    let page = &area.pages[0];
    assert_eq!(page.path, "/hr/ratings");
    assert_eq!(page.title, "Ratings");
    let names: Vec<&str> = page.elements.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"Save") && names.contains(&"Name"), "{names:?}");

    let discovering =
        Sighting { discovering: Some(1), account: Some("admin".into()), ..sighting(dir.path(), Some("Ratings")) };
    let (status, _) = read_page(&mut ratings_page(), DEFAULT_LIMIT, Some(&discovering)).await;
    assert_eq!(status, 200);
    let area = mapped_area(dir.path(), "Ratings").unwrap();
    assert!(area.explored_at.is_some(), "a discovery's read did not stamp the area");
    assert_eq!(area.account.as_deref(), Some("admin"));
}

/// A probe that matched files the locator it was asked about, even one the
/// snapshot never printed (a page cut off at its limit, say).
#[tokio::test]
async fn a_matched_probe_records_a_locator_the_snapshot_cut_off() {
    let dir = TempDir::new();
    let (status, text) = probe_page(&mut probed_page(1), &archive(), Some(&sighting(dir.path(), None))).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.starts_with("matches: 1"), "{text}");
    let area = mapped_area(dir.path(), "").expect("nothing was recorded");
    let page = area.pages.iter().find(|p| p.path == "/hr/ratings").expect("no page");
    assert!(holds_css(page, "#archive"), "{:?}", page.elements);
}

#[tokio::test]
async fn a_probe_with_no_match_records_nothing() {
    let dir = TempDir::new();
    let (status, text) = probe_page(&mut probed_page(0), &archive(), Some(&sighting(dir.path(), None))).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.starts_with("matches: 0"), "{text}");
    assert!(load_map(dir.path(), "acme", "Web").unwrap().areas.is_empty());
}

/// A try that worked files what it acted on; one that failed files nothing:
/// a locator that did not work was never seen working.
#[tokio::test]
async fn an_ok_try_records_its_targets_and_a_failed_try_does_not() {
    let dir = TempDir::new();
    let mut account = None;
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    let mut d = crate::common::FakePage::default().driver();
    let (status, text) =
        try_in(&mut d, &mut account, &mut lease, dir.path(), "acme", "Web", 7, &click_save()).await;
    assert_eq!(status, 200);
    assert!(text.starts_with("ok:"), "{text}");
    let area = mapped_area(dir.path(), "").expect("nothing was recorded");
    let page = area.pages.iter().find(|p| p.path == "/home").expect("no page");
    assert!(holds_css(page, "#save"), "{:?}", page.elements);

    let failed_dir = TempDir::new();
    let mut d = crate::common::FakePage::default().driver();
    d.block_after = Some(("Input.dispatchMouseEvent".into(), "the save was stopped".into()));
    let (status, text) =
        try_in(&mut d, &mut account, &mut lease, failed_dir.path(), "acme", "Web", 7, &click_save()).await;
    assert_eq!(status, 200);
    assert!(text.starts_with("failed:"), "{text}");
    assert!(load_map(failed_dir.path(), "acme", "Web").unwrap().areas.is_empty());
}

/// The Ruling: a discovery's own area first, then the area of the script
/// for the case being tried, then the unattributed bucket.
#[tokio::test]
async fn recordings_use_the_tried_cases_area() {
    let dir = TempDir::new();
    let script: CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": 7, "title": "Save a rating", "area": "Ratings", "steps": []
    }))
    .unwrap();
    v2_lib::autorun::store::save_script(dir.path(), &script).unwrap();

    let mut account = None;
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    let mut d = crate::common::FakePage::default().driver();
    let (_, text) = try_in(&mut d, &mut account, &mut lease, dir.path(), "acme", "Web", 7, &click_save()).await;
    assert!(text.starts_with("ok:"), "{text}");
    assert!(mapped_area(dir.path(), "Ratings").is_some(), "{:?}", load_map(dir.path(), "acme", "Web"));
    assert!(mapped_area(dir.path(), "").is_none(), "{:?}", load_map(dir.path(), "acme", "Web"));

    assert_eq!(recording_area(dir.path(), Some("Leave"), Some(7)).as_deref(), Some("Leave"));
    assert_eq!(recording_area(dir.path(), Some("  "), Some(7)).as_deref(), Some("Ratings"));
    assert_eq!(recording_area(dir.path(), None, Some(7)).as_deref(), Some("Ratings"));
    assert_eq!(recording_area(dir.path(), None, Some(99)), None);
    assert_eq!(recording_area(dir.path(), None, None), None);
}

/// An `expect_hidden` passes because nothing matched: it never saw its
/// locator, so it files nothing.
#[tokio::test]
async fn an_ok_expect_hidden_try_records_nothing() {
    let dir = TempDir::new();
    let mut account = None;
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    let mut d = crate::common::FakePage { found: 0, ..crate::common::FakePage::default() }.driver();
    let gone = Action::ExpectHidden { selector: "#gone".into(), timeout_ms: None };
    let (status, text) = try_in(&mut d, &mut account, &mut lease, dir.path(), "acme", "Web", 7, &gone).await;
    assert_eq!(status, 200);
    assert!(text.starts_with("ok:"), "{text}");
    assert!(load_map(dir.path(), "acme", "Web").unwrap().areas.is_empty());
}

/// A click that takes the page somewhere else was matched on the page it
/// started on, and is filed there.
#[tokio::test]
async fn a_click_that_navigates_is_recorded_under_the_page_it_was_on() {
    let dir = TempDir::new();
    let page = crate::common::FakePage::default();
    let clicked = std::sync::atomic::AtomicBool::new(false);
    let mut d = crate::common::ScriptedDriver::new(move |method, params| {
        if method == "Input.dispatchMouseEvent" {
            clicked.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if method == "Runtime.evaluate"
            && params["expression"] == "location.href"
            && clicked.load(std::sync::atomic::Ordering::SeqCst)
        {
            return Ok(serde_json::json!({ "result": { "value": "https://app.example/after" } }));
        }
        page.answer(method, params)
    });
    let mut account = None;
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    let (_, text) = try_in(&mut d, &mut account, &mut lease, dir.path(), "acme", "Web", 7, &click_save()).await;
    assert!(text.starts_with("ok:"), "{text}");
    let area = mapped_area(dir.path(), "").expect("nothing was recorded");
    let page = area.pages.iter().find(|p| p.path == "/home").expect("not filed under the starting page");
    assert!(holds_css(page, "#save"), "{:?}", page.elements);
    assert!(area.pages.iter().all(|p| p.path != "/after"), "{:?}", area.pages);
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
                downloads: vec![],
                tab: None,
                dialog: None,
                components: Vec::new(), duration_ms: None,
            }],
            proposed: String::new(),
            reason: String::new(),
            duration_ms: None,
            account: None,
            retried: None,
            notice: None,
            page_errors_seen: 0, phases: None,
        }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
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
        assert!(!body.contains("never put one in a proposal"), "{body}");
        assert!(body.contains("put it in the proposal") && body.contains("never a hash"), "{body}");
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
        "/autorun-discover-start",
        "/autorun-discover-action",
        "/autorun-discover-end",
        "/autorun-release",
        "/autorun-discover-area",
        "/autorun-failures",
        "/autorun-quirk",
        "/autorun-quirk-retire",
        "/autorun-defect",
        "/autorun-order",
        "/autorun-component-save",
        "/autorun-component-retire",
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

/// A component as the save route takes it.
fn component_body(why: Option<&str>) -> String {
    let mut body = serde_json::json!({
        "name": "Pick a date",
        "description": "picks a day in the calendar",
        "inputs": [{ "name": "day", "kind": "text", "description": "the day" }],
        "actions": [{ "kind": "click", "selector": { "role": "gridcell", "name": "{{day}}" } }],
    });
    if let Some(why) = why {
        body["why"] = serde_json::json!(why);
    }
    body.to_string()
}

#[tokio::test]
async fn a_component_saved_with_no_discovery_going_is_refused() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (status, out) =
        route(&ctx(), None, "POST", "/autorun-component-save", &component_body(Some("first")), "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("Try the component live in discovery first"), "{out}");
    // A body that is not a component says what one looks like.
    let (status, out) = route(&ctx(), None, "POST", "/autorun-component-save", "{\"name\": 5}", "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("\"inputs\""), "{out}");
    // The rules that need no discovery are checked first.
    let blank = component_body(None).replace("picks a day in the calendar", " ");
    let (status, out) = route(&ctx(), None, "POST", "/autorun-component-save", &blank, "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, "Pick a date needs a description."));
    let files = v2_lib::autorun::components::load_components(dir.path(), "acme", "Web").unwrap();
    assert!(files.components.is_empty());
}

#[tokio::test]
async fn a_component_is_retired_only_when_no_script_uses_it() {
    use v2_lib::autorun::components::{find, load_components, put, Component};
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let c: Component = serde_json::from_str(&component_body(None)).unwrap();
    put(dir.path(), "acme", "Web", c).unwrap();
    let mut used = scripted(7);
    used.steps[0].actions.push(Action::UseComponent { component: "pick a date".into(), inputs: Default::default() });
    save_scripts_atomically(dir.path(), &[used]).unwrap();
    let body = serde_json::json!({ "name": "Pick a date" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-component-retire", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (409, "Pick a date is used by case 7: change that script first."));
    assert!(find(&load_components(dir.path(), "acme", "Web").unwrap(), "Pick a date").is_some());

    save_scripts_atomically(dir.path(), &[scripted(7)]).unwrap();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-component-retire", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out, serde_json::json!({ "removed": "Pick a date" }).to_string());
    assert!(load_components(dir.path(), "acme", "Web").unwrap().components.is_empty());
    let (status, out) = route(&ctx(), None, "POST", "/autorun-component-retire", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (404, "Pick a date is not saved in this project"));
    let (status, _) = route(&ctx(), None, "POST", "/autorun-component-retire", "{}", "1.0.0").await;
    assert_eq!(status, 400);
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
    case.steps.push(StepRecord { step_number: 1, outcomes: vec![ActionOutcome::passed("ok")], screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None });
    case.steps.push(StepRecord {
        step_number: 2,
        outcomes: vec![ActionOutcome::failed("waited 5000ms: button \"Save\" not found")],
        screenshot: None,
        downloads: vec![],
        tab: None,
        dialog: None,
        components: Vec::new(), duration_ms: None,
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
    see_scripts(dir.path(), &first);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    let moved = case_7_in(Some("Manage Cycle"), "#toast").to_string();
    see_scripts(dir.path(), &moved);
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
    see_scripts(dir.path(), &declared);
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
    see_scripts(dir.path(), &first);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    let forgot = serde_json::json!({
        "scripts": case_7_in(None, ".toast"),
        "edits": [edit_step_2("the toast has no id, only a class")],
    })
    .to_string();
    see_scripts(dir.path(), &forgot);
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
    see_scripts(dir.path(), &kept);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &kept, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().area.as_deref(), Some("Manage Cycle"));
}

/// `list_test_files`: the project's Test files by name and human size, and
/// nothing else - no path, no date. Gated with the other Auto Run routes.
#[tokio::test]
async fn list_test_files_gives_names_and_sizes_only() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let folder = v2_lib::test_files::folder(dir.path(), "acme", "Web");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("appraisal.pdf"), vec![0u8; 1536]).unwrap();
    std::fs::write(folder.join("note.txt"), b"hello").unwrap();

    let (status, body) = route(&ctx(), None, "GET", "/autorun-test-files", "", "1.0.0").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        v,
        serde_json::json!({ "test_files": [
            { "name": "appraisal.pdf", "size": "1.5 KB" },
            { "name": "note.txt", "size": "5 bytes" },
        ] })
    );
    let shown = folder.display().to_string();
    assert!(!body.contains(&shown) && !body.contains("modified"), "{body}");

    // No project open: there is no Test files folder to read.
    let (status, body) = route(&BridgeContext::default(), None, "GET", "/autorun-test-files", "", "1.0.0").await;
    assert_eq!(status, 409, "{body}");
    assert!(body.contains("project"), "{body}");

    let (status, body) = autorun_guard_for("/autorun-test-files", false).expect("refused outside Auto Run");
    assert_eq!((status, body.as_str()), (404, "not available in this build"));
    assert!(autorun_guard_for("/autorun-test-files", true).is_none());
}

/// "The active environment" names its database as a person reads it - the
/// label, and `<database> on <server>` - and never the login.
#[tokio::test]
async fn the_active_environment_names_its_database_and_never_its_login() {
    use v2_lib::db::{credentials::databases, MemoryStore, SecretStore};
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let env = v2_lib::environments::active(dir.path()).unwrap();
    let store: std::sync::Arc<dyn SecretStore> = std::sync::Arc::new(MemoryStore::default());
    let db = databases(store.as_ref()).into_iter().find(|d| d.id == env.db_id).expect("the default database");
    assert!(!db.user.is_empty() && !db.server.is_empty() && !db.database.is_empty());

    let context = BridgeContext { db_secrets: Some(store), ..ctx() };
    let (status, body) = route(&context, None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert_eq!(status, 200);
    let section = body.split("## The active environment").nth(1).expect("the section");
    assert!(section.contains(&format!("\"{}\"", db.label)), "{section}");
    assert!(section.contains(&format!("{} on {}", db.database, db.server)), "{section}");
    assert!(!body.contains(&db.user), "the login's user never reaches the guide");
    assert!(!section.to_lowercase().contains("password=") && !section.contains("Server="), "{section}");
}

/// No database, no site address: the section says so rather than leaving
/// the assistant to guess.
#[test]
fn the_active_environment_says_when_it_has_no_database_or_address() {
    use v2_lib::autorun::guide::active_environment_section;
    let env = v2_lib::environments::Environment {
        id: "env-0000000a".into(),
        name: "Scratch".into(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: String::new(),
        test_environment: false,
        test_prefix: "AUTOTEST".into(),
    };
    let text = active_environment_section(&env, None);
    assert!(text.contains("\"Scratch\""), "{text}");
    assert!(text.contains("It has no database set."), "{text}");
    assert!(text.contains("It has no site address yet:"), "{text}");
    assert!(text.contains("any other project cannot sign in"), "{text}");
}

/// An environment whose database id names one the app no longer knows says
/// so, rather than that it never had one.
#[test]
fn the_active_environment_says_when_its_database_is_gone() {
    use v2_lib::autorun::guide::active_environment_section;
    let env = v2_lib::environments::Environment {
        id: "env-0000000b".into(),
        name: "Scratch".into(),
        start_url: "https://hr.example.internal/".into(),
        allowed_origins: vec![],
        db_id: "removed-db".into(),
        test_environment: false,
        test_prefix: "AUTOTEST".into(),
    };
    let text = active_environment_section(&env, None);
    assert!(text.contains("Its database is not set up any more."), "{text}");
    assert!(!text.contains("It has no database set."), "{text}");
}

/// Azure DevOps after case `id` was edited: the batch read returns its
/// steps `now`, and reading the work item as of `as_of` (the script's own
/// `saved_at`) returns what it said `before`. The as-of read answers only
/// when asked for exactly that moment.
async fn client_with_changed_case(id: i32, now: &[&str], before: &[&str], as_of: &str) -> (MockServer, AdoClient) {
    let steps = |expected: &[&str]| -> String {
        let s: Vec<Step> = expected
            .iter()
            .enumerate()
            .map(|(i, e)| Step { action: format!("Step {}", i + 1), expected: (*e).to_string(), shared: None })
            .collect();
        build_steps_xml(&s)
    };
    let server = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [{
            "id": id,
            "fields": { "System.Title": "Save a rating", "Microsoft.VSTS.TCM.Steps": steps(now) }
        }] })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/_apis/wit/workitems/{id}")))
        .and(wiremock::matchers::query_param("asOf", as_of))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id,
            "fields": { "Microsoft.VSTS.TCM.Steps": steps(before) }
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

/// Every save stamps the script with when it was saved: the moment a later
/// repair reads the test case as of, to see what the case itself changed.
#[tokio::test]
async fn a_saved_script_records_when_it_was_saved() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    see_scripts(dir.path(), &case_7("#toast", "Saved").to_string());
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &case_7("#toast", "Saved").to_string(), "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let saved_at = load_script(dir.path(), 7).unwrap().unwrap().saved_at.expect("no saved_at");
    // UTC, to the second, as Azure DevOps' asOf takes it: 2026-10-05T09:35:09Z
    assert_eq!(saved_at.len(), 20, "{saved_at}");
    assert!(saved_at.ends_with('Z') && saved_at.as_bytes()[10] == b'T', "{saved_at}");
}

/// The case's step 2 dropped its expected result after the script was
/// saved. A repair that follows it - step 2 no longer checks the toast - is
/// a repair, not a weakening: it is accepted, declared, and counted.
#[tokio::test]
async fn a_repair_may_follow_what_the_case_itself_changed() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    see_scripts(dir.path(), &case_7("#toast", "Saved").to_string());
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &case_7("#toast", "Saved").to_string(), "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let saved_at = load_script(dir.path(), 7).unwrap().unwrap().saved_at.expect("no saved_at");

    let (_changed, client) = client_with_changed_case(7, &["", ""], &["", "A toast says Saved"], &saved_at).await;
    let following = serde_json::json!({
        "scripts": [{
            "case_id": 7,
            "title": "Save a rating",
            "steps": [
                { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings" }] },
                { "step_number": 2, "actions": [{ "kind": "click", "selector": "#save" }] }
            ]
        }],
        "edits": [edit_step_2("the case no longer expects a toast at step 2")],
    })
    .to_string();
    see_scripts(dir.path(), &following);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &following, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out.lines().next().unwrap(), "saved 1 script(s): case 7 (repaired, 1 of 3 used)");
}

/// The as-of read says the case is exactly as it was: nothing the case
/// changed excuses a dropped check, so the old refusal stands.
#[tokio::test]
async fn a_repair_against_an_unchanged_case_still_may_not_drop_a_check() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    see_scripts(dir.path(), &case_7("#toast", "Saved").to_string());
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &case_7("#toast", "Saved").to_string(), "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let saved_at = load_script(dir.path(), 7).unwrap().unwrap().saved_at.expect("no saved_at");

    let (_same, client) =
        client_with_changed_case(7, &["", "A toast says Saved"], &["", "A toast says Saved"], &saved_at).await;
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
    see_scripts(dir.path(), &weakened);
    let (status, out) =
        route(&ctx(), Some(&client), "POST", "/autorun-script", &weakened, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("an assertion is never removed"), "{out}");
}

/// Marks affect order, not safety: an assistant marking a saved script
/// needs no "edits", uses none of the repair count, and the save says the
/// marks changed. A re-send with the same marks is unchanged again, and
/// one that leaves them out drops them.
#[tokio::test]
async fn marking_a_saved_script_is_no_repair() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let send = |body: serde_json::Value| {
        let client = &client;
        async move { route(&ctx(), Some(client), "POST", "/autorun-script", &body.to_string(), "1.0.0").await }
    };

    see_scripts(dir.path(), &case_7("#toast", "Saved").to_string());
    assert_eq!(send(case_7("#toast", "Saved")).await.0, 200);
    let mut marked = case_7("#toast", "Saved");
    marked[0]["changes"] = serde_json::json!(["cycle published"]);
    marked[0]["needs_unchanged"] = serde_json::json!(["cycle published"]);

    assert_eq!(send(marked.clone()).await, (200, "saved 1 script(s): case 7 (marks updated)".to_string()));
    let on_disk = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(on_disk.repairs, 0, "marking is not a repair");
    assert_eq!(on_disk.changes, vec!["cycle published".to_string()]);
    assert_eq!(on_disk.needs_unchanged, vec!["cycle published".to_string()]);

    assert_eq!(send(marked).await, (200, "saved 1 script(s): case 7 (unchanged)".to_string()));
    assert_eq!(send(case_7("#toast", "Saved")).await, (200, "saved 1 script(s): case 7 (marks updated)".to_string()));
    let on_disk = load_script(dir.path(), 7).unwrap().unwrap();
    assert!(on_disk.changes.is_empty() && on_disk.needs_unchanged.is_empty());
    assert_eq!(on_disk.repairs, 0);
}

// ------------------------------------------------------------ Auto Run's own order

/// A signed-in client whose PBI `pbi` is tested by `cases`: the PBI read
/// with its relations, then the batch read of those cases. The only two
/// requests `/autorun-order` makes, both reads.
async fn client_with_pbi(pbi: i32, cases: &[i32]) -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let relations: Vec<serde_json::Value> = cases
        .iter()
        .map(|id| {
            serde_json::json!({
                "rel": "Microsoft.VSTS.Common.TestedBy-Forward",
                "url": format!("https://dev.azure.com/acme/_apis/wit/workItems/{id}"),
            })
        })
        .collect();
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/_apis/wit/workitems/{pbi}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": pbi, "relations": relations })))
        .mount(&server)
        .await;
    let value: Vec<serde_json::Value> = cases
        .iter()
        .map(|id| serde_json::json!({ "id": id, "fields": { "System.Title": format!("Case {id}") } }))
        .collect();
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": value })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

fn scripted(case_id: i32) -> CaseScript {
    CaseScript {
        case_id,
        title: format!("Case {case_id}"),
        account: None,
        area: None,
        steps: vec![v2_lib::autorun::StepScript {
            step_number: 1,
            actions: vec![Action::Navigate { url: "https://app.example/".into() }],
            unchecked: None,
        }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None, ignore_page_errors: vec![], organization: None, project: None, checked: false,
    }
}

/// `set_autorun_order` saves Auto Run's own order for the PBI, and names
/// every listed case that has no saved script.
#[tokio::test]
async fn an_order_is_saved_for_the_pbi_with_a_warning_for_each_case_with_no_script() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    v2_lib::autorun::store::save_script(dir.path(), &scripted(501)).unwrap();
    v2_lib::autorun::store::save_script(dir.path(), &scripted(502)).unwrap();
    let (_server, client) = client_with_pbi(100, &[501, 502, 503, 504]).await;

    let body = serde_json::json!({ "pbi_id": 100, "case_ids": [503, 502, 501, 504] }).to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-order", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        vec![
            "saved Auto Run's order for PBI 100: 503, 502, 501, 504",
            "case 503 has no saved script",
            "case 504 has no saved script",
        ]
    );
    assert_eq!(v2_lib::autorun::store::load_order(dir.path(), 100), Some(vec![503, 502, 501, 504]));

    // Every case scripted: no warning.
    let body = serde_json::json!({ "pbi_id": 100, "case_ids": [502, 501] }).to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-order", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (200, "saved Auto Run's order for PBI 100: 502, 501"));
    assert_eq!(v2_lib::autorun::store::load_order(dir.path(), 100), Some(vec![502, 501]));
}

/// An id the PBI is not tested by is refused, each one by name, and
/// nothing is saved.
#[tokio::test]
async fn an_order_naming_a_case_not_in_the_pbi_is_refused() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_pbi(100, &[501, 502]).await;

    let body = serde_json::json!({ "pbi_id": 100, "case_ids": [501, 999, 502, 998] }).to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-order", &body, "1.0.0").await;
    assert_eq!(
        (status, out.as_str()),
        (400, "case 999 is not in PBI 100; case 998 is not in PBI 100")
    );
    assert_eq!(v2_lib::autorun::store::load_order(dir.path(), 100), None);
}

/// A body that is not an order is refused before anything is read.
#[tokio::test]
async fn an_order_that_is_not_one_is_refused() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_pbi(100, &[501, 502]).await;
    let send = |body: serde_json::Value| {
        let client = &client;
        async move { route(&ctx(), Some(client), "POST", "/autorun-order", &body.to_string(), "1.0.0").await }
    };

    let (status, out) = send(serde_json::json!({ "pbi_id": 100, "case_ids": [] })).await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("case_ids is empty"), "{out}");
    let (status, out) = send(serde_json::json!({ "pbi_id": 100, "case_ids": [501, 502, 501] })).await;
    assert_eq!((status, out.as_str()), (400, "case 501 is listed twice"));
    let (status, out) = send(serde_json::json!({ "case_ids": [501] })).await;
    assert_eq!(status, 400, "{out}");
    assert!(out.starts_with("that is not an order"), "{out}");
    assert_eq!(v2_lib::autorun::store::load_order(dir.path(), 100), None);
}

/// The PBI's cases come from Azure DevOps, so the order needs a signed-in
/// app.
#[tokio::test]
async fn an_order_needs_a_signed_in_app() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let body = serde_json::json!({ "pbi_id": 100, "case_ids": [501] }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-order", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (503, "sign in to Test Case Manager first"));
    assert_eq!(v2_lib::autorun::store::load_order(dir.path(), 100), None);
}

/// The live app as a save sees it: what the scripts in `body` use,
/// recorded as seen (`common::see_scripts`).
fn see_scripts(root: &std::path::Path, body: &str) {
    let c = ctx();
    crate::common::see_scripts(root, &c.org, &c.project, body);
}

/// The sentence a save refuses an unseen locator with.
fn never_seen(step: i32, what: &str) -> String {
    format!(
        "Step {step}: {what} was never seen on the live app. Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again."
    )
}

/// A new script is checked in full against the discovery map: a locator
/// the app never saw on the live page refuses the whole save.
#[tokio::test]
async fn a_new_script_with_an_unseen_locator_is_refused_and_nothing_is_written() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    // The page and the toast were seen; the Save button never was.
    let body = case_7("#toast", "Saved").to_string();
    see_scripts(dir.path(), &case_7("#toast", "Saved").to_string().replace("#save", "#toast"));

    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!((status, out), (400, never_seen(2, "#save")));
    assert_eq!(load_script(dir.path(), 7).unwrap(), None);
}

/// Saving a script again word for word is not checked: it was checked
/// when it was first saved, and what the map holds now does not undo that.
#[tokio::test]
async fn an_unchanged_resave_is_not_checked() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let body = case_7("#toast", "Saved").to_string();
    see_scripts(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    v2_lib::autorun::discovery_map::forget_area(dir.path(), "acme", "Web", "").unwrap();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("case 7 (unchanged)"), "{out}");
}

/// A repair is checked only at the steps it declares: step 1's page is no
/// longer in the map, and that is not the repair's concern.
#[tokio::test]
async fn a_repair_checks_only_its_declared_steps() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let first = case_7("#toast", "Saved").to_string();
    see_scripts(dir.path(), &first);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    v2_lib::autorun::discovery_map::forget_area(dir.path(), "acme", "Web", "").unwrap();

    let repair = |selector: &str| {
        serde_json::json!({ "scripts": case_7(selector, "Saved"), "edits": [edit_step_2("the toast moved")] })
            .to_string()
    };
    // Step 2's new toast was never seen.
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &repair(".toast"), "1.0.0").await;
    assert_eq!((status, out), (400, format!("{}
{}", never_seen(2, "#save"), never_seen(2, ".toast"))));
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 0);

    // Step 2's locators seen; step 1's unseen page is not looked at.
    see_scripts(
        dir.path(),
        &serde_json::json!([{ "case_id": 7, "title": "x", "steps": [{ "step_number": 2, "actions": [
            { "kind": "click", "selector": "#save" },
            { "kind": "expect_visible", "selector": ".toast" }
        ]}]}])
        .to_string(),
    );
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &repair(".toast"), "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(load_script(dir.path(), 7).unwrap().unwrap().repairs, 1);
}

/// A map that cannot be read refuses the save with why: the check cannot
/// be made, and a save it was not made on is not a save it passed.
#[tokio::test]
async fn an_unreadable_map_refuses_the_save() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let path = v2_lib::autorun::discovery_map::map_path(dir.path(), "acme", "Web");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ not a map").unwrap();

    let body = case_7("#toast", "Saved").to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    let file = v2_lib::autorun::discovery_map::map_file_name("acme", "Web");
    assert!(out.contains(&format!("The discovery map {file} is damaged")), "{out}");
    assert!(out.contains("Reset map"), "the refusal gives the person no way out: {out}");
    assert!(!out.contains(&dir.path().to_string_lossy().to_string()), "a full path reached the assistant: {out}");
    assert_eq!(load_script(dir.path(), 7).unwrap(), None);
}

/// "Edit a row" saved in the test project: click the row the script names.
fn put_edit_a_row(root: &std::path::Path) {
    let c: v2_lib::autorun::components::Component = serde_json::from_value(serde_json::json!({
        "name": "Edit a row", "description": "d", "version": 1,
        "inputs": [{ "name": "row", "kind": "target", "description": "" }],
        "actions": [{ "kind": "click", "selector": { "input": "row" } }]
    }))
    .unwrap();
    v2_lib::autorun::components::put(root, "acme", "Web", c).unwrap();
}

/// A script that uses a component saves once the component is in the
/// project and the row it is given was seen; before that, each is refused.
#[tokio::test]
async fn a_script_using_a_component_saves() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Edit a request", &[""])]).await;
    let body = serde_json::json!([{
        "case_id": 7,
        "title": "Edit a request",
        "steps": [{ "step_number": 1, "actions": [
            { "kind": "use_component", "component": "Edit a row", "inputs": { "row": { "role": "row", "name": "Alpha" } } }
        ]}]
    }])
    .to_string();

    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, "Step 1: Edit a row is not saved in this project"));

    put_edit_a_row(dir.path());
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!((status, out), (400, never_seen(1, "row \"Alpha\"")));
    assert_eq!(load_script(dir.path(), 7).unwrap(), None);

    let row: v2_lib::browser::locator::Target =
        serde_json::from_value(serde_json::json!({ "role": "row", "name": "Alpha" })).unwrap();
    v2_lib::autorun::discovery_map::record_matched(dir.path(), "acme", "Web", None, "/", &row, 0).unwrap();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(load_script(dir.path(), 7).unwrap().is_some());
}

/// A failed try names its picture by the full path of a file that is there,
/// never by the bare name: an assistant given only a name searched the
/// whole disk for it.
#[tokio::test]
async fn a_try_answer_names_the_picture_by_a_path_that_exists() {
    let dir = TempDir::new();
    let mut account = None;
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    // A page that cannot find the button and can take a picture.
    let page = crate::common::FakePage { found: 0, ..Default::default() };
    let mut d = crate::common::ScriptedDriver::new(move |method, params| match method {
        "Page.captureScreenshot" => Ok(serde_json::json!({ "data": "/9j/4AAQ" })),
        _ => page.answer(method, params),
    });
    let (status, text) = try_in(&mut d, &mut account, &mut lease, dir.path(), "acme", "Web", 7, &click_save()).await;
    assert_eq!(status, 200, "{text}");
    let at = text.find("(picture: ").unwrap_or_else(|| panic!("no picture in {text}")) + "(picture: ".len();
    let named = text[at..].trim_end_matches(')');
    let path = std::path::Path::new(named);
    assert!(path.is_absolute(), "{named}");
    assert!(path.is_file(), "{named}");
    assert_eq!(path.parent().unwrap(), dir.path().join("shots"), "{named}");
}

/// The assistant's save checks every step of a new script, so it stamps
/// the project and vouches: a locator it saved counts as seen in its area
/// after the map has let it go. A save from the editor takes that back.
#[tokio::test]
async fn a_bridge_saved_script_vouches_for_its_locators() {
    use v2_lib::autorun::discovery_map::{forget_area, record_seen};
    use v2_lib::autorun::seen_check::{check_seen, load_checked_map};
    use v2_lib::browser::locator::{LocatorStep, Target};
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(201, "Publish a rating", &[""])]).await;
    let publish = LocatorStep { role: Some("button".into()), name: Some("Publish".into()), ..LocatorStep::default() };
    v2_lib::autorun::nav::put_path(
        dir.path(),
        "acme",
        "Web",
        v2_lib::autorun::nav::ModulePath {
            area: "Ratings".to_string(),
            module: "Ratings".to_string(),
            clicks: vec![Target::from("#ratings")],
            arrived: "/ratings".to_string(),
            recorded: "2026-10-10T10:00:00Z".to_string(),
            start: String::new(),
            made_by: v2_lib::autorun::nav::MadeBy::Person,
        },
    )
    .unwrap();
    let line = v2_lib::browser::snapshot::SnapLine {
        role: "button".into(),
        name: "Publish".into(),
        locator: Target::One(publish.clone()),
        required: false,
    };
    record_seen(dir.path(), "acme", "Web", Some("Ratings"), "/ratings", "Ratings", &[line], None, Some(1), 1).unwrap();
    let script = |id: i32| {
        serde_json::json!({
            "case_id": id, "title": "Publish a rating", "area": "Ratings", "checked": false,
            "steps": [{ "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Publish" } }] }]
        })
    };
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &serde_json::json!([script(201)]).to_string(), "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let saved = load_script(dir.path(), 201).unwrap().unwrap();
    assert_eq!((saved.organization.as_deref(), saved.project.as_deref(), saved.checked), (Some("acme"), Some("Web"), true));

    // The map lets the button go; the saved script still vouches for it.
    forget_area(dir.path(), "acme", "Web", "Ratings").unwrap();
    let another: CaseScript = serde_json::from_value(script(202)).unwrap();
    let map = load_checked_map(dir.path(), "acme", "Web").unwrap();
    let components = v2_lib::autorun::components::ComponentFile::default();
    assert_eq!(check_seen(&map, &components, &another, &[], None), Ok(()));

    // A person's save from the editor takes the vouching back.
    v2_lib::commands::autorun::save_script_from_editor(dir.path(), "acme", "Web", saved).unwrap();
    let map = load_checked_map(dir.path(), "acme", "Web").unwrap();
    assert!(check_seen(&map, &components, &another, &[], None).is_err());
}

/// A script saved before scripts were stamped, resent by the assistant
/// unchanged: it is stamped checked only while it is the one project with
/// recorded areas (the legacy rule), and unchecked otherwise.
async fn resave_a_legacy_script(another_project_has_areas: bool) -> CaseScript {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(201, "Publish a rating", &[""])]).await;
    let area = |project: &str| {
        v2_lib::autorun::nav::put_path(
            dir.path(),
            "acme",
            project,
            v2_lib::autorun::nav::ModulePath {
                area: "Ratings".to_string(),
                module: "Ratings".to_string(),
                clicks: vec![v2_lib::browser::locator::Target::from("#ratings")],
                arrived: "/ratings".to_string(),
                recorded: "2026-10-10T10:00:00Z".to_string(),
                start: String::new(),
                made_by: v2_lib::autorun::nav::MadeBy::Person,
            },
        )
        .unwrap()
    };
    area("Web");
    if another_project_has_areas {
        area("Mobile");
    }
    let body = serde_json::json!([{
        "case_id": 201, "title": "Publish a rating", "area": "Ratings",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Publish" } }] }]
    }]);
    let legacy: Vec<CaseScript> = serde_json::from_value(body.clone()).unwrap();
    save_scripts_atomically(dir.path(), &legacy).unwrap();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body.to_string(), "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("unchanged"), "{out}");
    load_script(dir.path(), 201).unwrap().unwrap()
}

#[tokio::test]
async fn a_legacy_script_resaved_with_two_projects_with_areas_stays_unchecked() {
    let saved = resave_a_legacy_script(true).await;
    assert_eq!((saved.project.as_deref(), saved.checked), (Some("Web"), false));
}

#[tokio::test]
async fn a_legacy_script_resaved_as_the_one_project_with_areas_is_checked() {
    let saved = resave_a_legacy_script(false).await;
    assert_eq!((saved.project.as_deref(), saved.checked), (Some("Web"), true));
}

// ------------------------- every refusal at once, and a dry run

/// Every file under `root`, by its path, with its bytes.
fn every_file(root: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                out.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

/// Case `id` as `case_7` writes it, under another id.
fn case_as(id: i32, selector: &str, value: &str) -> serde_json::Value {
    let mut v = case_7(selector, value);
    v[0]["case_id"] = serde_json::json!(id);
    v
}

/// A bundle whose scripts name locators never seen lists every one of
/// them, case by case, each line after its case.
#[tokio::test]
async fn a_bundle_refusal_names_every_unseen_locator_of_every_case() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[
        (7, "Save a rating", &["", "A toast says Saved"]),
        (8, "Save a rating", &["", "A toast says Saved"]),
    ])
    .await;
    // The page was seen; neither Save button nor toast was.
    see_scripts(dir.path(), &serde_json::json!([{ "case_id": 1, "title": "x", "steps": [{ "step_number": 1, "actions": [
        { "kind": "navigate", "url": "https://app.example/ratings" }
    ]}]}]).to_string());
    let both = serde_json::Value::Array(
        [case_as(7, ".toast", "Saved"), case_as(8, ".toast", "Saved")].into_iter().map(|v| v[0].clone()).collect(),
    );
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &both.to_string(), "1.0.0").await;
    let expected = [
        format!("case 7: {}", never_seen(2, "#save")),
        format!("case 7: {}", never_seen(2, ".toast")),
        format!("case 8: {}", never_seen(2, "#save")),
        format!("case 8: {}", never_seen(2, ".toast")),
    ];
    assert_eq!((status, out), (400, expected.join("\n")));
    assert_eq!(load_script(dir.path(), 7).unwrap(), None);
    assert_eq!(load_script(dir.path(), 8).unwrap(), None);
}

/// A dry run that passes every check says it would save, and saves
/// nothing. It travels through the tool as `dry_run` beside `scripts`.
#[tokio::test]
async fn a_dry_run_that_passes_says_it_would_save() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let scripts = case_7("#toast", "Saved");
    see_scripts(dir.path(), &scripts.to_string());

    let body = tool_body(serde_json::json!({ "scripts": scripts, "dry_run": true }));
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (200, "would save 1 script(s): case 7 (new)"));
    assert_eq!(load_script(dir.path(), 7).unwrap(), None, "a dry run wrote the script");

    // The scripts as a string, as sibling tools take them, and false.
    let body = tool_body(serde_json::json!({ "scripts": scripts.to_string(), "dry_run": true }));
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (200, "would save 1 script(s): case 7 (new)"));
    let body = tool_body(serde_json::json!({ "scripts": scripts, "dry_run": false }));
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.starts_with("saved 1 script(s)"), "{out}");
    assert!(load_script(dir.path(), 7).unwrap().is_some());

    // Anything but true or false is refused, before anything is read.
    let odd = serde_json::json!({ "scripts": scripts, "dry_run": "yes" }).to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &odd, "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, "\"dry_run\" is true or false."));
}

/// A dry run writes nothing and records nothing, refused or not: no
/// script, no map, no stamp, no repair counted, no quirk. Every file under
/// the Auto Run folder is the same, byte for byte. A refused dry run
/// answers the refusal a save would.
#[tokio::test]
async fn a_dry_run_writes_and_records_nothing() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_cases(&[(7, "Save a rating", &["", "A toast says Saved"])]).await;
    let first = case_7("#toast", "Saved").to_string();
    see_scripts(dir.path(), &first);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let before = every_file(dir.path());

    let dry = |selector: &str, quirk: bool| {
        let mut edit = edit_step_2("the toast moved");
        if quirk {
            edit["quirk"] = serde_json::json!("Toasts fade after two seconds");
        }
        serde_json::json!({ "scripts": case_7(selector, "Saved"), "edits": [edit], "dry_run": true }).to_string()
    };
    // Refused: the same answer as the save, and nothing changes.
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &dry(".toast", true), "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, never_seen(2, ".toast").as_str()));
    assert_eq!(every_file(dir.path()), before, "a refused dry run changed a file");

    // Passing: would save, and still nothing changes - no repair counted,
    // no quirk recorded, the map left as it was.
    see_scripts(dir.path(), &case_7(".toast", "Saved").to_string());
    let seen = every_file(dir.path());
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &dry(".toast", true), "1.0.0").await;
    assert_eq!((status, out.as_str()), (200, "would save 1 script(s): case 7 (repaired, 1 of 3 used)"));
    assert_eq!(every_file(dir.path()), seen, "a dry run that passed changed a file");
    let kept = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(kept.repairs, 0);
    assert!(load_quirks(dir.path(), "acme", "Web").unwrap().is_empty());
}
