//! An assistant's replay to a step (`POST /autorun-replay`).
//!
//! The browser side is a fake host (`ReplayHost`): the replay it runs and
//! the page it reads are recorded, so these tests see exactly when the
//! replay runs. The engine itself is `autorun_replay_to`'s.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use v2_lib::ai_bridge::{
    after_ended_discovery, module_guard_for, autorun_replay_with, route, AssistantReplay, BridgeContext,
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
    save_nav(root, "acme", "Web", &NavFile { direct_urls: false, modules: vec![leave_area()], save_words: vec![], protected: vec![] })
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
    /// The ended discovery must not save: its guard stays on.
    no_save: bool,
    /// The browser would not open: the replay answers this `Err`.
    open_failed: Option<String>,
    /// The replay runs on until this is notified (a slow replay); `None`
    /// ends it at once.
    release: Option<tokio::sync::Notify>,
    /// The browser the replay leaves open, as the app says it.
    left_open: Option<v2_lib::autorun::replay_to::LeftOpen>,
    /// Where the app says the replay started.
    from: v2_lib::autorun::replay_to::ReplayFrom,
    /// What the replay made of a browser already signed in as the case's
    /// account, as the app says it.
    reuse: v2_lib::autorun::replay_to::Reuse,
}

impl FakeHost {
    fn ending(end: ReplayEnd) -> Self {
        FakeHost {
            replays: Mutex::new(vec![]),
            end,
            discovering: false,
            summary: None,
            no_save: false,
            open_failed: None,
            release: None,
            left_open: None,
            from: v2_lib::autorun::replay_to::ReplayFrom::Beginning,
            reuse: Default::default(),
        }
    }
    fn ready() -> Self {
        Self::ending(ReplayEnd::Ready { case_id: ID, step: 3, notice: None, whole: None })
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
            if let Some(release) = &self.release {
                v2_lib::autorun::replay_to::note_progress(2, 2);
                release.notified().await;
            }
            // As `replay_supervised` says a browser that would not open
            // after it ended the discovery.
            if let Some(why) = &self.open_failed {
                return Err(after_ended_discovery(self.discovering, why));
            }
            let discovery = DiscoveryEnded { ended: self.discovering, summary: self.summary.clone(), no_save: self.no_save };
            Ok(AssistantReplay {
                end: self.end.clone(),
                discovery,
                left_open: self.left_open.clone(),
                from: self.from,
                reuse: self.reuse.clone(),
            })
        })
    }
    fn page(&self, _organization: String, _project: String) -> HostFuture<'_, (u16, String)> {
        Box::pin(async { (200, "- button \"Submit\" [role=button name=\"Submit\"]".to_string()) })
    }
}

/// A host for the detached replay, which outlives the call that started it.
fn leak(host: FakeHost) -> &'static FakeHost {
    Box::leak(Box::new(host))
}

/// How long a call waits for its replay in these tests.
const WAIT: Duration = Duration::from_secs(5);

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
    let host = leak(FakeHost::ready());
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], "Started from the beginning. Replayed case 77 to step 3 - the browser is on the page before step 3 runs");
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
    let host = leak(FakeHost::ready());
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!(status, 200, "{text}");
    assert_eq!(host.replays().len(), 1);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["page"].is_string(), "{v}");
}

/// A replay in a browser already signed in as its case's account says so
/// where a sign-in would have been, after where it started - on a stop
/// too, and never on a refusal (nothing ran).
#[tokio::test]
async fn a_replay_that_skipped_the_sign_in_says_it_went_straight_to_the_area() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let mut ready = FakeHost::ready();
    ready.reuse.went = Some("Leave".into());
    let (status, text) = autorun_replay_with(&ctx(), &body(3), leak(ready), WAIT).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["sentence"],
        "Started from the beginning. Signed in already: went to Leave. Replayed case 77 to step 3 - the browser is on the page before step 3 runs"
    );
    let mut stopped = FakeHost::ending(ReplayEnd::StoppedAt {
        phase: v2_lib::autorun::replay_to::ReplayPhase::Step,
        step: 2,
        why: "nothing matched #s2".into(),
        outcomes: vec![],
    });
    stopped.reuse.went = Some("Leave".into());
    let (_, text) = autorun_replay_with(&ctx(), &body(3), leak(stopped), WAIT).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], "Started from the beginning. Signed in already: went to Leave. Replay stopped at step 2: nothing matched #s2");
}

/// A save the guard stopped while a signed-in browser went to the area is
/// said in the answer, before the replay's own sentence.
#[tokio::test]
async fn a_save_stopped_on_the_way_to_the_area_is_said() {
    use v2_lib::autorun::replay_to::save_stopped_line;
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let sentence = v2_lib::browser::save_guard::blocked("POST", "https://hr.example.internal/api/leave/save");
    let mut ready = FakeHost::ready();
    ready.reuse.save_stopped = Some(save_stopped_line("Leave", &sentence));
    let (status, text) = autorun_replay_with(&ctx(), &body(3), leak(ready), WAIT).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["sentence"],
        format!("Started from the beginning. Signed in already, but on the way to Leave: {sentence}. Signed in again and went once more. Replayed case 77 to step 3 - the browser is on the page before step 3 runs")
    );
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
            "Started from the beginning. Replay stopped at step 2: nothing matched #s2",
        ),
        (ReplayEnd::Stopped { step: 2 }, "Started from the beginning. The replay was stopped at step 2: it was asked to stop (Stop, Close browser or Release Auto Run browser)"),
        // The browser closing under it says so, and why, at step 1 too.
        (
            ReplayEnd::BrowserGone { step: 1, why: None, outcomes: vec![] },
            "Started from the beginning. Replay stopped at step 1: the Auto Run browser closed before the step finished (its window was closed, or it stopped answering), so the app let it go - replay again and a new browser opens",
        ),
    ] {
        let host = leak(FakeHost::ending(end));
        let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
        assert_eq!(status, 200, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v, json!({ "sentence": sentence }));
    }
    // A refusal the engine itself gives is the usual refusal.
    let host = leak(FakeHost::ending(ReplayEnd::Refused(ALREADY_RUNNING.into())));
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!((status, text.as_str()), (409, ALREADY_RUNNING));
}

#[tokio::test]
async fn refusals_before_running_never_ask_or_run() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());

    // No saved script.
    let host = leak(FakeHost::ready());
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!((status, text.as_str()), (409, "case 77 has no saved script"));

    // A step outside the script, on a must-not-save one: refused before
    // the person is asked.
    project(dir.path(), &script(true));
    let (status, text) = autorun_replay_with(&ctx(), &body(5), host, WAIT).await;
    assert_eq!((status, text.as_str()), (409, "step 5 is not in case 77's script (it has steps 1 to 3)"));

    // A replay already running.
    let one = OneReplay::claim().unwrap();
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!((status, text.as_str()), (409, ALREADY_RUNNING));
    drop(one);

    // A body without its fields.
    for bad in [json!({ "step": 3 }), json!({ "case_id": ID }), json!({ "case_id": "x", "step": 3 })] {
        let (status, _) = autorun_replay_with(&ctx(), &bad.to_string(), host, WAIT).await;
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
    let host = leak(FakeHost::ready());
    let (status, _) = autorun_replay_with(&off, &body(2), host, WAIT).await;
    assert_eq!(status, 200);
    assert_eq!(host.replays(), vec![ReplayRequest { case_id: ID, step: 2, db_read_access: false }]);
}

/// The route is an Auto Run one: gated with the others, and with no app
/// behind the bridge (a test, or before setup) it says so rather than
/// running anything.
#[tokio::test]
async fn the_route_is_gated_with_the_auto_run_routes() {
    assert!(module_guard_for("/autorun-replay", &Default::default()).is_some());
    assert!(module_guard_for("/autorun-replay", &v2_lib::modules::Module::ALL.into_iter().collect()).is_none());
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
    let ready = "Started from the beginning. Replayed case 77 to step 3 - the browser is on the page before step 3 runs";

    let host = leak(FakeHost { discovering: true, ..FakeHost::ready() });
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], format!("{ENDED_DISCOVERY_FIRST} {ready}"));
    assert!(v["page"].is_string(), "{v}");
    assert_eq!(host.replays().len(), 1, "the replay did not run");

    // A stop after the end still says the discovery is over.
    let stopped = ReplayEnd::Stopped { step: 2 };
    let host = leak(FakeHost { discovering: true, ..FakeHost::ending(stopped) });
    let (_, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], format!("{ENDED_DISCOVERY_FIRST} Started from the beginning. The replay was stopped at step 2: it was asked to stop (Stop, Close browser or Release Auto Run browser)"));

    let host = leak(FakeHost::ready());
    let (_, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], ready);
}

/// A replay that ended a no_save discovery says its guard stayed on for
/// this replay, and why, and that the next replay that may save is not
/// guarded.
#[tokio::test]
async fn the_replay_answer_says_a_no_save_discoverys_guard_stays_on() {
    use v2_lib::ai_bridge::DISCOVERY_GUARD_KEPT;
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    assert_eq!(
        DISCOVERY_GUARD_KEPT,
        "Its save guard stayed on for this replay, because the discovery was started without saving; your next replay of a case that may save is not guarded."
    );
    let ready = "Started from the beginning. Replayed case 77 to step 3 - the browser is on the page before step 3 runs";
    let host = leak(FakeHost { discovering: true, no_save: true, ..FakeHost::ready() });
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], format!("{ENDED_DISCOVERY_FIRST} {DISCOVERY_GUARD_KEPT} {ready}"));
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
    let host = leak(FakeHost { discovering: true, summary: Some(summary.clone()), ..FakeHost::ready() });
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["sentence"].as_str().unwrap().starts_with(ENDED_DISCOVERY_FIRST), "{v}");
    assert_eq!(v["summary"], serde_json::to_value(&summary).unwrap());
    assert!(v["page"].is_string(), "{v}");

    // A stop after the end carries it too.
    let stopped = ReplayEnd::Stopped { step: 2 };
    let host = leak(FakeHost { discovering: true, summary: Some(summary.clone()), ..FakeHost::ending(stopped) });
    let (_, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["summary"], serde_json::to_value(&summary).unwrap());

    let host = leak(FakeHost { discovering: true, ..FakeHost::ready() });
    let (_, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
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
        let host = leak(FakeHost { discovering, open_failed: Some(WOULD_NOT_OPEN.into()), ..FakeHost::ready() });
        let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
        assert_eq!((status, text), (503, said));
    }

    // The app's own replay says it the same way.
    let source = include_str!("../../src/commands/autorun.rs");
    let replay = &source[source.find("pub(crate) async fn replay_supervised").unwrap()..];
    let open = &replay[replay.find("open_if_none(app, &mut slot, by.opens_as())").unwrap()..];
    let open = &open[..open.len().min(300)];
    assert!(open.contains("after_ended_discovery(discovery.ended, &why)"), "{open}");
}

// ---- a replay that outlasts the call ----------------------------------------

/// A slow replay: one that runs until `host.release` is notified.
fn slow() -> &'static FakeHost {
    leak(FakeHost { release: Some(tokio::sync::Notify::new()), ..FakeHost::ready() })
}

/// The 202 a call answers while its replay still runs.
fn still_running_answer(text: &str) -> Value {
    let v: Value = serde_json::from_str(text).unwrap();
    assert_eq!(v["status"], "running", "{v}");
    v
}

/// The call times out, says where the replay is, and a second call for the
/// same case and step waits on that same replay and takes its answer: the
/// replay ran once, and is never refused as already running.
#[tokio::test]
async fn a_replay_past_the_wait_answers_still_running_and_a_second_call_attaches() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    assert_eq!(v2_lib::ai_bridge::REPLAY_ANSWER_WAIT, Duration::from_secs(45));
    let host = slow();
    let short = Duration::from_millis(50);
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, short).await;
    assert_eq!(status, 202, "{text}");
    let v = still_running_answer(&text);
    assert_eq!(
        v["sentence"],
        "still running, now at step 2 of 2 - call replay_autorun_to_step again with the same case (77) and step (3) to keep waiting"
    );
    assert_eq!((v["step"].as_i64(), v["of"].as_i64()), (Some(2), Some(2)));

    // Asked again: still running, still the same replay.
    let (status, _) = autorun_replay_with(&ctx(), &body(3), host, short).await;
    assert_eq!(status, 202);

    // Asked again while it ends: this call takes its answer.
    let (c, b) = (ctx(), body(3));
    let (answered, ()) = tokio::join!(autorun_replay_with(&c, &b, host, WAIT), async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        host.release.as_ref().unwrap().notify_one();
    });
    assert_eq!(answered.0, 200, "{}", answered.1);
    let v: Value = serde_json::from_str(&answered.1).unwrap();
    assert!(v["sentence"].as_str().unwrap().starts_with("Started from the beginning. Replayed case 77 to step 3"), "{v}");
    assert_eq!(host.replays().len(), 1, "the second call started another replay");
}

/// While the assistant's replay runs, a replay of another case or step is
/// refused, naming what is running.
#[tokio::test]
async fn a_replay_of_another_step_while_one_runs_is_refused() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let host = slow();
    let (status, _) = autorun_replay_with(&ctx(), &body(3), host, Duration::from_millis(30)).await;
    assert_eq!(status, 202);
    let other = leak(FakeHost::ready());
    let (status, text) = autorun_replay_with(&ctx(), &body(2), other, WAIT).await;
    assert_eq!((status, text.as_str()), (409, v2_lib::ai_bridge::replay_running_elsewhere(77, 3).as_str()));
    assert!(text.contains("case 77 to step 3"), "{text}");
    assert!(other.replays().is_empty(), "a second replay ran beside the first");

    // Let it end, and take its answer, so the slot is free again.
    host.release.as_ref().unwrap().notify_one();
    let (status, _) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!(status, 200);
    let (status, _) = autorun_replay_with(&ctx(), &body(2), other, WAIT).await;
    assert_eq!(status, 200, "the slot was not freed once the answer was taken");
}

/// A replay that ends while no call is waiting keeps its answer for the
/// next call with the same case and step, which gets it at once; it is
/// handed over once.
#[tokio::test]
async fn the_answer_of_a_replay_that_ended_unwatched_goes_to_the_next_call() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let host = slow();
    let (status, _) = autorun_replay_with(&ctx(), &body(3), host, Duration::from_millis(30)).await;
    assert_eq!(status, 202);
    host.release.as_ref().unwrap().notify_one();
    // Nobody waits while it ends.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (c, b) = (ctx(), body(3));
    let (status, text) = tokio::time::timeout(Duration::from_millis(500), autorun_replay_with(&c, &b, host, WAIT))
        .await
        .expect("the kept answer was not handed over at once");
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("Replayed case 77 to step 3"), "{text}");
    assert_eq!(host.replays().len(), 1);
}

// ---- the browser a replay leaves open ---------------------------------------

/// A replay that leaves the browser open says so on a line of its own:
/// what that blocks for its account, and how to free it - the release and
/// a discovery for the assistant's own browser, the person for theirs. A
/// refused replay says nothing of it.
#[tokio::test]
async fn a_replay_that_leaves_the_browser_open_says_what_it_blocks() {
    use v2_lib::autorun::replay_to::{left_open_line, LeftOpen};
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let mine = LeftOpen { account: Some("admin".into()), by_assistant: true };
    assert_eq!(
        left_open_line(&mine),
        "The Auto Run browser stays open on this step, signed in: discovery actions, component saves and template runs as admin are refused until start_autorun_discovery takes it over as it is. Do not release it between replays; release_autorun_browser is for a broken browser."
    );
    let theirs = LeftOpen { account: None, by_assistant: false };
    assert_eq!(
        left_open_line(&theirs),
        "The person's Auto Run browser stays open on this step: discovery actions and component saves are refused until the person closes it."
    );

    let host = leak(FakeHost { left_open: Some(mine.clone()), ..FakeHost::ready() });
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["sentence"],
        format!("Started from the beginning. Replayed case 77 to step 3 - the browser is on the page before step 3 runs\n{}", left_open_line(&mine))
    );

    let stopped = ReplayEnd::StoppedAt {
        phase: v2_lib::autorun::replay_to::ReplayPhase::Step,
        step: 2,
        why: "nothing matched #s2".into(),
        outcomes: vec![],
    };
    let host = leak(FakeHost { left_open: Some(theirs.clone()), ..FakeHost::ending(stopped) });
    let (_, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["sentence"].as_str().unwrap().ends_with(&left_open_line(&theirs)), "{v}");

    let host = leak(FakeHost { left_open: Some(mine), ..FakeHost::ending(ReplayEnd::Refused("no".into())) });
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!((status, text.as_str()), (409, "no"));
    // A replay that was stopped (Stop, Close, Release) says nothing of a
    // browser that may be closing behind the answer.
    let host = leak(FakeHost { left_open: Some(left_open_mine()), ..FakeHost::ending(ReplayEnd::Stopped { step: 2 }) });
    let (_, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(!v["sentence"].as_str().unwrap().contains("stays open"), "{v}");
}

fn left_open_mine() -> v2_lib::autorun::replay_to::LeftOpen {
    v2_lib::autorun::replay_to::LeftOpen { account: Some("admin".into()), by_assistant: true }
}

// ---- carrying on from where the last replay stood ---------------------------

/// The answer says where the replay started: carried on from step K, or
/// from the beginning; already standing before the step, it says so and
/// that nothing ran, with the page.
#[tokio::test]
async fn the_answer_says_where_the_replay_started() {
    use v2_lib::autorun::replay_to::ReplayFrom;
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));

    let host = leak(FakeHost { from: ReplayFrom::CarriedOn(2), ..FakeHost::ready() });
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["sentence"],
        "Carried on from step 2. Replayed case 77 to step 3 - the browser is on the page before step 3 runs"
    );

    let host = leak(FakeHost { from: ReplayFrom::AlreadyThere, ..FakeHost::ready() });
    let (status, text) = autorun_replay_with(&ctx(), &body(3), host, WAIT).await;
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sentence"], "The browser is already standing before step 3 of case 77: nothing ran.");
    assert!(v["page"].is_string(), "{v}");
}

// ---- an answer nobody took ---------------------------------------------------

/// A slow replay asked for once with a short wait, so it answers 202, then
/// let end while nobody waits.
async fn ended_unwatched(wait: Duration) -> &'static FakeHost {
    let host = slow();
    let (status, _) = autorun_replay_with(&ctx(), &body(3), host, wait).await;
    assert_eq!(status, 202);
    host.release.as_ref().unwrap().notify_one();
    tokio::time::sleep(Duration::from_millis(20)).await;
    host
}

/// An answer nobody took is handed over only while it is fresh: no older
/// than twice the call's wait (90 s with `REPLAY_ANSWER_WAIT`), the script
/// not saved since the replay started, and the browser not used since it
/// ended. Otherwise the old answer is dropped and a new replay starts.
#[tokio::test]
async fn an_answer_nobody_took_is_handed_over_only_while_fresh() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let short = Duration::from_millis(30);

    // Too old: past twice the wait.
    let first = ended_unwatched(short).await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    let again = leak(FakeHost::ready());
    let (status, _) = autorun_replay_with(&ctx(), &body(3), again, short).await;
    assert_eq!(status, 200);
    assert_eq!((first.replays().len(), again.replays().len()), (1, 1), "an old answer was handed over");

    // The script saved since the replay started.
    let first = ended_unwatched(short).await;
    let mut resaved = script(false);
    resaved.saved_at = Some("2026-10-13T00:00:00Z".into());
    store::save_script(dir.path(), &resaved).unwrap();
    let again = leak(FakeHost::ready());
    let (status, _) = autorun_replay_with(&ctx(), &body(3), again, WAIT).await;
    assert_eq!(status, 200);
    assert_eq!((first.replays().len(), again.replays().len()), (1, 1), "an answer from before a save was handed over");

    // The browser used since it ended.
    let first = ended_unwatched(short).await;
    v2_lib::commands::autorun::note_session_use();
    let again = leak(FakeHost::ready());
    let (status, _) = autorun_replay_with(&ctx(), &body(3), again, WAIT).await;
    assert_eq!(status, 200);
    assert_eq!((first.replays().len(), again.replays().len()), (1, 1), "an answer from before a use was handed over");

    // Fresh: handed over, and nothing new runs.
    let first = ended_unwatched(short).await;
    let again = leak(FakeHost::ready());
    let (status, _) = autorun_replay_with(&ctx(), &body(3), again, WAIT).await;
    assert_eq!(status, 200);
    assert_eq!((first.replays().len(), again.replays().len()), (1, 0));
}

/// While a replay holds the browser, a page read, a probe, a try and the
/// assistant's End discovery answer 409 at once rather than wait on it.
#[tokio::test]
async fn calls_that_need_the_browser_are_refused_at_once_while_a_replay_runs() {
    use v2_lib::ai_bridge::REPLAY_GOING;
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    let one = OneReplay::claim().unwrap();
    let probe = json!({ "selector": "#x" }).to_string();
    let try_body = json!({ "case_id": ID, "action": { "kind": "click", "selector": { "css": "#x" } } }).to_string();
    for (method, path, body) in [
        ("GET", "/autorun-page", String::new()),
        ("POST", "/autorun-probe", probe),
        ("POST", "/autorun-try", try_body),
        ("POST", "/autorun-discover-end", "{}".to_string()),
    ] {
        let answer = tokio::time::timeout(Duration::from_secs(2), route(&ctx(), None, method, path, &body, "test"))
            .await
            .unwrap_or_else(|_| panic!("{path} waited on the replay"));
        if path == "/autorun-page" {
            // A page read says what to call instead, and why it is not read.
            assert_eq!(answer, (409, v2_lib::ai_bridge::replay_going_for_a_page_read()), "{path}");
            assert!(answer.1.contains("call replay_autorun_to_step"), "{}", answer.1);
            assert!(answer.1.contains("hands back the page"), "{}", answer.1);
            continue;
        }
        assert_eq!(answer, (409, REPLAY_GOING.to_string()), "{path}");
    }
    drop(one);
    assert_eq!(REPLAY_GOING, "a replay is going - call replay_autorun_to_step with the same case and step to wait for it");
}

/// While a step runs, the 202 says which action it is on, for how long, and
/// how many of the step's actions already failed; a replay never retries.
/// Before any action has begun it says only the step, as before.
#[test]
fn still_running_names_the_action_and_the_failures_so_far() {
    use v2_lib::ai_bridge::replay_still_running_answer;
    use v2_lib::autorun::runner::ActionNow;
    let on = ActionNow {
        index: 2,
        of: 4,
        kind: "click".into(),
        target: "button \"Save\"".into(),
        since: Some(std::time::Instant::now() - Duration::from_secs(12)),
        failed: 1,
    };
    let v = replay_still_running_answer(77, 3, 2, 2, &on);
    assert_eq!(v["status"], "running");
    assert_eq!(
        v["sentence"],
        "still running, now at step 2 of 2, on action 2 of 4 (click button \"Save\") for 12 s, 1 of this step's actions already failed (a replay never tries an action again) - call replay_autorun_to_step again with the same case (77) and step (3) to keep waiting"
    );
    assert_eq!(v["action"]["index"], 2);
    assert_eq!(v["action"]["of"], 4);
    assert_eq!(v["action"]["kind"], "click");
    assert_eq!(v["action"]["seconds"], 12);
    assert_eq!(v["failed_in_step"], 1);
    assert_eq!(v["retries"], false);

    let none = replay_still_running_answer(77, 3, 0, 0, &ActionNow::default());
    assert_eq!(
        none["sentence"],
        "still running, now signing in and going to the case's area - call replay_autorun_to_step again with the same case (77) and step (3) to keep waiting"
    );
    assert!(none.get("action").is_none());
}
