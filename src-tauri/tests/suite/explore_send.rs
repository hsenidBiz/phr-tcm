//! `send_api_request`: one GET or POST from an explore session's own page,
//! in the account it signed in as. The page is `api_templates_runner`'s
//! `App` (the sign-in fake that also answers a token read and a request),
//! held by a `DiscoveryBrowser` of this file's own.

use crate::api_templates_runner::{answer, rig, App, Rig, ORG, PAGE, PROJECT, QUICK_PAUSES};
use crate::autorun_discovery::exploring;
use crate::common::quick;
use serde_json::{json, Value};
use std::collections::VecDeque;
use v2_lib::ai_bridge::{route, send_in, BridgeContext, DiscoveryBrowser, DiscoveryParts, API_WRITES_OFF};
use v2_lib::api_templates::one_off::{
    delete_reminder, edit_reminder, not_a_capture, CREATED_NEEDED, EFFECT_NEEDED, GET_ONLY_READS, NO_SESSION_FOR_SEND,
    POST_NEEDS_EXPLORE, SESSION_NOT_SIGNED_IN,
};
use v2_lib::api_templates::runner::RUN_LIMIT;
use v2_lib::autorun::explore::{begin, load, Change, ExploreRun};
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::store::set_root;
use v2_lib::autorun::test_made;
use v2_lib::browser::save_guard;
use v2_lib::commands::autorun::{DiscoveryState, GuardOwner};

const PATH: &str = "/hr/pmsv10/performancecycle";
const DELETE_YES: &str = "Tell the person what will be deleted and wait for their yes in the chat first.";

/// The explore browser: the rig's page, the account's lease and the
/// discovery under way.
struct PageBrowser {
    d: App,
    lease: Held,
    account: Option<String>,
    discovery: Option<DiscoveryState>,
    guarded_case: Option<i32>,
    guard_owner: Option<GuardOwner>,
}

impl DiscoveryBrowser for PageBrowser {
    type D = App;
    fn parts(&mut self) -> DiscoveryParts<'_, App> {
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
    fn close(self) {}
}

/// Every process-wide thing a send touches, in the order the suite takes
/// them: the template slot, the activity log, the held browsers, the leases.
fn locks() -> impl Sized {
    (
        crate::serial::api_template_run(),
        crate::serial::activity_log(),
        crate::serial::held_browsers(),
        crate::serial::account_leases(),
    )
}

fn ctx(api_writes: bool) -> BridgeContext {
    BridgeContext { org: ORG.into(), project: PROJECT.into(), api_writes, ..BridgeContext::default() }
}

/// The browser is on a page of the application, for as many token reads as
/// a test makes.
fn on_the_app(r: &Rig) {
    let href = format!("https://hr.example.internal{PAGE}");
    r.script.lock().unwrap().hrefs = VecDeque::from(vec![href; 6]);
}

/// A browser holding `admin` in `r`'s root, in `discovery`.
async fn holding(r: &mut Rig, discovery: Option<DiscoveryState>) -> Option<PageBrowser> {
    let mut lease = Held::supervised();
    lease.hold(r.root.path(), "admin").await.unwrap();
    on_the_app(r);
    Some(PageBrowser {
        d: r.browsers.next.take().unwrap(),
        lease,
        account: Some("admin".into()),
        discovery,
        guarded_case: None,
        guard_owner: None,
    })
}

/// A going explore session, its change list made. Hands back the browser
/// and the session's id.
async fn exploring_session(r: &mut Rig) -> (Option<PageBrowser>, String) {
    let id = begin(r.root.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let state = exploring("Leave").map(|s| DiscoveryState {
        explore: Some(ExploreRun { session: Some(id.clone()) }),
        ..s
    });
    (holding(r, state).await, id)
}

async fn send(browser: &mut Option<PageBrowser>, r: &Rig, writes: bool, body: Value) -> (u16, Value) {
    let (status, text) =
        send_in(browser, &ctx(writes), r.root.path(), &body.to_string(), &quick(), RUN_LIMIT, &QUICK_PAUSES).await;
    let parsed = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, parsed)
}

fn create_cycle() -> Value {
    json!({
        "step": { "method": "POST", "path": PATH, "form": { "CycleName": "AUTOTEST cycle" },
                  "capture": { "cycleId": "$.cycleId" } },
        "effect": "create",
        "created": { "kind": "cycle", "id": "cycleId", "name": "AUTOTEST cycle" }
    })
}

fn post_to(path: &str, effect: &str) -> Value {
    json!({ "step": { "method": "POST", "path": path, "form": { "x": "1" } }, "effect": effect })
}

#[tokio::test]
async fn send_needs_your_own_session() {
    let _serial = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    set_root(dir.path().to_path_buf());
    let body = json!({ "step": { "method": "GET", "path": "/hr/x" } }).to_string();
    let (status, text) = route(&ctx(true), None, "POST", "/api-template-send", &body, "1.0.0").await;
    assert_eq!((status, text.as_str()), (409, NO_SESSION_FOR_SEND));

    // And a slot that holds no discovery says the same.
    let mut nobody: Option<PageBrowser> = None;
    let out = send_in(&mut nobody, &ctx(true), dir.path(), &body, &quick(), RUN_LIMIT, &QUICK_PAUSES).await;
    assert_eq!(out, (409, NO_SESSION_FOR_SEND.to_string()));
}

#[tokio::test]
async fn a_post_needs_an_effect_the_switch_and_for_a_delete_the_persons_yes() {
    // Every one of these is refused before the slot is looked at.
    let dir = tempfile::tempdir().unwrap();
    let ask = |writes: bool, body: Value| {
        let (text, ctx, root) = (body.to_string(), ctx(writes), dir.path().to_path_buf());
        async move {
            let mut none: Option<PageBrowser> = None;
            send_in(&mut none, &ctx, &root, &text, &quick(), RUN_LIMIT, &QUICK_PAUSES).await
        }
    };

    let no_effect = json!({ "step": { "method": "POST", "path": PATH, "form": { "x": "1" } } });
    assert_eq!(ask(true, no_effect).await, (400, EFFECT_NEEDED.to_string()));

    assert_eq!(ask(false, create_cycle()).await, (400, API_WRITES_OFF.to_string()));

    assert_eq!(ask(true, post_to("/hr/x", "delete")).await, (400, DELETE_YES.to_string()));
    let mut asked_nobody = post_to("/hr/x", "delete");
    asked_nobody["person_said_yes"] = json!(false);
    assert_eq!(ask(true, asked_nobody).await, (400, DELETE_YES.to_string()));

    let get_edit = json!({ "step": { "method": "GET", "path": "/hr/x" }, "effect": "edit" });
    assert_eq!(ask(true, get_edit).await, (400, GET_ONLY_READS.to_string()));

    let mut wrong_id = create_cycle();
    wrong_id["created"]["id"] = json!("nope");
    assert_eq!(ask(true, wrong_id).await, (400, not_a_capture("nope")));

    let mut no_created = create_cycle();
    no_created.as_object_mut().unwrap().remove("created");
    assert_eq!(ask(true, no_created).await, (400, CREATED_NEEDED.to_string()));
}

#[tokio::test]
async fn a_create_runs_in_the_session_and_goes_on_the_change_list() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "cycleId": 274 }))], None);
    let (mut browser, session) = exploring_session(&mut r).await;

    let (status, v) = send(&mut browser, &r, true, create_cycle()).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["captured"]["cycleId"], json!(274), "{v}");
    assert_eq!(v["change"], json!(1), "{v}");
    assert_eq!(v["request"], format!("POST {PATH}"), "{v}");
    assert_eq!(r.navigated(), "", "the page was navigated");
    assert_eq!(browser.as_ref().unwrap().d.inner.calls_to("Page.navigate").len(), 0);
    assert_eq!(r.sign_ins(), 0);

    let s = load(r.root.path(), &session).unwrap();
    assert_eq!(s.changes.len(), 1, "{:?}", s.changes);
    assert!(
        matches!(&s.changes[0].change, Change::Created { kind, id, name, verified: true }
            if kind == "cycle" && id == "274" && name == "AUTOTEST cycle"),
        "{:?}",
        s.changes[0]
    );
    assert_eq!(s.changes[0].request.as_deref(), Some(format!("POST {PATH}").as_str()));
    let made = test_made::list(r.root.path());
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0].run_id, session);
    assert_eq!((made[0].kind.as_str(), made[0].id.as_str()), ("cycle", "274"));
    assert_eq!(s.writes.len(), 1, "{:?}", s.writes);
    assert_eq!((s.writes[0].method.as_str(), s.writes[0].path.as_str()), ("POST", PATH));
}

#[tokio::test]
async fn a_post_in_a_no_save_session_is_refused_with_the_guards_sentence() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "cycleId": 274 }))], None);
    let (mut browser, session) = exploring_session(&mut r).await;
    browser.as_mut().unwrap().discovery.as_mut().unwrap().no_save = true;

    let (status, text) =
        send_in(&mut browser, &ctx(true), r.root.path(), &create_cycle().to_string(), &quick(), RUN_LIMIT, &QUICK_PAUSES)
            .await;
    assert_eq!((status, text), (409, save_guard::blocked("POST", PATH)));
    assert!(r.fetched().is_empty(), "something was sent");
    assert!(load(r.root.path(), &session).unwrap().writes.is_empty());
}

#[tokio::test]
async fn a_post_outside_an_explore_session_is_refused_and_a_get_runs() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "found": true }))], None);
    let mut browser = holding(&mut r, exploring("Leave")).await;

    let (status, text) =
        send_in(&mut browser, &ctx(true), r.root.path(), &create_cycle().to_string(), &quick(), RUN_LIMIT, &QUICK_PAUSES)
            .await;
    assert_eq!((status, text.as_str()), (409, POST_NEEDS_EXPLORE));
    assert!(r.fetched().is_empty(), "something was sent");

    // A GET needs neither the switch nor an explore session.
    let get = json!({ "step": { "method": "GET", "path": "/hr/x", "query": { "id": "5" } } });
    let (status, v) = send(&mut browser, &r, false, get).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["status"], json!(200));
    assert_eq!(v["request"], "GET /hr/x");
    assert!(v.get("change").is_none() && v.get("record_change").is_none(), "{v}");
    assert_eq!(r.fetched().len(), 1);
}

#[tokio::test]
async fn an_edit_and_a_delete_remind_what_to_record() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "ok": true })), answer(200, json!({ "ok": true }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;

    let (status, v) = send(&mut browser, &r, true, post_to("/hr/x", "edit")).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["record_change"], json!(edit_reminder("POST /hr/x")), "{v}");
    assert!(v.get("change").is_none(), "{v}");

    let mut delete = post_to("/hr/x", "delete");
    delete["person_said_yes"] = json!(true);
    let (status, v) = send(&mut browser, &r, true, delete).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["record_change"], json!(delete_reminder("POST /hr/x")), "{v}");
}

#[tokio::test]
async fn the_answer_is_redacted_and_says_when_it_was_cut() {
    let _locks = locks();
    let long = json!({
        "status": 200, "contentType": "text/plain", "finalUrl": "https://hr.example.internal/hr/x",
        "redirected": false, "text": "a".repeat(65_536),
    });
    let mut r = rig(vec![answer(200, json!({ "password": "hunter2", "name": "n" })), long], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let get = json!({ "step": { "method": "GET", "path": "/hr/x" } });

    let (status, v) = send(&mut browser, &r, false, get.clone()).await;
    assert_eq!(status, 200, "{v}");
    let body = v["response"]["body"].as_str().unwrap();
    assert!(!body.contains("hunter2"), "{body}");
    assert!(body.contains("\"name\""), "{body}");
    assert_eq!(v["response"]["cut"], false, "{v}");
    assert_eq!(v["response"]["content_type"], "application/json; charset=utf-8", "{v}");

    let (_, v) = send(&mut browser, &r, false, get).await;
    assert_eq!(v["response"]["cut"], true, "{}", v["response"]["cut"]);
}

#[tokio::test]
async fn a_failed_call_says_where_and_still_answers() {
    let _locks = locks();
    let mut r = rig(vec![answer(500, json!({ "error": "boom" }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let get = json!({ "step": { "method": "GET", "path": "/hr/x" } });

    let (status, v) = send(&mut browser, &r, false, get).await;
    assert_eq!(status, 502, "{v}");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["status"], json!(500), "{v}");
    assert_eq!(v["failed"], "Request", "{v}");
    assert!(v["detail"].as_str().unwrap().contains("failed at Request"), "{v}");
}

#[tokio::test]
async fn a_session_that_no_longer_holds_its_account_sends_nothing() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "cycleId": 274 }))], None);
    let (mut browser, session) = exploring_session(&mut r).await;
    browser.as_mut().unwrap().lease.let_go();

    let (status, text) =
        send_in(&mut browser, &ctx(true), r.root.path(), &create_cycle().to_string(), &quick(), RUN_LIMIT, &QUICK_PAUSES)
            .await;
    assert_eq!((status, text.as_str()), (409, SESSION_NOT_SIGNED_IN));
    assert!(r.fetched().is_empty(), "something was sent");
    assert!(load(r.root.path(), &session).unwrap().changes.is_empty());
}

/// A create goes on the change list from `send_in` alone: the id the
/// response gave needs no check against the page's saves, so nothing else
/// may claim it did.
#[test]
fn only_send_records_a_create_by_itself() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut calls = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap().replace("\r\n", "\n");
                let name = p.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
                let lines: Vec<&str> = text.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    if !line.contains("record_send_create(") || line.contains("fn record_send_create(") {
                        continue;
                    }
                    // The function this line is in: the nearest line above it
                    // that starts a function at the margin.
                    let owner = lines[..i]
                        .iter()
                        .rev()
                        .find(|l| {
                            let t = l.trim_start_matches("pub ").trim_start_matches("async ");
                            t.starts_with("fn ") && !l.starts_with(' ')
                        })
                        .copied()
                        .unwrap_or("");
                    calls.push((name.clone(), owner.to_string()));
                }
            }
        }
    }
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "ai_bridge.rs");
    assert!(calls[0].1.contains("fn send_in<"), "{calls:?}");
}
