//! Browser dialogs: `expect_dialog` checks and answers one, a dialog nobody
//! expected is accepted and said (or fails the step, when the script asks),
//! and the page is never left waiting on one. The browser is a fake that
//! opens a dialog when a given call is made and answers it through the
//! run's own dialog book, as `Cdp` does; `browser_tabs` covers the real
//! client's routing, and `browser_live` a real browser.

use crate::common;

use common::{FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::patterns::{classify, ErrorClass};
use v2_lib::autorun::replay::{run_selection, Browsers};
use v2_lib::autorun::report::action_words;
use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
use v2_lib::autorun::{store, CaseScript, LocalRun, StepDialog, StepScript};
use v2_lib::browser::actions::{execute_with, Action, ActionOutcome, WITHIN_MS_ZERO};
use v2_lib::browser::dialogs::{PROMPT_NEEDS_ACCEPT, TEXT_OR_CONTAINS};
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn action(v: Value) -> Action {
    serde_json::from_value(v).expect("an action")
}

fn step(n: i32, actions: Value) -> StepScript {
    serde_json::from_value(json!({ "step_number": n, "actions": actions })).expect("a step")
}

const CLICK: &str = "Input.dispatchMouseEvent";

/// A page whose click opens a dialog of `kind` saying `message`.
fn opens(kind: &str, message: &str) -> ScriptedDriver {
    let mut d = FakePage::default().driver();
    d.dialogs_on_call.push((CLICK.to_string(), kind.to_string(), message.to_string()));
    d
}

/// One step, run as a run runs it. The step's outcomes and what it learned.
async fn run(d: &mut ScriptedDriver, s: &StepScript, fail_on_unexpected: bool) -> (Vec<ActionOutcome>, Option<StepDialog>) {
    let dir = tempfile::tempdir().unwrap();
    let mut account = None;
    let mut held = Held::supervised();
    let mut r = InRun { fail_on_unexpected_dialog: fail_on_unexpected, ..Default::default() };
    let out = run_step_in_run(
        d,
        dir.path(),
        "Acme",
        "Web",
        s,
        &quick(),
        &mut account,
        &mut held,
        None,
        AreaRoute::Unknown(NEEDS_SCRIPT_AREA),
        &mut r,
    )
    .await
    .unwrap();
    (out, r.dialog)
}

/// Every answer sent, in order.
fn answers(d: &ScriptedDriver) -> Vec<Value> {
    d.calls_to("Page.handleJavaScriptDialog")
}

fn met(kind: &str, message: &str) -> Option<StepDialog> {
    Some(StepDialog { kind: kind.to_string(), message: message.to_string() })
}

// ---- What a script may say ----------------------------------------------------

#[test]
fn each_refusal_is_said_where_the_script_is_written() {
    let refused = |v: Value| action(v).validate().unwrap_err();
    assert_eq!(
        refused(json!({ "kind": "expect_dialog", "text": "a", "contains": "b", "answer": "accept" })),
        TEXT_OR_CONTAINS
    );
    assert_eq!(TEXT_OR_CONTAINS, "expect_dialog takes text or contains, not both");
    assert_eq!(
        refused(json!({ "kind": "expect_dialog", "answer": "dismiss", "prompt_text": "Kim" })),
        PROMPT_NEEDS_ACCEPT
    );
    assert_eq!(PROMPT_NEEDS_ACCEPT, "prompt_text needs answer accept");
    assert_eq!(refused(json!({ "kind": "expect_dialog", "answer": "accept", "within_ms": 0 })), WITHIN_MS_ZERO);
    assert_eq!(
        refused(json!({ "kind": "expect_dialog", "answer": "accept", "within_ms": 60001 })),
        "expect_dialog waits at most 60000 ms, not 60001"
    );
    assert!(refused(json!({ "kind": "expect_dialog", "answer": "accept", "contains": " " })).contains("empty contains"));
    // An answer is required, and is one of the two.
    assert!(serde_json::from_value::<Action>(json!({ "kind": "expect_dialog", "text": "a" })).is_err());
    assert!(serde_json::from_value::<Action>(json!({ "kind": "expect_dialog", "answer": "maybe" })).is_err());
    for ok in [
        json!({ "kind": "expect_dialog", "answer": "accept" }),
        json!({ "kind": "expect_dialog", "text": "", "answer": "dismiss" }),
        json!({ "kind": "expect_dialog", "contains": "x", "answer": "accept", "prompt_text": "Kim", "within_ms": 60000 }),
    ] {
        assert!(action(ok.clone()).validate().is_ok(), "{ok}");
    }
    let a = action(json!({ "kind": "expect_dialog", "answer": "accept" }));
    assert!(a.is_check());
    // A guarded step checks nothing.
    let guarded = action(json!({ "kind": "when_visible", "selector": "#x", "then": [{ "kind": "expect_dialog", "answer": "accept" }] }));
    assert!(guarded.validate().is_err());
}

#[test]
fn new_fields_are_written_only_when_set() {
    for text in [
        r#"{"kind":"expect_dialog","answer":"accept"}"#,
        r#"{"kind":"expect_dialog","text":"Delete?","answer":"dismiss","within_ms":5000}"#,
        r#"{"kind":"expect_dialog","contains":"name","answer":"accept","prompt_text":"Kim"}"#,
    ] {
        let read: Action = serde_json::from_str(text).unwrap();
        assert_eq!(serde_json::to_string(&read).unwrap(), text);
    }
    let old = r#"{"case_id":1,"title":"t","steps":[]}"#;
    let script: CaseScript = serde_json::from_str(old).unwrap();
    assert!(!script.fail_on_unexpected_dialog);
    assert_eq!(serde_json::to_string(&script).unwrap(), old);
    let on: CaseScript = serde_json::from_str(r#"{"case_id":1,"title":"t","steps":[],"fail_on_unexpected_dialog":true}"#).unwrap();
    assert!(on.fail_on_unexpected_dialog);
    assert!(serde_json::to_string(&on).unwrap().contains(r#""fail_on_unexpected_dialog":true"#));
}

// ---- Checking and answering one ---------------------------------------------------

#[tokio::test]
async fn each_kind_is_claimed_answered_and_recorded() {
    for kind in ["alert", "confirm", "prompt", "beforeunload"] {
        let mut d = opens(kind, "Are you sure?");
        let s = step(1, json!([
            { "kind": "click", "selector": "#go" },
            { "kind": "expect_dialog", "text": "Are you sure?", "answer": "accept" }
        ]));
        let (out, dialog) = run(&mut d, &s, false).await;
        assert!(out.iter().all(|o| o.ok), "{kind}: {out:?}");
        assert_eq!(out[1].detail, format!("a {kind} dialog said \"Are you sure?\"; pressed OK"));
        // A claimed dialog is not said on the click as one nobody expected.
        assert_eq!(out[0].detail, "clicked #go", "{kind}");
        assert_eq!(answers(&d), [json!({ "accept": true })]);
        assert_eq!(dialog, met(kind, "Are you sure?"));
    }
}

#[tokio::test]
async fn dismiss_presses_cancel_and_prompt_text_is_typed_before_ok() {
    let mut d = opens("confirm", "Delete this cycle?");
    let s = step(1, json!([
        { "kind": "click", "selector": "#go" },
        { "kind": "expect_dialog", "contains": "DELETE", "answer": "dismiss" }
    ]));
    let (out, _) = run(&mut d, &s, false).await;
    assert!(out[1].ok, "{out:?}");
    assert_eq!(out[1].detail, "a confirm dialog said \"Delete this cycle?\"; pressed Cancel");
    assert_eq!(answers(&d), [json!({ "accept": false })]);

    let mut d = opens("prompt", "Your name?");
    let s = step(1, json!([
        { "kind": "click", "selector": "#go" },
        { "kind": "expect_dialog", "text": "  Your name?  ", "answer": "accept", "prompt_text": "Kim" }
    ]));
    let (out, _) = run(&mut d, &s, false).await;
    assert!(out[1].ok, "{out:?}");
    assert_eq!(out[1].detail, "a prompt dialog said \"Your name?\"; typed \"Kim\" and pressed OK");
    assert_eq!(answers(&d), [json!({ "accept": true, "promptText": "Kim" })]);
}

/// Every `expect_dialog` is armed when its step starts: written before the
/// click that opens its dialog, it still claims it.
#[tokio::test]
async fn an_expectation_written_before_the_click_still_claims_its_dialog() {
    let mut d = opens("confirm", "Leave?");
    let s = step(1, json!([
        { "kind": "expect_dialog", "answer": "dismiss" },
        { "kind": "click", "selector": "#go" }
    ]));
    let (out, dialog) = run(&mut d, &s, false).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(answers(&d), [json!({ "accept": false })]);
    assert_eq!(dialog, met("confirm", "Leave?"));
}

/// Two in one step claim the two dialogs in turn, first armed first.
#[tokio::test]
async fn two_expectations_claim_two_dialogs_in_order() {
    let mut d = FakePage::default().driver();
    d.dialogs_on_call.push((CLICK.to_string(), "confirm".to_string(), "First?".to_string()));
    // The click opens one, and the page a second straight after it.
    d.dialogs_on_call.push((CLICK.to_string(), "alert".to_string(), "Second".to_string()));
    let s = step(1, json!([
        { "kind": "expect_dialog", "text": "First?", "answer": "dismiss" },
        { "kind": "expect_dialog", "text": "Second", "answer": "accept" },
        { "kind": "click", "selector": "#go" },
        { "kind": "check_text", "value": "x" }
    ]));
    let (out, _) = run(&mut d, &s, false).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(answers(&d), [json!({ "accept": false }), json!({ "accept": true })]);
}

/// The run's expectations are the run's, not a tab's: a dialog in another
/// tab is claimed and answered there.
#[tokio::test]
async fn a_dialog_in_a_second_tab_is_claimed() {
    let mut d = opens("confirm", "Close the report?");
    d.tabs.open.push("report".to_string());
    d.tabs.current = "report".to_string();
    let s = step(1, json!([
        { "kind": "click", "selector": "#go" },
        { "kind": "expect_dialog", "contains": "report", "answer": "accept" }
    ]));
    let (out, dialog) = run(&mut d, &s, false).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert!(d.tabs.called_in("report").contains(&"Page.handleJavaScriptDialog".to_string()));
    assert_eq!(dialog, met("confirm", "Close the report?"));
}

// ---- Failures ----------------------------------------------------------------------

#[tokio::test]
async fn no_dialog_says_how_long_it_waited() {
    let mut d = FakePage::default().driver();
    let s = step(1, json!([
        { "kind": "click", "selector": "#go" },
        { "kind": "expect_dialog", "answer": "accept", "within_ms": 100 }
    ]));
    let (out, dialog) = run(&mut d, &s, false).await;
    assert!(out[0].ok);
    assert_eq!(out[1].detail, "no dialog appeared within 0.1 seconds");
    assert!(!out[1].ok);
    assert_eq!(classify(&out[1].detail, None), ErrorClass::NotFound);
    assert_eq!(dialog, None);
}

/// The wrong words: the dialog was still answered as the step asked, so
/// the page is not left stuck, and the step fails.
#[tokio::test]
async fn the_wrong_words_still_answer_as_asked_then_fail() {
    let mut d = opens("confirm", "Saved");
    let s = step(1, json!([
        { "kind": "click", "selector": "#go" },
        { "kind": "expect_dialog", "text": "Deleted", "answer": "dismiss" }
    ]));
    let (out, dialog) = run(&mut d, &s, false).await;
    assert_eq!(out[1].detail, "the dialog said \"Saved\", not \"Deleted\"");
    assert!(!out[1].ok);
    assert_eq!(answers(&d), [json!({ "accept": false })]);
    assert_eq!(dialog, met("confirm", "Saved"));
    assert_eq!(classify(&out[1].detail, None), ErrorClass::TextMismatch);

    let mut d = opens("alert", "Saved");
    let s = step(1, json!([
        { "kind": "click", "selector": "#go" },
        { "kind": "expect_dialog", "contains": "deleted", "answer": "accept" }
    ]));
    let (out, _) = run(&mut d, &s, false).await;
    assert_eq!(out[1].detail, "the dialog said \"Saved\", which does not contain \"deleted\"");
    assert_eq!(answers(&d), [json!({ "accept": true })]);
}

/// A message is page text, kept and said to 200 characters.
#[tokio::test]
async fn a_long_message_is_cut_to_200_characters() {
    let long = "x".repeat(450);
    let mut d = opens("alert", &long);
    let s = step(1, json!([
        { "kind": "click", "selector": "#go" },
        { "kind": "expect_dialog", "text": "y", "answer": "accept" }
    ]));
    let (out, dialog) = run(&mut d, &s, false).await;
    assert_eq!(dialog.unwrap().message.chars().count(), 200);
    assert_eq!(out[1].detail, format!("the dialog said \"{}\", not \"y\"", "x".repeat(200)));
}

// ---- Dialogs nobody expected ----------------------------------------------------

#[tokio::test]
async fn an_unexpected_dialog_is_accepted_and_said_on_the_step() {
    let mut d = opens("alert", "Hi there");
    let s = step(1, json!([{ "kind": "click", "selector": "#go" }, { "kind": "check_text", "value": "x" }]));
    let (out, dialog) = run(&mut d, &s, false).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[0].detail, "clicked #go (an alert dialog was accepted: \"Hi there\")");
    assert_eq!(answers(&d), [json!({ "accept": true })]);
    assert_eq!(dialog, met("alert", "Hi there"));
    // The note never decides what a failure is.
    assert_eq!(
        classify("waited 5ms: #go not found (a confirm dialog was accepted: \"is disabled\")", Some("#go")),
        ErrorClass::NotFound
    );
}

#[tokio::test]
async fn with_the_option_an_unexpected_dialog_fails_the_step_it_appeared_in() {
    let mut d = opens("confirm", "Discard changes?");
    let s = step(1, json!([{ "kind": "click", "selector": "#go" }, { "kind": "check_text", "value": "x" }]));
    let (out, _) = run(&mut d, &s, true).await;
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, "an unexpected confirm dialog appeared: \"Discard changes?\"");
    assert!(out[1].ok, "the rest of the step still ran: {out:?}");
    // Still accepted, so the page can go on.
    assert_eq!(answers(&d), [json!({ "accept": true })]);
    // A claimed one is not unexpected, option or not.
    let mut d = opens("confirm", "Discard changes?");
    let s = step(1, json!([{ "kind": "click", "selector": "#go" }, { "kind": "expect_dialog", "answer": "accept" }]));
    let (out, _) = run(&mut d, &s, true).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
}

/// An expectation no dialog claimed goes no further than its step: the
/// next step's dialog is nobody's.
#[tokio::test]
async fn a_leftover_expectation_is_disarmed_at_the_end_of_its_step() {
    let mut d = FakePage::default().driver();
    let first = step(1, json!([{ "kind": "check_text", "value": "x" }, { "kind": "expect_dialog", "answer": "dismiss", "within_ms": 50 }]));
    let (out, _) = run(&mut d, &first, false).await;
    assert!(!out[1].ok, "{out:?}");
    assert!(!d.book.is_armed(), "the expectation outlived its step");

    d.dialogs_on_call.push((CLICK.to_string(), "confirm".to_string(), "Sure?".to_string()));
    let second = step(2, json!([{ "kind": "click", "selector": "#go" }]));
    let (out, dialog) = run(&mut d, &second, false).await;
    // Accepted as nobody's, not dismissed as the first step asked.
    assert_eq!(answers(&d), [json!({ "accept": true })]);
    assert!(out[0].detail.ends_with("(a confirm dialog was accepted: \"Sure?\")"), "{}", out[0].detail);
    assert_eq!(dialog, met("confirm", "Sure?"));
}

// ---- Review Focus 1: the page never waits ---------------------------------------

/// A dialog that opens while a step waits for something else is answered
/// at once - before the wait's next look - and the wait carries on to its
/// end.
#[tokio::test]
async fn a_dialog_during_a_wait_is_answered_at_once_and_the_wait_carries_on() {
    for armed in [false, true] {
        let mut d = FakePage { appears_on_look: 4, ..FakePage::default() }.driver();
        d.dialogs_on_call.push(("Runtime.getProperties".to_string(), "alert".to_string(), "Working...".to_string()));
        let mut actions = vec![json!({ "kind": "wait_for", "selector": "#late", "timeout_ms": 2000 })];
        if armed {
            actions.push(json!({ "kind": "expect_dialog", "text": "Working...", "answer": "accept" }));
        }
        let (out, dialog) = run(&mut d, &step(1, json!(actions)), false).await;
        assert!(out.iter().all(|o| o.ok), "armed {armed}: {out:?}");
        let methods = d.methods();
        let answered = methods.iter().position(|m| m == "Page.handleJavaScriptDialog").expect("never answered");
        let first_look = methods.iter().position(|m| m == "Runtime.getProperties").unwrap();
        let looks_after = methods[answered..].iter().filter(|m| *m == "Runtime.getProperties").count();
        assert_eq!(answered, first_look + 1, "answered right after the call it opened during");
        assert_eq!(looks_after, 3, "the wait went on looking after the dialog");
        assert_eq!(dialog, met("alert", "Working..."));
    }
}

/// Run on its own (a try), an `expect_dialog` arms itself, so only a
/// dialog from then on is its own - one an earlier action met is not - and
/// it lets go of its expectation when it is done.
#[tokio::test]
async fn on_its_own_it_arms_itself_and_lets_go() {
    let mut d = opens("alert", "Hi");
    assert!(execute_with(&mut d, &action(json!({ "kind": "click", "selector": "#go" })), &quick()).await.ok);
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_dialog", "answer": "accept", "within_ms": 100 })), &quick()).await;
    assert_eq!(out.detail, "no dialog appeared within 0.1 seconds");
    assert!(!d.book.is_armed());
}

// ---- Words for reports ------------------------------------------------------------

#[test]
fn reports_have_words_for_it() {
    let say = |v: Value| action_words(&action(v));
    assert_eq!(say(json!({ "kind": "expect_dialog", "text": "Delete?", "answer": "dismiss" })), "expect a dialog saying \"Delete?\" and press Cancel");
    assert_eq!(say(json!({ "kind": "expect_dialog", "contains": "name", "answer": "accept", "prompt_text": "Kim" })), "expect a dialog containing \"name\", type \"Kim\" and press OK");
    assert_eq!(say(json!({ "kind": "expect_dialog", "answer": "accept" })), "expect a dialog and press OK");
    assert_eq!(
        v2_lib::autorun::patterns::action_target(&action(json!({ "kind": "expect_dialog", "answer": "accept" }))).as_deref(),
        Some("a dialog")
    );
    assert_eq!(
        v2_lib::ai_bridge::describe_try(&action(json!({ "kind": "expect_dialog", "text": "Delete?", "answer": "dismiss" })), false),
        "AI tried expect_dialog Delete? in the supervised browser: failed"
    );
}

// ---- A whole run ----------------------------------------------------------------------

struct One(Option<ScriptedDriver>, Vec<ScriptedDriver>);

impl Browsers for One {
    type D = ScriptedDriver;
    async fn open(&mut self) -> Result<Self::D, String> {
        self.0.take().ok_or_else(|| "no browser".to_string())
    }
    async fn close(&mut self, d: Self::D) {
        self.1.push(d);
    }
}

async fn run_case(root: &Path, script: &CaseScript, d: ScriptedDriver) -> LocalRun {
    store::save_script(root, script).unwrap();
    let mut browsers = One(Some(d), vec![]);
    let mut run = LocalRun {
        id: "run-d".into(),
        pbi_id: 42,
        started_at: "1700000000000".into(),
        cases: vec![],
        mode: "unattended".into(),
        published: None,
        environment: None,
        resets: vec![],
    };
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(script.case_id, script.title.clone())], &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    run
}

#[tokio::test]
async fn a_run_keeps_the_dialog_on_the_step_and_fails_on_an_unexpected_one_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let steps = json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#go" }, { "kind": "expect_dialog", "text": "Sure?", "answer": "dismiss" }] }
    ]);
    let script: CaseScript = serde_json::from_value(json!({ "case_id": 1, "title": "c", "steps": steps })).unwrap();
    let run = run_case(dir.path(), &script, opens("confirm", "Sure?")).await;
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
    let s1 = rec.steps.iter().find(|s| s.step_number == 1).unwrap();
    assert_eq!(s1.dialog, met("confirm", "Sure?"));
    let saved = serde_json::to_value(s1).unwrap();
    assert_eq!(saved["dialog"], json!({ "kind": "confirm", "message": "Sure?" }));

    let dir = tempfile::tempdir().unwrap();
    let steps = json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": "#go" }, { "kind": "check_text", "value": "x" }] }]);
    let script: CaseScript =
        serde_json::from_value(json!({ "case_id": 2, "title": "c", "steps": steps, "fail_on_unexpected_dialog": true })).unwrap();
    let run = run_case(dir.path(), &script, opens("alert", "Oops")).await;
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Failed");
    assert_eq!(rec.reason, "step 1: an unexpected alert dialog appeared: \"Oops\"");
    // A step that met no dialog writes nothing about one.
    let dir = tempfile::tempdir().unwrap();
    let steps = json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "x" }] }]);
    let script: CaseScript = serde_json::from_value(json!({ "case_id": 3, "title": "c", "steps": steps })).unwrap();
    let run = run_case(dir.path(), &script, FakePage::default().driver()).await;
    let saved = serde_json::to_value(&run.cases[0].steps[0]).unwrap();
    assert!(saved.get("dialog").is_none(), "{saved}");
}

/// A non-breaking space, a figure space, a narrow non-breaking space and a
/// tab: what a page or a pasted name may hold where a space is meant.
const ODD_SPACES: [&str; 4] = ["\u{a0}", "\u{2007}", "\u{202f}", "\t"];

/// A dialog's `contains` treats any Unicode space as a space, in the
/// message and in the script.
#[tokio::test]
async fn a_dialog_contains_treats_a_unicode_space_as_a_space() {
    for sp in ODD_SPACES {
        let mut d = opens("confirm", &format!("Delete{sp}this{sp}{sp}cycle?"));
        let s = step(1, json!([
            { "kind": "click", "selector": "#go" },
            { "kind": "expect_dialog", "contains": "delete this cycle", "answer": "dismiss" }
        ]));
        let (out, _) = run(&mut d, &s, false).await;
        assert!(out[1].ok, "{sp:?}: {out:?}");

        let mut d = opens("confirm", "Delete this cycle?");
        let s = step(1, json!([
            { "kind": "click", "selector": "#go" },
            { "kind": "expect_dialog", "contains": format!("this{sp}cycle"), "answer": "dismiss" }
        ]));
        let (out, _) = run(&mut d, &s, false).await;
        assert!(out[1].ok, "{sp:?}: {out:?}");
    }
}
