//! A project's known pop-ups (`close_when_shown` in its sign-in recipe):
//! closed by the runner, with one look and no wait, right before every
//! action that touches the page - so no script has to guard a click near
//! the bottom of PeoplesHR's pages against its cookie banner. Runs, tries
//! and replays all reach it through the same step loop (`run_step_in_run`).

use crate::common;

use common::{quick, ScriptedDriver};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::recipe::{builtin_recipe, save_recipe, SignInRecipe};
use v2_lib::autorun::runner::run_step;
use v2_lib::autorun::StepScript;
use v2_lib::browser::actions::Action;
use v2_lib::browser::input::COVERED_BY;

fn recipe(v: serde_json::Value) -> SignInRecipe {
    serde_json::from_value(v).expect("the test wrote an unparseable recipe")
}

fn cookie_banner() -> serde_json::Value {
    json!({ "name": "cookie banner", "shown": { "css": "#cookieBanner.show" }, "close": { "css": "#btnCookieClose" } })
}

fn with_popups(popups: serde_json::Value) -> serde_json::Value {
    json!({
        "start_url": "https://hr.example.internal/",
        "steps": [ { "kind": "click", "selector": { "css": "#login" } } ],
        "signed_in": { "css": "#marker" },
        "close_when_shown": popups
    })
}

fn refusal(popups: serde_json::Value) -> String {
    recipe(with_popups(popups)).validate().expect_err("this pop-up list should have been refused")
}

fn action(v: serde_json::Value) -> Action {
    serde_json::from_value(v).expect("the test wrote an action that does not parse")
}

fn step(actions: Vec<Action>) -> StepScript {
    StepScript { step_number: 1, actions, unchecked: None, cleanup: false, because: Vec::new() }
}

// ---------------------------------------------------------------- the recipe

#[test]
fn close_when_shown_parses_round_trips_and_is_optional() {
    let r = recipe(with_popups(json!([cookie_banner()])));
    assert_eq!(r.validate(), Ok(()));
    assert_eq!(r.close_when_shown.len(), 1);
    assert_eq!(r.close_when_shown[0].name, "cookie banner");
    let out = serde_json::to_value(&r).unwrap();
    assert_eq!(out["close_when_shown"], json!([cookie_banner()]), "it goes back out as written");

    let mut none = with_popups(json!([]));
    none.as_object_mut().unwrap().remove("close_when_shown");
    let r = recipe(none);
    assert!(r.close_when_shown.is_empty());
    assert!(serde_json::to_value(&r).unwrap().get("close_when_shown").is_none(), "an empty list is not written");
}

#[test]
fn an_entry_with_an_unknown_field_is_refused_as_it_is_read() {
    let mut entry = cookie_banner();
    entry["accept"] = json!({ "css": "#btnCookieAccept" });
    let err = serde_json::from_value::<SignInRecipe>(with_popups(json!([entry]))).unwrap_err().to_string();
    assert!(err.contains("accept"), "{err}");
}

#[test]
fn an_entry_needs_a_name_and_two_good_selectors() {
    let mut nameless = cookie_banner();
    nameless["name"] = json!("  ");
    assert_eq!(
        refusal(json!([nameless])),
        "close_when_shown 1: give the pop-up a short name, such as \"cookie banner\" - a step that closed it says \"closed the cookie banner\""
    );

    let mut empty_shown = cookie_banner();
    empty_shown["shown"] = json!({ "css": "" });
    let err = refusal(json!([cookie_banner(), empty_shown]));
    assert!(err.starts_with("close_when_shown 2 (cookie banner): \"shown\" - "), "{err}");

    let mut empty_close = cookie_banner();
    empty_close["close"] = json!("");
    assert_eq!(
        refusal(json!([empty_close])),
        "close_when_shown 1 (cookie banner): \"close\" - a selector is empty"
    );

    let mut list = cookie_banner();
    list["close"] = json!([]);
    assert_eq!(refusal(json!([list])), "close_when_shown 1 (cookie banner): \"close\" - a locator list is empty");
}

#[test]
fn a_placeholder_in_an_entry_is_refused() {
    let mut entry = cookie_banner();
    entry["close"] = json!({ "css": "#{{username}}" });
    assert_eq!(
        refusal(json!([entry])),
        "close_when_shown 1 (cookie banner): {{username}} and {{password}} are filled in for the recipe's own steps only - here they would be typed as they are"
    );
}

/// PeoplesHR's banner: shown while it has `show`, closed with its x -
/// never Accept All, so a run records no consent.
#[test]
fn the_built_in_recipe_closes_the_peopleshr_cookie_banner() {
    let r = builtin_recipe();
    assert_eq!(serde_json::to_value(&r.close_when_shown).unwrap(), json!([cookie_banner()]));
    let all = serde_json::to_string(&r).unwrap();
    assert!(!all.contains("Accept"), "{all}");
}

// ---------------------------------------------------------------- the runner

/// What the fake page saw: every click by the locator it was aimed at, and
/// every locator looked up, in order.
struct Seen {
    clicks: Arc<Mutex<Vec<String>>>,
    looks: Arc<Mutex<Vec<String>>>,
    banner: Arc<AtomicBool>,
}

impl Seen {
    fn clicks(&self) -> Vec<String> {
        self.clicks.lock().unwrap().clone()
    }
    fn looks_for(&self, css: &str) -> usize {
        self.looks.lock().unwrap().iter().filter(|s| *s == css).count()
    }
}

/// A page with PeoplesHR's cookie banner: `#cookieBanner.show` while it is
/// up, and while it is up every other element is covered by it, the way a
/// real browser's hit test says so. Clicking `#btnCookieClose` takes it
/// down - when `close_works`; otherwise the x is not there at all.
/// `#missing` is never on the page. Every other css locator is.
fn banner_page(banner_up: bool, close_works: bool) -> (ScriptedDriver, Seen) {
    let banner = Arc::new(AtomicBool::new(banner_up));
    let clicks = Arc::new(Mutex::new(Vec::<String>::new()));
    let looks = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Seen { clicks: clicks.clone(), looks: looks.clone(), banner: banner.clone() };
    let mut last = String::new();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let up = banner.load(Ordering::SeqCst);
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => {
                let mut probe = common::ready_probe();
                if up && last != "#btnCookieClose" {
                    probe["hit"] = json!(false);
                    probe["covered_by"] = json!("div#cookieBanner.cookie-banner.show");
                }
                json!({ "result": { "value": probe } })
            }
            "Runtime.callFunctionOn"
                if f == v2_lib::browser::locator::VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS || f == v2_lib::browser::input::HAS_FOCUS_JS =>
            {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last = params["arguments"][0]["value"].as_str().unwrap().to_string();
                looks.lock().unwrap().push(last.clone());
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = match last.as_str() {
                    "#cookieBanner.show" => up,
                    "#btnCookieClose" => up && close_works,
                    "#missing" => false,
                    _ => true,
                };
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Runtime.callFunctionOn" => json!({ "result": { "value": "text" } }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                clicks.lock().unwrap().push(last.clone());
                if last == "#btnCookieClose" {
                    banner.store(false, Ordering::SeqCst);
                }
                json!({})
            }
            _ => json!({}),
        })
    });
    (d, seen)
}

fn project_with_banner() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    save_recipe(dir.path(), "Acme", "Web", &recipe(with_popups(json!([cookie_banner()])))).unwrap();
    dir
}

#[tokio::test]
async fn a_shown_banner_is_closed_before_a_click_and_the_step_says_so() {
    let dir = project_with_banner();
    let (mut d, seen) = banner_page(true, true);
    let mut acc: Option<String> = None;
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![save]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(seen.clicks(), vec!["#btnCookieClose".to_string(), "#save".to_string()]);
    assert_eq!(out[0].detail, "clicked #save (closed the cookie banner first)");
    assert!(!seen.banner.load(Ordering::SeqCst));
}

#[tokio::test]
async fn a_fill_is_cleared_of_the_banner_too() {
    let dir = project_with_banner();
    let (mut d, seen) = banner_page(true, true);
    let mut acc: Option<String> = None;
    let fill = action(json!({ "kind": "fill", "selector": { "css": "#name" }, "value": "FY2026" }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![fill]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.ends_with("(closed the cookie banner first)"), "{}", out[0].detail);
    assert_eq!(seen.clicks(), vec!["#btnCookieClose".to_string()]);
}

#[tokio::test]
async fn with_nothing_shown_the_check_is_one_look_and_nothing_more() {
    let dir = project_with_banner();
    let (mut d, seen) = banner_page(false, true);
    let mut acc: Option<String> = None;
    let a = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let b = action(json!({ "kind": "click", "selector": { "css": "#next" } }));
    let began = std::time::Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a, b]), &quick(), &mut acc).await.unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[0].detail, "clicked #save", "no note when nothing was closed");
    assert_eq!(seen.looks_for("#cookieBanner.show"), 2, "one look per action - never a wait that polls");
    assert_eq!(seen.looks_for("#btnCookieClose"), 0);
    assert_eq!(seen.clicks(), vec!["#save".to_string(), "#next".to_string()]);
    assert!(began.elapsed() < std::time::Duration::from_millis(400), "took {:?}", began.elapsed());
}

#[tokio::test]
async fn checks_and_waits_never_close_anything() {
    let dir = project_with_banner();
    let (mut d, seen) = banner_page(true, true);
    let mut acc: Option<String> = None;
    let seen_banner = action(json!({ "kind": "expect_visible", "selector": { "css": "#cookieBanner.show" } }));
    let wait = action(json!({ "kind": "wait_for", "selector": { "css": "#save" }, "timeout_ms": 200 }));
    let out =
        run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![seen_banner, wait]), &quick(), &mut acc).await.unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert!(seen.clicks().is_empty());
    assert!(seen.banner.load(Ordering::SeqCst), "a check looks at the page as it is");
}

#[tokio::test]
async fn a_close_that_fails_leaves_the_actions_own_failure_and_says_so() {
    let dir = project_with_banner();
    let (mut d, seen) = banner_page(true, false);
    let mut acc: Option<String> = None;
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![save]), &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(
        out[0].detail.contains(&format!("{COVERED_BY}div#cookieBanner.cookie-banner.show")),
        "the covered-by failure stands: {}",
        out[0].detail
    );
    assert!(out[0].detail.contains("(the cookie banner was showing and could not be closed: "), "{}", out[0].detail);
    assert!(seen.clicks().is_empty());
    assert_eq!(seen.looks_for("#cookieBanner.show"), 1, "one check, no retry");
}

/// A script written before the runner did this still guards the banner
/// itself: the guarded click on the x is what closes it, and passes.
#[tokio::test]
async fn an_old_guard_on_the_close_control_stays_harmless() {
    let dir = project_with_banner();
    let (mut d, seen) = banner_page(true, true);
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": "#btnCookieClose" }, "within_ms": 200,
                               "then": [ { "kind": "click", "selector": { "css": "#btnCookieClose" } } ] }));
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard, save]), &quick(), &mut acc).await.unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(seen.clicks(), vec!["#btnCookieClose".to_string(), "#save".to_string()]);
    assert_eq!(out[1].detail, "clicked #save");
}

/// PeoplesHR's banner as tool errors #31, #34 and #71 met it: closing it
/// leaves its x in the page, moved outside the visible part of it. The x
/// is still drawn by the plain visibility rule, but a click's probe finds
/// it off the page (`onscreen: false`), as `SHOWN_JS` does. Every other
/// css locator is there, and covered while the banner is up.
fn banner_that_moves_off_the_page() -> (ScriptedDriver, Seen) {
    let banner = Arc::new(AtomicBool::new(true));
    let clicks = Arc::new(Mutex::new(Vec::<String>::new()));
    let looks = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Seen { clicks: clicks.clone(), looks: looks.clone(), banner: banner.clone() };
    let mut last = String::new();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let up = banner.load(Ordering::SeqCst);
        let off = !up && last == "#btnCookieClose";
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => {
                let mut probe = common::ready_probe();
                if off {
                    probe["onscreen"] = json!(false);
                    probe["hit"] = json!(false);
                } else if up && last != "#btnCookieClose" {
                    probe["hit"] = json!(false);
                    probe["covered_by"] = json!("div#cookieBanner.cookie-banner.show");
                }
                json!({ "result": { "value": probe } })
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::SHOWN_JS => json!({ "result": { "value": !off } }),
            "Runtime.callFunctionOn"
                if f == v2_lib::browser::locator::VISIBLE_JS || f == v2_lib::browser::input::HAS_FOCUS_JS =>
            {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last = params["arguments"][0]["value"].as_str().unwrap().to_string();
                looks.lock().unwrap().push(last.clone());
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = last != "#cookieBanner.show" || up;
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Runtime.callFunctionOn" => json!({ "result": { "value": "text" } }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                clicks.lock().unwrap().push(last.clone());
                if last == "#btnCookieClose" {
                    banner.store(false, Ordering::SeqCst);
                }
                json!({})
            }
            _ => json!({}),
        })
    });
    (d, seen)
}

/// Tool errors #31, #34, #71: the runner closes the banner before the
/// step's first click, so a guard on its x later in the step meets a
/// closed banner. The x left in the page outside the visible part of it is
/// not shown: the guard is skipped and the step carries on, where its
/// click used to wait 15 s and fail the step.
#[tokio::test]
async fn a_guard_on_a_banner_the_runner_already_closed_is_skipped() {
    let dir = project_with_banner();
    let (mut d, seen) = banner_that_moves_off_the_page();
    let mut acc: Option<String> = None;
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": "#btnCookieClose" }, "within_ms": 200,
                               "then": [ { "kind": "click", "selector": { "css": "#btnCookieClose" } } ] }));
    let next = action(json!({ "kind": "click", "selector": { "css": "#next" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![save, guard, next]), &quick(), &mut acc).await.unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[0].detail, "clicked #save (closed the cookie banner first)");
    assert_eq!(out[1].detail, format!("#btnCookieClose {}", v2_lib::browser::actions::NOT_SHOWN));
    assert_eq!(out[2].detail, "clicked #next");
    assert_eq!(seen.clicks(), vec!["#btnCookieClose".to_string(), "#save".to_string(), "#next".to_string()]);
}

#[tokio::test]
async fn a_project_with_no_list_closes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut plain = with_popups(json!([]));
    plain.as_object_mut().unwrap().remove("close_when_shown");
    save_recipe(dir.path(), "Acme", "Web", &recipe(plain)).unwrap();
    let (mut d, seen) = banner_page(true, true);
    let mut acc: Option<String> = None;
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![save]), &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok && out[0].detail.contains(COVERED_BY), "{:?}", out[0]);
    assert_eq!(seen.looks_for("#cookieBanner.show"), 0);
}

// ------------------------------------------------- a discovery's first look

/// `banner_page` whose banner comes up only on the `nth` look at it (a
/// banner that appears a moment after the page), closes with `#btnCookieClose`,
/// and is never up again once closed.
fn late_banner_page(nth: usize) -> (ScriptedDriver, Seen) {
    let banner = Arc::new(AtomicBool::new(false));
    let closed = Arc::new(AtomicBool::new(false));
    let clicks = Arc::new(Mutex::new(Vec::<String>::new()));
    let looks = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Seen { clicks: clicks.clone(), looks: looks.clone(), banner: banner.clone() };
    let mut last = String::new();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => {
                json!({ "result": { "value": common::ready_probe() } })
            }
            "Runtime.callFunctionOn"
                if f == v2_lib::browser::locator::VISIBLE_JS
                    || f == v2_lib::browser::input::SHOWN_JS
                    || f == v2_lib::browser::input::HAS_FOCUS_JS =>
            {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last = params["arguments"][0]["value"].as_str().unwrap().to_string();
                let mut l = looks.lock().unwrap();
                l.push(last.clone());
                if last == "#cookieBanner.show" && !closed.load(Ordering::SeqCst) {
                    let n = l.iter().filter(|s| *s == "#cookieBanner.show").count();
                    banner.store(n >= nth, Ordering::SeqCst);
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = match last.as_str() {
                    "#cookieBanner.show" | "#btnCookieClose" => banner.load(Ordering::SeqCst),
                    _ => true,
                };
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Runtime.callFunctionOn" => json!({ "result": { "value": "text" } }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                clicks.lock().unwrap().push(last.clone());
                if last == "#btnCookieClose" {
                    banner.store(false, Ordering::SeqCst);
                    closed.store(true, Ordering::SeqCst);
                }
                json!({})
            }
            _ => json!({}),
        })
    });
    (d, seen)
}

/// A banner that comes up a few looks in is waited for and closed with its
/// own x, and nothing else is clicked.
#[tokio::test]
async fn a_banner_that_comes_up_late_is_closed_within_the_wait_and_nothing_else_is_clicked() {
    use v2_lib::autorun::popups::close_shown_within;
    let (mut d, seen) = late_banner_page(3);
    let r = recipe(with_popups(json!([cookie_banner()])));
    let closed = close_shown_within(&mut d, &r, 3_000, &quick()).await;
    assert_eq!(closed, 1);
    assert_eq!(seen.clicks(), vec!["#btnCookieClose".to_string()]);
    assert!(seen.looks_for("#cookieBanner.show") >= 3);
}

/// With no banner, the wait ends at its limit, clicks nothing, and reports 0;
/// a wait of 0 is one look.
#[tokio::test]
async fn with_no_banner_up_nothing_is_clicked_and_a_zero_wait_is_one_look() {
    use v2_lib::autorun::popups::close_shown_within;
    let (mut d, seen) = banner_page(false, true);
    let r = recipe(with_popups(json!([cookie_banner()])));
    assert_eq!(close_shown_within(&mut d, &r, 0, &quick()).await, 0);
    assert_eq!(seen.looks_for("#cookieBanner.show"), 1);
    assert_eq!(close_shown_within(&mut d, &r, 150, &quick()).await, 0);
    assert!(seen.looks_for("#cookieBanner.show") > 2);
    assert!(seen.clicks().is_empty());
}
