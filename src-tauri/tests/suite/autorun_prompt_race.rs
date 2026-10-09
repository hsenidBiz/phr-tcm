//! The prompts a signed-in page may or may not show (a cookie banner, the
//! "another active session" modal, a menu drawn closed) are watched
//! together in ONE window, not each waited out in turn: a page that shows
//! none of them costs one short window, not the sum of every prompt's own.
//! The window is short after a saved session or a trip home, and longer
//! after a fresh login, where the session modal can arrive late.
//!
//! These run on a fake clock (`ScriptedDriver::idle_clock`): an idle adds
//! its wait to the clock and returns at once, so "the modal shows 3 s in"
//! costs no real time and the time a sign-in spent waiting is read back
//! exactly.

use crate::common;

use common::{account, quick, ScriptedDriver, PASSWORD};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::nav::{load_home, Home};
use v2_lib::autorun::recipe::SignInRecipe;
use v2_lib::autorun::sessions::{now_ms, save_session};
use v2_lib::autorun::signin::sign_in;
use v2_lib::autorun::timing::{FRESH_LOGIN_WINDOW_MS, PROMPT_WINDOW_MS};
use v2_lib::browser::cdp::{CdpError, Event};
use v2_lib::browser::input::{HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;
use v2_lib::browser::session::SavedSession;
use v2_lib::browser::timing::Timing;

/// Slack on top of a window: the idles a click's readiness check and the
/// loop's last poll add. Far below any second prompt's own window.
const MARGIN_MS: u64 = 200;

/// A sign-in like the built-in one: a form, then three optional prompts
/// with the built-in recipe's own windows (4 s, 4 s, 5 s).
fn prompt_recipe(sign_in_steps: serde_json::Value) -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": sign_in_steps,
        "after_sign_in": [
            { "kind": "when_visible", "selector": { "css": "#cookie" }, "within_ms": 4000,
              "then": [ { "kind": "click", "selector": { "css": "#cookie" } } ] },
            { "kind": "when_visible", "selector": { "css": "#modal" }, "within_ms": 4000,
              "then": [ { "kind": "click", "selector": { "css": "#modal" } } ] },
            { "kind": "when_visible", "selector": { "css": "#sidebar" }, "within_ms": 5000,
              "then": [ { "kind": "click", "selector": { "css": "#sidebar" } } ] }
        ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

fn form() -> serde_json::Value {
    json!([
        { "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" },
        { "kind": "fill", "selector": { "css": "#pass" }, "value": "{{password}}" },
        { "kind": "click", "selector": { "css": "#go" } }
    ])
}

/// What the fake page did: the clock (ms idled so far) and what was
/// clicked, by css, in order.
struct PromptApp {
    clock: Arc<AtomicU64>,
    clicked: Arc<Mutex<Vec<String>>>,
}

impl PromptApp {
    fn now(&self) -> u64 {
        self.clock.load(Ordering::SeqCst)
    }
    fn clicked(&self) -> Vec<String> {
        self.clicked.lock().unwrap().clone()
    }
}

/// A page whose form, home marker and buttons are always there, and whose
/// prompts each show from a time on the fake clock until clicked. A css
/// the page does not know is never there.
fn prompt_app(prompts: &[(&'static str, u64)]) -> (ScriptedDriver, PromptApp) {
    covering_app(prompts, &[])
}

/// `prompt_app` where some prompts sit over others: each `(over, under)`
/// pair says that while `over` shows, `under` is covered - drawn and
/// visible, but a click there would land on `over`.
fn covering_app(prompts: &[(&'static str, u64)], covers: &[(&'static str, &'static str)]) -> (ScriptedDriver, PromptApp) {
    page_app(Page { prompts: prompts.to_vec(), covers: covers.to_vec(), ..Page::default() })
}

/// What the fake page does, beyond its always-there form and marker.
#[derive(Default)]
struct Page {
    /// Each prompt's css and the fake-clock time it shows from, until
    /// clicked.
    prompts: Vec<(&'static str, u64)>,
    /// `(over, under)`: while `over` shows, `under` is covered.
    covers: Vec<(&'static str, &'static str)>,
    /// A prompt that stops matching at this fake-clock time.
    gone: Vec<(&'static str, u64)>,
    /// A prompt still sliding in until this fake-clock time: the probe
    /// sees it move within its watch (at least 2 frames and 50 ms).
    settling: Vec<(&'static str, u64)>,
    /// From the first readiness probe of a prompt, the browser answers no
    /// probe for this long (real time): a page whose main thread is busy.
    busy: Option<std::time::Duration>,
    /// From its own first readiness probe, the browser answers no probe of
    /// this prompt for this long (real time); every other element answers.
    stuck: Vec<(&'static str, std::time::Duration)>,
    /// Until this fake-clock time the browser finishes no look for any
    /// prompt: the page is busy before anything can be seen.
    looks_busy_until: Option<u64>,
}

fn page_app(page: Page) -> (ScriptedDriver, PromptApp) {
    let clock = Arc::new(AtomicU64::new(0));
    let clicked = Arc::new(Mutex::new(Vec::<String>::new()));
    let app = PromptApp { clock: clock.clone(), clicked: clicked.clone() };
    let prompts: Vec<(String, u64)> = page.prompts.iter().map(|(c, at)| (c.to_string(), *at)).collect();
    let covers: Vec<(String, String)> = page.covers.iter().map(|(o, u)| (o.to_string(), u.to_string())).collect();
    let gone: Vec<(String, u64)> = page.gone.iter().map(|(c, at)| (c.to_string(), *at)).collect();
    let busy = page.busy;
    let stuck: Vec<(String, std::time::Duration)> = page.stuck.iter().map(|(c, d)| (c.to_string(), *d)).collect();
    let mut stuck_since: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
    let looks_busy_until = page.looks_busy_until;
    let settling: Vec<(String, u64)> = page.settling.iter().map(|(c, at)| (c.to_string(), *at)).collect();
    let mut busy_since: Option<std::time::Instant> = None;
    let showing = move |css: &str, clock: &AtomicU64, clicked: &Mutex<Vec<String>>| -> bool {
        let now = clock.load(Ordering::SeqCst);
        if gone.iter().any(|(c, at)| c == css && now >= *at) {
            return false;
        }
        match prompts.iter().find(|(c, _)| c == css) {
            Some((c, at)) => now >= *at && !clicked.lock().unwrap().contains(c),
            None => ["#user", "#pass", "#go", "#marker"].contains(&css),
        }
    };
    let mut last_selector = String::new();
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Page.navigate" => json!({ "frameId": "F", "loaderId": "L" }),
            "Network.getAllCookies" => json!({ "cookies": [
                { "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true }
            ] }),
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => {
                let css = params["objectId"].as_str().unwrap_or("").trim_start_matches("el:").to_string();
                if let Some(busy) = busy {
                    if !["#user", "#pass", "#go"].contains(&css.as_str()) {
                        let since = *busy_since.get_or_insert_with(std::time::Instant::now);
                        if since.elapsed() < busy {
                            return Err(CdpError::Timeout { what: method.to_string(), ms: 250 });
                        }
                    }
                }
                if let Some((_, how_long)) = stuck.iter().find(|(c, _)| *c == css) {
                    let since = *stuck_since.entry(css.clone()).or_insert_with(std::time::Instant::now);
                    if since.elapsed() < *how_long {
                        return Err(CdpError::Timeout { what: method.to_string(), ms: 250 });
                    }
                }
                let over = covers.iter().find(|(o, u)| *u == css && showing(o, &clock, &clicked)).map(|(o, _)| o.clone());
                let now = clock.load(Ordering::SeqCst);
                let stable = !settling.iter().any(|(c, until)| *c == css && now < *until);
                json!({ "result": { "value": {
                    "visible": true, "onscreen": true, "enabled": true, "editable": true,
                    "hit": over.is_none(), "covered_by": over.unwrap_or_default(),
                    "x": 5.0, "y": 5.0, "rect": [0.0, 0.0, 10.0, 10.0], "stable": stable
                } } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                let css = params["arguments"][0]["value"].as_str().unwrap();
                let form = ["#user", "#pass", "#go", "#marker"].contains(&css);
                if !form && looks_busy_until.is_some_and(|until| clock.load(Ordering::SeqCst) < until) {
                    return Err(CdpError::Timeout { what: method.to_string(), ms: 250 });
                }
                last_selector = params["arguments"][0]["value"].as_str().unwrap().to_string();
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.callFunctionOn" => json!({ "result": { "value": "text" } }),
            "Runtime.getProperties" => {
                let there = showing(&last_selector, &clock, &clicked);
                let id = format!("el:{last_selector}");
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": id } })] } else { vec![] } })
            }
            "Input.dispatchMouseEvent" => {
                if params["type"] == "mouseReleased" {
                    clicked.lock().unwrap().push(last_selector.clone());
                }
                json!({})
            }
            _ => json!({}),
        })
    });
    d.on_every_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }) },
    ));
    d.idle_clock = Some(app.clock.clone());
    (d, app)
}

fn with_saved_session(dir: &std::path::Path) {
    let saved = SavedSession {
        saved_at_ms: now_ms(),
        cookies: vec![json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true })],
        local_storage: vec![],
    };
    save_session(dir, "admin", &saved).unwrap();
}

#[tokio::test]
async fn prompts_that_never_appear_cost_one_window_not_five() {
    // After a saved session: one short window, not 4 + 4 + 5 s.
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = prompt_app(&[]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok && out.used_saved_session, "{}", out.detail);
    assert!(app.now() >= PROMPT_WINDOW_MS, "the window was not watched out: {} ms", app.now());
    assert!(app.now() <= PROMPT_WINDOW_MS + MARGIN_MS, "waited {} ms for prompts that never came", app.now());
    let carried: Vec<_> = out.steps.iter().filter(|s| s.detail.contains("did not appear, carried on")).collect();
    assert_eq!(carried.len(), 3, "each prompt still says it was carried past: {:?}", out.steps);
    assert!(app.clicked().is_empty(), "{:?}", app.clicked());

    // After a fresh login: one longer window, still not the sum.
    let dir = tempfile::tempdir().unwrap();
    let (mut d, app) = prompt_app(&[]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok && !out.used_saved_session, "{}", out.detail);
    assert!(app.now() >= FRESH_LOGIN_WINDOW_MS, "the fresh-login window was not watched out: {} ms", app.now());
    assert!(app.now() <= FRESH_LOGIN_WINDOW_MS + MARGIN_MS, "waited {} ms after a fresh login", app.now());
    assert!(!serde_json::to_string(&out).unwrap().contains(PASSWORD));
}

#[tokio::test]
async fn a_late_session_modal_after_a_fresh_login_is_still_dismissed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, app) = prompt_app(&[("#modal", 3000)]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok && !out.used_saved_session, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#go", "#modal"], "the modal 3 s in was dismissed");
    assert_eq!(out.appeared.len(), 1, "{:?}", out.appeared);
    assert!(out.appeared[0].contains("#modal"), "{:?}", out.appeared);
}

#[tokio::test]
async fn a_saved_session_reuse_uses_the_short_window() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    // Something that would only show 3 s in is past the short window.
    let (mut d, app) = prompt_app(&[("#modal", 3000)]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok && out.used_saved_session, "{}", out.detail);
    assert!(app.clicked().is_empty(), "the short window should have ended first: {:?}", app.clicked());
    assert!(app.now() <= PROMPT_WINDOW_MS + MARGIN_MS, "waited {} ms", app.now());
}

#[tokio::test]
async fn prompts_are_handled_in_recipe_order_when_several_appear() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = prompt_app(&[("#sidebar", 0), ("#modal", 0), ("#cookie", 0)]);
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#cookie", "#modal", "#sidebar"]);
    assert_eq!(out.appeared.len(), 3, "{:?}", out.appeared);
    // Every prompt handled: the window ends there, not at its end.
    assert!(app.now() < PROMPT_WINDOW_MS / 2, "waited {} ms with nothing left to watch", app.now());
}

#[tokio::test]
async fn a_trip_home_uses_the_short_window() {
    let r = prompt_recipe(form());
    let (mut d, app) = prompt_app(&[("#cookie", 0), ("#modal", 3000)]);
    let out = load_home(&mut d, &Home::of(&r), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#cookie"], "the modal 3 s in is past the short window");
    assert!(app.now() >= PROMPT_WINDOW_MS, "the rest of the window was not watched: {} ms", app.now());
    assert!(app.now() <= PROMPT_WINDOW_MS + MARGIN_MS, "waited {} ms", app.now());
}

/// A sign-in step's optional prompt is looked for once, not waited out,
/// when the login form is already showing: the page has loaded, and a
/// banner that is not there now is not coming.
#[tokio::test]
async fn a_sign_in_prompt_is_looked_for_once_when_the_login_field_is_showing() {
    let dir = tempfile::tempdir().unwrap();
    let steps = json!([
        { "kind": "when_visible", "selector": { "css": "#banner" }, "within_ms": 3000,
          "then": [ { "kind": "click", "selector": { "css": "#banner" } } ] },
        { "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" },
        { "kind": "fill", "selector": { "css": "#pass" }, "value": "{{password}}" },
        { "kind": "click", "selector": { "css": "#go" } }
    ]);
    let mut r = prompt_recipe(steps);
    r.after_sign_in.clear();
    let (mut d, app) = prompt_app(&[]);
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(out.steps.iter().any(|s| s.detail.contains("#banner") && s.detail.contains("did not appear, carried on")), "{:?}", out.steps);
    assert!(app.now() < 500, "the banner was waited for: {} ms", app.now());

    // And one that IS showing is still dismissed.
    let dir = tempfile::tempdir().unwrap();
    let (mut d, app) = prompt_app(&[("#banner", 0)]);
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#banner", "#go"]);
}

/// The menu toggle shows at once; the session modal comes 250 ms later
/// and takes the click layer. With a watched run's highlight pause the
/// toggle's click is re-checked under the modal and refused. That is not a
/// failure of the sign-in: the modal is dismissed on a later look and the
/// toggle clicked after it, within the window.
#[tokio::test]
async fn a_late_modal_covering_the_sidebar_is_handled_and_the_sidebar_clicked_after() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = covering_app(&[("#sidebar", 0), ("#modal", 250)], &[("#modal", "#sidebar")]);
    let watched = Timing { highlight_ms: 350, ..quick() };
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &watched).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#modal", "#sidebar"]);
    assert!(out.steps.iter().all(|s| s.ok), "a refused attempt is not a failed step: {:?}", out.steps);
    // The cookie banner never comes, so the window runs out; on the fake
    // clock the three highlight pauses come on top of the race's own polls.
    assert!(app.now() <= PROMPT_WINDOW_MS + 3 * 350 + MARGIN_MS, "waited {} ms", app.now());
}

/// The cookie bar lands over the menu toggle while the toggle's click is
/// waiting for it to be usable (it is still sliding in): the bar is
/// dismissed first, then the toggle clicked.
#[tokio::test]
async fn a_cookie_bar_covering_the_menu_toggle_is_handled_first() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = page_app(Page {
        prompts: vec![("#sidebar", 0), ("#cookie", 5)],
        covers: vec![("#cookie", "#sidebar")],
        settling: vec![("#sidebar", 20)],
        ..Page::default()
    });
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#cookie", "#sidebar"]);
    assert!(out.steps.iter().all(|s| s.ok), "{:?}", out.steps);
}

/// A recorded recipe with short prompt windows: the old one-by-one waits
/// watched the modal from 1000 to 2000 ms when no cookie banner came, so
/// the shared window is never shorter than that whole span.
#[tokio::test]
async fn a_recorded_recipe_never_gets_a_shorter_window_than_before() {
    let dir = tempfile::tempdir().unwrap();
    let r: SignInRecipe = serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": form(),
        "after_sign_in": [
            { "kind": "when_visible", "selector": { "css": "#cookie" }, "within_ms": 1000,
              "then": [ { "kind": "click", "selector": { "css": "#cookie" } } ] },
            { "kind": "when_visible", "selector": { "css": "#modal" }, "within_ms": 1000,
              "then": [ { "kind": "click", "selector": { "css": "#modal" } } ] }
        ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap();
    let (mut d, app) = prompt_app(&[("#modal", 1500)]);
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert!(out.ok && !out.used_saved_session, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#go", "#modal"], "the modal at 1.5 s was dismissed");
    assert!(app.now() <= 2000 + MARGIN_MS, "never longer than the old waits together: {} ms", app.now());
}

/// The page's main thread is busy when the menu toggle shows: for 150 ms
/// the browser answers no readiness probe, longer than one short attempt
/// inside the watch. That is not the browser gone silent: the toggle gets
/// an ordinary action's full wait, and is clicked once the page answers.
#[tokio::test]
async fn a_busy_page_during_a_short_attempt_is_not_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = page_app(Page {
        prompts: vec![("#sidebar", 0)],
        busy: Some(std::time::Duration::from_millis(150)),
        ..Page::default()
    });
    // Short attempts get 3 polls (30 ms); the full budget is 1000 ms.
    let patient = Timing { action_ms: 1000, ..quick() };
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &patient).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#sidebar"]);
    assert!(out.steps.iter().all(|s| s.ok), "{:?}", out.steps);
}

/// The toggle is covered by something no prompt dismisses, and then goes
/// away (the page redrew it open). When the window ends it no longer
/// matches, so it is carried past - not given a final full-length try at
/// an element that is not there.
#[tokio::test]
async fn a_blocked_prompt_that_stopped_matching_is_not_retried_after_the_window() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = page_app(Page {
        prompts: vec![("#sidebar", 0), ("#overlay", 0)],
        covers: vec![("#overlay", "#sidebar")],
        gone: vec![("#sidebar", 500)],
        ..Page::default()
    });
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(app.clicked().is_empty(), "{:?}", app.clicked());
    let last = out.steps.last().unwrap();
    assert!(last.ok && last.detail.contains("#sidebar went away") && last.detail.contains("carried on"), "{:?}", out.steps);
}

/// A prompt with two actions: the first closes it, the second fails. That
/// is the second action's failure, not the prompt going away by itself:
/// the sign-in says so, and is not recorded as passed.
#[tokio::test]
async fn a_prompt_whose_second_action_fails_after_the_first_closed_it_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let r: SignInRecipe = serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": form(),
        "after_sign_in": [
            { "kind": "when_visible", "selector": { "css": "#cookie" }, "within_ms": 4000,
              "then": [
                  { "kind": "click", "selector": { "css": "#cookie" } },
                  { "kind": "click", "selector": { "css": "#confirm" } }
              ] }
        ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap();
    let (mut d, app) = prompt_app(&[("#cookie", 0)]);
    let out = sign_in(&mut d, dir.path(), &r, &account(), &quick()).await;
    assert_eq!(app.clicked(), vec!["#cookie"]);
    assert!(!out.ok, "the second action failed, yet the sign-in passed: {:?}", out.steps);
    assert!(out.detail.contains("#confirm"), "{}", out.detail);
    assert!(!out.steps.iter().any(|s| s.detail.contains("went away")), "{:?}", out.steps);
}

/// The menu toggle's probe times out on a short attempt (the browser is
/// slow to answer for that one element). That is not escalated at once to
/// a full action's wait, which would hold up every other prompt: the
/// cookie bar arriving meanwhile is dismissed first, and the toggle is
/// clicked once it answers.
#[tokio::test]
async fn a_short_attempt_timeout_does_not_hold_up_other_prompts() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = page_app(Page {
        prompts: vec![("#sidebar", 0), ("#cookie", 100)],
        stuck: vec![("#sidebar", std::time::Duration::from_millis(300))],
        ..Page::default()
    });
    let patient = Timing { action_ms: 1000, ..quick() };
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &patient).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#cookie", "#sidebar"]);
    assert!(out.steps.iter().all(|s| s.ok), "{:?}", out.steps);
}

/// The page is busy for longer than the whole window: no look finishes
/// until after it. That is not the browser gone silent while a look can
/// still finish within an ordinary action's wait: the watch keeps looking,
/// and the toggle is clicked once the page answers.
#[tokio::test]
async fn a_page_busy_longer_than_the_window_is_not_silence() {
    let dir = tempfile::tempdir().unwrap();
    with_saved_session(dir.path());
    let (mut d, app) = page_app(Page {
        prompts: vec![("#sidebar", 0)],
        looks_busy_until: Some(PROMPT_WINDOW_MS + 500),
        ..Page::default()
    });
    let patient = Timing { action_ms: 5000, ..quick() };
    let out = sign_in(&mut d, dir.path(), &prompt_recipe(form()), &account(), &patient).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(app.clicked(), vec!["#sidebar"]);
}
