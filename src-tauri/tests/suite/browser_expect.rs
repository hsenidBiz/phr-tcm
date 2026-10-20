//! Expectations look again until they hold or time runs out, and a failure
//! says what was actually seen.

use crate::common;

use common::{FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use v2_lib::browser::actions::{execute_with, Action};
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::expect::{expect, Check};
use v2_lib::browser::locator::Target;
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 250, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn action(v: serde_json::Value) -> Action {
    serde_json::from_value(v).unwrap()
}

fn timeout_of(method: &str) -> CdpError {
    CdpError::Timeout { what: method.to_string(), ms: 50 }
}

#[tokio::test]
async fn visible_holds_once_the_element_shows_up() {
    let mut d = FakePage { appears_on_look: 3, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": { "css": "#toast" } })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "#toast is visible");
}

#[tokio::test]
async fn visible_fails_with_what_it_saw() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": { "css": "#toast" } })), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("250ms") && out.detail.contains("#toast") && out.detail.contains("is not on the page"), "{}", out.detail);

    // A legacy selector has no visibility filter of its own, so the
    // expectation checks: there, but not showing, is not visible.
    let mut d = FakePage { visible: false, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": "#toast" })), &quick()).await;
    assert!(!out.ok && out.detail.contains("is there but cannot be seen"), "{}", out.detail);
}

/// `expect_visible` on a locator that matches several elements must not
/// silently pick the first one - it has to say the match is ambiguous,
/// the same way the text and attribute checks already do.
#[tokio::test]
async fn visible_rejects_an_ambiguous_match() {
    let mut d = FakePage { found: 3, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": { "css": "button" } })), &quick()).await;
    assert!(!out.ok && out.detail.contains("matched 3 elements"), "{}", out.detail);
}

#[tokio::test]
async fn hidden_holds_when_nothing_can_be_seen() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_hidden", "selector": { "role": "dialog" } })), &quick()).await;
    assert!(out.ok, "{}", out.detail);

    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_hidden", "selector": { "css": "#spinner" } })), &quick()).await;
    assert!(!out.ok && out.detail.contains("is still visible"), "{}", out.detail);
}

/// The text settles after a moment, the way a saved record's name does.
#[tokio::test]
async fn text_is_compared_after_collapsing_whitespace_and_retried() {
    let mut d = FakePage { texts: vec!["Saving...", "  Custom   4-Point\n"], ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_text", "selector": { "css": "h2" }, "equals": "Custom 4-Point" })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
}

#[tokio::test]
async fn a_text_mismatch_shows_both_sides() {
    let mut d = FakePage { texts: vec!["Custom 5-Point"], ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_text", "selector": { "css": "h2" }, "equals": "Custom 4-Point" })),
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains("\"Custom 4-Point\"") && out.detail.contains("\"Custom 5-Point\""), "{}", out.detail);
}

#[tokio::test]
async fn contains_text_is_a_substring_and_case_matters() {
    let mut d = FakePage { texts: vec!["Rating method saved successfully"], ..FakePage::default() }.driver();
    let yes = action(json!({ "kind": "expect_contains_text", "selector": { "css": ".toast" }, "value": "saved successfully" }));
    assert!(execute_with(&mut d, &yes, &quick()).await.ok);
    let no = action(json!({ "kind": "expect_contains_text", "selector": { "css": ".toast" }, "value": "Saved Successfully" }));
    assert!(!execute_with(&mut d, &no, &quick()).await.ok);
}

/// Reading "the" text of three elements would be a guess.
#[tokio::test]
async fn a_text_expectation_needs_exactly_one_element() {
    let mut d = FakePage { found: 3, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_text", "selector": { "css": "li" }, "equals": "one" })),
        &quick(),
    )
    .await;
    assert!(!out.ok && out.detail.contains("matched 3 elements"), "{}", out.detail);
}

#[tokio::test]
async fn count_says_how_many_it_found() {
    let mut d = FakePage { found: 3, ..FakePage::default() }.driver();
    let three = action(json!({ "kind": "expect_count", "selector": { "css": "tbody tr" }, "equals": 3 }));
    assert!(execute_with(&mut d, &three, &quick()).await.ok);
    let two = action(json!({ "kind": "expect_count", "selector": { "css": "tbody tr" }, "equals": 2 }));
    let out = execute_with(&mut d, &two, &quick()).await;
    assert!(!out.ok && out.detail.contains("expected 2") && out.detail.contains("counted 3"), "{}", out.detail);
    // Zero is a real expectation: "the row is gone".
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let none = action(json!({ "kind": "expect_count", "selector": { "css": "tbody tr" }, "equals": 0 }));
    assert!(execute_with(&mut d, &none, &quick()).await.ok);
}

#[tokio::test]
async fn attribute_compares_and_reports_a_missing_one() {
    let check = action(json!({ "kind": "expect_attribute", "selector": { "css": "#agree" }, "name": "aria-checked", "equals": "true" }));
    let mut d = FakePage { attribute: Some("true"), ..FakePage::default() }.driver();
    assert!(execute_with(&mut d, &check, &quick()).await.ok);
    let mut d = FakePage { attribute: None, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(!out.ok && out.detail.contains("has no aria-checked"), "{}", out.detail);
}

/// `absent: true` passes when the element has no such attribute and fails
/// naming the value it has; it takes the place of `equals`, never both.
#[tokio::test]
async fn an_absent_attribute_passes_only_when_it_is_not_there() {
    let check = action(json!({ "kind": "expect_attribute", "selector": { "css": "#toggle" }, "name": "aria-pressed", "absent": true }));
    let mut d = FakePage { attribute: None, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(out.ok && out.detail == "#toggle has no aria-pressed attribute", "{}", out.detail);
    let mut d = FakePage { attribute: Some("false"), ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(!out.ok && out.detail.contains("still has aria-pressed=\"false\""), "{}", out.detail);

    let both = action(json!({ "kind": "expect_attribute", "selector": "#t", "name": "x", "equals": "1", "absent": true }));
    assert_eq!(both.validate().unwrap_err(), v2_lib::browser::actions::ATTRIBUTE_EQUALS_OR_ABSENT);
    let neither = action(json!({ "kind": "expect_attribute", "selector": "#t", "name": "x" }));
    assert_eq!(neither.validate().unwrap_err(), v2_lib::browser::actions::ATTRIBUTE_NEEDS_ONE);
    // `absent` is written only when it is set, and `equals` only when given,
    // so a script saved before either existed reads back as it was written.
    let old = json!({ "kind": "expect_attribute", "selector": "#t", "name": "x", "equals": "" });
    assert_eq!(serde_json::to_value(action(old.clone())).unwrap(), old);
    assert_eq!(serde_json::to_value(&check).unwrap()["absent"], true);
    assert!(serde_json::to_value(&check).unwrap().get("equals").is_none());
}

/// `expect_enabled` and `expect_disabled` read one element's state, say why
/// it is disabled, and need exactly one match.
#[tokio::test]
async fn enabled_and_disabled_read_one_elements_state() {
    let enabled = action(json!({ "kind": "expect_enabled", "selector": { "css": "#save" } }));
    let disabled = action(json!({ "kind": "expect_disabled", "selector": { "css": "#save" } }));
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &enabled, &quick()).await;
    assert!(out.ok && out.detail == "#save is enabled", "{}", out.detail);
    let out = execute_with(&mut d, &disabled, &quick()).await;
    assert!(!out.ok && out.detail.ends_with(" is enabled"), "{}", out.detail);

    let mut d = FakePage { disabled_why: "it has aria-disabled=\"true\"", ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &disabled, &quick()).await;
    assert!(out.ok && out.detail == "#save is disabled: it has aria-disabled=\"true\"", "{}", out.detail);
    let out = execute_with(&mut d, &enabled, &quick()).await;
    assert!(!out.ok && out.detail.contains("is disabled: it has aria-disabled"), "{}", out.detail);

    let mut d = FakePage { found: 2, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &enabled, &quick()).await;
    assert!(!out.ok && out.detail.contains("matched 2 elements"), "{}", out.detail);
    assert!(enabled.is_check() && disabled.is_check());
    assert_eq!(enabled.kind(), "expect_enabled");
    assert_eq!(disabled.kind(), "expect_disabled");
}

/// `expect_checked` reads one box's live state: ticked, clear, partly
/// ticked (never either), or no checked state at all. `checked` is true
/// when left out, and written back as given.
#[tokio::test]
async fn checked_reads_one_boxs_live_state() {
    use v2_lib::browser::expect::{IS_CHECKED, IS_MIXED, IS_NOT_CHECKED, MATCHES_A_CONTAINER, NOT_CHECKABLE};
    let ticked = action(json!({ "kind": "expect_checked", "selector": { "css": "#agree" } }));
    let clear = action(json!({ "kind": "expect_checked", "selector": { "css": "#agree" }, "checked": false }));
    assert_eq!(serde_json::to_value(&ticked).unwrap()["checked"], true);
    assert!(ticked.is_check() && clear.is_check());
    assert_eq!(ticked.kind(), "expect_checked");

    let mut d = FakePage { checked: Some("true"), ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &ticked, &quick()).await;
    assert!(out.ok && out.detail == format!("#agree {IS_CHECKED}"), "{}", out.detail);
    let out = execute_with(&mut d, &clear, &quick()).await;
    assert!(!out.ok && out.detail.contains(IS_CHECKED), "{}", out.detail);

    let mut d = FakePage { checked: Some("false"), ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &clear, &quick()).await;
    assert!(out.ok && out.detail == format!("#agree {IS_NOT_CHECKED}"), "{}", out.detail);
    let out = execute_with(&mut d, &ticked, &quick()).await;
    assert!(!out.ok && out.detail.contains(IS_NOT_CHECKED), "{}", out.detail);

    let mut d = FakePage { checked: Some("mixed"), ..FakePage::default() }.driver();
    for a in [&ticked, &clear] {
        let out = execute_with(&mut d, a, &quick()).await;
        assert!(!out.ok && out.detail.contains(IS_MIXED), "{}", out.detail);
    }
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &ticked, &quick()).await;
    assert!(!out.ok && out.detail.contains(NOT_CHECKABLE), "{}", out.detail);
    let mut d = FakePage { checked: Some("container"), ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &ticked, &quick()).await;
    assert!(!out.ok && out.detail.contains(MATCHES_A_CONTAINER), "{}", out.detail);
    let mut d = FakePage { found: 2, checked: Some("true"), ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &ticked, &quick()).await;
    assert!(!out.ok && out.detail.contains("matched 2 elements"), "{}", out.detail);
}

#[tokio::test]
async fn its_own_timeout_wins_over_the_default() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_visible", "selector": { "css": "#x" }, "timeout_ms": 40 })),
        &quick(),
    )
    .await;
    assert!(out.detail.contains("40ms"), "{}", out.detail);
}

/// A locator keeps only what a person could see, so an element that IS in
/// the page but hidden resolves to nothing - and "is not on the page" is
/// then a lie that sends someone looking for a selector bug. Asked again
/// including hidden elements, the honest answer is reachable.
#[tokio::test]
async fn a_hidden_element_is_not_reported_as_missing() {
    // Answers the visible-only look with nothing and the second look with
    // one element, the way a `display:none` element really behaves.
    let asked_for_visible = Arc::new(AtomicBool::new(true));
    let flag = asked_for_visible.clone();
    let mut d = ScriptedDriver::new(move |method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" => {
            // CSS_JS takes (selector, visibleOnly).
            flag.store(params["arguments"][1]["value"].as_bool().unwrap_or(true), Ordering::SeqCst);
            Ok(json!({ "result": { "objectId": "arr" } }))
        }
        "Runtime.getProperties" => {
            let found: Value = if flag.load(Ordering::SeqCst) {
                json!([])
            } else {
                json!([{ "name": "0", "value": { "objectId": "el-0" } }])
            };
            Ok(json!({ "result": found }))
        }
        _ => Ok(json!({})),
    });
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_visible", "selector": { "css": "#ghost" }, "timeout_ms": 40 })),
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains("is there but cannot be seen"), "{}", out.detail);
    assert!(!out.detail.contains("is not on the page"), "{}", out.detail);
}

/// Every way out of the loop hands the deadline back, or the next action
/// inherits a budget that has already run out.
#[tokio::test]
async fn an_expectation_always_hands_its_deadline_back() {
    let visible = action(json!({ "kind": "expect_visible", "selector": { "css": "#toast" } }));

    let mut d = FakePage::default().driver();
    assert!(execute_with(&mut d, &visible, &quick()).await.ok);
    assert!(d.deadlines.first().is_some_and(Option::is_some), "it never set one");
    assert!(d.deadline_was_cleared(), "after success");

    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    assert!(!execute_with(&mut d, &visible, &quick()).await.ok);
    assert!(d.deadline_was_cleared(), "after a page failure");

    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    let out = execute_with(&mut d, &visible, &quick()).await;
    assert!(out.harness, "{}", out.detail);
    assert!(d.deadline_was_cleared(), "after a harness failure");
}

/// A single call that eats the whole budget and then times out never let
/// a single look complete either - the wait ending mid-call rather than
/// between two of them does not change that. This used to be misreported
/// with page wording ("waited 250ms: ..."); it is the browser's silence,
/// not a slow page, and is now said that way.
#[tokio::test]
async fn a_call_that_burns_the_whole_budget_with_no_look_is_the_harness() {
    let mut d = ScriptedDriver::new(|method, _| {
        std::thread::sleep(Duration::from_millis(350));
        Err(CdpError::Timeout { what: method.to_string(), ms: 350 })
    });
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_visible", "selector": { "css": "#toast" } })),
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.harness, "no look ever completed: {}", out.detail);
    assert!(out.detail.contains("browser did not answer"), "{}", out.detail);
    assert!(d.deadline_was_cleared());
}

/// Not one look completed before the deadline - every call timed out - so
/// this is the browser's silence, not a missing element. Reporting it as
/// "not found" would send someone looking for a selector bug that was
/// never there.
#[tokio::test]
async fn a_browser_that_never_answers_is_not_reported_as_a_missing_element() {
    let mut d = ScriptedDriver::new(|method, _| Err(timeout_of(method)));
    let out = expect(&mut d, &Target::from("#save"), Check::Visible, 300, 50).await;
    assert!(!out.ok);
    assert!(out.harness, "no look completed, so this is the browser's failure");
    assert!(out.detail.contains("browser did not answer"), "{}", out.detail);
    assert!(!out.detail.contains("not found"), "{}", out.detail);
}

/// Once one look has completed (the browser answered, even if the
/// element was not yet there), later silence still ends the wait with
/// today's page wording, not a harness failure.
#[tokio::test]
async fn one_completed_look_keeps_the_page_wording() {
    // Not visible, so the first look holds neither element nor visibility
    // - a completed look reporting "is there but cannot be seen" - and
    // every call after it (starting with the very next `look`'s own
    // first call) goes silent. A full look here is 5 calls: the loop's
    // own `release`, then `resolve` (document, the css lookup, and
    // getProperties), then the visibility check.
    let page = FakePage { visible: false, ..FakePage::default() };
    let calls = AtomicUsize::new(0);
    let mut d = ScriptedDriver::new(move |method, params| {
        if calls.fetch_add(1, Ordering::SeqCst) < 5 {
            page.answer(method, params)
        } else {
            Err(timeout_of(method))
        }
    });
    let out = expect(&mut d, &Target::from("#never-there"), Check::Visible, 300, 50).await;
    assert!(!out.ok && !out.harness, "{}", out.detail);
    assert!(out.detail.starts_with("waited"), "{}", out.detail);
}

#[test]
fn an_expectation_is_validated_like_any_other_action() {
    assert!(action(json!({ "kind": "expect_visible", "selector": {} })).validate().is_err());
    assert!(action(json!({ "kind": "expect_attribute", "selector": "#a", "name": " ", "equals": "x" })).validate().is_err());
    // An empty expected text is legitimate: "the field is now empty".
    assert!(action(json!({ "kind": "expect_text", "selector": "#a", "equals": "" })).validate().is_ok());
    assert!(action(json!({ "kind": "expect_contains_text", "selector": "#a", "value": "" })).validate().is_err());
}

#[test]
fn a_pattern_is_refused_when_it_cannot_be_carried_or_fits_any_text() {
    let check = |p: &str| v2_lib::browser::pattern::check_pattern("expect_text", p);
    let carried = |what: &str| {
        format!("expect_text: \"matches\" uses {what}, which a Playwright export cannot carry - write the pattern without it")
    };
    assert_eq!(check("").unwrap_err(), "expect_text has an empty \"matches\" - it would match any text");
    assert_eq!(check("   ").unwrap_err(), "expect_text has an empty \"matches\" - it would match any text");
    assert_eq!(check(&"a".repeat(501)).unwrap_err(), "expect_text: \"matches\" is longer than 500 characters");
    assert!(check(&"a".repeat(500)).is_ok());
    assert_eq!(check("(?i)draft").unwrap_err(), carried("a (?...) flag or group other than (?:...)"));
    assert_eq!(check("\\Adraft").unwrap_err(), carried("\\A, \\z or \\Z"));
    assert_eq!(check("draft\\z").unwrap_err(), carried("\\A, \\z or \\Z"));
    assert_eq!(check("\\p{L}+").unwrap_err(), carried("\\p{...} or \\P{...}"));
    assert_eq!(check("[[:alpha:]]").unwrap_err(), carried("a [[:class:]]"));
    assert!(check("a{").unwrap_err().starts_with("expect_text: \"matches\" is not a valid pattern: "));
    let any = "expect_text: \"matches\" fits any text, even an empty one - write what the text must hold";
    assert_eq!(check(".*").unwrap_err(), any);
    for ok in ["^Draft \\(\\d+\\)$", "^$", "^\\(?\\d+\\)?$", "[(?]x", "(?:ab)+c", "\\\\Adraft"] {
        assert_eq!(check(ok), Ok(()), "{ok}");
    }
}

#[tokio::test]
async fn text_matches_a_pattern_on_the_collapsed_text() {
    let check = action(json!({ "kind": "expect_text", "selector": { "css": "#n" }, "matches": "^Showing 1-10 of \\d+$" }));
    let mut d = FakePage { texts: vec!["Showing  1-10 of 57"], ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "#n matches /^Showing 1-10 of \\d+$/: \"Showing 1-10 of 57\"");

    let mut d = FakePage { texts: vec!["Showing 1-10 of many"], ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.ends_with("expected text to match /^Showing 1-10 of \\d+$/ but saw \"Showing 1-10 of many\""), "{}", out.detail);
}

#[tokio::test]
async fn a_pattern_is_not_anchored_unless_written() {
    let pattern = |p: &str| action(json!({ "kind": "expect_text", "selector": { "css": "#n" }, "matches": p }));
    let mut d = FakePage { texts: vec!["Draft (3)"], ..FakePage::default() }.driver();
    assert!(execute_with(&mut d, &pattern("Draft"), &quick()).await.ok);
    assert!(!execute_with(&mut d, &pattern("^raft"), &quick()).await.ok);
}

#[tokio::test]
async fn attribute_matches_a_pattern_and_names_a_missing_attribute() {
    let check = action(json!({ "kind": "expect_attribute", "selector": { "css": "#b" }, "name": "aria-label", "matches": "^View " }));
    let mut d = FakePage { attribute: Some("View A"), ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "#b has aria-label=\"View A\", matching /^View /");
    let mut d = FakePage { attribute: None, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(!out.ok && out.detail.ends_with("has no aria-label attribute"), "{}", out.detail);
    let mut d = FakePage { attribute: Some("Edit x"), ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(!out.ok && out.detail.ends_with("expected aria-label=/^View / but saw \"Edit x\""), "{}", out.detail);
}

#[tokio::test]
async fn count_bounds_pass_and_fail_with_the_count_seen() {
    let bound = |k: &str, n: u32| action(json!({ "kind": "expect_count", "selector": { "css": "li" }, k: n }));
    let mut d = FakePage { found: 3, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &bound("at_least", 2), &quick()).await;
    assert!(out.ok && out.detail.starts_with("counted 3: "), "{}", out.detail);
    let out = execute_with(&mut d, &bound("at_least", 4), &quick()).await;
    assert!(!out.ok && out.detail.ends_with("expected at least 4, counted 3"), "{}", out.detail);
    let out = execute_with(&mut d, &bound("at_most", 3), &quick()).await;
    assert!(out.ok && out.detail.starts_with("counted 3: "), "{}", out.detail);
    let out = execute_with(&mut d, &bound("at_most", 2), &quick()).await;
    assert!(!out.ok && out.detail.ends_with("expected at most 2, counted 3"), "{}", out.detail);
}

/// `at_most` passes on too few, so the first empty looks of a list still
/// loading must not pass it: the count has to hold still first.
#[tokio::test]
async fn at_most_waits_for_the_count_to_hold_still() {
    let mut d = FakePage { found: 3, appears_on_look: 3, ..FakePage::default() }.driver();
    let check = action(json!({ "kind": "expect_count", "selector": { "css": "li" }, "at_most": 5, "timeout_ms": 3000 }));
    let began = std::time::Instant::now();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(out.ok && out.detail.starts_with("counted 3:"), "{}", out.detail);
    // The looks run at 0, 250 and 500 ms (the third sees 3 and restarts the
    // hold), so it holds from 500 ms and passes at 1250: SETTLE_MS after the
    // look that first saw 3, not after the empty ones.
    assert!(began.elapsed() >= Duration::from_millis(1250), "{:?}", began.elapsed());
}

/// The export cannot carry these forms (JavaScript reads them as something
/// else, or refuses them), so the app refuses them at save, each by name.
#[test]
fn a_pattern_in_a_form_javascript_reads_differently_is_refused() {
    let check = |p: &str| v2_lib::browser::pattern::check_pattern("expect_text", p);
    let carried = |what: &str| {
        format!("expect_text: \"matches\" uses {what}, which a Playwright export cannot carry - write the pattern without it")
    };
    for (p, what) in [
        ("\\x{41}", "\\x{...} or \\u{...}"),
        ("\\u{41}", "\\x{...} or \\u{...}"),
        ("\\U00000041", "\\U..."),
        ("a\\b{start}", "\\b{...}"),
        ("\\B{end}a", "\\b{...}"),
        ("\\<word\\>", "\\< or \\>"),
        ("word\\>", "\\< or \\>"),
        ("\\a", "\\a"),
        ("[]a]", "a class starting with ]"),
        ("[^]a]", "a class starting with ]"),
        ("[a[bc]]", "a class inside a class"),
        ("[a-z&&[^aeiou]]", "a class set operation (&&, -- or ~~)"),
        ("[a-z--b]", "a class set operation (&&, -- or ~~)"),
        ("[a~~b]", "a class set operation (&&, -- or ~~)"),
    ] {
        assert_eq!(check(p).unwrap_err(), carried(what), "{p}");
    }
    // What JavaScript reads the same stays.
    for ok in ["\\x41", "\\u0041", "[a-z-]", "[\\w-]", "a\\.b", "[a\\]b]", "\\(x\\)", "a&b", "a-b", "\\t\\n"] {
        assert_eq!(check(ok), Ok(()), "{ok}");
    }
}

/// `\w \d \s \b` are ASCII in JavaScript, so in a run too: an accented name
/// is not a word there, and the export agrees. Literal text still matches
/// itself, whatever the script.
#[tokio::test]
async fn the_perl_classes_are_ascii_as_in_javascript() {
    let run = |text: &'static str, p: &str| {
        let check = action(json!({ "kind": "expect_text", "selector": { "css": "#n" }, "matches": p }));
        async move {
            let mut d = FakePage { texts: vec![text], ..FakePage::default() }.driver();
            execute_with(&mut d, &check, &quick()).await.ok
        }
    };
    assert!(run("Jose Silva", "^\\w+ \\w+$").await);
    assert!(!run("José Silva", "^\\w+ \\w+$").await);
    assert!(run("José Silva", "^Jos\\W Silva$").await, "an accented letter is a non-word character");
    assert!(run("José Silva", "^José \\w+$").await, "literal non-ASCII text matches itself");
    assert!(!run("José Silva", "^[\\w ]+$").await);
    assert!(run("Jose Silva", "^[\\w ]+$").await);
    assert!(run("Jose Silva", "^[^\\W]+ \\S+$").await);
    assert!(run("Ref 42", "\\bRef \\d+\\b").await);
    assert!(!run("Ref ٤٢", "^Ref \\d+$").await, "an Arabic-Indic digit is no \\d in JavaScript");
    assert!(run("Ref 42 x", "^Ref\\s\\d+\\s\\w$").await);
    assert!(run("Ref 42", "^\\w+\\B\\w* \\d+$").await);
    // The refusals use the same compile, so a bad rewrite shows at save.
    assert_eq!(v2_lib::browser::pattern::check_pattern("expect_text", "^\\w+ \\d{2}$"), Ok(()));
}

#[test]
fn at_most_needs_time_to_see_the_count_hold_still() {
    use v2_lib::browser::actions::{AT_MOST_MIN_MS, COUNT_AT_MOST_TIME};
    assert_eq!(COUNT_AT_MOST_TIME, "expect_count: at_most needs at least 1500 ms to see the count hold still");
    assert_eq!(AT_MOST_MIN_MS, 1500);
    let with = |timeout: serde_json::Value, bound: &str| {
        let mut v = json!({ "kind": "expect_count", "selector": "li", bound: 2 });
        if !timeout.is_null() {
            v["timeout_ms"] = timeout;
        }
        action(v).validate()
    };
    assert_eq!(with(json!(300), "at_most"), Err(COUNT_AT_MOST_TIME.to_string()));
    assert_eq!(with(json!(1499), "at_most"), Err(COUNT_AT_MOST_TIME.to_string()));
    assert_eq!(with(json!(1500), "at_most"), Ok(()));
    assert_eq!(with(serde_json::Value::Null, "at_most"), Ok(()));
    // Only `at_most` settles, so only it is held to the time.
    assert_eq!(with(json!(300), "at_least"), Ok(()));
    assert_eq!(with(json!(300), "equals"), Ok(()));
}

// ---- expect_each: one check on every item of a list -------------------

/// A page of lists, for the checks that read every item. An element is an
/// object id; a css locator called on object `R` with selector `S` finds
/// `found[(R, S)]`, and the page's document is `doc`. Text and attributes
/// are answered by object id. An id in `detached` is out of the page until
/// that many looks (loads of the document) have been made.
#[derive(Default)]
struct ListPage {
    found: std::collections::HashMap<(String, String), Vec<String>>,
    texts: std::collections::HashMap<String, String>,
    attrs: std::collections::HashMap<String, String>,
    /// What a person calls an element (its label or name), when that is
    /// not its text: how a form control is named without its value.
    names: std::collections::HashMap<String, String>,
    detached: Vec<String>,
    detached_looks: usize,
    /// In the page but not drawn: a locator that keeps only what is seen
    /// leaves them out, and they are not visible.
    hidden: std::collections::HashSet<String>,
    /// What a frame element answers (`FRAME_JS`): "frame" or "unreachable".
    frames: std::collections::HashMap<String, String>,
    /// From this look on, `grown` replaces what `found` lists for a key:
    /// a list still filling in.
    grows_at: usize,
    grown: std::collections::HashMap<(String, String), Vec<String>>,
}

impl ListPage {
    /// `n` items `c1..cn` matching `css` in the document.
    fn list(mut self, css: &str, n: usize) -> Self {
        let ids = (1..=n).map(|i| format!("c{i}")).collect();
        self.found.insert(("doc".to_string(), css.to_string()), ids);
        self
    }
    fn text(mut self, id: &str, text: &str) -> Self {
        self.texts.insert(id.to_string(), text.to_string());
        self
    }
    fn attr(mut self, id: &str, value: &str) -> Self {
        self.attrs.insert(id.to_string(), value.to_string());
        self
    }
    /// Item `c{item}` holds `n` elements matching `css`: `c{item}-{css}-1`...
    fn inside(mut self, item: usize, css: &str, n: usize) -> Self {
        let ids = (1..=n).map(|k| format!("c{item}-{css}-{k}")).collect();
        self.found.insert((format!("c{item}"), css.to_string()), ids);
        self
    }
    /// Answers as a browser would; the counter is how many looks were made.
    fn driver(self, looks: Arc<AtomicUsize>) -> common::ScriptedDriver {
        use v2_lib::browser::each::CONNECTED_JS;
        use v2_lib::browser::expect::{READ_ATTR_JS, READ_TEXT_JS};
        use v2_lib::browser::input::SHOWN_JS;
        use v2_lib::browser::locator::{FRAME_JS, VISIBLE_JS};
        common::ScriptedDriver::new(move |method, params| {
            let f = params["functionDeclaration"].as_str().unwrap_or("");
            let id = params["objectId"].as_str().unwrap_or("").to_string();
            Ok(match method {
                "Runtime.evaluate" if params["expression"] == "document" => {
                    looks.fetch_add(1, Ordering::SeqCst);
                    json!({ "result": { "objectId": "doc" } })
                }
                "Runtime.callFunctionOn" if f == VISIBLE_JS || f == SHOWN_JS => {
                    json!({ "result": { "value": !self.hidden.contains(&id) } })
                }
                "Runtime.callFunctionOn" if f == CONNECTED_JS => {
                    let gone = self.detached.contains(&id) && looks.load(Ordering::SeqCst) <= self.detached_looks;
                    json!({ "result": { "value": !gone } })
                }
                "Runtime.callFunctionOn" if f == FRAME_JS => {
                    json!({ "result": { "value": self.frames.get(&id).map_or("element", String::as_str) } })
                }
                "Runtime.callFunctionOn" if f == READ_TEXT_JS => {
                    json!({ "result": { "value": self.texts.get(&id).cloned().unwrap_or_default() } })
                }
                "Runtime.callFunctionOn" if f == READ_ATTR_JS => {
                    json!({ "result": { "value": self.attrs.get(&id) } })
                }
                // The label a failure names an item by: read from the
                // element's labels, never its value.
                "Runtime.callFunctionOn" if f.contains("this.labels") => {
                    let said = self.names.get(&id).or_else(|| self.texts.get(&id)).cloned().unwrap_or_default();
                    json!({ "result": { "value": said } })
                }
                "Runtime.callFunctionOn" => {
                    let css = params["arguments"][0]["value"].as_str().unwrap_or("");
                    let seen_only = params["arguments"][1]["value"].as_bool().unwrap_or(true);
                    json!({ "result": { "objectId": format!("arr|{id}|{seen_only}|{css}") } })
                }
                "Runtime.getProperties" => {
                    let mut parts = id.splitn(4, '|');
                    let (_, root, seen_only, css) =
                        (parts.next(), parts.next().unwrap_or(""), parts.next() == Some("true"), parts.next().unwrap_or(""));
                    let key = (root.to_string(), css.to_string());
                    let grown = looks.load(Ordering::SeqCst) >= self.grows_at && self.grown.contains_key(&key);
                    let ids = if grown { self.grown.get(&key) } else { self.found.get(&key) }.cloned().unwrap_or_default();
                    let ids: Vec<String> = ids.into_iter().filter(|e| !(seen_only && self.hidden.contains(e))).collect();
                    json!({ "result": ids.iter().enumerate()
                        .map(|(i, e)| json!({ "name": i.to_string(), "value": { "objectId": e } }))
                        .collect::<Vec<_>>() })
                }
                _ => json!({}),
            })
        })
    }
}

fn each(v: Value) -> Action {
    let mut base = json!({ "kind": "expect_each", "selector": { "css": "div.card" } });
    for (k, val) in v.as_object().unwrap() {
        base[k] = val.clone();
    }
    action(base)
}

#[tokio::test]
async fn each_passes_when_every_item_matches_and_names_the_count() {
    let page = ListPage::default()
        .list("div.card", 3)
        .text("c1", "AUTOTEST A")
        .text("c2", "AUTOTEST B")
        .text("c3", "AUTOTEST C");
    let mut d = page.driver(Arc::default());
    let out = execute_with(&mut d, &each(json!({ "matches": "^AUTOTEST " })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "3 items of div.card each match /^AUTOTEST /");
    assert!(d.deadline_was_cleared());
}

/// The brief names a 300 ms wait; a list is only trusted once it has held
/// still, so a wait under 1500 ms is refused and this one is the default
/// (250 ms in these tests).
#[tokio::test]
async fn each_fails_naming_the_item_and_its_text() {
    let page = ListPage::default()
        .list("div.card", 3)
        .text("c1", "AUTOTEST A")
        .text("c2", "Other B")
        .text("c3", "AUTOTEST C");
    let mut d = page.driver(Arc::default());
    let out = execute_with(&mut d, &each(json!({ "matches": "^AUTOTEST " })), &quick()).await;
    assert!(!out.ok);
    assert_eq!(
        out.detail,
        "waited 250ms: div.card item 2 of 3 (\"Other B\"): expected text to match /^AUTOTEST / but saw \"Other B\""
    );
}

#[tokio::test]
async fn each_needs_min_items() {
    let mut d = ListPage::default().driver(Arc::default());
    let out = execute_with(&mut d, &each(json!({ "not_empty": true })), &quick()).await;
    assert!(!out.ok && out.detail.ends_with("expected at least 1, counted 0"), "{}", out.detail);

    let page = ListPage::default().list("div.card", 2).text("c1", "a").text("c2", "b");
    let mut d = page.driver(Arc::default());
    let out = execute_with(&mut d, &each(json!({ "not_empty": true, "min": 3 })), &quick()).await;
    assert!(!out.ok && out.detail.ends_with("expected at least 3, counted 2"), "{}", out.detail);
    // Two is enough for a min of 2, and a single item is "1 item".
    let page = ListPage::default().list("div.card", 2).text("c1", "a").text("c2", "b");
    let out = execute_with(&mut page.driver(Arc::default()), &each(json!({ "not_empty": true, "min": 2 })), &quick()).await;
    assert!(out.ok && out.detail == "2 items of div.card each have text", "{}", out.detail);
    let page = ListPage::default().list("div.card", 1).text("c1", "a");
    let out = execute_with(&mut page.driver(Arc::default()), &each(json!({ "equals": "a" })), &quick()).await;
    assert_eq!(out.detail, "1 item of div.card each read \"a\"");
}

#[tokio::test]
async fn each_reads_a_child_inside_each_item() {
    let child = json!({ "css": "h3" });
    let run = |counts: [usize; 3]| {
        let mut page = ListPage::default().list("div.card", 3);
        for (i, n) in counts.iter().enumerate() {
            page = page.inside(i + 1, "h3", *n).text(&format!("c{}-h3-1", i + 1), "Title");
        }
        let child = child.clone();
        async move { execute_with(&mut page.driver(Arc::default()), &each(json!({ "child": child, "not_empty": true })), &quick()).await }
    };
    let out = run([1, 0, 1]).await;
    assert!(!out.ok);
    assert!(out.detail.contains("item 2 of 3 (") && out.detail.ends_with("h3 is not on the page"), "{}", out.detail);
    let out = run([1, 2, 1]).await;
    assert!(!out.ok && out.detail.ends_with("h3 matched 2 elements - narrow it, or add nth"), "{}", out.detail);
    let out = run([1, 1, 1]).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "3 items of div.card, each with h3 that has text");
}

#[tokio::test]
async fn each_looks_again_when_an_item_is_detached_mid_read() {
    let build = |detached_looks: usize| {
        let mut page = ListPage::default().list("div.card", 2).text("c1", "AUTOTEST A").text("c2", "AUTOTEST B");
        page.detached = vec!["c1".to_string()];
        page.detached_looks = detached_looks;
        page
    };
    let looks = Arc::new(AtomicUsize::new(0));
    let out = execute_with(&mut build(1).driver(looks.clone()), &each(json!({ "matches": "^AUTOTEST " })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(looks.load(Ordering::SeqCst) >= 2, "a second look was made");

    let out = execute_with(&mut build(usize::MAX).driver(Arc::default()), &each(json!({ "matches": "^AUTOTEST " })), &quick()).await;
    assert!(!out.ok && out.detail.ends_with("the list changed while it was read"), "{}", out.detail);
}

/// A detached item that would also have failed is reported as the list
/// changing, never as its stale text: the probe comes before the verdict.
#[tokio::test]
async fn a_failing_item_that_is_detached_reports_the_list_changing() {
    let mut page = ListPage::default().list("div.card", 2).text("c1", "AUTOTEST A").text("c2", "Other B");
    page.detached = vec!["c2".to_string()];
    page.detached_looks = usize::MAX;
    let out = execute_with(&mut page.driver(Arc::default()), &each(json!({ "matches": "^AUTOTEST " })), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.ends_with("the list changed while it was read"), "{}", out.detail);
    assert!(!out.detail.contains("Other B"), "{}", out.detail);
}

/// The element that was read is the one probed: a child remounted inside a
/// card that stays in the page leaves a stale text behind.
#[tokio::test]
async fn each_probes_the_child_it_read_not_only_the_item() {
    let mut page = ListPage::default().list("div.card", 1).inside(1, "h3", 1).text("c1-h3-1", "AUTOTEST A");
    page.detached = vec!["c1-h3-1".to_string()];
    page.detached_looks = usize::MAX;
    let out = execute_with(
        &mut page.driver(Arc::default()),
        &each(json!({ "child": { "css": "h3" }, "matches": "^AUTOTEST " })),
        &quick(),
    )
    .await;
    assert!(!out.ok, "a child no longer in the page was read: {}", out.detail);
    assert!(out.detail.ends_with("the list changed while it was read"), "{}", out.detail);
}

#[tokio::test]
async fn each_refuses_a_list_longer_than_it_reads() {
    let page = ListPage::default().list("div.card", 201);
    let out = execute_with(&mut page.driver(Arc::default()), &each(json!({ "not_empty": true })), &quick()).await;
    assert!(
        !out.ok && out.detail.ends_with("matched 201 items, more than the 200 expect_each reads - narrow the list"),
        "{}",
        out.detail
    );
}

#[tokio::test]
async fn each_attribute_checks_read_the_attribute_not_the_text() {
    // The text would match; the attribute is what is asked.
    let page = ListPage::default()
        .list("div.card", 2)
        .text("c1", "View A")
        .text("c2", "View B")
        .attr("c1", "View A");
    let out = execute_with(
        &mut page.driver(Arc::default()),
        &each(json!({ "attribute": "aria-label", "matches": "^View " })),
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains("item 2 of 2 (") && out.detail.ends_with("has no aria-label attribute"), "{}", out.detail);

    let page = ListPage::default().list("div.card", 2).attr("c1", "View A").attr("c2", "View B");
    let out = execute_with(
        &mut page.driver(Arc::default()),
        &each(json!({ "attribute": "aria-label", "matches": "^View " })),
        &quick(),
    )
    .await;
    assert_eq!(out.detail, "2 items of div.card each have aria-label matching /^View /");
}

/// Every reason an item can fail, in the words a person reads, and the way
/// each passes.
#[tokio::test]
async fn each_says_why_in_every_form() {
    let run = |attrs: &'static [(&str, &str)], texts: &'static [(&str, &str)], v: Value| async move {
        let mut page = ListPage::default().list("div.card", 2);
        for (k, t) in texts {
            page = page.text(k, t);
        }
        for (k, a) in attrs {
            page = page.attr(k, a);
        }
        execute_with(&mut page.driver(Arc::default()), &each(v), &quick()).await.detail
    };
    let t: &[(&str, &str)] = &[("c1", "Draft one"), ("c2", "Draft  two")];
    assert!(run(&[], t, json!({ "equals": "Draft one" })).await.ends_with("item 2 of 2 (\"Draft two\"): expected text \"Draft one\" but saw \"Draft two\""));
    assert!(run(&[], t, json!({ "contains": "one" })).await.ends_with("item 2 of 2 (\"Draft two\"): expected it to contain \"one\" but saw \"Draft two\""));
    assert!(run(&[], &[("c1", "x")], json!({ "not_empty": true })).await.ends_with("item 2 of 2 (\"\"): expected text but saw \"\""));
    assert_eq!(run(&[], t, json!({ "contains": "Draft" })).await, "2 items of div.card each contain \"Draft\"");
    let a: &[(&str, &str)] = &[("c1", "/a"), ("c2", "/b")];
    assert!(run(a, &[], json!({ "attribute": "href", "equals": "/a" })).await.ends_with("item 2 of 2 (\"\"): expected href=\"/a\" but saw \"/b\""));
    assert!(run(a, &[], json!({ "attribute": "href", "contains": "a" })).await.ends_with("expected href=\"a\" as part of it but saw \"/b\""));
    assert!(run(a, &[], json!({ "attribute": "href", "matches": "a$" })).await.ends_with("expected href=/a$/ but saw \"/b\""));
    assert!(run(&[("c1", "/a"), ("c2", "")], &[], json!({ "attribute": "href", "not_empty": true })).await.ends_with("expected href=(not empty) but saw \"\""));
    assert_eq!(run(a, &[], json!({ "attribute": "href", "not_empty": true })).await, "2 items of div.card each have a href");
    assert_eq!(run(a, &[], json!({ "attribute": "href", "contains": "/" })).await, "2 items of div.card each have href containing \"/\"");
}

/// A long label is cut, and the item's own text is read when the check
/// looked at something else.
#[tokio::test]
async fn each_cuts_a_long_label_and_reads_the_item_for_a_child_failure() {
    let long = "x".repeat(80);
    let page = ListPage::default().list("div.card", 1).text("c1", &long);
    let out = execute_with(&mut page.driver(Arc::default()), &each(json!({ "equals": "short" })), &quick()).await;
    assert!(out.detail.contains(&format!("(\"{}...\")", "x".repeat(60))), "{}", out.detail);

    let page = ListPage::default().list("div.card", 1).text("c1", "Card one").inside(1, "h3", 0);
    let out = execute_with(
        &mut page.driver(Arc::default()),
        &each(json!({ "child": { "css": "h3" }, "visible": true })),
        &quick(),
    )
    .await;
    assert!(out.detail.ends_with("item 1 of 1 (\"Card one\"): h3 is not on the page"), "{}", out.detail);
}

/// The branch that tells a child that is there but not drawn from one that
/// is missing: found only when the locator may match what cannot be seen,
/// and, for a locator that already may, by its own visibility.
#[tokio::test]
async fn each_names_a_child_that_is_there_but_hidden() {
    let hidden = |locator: Value| {
        let mut page = ListPage::default().list("div.card", 1).text("c1", "Card one").inside(1, "h3", 1);
        page.hidden.insert("c1-h3-1".to_string());
        let each = each(json!({ "child": locator, "visible": true }));
        async move { execute_with(&mut page.driver(Arc::default()), &each, &quick()).await }
    };
    let out = hidden(json!({ "css": "h3" })).await;
    assert!(!out.ok && out.detail.ends_with("item 1 of 1 (\"Card one\"): h3 is there but cannot be seen"), "{}", out.detail);
    // A locator written to match what is hidden finds it, and it is not visible.
    let out = hidden(json!({ "css": "h3", "visible": false })).await;
    assert!(!out.ok && out.detail.ends_with("item 1 of 1 (\"Card one\"): h3 is there but cannot be seen"), "{}", out.detail);
}

/// A list still filling in passes nothing on the items drawn so far: the
/// third card arrives on the second look and fails.
#[tokio::test]
async fn each_does_not_pass_a_list_that_is_still_filling_in() {
    let mut page = ListPage::default()
        .list("div.card", 2)
        .text("c1", "AUTOTEST A")
        .text("c2", "AUTOTEST B")
        .text("c3", "Other C");
    page.grows_at = 2;
    page.grown.insert(("doc".to_string(), "div.card".to_string()), vec!["c1".into(), "c2".into(), "c3".into()]);
    let out = execute_with(
        &mut page.driver(Arc::default()),
        &each(json!({ "matches": "^AUTOTEST ", "timeout_ms": 2000 })),
        &quick(),
    )
    .await;
    assert!(!out.ok, "a list still filling in passed: {}", out.detail);
    assert!(out.detail.contains("item 3 of 3 (\"Other C\")"), "{}", out.detail);
}

/// A stable passing list passes once it has held still, not on the first look.
#[tokio::test]
async fn each_passes_a_stable_list_only_after_it_has_held_still() {
    let page = ListPage::default().list("div.card", 2).text("c1", "AUTOTEST A").text("c2", "AUTOTEST B");
    let looks = Arc::new(AtomicUsize::new(0));
    let started = std::time::Instant::now();
    let out = execute_with(
        &mut page.driver(looks.clone()),
        &each(json!({ "matches": "^AUTOTEST ", "timeout_ms": 2000 })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "2 items of div.card each match /^AUTOTEST /");
    assert!(looks.load(Ordering::SeqCst) >= 3, "passed on {} look(s)", looks.load(Ordering::SeqCst));
    assert!(started.elapsed() >= Duration::from_millis(700), "passed after {:?}", started.elapsed());
}

/// A form control is never named by what was typed into it: not a
/// password, not anything.
#[tokio::test]
async fn each_names_a_failing_control_by_its_label_not_its_value() {
    let mut page = ListPage::default().list("input.secret", 1).text("c1", "hunter2");
    page.names.insert("c1".to_string(), "Password".to_string());
    let out = execute_with(
        &mut page.driver(Arc::default()),
        &action(json!({ "kind": "expect_each", "selector": { "css": "input.secret" }, "attribute": "data-x", "not_empty": true })),
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(!out.detail.contains("hunter2"), "the typed value reached the failure: {}", out.detail);
    assert!(out.detail.contains("item 1 of 1 (\"Password\"): has no data-x attribute"), "{}", out.detail);
}

/// A child behind a frame this page cannot enter is not "not on the page".
#[tokio::test]
async fn each_child_behind_an_unreachable_frame_says_so() {
    let mut page = ListPage::default().list("div.card", 1).text("c1", "Card one").inside(1, "iframe", 1);
    page.frames.insert("c1-iframe-1".to_string(), "unreachable".to_string());
    let child = json!([{ "css": "iframe" }, { "css": "h3" }]);
    let out = execute_with(&mut page.driver(Arc::default()), &each(json!({ "child": child, "not_empty": true })), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("holds a page from another site (or has not loaded), which Auto Run cannot reach"), "{}", out.detail);
    assert!(!out.detail.ends_with("is not on the page"), "{}", out.detail);
    assert_eq!(v2_lib::autorun::patterns::classify(&out.detail, Some("div.card")), v2_lib::autorun::patterns::ErrorClass::FrameUnreachable);
}

/// A first look that runs out of time on a long list is the list check's
/// own failure, not a browser that stopped answering.
#[tokio::test]
async fn each_overrunning_look_is_the_lists_own_failure() {
    use v2_lib::browser::expect::READ_TEXT_JS;
    let page = ListPage::default().list("div.card", 3).text("c1", "a").text("c2", "b").text("c3", "c");
    let mut d = page.driver(Arc::default()).answering_first(|method, params| {
        (method == "Runtime.callFunctionOn" && params["functionDeclaration"] == READ_TEXT_JS)
            .then(|| Err(CdpError::Timeout { what: "Runtime.callFunctionOn".to_string(), ms: 50 }))
    });
    let out = execute_with(&mut d, &each(json!({ "not_empty": true })), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "waited 250ms: div.card could not read every item in time - narrow the list, or allow more time");
    assert_eq!(
        v2_lib::autorun::patterns::classify(&out.detail, Some("div.card")),
        v2_lib::autorun::patterns::ErrorClass::TimedOut
    );
}
