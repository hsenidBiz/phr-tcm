//! An assistant's replay to a step (`POST /autorun-replay`) and the person's
//! Allow it waits for on a must-not-save script (`autorun::replay_ask`).
//!
//! The browser side is a fake host (`ReplayHost`): what the app shows, the
//! replay it runs and the page it reads are recorded, so these tests see
//! exactly when the replay runs - and that it never runs before Allow. The
//! engine itself is `autorun_replay_to`'s.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use v2_lib::ai_bridge::{autorun_guard_for, autorun_replay_with, route, BridgeContext, HostFuture, ReplayHost};
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::nav::{save_nav, ModulePath, NavFile};
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::replay_ask::{Asks, Notice, ALREADY_WAITING, DECLINED, NOT_WAITING, NO_ANSWER, WAIT};
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

/// What the app was told, what it ran and what the page said.
struct FakeHost {
    notices: Mutex<Vec<String>>,
    replays: Mutex<Vec<(String, String, ReplayRequest)>>,
    end: ReplayEnd,
}

impl FakeHost {
    fn ending(end: ReplayEnd) -> Self {
        FakeHost { notices: Mutex::new(vec![]), replays: Mutex::new(vec![]), end }
    }
    fn ready() -> Self {
        Self::ending(ReplayEnd::Ready { case_id: ID, step: 3, notice: None })
    }
    fn notices(&self) -> Vec<String> {
        self.notices.lock().unwrap().clone()
    }
    fn replays(&self) -> Vec<ReplayRequest> {
        self.replays.lock().unwrap().iter().map(|r| r.2.clone()).collect()
    }
}

impl ReplayHost for FakeHost {
    fn notify(&self, notice: Notice<'_>) {
        let line = match notice {
            Notice::Asked(a) => format!("asked {} {} ({}) step {}", a.id, a.case_id, a.title, a.step),
            Notice::Ended(id) => format!("ended {id}"),
        };
        self.notices.lock().unwrap().push(line);
    }
    fn replay(&self, organization: String, project: String, req: ReplayRequest) -> HostFuture<'_, Result<ReplayEnd, String>> {
        Box::pin(async move {
            self.replays.lock().unwrap().push((organization, project, req));
            Ok(self.end.clone())
        })
    }
    fn page(&self) -> HostFuture<'_, (u16, String)> {
        Box::pin(async { (200, "- button \"Submit\" [role=button name=\"Submit\"]".to_string()) })
    }
}

fn body(step: i32) -> String {
    json!({ "case_id": ID, "step": step }).to_string()
}

/// Wait until `asks` holds a request, and give back its id.
async fn waiting_id(asks: &Asks) -> String {
    for _ in 0..500 {
        if let Some(a) = asks.waiting() {
            return a.id;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("no request ever waited");
}

// ---- the registry -----------------------------------------------------------

#[test]
fn the_wait_is_two_minutes() {
    assert_eq!(WAIT, Duration::from_secs(120));
}

#[tokio::test]
async fn allow_answers_the_waiting_request_and_the_end_is_told() {
    let asks = Asks::new();
    let host = FakeHost::ready();
    let notify = |n: Notice<'_>| host.notify(n);
    let (asked, id) = tokio::join!(asks.ask(ID, "Leave request", 3, Duration::from_secs(5), &notify), async {
        let id = waiting_id(&asks).await;
        asks.answer(&id, true).unwrap();
        id
    });
    assert_eq!(asked, Ok(()));
    assert_eq!(host.notices(), vec![format!("asked {id} 77 (Leave request) step 3"), format!("ended {id}")]);
    assert!(asks.waiting().is_none());
}

#[tokio::test]
async fn deny_is_the_declined_sentence() {
    let asks = Asks::new();
    let (asked, _) = tokio::join!(asks.ask(ID, "Leave request", 3, Duration::from_secs(5), &|_| {}), async {
        let id = waiting_id(&asks).await;
        asks.answer(&id, false).unwrap();
    });
    assert_eq!(asked.unwrap_err(), DECLINED);
    assert_eq!(DECLINED, "the person declined the replay");
}

#[tokio::test]
async fn no_answer_in_time_is_the_timeout_sentence_and_a_late_allow_is_refused() {
    let asks = Asks::new();
    let host = FakeHost::ready();
    let notify = |n: Notice<'_>| host.notify(n);
    let (asked, id) = tokio::join!(asks.ask(ID, "Leave request", 3, Duration::from_millis(60), &notify), waiting_id(&asks));
    assert_eq!(asked.unwrap_err(), NO_ANSWER);
    assert_eq!(NO_ANSWER, "the person did not answer within 2 minutes");
    assert!(asks.waiting().is_none(), "an expired request still waits");
    assert_eq!(host.notices().last().cloned(), Some(format!("ended {id}")), "the modal is told it expired");
    assert_eq!(asks.answer(&id, true).unwrap_err(), NOT_WAITING);
    assert_eq!(NOT_WAITING, "that replay request is no longer waiting");
}

#[tokio::test]
async fn a_second_request_while_one_waits_is_refused() {
    let asks = Asks::new();
    let (first, second) = tokio::join!(asks.ask(ID, "Leave request", 3, Duration::from_secs(5), &|_| {}), async {
        let id = waiting_id(&asks).await;
        let second = asks.ask(78, "Other", 2, Duration::from_secs(5), &|_| panic!("the second was shown")).await;
        assert_eq!(asks.waiting().map(|a| a.id), Some(id.clone()), "the first still waits");
        asks.answer(&id, true).unwrap();
        second
    });
    assert_eq!(first, Ok(()));
    assert_eq!(second.unwrap_err(), ALREADY_WAITING);
    assert_eq!(ALREADY_WAITING, "a replay request is already waiting for the person");
}

#[test]
fn an_unknown_id_is_not_waiting() {
    let asks = Asks::new();
    assert_eq!(asks.answer("nothing", true).unwrap_err(), NOT_WAITING);
}

#[tokio::test]
async fn a_request_whose_caller_went_away_stops_waiting() {
    let asks = Asks::new();
    let host = FakeHost::ready();
    let notify = |n: Notice<'_>| host.notify(n);
    let gone = tokio::time::timeout(
        Duration::from_millis(50),
        asks.ask(ID, "Leave request", 3, Duration::from_secs(5), &notify),
    )
    .await;
    assert!(gone.is_err(), "the ask should still have been waiting");
    assert!(asks.waiting().is_none(), "the dropped request still waits");
    assert!(host.notices().last().unwrap().starts_with("ended "), "{:?}", host.notices());
}

// ---- the route ----------------------------------------------------------------

#[tokio::test]
async fn a_must_not_save_script_waits_for_allow_before_anything_runs() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(true));
    let (asks, host) = (Asks::new(), FakeHost::ready());

    let (c, b) = (ctx(), body(3));
    let ((status, text), ()) = tokio::join!(
        autorun_replay_with(&c, &b, &host, &asks, Duration::from_secs(5)),
        async {
            let id = waiting_id(&asks).await;
            // Shown, and nothing opened, signed in or ran.
            assert_eq!(host.notices(), vec![format!("asked {id} 77 (Leave request) step 3")]);
            assert!(host.replays().is_empty(), "the replay ran before Allow");
            asks.answer(&id, true).unwrap();
        }
    );
    assert_eq!(status, 200, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["sentence"],
        "replayed case 77 to step 3 - the browser is on the page before step 3 runs"
    );
    assert_eq!(v["page"], "- button \"Submit\" [role=button name=\"Submit\"]");
    assert_eq!(host.replays(), vec![ReplayRequest { case_id: ID, step: 3, db_read_access: true }]);
    let replays = host.replays.lock().unwrap();
    assert_eq!((replays[0].0.as_str(), replays[0].1.as_str()), ("acme", "Web"));
}

#[tokio::test]
async fn deny_refuses_and_nothing_runs() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(true));
    let (asks, host) = (Asks::new(), FakeHost::ready());
    let (c, b) = (ctx(), body(3));
    let ((status, text), ()) = tokio::join!(
        autorun_replay_with(&c, &b, &host, &asks, Duration::from_secs(5)),
        async {
            let id = waiting_id(&asks).await;
            asks.answer(&id, false).unwrap();
        }
    );
    assert_eq!((status, text.as_str()), (409, DECLINED));
    assert!(host.replays().is_empty());
}

#[tokio::test]
async fn no_answer_refuses_with_the_timeout_sentence() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(true));
    let (asks, host) = (Asks::new(), FakeHost::ready());
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host, &asks, Duration::from_millis(60)).await;
    assert_eq!((status, text.as_str()), (409, NO_ANSWER));
    assert!(host.replays().is_empty());
    assert!(asks.waiting().is_none());
}

#[tokio::test]
async fn a_second_request_while_one_waits_is_refused_by_the_route() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(true));
    let (asks, host) = (Asks::new(), FakeHost::ready());
    let (c, b) = (ctx(), body(3));
    let (first, second) = tokio::join!(
        autorun_replay_with(&c, &b, &host, &asks, Duration::from_secs(5)),
        async {
            let id = waiting_id(&asks).await;
            let second = autorun_replay_with(&ctx(), &body(2), &host, &asks, Duration::from_secs(5)).await;
            asks.answer(&id, true).unwrap();
            second
        }
    );
    assert_eq!(first.0, 200, "{}", first.1);
    assert_eq!((second.0, second.1.as_str()), (409, ALREADY_WAITING));
    assert_eq!(host.replays().len(), 1, "only the allowed request ran");
}

#[tokio::test]
async fn a_normal_script_runs_without_asking() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let (asks, host) = (Asks::new(), FakeHost::ready());
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host, &asks, Duration::from_secs(5)).await;
    assert_eq!(status, 200, "{text}");
    assert!(host.notices().is_empty(), "the person was asked: {:?}", host.notices());
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
        (ReplayEnd::Stopped { step: 2 }, "the replay was stopped at step 2"),
    ] {
        let (asks, host) = (Asks::new(), FakeHost::ending(end));
        let (status, text) = autorun_replay_with(&ctx(), &body(3), &host, &asks, Duration::from_secs(5)).await;
        assert_eq!(status, 200, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v, json!({ "sentence": sentence }));
    }
    // A refusal the engine itself gives is the usual refusal.
    let (asks, host) = (Asks::new(), FakeHost::ending(ReplayEnd::Refused(ALREADY_RUNNING.into())));
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host, &asks, Duration::from_secs(5)).await;
    assert_eq!((status, text.as_str()), (409, ALREADY_RUNNING));
}

#[tokio::test]
async fn refusals_before_running_never_ask_or_run() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());

    // No saved script.
    let (asks, host) = (Asks::new(), FakeHost::ready());
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host, &asks, Duration::from_secs(5)).await;
    assert_eq!((status, text.as_str()), (409, "case 77 has no saved script"));

    // A step outside the script, on a must-not-save one: refused before
    // the person is asked.
    project(dir.path(), &script(true));
    let (status, text) = autorun_replay_with(&ctx(), &body(5), &host, &asks, Duration::from_secs(5)).await;
    assert_eq!((status, text.as_str()), (409, "step 5 is not in case 77's script (it has steps 1 to 3)"));

    // A replay already running.
    let one = OneReplay::claim().unwrap();
    let (status, text) = autorun_replay_with(&ctx(), &body(3), &host, &asks, Duration::from_secs(5)).await;
    assert_eq!((status, text.as_str()), (409, ALREADY_RUNNING));
    drop(one);

    // A body without its fields.
    for bad in [json!({ "step": 3 }), json!({ "case_id": ID }), json!({ "case_id": "x", "step": 3 })] {
        let (status, _) = autorun_replay_with(&ctx(), &bad.to_string(), &host, &asks, Duration::from_secs(5)).await;
        assert_eq!(status, 400, "{bad}");
    }

    assert!(host.notices().is_empty(), "{:?}", host.notices());
    assert!(host.replays().is_empty());
}

#[tokio::test]
async fn database_read_access_follows_the_db_query_switch() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    project(dir.path(), &script(false));
    let off = BridgeContext { disabled_tools: vec!["db_lookup".into(), "db_query".into()], ..ctx() };
    let (asks, host) = (Asks::new(), FakeHost::ready());
    let (status, _) = autorun_replay_with(&off, &body(2), &host, &asks, Duration::from_secs(5)).await;
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
