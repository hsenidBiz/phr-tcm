//! An assistant's replay to a step (`POST /autorun-replay`).
//!
//! The browser side is a fake host (`ReplayHost`): the replay it runs and
//! the page it reads are recorded, so these tests see exactly when the
//! replay runs. The engine itself is `autorun_replay_to`'s.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use v2_lib::ai_bridge::{
    after_ended_discovery, autorun_guard_for, autorun_replay_with, route, AssistantReplay, BridgeContext,
    DiscoveryEnded, HostFuture, ReplayHost, ENDED_DISCOVERY_FIRST,
};
use v2_lib::autorun::mapping_summary::MappingSummary;
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::nav::{save_nav, ModulePath, NavFile};
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::replay_to::{OneReplay, ReplayEnd, ReplayRequest, ALREADY_RUNNING};
use v2_lib::autorun::{store, CaseScript};

const ID: i32 = 77;

fn leave_area() -> ModulePath {
    serde_json::from_value(json!({
        "module": "Leave",
        "area": "Leave",
        "clicks": [ { "role": "link", "name": "Leave", "exact": true } ],
        "arrived": "/hr/leave",
        "recorded": "2026-10-05T10:00:00Z"
    }))
    .unwrap()
}

/// Case 77: three one-click steps in the Leave area, run as `admin`.
fn script(no_save: bool) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": ID,
        "title": "Leave request",
        "account": "admin",
        "area": "Leave",
        "no_save": no_save,
        "steps": [
            { "step_number": 1, "actions": [ { "kind": "click", "selector": { "css": "#s1" } } ] },
            { "step_number": 2, "actions": [ { "kind": "click", "selector": { "css": "#s2" } } ] },
            { "step_number": 3, "actions": [ { "kind": "click", "selector": { "css": "#s3" } } ] }
        ]
    }))
    .unwrap()
}

/// The project, its sign-in, its account, the Leave area and `script`.
fn project(root: &Path, script: &CaseScript) {
    save_nav(root, "acme", "Web", &NavFile { direct_urls: false, modules: vec![leave_area()], save_words: vec![] })
        .unwrap();
    save_recipe(root, "acme", "Web", &common_recipe()).unwrap();
    save_accounts(root, &[crate::common::account()]).unwrap();
    store::save_script(root, script).unwrap();
}

fn common_recipe() -> v2_lib::autorun::recipe::SignInRecipe {
    crate::common::menu_recipe()
}

fn ctx() -> BridgeContext {
    BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() }
}

/// What the app ran and what the page said.
struct FakeHost {
    replays: Mutex<Vec<(String, String, ReplayRequest)>>,
    end: ReplayEnd,
    /// A discovery holds the browser: the replay ends it first.
    discovering: bool,
    /// The ended discovery was a mapping run with this summary.
    summary: Option<MappingSummary>,
    /// The browser would not open: the replay answers this `Err`.
    open_failed: Option<String>,
}

impl FakeHost {
    fn ending(end: ReplayEnd) -> Self {
        FakeHost {
            replays: Mutex::new(vec![]),
            end,
            discovering: false,
            summary: None,
            open_failed: None,
        }
    }
    fn ready() -> Self {
        Self::ending(ReplayEnd::Ready { case_id: ID, step: 3, notice: None })
    }
    fn replays(&self) -> Vec<ReplayRequest> {
        self.replays.lock().unwrap().iter().map(|r| r.2.clone()).collect()
    }
}

impl ReplayHost for FakeHost {
    fn replay(
        &self,
        organization: String,
        project: String,
        req: ReplayRequest,
    ) -> HostFuture<'_, Result<AssistantReplay, String>> {
        Box::pin(async move {
            self.replays.lock().unwrap().push((organization, project, req));
            // As `replay_supervised` says a browser that would not open
            // after it ended the discovery.
            if let Some(why) = &self.open_failed {
                return Err(after_ended_discovery(self.discovering, why));
            }
            let discovery = DiscoveryEnded { ended: self.discovering, summary: self.summary.clone() };
            Ok(AssistantReplay { end: self.end.clone(), discovery })
        })
    }
    fn page(&self, _organization: String, _project: String) -> HostFuture<'_, (u16, String)> {
        Box::pin(async { (200, "- button \"Submit\" [role=button name=\"Submit\"]".to_string()) })
    }
}

fn body(step: i32) -> String {
    json!({ "case_id": ID, "step": step }).to_string()
}

// ---- the route ----------------------------------------------------------------

/// A script marked must not save replays at once: nobody is asked, and its
/// no-save guard is what stops its saves (`guard_for_case`).
#[tokio::test]
async fn a_must_not_save_script_replays_without_asking() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(true));
    let host = FakeHost::ready();
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], "replayed case 77 to step 3 - the browser is on the page before step 3 runs");
    assert_eq!(v["page"], "- button \"Submit\" [role=button name=\"Submit\"]");
    assert_eq!(host.replays(), vec![ReplayRequest { case_id: ID, step: 3, db_read_access: true }]);
    let replays = host.replays.lock().unwrap();
    assert_eq!((replays[0].0.as_str(), replays[0].1.as_str()), ("acme", "Web"));
}

/// Nothing of the old Allow prompt is left: no request registry, no event
/// and no command to answer it.
#[test]
fn nothing_asks_the_person_before_a_replay() {
    let norm = |s: &str| s.replace("
", "
");
    let lib = norm(include_str!("../../src/lib.rs"));
    let events = norm(include_str!("../../src/events.rs"));
    let bridge = norm(include_str!("../../src/ai_bridge.rs"));
    for gone in ["AutorunReplayRequest", "auto_run_answer_replay_request", "replay_ask"] {
        assert!(!lib.contains(gone), "lib.rs still names {gone}");
        assert!(!events.contains(gone), "events.rs still names {gone}");
        assert!(!bridge.contains(gone), "ai_bridge.rs still names {gone}");
    }
}

#[tokio::test]
async fn a_normal_script_runs_without_asking() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let host = FakeHost::ready();
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    assert_eq!(status, 200, "{text}");
    assert_eq!(host.replays().len(), 1);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["page"].is_string(), "{v}");
}

#[tokio::test]
async fn a_replay_that_stops_answers_its_sentence_without_a_page() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    for (end, sentence) in [
        (
            ReplayEnd::StoppedAt {
                phase: v2_lib::autorun::replay_to::ReplayPhase::Step,
                step: 2,
                why: "nothing matched #s2".into(),
                outcomes: vec![],
            },
            "replay stopped at step 2: nothing matched #s2",
        ),
        (ReplayEnd::Stopped { step: 2 }, "the replay was stopped at step 2: it was asked to stop (Stop, Close browser or Release Auto Run browser)"),
        // The browser closing under it says so, and why, at step 1 too.
        (
            ReplayEnd::BrowserGone { step: 1, why: None, outcomes: vec![] },
            "replay stopped at step 1: the Auto Run browser closed before the step finished (its window was closed, or it stopped answering), so the app let it go - replay again and a new browser opens",
        ),
    ] {
        let host = FakeHost::ending(end);
        let (status, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
        assert_eq!(status, 200, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v, json!({ "sentence": sentence }));
    }
    // A refusal the engine itself gives is the usual refusal.
    let host = FakeHost::ending(ReplayEnd::Refused(ALREADY_RUNNING.into()));
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    assert_eq!((status, text.as_str()), (409, ALREADY_RUNNING));
}

#[tokio::test]
async fn refusals_before_running_never_ask_or_run() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());

    // No saved script.
    let host = FakeHost::ready();
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    assert_eq!((status, text.as_str()), (409, "case 77 has no saved script"));

    // A step outside the script, on a must-not-save one: refused before
    // the person is asked.
    project(dir.path(), &script(true));
    let (status, text) = autorun_replay_with(&ctx(), &body(5), &host).await;
    assert_eq!((status, text.as_str()), (409, "step 5 is not in case 77's script (it has steps 1 to 3)"));

    // A replay already running.
    let one = OneReplay::claim().unwrap();
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    assert_eq!((status, text.as_str()), (409, ALREADY_RUNNING));
    drop(one);

    // A body without its fields.
    for bad in [json!({ "step": 3 }), json!({ "case_id": ID }), json!({ "case_id": "x", "step": 3 })] {
        let (status, _) = autorun_replay_with(&ctx(), &bad.to_string(), &host).await;
        assert_eq!(status, 400, "{bad}");
    }

    assert!(host.replays().is_empty());
}

#[tokio::test]
async fn database_read_access_follows_the_db_query_switch() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let off = BridgeContext { disabled_tools: vec!["db_lookup".into(), "db_query".into()], ..ctx() };
    let host = FakeHost::ready();
    let (status, _) = autorun_replay_with(&off, &body(2), &host).await;
    assert_eq!(status, 200);
    assert_eq!(host.replays(), vec![ReplayRequest { case_id: ID, step: 2, db_read_access: false }]);
}

/// The route is an Auto Run one: gated with the others, and with no app
/// behind the bridge (a test, or before setup) it says so rather than
/// running anything.
#[tokio::test]
async fn the_route_is_gated_with_the_auto_run_routes() {
    assert!(autorun_guard_for("/autorun-replay", false).is_some());
    assert!(autorun_guard_for("/autorun-replay", true).is_none());
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let (status, text) = route(&ctx(), None, "POST", "/autorun-replay", &body(2), "test").await;
    assert_eq!(status, 503, "{text}");
}

// ---- a discovery holding the browser ---------------------------------------

/// A replay that ended the assistant's discovery says so first; one that
/// found none answers as before.
#[tokio::test]
async fn the_replay_answer_says_the_discovery_was_ended() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    assert_eq!(
        ENDED_DISCOVERY_FIRST,
        "Ended your discovery first (what it mapped is kept); start a new one to explore again."
    );
    let ready = "replayed case 77 to step 3 - the browser is on the page before step 3 runs";

    let host = FakeHost { discovering: true, ..FakeHost::ready() };
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], format!("{ENDED_DISCOVERY_FIRST} {ready}"));
    assert!(v["page"].is_string(), "{v}");
    assert_eq!(host.replays().len(), 1, "the replay did not run");

    // A stop after the end still says the discovery is over.
    let stopped = ReplayEnd::Stopped { step: 2 };
    let host = FakeHost { discovering: true, ..FakeHost::ending(stopped) };
    let (_, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], format!("{ENDED_DISCOVERY_FIRST} the replay was stopped at step 2: it was asked to stop (Stop, Close browser or Release Auto Run browser)"));

    let host = FakeHost::ready();
    let (_, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], ready);
}

/// A mapping run the replay ended hands back its summary beside the
/// sentence, as `end_autorun_discovery` does; an ordinary one adds none.
#[tokio::test]
async fn a_replay_that_ended_a_mapping_run_answers_its_summary() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let summary: MappingSummary = serde_json::from_value(json!({
        "ran_at": 1, "modules": ["Leave"], "added": ["Leave Apply"], "updated": [], "unchanged": [], "unreached": [], "blocked_writes": 0
    }))
    .unwrap();
    let host = FakeHost { discovering: true, summary: Some(summary.clone()), ..FakeHost::ready() };
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["sentence"].as_str().unwrap().starts_with(ENDED_DISCOVERY_FIRST), "{v}");
    assert_eq!(v["summary"], serde_json::to_value(&summary).unwrap());
    assert!(v["page"].is_string(), "{v}");

    // A stop after the end carries it too.
    let stopped = ReplayEnd::Stopped { step: 2 };
    let host = FakeHost { discovering: true, summary: Some(summary.clone()), ..FakeHost::ending(stopped) };
    let (_, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["summary"], serde_json::to_value(&summary).unwrap());

    let host = FakeHost { discovering: true, ..FakeHost::ready() };
    let (_, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v.get("summary").is_none(), "{v}");
}

/// A browser that would not open after the replay ended the discovery
/// still says the discovery was ended; with none ended, it says only why.
#[tokio::test]
async fn a_browser_that_would_not_open_after_the_end_still_says_the_discovery_was_ended() {
    const WOULD_NOT_OPEN: &str = "the browser would not open - try again";
    assert_eq!(after_ended_discovery(true, WOULD_NOT_OPEN), format!("{ENDED_DISCOVERY_FIRST} {WOULD_NOT_OPEN}"));
    assert_eq!(after_ended_discovery(false, WOULD_NOT_OPEN), WOULD_NOT_OPEN);

    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    for (discovering, said) in
        [(true, format!("{ENDED_DISCOVERY_FIRST} {WOULD_NOT_OPEN}")), (false, WOULD_NOT_OPEN.to_string())]
    {
        let host = FakeHost { discovering, open_failed: Some(WOULD_NOT_OPEN.into()), ..FakeHost::ready() };
        let (status, text) = autorun_replay_with(&ctx(), &body(3), &host).await;
        assert_eq!((status, text), (503, said));
    }

    // The app's own replay says it the same way.
    let source = include_str!("../../src/commands/autorun.rs");
    let replay = &source[source.find("pub(crate) async fn replay_supervised").unwrap()..];
    let open = &replay[replay.find("open_if_none(app, &mut slot)").unwrap()..];
    let open = &open[..open.len().min(300)];
    assert!(open.contains("after_ended_discovery(discovery.ended, &why)"), "{open}");
}
