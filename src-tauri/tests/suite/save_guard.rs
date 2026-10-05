//! No-save scripts (run safety §1): which requests are saves, what the
//! case says when one is stopped, the CDP client answering every paused
//! request at once, and the runner failing the case.
//!
//! The browser half is proven against real Edge in `browser_live.rs`
//! (`a_no_save_case_*`); this file holds the pure rules and the fakes.

use crate::common;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use v2_lib::ai_bridge::{try_case_id, try_in, TRY_NEEDS_CASE};
use v2_lib::autorun::edits::check_edits;
use v2_lib::autorun::guide::autorun_guide;
use v2_lib::autorun::nav::{load_nav, set_save_words, view};
use v2_lib::autorun::patterns::{classify, ErrorClass};
use v2_lib::autorun::replay::{propose, run_case, MODULE_STEP};
use v2_lib::autorun::transient::is_transient;
use v2_lib::autorun::{CaseRecord, CaseScript, StepRecord};
use v2_lib::browser::actions::ActionOutcome;
use v2_lib::browser::cdp::{Cdp, CdpError, Transport};
use v2_lib::browser::save_guard::{
    blocked, check_words, is_blocked, is_save, path_of, setup_failed, SAVE_WORDS,
};
use v2_lib::browser::timing::Timing;
use v2_lib::commands::autorun::guard_for_case;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

const SENTENCE: &str =
    "this script must not save, but the page tried to send POST /api/Save - it was stopped before it reached the server";

// ---------------------------------------------------------------- is_save

#[test]
fn every_writing_method_with_every_save_word_is_a_save() {
    for method in ["POST", "PUT", "PATCH", "DELETE", "post", "Put"] {
        for word in SAVE_WORDS {
            let url = format!("https://hr.example/api/Cycle{}Now", word.to_uppercase());
            assert!(is_save(method, &url, &[]), "{method} {url} should be a save");
        }
    }
}

#[test]
fn a_reading_method_is_never_a_save() {
    for method in ["GET", "HEAD", "OPTIONS", "get"] {
        assert!(!is_save(method, "https://hr.example/api/Save", &[]), "{method}");
        assert!(!is_save(method, "https://hr.example/api/search", &words(&["search"])), "{method}");
    }
}

#[test]
fn a_writing_method_without_a_save_word_goes_through() {
    assert!(!is_save("POST", "https://hr.example/api/Search", &[]));
    assert!(!is_save("PUT", "https://hr.example/hr/leave/apply", &[]));
}

#[test]
fn the_query_and_the_fragment_are_left_out() {
    assert!(!is_save("POST", "https://hr.example/api/Search?then=save&x=update", &[]));
    assert!(!is_save("POST", "https://hr.example/api/Search#save", &[]));
    assert!(is_save("POST", "https://hr.example/api/SAVE?q=1", &[]));
}

#[test]
fn the_host_is_left_out() {
    assert!(!is_save("POST", "https://save.example/api/Search", &[]));
    assert!(!is_save("POST", "https://update-server:8080/api/Search", &[]));
}

#[test]
fn a_project_pattern_blocks_its_own_path_ignoring_case() {
    assert!(is_save("POST", "https://hr.example/api/Search", &words(&["search"])));
    assert!(is_save("POST", "https://hr.example/api/Search", &words(&["  SEARCH "])));
    assert!(is_save("PATCH", "https://hr.example/hr/Recalculate/7", &words(&["recalc"])));
    assert!(!is_save("POST", "https://hr.example/api/Search", &words(&["", "  "])), "a blank pattern matches nothing");
}

#[test]
fn the_path_of_an_address_has_no_host_query_or_fragment() {
    assert_eq!(path_of("https://hr.example:8443/api/Save?token=secret#x"), "/api/Save");
    assert_eq!(path_of("http://127.0.0.1:5000"), "/");
}

// ---------------------------------------------------------------- the sentence

#[test]
fn the_sentence_names_the_method_and_the_path_only() {
    let s = blocked("post", "http://127.0.0.1:61234/api/Save?token=hunter2");
    assert_eq!(s, SENTENCE);
    assert!(!s.contains("127.0.0.1") && !s.contains("hunter2"), "{s}");
    assert!(is_blocked(&s));
    assert!(!is_blocked("step 1: clicked button \"Save\""));
}

#[test]
fn the_setup_refusal_says_why() {
    assert_eq!(setup_failed("Fetch.enable was refused"), "the no-save guard could not be set up: Fetch.enable was refused");
}

// ---------------------------------------------------------------- project words

#[test]
fn project_words_are_kept_trimmed_lowercased_and_once() {
    assert_eq!(check_words(&words(&[" Recalc ", "SEARCH", "recalc"])).unwrap(), words(&["recalc", "search"]));
}

#[test]
fn a_project_word_is_refused_when_it_cannot_work() {
    assert!(check_words(&words(&["  "])).unwrap_err().contains("blank"));
    assert!(check_words(&words(&["save"])).unwrap_err().contains("already a built-in save word"));
    assert!(check_words(&words(&["a b"])).unwrap_err().contains("space"));
    assert!(check_words(&words(&["x?y"])).unwrap_err().contains("query"));
    assert!(check_words(&words(&[&"x".repeat(61)])).unwrap_err().contains("at most 60"));
}

#[test]
fn the_project_words_live_in_its_settings_file_and_its_view_shows_both_lists() {
    let dir = tempfile::tempdir().unwrap();
    let nav = set_save_words(dir.path(), "acme", "PMS", &words(&["Recalc"])).unwrap();
    assert_eq!(nav.save_words, words(&["recalc"]));
    assert_eq!(load_nav(dir.path(), "acme", "PMS").unwrap().save_words, words(&["recalc"]));
    let v = view(&nav);
    assert_eq!(v.save_words, words(&["recalc"]));
    assert_eq!(v.built_in_save_words, SAVE_WORDS.iter().map(|w| w.to_string()).collect::<Vec<_>>());
    // Removing goes through the same call.
    let nav = set_save_words(dir.path(), "acme", "PMS", &[]).unwrap();
    assert!(nav.save_words.is_empty());
    // A refused list leaves the file as it was.
    set_save_words(dir.path(), "acme", "PMS", &words(&["keep"])).unwrap();
    assert!(set_save_words(dir.path(), "acme", "PMS", &words(&["save"])).is_err());
    assert_eq!(load_nav(dir.path(), "acme", "PMS").unwrap().save_words, words(&["keep"]));
}

#[test]
fn a_settings_file_without_save_words_loads_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let path = v2_lib::autorun::nav::nav_path(dir.path(), "acme", "PMS");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{ "direct_urls": false, "modules": [] }"#).unwrap();
    let nav = load_nav(dir.path(), "acme", "PMS").unwrap();
    assert!(nav.save_words.is_empty());
    assert!(!nav.direct_urls);
}

// ---------------------------------------------------------------- the CDP client

struct FakeTransport {
    incoming: VecDeque<String>,
    sent: Vec<String>,
    /// The first frame sent for this method never finishes writing - a
    /// write a deadline then cuts short.
    stall_once: Option<&'static str>,
}

impl FakeTransport {
    fn new(frames: &[&str]) -> Self {
        FakeTransport { incoming: frames.iter().map(|f| f.to_string()).collect(), sent: vec![], stall_once: None }
    }
}

impl Transport for FakeTransport {
    async fn send(&mut self, text: String) -> Result<(), String> {
        if self.stall_once.is_some_and(|m| text.contains(m)) {
            self.stall_once = None;
            std::future::pending::<()>().await;
        }
        self.sent.push(text);
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, String>> {
        match self.incoming.pop_front() {
            Some(f) => Some(Ok(f)),
            // Silent, not closed: `idle` must wait out its time.
            None => {
                std::future::pending::<()>().await;
                None
            }
        }
    }
}

fn paused(id: &str, method: &str, url: &str) -> String {
    json!({ "method": "Fetch.requestPaused", "params": {
        "requestId": id, "resourceType": "XHR",
        "request": { "method": method, "url": url, "headers": {} }
    } })
    .to_string()
}

fn sent(cdp: &Cdp<FakeTransport>) -> Vec<serde_json::Value> {
    cdp.transport().sent.iter().map(|s| serde_json::from_str(s).unwrap()).collect()
}

fn answer_to(cdp: &Cdp<FakeTransport>, request_id: &str) -> Option<serde_json::Value> {
    sent(cdp).into_iter().find(|f| f["params"]["requestId"] == request_id)
}

/// A client guarded with these words: the bypass and `Fetch.enable` took
/// ids 1 and 2, so its next frame is 3.
async fn guarded(patterns: &[&str]) -> Cdp<FakeTransport> {
    let mut cdp = Cdp::over(FakeTransport::new(&[r#"{"id":1,"result":{}}"#, r#"{"id":2,"result":{}}"#]));
    cdp.guard_saves(&words(patterns)).await.unwrap();
    cdp
}

#[tokio::test]
async fn guarding_bypasses_service_workers_then_intercepts_every_request() {
    let cdp = guarded(&["recalc"]).await;
    let f = sent(&cdp);
    assert_eq!(f[0]["method"], "Network.setBypassServiceWorker");
    assert_eq!(f[0]["params"]["bypass"], true);
    assert_eq!(f[1]["method"], "Fetch.enable");
    assert_eq!(f[1]["params"]["patterns"][0]["urlPattern"], "*");
    assert_eq!(f[1]["params"]["patterns"][0]["requestStage"], "Request");
    assert!(cdp.is_guarding_saves());
}

/// Fail closed: a browser that will not bypass its service workers is not
/// guarded at all, and interception is never even asked for.
#[tokio::test]
async fn a_refused_service_worker_bypass_leaves_the_client_unguarded() {
    let mut cdp = Cdp::over(FakeTransport::new(&[r#"{"id":1,"error":{"code":-32000,"message":"nope"}}"#]));
    let err = cdp.guard_saves(&[]).await.unwrap_err();
    assert!(err.to_string().contains("Network.setBypassServiceWorker") && err.to_string().contains("nope"), "{err}");
    assert!(!cdp.is_guarding_saves());
    assert_eq!(sent(&cdp).len(), 1, "Fetch.enable was asked for anyway");
}

#[tokio::test]
async fn a_refused_interception_is_an_error_and_leaves_the_client_unguarded() {
    let mut cdp = Cdp::over(FakeTransport::new(&[
        r#"{"id":1,"result":{}}"#,
        r#"{"id":2,"error":{"code":-32000,"message":"nope"}}"#,
    ]));
    let err = cdp.guard_saves(&[]).await.unwrap_err();
    assert!(err.to_string().contains("nope"), "{err}");
    assert!(!cdp.is_guarding_saves());
}

/// Every paused request is answered inside the event handling, while
/// another call waits for its own reply - and that reply still arrives.
#[tokio::test]
async fn paused_requests_are_answered_while_a_call_waits() {
    let mut cdp = guarded(&["search"]).await;
    let frames = [
        paused("r1", "POST", "https://hr.example/api/Save?token=x"),
        paused("r2", "GET", "https://hr.example/api/Save"),
        paused("r3", "POST", "https://hr.example/api/Search"),
        paused("r4", "POST", "https://hr.example/api/List"),
        r#"{"id":3,"result":{"done":true}}"#.to_string(),
    ];
    cdp.transport_mut().incoming.extend(frames);
    let got = cdp.call("Runtime.evaluate", json!({})).await.unwrap();
    assert_eq!(got["done"], true, "the call's own reply still came back");

    let r1 = answer_to(&cdp, "r1").expect("the save was never answered");
    assert_eq!(r1["method"], "Fetch.failRequest");
    assert_eq!(r1["params"]["errorReason"], "BlockedByClient");
    assert_eq!(answer_to(&cdp, "r2").unwrap()["method"], "Fetch.continueRequest");
    assert_eq!(answer_to(&cdp, "r3").unwrap()["method"], "Fetch.failRequest", "the project word blocks it");
    assert_eq!(answer_to(&cdp, "r4").unwrap()["method"], "Fetch.continueRequest");
    // Every answer has its own id, none reused from the call.
    let ids: Vec<u64> = sent(&cdp).iter().map(|f| f["id"].as_u64().unwrap()).collect();
    let mut unique = ids.clone();
    unique.dedup();
    assert_eq!(ids, unique);

    // The first save is what the case reports, once.
    assert_eq!(cdp.take_save_blocked().as_deref(), Some(SENTENCE));
    assert_eq!(cdp.take_save_blocked(), None);
}

/// Minor (a): a call's deadline that cuts the answer to a paused request
/// short mid-write never leaves the request paused - the answer is written
/// again before the next frame goes out.
#[tokio::test]
async fn an_answer_a_deadline_cut_short_is_sent_again() {
    let mut cdp = guarded(&[]).await;
    cdp.transport_mut().stall_once = Some("Fetch.continueRequest");
    cdp.transport_mut().incoming.push_back(paused("r1", "GET", "https://hr.example/app.js"));
    let cut = cdp.call_within("Runtime.evaluate", json!({}), Duration::from_millis(50)).await;
    assert!(matches!(cut, Err(CdpError::Timeout { .. })), "{cut:?}");
    assert!(answer_to(&cdp, "r1").is_none(), "the stalled write finished after all");
    // The call after it writes the answer first.
    cdp.transport_mut().incoming.push_back(r#"{"id":5,"result":{}}"#.to_string());
    cdp.call("Runtime.evaluate", json!({})).await.unwrap();
    let f = sent(&cdp);
    let answer = f.iter().position(|x| x["params"]["requestId"] == "r1").expect("never answered");
    let call = f.iter().position(|x| x["id"] == 5).unwrap();
    assert!(answer < call, "the answer went after the next call: {f:?}");
    assert_eq!(f[answer]["method"], "Fetch.continueRequest");
}

#[tokio::test]
async fn an_unguarded_client_continues_a_paused_request_rather_than_leave_it() {
    let mut cdp = Cdp::over(FakeTransport::new(&[
        &paused("r1", "POST", "https://hr.example/api/Save"),
        r#"{"id":1,"result":{}}"#,
    ]));
    cdp.call("Runtime.evaluate", json!({})).await.unwrap();
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.continueRequest");
    assert_eq!(cdp.take_save_blocked(), None);
}

#[tokio::test]
async fn a_held_guard_lets_a_sign_in_save_through_and_records_nothing() {
    let mut cdp = guarded(&[]).await;
    cdp.hold_saves(true);
    cdp.transport_mut().incoming.extend([
        paused("r1", "POST", "https://hr.example/Account/SubmitLogin"),
        r#"{"id":3,"result":{}}"#.to_string(),
    ]);
    cdp.call("Runtime.evaluate", json!({})).await.unwrap();
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.continueRequest");
    assert_eq!(cdp.take_save_blocked(), None);
    cdp.hold_saves(false);
    // The continue for r1 took id 4, so this call is 5.
    cdp.transport_mut().incoming.extend([
        paused("r2", "POST", "https://hr.example/api/Save"),
        r#"{"id":5,"result":{}}"#.to_string(),
    ]);
    cdp.call("Runtime.evaluate", json!({})).await.unwrap();
    assert_eq!(answer_to(&cdp, "r2").unwrap()["method"], "Fetch.failRequest");
}

/// Between calls - a wait loop's pause - a guarded client keeps reading,
/// so a paused request is not left waiting for the next call.
#[tokio::test]
async fn a_guarded_client_answers_paused_requests_while_it_idles() {
    let mut cdp = guarded(&[]).await;
    cdp.transport_mut().incoming.push_back(paused("r1", "GET", "https://hr.example/app.js"));
    cdp.idle(Duration::from_millis(60)).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.continueRequest");
}

/// An unguarded client idles as it always did: a plain sleep that reads
/// nothing (Review Focus 5).
#[tokio::test]
async fn an_unguarded_client_idles_without_reading() {
    let mut cdp = Cdp::over(FakeTransport::new(&[]));
    cdp.transport_mut().incoming.push_back(paused("r1", "GET", "https://hr.example/app.js"));
    let began = std::time::Instant::now();
    cdp.idle(Duration::from_millis(40)).await;
    assert!(began.elapsed() >= Duration::from_millis(40));
    assert!(cdp.transport().sent.is_empty());
    assert_eq!(cdp.transport().incoming.len(), 1);
}

#[tokio::test]
async fn stopping_the_guard_switches_interception_off() {
    let mut cdp = guarded(&[]).await;
    cdp.transport_mut().incoming.push_back(r#"{"id":3,"result":{}}"#.to_string());
    cdp.stop_guarding_saves().await.unwrap();
    assert_eq!(sent(&cdp)[2]["method"], "Fetch.disable");
    assert!(!cdp.is_guarding_saves());
}

/// Minor (b): a browser that refuses to stop intercepting is still guarded
/// - its paused saves are still stopped, not continued.
#[tokio::test]
async fn a_refused_stop_keeps_the_guard_answering() {
    let mut cdp = guarded(&[]).await;
    cdp.transport_mut().incoming.push_back(r#"{"id":3,"error":{"code":-32000,"message":"busy"}}"#.to_string());
    assert!(cdp.stop_guarding_saves().await.is_err());
    assert!(cdp.is_guarding_saves());
    cdp.transport_mut().incoming.extend([
        paused("r1", "POST", "https://hr.example/api/Save"),
        r#"{"id":4,"result":{}}"#.to_string(),
    ]);
    cdp.call("Runtime.evaluate", json!({})).await.unwrap();
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.failRequest");
}

/// Minor (d): the supervised browser's between-commands reader. It answers
/// a request paused while no command runs, and ends - clearing its flag -
/// once the slot is empty.
#[tokio::test]
async fn the_between_commands_reader_answers_and_ends_with_the_browser() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let mut cdp = guarded(&[]).await;
    cdp.transport_mut().incoming.push_back(paused("r1", "GET", "https://hr.example/poll"));
    let slot = tokio::sync::Mutex::new(Some(cdp));
    let running = AtomicBool::new(true);
    let reader = v2_lib::commands::autorun::keep_answering(
        &slot,
        &running,
        |c: &mut Cdp<FakeTransport>| c,
        Duration::from_millis(10),
        Duration::from_millis(5),
    );
    let closer = async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        slot.lock().await.take().expect("the browser was taken early")
    };
    let ((), cdp) = tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(reader, closer) })
        .await
        .expect("the reader never ended");
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.continueRequest");
    assert!(!running.load(Ordering::SeqCst), "the reader ended without clearing its flag");
}

#[tokio::test]
async fn the_between_commands_reader_ends_when_the_guard_is_lifted() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let cdp = Cdp::over(FakeTransport::new(&[]));
    let slot = tokio::sync::Mutex::new(Some(cdp));
    let running = AtomicBool::new(true);
    tokio::time::timeout(
        Duration::from_secs(5),
        v2_lib::commands::autorun::keep_answering(&slot, &running, |c: &mut Cdp<FakeTransport>| c, Duration::from_millis(5), Duration::from_millis(5)),
    )
    .await
    .expect("an unguarded browser kept the reader going");
    assert!(!running.load(Ordering::SeqCst));
}

// ---------------------------------------------------------------- classes and proposals

#[test]
fn a_blocked_save_has_its_own_class() {
    let class = classify(SENTENCE, None);
    assert_eq!(class, ErrorClass::SaveBlocked);
    assert_eq!(class.key(), "no-save");
}

fn no_save_script(steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 7, "title": "t", "no_save": true, "steps": steps })).unwrap()
}

fn one_click() -> serde_json::Value {
    json!([{ "step_number": 1, "actions": [
        { "kind": "click", "selector": "#save" },
        { "kind": "check_text", "value": "Saved" }
    ] }])
}

#[test]
fn a_blocked_save_proposes_failed_and_is_never_retried() {
    let script = no_save_script(one_click());
    let steps = vec![StepRecord {
        step_number: 1,
        outcomes: vec![ActionOutcome::failed(SENTENCE), ActionOutcome::failed("not run: the page tried to save")],
        screenshot: None,
    }];
    let p = propose(&script, &steps, None, false);
    assert_eq!(p.verdict, "Failed");
    assert_eq!(p.reason, format!("step 1: {SENTENCE}"));
    let record = CaseRecord {
        case_id: 7,
        title: "t".into(),
        verdict: String::new(),
        note: String::new(),
        steps,
        proposed: "Failed".into(),
        reason: p.reason,
        duration_ms: None,
        account: None,
        retried: None,
    };
    assert_eq!(is_transient(&record, Some(&script)), None);
}

/// A save fired while the run took the case to its module is still the
/// case failing, not a run that could not start it.
#[test]
fn a_save_blocked_on_the_way_to_the_module_proposes_failed() {
    let script = no_save_script(one_click());
    let steps = vec![StepRecord { step_number: MODULE_STEP, outcomes: vec![ActionOutcome::failed(SENTENCE)], screenshot: None }];
    let p = propose(&script, &steps, None, false);
    assert_eq!(p.verdict, "Failed");
    assert!(p.reason.ends_with(SENTENCE), "{}", p.reason);
}

// ---------------------------------------------------------------- the repair gate and the editor

#[test]
fn a_repair_cannot_turn_must_not_save_off() {
    let old = no_save_script(one_click());
    let mut new = old.clone();
    new.no_save = false;
    let why = check_edits(&old, &new, None).unwrap_err();
    assert!(why.contains("Must not save"), "{why}");
}

#[test]
fn a_repair_may_turn_must_not_save_on() {
    let mut old = no_save_script(one_click());
    old.no_save = false;
    let mut new = old.clone();
    new.no_save = true;
    assert_eq!(check_edits(&old, &new, None), Ok(()));
}

#[test]
fn a_person_saving_from_the_editor_may_turn_it_off() {
    let dir = tempfile::tempdir().unwrap();
    let old = no_save_script(one_click());
    v2_lib::autorun::store::save_script(dir.path(), &old).unwrap();
    let mut new = old.clone();
    new.no_save = false;
    v2_lib::commands::autorun::save_script_from_editor(dir.path(), "acme", "PMS", new).unwrap();
    let back = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    assert!(!back.no_save);
}

#[test]
fn the_flag_is_written_only_when_it_is_on() {
    let mut s = no_save_script(one_click());
    assert!(serde_json::to_string(&s).unwrap().contains("\"no_save\":true"));
    s.no_save = false;
    assert!(!serde_json::to_string(&s).unwrap().contains("no_save"));
    let old: CaseScript = serde_json::from_value(json!({ "case_id": 1, "title": "t", "steps": [] })).unwrap();
    assert!(!old.no_save, "an old script file loads as before");
}

#[test]
fn the_guide_tells_the_assistant_about_the_flag() {
    let g = autorun_guide();
    assert!(g.contains("\"no_save\": true"), "the guide never shows the flag");
    assert!(g.contains("shared draft"), "the guide never says when to set it");
    assert!(g.contains(SENTENCE.split(" POST").next().unwrap()), "the guide never quotes the failure");
    for w in SAVE_WORDS {
        assert!(g.contains(w), "the guide never names the save word {w}");
    }
}

// ---------------------------------------------------------------- the runner

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 20, highlight_ms: 0, lease_wait_ms: 300 }
}

#[tokio::test]
async fn a_no_save_case_switches_the_guard_on_before_anything_else() {
    let dir = tempfile::tempdir().unwrap();
    set_save_words(dir.path(), "acme", "PMS", &words(&["recalc"])).unwrap();
    let mut d = common::FakePage::default().driver();
    let rec = run_case(&mut d, dir.path(), "acme", "PMS", &no_save_script(one_click()), &quick(), &AtomicBool::new(false), &mut |_| {}).await;
    assert_eq!(d.methods()[..2], ["Network.setBypassServiceWorker".to_string(), "Fetch.enable".to_string()]);
    assert_eq!(rec.proposed, "Passed", "{rec:?}");
}

#[tokio::test]
async fn a_script_without_the_flag_is_never_intercepted() {
    let dir = tempfile::tempdir().unwrap();
    let mut script = no_save_script(one_click());
    script.no_save = false;
    let mut d = common::FakePage::default().driver();
    run_case(&mut d, dir.path(), "acme", "PMS", &script, &quick(), &AtomicBool::new(false), &mut |_| {}).await;
    assert!(!d.methods().iter().any(|m| m.starts_with("Fetch.")), "{:?}", d.methods());
}

#[tokio::test]
async fn a_guard_that_cannot_start_blocks_the_case_before_step_1() {
    let dir = tempfile::tempdir().unwrap();
    let page = common::FakePage::default();
    let mut d = common::ScriptedDriver::new(move |method, params| match method {
        "Fetch.enable" => Err(CdpError::Protocol { method: "Fetch.enable".into(), message: "not allowed".into() }),
        _ => page.answer(method, params),
    });
    let rec = run_case(&mut d, dir.path(), "acme", "PMS", &no_save_script(one_click()), &quick(), &AtomicBool::new(false), &mut |_| {}).await;
    assert_eq!(rec.proposed, "Blocked");
    assert!(rec.reason.starts_with("the no-save guard could not be set up: "), "{}", rec.reason);
    assert_eq!(d.methods(), vec!["Network.setBypassServiceWorker".to_string(), "Fetch.enable".to_string()], "nothing ran after the refusal");
    assert!(rec.steps.iter().flat_map(|s| &s.outcomes).all(|o| o.detail.starts_with("not run:")));
}

#[tokio::test]
async fn a_blocked_save_fails_the_step_that_was_running_and_stops_the_case() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = common::FakePage::default().driver();
    // The block is noticed during the click's own calls.
    d.block_after = Some(("Input.dispatchMouseEvent".into(), SENTENCE.into()));
    let script = no_save_script(json!([
        { "step_number": 1, "actions": [
            { "kind": "click", "selector": "#save" },
            { "kind": "check_text", "value": "Saved" }
        ] },
        { "step_number": 2, "actions": [{ "kind": "check_text", "value": "Saved" }] }
    ]));
    let rec = run_case(&mut d, dir.path(), "acme", "PMS", &script, &quick(), &AtomicBool::new(false), &mut |_| {}).await;
    assert_eq!(rec.proposed, "Failed", "{rec:?}");
    assert_eq!(rec.reason, format!("step 1: {SENTENCE}"));
    let step1 = &rec.steps[0].outcomes;
    assert_eq!(step1[0].detail, SENTENCE);
    assert!(!step1[0].ok);
    assert!(step1[1].detail.starts_with("not run:"), "{}", step1[1].detail);
    assert!(rec.steps[1].outcomes[0].detail.starts_with("not run:"));
}

#[tokio::test]
async fn a_save_blocked_before_step_1_fails_the_case_without_running_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = common::FakePage::default().driver();
    // Noticed while the guard itself was being switched on: the page's own
    // load sent it.
    d.block_after = Some(("Fetch.enable".into(), SENTENCE.into()));
    let rec = run_case(&mut d, dir.path(), "acme", "PMS", &no_save_script(one_click()), &quick(), &AtomicBool::new(false), &mut |_| {}).await;
    assert_eq!(rec.proposed, "Failed", "{rec:?}");
    assert!(rec.reason.ends_with(SENTENCE), "{}", rec.reason);
    assert!(!d.methods().iter().any(|m| m == "Input.dispatchMouseEvent"), "the click ran: {:?}", d.methods());
}

#[tokio::test]
async fn a_browser_that_will_not_bypass_service_workers_blocks_the_case() {
    let dir = tempfile::tempdir().unwrap();
    let page = common::FakePage::default();
    let mut d = common::ScriptedDriver::new(move |method, params| match method {
        "Network.setBypassServiceWorker" => {
            Err(CdpError::Protocol { method: "Network.setBypassServiceWorker".into(), message: "not allowed".into() })
        }
        _ => page.answer(method, params),
    });
    let rec = run_case(&mut d, dir.path(), "acme", "PMS", &no_save_script(one_click()), &quick(), &AtomicBool::new(false), &mut |_| {}).await;
    assert_eq!(rec.proposed, "Blocked");
    assert!(rec.reason.starts_with("the no-save guard could not be set up: "), "{}", rec.reason);
    assert_eq!(d.methods(), vec!["Network.setBypassServiceWorker".to_string()], "anything ran after the refusal");
}

// ---------------------------------------------------------------- the supervised browser and the try

fn on_disk(dir: &std::path::Path, case_id: i32, no_save: bool) {
    let mut s = no_save_script(one_click());
    s.case_id = case_id;
    s.no_save = no_save;
    v2_lib::autorun::store::save_script(dir, &s).unwrap();
}

/// Minor (d): a step of a no-save case guards the browser; a step of a case
/// without the flag lifts the guard again.
#[tokio::test]
async fn the_guard_follows_the_case_whose_step_runs() {
    let dir = tempfile::tempdir().unwrap();
    on_disk(dir.path(), 7, true);
    on_disk(dir.path(), 8, false);
    set_save_words(dir.path(), "acme", "PMS", &words(&["recalc"])).unwrap();
    let mut d = common::FakePage::default().driver();
    let mut held = None;
    guard_for_case(&mut d, &mut held, dir.path(), "acme", "PMS", 7).await.unwrap();
    assert_eq!(held, Some(7));
    assert!(d.methods().contains(&"Fetch.enable".to_string()));
    guard_for_case(&mut d, &mut held, dir.path(), "acme", "PMS", 8).await.unwrap();
    assert_eq!(held, None);
    assert_eq!(d.methods().last().map(String::as_str), Some("Fetch.disable"));
}

#[tokio::test]
async fn a_case_with_no_script_on_this_machine_is_not_guarded() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = common::FakePage::default().driver();
    let mut held = None;
    guard_for_case(&mut d, &mut held, dir.path(), "acme", "PMS", 99).await.unwrap();
    assert!(!d.methods().iter().any(|m| m.starts_with("Fetch.")), "{:?}", d.methods());
}

#[tokio::test]
async fn a_supervised_guard_that_cannot_start_refuses_the_step() {
    let dir = tempfile::tempdir().unwrap();
    on_disk(dir.path(), 7, true);
    let page = common::FakePage::default();
    let mut d = common::ScriptedDriver::new(move |method, params| match method {
        "Fetch.enable" => Err(CdpError::Protocol { method: "Fetch.enable".into(), message: "not allowed".into() }),
        _ => page.answer(method, params),
    });
    let mut held = None;
    let why = guard_for_case(&mut d, &mut held, dir.path(), "acme", "PMS", 7).await.unwrap_err();
    assert!(why.starts_with("the no-save guard could not be set up: "), "{why}");
    assert_eq!(held, None);
}

/// Minor (c): a save stopped for one case and not yet reported is that
/// case's - written to the log under it - and never carried into the next.
#[tokio::test]
async fn a_stopped_save_is_not_carried_into_the_next_case() {
    let _log = crate::serial::log_tail();
    let dir = tempfile::tempdir().unwrap();
    on_disk(dir.path(), 7, true);
    on_disk(dir.path(), 8, true);
    let mut d = common::FakePage::default().driver();
    let mut held = None;
    guard_for_case(&mut d, &mut held, dir.path(), "acme", "PMS", 7).await.unwrap();
    d.save_blocked = Some(SENTENCE.to_string());
    guard_for_case(&mut d, &mut held, dir.path(), "acme", "PMS", 8).await.unwrap();
    assert_eq!(held, Some(8));
    assert_eq!(d.save_blocked, None, "case 7's save was carried into case 8");
    let lines: Vec<String> = v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect();
    assert!(lines.iter().any(|l| l == &format!("Auto Run, case 7: {SENTENCE}")), "{lines:?}");
    // The same case asked again keeps what it has not reported yet.
    d.save_blocked = Some(SENTENCE.to_string());
    guard_for_case(&mut d, &mut held, dir.path(), "acme", "PMS", 8).await.unwrap();
    assert_eq!(d.save_blocked.as_deref(), Some(SENTENCE));
}

/// Minor (d): the try route's guard. A try for a no-save case runs in a
/// guarded browser, and a save the page tries fails it with the run's own
/// sentence.
#[tokio::test]
async fn a_try_for_a_no_save_case_is_guarded_and_fails_on_a_save() {
    let dir = tempfile::tempdir().unwrap();
    on_disk(dir.path(), 7, true);
    let mut d = common::FakePage::default().driver();
    d.block_after = Some(("Input.dispatchMouseEvent".into(), SENTENCE.into()));
    let mut held = None;
    let mut account = None;
    guard_for_case(&mut d, &mut held, dir.path(), "acme", "PMS", 7).await.unwrap();
    let click: v2_lib::browser::actions::Action =
        serde_json::from_value(json!({ "kind": "click", "selector": "#save" })).unwrap();
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    let (status, text) = try_in(&mut d, &mut account, &mut lease, dir.path(), "acme", "PMS", &click).await;
    assert_eq!(status, 200);
    assert!(text.starts_with(&format!("failed: {SENTENCE}")), "{text}");
    let m = d.methods();
    let enabled = m.iter().position(|x| x == "Fetch.enable").expect("never guarded");
    let clicked = m.iter().position(|x| x == "Input.dispatchMouseEvent").expect("never clicked");
    assert!(enabled < clicked, "the click ran before the guard: {m:?}");
}

#[test]
fn a_try_must_name_its_case_as_a_number() {
    assert_eq!(try_case_id(r#"{ "action": {} }"#), Err((400, TRY_NEEDS_CASE.to_string())));
    assert_eq!(try_case_id(r#"{ "action": {}, "case_id": null }"#), Err((400, TRY_NEEDS_CASE.to_string())));
    assert_eq!(try_case_id(r#"{ "case_id": "7" }"#), Err((400, "case_id must be a number".to_string())));
    assert_eq!(try_case_id(r#"{ "case_id": 7.5 }"#), Err((400, "case_id must be a number".to_string())));
    assert_eq!(try_case_id(r#"{ "case_id": 7 }"#), Ok(7));
    assert_eq!(TRY_NEEDS_CASE, "name the case this try is for (case_id), so its no-save guard applies");
}

// ---------------------------------------------------------------- import

#[test]
fn an_import_keeps_must_not_save_on_a_script_that_has_it() {
    let dir = tempfile::tempdir().unwrap();
    on_disk(dir.path(), 7, true);
    let mut plain = no_save_script(one_click());
    plain.no_save = false;
    let file = dir.path().join("bundle.json");
    std::fs::write(&file, serde_json::to_string(&vec![plain]).unwrap()).unwrap();
    v2_lib::commands::autorun::import_scripts_from_path(dir.path(), "acme", "PMS", file.to_str().unwrap()).unwrap();
    assert!(v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap().no_save, "the import cleared it");
}

#[test]
fn an_import_can_turn_must_not_save_on() {
    let dir = tempfile::tempdir().unwrap();
    on_disk(dir.path(), 7, false);
    let file = dir.path().join("bundle.json");
    std::fs::write(&file, serde_json::to_string(&vec![no_save_script(one_click())]).unwrap()).unwrap();
    v2_lib::commands::autorun::import_scripts_from_path(dir.path(), "acme", "PMS", file.to_str().unwrap()).unwrap();
    assert!(v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap().no_save);
}
