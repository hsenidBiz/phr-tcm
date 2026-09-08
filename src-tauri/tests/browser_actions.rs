//! What each action becomes in the page, and how the executor treats
//! the answer. A fake evaluator stands in for the browser so every rule
//! here is pinned without starting Edge.

use v2_lib::browser::actions::{execute, highlight_js, js_for, Action, ActionOutcome, Evaluator};

/// Returns canned answers in order, and remembers every expression it
/// was asked to run.
struct FakeEval {
    answers: Vec<serde_json::Value>,
    seen: Vec<String>,
}

impl FakeEval {
    fn new(answers: Vec<serde_json::Value>) -> Self {
        FakeEval { answers, seen: vec![] }
    }
}

impl Evaluator for FakeEval {
    async fn eval(&mut self, expression: &str) -> Result<serde_json::Value, String> {
        self.seen.push(expression.to_string());
        if self.answers.is_empty() {
            return Ok(serde_json::json!({ "result": { "value": { "ok": true, "detail": "" } } }));
        }
        Ok(self.answers.remove(0))
    }
}

fn value(ok: bool, detail: &str) -> serde_json::Value {
    serde_json::json!({ "result": { "value": { "ok": ok, "detail": detail } } })
}

/// A selector with a quote in it must not be able to close the string
/// it sits in and run its own code - the scripts are authored by hand
/// and later by an assistant, so neither is trusted input.
#[test]
fn selectors_and_values_are_escaped_into_the_expression() {
    let js = js_for(&Action::Fill {
        selector: r#"input[name="x"]"#.to_string(),
        value: "he said \"hi\"\n</script>".to_string(),
    });
    assert!(js.contains(r#"\"x\""#), "selector not escaped: {js}");
    assert!(!js.contains("</script>"), "raw value leaked into the source: {js}");
}

/// React (and anything else with a controlled input) ignores a plain
/// .value assignment. The native setter plus an input event is what
/// actually moves a modern form.
#[test]
fn fill_uses_the_native_setter_so_controlled_inputs_notice() {
    let js = js_for(&Action::Fill {
        selector: "#user".to_string(),
        value: "kim".to_string(),
    });
    assert!(js.contains("getOwnPropertyDescriptor"), "{js}");
    assert!(js.contains("new Event('input'"), "{js}");
}

/// Test cases are written in prose, so the scripts derived from them
/// need to address things the way the prose does: by their words.
#[test]
fn a_text_selector_is_supported_alongside_css() {
    let js = js_for(&Action::Click { selector: "text=Sign in".to_string() });
    assert!(js.contains("text="), "{js}");
    assert!(js.contains("innerText"), "the text branch must read innerText: {js}");
}

#[test]
fn highlight_outlines_the_element_it_is_about_to_touch() {
    let js = highlight_js("#go");
    assert!(js.contains("outline"), "{js}");
    assert!(js.contains("#go"), "{js}");
}

#[tokio::test]
async fn a_successful_action_reports_what_it_did() {
    // A click highlights before it acts (see the test below), so the
    // fake needs one answer for that throwaway call plus one for the
    // click itself.
    let mut ev = FakeEval::new(vec![value(true, "highlighted"), value(true, "clicked Sign in")]);
    let out = execute(&mut ev, &Action::Click { selector: "text=Sign in".into() }).await;
    assert_eq!(out, ActionOutcome { ok: true, detail: "clicked Sign in".into() });
}

/// The human is the oracle, so the executor's job is to report
/// faithfully - a missing element is a plain false with a reason, not a
/// silent pass and not a crash.
#[tokio::test]
async fn a_missing_element_fails_with_the_reason() {
    let mut ev = FakeEval::new(vec![value(false, "not found: #nope")]);
    let out = execute(&mut ev, &Action::Click { selector: "#nope".into() }).await;
    assert!(!out.ok);
    assert!(out.detail.contains("not found"), "{}", out.detail);
}

/// A dropped socket must not read as a failed assertion about the app
/// under test - it is a failure of the harness, and it says so.
#[tokio::test]
async fn a_transport_error_is_reported_as_a_harness_problem() {
    struct Broken;
    impl Evaluator for Broken {
        async fn eval(&mut self, _e: &str) -> Result<serde_json::Value, String> {
            Err("the DevTools socket closed".to_string())
        }
    }
    let out = execute(&mut Broken, &Action::CheckText { value: "Dashboard".into() }).await;
    assert!(!out.ok);
    assert!(out.detail.contains("browser"), "{}", out.detail);
}

/// Clicking highlights first, so the watcher sees WHERE the click went.
#[tokio::test]
async fn a_click_highlights_before_it_acts() {
    let mut ev = FakeEval::new(vec![value(true, "highlighted"), value(true, "clicked")]);
    let _ = execute(&mut ev, &Action::Click { selector: "#go".into() }).await;
    assert_eq!(ev.seen.len(), 2, "expected highlight then click: {:?}", ev.seen);
    assert!(ev.seen[0].contains("outline"), "first call was not the highlight: {}", ev.seen[0]);
}

/// Waiting polls rather than sleeping a fixed guess, and gives up with
/// a verdict instead of hanging the run.
#[tokio::test]
async fn wait_for_polls_until_it_appears() {
    let mut ev = FakeEval::new(vec![
        value(false, "not found: #late"),
        value(false, "not found: #late"),
        value(true, "found: #late"),
    ]);
    let out = execute(
        &mut ev,
        &Action::WaitFor { selector: "#late".into(), timeout_ms: 2000 },
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(ev.seen.len(), 3);
}
