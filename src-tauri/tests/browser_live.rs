//! The same rules as the unit tests, against a REAL browser. Ignored by
//! default: they start (headless) Edge. Run them on purpose:
//!
//!   cargo test --test browser_live -- --ignored --test-threads=1
//!
//! One at a time, because each starts its own browser.
//!
//! Every assertion reads something the PAGE wrote - a mirror updated by an
//! `oninput` handler, a paragraph written by an `onclick` - never something
//! Rust believes it sent. That is the whole point of this file: the unit
//! tests answer from a fake that was written from the same beliefs as the
//! code, so only a real browser can say whether those beliefs were right.

use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use v2_lib::autorun::accounts::{save_accounts, Account};
use v2_lib::autorun::recipe::{save_recipe, SignInRecipe};
use v2_lib::autorun::replay::{run_selection, Browsers, SIGN_IN_STEP};
use v2_lib::autorun::signin::sign_in;
use v2_lib::autorun::{sessions, store, CaseScript, LocalRun};
use v2_lib::browser::actions::{execute_in, execute_with, Action, ActionOutcome, HIGHLIGHT_JS, Policy};
use v2_lib::browser::cdp::Cdp;
use v2_lib::browser::launch::{background_args, launch_with, Browser, LaunchedBrowser};
use v2_lib::browser::locator::{resolve, Target};
use v2_lib::browser::page;
use v2_lib::browser::timing::Timing;
use v2_lib::events::ReplayProgress;

struct Live {
    browser: LaunchedBrowser,
    cdp: Cdp,
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.browser.child.kill();
        let _ = self.browser.child.wait();
        // Windows holds the profile's files open for a moment after the
        // browser goes. A few retries is the difference between a clean
        // temp folder and one left behind per test.
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.browser.profile_dir).is_ok()
                || !self.browser.profile_dir.exists()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn fixture_url() -> String {
    let path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/autorun-live.html").replace('\\', "/");
    // The repo path has spaces in it on this machine.
    format!("file:///{}", path.trim_start_matches('/').replace(' ', "%20"))
}

/// Short enough that a failing test fails fast, long enough for the
/// fixture's 700 ms of being disabled and covered.
fn timing() -> Timing {
    Timing { action_ms: 4000, expect_ms: 3000, nav_ms: 15000, poll_ms: 100, highlight_ms: 0 }
}

fn action_of(value: serde_json::Value) -> Action {
    serde_json::from_value(value).expect("the test wrote an invalid action")
}

async fn run_with(live: &mut Live, action: serde_json::Value, timing: &Timing) -> ActionOutcome {
    execute_with(&mut live.cdp, &action_of(action), timing).await
}

async fn run(live: &mut Live, action: serde_json::Value) -> ActionOutcome {
    run_with(live, action, &timing()).await
}

/// The browser needs a moment before its debugging port answers, and how
/// long varies with what else the machine is doing. Retrying beats a fixed
/// sleep that is either slow or flaky.
async fn open() -> Live {
    let mut browser = launch_with(Browser::Edge, &["--headless=new"]).expect("Edge did not start");
    let mut last = String::new();
    let mut connected = None;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        match Cdp::connect(browser.port).await {
            Ok(c) => {
                connected = Some(c);
                break;
            }
            Err(e) => last = e,
        }
    }
    let Some(cdp) = connected else {
        // There is no `Live` yet, so nothing would tidy up after this.
        let port = browser.port;
        let _ = browser.child.kill();
        let _ = browser.child.wait();
        let _ = std::fs::remove_dir_all(&browser.profile_dir);
        panic!("could not reach Edge on port {port}: {last}");
    };
    let mut live = Live { browser, cdp };
    let out = run(&mut live, json!({ "kind": "navigate", "url": fixture_url() })).await;
    assert!(out.ok, "the fixture did not load: {}", out.detail);
    live
}

fn must(out: ActionOutcome) {
    assert!(out.ok, "{}", out.detail);
}

fn refused(out: ActionOutcome, contains: &str) {
    assert!(!out.ok, "expected a failure, got: {}", out.detail);
    assert!(out.detail.contains(contains), "expected {contains:?} in: {}", out.detail);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_role_locator_ignores_the_hidden_twin_and_really_clicks() {
    let mut live = open().await;
    // Two buttons are called "Save changes"; one is display:none. If the
    // hidden one counted, this would fail with "matched 2 elements".
    must(run(&mut live, json!({ "kind": "click", "selector": { "role": "button", "name": "save changes" } })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 1" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_click_waits_out_disabled_and_covered() {
    let mut live = open().await;
    // #late is disabled and under a full-page overlay for 700 ms.
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#late" } })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 1" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn typing_reaches_the_page_as_real_input_and_can_be_cleared() {
    let mut live = open().await;
    let field = json!({ "role": "textbox", "name": "Method Name" });
    must(run(&mut live, json!({ "kind": "fill", "selector": field, "value": "Custom 4-Point" })).await);
    // #echo is written by the field's own oninput handler.
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#echo", "equals": "Custom 4-Point" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": field, "value": "Second" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#echo", "equals": "Second" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": field, "value": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#echo", "equals": "" })).await);
}

/// The kinds of field where `select()` is a no-op, where typing does not
/// work at all, and where there is no `value` property. Each one is read
/// back through the mirror its own `oninput` writes, so "it replaced what
/// was there" is what the PAGE saw and not what Rust meant.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn every_kind_of_field_is_filled_and_cleared_through_what_the_page_sees() {
    let mut live = open().await;
    // A number field already holding 7. Its #num-log records every input
    // event it raises, in order: filling it must produce exactly ONE,
    // carrying the new value. A clear-then-type would read "[][42]", and
    // that empty event is something a page's own validator reacts to.
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#num" }, "value": "42" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#num-echo", "equals": "42" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#num-log", "equals": "[42]" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#num" }, "value": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#num-echo", "equals": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#num-log", "equals": "[42][]" })).await);

    // Email is the other type whose selection cannot be read back, and it
    // behaves the same way: one event in, one event out.
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#mail" }, "value": "a@b.example" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#mail-echo", "equals": "a@b.example" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#mail-log", "equals": "[a@b.example]" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#mail" }, "value": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#mail-echo", "equals": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#mail-log", "equals": "[a@b.example][]" })).await);

    // A date field is the one kind set directly, because typing into it
    // depends on the machine's locale. It too raises one event per fill.
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#due" }, "value": "2026-03-05" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#due-echo", "equals": "2026-03-05" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#due-log", "equals": "[2026-03-05]" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#due" }, "value": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#due-echo", "equals": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#due-log", "equals": "[2026-03-05][]" })).await);

    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#notes" }, "value": "new notes" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#notes-echo", "equals": "new notes" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#notes" }, "value": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#notes-echo", "equals": "" })).await);

    // A contenteditable has no value at all: the range is selected first
    // so what is typed replaces it.
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#rich" }, "value": "new rich" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#rich-echo", "equals": "new rich" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#rich" }, "value": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#rich-echo", "equals": "" })).await);
}

/// Focusing and typing are separate round trips, and the text goes to
/// whatever holds the focus when it lands. #grabby hands the focus
/// straight to #grabbed, the way an autofocusing dialog does: the fill
/// must refuse rather than type into the other field and call it a pass.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn typing_is_refused_when_the_page_takes_the_focus_away() {
    let mut live = open().await;
    refused(
        run(&mut live, json!({ "kind": "fill", "selector": { "css": "#grabby" }, "value": "Stolen" })).await,
        "lost focus before it could be typed into",
    );
    // Read back through the PAGE's own oninput mirrors: neither field saw
    // a character.
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#grabbed-echo", "equals": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#grabby-echo", "equals": "" })).await);
}

/// A relative url is resolved in the page, against the page's own
/// address. Here that is a file name beside the fixture.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_relative_navigate_resolves_against_the_page_it_is_on() {
    let mut live = open().await;
    let out = run(&mut live, json!({ "kind": "navigate", "url": "autorun-live-2.html" })).await;
    assert!(out.ok, "{}", out.detail);
    assert!(out.detail.contains("autorun-live-2.html"), "the outcome names where it went: {}", out.detail);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#second" }, "equals": "Second fixture" })).await);
    must(run(&mut live, json!({ "kind": "check_url", "contains": "autorun-live-2.html" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_native_list_is_chosen_by_its_label() {
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#type" }, "value": "Numeric" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#picked", "equals": "n" })).await);
    // A label a person typed in the wrong case still picks the option.
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#type" }, "value": "tEXT" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#picked", "equals": "t" })).await);
    refused(
        run(&mut live, json!({ "kind": "fill", "selector": { "css": "#type" }, "value": "Nope" })).await,
        "the list has no option \"Nope\"",
    );
    refused(
        run(&mut live, json!({ "kind": "fill", "selector": { "css": "#type" }, "value": "Disabled Choice" })).await,
        "the option \"Disabled Choice\" is disabled",
    );
    // Neither refusal changed the page.
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#picked", "equals": "t" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_chain_reaches_the_button_inside_the_dialog() {
    let mut live = open().await;
    // On its own the name is ambiguous: a dialog button and a page button.
    refused(
        run(&mut live, json!({ "kind": "click", "selector": { "role": "button", "name": "Add Method", "exact": true } })).await,
        "matched 2 elements",
    );
    must(run(&mut live, json!({ "kind": "click", "selector": [
        { "role": "dialog", "name": "Add Rating Method" },
        { "role": "button", "name": "Add Method" }
    ] })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#which", "equals": "dialog button" })).await);

    // The string form scripts have always used: `text=` takes the LAST
    // match, which here is the page button, not the dialog's.
    must(run(&mut live, json!({ "kind": "click", "selector": "text=Add Method" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#which", "equals": "page button" })).await);

    // And `nth` picks from the two the bare name matched, in page order.
    must(run(&mut live, json!({ "kind": "click", "selector": { "role": "button", "name": "Add Method", "exact": true, "nth": 0 } })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#which", "equals": "dialog button" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn an_alert_does_not_freeze_the_run() {
    let mut live = open().await;
    let out = run(&mut live, json!({ "kind": "click", "selector": { "css": "#alerter" } })).await;
    assert!(out.ok, "{}", out.detail);
    // The very next call would hang forever on the old client.
    let next = run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 1" })).await;
    assert!(next.ok, "{}", next.detail);
    assert!(
        out.detail.contains("alert: Saved!") || next.detail.contains("alert: Saved!"),
        "nobody was told about the alert: {:?} / {:?}",
        out.detail,
        next.detail
    );
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn expectations_wait_and_say_what_they_saw() {
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "expect_visible", "selector": { "role": "heading", "name": "Fixture" } })).await);
    must(run(&mut live, json!({ "kind": "expect_count", "selector": { "css": "#rows li" }, "equals": 3 })).await);
    // The spinner is there at first and removed after 700 ms.
    must(run(&mut live, json!({ "kind": "expect_hidden", "selector": { "css": "#spinner" } })).await);
    must(run(&mut live, json!({ "kind": "expect_count", "selector": { "css": "#spinner" }, "equals": 0 })).await);
    must(run(&mut live, json!({ "kind": "expect_contains_text", "selector": { "css": "#rows" }, "value": "two" })).await);
    // #ghost really is in the page and really cannot be seen. A locator
    // keeps only what is visible, so saying "is not on the page" here
    // would send someone hunting for a selector bug that is not there.
    refused(
        run(&mut live, json!({ "kind": "expect_visible", "selector": { "css": "#ghost" }, "timeout_ms": 500 })).await,
        "is there but cannot be seen",
    );
    refused(
        run(&mut live, json!({ "kind": "expect_visible", "selector": { "css": "#really-not-here" }, "timeout_ms": 500 })).await,
        "is not on the page",
    );

    let started = std::time::Instant::now();
    let out = run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 9", "timeout_ms": 600 })).await;
    assert!(!out.ok);
    assert!(out.detail.contains("\"clicked 9\"") && out.detail.contains("\"clicked 0\""), "{}", out.detail);
    assert!(started.elapsed() < Duration::from_secs(3), "it must give up at its timeout");
}

/// The checks the unit tests could only fake: an attribute read off a real
/// element, the whole page's text, the address bar, and both shapes of
/// `wait_for`.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn attributes_page_text_the_url_and_waiting_for_an_element() {
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "expect_attribute", "selector": { "css": "#save" }, "name": "data-state", "equals": "ready" })).await);
    let out = run(&mut live, json!({ "kind": "expect_attribute", "selector": { "css": "#save" }, "name": "data-state", "equals": "gone", "timeout_ms": 500 })).await;
    refused(out, "expected data-state=\"gone\" but saw \"ready\"");
    refused(
        run(&mut live, json!({ "kind": "expect_attribute", "selector": { "css": "#save" }, "name": "data-nothing", "equals": "x", "timeout_ms": 400 })).await,
        "has no data-nothing attribute",
    );

    must(run(&mut live, json!({ "kind": "check_text", "value": "Method Name" })).await);
    refused(
        run(&mut live, json!({ "kind": "check_text", "value": "Not On This Page" })).await,
        "page does NOT contain Not On This Page",
    );
    must(run(&mut live, json!({ "kind": "check_url", "contains": "autorun-live.html" })).await);
    refused(run(&mut live, json!({ "kind": "check_url", "contains": "dev.azure.com" })).await, "url is file:///");

    // Both shapes of selector: the string scripts have always used, and a
    // locator.
    must(run(&mut live, json!({ "kind": "wait_for", "selector": "#late", "timeout_ms": 2000 })).await);
    must(run(&mut live, json!({ "kind": "wait_for", "selector": { "role": "button", "name": "Late button" }, "timeout_ms": 2000 })).await);
    refused(
        run(&mut live, json!({ "kind": "wait_for", "selector": { "css": "#never-here" }, "timeout_ms": 400 })).await,
        "waited 400ms and never saw #never-here",
    );
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_page_that_will_not_load_is_reported_and_a_screenshot_is_a_jpeg() {
    let mut live = open().await;
    let shot = page::screenshot(&mut live.cdp).await.expect("no screenshot");
    assert_eq!(&shot[..3], &[0xFF, 0xD8, 0xFF]);
    // Port 1 is refused by the browser itself, so this needs no network.
    refused(run(&mut live, json!({ "kind": "navigate", "url": "http://127.0.0.1:1/" })).await, "would not load");
}

/// A control can be far taller than the window; the click point is the
/// middle of the part that is actually on screen, not the middle of the
/// element (which would be off the bottom and hit nothing).
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn an_element_taller_than_the_viewport_is_still_clickable() {
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#tall" } })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#which" }, "equals": "tall" })).await);
}

/// Clicking something mid-animation is how a runner clicks the wrong
/// thing: the element has moved by the time the event lands.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_moving_element_is_refused_and_one_that_settles_is_clicked() {
    let mut live = open().await;
    refused(run(&mut live, json!({ "kind": "click", "selector": { "css": "#mover" } })).await, "is still moving");
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 0" })).await);
    // #settler moves for about 1.2 s and then stops for good.
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#settler" } })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#which" }, "equals": "settled" })).await);
}

/// The outline a watcher sees, and the gap it opens. A page is free to put
/// a modal up during that gap, and the click must not go through it.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn the_highlight_shows_and_a_target_covered_during_it_is_refused() {
    let mut live = open().await;
    let target: Target = serde_json::from_value(json!({ "css": "#save" })).expect("a valid target");
    let handles = resolve(&mut live.cdp, &target).await.expect("could not resolve #save");
    assert_eq!(handles.len(), 1);
    page::call_value(&mut live.cdp, &handles[0], HIGHLIGHT_JS, &[]).await.expect("highlight failed");
    let read = "document.getElementById('save').style.outline";
    let outlined = page::eval_value(&mut live.cdp, read).await.expect("could not read the outline");
    let outlined = outlined.as_str().unwrap_or("").to_string();
    assert!(outlined.contains("3px") && outlined.contains("solid"), "no outline: {outlined:?}");
    // And it takes itself back off, so nothing is left marked up. Polled
    // rather than slept through: the production timer is 1200ms, and a
    // fixed wait just over it is a flake waiting for a loaded machine.
    let mut after = outlined.clone();
    let give_up = std::time::Instant::now() + Duration::from_secs(5);
    while !after.is_empty() && std::time::Instant::now() < give_up {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let v = page::eval_value(&mut live.cdp, read).await.expect("could not read the outline");
        after = v.as_str().unwrap_or("still there").to_string();
    }
    assert_eq!(after, "", "the outline was never taken back off");

    // #coverme covers itself the moment its style changes, which is what
    // the highlight does. A long enough pause and the cover is up before
    // the click.
    let slow = Timing { highlight_ms: 900, action_ms: 6000, ..timing() };
    let out = run_with(&mut live, json!({ "kind": "click", "selector": { "css": "#coverme" } }), &slow).await;
    // And it names what is in the way, which is the only thing a person
    // watching can act on.
    refused(out.clone(), "moved or was covered just before the click");
    refused(out, "is covered by div#cover");
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 0" })).await);
}

/// Text locators: the deepest element carrying the words, not the panel
/// around it; `exact` when the words are a prefix of something longer; and
/// never a hidden copy.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn text_locators_take_the_deepest_visible_match() {
    let mut live = open().await;
    let deep = json!({ "text": "Unique Deep Words" });
    must(run(&mut live, json!({ "kind": "expect_count", "selector": deep, "equals": 1 })).await);
    // The one match is the span, not the div that wraps it.
    must(run(&mut live, json!({ "kind": "expect_attribute", "selector": deep, "name": "id", "equals": "deep-inner" })).await);
    // There really is a second, hidden copy of those words: asked for it
    // by name, the runner finds it and says it cannot be seen.
    must(run(&mut live, json!({ "kind": "expect_count", "selector": { "text": "Unique Deep Words", "visible": false }, "equals": 2 })).await);
    refused(
        run(&mut live, json!({ "kind": "expect_visible", "selector": { "text": "Unique Deep Words", "visible": false, "nth": 1 }, "timeout_ms": 500 })).await,
        "is there but cannot be seen",
    );

    refused(
        run(&mut live, json!({ "kind": "expect_visible", "selector": { "text": "Exactly This" }, "timeout_ms": 500 })).await,
        "matched 2 elements",
    );
    must(run(&mut live, json!({ "kind": "expect_attribute", "selector": { "text": "Exactly This", "exact": true }, "name": "id", "equals": "exacty" })).await);
}

/// Two matched roots that nest reach the same element twice. Without the
/// de-duplication by backend node id, the count below is 2 and `nth: 1`
/// points at a second copy of the same button.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn nested_scopes_do_not_count_the_same_element_twice() {
    let mut live = open().await;
    // Three plain rows plus the outer and inner nested items.
    must(run(&mut live, json!({ "kind": "expect_count", "selector": { "role": "listitem" }, "equals": 5 })).await);
    let chain = json!([{ "role": "listitem" }, { "role": "button", "name": "Nested Go" }]);
    must(run(&mut live, json!({ "kind": "expect_count", "selector": chain, "equals": 1 })).await);
    must(run(&mut live, json!({ "kind": "click", "selector": chain })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#which" }, "equals": "nested button" })).await);
}

/// The fixture carries an iframe, so a sub-frame load is always in flight
/// alongside the main one; a navigation must wait for its OWN load. A
/// `#fragment` loads nothing at all and must not wait for anything.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn navigation_waits_for_its_own_load_and_a_fragment_waits_for_nothing() {
    let mut live = open().await;
    // A second full load of a page that has a sub-frame.
    let out = run(&mut live, json!({ "kind": "navigate", "url": fixture_url() })).await;
    assert!(out.ok, "{}", out.detail);
    assert!(out.detail.starts_with("loaded "), "{}", out.detail);
    must(run(&mut live, json!({ "kind": "expect_visible", "selector": { "css": "#frame" } })).await);

    let started = std::time::Instant::now();
    let out = run(&mut live, json!({ "kind": "navigate", "url": format!("{}#bottom", fixture_url()) })).await;
    assert!(out.ok, "{}", out.detail);
    assert!(out.detail.starts_with("moved to "), "a fragment loads nothing: {}", out.detail);
    assert!(started.elapsed() < Duration::from_secs(5), "it waited for a load that never comes");
    must(run(&mut live, json!({ "kind": "check_url", "contains": "#bottom" })).await);
}

/// A web application small enough to read in one go. `GET /` is the home
/// page for a browser carrying a cookie the server still honours, and the
/// login form for anyone else. `POST /login` checks the login, sets an
/// HttpOnly cookie, and answers with a page that stores a token in
/// localStorage (quotes and all) and then goes home, optionally only after
/// a "Continue here" prompt that shows up a quarter of a second late.
struct App {
    port: u16,
    logins: Arc<AtomicUsize>,
    generation: Arc<AtomicUsize>,
    prompt: Arc<AtomicBool>,
}

const LOGIN_PAGE: &str = r#"<!doctype html><title>Login</title>
<form method="post" action="/login">
  <label>Username <input name="u"></label>
  <label>Password <input name="p" type="password"></label>
  <button>Login</button>
</form>"#;

/// A tiny page behind the sign-in: a button that reveals a form, and a
/// paragraph an onclick handler writes into - so a script can prove it
/// really drove the page rather than a fake that agrees with whatever it
/// is told.
const LEAVE_PAGE: &str = r#"<h1>Leave</h1><button id="new" onclick="document.getElementById('form').hidden=false">New request</button><div id="form" hidden><input aria-label="Reason"><button onclick="document.getElementById('msg').textContent='Saved'">Save</button></div><p id="msg"></p>"#;

/// The same page with no Save button, so a script that clicks one fails
/// for a real, page-shaped reason.
const BROKEN_PAGE: &str = r#"<h1>Leave</h1><button id="new" onclick="document.getElementById('form').hidden=false">New request</button><div id="form" hidden><input aria-label="Reason"></div><p id="msg"></p>"#;

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            // Two hex digits must follow; anything else is a literal percent.
            b'%' if i + 3 <= bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form_field(body: &str, name: &str) -> String {
    body.split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| percent_decode(v))
        .unwrap_or_default()
}

fn respond(stream: &mut std::net::TcpStream, extra_headers: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
        body.len()
    );
}

impl App {
    fn start() -> App {
        let listener = TcpListener::bind("127.0.0.1:0").expect("no free port");
        let port = listener.local_addr().unwrap().port();
        let app = App {
            port,
            logins: Arc::new(AtomicUsize::new(0)),
            generation: Arc::new(AtomicUsize::new(0)),
            prompt: Arc::new(AtomicBool::new(false)),
        };
        let (logins, generation, prompt) = (app.logins.clone(), app.generation.clone(), app.prompt.clone());
        // The thread ends with the test process; the listener has no other owner.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                // Headers first, then as much body as Content-Length says.
                let (head, body) = loop {
                    let Ok(n) = stream.read(&mut buf) else { break (String::new(), String::new()) };
                    if n == 0 {
                        break (String::from_utf8_lossy(&raw).into_owned(), String::new());
                    }
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).into_owned();
                    if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                        let want = head
                            .lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        if rest.len() >= want {
                            break (head.to_string(), rest.to_string());
                        }
                    }
                };
                let first = head.lines().next().unwrap_or("");
                let cookie = head
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().starts_with("cookie:").then(|| l[7..].trim().to_string()))
                    .unwrap_or_default();
                let gen = generation.load(Ordering::SeqCst);
                let user = cookie
                    .split(';')
                    .filter_map(|c| c.trim().strip_prefix("sid="))
                    .filter_map(|v| v.rsplit_once('-'))
                    .find(|(_, g)| *g == gen.to_string())
                    .map(|(u, _)| u.to_string());

                if first.starts_with("POST /login") {
                    let (u, p) = (form_field(&body, "u"), form_field(&body, "p"));
                    let good = matches!((u.as_str(), p.as_str()), ("kim", "p\"w 1") | ("lee", "s3cret"));
                    if !good {
                        respond(&mut stream, "", "<!doctype html><p id=\"bad\">Wrong username or password</p>");
                        continue;
                    }
                    logins.fetch_add(1, Ordering::SeqCst);
                    let go = if prompt.load(Ordering::SeqCst) {
                        "setTimeout(() => { const b = document.createElement('button'); b.textContent = 'Continue here'; b.onclick = () => { location.href = '/'; }; document.body.append(b); }, 250);"
                    } else {
                        "location.replace('/');"
                    };
                    let page = format!(
                        "<!doctype html><title>Signing in</title><body><script>localStorage.setItem('token', 'tok \"q\" {u}'); {go}</script></body>"
                    );
                    respond(&mut stream, &format!("Set-Cookie: sid={u}-{gen}; HttpOnly; Path=/; SameSite=Lax\r\n"), &page);
                } else if first.starts_with("GET / ") {
                    match user {
                        Some(u) => respond(
                            &mut stream,
                            "",
                            &format!("<!doctype html><title>Home</title><h1 id=\"home\">Home</h1><p id=\"who\"></p><script>document.getElementById('who').textContent = '{u} / ' + localStorage.getItem('token');</script>"),
                        ),
                        None => respond(&mut stream, "", LOGIN_PAGE),
                    }
                } else if first.starts_with("GET /leave") {
                    match user {
                        Some(_) => respond(&mut stream, "", LEAVE_PAGE),
                        None => respond(&mut stream, "", LOGIN_PAGE),
                    }
                } else if first.starts_with("GET /broken") {
                    match user {
                        Some(_) => respond(&mut stream, "", BROKEN_PAGE),
                        None => respond(&mut stream, "", LOGIN_PAGE),
                    }
                } else {
                    respond(&mut stream, "", "<!doctype html><p>nothing here</p>");
                }
            }
        });
        app
    }
    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    /// The same server under another ORIGIN, for the allowlist.
    fn other_origin(&self) -> String {
        format!("http://localhost:{}", self.port)
    }
}

fn recipe_for(app: &App) -> SignInRecipe {
    let r: SignInRecipe = serde_json::from_value(json!({
        "start_url": format!("{}/", app.base()),
        "steps": [
            { "kind": "fill", "selector": { "role": "textbox", "name": "Username" }, "value": "{{username}}" },
            { "kind": "fill", "selector": { "css": "input[type=password]" }, "value": "{{password}}" },
            { "kind": "click", "selector": { "role": "button", "name": "Login" } },
            { "kind": "when_visible", "selector": { "role": "button", "name": "Continue here" }, "within_ms": 1500,
              "then": [ { "kind": "click", "selector": { "role": "button", "name": "Continue here" } } ] }
        ],
        "signed_in": { "css": "#home" }
    }))
    .unwrap();
    r.validate().expect("the test wrote an invalid recipe");
    r
}

fn kim() -> Account {
    Account { key: "kim".into(), label: "Kim".into(), username: "kim".into(), password: "p\"w 1".into() }
}

fn lee() -> Account {
    Account { key: "lee".into(), label: "Lee".into(), username: "lee".into(), password: "s3cret".into() }
}

async fn who(live: &mut Live) -> String {
    page::eval_value(&mut live.cdp, "document.getElementById('who')?.textContent ?? ''")
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn the_recipe_signs_in_through_the_real_form_and_saves_the_session() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    let mut live = open().await;
    let out = sign_in(&mut live.cdp, root.path(), &recipe_for(&app), &kim(), &timing()).await;
    assert!(out.ok, "{} / {:?}", out.detail, out.steps);
    assert!(!out.used_saved_session);
    // Both halves arrived: the HttpOnly cookie (the server greets kim) and
    // the token with its quotes intact.
    assert_eq!(who(&mut live).await, "kim / tok \"q\" kim");
    assert_eq!(app.logins.load(Ordering::SeqCst), 1);
    let saved = std::fs::read_to_string(v2_lib::autorun::accounts::session_path(root.path(), "kim")).unwrap();
    assert!(saved.contains("sid"), "the HttpOnly cookie was not captured");
    // The password reaches the page and nowhere else.
    for s in &out.steps {
        assert!(!s.detail.contains("p\"w 1"), "a step detail shows the password: {}", s.detail);
    }
    assert!(!saved.contains("p\\\"w 1"));
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_second_browser_is_signed_in_from_the_saved_session_without_touching_the_form() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    {
        let mut first = open().await;
        assert!(sign_in(&mut first.cdp, root.path(), &recipe_for(&app), &kim(), &timing()).await.ok);
    } // the first browser and its whole profile are gone
    let mut second = open().await;
    let out = sign_in(&mut second.cdp, root.path(), &recipe_for(&app), &kim(), &timing()).await;
    assert!(out.ok && out.used_saved_session, "{}", out.detail);
    assert_eq!(who(&mut second).await, "kim / tok \"q\" kim");
    assert_eq!(app.logins.load(Ordering::SeqCst), 1, "the form was posted a second time");
    // The seed script is gone: a token the page removes stays removed.
    must(run(&mut second, json!({ "kind": "navigate", "url": format!("{}/", app.base()) })).await);
    page::eval_value(&mut second.cdp, "localStorage.removeItem('token')").await.unwrap();
    must(run(&mut second, json!({ "kind": "navigate", "url": format!("{}/", app.base()) })).await);
    assert_eq!(who(&mut second).await, "kim / null");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_session_the_server_no_longer_honours_falls_back_to_the_form() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    {
        let mut first = open().await;
        assert!(sign_in(&mut first.cdp, root.path(), &recipe_for(&app), &kim(), &timing()).await.ok);
    }
    app.generation.fetch_add(1, Ordering::SeqCst); // every old cookie is now worthless
    let mut second = open().await;
    let out = sign_in(&mut second.cdp, root.path(), &recipe_for(&app), &kim(), &timing()).await;
    assert!(out.ok && !out.used_saved_session, "{}", out.detail);
    assert_eq!(app.logins.load(Ordering::SeqCst), 2);
    assert_eq!(who(&mut second).await, "kim / tok \"q\" kim");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn changing_account_in_one_browser_leaves_nothing_of_the_last_one() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    let mut live = open().await;
    assert!(sign_in(&mut live.cdp, root.path(), &recipe_for(&app), &kim(), &timing()).await.ok);
    let out = sign_in(&mut live.cdp, root.path(), &recipe_for(&app), &lee(), &timing()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(who(&mut live).await, "lee / tok \"q\" lee");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_prompt_that_only_sometimes_appears_is_handled_either_way() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    app.prompt.store(true, Ordering::SeqCst);
    let mut live = open().await;
    let with = sign_in(&mut live.cdp, root.path(), &recipe_for(&app), &kim(), &timing()).await;
    assert!(with.ok, "{} / {:?}", with.detail, with.steps);
    // And with no prompt (the first test) the same recipe passed too, saying so.
    app.prompt.store(false, Ordering::SeqCst);
    v2_lib::autorun::sessions::forget_session(root.path(), "kim");
    let without = sign_in(&mut live.cdp, root.path(), &recipe_for(&app), &kim(), &timing()).await;
    assert!(without.ok, "{}", without.detail);
    assert!(without.steps.iter().any(|s| s.detail.contains("did not appear")), "{:?}", without.steps);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_wrong_password_says_which_account_to_check() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    let mut live = open().await;
    let mut wrong = kim();
    wrong.password = "nope-nope".into();
    let t = Timing { nav_ms: 2500, ..timing() };
    let out = sign_in(&mut live.cdp, root.path(), &recipe_for(&app), &wrong, &t).await;
    assert!(!out.ok);
    assert!(out.detail.contains("\"kim\""), "{}", out.detail);
    assert!(!out.detail.contains("nope-nope"));
    assert!(!v2_lib::autorun::accounts::session_path(root.path(), "kim").exists(), "a failed sign-in saved a session");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_navigate_outside_the_projects_origins_is_refused_before_it_happens() {
    let app = App::start();
    let mut live = open().await;
    let policy = Policy::only(recipe_for(&app).origins());
    let inside = execute_in(&mut live.cdp, &action_of(json!({ "kind": "navigate", "url": format!("{}/", app.base()) })), &timing(), &policy).await;
    assert!(inside.ok, "{}", inside.detail);
    let outside = execute_in(&mut live.cdp, &action_of(json!({ "kind": "navigate", "url": format!("{}/", app.other_origin()) })), &timing(), &policy).await;
    refused(outside, "allowed origins");
    let still = page::eval_value(&mut live.cdp, "location.origin").await.unwrap();
    assert_eq!(still.as_str(), Some(app.base().as_str()), "the browser went there anyway");
    // A relative address resolves first and is judged after.
    let relative = execute_in(&mut live.cdp, &action_of(json!({ "kind": "navigate", "url": "/" })), &timing(), &policy).await;
    assert!(relative.ok, "{}", relative.detail);
}

// ---------------------------------------------------------------------
// The unattended replay engine, against the same server and a real
// browser per case - the `Browsers` the command builds, not a fake.
// ---------------------------------------------------------------------

/// One real background browser per `open`, with the same patience as
/// `open()` above, counting how many were started.
struct LiveBrowsers {
    started: usize,
    current: Option<LaunchedBrowser>,
}

impl Browsers for LiveBrowsers {
    type D = Cdp;
    async fn open(&mut self) -> Result<Cdp, String> {
        let extra = background_args();
        let browser = launch_with(Browser::Edge, &extra)?;
        self.started += 1;
        let mut last = String::new();
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            match Cdp::connect(browser.port).await {
                Ok(c) => {
                    self.current = Some(browser);
                    return Ok(c);
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }
    async fn close(&mut self, d: Cdp) {
        drop(d);
        if let Some(mut b) = self.current.take() {
            let _ = b.child.kill();
            let _ = b.child.wait();
            for _ in 0..20 {
                if std::fs::remove_dir_all(&b.profile_dir).is_ok() || !b.profile_dir.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

impl Drop for LiveBrowsers {
    /// A failing assertion between `open` and `close` (and every early
    /// return `?` makes possible) must never leave an Edge process, or its
    /// throwaway profile, still on the machine.
    fn drop(&mut self) {
        if let Some(mut b) = self.current.take() {
            let _ = b.child.kill();
            let _ = b.child.wait();
            let _ = std::fs::remove_dir_all(&b.profile_dir);
        }
    }
}

/// Build one case's script from JSON, the same shape the unit tests use.
fn case_script(case_id: i32, account: Option<&str>, steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": case_id,
        "title": format!("case {case_id}"),
        "account": account,
        "steps": steps
    }))
    .expect("the test wrote an invalid case script")
}

fn new_run(pbi_id: i32) -> LocalRun {
    LocalRun {
        id: store::new_run_id(),
        pbi_id,
        started_at: "1700000000000".into(),
        cases: vec![],
        mode: "unattended".into(),
        published: None,
    }
}

#[tokio::test]
#[ignore = "starts real headless Edge processes"]
async fn an_unattended_selection_runs_each_case_in_its_own_browser_and_proposes_honestly() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    save_recipe(root.path(), "acme", "Web", &recipe_for(&app)).unwrap();
    save_accounts(root.path(), &[kim()]).unwrap();

    // Case 1: signs in, opens the request form and saves it - every action
    // is expected to pass.
    let case1 = case_script(
        1,
        Some("kim"),
        json!([
            { "step_number": 1, "actions": [
                { "kind": "navigate", "url": format!("{}/leave", app.base()) },
                { "kind": "click", "selector": { "role": "button", "name": "New request" } }
            ]},
            { "step_number": 2, "actions": [
                { "kind": "fill", "selector": { "role": "textbox", "name": "Reason" }, "value": "Need a new chair" }
            ]},
            { "step_number": 3, "actions": [
                { "kind": "click", "selector": { "role": "button", "name": "Save" } }
            ]},
            { "step_number": 4, "actions": [
                { "kind": "expect_contains_text", "selector": { "css": "#msg" }, "value": "Saved" }
            ]}
        ]),
    );
    // Case 2: the page that is missing its Save button - step 2 fails and
    // step 3 (the check) never runs.
    let case2 = case_script(
        2,
        Some("kim"),
        json!([
            { "step_number": 1, "actions": [
                { "kind": "navigate", "url": format!("{}/broken", app.base()) },
                { "kind": "click", "selector": { "role": "button", "name": "New request" } }
            ]},
            { "step_number": 2, "actions": [
                { "kind": "click", "selector": { "role": "button", "name": "Save" } }
            ]},
            { "step_number": 3, "actions": [
                { "kind": "expect_contains_text", "selector": { "css": "#msg" }, "value": "Saved" }
            ]}
        ]),
    );
    // Case 3: nobody signed in, and nothing checked - there is nothing to
    // propose either way.
    let case3 = case_script(
        3,
        None,
        json!([
            { "step_number": 1, "actions": [
                { "kind": "navigate", "url": format!("{}/", app.base()) }
            ]}
        ]),
    );
    store::save_scripts_atomically(root.path(), &[case1, case2, case3]).unwrap();

    let mut browsers = LiveBrowsers { started: 0, current: None };
    let mut run = new_run(42);
    let cases = vec![
        (1, "leaves a request".to_string()),
        (2, "hits a page missing its Save button".to_string()),
        (3, "just looks".to_string()),
    ];
    let cancel = AtomicBool::new(false);
    let mut events: Vec<ReplayProgress> = vec![];
    let res = run_selection(&mut browsers, root.path(), "acme", "Web", &mut run, &cases, &timing(), &cancel, &mut |p| {
        events.push(p);
    })
    .await;
    assert!(res.is_ok(), "{res:?}");
    assert_eq!(browsers.started, 3);
    assert_eq!(run.cases.len(), 3);

    let rec1 = &run.cases[0];
    assert_eq!(rec1.proposed, "Passed", "{} / {:?}", rec1.reason, rec1.steps);
    assert_eq!(rec1.verdict, "");
    assert_eq!(rec1.steps[0].step_number, SIGN_IN_STEP);
    assert_eq!(rec1.account.as_deref(), Some("kim"));
    for s in rec1.steps.iter().filter(|s| s.step_number != SIGN_IN_STEP) {
        let name = s.screenshot.as_ref().unwrap_or_else(|| panic!("step {} has no screenshot", s.step_number));
        let bytes = store::load_shot(root.path(), name).expect("the screenshot file should exist");
        assert!(
            bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8,
            "step {} screenshot does not start with the JPEG magic bytes",
            s.step_number
        );
    }

    let rec2 = &run.cases[1];
    assert_eq!(rec2.proposed, "Failed", "{}", rec2.reason);
    assert!(rec2.reason.starts_with("step 2:"), "{}", rec2.reason);
    let step3 = rec2.steps.iter().find(|s| s.step_number == 3).expect("case 2 should still record step 3");
    for o in &step3.outcomes {
        assert_eq!(o.detail, "not run: an earlier step of this case failed");
    }
    let step2 = rec2.steps.iter().find(|s| s.step_number == 2).expect("case 2 should still record step 2");
    assert!(
        step2.outcomes.iter().any(|o| o.screenshot.is_some()),
        "the failed step should carry its own screenshot: {:?}",
        step2.outcomes
    );

    // The second case did not inherit the first one's browser - it got a
    // fresh one, but restored kim's session rather than touching the form.
    assert_eq!(app.logins.load(Ordering::SeqCst), 1, "case 2 should not have posted the login form again");
    let case2_signin = rec2.steps.iter().find(|s| s.step_number == SIGN_IN_STEP).expect("case 2 should have signed in");
    assert!(
        case2_signin.outcomes.last().expect("a sign-in outcome").detail.contains("from a saved session"),
        "{:?}",
        case2_signin.outcomes
    );

    let rec3 = &run.cases[2];
    assert_eq!(rec3.proposed, "");
    assert!(rec3.reason.contains("checks nothing"), "{}", rec3.reason);

    let saved = store::load_run(root.path(), &run.id).unwrap().expect("the run should be on disk");
    assert_eq!(saved, run);
    assert_eq!(saved.mode, "unattended");
    assert!(saved.published.is_none());

    for e in &events {
        assert_eq!(e.run_id, run.id);
    }
    let case1_phases: Vec<&str> = events.iter().filter(|e| e.case_id == 1).map(|e| e.phase.as_str()).collect();
    assert_eq!(case1_phases.first(), Some(&"opening"), "{case1_phases:?}");
    assert_eq!(case1_phases.get(1), Some(&"signing_in"), "{case1_phases:?}");
    assert_eq!(case1_phases.last(), Some(&"done"), "{case1_phases:?}");
    assert!(
        case1_phases[2..case1_phases.len() - 1].iter().all(|p| *p == "step"),
        "{case1_phases:?}"
    );

    // With the saved session forgotten, case 2 has nothing to restore, so
    // signing it in the ordinary way costs a second real login - but only
    // one, because case 1 (running again first) re-saves the session case
    // 2 then restores exactly as it did above.
    sessions::forget_session(root.path(), "kim");
    let logins_before = app.logins.load(Ordering::SeqCst);
    let mut run2 = new_run(42);
    run_selection(&mut browsers, root.path(), "acme", "Web", &mut run2, &cases[..2], &timing(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(
        app.logins.load(Ordering::SeqCst),
        logins_before + 1,
        "forgetting kim's session should force exactly one more form sign-in"
    );
}

#[tokio::test]
#[ignore = "starts real headless Edge processes"]
async fn stopping_an_unattended_run_keeps_what_was_done_and_starts_nothing_more() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    save_recipe(root.path(), "acme", "Web", &recipe_for(&app)).unwrap();

    // No account: the point of this test is stopping BETWEEN cases, not
    // signing in - a script that checks one thing is enough to see a
    // "Passed" for the case that gets to run.
    let base = case_script(
        11,
        None,
        json!([
            { "step_number": 1, "actions": [
                { "kind": "navigate", "url": format!("{}/", app.base()) },
                { "kind": "check_text", "value": "Login" }
            ]}
        ]),
    );
    let mut scripts = Vec::new();
    for id in [11, 12, 13] {
        let mut s = base.clone();
        s.case_id = id;
        scripts.push(s);
    }
    store::save_scripts_atomically(root.path(), &scripts).unwrap();

    let mut browsers = LiveBrowsers { started: 0, current: None };
    let mut run = new_run(7);
    let cases = vec![(11, "first".to_string()), (12, "second".to_string()), (13, "third".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root.path(), "acme", "Web", &mut run, &cases, &timing(), &cancel, &mut |p| {
        if p.case_id == 12 && p.phase == "opening" {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .await
    .unwrap();

    assert_eq!(run.cases.len(), 2, "case 13 never started, so it is left out entirely: {:?}", run.cases.iter().map(|c| c.case_id).collect::<Vec<_>>());
    assert_eq!(run.cases[0].proposed, "Passed", "{}", run.cases[0].reason);
    assert!(run.cases[1].proposed.is_empty(), "{}", run.cases[1].proposed);
    assert!(
        run.cases[1].steps.iter().all(|s| s.outcomes.iter().all(|o| o.detail == "not run: the run was stopped")),
        "{:?}",
        run.cases[1].steps
    );
    assert_eq!(browsers.started, 2);

    let saved = store::load_run(root.path(), &run.id).unwrap().expect("the run should be on disk");
    assert_eq!(saved.cases.len(), 2);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_background_browser_really_is_a_desktop_sized_window() {
    let mut browsers = LiveBrowsers { started: 0, current: None };
    let mut cdp = browsers.open().await.expect("Edge did not start");
    let nav = execute_with(&mut cdp, &action_of(json!({ "kind": "navigate", "url": fixture_url() })), &timing()).await;
    assert!(nav.ok, "{}", nav.detail);
    let size = page::eval_value(&mut cdp, "[window.innerWidth, window.innerHeight]")
        .await
        .expect("could not read the window size");
    let dims = size.as_array().expect("window size should read as an array");
    let width = dims[0].as_f64().unwrap_or(0.0);
    let height = dims[1].as_f64().unwrap_or(0.0);
    browsers.close(cdp).await;
    // A headless default of 800x600 would lay the application out as a
    // tablet - `background_args`'s --window-size must actually take
    // effect. >= rather than == because a scrollbar can shave a little off
    // the reported inner width.
    assert!(width >= 1300.0, "width was {width}, expected close to 1366");
    assert!(height >= 700.0, "height was {height}, expected close to 900");
}
