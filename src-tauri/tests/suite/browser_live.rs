//! The same rules as the unit tests, against a REAL browser. Ignored by
//! default: they start (headless) Edge. Run them on purpose:
//!
//!   cargo test --test suite browser_live:: -- --ignored --test-threads=1
//!
//! One at a time, because each starts its own browser. The `browser_live::`
//! filter matters: without it `--ignored` also runs the other ignored tests
//! in the suite, and one of those needs the network.
//!
//! Every assertion reads something the PAGE wrote - a mirror updated by an
//! `oninput` handler, a paragraph written by an `onclick` - never something
//! Rust believes it sent. That is the whole point of this file: the unit
//! tests answer from a fake that was written from the same beliefs as the
//! code, so only a real browser can say whether those beliefs were right.

use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use v2_lib::autorun::accounts::{save_accounts, Account};
use v2_lib::autorun::nav::{check_path, load_nav, put_path};
use v2_lib::autorun::recorder;
use v2_lib::autorun::recipe::{save_recipe, SignInRecipe};
use v2_lib::autorun::replay::{run_selection, Browsers, SIGN_IN_STEP};
use v2_lib::autorun::runner::run_step;
use v2_lib::autorun::signin_recorder::{self, Draft, FieldChoice, FieldRole, Step};
use v2_lib::autorun::signin::sign_in;
use v2_lib::autorun::{sessions, store, CaseScript, LocalRun, StepScript};
use v2_lib::browser::actions::{execute_in, execute_with, Action, ActionOutcome, HIGHLIGHT_JS, Policy};
use v2_lib::browser::cdp::Cdp;
use v2_lib::browser::launch::{background_args, launch_with, Browser, LaunchedBrowser};
use v2_lib::browser::locator::{resolve, Target};
use v2_lib::browser::page;
use v2_lib::browser::snapshot::{probe, snapshot, DEFAULT_LIMIT};
use v2_lib::browser::timing::Timing;
use v2_lib::commands::autorun_record_signin::check_sign_in;
use v2_lib::events::ReplayProgress;

/// Windows holds a just-exited browser's profile files open for a moment;
/// a few retries is the difference between a clean temp folder and one
/// left behind per test. Shared by every place a browser's profile is
/// torn down: `Live`'s own `Drop`, `LiveBrowsers::close`, and
/// `LiveBrowsers`'s own `Drop`.
fn remove_profile_dir(dir: &Path) {
    for _ in 0..20 {
        if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Live {
    browser: LaunchedBrowser,
    cdp: Cdp,
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.browser.child.kill();
        let _ = self.browser.child.wait();
        remove_profile_dir(&self.browser.profile_dir);
    }
}

fn fixture_url() -> String {
    let path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/autorun-live.html").replace('\\', "/");
    // The repo path has spaces in it on this machine.
    format!("file:///{}", path.trim_start_matches('/').replace(' ', "%20"))
}

/// The page whose frames are built the way `<phr-employee-search>` builds
/// its own: no `src`, filled with `document.write`.
fn iframe_fixture_url() -> String {
    fixture_url().replace("autorun-live.html", "autorun-iframe.html")
}

/// Short enough that a failing test fails fast, long enough for the
/// fixture's 700 ms of being disabled and covered.
fn timing() -> Timing {
    Timing { action_ms: 4000, expect_ms: 3000, nav_ms: 15000, poll_ms: 100, highlight_ms: 0, lease_wait_ms: 300 }
}

fn action_of(value: serde_json::Value) -> Action {
    serde_json::from_value(value).expect("the test wrote an invalid action")
}

fn action_target(value: serde_json::Value) -> Target {
    serde_json::from_value(value).expect("the test wrote an invalid locator")
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

/// `open()`, then on to the frame fixture.
async fn open_iframes() -> Live {
    let mut live = open().await;
    let out = run(&mut live, json!({ "kind": "navigate", "url": iframe_fixture_url() })).await;
    assert!(out.ok, "the iframe fixture did not load: {}", out.detail);
    must(run(&mut live, json!({ "kind": "expect_visible", "selector": { "css": "#es-frame" } })).await);
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

/// A name or a text saved with an em dash finds the page's en dash and
/// hyphen, through the real accessibility tree and the real `TEXT_JS`.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_saved_em_dash_finds_the_pages_en_dash_and_hyphen() {
    let mut live = open().await;
    let add = r#"(() => {
      const bar = document.createElement('div');
      bar.setAttribute('role', 'progressbar');
      bar.setAttribute('aria-label', 'Step 1 of 9 \u2013 Cycle Setup');
      bar.style.cssText = 'width:40px;height:10px';
      const hyphen = document.createElement('p');
      hyphen.textContent = 'Step 2 of 9 - Eval Rules';
      const en = document.createElement('p');
      en.textContent = 'Step 3 of 9 \u2013 Close';
      document.body.append(bar, hyphen, en);
      return true;
    })()"#;
    page::eval_value(&mut live.cdp, add).await.expect("could not add the dash elements");
    for (locator, what) in [
        (json!({ "role": "progressbar", "name": "Step 1 of 9 \u{2014} Cycle Setup", "exact": true }), "role name, en dash"),
        (json!({ "text": "Step 2 of 9 \u{2014} Eval Rules", "exact": true }), "text, hyphen"),
        (json!({ "text": "Step 3 of 9 \u{2014} Close", "exact": true }), "text, en dash"),
        (json!({ "text": "step 3 of 9 - close" }), "text, contains"),
    ] {
        let handles = resolve(&mut live.cdp, &action_target(locator)).await.expect("resolve failed");
        assert_eq!(handles.len(), 1, "{what}");
    }
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
        [&out.detail, &next.detail].iter().any(|d| d.contains("(an alert dialog was accepted: \"Saved!\")")),
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

/// 2026-09-30, PeoplesHR's menu: the words of an entry are
/// `pointer-events: none` inside the row that takes the click, so the
/// point at the words' centre belongs to the row. That is where a person's
/// click lands too, and the row is not covering its own words.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn words_that_let_a_click_through_to_their_row_are_clicked_through_it() {
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "click", "selector": { "text": "Menu Row Words", "exact": true } })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#which" }, "equals": "menu row" })).await);
}

/// 2026-10-01, the real PeoplesHR cause: "Performance Management System"
/// wraps onto two lines, and the centre of the words' box lands past the
/// end of the short second line - on the row, not the words. The click
/// aims at a line the words are really drawn on.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn words_that_wrap_are_clicked_on_a_line_they_are_drawn_on() {
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "click", "selector": { "text": "Wrapped Menu Entry Words" } })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#which" }, "equals": "wrapped row" })).await);
}

/// 2026-10-01, PeoplesHR: a menu entry's words were also on the page
/// elsewhere, so the recorded words matched three things in the fresh
/// browser's check. The recorder counts at the click and scopes the words
/// to the widest id that holds them once; twins inside one id get their
/// place. Each recorded locator must then click the one that was clicked.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_recorded_click_on_words_shown_twice_finds_only_that_one_again() {
    let mut live = open().await;
    recorder::arm(&mut live.cdp).await.expect("the recorder could not listen");
    let menu_echo = json!([{ "css": "#rec-menu-list" }, { "text": "Echoed Entry", "exact": true }]);
    let child_twin = json!([{ "css": "#rec-menu-list" }, { "text": "Twin Label", "exact": true, "nth": 1 }]);
    must(run(&mut live, json!({ "kind": "click", "selector": menu_echo })).await);
    must(run(&mut live, json!({ "kind": "click", "selector": child_twin })).await);
    let (stop, cancel) = (AtomicBool::new(true), AtomicBool::new(false));
    let captured = recorder::capture(&mut live.cdp, &stop, &cancel, &mut |_| {}).await;
    assert_eq!(captured.clicks.len(), 2, "{captured:?}");
    assert_eq!(captured.clicks[0], action_target(menu_echo.clone()), "scoped to the menu tree, not the sidebar around both lists");
    assert_eq!(captured.clicks[1], action_target(child_twin.clone()), "the child twin, by its place in the menu tree");

    // And each finds only what was clicked: replayed, the page says so.
    let replay = |t: &Target| json!({ "kind": "click", "selector": serde_json::to_value(t).unwrap() });
    must(run(&mut live, replay(&captured.clicks[0])).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#which" }, "equals": "menu echo" })).await);
    must(run(&mut live, replay(&captured.clicks[1])).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#which" }, "equals": "child twin" })).await);
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

/// The accessibility-tree snapshot an assistant reads instead of a
/// screenshot: it names real elements, in a shape a locator can be parsed
/// back out of, and it never prints a password's value.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn the_snapshot_names_what_a_locator_can_reach() {
    let mut live = open().await;
    let text = snapshot(&mut live.cdp, DEFAULT_LIMIT).await.expect("the snapshot call failed");

    // The visible "Save changes" button, not its display:none twin - if
    // the hidden one were counted too the probe below would answer
    // "matches: 2".
    let line = text
        .lines()
        .find(|l| l.contains("button") && l.contains("\"Save changes\""))
        .unwrap_or_else(|| panic!("no line named the Save changes button:\n{text}"));
    let arrow = line.rfind(" -> ").expect("the line has no locator suffix");
    let suffix = &line[arrow + 4..];
    let value: serde_json::Value =
        serde_json::from_str(suffix).unwrap_or_else(|e| panic!("the locator suffix is not valid JSON ({e}): {suffix}"));
    let target: Target =
        serde_json::from_value(value).unwrap_or_else(|e| panic!("the locator does not parse as a Target ({e}): {suffix}"));
    target.validate().expect("the locator does not validate");
    let probed = probe(&mut live.cdp, &target).await.expect("probe failed");
    assert!(probed.starts_with("matches: 1"), "expected exactly one match: {probed}");

    // The password field is named, but never its value.
    assert!(text.to_lowercase().contains("password"), "no password field in the snapshot:\n{text}");
    assert!(!text.contains("s3cret"), "the password value leaked into the snapshot: {text}");
}

/// `probe` answers how many elements a locator reaches right now, prints
/// one line per match (up to 10), and tells the caller how to narrow it
/// down when there is more than one.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_probe_counts_what_the_page_shows_and_says_how_to_narrow() {
    let mut live = open().await;
    // Ambiguous on its own: a dialog button and a page button, both named
    // "Add Method" and both visible.
    let target = action_target(json!({ "role": "button", "name": "Add Method", "exact": true }));
    let out = probe(&mut live.cdp, &target).await.expect("probe failed");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "matches: 2", "{out}");
    assert_eq!(lines.len(), 4, "expected the header, two match lines and the narrow sentence: {out}");
    assert!(lines.last().unwrap().contains("narrow the locator"), "{out}");

    let narrowed = action_target(json!({ "role": "button", "name": "Add Method", "exact": true, "nth": 1 }));
    let out2 = probe(&mut live.cdp, &narrowed).await.expect("probe failed");
    assert!(out2.starts_with("matches: 1"), "{out2}");
}

/// `run_step` is what an assistant's "try one action" tool ultimately
/// calls. Its effect has to land on the real page (read back through the
/// page's own mirror, never through what Rust believes it sent), and a
/// filled value must never come back out through the outcome's `detail`.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_tried_action_really_happens_in_the_page() {
    let mut live = open().await;
    let root = tempfile::tempdir().unwrap();
    let mut account: Option<String> = None;

    let click_step = StepScript {
        step_number: 1,
        actions: vec![action_of(json!({ "kind": "click", "selector": { "css": "#save" } }))],
        unchecked: None,
    };
    let outcomes = run_step(&mut live.cdp, root.path(), "org", "proj", &click_step, &timing(), &mut account)
        .await
        .expect("run_step failed");
    assert_eq!(outcomes.len(), 1);
    must(outcomes.into_iter().next().unwrap());
    let mirror = page::eval_value(&mut live.cdp, "document.getElementById('count').textContent").await.unwrap();
    assert_eq!(mirror.as_str(), Some("clicked 1"), "the click did not really reach the page");

    let fill_step = StepScript {
        step_number: 2,
        actions: vec![action_of(json!({ "kind": "fill", "selector": { "css": "#name" }, "value": "hello" }))],
        unchecked: None,
    };
    let out = run_step(&mut live.cdp, root.path(), "org", "proj", &fill_step, &timing(), &mut account)
        .await
        .expect("run_step failed");
    assert_eq!(out.len(), 1);
    let outcome = &out[0];
    assert!(outcome.ok, "{}", outcome.detail);
    let echo = page::eval_value(&mut live.cdp, "document.getElementById('echo').textContent").await.unwrap();
    assert_eq!(echo.as_str(), Some("hello"), "the fill did not really reach the page");
    assert!(!outcome.detail.contains("hello"), "the outcome detail leaked the filled value: {}", outcome.detail);
}

/// `when_visible` against a real page: a cookie banner that covers the
/// page half a second after it loads is dismissed, and the click in the
/// next step reaches the button the banner was covering. A second guard,
/// once the banner is gone, passes with "not shown, skipped".
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_banner_that_shows_up_late_is_dismissed_and_the_next_step_works() {
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "navigate", "url": fixture_url().replace("autorun-live.html", "autorun-banner.html") })).await);
    let root = tempfile::tempdir().unwrap();
    let mut account: Option<String> = None;
    let guard = json!({ "kind": "when_visible", "selector": { "css": "#close" }, "within_ms": 4000,
                        "then": [ { "kind": "click", "selector": { "css": "#close" } } ] });

    let dismiss = StepScript { step_number: 1, actions: vec![action_of(guard.clone())], unchecked: None };
    let out = run_step(&mut live.cdp, root.path(), "org", "proj", &dismiss, &timing(), &mut account)
        .await
        .expect("run_step failed");
    assert_eq!(out.len(), 1);
    assert!(out[0].ok && out[0].detail.contains("clicked"), "{}", out[0].detail);
    let gone = page::eval_value(&mut live.cdp, "document.getElementById('banner') === null").await.unwrap();
    assert_eq!(gone.as_bool(), Some(true), "the banner was not really dismissed");

    let next = StepScript {
        step_number: 2,
        actions: vec![
            action_of(json!({ "kind": "when_visible", "selector": { "css": "#close" }, "within_ms": 300,
                              "then": [ { "kind": "click", "selector": { "css": "#close" } } ] })),
            action_of(json!({ "kind": "click", "selector": { "css": "#save" } })),
        ],
        unchecked: None,
    };
    let out = run_step(&mut live.cdp, root.path(), "org", "proj", &next, &timing(), &mut account)
        .await
        .expect("run_step failed");
    assert!(out[0].ok && out[0].detail.ends_with("not shown, skipped"), "{}", out[0].detail);
    must(out[1].clone());
    let mirror = page::eval_value(&mut live.cdp, "document.getElementById('count').textContent").await.unwrap();
    assert_eq!(mirror.as_str(), Some("saved"), "the click after the guard did not reach the page");
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
                            &format!("<!doctype html><title>Home</title><nav><a href=\"/leave\">Leave</a></nav><h1 id=\"home\">Home</h1><p id=\"who\"></p><script>document.getElementById('who').textContent = '{u} / ' + localStorage.getItem('token');</script>"),
                        ),
                        None => respond(&mut stream, "", LOGIN_PAGE),
                    }
                } else if first.starts_with("GET /leave ") {
                    match user {
                        Some(_) => respond(&mut stream, "", LEAVE_PAGE),
                        None => respond(&mut stream, "", LOGIN_PAGE),
                    }
                } else if first.starts_with("GET /exports ") {
                    // The export page, served from the app's own origin so
                    // a run's allowlist lets a case go there.
                    let page = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/autorun-download.html");
                    respond(&mut stream, "", &std::fs::read_to_string(page).unwrap_or_default());
                } else if first.starts_with("GET /broken ") {
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
    let saved = std::fs::read_to_string(v2_lib::autorun::accounts::session_path(root.path(), "kim").unwrap()).unwrap();
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
    assert!(!v2_lib::autorun::accounts::session_path(root.path(), "kim").unwrap().exists(), "a failed sign-in saved a session");
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
            remove_profile_dir(&b.profile_dir);
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
            remove_profile_dir(&b.profile_dir);
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
        environment: None,
        resets: vec![],
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

#[tokio::test]
#[ignore = "starts real headless Edge processes"]
async fn a_recorded_menu_path_is_saved_only_after_it_replays_in_a_fresh_browser() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    save_recipe(root.path(), "acme", "Web", &recipe_for(&app)).unwrap();
    save_accounts(root.path(), &[kim()]).unwrap();
    let recipe = recipe_for(&app);

    // Record: sign in, listen, and click the menu entry with the real mouse.
    let mut live = open().await;
    assert!(sign_in(&mut live.cdp, root.path(), &recipe, &kim(), &timing()).await.ok);
    recorder::arm(&mut live.cdp).await.expect("the recorder could not listen");
    must(run(&mut live, json!({ "kind": "click", "selector": { "role": "link", "name": "Leave" } })).await);
    must(run(&mut live, json!({ "kind": "expect_visible", "selector": { "role": "heading", "name": "Leave" } })).await);
    let (stop, cancel) = (AtomicBool::new(true), AtomicBool::new(false));
    let captured = recorder::capture(&mut live.cdp, &stop, &cancel, &mut |_| {}).await;
    assert_eq!(captured.clicks.len(), 1, "{captured:?}");
    assert!(captured.clicks[0].describe().contains("Leave"), "{captured:?}");
    let path = recorder::finish("Leave", "", captured, "2026-09-24T10:00:00Z").unwrap();
    assert_eq!(path.arrived, "/leave");
    drop(live);

    // Check in a fresh browser, then save; a wrong ending is refused.
    let mut second = open().await;
    assert_eq!(check_path(&mut second.cdp, root.path(), &recipe, &kim(), &path, &timing(), &mut v2_lib::autorun::lease::Held::setup()).await, Ok("/leave".to_string()));
    drop(second);
    let mut wrong = path.clone();
    wrong.arrived = "/nowhere".into();
    let mut third = open().await;
    let err = check_path(&mut third.cdp, root.path(), &recipe, &kim(), &wrong, &Timing { nav_ms: 2000, ..timing() }, &mut v2_lib::autorun::lease::Held::setup()).await.unwrap_err();
    assert!(err.contains("the page ended on /leave, not /nowhere"), "{err}");
    put_path(root.path(), "acme", "Web", path).unwrap();
    assert_eq!(load_nav(root.path(), "acme", "Web").unwrap().modules.len(), 1);
}

/// The sign-in recorder against a real page: what a person does in the
/// browser - typing, Enter, a real click in pick mode - is what the page
/// reports, the picked click is not carried out, and the recipe made from
/// it signs in on its own in a fresh browser.
#[tokio::test]
#[ignore = "starts real headless Edge processes"]
async fn a_recorded_sign_in_is_what_the_page_saw_and_signs_in_again_in_a_fresh_browser() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    let start = format!("{}/", app.base());
    let mut live = open().await;
    signin_recorder::prepare(&mut live.cdp, &start, &timing()).await.expect("the recording could not start");
    let (stop, cancel, no_pick) = (AtomicBool::new(true), AtomicBool::new(false), AtomicBool::new(false));
    let mut steps: Vec<Step> = vec![];

    // Typing into the username field, then the password: moving on from
    // the username reports it, named by the accessibility tree.
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "input[name=u]" }, "value": "kim" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "input[name=p]" }, "value": "p\"w 1" })).await);
    let first = signin_recorder::capture(&mut live.cdp, &stop, &cancel, &no_pick, &mut |_| {}).await;
    steps.extend(first.steps);

    // Enter in the password field: the field, then the form's button - once,
    // though the browser's own Enter also clicks it.
    for kind in ["keyDown", "keyUp"] {
        live.cdp
            .call(
                "Input.dispatchKeyEvent",
                json!({ "type": kind, "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13, "text": "\r" }),
            )
            .await
            .expect("the key did not go in");
    }
    must(run(&mut live, json!({ "kind": "expect_visible", "selector": { "css": "#home" } })).await);
    let second = signin_recorder::capture(&mut live.cdp, &stop, &cancel, &no_pick, &mut |_| {}).await;
    steps.extend(second.steps);
    let words: Vec<String> = steps.iter().map(|s| s.target().describe()).collect();
    assert_eq!(steps.len(), 3, "{words:?}");
    assert!(matches!(&steps[0], Step::Field { password: false, .. }), "{words:?}");
    assert_eq!(words[0], "textbox \"Username\"");
    assert!(matches!(&steps[1], Step::Field { password: true, .. }), "{words:?}");
    assert!(matches!(&steps[2], Step::Click(_)) && words[2].contains("Login"), "{words:?}");

    // Pick mode: a real click on Leave is the signed-in check, and the page
    // does not go to Leave.
    page::eval_value(&mut live.cdp, signin_recorder::PICK_ON_JS).await.expect("pick mode");
    must(run(&mut live, json!({ "kind": "click", "selector": { "role": "link", "name": "Leave" } })).await);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let still_home = page::eval_value(&mut live.cdp, "!!document.getElementById('home')").await.unwrap();
    assert_eq!(still_home, json!(true), "the picked click was carried out");
    let pick = AtomicBool::new(true);
    let third = signin_recorder::capture(&mut live.cdp, &stop, &cancel, &pick, &mut |_| {}).await;
    assert!(third.steps.is_empty(), "{:?}", third.steps);
    let marker = third.marker.expect("the pick was not reported");
    assert_eq!(marker.describe(), "link \"Leave\"");
    assert!(!pick.load(Ordering::SeqCst));
    drop(live);

    let draft = Draft { organization: "acme".into(), project: "Web".into(), start_url: start, steps, marker: Some(marker) };
    let fields = [FieldChoice { role: FieldRole::Username, text: String::new() }, FieldChoice { role: FieldRole::Password, text: String::new() }];
    let recipe = draft.recipe(&fields, None).expect("the recording made no recipe");
    let before = app.logins.load(Ordering::SeqCst);
    let mut fresh = open().await;
    check_sign_in(&mut fresh.cdp, root.path(), &recipe, &kim(), &timing(), &mut v2_lib::autorun::lease::Held::setup()).await.expect("the recorded recipe did not sign in");
    assert_eq!(app.logins.load(Ordering::SeqCst), before + 1, "it signed in through the form");
    drop(fresh);

    // The wrong password is refused, and the sentence says so without it.
    let mut wrong = kim();
    wrong.password = "nope".into();
    let mut third_browser = open().await;
    let err = check_sign_in(&mut third_browser.cdp, root.path(), &recipe, &wrong, &Timing { nav_ms: 3000, ..timing() }, &mut v2_lib::autorun::lease::Held::setup())
        .await
        .unwrap_err();
    assert!(err.starts_with("the recorded steps ran, but the signed-in check never appeared"), "{err}");
    assert!(!err.contains("nope") && !err.contains("://"), "{err}");
}

/// `upload` against the real thing: a test file put into a plain file
/// input, and into a hidden one through the chooser its button opens - and
/// a button that opens no chooser refused with the sentence. What the
/// PAGE saw (each input's `change` handler writes the file's name and size)
/// is what is checked.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn upload_reaches_a_file_input_directly_and_through_the_chooser() {
    const PAGE: &str = r#"<!doctype html><html><body>
<input type="file" id="direct" onchange="document.getElementById('seen-direct').textContent = this.files[0].name + ' ' + this.files[0].size">
<p id="seen-direct"></p>
<input type="file" id="hidden" style="display:none" onchange="document.getElementById('seen-chooser').textContent = this.files[0].name + ' ' + this.files[0].size">
<button id="attach" onclick="document.getElementById('hidden').click()">Attach</button>
<p id="seen-chooser"></p>
<button id="nothing">Nothing</button>
</body></html>"#;
    let mut live = open().await;
    let root = tempfile::tempdir().unwrap();
    let files = v2_lib::test_files::folder(root.path(), "acme", "PMS");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("cv.txt"), b"hello").unwrap();
    let page = root.path().join("upload.html");
    std::fs::write(&page, PAGE).unwrap();
    let url = format!(
        "file:///{}",
        page.display().to_string().replace('\\', "/").trim_start_matches('/').replace(' ', "%20")
    );
    must(run(&mut live, json!({ "kind": "navigate", "url": url })).await);

    let step = StepScript {
        step_number: 1,
        actions: vec![
            action_of(json!({ "kind": "upload", "selector": { "css": "#direct" }, "file": "cv.txt" })),
            action_of(json!({ "kind": "upload", "selector": { "role": "button", "name": "Attach" }, "file": "cv.txt" })),
            action_of(json!({ "kind": "upload", "selector": { "css": "#nothing" }, "file": "cv.txt" })),
        ],
        unchecked: None,
    };
    let mut account = None;
    let out = run_step(&mut live.cdp, root.path(), "acme", "PMS", &step, &timing(), &mut account).await.unwrap();
    assert_eq!(out[0].detail, "uploaded \"cv.txt\" (5 bytes) to #direct");
    must(out[0].clone());
    must(out[1].clone());
    refused(out[2].clone(), "did not open a file chooser");

    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#seen-direct" }, "equals": "cv.txt 5" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#seen-chooser" }, "equals": "cv.txt 5" })).await);
}

/// Real Edge sends what the page log reads: a request that failed and a
/// console error, after `watch` - with the address's query string gone.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn the_page_log_names_a_failed_request_and_a_console_error_from_a_real_browser() {
    let mut live = open().await;
    v2_lib::browser::page_log::watch(&mut live.cdp).await.unwrap();
    // A port nothing listens on any more.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    page::eval_value(
        &mut live.cdp,
        &format!(
            "fetch('http://127.0.0.1:{port}/hr/pmsv10/menu?token=secret').catch(() => {{}}); console.error('menu data missing'); \
             const s = document.createElement('script'); s.text = 'var x = ;'; document.body.appendChild(s); 1"
        ),
    )
    .await
    .unwrap();

    // Events are read while a call waits; a few cheap calls read them in.
    let mut lines = vec![];
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = page::eval_value(&mut live.cdp, "1").await;
        lines = live.cdp.page_log();
        if lines.iter().any(|l| l.starts_with("request failed"))
            && lines.iter().any(|l| l.starts_with("console error"))
            && lines.iter().any(|l| l.starts_with("uncaught error"))
        {
            break;
        }
    }
    assert!(lines.iter().any(|l| l.starts_with("console error: menu data missing (at ")), "{lines:?}");
    // A script the page inserted itself: Edge names the line it broke on.
    let thrown = lines.iter().find(|l| l.starts_with("uncaught error: SyntaxError")).unwrap_or_else(|| panic!("{lines:?}"));
    assert!(thrown.contains(" (at ") && (thrown.contains("line 1") || thrown.contains(":1:")), "{thrown}");
    let failed = lines.iter().find(|l| l.starts_with("request failed (")).unwrap_or_else(|| panic!("{lines:?}"));
    assert!(failed.ends_with(&format!("GET http://127.0.0.1:{port}/hr/pmsv10/menu")), "{failed}");
    assert!(!lines.iter().any(|l| l.contains("secret")), "{lines:?}");
}

/// The two API checks against a real page and a real server. The page
/// POSTs with `fetch` when a button is clicked; the server answers JSON.
/// `expect_response` has to see that request (and only a request made
/// during its own step), pass on the right status and fields, and fail on
/// a wrong status or a wrong field. `api_request` has to make the page
/// send a GET whose query arrives encoded, and judge the answer the same
/// way. What is checked is what the SERVER saw and answered, never what
/// Rust believes it sent.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn api_checks_read_the_requests_a_real_page_makes() {
    const PAGE: &str = r#"<!doctype html><html><body>
<button id="save" onclick="fetch('/PerformanceCycle/Save?token=hunter2', { method: 'POST' }).then(r => r.text()).then(t => { document.getElementById('out').textContent = t; })">Save</button>
<button id="expired" onclick="fetch('/PerformanceCycle/Expired?token=hunter2', { method: 'POST' })">Save as an ended session</button>
<p id="out"></p>
</body></html>"#;
    let listener = TcpListener::bind("127.0.0.1:0").expect("no free port");
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = seen.clone();
    // The thread ends with the test process; the listener has no other owner.
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // A connection the browser opens ahead of time and never uses
            // must not stall every request behind it.
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 4096];
            let Ok(n) = stream.read(&mut buf) else { continue };
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let line = head.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            // An ended session: the save is sent on to the sign-in page.
            if line.starts_with("POST /PerformanceCycle/Expired") {
                let _ = write!(
                    stream,
                    "HTTP/1.1 302 Found
Location: /Account/Login?ReturnUrl=%2FPerformanceCycle%3Ftoken%3Dhunter2
Content-Length: 0
Connection: close

"
                );
                continue;
            }
            let (kind, body) = if line.starts_with("POST /PerformanceCycle/Save") {
                ("application/json", r#"{"success":true,"id":7}"#.to_string())
            } else if line.starts_with("GET /api/cycles/42") {
                let include = line.split("include=").nth(1).and_then(|r| r.split([' ', '&']).next()).unwrap_or("none");
                ("application/json", format!(r#"{{"name":"Q4 Cycle","include":"{include}","extra":1}}"#))
            } else if line.starts_with("GET /Account/Login") {
                ("text/html; charset=utf-8", "<html><title>Sign in</title></html>".to_string())
            } else if line.starts_with("GET / ") {
                ("text/html; charset=utf-8", PAGE.to_string())
            } else {
                let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });

    let mut live = open().await;
    v2_lib::browser::page_log::watch(&mut live.cdp).await.expect("the network domain did not switch on");
    must(run(&mut live, json!({ "kind": "navigate", "url": format!("http://127.0.0.1:{port}/") })).await);
    let root = tempfile::tempdir().unwrap();
    let mut account: Option<String> = None;
    let step = |number: i32, actions: Vec<serde_json::Value>| StepScript {
        step_number: number,
        actions: actions.into_iter().map(action_of).collect(),
        unchecked: None,
    };
    let click = json!({ "kind": "click", "selector": { "css": "#save" } });

    // The click and its check share a step: right status, right fields.
    let right = step(1, vec![
        click.clone(),
        json!({ "kind": "expect_response", "method": "POST", "url_contains": "/performancecycle/save", "json": { "success": true } }),
    ]);
    let out = run_step(&mut live.cdp, root.path(), "acme", "PMS", &right, &timing(), &mut account).await.unwrap();
    must(out[0].clone());
    must(out[1].clone());
    assert!(!out[1].detail.contains("hunter2"), "the query string reached the outcome: {}", out[1].detail);
    let echoed = page::eval_value(&mut live.cdp, "document.getElementById('out').textContent").await.unwrap();
    assert_eq!(echoed.as_str(), Some(r#"{"success":true,"id":7}"#), "the page never made the request");

    // A wrong status and a wrong field both fail, naming the method and path.
    let wrong = step(2, vec![
        click.clone(),
        json!({ "kind": "expect_response", "url_contains": "/PerformanceCycle/Save", "status": 500 }),
        json!({ "kind": "expect_response", "url_contains": "/PerformanceCycle/Save", "json": { "success": false } }),
    ]);
    let out = run_step(&mut live.cdp, root.path(), "acme", "PMS", &wrong, &timing(), &mut account).await.unwrap();
    must(out[0].clone());
    refused(out[1].clone(), "answered 200, expected 500");
    refused(out[2].clone(), "expected success = false, got true");
    for o in &out[1..] {
        assert!(!o.detail.contains("hunter2") && !o.detail.contains("127.0.0.1"), "{}", o.detail);
    }

    // The earlier steps' requests are not this step's: nothing was sent here.
    let none = step(3, vec![json!({ "kind": "expect_response", "url_contains": "/PerformanceCycle/Save", "timeout_ms": 600 })]);
    let out = run_step(&mut live.cdp, root.path(), "acme", "PMS", &none, &timing(), &mut account).await.unwrap();
    refused(out[0].clone(), "no request matching");

    // api_request: the page sends the GET, the query arrives encoded, and
    // the answer is judged by status and by the fields listed.
    let ask = step(4, vec![
        json!({ "kind": "api_request", "path": "/api/cycles/42", "query": { "include": "rules" },
                "expect": { "status": 200, "json": { "name": "Q4 Cycle", "include": "rules" } } }),
        json!({ "kind": "api_request", "path": "/api/cycles/42", "expect": { "json": { "name": "Q1 Cycle" } } }),
        json!({ "kind": "api_request", "path": "/api/cycles/42", "expect": { "status": 404 } }),
        json!({ "kind": "api_request", "path": "/api/missing" }),
    ]);
    let out = run_step(&mut live.cdp, root.path(), "acme", "PMS", &ask, &timing(), &mut account).await.unwrap();
    must(out[0].clone());
    refused(out[1].clone(), "GET /api/cycles/42");
    refused(out[2].clone(), "answered 200, expected 404");
    refused(out[3].clone(), "answered 404, expected 200");
    assert!(
        seen.lock().unwrap().iter().any(|l| l.starts_with("GET /api/cycles/42?include=rules ")),
        "the server never saw the encoded query: {:?}",
        seen.lock().unwrap()
    );

    // Chrome's own redirect events: the POST answered 302 is still the POST
    // the page made - it passes as a 302, and fails as a 200 naming where it
    // was sent (the path only), promptly rather than at the timeout.
    let expired = step(5, vec![
        json!({ "kind": "click", "selector": { "css": "#expired" } }),
        json!({ "kind": "expect_response", "method": "POST", "url_contains": "/PerformanceCycle/Expired", "status": 302 }),
        json!({ "kind": "expect_response", "method": "POST", "url_contains": "/PerformanceCycle/Expired", "timeout_ms": 8000 }),
    ]);
    let began = std::time::Instant::now();
    let out = run_step(&mut live.cdp, root.path(), "acme", "PMS", &expired, &timing(), &mut account).await.unwrap();
    must(out[0].clone());
    must(out[1].clone());
    assert_eq!(out[2].detail, "POST /PerformanceCycle/Expired was redirected to /Account/Login");
    assert!(!out[2].ok);
    assert!(began.elapsed() < Duration::from_secs(8), "the redirect waited out the timeout");
    assert!(
        seen.lock().unwrap().iter().any(|l| l.starts_with("GET /Account/Login")),
        "the browser never followed the redirect: {:?}",
        seen.lock().unwrap()
    );
}

/// Spike: can Chrome's role lookup reach inside a same-origin frame when it
/// is handed the frame's DOCUMENT, and does role `Iframe` find the frame
/// element itself? The answers decide how a role step searches inside a
/// frame, and what step the snapshot prints for an iframe.
///
/// Observed on Edge 153 (2026-10-03): (a) yes - handed the frame document,
/// queryAXTree returns the frame's own buttons, so a role step searches
/// inside a frame with no fallback; (b) yes - role "Iframe" (capitalised;
/// "iframe" finds nothing) returns each visible iframe by its title, and
/// not the hidden twin. Asserted, so a browser that changes either fails
/// here first.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_spike_role_lookup_through_a_frame_document() {
    let mut live = open_iframes().await;
    let top = page::document(&mut live.cdp).await.expect("no document");
    let frames = page::call_elements(&mut live.cdp, &top, "function() { return [document.querySelector('#es-frame')]; }", &[])
        .await
        .expect("no frame element");
    let docs = page::call_elements(&mut live.cdp, &frames[0], "function() { return [this.contentDocument]; }", &[])
        .await
        .expect("no frame document");
    let names = |r: &serde_json::Value| -> Vec<String> {
        r["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|n| !n["ignored"].as_bool().unwrap_or(false))
            .map(|n| n["name"]["value"].as_str().unwrap_or("").to_string())
            .collect()
    };
    let inside = live
        .cdp
        .call("Accessibility.queryAXTree", json!({ "objectId": docs[0], "role": "button" }))
        .await
        .expect("queryAXTree on the frame document failed");
    assert_eq!(names(&inside), vec!["Select".to_string()], "(a) the role lookup did not reach inside the frame");
    let r = live
        .cdp
        .call("Accessibility.queryAXTree", json!({ "objectId": top, "role": "Iframe" }))
        .await
        .expect("queryAXTree for the iframe role failed");
    assert_eq!(
        names(&r),
        vec![
            "Edge frame".to_string(),
            "Employee Search".to_string(),
            "Locked frame".to_string(),
            String::new(),
            "Padded frame".to_string()
        ],
        "(b) role Iframe did not list the visible frames by title"
    );
}

/// The visible employee-search frame, as a chain's first step.
fn es(inner: serde_json::Value) -> serde_json::Value {
    json!([{ "css": "iframe[title='Employee Search']" }, inner])
}

/// Reading inside a frame needs no click, so it proves the resolver alone.
/// The page also holds a hidden twin of the frame: it must not count.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_a_chain_reads_inside_the_frame() {
    let mut live = open_iframes().await;
    must(run(&mut live, json!({ "kind": "expect_text", "selector": es(json!({ "css": "#pick" })), "equals": "Select" })).await);
    must(run(&mut live, json!({ "kind": "expect_count",
        "selector": es(json!({ "role": "button", "name": "Select", "exact": true })), "equals": 1 })).await);
}

/// When the iframe is the chain's target it stays the iframe element.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_the_last_step_keeps_the_iframe_itself() {
    let mut live = open_iframes().await;
    must(run(&mut live, json!({ "kind": "expect_visible", "selector": { "css": "#es-frame" } })).await);
    must(run(&mut live, json!({ "kind": "expect_count", "selector": [{ "css": "#es-wrap" }, { "css": "iframe" }], "equals": 1 })).await);
}

/// A sandboxed frame cannot be entered. Every check through it says so -
/// expect_hidden and a count of 0 must not pass just because nothing
/// was found.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_an_unreachable_frame_is_named_not_reported_missing() {
    let mut live = open_iframes().await;
    let locked = json!([{ "css": "#locked" }, { "role": "button", "name": "Locked" }]);
    let why = "holds a page from another site";
    refused(run(&mut live, json!({ "kind": "expect_visible", "selector": locked })).await, why);
    refused(run(&mut live, json!({ "kind": "expect_hidden", "selector": locked })).await, why);
    refused(run(&mut live, json!({ "kind": "expect_count", "selector": locked, "equals": 0 })).await, why);
    refused(run(&mut live, json!({ "kind": "click", "selector": locked })).await, why);
    refused(run(&mut live, json!({ "kind": "wait_for", "selector": locked, "timeout_ms": 1500 })).await, why);
}

/// The frame sits below a tall spacer, 180px in, inside a 7px border: the
/// click point has to add all of that or it lands beside the button.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_a_click_lands_on_the_element_inside_a_scrolled_bordered_frame() {
    let mut live = open_iframes().await;
    must(run(&mut live, json!({ "kind": "click", "selector": es(json!({ "css": "#pick" })) })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": es(json!({ "css": "#out" })), "equals": "picked" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_typing_reaches_an_input_inside_the_frame() {
    let mut live = open_iframes().await;
    must(run(&mut live, json!({ "kind": "fill",
        "selector": es(json!({ "role": "textbox", "name": "Search employees" })), "value": "Ethan" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": es(json!({ "css": "#out" })), "equals": "typed:Ethan" })).await);
}

/// An overlay on the PAGE over the frame is in the way of a person's click
/// as much as one inside the frame: wait, then say what covers it.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_a_parent_overlay_over_the_frame_is_reported_as_covering() {
    let mut live = open_iframes().await;
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#show-cover" } })).await);
    let out = run(&mut live, json!({ "kind": "click", "selector": es(json!({ "css": "#pick" })) })).await;
    refused(out.clone(), "covered by");
    refused(out, "div#cover");
    // Empty, so it has no height: only a hidden-inclusive look can read it.
    must(run(&mut live, json!({ "kind": "expect_text",
        "selector": es(json!({ "css": "#out", "visible": false })), "equals": "" })).await);
}

/// A text step inside a frame: its in-page filter checks `instanceof
/// HTMLElement`, which only holds when the search runs in the FRAME's own
/// context - so this fails if the resolver hands on a parent-side handle.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_a_text_step_finds_words_inside_the_frame() {
    let mut live = open_iframes().await;
    must(run(&mut live, json!({ "kind": "expect_count", "selector": es(json!({ "text": "Select", "exact": true })), "equals": 1 })).await);
}

/// The chain the snapshot prints for an element inside a frame is the one a
/// script pastes: it must reach that element and nothing else.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_the_snapshot_prints_paste_ready_chains() {
    let mut live = open_iframes().await;
    let text = snapshot(&mut live.cdp, DEFAULT_LIMIT).await.expect("no snapshot");
    let chain_of = |needle: &str| -> serde_json::Value {
        let line = text.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("no line with {needle} in:\n{text}"));
        serde_json::from_str(line.rsplit(" -> ").next().unwrap()).unwrap()
    };
    let select = chain_of("button \"Select\"");
    assert!(select.is_array(), "not a chain: {select}");
    must(run(&mut live, json!({ "kind": "click", "selector": select })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": es(json!({ "css": "#out" })), "equals": "picked" })).await);
    // A frame with neither id nor title still gets a chain that resolves.
    must(run(&mut live, json!({ "kind": "expect_count", "selector": chain_of("button \"Bare inner\""), "equals": 1 })).await);
    // The sandboxed frame says it cannot be read rather than offering a
    // chain that would fail.
    assert!(text.contains("frame contents could not be read"), "{text}");
    assert!(!text.contains("button \"Locked\""), "{text}");
}

/// Padding sits between an iframe's border and its viewport. A click point
/// that adds only the border lands on the padding - still the iframe, so no
/// cover check notices - and the button never hears it.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_a_click_inside_a_padded_frame_lands_on_the_element() {
    let mut live = open_iframes().await;
    let pad = |inner: serde_json::Value| json!([{ "css": "#pad-frame" }, inner]);
    must(run(&mut live, json!({ "kind": "click", "selector": pad(json!({ "css": "#pad-btn" })) })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": pad(json!({ "css": "#pout" })), "equals": "hit" })).await);
}

/// Spec 9: `#edge-inner` sits half past `#edge-frame`'s right edge, so only
/// the left half of the button inside shows. The middle of the whole button
/// is on that edge, where nothing can be clicked; the point is re-centred
/// inside the half that is left, and the click lands.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_a_button_half_hidden_by_a_frames_edge_is_clicked_in_its_visible_part() {
    let mut live = open_iframes().await;
    let edge = |inner: serde_json::Value| json!([{ "css": "#edge-frame" }, { "css": "#edge-inner" }, inner]);
    must(run(&mut live, json!({ "kind": "click", "selector": edge(json!({ "css": "#edge-btn" })) })).await);
    must(run(&mut live, json!({ "kind": "expect_text",
        "selector": edge(json!({ "css": "#eout", "visible": false })), "equals": "hit" })).await);
}

/// Spec 9: `check_text` means anywhere on the page, so it reads the words of
/// same-origin frames at any depth - but not a hidden frame's, which no one
/// can see, and not a frame from another site, which it cannot read.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_check_text_reads_same_origin_frames_at_any_depth() {
    let mut live = open_iframes().await;
    must(run(&mut live, json!({ "kind": "check_text", "value": "Deep frame words" })).await);
    must(run(&mut live, json!({ "kind": "check_text", "value": "bare inner" })).await);
    refused(run(&mut live, json!({ "kind": "check_text", "value": "Hidden frame words" })).await, "page does NOT contain");
    refused(run(&mut live, json!({ "kind": "check_text", "value": "Locked" })).await, "page does NOT contain");
}

/// The step the snapshot prints for an iframe with an odd id or title
/// (a space, a quote, a leading digit, a colon, a backslash) is a selector
/// the browser parses, and it matches that frame and no other.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn frame_an_odd_id_or_title_prints_a_step_that_matches_its_frame() {
    let mut live = open().await;
    let values = crate::browser_locator::ODD_FRAME_VALUES;
    let add = format!(
        "{}.forEach((v, k) => {{ const f = document.createElement('iframe'); f.setAttribute('id', v); \
         f.setAttribute('title', v); f.dataset.k = String(k); document.body.appendChild(f); }}); true",
        json!(values)
    );
    let added = live.cdp.call("Runtime.evaluate", json!({ "expression": add, "returnByValue": true })).await.expect("no answer");
    assert_eq!(added["result"]["value"], true, "{added}");
    for (k, value) in values.iter().enumerate() {
        for attr in ["id", "title"] {
            let step = v2_lib::browser::snapshot::frame_step("", &json!([attr, value]), 0);
            let css = step["css"].as_str().unwrap().to_string();
            let check = format!(
                "(() => {{ try {{ return Array.from(document.querySelectorAll({})).map(e => e.dataset.k).join(','); }} \
                 catch (e) {{ return 'invalid: ' + e.message; }} }})()",
                json!(css)
            );
            let r = live.cdp.call("Runtime.evaluate", json!({ "expression": check, "returnByValue": true })).await.expect("no answer");
            assert_eq!(r["result"]["value"], json!(k.to_string()), "{attr} {value:?} printed {css}");
        }
    }
}

// ---------------------------------------------------------------------
// No-save scripts (run safety §1): the save is stopped inside the browser.

/// A tiny server for `autorun-save.html`: it serves the page, answers the
/// two POSTs, and remembers every request line it was sent - so a test can
/// say what reached the SERVER, never what Rust believes it stopped.
struct SaveServer {
    port: u16,
    seen: Arc<std::sync::Mutex<Vec<String>>>,
}

impl SaveServer {
    fn start() -> SaveServer {
        let page = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/autorun-save.html"))
            .expect("the save fixture is missing");
        let listener = TcpListener::bind("127.0.0.1:0").expect("no free port");
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = seen.clone();
        // The thread ends with the test process; the listener has no other owner.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 4096];
                let Ok(n) = stream.read(&mut buf) else { continue };
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let line = head.lines().next().unwrap_or("").to_string();
                log.lock().unwrap().push(line.clone());
                let (kind, body) = if line.starts_with("GET /save-page") {
                    ("text/html; charset=utf-8", page.clone())
                } else if line.starts_with("POST /api/Save") {
                    ("text/plain", "saved".to_string())
                } else if line.starts_with("POST /api/Search") {
                    ("text/plain", "searched".to_string())
                } else {
                    let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        SaveServer { port, seen }
    }

    fn page(&self, query: &str) -> String {
        format!("http://127.0.0.1:{}/save-page{query}", self.port)
    }

    /// Did a request starting with these words reach the server? Asked
    /// after a moment, so a request still on its way is counted.
    fn got(&self, start: &str) -> bool {
        std::thread::sleep(Duration::from_millis(400));
        self.seen.lock().unwrap().iter().any(|l| l.starts_with(start))
    }
}

const SAVE_STOPPED: &str =
    "this script must not save, but the page tried to send POST /api/Save - it was stopped before it reached the server";

fn save_case(no_save: bool, url: &str, steps_after: serde_json::Value) -> CaseScript {
    let mut steps = vec![json!({ "step_number": 1, "actions": [{ "kind": "navigate", "url": url }] })];
    steps.extend(steps_after.as_array().cloned().unwrap_or_default());
    serde_json::from_value(json!({ "case_id": 901, "title": "draft", "no_save": no_save, "steps": steps }))
        .expect("the test wrote an invalid case script")
}

fn click_then_see(button: &str, text: &str) -> serde_json::Value {
    json!([{ "step_number": 2, "actions": [
        { "kind": "click", "selector": { "css": button } },
        { "kind": "expect_text", "selector": { "css": "#out" }, "equals": text }
    ] }])
}

async fn run_save_case(live: &mut Live, root: &Path, script: &CaseScript) -> v2_lib::autorun::CaseRecord {
    v2_lib::autorun::replay::run_case(
        &mut live.cdp,
        root,
        "acme",
        "PMS",
        script,
        &timing(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .await
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_no_save_case_has_its_save_stopped_before_it_reaches_the_server() {
    let server = SaveServer::start();
    let root = tempfile::tempdir().unwrap();
    let mut live = open().await;
    let script = save_case(true, &server.page(""), click_then_see("#save", "saved"));
    let rec = run_save_case(&mut live, root.path(), &script).await;
    assert!(!server.got("POST /api/Save"), "the save reached the server: {:?}", server.seen.lock().unwrap());
    assert!(server.got("GET /save-page"), "the page itself was held back too");
    assert_eq!(rec.proposed, "Failed", "{rec:?}");
    assert_eq!(rec.reason, format!("step 2: {SAVE_STOPPED}"));
    assert!(!rec.reason.contains("hunter2") && !rec.reason.contains("127.0.0.1"), "{}", rec.reason);
    let step2 = &rec.steps[1].outcomes;
    assert!(step2.iter().any(|o| o.detail == SAVE_STOPPED), "{step2:?}");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_no_save_case_lets_a_search_through() {
    let server = SaveServer::start();
    let root = tempfile::tempdir().unwrap();
    let mut live = open().await;
    let script = save_case(true, &server.page(""), click_then_see("#search", "searched"));
    let rec = run_save_case(&mut live, root.path(), &script).await;
    assert_eq!(rec.proposed, "Passed", "{rec:?}");
    assert!(server.got("POST /api/Search"));
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_project_word_blocks_its_own_path() {
    let server = SaveServer::start();
    let root = tempfile::tempdir().unwrap();
    v2_lib::autorun::nav::set_save_words(root.path(), "acme", "PMS", &["search".to_string()]).unwrap();
    let mut live = open().await;
    let script = save_case(true, &server.page(""), click_then_see("#search", "searched"));
    let rec = run_save_case(&mut live, root.path(), &script).await;
    assert!(!server.got("POST /api/Search"), "the search reached the server");
    assert_eq!(rec.proposed, "Failed", "{rec:?}");
    assert!(rec.reason.contains("tried to send POST /api/Search - it was stopped"), "{}", rec.reason);
}

/// Review Focus 2: a page that saves the moment it opens, before any
/// action of the script touches it.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_save_the_page_fires_as_it_loads_is_stopped_and_fails_the_case() {
    let server = SaveServer::start();
    let root = tempfile::tempdir().unwrap();
    let mut live = open().await;
    let script = save_case(true, &server.page("?autosave=1"), json!([]));
    let rec = run_save_case(&mut live, root.path(), &script).await;
    assert!(!server.got("POST /api/Save"), "the page's own save reached the server");
    assert_eq!(rec.proposed, "Failed", "{rec:?}");
    assert_eq!(rec.reason, format!("step 1: {SAVE_STOPPED}"));
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_script_without_the_flag_is_not_intercepted() {
    let server = SaveServer::start();
    let root = tempfile::tempdir().unwrap();
    let mut live = open().await;
    let script = save_case(false, &server.page(""), click_then_see("#save", "saved"));
    let rec = run_save_case(&mut live, root.path(), &script).await;
    assert_eq!(rec.proposed, "Passed", "{rec:?}");
    assert!(server.got("POST /api/Save"));
    assert!(!live.cdp.is_guarding_saves());
}

/// Review Focus 1: between calls - nothing asking the browser anything -
/// a guarded browser's requests are still answered, not left paused until
/// the next call.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_guarded_browser_answers_requests_made_between_calls() {
    let server = SaveServer::start();
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "navigate", "url": server.page("") })).await);
    live.cdp.guard_saves(&[]).await.expect("the guard did not start");
    page::eval_value(&mut live.cdp, "setTimeout(() => send('/api/Search'), 100); 1").await.unwrap();
    // No call at all now: only the idle pump reads the paused request.
    live.cdp.idle(Duration::from_millis(1500)).await;
    assert!(server.got("POST /api/Search"), "the request was left paused between calls");
    let out = page::eval_value(&mut live.cdp, "document.getElementById('out').textContent").await.unwrap();
    assert_eq!(out.as_str(), Some("searched"));
}

/// Review focus 3 against real Edge: only the types that can save are
/// paused now, and a form post (a `Document`) and a beacon (a `Ping`) from
/// the guarded page itself still never reach the server.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_beacon_and_a_form_post_from_a_guarded_page_never_reach_the_server() {
    let server = SaveServer::start();
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "navigate", "url": server.page("") })).await);
    live.cdp.guard_saves(&[]).await.expect("the guard did not start");
    page::eval_value(&mut live.cdp, "navigator.sendBeacon('/api/SaveBeacon', 'draft'); postForm(); 1").await.unwrap();
    live.cdp.idle(Duration::from_millis(1500)).await;
    assert!(!server.got("POST /api/Save"), "a save reached the server: {:?}", server.seen.lock().unwrap());
    let stopped = live.cdp.take_saves_stopped();
    for path in ["/api/SaveBeacon", "/api/Save"] {
        assert!(stopped.iter().any(|(m, p)| m == "POST" && p == path), "{path} was not stopped: {stopped:?}");
    }
}

/// A tab the page opens (a `target=_blank` link) is attached and guarded
/// before it runs: the form it posts the moment it opens is stopped, the
/// case hears of it, and the script still acts in the first tab.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_tab_the_page_opens_is_attached_and_its_save_is_stopped() {
    let server = SaveServer::start();
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "navigate", "url": server.page("") })).await);
    live.cdp.guard_saves(&[]).await.expect("the guard did not start");
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#popup" } })).await);
    for _ in 0..50 {
        if live.cdp.tabs().len() > 1 {
            break;
        }
        live.cdp.idle(Duration::from_millis(100)).await;
    }
    assert_eq!(live.cdp.tabs().len(), 2, "the new tab was never attached");
    live.cdp.idle(Duration::from_millis(1500)).await;
    assert!(server.got("GET /save-page?formsave=1"), "the new tab never ran: {:?}", server.seen.lock().unwrap());
    assert!(!server.got("POST /api/Save"), "the new tab's form post reached the server");
    assert_eq!(live.cdp.take_save_blocked().as_deref(), Some(SAVE_STOPPED));
    let search = page::eval_value(&mut live.cdp, "location.search").await.unwrap();
    assert_eq!(search.as_str(), Some(""), "the script no longer acts in the first tab");
}

/// The tab actions end to end: follow a `target=_blank` link, claim the new
/// tab by its address, read it, close it (`main` is current again), open a
/// second tab at the page and go back to `main`. The guard is on all the
/// way: the script clicks `#formsave` in the new tab, which posts a form,
/// and the server must never receive it.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_script_follows_a_link_into_a_tab_reads_it_closes_it_and_opens_another() {
    let server = SaveServer::start();
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "navigate", "url": server.page("") })).await);
    live.cdp.guard_saves(&[]).await.expect("the guard did not start");
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#report" } })).await);
    must(run(&mut live, json!({ "kind": "expect_tab", "name": "report", "url_contains": "report=1" })).await);
    must(run(&mut live, json!({ "kind": "switch_tab", "name": "report" })).await);
    must(run(&mut live, json!({ "kind": "check_url", "contains": "report=1" })).await);
    must(run(&mut live, json!({ "kind": "check_text", "value": "Save by form" })).await);
    // The form post from the new tab is stopped before it reaches the server.
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#formsave" } })).await);
    live.cdp.idle(Duration::from_millis(800)).await;
    must(run(&mut live, json!({ "kind": "close_tab", "name": "report" })).await);
    // `main` is current again: its own address has no query.
    must(run(&mut live, json!({ "kind": "check_text", "value": "Open the report" })).await);
    let search = page::eval_value(&mut live.cdp, "location.search").await.unwrap();
    assert_eq!(search.as_str(), Some(""), "main is not the current tab after the close");
    must(run(&mut live, json!({ "kind": "open_tab", "name": "second", "url": server.page("?second=1") })).await);
    must(run(&mut live, json!({ "kind": "check_url", "contains": "second=1" })).await);
    must(run(&mut live, json!({ "kind": "switch_tab", "name": "main" })).await);
    let url = page::eval_value(&mut live.cdp, "location.search").await.unwrap();
    assert_eq!(url.as_str(), Some(""), "main did not become current again");
    assert!(server.got("GET /save-page?report=1"), "the new tab never ran: {:?}", server.seen.lock().unwrap());
    assert!(server.got("GET /save-page?second=1"), "the second tab never ran");
    assert!(!server.got("POST /api/Save"), "the new tab's form post reached the server: {:?}", server.seen.lock().unwrap());
    assert_eq!(live.cdp.take_save_blocked().as_deref(), Some(SAVE_STOPPED));
}

const DRAFT_STOPPED: &str =
    "this script must not save, but the page tried to send POST /api/SaveDraft - it was stopped before it reached the server";

/// The App's recipe, with the draft server's origin allowed, so a script
/// may open the draft and then sign in.
fn recipe_with_draft(app: &App, server: &SaveServer) -> SignInRecipe {
    let mut r = recipe_for(app);
    r.allowed_origins = vec![format!("http://127.0.0.1:{}", server.port)];
    r.validate().expect("the test wrote an invalid recipe");
    r
}

/// Review fix 3: the sign-in's exemption starts only once it has arrived
/// on its start page. A save the draft page sends as it is left (a beacon
/// on pagehide) is still stopped - and the sign-in's own login POST, which
/// a project word would otherwise catch, still goes through.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_save_sent_while_a_sign_in_leaves_the_draft_is_still_stopped() {
    let app = App::start();
    let server = SaveServer::start();
    let root = tempfile::tempdir().unwrap();
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "navigate", "url": server.page("?unloadsave=1") })).await);
    live.cdp.guard_saves(&["login".to_string()]).await.expect("the guard did not start");
    let out = sign_in(&mut live.cdp, root.path(), &recipe_with_draft(&app, &server), &kim(), &timing()).await;
    assert!(out.ok, "the sign-in itself was stopped: {} / {:?}", out.detail, out.steps);
    assert_eq!(app.logins.load(Ordering::SeqCst), 1, "the login POST never reached the server");
    assert!(!server.got("POST /api/SaveDraft"), "the draft's save reached the server: {:?}", server.seen.lock().unwrap());
    assert_eq!(live.cdp.take_save_blocked().as_deref(), Some(DRAFT_STOPPED));
}

/// The same through a script: a `sign_in` in the middle of a no-save case,
/// on the draft, fails the step with the sentence.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_mid_script_sign_in_that_leaves_a_saving_draft_fails_the_case() {
    let app = App::start();
    let server = SaveServer::start();
    let root = tempfile::tempdir().unwrap();
    save_recipe(root.path(), "acme", "PMS", &recipe_with_draft(&app, &server)).unwrap();
    save_accounts(root.path(), &[kim()]).unwrap();
    let mut live = open().await;
    let script = save_case(
        true,
        &server.page("?unloadsave=1"),
        json!([{ "step_number": 2, "actions": [{ "kind": "sign_in", "account": "kim" }] }]),
    );
    let rec = run_save_case(&mut live, root.path(), &script).await;
    assert!(!server.got("POST /api/SaveDraft"), "the draft's save reached the server");
    assert_eq!(rec.proposed, "Failed", "{rec:?}");
    assert_eq!(rec.reason, format!("step 2: {DRAFT_STOPPED}"));
}

/// A one-page site for the keyboard and reload checks below: `/` counts its
/// own loads in the tab's session storage (which a reload keeps), and a
/// click - or Enter on the focused button - writes into `#out`.
/// A page that throws when #boom is pressed and whose #save posts to a path
/// the server answers 500; `/api/Own` answers 500 too, for the run's own
/// `api_request`.
fn errors_site() -> u16 {
    const PAGE: &str = r#"<!doctype html><html><body>
<button id="boom">Boom</button>
<button id="save">Save</button>
<p id="out">nothing yet</p>
<script>
document.getElementById('boom').addEventListener('click', () => {
  document.getElementById('out').textContent = 'boomed';
  throw new Error('boom from the page');
});
document.getElementById('save').addEventListener('click', async () => {
  await fetch('/api/Broken?token=hunter2', { method: 'POST' });
  document.getElementById('out').textContent = 'tried';
});
</script>
</body></html>"#;
    let listener = TcpListener::bind("127.0.0.1:0").expect("no free port");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 4096];
            let Ok(n) = stream.read(&mut buf) else { continue };
            let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
            if line.starts_with("GET / ") {
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
                    PAGE.len()
                );
            } else if line.contains(" /api/") {
                let _ = write!(stream, "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 2\r\nConnection: close\r\n\r\nno");
            } else {
                let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        }
    });
    port
}

/// A real page's thrown error fails a `fail` step; a real 5xx is counted
/// by a `flag` step, and the run's own `api_request` 500 is not.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn page_errors_fail_or_flag_a_real_page() {
    use v2_lib::autorun::lease::Held;
    use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
    use v2_lib::autorun::PageErrors;
    let port = errors_site();
    let mut live = open().await;
    v2_lib::browser::page_log::watch(&mut live.cdp).await.unwrap();
    must(run(&mut live, json!({ "kind": "navigate", "url": format!("http://127.0.0.1:{port}/") })).await);
    v2_lib::autorun::page_errors::drop_all(&mut live.cdp);
    let dir = tempfile::tempdir().unwrap();
    let mut account = None;
    let mut held = Held::supervised();

    let step: StepScript = serde_json::from_value(json!({ "step_number": 1, "actions": [
        { "kind": "click", "selector": { "css": "#boom" } },
        { "kind": "expect_text", "selector": { "css": "#out" }, "equals": "boomed" }
    ] }))
    .unwrap();
    let mut r = InRun { page_errors: Some(PageErrors::Fail), ..Default::default() };
    let out = run_step_in_run(&mut live.cdp, dir.path(), "Acme", "Web", &step, &timing(), &mut account, &mut held, None, AreaRoute::Unknown(NEEDS_SCRIPT_AREA), &mut r)
        .await
        .unwrap();
    assert!(out[0].ok, "{out:?}");
    assert_eq!(out[1].detail, "the page had an error: Error: boom from the page");

    let step: StepScript = serde_json::from_value(json!({ "step_number": 2, "actions": [
        { "kind": "click", "selector": { "css": "#save" } },
        { "kind": "expect_text", "selector": { "css": "#out" }, "equals": "tried" },
        { "kind": "api_request", "path": "/api/Own", "expect": { "status": 500 } }
    ] }))
    .unwrap();
    let mut r = InRun { page_errors: Some(PageErrors::Flag), ..Default::default() };
    let out = run_step_in_run(&mut live.cdp, dir.path(), "Acme", "Web", &step, &timing(), &mut account, &mut held, None, AreaRoute::Unknown(NEEDS_SCRIPT_AREA), &mut r)
        .await
        .unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(r.page_errors_seen, 1, "{out:?}");
    let last = &out[2].detail;
    assert!(last.ends_with(" (page errors: a request was answered 500: POST /api/Broken)"), "{last}");
    assert!(!last.contains("hunter2") && !last.contains("127.0.0.1"), "{last}");
}

fn keys_site() -> u16 {
    const PAGE: &str = r#"<!doctype html><html><body>
<p id="loads"></p>
<button id="a" onclick="document.getElementById('out').textContent='pressed A'">Alpha</button>
<button id="b">Beta</button>
<p id="out">nothing pressed</p>
<script>
const n = Number(sessionStorage.getItem('n') || 0) + 1;
sessionStorage.setItem('n', String(n));
document.getElementById('loads').textContent = 'load ' + n;
</script>
</body></html>"#;
    let listener = TcpListener::bind("127.0.0.1:0").expect("no free port");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 4096];
            let Ok(n) = stream.read(&mut buf) else { continue };
            let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
            if !line.starts_with("GET / ") {
                let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
                PAGE.len()
            );
        }
    });
    port
}

/// The table checks read a real HTML table (its `thead`, not its `tfoot`)
/// and a real div-based ARIA grid that fills itself half a second after the
/// page loads - the first check on it has to wait.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn table_checks_read_a_real_table_and_a_loading_grid() {
    let mut live = open().await;
    let url = fixture_url().replace("autorun-live.html", "autorun-table.html");
    must(run(&mut live, json!({ "kind": "navigate", "url": url })).await);
    let html = json!({ "css": "#people-table" });
    let grid = json!({ "role": "grid", "name": "Staff" });

    // The grid first, while it is still empty: the check waits for it.
    let out = run(&mut live, json!({ "kind": "expect_row", "table": grid, "cells": { "Name": "eli", "Status": "Active" } })).await;
    must(out.clone());
    assert_eq!(out.detail, "row 2 has Name \"eli\", Status \"Active\"");
    must(run(&mut live, json!({ "kind": "expect_row_count", "table": grid, "equals": 3 })).await);
    must(run(&mut live, json!({ "kind": "expect_sorted", "table": grid, "column": "Joined", "order": "descending", "as": "date" })).await);
    must(run(&mut live, json!({ "kind": "expect_no_row", "table": grid, "cells": { "Name": "Ghost" } })).await);
    let wrong = run_with(
        &mut live,
        json!({ "kind": "expect_sorted", "table": grid, "column": "Name", "order": "descending", "timeout_ms": 300 }),
        &timing(),
    )
    .await;
    assert_eq!(wrong.detail, "Name is not in descending order - row 1 \"Dee Fox\" comes before row 2 \"Eli Gray\"");

    must(run(&mut live, json!({ "kind": "expect_row", "table": html, "cells": { "name": "Ann Lee", "Status": "active" }, "exact": true })).await);
    must(run(&mut live, json!({ "kind": "expect_row_count", "table": html, "equals": 3 })).await);
    must(run(&mut live, json!({ "kind": "expect_row_count", "table": html, "at_least": 2 })).await);
    must(run(&mut live, json!({ "kind": "expect_sorted", "table": html, "column": "Name", "order": "ascending" })).await);
    must(run(&mut live, json!({ "kind": "expect_sorted", "table": html, "column": "Joined", "order": "descending", "as": "date" })).await);
    let salary = run_with(
        &mut live,
        json!({ "kind": "expect_sorted", "table": html, "column": "Salary", "order": "descending", "as": "number", "timeout_ms": 300 }),
        &timing(),
    )
    .await;
    assert!(salary.ok, "1,200 then 950 then -5 is descending, and the tfoot total is not a row: {}", salary.detail);
    let wrong = run_with(
        &mut live,
        json!({ "kind": "expect_row_count", "table": html, "at_most": 2, "timeout_ms": 300 }),
        &timing(),
    )
    .await;
    assert_eq!(wrong.detail, "the table has 3 rows, not at most 2");
    let unknown = run_with(&mut live, json!({ "kind": "expect_row", "table": html, "cells": { "Grade": "A" }, "timeout_ms": 300 }), &timing()).await;
    assert_eq!(unknown.detail, "the table has no column \"Grade\" - its columns are \"Name\", \"Status\", \"Joined\", \"Salary\"");
}

/// A `confirm` the page opens is dismissed by the step's `expect_dialog`,
/// which then checks its words; the page writes what its `confirm` was
/// answered. A dialog nobody expected is accepted, and said on the step.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn expect_dialog_answers_a_real_confirm() {
    let mut live = open().await;
    let url = fixture_url().replace("autorun-live.html", "autorun-dialog.html");
    must(run(&mut live, json!({ "kind": "navigate", "url": url })).await);
    let dir = tempfile::tempdir().unwrap();
    let mut account = None;
    let step: StepScript = serde_json::from_value(json!({ "step_number": 1, "actions": [
        { "kind": "click", "selector": { "css": "#delete" } },
        { "kind": "expect_dialog", "text": "Delete this cycle?", "answer": "dismiss" },
        { "kind": "expect_text", "selector": { "css": "#answer" }, "equals": "kept" }
    ] }))
    .unwrap();
    let out = run_step(&mut live.cdp, dir.path(), "Acme", "Web", &step, &timing(), &mut account).await.unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[1].detail, "a confirm dialog said \"Delete this cycle?\"; pressed Cancel");

    // The wrong words: still answered as asked (OK here), then failed.
    let step: StepScript = serde_json::from_value(json!({ "step_number": 2, "actions": [
        { "kind": "click", "selector": { "css": "#delete" } },
        { "kind": "expect_dialog", "contains": "archive", "answer": "accept" },
        { "kind": "expect_text", "selector": { "css": "#answer" }, "equals": "deleted" }
    ] }))
    .unwrap();
    let out = run_step(&mut live.cdp, dir.path(), "Acme", "Web", &step, &timing(), &mut account).await.unwrap();
    assert_eq!(out[1].detail, "the dialog said \"Delete this cycle?\", which does not contain \"archive\"");
    assert!(out[2].ok, "the page went on: {out:?}");

    // Nobody expected this alert: accepted, said, and the page went on.
    let step: StepScript = serde_json::from_value(json!({ "step_number": 3, "actions": [
        { "kind": "click", "selector": { "css": "#greet" } },
        { "kind": "expect_text", "selector": { "css": "#answer" }, "equals": "greeted" }
    ] }))
    .unwrap();
    let out = run_step(&mut live.cdp, dir.path(), "Acme", "Web", &step, &timing(), &mut account).await.unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    let said = out.iter().any(|o| o.detail.contains("(an alert dialog was accepted: \"Hello\")"));
    assert!(said, "{out:?}");
}

/// `drag` reorders a list sorted by pointer events (the SortableJS kind)
/// and a list using the browser's own drag and drop, and Ctrl+ArrowUp - a
/// combination, held as a keyboard holds it - reorders both from the
/// keyboard. Every order is the one the PAGE wrote after it moved an item.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn drag_and_ctrl_arrow_reorder_real_lists() {
    let mut live = open().await;
    let url = fixture_url().replace("autorun-live.html", "autorun-drag.html");
    must(run(&mut live, json!({ "kind": "navigate", "url": url })).await);
    let order = |list: &str, equals: &str| json!({ "kind": "expect_text", "selector": { "css": format!("#{list}-order") }, "equals": equals });

    // Pointer events: D before A, then D after B.
    let out = run(&mut live, json!({ "kind": "drag", "from": { "css": "#m-d" }, "to": { "css": "#m-a" }, "position": "before" })).await;
    must(out.clone());
    assert_eq!(out.detail, "dragged #m-d before #m-a");
    must(run(&mut live, order("mouse", "D,A,B,C")).await);
    must(run(&mut live, json!({ "kind": "drag", "from": { "css": "#m-d" }, "to": { "css": "#m-b" }, "position": "after" })).await);
    must(run(&mut live, order("mouse", "A,B,D,C")).await);

    // The browser's own drag and drop: C before A, then A after D.
    must(run(&mut live, json!({ "kind": "drag", "from": { "css": "#n-c" }, "to": { "css": "#n-a" }, "position": "before" })).await);
    must(run(&mut live, order("native", "C,A,B,D")).await);
    must(run(&mut live, json!({ "kind": "drag", "from": { "css": "#n-a" }, "to": { "css": "#n-d" }, "position": "after" })).await);
    must(run(&mut live, order("native", "C,B,D,A")).await);

    // The keyboard: the page moves the focused item only when its keydown
    // says Ctrl is down.
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#n-a" } })).await);
    must(run(&mut live, json!({ "kind": "press_key", "key": "Ctrl+ArrowUp", "times": 2 })).await);
    must(run(&mut live, order("native", "C,A,B,D")).await);
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#m-c" } })).await);
    must(run(&mut live, json!({ "kind": "press_key", "key": "ctrl+ArrowUp" })).await);
    must(run(&mut live, order("mouse", "A,B,C,D")).await);
    // An arrow without Ctrl moves nothing.
    must(run(&mut live, json!({ "kind": "press_key", "key": "ArrowUp" })).await);
    must(run(&mut live, order("mouse", "A,B,C,D")).await);

    // Interception was switched off: a later click is an ordinary click.
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#n-b" } })).await);
    must(run(&mut live, json!({ "kind": "expect_focused", "selector": { "css": "#n-b" } })).await);
}

/// Tab and Shift+Tab really move the focus, Enter really presses the focused
/// button, and a reload really loads the page again - each read from what
/// the page itself shows, never from what Rust sent.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn the_keyboard_and_a_reload_reach_a_real_page() {
    let port = keys_site();
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "navigate", "url": format!("http://127.0.0.1:{port}/") })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#loads" }, "equals": "load 1" })).await);

    // A click leaves the focus on what it clicked.
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#a" } })).await);
    must(run(&mut live, json!({ "kind": "expect_focused", "selector": { "css": "#a" } })).await);

    let tab = run(&mut live, json!({ "kind": "press_key", "key": "Tab" })).await;
    must(tab.clone());
    assert!(tab.detail.contains("button#b"), "where the focus went: {}", tab.detail);
    must(run(&mut live, json!({ "kind": "expect_focused", "selector": { "css": "#b" } })).await);
    // And the check fails when it should, saying where the focus is.
    let wrong = run_with(
        &mut live,
        json!({ "kind": "expect_focused", "selector": { "css": "#a" }, "timeout_ms": 300 }),
        &timing(),
    )
    .await;
    assert!(!wrong.ok && wrong.detail.contains("button#b"), "{}", wrong.detail);

    must(run(&mut live, json!({ "kind": "press_key", "key": "Shift+Tab" })).await);
    must(run(&mut live, json!({ "kind": "expect_focused", "selector": { "css": "#a" } })).await);

    // Enter on a focused button presses it.
    must(run(&mut live, json!({ "kind": "press_key", "key": "Enter" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#out" }, "equals": "pressed A" })).await);

    // A reload is a fresh document: the page counts a second load, and what
    // Enter wrote is gone. (An empty element has no size, so a locator never
    // sees one - the page starts with words instead.)
    must(run(&mut live, json!({ "kind": "reload" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#loads" }, "equals": "load 2" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#out" }, "equals": "nothing pressed" })).await);
}

/// A real site sets an HttpOnly session cookie, which the page's own script
/// cannot see; `expire_session` drops it, and the site's next answer says
/// it saw no session. The network domain is never switched on first.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn ending_the_session_drops_the_cookie_a_real_site_set() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("no free port");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 4096];
            let Ok(n) = stream.read(&mut buf) else { continue };
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let line = head.lines().next().unwrap_or("").to_string();
            let has_session = head.lines().any(|l| l.to_ascii_lowercase().starts_with("cookie:") && l.contains("sid="));
            let (cookie, body) = if line.starts_with("GET /whoami") {
                ("", if has_session { "signed in" } else { "no session" }.to_string())
            } else if line.starts_with("GET / ") {
                ("Set-Cookie: sid=s3cret; HttpOnly; Path=/; SameSite=Lax\r\n", "<p>home</p>".to_string())
            } else {
                let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            };
            let page = format!("<!doctype html><html><body><p id=\"who\">{body}</p></body></html>");
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\n{cookie}Content-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
        }
    });

    let mut live = open().await;
    let site = format!("http://127.0.0.1:{port}");
    must(run(&mut live, json!({ "kind": "navigate", "url": format!("{site}/") })).await);
    must(run(&mut live, json!({ "kind": "navigate", "url": format!("{site}/whoami") })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#who" }, "equals": "signed in" })).await);

    let ended = run(&mut live, json!({ "kind": "expire_session" })).await;
    must(ended.clone());
    assert!(ended.detail.contains("dropped 1 cookie(s)"), "{}", ended.detail);
    assert!(!ended.detail.contains("s3cret") && !ended.detail.contains("sid"), "a cookie was named: {}", ended.detail);

    must(run(&mut live, json!({ "kind": "reload" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": { "css": "#who" }, "equals": "no session" })).await);

    // Nothing left to end.
    let again = run(&mut live, json!({ "kind": "expire_session" })).await;
    assert!(!again.ok, "{}", again.detail);
}

// ------------------------------------------------------------ downloads

/// `open()`, downloads on into `dir`, then on to the export page.
async fn open_downloads(dir: &Path) -> Live {
    let mut live = open().await;
    live.cdp.enable_downloads(dir).await.expect("downloads could not be switched on");
    let url = fixture_url().replace("autorun-live.html", "autorun-download.html");
    must(run(&mut live, json!({ "kind": "navigate", "url": url })).await);
    live
}

/// Read the browser until `n` downloads have finished, or 15 s.
async fn until_finished(live: &mut Live, n: usize) -> Vec<v2_lib::browser::downloads::DownloadEntry> {
    use v2_lib::browser::downloads::DownloadState;
    let until = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let all = live.cdp.downloads();
        let done = all.iter().filter(|d| d.state != DownloadState::InProgress).count();
        if done >= n || std::time::Instant::now() >= until {
            return all;
        }
        live.cdp.pump(Duration::from_millis(100)).await;
    }
}

/// Every file in `dir`, sorted, so a test can say exactly what is there.
fn files_in(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> =
        std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    out.sort();
    out
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_browser_keeps_its_downloads_under_their_own_names() {
    use v2_lib::browser::downloads::DownloadState;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("downloads").join("run-1");
    let mut live = open_downloads(&folder).await;
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#csv" } })).await);
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#xlsx" } })).await);
    let all = until_finished(&mut live, 2).await;
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all.iter().all(|d| d.state == DownloadState::Completed), "{all:?}");
    assert_eq!(all[0].name, "report.csv");
    assert_eq!(all[1].name, "Template.xlsx");
    assert_eq!(all[0].path, folder.join("report.csv"));
    assert_eq!(all[1].path, folder.join("Template.xlsx"));
    assert_eq!(std::fs::read_to_string(&all[0].path).unwrap(), "Employee No,Name\r\nE001,Ada\r\n");
    assert_eq!(all[0].bytes, 28);
    let xlsx = std::fs::read(&all[1].path).unwrap();
    assert!(xlsx.starts_with(b"PK"), "the workbook arrived whole");
    assert_eq!(all[1].bytes, xlsx.len() as u64);
    // Nothing is left under a guid.
    assert_eq!(files_in(&folder), ["Template.xlsx", "report.csv"]);
}

/// Review Focus 1, in a real browser.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn two_downloads_of_one_name_are_both_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut live = open_downloads(dir.path()).await;
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#twice" } })).await);
    let all = until_finished(&mut live, 2).await;
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!(files_in(dir.path()), ["same (2).csv", "same.csv"]);
    let mut texts: Vec<String> = all.iter().map(|d| std::fs::read_to_string(&d.path).unwrap()).collect();
    texts.sort();
    assert_eq!(texts, ["first", "second"]);
}

/// Review Focus 2, in a real browser: whatever the browser makes of a
/// hostile name, the file lands inside the folder under a plain name.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_hostile_name_lands_inside_the_folder() {
    use v2_lib::browser::downloads::{sanitise_name, DownloadState};
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("inner");
    let mut live = open_downloads(&folder).await;
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#hostile" } })).await);
    let all = until_finished(&mut live, 1).await;
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].state, DownloadState::Completed);
    assert_eq!(all[0].path.parent(), Some(folder.as_path()));
    let name = all[0].path.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(sanitise_name(&name), name, "the name on disk is already a plain one");
    assert_eq!(std::fs::read_to_string(&all[0].path).unwrap(), "hostile");
    assert_eq!(files_in(dir.path()), ["inner"], "nothing landed beside the folder");
}

/// One step of `expect_download`'s live test, run as a run runs it.
async fn download_step(live: &mut Live, root: &Path, n: i32, actions: Vec<serde_json::Value>) -> Vec<ActionOutcome> {
    let s = StepScript { step_number: n, actions: actions.into_iter().map(action_of).collect(), unchecked: None };
    let mut account: Option<String> = None;
    run_step(&mut live.cdp, root, "org", "proj", &s, &timing(), &mut account).await.expect("run_step failed")
}

/// `expect_download` in a real browser: the export clicked and the
/// workbook's headers and a cell checked in the same step; a wrong name
/// fails; and a step that clicks nothing hears of no download, though
/// earlier steps downloaded two (Review Focus 3).
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn expect_download_checks_the_file_a_step_downloaded() {
    let dir = tempfile::tempdir().unwrap();
    let mut live = open_downloads(&dir.path().join("downloads")).await;

    let out = download_step(&mut live, dir.path(), 1, vec![
        json!({ "kind": "click", "selector": { "css": "#xlsx" } }),
        json!({ "kind": "expect_download", "name": "template*.XLSX",
                "headers": { "exact": ["Employee No", "Name"] },
                "cells": [ { "ref": "B2", "text": "Ada" }, { "ref": "A2", "text": "E0", "match": "contains" } ] }),
    ])
    .await;
    must(out[0].clone());
    assert!(out[1].ok, "{}", out[1].detail);
    assert!(out[1].detail.starts_with("downloaded \"Template.xlsx\" ("), "{}", out[1].detail);
    assert!(out[1].detail.ends_with(", headers match, B2 is \"Ada\", A2 contains \"E0\""), "{}", out[1].detail);

    let out = download_step(&mut live, dir.path(), 2, vec![
        json!({ "kind": "click", "selector": { "css": "#csv" } }),
        json!({ "kind": "expect_download", "name": "Template*.xlsx" }),
    ])
    .await;
    assert!(!out[1].ok);
    assert_eq!(out[1].detail, "got \"report.csv\", expected a file named \"Template*.xlsx\"");

    let out = download_step(&mut live, dir.path(), 3, vec![
        json!({ "kind": "expect_download", "name": "*", "within_ms": 1500 }),
    ])
    .await;
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, "no download started within 1.5 s");
}

/// A PDF a real browser downloaded, read page by page: its text anywhere,
/// its page count and its last page pass; a phrase on the wrong page fails
/// with the spec's sentence.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn expect_download_reads_a_downloaded_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let mut live = open_downloads(&dir.path().join("downloads")).await;

    let out = download_step(&mut live, dir.path(), 1, vec![
        json!({ "kind": "click", "selector": { "css": "#pdf" } }),
        json!({ "kind": "expect_download", "name": "payslip*.pdf",
                "pdf": { "contains": ["Ada Lovelace"], "pages": { "equals": 2 },
                         "on_page": [ { "page": 1, "contains": "Payslip for October" }, { "page": -1, "contains": "Total 12,500.00" } ] } }),
    ])
    .await;
    must(out[0].clone());
    assert!(out[1].ok, "{}", out[1].detail);
    assert!(out[1].detail.starts_with("downloaded \"Payslip.pdf\" ("), "{}", out[1].detail);
    assert!(
        out[1].detail.ends_with(
            ", the PDF contains \"Ada Lovelace\", the PDF has 2 pages, page 1 of the PDF contains \"Payslip for October\", page 2 of the PDF contains \"Total 12,500.00\""
        ),
        "{}",
        out[1].detail
    );

    let out = download_step(&mut live, dir.path(), 2, vec![
        json!({ "kind": "click", "selector": { "css": "#pdf" } }),
        json!({ "kind": "expect_download", "name": "Payslip*.pdf", "pdf": { "on_page": [ { "page": 1, "contains": "Total" } ] } }),
    ])
    .await;
    assert!(!out[1].ok);
    assert_eq!(out[1].detail, "page 1 of the PDF does not contain \"Total\"");
}

/// A replay to a step against a real page: a three-step case replayed to
/// step 3 signs in, takes the recorded trip to its area, runs steps 1 and
/// 2, and leaves the page where step 2 left it - step 3's Save never ran.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_replay_to_step_3_leaves_the_page_where_step_2_left_it() {
    use v2_lib::autorun::nav::{save_nav, NavFile};
    use v2_lib::autorun::preconditions::{NoDb, PreconditionDb};
    use v2_lib::autorun::replay_to::{replay_to_checked, ReplayEnd, ReplayRequest};
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    save_recipe(root.path(), "acme", "Web", &recipe_for(&app)).unwrap();
    save_accounts(root.path(), &[kim()]).unwrap();
    let area = serde_json::from_value(json!({
        "module": "Leave", "area": "Leave",
        "clicks": [ { "role": "link", "name": "Leave", "exact": true } ],
        "arrived": "/leave", "recorded": "2026-10-05T10:00:00Z"
    }))
    .unwrap();
    save_nav(root.path(), "acme", "Web", &NavFile { direct_urls: true, modules: vec![area], save_words: vec![] }).unwrap();
    let mut script = case_script(
        31,
        Some("kim"),
        json!([
            { "step_number": 1, "actions": [ { "kind": "click", "selector": { "role": "button", "name": "New request" } } ] },
            { "step_number": 2, "actions": [ { "kind": "fill", "selector": { "role": "textbox", "name": "Reason" }, "value": "Back on Monday" } ] },
            { "step_number": 3, "actions": [ { "kind": "click", "selector": { "role": "button", "name": "Save" } } ] }
        ]),
    );
    script.area = Some("Leave".into());
    store::save_script(root.path(), &script).unwrap();

    let mut live = open().await;
    let (mut account, mut guarded) = (None, None);
    let mut lease = v2_lib::autorun::lease::Held::supervised();
    let cancel = AtomicBool::new(false);
    let end = replay_to_checked(
        &mut live.cdp,
        &mut v2_lib::autorun::setup::NoBrowsers::<crate::common::ScriptedDriver>::default(),
        root.path(),
        "acme",
        "Web",
        &ReplayRequest { case_id: 31, step: 3, db_read_access: false },
        &mut account,
        &mut lease,
        &mut guarded,
        true,
        &timing(),
        &cancel,
        || -> PreconditionDb<NoDb> { PreconditionDb::ReadingOff },
        |_, _| {},
    )
    .await;
    assert_eq!(end, ReplayEnd::Ready { case_id: 31, step: 3, notice: None }, "{}", end.sentence());
    let state = page::eval_value(
        &mut live.cdp,
        "[location.pathname, document.getElementById('form').hidden, document.querySelector('[aria-label=Reason]').value, document.getElementById('msg').textContent]",
    )
    .await
    .unwrap();
    assert_eq!(state, json!(["/leave", false, "Back on Monday", ""]), "the page is not where step 2 left it");
}

// ------------------------------------- one browser, a context per case

/// The real `OneBrowser`, started the way the command starts it: a
/// background Edge, asked until it answers, a connection to the browser
/// itself per case. Counts launches and closes, and says the port so the
/// test can look at the browser from outside.
struct LiveLauncher {
    launches: Arc<AtomicUsize>,
    closes: Arc<AtomicUsize>,
    port: Arc<std::sync::Mutex<Option<u16>>>,
}

impl v2_lib::autorun::one_browser::Launcher for LiveLauncher {
    type Process = LaunchedBrowser;
    type T = v2_lib::browser::cdp::WsTransport;

    async fn launch(&mut self) -> Result<LaunchedBrowser, String> {
        let extra = background_args();
        let mut browser = launch_with(Browser::Edge, &extra)?;
        self.launches.fetch_add(1, Ordering::SeqCst);
        let mut last = String::new();
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            match Cdp::answers(browser.port).await {
                Ok(()) => {
                    *self.port.lock().unwrap() = Some(browser.port);
                    return Ok(browser);
                }
                Err(e) => last = e,
            }
        }
        let _ = browser.child.kill();
        let _ = browser.child.wait();
        remove_profile_dir(&browser.profile_dir);
        Err(last)
    }

    async fn connect(&mut self, p: &LaunchedBrowser) -> Result<Cdp, String> {
        Cdp::connect_browser(p.port).await
    }

    fn alive(&mut self, p: &mut LaunchedBrowser) -> bool {
        matches!(p.child.try_wait(), Ok(None))
    }

    fn close(&mut self, mut p: LaunchedBrowser) {
        let _ = p.child.kill();
        let _ = p.child.wait();
        remove_profile_dir(&p.profile_dir);
        self.closes.fetch_add(1, Ordering::SeqCst);
    }
}

/// What one case's connection saw, read as the run handed it out and
/// took it back.
#[derive(Debug, Default)]
struct CaseSeen {
    context: Option<String>,
    tabs_at_start: usize,
    tabs_at_end: usize,
    downloads: usize,
}

/// `OneBrowser` with eyes: each case's context, tabs and downloads are
/// read on the way past. While the second case runs, another connection
/// makes a page in a context of its own, which the case must never adopt.
struct WatchedOneBrowser {
    inner: v2_lib::autorun::one_browser::OneBrowser<LiveLauncher>,
    port: Arc<std::sync::Mutex<Option<u16>>>,
    seen: Vec<CaseSeen>,
    stranger: Option<Cdp>,
}

impl Browsers for WatchedOneBrowser {
    type D = Cdp;
    async fn open(&mut self) -> Result<Cdp, String> {
        let mut d = self.inner.open().await?;
        if !self.seen.is_empty() && self.stranger.is_none() {
            let port = self.port.lock().unwrap().expect("the browser was launched");
            let mut stranger = Cdp::connect_browser(port).await?;
            stranger.drive_new_context().await.map_err(|e| e.to_string())?;
            self.stranger = Some(stranger);
        }
        // Long enough for every page the browser attaches (its own first
        // page, and the stranger's) to reach this connection.
        d.pump(Duration::from_millis(500)).await;
        self.seen.push(CaseSeen {
            context: d.browser_context().map(str::to_string),
            tabs_at_start: d.tabs().len(),
            ..CaseSeen::default()
        });
        Ok(d)
    }
    async fn close(&mut self, d: Cdp) {
        if let Some(seen) = self.seen.last_mut() {
            seen.tabs_at_end = d.tabs().len();
            seen.downloads = d.all_downloads().len();
        }
        self.inner.close(d).await;
    }
}

/// Two cases through the real `OneBrowser` and a real Edge: one browser
/// for both, a context per case, disposed after it. Case 1 signs in (the
/// server sets an HttpOnly cookie) and downloads a file; case 2 signs in
/// as nobody and must meet the login form, so case 1's cookie did not
/// reach it. Each case has only its own page, though the browser's first
/// page and another context's page are attached to its connection.
#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn one_browser_gives_each_case_a_context_of_its_own() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    save_recipe(root.path(), "acme", "Web", &recipe_for(&app)).unwrap();
    save_accounts(root.path(), &[kim()]).unwrap();
    let exports = format!("{}/exports", app.base());
    let case1 = case_script(
        1,
        Some("kim"),
        json!([
            { "step_number": 1, "actions": [
                { "kind": "navigate", "url": exports },
                { "kind": "click", "selector": { "css": "#csv" } },
                { "kind": "expect_download", "name": "report.csv" }
            ]}
        ]),
    );
    let case2 = case_script(
        2,
        None,
        json!([
            { "step_number": 1, "actions": [
                { "kind": "navigate", "url": format!("{}/leave", app.base()) },
                { "kind": "expect_visible", "selector": { "css": "input[type=password]" } }
            ]}
        ]),
    );
    store::save_scripts_atomically(root.path(), &[case1, case2]).unwrap();

    let launches = Arc::new(AtomicUsize::new(0));
    let closes = Arc::new(AtomicUsize::new(0));
    let port = Arc::new(std::sync::Mutex::new(None));
    let launcher = LiveLauncher { launches: launches.clone(), closes: closes.clone(), port: port.clone() };
    let mut browsers = WatchedOneBrowser {
        inner: v2_lib::autorun::one_browser::OneBrowser::new(launcher),
        port: port.clone(),
        seen: vec![],
        stranger: None,
    };
    let mut run = new_run(42);
    let cases = vec![(1, "signs in and downloads".to_string()), (2, "signs in as nobody".to_string())];
    let cancel = AtomicBool::new(false);
    let res = run_selection(&mut browsers, root.path(), "acme", "Web", &mut run, &cases, &timing(), &cancel, &mut |_| {}).await;
    assert!(res.is_ok(), "{res:?}");
    assert_eq!(run.cases.len(), 2);
    let (rec1, rec2) = (&run.cases[0], &run.cases[1]);
    assert_eq!(rec1.proposed, "Passed", "{} / {:?}", rec1.reason, rec1.steps);
    assert_eq!(rec2.proposed, "Passed", "case 1's cookie reached case 2: {} / {:?}", rec2.reason, rec2.steps);

    // One browser for both cases, still open until the run lets it go.
    assert_eq!(launches.load(Ordering::SeqCst), 1, "one launch for the run");
    assert_eq!(closes.load(Ordering::SeqCst), 0, "the browser stays between cases");

    // A context per case, each with only its own page, start to end.
    let seen = &browsers.seen;
    assert_eq!(seen.len(), 2, "{seen:?}");
    let contexts: Vec<String> = seen.iter().map(|s| s.context.clone().expect("each case has a context")).collect();
    assert_ne!(contexts[0], contexts[1], "{seen:?}");
    assert!(
        seen.iter().all(|s| s.tabs_at_start == 1 && s.tabs_at_end == 1),
        "a page of another context was adopted: {seen:?}"
    );

    // Case 1's download reached case 1's connection, and landed in the
    // run's own folder; case 2 heard of none.
    assert_eq!(seen[0].downloads, 1, "{seen:?}");
    assert_eq!(seen[1].downloads, 0, "{seen:?}");
    let landed = store::downloads_dir(root.path(), &run.id).join("report.csv");
    assert_eq!(std::fs::read_to_string(&landed).unwrap(), "Employee No,Name\r\nE001,Ada\r\n");

    // Each context was disposed when its case ended; the stranger's is
    // still there, so the listing is a real one.
    let port = port.lock().unwrap().expect("the browser was launched");
    let mut outside = Cdp::connect_browser(port).await.expect("the browser is still running");
    let listed = outside.call("Target.getBrowserContexts", json!({})).await.expect("contexts listed");
    let live: Vec<String> = listed["browserContextIds"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let stranger_context = browsers.stranger.as_ref().and_then(|s| s.browser_context().map(str::to_string));
    assert!(stranger_context.as_ref().is_some_and(|c| live.contains(c)), "{live:?}");
    for c in &contexts {
        assert!(!live.contains(c), "case context {c} was not disposed: {live:?}");
    }
    if let Some(mut s) = browsers.stranger.take() {
        let _ = s.dispose_context().await;
    }
    drop(outside);

    // Each case's open and close were timed.
    for rec in [rec1, rec2] {
        let p = rec.phases.as_ref().unwrap_or_else(|| panic!("case {} has no phases", rec.case_id));
        assert!(p.open_ms > 0, "case {}: {p:?}", rec.case_id);
        assert!(p.open_ms + p.close_ms <= p.total_ms, "case {}: {p:?}", rec.case_id);
    }

    drop(browsers);
    assert_eq!(closes.load(Ordering::SeqCst), 1, "closed once, at the end of the run");
}
