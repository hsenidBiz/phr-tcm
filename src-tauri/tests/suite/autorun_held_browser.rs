//! A browser the app still holds after it is gone: before anything is
//! refused only because a browser is held, a dead one is let go; a held
//! browser that fails to answer is let go right there; and Release Auto Run
//! browser clears every record and closes only the app's own browsers.
//!
//! No real browser here: the slot holds a fake whose liveness the test
//! sets. The real liveness check and a real Release are in
//! `browser_live_tree` (ignored, they start Edge).

use crate::common::ScriptedDriver;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use v2_lib::ai_bridge::{
    forget_gone_browser, let_go_if_silent, read_page, refuse_while_held, DiscoveryBrowser, DiscoveryParts, BROWSER_GONE,
};
use v2_lib::autorun::lease::Held;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::tree::{self, Ends};
use v2_lib::commands::autorun::{
    busy_browser_sentence, release_autorun_browsers, release_for_assistant, release_in_for_assistant, released_sentence,
    DiscoveryState, PERSONS_BROWSER_OPEN, TEMPLATE_RUN_GOING,
};

/// A held browser as the slot sees it: alive or not as `alive` says, and
/// `closed` once the slot closed it.
struct HeldOne {
    d: ScriptedDriver,
    lease: Held,
    account: Option<String>,
    discovery: Option<DiscoveryState>,
    alive: bool,
    closed: Arc<AtomicBool>,
}

impl DiscoveryBrowser for HeldOne {
    type D = ScriptedDriver;
    fn parts(&mut self) -> DiscoveryParts<'_, ScriptedDriver> {
        DiscoveryParts {
            driver: &mut self.d,
            lease: &mut self.lease,
            signed_in: &mut self.account,
            discovery: &mut self.discovery,
        }
    }
    fn close(self) {
        self.closed.store(true, Ordering::SeqCst);
    }
    async fn alive(&mut self) -> bool {
        self.alive
    }
}

/// A driver whose browser has gone: every call is `Closed`.
fn gone_driver() -> ScriptedDriver {
    ScriptedDriver::new(|_, _| Err(CdpError::Closed))
}

fn held(alive: bool, discovery: Option<DiscoveryState>) -> (Option<HeldOne>, Arc<AtomicBool>) {
    let closed = Arc::new(AtomicBool::new(false));
    let d = if alive { ScriptedDriver::new(|_, _| Ok(serde_json::json!({}))) } else { gone_driver() };
    let b = HeldOne { d, lease: Held::supervised(), account: None, discovery, alive, closed: closed.clone() };
    (Some(b), closed)
}

fn discovering() -> Option<DiscoveryState> {
    Some(DiscoveryState { area: Some("Leave".into()), account: Some("admin".into()), started_at: 1, tried: Vec::new(), mapping: None })
}

/// The report: a supervised browser held after it was gone refused every
/// new discovery with "close the supervised browser first" until the app
/// restarted. Now the dead browser is closed through its normal close, the
/// slot is emptied, and the discovery is not refused.
#[tokio::test]
async fn a_dead_held_browser_no_longer_refuses_a_discovery_and_is_cleared() {
    let (mut slot, closed) = held(false, None);
    assert_eq!(refuse_while_held(&mut slot).await, Ok(()));
    assert!(slot.is_none(), "the dead browser is still held");
    assert!(closed.load(Ordering::SeqCst), "it was dropped without its normal close");

    // A dead discovery's browser the same.
    let (mut slot, closed) = held(false, discovering());
    assert_eq!(refuse_while_held(&mut slot).await, Ok(()));
    assert!(slot.is_none() && closed.load(Ordering::SeqCst));
}

/// A held browser that is really there is still refused for, with the
/// sentence it always had, and kept open.
#[tokio::test]
async fn a_live_held_browser_still_refuses_and_is_kept() {
    let (mut slot, closed) = held(true, None);
    assert_eq!(refuse_while_held(&mut slot).await, Err("close the supervised browser first".to_string()));
    assert!(slot.is_some() && !closed.load(Ordering::SeqCst));

    let (mut slot, _) = held(true, discovering());
    assert_eq!(refuse_while_held(&mut slot).await, Err(busy_browser_sentence(true).to_string()));
    assert!(slot.is_some());

    // Nothing held: nothing refused.
    let mut none: Option<HeldOne> = None;
    assert_eq!(refuse_while_held(&mut none).await, Ok(()));
    assert!(!forget_gone_browser(&mut none).await);
}

/// The report's `503: the browser did not answer: the browser closed before
/// answering`, over and over: a page read from a dead held browser now lets
/// it go right there and says no browser is open, and the next read finds
/// none rather than the same dead one.
#[tokio::test]
async fn a_page_read_from_a_dead_browser_says_none_is_open_and_clears_it() {
    let (mut slot, closed) = held(false, None);
    let answer = read_page(slot.as_mut().unwrap().parts().driver, 50, None).await;
    assert_eq!(answer.0, 503, "{answer:?}");
    assert!(answer.1.contains("the browser closed before answering"), "{answer:?}");

    let (status, said) = let_go_if_silent(&mut slot, answer).await;
    assert_eq!((status, said.as_str()), (409, BROWSER_GONE));
    assert!(said.contains("no Auto Run browser is open"), "{said}");
    assert!(!said.contains("http") && !said.contains("127.0.0.1") && !said.contains('?'), "{said}");
    assert!(!said.contains('\u{2014}') && !said.contains('\u{2013}'), "a long dash: {said}");
    assert!(slot.is_none() && closed.load(Ordering::SeqCst));
}

/// An answer that failed while the browser is still there - a slow page,
/// a tab the script closed - is passed on as it is, and the browser kept.
#[tokio::test]
async fn a_failed_answer_from_a_live_browser_keeps_it() {
    let (mut slot, closed) = held(true, None);
    let slow = (503, "the browser did not answer: the browser did not answer Runtime.evaluate within 2000ms".to_string());
    assert_eq!(let_go_if_silent(&mut slot, slow.clone()).await, slow);
    assert!(slot.is_some() && !closed.load(Ordering::SeqCst));

    // An ordinary answer never asks whether the browser is there.
    let (mut slot, closed) = held(false, None);
    let fine = (200, "button \"Save\"".to_string());
    assert_eq!(let_go_if_silent(&mut slot, fine.clone()).await, fine);
    assert!(slot.is_some() && !closed.load(Ordering::SeqCst));

    // A tried action's failure saying the browser closed is heard too.
    let (mut slot, closed) = held(false, None);
    let failed = (200, "failed: the browser closed before answering".to_string());
    assert_eq!(let_go_if_silent(&mut slot, failed).await.0, 409);
    assert!(slot.is_none() && closed.load(Ordering::SeqCst));
}

/// One of the app's own browsers in the registry, as `tree::end_all` ends
/// it: counted, never a real process.
struct AppBrowser {
    dir: PathBuf,
    ended: AtomicUsize,
}

impl Ends for AppBrowser {
    fn terminate(&self) {}
    fn end(&self) -> bool {
        self.ended.fetch_add(1, Ordering::SeqCst);
        true
    }
    fn profile_dir(&self) -> &Path {
        &self.dir
    }
}

/// Release clears every Auto Run browser record and closes the app's own
/// browsers - only those in its registry, which holds nothing but browsers
/// the app started in jobs of its own. With nothing held it says so.
#[tokio::test(flavor = "multi_thread")]
async fn release_clears_every_record_and_closes_only_the_apps_own_browsers() {
    let _a = crate::serial::autorun();
    let _t = crate::serial::api_template_run();
    let _h = crate::serial::held_browsers();
    let leftover = Arc::new(AppBrowser { dir: std::env::temp_dir().join("tcm-autorun-release-test"), ended: AtomicUsize::new(0) });
    tree::register(&leftover);

    let said = release_autorun_browsers().await.expect("release refused");
    assert_eq!(leftover.ended.load(Ordering::SeqCst), 1, "the app's own leftover browser was not closed");
    assert!(said.contains("1 of the app's own browsers was closed"), "{said}");
    assert!(!v2_lib::commands::autorun::auto_run_discovery_active());
    assert!(v2_lib::commands::autorun::open_session_refusal().await.is_none(), "a browser is still held");
    drop(leftover);

    // Again: nothing held, nothing to close, and it says so.
    let said = release_autorun_browsers().await.expect("release refused");
    assert_eq!(said, released_sentence(false, 0, 0));
    assert!(said.contains("nothing to release"), "{said}");

    // The person's command is the same release.
    assert_eq!(v2_lib::commands::autorun::auto_run_release_browser().await, Ok(said));
}

/// While an API template run holds its browser, Release waits for its own
/// Stop rather than closing the browser under it.
#[tokio::test]
async fn release_is_refused_while_a_template_run_is_going() {
    let _a = crate::serial::autorun();
    let _t = crate::serial::api_template_run();
    let claim = v2_lib::api_templates::runner::claim().expect("the run slot is free");
    assert_eq!(release_autorun_browsers().await, Err(TEMPLATE_RUN_GOING.to_string()));
    drop(claim);
}

/// What Release says: what it let go, how many of the app's browsers it
/// closed, and any that would not close yet. No long dash.
#[test]
fn the_release_sentence_says_what_was_released() {
    assert_eq!(released_sentence(true, 0, 0), "the Auto Run browser is released and closed");
    assert_eq!(
        released_sentence(true, 2, 0),
        "the Auto Run browser is released and closed, and 2 more of the app's own browsers were closed"
    );
    assert_eq!(
        released_sentence(false, 2, 1),
        "no Auto Run browser was held; 1 of the app's own browsers was closed; 1 would not close yet, and Windows ends it when the app exits"
    );
    for s in [released_sentence(false, 0, 0), released_sentence(true, 3, 2)] {
        assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'), "{s}");
    }
}

/// The assistant's tool and route: `release_autorun_browser` posts to
/// `/autorun-release`, which answers the same release.
#[tokio::test(flavor = "multi_thread")]
async fn the_assistant_releases_through_its_route() {
    let _a = crate::serial::autorun();
    let _t = crate::serial::api_template_run();
    let _h = crate::serial::held_browsers();
    let ctx = v2_lib::ai_bridge::BridgeContext { org: "acme".into(), project: "Web".into(), ..Default::default() };
    let (status, said) = v2_lib::ai_bridge::route(&ctx, None, "POST", "/autorun-release", "{}", "1.0.0").await;
    assert_eq!(status, 200, "{said}");
    assert!(said.contains("release") || said.contains("released"), "{said}");
    let mcp = include_str!("../../src/mcp.rs");
    assert!(mcp.contains("\"release_autorun_browser\" => call(\"POST\", \"/autorun-release\""), "the tool does not reach the route");
    assert!(v2_lib::ai_tools::DEV_ONLY_TOOLS.contains(&"release_autorun_browser"), "not on the Auto Run gate");
    let guide = include_str!("../../src/autorun/guide.rs");
    assert!(guide.contains("`release_autorun_browser`"), "the guide does not name it");
}

/// Every refusal made only because a browser is held asks first whether
/// it is alive: discovery's open, Open browser, a replay's open, a step
/// beside a discovery, and the others' shared refusal.
#[test]
fn every_refusal_for_a_held_browser_asks_first_whether_it_is_alive() {
    let source = include_str!("../../src/commands/autorun.rs");
    let body = |name: &str| {
        let at = source.find(name).unwrap_or_else(|| panic!("{name} is gone"));
        let rest = &source[at..];
        &rest[..rest.find("\n}\n").unwrap_or(rest.len())]
    };
    assert!(body("pub(crate) async fn open_for_discovery").contains("refuse_while_held(&mut slot)"));
    assert!(body("pub async fn auto_run_open_browser").contains("let_go_if_gone(&mut slot)"));
    assert!(body("async fn open_if_none").contains("let_go_if_gone(slot)"));
    assert!(body("pub(crate) async fn replay_supervised").contains("let_go_if_gone(&mut slot)"));
    assert!(body("pub async fn auto_run_step").contains("let_go_if_gone(&mut slot)"));
    assert!(body("pub async fn open_session_refusal").contains("let_go_if_gone(&mut slot)"));
    let bridge = include_str!("../../src/ai_bridge.rs");
    let page = &bridge[bridge.find("pub async fn supervised_page").unwrap()..];
    assert!(page[..page.find("\n}\n").unwrap()].contains("let_go_if_silent"), "the page read keeps a dead browser");
}

/// A registered browser whose end tries to start each kind of run, as a
/// person pressing Run while Release is closing browsers would: every one
/// must find its claim taken.
struct StartsDuringRelease {
    dir: PathBuf,
    started: std::sync::Mutex<Vec<&'static str>>,
}

impl Ends for StartsDuringRelease {
    fn terminate(&self) {}
    fn end(&self) -> bool {
        let mut started = self.started.lock().unwrap();
        if v2_lib::commands::autorun_replay::OneAtATime::claim().is_some() {
            started.push("unattended");
        }
        if v2_lib::commands::autorun_record::RecorderClaim::claim().is_some() {
            started.push("recording");
        }
        if v2_lib::api_templates::runner::claim().is_some() {
            started.push("template");
        }
        true
    }
    fn profile_dir(&self) -> &Path {
        &self.dir
    }
}

/// Release never races a run: it holds the unattended, recorder and
/// template claims until every browser is ended, so nothing starts in
/// between; and a run that already holds its claim refuses Release.
#[tokio::test(flavor = "multi_thread")]
async fn release_holds_the_run_claims_until_every_browser_is_ended() {
    let _a = crate::serial::autorun();
    let _t = crate::serial::api_template_run();
    let _h = crate::serial::held_browsers();
    let racer = Arc::new(StartsDuringRelease {
        dir: std::env::temp_dir().join("tcm-autorun-release-race"),
        started: std::sync::Mutex::new(Vec::new()),
    });
    tree::register(&racer);
    release_autorun_browsers().await.expect("release refused");
    let started = racer.started.lock().unwrap().clone();
    assert!(started.is_empty(), "a run started mid-release: {started:?}");
    drop(racer);
    // Free again once Release is done.
    assert!(v2_lib::commands::autorun_replay::OneAtATime::claim().is_some());

    // A start that holds its claim first refuses Release.
    let run = v2_lib::commands::autorun_replay::OneAtATime::claim().unwrap();
    assert_eq!(
        release_autorun_browsers().await,
        Err("an unattended run is going - wait for it, or stop it first".to_string())
    );
    drop(run);
    let recording = v2_lib::commands::autorun_record::RecorderClaim::claim().unwrap();
    assert_eq!(release_autorun_browsers().await, Err(v2_lib::commands::autorun_record::RECORDING_BUSY.to_string()));
    drop(recording);
}

/// The assistant's release lets go only of what is truly gone, or its own:
/// a dead browser is let go; a live discovery is ended as
/// end_autorun_discovery ends it; a live browser the person opened is kept,
/// and the answer asks for the person.
#[tokio::test]
async fn the_assistants_release_keeps_a_live_persons_browser() {
    let (mut slot, closed) = held(true, None);
    let (status, said) = release_in_for_assistant(&mut slot).await;
    assert_eq!((status, said.as_str()), (409, PERSONS_BROWSER_OPEN));
    assert!(!said.contains('\u{2014}') && !said.contains('\u{2013}'), "{said}");
    assert!(slot.is_some() && !closed.load(Ordering::SeqCst), "the person's browser was closed");

    let (mut slot, closed) = held(false, None);
    assert_eq!(release_in_for_assistant(&mut slot).await.0, 200);
    assert!(slot.is_none() && closed.load(Ordering::SeqCst), "a dead browser was kept");

    let (mut slot, closed) = held(true, discovering());
    let (status, said) = release_in_for_assistant(&mut slot).await;
    assert_eq!(status, 200, "{said}");
    assert!(said.contains("discovery is over"), "{said}");
    assert!(slot.is_none() && closed.load(Ordering::SeqCst));

    let mut none: Option<HeldOne> = None;
    assert_eq!(release_in_for_assistant(&mut none).await.0, 200);
}

/// The assistant's release never stops a replay going: it waits for it.
#[tokio::test]
async fn the_assistants_release_never_stops_a_replay() {
    let _a = crate::serial::autorun();
    let replay = v2_lib::autorun::replay_to::OneReplay::claim().unwrap();
    let (status, said) = release_for_assistant().await;
    assert_eq!((status, said.as_str()), (409, v2_lib::autorun::replay_to::ALREADY_RUNNING));
    assert!(!v2_lib::autorun::replay_to::CANCEL.load(Ordering::SeqCst), "the replay was asked to stop");
    drop(replay);
}

/// An unknown page gives no hint, never "(the page is /)".
#[test]
fn an_unknown_page_gives_no_page_hint() {
    use v2_lib::ai_bridge::with_page_where;
    assert_eq!(with_page_where("button \"Go\" not found", ""), "button \"Go\" not found");
    assert_eq!(with_page_where("x", "https://h.example/hr/home/index?id=1"), "x (the page is /hr/home/index)");
}
