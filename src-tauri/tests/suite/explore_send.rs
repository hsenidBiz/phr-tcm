//! `send_api_request`: one GET or POST from an explore session's own page,
//! in the account it signed in as. The page is `api_templates_runner`'s
//! `App` (the sign-in fake that also answers a token read and a request),
//! held by a `DiscoveryBrowser` of this file's own.

use crate::api_templates_runner::{answer, page_reply, rig, App, Rig, ORG, PAGE, PROJECT, QUICK_PAUSES};
use crate::autorun_discovery::exploring;
use crate::common::quick;
use serde_json::{json, Value};
use std::collections::VecDeque;
use v2_lib::ai_bridge::{
    route, run_saved_in, send_in, BridgeContext, DiscoveryBrowser, DiscoveryParts, API_WRITES_OFF, RAN_IN_YOUR_SESSION,
    RUN_REMINDER,
};
use v2_lib::api_templates::one_off::{
    delete_reminder, edit_reminder, not_a_capture, CREATED_NEEDED, EFFECT_NEEDED, GET_ONLY_READS, NO_SESSION_FOR_SEND,
    POST_NEEDS_EXPLORE, SESSION_NOT_SIGNED_IN,
};
use v2_lib::api_templates::one_off::{DELETES_NOT_SAVED, EXPLORE_MODULE, NO_TOKEN_NOT_SAVED, SECRET_NOT_SAVED};
use v2_lib::api_templates::runner::RUN_LIMIT;
use v2_lib::api_templates::store;
use v2_lib::api_templates::{Effect, EXPLORE_PROOF};
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
    // A capture with nothing in it: the page has shown and sent nothing yet.
    let mut app = r.browsers.next.take().unwrap();
    app.inner.capture = Some(v2_lib::browser::net_capture::NetCapture::default());
    Some(PageBrowser {
        d: app,
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
        explore: Some(ExploreRun { session: Some(id.clone()), ..ExploreRun::default() }),
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

/// The create the saving tests send: a save of the cycle setup.
fn save_progress() -> Value {
    json!({
        "step": { "method": "POST", "path": "/hr/pmsv10/PerformanceCycle", "query": { "handler": "SaveProgress" },
                  "form": { "CycleName": "AUTOTEST cycle" }, "capture": { "cycleId": "$.cycleId" } },
        "effect": "create",
        "created": { "kind": "cycle", "id": "cycleId", "name": "AUTOTEST cycle" }
    })
}

fn saved_ids(r: &Rig) -> Vec<String> {
    store::list(r.root.path(), ORG, PROJECT).unwrap().into_iter().map(|s| s.template.id).collect()
}

#[tokio::test]
async fn a_working_call_is_saved_once_and_matched_after() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "cycleId": 274 })), answer(200, json!({ "cycleId": 275 }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let id = "post-hr-pmsv10-performancecycle-saveprogress";

    let (status, v) = send(&mut browser, &r, true, save_progress()).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"], json!({ "id": id, "saved": true }), "{v}");
    let t = store::load(r.root.path(), ORG, PROJECT, id).unwrap().expect("the template was saved");
    let proven = t.proven.clone().expect("proven");
    assert_eq!(proven.via.as_deref(), Some(EXPLORE_PROOF));
    assert_eq!(proven.account, "admin");
    assert_eq!(t.antiforgery.page, PAGE);
    assert_eq!(t.effect, Effect::Create);
    assert_eq!(t.module, EXPLORE_MODULE);
    assert_eq!(t.title, "POST /hr/pmsv10/PerformanceCycle (SaveProgress)");
    assert_eq!(t.steps.len(), 1);
    let runs = store::list(r.root.path(), ORG, PROJECT).unwrap().remove(0).runs;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!((runs[0].mode.as_str(), runs[0].ok), ("explore", true));
    let before = saved_ids(&r).len();

    on_the_app(&r);
    let (status, v) = send(&mut browser, &r, true, save_progress()).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"], json!({ "id": id, "reused": true }), "{v}");
    assert_eq!(saved_ids(&r).len(), before);
}

#[tokio::test]
async fn a_get_is_saved_as_a_read_template() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "cycles": [ { "id": 7 } ] }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let get = json!({ "step": { "method": "GET", "path": "/hr/pmsv10/cycles", "capture": { "first": "$.cycles[0].id" } } });

    let (status, v) = send(&mut browser, &r, false, get).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"], json!({ "id": "get-hr-pmsv10-cycles", "saved": true }), "{v}");
    let t = store::load(r.root.path(), ORG, PROJECT, "get-hr-pmsv10-cycles").unwrap().unwrap();
    assert_eq!(t.effect, Effect::Read);
    assert_eq!(t.outputs, vec!["first".to_string()]);
    assert_eq!(t.proven.unwrap().outputs["first"], json!(7));
}

#[tokio::test]
async fn a_saved_call_expects_the_status_it_answered() {
    let _locks = locks();
    let mut r = rig(vec![answer(201, json!({ "cycleId": 274 }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;

    let (status, v) = send(&mut browser, &r, true, save_progress()).await;
    assert_eq!(status, 200, "{v}");
    let id = v["template"]["id"].as_str().unwrap_or_default().to_string();
    let t = store::load(r.root.path(), ORG, PROJECT, &id).unwrap().expect("saved");
    assert_eq!(t.steps[0].expect.status, 201);
}

#[tokio::test]
async fn a_call_from_a_page_with_no_token_is_not_saved() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "found": true }))], None);
    r.script.lock().unwrap().token = None;
    let (mut browser, _) = exploring_session(&mut r).await;
    let get = json!({ "step": { "method": "GET", "path": "/hr/x" } });

    let (status, v) = send(&mut browser, &r, false, get).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["template"]["not_saved"], NO_TOKEN_NOT_SAVED, "{v}");
    assert!(saved_ids(&r).is_empty());
}

#[tokio::test]
async fn a_failed_call_and_a_delete_call_are_never_saved() {
    let _locks = locks();
    let mut r = rig(vec![answer(400, json!({ "error": "no" })), answer(200, json!({ "ok": true }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;

    let (status, v) = send(&mut browser, &r, true, save_progress()).await;
    assert_eq!(status, 502, "{v}");
    assert!(v.get("template").is_none(), "{v}");
    assert!(saved_ids(&r).is_empty());

    on_the_app(&r);
    let mut delete = post_to("/hr/x", "delete");
    delete["person_said_yes"] = json!(true);
    let (status, v) = send(&mut browser, &r, true, delete).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"]["not_saved"], DELETES_NOT_SAVED, "{v}");
    assert!(saved_ids(&r).is_empty());
}

#[tokio::test]
async fn a_call_that_sends_a_secret_as_written_is_not_saved() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "cycleId": 274 })), answer(200, json!({ "cycleId": 275 }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;

    let mut written = save_progress();
    written["step"]["form"]["Password"] = json!("hunter2");
    let (status, v) = send(&mut browser, &r, true, written).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"]["not_saved"], SECRET_NOT_SAVED, "{v}");
    assert!(saved_ids(&r).is_empty());
    assert!(!v.to_string().contains("hunter2"), "{v}");

    // The same call with the secret sent as a param is saved, holding the placeholder.
    on_the_app(&r);
    let mut as_param = save_progress();
    as_param["step"]["form"]["Password"] = json!("{{pw}}");
    as_param["params"] = json!([ { "name": "pw", "type": "string", "required": true } ]);
    as_param["values"] = json!({ "pw": "hunter2" });
    let (status, v) = send(&mut browser, &r, true, as_param).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"]["saved"], true, "{v}");
    let saved = std::fs::read_dir(store::templates_dir(r.root.path(), ORG, PROJECT))
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect::<String>();
    assert!(saved.contains("{{pw}}") && !saved.contains("hunter2"), "{saved}");
}

#[tokio::test]
async fn the_assistants_id_and_name_are_used_and_made_unique() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "cycleId": 274 })), answer(200, json!({ "ok": true }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;

    let mut first = save_progress();
    first["id"] = json!("make-cycle");
    let (_, v) = send(&mut browser, &r, true, first).await;
    assert_eq!(v["template"], json!({ "id": "make-cycle", "saved": true }), "{v}");

    on_the_app(&r);
    let mut second = post_to("/hr/pmsv10/other", "edit");
    second["id"] = json!("make-cycle");
    second["name"] = json!("Make a cycle");
    second["module"] = json!("PMS / Cycles");
    second["description"] = json!("Makes one.");
    let (_, v) = send(&mut browser, &r, true, second).await;
    assert_eq!(v["template"], json!({ "id": "make-cycle-2", "saved": true }), "{v}");
    let t = store::load(r.root.path(), ORG, PROJECT, "make-cycle-2").unwrap().unwrap();
    assert_eq!(
        (t.title.as_str(), t.module.as_str(), t.description.as_str()),
        ("Make a cycle", "PMS / Cycles", "Makes one.")
    );
    assert_eq!(t.effect, Effect::Edit);
    assert!(t.sources[0].starts_with("explore session "), "{:?}", t.sources);
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

/// A saved, proven copy of the runner suite's template, and the run request
/// for it as `account`.
fn saved_run(r: &Rig, account: &str, stage: bool) -> v2_lib::api_templates::runner::RunRequest {
    use v2_lib::api_templates::runner::Mode;
    use v2_lib::api_templates::Proven;
    let mut t = crate::api_templates_runner::template();
    t.proven = Some(Proven {
        at: "2026-09-01 09:00:00".into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: std::collections::BTreeMap::from([("cycleId".to_string(), json!(1))]),
        environment: None,
        via: None,
    });
    if stage {
        t.stage = Some(v2_lib::api_templates::flow::StageRef { flow: "a-flow".into(), id: "a-stage".into() });
    }
    store::save(r.root.path(), ORG, PROJECT, &t).unwrap();
    let mut req = crate::api_templates_runner::request(t, Mode::Run);
    req.account = account.into();
    req
}

async fn run_saved(
    browser: &mut Option<PageBrowser>,
    r: &Rig,
    req: &v2_lib::api_templates::runner::RunRequest,
) -> Option<(u16, String)> {
    run_saved_in(browser, &ctx(true), r.root.path(), req, &quick(), RUN_LIMIT, &QUICK_PAUSES).await
}

#[tokio::test]
async fn a_saved_template_runs_in_your_explore_session_when_it_holds_the_account() {
    let _locks = locks();
    let both = answer(200, json!({ "success": true, "cycleId": 274 }));
    let mut r = rig(vec![both.clone(), both], None);
    let (mut browser, session) = exploring_session(&mut r).await;
    let req = saved_run(&r, "admin", false);

    let (status, text) = run_saved(&mut browser, &r, &req).await.expect("the session ran it");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["ran_in"], RAN_IN_YOUR_SESSION, "{v}");
    assert_eq!(v["record_change"], RUN_REMINDER, "{v}");
    assert_eq!(v["outputs"]["cycleId"], json!(274), "{v}");

    let d = &browser.as_ref().unwrap().d;
    let navigations = d.inner.calls_to("Page.navigate");
    assert_eq!(navigations.len(), 1, "{navigations:?}");
    assert!(navigations[0]["url"].as_str().unwrap_or("").ends_with(PAGE), "{navigations:?}");
    assert_eq!(r.sign_ins(), 0);

    let runs = store::list(r.root.path(), ORG, PROJECT).unwrap().remove(0).runs;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].mode, "run");
    assert!(runs[0].ok);

    let s = load(r.root.path(), &session).unwrap();
    let posted: Vec<(&str, &str)> = s.writes.iter().map(|w| (w.method.as_str(), w.path.as_str())).collect();
    // Each save is noted with its handler: it is part of which save it is.
    assert_eq!(
        posted,
        vec![
            ("POST", "/hr/pmsv10/performancecycle?handler=SaveProgress"),
            ("POST", "/hr/pmsv10/performancecycle?handler=SaveEvalRulesProgress")
        ],
        "{:?}",
        s.writes
    );
}

#[tokio::test]
async fn another_account_or_a_flow_template_is_left_to_the_usual_run() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    let (mut browser, session) = exploring_session(&mut r).await;

    let other = saved_run(&r, "other", false);
    assert_eq!(run_saved(&mut browser, &r, &other).await, None);

    let on_a_flow = saved_run(&r, "admin", true);
    assert_eq!(run_saved(&mut browser, &r, &on_a_flow).await, None);

    // Nor a slot with no explore session in it.
    let plain = saved_run(&r, "admin", false);
    let mut nobody: Option<PageBrowser> = None;
    assert_eq!(run_saved(&mut nobody, &r, &plain).await, None);
    // Nor a discovery that is not an explore session.
    browser.as_mut().unwrap().discovery.as_mut().unwrap().explore = None;
    assert_eq!(run_saved(&mut browser, &r, &plain).await, None);

    assert!(r.fetched().is_empty(), "something was sent");
    assert!(load(r.root.path(), &session).unwrap().writes.is_empty());
}

/// A saved template run inside an explore session notes a postback that may
/// have saved (it timed out after the post went out), the same as a one-off.
#[tokio::test]
async fn a_saved_template_postback_that_may_have_saved_is_noted() {
    use v2_lib::api_templates::runner::Mode;
    use v2_lib::api_templates::Proven;
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().postbacks = VecDeque::from(vec![json!({ "error": "timeout", "posted": true })]);
    let (mut browser, session) = exploring_session(&mut r).await;
    let mut t = crate::api_postback_runner::template(vec![crate::api_postback_runner::step(
        "Save",
        EVAL_PAGE,
        json!({}),
    )]);
    t.proven = Some(Proven {
        at: "2026-09-01 09:00:00".into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: std::collections::BTreeMap::new(),
        environment: None,
        via: None,
    });
    store::save(r.root.path(), ORG, PROJECT, &t).unwrap();
    let mut req = crate::api_templates_runner::request(t, Mode::Run);
    req.values.clear();

    let (_, text) = run_saved(&mut browser, &r, &req).await.expect("the session ran it");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(posted(&r).len(), 1, "a postback was sent again: {v}");
    let s = load(r.root.path(), &session).unwrap();
    assert_eq!(s.writes.len(), 1, "a postback that may have saved was not noted: {:?} {v}", s.writes);
    assert_eq!(s.writes[0].method, "POST");
    assert!(v["record_change"].is_string(), "{v}");
}

#[tokio::test]
async fn a_post_template_in_a_no_save_session_is_refused() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    let (mut browser, session) = exploring_session(&mut r).await;
    browser.as_mut().unwrap().discovery.as_mut().unwrap().no_save = true;
    let req = saved_run(&r, "admin", false);

    let answered = run_saved(&mut browser, &r, &req).await;
    assert_eq!(answered, Some((409, save_guard::blocked("POST", "/hr/pmsv10/performancecycle"))));
    assert!(r.fetched().is_empty(), "something was sent");
    assert_eq!(browser.as_ref().unwrap().d.inner.calls_to("Page.navigate").len(), 0);
    assert!(store::list(r.root.path(), ORG, PROJECT).unwrap().remove(0).runs.is_empty());
    assert!(load(r.root.path(), &session).unwrap().writes.is_empty());
}

// ---- The final fix wave ----

fn created_change(kind: &str, id: &str, name: &str) -> Change {
    Change::Created { kind: kind.into(), id: id.into(), name: name.into(), verified: false }
}

/// C1: a create the call made is verified, and so deletable by an undo, only
/// when the app can show its id is new to the session.
#[tokio::test]
async fn a_send_create_is_verified_only_when_its_id_is_new_to_the_session() {
    use v2_lib::autorun::explore::NOT_VERIFIED;
    let _locks = locks();
    let mut r = rig(
        vec![
            answer(200, json!({ "cycleId": 274 })),
            answer(200, json!({ "cycleId": 275 })),
            answer(200, json!({ "cycleId": 276 })),
            answer(200, json!({ "cycleId": 277 })),
        ],
        None,
    );
    let (mut browser, session) = exploring_session(&mut r).await;

    // Nothing earlier carried the id: verified, on the test-made list.
    let (_, v) = send(&mut browser, &r, true, create_cycle()).await;
    assert_eq!((v["change"].clone(), v["verified"].clone()), (json!(1), json!(true)), "{v}");
    assert!(v.get("note").is_none(), "{v}");
    assert_eq!(test_made::list(r.root.path()).len(), 1);

    // The call itself sends the id it names: that is an edit, not a create.
    on_the_app(&r);
    let mut sends_it = create_cycle();
    sends_it["step"]["form"]["ParentId"] = json!("275");
    let (_, v) = send(&mut browser, &r, true, sends_it).await;
    assert_eq!(v["verified"], json!(false), "{v}");
    assert_eq!(v["note"], NOT_VERIFIED, "{v}");
    assert_eq!(v["change"], json!(2), "{v}");

    // An earlier change on the list already names the id.
    on_the_app(&r);
    v2_lib::autorun::explore::record_change(
        r.root.path(),
        &session,
        created_change("cycle", "276", "Seen before"),
        None,
        &[],
        0,
    )
    .unwrap();
    let (_, v) = send(&mut browser, &r, true, create_cycle()).await;
    assert_eq!(v["verified"], json!(false), "{v}");

    // No capture to prove it with: kept on the list, not verified.
    on_the_app(&r);
    browser.as_mut().unwrap().d.inner.capture = None;
    let (_, v) = send(&mut browser, &r, true, create_cycle()).await;
    assert_eq!(v["verified"], json!(false), "{v}");
    assert_eq!(v["note"], NOT_VERIFIED, "{v}");

    // Only the first reached the test-made list.
    let made = test_made::list(r.root.path());
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0].id, "274");
    let s = load(r.root.path(), &session).unwrap();
    let verified: Vec<bool> = s
        .changes
        .iter()
        .filter_map(|c| match &c.change {
            Change::Created { verified, .. } => Some(*verified),
            _ => None,
        })
        .collect();
    // The first send, the one that sent its own id, the one recorded by hand, the one an earlier change named, the one with no capture.
    assert_eq!(verified, vec![true, false, false, false, false]);
}

/// I1: every POST that is sent is noted as a save, whatever its effect, with
/// its handler.
#[tokio::test]
async fn every_post_that_is_sent_is_noted_as_a_save_with_its_handler() {
    use v2_lib::autorun::explore::unaccounted;
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "found": true }))], None);
    let (mut browser, session) = exploring_session(&mut r).await;

    let search = json!({
        "step": { "method": "POST", "path": PATH, "query": { "handler": "Search" }, "form": { "q": "x" } },
        "effect": "read"
    });
    let (status, v) = send(&mut browser, &r, true, search).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["request"], format!("POST {PATH}?handler=Search"), "{v}");
    let s = load(r.root.path(), &session).unwrap();
    assert_eq!(s.writes.len(), 1, "{:?}", s.writes);
    assert_eq!(s.writes[0].path, format!("{PATH}?handler=Search"));
    assert_eq!(unaccounted(&s), vec![format!("POST {PATH}?handler=Search")]);
}

/// I3: a secret in the query, or in the token page's query, runs but is not saved.
#[tokio::test]
async fn a_secret_in_the_query_or_the_token_page_is_not_saved() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "ok": true })); 3], None);
    let (mut browser, _) = exploring_session(&mut r).await;

    let by_name = json!({ "step": { "method": "GET", "path": "/hr/x", "query": { "apiKey": "abc123" } } });
    let (status, v) = send(&mut browser, &r, false, by_name).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["template"]["not_saved"], SECRET_NOT_SAVED, "{v}");
    assert!(!v.to_string().contains("abc123"), "{v}");

    // A token-shaped value under an innocent name.
    on_the_app(&r);
    let opaque = "Zk3qT9vLm2Xw8pRt5BnYc7DfGh1JsKe4UoIa6MlPq0WxCz";
    let by_value = json!({ "step": { "method": "GET", "path": "/hr/x", "query": { "q": opaque } } });
    let (_, v) = send(&mut browser, &r, false, by_value).await;
    assert_eq!(v["template"]["not_saved"], SECRET_NOT_SAVED, "{v}");

    // The token page's own query: the page the browser stood on, with it.
    on_the_app(&r);
    r.script.lock().unwrap().hrefs =
        VecDeque::from(vec![format!("https://hr.example.internal{PAGE}?access_token=abc123xyz"); 6]);
    let page = json!({
        "step": { "method": "GET", "path": "/hr/x", "query": { "id": "5" } },
        "antiforgery_page": format!("{PAGE}?access_token=abc123xyz")
    });
    let (status, v) = send(&mut browser, &r, false, page).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"]["not_saved"], SECRET_NOT_SAVED, "{v}");
    assert!(!v.to_string().contains("abc123xyz"), "{v}");
    assert!(saved_ids(&r).is_empty());
}

/// I4: a template that never worked is not a match.
#[tokio::test]
async fn an_unproven_template_is_never_reused() {
    use v2_lib::api_templates::one_off::{as_template, parse};
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "cycleId": 274 }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;

    let one = parse(&save_progress().to_string()).unwrap();
    let mut draft = as_template(&one, "my-draft", PAGE);
    draft.proven = None;
    store::save(r.root.path(), ORG, PROJECT, &draft).unwrap();

    let (status, v) = send(&mut browser, &r, true, save_progress()).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"]["saved"], json!(true), "{v}");
    assert_ne!(v["template"]["id"], json!("my-draft"), "{v}");
    assert!(v["template"].get("reused").is_none(), "{v}");
    assert_eq!(saved_ids(&r).len(), 2);
}

/// I4: a path segment that is a record id becomes a param, so one template
/// serves every record.
#[tokio::test]
async fn a_record_id_in_the_path_becomes_one_template_for_every_record() {
    let _locks = locks();
    let mut r = rig(
        vec![
            answer(200, json!({ "name": "A" })),
            answer(200, json!({ "name": "B" })),
            answer(200, json!({ "name": "C" })),
        ],
        None,
    );
    let (mut browser, _) = exploring_session(&mut r).await;

    let get = |path: &str| json!({ "step": { "method": "GET", "path": path } });
    let (_, v) = send(&mut browser, &r, false, get("/hr/employees/101")).await;
    assert_eq!(v["template"], json!({ "id": "get-hr-employees-id", "saved": true }), "{v}");
    let t = store::load(r.root.path(), ORG, PROJECT, "get-hr-employees-id").unwrap().unwrap();
    assert_eq!(t.steps[0].path, "/hr/employees/{{id}}");
    assert_eq!(t.params.len(), 1);
    assert_eq!(t.params[0].name, "id");
    assert!(t.params[0].required);

    on_the_app(&r);
    let (_, v) = send(&mut browser, &r, false, get("/hr/employees/102")).await;
    assert_eq!(v["template"], json!({ "id": "get-hr-employees-id", "reused": true }), "{v}");

    // A GUID segment is an id too.
    on_the_app(&r);
    let (_, v) = send(&mut browser, &r, false, get("/hr/employees/6f1c2d3e-4a5b-4c6d-8e7f-90a1b2c3d4e5")).await;
    assert_eq!(v["template"], json!({ "id": "get-hr-employees-id", "reused": true }), "{v}");
    assert_eq!(saved_ids(&r), vec!["get-hr-employees-id".to_string()]);
}

/// M6: a POST that says it only reads is saved as a read only while the
/// change list shows it changed nothing.
#[tokio::test]
async fn a_post_that_says_read_is_saved_as_read_unless_the_list_shows_it_changed_something() {
    use v2_lib::autorun::explore::record_change;
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "ok": true })), answer(200, json!({ "ok": true }))], None);
    let (mut browser, session) = exploring_session(&mut r).await;
    let call = |handler: &str| {
        json!({
            "step": { "method": "POST", "path": PATH, "query": { "handler": handler }, "form": { "q": "x" } },
            "effect": "read"
        })
    };

    let (_, v) = send(&mut browser, &r, true, call("Find")).await;
    let t = store::load(r.root.path(), ORG, PROJECT, v["template"]["id"].as_str().unwrap()).unwrap().unwrap();
    assert_eq!(t.effect, Effect::Read);

    // The session already recorded a change for this very save.
    let changed = Change::Changed {
        record: "Cycle".into(),
        field: "Status".into(),
        before: None,
        after: Some(json!("Published")),
        hidden: false,
    };
    record_change(r.root.path(), &session, changed, Some(format!("POST {PATH}?handler=Publish")), &[], 0).unwrap();
    on_the_app(&r);
    let (_, v) = send(&mut browser, &r, true, call("Publish")).await;
    let t = store::load(r.root.path(), ORG, PROJECT, v["template"]["id"].as_str().unwrap()).unwrap().unwrap();
    assert_eq!(t.effect, Effect::Edit);
}

/// M2: a GET from a plain discovery runs, and says why it is not saved.
#[tokio::test]
async fn a_get_from_a_plain_discovery_says_it_is_saved_only_from_an_explore_session() {
    use v2_lib::api_templates::one_off::PLAIN_DISCOVERY_NOT_SAVED;
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "found": true }))], None);
    let mut browser = holding(&mut r, exploring("Leave")).await;
    let get = json!({ "step": { "method": "GET", "path": "/hr/x" } });
    let (status, v) = send(&mut browser, &r, false, get).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"], json!({ "not_saved": PLAIN_DISCOVERY_NOT_SAVED }), "{v}");
    assert_eq!(PLAIN_DISCOVERY_NOT_SAVED, "saved only from an explore session");
    assert!(saved_ids(&r).is_empty());
}

/// I2: the guide says what the code requires on `created`, in the code's words.
#[test]
fn the_guide_says_what_created_needs() {
    let guide = v2_lib::api_templates::guide::text(&[], None);
    let flat = guide.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("A create always names what it made, in `created`"), "{flat}");
    assert!(flat.contains("`id`, which is the NAME of one of the step's `capture`s"), "{flat}");
    assert!(flat.contains("A create whose answer carries no id cannot be recorded here"), "{flat}");
    assert!(!flat.contains("unless the answer carries the id"), "{flat}");
    // The code's own refusal says the same thing.
    assert!(CREATED_NEEDED.contains("with id the name of one of the step's captures"), "{CREATED_NEEDED}");
}

const EVAL_PAGE: &str = "/Perf/Eval.aspx";

fn postback_call(extra: Value) -> Value {
    let mut pb = json!({ "page": EVAL_PAGE, "button": "btnSave" });
    for (k, v) in extra.as_object().expect("an object") {
        pb[k] = v.clone();
    }
    json!({ "step": { "postback": pb }, "effect": "edit" })
}

fn posted(r: &Rig) -> Vec<Vec<Value>> {
    r.script.lock().unwrap().posted.clone()
}

#[tokio::test]
async fn a_postback_one_off_carries_on_from_the_last_one_and_is_not_saved() {
    use v2_lib::api_templates::one_off::{CARRIED_ON, LOADED_FRESH};
    use v2_lib::api_templates::postback::POSTBACK_NOT_SAVED;
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().postbacks = VecDeque::from(vec![
        page_reply(200, json!({ "carried": false })),
        page_reply(200, json!({ "carried": true })),
    ]);
    let (mut browser, _session) = exploring_session(&mut r).await;

    let (status, first) = send(&mut browser, &r, true, postback_call(json!({}))).await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["postback"], json!(LOADED_FRESH), "{first}");
    assert_eq!(first["request"], format!("POST {EVAL_PAGE}"), "{first}");
    assert_eq!(first["template"], json!({ "not_saved": POSTBACK_NOT_SAVED }), "{first}");

    on_the_app(&r);
    let (status, second) = send(&mut browser, &r, true, postback_call(json!({ "argument": "7" }))).await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(second["postback"], json!(CARRIED_ON), "{second}");
    assert_eq!(second["template"], json!({ "not_saved": POSTBACK_NOT_SAVED }), "{second}");

    let sent = posted(&r);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[0][3], json!("if_there"));
    assert_eq!(sent[1][3], json!("if_there"));
    assert_eq!(sent[0][2], json!("one-off"));
    assert_eq!(sent[1][2], json!("one-off"));
    assert!(saved_ids(&r).is_empty(), "a postback one-off was saved");
    assert_eq!(r.navigated(), "", "the page was navigated");

    // Neither answer names a host or carries page state.
    for v in [&first, &second] {
        let text = v.to_string();
        assert!(!text.contains("example.internal"), "{text}");
        assert!(!text.contains("__VIEWSTATE"), "{text}");
    }
}

#[tokio::test]
async fn fresh_true_loads_the_page_again() {
    use v2_lib::api_templates::one_off::LOADED_FRESH;
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().postbacks = VecDeque::from(vec![page_reply(200, json!({ "carried": false }))]);
    let (mut browser, _) = exploring_session(&mut r).await;
    let (status, v) = send(&mut browser, &r, true, postback_call(json!({ "fresh": true }))).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["postback"], json!(LOADED_FRESH), "{v}");
    assert_eq!(posted(&r)[0][3], json!("fresh"));
    assert_eq!(r.navigated(), "", "a fresh load is an in-page fetch, not a navigation");
}

#[tokio::test]
async fn a_postback_one_off_runs_on_a_page_with_no_token() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().token = None;
    r.script.lock().unwrap().postbacks = VecDeque::from(vec![page_reply(200, json!({}))]);
    let (mut browser, _) = exploring_session(&mut r).await;
    let (status, v) = send(&mut browser, &r, true, postback_call(json!({}))).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(r.navigated(), "", "a one-off postback navigated");
    assert_eq!(browser.as_ref().unwrap().d.inner.calls_to("Page.navigate").len(), 0);
}

#[tokio::test]
async fn a_postback_one_off_is_noted_as_a_save() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().postbacks = VecDeque::from(vec![page_reply(200, json!({}))]);
    let (mut browser, session) = exploring_session(&mut r).await;
    let (status, v) = send(&mut browser, &r, true, postback_call(json!({}))).await;
    assert_eq!(status, 200, "{v}");
    let s = load(r.root.path(), &session).unwrap();
    assert_eq!(s.writes.len(), 1, "{:?}", s.writes);
    assert_eq!((s.writes[0].method.as_str(), s.writes[0].path.as_str()), ("POST", EVAL_PAGE));
}

#[tokio::test]
async fn a_failed_postback_one_off_is_not_retried_and_not_saved() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().postbacks =
        VecDeque::from(vec![page_reply(400, json!({ "empty": true, "excerpt": "" })), page_reply(200, json!({}))]);
    let (mut browser, _) = exploring_session(&mut r).await;
    let (status, v) = send(&mut browser, &r, true, postback_call(json!({}))).await;
    assert_eq!(status, 502, "{v}");
    assert_eq!(posted(&r).len(), 1, "a postback one-off was sent again");
    assert!(v["template"].is_null(), "{v}");
}

#[tokio::test]
async fn a_postback_one_off_that_may_have_saved_is_noted_and_reminds_to_record() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().postbacks = VecDeque::from(vec![json!({ "error": "timeout", "posted": true })]);
    let (mut browser, session) = exploring_session(&mut r).await;
    let (status, v) = send(&mut browser, &r, true, postback_call(json!({}))).await;
    assert_eq!(status, 502, "{v}");
    assert_eq!(posted(&r).len(), 1, "a postback one-off was sent again");
    let s = load(r.root.path(), &session).unwrap();
    assert_eq!(s.writes.len(), 1, "a postback that may have saved was not noted: {:?}", s.writes);
    assert_eq!((s.writes[0].method.as_str(), s.writes[0].path.as_str()), ("POST", EVAL_PAGE));
    assert!(v["record_change"].is_string(), "no reminder to record the change: {v}");
}

#[tokio::test]
async fn a_postback_one_off_that_never_went_out_is_not_noted() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().postbacks = VecDeque::from(vec![json!({ "error": "timeout" })]);
    let (mut browser, session) = exploring_session(&mut r).await;
    let (status, v) = send(&mut browser, &r, true, postback_call(json!({}))).await;
    assert_eq!(status, 502, "{v}");
    assert!(load(r.root.path(), &session).unwrap().writes.is_empty());
    assert!(v["record_change"].is_null(), "{v}");
}

fn post_form(path: &str, form: Value) -> Value {
    json!({ "step": { "method": "POST", "path": path, "form": form }, "effect": "edit" })
}

#[tokio::test]
async fn save_false_sends_but_saves_and_matches_nothing() {
    use v2_lib::api_templates::one_off::NOT_SAVED_ASKED;
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "ok": true })), answer(200, json!({ "ok": true }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;

    let mut asked = post_form("/hr/x", json!({ "x": "1" }));
    asked["save"] = json!(false);
    let (status, v) = send(&mut browser, &r, true, asked).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"], json!({ "not_saved": NOT_SAVED_ASKED }), "{v}");
    assert_eq!(r.fetched().len(), 1, "the call was sent");
    assert!(saved_ids(&r).is_empty());

    // The same call without save: false is saved as a new template, not matched to anything.
    on_the_app(&r);
    let (_, v) = send(&mut browser, &r, true, post_form("/hr/x", json!({ "x": "1" }))).await;
    assert_eq!(v["template"]["saved"], json!(true), "{v}");
    assert_eq!(saved_ids(&r).len(), 1);
}

#[tokio::test]
async fn a_later_working_call_replaces_the_failing_auto_save() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "id": -1 })), answer(200, json!({ "id": 5 }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let id = "post-hr-x";

    let (_, v) = send(&mut browser, &r, true, post_form("/hr/x", json!({ "x": "1" }))).await;
    assert_eq!(v["template"], json!({ "id": id, "saved": true }), "{v}");

    on_the_app(&r);
    let (status, v) = send(&mut browser, &r, true, post_form("/hr/x", json!({ "y": "2", "z": "3" }))).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["template"], json!({ "id": id, "replaced": true }), "{v}");
    assert_eq!(saved_ids(&r), vec![id.to_string()], "no -2 copy");
    let t = store::load(r.root.path(), ORG, PROJECT, id).unwrap().unwrap();
    let form = t.steps[0].form.as_ref().expect("a form");
    assert!(form.contains_key("y") && !form.contains_key("x"), "{form:?}");
    assert!(t.proven.is_some());
}

#[tokio::test]
async fn a_template_from_another_session_or_in_use_is_never_replaced() {
    let _locks = locks();
    let mut r = rig(
        vec![answer(200, json!({ "ok": true })), answer(200, json!({ "ok": true })), answer(200, json!({ "ok": true }))],
        None,
    );
    let (mut browser, _) = exploring_session(&mut r).await;
    let (_, v) = send(&mut browser, &r, true, post_form("/hr/x", json!({ "x": "1" }))).await;
    assert_eq!(v["template"]["id"], json!("post-hr-x"), "{v}");

    // A later session: nothing was auto-saved by it yet.
    browser.as_mut().unwrap().discovery.as_mut().unwrap().explore.as_mut().unwrap().auto_saved.clear();
    on_the_app(&r);
    let (_, v) = send(&mut browser, &r, true, post_form("/hr/x", json!({ "y": "2" }))).await;
    assert_eq!(v["template"], json!({ "id": "post-hr-x-2", "saved": true }), "{v}");

    // This session saved post-hr-x-2, but a saved fixture names it now.
    let dir = v2_lib::api_templates::fixture_store::fixtures_dir(r.root.path(), ORG, PROJECT);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("uses-it.json"),
        json!({ "id": "uses-it", "name": "Uses it", "account": "admin", "steps": [ { "template": "post-hr-x-2" } ] }).to_string(),
    )
    .unwrap();
    on_the_app(&r);
    let (_, v) = send(&mut browser, &r, true, post_form("/hr/x", json!({ "w": "3" }))).await;
    assert_eq!(v["template"], json!({ "id": "post-hr-x-3", "saved": true }), "{v}");
    assert_eq!(saved_ids(&r).len(), 3);
}

#[tokio::test]
async fn a_timed_out_post_is_noted_and_may_have_been_processed() {
    use v2_lib::api_templates::runner::SENT_NO_ANSWER;
    let _locks = locks();
    let mut r = rig(vec![json!({ "error": "timeout" })], None);
    let (mut browser, session) = exploring_session(&mut r).await;

    let mut edit = post_form("/hr/x", json!({ "x": "1" }));
    edit["timeout_s"] = json!(60);
    let (status, v) = send(&mut browser, &r, true, edit).await;
    assert_eq!(status, 502, "{v}");
    assert!(v["detail"].as_str().unwrap().contains(SENT_NO_ANSWER), "{v}");
    assert!(v["detail"].as_str().unwrap().contains("60 seconds"), "{v}");
    assert_eq!(v["timed_out"], json!(true), "{v}");
    assert_eq!(r.fetched()[0][2], json!(60_000), "the page was told 60 seconds");
    let s = load(r.root.path(), &session).unwrap();
    assert_eq!(s.writes.len(), 1, "{:?}", s.writes);
    assert_eq!(s.writes[0].path, "/hr/x");
    assert_eq!(v["record_change"], json!(edit_reminder("POST /hr/x")), "{v}");
}

#[tokio::test]
async fn a_devtools_timeout_says_it_may_not_know() {
    use v2_lib::api_templates::runner::MAYBE_SENT_NO_ANSWER;
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().fetch_gives_up = true;
    let (mut browser, session) = exploring_session(&mut r).await;

    // The browser stops answering for the longest the wait allows.
    let mut edit = post_form("/hr/x", json!({ "x": "1" }));
    edit["timeout_s"] = json!(5);
    let (status, v) = send(&mut browser, &r, true, edit).await;
    assert_eq!(status, 502, "{v}");
    assert!(v["detail"].as_str().unwrap().contains(MAYBE_SENT_NO_ANSWER), "{v}");
    assert_eq!(v["timed_out"], json!(true), "{v}");
    assert_eq!(load(r.root.path(), &session).unwrap().writes.len(), 1);
}

#[tokio::test]
async fn a_template_proven_another_way_or_named_by_a_flow_is_never_replaced() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "ok": true })); 3], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let (_, v) = send(&mut browser, &r, true, post_form("/hr/x", json!({ "x": "1" }))).await;
    assert_eq!(v["template"]["id"], json!("post-hr-x"), "{v}");

    // A prove of the same template afterwards: no longer proven by an explore call.
    let mut t = store::load(r.root.path(), ORG, PROJECT, "post-hr-x").unwrap().unwrap();
    t.proven.as_mut().unwrap().via = None;
    store::save(r.root.path(), ORG, PROJECT, &t).unwrap();
    on_the_app(&r);
    let (_, v) = send(&mut browser, &r, true, post_form("/hr/x", json!({ "y": "2" }))).await;
    assert_eq!(v["template"], json!({ "id": "post-hr-x-2", "saved": true }), "{v}");

    // A flow that names the newer template keeps it too.
    let dir = v2_lib::api_templates::flow_store::flows_dir(r.root.path(), ORG, PROJECT);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("f.json"),
        json!({ "id": "f", "title": "F", "module": "M", "subject": { "name": "cycleId", "type": "number" }, "stages": [ { "id": "post-hr-x-2", "title": "S", "check": "x" } ] }).to_string(),
    )
    .unwrap();
    on_the_app(&r);
    let (_, v) = send(&mut browser, &r, true, post_form("/hr/x", json!({ "w": "3" }))).await;
    assert_eq!(v["template"]["saved"], json!(true), "{v}");
    assert_eq!(saved_ids(&r).len(), 3);
}

// ---------------------------------------------------------------------------
// Kept session values: `keep` and `{{var.name}}` (item 82)
// ---------------------------------------------------------------------------

/// A 480-character value, as long as a real session token.
fn long_value() -> String {
    format!("kv-{}", "x9Q".repeat(159))
}

fn keep_token_call() -> Value {
    json!({ "step": { "method": "GET", "path": "/hr/x" }, "keep": { "token": "$.Ref" } })
}

fn post_with_var(json_body: Value) -> Value {
    json!({ "step": { "method": "POST", "path": "/hr/y", "json": json_body }, "effect": "edit" })
}

#[tokio::test]
async fn keep_stores_a_value_and_answers_only_its_length() {
    let _locks = locks();
    let value = long_value();
    let mut r = rig(vec![answer(200, json!({ "Ref": value }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;

    let (status, v) = send(&mut browser, &r, true, keep_token_call()).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["kept"], json!(["token (480 characters)"]), "{v}");
    assert!(!v.to_string().contains(&value), "the value came back: {v}");
    assert!(v["response"]["body"].as_str().unwrap().contains("{{var.token}}"), "{v}");
    let held = &browser.as_ref().unwrap().discovery.as_ref().unwrap().kept;
    assert_eq!(held.get("token"), Some(&json!(value)), "it is held for the discovery");
}

#[tokio::test]
async fn a_later_call_sends_the_kept_value() {
    use v2_lib::api_templates::one_off::VAR_NOT_SAVED;
    let _locks = locks();
    let value = long_value();
    let mut r = rig(vec![answer(200, json!({ "Ref": value })), answer(200, json!({ "echo": value }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    send(&mut browser, &r, true, keep_token_call()).await;

    on_the_app(&r);
    let (status, v) = send(&mut browser, &r, true, post_with_var(json!({ "t": "{{var.token}}" }))).await;
    assert_eq!(status, 200, "{v}");
    let sent = &r.fetched()[1][0]["body"];
    assert_eq!(sent["value"], json!({ "t": value }), "the page received the real value: {sent}");
    assert_eq!(v["template"], json!({ "not_saved": VAR_NOT_SAVED }), "{v}");
    assert!(!v.to_string().contains(&value), "the echo came back: {v}");
    assert!(v["response"]["body"].as_str().unwrap().contains("{{var.token}}"), "{v}");
}

#[tokio::test]
async fn a_kept_value_in_values_still_saves_the_template() {
    let _locks = locks();
    let value = long_value();
    let mut r = rig(vec![answer(200, json!({ "Ref": value })), answer(200, json!({ "ok": true }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    send(&mut browser, &r, true, keep_token_call()).await;

    on_the_app(&r);
    let mut call = post_with_var(json!({ "t": "{{p}}" }));
    call["params"] = json!([{ "name": "p", "type": "string", "required": true }]);
    call["values"] = json!({ "p": "{{var.token}}" });
    let (status, v) = send(&mut browser, &r, true, call).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(r.fetched()[1][0]["body"]["value"], json!({ "t": value }));
    assert_eq!(v["template"]["saved"], json!(true), "{v}");
    let id = v["template"]["id"].as_str().unwrap();
    let t = store::load(r.root.path(), ORG, PROJECT, id).unwrap().unwrap();
    let text = serde_json::to_string(&t).unwrap();
    assert!(!text.contains(&value), "a saved template never holds a kept value: {text}");
    assert!(text.contains("{{p}}"), "{text}");
}

#[tokio::test]
async fn a_var_not_kept_is_refused_naming_it() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let (status, v) = send(&mut browser, &r, true, post_with_var(json!({ "t": "{{var.x}}" }))).await;
    assert_eq!(status, 400, "{v}");
    assert_eq!(
        v,
        json!("{{var.x}} is not kept in this discovery - keep it first with keep on send_api_request")
    );
    assert!(r.fetched().is_empty(), "nothing was sent");

    let mut call = post_with_var(json!({ "t": "{{p}}" }));
    call["params"] = json!([{ "name": "p", "type": "string", "required": true }]);
    call["values"] = json!({ "p": "{{var.y}}" });
    let (status, v) = send(&mut browser, &r, true, call).await;
    assert_eq!(status, 400, "{v}");
    assert!(v.as_str().unwrap().starts_with("{{var.y}} is not kept"), "{v}");
}

#[tokio::test]
async fn a_keep_that_finds_nothing_says_so_and_keeps_the_rest() {
    let _locks = locks();
    let value = long_value();
    let mut r = rig(vec![answer(200, json!({ "Ref": value }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let mut call = keep_token_call();
    call["keep"] = json!({ "token": "$.Ref", "gone": "$.Nope" });
    let (status, v) = send(&mut browser, &r, true, call).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["kept"], json!(["token (480 characters)"]), "{v}");
    assert_eq!(v["keep_failed"], json!(["gone: found nothing at $.Nope"]), "{v}");
}

#[tokio::test]
async fn kept_values_end_with_the_discovery() {
    let _locks = locks();
    let value = long_value();
    let mut r = rig(vec![answer(200, json!({ "Ref": value }))], None);
    let (mut browser, id) = exploring_session(&mut r).await;
    send(&mut browser, &r, true, keep_token_call()).await;
    assert!(!browser.as_ref().unwrap().discovery.as_ref().unwrap().kept.is_empty());

    let fresh = exploring("Leave").map(|s| DiscoveryState {
        explore: Some(ExploreRun { session: Some(id), ..ExploreRun::default() }),
        ..s
    });
    assert!(fresh.as_ref().unwrap().kept.is_empty());
    browser.as_mut().unwrap().discovery = fresh;
    on_the_app(&r);
    let (status, v) = send(&mut browser, &r, true, post_with_var(json!({ "t": "{{var.token}}" }))).await;
    assert_eq!(status, 400, "{v}");
    assert!(v.as_str().unwrap().contains("{{var.token}} is not kept"), "{v}");
}

#[tokio::test]
async fn a_kept_value_is_never_in_the_answer_or_the_log() {
    let _log = crate::serial::log_tail();
    let _locks = locks();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());
    let value = long_value();
    let mut r = rig(
        vec![
            answer(200, json!({ "Ref": value })),
            answer(200, json!({ "Ref": value, "other": "fine" })),
            answer(500, json!({ "error": format!("bad token {value}") })),
            answer(200, json!({ "Ref": value })),
        ],
        None,
    );
    let (mut browser, _) = exploring_session(&mut r).await;
    let mut texts = vec![];

    let (_, v) = send(&mut browser, &r, true, keep_token_call()).await;
    texts.push(v.to_string());

    on_the_app(&r);
    let call = json!({ "step": { "method": "GET", "path": "/hr/x", "capture": { "again": "$.Ref" } }, "save": false });
    let (status, v) = send(&mut browser, &r, true, call).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["captured"]["again"], json!("{{var.token}}"), "{v}");
    texts.push(v.to_string());

    on_the_app(&r);
    let (status, v) = send(&mut browser, &r, true, post_with_var(json!({ "t": "{{var.token}}" }))).await;
    assert_eq!(status, 502, "{v}");
    assert!(v["detail"].as_str().unwrap().contains("{{var.token}}"), "{v}");
    texts.push(v.to_string());

    on_the_app(&r);
    let mut call = keep_token_call();
    call["step"]["expect"] = json!({ "status": 201 });
    let (status, v) = send(&mut browser, &r, true, call).await;
    assert_eq!(status, 502, "{v}");
    texts.push(v.to_string());

    for text in &texts {
        assert!(!text.contains(&value), "the value is in an answer: {text}");
    }
    let log: String = v2_lib::applog::recent(500).iter().map(|l| l.message.clone()).collect::<Vec<_>>().join("\n");
    assert!(!log.contains(&value), "the value is in the log");
    let records = crate::common::activity_records(activity.path(), "api");
    assert!(!records.is_empty());
    assert!(!records.iter().any(|r| r.to_string().contains(&value)), "the value is in an activity record");
    assert!(records.iter().any(|r| r.to_string().contains("{{var.token}}")), "the record shows the name");
}

/// A saved template whose token page is `page`.
fn saved_on(r: &Rig, id: &str, page: &str) {
    let mut t = crate::api_templates_runner::template();
    t.id = id.to_string();
    t.antiforgery.page = page.to_string();
    store::save(r.root.path(), ORG, PROJECT, &t).unwrap();
}

fn at(path: &str) -> String {
    format!("https://hr.example.internal{path}")
}

#[tokio::test]
async fn a_post_with_no_token_uses_the_projects_one_token_page() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({ "ok": true }))], None);
    saved_on(&r, "has-a-token-page", "/hr/pmsv10/updatehub");
    let (mut browser, _) = exploring_session(&mut r).await;
    {
        let mut s = r.script.lock().unwrap();
        s.hrefs = VecDeque::from(vec![at(PAGE), at(PAGE), at("/hr/pmsv10/updatehub"), at("/hr/pmsv10/updatehub")]);
        s.tokens = VecDeque::from(vec![None]);
    }
    let (status, v) = send(&mut browser, &r, true, post_to(PATH, "edit")).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["token_page"], "/hr/pmsv10/updatehub (the page this project's templates use)", "{v}");
    assert_eq!(r.fetched().len(), 1, "sent once");
    assert!(r.navigated().ends_with("/hr/pmsv10/updatehub"), "{}", r.navigated());
}

#[tokio::test]
async fn a_post_with_no_token_and_two_pages_is_refused_naming_them() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    saved_on(&r, "page-one", "/hr/pmsv10/updatehub");
    saved_on(&r, "page-two", "/hr/pmsv10/performancecycle?mode=create");
    let (mut browser, _) = exploring_session(&mut r).await;
    r.script.lock().unwrap().token = None;
    let (status, v) = send(&mut browser, &r, true, post_to(PATH, "edit")).await;
    assert_eq!(status, 502, "{v}");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["failed"], "Anti-forgery token", "{v}");
    let detail = v["detail"].as_str().unwrap();
    assert!(detail.starts_with("this page has no anti-forgery token, so the POST was not sent - name a page with one in antiforgery_page; this project's templates use: "), "{detail}");
    assert!(detail.contains("/hr/pmsv10/performancecycle?mode=create") && detail.contains("/hr/pmsv10/updatehub"), "{detail}");
    assert!(!detail.contains("example.internal"), "{detail}");
    assert!(r.fetched().is_empty(), "something was sent");
}

#[tokio::test]
async fn a_post_with_no_token_and_no_saved_page_says_so() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    r.script.lock().unwrap().token = None;
    let (status, v) = send(&mut browser, &r, true, post_to(PATH, "edit")).await;
    assert_eq!(status, 502, "{v}");
    assert!(v["detail"].as_str().unwrap().ends_with("; no saved template names one yet"), "{v}");
    assert!(r.fetched().is_empty());
}

fn login_page(final_path: &str, redirected: bool) -> Value {
    json!({
        "status": 200,
        "contentType": "text/html; charset=utf-8",
        "finalUrl": at(final_path),
        "redirected": redirected,
        "text": "<html><form><input name=\"user\"><input type=\"password\" name=\"pass\"></form></html>",
    })
}

#[tokio::test]
async fn a_login_page_the_call_was_sent_to_another_page_for_is_signed_out_and_the_next_call_is_not_sent() {
    let _locks = locks();
    let mut r = rig(vec![login_page("/account/in", true), answer(200, json!({ "late": true }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let get = json!({ "step": { "method": "GET", "path": "/hr/x" } });

    let (status, v) = send(&mut browser, &r, false, get.clone()).await;
    assert_eq!(status, 502, "{v}");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["signed_out"], true, "{v}");
    assert_eq!(v["detail"], v2_lib::api_templates::one_off::SIGNED_OUT, "{v}");
    assert!(v.get("response").is_none(), "a signed-out answer carries no body: {v}");
    assert!(v.get("template").is_none(), "{v}");
    assert!(!v.to_string().contains("password"), "{v}");
    {
        let b = browser.as_mut().unwrap();
        assert!(b.account.is_none(), "still signed in");
        assert!(b.discovery.as_ref().unwrap().signed_out);
    }

    let (status, text) =
        send_in(&mut browser, &ctx(false), r.root.path(), &get.to_string(), &quick(), RUN_LIMIT, &QUICK_PAUSES).await;
    assert_eq!((status, text.as_str()), (409, v2_lib::api_templates::one_off::SIGNED_OUT_BEFORE));
    assert_eq!(r.fetched().len(), 1, "the second call was sent");
}

#[tokio::test]
async fn a_page_with_a_password_field_that_answers_in_place_is_an_answer() {
    let _locks = locks();
    let mut r = rig(vec![login_page("/hr/x", false)], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let get = json!({ "step": { "method": "GET", "path": "/hr/x" } });
    let (status, v) = send(&mut browser, &r, false, get).await;
    assert_eq!(status, 200, "{v}");
    assert!(v.get("signed_out").is_none(), "{v}");
    assert!(!browser.as_mut().unwrap().discovery.as_ref().unwrap().signed_out);
}

#[tokio::test]
async fn a_redirect_home_with_no_password_field_is_not_signed_out() {
    let _locks = locks();
    let mut home = login_page("/", true);
    home["text"] = json!("<html>dashboard</html>");
    let mut r = rig(vec![home], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let get = json!({ "step": { "method": "GET", "path": "/hr/x" } });
    let (_, v) = send(&mut browser, &r, false, get).await;
    assert!(v.get("signed_out").is_none(), "{v}");
    assert!(!browser.as_mut().unwrap().discovery.as_ref().unwrap().signed_out);
}

#[tokio::test]
async fn a_sign_in_page_asked_for_on_purpose_is_an_answer_not_a_sign_out() {
    let _locks = locks();
    let mut r = rig(vec![login_page("/", false)], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let get = json!({ "step": { "method": "GET", "path": "/" } });
    let (status, v) = send(&mut browser, &r, false, get).await;
    assert_eq!(status, 200, "{v}");
    assert!(v.get("signed_out").is_none(), "{v}");
    assert!(!browser.as_mut().unwrap().discovery.as_ref().unwrap().signed_out);
}

#[tokio::test]
async fn a_postback_on_a_page_with_a_password_field_is_not_signed_out() {
    let _locks = locks();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().postbacks = VecDeque::from(vec![page_reply(
        200,
        json!({ "excerpt": "<html><input type=\"password\" name=\"p\"></html>" }),
    )]);
    let (mut browser, _) = exploring_session(&mut r).await;
    let (status, v) = send(&mut browser, &r, true, postback_call(json!({}))).await;
    assert_eq!(status, 200, "{v}");
    assert!(v.get("signed_out").is_none(), "{v}");
    assert!(!browser.as_mut().unwrap().discovery.as_ref().unwrap().signed_out);
}

#[tokio::test]
async fn a_postback_the_runner_judges_signed_out_ends_the_discovery_for_later_sends() {
    let _locks = locks();
    let mut r = rig(vec![answer(200, json!({}))], None);
    r.script.lock().unwrap().postbacks = VecDeque::from(vec![page_reply(200, json!({ "signedOut": true }))]);
    let (mut browser, _) = exploring_session(&mut r).await;
    let (status, v) = send(&mut browser, &r, true, postback_call(json!({}))).await;
    assert_eq!(status, 502, "{v}");
    assert!(browser.as_mut().unwrap().discovery.as_ref().unwrap().signed_out, "{v}");
    let get = json!({ "step": { "method": "GET", "path": "/hr/x" } });
    let (status, text) =
        send_in(&mut browser, &ctx(true), r.root.path(), &get.to_string(), &quick(), RUN_LIMIT, &QUICK_PAUSES).await;
    assert_eq!((status, text.as_str()), (409, v2_lib::api_templates::one_off::SIGNED_OUT_BEFORE));
}

#[tokio::test]
async fn a_kept_value_in_the_path_or_query_is_never_in_the_record_or_the_log() {
    let _log = crate::serial::log_tail();
    let _locks = locks();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());
    // Not token-shaped (spaces), so only the kept-value hiding can mask it.
    let value = "emp 1234 north";
    let mut r = rig(vec![answer(200, json!({ "Ref": value })), answer(200, json!({ "ok": true }))], None);
    let (mut browser, _) = exploring_session(&mut r).await;
    let (_, v) = send(&mut browser, &r, true, keep_token_call()).await;
    assert_eq!(v["kept"], json!(["token (14 characters)"]), "{v}");

    on_the_app(&r);
    let call = json!({
        "step": { "method": "GET", "path": "/hr/emp/{{var.token}}", "query": { "q": "{{var.token}}" } },
        "save": false
    });
    let (status, v) = send(&mut browser, &r, true, call).await;
    assert_eq!(status, 200, "{v}");

    let records = crate::common::activity_records(activity.path(), "api");
    let sent = records.iter().map(|r| r.to_string()).collect::<Vec<_>>().join("\n");
    let log: String = v2_lib::applog::recent(500).iter().map(|l| l.message.clone()).collect::<Vec<_>>().join("\n");
    for text in [v.to_string(), sent.clone(), log] {
        for form in [value, "emp%201234%20north", "emp+1234+north"] {
            assert!(!text.contains(form), "{form} shows in: {text}");
        }
    }
    assert!(sent.contains("{{var.token}}"), "the record shows the name: {sent}");
}

#[tokio::test]
async fn kept_lists_and_numbers_are_hidden_too() {
    let _locks = locks();
    let mut r = rig(
        vec![
            answer(200, json!({ "Ids": ["first value here", "second value", "x"], "Num": 123456789012u64 })),
            answer(200, json!({ "echo": "first value here and second value", "n": 123456789012u64 })),
        ],
        None,
    );
    let (mut browser, _) = exploring_session(&mut r).await;
    let mut call = json!({ "step": { "method": "GET", "path": "/hr/x" }, "keep": { "ids": "$.Ids", "num": "$.Num" } });
    call["save"] = json!(false);
    let (status, v) = send(&mut browser, &r, true, call).await;
    assert_eq!(status, 200, "{v}");
    let text = v.to_string();
    for shown in ["first value here", "second value", "123456789012"] {
        assert!(!text.contains(shown), "{shown} shows in: {text}");
    }
    assert!(text.contains("{{var.ids}}") && text.contains("{{var.num}}"), "{text}");

    on_the_app(&r);
    let (_, v) = send(&mut browser, &r, true, json!({ "step": { "method": "GET", "path": "/hr/y" }, "save": false })).await;
    let text = v.to_string();
    for shown in ["first value here", "second value", "123456789012"] {
        assert!(!text.contains(shown), "{shown} shows in a later answer: {text}");
    }
}
