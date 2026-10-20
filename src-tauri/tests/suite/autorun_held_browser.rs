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
    guarded_case: Option<i32>,
    guard_owner: Option<v2_lib::commands::autorun::GuardOwner>,
    alive: bool,
    closed: Arc<AtomicBool>,
    /// The assistant opened it (its replay); otherwise the person did.
    assistant: bool,
    /// How often a discovery took it over (`DiscoveryBrowser::taken_over`).
    taken_over: usize,
    /// Which browser it is, when the test says.
    kind: Option<v2_lib::browser::launch::Browser>,
}

impl DiscoveryBrowser for HeldOne {
    type D = ScriptedDriver;
    fn parts(&mut self) -> DiscoveryParts<'_, ScriptedDriver> {
        DiscoveryParts {
            driver: &mut self.d,
            lease: &mut self.lease,
            signed_in: &mut self.account,
            discovery: &mut self.discovery,
            guarded_case: &mut self.guarded_case,
            guard_owner: &mut self.guard_owner,
            overrides: None,
        }
    }
    fn close(self) {
        self.closed.store(true, Ordering::SeqCst);
    }
    async fn alive(&mut self) -> bool {
        self.alive
    }
    fn opened_by_assistant(&mut self) -> bool {
        self.assistant
    }
    fn taken_over(&mut self) {
        self.taken_over += 1;
    }
    fn browser(&mut self) -> Option<v2_lib::browser::launch::Browser> {
        self.kind
    }
}

/// A driver whose browser has gone: every call is `Closed`.
fn gone_driver() -> ScriptedDriver {
    ScriptedDriver::new(|_, _| Err(CdpError::Closed))
}

fn held(alive: bool, discovery: Option<DiscoveryState>) -> (Option<HeldOne>, Arc<AtomicBool>) {
    let closed = Arc::new(AtomicBool::new(false));
    let d = if alive { ScriptedDriver::new(|_, _| Ok(serde_json::json!({}))) } else { gone_driver() };
    let b = HeldOne {
        d,
        lease: Held::supervised(),
        account: None,
        discovery,
        guarded_case: None,
        guard_owner: None,
        alive,
        closed: closed.clone(),
        assistant: false,
        taken_over: 0,
        kind: None,
    };
    (Some(b), closed)
}

/// `held`, the browser opened by the assistant's replay.
fn left_open_by_assistant(alive: bool, discovery: Option<DiscoveryState>) -> (Option<HeldOne>, Arc<AtomicBool>) {
    let (mut slot, closed) = held(alive, discovery);
    if let Some(b) = slot.as_mut() {
        b.assistant = true;
    }
    (slot, closed)
}

fn discovering() -> Option<DiscoveryState> {
    Some(DiscoveryState {
        area: Some("Leave".into()),
        account: Some("admin".into()),
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
    })
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

/// The assistant's tool and route: `reset_autorun_browser` with
/// `release: true` posts to `/autorun-release`, which answers the same
/// release.
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
    assert!(mcp.contains("Ok(true) => call(\"POST\", \"/autorun-release\", \"{}\")"), "the tool does not reach the route");
    assert!(mcp.contains("\"reset_autorun_browser\" => reset_call(args, call)"), "the tool does not reach the route");
    assert_eq!(
        v2_lib::ai_tools::module_of_tool("reset_autorun_browser"),
        Some(v2_lib::modules::Module::AutoRun),
        "not on the Auto Run gate"
    );
    let guide = include_str!("../../src/autorun/guide.rs");
    assert!(guide.contains("`reset_autorun_browser` with `release: true`"), "the guide does not name it");
}

/// Every refusal made only because a browser is held asks first whether
/// it is alive: discovery's open, Open browser, a replay's open, a step
/// beside a discovery, and the others' shared refusal.
#[test]
fn every_refusal_for_a_held_browser_asks_first_whether_it_is_alive() {
    // Line endings follow the checkout (CRLF on Windows), so read as LF.
    let source = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    let source = source.as_str();
    let body = |name: &str| {
        let at = source.find(name).unwrap_or_else(|| panic!("{name} is gone"));
        let rest = &source[at..];
        &rest[..rest.find("\n}\n").unwrap_or(rest.len())]
    };
    assert!(body("pub(crate) async fn open_for_discovery").contains("make_way_for_discovery(&mut slot, wanted, guarded)"));
    // A guarded discovery hands the takeover where its save words are, so
    // its guard goes on under this same lock.
    assert!(body("pub(crate) async fn open_for_discovery").contains("(mapping.is_some() || no_save.no_save).then_some(from)"));
    assert!(body("pub async fn auto_run_open_browser").contains("let_go_if_gone(&mut slot)"));
    assert!(body("async fn open_if_none").contains("let_go_if_gone(slot)"));
    assert!(body("pub(crate) async fn replay_supervised").contains("let_go_if_gone(&mut slot)"));
    assert!(body("pub async fn auto_run_step").contains("let_go_if_gone(&mut slot)"));
    assert!(body("pub async fn open_session_refusal").contains("let_go_if_gone(&mut slot)"));
    let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
    // `supervised_page` hands its read to `supervised_page_within`.
    let page = &bridge[bridge.find("pub async fn supervised_page_within").unwrap()..];
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

/// The assistant's release closes a live browser its own replay opened,
/// through its normal close, and still keeps one the person opened.
#[tokio::test]
async fn the_assistants_release_closes_the_browser_its_replay_left_open() {
    let (mut slot, closed) = left_open_by_assistant(true, None);
    let (status, said) = release_in_for_assistant(&mut slot).await;
    assert_eq!((status, said.as_str()), (200, v2_lib::commands::autorun::RELEASED_YOURS));
    assert!(slot.is_none() && closed.load(Ordering::SeqCst), "the assistant's browser was kept");
    assert!(!said.contains('\u{2014}') && !said.contains('\u{2013}'), "{said}");

    let (mut slot, closed) = held(true, None);
    assert_eq!(release_in_for_assistant(&mut slot).await.0, 409);
    assert!(slot.is_some() && !closed.load(Ordering::SeqCst), "the person's browser was closed");
}

/// The replay's browser as a takeover finds it: signed in as admin,
/// guarding its no_save case 77 for the replay's call, with a report tab
/// its steps opened, a dialog it expected and one it saw.
async fn replays_browser() -> (Option<HeldOne>, Arc<AtomicBool>) {
    use v2_lib::browser::cdp::Driver;
    use v2_lib::browser::actions::DialogAnswer;
    use v2_lib::browser::dialogs::plan_of;
    use v2_lib::commands::autorun::GuardOwner;
    let (mut slot, closed) = left_open_by_assistant(true, None);
    let b = slot.as_mut().unwrap();
    b.account = Some("admin".into());
    b.d.guard_saves(&[]).await.unwrap();
    b.guarded_case = Some(77);
    b.guard_owner = Some(GuardOwner::Call { case_id: Some(77) });
    b.d.tabs.open.push("report".into());
    b.d.book.arm(vec![plan_of(1, DialogAnswer::Accept, &None), plan_of(2, DialogAnswer::Dismiss, &None)]);
    b.d.book.opened("alert", "Saved");
    (slot, closed)
}

/// A discovery takes over a live browser the assistant's replay left open,
/// and says so: the browser is kept, signed in, its other tabs closed, the
/// dialogs the replay expected or saw forgotten, and the replay's stand and
/// last page change too. An unguarded discovery leaves the replay's guard
/// to its start (lifted once the sign-in is decided). One the person
/// opened, or a discovery's own, still refuses it.
#[tokio::test]
async fn a_discovery_takes_over_the_browser_the_replay_left() {
    use v2_lib::ai_bridge::{make_way_for_discovery, said_made_way, MadeWay, CLOSED_LEFT_OPEN, TOOK_OVER_LEFT_OPEN};
    use v2_lib::browser::cdp::Driver;
    use v2_lib::commands::autorun::GuardOwner;
    let (mut slot, closed) = replays_browser().await;
    assert_eq!(make_way_for_discovery(&mut slot, None, None).await, Ok(MadeWay::TookOver));
    let b = slot.as_ref().expect("the replay's browser was closed");
    assert!(!closed.load(Ordering::SeqCst), "the replay's browser was closed");
    assert_eq!(b.d.tabs.closed_others, 1, "the replay's other tabs were left open");
    assert!(b.d.tabs.open.is_empty());
    assert!(!b.d.book.is_armed() && b.d.book.seen().is_empty(), "the replay's dialogs were handed to the discovery");
    assert_eq!(b.taken_over, 1, "the replay's stand and last page change were kept");
    assert_eq!(b.account.as_deref(), Some("admin"), "the sign-in was dropped");
    // Not lifted here: the start lifts it once it has decided the sign-in.
    assert!(b.d.is_guarding_saves() && b.d.calls_to("Fetch.disable").is_empty());
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (Some(77), Some(GuardOwner::Call { case_id: Some(77) })));

    // The person's own browser is never taken over.
    let (mut slot, closed) = held(true, None);
    assert_eq!(make_way_for_discovery(&mut slot, None, None).await, Err(busy_browser_sentence(false).to_string()));
    let b = slot.as_ref().unwrap();
    assert!(!closed.load(Ordering::SeqCst) && b.taken_over == 0 && b.d.tabs.closed_others == 0);

    let (mut slot, _) = left_open_by_assistant(true, discovering());
    assert_eq!(make_way_for_discovery(&mut slot, None, Some(guard_from(Path::new(".")))).await, Err(busy_browser_sentence(true).to_string()));
    assert_eq!(slot.as_ref().unwrap().taken_over, 0);

    // The answer says it: beside a start's JSON, before a refusal's text.
    assert_eq!(TOOK_OVER_LEFT_OPEN, "Took over the browser your replay left open (its other tabs closed).");
    let (status, text) = said_made_way(MadeWay::TookOver, (200, r#"{"signed_in":true}"#.to_string()));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!((status, v["took_over"].as_str(), v.get("closed")), (200, Some(TOOK_OVER_LEFT_OPEN), None));
    assert_eq!(said_made_way(MadeWay::TookOver, (409, "no".into())), (409, format!("{TOOK_OVER_LEFT_OPEN} no")));
    assert_eq!(CLOSED_LEFT_OPEN, "Closed the browser your replay left open.");
    let (_, text) = said_made_way(MadeWay::Closed, (200, r#"{"signed_in":true}"#.to_string()));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["closed"].as_str(), Some(CLOSED_LEFT_OPEN));
    assert_eq!(said_made_way(MadeWay::Closed, (409, "no".into())), (409, format!("{CLOSED_LEFT_OPEN} no")));
    assert_eq!(said_made_way(MadeWay::Free, (409, "no".into())), (409, "no".to_string()));
}

/// A discovery that will be guarded - started with no_save, or a mapping
/// run - takes the replay's guard over as it is: never lifted, not for a
/// moment, its owner now the discovery's.
#[tokio::test]
async fn a_guarded_discovery_takes_the_replays_guard_over_without_lifting_it() {
    use v2_lib::ai_bridge::{make_way_for_discovery, MadeWay};
    use v2_lib::browser::cdp::Driver;
    use v2_lib::commands::autorun::GuardOwner;
    let (mut slot, _) = replays_browser().await;
    assert_eq!(make_way_for_discovery(&mut slot, None, Some(guard_from(Path::new(".")))).await, Ok(MadeWay::TookOver));
    let b = slot.as_ref().unwrap();
    assert!(b.d.is_guarding_saves(), "the guard went off for the takeover");
    assert!(b.d.calls_to("Fetch.disable").is_empty(), "the guard was stopped and put back");
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (None, Some(GuardOwner::Discovery)));
}

/// Where a guarded discovery in these tests reads its save words from:
/// `root`, which holds no areas file, so only the built-in words.
fn guard_from(root: &Path) -> v2_lib::ai_bridge::GuardFrom<'_> {
    v2_lib::ai_bridge::GuardFrom { root, organization: "acme", project: "Web" }
}

/// The final review's Important finding: a no_save (or mapping) discovery
/// taking over an UNGUARDED replay browser, signed in, had no guard on from
/// the takeover until its start ran - and `open_for_discovery` lets the
/// lock go in between, so a parallel try could save with a no_save
/// discovery on record. The guard is now on when the takeover returns,
/// still under the lock it was taken under, and it is the discovery's. An
/// unguarded discovery arms nothing. A guard that will not go on refuses
/// the discovery and leaves the replay's browser as it was.
#[tokio::test]
async fn a_guarded_discovery_arms_its_guard_when_it_takes_over_an_unguarded_browser() {
    use v2_lib::ai_bridge::{make_way_for_discovery, MadeWay};
    use v2_lib::browser::cdp::Driver;
    use v2_lib::commands::autorun::GuardOwner;
    let root = tempfile::tempdir().unwrap();
    let unguarded = || {
        let (mut slot, closed) = left_open_by_assistant(true, None);
        slot.as_mut().unwrap().account = Some("admin".into());
        (slot, closed)
    };

    let (mut slot, _) = unguarded();
    assert!(!slot.as_ref().unwrap().d.is_guarding_saves());
    assert_eq!(make_way_for_discovery(&mut slot, None, Some(guard_from(root.path()))).await, Ok(MadeWay::TookOver));
    let b = slot.as_ref().unwrap();
    assert!(b.d.is_guarding_saves(), "the takeover returned, and the lock with it, with no guard on");
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (None, Some(GuardOwner::Discovery)));
    assert_eq!(b.account.as_deref(), Some("admin"), "the sign-in was dropped");

    // An unguarded discovery leaves the browser unguarded.
    let (mut slot, _) = unguarded();
    assert_eq!(make_way_for_discovery(&mut slot, None, None).await, Ok(MadeWay::TookOver));
    let b = slot.as_ref().unwrap();
    assert!(!b.d.is_guarding_saves() && b.d.calls_to("Fetch.enable").is_empty());
    assert_eq!(b.guard_owner, None);

    // A guard that will not go on: refused, nothing taken over or closed.
    let (mut slot, closed) = unguarded();
    let d = std::mem::replace(&mut slot.as_mut().unwrap().d, ScriptedDriver::new(|_, _| Ok(serde_json::json!({}))));
    slot.as_mut().unwrap().d = d.answering_first(|method, _| {
        (method == "Network.setBypassServiceWorker")
            .then(|| Err(CdpError::Protocol { method: method.into(), message: "busy".into() }))
    });
    let refused = make_way_for_discovery(&mut slot, None, Some(guard_from(root.path()))).await;
    let why = refused.expect_err("a discovery that could not be guarded took the browser over");
    assert!(why.starts_with(v2_lib::browser::save_guard::SETUP_FAILED), "{why}");
    let b = slot.as_ref().expect("the replay's browser was let go");
    assert!(!closed.load(Ordering::SeqCst));
    assert_eq!((b.taken_over, b.d.tabs.closed_others), (0, 0), "a refused takeover still took the browser over");
    assert!(!b.d.is_guarding_saves());
    assert_eq!(b.guard_owner, None);
}

/// A discovery that names the other browser (Edge or Chrome) than the one
/// the replay left closes that one, and opens the one it named; naming the
/// same one, or none, keeps it.
#[tokio::test]
async fn a_discovery_naming_the_other_browser_closes_the_replays() {
    use v2_lib::ai_bridge::{make_way_for_discovery, MadeWay};
    use v2_lib::browser::launch::Browser;
    let edge = || async {
        let (mut slot, closed) = replays_browser().await;
        slot.as_mut().unwrap().kind = Some(Browser::Edge);
        (slot, closed)
    };
    let (mut slot, closed) = edge().await;
    assert_eq!(make_way_for_discovery(&mut slot, Some(Browser::Chrome), None).await, Ok(MadeWay::Closed));
    assert!(slot.is_none() && closed.load(Ordering::SeqCst), "Edge was taken over for a discovery in Chrome");
    for wanted in [Some(Browser::Edge), None] {
        let (mut slot, closed) = edge().await;
        assert_eq!(make_way_for_discovery(&mut slot, wanted, None).await, Ok(MadeWay::TookOver), "{wanted:?}");
        assert!(slot.is_some() && !closed.load(Ordering::SeqCst));
    }
    // The person's browser is refused, whichever is named.
    let (mut slot, closed) = held(true, None);
    slot.as_mut().unwrap().kind = Some(Browser::Edge);
    assert!(make_way_for_discovery(&mut slot, Some(Browser::Chrome), None).await.is_err());
    assert!(slot.is_some() && !closed.load(Ordering::SeqCst));
}

/// The person's End discovery closes the assistant's discovery browser so
/// Auto Run can run again (the mapping run's summary kept); the
/// assistant's end_autorun_discovery keeps it, signed in.
#[tokio::test]
async fn the_persons_end_closes_the_discovery_browser_and_the_assistants_keeps_it() {
    use v2_lib::ai_bridge::{end_discovery_as, EndedBy, DISCOVERY_ENDED_KEPT};
    use v2_lib::commands::autorun::MappingRun;
    let mapping = discovering().map(|s| DiscoveryState { mapping: Some(MappingRun::new(&["Leave".to_string()])), ..s });
    let (mut slot, closed) = left_open_by_assistant(true, mapping);
    let (status, said) = end_discovery_as(&mut slot, EndedBy::Person).await;
    assert_eq!(status, 200, "{said}");
    let v: serde_json::Value = serde_json::from_str(&said).expect("no summary");
    assert_eq!(v["detail"], "the discovery is over and its browser is closed");
    assert!(v["summary"].is_object(), "{said}");
    assert!(slot.is_none() && closed.load(Ordering::SeqCst), "the person's End discovery kept the browser");

    let (mut slot, closed) = left_open_by_assistant(true, discovering());
    let (status, said) = end_discovery_as(&mut slot, EndedBy::Assistant).await;
    assert_eq!((status, said.as_str()), (200, DISCOVERY_ENDED_KEPT));
    assert!(slot.as_ref().is_some_and(|b| b.discovery.is_none()) && !closed.load(Ordering::SeqCst));

    // In the app: the Setup card's button is the person's, the bridge's
    // end_autorun_discovery the assistant's.
    let source = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    let button = &source[source.find("pub async fn auto_run_end_discovery").unwrap()..];
    assert!(button[..button.find("\n}\n").unwrap()].contains("end_discovery(crate::ai_bridge::EndedBy::Person)"));
    let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
    let route = &bridge[bridge.find("(\"POST\", \"/autorun-discover-end\")").unwrap()..];
    assert!(route[..route.find("},").unwrap()].contains("end_discovery(EndedBy::Assistant)"));
}

/// A browser that has gone is still let go first, through its normal
/// close: nothing is taken over, and the discovery opens a new one.
#[tokio::test]
async fn a_dead_browser_is_still_let_go() {
    use v2_lib::ai_bridge::{make_way_for_discovery, MadeWay};
    let (mut slot, closed) = left_open_by_assistant(false, None);
    assert_eq!(make_way_for_discovery(&mut slot, None, None).await, Ok(MadeWay::Free), "a gone browser was taken over");
    assert!(slot.is_none() && closed.load(Ordering::SeqCst));
    let (mut slot, closed) = held(false, None);
    assert_eq!(make_way_for_discovery(&mut slot, None, None).await, Ok(MadeWay::Free));
    assert!(slot.is_none() && closed.load(Ordering::SeqCst));
    let mut none: Option<HeldOne> = None;
    assert_eq!(make_way_for_discovery(&mut none, None, None).await, Ok(MadeWay::Free));
}

/// The assistant's replay after its discovery ends the discovery and keeps
/// the browser, signed in: a no_save discovery's guard stays on for that
/// replay (it is the discovery's last call), and any other discovery's
/// guard - a mapping run's - is lifted. The run's summary is kept.
#[tokio::test]
async fn a_replay_after_a_discovery_keeps_the_browser() {
    use v2_lib::ai_bridge::end_discovery_for_replay;
    use v2_lib::browser::cdp::Driver;
    use v2_lib::commands::autorun::{GuardOwner, MappingRun};
    let dir = tempfile::tempdir().unwrap();
    let no_save = discovering().map(|s| DiscoveryState { no_save: true, ..s });
    let (mut slot, closed) = left_open_by_assistant(true, no_save);
    {
        let b = slot.as_mut().unwrap();
        b.account = Some("admin".into());
        b.d.guard_saves(&[]).await.unwrap();
        b.guard_owner = Some(GuardOwner::Discovery);
    }
    let ended = end_discovery_for_replay(&mut slot, dir.path()).await;
    assert!(ended.ended && ended.no_save, "{ended:?}");
    let b = slot.as_ref().expect("the discovery's browser was closed");
    assert!(!closed.load(Ordering::SeqCst));
    assert!(b.discovery.is_none(), "the discovery was not ended");
    assert_eq!(b.account.as_deref(), Some("admin"), "the sign-in was dropped");
    assert!(b.d.is_guarding_saves(), "a no_save discovery's guard was off before its last call");
    assert_eq!(b.guard_owner, Some(GuardOwner::Discovery));

    let mapping = discovering().map(|s| DiscoveryState { mapping: Some(MappingRun::new(&["Leave".to_string()])), ..s });
    let (mut slot, closed) = left_open_by_assistant(true, mapping);
    {
        let b = slot.as_mut().unwrap();
        b.d.guard_saves(&[]).await.unwrap();
        b.guard_owner = Some(GuardOwner::Discovery);
    }
    let ended = end_discovery_for_replay(&mut slot, dir.path()).await;
    assert!(ended.ended && !ended.no_save && ended.summary.is_some(), "{ended:?}");
    let b = slot.as_ref().expect("the mapping run's browser was closed");
    assert!(!closed.load(Ordering::SeqCst) && b.discovery.is_none());
    assert!(!b.d.is_guarding_saves(), "the mapping run's guard outlived it into the replay");
    assert_eq!(b.guard_owner, None);
}

/// The assistant's release is still the repair: it closes a discovery's
/// browser, the discovery ended first, where ending it keeps the browser.
#[tokio::test]
async fn the_assistants_release_still_closes_a_discoverys_browser() {
    let (mut slot, closed) = left_open_by_assistant(true, discovering());
    let (status, said) = release_in_for_assistant(&mut slot).await;
    assert_eq!((status, said.as_str()), (200, "the discovery is over and its browser is closed"));
    assert!(slot.is_none() && closed.load(Ordering::SeqCst), "release kept the discovery's browser");
}

/// Who opened the browser is marked where it opens, and the person's own
/// step or replay makes a browser theirs; the assistant's replay keeps an
/// open browser's mark.
#[test]
fn the_browser_is_marked_with_who_opened_it() {
    let source = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    let body = |start: &str| -> String {
        let from = &source[source.find(start).unwrap()..];
        from[..from.find("\n}\n").unwrap()].to_string()
    };
    assert!(body("pub async fn auto_run_open_browser").contains("OpenedBy::Person"));
    let discovery = body("pub(crate) async fn open_for_discovery");
    assert!(discovery.contains("OpenedBy::Assistant") && discovery.contains("make_way_for_discovery"), "{discovery}");
    let step = body("pub async fn auto_run_step");
    // The person taking the browser marks it theirs (and takes away what
    // the assistant changed for testing): `Session::make_persons`.
    let persons = body("    pub(crate) async fn make_persons");
    assert!(persons.contains("self.opened_by = OpenedBy::Person;"), "{persons}");
    assert!(persons.contains("overrides::take_for_person"), "{persons}");
    assert!(step.contains("session.make_persons().await?;"), "{step}");
    let replay = body("pub(crate) async fn replay_supervised");
    assert!(replay.contains("open_if_none(app, &mut slot, by.opens_as())"), "{replay}");
    assert!(replay.contains("if by == ReplayBy::Person {\n        if let Err(why) = session.make_persons().await {"), "{replay}");
    use v2_lib::commands::autorun::{OpenedBy, ReplayBy};
    assert_eq!((ReplayBy::Person.opens_as(), ReplayBy::Assistant.opens_as()), (OpenedBy::Person, OpenedBy::Assistant));
}

/// A template refused its account because the Auto Run browser holds it
/// names the release when the assistant opened that browser.
#[test]
fn a_template_held_off_by_the_assistants_browser_names_the_release() {
    use v2_lib::ai_bridge::{template_report_hint, FREE_IT_WITH_RELEASE};
    use v2_lib::autorun::lease::in_use_by_browser;
    assert_eq!(in_use_by_browser("automation"), "the account automation was in use by the Auto Run browser - try again when it is free");
    let report = |detail: String| serde_json::json!({ "ok": false, "steps": [{ "detail": detail }] }).to_string();
    let refused = report(in_use_by_browser("automation"));
    let v: serde_json::Value = serde_json::from_str(&template_report_hint(refused.clone(), "automation", true)).unwrap();
    assert_eq!(v["free_it"], FREE_IT_WITH_RELEASE);
    assert!(FREE_IT_WITH_RELEASE.contains("reset_autorun_browser and `release: true`"));
    assert_eq!(template_report_hint(refused.clone(), "automation", false), refused, "a browser not the assistant's, or holding another account, named the release");
    // Another account's refusal is not this template's.
    assert_eq!(template_report_hint(refused.clone(), "evelyn1", true), refused);
    let other = report("the account automation was in use by an unattended run - try again when it is free".into());
    assert_eq!(template_report_hint(other.clone(), "automation", true), other);
    // The app asks whether the assistant's own browser holds this very account.
    let source = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    let holds = &source[source.find("pub async fn assistant_browser_holds").unwrap()..];
    assert!(holds[..holds.find("\n}\n").unwrap()].contains("s.lease.account() == Some(key)"));
}

/// The assistant's release never stops a replay going: it waits for it.
#[tokio::test]
async fn the_assistants_release_never_stops_a_replay() {
    let _a = crate::serial::autorun();
    let replay = v2_lib::autorun::replay_to::OneReplay::claim().unwrap();
    let (status, said) = release_for_assistant().await;
    assert_eq!((status, said.clone()), (409, v2_lib::ai_bridge::replay_going_for_a_release()));
    assert!(said.contains("call replay_autorun_to_step"), "{said}");
    // The person's replay (no call of the assistant's waits on it): the
    // stop tool, which never stops it, is not offered.
    assert!(!said.contains("stop: true"), "{said}");
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

/// The body of the function `name` in `source`, to its closing brace at
/// the start of a line.
fn body_of<'a>(source: &'a str, name: &str) -> &'a str {
    let at = source.find(name).unwrap_or_else(|| panic!("{name} is gone"));
    let rest = &source[at..];
    &rest[..rest.find("\n}\n").unwrap_or(rest.len())]
}

/// A fixture run, a cleanup run and a case's setup (a watched start's,
/// through `auto_run_check_preconditions`, included) each hold the template
/// runner's one claim for as long as their browser is open, so Release,
/// which takes that claim, refuses while any of them is going and never
/// ends its browser mid-run.
#[tokio::test]
async fn release_refuses_while_a_fixture_cleanup_or_setup_run_holds_its_browser() {
    let _a = crate::serial::autorun();
    let _t = crate::serial::api_template_run();
    let before = |body: &str, first: &str, then: &str| match (body.find(first), body.find(then)) {
        (Some(a), Some(b)) => a < b,
        _ => panic!("{first} or {then} is gone"),
    };
    let fixture = include_str!("../../src/api_templates/fixture_run.rs");
    let run_saved = body_of(fixture, "pub async fn run_saved");
    assert!(before(run_saved, "claim()", "run_fixture("), "a fixture run opens its browser unclaimed");
    let commands = include_str!("../../src/commands/api_templates.rs");
    let cleanup = body_of(commands, "pub async fn auto_run_cleanup_run");
    assert!(before(cleanup, "runner::claim()", "run_cleanup("), "a cleanup run opens its browser unclaimed");
    let setup = include_str!("../../src/autorun/setup.rs");
    let prepare = body_of(setup, "pub async fn prepare_case_within");
    assert!(before(prepare, "claim()", "run_fixture_for("), "a case's setup opens its browser unclaimed");
    assert!(body_of(setup, "pub async fn check_supervised").contains("prepare_case_within("));
    let autorun = include_str!("../../src/commands/autorun.rs");
    assert!(body_of(autorun, "pub async fn auto_run_check_preconditions").contains("supervised_setup("));
    assert!(body_of(autorun, "async fn supervised_setup").contains("check_supervised("));

    // While that claim is held, Release refuses with its sentence.
    for _run in ["fixture", "cleanup", "setup"] {
        let held = v2_lib::api_templates::runner::claim().expect("the run slot is free");
        let refused = release_autorun_browsers().await;
        assert_eq!(refused, Err(TEMPLATE_RUN_GOING.to_string()));
        assert!(TEMPLATE_RUN_GOING.contains("fixture") && TEMPLATE_RUN_GOING.contains("cleanup") && TEMPLATE_RUN_GOING.contains("setup"));
        drop(held);
    }
}

/// Nothing a reset does was done to `b`: no tab closed, no page loaded, no
/// guard lifted, nothing forgotten.
fn untouched_by_a_reset(b: &HeldOne) {
    assert_eq!(b.d.tabs.closed_others, 0, "a tab was closed");
    assert!(b.d.calls_to("Page.navigate").is_empty(), "a page was loaded");
    assert!(b.d.calls_to("Fetch.disable").is_empty(), "the guard was lifted");
    assert_eq!(b.taken_over, 0, "the replay's stand or last page change was forgotten");
}

/// Review Focus 5: the assistant's reset never touches the person's
/// browser - one the person opened, or one holding a guard the person set -
/// and asks for the person, as release does.
#[tokio::test]
async fn a_reset_of_the_persons_browser_is_refused() {
    use v2_lib::browser::cdp::Driver;
    use v2_lib::commands::autorun::{reset_in_for_assistant, GuardOwner};
    let dir = tempfile::tempdir().unwrap();
    let quick = v2_lib::browser::timing::Timing::supervised();
    let (mut slot, closed) = held(true, None);
    slot.as_mut().unwrap().account = Some("admin".into());
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick).await;
    assert_eq!((status, said.as_str()), (409, PERSONS_BROWSER_OPEN));
    let b = slot.as_ref().expect("the person's browser was let go");
    assert!(!closed.load(Ordering::SeqCst), "the person's browser was closed");
    untouched_by_a_reset(b);
    assert_eq!(b.account.as_deref(), Some("admin"));

    // A guard the person set is never lifted, whoever opened the browser.
    let (mut slot, closed) = left_open_by_assistant(true, None);
    {
        let b = slot.as_mut().unwrap();
        b.d.guard_saves(&[]).await.unwrap();
        b.guarded_case = Some(9);
        b.guard_owner = Some(GuardOwner::Person);
    }
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick).await;
    assert_eq!((status, said.as_str()), (409, PERSONS_BROWSER_OPEN));
    let b = slot.as_ref().unwrap();
    assert!(!closed.load(Ordering::SeqCst));
    untouched_by_a_reset(b);
    assert!(b.d.is_guarding_saves());
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (Some(9), Some(GuardOwner::Person)));
}

/// A reset of the assistant's own browser takes it over (`taken_over`, which
/// in the app forgets where the last replay stood), so the next replay starts
/// from the beginning - what "already standing before step N" tells the
/// assistant to do. It does so whether the home page then loads or not.
#[tokio::test]
async fn a_reset_forgets_where_the_last_replay_stood() {
    use v2_lib::commands::autorun::reset_in_for_assistant;
    let dir = tempfile::tempdir().unwrap();
    v2_lib::autorun::recipe::save_recipe(dir.path(), "acme", "Web", &crate::common::menu_recipe()).unwrap();
    let quick = v2_lib::browser::timing::Timing {
        action_ms: 300,
        expect_ms: 300,
        nav_ms: 300,
        poll_ms: 20,
        highlight_ms: 0,
        lease_wait_ms: 300,
    };
    let (mut slot, closed) = left_open_by_assistant(true, None);
    let (status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick).await;
    assert!(!closed.load(Ordering::SeqCst), "{status}: {said}");
    let b = slot.as_ref().expect("the browser was let go");
    assert_eq!(b.taken_over, 1, "the reset kept the replay's stand: {status}: {said}");
}

/// Review Focus 5: a reset while a replay runs is refused at once, and the
/// replay is not asked to stop.
#[tokio::test]
async fn a_reset_during_a_replay_is_refused() {
    use v2_lib::commands::autorun::{reset_for_assistant, REPLAY_RUNNING_AT_RESET};
    let _a = crate::serial::autorun();
    let replay = v2_lib::autorun::replay_to::OneReplay::claim().unwrap();
    let answer = tokio::time::timeout(std::time::Duration::from_secs(2), reset_for_assistant("acme", "Web"))
        .await
        .expect("the reset waited on the replay");
    assert_eq!(answer, (409, REPLAY_RUNNING_AT_RESET.to_string()));
    assert_eq!(REPLAY_RUNNING_AT_RESET, "A replay is running: try again when it has finished.");
    assert!(!v2_lib::autorun::replay_to::CANCEL.load(Ordering::SeqCst), "the replay was asked to stop");
    drop(replay);
}

/// With no browser held - or one that has gone, which is let go through
/// its normal close - a reset says none is open.
#[tokio::test]
async fn a_reset_with_no_browser_says_so() {
    use v2_lib::commands::autorun::{reset_in_for_assistant, NO_BROWSER_OPEN};
    let dir = tempfile::tempdir().unwrap();
    let quick = v2_lib::browser::timing::Timing::supervised();
    assert_eq!(NO_BROWSER_OPEN, "No browser is open.");
    let mut none: Option<HeldOne> = None;
    assert_eq!(reset_in_for_assistant(&mut none, dir.path(), "acme", "Web", &quick).await, (404, NO_BROWSER_OPEN.to_string()));
    let (mut slot, closed) = left_open_by_assistant(false, None);
    assert_eq!(reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick).await, (404, NO_BROWSER_OPEN.to_string()));
    assert!(slot.is_none() && closed.load(Ordering::SeqCst), "a gone browser was kept");
}

/// The assistant's tool and route: `reset_autorun_browser` posts to
/// `/autorun-reset`, on the Auto Run gate beside release.
#[tokio::test(flavor = "multi_thread")]
async fn the_assistant_resets_through_its_route() {
    let _a = crate::serial::autorun();
    let _h = crate::serial::held_browsers();
    let dir = tempfile::tempdir().unwrap();
    v2_lib::autorun::store::set_root(dir.path().to_path_buf());
    let ctx = v2_lib::ai_bridge::BridgeContext { org: "acme".into(), project: "Web".into(), ..Default::default() };
    let answer = v2_lib::ai_bridge::route(&ctx, None, "POST", "/autorun-reset", "{}", "1.0.0").await;
    assert_eq!(answer, (404, v2_lib::commands::autorun::NO_BROWSER_OPEN.to_string()), "the route is missing");
    let mcp = include_str!("../../src/mcp.rs");
    assert!(mcp.contains("Ok(false) => call(\"POST\", \"/autorun-reset\", \"{}\")"), "the tool does not reach the route");
    assert_eq!(
        v2_lib::ai_tools::module_of_tool("reset_autorun_browser"),
        Some(v2_lib::modules::Module::AutoRun),
        "not on the Auto Run gate"
    );
    // A replay is refused before it waits for the browser.
    let source = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    let body = body_of(&source, "pub async fn reset_for_assistant(");
    let before_lock = &body[..body.find("SESSION.lock()").expect("the reset never takes the browser")];
    assert!(before_lock.contains("replay_to::is_running()"), "a replay is not refused first");
    // With no data directory and no browser, none is open - not a 503.
    let after_lock = &body[body.find("SESSION.lock()").unwrap()..];
    let no_root = &after_lock[after_lock.find("configured_root()").expect("the data directory is read before the lock")..];
    let no_root = &no_root[..no_root.find("NO_DATA_DIRECTORY").unwrap()];
    assert!(no_root.contains("if slot.is_none() {") && no_root.contains("(404, NO_BROWSER_OPEN.to_string())"), "{no_root}");
}
