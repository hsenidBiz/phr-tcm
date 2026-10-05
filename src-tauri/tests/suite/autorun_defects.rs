//! The suspected application defect: a mark on a case's script saying the
//! script is right and the application did not do what the case expects.
//!
//! Set only by `mark_autorun_suspected_defect` (the bridge's
//! `/autorun-defect`), through `store::set_suspected_defect`. Every save
//! path keeps the mark already on disk and ignores an incoming one; an
//! assistant's repair of the marked step is the one save that removes it,
//! and a person can clear it from the app.

use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{autorun_guard_for, route, BridgeContext};
use crate::common;
use std::sync::atomic::AtomicBool;
use v2_lib::autorun::defects::{append_cleared, check_mark, passed};
use v2_lib::autorun::failures::describe_failures;
use v2_lib::autorun::publish::comment_for;
use v2_lib::autorun::replay::{propose, run_selection, Browsers};
use v2_lib::browser::timing::Timing;
use v2_lib::autorun::quirks::{count_saved_run, source_from_run};
use v2_lib::autorun::store::{
    clear_scripts, clear_suspected_defect_at, load_script, save_run, save_script, save_scripts_atomically, set_root,
    set_suspected_defect,
};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepRecord, SuspectedDefect};
use v2_lib::browser::actions::ActionOutcome;
use v2_lib::commands::autorun::{clear_suspected_defect, save_script_from_editor};
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
        let dir = std::env::temp_dir().join(format!("tcm-autorun-defects-{nanos}-{n}"));
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

const NOW: u64 = 1_700_000_000_123;

fn ctx() -> BridgeContext {
    BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() }
}

/// Case `case_id`: three steps, each one click, step 3 also checking the
/// toast. Two repairs in and a reason on the last, so a test can see the
/// mark leave both alone.
fn script(case_id: i32) -> CaseScript {
    serde_json::from_value(serde_json::json!({
        "case_id": case_id,
        "title": "Save a rating",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#open" }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": "#edit" }] },
            { "step_number": 3, "actions": [
                { "kind": "click", "selector": "#save" },
                { "kind": "expect_contains_text", "selector": "#toast", "value": "Saved" }
            ]}
        ],
        "repairs": 2,
        "last_repair": "the edit button moved"
    }))
    .unwrap()
}

fn step(n: i32, outcomes: Vec<ActionOutcome>) -> StepRecord {
    StepRecord { step_number: n, outcomes, screenshot: None }
}

/// Case `case_id` as a run recorded it: steps 1 and 2 passed, step 3's
/// check failed on what the application showed.
fn failed_at_3(case_id: i32) -> CaseRecord {
    CaseRecord {
        case_id,
        title: "Save a rating".into(),
        verdict: String::new(),
        note: String::new(),
        steps: vec![
            step(1, vec![ActionOutcome::passed("ok")]),
            step(2, vec![ActionOutcome::passed("ok")]),
            step(
                3,
                vec![
                    ActionOutcome::passed("ok"),
                    ActionOutcome::failed("waited 5000ms: \"#toast\" never contained \"Saved\" - it said \"Error 500\""),
                ],
            ),
        ],
        proposed: "Failed".into(),
        reason: "step 3: the toast said Error 500".into(),
        duration_ms: None,
        account: None,
        retried: None,
    }
}

fn run_of(id: &str, started_at: &str, cases: Vec<CaseRecord>) -> LocalRun {
    LocalRun {
        id: id.into(),
        pbi_id: 1,
        started_at: started_at.into(),
        cases,
        mode: "unattended".into(),
        published: None,
        environment: None,
    }
}

fn mark(step_number: i32, note: &str) -> SuspectedDefect {
    SuspectedDefect { step_number, note: note.into(), marked_at: "1".into() }
}

// ------------------------------------------------------------ check_mark

#[test]
fn a_case_with_no_script_has_nothing_to_mark() {
    let run = run_of("run-1", "1", vec![failed_at_3(501)]);
    let err = check_mark(Some(&run), None, 501, 3, "the toast said Error 500", NOW).unwrap_err();
    assert_eq!(err, "case 501 has no script to mark");
}

#[test]
fn a_step_the_script_does_not_have_is_refused() {
    let run = run_of("run-1", "1", vec![failed_at_3(501)]);
    let err = check_mark(Some(&run), Some(&script(501)), 501, 9, "the toast said Error 500", NOW).unwrap_err();
    assert_eq!(err, "step 9 is not in the script");
}

/// The same rule, and the same sentence, `record_autorun_quirk` uses for
/// its `cases`: only a step that failed in the case's newest run.
#[test]
fn a_step_that_did_not_fail_in_the_newest_run_is_refused_with_the_quirk_sentence() {
    let run = run_of("run-1", "1", vec![failed_at_3(501)]);
    let sc = script(501);
    let err = check_mark(Some(&run), Some(&sc), 501, 2, "the toast said Error 500", NOW).unwrap_err();
    let expected = source_from_run(Some(&run), Some(&sc), 501, &[2]).unwrap_err();
    assert_eq!(err, expected);
    assert!(err.contains("case 501 step 2 did not fail"), "{err}");
}

/// A sign-in that failed, a browser that stopped answering and a case the
/// person marked Blocked are `STOP:` failures: never the application's
/// defect, even with the marked step failed too.
#[test]
fn a_stop_failure_is_not_an_application_defect() {
    let mut signin = failed_at_3(501);
    signin.steps.insert(0, step(0, vec![ActionOutcome::failed("the password was refused")]));
    let mut browser = failed_at_3(501);
    browser.reason = "the browser stopped answering after step 2".into();
    let mut blocked = failed_at_3(501);
    blocked.verdict = "Blocked".into();
    for case in [signin, browser, blocked] {
        let shown = format!("{case:?}");
        let run = run_of("run-1", "1", vec![case]);
        let err = check_mark(Some(&run), Some(&script(501)), 501, 3, "the toast said Error 500", NOW).unwrap_err();
        assert_eq!(err, "a STOP failure is not an application defect", "{shown}");
    }
}

#[test]
fn the_note_is_not_blank_and_at_most_300_characters() {
    let run = run_of("run-1", "1", vec![failed_at_3(501)]);
    let sc = script(501);
    let err = check_mark(Some(&run), Some(&sc), 501, 3, "   ", NOW).unwrap_err();
    assert_eq!(err, "a suspected defect needs a note saying what the application did");
    let err = check_mark(Some(&run), Some(&sc), 501, 3, &"x".repeat(301), NOW).unwrap_err();
    assert_eq!(err, "the note is longer than 300 characters");
    // Measured on the trimmed note: 300 characters with spaces round it fit.
    let ok = check_mark(Some(&run), Some(&sc), 501, 3, &format!("  {}  ", "é".repeat(300)), NOW).unwrap();
    assert_eq!(ok.note, "é".repeat(300));
}

#[test]
fn a_failed_application_step_is_marked_now() {
    let run = run_of("run-1", "1", vec![failed_at_3(501)]);
    let got = check_mark(
        Some(&run),
        Some(&script(501)),
        501,
        3,
        "  saving answers Error 500; the case expects the Saved toast ",
        NOW,
    )
    .unwrap();
    assert_eq!(
        got,
        SuspectedDefect {
            step_number: 3,
            note: "saving answers Error 500; the case expects the Saved toast".into(),
            marked_at: NOW.to_string(),
        }
    );
}

/// The note goes through the same address and token scrub API-check
/// excerpts do: no host, no query, no bearer token or JWT.
#[test]
fn the_note_is_stored_without_an_address_or_a_token() {
    let run = run_of("run-1", "1", vec![failed_at_3(501)]);
    let got = check_mark(
        Some(&run),
        Some(&script(501)),
        501,
        3,
        "saved at https://app.example/x?token=abc with Bearer eyJa.b.c",
        NOW,
    )
    .unwrap();
    assert!(!got.note.contains("://"), "{}", got.note);
    assert!(!got.note.contains("token=abc"), "{}", got.note);
    assert!(!got.note.contains("eyJ"), "{}", got.note);
    assert!(got.note.starts_with("saved at "), "{}", got.note);
}

// ------------------------------------------------------------ the store

fn raw(dir: &std::path::Path, case_id: i32) -> serde_json::Value {
    let text = std::fs::read_to_string(dir.join("scripts").join(format!("case-{case_id}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn setting_a_mark_leaves_the_steps_and_the_repairs_exactly_as_they_were() {
    let dir = TempDir::new();
    save_script(dir.path(), &script(501)).unwrap();
    let before = raw(dir.path(), 501);

    set_suspected_defect(dir.path(), 501, Some(mark(3, "first"))).unwrap();
    let after = raw(dir.path(), 501);
    for key in ["steps", "repairs", "last_repair", "title", "case_id"] {
        assert_eq!(after[key], before[key], "{key}");
    }
    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, Some(mark(3, "first")));

    // One mark per case: a second replaces the first.
    set_suspected_defect(dir.path(), 501, Some(mark(2, "second"))).unwrap();
    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, Some(mark(2, "second")));

    // None removes it, and the file goes back to what it was.
    set_suspected_defect(dir.path(), 501, None).unwrap();
    let cleared = raw(dir.path(), 501);
    assert!(cleared.get("suspected_defect").is_none(), "{cleared}");
    assert_eq!(cleared, before);
    // No staging file left behind.
    let names: Vec<String> = std::fs::read_dir(dir.path().join("scripts"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["case-501.json".to_string()]);
}

#[test]
fn marking_a_case_with_no_script_is_refused() {
    let dir = TempDir::new();
    assert!(set_suspected_defect(dir.path(), 501, Some(mark(3, "x"))).is_err());
    assert!(load_script(dir.path(), 501).unwrap().is_none(), "no file appears");
}

/// Every save keeps the mark already on disk and ignores the one it was
/// sent: a stale copy can neither drop a mark nor bring back a cleared one,
/// and only `set_suspected_defect` changes it.
#[test]
fn a_bundle_save_keeps_the_mark_on_disk_whatever_it_was_sent() {
    let dir = TempDir::new();
    save_script(dir.path(), &script(501)).unwrap();
    set_suspected_defect(dir.path(), 501, Some(mark(3, "on disk"))).unwrap();

    // Sent with none.
    let mut sent = script(501);
    sent.title = "Save a rating, renamed".into();
    save_scripts_atomically(dir.path(), std::slice::from_ref(&sent)).unwrap();
    let saved = load_script(dir.path(), 501).unwrap().unwrap();
    assert_eq!(saved.title, "Save a rating, renamed", "the save itself landed");
    assert_eq!(saved.suspected_defect, Some(mark(3, "on disk")));

    // Sent with a different one: the disk wins.
    sent.suspected_defect = Some(mark(1, "sent"));
    save_scripts_atomically(dir.path(), std::slice::from_ref(&sent)).unwrap();
    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, Some(mark(3, "on disk")));

    // A brand new script saves with none, whatever it carried.
    let mut fresh = script(502);
    fresh.suspected_defect = Some(mark(3, "sent"));
    save_scripts_atomically(dir.path(), std::slice::from_ref(&fresh)).unwrap();
    assert_eq!(load_script(dir.path(), 502).unwrap().unwrap().suspected_defect, None);

    // And a cleared mark is not brought back by a stale copy.
    set_suspected_defect(dir.path(), 501, None).unwrap();
    sent.suspected_defect = Some(mark(3, "on disk"));
    save_scripts_atomically(dir.path(), std::slice::from_ref(&sent)).unwrap();
    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, None);
}

/// The editor's save resets the repair count, and keeps the mark.
#[test]
fn the_editor_save_keeps_the_mark() {
    let dir = TempDir::new();
    save_script(dir.path(), &script(501)).unwrap();
    set_suspected_defect(dir.path(), 501, Some(mark(3, "on disk"))).unwrap();
    save_script_from_editor(dir.path(), "acme", "Web", script(501)).unwrap();
    let saved = load_script(dir.path(), 501).unwrap().unwrap();
    assert_eq!(saved.repairs, 0);
    assert_eq!(saved.suspected_defect, Some(mark(3, "on disk")));
}

/// M5: a save that removes the marked step drops the mark (it could never
/// label or pass again); one that keeps the step keeps it.
#[test]
fn a_save_that_removes_the_marked_step_drops_the_mark_and_one_that_keeps_it_keeps_it() {
    let dir = TempDir::new();
    save_script(dir.path(), &script(501)).unwrap();
    set_suspected_defect(dir.path(), 501, Some(mark(3, "on disk"))).unwrap();

    // The editor deletes step 3 (the marked one).
    let mut without = script(501);
    without.steps.retain(|s| s.step_number != 3);
    save_script_from_editor(dir.path(), "acme", "Web", without.clone()).unwrap();
    let saved = load_script(dir.path(), 501).unwrap().unwrap();
    assert_eq!(saved.steps.len(), 2, "the save itself landed");
    assert_eq!(saved.suspected_defect, None);
    assert!(raw(dir.path(), 501).get("suspected_defect").is_none());

    // A save that keeps the marked step keeps the mark (and so does a
    // bundle save that deletes some other step).
    save_script(dir.path(), &script(502)).unwrap();
    set_suspected_defect(dir.path(), 502, Some(mark(3, "on disk"))).unwrap();
    let mut other = script(502);
    other.steps.retain(|s| s.step_number != 1);
    save_scripts_atomically(dir.path(), std::slice::from_ref(&other)).unwrap();
    assert_eq!(load_script(dir.path(), 502).unwrap().unwrap().suspected_defect, Some(mark(3, "on disk")));
}

/// M2: a compare-and-clear. It clears only the mark that is on that step.
#[test]
fn clearing_a_step_removes_only_a_mark_that_is_on_that_step() {
    let dir = TempDir::new();
    save_script(dir.path(), &script(501)).unwrap();
    let before = raw(dir.path(), 501);

    // No mark at all: nothing to clear.
    assert_eq!(clear_suspected_defect_at(dir.path(), 501, 3), Ok(false));

    // A mark on that step goes, and nothing else changes.
    set_suspected_defect(dir.path(), 501, Some(mark(3, "on disk"))).unwrap();
    assert_eq!(clear_suspected_defect_at(dir.path(), 501, 3), Ok(true));
    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, None);
    assert_eq!(raw(dir.path(), 501), before);

    // A mark that moved to another step between the read and the clear
    // survives, and the clear says it did nothing.
    set_suspected_defect(dir.path(), 501, Some(mark(1, "moved"))).unwrap();
    assert_eq!(clear_suspected_defect_at(dir.path(), 501, 3), Ok(false));
    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, Some(mark(1, "moved")));

    // A case with no script is an error, like a set.
    assert!(clear_suspected_defect_at(dir.path(), 999, 3).is_err());
}

/// M1: deleting scripts and setting a mark never leave a deleted script
/// behind. The race is narrow, so this hammers it; the lock is what makes
/// it pass every time.
#[test]
fn clearing_scripts_never_lets_a_racing_mark_bring_one_back() {
    let dir = TempDir::new();
    for round in 0..200 {
        save_script(dir.path(), &script(501)).unwrap();
        let root = dir.path().to_path_buf();
        let setter = std::thread::spawn({
            let root = root.clone();
            move || {
                let _ = set_suspected_defect(&root, 501, Some(mark(3, "racing")));
            }
        });
        let clearer = std::thread::spawn({
            let root = root.clone();
            move || clear_scripts(&root, &[501]).unwrap()
        });
        setter.join().unwrap();
        clearer.join().unwrap();
        assert!(load_script(dir.path(), 501).unwrap().is_none(), "round {round}: a deleted script came back");
    }
}

/// The pure half of `auto_run_clear_suspected_defect`: the person's Clear.
#[test]
fn clearing_from_the_app_removes_the_mark_and_nothing_else() {
    let dir = TempDir::new();
    save_script(dir.path(), &script(501)).unwrap();
    let before = raw(dir.path(), 501);
    set_suspected_defect(dir.path(), 501, Some(mark(3, "on disk"))).unwrap();
    clear_suspected_defect(dir.path(), 501).unwrap();
    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, None);
    assert_eq!(raw(dir.path(), 501), before);
    // Clearing a case with no mark is not an error.
    clear_suspected_defect(dir.path(), 501).unwrap();
}

// ------------------------------------------------------------ the route

async fn post(body: serde_json::Value) -> (u16, String) {
    route(&ctx(), None, "POST", "/autorun-defect", &body.to_string(), "1.0.0").await
}

/// `mark_autorun_suspected_defect` reaches `/autorun-defect`: the newest
/// run of the case decides, the stored mark comes back, and each refusal
/// is a 400 with its sentence and writes nothing.
#[tokio::test]
async fn the_route_stores_the_mark_or_refuses_with_the_sentence() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    save_script(dir.path(), &script(501)).unwrap();
    save_script(dir.path(), &script(503)).unwrap();
    let mut blocked = failed_at_3(503);
    blocked.verdict = "Blocked".into();
    // An older run where 501's step 2 failed, and the newest, where only
    // step 3 did: the newest decides.
    let mut older = failed_at_3(501);
    older.steps[1] = step(2, vec![ActionOutcome::failed("waited 5000ms: button \"Edit\" not found")]);
    save_run(dir.path(), &run_of("run-1700000000000", "1700000000000", vec![older])).unwrap();
    save_run(dir.path(), &run_of("run-1700000000500", "1700000000500", vec![failed_at_3(501), failed_at_3(502), blocked]))
        .unwrap();

    let refusals: Vec<(serde_json::Value, String)> = vec![
        (serde_json::json!({ "case_id": 502, "step_number": 3, "note": "n" }), "case 502 has no script to mark".into()),
        (serde_json::json!({ "case_id": 501, "step_number": 9, "note": "n" }), "step 9 is not in the script".into()),
        (
            serde_json::json!({ "case_id": 501, "step_number": 2, "note": "n" }),
            "case 501 step 2 did not fail in the newest run of that case on this machine (run-1700000000500) - name only steps that failed there, or leave the case out".into(),
        ),
        (
            serde_json::json!({ "case_id": 503, "step_number": 3, "note": "n" }),
            "a STOP failure is not an application defect".into(),
        ),
        (
            serde_json::json!({ "case_id": 501, "step_number": 3, "note": " " }),
            "a suspected defect needs a note saying what the application did".into(),
        ),
        (
            serde_json::json!({ "case_id": 501, "step_number": 3, "note": "x".repeat(301) }),
            "the note is longer than 300 characters".into(),
        ),
    ];
    for (body, sentence) in refusals {
        let (status, out) = post(body.clone()).await;
        assert_eq!((status, out.as_str()), (400, sentence.as_str()), "{body}");
    }
    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, None, "a refusal writes nothing");

    let (status, out) = post(serde_json::json!({ "case_id": 501 })).await;
    assert_eq!(status, 400, "{out}");

    let (status, out) =
        post(serde_json::json!({ "case_id": 501, "step_number": 3, "note": "saving answers Error 500" })).await;
    assert_eq!(status, 200, "{out}");
    let answered: SuspectedDefect = serde_json::from_str(&out).unwrap();
    assert_eq!(answered.step_number, 3);
    assert_eq!(answered.note, "saving answers Error 500");
    let stored = load_script(dir.path(), 501).unwrap().unwrap();
    assert_eq!(stored.suspected_defect, Some(answered));
    assert_eq!(stored.steps, script(501).steps, "the script's actions are untouched");
    assert_eq!((stored.repairs, stored.last_repair.as_deref()), (2, Some("the edit button moved")));
}

/// Offered only where Auto Run is, like `record_autorun_quirk`.
#[test]
fn the_route_is_refused_where_auto_run_is_not_offered() {
    let (status, body) = autorun_guard_for("/autorun-defect", false).expect("refused outside Auto Run");
    assert_eq!((status, body.as_str()), (404, "not available in this build"));
    assert!(autorun_guard_for("/autorun-defect", true).is_none());
}

// --------------------------------------------- an assistant's repair

/// Azure DevOps standing in for case 7's lookup: three steps, nothing
/// expected, so the floor asks for no check.
async fn client_with_case_7() -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let steps: Vec<Step> =
        (1..=3).map(|i| Step { action: format!("Step {i}"), expected: String::new(), shared: None }).collect();
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [{
            "id": 7,
            "fields": { "System.Title": "Save a rating", "Microsoft.VSTS.TCM.Steps": build_steps_xml(&steps) }
        }] })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

/// Case 7 with each named step's click on its own selector.
fn case_7_with(clicks: &[(i32, &str)]) -> serde_json::Value {
    let mut sc = serde_json::to_value(script(7)).unwrap();
    sc.as_object_mut().unwrap().remove("repairs");
    sc.as_object_mut().unwrap().remove("last_repair");
    for (n, selector) in clicks {
        sc["steps"][(n - 1) as usize]["actions"][0]["selector"] = serde_json::json!(selector);
    }
    serde_json::json!([sc])
}

/// A repair of another step keeps the mark; a repair whose declared steps
/// include the marked one removes it - the assistant has decided the step
/// was the script's fault after all - and the answer says so.
#[tokio::test]
async fn a_repair_of_the_marked_step_removes_the_mark_and_of_another_keeps_it() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_case_7().await;
    let first = case_7_with(&[]).to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    set_suspected_defect(dir.path(), 7, Some(mark(3, "saving answers Error 500"))).unwrap();

    let other = serde_json::json!({
        "scripts": case_7_with(&[(2, "#edit-button")]),
        "edits": [{ "case_id": 7, "steps": [2], "why": "the edit button has a new id" }],
    })
    .to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &other, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(!out.contains("suspected defect"), "{out}");
    assert_eq!(
        load_script(dir.path(), 7).unwrap().unwrap().suspected_defect,
        Some(mark(3, "saving answers Error 500"))
    );

    let marked = serde_json::json!({
        "scripts": case_7_with(&[(2, "#edit-button"), (3, "#save-button")]),
        "edits": [{ "case_id": 7, "steps": [3], "why": "the save button has a new id" }],
    })
    .to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &marked, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("suspected defect at step 3 cleared - the step was repaired"), "{out}");
    let saved = load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(saved.suspected_defect, None);
    assert_eq!(saved.repairs, 2, "two repairs, the mark's removal no extra one");
}

// ------------------------------------------------------------- in runs

const MARK_NOTE: &str = "saving answers Error 500";

/// `script(case_id)` carrying a mark on step 3.
fn marked_script(case_id: i32) -> CaseScript {
    let mut sc = script(case_id);
    sc.suspected_defect = Some(mark(3, MARK_NOTE));
    sc
}

fn ok_step(n: i32) -> StepRecord {
    step(n, vec![ActionOutcome::passed("ok")])
}

fn bad_step(n: i32, why: &str) -> StepRecord {
    step(n, vec![ActionOutcome::failed(why)])
}

#[test]
fn a_failure_at_the_marked_step_is_labelled_and_keeps_the_usual_sentence() {
    let steps = vec![ok_step(1), ok_step(2), bad_step(3, "the toast said Error 500")];
    let got = propose(&marked_script(501), &steps, None, false);
    assert_eq!(got.verdict, "Failed");
    assert_eq!(
        got.reason,
        "Suspected application defect at step 3: saving answers Error 500 - step 3: the toast said Error 500"
    );
    // An unmarked script gives the ordinary sentence.
    let plain = propose(&script(501), &steps, None, false);
    assert_eq!((plain.verdict, plain.reason.as_str()), ("Failed", "step 3: the toast said Error 500"));
}

#[test]
fn a_failure_at_another_step_is_an_ordinary_failure() {
    let steps = vec![ok_step(1), bad_step(2, "button \"Edit\" not found")];
    let got = propose(&marked_script(501), &steps, None, false);
    assert_eq!((got.verdict, got.reason.as_str()), ("Failed", "step 2: button \"Edit\" not found"));
}

#[test]
fn a_blocked_case_is_not_labelled() {
    let mut harness = ActionOutcome::failed("gone");
    harness.harness = true;
    let steps = vec![ok_step(1), ok_step(2), step(3, vec![harness])];
    let got = propose(&marked_script(501), &steps, None, false);
    assert_eq!(got.verdict, "Blocked");
    assert!(!got.reason.contains("Suspected"), "{}", got.reason);
}

#[test]
fn the_marked_step_passed_only_when_it_ran_and_every_outcome_passed() {
    let sc = marked_script(501);
    assert!(passed(&sc, &[ok_step(1), ok_step(2), ok_step(3)]));
    assert!(!passed(&sc, &[ok_step(1), ok_step(2)]), "never reached");
    assert!(!passed(&sc, &[ok_step(1), ok_step(2), bad_step(3, "x")]));
    assert!(
        !passed(&sc, &[step(3, vec![ActionOutcome::passed("ok"), ActionOutcome::failed("x")])]),
        "one failed outcome in the step"
    );
    assert!(!passed(&sc, &[step(3, vec![])]), "no outcomes is not a pass");
    assert!(!passed(&script(501), &[ok_step(3)]), "no mark, nothing to clear");
}

struct Fake(std::collections::VecDeque<common::ScriptedDriver>);

impl Browsers for Fake {
    type D = common::ScriptedDriver;
    async fn open(&mut self) -> Result<Self::D, String> {
        self.0.pop_front().ok_or_else(|| "no browser".to_string())
    }
    async fn close(&mut self, _d: Self::D) {}
}

/// A page where a `check_text` passes only for the value "yes".
fn checking_driver() -> common::ScriptedDriver {
    common::ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" if params["expression"] == "document" => {
            Ok(serde_json::json!({ "result": { "objectId": "doc" } }))
        }
        "Runtime.callFunctionOn" => {
            let arg = params["arguments"][0]["value"].as_str().unwrap_or("");
            Ok(serde_json::json!({ "result": { "value": arg == "yes" } }))
        }
        "Page.captureScreenshot" => Ok(serde_json::json!({ "data": "/9j/4AAQ" })),
        _ => Ok(serde_json::json!({})),
    })
}

/// Case 1: three `check_text` steps whose values say whether each passes.
fn checks(values: [&str; 3]) -> CaseScript {
    serde_json::from_value(serde_json::json!({
        "case_id": 1,
        "title": "case 1",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": values[0] }] },
            { "step_number": 2, "actions": [{ "kind": "check_text", "value": values[1] }] },
            { "step_number": 3, "actions": [{ "kind": "check_text", "value": values[2] }] }
        ]
    }))
    .unwrap()
}

/// Saves `sc` with its mark (the mark only lands through the store), then
/// runs case 1 unattended on `checking_driver`.
async fn unattended(root: &std::path::Path, sc: &CaseScript) -> LocalRun {
    save_script(root, sc).unwrap();
    set_suspected_defect(root, sc.case_id, sc.suspected_defect.clone()).unwrap();
    let mut browsers = Fake([checking_driver()].into());
    let mut run = run_of("run-x", "1700000000000", vec![]);
    let quick = Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 20, highlight_ms: 0, lease_wait_ms: 300 };
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(sc.case_id, sc.title.clone())], &quick, &cancel, &mut |_| {})
        .await
        .unwrap();
    run
}

#[tokio::test]
async fn an_unattended_run_that_passes_the_marked_step_clears_the_mark_and_says_so() {
    let dir = TempDir::new();
    let mut sc = checks(["yes", "yes", "yes"]);
    sc.suspected_defect = Some(mark(3, MARK_NOTE));
    let run = unattended(dir.path(), &sc).await;
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Passed");
    assert_eq!(
        rec.reason,
        "every action of 3 steps passed. The suspected defect at step 3 did not happen this time - the mark was cleared."
    );
    let stored = load_script(dir.path(), 1).unwrap().unwrap();
    assert_eq!(stored.suspected_defect, None);
    assert_eq!(stored.steps, sc.steps, "the script's actions are untouched");
}

/// The cleared sentence joins the reason as a sentence of its own: after a
/// full stop when the reason has none, after a space when it already ends
/// in one, and alone when there is no reason.
#[test]
fn the_cleared_sentence_is_joined_to_the_reason_as_its_own_sentence() {
    let cleared = "The suspected defect at step 3 did not happen this time - the mark was cleared.";
    assert_eq!(append_cleared("every action passed", 3), format!("every action passed. {cleared}"));
    for end in [".", "!", "?"] {
        assert_eq!(append_cleared(&format!("all good{end}"), 3), format!("all good{end} {cleared}"));
    }
    assert_eq!(append_cleared("all good. ", 3), format!("all good. {cleared}"));
    assert_eq!(append_cleared("", 3), cleared);
    assert_eq!(append_cleared("  ", 3), cleared);
}

#[tokio::test]
async fn an_unattended_run_that_fails_the_marked_step_keeps_the_mark_and_labels_the_failure() {
    let dir = TempDir::new();
    let mut sc = checks(["yes", "yes", "no"]);
    sc.suspected_defect = Some(mark(3, MARK_NOTE));
    let run = unattended(dir.path(), &sc).await;
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Failed");
    assert!(
        rec.reason.starts_with("Suspected application defect at step 3: saving answers Error 500 - step 3: "),
        "{}",
        rec.reason
    );
    assert!(!rec.reason.contains("was cleared"), "{}", rec.reason);
    assert_eq!(load_script(dir.path(), 1).unwrap().unwrap().suspected_defect, Some(mark(3, MARK_NOTE)));
}

#[tokio::test]
async fn an_unattended_run_that_stops_before_the_marked_step_keeps_the_mark() {
    let dir = TempDir::new();
    let mut sc = checks(["yes", "no", "yes"]);
    sc.suspected_defect = Some(mark(3, MARK_NOTE));
    let run = unattended(dir.path(), &sc).await;
    let rec = &run.cases[0];
    assert!(rec.reason.starts_with("step 2: "), "{}", rec.reason);
    assert!(!rec.reason.contains("was cleared"), "{}", rec.reason);
    assert_eq!(load_script(dir.path(), 1).unwrap().unwrap().suspected_defect, Some(mark(3, MARK_NOTE)));
}

#[test]
fn counting_a_supervised_run_that_passed_the_marked_step_clears_the_mark_and_leaves_the_run_alone() {
    let dir = TempDir::new();
    save_script(dir.path(), &script(501)).unwrap();
    set_suspected_defect(dir.path(), 501, Some(mark(3, MARK_NOTE))).unwrap();
    let mut case = failed_at_3(501);
    case.steps[2] = ok_step(3);
    case.proposed = String::new();
    case.reason = String::new();
    let mut run = run_of("run-60", "60", vec![case]);
    run.mode = "supervised".into();
    save_run(dir.path(), &run).unwrap();
    let file = dir.path().join("runs").join("run-60.json");
    let before = std::fs::read(&file).unwrap();

    count_saved_run(dir.path(), "Acme", "Web", "run-60", 5).unwrap();

    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, None);
    assert_eq!(std::fs::read(&file).unwrap(), before, "a person's run carries no machine reason");
}

#[test]
fn counting_a_supervised_run_that_failed_the_marked_step_keeps_the_mark() {
    let dir = TempDir::new();
    save_script(dir.path(), &script(501)).unwrap();
    set_suspected_defect(dir.path(), 501, Some(mark(3, MARK_NOTE))).unwrap();
    save_run(dir.path(), &run_of("run-61", "61", vec![failed_at_3(501)])).unwrap();
    count_saved_run(dir.path(), "Acme", "Web", "run-61", 5).unwrap();
    assert_eq!(load_script(dir.path(), 501).unwrap().unwrap().suspected_defect, Some(mark(3, MARK_NOTE)));
}

#[test]
fn the_failures_text_names_the_mark_and_a_clean_case_has_no_such_line() {
    let run = run_of("run-70", "70", vec![failed_at_3(501), failed_at_3(502)]);
    let text = describe_failures(&run, &[marked_script(501), script(502)]);
    let (first, second) = text.split_once("## Case 502").expect("both cases listed");
    assert!(first.contains("suspected defect at step 3: saving answers Error 500"), "{first}");
    assert!(!second.contains("suspected defect"), "{second}");
}

#[test]
fn the_result_comment_carries_the_defect_reason() {
    let mut case = failed_at_3(501);
    case.reason =
        "Suspected application defect at step 3: saving answers Error 500 - step 3: the toast said Error 500".into();
    assert_eq!(
        comment_for(&case),
        "Auto Run: Suspected application defect at step 3: saving answers Error 500 - step 3: the toast said Error 500"
    );
}
