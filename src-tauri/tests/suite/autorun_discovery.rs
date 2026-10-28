//! Discovery sessions: the assistant opens the Auto Run browser itself,
//! signs in as a saved account (the app types the password), and explores
//! the live application one action at a time. What it sees, the writes the
//! page sends and what each action led to are kept in the discovery map.
//!
//! The real browser is never opened here. The routes' checks that need no
//! browser run through `route`; everything after the browser opens runs
//! through the `_in` functions, against a fake browser slot that holds a
//! scripted driver and says when it was closed.

use crate::common::{account, pick_a_date, quick, ready_probe, FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use crate::common::route_without_rules as route;
use v2_lib::ai_bridge::{
    close_browser_in, discover_action_in, discover_area_in, discover_start_in, discovery_sighting, end_discovery_in,
    read_page, refuse_while_discovering, repair_area_in, BridgeContext, DiscoveryBrowser, DiscoveryParts,
    NO_DISCOVERY,
};
use v2_lib::autorun::area_repair::{LONG_REASON, NOTHING_TO_REPAIR, SAME_PATH};
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::components::{draft_fingerprint, put, Component};
use v2_lib::autorun::discovery_map::{load_map, map_path, AreaMap};
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::nav::{find_area, load_nav, nav_path, put_path, MadeBy, ModulePath};
use v2_lib::autorun::recipe::{save_recipe, SignInRecipe};
use v2_lib::autorun::store::set_root;
use v2_lib::browser::actions::{Action, CHECK_TEXT_JS, HIGHLIGHT_JS};
use v2_lib::browser::cdp::Event;
use v2_lib::browser::input::{FOCUS_JS, HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::{Target, VISIBLE_JS};
use v2_lib::browser::snapshot::DEFAULT_LIMIT;
use v2_lib::autorun::mapping_summary::{load_summary, summarize, summary_path, MappingSummary};
use v2_lib::commands::autorun::{busy_browser_sentence, DiscoveryState, GuardOwner, MappingPlace, MappingRun, MappingScreen};

pub(crate) const ORG: &str = "acme";
pub(crate) const PROJECT: &str = "Web";

pub(crate) struct TempDir(std::path::PathBuf);

impl TempDir {
    pub(crate) fn new() -> Self {
        use std::sync::atomic::AtomicU64;
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-autorun-discovery-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn ctx() -> BridgeContext {
    BridgeContext { org: ORG.into(), project: PROJECT.into(), ..BridgeContext::default() }
}

/// One click on `#go`, then `#marker` says the sign-in arrived.
fn recipe() -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

/// A root holding the project's recipe and the `admin` account (login
/// `kim`).
pub(crate) fn root_with_recipe_and_account() -> TempDir {
    let dir = TempDir::new();
    save_recipe(dir.path(), ORG, PROJECT, &recipe()).unwrap();
    save_accounts(dir.path(), &[account()]).unwrap();
    dir
}

/// The Auto Run browser as the `_in` functions see it: a driver, its lease,
/// its signed-in account and the discovery under way. `closed` says
/// whether the slot closed it; `gone` makes it a browser that has gone.
pub(crate) struct FakeBrowser {
    pub(crate) d: ScriptedDriver,
    pub(crate) lease: Held,
    pub(crate) account: Option<String>,
    pub(crate) discovery: Option<DiscoveryState>,
    /// The case whose guard it holds and who owns the guard, as the
    /// session keeps them.
    pub(crate) guarded_case: Option<i32>,
    pub(crate) guard_owner: Option<GuardOwner>,
    pub(crate) closed: Arc<AtomicBool>,
    pub(crate) gone: Arc<AtomicBool>,
    /// The assistant opened it, as `open_for_discovery` does; a test's
    /// slot is the person's unless it says otherwise.
    pub(crate) assistant: bool,
}

impl DiscoveryBrowser for FakeBrowser {
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
        !self.gone.load(Ordering::SeqCst)
    }
    fn opened_by_assistant(&mut self) -> bool {
        self.assistant
    }
}

/// A browser slot holding `d`, opened for a discovery (no account yet)
/// when `discovering`, else the person's own browser.
pub(crate) fn slot(d: ScriptedDriver, discovery: Option<DiscoveryState>) -> (Option<FakeBrowser>, Arc<AtomicBool>) {
    let closed = Arc::new(AtomicBool::new(false));
    let gone = Arc::new(AtomicBool::new(false));
    let b = FakeBrowser {
        d,
        lease: Held::supervised(),
        account: None,
        discovery,
        guarded_case: None,
        guard_owner: None,
        closed: closed.clone(),
        gone,
        assistant: false,
    };
    (Some(b), closed)
}

pub(crate) fn opened_for_discovery() -> Option<DiscoveryState> {
    Some(DiscoveryState {
        area: None,
        account: None,
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
        explore: None,
    })
}

pub(crate) fn exploring(area: &str) -> Option<DiscoveryState> {
    Some(DiscoveryState {
        area: Some(area.to_string()),
        account: Some("admin".to_string()),
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
        explore: None,
    })
}

/// `state`, made a mapping run over `modules`.
fn mapping(state: Option<DiscoveryState>, modules: &[&str]) -> Option<DiscoveryState> {
    let modules: Vec<String> = modules.iter().map(|m| m.to_string()).collect();
    state.map(|s| DiscoveryState { mapping: Some(MappingRun::new(&modules)), ..s })
}

pub(crate) fn mapped_area(root: &std::path::Path, area: &str) -> Option<AreaMap> {
    load_map(root, ORG, PROJECT).unwrap().areas.into_iter().find(|a| a.area == area)
}

/// A small application: a sign-in page whose `#go` click lands on
/// `/hr/home/index`, and a page with a Name field and a Save button. Its
/// address always carries a query string, which nothing may keep. `#marker`
/// is found only when `marker_shows`.
pub(crate) fn signin_app(marker_shows: bool) -> ScriptedDriver {
    let path = Arc::new(Mutex::new("/".to_string()));
    let mut last_css = String::new();
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Page.navigate" => {
                *path.lock().unwrap() = v2_lib::autorun::nav::path_of(params["url"].as_str().unwrap_or(""));
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => json!({ "result": {
                "value": format!("https://hr.example.internal{}?token=t0p-secret#top", path.lock().unwrap())
            } }),
            "Runtime.evaluate" if params["expression"] == "document.title" => json!({ "result": { "value": "Home" } }),
            "Runtime.evaluate" => {
                json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } })
            }
            "Accessibility.getFullAXTree" => json!({ "nodes": [
                { "nodeId": "1", "ignored": false, "role": { "value": "form" }, "name": { "value": "Leave" },
                  "childIds": ["2", "3"] },
                { "nodeId": "2", "ignored": false, "role": { "value": "textbox" }, "name": { "value": "Name" },
                  "childIds": [] },
                { "nodeId": "3", "ignored": false, "role": { "value": "button" }, "name": { "value": "Save" },
                  "childIds": [] }
            ] }),
            "Network.getAllCookies" => json!({ "cookies": [] }),
            "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": ready_probe() } }),
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == FOCUS_JS => json!({ "result": { "value": "text" } }),
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = last_css != "#marker" || marker_shows;
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                if last_css == "#go" {
                    *path.lock().unwrap() = "/hr/home/index".to_string();
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
    d
}

/// A page on which every locator finds one ready element. Once `moves_on`
/// has been called its address is `after`; before, `/hr/leave/new`.
pub(crate) fn leave_page(moves_on: &'static str, after: &'static str) -> ScriptedDriver {
    let page = FakePage::default();
    let moved = AtomicBool::new(false);
    ScriptedDriver::new(move |method, params| {
        if method == moves_on {
            moved.store(true, Ordering::SeqCst);
        }
        if method == "Runtime.evaluate" && params["expression"] == "location.href" {
            let at = if moved.load(Ordering::SeqCst) { after } else { "https://hr.example.internal/hr/leave/new?x=1" };
            return Ok(json!({ "result": { "value": at } }));
        }
        page.answer(method, params)
    })
    .with_net_record()
}

pub(crate) fn sent(id: &str, method: &str, url: &str) -> Event {
    Event {
        method: "Network.requestWillBeSent".into(),
        params: json!({ "requestId": id, "request": { "url": url, "method": method } }),
    }
}

pub(crate) fn parsed(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("not JSON ({e}): {body}"))
}

// ------------------------------------------------------------------ start

/// A start signs in as the named account, says where it landed (a path,
/// never a query string), hands back the page, and files that page in the
/// map under the discovery's area, without marking the area explored.
#[tokio::test]
async fn start_signs_in_and_returns_the_landing_page() {
    let dir = root_with_recipe_and_account();
    let (mut browser, closed) = slot(signin_app(true), opened_for_discovery());

    let (status, body) =
        discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some("Leave"), &quick()).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["signed_in"], true, "{body}");
    assert_eq!(v["path"], "/hr/home/index", "{body}");
    assert!(v["page"].as_str().unwrap().contains("button \"Save\""), "{body}");
    assert!(!body.contains("t0p-secret"), "the address's query string came back: {body}");
    assert!(!closed.load(Ordering::SeqCst));

    let b = browser.as_ref().expect("the browser stays open");
    let state = b.discovery.as_ref().expect("the discovery is under way");
    assert_eq!(state.area.as_deref(), Some("Leave"));
    assert_eq!(state.account.as_deref(), Some("admin"));
    assert_eq!(b.account.as_deref(), Some("admin"));

    let area = mapped_area(dir.path(), "Leave").expect("the landing page was not recorded");
    assert_eq!(area.explored_at, None, "the landing page is not the area: it marks nothing explored");
    let page = area.pages.iter().find(|p| p.path == "/hr/home/index").expect("no landing page");
    assert!(page.elements.iter().any(|e| e.name == "Save"), "{:?}", page.elements);
    let file = std::fs::read_to_string(map_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(!file.contains("kim"), "a login reached the map: {file}");
    assert!(!file.contains("t0p-secret"), "a query string reached the map: {file}");
}

/// A start sent `page: false` answers without the landing page, as its route
/// gives it (`without_page` over `discover_start_in`'s answer); the page is
/// still read and recorded. The route itself opens a real browser first, so
/// it is driven here from where the fake browser can stand in.
#[tokio::test]
async fn a_start_asked_for_no_page_leaves_it_out() {
    use v2_lib::ai_bridge::{without_page, PAGE_LEFT_OUT};
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(signin_app(true), opened_for_discovery());
    let answer = discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some("Leave"), &quick()).await;
    let body = json!({ "account": "admin", "area": "Leave", "page": false }).to_string();
    let (status, out) = without_page(answer, &body);
    assert_eq!(status, 200, "{out}");
    let v = parsed(&out);
    assert!(v.get("page").is_none(), "{out}");
    assert_eq!(v["page_left_out"], PAGE_LEFT_OUT);
    assert_eq!((v["signed_in"].clone(), v["path"].clone()), (json!(true), json!("/hr/home/index")), "{out}");
    let area = mapped_area(dir.path(), "Leave").expect("the landing page was not recorded");
    let page = area.pages.iter().find(|p| p.path == "/hr/home/index").expect("no landing page");
    assert!(page.elements.iter().any(|e| e.name == "Save"), "{:?}", page.elements);
}

/// Nothing opens while something else holds the browser: an unattended
/// run, a recording, a replay to a step. A browser already open (the
/// person's, or a discovery) is refused with the sentence that names what
/// to do - for a discovery, `end_autorun_discovery`.
#[tokio::test]
async fn start_is_refused_while_an_unattended_run_or_a_session_holds_the_browser() {
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let body = json!({ "account": "admin" }).to_string();

    let run = v2_lib::commands::autorun_replay::OneAtATime::claim().expect("nothing is running");
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-start", &body, "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out, "an unattended run is going - wait for it, or stop it first");
    drop(run);

    let recording = v2_lib::commands::autorun_record::RecorderClaim::claim().expect("nothing is recording");
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-start", &body, "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out, v2_lib::commands::autorun_record::RECORDING_BUSY);
    drop(recording);

    let replay = v2_lib::autorun::replay_to::OneReplay::claim().expect("nothing is replaying");
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-start", &body, "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out, v2_lib::autorun::replay_to::ALREADY_RUNNING);
    drop(replay);

    assert_eq!(busy_browser_sentence(false), "close the supervised browser first");
    assert!(busy_browser_sentence(true).contains("end_autorun_discovery"), "{}", busy_browser_sentence(true));

    // The person's own browser, taken over between the open and the sign-in:
    // the discovery does not sign in there, and does not close it.
    let (mut theirs, closed) = slot(signin_app(true), None);
    let (status, out) = discover_start_in(&mut theirs, dir.path(), ORG, PROJECT, "admin", None, &quick()).await;
    assert_eq!(status, 409, "{out}");
    assert!(!closed.load(Ordering::SeqCst), "the person's browser was closed");
    assert!(theirs.as_ref().unwrap().d.calls_to("Page.navigate").is_empty(), "it signed in in the person's browser");
}

/// A sign-in that does not arrive closes the browser it opened and says
/// why, in the sign-in's own words.
#[tokio::test]
async fn a_failed_sign_in_closes_the_browser_and_says_why() {
    let dir = root_with_recipe_and_account();
    let (mut browser, closed) = slot(signin_app(false), opened_for_discovery());
    let (status, out) = discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", None, &quick()).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.starts_with(v2_lib::autorun::signin::MARKER_NEVER_APPEARED), "{out}");
    assert!(closed.load(Ordering::SeqCst), "the browser was left open");
    assert!(browser.is_none());

    let (mut browser, closed) = slot(signin_app(true), opened_for_discovery());
    let (status, out) = discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "nobody", None, &quick()).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("nobody"), "{out}");
    assert!(closed.load(Ordering::SeqCst), "the browser was left open");
}

// ---------------------------------------------------------------- actions

/// One action: what it sent (the writes, by method and path), the dialog
/// it raised, where it moved to and the page there. The writes go into the
/// area's log, the page into the map, and a line saying what happened into
/// the area's outcomes. Naming an area moves the discovery to it.
#[tokio::test]
async fn an_action_reports_writes_dialogs_and_the_new_path_and_logs_writes_in_the_map() {
    let dir = root_with_recipe_and_account();
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list?page=2");
    d.on_call_events.push(("Input.dispatchMouseEvent".into(), sent("1", "POST", "https://hr.example.internal/hr/leave/save?id=5")));
    d.on_call_events.push(("Input.dispatchMouseEvent".into(), sent("2", "GET", "https://hr.example.internal/hr/leave/list")));
    d.dialogs_on_call.push(("Input.dispatchMouseEvent".into(), "alert".into(), "Leave saved".into()));
    let (mut browser, _) = slot(d, exploring("Leave"));

    let save = Action::Click { selector: "#save".into() };
    let (status, body) =
        discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, Some("Leave Requests")).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    assert_eq!(v["path"], "/hr/leave/list", "{body}");
    assert_eq!(v["dialogs"], json!(["alert: Leave saved"]), "{body}");
    assert_eq!(v["writes"], json!([{ "method": "POST", "path": "/hr/leave/save" }]), "{body}");
    assert!(v["page"].is_string(), "{body}");
    assert!(v["detail"].is_string(), "{body}");

    let state = browser.as_ref().unwrap().discovery.as_ref().unwrap();
    assert_eq!(state.area.as_deref(), Some("Leave Requests"));
    let area = mapped_area(dir.path(), "Leave Requests").expect("nothing was filed under the new area");
    assert_eq!(area.writes.len(), 1, "{:?}", area.writes);
    assert_eq!(area.writes[0].method, "POST");
    assert_eq!(area.writes[0].path, "/hr/leave/save");
    assert!(area.writes[0].step.contains("#save"), "{:?}", area.writes[0]);
    assert!(area.outcomes.iter().any(|o| o.contains("#save") && o.contains("Leave saved")), "{:?}", area.outcomes);
    assert!(area.pages.iter().any(|p| p.path == "/hr/leave/list"), "{:?}", area.pages);
    assert!(area.explored_at.is_some());

    // An action that moves the page and raises nothing says where it went.
    let d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/view?id=9");
    let (mut browser, _) = slot(d, exploring("Leave"));
    let open = Action::Click { selector: "#open".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None, None).await;
    assert_eq!(status, 200, "{body}");
    let area = mapped_area(dir.path(), "Leave").unwrap();
    assert!(area.outcomes.iter().any(|o| o.contains("#open") && o.contains("moved to /hr/leave/view")), "{:?}", area.outcomes);
    assert!(area.outcomes.iter().all(|o| !o.contains("id=9")), "{:?}", area.outcomes);
}

/// Neither the map nor the answer's write list keeps a query string, and
/// the map never keeps what a `fill` typed - not in the write log, not in
/// what a dialog said back.
#[tokio::test]
async fn an_action_never_logs_a_query_string_or_a_typed_value() {
    let dir = root_with_recipe_and_account();
    let typed = "Jane-Secret-42";
    let mut d = leave_page("Input.insertText", "https://hr.example.internal/hr/leave/new?draft=1");
    d.on_call_events.push((
        "Input.insertText".into(),
        sent("1", "PUT", &format!("https://hr.example.internal/hr/autosave?name={typed}&token=abc#frag")),
    ));
    d.dialogs_on_call.push(("Input.insertText".into(), "alert".into(), format!("Saved {typed}")));
    let (mut browser, _) = slot(d, exploring("Leave"));

    let fill = Action::Fill { selector: "#name".into(), value: typed.to_string() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &fill, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["writes"], json!([{ "method": "PUT", "path": "/hr/autosave" }]), "{body}");

    let file = std::fs::read_to_string(map_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(!file.contains(typed), "a typed value reached the map: {file}");
    assert!(!file.contains("token=abc") && !file.contains("draft=1") && !file.contains('?'), "{file}");
    let area = mapped_area(dir.path(), "Leave").unwrap();
    assert_eq!(area.writes.len(), 1, "{:?}", area.writes);
    assert!(area.outcomes.iter().any(|o| o.contains("Saved")), "{:?}", area.outcomes);
}

/// A navigate (or a new tab) to a site outside the recipe's is refused
/// before the browser is touched.
#[tokio::test]
async fn navigate_outside_the_allowed_sites_is_refused() {
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    for action in [
        json!({ "kind": "navigate", "url": "https://evil.example/steal" }),
        json!({ "kind": "open_tab", "name": "other", "url": "https://evil.example/steal" }),
    ] {
        let body = json!({ "action": action }).to_string();
        let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-action", &body, "1.0.0").await;
        assert_eq!(status, 400, "{out}");
        assert!(out.contains("https://evil.example"), "{out}");
        assert!(out.contains("allowed"), "{out}");
    }
    // An allowed one gets past the check, to the browser that is not there.
    let body = json!({ "action": { "kind": "navigate", "url": "https://hr.example.internal/hr/leave" } }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-action", &body, "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("start_autorun_discovery"), "{out}");
}

/// Signing in is the start's job, and a local file is never somewhere a
/// discovery goes: both refused with `/autorun-try`'s own sentences.
#[tokio::test]
async fn sign_in_and_file_urls_are_refused_as_actions() {
    // The refusals are compared word for word, and the route ends every
    // answer with what main's browser has changed for testing.
    let _g = crate::serial::autorun();
    let body = json!({ "action": { "kind": "sign_in", "account": "admin" } }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-action", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "sign_in is not a thing an assistant does - the person signs in");

    for action in [
        json!({ "kind": "navigate", "url": "file:///etc/passwd" }),
        json!({ "kind": "open_tab", "name": "local", "url": "  FILE://C:/secrets.txt" }),
    ] {
        let body = json!({ "action": action }).to_string();
        let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-action", &body, "1.0.0").await;
        assert_eq!(status, 400, "{out}");
        assert_eq!(out, "a tried navigate goes to http or https only");
    }
}

// -------------------------------------------------------------- the end

/// Closing the browser ends the discovery with it: nothing is left for the
/// next action to run in.
#[tokio::test]
async fn close_browser_ends_a_discovery_session() {
    let dir = root_with_recipe_and_account();
    let (mut browser, closed) = slot(FakePage::default().driver(), exploring("Leave"));
    assert!(close_browser_in(&mut browser), "there was a browser to close");
    assert!(closed.load(Ordering::SeqCst));
    assert!(browser.is_none());
    let save = Action::Click { selector: "#save".into() };
    let (status, out) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("start_autorun_discovery"), "{out}");
    assert!(!close_browser_in(&mut browser), "nothing left to close");
}

/// Ending a discovery keeps its browser, signed in, for the next replay or
/// discovery, and ending again - or with no browser, or with the person's
/// own browser open - is fine and closes nothing.
#[tokio::test]
async fn end_is_idempotent() {
    let (mut browser, closed) = slot(FakePage::default().driver(), exploring("Leave"));
    browser.as_mut().unwrap().account = Some("admin".into());
    let (status, said) = end_discovery_in(&mut browser).await;
    assert_eq!((status, said.as_str()), (200, v2_lib::ai_bridge::DISCOVERY_ENDED_KEPT));
    assert!(!closed.load(Ordering::SeqCst), "ending the discovery closed its browser");
    let b = browser.as_ref().expect("ending the discovery let its browser go");
    assert!(b.discovery.is_none(), "the discovery is still going");
    assert_eq!(b.account.as_deref(), Some("admin"), "its sign-in was dropped");
    let (status, said) = end_discovery_in(&mut browser).await;
    assert_eq!((status, said.as_str()), (200, "no discovery is going"));
    assert!(!closed.load(Ordering::SeqCst));
    let mut none: Option<FakeBrowser> = None;
    let (status, _) = end_discovery_in(&mut none).await;
    assert_eq!(status, 200);

    let (mut theirs, closed) = slot(FakePage::default().driver(), None);
    let (status, _) = end_discovery_in(&mut theirs).await;
    assert_eq!(status, 200);
    assert!(!closed.load(Ordering::SeqCst), "ending a discovery closed the person's browser");
    assert!(theirs.is_some());

    for _ in 0..2 {
        let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-end", "", "1.0.0").await;
        assert_eq!(status, 200, "{out}");
    }
}

// ---------------------------------------------------------------- mapping

/// A mapping run names the modules it maps: none, or only blank names, is
/// refused before anything opens, as is a `mapping` that is not true or
/// false.
#[tokio::test]
async fn a_mapping_run_needs_a_module() {
    // The refusal is compared word for word, and the route ends every
    // answer with what main's browser has changed for testing.
    let _g = crate::serial::autorun();
    for body in [
        json!({ "account": "admin", "mapping": true }),
        json!({ "account": "admin", "mapping": true, "modules": [] }),
        json!({ "account": "admin", "mapping": true, "modules": ["  ", ""] }),
    ] {
        let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-start", &body.to_string(), "1.0.0").await;
        assert_eq!(status, 400, "{body}: {out}");
        assert_eq!(out, "Name at least one module to map.", "{body}");
    }
    let body = json!({ "account": "admin", "mapping": "yes", "modules": ["Leave"] }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-start", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("mapping"), "{out}");
    let body = json!({ "account": "admin", "mapping": true, "modules": "Leave" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-start", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("modules"), "{out}");
}

/// A mapping run signs in, then switches the save guard on before it reads
/// the landing page. Every save the page tries after that is stopped,
/// counted on the run and in the action's answer, and logged by method and
/// path only.
#[tokio::test]
async fn a_save_the_page_sends_during_mapping_is_blocked_and_counted() {
    use v2_lib::browser::cdp::Driver;
    let _log = crate::serial::log_tail();
    let dir = root_with_recipe_and_account();

    let (mut browser, closed) = slot(signin_app(true), mapping(opened_for_discovery(), &["Leave", " "]));
    let (status, body) =
        discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some("Leave"), &quick()).await;
    assert_eq!(status, 200, "{body}");
    assert!(!closed.load(Ordering::SeqCst));
    let b = browser.as_ref().unwrap();
    assert!(b.d.is_guarding_saves(), "the mapping run's guard is not on");
    let methods: Vec<&str> = b.d.calls.iter().map(|(m, _)| m.as_str()).collect();
    let guard = methods.iter().position(|m| *m == "Fetch.enable").expect("the guard never went on");
    let sign_in = methods.iter().position(|m| *m == "Input.dispatchMouseEvent").expect("no sign-in click");
    let landing = methods.iter().rposition(|m| *m == "Accessibility.getFullAXTree").expect("no page read");
    assert!(sign_in < guard, "the guard went on before the sign-in: {methods:?}");
    assert!(guard < landing, "the landing page was read before the guard went on: {methods:?}");
    let run = b.discovery.as_ref().unwrap().mapping.as_ref().expect("the sign-in dropped the mapping run");
    assert_eq!(run.modules, vec!["Leave".to_string()]);
    assert_eq!(run.blocked_writes, 0);

    // A click on which the page sends two saves, in a guarded mapping browser.
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list?page=2");
    d.guard_saves(&[]).await.unwrap();
    d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "POST".into(),
        "https://hr.example.internal/hr/leave/save?id=5&token=t0p-secret".into(),
    ));
    d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "DELETE".into(),
        "https://hr.example.internal/hr/leave/delete/7".into(),
    ));
    let (mut browser, _) = slot(d, mapping(exploring("Leave"), &["Leave"]));
    let save = Action::Click { selector: "#save".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "a stopped save failed the click that set it off: {body}");
    assert!(!v2_lib::browser::save_guard::is_blocked(v["detail"].as_str().unwrap_or("")), "{body}");
    assert_eq!(v["blocked"], 2, "{body}");
    assert!(!body.contains("t0p-secret"), "{body}");
    let run = browser.as_ref().unwrap().discovery.as_ref().unwrap().mapping.as_ref().unwrap();
    assert_eq!(run.blocked_writes, 2);

    // The next action adds to the run's count; its answer says its own.
    browser.as_mut().unwrap().d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "PUT".into(),
        "https://hr.example.internal/hr/leave/update".into(),
    ));
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["ok"], true, "{body}");
    assert_eq!(parsed(&body)["blocked"], 1, "{body}");
    let run = browser.as_ref().unwrap().discovery.as_ref().unwrap().mapping.as_ref().unwrap();
    assert_eq!(run.blocked_writes, 3);

    let lines: Vec<String> = v2_lib::applog::recent(500).into_iter().map(|l| l.message).collect();
    for logged in ["POST /hr/leave/save", "DELETE /hr/leave/delete/7", "PUT /hr/leave/update"] {
        assert!(lines.iter().any(|l| l.ends_with(logged)), "{logged} was not logged: {lines:?}");
    }
    assert!(!lines.iter().any(|l| l.contains("t0p-secret") || l.contains("hr.example.internal/hr/leave")), "{lines:?}");
}

/// A save stopped outside an action (here while the page is read after
/// one) is counted, and never fails the next action: that click still runs.
#[tokio::test]
async fn a_save_stopped_between_actions_never_fails_the_next_one() {
    use v2_lib::browser::cdp::Driver;
    let dir = root_with_recipe_and_account();
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list?page=2");
    d.guard_saves(&[]).await.unwrap();
    d.saves_on_call.push((
        "Accessibility.getFullAXTree".into(),
        "POST".into(),
        "https://hr.example.internal/api/SaveLastVisited".into(),
    ));
    let (mut browser, _) = slot(d, mapping(exploring("Leave"), &["Leave"]));
    let open = Action::Click { selector: "#open".into() };

    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["ok"], true, "{body}");
    let b = browser.as_ref().unwrap();
    assert!(b.d.saves_on_call.is_empty(), "the page read never sent its save");
    let clicks_before = b.d.calls_to("Input.dispatchMouseEvent").len();

    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "the stopped save failed the next action: {body}");
    assert!(!v2_lib::browser::save_guard::is_blocked(v["detail"].as_str().unwrap_or("")), "{body}");
    let b = browser.as_ref().unwrap();
    assert!(b.d.calls_to("Input.dispatchMouseEvent").len() > clicks_before, "the next click never ran");
    assert_eq!(b.discovery.as_ref().unwrap().mapping.as_ref().unwrap().blocked_writes, 1);
}

/// A mapping run never goes unguarded: when its guard cannot go on, the
/// start answers with the guard's own sentence and closes the browser.
#[tokio::test]
async fn a_mapping_run_whose_guard_cannot_go_on_closes_its_browser() {
    let dir = root_with_recipe_and_account();
    let nav = v2_lib::autorun::nav::nav_path(dir.path(), ORG, PROJECT);
    std::fs::create_dir_all(nav.parent().unwrap()).unwrap();
    std::fs::write(&nav, "not json").unwrap();
    let (mut browser, closed) = slot(signin_app(true), mapping(opened_for_discovery(), &["Leave"]));
    let (status, out) = discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", None, &quick()).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.starts_with(v2_lib::browser::save_guard::SETUP_FAILED), "{out}");
    assert!(closed.load(Ordering::SeqCst), "the browser was left open unguarded");
    assert!(browser.is_none(), "the discovery slot still holds the browser");
}

// ------------------------------------------------- a case that must not save

/// Case 77's script, marked `no_save` when `no_save`.
fn save_case_77(root: &std::path::Path, no_save: bool) {
    let mut s: v2_lib::autorun::CaseScript = serde_json::from_value(json!({
        "case_id": 77,
        "title": "Leave request",
        "account": "admin",
        "area": "Leave",
        "steps": [ { "step_number": 1, "actions": [ { "kind": "click", "selector": { "css": "#s1" } } ] } ]
    }))
    .unwrap();
    s.no_save = no_save;
    v2_lib::autorun::store::save_script(root, &s).unwrap();
}

/// `state`, explored for case `id`.
fn for_case(state: Option<DiscoveryState>, id: i32) -> Option<DiscoveryState> {
    state.map(|s| DiscoveryState { case_id: Some(id), ..s })
}

/// A discovery for a case marked `no_save` puts the guard on before the
/// action, so the save a click sends is BLOCKED, not only reported: the
/// action fails with the guard's sentence, and the answer names the request.
/// A guard lifted between two actions is back on before the next.
#[tokio::test]
async fn a_discovery_for_a_no_save_case_blocks_a_save_a_click_sends() {
    use v2_lib::browser::cdp::Driver;
    let dir = root_with_recipe_and_account();
    save_case_77(dir.path(), true);
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "POST".into(),
        "https://hr.example.internal/api/leave/create?token=t0p-secret".into(),
    ));
    let (mut browser, _) = slot(d, for_case(exploring("Leave"), 77));
    let save = Action::Click { selector: "#save".into() };

    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], false, "a blocked save did not fail the action: {body}");
    let detail = v["detail"].as_str().unwrap_or("");
    assert!(v2_lib::browser::save_guard::is_blocked(detail), "{body}");
    assert!(detail.contains("POST /api/leave/create"), "{body}");
    assert_eq!(v["no_save"], true, "{body}");
    assert_eq!(v["blocked"], 1, "{body}");
    assert_eq!(v["blocked_writes"], json!(["POST /api/leave/create"]), "{body}");
    assert!(!body.contains("t0p-secret"), "{body}");
    let b = browser.as_ref().unwrap();
    let methods: Vec<&str> = b.d.calls.iter().map(|(m, _)| m.as_str()).collect();
    let guard = methods.iter().position(|m| *m == "Fetch.enable").expect("the guard never went on");
    let click = methods.iter().position(|m| *m == "Input.dispatchMouseEvent").expect("no click");
    assert!(guard < click, "the click ran before the guard went on: {methods:?}");
    assert!(b.discovery.as_ref().unwrap().no_save, "the discovery did not keep its no-save mark");

    // Something lifts the guard between two actions: it is back on first.
    browser.as_mut().unwrap().d.stop_guarding_saves().await.unwrap();
    assert!(!browser.as_ref().unwrap().d.is_guarding_saves());
    browser.as_mut().unwrap().d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "PUT".into(),
        "https://hr.example.internal/api/leave/7".into(),
    ));
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], false, "{body}");
    assert_eq!(v["blocked_writes"], json!(["PUT /api/leave/7"]), "{body}");
    assert!(browser.as_ref().unwrap().d.is_guarding_saves(), "the guard was not put back on");
}

/// A save the guard failed inside the browser never reached the server: the
/// record shows it as a failed request, and the answer lists it as blocked,
/// never as a write (the batch line says "blocked", not "wrote").
#[tokio::test]
async fn a_save_the_guard_blocked_is_listed_as_blocked_not_written() {
    let dir = root_with_recipe_and_account();
    save_case_77(dir.path(), true);
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    d.on_call_events.push((
        "Input.dispatchMouseEvent".into(),
        sent("1", "POST", "https://hr.example.internal/hr/Home/SetUserPreferences?x=1"),
    ));
    d.on_call_events.push((
        "Input.dispatchMouseEvent".into(),
        Event {
            method: "Network.loadingFailed".into(),
            params: json!({ "requestId": "1", "errorText": "net::ERR_BLOCKED_BY_CLIENT" }),
        },
    ));
    let (mut browser, _) = slot(d, for_case(exploring("Leave"), 77));
    let save = Action::Click { selector: "#save".into() };

    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["writes"], json!([]), "{body}");
    assert_eq!(v["blocked_writes"], json!(["POST /hr/Home/SetUserPreferences"]), "{body}");

    let batch = vec![save.clone()];
    let stop = AtomicBool::new(false);
    let mut again = browser;
    again.as_mut().unwrap().d.on_call_events.push((
        "Input.dispatchMouseEvent".into(),
        sent("2", "POST", "https://hr.example.internal/hr/Home/SetUserPreferences"),
    ));
    again.as_mut().unwrap().d.on_call_events.push((
        "Input.dispatchMouseEvent".into(),
        Event {
            method: "Network.loadingFailed".into(),
            params: json!({ "requestId": "2", "errorText": "net::ERR_BLOCKED_BY_CLIENT" }),
        },
    ));
    use v2_lib::ai_bridge::discover_actions_in;
    let (status, text) = discover_actions_in(&mut again, dir.path(), ORG, PROJECT, &batch, None, None, true, &stop).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("; blocked POST /hr/Home/SetUserPreferences"), "{text}");
    assert!(!text.contains("wrote"), "{text}");
}

/// A save the guard stopped that is seen both ways - as a failed request in
/// the record and as a stop the guard counted - is listed once.
#[tokio::test]
async fn a_blocked_save_seen_both_ways_is_listed_once() {
    let dir = root_with_recipe_and_account();
    save_case_77(dir.path(), true);
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    d.on_call_events.push((
        "Input.dispatchMouseEvent".into(),
        sent("1", "POST", "https://hr.example.internal/hr/Home/SetUserPreferences"),
    ));
    d.on_call_events.push((
        "Input.dispatchMouseEvent".into(),
        Event {
            method: "Network.loadingFailed".into(),
            params: json!({ "requestId": "1", "errorText": "net::ERR_BLOCKED_BY_CLIENT" }),
        },
    ));
    d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "POST".into(),
        "https://hr.example.internal/hr/Home/SetUserPreferences".into(),
    ));
    let (mut browser, _) = slot(d, for_case(exploring("Leave"), 77));
    let save = Action::Click { selector: "#save".into() };

    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["writes"], json!([]), "{body}");
    assert_eq!(v["blocked_writes"], json!(["POST /hr/Home/SetUserPreferences"]), "{body}");
    assert_eq!(v["blocked"], 1, "{body}");
}

/// A discovery STARTED with `no_save: true` (`open_for_discovery` puts the
/// start's mark on it) guards with no case named, and the mark is never
/// taken off by a later call that does not say it: the guard is the
/// discovery's for as long as it lasts.
#[tokio::test]
async fn a_discovery_told_no_save_blocks_saves_and_stays_guarded() {
    let dir = root_with_recipe_and_account();
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "POST".into(),
        "https://hr.example.internal/api/Employee/AddEmployee".into(),
    ));
    let told = v2_lib::ai_bridge::discovery_for(dir.path(), &json!({ "no_save": true }).to_string()).unwrap();
    let started = exploring("Leave").map(|s| DiscoveryState { no_save: told.no_save, ..s });
    let (mut browser, _) = slot(d, started);
    let quiet = v2_lib::ai_bridge::discovery_for(dir.path(), "{}").unwrap();
    v2_lib::ai_bridge::mark_discovery_for(&mut browser, quiet);
    assert!(browser.as_ref().unwrap().discovery.as_ref().unwrap().no_save, "a later call took the mark off");

    let save = Action::Click { selector: "#save".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], false, "{body}");
    assert_eq!(v["blocked_writes"], json!(["POST /api/Employee/AddEmployee"]), "{body}");
    use v2_lib::browser::cdp::Driver;
    let b = browser.as_ref().unwrap();
    assert!(b.d.is_guarding_saves(), "the call's end lifted the discovery's guard");
    assert_eq!(b.guard_owner, Some(GuardOwner::Discovery));
}

/// An action's own `no_save` (not the discovery's) guards that call only:
/// its save is blocked, the guard is lifted when the call ends, and the
/// next action - which does not say it - may save. The discovery itself
/// never becomes one that must not save. A batch's `no_save` lasts the
/// whole batch, and is lifted when it ends.
#[tokio::test]
async fn a_no_save_action_guards_only_its_own_call() {
    use v2_lib::ai_bridge::{discover_actions_in, discovery_for, mark_discovery_for};
    use v2_lib::browser::cdp::Driver;
    let dir = root_with_recipe_and_account();
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "POST".into(),
        "https://hr.example.internal/api/leave/create".into(),
    ));
    let (mut browser, _) = slot(d, exploring("Leave"));
    let save = Action::Click { selector: "#save".into() };

    // Action 1 says no_save: guarded, its save blocked.
    mark_discovery_for(&mut browser, discovery_for(dir.path(), &json!({ "no_save": true }).to_string()).unwrap());
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], false, "the call's save was not blocked: {body}");
    assert_eq!(v["no_save"], true, "{body}");
    assert_eq!(v["blocked_writes"], json!(["POST /api/leave/create"]), "{body}");
    let b = browser.as_ref().unwrap();
    let methods: Vec<&str> = b.d.calls.iter().map(|(m, _)| m.as_str()).collect();
    let on = methods.iter().position(|m| *m == "Fetch.enable").expect("the call was never guarded");
    let click = methods.iter().position(|m| *m == "Input.dispatchMouseEvent").expect("no click");
    let off = methods.iter().rposition(|m| *m == "Fetch.disable").expect("the call's guard was never lifted");
    assert!(on < click && click < off, "{methods:?}");
    assert!(!b.d.is_guarding_saves(), "the guard outlived its call");
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (None, None));
    assert!(!b.discovery.as_ref().unwrap().no_save, "an action's no_save became the discovery's");
    assert_eq!(b.d.saves_stopped.len(), 0, "{:?}", b.d.saves_stopped);

    // Action 2 does not say it: unguarded, so its save goes through.
    browser.as_mut().unwrap().d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "PUT".into(),
        "https://hr.example.internal/api/leave/7".into(),
    ));
    mark_discovery_for(&mut browser, discovery_for(dir.path(), "{}").unwrap());
    let enables_before = browser.as_ref().unwrap().d.calls_to("Fetch.enable").len();
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "the next action was still guarded: {body}");
    assert!(v.get("no_save").is_none() && v.get("blocked_writes").is_none(), "{body}");
    let b = browser.as_ref().unwrap();
    assert_eq!(b.d.calls_to("Fetch.enable").len(), enables_before, "the next action was guarded");
    assert!(!b.d.is_guarding_saves());
    assert!(b.d.saves_stopped.is_empty(), "a save was stopped: {:?}", b.d.saves_stopped);

    // A batch that says no_save: every action of it guarded, lifted after.
    browser.as_mut().unwrap().d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "POST".into(),
        "https://hr.example.internal/api/leave/insert".into(),
    ));
    mark_discovery_for(&mut browser, discovery_for(dir.path(), &json!({ "no_save": true }).to_string()).unwrap());
    let stop = AtomicBool::new(false);
    let (status, text) =
        discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &[save.clone()], None, None, true, &stop).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("blocked POST /api/leave/insert"), "the batch's save was not blocked: {text}");
    let b = browser.as_ref().unwrap();
    assert!(!b.d.is_guarding_saves(), "the batch's guard outlived it");
    assert_eq!(b.guard_owner, None);
    assert!(!b.discovery.as_ref().unwrap().no_save);
}

/// Whether the discovery a replay ends must not save is decided as before
/// each of its actions (`discovery_must_not_save`): a case marked `no_save`
/// after the discovery's last action is carried into the replay too.
#[tokio::test]
async fn a_case_marked_after_the_last_action_is_carried_into_the_replay() {
    use v2_lib::ai_bridge::end_discovery_for_replay;
    let dir = root_with_recipe_and_account();
    save_case_77(dir.path(), false);
    let d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    let (mut browser, _) = slot(d, for_case(exploring("Leave"), 77));
    let open = Action::Click { selector: "#open".into() };
    let (status, _) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None, None).await;
    assert_eq!(status, 200);
    assert!(!browser.as_ref().unwrap().discovery.as_ref().unwrap().no_save);
    save_case_77(dir.path(), true);
    let ended = end_discovery_for_replay(&mut browser, dir.path()).await;
    assert!(ended.ended && ended.no_save, "a case marked after the last action was not carried: {ended:?}");
    // A script that cannot be read counts as marked, as before an action.
    let (mut browser, _) = slot(FakePage::default().driver(), for_case(exploring("Leave"), 78));
    let marked = find_file(dir.path(), "case-77.json").expect("case 77's script is not on disk");
    std::fs::write(marked.with_file_name("case-78.json"), "not json").unwrap();
    assert!(end_discovery_for_replay(&mut browser, dir.path()).await.no_save, "an unreadable script was not carried");
}

/// The file named `name` somewhere under `dir`.
fn find_file(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
    }
    None
}

/// A call's own `no_save` is never lifted from under the discovery's: in a
/// discovery for a case marked `no_save`, a call that says it too ends with
/// the guard still on.
#[tokio::test]
async fn a_calls_no_save_never_lifts_the_discoverys_guard() {
    use v2_lib::ai_bridge::{discovery_for, mark_discovery_for};
    use v2_lib::browser::cdp::Driver;
    let dir = root_with_recipe_and_account();
    save_case_77(dir.path(), true);
    let d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    let (mut browser, _) = slot(d, for_case(exploring("Leave"), 77));
    mark_discovery_for(&mut browser, discovery_for(dir.path(), &json!({ "no_save": true }).to_string()).unwrap());
    let open = Action::Click { selector: "#open".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None, None).await;
    assert_eq!(status, 200, "{body}");
    let b = browser.as_ref().unwrap();
    assert!(b.d.is_guarding_saves(), "the call lifted the guard its case's mark needs");
    assert!(b.d.calls_to("Fetch.disable").is_empty());
    assert_eq!(b.guard_owner, Some(GuardOwner::Discovery));
}

/// A call's guard is tagged with the case THAT call named, not the case the
/// discovery is for: in a discovery for case 5 (not marked), a call naming
/// case 77 (marked `no_save`) is guarded for case 77. Seen when the
/// browser will not lift the call's guard as the call ends, so it stays.
#[tokio::test]
async fn a_calls_guard_is_tagged_with_the_calls_own_case() {
    use v2_lib::ai_bridge::{discovery_for, mark_discovery_for};
    let dir = root_with_recipe_and_account();
    save_case_77(dir.path(), true);
    let d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list").answering_first(
        |method, _| {
            (method == "Fetch.disable").then(|| {
                Err(v2_lib::browser::cdp::CdpError::Protocol { method: method.into(), message: "busy".into() })
            })
        },
    );
    let (mut browser, _) = slot(d, for_case(exploring("Leave"), 5));
    mark_discovery_for(&mut browser, discovery_for(dir.path(), &json!({ "case_id": 77 }).to_string()).unwrap());
    let open = Action::Click { selector: "#open".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None, None).await;
    assert_eq!(status, 200, "{body}");
    let b = browser.as_ref().unwrap();
    // (The fake records a refused Fetch.disable as sent, so it is asked
    // whether the guard went on, not whether it is on now.)
    assert!(!b.d.calls_to("Fetch.enable").is_empty(), "the call ran unguarded");
    assert_eq!(b.discovery.as_ref().unwrap().case_id, Some(5), "the call's case became the discovery's");
    assert_eq!(b.guard_owner, Some(GuardOwner::Call { case_id: Some(77) }), "the call's guard names the wrong case");
}

/// Ending a discovery lifts its guard: its browser is kept, signed in, and
/// unguarded, so the next replay of a case that may save is not stopped by
/// a guard the discovery left. The assistant's replay that ends a no_save
/// discovery is told so (`end_discovery_for_replay`), to guard itself, the
/// discovery's last call. With no discovery going, ending lifts nothing: any
/// other guard - the person's, a call's - is not a discovery's to lift.
#[tokio::test]
async fn ending_a_discovery_lifts_its_guard() {
    use v2_lib::ai_bridge::end_discovery_for_replay;
    use v2_lib::browser::cdp::Driver;
    let dir = root_with_recipe_and_account();
    // Going: the browser is kept, and its guard lifted.
    let started = exploring("Leave").map(|s| DiscoveryState { no_save: true, ..s });
    let (mut browser, closed) = slot(leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/x"), started);
    let open = Action::Click { selector: "#open".into() };
    let (status, _) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None, None).await;
    assert_eq!(status, 200);
    assert_eq!(browser.as_ref().unwrap().guard_owner, Some(GuardOwner::Discovery));
    assert!(browser.as_ref().unwrap().d.is_guarding_saves());
    let (status, said) = end_discovery_in(&mut browser).await;
    assert_eq!((status, said.as_str()), (200, v2_lib::ai_bridge::DISCOVERY_ENDED_KEPT));
    assert!(!closed.load(Ordering::SeqCst), "ending the discovery closed its browser");
    let b = browser.as_ref().expect("ending the discovery let its browser go");
    assert!(!b.d.is_guarding_saves(), "the discovery's guard outlived it");
    assert_eq!((b.guarded_case, b.guard_owner.clone(), b.discovery.is_none()), (None, None, true));

    // A guard the browser will not lift is never left behind with no
    // discovery to end it: the browser is closed through its normal close.
    let started = exploring("Leave").map(|s| DiscoveryState { no_save: true, ..s });
    let refusing = ScriptedDriver::new(|method, _| match method {
        "Fetch.disable" => Err(v2_lib::browser::cdp::CdpError::Protocol { method: method.into(), message: "busy".into() }),
        _ => Ok(json!({})),
    });
    let (mut browser, closed) = slot(refusing, started);
    browser.as_mut().unwrap().assistant = true;
    browser.as_mut().unwrap().d.calls.push(("Fetch.enable".into(), json!({})));
    browser.as_mut().unwrap().guard_owner = Some(GuardOwner::Discovery);
    let (status, said) = end_discovery_in(&mut browser).await;
    assert_eq!((status, said.as_str()), (200, "the discovery is over and its browser is closed"));
    assert!(closed.load(Ordering::SeqCst) && browser.is_none(), "a guarded browser outlived its discovery");

    // The replay's end says whether the discovery it ended must not save.
    let started = exploring("Leave").map(|s| DiscoveryState { no_save: true, ..s });
    let (mut browser, _) = slot(FakePage::default().driver(), started);
    assert!(end_discovery_for_replay(&mut browser, dir.path()).await.no_save, "a no_save discovery's end did not say so");
    let (mut browser, _) = slot(FakePage::default().driver(), exploring("Leave"));
    assert!(!end_discovery_for_replay(&mut browser, dir.path()).await.no_save);

    // A call's guard, with no discovery going, is not a discovery's either.
    let mut d = FakePage::default().driver();
    d.guard_saves(&[]).await.unwrap();
    let (mut browser, closed) = slot(d, None);
    browser.as_mut().unwrap().guard_owner = Some(GuardOwner::Call { case_id: None });
    let (status, said) = end_discovery_in(&mut browser).await;
    assert_eq!((status, said.as_str()), (200, "no discovery is going"));
    let b = browser.as_ref().expect("ending no discovery closed the browser");
    assert!(!closed.load(Ordering::SeqCst));
    assert!(b.d.is_guarding_saves(), "ending no discovery lifted a call's guard");

    // The person's guard is not a discovery's.
    let mut d = FakePage::default().driver();
    d.guard_saves(&[]).await.unwrap();
    let (mut theirs, _) = slot(d, None);
    theirs.as_mut().unwrap().guard_owner = Some(GuardOwner::Person);
    theirs.as_mut().unwrap().guarded_case = Some(5);
    let (status, said) = end_discovery_in(&mut theirs).await;
    assert_eq!((status, said.as_str()), (200, "no discovery is going"));
    let b = theirs.as_ref().unwrap();
    assert!(b.d.is_guarding_saves(), "ending no discovery lifted the person's guard");
    assert_eq!((b.guarded_case, b.guard_owner.clone()), (Some(5), Some(GuardOwner::Person)));
}

/// An ordinary discovery still only reports what the page wrote: no guard
/// goes on, and the action keeps its own outcome.
#[tokio::test]
async fn an_ordinary_discovery_only_reports_a_create() {
    let dir = root_with_recipe_and_account();
    save_case_77(dir.path(), false);
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    d.on_call_events.push(("Input.dispatchMouseEvent".into(), sent("1", "POST", "https://hr.example.internal/api/leave/create")));
    // Named, but not marked: still an ordinary discovery.
    let (mut browser, _) = slot(d, for_case(exploring("Leave"), 77));
    let save = Action::Click { selector: "#save".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    assert_eq!(v["writes"], json!([{ "method": "POST", "path": "/api/leave/create" }]), "{body}");
    assert!(v.get("blocked_writes").is_none(), "{body}");
    assert!(v.get("no_save").is_none(), "{body}");
    let b = browser.as_ref().unwrap();
    assert!(b.d.calls_to("Fetch.enable").is_empty(), "an ordinary discovery was guarded");
    assert!(!b.discovery.as_ref().unwrap().no_save);
}

/// A case marked `no_save` after the discovery started is guarded from the
/// next action: the script is read again each time.
#[tokio::test]
async fn a_no_save_mark_added_mid_discovery_counts_from_the_next_action() {
    let dir = root_with_recipe_and_account();
    save_case_77(dir.path(), false);
    let d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    let (mut browser, _) = slot(d, for_case(exploring("Leave"), 77));
    let open = Action::Click { selector: "#open".into() };
    let (status, _) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None, None).await;
    assert_eq!(status, 200);
    assert!(browser.as_ref().unwrap().d.calls_to("Fetch.enable").is_empty());
    save_case_77(dir.path(), true);
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None, None).await;
    assert_eq!(status, 200, "{body}");
    assert!(!browser.as_ref().unwrap().d.calls_to("Fetch.enable").is_empty(), "the new mark was not guarded");
    assert!(browser.as_ref().unwrap().discovery.as_ref().unwrap().no_save);
}

/// When the guard cannot go on, the action is refused with the guard's own
/// sentence and never runs: such a discovery never acts unguarded.
#[tokio::test]
async fn a_no_save_discovery_whose_guard_cannot_go_on_runs_nothing() {
    let dir = root_with_recipe_and_account();
    let nav = v2_lib::autorun::nav::nav_path(dir.path(), ORG, PROJECT);
    std::fs::create_dir_all(nav.parent().unwrap()).unwrap();
    std::fs::write(&nav, "not json").unwrap();
    let d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    let state = exploring("Leave").map(|s| DiscoveryState { no_save: true, ..s });
    let (mut browser, _) = slot(d, state);
    let save = Action::Click { selector: "#save".into() };
    let (status, out) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.starts_with(v2_lib::browser::save_guard::SETUP_FAILED), "{out}");
    assert!(browser.as_ref().unwrap().d.calls_to("Input.dispatchMouseEvent").is_empty(), "the click ran unguarded");
}

/// A discovery started for a no-save case puts its guard on once the
/// sign-in has arrived and before the landing page is read, and says so.
#[tokio::test]
async fn a_discovery_started_for_a_no_save_case_is_guarded_after_the_sign_in() {
    use v2_lib::browser::cdp::Driver;
    let dir = root_with_recipe_and_account();
    save_case_77(dir.path(), true);
    let (mut browser, closed) = slot(signin_app(true), for_case(opened_for_discovery(), 77));
    let (status, body) =
        discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some("Leave"), &quick()).await;
    assert_eq!(status, 200, "{body}");
    assert!(!closed.load(Ordering::SeqCst));
    assert_eq!(parsed(&body)["no_save"], true, "{body}");
    let b = browser.as_ref().unwrap();
    assert!(b.d.is_guarding_saves(), "the discovery's guard is not on");
    let methods: Vec<&str> = b.d.calls.iter().map(|(m, _)| m.as_str()).collect();
    let guard = methods.iter().position(|m| *m == "Fetch.enable").expect("the guard never went on");
    let sign_in = methods.iter().position(|m| *m == "Input.dispatchMouseEvent").expect("no sign-in click");
    let landing = methods.iter().rposition(|m| *m == "Accessibility.getFullAXTree").expect("no page read");
    assert!(sign_in < guard && guard < landing, "{methods:?}");
    let state = b.discovery.as_ref().unwrap();
    assert_eq!(state.case_id, Some(77));
    assert!(state.no_save);
}

/// A batch in a no-save discovery says on each line what was blocked, and
/// counts the blocks under the no-save guard, not a mapping run.
#[tokio::test]
async fn a_no_save_batch_names_each_blocked_save() {
    let dir = root_with_recipe_and_account();
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list");
    d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "POST".into(),
        "https://hr.example.internal/api/leave/insert".into(),
    ));
    let state = exploring("Leave").map(|s| DiscoveryState { no_save: true, ..s });
    let (mut browser, _) = slot(d, state);
    let actions = vec![Action::Click { selector: "#save".into() }];
    let stop = AtomicBool::new(false);
    let (status, text) =
        v2_lib::ai_bridge::discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &stop)
            .await;
    assert_eq!(status, 200, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("1. failed:") && lines[0].contains("; blocked POST /api/leave/insert"), "{text}");
    assert!(lines.contains(&"Saves blocked by the no-save guard: 1."), "{text}");
}

/// What a discovery call names: a case marked `no_save` is enough, a case
/// with no script or an ordinary one is not, and a wrong value is refused.
#[test]
fn a_discovery_call_names_its_case_and_whether_it_must_not_save() {
    use v2_lib::ai_bridge::discovery_for;
    let dir = TempDir::new();
    save_case_77(dir.path(), true);
    let marked = discovery_for(dir.path(), &json!({ "case_id": 77 }).to_string()).unwrap();
    assert_eq!((marked.case_id, marked.no_save), (Some(77), true));
    let unknown = discovery_for(dir.path(), &json!({ "case_id": 78 }).to_string()).unwrap();
    assert_eq!((unknown.case_id, unknown.no_save), (Some(78), false));
    let told = discovery_for(dir.path(), &json!({ "no_save": true }).to_string()).unwrap();
    assert_eq!((told.case_id, told.no_save), (None, true));
    let neither = discovery_for(dir.path(), "{}").unwrap();
    assert_eq!((neither.case_id, neither.no_save), (None, false));
    for bad in [json!({ "case_id": "77" }), json!({ "case_id": -1 }), json!({ "no_save": "yes" })] {
        let (status, _) = discovery_for(dir.path(), &bad.to_string()).unwrap_err();
        assert_eq!(status, 400, "{bad}");
    }
}

/// A person's Run step is refused while a discovery holds the browser: it
/// would act behind the assistant's back, and could lift a mapping run's
/// save guard. Refused under the session's lock, before the guard is
/// touched, with the busy sentence.
#[tokio::test]
async fn a_step_is_refused_while_a_discovery_holds_the_browser() {
    let (mut discovering, _) = slot(signin_app(true), mapping(exploring("Leave"), &["Leave"]));
    assert_eq!(refuse_while_discovering(&mut discovering), Err(busy_browser_sentence(true).to_string()));
    let (mut theirs, _) = slot(signin_app(true), None);
    assert_eq!(refuse_while_discovering(&mut theirs), Ok(()));

    let source = include_str!("../../src/commands/autorun.rs");
    let step = &source[source.find("pub async fn auto_run_step").unwrap()..];
    let step = &step[..step.find("guard_supervised(").unwrap()];
    assert!(step.contains("refuse_while_discovering(&mut slot)?"), "a step does not refuse a discovery's browser");
}

/// An ordinary discovery is not a mapping run: no guard goes on, the
/// page's saves go through, and the answer carries no `blocked`.
#[tokio::test]
async fn ordinary_discovery_still_allows_saves() {
    use v2_lib::browser::cdp::Driver;
    let dir = root_with_recipe_and_account();

    let (mut browser, _) = slot(signin_app(true), opened_for_discovery());
    let (status, body) =
        discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some("Leave"), &quick()).await;
    assert_eq!(status, 200, "{body}");
    let b = browser.as_ref().unwrap();
    assert!(!b.d.is_guarding_saves());
    assert!(b.d.calls_to("Fetch.enable").is_empty(), "an ordinary discovery was guarded");
    assert!(b.discovery.as_ref().unwrap().mapping.is_none());

    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list?page=2");
    d.on_call_events.push((
        "Input.dispatchMouseEvent".into(),
        sent("1", "POST", "https://hr.example.internal/hr/leave/save?id=5"),
    ));
    d.saves_on_call.push((
        "Input.dispatchMouseEvent".into(),
        "POST".into(),
        "https://hr.example.internal/hr/leave/save?id=5".into(),
    ));
    let (mut browser, _) = slot(d, exploring("Leave"));
    let save = Action::Click { selector: "#save".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    assert!(v.get("blocked").is_none(), "{body}");
    assert_eq!(v["writes"], json!([{ "method": "POST", "path": "/hr/leave/save" }]), "{body}");
    assert!(browser.as_ref().unwrap().d.saves_stopped.is_empty(), "a save was stopped");
}

// ------------------------------------------------- what a page read files

/// A page read in a browser a discovery holds stamps its area explored, by
/// the discovery's account KEY - not by whoever the browser last signed in
/// as. Outside a discovery the same read stamps nothing.
#[tokio::test]
async fn a_discovery_page_read_stamps_explored_at_and_the_account_key() {
    let dir = TempDir::new();
    let state = DiscoveryState {
        area: Some("Leave".into()),
        account: Some("admin".into()),
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
        explore: None,
    };
    let at = discovery_sighting(dir.path(), ORG, PROJECT, Some(&state), None, Some("manager"))
        .expect("a project is chosen, so there is somewhere to file it");
    assert!(at.discovering.is_some());
    let (status, text) = read_page(&mut signin_app(true), DEFAULT_LIMIT, Some(&at)).await;
    assert_eq!(status, 200, "{text}");
    let area = mapped_area(dir.path(), "Leave").expect("nothing was recorded");
    assert!(area.explored_at.is_some(), "a discovery's page read did not stamp the area");
    assert_eq!(area.account.as_deref(), Some("admin"));

    let other = TempDir::new();
    let at = discovery_sighting(other.path(), ORG, PROJECT, None, None, Some("manager")).unwrap();
    assert!(at.discovering.is_none());
    let (status, _) = read_page(&mut signin_app(true), DEFAULT_LIMIT, Some(&at)).await;
    assert_eq!(status, 200);
    let area = mapped_area(other.path(), "").expect("nothing was recorded");
    assert_eq!(area.explored_at, None, "a page read outside a discovery is not one");

    assert!(discovery_sighting(dir.path(), ORG, "  ", Some(&state), None, None).is_none(), "no project, nowhere to file");
}

/// A navigate's address is kept by its path only: neither its host nor its
/// query string reaches the map - not the write log's step, not the
/// outcome line.
#[tokio::test]
async fn a_navigate_action_never_puts_a_host_or_query_in_the_map_or_its_outcome() {
    let dir = root_with_recipe_and_account();
    let mut d = leave_page("Page.navigate", "https://hr.example.internal/x/y?token=abc");
    d.on_call_events.push(("Page.navigate".into(), sent("1", "POST", "https://hr.example.internal/x/track?token=abc")));
    d.on_every_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }) },
    ));
    let (mut browser, _) = slot(d, exploring("Leave"));
    let go = Action::Navigate { url: "https://hr.example.internal/x/y?token=abc".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &go, None, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["ok"], true, "{body}");

    let area = mapped_area(dir.path(), "Leave").unwrap();
    assert_eq!(area.writes.len(), 1, "{:?}", area.writes);
    assert!(!area.outcomes.is_empty(), "no outcome was recorded");
    let file = std::fs::read_to_string(map_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(!file.contains("token"), "a query string reached the map: {file}");
    assert!(!file.contains("hr.example.internal"), "a host reached the map: {file}");
}

// ------------------------------------------------------- saving an area

/// A small menu-driven application. `#go` signs in (landing on
/// `/hr/home/index`), `#leave` opens `/hr/leave` and `#apply` opens
/// `/hr/leave/apply`; `#marker` shows only while signed in, and `#missing`
/// is never on the page. `#menu` opens `/hr/menu`, which alone holds a
/// Payroll button, and `#balance` opens `/hr/leave/balance`, which alone
/// holds an Approve button. Its address always carries a query string.
fn menu_app(signed_in: bool, at: &str) -> ScriptedDriver {
    menu_app_dying_on(signed_in, at, "")
}

/// `menu_app`, whose browser stops answering when the element `dies_on`
/// is clicked ("" never).
fn menu_app_dying_on(signed_in: bool, at: &str, dies_on: &'static str) -> ScriptedDriver {
    let path = Arc::new(Mutex::new(at.to_string()));
    let signed = Arc::new(AtomicBool::new(signed_in));
    let mut last_css = String::new();
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        if !dies_on.is_empty() && method == "Input.dispatchMouseEvent" && last_css == dies_on {
            return Err(v2_lib::browser::cdp::CdpError::Closed);
        }
        Ok(match method {
            "Page.navigate" => {
                *path.lock().unwrap() = v2_lib::autorun::nav::path_of(params["url"].as_str().unwrap_or(""));
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => json!({ "result": {
                "value": format!("https://hr.example.internal{}?token=t0p-secret#top", path.lock().unwrap())
            } }),
            "Runtime.evaluate" if params["expression"] == "document.title" => json!({ "result": { "value": "Home" } }),
            "Runtime.evaluate" => {
                json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } })
            }
            "Accessibility.getFullAXTree" => {
                let own = match path.lock().unwrap().as_str() {
                    "/hr/menu" => Some("Payroll"),
                    "/hr/leave/balance" => Some("Approve"),
                    _ => None,
                };
                let mut nodes = vec![
                    json!({ "nodeId": "1", "ignored": false, "role": { "value": "form" }, "name": { "value": "Leave" },
                      "childIds": if own.is_some() { json!(["2", "3", "4"]) } else { json!(["2", "3"]) } }),
                    json!({ "nodeId": "2", "ignored": false, "role": { "value": "textbox" }, "name": { "value": "Name" },
                      "childIds": [] }),
                    json!({ "nodeId": "3", "ignored": false, "role": { "value": "button" }, "name": { "value": "Save" },
                      "childIds": [] }),
                ];
                if let Some(own) = own {
                    nodes.push(json!({ "nodeId": "4", "ignored": false, "role": { "value": "button" },
                      "name": { "value": own }, "childIds": [] }));
                }
                json!({ "nodes": nodes })
            }
            "Network.getAllCookies" => json!({ "cookies": [] }),
            "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": ready_probe() } }),
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == FOCUS_JS => json!({ "result": { "value": "text" } }),
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = match last_css.as_str() {
                    "#marker" => signed.load(Ordering::SeqCst),
                    "#missing" => false,
                    _ => true,
                };
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                let to = match last_css.as_str() {
                    "#go" => {
                        signed.store(true, Ordering::SeqCst);
                        Some("/hr/home/index")
                    }
                    "#leave" => Some("/hr/leave"),
                    "#apply" => Some("/hr/leave/apply"),
                    "#menu" => Some("/hr/menu"),
                    "#balance" => Some("/hr/leave/balance"),
                    _ => None,
                };
                if let Some(to) = to {
                    *path.lock().unwrap() = to.to_string();
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
    d
}

fn css(sel: &str) -> Target {
    serde_json::from_value(json!({ "css": sel })).unwrap()
}

/// Clicks that arrive where the assistant stands are replayed from home
/// and saved as an area - module, clicks, where they arrived, when, and
/// the home page they start from - and the discovery moves to it.
#[tokio::test]
async fn an_area_whose_clicks_arrive_is_saved_and_becomes_current() {
    let dir = root_with_recipe_and_account();
    let (mut browser, closed) = slot(menu_app(true, "/hr/leave/apply"), exploring("Leave"));
    browser.as_mut().unwrap().account = Some("admin".into());

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, " Leave Apply ", " Leave ", clicks.clone(), &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["saved"], true, "{body}");
    assert_eq!(v["arrived"], "/hr/leave/apply", "{body}");
    assert!(!body.contains("t0p-secret"), "{body}");
    assert!(!closed.load(Ordering::SeqCst));

    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    let saved = find_area(&nav, "leave apply").expect("the area was not saved");
    assert_eq!(saved.area, "Leave Apply");
    assert_eq!(saved.module, "Leave");
    assert_eq!(saved.clicks, clicks);
    assert_eq!(saved.arrived, "/hr/leave/apply");
    assert_eq!(saved.start, "/", "the home page the clicks start from");
    let r = saved.recorded.as_bytes();
    assert!(r.len() == 20 && r[4] == b'-' && r[10] == b'T' && r[19] == b'Z', "{}", saved.recorded);

    let state = browser.as_ref().unwrap().discovery.as_ref().unwrap();
    assert_eq!(state.area.as_deref(), Some("Leave Apply"));
}

/// A browser whose saved session has gone signs in again, as the
/// discovery's account, before the clicks are replayed.
#[tokio::test]
async fn an_area_is_checked_after_signing_in_again_when_the_session_is_gone() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(menu_app(false, "/hr/leave/apply"), exploring("Leave"));

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(browser.as_ref().unwrap().account.as_deref(), Some("admin"), "it did not sign in again");
    assert!(find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Leave Apply").is_some());
}

/// Clicks that do not arrive are refused with where they stopped and what
/// the page showed - at most 40 lines of it - and nothing is saved.
#[tokio::test]
async fn an_area_whose_clicks_do_not_arrive_is_refused_with_what_the_page_showed() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(menu_app(true, "/hr/leave/apply"), exploring("Leave"));
    browser.as_mut().unwrap().account = Some("admin".into());

    let clicks = vec![css("#leave"), css("#missing")];
    let (status, out) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.starts_with("The clicks did not arrive: click 2"), "{out}");
    let (_, showed) = out.split_once(". The page showed: ").expect("no page in the refusal");
    assert!(showed.contains("button \"Save\""), "{out}");
    assert!(showed.lines().count() <= 40, "{out}");
    assert!(!out.contains("t0p-secret"), "{out}");

    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    assert!(find_area(&nav, "Leave Apply").is_none(), "a path that did not arrive was saved");
    assert_eq!(browser.as_ref().unwrap().discovery.as_ref().unwrap().area.as_deref(), Some("Leave"));
}

/// A name already taken is refused before the browser is touched.
#[tokio::test]
async fn an_existing_area_name_is_refused() {
    let dir = root_with_recipe_and_account();
    let existing = ModulePath {
        area: "Leave Apply".into(),
        module: "Leave".into(),
        clicks: vec![css("#leave")],
        arrived: "/hr/leave".into(),
        recorded: "2026-10-01T00:00:00Z".into(),
        start: String::new(),
        made_by: MadeBy::Person,
        repaired_by_assistant: None,
        preferences: Default::default(),
        mode: None,
    };
    put_path(dir.path(), ORG, PROJECT, existing.clone()).unwrap();
    let (mut browser, _) = slot(menu_app(true, "/hr/leave/apply"), exploring("Leave"));

    let (status, out) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "leave apply", "Leave", vec![css("#apply")], &quick())
            .await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(
        out,
        "An area named 'leave apply' already exists; pick another name or ask the person to replace it in Auto Run"
    );
    assert!(browser.as_ref().unwrap().d.calls_to("Page.navigate").is_empty(), "the browser was touched");
    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(find_area(&nav, "Leave Apply"), Some(&existing), "the existing area was changed");
}

// ------------------------------- an area's clicks match exactly one element

/// What `objectives_app` is built to do.
#[derive(Clone)]
struct Objectives {
    /// A second link of the name, hidden, inside `#recent`.
    twin: bool,
    /// The one list that holds only the seen link has an id.
    scoping_id: bool,
    /// The backend id the link inside `#admin-list` has (100 is the seen link).
    scoped_finds: i64,
    /// The link is drawn this long after the page was first asked for it.
    shows_after_ms: u64,
    /// Asking the page for the scoping id fails.
    pin_call_fails: bool,
}

impl Default for Objectives {
    fn default() -> Self {
        Objectives { twin: false, scoping_id: false, scoped_finds: 100, shows_after_ms: 0, pin_call_fails: false }
    }
}

fn objectives_app(twin: bool, scoping_id: bool) -> ScriptedDriver {
    objectives(Objectives { twin, scoping_id, ..Objectives::default() })
}

/// A page whose `#admin` button opens `/hr/admin`, where link "Strategic
/// Objectives" (`ax-100`, seen, inside `#admin-list`, which is inside
/// `#ehrm-modulemenus-8`) opens `/hr/objectives`. See `Objectives` for the
/// twin and the rest. `#marker` is always there.
fn objectives(cfg: Objectives) -> ScriptedDriver {
    let path = Arc::new(Mutex::new("/hr/objectives".to_string()));
    let asked = Mutex::new(None::<std::time::Instant>);
    let mut last_css = String::new();
    let mut last_probed = String::new();
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let object = params["objectId"].as_str().unwrap_or("").to_string();
        Ok(match method {
            "Page.navigate" => {
                *path.lock().unwrap() = v2_lib::autorun::nav::path_of(params["url"].as_str().unwrap_or(""));
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => json!({ "result": {
                "value": format!("https://hr.example.internal{}", path.lock().unwrap())
            } }),
            "Runtime.evaluate" => json!({ "result": { "value": null } }),
            "Accessibility.queryAXTree" if params["role"] == "link" => {
                let node = |id: i64| {
                    json!({ "nodeId": format!("n{id}"), "role": { "value": "link" },
                        "name": { "value": "Strategic Objectives" }, "backendDOMNodeId": id })
                };
                // The link is drawn some time after the page first looked.
                let began = *asked.lock().unwrap().get_or_insert_with(std::time::Instant::now);
                if began.elapsed().as_millis() < u128::from(cfg.shows_after_ms) {
                    return Ok(json!({ "nodes": [] }));
                }
                // The seen link is under the document, `#ehrm-modulemenus-8`
                // and `#admin-list`; the twin under the document,
                // `#ehrm-modulemenus-8` and `#recent`.
                let mut nodes = vec![];
                match object.as_str() {
                    "css:#recent" => {}
                    "css:#admin-list" => nodes.push(node(cfg.scoped_finds)),
                    _ => nodes.push(node(100)),
                }
                if cfg.twin && matches!(object.as_str(), "doc" | "css:#recent" | "css:#ehrm-modulemenus-8") {
                    nodes.push(node(101));
                }
                json!({ "nodes": nodes })
            }
            "Accessibility.queryAXTree" => json!({ "nodes": [] }),
            "DOM.resolveNode" => json!({ "object": { "objectId": format!("ax-{}", params["backendNodeId"]) } }),
            "DOM.describeNode" => {
                let n: i64 = object.strip_prefix("ax-").and_then(|n| n.parse().ok()).unwrap_or(1);
                json!({ "node": { "backendNodeId": n } })
            }
            "Runtime.callFunctionOn" if f == PROBE_JS => {
                last_probed = object.clone();
                json!({ "result": { "value": ready_probe() } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS => {
                json!({ "result": { "value": object != "ax-101" } })
            }
            "Runtime.callFunctionOn" if f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if f == v2_lib::autorun::nav::PIN_ID_JS => {
                if cfg.pin_call_fails {
                    return Err(v2_lib::browser::cdp::CdpError::Protocol {
                        method: "Runtime.callFunctionOn".into(),
                        message: "Argument should belong to the same JavaScript world as target object".into(),
                    });
                }
                // Asked of the seen link, with the twin as the argument.
                let right = object == "ax-100" && params["arguments"][0]["objectId"] == "ax-101";
                json!({ "result": { "value": if right && cfg.scoping_id { json!("admin-list") } else { Value::Null } } })
            }
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => json!({ "result": [
                { "name": "0", "value": { "objectId": format!("css:{}", last_css) } }
            ] }),
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                match last_probed.as_str() {
                    "css:#admin" => *path.lock().unwrap() = "/hr/admin".to_string(),
                    "ax-100" => *path.lock().unwrap() = "/hr/objectives".to_string(),
                    _ => {}
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
    d
}

fn link(name: &str) -> Target {
    serde_json::from_value(json!({ "role": "link", "name": name })).unwrap()
}

/// A click whose name a second, hidden element shares is saved inside the
/// one list that holds only it: a chain from that list's id to the click.
#[tokio::test]
async fn an_area_click_with_a_hidden_twin_is_saved_scoped_to_its_own_list() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(objectives_app(true, true), exploring("Admin"));
    browser.as_mut().unwrap().account = Some("admin".into());

    let clicks = vec![css("#admin"), link("Strategic Objectives")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Objectives", "Admin", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["saved"], true, "{body}");
    assert_eq!(v["pinned"], json!(["click 2: scoped to #admin-list"]), "{body}");

    let saved = find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Objectives").cloned().expect("not saved");
    let want: Target = serde_json::from_value(json!([
        { "css": "#admin-list" },
        { "role": "link", "name": "Strategic Objectives" }
    ]))
    .unwrap();
    assert_eq!(saved.clicks, vec![css("#admin"), want]);
}

/// A twin and no id to scope by: the save is refused, saying so, and the
/// areas file is not written.
#[tokio::test]
async fn an_area_click_with_a_twin_and_no_scoping_id_is_refused() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(objectives_app(true, false), exploring("Admin"));
    browser.as_mut().unwrap().account = Some("admin".into());

    let clicks = vec![css("#admin"), link("Strategic Objectives")];
    let (status, out) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Objectives", "Admin", clicks, &quick()).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.starts_with("The clicks did not arrive: "), "{out}");
    assert!(out.contains("counting hidden ones"), "{out}");
    assert!(out.contains("click 2"), "{out}");
    assert!(!nav_path(dir.path(), ORG, PROJECT).exists(), "the areas file was written");
    assert!(find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Objectives").is_none());
}

/// A click that is the only element of its name is saved with the name
/// exact, so a longer name added later cannot make it match two.
#[tokio::test]
async fn a_unique_area_click_is_saved_exact() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(objectives_app(false, false), exploring("Admin"));
    browser.as_mut().unwrap().account = Some("admin".into());

    let clicks = vec![css("#admin"), link("Strategic Objectives")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Objectives", "Admin", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["pinned"], json!(["click 2: exact name"]), "{body}");

    let saved = find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Objectives").cloned().expect("not saved");
    let want: Target = serde_json::from_value(json!({ "role": "link", "name": "Strategic Objectives", "exact": true })).unwrap();
    assert_eq!(saved.clicks, vec![css("#admin"), want]);
}

fn chain(steps: Value) -> Target {
    serde_json::from_value(steps).unwrap()
}

/// A discovery in the Admin module, signed in, standing on `driver`.
fn admin_discovery(driver: ScriptedDriver, state: Option<DiscoveryState>) -> Option<FakeBrowser> {
    let (mut browser, _) = slot(driver, state);
    browser.as_mut().unwrap().account = Some("admin".into());
    browser
}

/// Save Objectives with clicks `#admin` then `second`, in a fresh project.
async fn save_objectives(
    cfg: Objectives,
    second: Target,
) -> (TempDir, Option<FakeBrowser>, u16, String) {
    let dir = root_with_recipe_and_account();
    let mut browser = admin_discovery(objectives(cfg), exploring("Admin"));
    let (status, body) = discover_area_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        "Objectives",
        "Admin",
        vec![css("#admin"), second],
        &quick(),
    )
    .await;
    (dir, browser, status, body)
}

/// Row 75's own shape: the click already is a chain inside the menu that
/// holds both twins. The pin goes inside that menu's one list, and the
/// chain it keeps still finds exactly the clicked link.
#[tokio::test]
async fn a_chain_click_with_a_twin_in_its_own_scope_is_scoped_inside_it() {
    let own = chain(json!([{ "css": "#ehrm-modulemenus-8" }, { "role": "link", "name": "Strategic Objectives" }]));
    let cfg = Objectives { twin: true, scoping_id: true, ..Objectives::default() };
    let (dir, _, status, body) = save_objectives(cfg, own).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["pinned"], json!(["click 2: scoped to #admin-list"]), "{body}");
    let saved = find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Objectives").cloned().expect("not saved");
    let want = chain(json!([{ "css": "#admin-list" }, { "role": "link", "name": "Strategic Objectives" }]));
    assert_eq!(saved.clicks, vec![css("#admin"), want]);
}

/// A scope that finds exactly one element is not enough: it must be the
/// element that was clicked. Here the chain finds another link alone.
#[tokio::test]
async fn a_scoped_chain_that_finds_another_element_is_refused() {
    let cfg = Objectives { twin: true, scoping_id: true, scoped_finds: 102, ..Objectives::default() };
    let (dir, _, status, out) = save_objectives(cfg, link("Strategic Objectives")).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("counting hidden ones"), "{out}");
    assert!(find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Objectives").is_none());
}

/// A submenu that draws a moment after the click before it is still pinned:
/// the pin waits for the click to show, as the click itself does.
#[tokio::test]
async fn a_click_that_draws_late_is_still_pinned() {
    let cfg = Objectives { twin: true, scoping_id: true, shows_after_ms: 150, ..Objectives::default() };
    let (_, _, status, body) = save_objectives(cfg, link("Strategic Objectives")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["pinned"], json!(["click 2: scoped to #admin-list"]), "{body}");
}

/// A click the pin never saw (it draws after the pin gave up, but while the
/// click still waits) is saved as sent, and the answer says it was not
/// checked.
#[tokio::test]
async fn a_click_the_pin_never_saw_is_saved_as_sent_with_a_note() {
    let cfg = Objectives { twin: true, scoping_id: true, shows_after_ms: 600, ..Objectives::default() };
    let (dir, _, status, body) = save_objectives(cfg, link("Strategic Objectives")).await;
    assert_eq!(status, 200, "{body}");
    let notes = parsed(&body)["pinned"].clone();
    assert!(notes[0].as_str().is_some_and(|n| n.starts_with("click 2: could not be checked")), "{body}");
    let saved = find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Objectives").cloned().expect("not saved");
    assert_eq!(saved.clicks, vec![css("#admin"), link("Strategic Objectives")]);
}

/// The browser failing while the scope is looked for is said as that, not
/// as a page with no id.
#[tokio::test]
async fn a_browser_error_while_scoping_is_not_reported_as_a_page_with_no_id() {
    let cfg = Objectives { twin: true, scoping_id: true, pin_call_fails: true, ..Objectives::default() };
    let (_, _, status, out) = save_objectives(cfg, link("Strategic Objectives")).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("could not look"), "{out}");
    assert!(!out.contains("no part of the page with an id"), "{out}");
    assert!(out.contains("click 2"), "{out}");
}

/// A refused pin is reported at the click, so a mapping summary and the log
/// read it there and not at home.
#[tokio::test]
async fn a_pin_refusal_is_reported_at_its_click() {
    use v2_lib::autorun::nav::{go_to_module_pinning, Home, Route, Where};
    let dir = root_with_recipe_and_account();
    let mut d = objectives_app(true, false);
    let path = ModulePath {
        area: "Objectives".into(),
        module: "Admin".into(),
        clicks: vec![css("#admin"), link("Strategic Objectives")],
        arrived: "/hr/objectives".into(),
        recorded: "2026-10-01T00:00:00Z".into(),
        start: "/hr/objectives".into(),
        made_by: MadeBy::Person,
        repaired_by_assistant: None,
        preferences: Default::default(),
        mode: None,
    };
    let recipe = v2_lib::autorun::recipe::load_effective_recipe(dir.path(), ORG, PROJECT).unwrap();
    let route = Route { home: Home::of(&recipe), path };
    let failure = go_to_module_pinning(&mut d, &route, &quick()).await.unwrap_err();
    assert!(matches!(failure.at, Where::Click { n: 2, .. }), "{:?}", failure.at);
    assert!(!failure.harness);
}

/// An older mapping-made area with a role-and-name click.
fn older_objectives(made_by: MadeBy, arrived: &str) -> ModulePath {
    ModulePath {
        area: "Objectives".into(),
        module: "Admin".into(),
        clicks: vec![css("#admin"), link("Strategic Objectives")],
        arrived: arrived.into(),
        recorded: "2026-10-01T00:00:00Z".into(),
        start: "/".into(),
        made_by,
        repaired_by_assistant: None,
        preferences: Default::default(),
        mode: None,
    }
}

/// An area a mapping run made before clicks were pinned, whose role-and-name
/// click still matches one element, is found unchanged: no write, and the
/// run counts it unchanged (pinning would only add `exact`).
#[tokio::test]
async fn a_mapping_rerun_finds_an_older_area_unchanged_when_its_click_still_matches_one() {
    let dir = root_with_recipe_and_account();
    let older = older_objectives(MadeBy::Mapping, "/hr/objectives");
    put_path(dir.path(), ORG, PROJECT, older.clone()).unwrap();
    let before = file_state(dir.path());
    let mut browser = admin_discovery(objectives_app(false, false), mapping(exploring("Admin"), &["Admin"]));

    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Objectives", "Admin", older.clicks.clone(), &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!((v["saved"].clone(), v["unchanged"].clone()), (json!(false), json!(true)), "{body}");
    assert_eq!(file_state(dir.path()), before, "the areas file was written");
    let run = the_run(&browser);
    assert_eq!(run.unchanged, vec!["Objectives".to_string()]);
    assert!(run.updated.is_empty(), "{run:?}");
}

/// The same older area, whose click now has a twin on the page: the pinned
/// click is saved and reported, and the run counts the area updated.
#[tokio::test]
async fn a_mapping_rerun_saves_an_older_areas_click_pinned_once_it_has_a_twin() {
    let dir = root_with_recipe_and_account();
    let older = older_objectives(MadeBy::Mapping, "/hr/objectives");
    put_path(dir.path(), ORG, PROJECT, older.clone()).unwrap();
    let mut browser = admin_discovery(objectives_app(true, true), mapping(exploring("Admin"), &["Admin"]));

    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Objectives", "Admin", older.clicks.clone(), &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["saved"], true, "{body}");
    assert_eq!(v["pinned"], json!(["click 2: scoped to #admin-list"]), "{body}");
    let saved = find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Objectives").cloned().unwrap();
    assert_eq!(saved.clicks[1], chain(json!([{ "css": "#admin-list" }, { "role": "link", "name": "Strategic Objectives" }])));
    assert_eq!(the_run(&browser).updated.len(), 1);
}

/// A repair that sends the saved clicks again is not the same path when the
/// app pins them: here the saved path fails only because its recorded
/// arrival is wrong, and the pinned clicks differ from the saved ones.
#[tokio::test]
async fn a_repair_that_resends_the_old_clicks_is_judged_after_they_are_pinned() {
    let dir = root_with_recipe_and_account();
    let older = older_objectives(MadeBy::Person, "/hr/elsewhere");
    put_path(dir.path(), ORG, PROJECT, older.clone()).unwrap();
    let mut browser = admin_discovery(objectives_app(false, false), exploring("Admin"));

    let (status, out) = repair_area_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        "Objectives",
        "",
        older.clicks.clone(),
        "The arrival moved",
        &quick(),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    let saved = find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Objectives").cloned().unwrap();
    assert_eq!(saved.arrived, "/hr/objectives");
    assert_ne!(saved.clicks, older.clicks);
}

// ------------------------------------------- an assistant's repair

const MOVED: &str = "The Leave menu moved under Self Service";

/// A person's area "Leave Apply" under Leave, recorded with `clicks`.
fn persons_leave_apply(clicks: Vec<Target>) -> ModulePath {
    ModulePath {
        area: "Leave Apply".into(),
        module: "Leave".into(),
        clicks,
        arrived: "/hr/leave/apply".into(),
        recorded: "2026-10-01T00:00:00Z".into(),
        start: "/".into(),
        made_by: MadeBy::Person,
        repaired_by_assistant: None,
        preferences: Default::default(),
        mode: None,
    }
}

/// An ordinary discovery standing on `/hr/leave/apply`, signed in.
fn discovery_on_leave_apply() -> Option<FakeBrowser> {
    let (mut browser, _) = slot(menu_app(true, "/hr/leave/apply"), exploring("Leave"));
    browser.as_mut().unwrap().account = Some("admin".into());
    browser
}

/// A person's area whose saved clicks no longer arrive is repaired: the new
/// clicks replace them, the area keeps its name, module, maker and when it
/// was recorded, and the note keeps the old path and the reason. The answer
/// says so, and that the person can put the old path back.
#[tokio::test]
async fn a_repair_replaces_a_path_that_no_longer_arrives_and_is_marked() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#missing")])).unwrap();
    let mut browser = discovery_on_leave_apply();

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        repair_area_in(&mut browser, dir.path(), ORG, PROJECT, "leave apply", "", clicks.clone(), MOVED, &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["saved"], true, "{body}");
    assert_eq!(v["repaired"], true, "{body}");
    let message = v["message"].as_str().unwrap();
    assert!(message.starts_with(&format!("area \"Leave Apply\" repaired: {MOVED}")), "{message}");
    assert!(message.contains("put the old path back"), "{message}");

    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(nav.modules.len(), 1, "{:?}", nav.modules);
    let saved = find_area(&nav, "Leave Apply").unwrap();
    assert_eq!(saved.area, "Leave Apply");
    assert_eq!(saved.module, "Leave", "a repair moved the area");
    assert_eq!(saved.made_by, MadeBy::Person);
    assert_eq!(saved.recorded, "2026-10-01T00:00:00Z");
    assert_eq!(saved.clicks, clicks);
    assert_eq!(saved.arrived, "/hr/leave/apply");
    let note = saved.repaired_by_assistant.as_ref().expect("the repair was not recorded");
    assert_eq!(note.previous_clicks, vec![css("#missing")]);
    assert_eq!(note.previous_arrived, "/hr/leave/apply");
    assert_eq!(note.reason, MOVED);
    assert_eq!(note.at.len(), 20, "{}", note.at);
    let state = browser.as_ref().unwrap().discovery.as_ref().unwrap();
    assert_eq!(state.area.as_deref(), Some("Leave Apply"));
}

/// The area's saved path is tried first; when it still arrives there is
/// nothing to repair and nothing is written.
#[tokio::test]
async fn a_repair_is_refused_when_the_old_path_still_works() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#leave"), css("#apply")])).unwrap();
    let before = file_state(dir.path());
    let mut browser = discovery_on_leave_apply();

    let (status, out) =
        repair_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", vec![css("#apply")], MOVED, &quick())
            .await;
    assert_eq!((status, out.as_str()), (409, NOTHING_TO_REPAIR));
    assert_eq!(file_state(dir.path()), before, "the areas file was written");
}

/// A repair names an area the project has, under its own module.
#[tokio::test]
async fn a_repair_of_an_unknown_area_or_into_another_module_is_refused() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#missing")])).unwrap();
    let before = file_state(dir.path());

    let mut browser = discovery_on_leave_apply();
    let (status, out) =
        repair_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Balance", "Leave", vec![css("#apply")], MOVED, &quick())
            .await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("no area named 'Leave Balance'"), "{out}");
    assert!(browser.as_ref().unwrap().d.calls_to("Page.navigate").is_empty(), "the browser was touched");

    let mut browser = discovery_on_leave_apply();
    let (status, out) =
        repair_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Payroll", vec![css("#apply")], MOVED, &quick())
            .await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("under Leave"), "{out}");
    assert!(browser.as_ref().unwrap().d.calls_to("Page.navigate").is_empty(), "the browser was touched");
    assert_eq!(file_state(dir.path()), before, "the areas file was written");
}

/// New clicks that do not arrive repair nothing: the refusal says where
/// they stopped, and the file is as it was.
#[tokio::test]
async fn a_repair_whose_new_clicks_do_not_arrive_writes_nothing() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#missing")])).unwrap();
    let before = file_state(dir.path());
    let mut browser = discovery_on_leave_apply();

    let (status, out) = repair_area_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        "Leave Apply",
        "Leave",
        vec![css("#leave"), css("#missing")],
        MOVED,
        &quick(),
    )
    .await;
    assert_eq!(status, 409, "{out}");
    assert!(out.starts_with("The clicks did not arrive: click 2"), "{out}");
    assert_eq!(file_state(dir.path()), before, "the areas file was written");
}

/// A mapping run can repair a person's area too: it stays the person's,
/// and the run counts it as updated.
#[tokio::test]
async fn a_mapping_run_repair_keeps_the_maker_and_counts_an_update() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#missing")])).unwrap();
    let mut browser = mapping_browser();

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        repair_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks.clone(), MOVED, &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    let saved = find_area(&nav, "Leave Apply").unwrap();
    assert_eq!(saved.made_by, MadeBy::Person);
    assert_eq!(saved.clicks, clicks);
    let run = the_run(&browser);
    assert_eq!(run.updated.len(), 1, "{run:?}");
    assert!(run.added.is_empty(), "{run:?}");
}

/// The route takes `repair` as `{ "reason": ... }`, and a repair may leave
/// the module out; anything else is refused before the browser is asked.
#[tokio::test]
async fn the_area_route_reads_a_repair() {
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    for bad in [
        json!({ "name": "Leave Apply", "module": "Leave", "clicks": [{ "css": "#leave" }], "repair": "moved" }),
        json!({ "name": "Leave Apply", "module": "Leave", "clicks": [{ "css": "#leave" }], "repair": {} }),
    ] {
        let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-area", &bad.to_string(), "1.0.0").await;
        assert_eq!(status, 400, "{bad}: {out}");
        assert!(out.contains("\"repair\" is"), "{bad}: {out}");
    }
    let body = json!({ "name": "Leave Apply", "clicks": [{ "css": "#leave" }], "repair": { "reason": MOVED } }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-area", &body, "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("start_autorun_discovery"), "{out}");
}

/// A repair whose reason is refused, for `repair_area_in` with these
/// arguments: the status and the answer, and whether the browser was
/// touched.
async fn try_reason(dir: &TempDir, reason: &str) -> (u16, String, bool) {
    let mut browser = discovery_on_leave_apply();
    let (status, out) =
        repair_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", vec![css("#apply")], reason, &quick())
            .await;
    let touched = !browser.as_ref().unwrap().d.calls_to("Page.navigate").is_empty();
    (status, out, touched)
}

/// A repair needs a reason: one line of at most 200 characters. Each
/// refusal comes before the browser is touched, and nothing is written.
#[tokio::test]
async fn a_repair_without_a_proper_reason_is_refused_and_writes_nothing() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#missing")])).unwrap();
    let before = file_state(dir.path());
    for reason in ["", "   ", "two\nlines", "two\rlines"] {
        let (status, out, touched) = try_reason(&dir, reason).await;
        assert_eq!(status, 400, "{reason:?}: {out}");
        assert!(!touched, "{reason:?}: the browser was touched");
        assert_eq!(file_state(dir.path()), before, "{reason:?}: the areas file was written");
    }
    let (status, out, touched) = try_reason(&dir, &"x".repeat(201)).await;
    assert_eq!((status, out.as_str()), (400, LONG_REASON));
    assert!(!touched, "the browser was touched");
    assert_eq!(file_state(dir.path()), before, "the areas file was written");

    let (status, out, _) = try_reason(&dir, &"y".repeat(200)).await;
    assert_eq!(status, 200, "a 200-character reason was refused: {out}");
}

/// The reason is scrubbed like every assistant-written note before it is
/// kept and answered: an address keeps only its path, a token is hidden.
#[tokio::test]
async fn a_repairs_reason_is_scrubbed_before_it_is_kept() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#missing")])).unwrap();
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJraW0ifQ.c2lnbmF0dXJlLXNpZ25hdHVyZQ";
    let reason = format!("Leave moved to https://hr.example.com/self/leave?x=1 and www.hr.example.com/a, token {jwt}");
    let (status, body, _) = try_reason(&dir, &reason).await;
    assert_eq!(status, 200, "{body}");
    let kept = find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Leave Apply")
        .unwrap()
        .repaired_by_assistant
        .clone()
        .unwrap()
        .reason;
    for leaked in ["hr.example.com", "https://", "x=1", jwt] {
        assert!(!kept.contains(leaked), "{leaked} kept in {kept}");
        assert!(!body.contains(leaked), "{leaked} answered in {body}");
    }
    assert!(kept.starts_with("Leave moved to /self/leave"), "{kept}");
}

/// New clicks that are the saved ones repair nothing, and the browser is
/// not touched.
#[tokio::test]
async fn a_repair_with_the_saved_clicks_is_refused() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#leave"), css("#missing")])).unwrap();
    let before = file_state(dir.path());
    let mut browser = discovery_on_leave_apply();
    let (status, out) = repair_area_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        "Leave Apply",
        "Leave",
        vec![css("#leave"), css("#missing")],
        MOVED,
        &quick(),
    )
    .await;
    assert_eq!((status, out.as_str()), (409, SAME_PATH));
    assert_eq!(file_state(dir.path()), before, "the areas file was written");
}

/// A browser that stops answering while the saved path is tried says so,
/// and nothing is written: a dead browser proves nothing about the path.
#[tokio::test]
async fn a_browser_that_stops_answering_during_the_check_writes_nothing() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#dead")])).unwrap();
    let before = file_state(dir.path());
    let (mut browser, _) = slot(menu_app_dying_on(true, "/hr/leave/apply", "#dead"), exploring("Leave"));
    browser.as_mut().unwrap().account = Some("admin".into());

    let (status, out) = repair_area_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        "Leave Apply",
        "Leave",
        vec![css("#leave"), css("#apply")],
        MOVED,
        &quick(),
    )
    .await;
    assert_eq!(status, 409, "{out}");
    assert!(out.starts_with("the browser stopped answering while the area's saved path was tried"), "{out}");
    assert_eq!(file_state(dir.path()), before, "the areas file was written");
}

/// A mapping run that saves its own area again with another path clears
/// an assistant's repair note: the note described the path it replaced.
#[tokio::test]
async fn a_mapping_update_clears_a_repair_note() {
    let dir = root_with_recipe_and_account();
    let mut area = leave_apply(MadeBy::Mapping, vec![css("#apply")], "/hr/leave/apply");
    area.repaired_by_assistant = Some(v2_lib::autorun::nav::AssistantRepair {
        previous_clicks: vec![css("#missing")],
        previous_arrived: "/hr/leave/apply".into(),
        previous_start: "/".into(),
        reason: MOVED.into(),
        at: "2026-10-20T09:00:00Z".into(),
    });
    put_path(dir.path(), ORG, PROJECT, area).unwrap();
    let mut browser = mapping_browser();

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks.clone(), &quick()).await;
    assert_eq!(status, 200, "{body}");
    let saved = find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Leave Apply").unwrap().clone();
    assert_eq!(saved.clicks, clicks);
    assert_eq!(saved.repaired_by_assistant, None);
}

// ------------------------------------------- saving in a mapping run

/// A mapping run over Leave, signed in as `admin`, standing on
/// `/hr/leave/apply`.
fn mapping_browser() -> Option<FakeBrowser> {
    let (mut browser, _) = slot(menu_app(true, "/hr/leave/apply"), mapping(exploring("Leave"), &["Leave"]));
    browser.as_mut().unwrap().account = Some("admin".into());
    browser
}

fn the_run(browser: &Option<FakeBrowser>) -> &MappingRun {
    browser.as_ref().unwrap().discovery.as_ref().unwrap().mapping.as_ref().unwrap()
}

/// An area already in the file: Leave Apply, under Leave.
fn leave_apply(made_by: MadeBy, clicks: Vec<Target>, arrived: &str) -> ModulePath {
    ModulePath {
        area: "Leave Apply".into(),
        module: "Leave".into(),
        clicks,
        arrived: arrived.into(),
        recorded: "2026-10-01T00:00:00Z".into(),
        start: "/".into(),
        made_by,
        repaired_by_assistant: None,
        preferences: Default::default(),
        mode: None,
    }
}

/// The areas file's bytes and when it was last written.
fn file_state(root: &std::path::Path) -> (Vec<u8>, std::time::SystemTime) {
    let path = nav_path(root, ORG, PROJECT);
    (std::fs::read(&path).unwrap(), std::fs::metadata(&path).unwrap().modified().unwrap())
}

/// A screen a mapping run reaches is saved as the run's own, and counted
/// as added. The file says so in words.
#[tokio::test]
async fn a_mapping_save_is_made_by_mapping() {
    let dir = root_with_recipe_and_account();
    let mut browser = mapping_browser();

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["saved"], true, "{body}");

    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(find_area(&nav, "Leave Apply").unwrap().made_by, MadeBy::Mapping);
    let file = std::fs::read_to_string(nav_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(file.contains("\"made_by\": \"mapping\""), "{file}");
    let run = the_run(&browser);
    assert_eq!(run.added, vec!["Leave Apply".to_string()]);
    assert!(run.updated.is_empty() && run.unchanged.is_empty() && run.unreached.is_empty(), "{run:?}");
}

/// An ordinary discovery's save is the person's: they asked for it.
#[tokio::test]
async fn an_ordinary_discovery_save_is_made_by_a_person() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(menu_app(true, "/hr/leave/apply"), exploring("Leave"));
    browser.as_mut().unwrap().account = Some("admin".into());

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(find_area(&nav, "Leave Apply").unwrap().made_by, MadeBy::Person);
}

/// The run's own area, reached by other clicks now, is saved again with
/// them and counted as updated, with its old and new menu path in words.
#[tokio::test]
async fn a_mapping_area_whose_path_changed_is_updated() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, leave_apply(MadeBy::Mapping, vec![css("#apply")], "/hr/leave/apply")).unwrap();
    let mut browser = mapping_browser();

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks.clone(), &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["saved"], true, "{body}");

    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(nav.modules.len(), 1, "{:?}", nav.modules);
    let saved = find_area(&nav, "Leave Apply").unwrap();
    assert_eq!(saved.clicks, clicks);
    assert_eq!(saved.made_by, MadeBy::Mapping);
    let run = the_run(&browser);
    assert_eq!(
        run.updated,
        vec![(
            "Leave Apply".to_string(),
            v2_lib::autorun::nav::menu_path(&[css("#apply")]),
            v2_lib::autorun::nav::menu_path(&clicks),
        )]
    );
    assert_ne!(run.updated[0].1, run.updated[0].2);
    assert!(!run.updated[0].2.contains("/hr/"), "a path held an address: {run:?}");
    assert!(run.added.is_empty() && run.unchanged.is_empty(), "{run:?}");
}

/// The same screen mapped twice is saved once: the second time it is
/// unchanged, the file is not written and nothing is duplicated.
#[tokio::test]
async fn mapping_the_same_screen_twice_is_unchanged() {
    let dir = root_with_recipe_and_account();
    let mut browser = mapping_browser();
    let clicks = vec![css("#leave"), css("#apply")];

    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks.clone(), &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    let before = file_state(dir.path());

    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["saved"], false, "{body}");
    assert_eq!(v["unchanged"], true, "{body}");
    assert_eq!(file_state(dir.path()), before, "the areas file was written again");

    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(nav.modules.len(), 1, "{:?}", nav.modules);
    let run = the_run(&browser);
    assert_eq!(run.added, vec!["Leave Apply".to_string()]);
    assert_eq!(run.unchanged, vec!["Leave Apply".to_string()]);
    assert!(run.updated.is_empty(), "{run:?}");
}

/// A person's area is refused before the browser is touched, counted as
/// unchanged and left exactly as it is.
#[tokio::test]
async fn a_person_area_is_never_changed_by_mapping() {
    let dir = root_with_recipe_and_account();
    let theirs = leave_apply(MadeBy::Person, vec![css("#apply")], "/hr/leave/apply");
    put_path(dir.path(), ORG, PROJECT, theirs.clone()).unwrap();
    let before = file_state(dir.path());
    let mut browser = mapping_browser();

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, out) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "leave apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(
        out,
        "An area named 'Leave Apply' was recorded by a person, so this mapping run leaves it as it is"
    );
    assert!(browser.as_ref().unwrap().d.calls_to("Page.navigate").is_empty(), "the browser was touched");
    assert_eq!(file_state(dir.path()), before, "the areas file was written");
    assert_eq!(find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Leave Apply"), Some(&theirs));
    let run = the_run(&browser);
    assert_eq!(run.unchanged, vec!["Leave Apply".to_string()]);
    assert!(run.added.is_empty() && run.updated.is_empty(), "{run:?}");
    // Located by where the person's area arrives, so it can stand for an
    // unreached entry of the same screen.
    let located = run.outcomes.last().and_then(|o| o.2.clone()).expect("the kept area was not located");
    assert_eq!(located.arrived.as_deref(), Some("/hr/leave/apply"));
}

/// A screen named like an area already there, in another case and with
/// other spacing, is that area, not a second one.
#[tokio::test]
async fn a_screen_named_like_an_existing_area_matches_it() {
    let dir = root_with_recipe_and_account();
    let clicks = vec![css("#leave"), css("#apply")];
    put_path(dir.path(), ORG, PROJECT, leave_apply(MadeBy::Mapping, clicks.clone(), "/hr/leave/apply")).unwrap();
    let before = file_state(dir.path());
    let mut browser = mapping_browser();

    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "  leave \u{a0}  APPLY ", "Leave", clicks, &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["unchanged"], true, "{body}");
    assert_eq!(file_state(dir.path()), before, "the areas file was written");
    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(nav.modules.len(), 1, "{:?}", nav.modules);
    assert_eq!(nav.modules[0].area, "Leave Apply");
    let run = the_run(&browser);
    assert_eq!(run.unchanged, vec!["Leave Apply".to_string()], "{run:?}");
    assert!(run.added.is_empty() && run.updated.is_empty(), "{run:?}");
}

/// With 150 screens saved in this run, added and updated together, the
/// next is refused before the browser is touched, and nothing is saved.
#[tokio::test]
async fn the_cap_refuses_the_151st_screen() {
    let dir = root_with_recipe_and_account();
    let mut browser = mapping_browser();
    {
        let run = browser.as_mut().unwrap().discovery.as_mut().unwrap().mapping.as_mut().unwrap();
        run.added = (0..100).map(|i| format!("Screen {i}")).collect();
        run.updated = (0..50).map(|i| (format!("Moved {i}"), "a".to_string(), "b".to_string())).collect();
        run.unchanged = (0..30).map(|i| format!("Same {i}")).collect();
    }

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, out) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out, "This mapping run has saved 150 screens; end it and start another for the rest.");
    assert!(browser.as_ref().unwrap().d.calls_to("Page.navigate").is_empty(), "the browser was touched");
    assert!(find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Leave Apply").is_none());
    let run = the_run(&browser);
    assert_eq!((run.added.len(), run.updated.len(), run.unchanged.len()), (100, 50, 30));
}

/// Unchanged screens do not count toward the cap: 149 saved and any
/// number unchanged still saves one more.
#[tokio::test]
async fn the_cap_counts_only_saved_screens() {
    let dir = root_with_recipe_and_account();
    let mut browser = mapping_browser();
    {
        let run = browser.as_mut().unwrap().discovery.as_mut().unwrap().mapping.as_mut().unwrap();
        run.added = (0..149).map(|i| format!("Screen {i}")).collect();
        run.unchanged = (0..30).map(|i| format!("Same {i}")).collect();
    }
    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(the_run(&browser).added.len(), 150);
}

/// A screen the replay does not reach is refused as in any discovery,
/// saves nothing, and is counted as unreached with the reason.
#[tokio::test]
async fn a_mapping_screen_that_is_not_reached_is_unreached() {
    let dir = root_with_recipe_and_account();
    let mut browser = mapping_browser();

    let clicks = vec![css("#leave"), css("#missing")];
    let (status, out) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 409, "{out}");
    assert!(out.starts_with("The clicks did not arrive: click 2"), "{out}");
    assert!(find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Leave Apply").is_none());
    let run = the_run(&browser);
    assert_eq!(run.unreached.len(), 1, "{run:?}");
    assert_eq!(run.unreached[0].0, "Leave Apply");
    assert!(run.unreached[0].1.starts_with("click 2"), "{run:?}");
    assert!(!run.unreached[0].1.contains("t0p-secret"), "{run:?}");
    assert!(run.added.is_empty(), "{run:?}");
}

/// An update asked for in another case keeps the name as it was stored.
#[tokio::test]
async fn an_update_named_in_another_case_keeps_the_stored_name() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, leave_apply(MadeBy::Mapping, vec![css("#apply")], "/hr/leave/apply")).unwrap();
    let mut browser = mapping_browser();

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "leave apply", "Leave", clicks.clone(), &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["saved"], true, "{body}");

    let nav = load_nav(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(nav.modules.len(), 1, "{:?}", nav.modules);
    assert_eq!(nav.modules[0].area, "Leave Apply");
    assert_eq!(nav.modules[0].clicks, clicks);
    let run = the_run(&browser);
    assert_eq!(run.updated.len(), 1, "{run:?}");
    assert_eq!(run.updated[0].0, "Leave Apply");
}

/// A save the page sends while an area save replays its clicks is stopped
/// and counted on the run as soon as the save answers, not at the end.
#[tokio::test]
async fn a_save_stopped_on_an_area_saves_trip_is_counted_at_once() {
    use v2_lib::browser::cdp::Driver;
    let dir = root_with_recipe_and_account();
    let mut browser = mapping_browser();
    {
        let d = &mut browser.as_mut().unwrap().d;
        d.guard_saves(&[]).await.unwrap();
        d.saves_on_call.push((
            "Input.dispatchMouseEvent".into(),
            "POST".into(),
            "https://hr.example.internal/api/SaveLastVisited?id=1".into(),
        ));
    }

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["saved"], true, "{body}");
    let b = browser.as_ref().unwrap();
    assert!(b.d.saves_on_call.is_empty(), "the trip never sent its save");
    assert!(b.d.saves_stopped.is_empty(), "the stopped save was left to carry over");
    assert_eq!(the_run(&browser).blocked_writes, 1);
}

// ------------------------------------- an area is checked from a fresh home

/// A recipe whose home is `/hr/home/index` and whose `after_sign_in` is
/// `after`: what a fresh load of home runs once it shows `#marker`.
fn root_with_after_sign_in(after: Value) -> TempDir {
    let dir = TempDir::new();
    let recipe: SignInRecipe = serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/hr/home/index",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "after_sign_in": after,
        "signed_in": { "css": "#marker" }
    }))
    .unwrap();
    save_recipe(dir.path(), ORG, PROJECT, &recipe).unwrap();
    save_accounts(dir.path(), &[account()]).unwrap();
    dir
}

/// PeoplesHR's own `after_sign_in`: the toggle, only while the menu is closed.
fn open_a_closed_menu() -> Value {
    json!([ { "kind": "when_visible", "selector": { "css": "#toggle:not(.active)" }, "within_ms": 100,
        "then": [ { "kind": "click", "selector": { "css": "#toggle" } } ] } ])
}

/// What `sidebar_app` saw, in order (`navigate <path>`, `click <css>`),
/// where its page is, whether its menu is open, and whether it is signed
/// in (`#marker` shows only then; `#go` signs it in).
struct Sidebar {
    log: Arc<Mutex<Vec<String>>>,
    path: Arc<Mutex<String>>,
    open: Arc<AtomicBool>,
    signed: Arc<AtomicBool>,
}

/// An application with a left menu that remembers whether it is open, the
/// way PeoplesHR keeps it in the browser's storage: a page load leaves it
/// as it was. `#toggle` opens or closes it, and `#toggle:not(.active)` is
/// on the page only while it is closed. The menu's `#talent` and `#wizard`
/// are found only while it is open; `#wizard` lands on `screen`. The page
/// stands on `at`, signed in, and `#missing` is never there.
fn sidebar_app(open: bool, at: &str, screen: &'static str) -> (ScriptedDriver, Sidebar) {
    let app = Sidebar {
        log: Arc::new(Mutex::new(vec![])),
        path: Arc::new(Mutex::new(at.to_string())),
        open: Arc::new(AtomicBool::new(open)),
        signed: Arc::new(AtomicBool::new(true)),
    };
    let (log, path, menu, signed) = (app.log.clone(), app.path.clone(), app.open.clone(), app.signed.clone());
    let mut last_css = String::new();
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Page.navigate" => {
                let p = v2_lib::autorun::nav::path_of(params["url"].as_str().unwrap_or(""));
                log.lock().unwrap().push(format!("navigate {p}"));
                *path.lock().unwrap() = p;
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => json!({ "result": {
                "value": format!("https://hr.example.internal{}?token=t0p-secret", path.lock().unwrap())
            } }),
            "Runtime.evaluate" if params["expression"] == "document.title" => json!({ "result": { "value": "Home" } }),
            "Runtime.evaluate" => {
                json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } })
            }
            "Accessibility.getFullAXTree" => json!({ "nodes": [
                { "nodeId": "1", "ignored": false, "role": { "value": "button" }, "name": { "value": "Save" },
                  "childIds": [] }
            ] }),
            "Network.getAllCookies" => json!({ "cookies": [] }),
            "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": ready_probe() } }),
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS || f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == FOCUS_JS => json!({ "result": { "value": "text" } }),
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let open = menu.load(Ordering::SeqCst);
                let there = match last_css.as_str() {
                    "#toggle:not(.active)" => !open,
                    "#talent" | "#wizard" => open,
                    "#missing" => false,
                    "#marker" => signed.load(Ordering::SeqCst),
                    _ => true,
                };
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                log.lock().unwrap().push(format!("click {last_css}"));
                match last_css.as_str() {
                    "#go" => signed.store(true, Ordering::SeqCst),
                    "#toggle" => {
                        menu.fetch_xor(true, Ordering::SeqCst);
                    }
                    "#wizard" => *path.lock().unwrap() = screen.to_string(),
                    _ => {}
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
    (d, app)
}

/// A mapping run over Talent on `d`, signed in as `admin`.
fn talent_run(d: ScriptedDriver) -> Option<FakeBrowser> {
    let (mut browser, _) = slot(d, mapping(exploring("Talent"), &["Talent"]));
    browser.as_mut().unwrap().account = Some("admin".into());
    browser
}

/// The check loads home afresh before the first click and runs
/// `after_sign_in` there exactly once. A plain toggle proves it: run twice,
/// it would close the menu it opened, and the first click would not find
/// its entry.
#[tokio::test]
async fn an_area_check_starts_from_home_with_the_menu_open_once() {
    let dir = root_with_after_sign_in(json!([ { "kind": "click", "selector": { "css": "#toggle" } } ]));
    let (d, app) = sidebar_app(false, "/talent/wizard", "/talent/wizard");
    let mut browser = talent_run(d);

    let clicks = vec![css("#talent"), css("#wizard")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Definition Wizard", "Talent", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["arrived"], "/talent/wizard", "{body}");
    let log = app.log.lock().unwrap().clone();
    assert_eq!(log[..4], ["navigate /hr/home/index", "click #toggle", "click #talent", "click #wizard"], "{log:?}");
    assert_eq!(log.iter().filter(|l| *l == "click #toggle").count(), 1, "{log:?}");
    assert_eq!(log.iter().filter(|l| l.starts_with("navigate")).count(), 1, "{log:?}");
    assert!(app.open.load(Ordering::SeqCst));
}

/// A session that has gone is signed in again, and that sign-in is the
/// check's start: it already ran `after_sign_in`, so the page is not
/// loaded again to run it a second time. A plain toggle proves it ran once.
#[tokio::test]
async fn an_area_check_that_signs_in_again_opens_the_menu_once() {
    let dir = root_with_after_sign_in(json!([ { "kind": "click", "selector": { "css": "#toggle" } } ]));
    let (d, app) = sidebar_app(false, "/talent/wizard", "/talent/wizard");
    app.signed.store(false, Ordering::SeqCst);
    let mut browser = talent_run(d);

    let clicks = vec![css("#talent"), css("#wizard")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Definition Wizard", "Talent", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["arrived"], "/talent/wizard", "{body}");
    let log = app.log.lock().unwrap().clone();
    assert!(log.contains(&"click #go".to_string()), "it did not sign in again: {log:?}");
    assert_eq!(log.iter().filter(|l| *l == "click #toggle").count(), 1, "{log:?}");
    let after_go = log.iter().position(|l| l == "click #go").unwrap();
    assert_eq!(log[after_go..], ["click #go", "click #toggle", "click #talent", "click #wizard"], "{log:?}");
    assert!(app.open.load(Ordering::SeqCst));
}

/// 2026-10-09: a person, or an assistant, left the menu closed, on a
/// screen whose address still reads like home (a single-page application's
/// can). The check used to start from that page as it stood, found no
/// "Talent" and refused the save. It now starts from a fresh home page,
/// where `after_sign_in` opens the menu, and the saved clicks need nothing
/// left open by hand.
#[tokio::test]
async fn a_closed_menu_does_not_refuse_an_area_save() {
    let dir = root_with_after_sign_in(open_a_closed_menu());
    let (d, app) = sidebar_app(false, "/hr/home/index", "/hr/home/index");
    let mut browser = talent_run(d);

    let clicks = vec![css("#talent"), css("#wizard")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Definition Wizard", "Talent", clicks.clone(), &quick())
            .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["saved"], true, "{body}");
    let log = app.log.lock().unwrap().clone();
    assert_eq!(log[..4], ["navigate /hr/home/index", "click #toggle", "click #talent", "click #wizard"], "{log:?}");
    let saved = find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Definition Wizard").cloned().unwrap();
    assert_eq!(saved.clicks, clicks, "the toggle is not one of the saved clicks");
    assert_eq!(the_run(&browser).added, vec!["Definition Wizard".to_string()]);
}

/// 2026-10-09: a save was refused, and the same screen was then saved
/// under another name. The refused name is not listed as not reached.
#[tokio::test]
async fn an_unreached_screen_saved_later_under_another_name_leaves_the_summary() {
    let dir = root_with_after_sign_in(open_a_closed_menu());
    let (d, app) = sidebar_app(false, "/talent/wizard", "/talent/wizard");
    let mut browser = talent_run(d);

    let missing = vec![css("#talent"), css("#missing")];
    let (status, out) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Definition Wizard", "Talent", missing, &quick()).await;
    assert_eq!(status, 409, "{out}");
    // The assistant goes back to the screen before saving it again.
    *app.path.lock().unwrap() = "/talent/wizard".to_string();
    let clicks = vec![css("#talent"), css("#wizard")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Talent Wizard", "Talent", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");

    let s = summarize(the_run(&browser));
    assert_eq!(s.added, vec!["Talent Wizard".to_string()]);
    assert!(s.unreached.is_empty(), "{s:?}");
}

/// A screen not reached stays listed when what was saved after it is
/// another screen.
#[tokio::test]
async fn an_unreached_screen_stays_listed_when_another_screen_is_saved() {
    let dir = root_with_after_sign_in(open_a_closed_menu());
    let (d, app) = sidebar_app(false, "/talent/review", "/talent/wizard");
    let mut browser = talent_run(d);

    let missing = vec![css("#talent"), css("#missing")];
    let (status, out) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Review", "Talent", missing, &quick()).await;
    assert_eq!(status, 409, "{out}");
    // The assistant moves on to another screen and saves that.
    *app.path.lock().unwrap() = "/talent/wizard".to_string();
    let clicks = vec![css("#talent"), css("#wizard")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Definition Wizard", "Talent", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");

    let s = summarize(the_run(&browser));
    assert_eq!(s.added, vec!["Definition Wizard".to_string()]);
    assert_eq!(s.unreached.len(), 1, "{s:?}");
    assert_eq!(s.unreached[0].name, "Review");
}

/// With no address path known for a screen not reached, a later save with
/// the same clicks is that screen; other clicks are not, and a screen
/// nothing is known about stays listed.
#[test]
fn an_unreached_screen_with_no_address_matches_a_later_save_by_its_clicks() {
    let mut run = MappingRun::new(&["Talent".to_string()]);
    run.record_unreached("Wizard".into(), "the home page did not load");
    run.locate_last(MappingScreen { arrived: None, menu: "Talent, then Wizard".into() });
    run.record_unreached("Review".into(), "the home page did not load");
    run.locate_last(MappingScreen { arrived: None, menu: "Talent, then Review".into() });
    run.record_unreached("Unplaced".into(), "click 1 was not found");
    run.record_added("Definition Wizard".into());
    run.locate_last(MappingScreen { arrived: Some("/talent/wizard".into()), menu: "Talent, then Wizard".into() });

    let s = summarize(&run);
    let names: Vec<&str> = s.unreached.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, ["Review", "Unplaced"], "{s:?}");
}

/// A screen not reached that was saved EARLIER in the run under another
/// name leaves the summary too, and so does one a person's kept area
/// (found by where it arrives) stands for. The same name saved earlier
/// and then not reached stays listed: that is its last outcome.
#[test]
fn an_unreached_screen_saved_earlier_under_another_name_leaves_the_summary() {
    let at = |path: &str| MappingScreen { arrived: Some(path.into()), menu: String::new() };
    let mut run = MappingRun::new(&["Talent".to_string()]);
    run.record_added("Definition Wizard".into());
    run.locate_last(at("/talent/wizard"));
    run.record_unchanged("Leave Apply".into());
    run.locate_last(at("/hr/leave/apply"));
    run.record_unreached("Talent Wizard".into(), "click 2 was not found");
    run.locate_last(at("/talent/wizard"));
    run.record_unreached("Apply for Leave".into(), "click 1 was not found");
    run.locate_last(at("/hr/leave/apply"));
    run.record_added("Review".into());
    run.locate_last(at("/talent/review"));
    run.record_unreached("Review".into(), "click 1 was not found");
    run.locate_last(at("/talent/review"));

    let s = summarize(&run);
    assert_eq!(s.added, vec!["Definition Wizard".to_string()]);
    assert_eq!(s.unchanged, vec!["Leave Apply".to_string()]);
    let names: Vec<&str> = s.unreached.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, ["Review"], "{s:?}");
}

/// What `area`'s map holds on `page`, by name.
fn seen_on(root: &std::path::Path, area: &str, page: &str) -> Vec<String> {
    mapped_area(root, area)
        .and_then(|a| a.pages.into_iter().find(|p| p.path == page))
        .map(|p| p.elements.into_iter().map(|e| e.name).collect())
        .unwrap_or_default()
}

/// Every name `area`'s map holds, on any page.
fn seen_in(root: &std::path::Path, area: &str) -> Vec<String> {
    mapped_area(root, area)
        .map(|a| a.pages.into_iter().flat_map(|p| p.elements).map(|e| e.name).collect())
        .unwrap_or_default()
}

/// Marks `area` explored at `at` in the map file, as an earlier read would.
fn stamp_explored(root: &std::path::Path, area: &str, at: u64) {
    let path = map_path(root, ORG, PROJECT);
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let entry = v["areas"].as_array_mut().unwrap().iter_mut().find(|a| a["area"] == area).expect("no such area");
    entry["explored_at"] = json!(at);
    std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

/// A mapping run files each screen's elements under that screen: a saved
/// screen's own page under it, and what the run reads on its way to the
/// next screen under no area, which neither stamps nor feeds the screen
/// saved before.
#[tokio::test]
async fn a_mapping_run_files_each_screens_elements_under_that_screen() {
    let dir = root_with_recipe_and_account();
    let started =
        Some(DiscoveryState { account: Some("admin".into()), ..opened_for_discovery().unwrap() });
    let (mut browser, _) = slot(menu_app(true, "/hr/leave/apply"), mapping(started, &["Leave"]));
    browser.as_mut().unwrap().account = Some("admin".into());

    let (status, body) = discover_area_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        "Leave Apply",
        "Leave",
        vec![css("#leave"), css("#apply")],
        &quick(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["saved"], true, "{body}");
    let own = seen_on(dir.path(), "Leave Apply", "/hr/leave/apply");
    assert!(own.contains(&"Name".to_string()) && own.contains(&"Save".to_string()), "{own:?}");
    assert_eq!(browser.as_ref().unwrap().discovery.as_ref().unwrap().area, None, "the run stayed in the saved area");
    stamp_explored(dir.path(), "Leave Apply", 5);

    // The walk to the next screen: the menu, then the screen itself.
    for go in ["#menu", "#balance"] {
        let step = Action::Click { selector: go.into() };
        let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &step, None, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(parsed(&body)["ok"], true, "{body}");
    }
    let (status, body) = discover_area_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        "Leave Balance",
        "Leave",
        vec![css("#leave"), css("#balance")],
        &quick(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["saved"], true, "{body}");

    let a = mapped_area(dir.path(), "Leave Apply").unwrap();
    let in_a = seen_in(dir.path(), "Leave Apply");
    assert!(!in_a.contains(&"Payroll".to_string()), "the walk's menu was filed under the screen before: {in_a:?}");
    assert!(!in_a.contains(&"Approve".to_string()), "the next screen was filed under the one before: {in_a:?}");
    assert_eq!(a.explored_at, Some(5), "the walk stamped the screen saved before as explored");
    let in_b = seen_on(dir.path(), "Leave Balance", "/hr/leave/balance");
    assert!(in_b.contains(&"Approve".to_string()), "the screen's own page was not filed under it: {in_b:?}");
    assert!(mapped_area(dir.path(), "Leave Balance").unwrap().explored_at.is_some());
    assert!(seen_on(dir.path(), "", "/hr/menu").contains(&"Payroll".to_string()), "the walk was not filed under no area");
    assert_eq!(browser.as_ref().unwrap().discovery.as_ref().unwrap().area, None);
}

/// An ordinary discovery goes on to explore the area it saved: what it
/// reads next is filed under that area.
#[tokio::test]
async fn an_ordinary_discovery_files_its_next_reads_under_the_saved_area() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(menu_app(true, "/hr/leave/apply"), exploring("Leave"));
    browser.as_mut().unwrap().account = Some("admin".into());

    let (status, body) = discover_area_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        "Leave Apply",
        "Leave",
        vec![css("#leave"), css("#apply")],
        &quick(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(browser.as_ref().unwrap().discovery.as_ref().unwrap().area.as_deref(), Some("Leave Apply"));

    let step = Action::Click { selector: "#menu".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &step, None, None).await;
    assert_eq!(status, 200, "{body}");
    assert!(seen_on(dir.path(), "Leave Apply", "/hr/menu").contains(&"Payroll".to_string()), "{body}");
    assert!(seen_on(dir.path(), "", "/hr/menu").is_empty(), "an ordinary discovery's read went under no area");
}

/// No discovery, no saving: not with no browser, not in the person's own
/// browser (which is neither touched nor closed), and not through the
/// route.
#[tokio::test]
async fn saving_an_area_needs_a_discovery_session() {
    let dir = root_with_recipe_and_account();
    let mut none: Option<FakeBrowser> = None;
    let (status, out) =
        discover_area_in(&mut none, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", vec![css("#leave")], &quick())
            .await;
    assert_eq!((status, out.as_str()), (409, NO_DISCOVERY));

    let (mut theirs, closed) = slot(menu_app(true, "/hr/leave"), None);
    let (status, out) =
        discover_area_in(&mut theirs, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", vec![css("#leave")], &quick())
            .await;
    assert_eq!((status, out.as_str()), (409, NO_DISCOVERY));
    assert!(!closed.load(Ordering::SeqCst));
    assert!(theirs.as_ref().unwrap().d.calls_to("Page.navigate").is_empty(), "the person's browser was touched");
    assert!(find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Leave Apply").is_none());

    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let body = json!({ "name": "Leave Apply", "module": "Leave", "clicks": [{ "css": "#leave" }] }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-area", &body, "1.0.0").await;
    assert_eq!(status, 409, "{out}");
    assert!(out.contains("start_autorun_discovery"), "{out}");

    for bad in [
        json!({ "module": "Leave", "clicks": [{ "css": "#leave" }] }),
        json!({ "name": "Leave Apply", "clicks": [{ "css": "#leave" }] }),
        json!({ "name": "Leave Apply", "module": "Leave", "clicks": [] }),
    ] {
        let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-area", &bad.to_string(), "1.0.0").await;
        assert_eq!(status, 400, "{bad}: {out}");
    }
}

/// The Discovery card's read: each area with its pages, elements, writes
/// and whether it is stale, with why. The unattributed bucket is listed
/// for its writes but is never stale: it is no area to explore.
#[test]
fn load_map_command_reports_stale_and_counts() {
    use v2_lib::autorun::discovery_map::STALE_AFTER_MS;
    use v2_lib::commands::autorun::map_view;
    let dir = TempDir::new();
    let now: u64 = 100 * STALE_AFTER_MS;
    let element = |n: &str| {
        json!({
            "key": { "Role": { "role": "button", "name": n } },
            "locator": { "css": format!("#{n}") },
            "role": "button", "name": n, "kind": "button", "required": false
        })
    };
    let area = |name: &str, explored: Option<u64>, failed: bool, pages: Value, writes: Value| {
        json!({
            "area": name, "explored_at": explored, "account": explored.map(|_| "admin"),
            "failed_since": failed, "pages": pages, "outcomes": [], "writes": writes
        })
    };
    let map = json!({ "areas": [
        area("Leave Apply", Some(now - 1000), false, json!([
            { "path": "/hr/leave", "title": "Leave", "elements": [element("save"), element("cancel")] },
            { "path": "/hr/leave/new", "title": "New", "elements": [element("submit")] }
        ]), json!([{ "method": "POST", "path": "/api/leave", "at": now - 900, "step": "click Save" }])),
        area("Payroll", Some(now - STALE_AFTER_MS - 1), false, json!([]), json!([])),
        area("Claims", Some(now - 1000), true, json!([]), json!([])),
        area("Reports", None, false, json!([]), json!([])),
        area("", None, false, json!([]), json!([{ "method": "PUT", "path": "/api/x", "at": 1, "step": "s" }])),
    ]});
    let path = map_path(dir.path(), ORG, PROJECT);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, map.to_string()).unwrap();

    let view = map_view(dir.path(), ORG, PROJECT, now).unwrap();
    let names: Vec<&str> = view.areas.iter().map(|a| a.area.as_str()).collect();
    assert_eq!(names, ["Leave Apply", "Payroll", "Claims", "Reports", ""]);

    let leave = &view.areas[0];
    assert_eq!((leave.pages, leave.elements), (2, 3));
    assert_eq!(leave.account.as_deref(), Some("admin"));
    assert_eq!(leave.explored_at, Some(now - 1000));
    assert!(!leave.stale);
    assert_eq!(leave.stale_reason, None);
    assert_eq!(leave.writes.len(), 1);
    assert_eq!((leave.writes[0].method.as_str(), leave.writes[0].path.as_str()), ("POST", "/api/leave"));

    let reason = |i: usize| (view.areas[i].stale, view.areas[i].stale_reason.clone());
    assert_eq!(reason(1), (true, Some("Explored more than 30 days ago".to_string())));
    assert_eq!(reason(2), (true, Some("A script failed there since it was explored".to_string())));
    assert_eq!(reason(3), (true, Some("No map yet".to_string())));
    assert_eq!(reason(4), (false, None), "the unattributed bucket is never stale");
    assert_eq!(view.areas[4].writes.len(), 1);

    // No map file at all: no areas, not an error.
    let empty = TempDir::new();
    assert!(map_view(empty.path(), ORG, PROJECT, now).unwrap().areas.is_empty());
}

// ------------------------------------------ final review: areas, the browser

fn recorded(dir: &std::path::Path, name: &str) {
    put_path(
        dir,
        ORG,
        PROJECT,
        ModulePath {
            area: name.into(),
            module: "Leave".into(),
            clicks: vec![css("#leave")],
            arrived: "/hr/leave".into(),
            recorded: "2026-10-01T00:00:00Z".into(),
            start: String::new(),
            made_by: MadeBy::Person,
            repaired_by_assistant: None,
            preferences: Default::default(),
            mode: None,
        },
    )
    .unwrap();
}

/// Finding 6: starting a discovery in an area reads the landing page, but
/// that is not the area: it neither marks the area explored nor clears a
/// failure since. What the discovery then sees in the area does.
#[tokio::test]
async fn starting_discovery_does_not_mark_an_area_explored() {
    let dir = root_with_recipe_and_account();
    recorded(dir.path(), "Leave");
    v2_lib::autorun::discovery_map::mark_failed(dir.path(), ORG, PROJECT, "Leave").unwrap();
    let (mut browser, _) = slot(signin_app(true), opened_for_discovery());

    let (status, body) =
        discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some(" leave "), &quick()).await;
    assert_eq!(status, 200, "{body}");
    let state = browser.as_ref().unwrap().discovery.as_ref().unwrap();
    assert_eq!(state.area.as_deref(), Some("Leave"), "the recorded area's own name");
    let area = mapped_area(dir.path(), "Leave").expect("the landing page was not filed");
    assert_eq!(area.explored_at, None, "starting marked the area explored");
    assert!(area.failed_since, "starting cleared the failure since");

    // An action in the area does.
    let (mut browser, _) = slot(leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave"), exploring("Leave"));
    let (status, body) = discover_action_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        &Action::Click { selector: "#save".into() },
        None,
        Some("LEAVE"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(browser.as_ref().unwrap().discovery.as_ref().unwrap().area.as_deref(), Some("Leave"));
    let map = load_map(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(map.areas.len(), 1, "a second spelling made a second area: {:?}", map.areas);
    assert!(map.areas[0].explored_at.is_some(), "what was seen in the area did not mark it explored");
    assert!(!map.areas[0].failed_since);
}

/// Finding 3: a replay never runs inside the discovery's browser. Refused
/// with the sentence that names `end_autorun_discovery`; the person's own
/// browser, or none, is no reason to refuse.
#[tokio::test]
async fn a_replay_is_refused_while_discovery_holds_the_browser() {
    use v2_lib::ai_bridge::refuse_while_discovering;
    let (mut discovering, closed) = slot(FakePage::default().driver(), exploring("Leave"));
    let why = refuse_while_discovering(&mut discovering).unwrap_err();
    assert_eq!(why, busy_browser_sentence(true));
    assert!(why.contains("end_autorun_discovery"), "{why}");
    assert!(discovering.is_some() && !closed.load(Ordering::SeqCst), "the discovery's browser was touched");
    // Opened for a discovery, still signing in: refused too.
    let (mut opening, _) = slot(FakePage::default().driver(), opened_for_discovery());
    assert!(refuse_while_discovering(&mut opening).is_err());

    let (mut theirs, _) = slot(FakePage::default().driver(), None);
    assert_eq!(refuse_while_discovering(&mut theirs), Ok(()));
    let mut none: Option<FakeBrowser> = None;
    assert_eq!(refuse_while_discovering(&mut none), Ok(()));

    let source = include_str!("../../src/commands/autorun.rs");
    let replay = &source[source.find("pub(crate) async fn replay_supervised").unwrap()..];
    let replay = &replay[..replay.find("open_if_none(").unwrap()];
    assert!(replay.contains("refuse_while_discovering(&mut slot)"), "replay_supervised opens without asking");
}

/// Finding 5: Open browser never replaces a discovery's browser - neither
/// the button nor RunPane's own between-case and Continue paths, which all
/// call `auto_run_open_browser`. It refuses with the same sentence.
#[tokio::test]
async fn open_browser_is_refused_while_discovery_holds_the_browser() {
    let source = include_str!("../../src/commands/autorun.rs");
    let open = &source[source.find("pub async fn auto_run_open_browser").unwrap()..];
    let open = &open[..open.find("open_into(").unwrap()];
    assert!(open.contains("auto_run_discovery_active()"), "Open browser does not ask before stopping a replay");
    assert!(open.contains("refuse_while_discovering(&mut slot)?"), "Open browser does not ask under the lock");
    assert!(busy_browser_sentence(true).contains("End discovery"), "{}", busy_browser_sentence(true));
}

/// Finding 4: End discovery ends the discovery through the same ending as
/// `/autorun-discover-end`, as the person - who closes the browser so Auto
/// Run can run again (`EndedBy::Person`) - and is fine with nothing to end.
#[tokio::test]
async fn the_end_discovery_command_ends_like_the_route() {
    let _g = crate::serial::autorun();
    for _ in 0..2 {
        assert_eq!(v2_lib::commands::autorun::auto_run_end_discovery().await, Ok(()));
    }
    assert!(!v2_lib::commands::autorun::auto_run_discovery_active());
    let source = include_str!("../../src/commands/autorun.rs");
    let cmd = &source[source.find("pub async fn auto_run_end_discovery").unwrap()..];
    assert!(
        cmd[..cmd.find('}').unwrap()].contains("end_discovery(crate::ai_bridge::EndedBy::Person, SlotNo::MAIN).await"),
        "not the route's own ending, as the person"
    );
}

// ------------------------------------------------- trying a component

/// "Pick a date" used on `#day` with a day that must never reach the map.
fn pick_a_date_use() -> Action {
    serde_json::from_value(json!({
        "kind": "use_component", "component": "pick a  DATE",
        "inputs": { "field": { "css": "#day" }, "day": "Secret-Day-17" }
    }))
    .unwrap()
}

fn draft(actions: Value) -> Component {
    let mut c = pick_a_date();
    c.actions = serde_json::from_value(actions).unwrap();
    c
}

/// A draft that is not saved anywhere is expanded and run action by
/// action, each through discovery's own path: every action answers in
/// `steps`, each one's outcome line is filed, nothing typed is kept, and
/// the try that worked is fingerprinted on the discovery.
#[tokio::test]
async fn a_draft_component_can_be_tried_and_is_fingerprinted() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/new?x=1"), exploring("Leave"));
    let c = pick_a_date();

    let (status, body) =
        discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &pick_a_date_use(), Some(&c), None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    let steps = v["steps"].as_array().expect("a component try answers each of its actions");
    assert_eq!(steps.len(), 3, "{body}");
    assert!(steps.iter().all(|s| s["ok"] == true && s["detail"].is_string()), "{body}");
    assert_eq!(steps[0]["action"], "click #day", "{body}");
    assert_eq!(steps[2]["action"], "click text \"Done\"", "{body}");
    assert!(!body.contains("Secret-Day-17"), "a typed value was handed back: {body}");

    let area = mapped_area(dir.path(), "Leave").unwrap();
    for each in ["click #day", "fill #day", "click text"] {
        assert!(area.outcomes.iter().any(|o| o.starts_with(each)), "no outcome for {each}: {:?}", area.outcomes);
    }
    let file = std::fs::read_to_string(map_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(!file.contains("Secret-Day-17"), "a typed value reached the map: {file}");

    let state = browser.as_ref().unwrap().discovery.as_ref().unwrap();
    assert_eq!(state.tried, vec![draft_fingerprint(&c)]);

    // The fingerprint is stable, ignores how the name is written, and moves
    // with the actions and the inputs.
    let mut renamed = c.clone();
    renamed.name = "  PICK a date ".into();
    renamed.description = "something else".into();
    assert_eq!(draft_fingerprint(&renamed), draft_fingerprint(&c));
    assert_eq!(draft_fingerprint(&c).len(), 64);
    let changed = draft(json!([{ "kind": "click", "selector": { "input": "field" } }]));
    assert_ne!(draft_fingerprint(&changed), draft_fingerprint(&c));
    let mut fewer = c.clone();
    fewer.inputs.pop();
    assert_ne!(draft_fingerprint(&fewer), draft_fingerprint(&c));
}

/// #54/#55: the text a component's `wait_for` waited for, and the element
/// its `expect_text` read, are filed as seen when that action passed - a
/// toast gone a few seconds later included (`autorun_bridge`'s try of a
/// `wait_for` whose text is gone): the read one as one. Saving the
/// component that waits for it then passes the seen check.
#[tokio::test]
async fn a_tried_components_wait_for_and_expect_text_file_what_they_found() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/new?x=1"), exploring("Leave"));
    let c = draft(json!([
        { "kind": "click", "selector": { "input": "field" } },
        { "kind": "fill", "selector": { "input": "field" }, "value": "{{day}}" },
        { "kind": "wait_for", "selector": { "text": "Proficiency profile saved" }, "timeout_ms": 500 },
        { "kind": "expect_text", "selector": { "text": "Saved" }, "equals": "Saved" }
    ]));
    let (status, body) =
        discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &pick_a_date_use(), Some(&c), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["ok"], true, "{body}");

    use v2_lib::browser::locator::SeenKey;
    let area = mapped_area(dir.path(), "Leave").unwrap();
    let matched = |t: &str| area.sightings.iter().find(|s| s.key == SeenKey::Text(t.into())).map(|s| s.matched);
    assert!(matched("Proficiency profile saved").is_some(), "{:?}", area.sightings);
    assert_eq!(matched("Saved"), Some(1), "{:?}", area.sightings);

    let map = load_map(dir.path(), ORG, PROJECT).unwrap();
    let waits: Vec<Action> = serde_json::from_value(json!([
        { "kind": "wait_for", "selector": { "text": "Proficiency profile saved" }, "timeout_ms": 500 },
        { "kind": "expect_text", "selector": { "text": "Saved" }, "equals": "Saved" }
    ]))
    .unwrap();
    assert_eq!(v2_lib::autorun::seen_check::check_component_seen(&map, Some("Leave"), &waits), Ok(()));
}

/// A try stops at the first action that fails, as a script step does; the
/// actions after it are not run and the try is not fingerprinted.
#[tokio::test]
async fn a_failed_try_is_not_fingerprinted() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/new?x=1"), exploring("Leave"));
    let c = draft(json!([
        { "kind": "click", "selector": { "input": "field" } },
        { "kind": "return_to_area", "area": "Nowhere At All" },
        { "kind": "fill", "selector": { "input": "field" }, "value": "{{day}}" }
    ]));

    let (status, body) =
        discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &pick_a_date_use(), Some(&c), None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], false, "{body}");
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "the action after the failure ran: {body}");
    assert_eq!(steps[0]["ok"], true, "{body}");
    assert_eq!(steps[1]["ok"], false, "{body}");
    assert_eq!(v["detail"], steps[1]["detail"], "{body}");

    let state = browser.as_ref().unwrap().discovery.as_ref().unwrap();
    assert!(state.tried.is_empty(), "{:?}", state.tried);
}

/// A draft (or a saved component) whose first action fails says where the
/// browser is: the page's path only, never its host or query. Most often it
/// stands on another screen than the one the component starts on.
#[tokio::test]
async fn a_component_failing_on_its_first_action_says_the_page() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/new?x=1"), exploring("Leave"));
    let c = draft(json!([
        { "kind": "return_to_area", "area": "Nowhere At All" },
        { "kind": "click", "selector": { "input": "field" } },
        { "kind": "fill", "selector": { "input": "field" }, "value": "{{day}}" }
    ]));
    let (status, body) =
        discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &pick_a_date_use(), Some(&c), None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], false, "{body}");
    let detail = v["detail"].as_str().unwrap();
    assert!(detail.ends_with("(the page is /hr/leave/new)"), "{detail}");
    assert_eq!(v["steps"][0]["detail"], v["detail"], "{body}");
    assert!(!body.contains("hr.example.internal") && !body.contains("x=1"), "a host or query came back: {body}");

    // A later action failing is the step's own: no page added.
    let later = draft(json!([
        { "kind": "click", "selector": { "input": "field" } },
        { "kind": "return_to_area", "area": "Nowhere At All" }
    ]));
    let (_, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &pick_a_date_use(), Some(&later), None).await;
    let v = parsed(&body);
    assert_eq!(v["ok"], false, "{body}");
    assert!(!v["detail"].as_str().unwrap().contains("the page is"), "{body}");
}

/// Without a draft the saved component is expanded and run; one that is
/// not saved is refused before the browser is touched.
#[tokio::test]
async fn trying_a_saved_component_expands_it() {
    let dir = root_with_recipe_and_account();
    put(dir.path(), ORG, PROJECT, pick_a_date()).unwrap();
    let (mut browser, _) = slot(leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/new?x=1"), exploring("Leave"));

    let (status, body) =
        discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &pick_a_date_use(), None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    assert_eq!(v["steps"].as_array().map(Vec::len), Some(3), "{body}");
    let state = browser.as_ref().unwrap().discovery.as_ref().unwrap();
    assert_eq!(state.tried, vec![draft_fingerprint(&pick_a_date())]);

    let missing: Action =
        serde_json::from_value(json!({ "kind": "use_component", "component": "Not There" })).unwrap();
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &missing, None, None).await;
    assert_eq!(status, 400, "{body}");
    // How to try a new one is said with it, and with no long dash.
    assert_eq!(body, "\"Not There\" is not saved in this project - to try a new one, send it as draft");

    // A draft for another component is not this use's.
    let mut other = pick_a_date();
    other.name = "Pick a time".into();
    let (status, body) =
        discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &pick_a_date_use(), Some(&other), None).await;
    assert_eq!(status, 400, "{body}");
}

/// The route reads a `draft` only for a `use_component`, and refuses one
/// that is not a component, before the browser is touched.
#[tokio::test]
async fn the_route_takes_a_draft_only_for_a_use_component() {
    let click = json!({ "kind": "click", "selector": "#a" });
    let body = json!({ "action": click, "draft": serde_json::to_value(pick_a_date()).unwrap() }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-action", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("use_component"), "{out}");

    let body = json!({ "action": pick_a_date_use(), "draft": { "name": 5 } }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-action", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("not a component"), "{out}");
}

// ------------------------------------------- a mapping run's summary

/// A mapping run over Leave, signed in as `admin`, standing on
/// `/hr/leave/apply`, whose summary is kept under `root`.
fn mapping_browser_in(root: &std::path::Path) -> (Option<FakeBrowser>, Arc<AtomicBool>) {
    let (mut browser, closed) = slot(menu_app(true, "/hr/leave/apply"), mapping(exploring("Leave"), &["Leave"]));
    let b = browser.as_mut().unwrap();
    b.account = Some("admin".into());
    b.discovery.as_mut().unwrap().mapping.as_mut().unwrap().place =
        Some(MappingPlace { root: root.to_path_buf(), organization: ORG.into(), project: PROJECT.into() });
    (browser, closed)
}

/// Ending a mapping run keeps its summary under the project, answers with
/// it, logs one line of counts and names, and closes the browser. A save
/// the guard blocked after the last action is still counted.
#[tokio::test]
async fn ending_a_mapping_run_saves_and_returns_its_summary() {
    let _log = crate::serial::log_tail();
    let dir = root_with_recipe_and_account();
    let (mut browser, closed) = mapping_browser_in(dir.path());
    let started = the_run(&browser).started_at;

    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    // The assistant goes to the Payroll screen before saving it: standing
    // on Leave Apply, the save would claim that screen, which the run has
    // saved already.
    {
        use v2_lib::browser::cdp::Driver;
        let d = &mut browser.as_mut().unwrap().d;
        d.call("Page.navigate", json!({ "url": "https://hr.example.internal/hr/menu" })).await.unwrap();
    }
    let (status, _) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Payroll", "Payroll", vec![css("#missing")], &quick())
            .await;
    assert_eq!(status, 409);
    // The page tries a save after the last action: nothing drains it but the end.
    browser.as_mut().unwrap().d.saves_stopped.push(("POST".into(), "/api/SaveLastVisited".into()));

    assert_eq!(load_summary(dir.path(), ORG, PROJECT), Ok(None), "kept before the run ended");
    let (status, body) = end_discovery_in(&mut browser).await;
    assert_eq!(status, 200, "{body}");
    assert!(!closed.load(Ordering::SeqCst), "ending the mapping run closed its browser");
    assert!(browser.as_ref().is_some_and(|b| b.discovery.is_none()), "the run is still going");
    assert_eq!(parsed(&body)["detail"], v2_lib::ai_bridge::DISCOVERY_ENDED_KEPT);
    {
        use v2_lib::browser::cdp::Driver;
        let b = browser.as_ref().unwrap();
        assert!(!b.d.is_guarding_saves(), "the mapping run's guard outlived it");
        assert_eq!(b.guard_owner, None);
    }

    let answered: MappingSummary = serde_json::from_value(parsed(&body)["summary"].clone()).expect("no summary");
    assert_eq!(answered.ran_at, started);
    assert_eq!(answered.modules, vec!["Leave".to_string()]);
    assert_eq!(answered.added, vec!["Leave Apply".to_string()]);
    assert!(answered.updated.is_empty() && answered.unchanged.is_empty(), "{answered:?}");
    assert_eq!(answered.unreached.len(), 1, "{answered:?}");
    assert_eq!(answered.unreached[0].name, "Payroll");
    assert!(answered.unreached[0].reason.starts_with("click 1"), "{answered:?}");
    assert_eq!(answered.blocked_writes, 1, "the save blocked after the last action was lost");
    assert_eq!(load_summary(dir.path(), ORG, PROJECT), Ok(Some(answered.clone())));
    let slug = v2_lib::autorun::recipe::project_slug(ORG, PROJECT);
    assert!(summary_path(dir.path(), ORG, PROJECT).ends_with(format!("projects/{slug}-mapping.json")));

    let lines: Vec<String> = v2_lib::applog::recent(500).into_iter().map(|l| l.message).collect();
    let ended = lines.iter().rev().find(|l| l.contains("mapping run ended")).expect("no line for the run");
    assert_eq!(
        ended,
        "Auto Run mapping run ended: 1 added (Leave Apply), 0 updated (none), 0 unchanged (none), 1 not reached (Payroll), 1 saves blocked",
        "a reason or path was logged"
    );

    // Ending again keeps that summary and answers as before.
    let (status, body) = end_discovery_in(&mut browser).await;
    assert_eq!((status, body.as_str()), (200, "no discovery is going"));
    assert_eq!(load_summary(dir.path(), ORG, PROJECT), Ok(Some(answered)));

    // An ordinary discovery keeps none, and answers in words.
    let other = root_with_recipe_and_account();
    let (mut browser, _) = slot(menu_app(true, "/hr/leave"), exploring("Leave"));
    let (status, body) = end_discovery_in(&mut browser).await;
    assert_eq!((status, body.as_str()), (200, v2_lib::ai_bridge::DISCOVERY_ENDED_KEPT));
    assert_eq!(
        v2_lib::ai_bridge::DISCOVERY_ENDED_KEPT,
        "the discovery is over; its browser stays open and signed in, for your next replay or discovery"
    );
    assert_eq!(load_summary(other.path(), ORG, PROJECT), Ok(None));
}

/// A mapping run ended any other way - Close browser here - keeps its
/// summary too, written whole, and the one after replaces it.
#[tokio::test]
async fn a_mapping_run_closed_mid_way_keeps_its_summary() {
    let _log = crate::serial::log_tail();
    let dir = root_with_recipe_and_account();
    let (mut browser, closed) = mapping_browser_in(dir.path());
    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");
    browser.as_mut().unwrap().d.saves_stopped.push(("PUT".into(), "/api/x".into()));

    assert!(close_browser_in(&mut browser));
    assert!(closed.load(Ordering::SeqCst));
    let kept = load_summary(dir.path(), ORG, PROJECT).unwrap().expect("closing lost the summary");
    assert_eq!(kept.added, vec!["Leave Apply".to_string()]);
    assert_eq!(kept.blocked_writes, 1);
    let folder = summary_path(dir.path(), ORG, PROJECT).parent().unwrap().to_path_buf();
    let leftovers: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.contains("tcm-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "a half-written file was left: {leftovers:?}");

    // The next run, closed with nothing done, replaces it.
    let (mut browser, _) = mapping_browser_in(dir.path());
    assert!(close_browser_in(&mut browser));
    let kept = load_summary(dir.path(), ORG, PROJECT).unwrap().unwrap();
    assert!(kept.added.is_empty() && kept.blocked_writes == 0, "{kept:?}");
}

/// A screen met more than once is in one list only: the list of its last
/// outcome, names compared as area names are.
#[test]
fn the_summary_keeps_each_screens_last_outcome() {
    let mut run = MappingRun::new(&["Leave".to_string()]);
    run.record_added("Leave Apply".into());
    run.record_unchanged(" leave  APPLY".into());
    run.record_updated("Payroll".into(), "Payroll".into(), "Pay, then Payroll".into());
    run.record_unreached("Payroll".into(), "click 1 was not found");
    run.record_unchanged("Claims".into());
    run.record_unchanged("claims".into());
    run.record_unreached("Reports".into(), "click 2 was not found");
    run.record_added("Reports".into());

    let s = summarize(&run);
    assert_eq!(s.added, vec!["Reports".to_string()]);
    assert!(s.updated.is_empty(), "{s:?}");
    assert_eq!(s.unchanged, vec!["leave  APPLY".to_string(), "claims".to_string()]);
    assert_eq!(s.unreached.len(), 1, "{s:?}");
    assert_eq!((s.unreached[0].name.as_str(), s.unreached[0].reason.as_str()), ("Payroll", "click 1 was not found"));
}

/// Nothing the summary keeps, answers or logs holds a host, a query string
/// or an address: not a replay's reason, and not a failure while going home
/// that named the address it was opening.
#[tokio::test]
async fn the_summary_names_no_address() {
    let _log = crate::serial::log_tail();
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = mapping_browser_in(dir.path());
    let (status, _) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Payroll", "Payroll", vec![css("#missing")], &quick())
            .await;
    assert_eq!(status, 409);
    {
        let run = browser.as_mut().unwrap().discovery.as_mut().unwrap().mapping.as_mut().unwrap();
        run.record_unreached(
            "Leave Apply".into(),
            "home: could not open \"https://hr.example.internal/hr/home?token=t0p-secret#top\" (timed out)",
        );
        assert!(!run.unreached[1].1.contains("t0p-secret"), "stored with its address: {run:?}");
        // Set straight on the list, past `record_unreached`: cleaned on the way out.
        run.unreached.push(("Claims".into(), "went to http://hr.example.internal:8080/claims?id=5 instead".into()));
        run.record_updated("Reports".into(), "Reports".into(), "Reports, then /hr/reports?x=t0p-secret".into());
    }
    let (status, body) = end_discovery_in(&mut browser).await;
    assert_eq!(status, 200, "{body}");

    let file = std::fs::read_to_string(summary_path(dir.path(), ORG, PROJECT)).unwrap();
    let lines: Vec<String> = v2_lib::applog::recent(500).into_iter().map(|l| l.message).collect();
    let logged = lines.iter().filter(|l| l.contains("mapping run")).cloned().collect::<Vec<_>>().join("\n");
    for text in [&file, &body, &logged] {
        for leak in ["hr.example.internal", "t0p-secret", "://", "?", "#top", "8080"] {
            assert!(!text.contains(leak), "{leak} in {text}");
        }
    }
    let kept = load_summary(dir.path(), ORG, PROJECT).unwrap().unwrap();
    let reason = |name: &str| kept.unreached.iter().find(|u| u.name == name).unwrap().reason.clone();
    assert_eq!(reason("Leave Apply"), "home: could not open \"/hr/home\" (timed out)");
    assert_eq!(reason("Claims"), "went to /claims instead");
    assert_eq!(kept.updated[0].new_path, "Reports, then /hr/reports");
}

// ------------------------- a save refused for a locator never seen

use v2_lib::ai_bridge::{record_refused_in, save_component_in};
use v2_lib::autorun::components::UserCases;
use v2_lib::browser::snapshot::PROBE_SUMMARY_JS;

/// A page at `/hr/cycles` on which every locator finds `found` elements,
/// each `visible` or not. Anything a probe never sends (a click, a key)
/// fails the test.
fn cycles_page(found: usize, visible: bool) -> ScriptedDriver {
    cycles_page_at("https://hr.example.internal/hr/cycles?page=2", found, visible)
}

/// `cycles_page`, at `href`. A legacy string resolves as `LEGACY_JS`
/// does, to its first match only, and `LEGACY_COUNT_JS` counts every one.
fn cycles_page_at(href: &'static str, found: usize, visible: bool) -> ScriptedDriver {
    let mut legacy_next = false;
    ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.callFunctionOn" if f == v2_lib::browser::locator::LEGACY_JS => {
                legacy_next = true;
                Ok(json!({ "result": { "objectId": "arr" } }))
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::locator::LEGACY_COUNT_JS => {
                Ok(json!({ "result": { "value": found } }))
            }
            "Runtime.evaluate" if params["expression"] == "document" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.evaluate" if params["expression"] == "location.href" => Ok(json!({ "result": { "value": href } })),
            "Runtime.evaluate" if params["expression"] == "document.title" => Ok(json!({ "result": { "value": "Cycles" } })),
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": visible } })),
            "Accessibility.getPartialAXTree" => Ok(serde_json::json!({ "nodes": [] })),
            "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => Ok(json!({
                "result": { "value": { "tag": "button", "text": "2", "rect": [10.0, 20.0, 30.0, 24.0] } }
            })),
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => {
                let n = if std::mem::take(&mut legacy_next) { found.min(1) } else { found };
                Ok(json!({ "result": (0..n)
                    .map(|i| json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } }))
                    .collect::<Vec<_>>() }))
            }
            other => panic!("a probe never sends {other} {params}"),
        }
    })
}

/// A driver that fails the test on any call at all: nothing was probed.
fn untouched_page() -> ScriptedDriver {
    ScriptedDriver::new(|method, params| panic!("nothing should reach the page, got {method} {params}"))
}

/// "Next page": one click on the pager's page 2 button, which only shows
/// with more than ten cycles.
fn next_page() -> Component {
    serde_json::from_value(json!({
        "name": "Next page",
        "description": "Opens the second page of cycles",
        "inputs": [],
        "actions": [{ "kind": "click", "selector": { "css": "#pager-2" } }]
    }))
    .unwrap()
}

/// A discovery of `area` that has tried `c`.
fn tried_in(area: &str, c: &Component) -> Option<DiscoveryState> {
    exploring(area).map(|s| DiscoveryState { tried: vec![draft_fingerprint(c)], ..s })
}

/// Spec group 5: a component written before its locator was ever on the
/// page is refused, the refused locator is checked on the discovery's
/// page, found there once and visible, recorded under the discovery's
/// area, and the save passes on its one retry. The answer says so first.
#[tokio::test]
async fn a_save_refused_in_discovery_records_a_locator_on_the_page_and_passes() {
    let dir = TempDir::new();
    let c = next_page();
    let (mut browser, _) = slot(cycles_page(1, true), tried_in("Cycles", &c));
    let now = 5;

    let (status, body) =
        save_component_in(&mut browser, dir.path(), ORG, PROJECT, c.clone(), None, now, Some(&UserCases::default())).await;
    assert_eq!(status, 200, "{body}");
    let (said, saved) = body.split_once('\n').expect("the recorded line, then the save's answer");
    assert_eq!(said, "Recorded on the current page: #pager-2.");
    assert_eq!(parsed(saved)["version"], 1, "{body}");

    let area = mapped_area(dir.path(), "Cycles").expect("nothing was recorded");
    let page = area.pages.iter().find(|p| p.path == "/hr/cycles").expect("no page, or a query was kept");
    assert!(
        page.elements.iter().any(|e| e.key == v2_lib::browser::locator::SeenKey::Css("#pager-2".into())),
        "{:?}",
        page.elements
    );

    // Found twice: recorded with its count, but a click needs a locator seen
    // matching exactly one, so the save is still refused and says why.
    let fresh = TempDir::new();
    let (mut browser, _) = slot(cycles_page(2, true), tried_in("Cycles", &c));
    let (status, body) =
        save_component_in(&mut browser, fresh.path(), ORG, PROJECT, c.clone(), None, now, Some(&UserCases::default())).await;
    assert_eq!(status, 400, "{body}");
    assert!(
        body.starts_with(
            "Recorded on the current page: #pager-2 (matched 2 elements).\nAction 1: #pager-2 was seen on the live app only matching several elements"
        ),
        "{body}"
    );
    let twice = mapped_area(fresh.path(), "Cycles").expect("nothing was recorded");
    assert!(twice.sightings.iter().any(|s| s.matched == 2), "{:?}", twice.sightings);

    // Found hidden, or not found: nothing is recorded, and the save's own
    // refusal follows the line that says so.
    for (found, visible) in [(1, false), (0, true)] {
        let fresh = TempDir::new();
        let (mut browser, _) = slot(cycles_page(found, visible), tried_in("Cycles", &c));
        let (status, body) =
            save_component_in(&mut browser, fresh.path(), ORG, PROJECT, c.clone(), None, now, Some(&UserCases::default()))
                .await;
        assert_eq!(status, 400, "{found} {visible}: {body}");
        assert!(
            body.starts_with(
                "Recorded on the current page: nothing - no refused locator matched a visible element.\nAction 1: #pager-2 was never seen on the live app"
            ),
            "{found} {visible}: {body}"
        );
        assert!(load_map(fresh.path(), ORG, PROJECT).unwrap().areas.is_empty(), "{found} {visible}");
    }
}

/// During a discovery, a refused locator found on the page matching several
/// visible elements is recorded with how many it matched, and the answer
/// says so. Found matching several but none of them visible, it is not.
#[tokio::test]
async fn record_on_page_records_a_many_match_with_its_count() {
    let dir = TempDir::new();
    let (mut browser, _) = slot(cycles_page(2, true), exploring("Cycles"));
    let probed = record_refused_in(&mut browser, dir.path(), ORG, PROJECT, &[Target::from("#pager-2")]).await;
    assert_eq!(probed, Some(vec!["#pager-2 (matched 2 elements)".to_string()]));
    let area = mapped_area(dir.path(), "Cycles").expect("nothing was recorded");
    let key = v2_lib::browser::locator::SeenKey::Css("#pager-2".into());
    assert_eq!(area.sightings.iter().find(|s| s.key == key).map(|s| s.matched), Some(2), "{:?}", area.sightings);
    let page = area.pages.iter().find(|p| p.path == "/hr/cycles").expect("no page");
    assert_eq!(page.elements.iter().find(|e| e.key == key).map(|e| e.matched), Some(2), "{:?}", page.elements);
    assert_eq!(
        v2_lib::ai_bridge::after_recording(probed.as_deref().unwrap(), (400, "refused".to_string())).1,
        "Recorded on the current page: #pager-2 (matched 2 elements).\nrefused"
    );

    let hidden = TempDir::new();
    let (mut browser, _) = slot(cycles_page(3, false), exploring("Cycles"));
    let probed = record_refused_in(&mut browser, hidden.path(), ORG, PROJECT, &[Target::from("#pager-2")]).await;
    assert_eq!(probed, Some(Vec::new()));
    assert!(load_map(hidden.path(), ORG, PROJECT).unwrap().areas.is_empty());
}

/// The save that started this: a component whose `expect_hidden` names
/// any Edit button but the shared draft's, with a `:not(...)` selector.
/// Before the search filters the list that selector matches two buttons,
/// so the refused save's page check records it matching two, and the save
/// passes on its retry: a check that can pass with several matches may
/// use a locator seen matching several.
#[tokio::test]
async fn a_component_expect_hidden_on_a_not_selector_seen_matching_two_saves() {
    let dir = TempDir::new();
    let selector = ".cycle-row:not([data-cycle-id=\"10301\"]) button[title=\"Edit\"]";
    let c: Component = serde_json::from_value(json!({
        "name": "Only the draft is editable",
        "description": "Checks no other cycle offers Edit",
        "inputs": [],
        "actions": [{ "kind": "expect_hidden", "selector": { "css": selector } }]
    }))
    .unwrap();
    let (mut browser, _) = slot(cycles_page(2, true), tried_in("Cycles", &c));
    let (status, body) =
        save_component_in(&mut browser, dir.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default())).await;
    assert_eq!(status, 200, "{body}");
    let (said, saved) = body.split_once('\n').expect("the recorded line, then the save's answer");
    assert_eq!(said, format!("Recorded on the current page: {selector} (matched 2 elements)."));
    assert_eq!(parsed(saved)["version"], 1, "{body}");
}

/// With no discovery going, the save is refused as it always was: nothing
/// reaches the page and nothing is recorded. In discovery, a refusal for
/// anything but unseen locators is never probed either.
#[tokio::test]
async fn a_save_refused_outside_discovery_records_nothing() {
    let c = next_page();

    // The person's own browser: no discovery, so nothing was tried.
    let dir = TempDir::new();
    let (mut browser, _) = slot(untouched_page(), None);
    let (status, body) =
        save_component_in(&mut browser, dir.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default())).await;
    assert_eq!(status, 409, "{body}");
    assert!(!body.contains("Recorded on the current page"), "{body}");
    assert!(load_map(dir.path(), ORG, PROJECT).unwrap().areas.is_empty());

    // No browser at all.
    let mut empty: Option<FakeBrowser> = None;
    let (status, body) =
        save_component_in(&mut empty, dir.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default())).await;
    assert_eq!(status, 409, "{body}");
    assert!(load_map(dir.path(), ORG, PROJECT).unwrap().areas.is_empty());

    // In discovery, refused for a page address it never saw as well: not
    // probed, and the refusal is the save's own.
    let away: Component = serde_json::from_value(json!({
        "name": "Away", "description": "d", "inputs": [],
        "actions": [
            { "kind": "navigate", "url": "https://hr.example.internal/hr/elsewhere" },
            { "kind": "click", "selector": { "css": "#pager-2" } }
        ]
    }))
    .unwrap();
    let (mut browser, _) = slot(untouched_page(), tried_in("Cycles", &away));
    let (status, body) =
        save_component_in(&mut browser, dir.path(), ORG, PROJECT, away, None, 5, Some(&UserCases::default())).await;
    assert_eq!(status, 400, "{body}");
    assert!(body.starts_with("Action 1: /hr/elsewhere was never seen"), "{body}");
    assert!(load_map(dir.path(), ORG, PROJECT).unwrap().areas.is_empty());

    // Outside discovery the page check itself does nothing.
    let (mut browser, _) = slot(untouched_page(), None);
    let probed = record_refused_in(&mut browser, dir.path(), ORG, PROJECT, &[Target::from("#pager-2")]).await;
    assert_eq!(probed, None);
}

/// A refused locator holding a placeholder, or a component input's place,
/// cannot be probed as written: it is left refused and never reaches the
/// page.
#[tokio::test]
async fn a_refused_locator_with_a_placeholder_is_never_probed() {
    let dir = TempDir::new();
    let (mut browser, _) = slot(untouched_page(), exploring("Cycles"));
    let held: Target = serde_json::from_value(json!({ "css": "div[data-cycle-id=\"{{setup.cycle_id}}\"]" })).unwrap();
    let input: Target = serde_json::from_value(json!({ "input": "row" })).unwrap();
    let probed = record_refused_in(&mut browser, dir.path(), ORG, PROJECT, &[held, input]).await;
    assert_eq!(probed, Some(Vec::new()));
    assert!(load_map(dir.path(), ORG, PROJECT).unwrap().areas.is_empty());
}

/// A script save is checked in each script's own area, and a sighting is
/// filed under the discovery's: with any script on another area (or on
/// none while the discovery has one), nothing reaches the page and nothing
/// is recorded. The same area, written in another case or spacing, is
/// checked as usual.
#[tokio::test]
async fn a_refused_script_save_on_another_area_records_nothing() {
    use v2_lib::ai_bridge::record_refused_for_scripts_in;
    let pager = [Target::from("#pager-2")];

    let dir = TempDir::new();
    for areas in [vec![Some("Ratings")], vec![Some("Cycles"), Some("Ratings")], vec![None]] {
        let (mut browser, _) = slot(untouched_page(), exploring("Cycles"));
        let probed = record_refused_for_scripts_in(&mut browser, dir.path(), ORG, PROJECT, &areas, &pager).await;
        assert_eq!(probed, None, "{areas:?}");
    }
    assert!(load_map(dir.path(), ORG, PROJECT).unwrap().areas.is_empty());

    let (mut browser, _) = slot(cycles_page(1, true), exploring("Cycles"));
    let areas = [Some("Cycles"), Some("  cycles ")];
    let probed = record_refused_for_scripts_in(&mut browser, dir.path(), ORG, PROJECT, &areas, &pager).await;
    assert_eq!(probed, Some(vec!["#pager-2".to_string()]));
    assert!(mapped_area(dir.path(), "Cycles").is_some(), "nothing was recorded");
}

/// The log says how many refused locators were recorded or could not be
/// checked, never which: a locator is the assistant's writing and can hold
/// an address with a query string.
#[tokio::test]
async fn a_refused_locator_is_counted_in_the_log_never_written_out() {
    use v2_lib::browser::cdp::CdpError;
    let _log = crate::serial::log_tail();
    let link: Target =
        serde_json::from_value(json!({ "css": "a[href=\"https://hr.example.internal/hr/cycles?secret=7\"]" })).unwrap();
    let ours = |lines: &[String]| -> Vec<String> {
        lines.iter().filter(|l| l.starts_with("Auto Run save:")).cloned().collect()
    };
    let dir = TempDir::new();
    let (mut browser, _) = slot(cycles_page(1, true), exploring("Cycles"));
    let probed = record_refused_in(&mut browser, dir.path(), ORG, PROJECT, std::slice::from_ref(&link)).await;
    assert_eq!(probed.map(|p| p.len()), Some(1));

    let broken = ScriptedDriver::new(|method, _| Err(CdpError::Protocol { method: method.to_string(), message: "boom".into() }));
    let (mut browser, _) = slot(broken, exploring("Cycles"));
    let probed = record_refused_in(&mut browser, dir.path(), ORG, PROJECT, std::slice::from_ref(&link)).await;
    assert_eq!(probed, Some(Vec::new()));

    let lines: Vec<String> = v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect();
    let said = ours(&lines);
    assert!(said.iter().any(|l| l.contains("recorded 1 of 1 refused locator(s)")), "{said:?}");
    assert!(said.iter().any(|l| l.contains("1 refused locator(s) could not be checked")), "{said:?}");
    for l in &said {
        assert!(!l.contains("hr.example.internal") && !l.contains("secret") && !l.contains("a[href"), "{l}");
    }
}

// ---- the assistant's replay ends its own discovery ---------------------------

/// The assistant's replay to a step, asked for while its discovery holds
/// the browser, ends that discovery the way End discovery does - the map
/// it filled is kept - and then replays in that same browser, still signed
/// in: the end and the replay sit under the one session lock, with nothing
/// between them.
#[tokio::test]
async fn a_replay_requested_during_a_discovery_ends_it_and_replays() {
    use v2_lib::ai_bridge::end_discovery_for_replay;
    let _log = crate::serial::log_tail();
    let dir = root_with_recipe_and_account();
    v2_lib::autorun::discovery_map::record_outcome(dir.path(), ORG, PROJECT, "Leave", "click Apply: moved to /hr/leave")
        .unwrap();
    let (mut browser, closed) = slot(menu_app(true, "/hr/leave"), exploring("Leave"));

    let ended = end_discovery_for_replay(&mut browser, dir.path()).await;
    assert!(ended.ended, "the discovery was not ended");
    assert_eq!(ended.summary, None, "an ordinary discovery has no mapping summary");
    assert!(!closed.load(Ordering::SeqCst), "the replay closed the discovery's browser");
    assert!(browser.as_ref().is_some_and(|b| b.discovery.is_none()), "the discovery is still going");
    let kept = mapped_area(dir.path(), "Leave").expect("ending the discovery lost its map");
    assert_eq!(kept.outcomes, vec!["click Apply: moved to /hr/leave".to_string()]);

    // Nothing to end: the person's own browser, or none, is left alone.
    let (mut theirs, theirs_closed) = slot(FakePage::default().driver(), None);
    assert!(!end_discovery_for_replay(&mut theirs, dir.path()).await.ended);
    assert!(theirs.is_some() && !theirs_closed.load(Ordering::SeqCst));
    let mut none: Option<FakeBrowser> = None;
    assert_eq!(end_discovery_for_replay(&mut none, dir.path()).await, v2_lib::ai_bridge::DiscoveryEnded::default());

    // In the app: the assistant's replay ends it under the lock, says so
    // to the window, and only then opens and replays.
    let source = include_str!("../../src/commands/autorun.rs");
    let replay = &source[source.find("pub(crate) async fn replay_supervised").unwrap()..];
    let replay = &replay[..replay.find("replay_to_traced(").unwrap()];
    let lock = replay.find("session_slot(n).lock().await").unwrap();
    let end = replay.find("end_discovery_for_replay(&mut slot, &root)").expect("the assistant's replay does not end it");
    let ended = replay.find("if discovery.ended {").expect("the window is told even when nothing ended");
    let publish = replay.find("publish_discovery_in(n, &slot)").expect("the window is not told");
    let open = replay.find("open_if_none(").unwrap();
    assert!(lock < end && end < ended && ended < publish && publish < open, "the end is not under the replay's lock");
    assert_eq!(replay.matches("session_slot(n).lock()").count(), 1, "the lock is let go between the end and the replay");
    assert_eq!(replay.matches("SESSION.lock()").count(), 0, "the replay locks main's session whichever slot it is for");
}

/// A mapping run ended by the assistant's replay keeps its summary, as
/// End discovery would, and its browser for the replay.
#[tokio::test]
async fn a_mapping_run_ended_by_a_replay_saves_its_summary() {
    use v2_lib::ai_bridge::end_discovery_for_replay;
    let _log = crate::serial::log_tail();
    let dir = root_with_recipe_and_account();
    let (mut browser, closed) = mapping_browser_in(dir.path());
    let clicks = vec![css("#leave"), css("#apply")];
    let (status, body) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Leave Apply", "Leave", clicks, &quick()).await;
    assert_eq!(status, 200, "{body}");

    let ended = end_discovery_for_replay(&mut browser, dir.path()).await;
    assert!(ended.ended && !closed.load(Ordering::SeqCst) && browser.is_some());
    let kept = load_summary(dir.path(), ORG, PROJECT).unwrap().expect("the replay lost the run's summary");
    assert_eq!(kept.added, vec!["Leave Apply".to_string()]);
    assert_eq!(ended.summary, Some(kept), "the replay does not hand back the summary it kept");
    assert!(find_area(&load_nav(dir.path(), ORG, PROJECT).unwrap(), "Leave Apply").is_some(), "the saved area went");
}

/// Only the assistant's replay ends a discovery. The person's own Open
/// browser, Run step and Replay to step are still refused while one holds
/// the browser.
#[tokio::test]
async fn the_person_open_browser_is_still_refused_while_discovering() {
    let (mut discovering, closed) = slot(FakePage::default().driver(), exploring("Leave"));
    assert_eq!(refuse_while_discovering(&mut discovering), Err(busy_browser_sentence(true).to_string()));
    assert!(discovering.is_some() && !closed.load(Ordering::SeqCst));

    let source = include_str!("../../src/commands/autorun.rs");
    let open = &source[source.find("pub async fn auto_run_open_browser").unwrap()..];
    let open = &open[..open.find("open_into(").unwrap()];
    assert!(open.contains("refuse_while_discovering(&mut slot)?"), "Open browser no longer refuses");
    assert!(!open.contains("end_discovery_for_replay"), "Open browser ends a discovery");

    // The person's Replay to step is `ReplayBy::Person`, which refuses.
    // The person's replay may lift any guard; the assistant's only one it
    // left itself for another case's call, never the person's or a no_save
    // discovery's.
    use v2_lib::commands::autorun::ReplayBy;
    let call = GuardOwner::Call { case_id: Some(5) };
    assert!(!ReplayBy::Person.ends_discovery() && ReplayBy::Person.may_lift(Some(&GuardOwner::Person)));
    assert!(ReplayBy::Assistant.ends_discovery() && !ReplayBy::Assistant.may_lift(Some(&GuardOwner::Person)));
    assert!(!ReplayBy::Assistant.may_lift(Some(&GuardOwner::Discovery)));
    assert!(ReplayBy::Assistant.may_lift(Some(&call)) && ReplayBy::Assistant.may_lift(None));
    let person = &source[source.find("pub async fn auto_run_replay_to_step").unwrap()..];
    let person = &person[..person.find("pub(crate) async fn replay_supervised").unwrap()];
    assert!(person.contains("replay_supervised(&app, SlotNo::MAIN, &organization, &project, req, ReplayBy::Person, None)"), "{person}");
    let replay = &source[source.find("pub(crate) async fn replay_supervised").unwrap()..];
    let replay = &replay[..replay.find("open_if_none(").unwrap()];
    assert!(replay.contains("if by.ends_discovery() {"), "{replay}");
    assert!(replay.contains("} else if let Err(why) = crate::ai_bridge::refuse_while_discovering(&mut slot) {"));
    // And the assistant's host is the only caller that ends a discovery.
    let host = include_str!("../../src/commands/ai_bridge.rs");
    assert!(host.contains("replay_supervised(&self.0, slot, &organization, &project, req, ReplayBy::Assistant, holder.as_ref())"));
    assert_eq!(source.matches("ReplayBy::Assistant)").count(), 0, "a person's path replays as the assistant");
    // Every call of `replay_supervised` in this file (not its head) is a
    // person's path, and none names the assistant between its name and its
    // `.await`: the assistant's only caller is the host above. The argument
    // list ends in the holder now, so the old needle alone could not fail.
    let mut calls = 0;
    for (at, _) in source.match_indices("replay_supervised(") {
        if source[..at].ends_with("async fn ") {
            continue;
        }
        let call = &source[at..];
        let call = &call[..call.find(".await").expect("a call of replay_supervised is awaited")];
        assert!(!call.contains("ReplayBy::Assistant"), "a person's path replays as the assistant: {call}");
        calls += 1;
    }
    assert!(calls >= 1, "no call of replay_supervised was found to check");
    assert_eq!(source.matches("refuse_while_discovering(&mut slot)").count(), 3, "a refusal went missing");
}

// ------------------------------------------ a page read during a discovery

use v2_lib::ai_bridge::{page_read_in, READ_OFF_THE_APP, READ_WITH_NO_AREA};

/// The Leave page, on the recipe's own origin, with a query string nothing
/// may keep.
const LEAVE_PAGE: &str = "https://hr.example.internal/hr/leave?token=t0p-secret#top";

/// The AX tree of a page holding one button per name, under a root the
/// snapshot folds away: each button is one printed line.
fn buttons_tree(names: &[String]) -> Value {
    let mut nodes = vec![json!({
        "nodeId": "root", "ignored": true, "role": { "value": "generic" },
        "childIds": (0..names.len()).map(|i| format!("b{i}")).collect::<Vec<_>>()
    })];
    for (i, n) in names.iter().enumerate() {
        nodes.push(json!({
            "nodeId": format!("b{i}"), "ignored": false, "role": { "value": "button" },
            "name": { "value": n }, "childIds": []
        }));
    }
    json!({ "nodes": nodes })
}

/// A page at `href` holding a button for each of `names`.
fn buttons_page(href: &str, names: &[&str]) -> ScriptedDriver {
    let href = href.to_string();
    let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    ScriptedDriver::new(move |method, params| {
        Ok(match method {
            "Accessibility.getFullAXTree" => buttons_tree(&names),
            "Runtime.evaluate" if params["expression"] == "location.href" => json!({ "result": { "value": href } }),
            "Runtime.evaluate" if params["expression"] == "document.title" => {
                json!({ "result": { "value": "Leave" } })
            }
            _ => json!({}),
        })
    })
}

/// A page at `href` on which an action works as on `FakePage`, holding a
/// button for each of `names`.
fn acting_page(href: &'static str, names: Vec<String>) -> ScriptedDriver {
    let page = FakePage { href, ..FakePage::default() };
    ScriptedDriver::new(move |method, params| match method {
        "Accessibility.getFullAXTree" => Ok(buttons_tree(&names)),
        _ => page.answer(method, params),
    })
}

/// The names of the buttons the area has sighted, sorted.
fn sighted(root: &std::path::Path, area: &str) -> Vec<String> {
    let mut names: Vec<String> = mapped_area(root, area)
        .map(|a| a.sightings.into_iter().filter_map(|s| s.link.name).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// The names of the elements the map holds for the Leave page now, sorted:
/// what a discovery's whole read replaces (sightings it never drops).
fn on_leave_page(root: &std::path::Path) -> Vec<String> {
    let area = mapped_area(root, "Leave").expect("no area");
    let page = area.pages.iter().find(|p| p.path == "/hr/leave").expect("no page");
    let mut names: Vec<String> = page.elements.iter().map(|e| e.name.clone()).collect();
    names.sort();
    names
}

/// `exploring(area)`, as a discovery that started after everything filed so
/// far: a read of the whole page would drop what it does not show.
fn exploring_anew(area: &str) -> Option<DiscoveryState> {
    exploring(area).map(|s| DiscoveryState { started_at: u64::MAX, ..s })
}

/// During a discovery with an area, every line the read returned is filed
/// under that area, keyed and kept as an action's read is, and the answer
/// ends by saying how many were recorded and where. The page text itself
/// is unchanged and names no host or query string.
#[tokio::test]
async fn a_page_read_in_discovery_records_its_lines() {
    let dir = root_with_recipe_and_account();
    let (mut held, _) = slot(buttons_page(LEAVE_PAGE, &["Save", "Cancel"]), exploring("Leave"));
    let (status, text) =
        page_read_in(held.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(status, 200, "{text}");
    let (_, plain) = read_page(&mut buttons_page(LEAVE_PAGE, &["Save", "Cancel"]), DEFAULT_LIMIT, None).await;
    assert_eq!(text, format!("{plain}\n\nRecorded 2 elements as seen on Leave."));
    assert!(!text.contains("hr.example.internal") && !text.contains("t0p-secret"), "{text}");

    let area = mapped_area(dir.path(), "Leave").expect("nothing was recorded under the area");
    assert_eq!(sighted(dir.path(), "Leave"), ["Cancel", "Save"]);
    assert!(area.sightings.iter().all(|s| s.page == "/hr/leave"), "{:?}", area.sightings);
    assert!(area.explored_at.is_some(), "a discovery's read did not stamp the area");
    assert_eq!(area.account.as_deref(), Some("admin"));
    let file = std::fs::read_to_string(map_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(!file.contains("t0p-secret") && !file.contains('?'), "{file}");
}

/// Outside a discovery the read is filed as it was before (the
/// unattributed bucket here), explores nothing, and the answer is the page
/// text alone.
#[tokio::test]
async fn a_page_read_outside_discovery_adds_no_answer_line_and_explores_nothing() {
    let dir = root_with_recipe_and_account();
    let (mut held, _) = slot(buttons_page(LEAVE_PAGE, &["Save"]), None);
    let (status, text) =
        page_read_in(held.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(status, 200, "{text}");
    let (_, plain) = read_page(&mut buttons_page(LEAVE_PAGE, &["Save"]), DEFAULT_LIMIT, None).await;
    assert_eq!(text, plain, "a read outside a discovery gained a line");
    assert!(!text.contains("Recorded"), "{text}");

    let map = load_map(dir.path(), ORG, PROJECT).unwrap();
    assert!(map.areas.iter().all(|a| a.explored_at.is_none()), "{map:?}");
    assert_eq!(sighted(dir.path(), ""), ["Save"], "the shipped recording outside a discovery went");
}

/// A read cut short by its limit files only the lines it returned, and
/// drops nothing past the cut: it never counts as exploring the whole page.
/// The same page read whole by a later discovery does drop the page's
/// elements it does not show, which is what the cut is kept from; the
/// sightings stay.
#[tokio::test]
async fn a_limited_read_records_only_returned_lines() {
    let dir = root_with_recipe_and_account();
    let (mut first, _) = slot(buttons_page(LEAVE_PAGE, &["Earlier", "Older"]), exploring("Leave"));
    page_read_in(first.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(sighted(dir.path(), "Leave"), ["Earlier", "Older"]);

    let (mut cut, _) = slot(buttons_page(LEAVE_PAGE, &["Save", "Cancel", "Delete"]), exploring_anew("Leave"));
    let (status, text) = page_read_in(cut.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, 1).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("2 more lines - call get_autorun_page"), "the read was not cut: {text}");
    assert!(text.ends_with("\n\nRecorded 1 element as seen on Leave."), "{text}");
    assert_eq!(sighted(dir.path(), "Leave"), ["Earlier", "Older", "Save"], "a cut read dropped or overreached");
    let area = mapped_area(dir.path(), "Leave").unwrap();
    let page = area.pages.iter().find(|p| p.path == "/hr/leave").expect("no page");
    let names: Vec<&str> = page.elements.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"Earlier") && names.contains(&"Older") && names.contains(&"Save"), "{names:?}");
    assert!(!names.contains(&"Cancel") && !names.contains(&"Delete"), "{names:?}");

    let (mut whole, _) = slot(buttons_page(LEAVE_PAGE, &["Save"]), exploring_anew("Leave"));
    page_read_in(whole.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(on_leave_page(dir.path()), ["Save"], "a whole read no longer explores the page");
    assert_eq!(sighted(dir.path(), "Leave"), ["Earlier", "Older", "Save"], "a whole read dropped a sighting");
}

/// An action's read is the same path: cut at its limit, it files only what
/// it returned and keeps what the area saw on the page before.
#[tokio::test]
async fn a_cut_action_read_records_only_returned_lines() {
    let dir = root_with_recipe_and_account();
    let (mut first, _) = slot(buttons_page(LEAVE_PAGE, &["Earlier"]), exploring("Leave"));
    page_read_in(first.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;

    let limit = v2_lib::browser::snapshot::PAGE_LINES_DEFAULT;
    let names: Vec<String> = (0..limit + 5).map(|i| format!("Row {i:03}")).collect();
    let d = acting_page("https://hr.example.internal/hr/leave", names);
    let (mut browser, _) = slot(d, exploring_anew("Leave"));
    let click = Action::Click { selector: "#save".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &click, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    assert!(v["page"].as_str().unwrap_or("").contains("5 more lines - "), "the action's read was not cut");

    let seen = sighted(dir.path(), "Leave");
    assert!(seen.contains(&"Earlier".to_string()), "a cut action read dropped an earlier sighting: {seen:?}");
    assert!(seen.contains(&"Row 000".to_string()) && seen.contains(&format!("Row {:03}", limit - 1)));
    assert!(!seen.contains(&format!("Row {:03}", limit)), "a line past the cut was recorded");
}

/// With no current area, nothing is recorded, and the answer ends by
/// saying how to name one.
#[tokio::test]
async fn a_read_with_no_area_says_so() {
    let dir = root_with_recipe_and_account();
    let (mut held, _) = slot(buttons_page(LEAVE_PAGE, &["Save"]), opened_for_discovery());
    let (status, text) =
        page_read_in(held.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.ends_with(&format!("\n\n{READ_WITH_NO_AREA}")), "{text}");
    assert!(READ_WITH_NO_AREA.contains("save_autorun_area") && READ_WITH_NO_AREA.contains("discover_autorun_actions"));
    assert!(load_map(dir.path(), ORG, PROJECT).unwrap().areas.is_empty(), "a read with no area recorded");
}

/// A page off the application's own origins files nothing, whether a page
/// read or an action's read shows it, and a page read says so.
#[tokio::test]
async fn a_page_read_off_the_app_records_nothing() {
    let dir = root_with_recipe_and_account();
    let (mut held, _) =
        slot(buttons_page("https://elsewhere.example/sso?ticket=abc", &["Continue"]), exploring("Leave"));
    let (status, text) =
        page_read_in(held.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.ends_with(&format!("\n\n{READ_OFF_THE_APP}")), "{text}");
    assert!(!text.contains("elsewhere.example") && !text.contains("ticket"), "{text}");
    assert!(load_map(dir.path(), ORG, PROJECT).unwrap().areas.is_empty(), "an off-site page was recorded");

    let d = acting_page("https://elsewhere.example/sso", vec!["Continue".to_string()]);
    let (mut browser, _) = slot(d, exploring("Leave"));
    let click = Action::Click { selector: "#save".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &click, None, None).await;
    assert_eq!(status, 200, "{body}");
    assert!(sighted(dir.path(), "Leave").is_empty(), "an action's read of an off-site page was recorded");
}

/// A read the limit cut short saw only part of the page: it never stamps
/// the area explored, never clears a failure and never sets the account.
/// A whole read of the same page does all three.
#[tokio::test]
async fn a_cut_read_does_not_freshen_the_area() {
    let dir = root_with_recipe_and_account();
    v2_lib::autorun::discovery_map::mark_failed(dir.path(), ORG, PROJECT, "Leave").unwrap();
    let (mut cut, _) = slot(buttons_page(LEAVE_PAGE, &["Save", "Cancel"]), exploring("Leave"));
    let (_, text) = page_read_in(cut.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, 1).await;
    assert!(text.ends_with("Recorded 1 element as seen on Leave."), "{text}");
    let area = mapped_area(dir.path(), "Leave").unwrap();
    assert_eq!(area.explored_at, None, "a cut read stamped the area explored");
    assert!(area.failed_since, "a cut read cleared the area's failure");
    assert_eq!(area.account, None, "a cut read set the account");

    let (mut whole, _) = slot(buttons_page(LEAVE_PAGE, &["Save", "Cancel"]), exploring("Leave"));
    page_read_in(whole.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    let area = mapped_area(dir.path(), "Leave").unwrap();
    assert!(area.explored_at.is_some() && !area.failed_since, "{area:?}");
    assert_eq!(area.account.as_deref(), Some("admin"));
}

/// A page on an origin the recipe allows besides its own start page is the
/// application's, and is recorded.
#[tokio::test]
async fn a_page_on_an_extra_allowed_origin_is_recorded() {
    let dir = TempDir::new();
    let mut with_files = recipe();
    with_files.allowed_origins = vec!["https://files.example.internal".to_string()];
    save_recipe(dir.path(), ORG, PROJECT, &with_files).unwrap();
    let (mut held, _) = slot(buttons_page("https://files.example.internal/docs/list", &["Upload"]), exploring("Leave"));
    let (status, text) =
        page_read_in(held.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.ends_with("Recorded 1 element as seen on Leave."), "{text}");
    assert_eq!(sighted(dir.path(), "Leave"), ["Upload"]);
}

/// A browser on about:blank, or on its own error page, is not on the
/// application, whether or not a recipe limits the origins: nothing is
/// filed, and the area is not stamped.
#[tokio::test]
async fn a_blank_or_error_page_records_nothing_with_or_without_a_recipe() {
    for with_recipe in [true, false] {
        for href in ["about:blank", "chrome-error://chromewebdata/"] {
            let dir = if with_recipe { root_with_recipe_and_account() } else { TempDir::new() };
            let (mut held, _) = slot(buttons_page(href, &["Reload"]), exploring("Leave"));
            let (status, text) =
                page_read_in(held.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
            assert_eq!(status, 200, "{text}");
            assert!(text.ends_with(&format!("\n\n{READ_OFF_THE_APP}")), "{href}, recipe {with_recipe}: {text}");
            let map = load_map(dir.path(), ORG, PROJECT).unwrap();
            assert!(map.areas.is_empty(), "{href}, recipe {with_recipe}: {map:?}");
        }
    }
}

/// A bare `return_to_area` in a discovery goes back to the discovery's own
/// area: a discovery is no case, so there is no script to name one. With
/// no area named yet it says how to name one, never "case 0".
#[tokio::test]
async fn a_bare_return_to_area_in_a_discovery_goes_to_its_own_area() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, leave_apply(MadeBy::Person, vec![css("#apply")], "/hr/leave/apply")).unwrap();
    let back = Action::ReturnToArea { area: None };

    let (mut browser, _) = slot(menu_app(true, "/hr/leave/balance"), exploring("Leave Apply"));
    browser.as_mut().unwrap().account = Some("admin".into());
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &back, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert!(!body.contains("case 0"), "{body}");
    assert_eq!(v["ok"], true, "{body}");
    assert_eq!(v["path"], "/hr/leave/apply", "{body}");

    let (mut browser, _) = slot(menu_app(true, "/hr/leave/balance"), opened_for_discovery());
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &back, None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], false, "{body}");
    assert!(v["detail"].as_str().unwrap().contains(v2_lib::autorun::runner::NEEDS_DISCOVERY_AREA), "{body}");
}

/// The answer names the area as the map files it: the recorded area's own
/// spelling, not the one the discovery was given.
#[tokio::test]
async fn the_answer_names_the_area_as_the_map_files_it() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, leave_apply(MadeBy::Person, vec![css("#leave")], "/hr/leave/apply")).unwrap();
    let (mut held, _) = slot(buttons_page(LEAVE_PAGE, &["Save"]), exploring("leave   apply"));
    let (_, text) = page_read_in(held.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert!(text.ends_with("Recorded 1 element as seen on Leave Apply."), "{text}");
    assert_eq!(sighted(dir.path(), "Leave Apply"), ["Save"]);
}

// ------------------------- every refused locator at once, and a dry run

use crate::common::every_file;

/// "Three pages": one click on each of three pager buttons, none seen.
fn three_pages() -> Component {
    serde_json::from_value(json!({
        "name": "Three pages",
        "description": "Opens three pages of cycles in turn",
        "inputs": [],
        "actions": [
            { "kind": "click", "selector": { "css": "#pager-1" } },
            { "kind": "click", "selector": { "css": "#pager-2" } },
            { "kind": "click", "selector": { "css": "#pager-3" } }
        ]
    }))
    .unwrap()
}

/// During a discovery, the page check runs over every refused locator,
/// not only the first: each one there is recorded, the save is checked
/// once more, and the answer names them all. When none is there, the
/// refusal that follows still lists every one. A script save's page check
/// takes the whole list the same way.
#[tokio::test]
async fn record_on_page_runs_over_every_refused_locator() {
    let c = three_pages();
    let refused = |i: usize| {
        format!("Action {i}: #pager-{i} was never seen on the live app; this click needs one seen matching exactly one element. Find it on the page first with probe_autorun_locator or discover_autorun_actions, then save again.")
    };

    let dir = TempDir::new();
    let (mut browser, _) = slot(cycles_page(1, true), tried_in("Cycles", &c));
    let (status, body) =
        save_component_in(&mut browser, dir.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default())).await;
    assert_eq!(status, 200, "{body}");
    let (said, saved) = body.split_once('\n').expect("the recorded line, then the save's answer");
    assert_eq!(said, "Recorded on the current page: #pager-1, #pager-2, #pager-3.");
    assert_eq!(parsed(saved)["version"], 1, "{body}");

    // None of them on the page: the refusal after the line lists all three.
    let fresh = TempDir::new();
    let (mut browser, _) = slot(cycles_page(0, true), tried_in("Cycles", &c));
    let (status, body) =
        save_component_in(&mut browser, fresh.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default())).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body,
        format!(
            "Recorded on the current page: nothing - no refused locator matched a visible element.\n{}\n{}\n{}",
            refused(1),
            refused(2),
            refused(3)
        )
    );

    // A script save hands over every refused locator, and every one there
    // is recorded under the discovery's area, so the check then passes.
    use v2_lib::ai_bridge::record_refused_for_scripts_in;
    let script: v2_lib::autorun::CaseScript = serde_json::from_value(json!({
        "case_id": 7, "title": "T", "area": "Cycles",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#pager-1" }] },
            { "step_number": 2, "actions": [
                { "kind": "click", "selector": "#pager-2" },
                { "kind": "click", "selector": "#pager-3" }
            ] }
        ]
    }))
    .unwrap();
    let none = v2_lib::autorun::components::ComponentFile::default();
    let scripts_dir = TempDir::new();
    let map = load_map(scripts_dir.path(), ORG, PROJECT).unwrap();
    let targets = v2_lib::autorun::seen_check::unseen_targets(&map, &none, &script, &[], None, &[]).unwrap();
    assert_eq!(targets.len(), 3);
    let (mut browser, _) = slot(cycles_page(1, true), exploring("Cycles"));
    let probed =
        record_refused_for_scripts_in(&mut browser, scripts_dir.path(), ORG, PROJECT, &[Some("Cycles")], &targets).await;
    assert_eq!(probed, Some(vec!["#pager-1".to_string(), "#pager-2".to_string(), "#pager-3".to_string()]));
    let map = load_map(scripts_dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(v2_lib::autorun::seen_check::check_seen(&map, &none, &script, &[], None), Ok(()));
}

/// A component's dry run answers what the save would, refused or not, and
/// writes nothing, probes nothing and records nothing: no component, no
/// map, every file the same byte for byte. It is not a try either.
#[tokio::test]
async fn a_component_dry_run_writes_and_records_nothing() {
    use v2_lib::ai_bridge::dry_run_component_in;
    use v2_lib::autorun::components::{components_path, TRY_IT_FIRST};
    let dir = TempDir::new();
    let c = next_page();

    // Refused: the save's own refusal, with no page check before it.
    let (mut browser, _) = slot(untouched_page(), tried_in("Cycles", &c));
    let before = every_file(dir.path());
    let (status, body) =
        dry_run_component_in(&mut browser, dir.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default()));
    assert_eq!(
        (status, body.as_str()),
        (400, "Action 1: #pager-2 was never seen on the live app; this click needs one seen matching exactly one element. Find it on the page first with probe_autorun_locator or discover_autorun_actions, then save again.")
    );
    assert_eq!(every_file(dir.path()), before);
    assert!(load_map(dir.path(), ORG, PROJECT).unwrap().areas.is_empty());

    // Seen and tried: it would save as version 1, and nothing is written.
    v2_lib::autorun::discovery_map::record_matched(dir.path(), ORG, PROJECT, Some("Cycles"), "/hr/cycles", &Target::from("#pager-2"), v2_lib::autorun::sessions::now_ms())
        .unwrap();
    let seen = every_file(dir.path());
    let (status, body) =
        dry_run_component_in(&mut browser, dir.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default()));
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body), json!({ "would_save": "Next page", "version": 1, "changes": 0 }));
    assert_eq!(every_file(dir.path()), seen, "a dry run changed a file");
    assert!(!components_path(dir.path(), ORG, PROJECT).exists());
    assert!(browser.as_ref().unwrap().d.calls.is_empty(), "a dry run reached the page");

    // Not tried in this discovery: refused as a save is, and a dry run
    // does not make it a try.
    let (mut browser, _) = slot(untouched_page(), exploring("Cycles"));
    let (status, body) =
        dry_run_component_in(&mut browser, dir.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default()));
    assert_eq!((status, body.as_str()), (409, TRY_IT_FIRST));
    let (status, body) =
        save_component_in(&mut browser, dir.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default())).await;
    assert_eq!((status, body.as_str()), (409, TRY_IT_FIRST));
    assert_eq!(every_file(dir.path()), seen);
}

/// The component save route reads `dry_run` as true or false only, and a
/// dry run with no discovery going gives the save's own refusal, writing
/// nothing, as the save itself does.
#[tokio::test]
async fn the_component_save_route_takes_a_dry_run() {
    let dir = TempDir::new();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let with = |dry_run: Value| {
        let mut body = serde_json::to_value(next_page()).unwrap();
        body["dry_run"] = dry_run;
        body.to_string()
    };
    let before = every_file(dir.path());

    let (status, out) = route(&ctx(), None, "POST", "/autorun-component-save", &with(json!("yes")), "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, "\"dry_run\" is true or false."));

    let (dry_status, dry) = route(&ctx(), None, "POST", "/autorun-component-save", &with(json!(true)), "1.0.0").await;
    assert_ne!(dry_status, 200, "{dry}");
    assert_eq!(every_file(dir.path()), before, "a dry run changed a file");
    let (status, saved) = route(&ctx(), None, "POST", "/autorun-component-save", &with(json!(false)), "1.0.0").await;
    assert_eq!((dry_status, dry.as_str()), (status, saved.as_str()), "a dry run answered unlike the save");
    assert_eq!(every_file(dir.path()), before, "a refused save changed a file");
}

/// The page check a refused save makes files nothing from a page off the
/// application's origins: the answer says nothing was recorded, the map
/// stays empty, and the save's own refusal follows.
#[tokio::test]
async fn record_on_page_off_the_app_records_nothing() {
    let dir = root_with_recipe_and_account();
    let c = next_page();
    let (mut browser, _) = slot(cycles_page_at("https://elsewhere.example/sso?ticket=abc", 1, true), tried_in("Cycles", &c));
    let (status, body) =
        save_component_in(&mut browser, dir.path(), ORG, PROJECT, c.clone(), None, 5, Some(&UserCases::default())).await;
    assert_eq!(status, 400, "{body}");
    assert!(
        body.starts_with(
            "Recorded on the current page: nothing - no refused locator matched a visible element.\nAction 1: #pager-2 was never seen on the live app"
        ),
        "{body}"
    );
    assert!(!body.contains("elsewhere.example") && !body.contains("ticket"), "{body}");
    assert!(load_map(dir.path(), ORG, PROJECT).unwrap().areas.is_empty(), "an off-site page was recorded");
}

/// A page at `/hr/leave` holding a button for each of `names` and an iframe
/// whose contents cannot be read: the page says it cannot reach into it
/// when `cross_origin` (another site), else the frame cannot even be asked
/// (still loading, say).
fn framed_page(names: &[&str], cross_origin: bool) -> ScriptedDriver {
    let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    ScriptedDriver::new(move |method, params| {
        Ok(match method {
            "DOM.resolveNode" if cross_origin => json!({ "object": { "objectId": "help-frame" } }),
            "Runtime.callFunctionOn" if cross_origin && params["functionDeclaration"] == v2_lib::browser::locator::FRAME_JS => {
                json!({ "result": { "value": "unreachable" } })
            }
            "Accessibility.getFullAXTree" => {
                let mut tree = buttons_tree(&names);
                let nodes = tree["nodes"].as_array_mut().unwrap();
                nodes[0]["childIds"].as_array_mut().unwrap().push(json!("help"));
                nodes.push(json!({
                    "nodeId": "help", "ignored": false, "role": { "value": "Iframe" },
                    "name": { "value": "Help" }, "backendDOMNodeId": 77, "childIds": []
                }));
                tree
            }
            "Runtime.evaluate" if params["expression"] == "location.href" => json!({ "result": { "value": LEAVE_PAGE } }),
            "Runtime.evaluate" if params["expression"] == "document.title" => json!({ "result": { "value": "Leave" } }),
            _ => json!({}),
        })
    })
}

/// A read with a frame it could not read did not see the whole page: like a
/// read cut at its limit, it files what it showed, keeps what the area saw
/// on the page before, and does not stamp the area explored.
#[tokio::test]
async fn a_read_with_an_unreadable_frame_drops_nothing() {
    let dir = root_with_recipe_and_account();
    let (mut first, _) = slot(buttons_page(LEAVE_PAGE, &["Earlier", "Older"]), exploring("Leave"));
    page_read_in(first.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, 1).await;
    assert_eq!(sighted(dir.path(), "Leave"), ["Earlier"]);
    assert_eq!(mapped_area(dir.path(), "Leave").unwrap().explored_at, None);

    let (mut framed, _) = slot(framed_page(&["Save"], false), exploring_anew("Leave"));
    let (status, text) =
        page_read_in(framed.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("frame contents could not be read"), "{text}");
    assert!(!text.contains("... and"), "the read was cut by its limit: {text}");
    assert_eq!(sighted(dir.path(), "Leave"), ["Earlier", "Help", "Save"], "a read missing a frame dropped a sighting");
    assert_eq!(mapped_area(dir.path(), "Leave").unwrap().explored_at, None, "a read missing a frame stamped the area");

    // The same page with every frame read is whole, and does drop the
    // page's elements it does not show; the sightings stay.
    let (mut whole, _) = slot(buttons_page(LEAVE_PAGE, &["Save"]), exploring_anew("Leave"));
    page_read_in(whole.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(on_leave_page(dir.path()), ["Save"]);
    assert_eq!(sighted(dir.path(), "Leave"), ["Earlier", "Help", "Save"]);
    assert!(mapped_area(dir.path(), "Leave").unwrap().explored_at.is_some());
}

/// A frame from another site (a badge, a chat widget) holds nothing Auto
/// Run could ever reach, so a read with one is still the whole page: it
/// drops what it does not show and stamps the area explored, with the
/// frame's note line still printed.
#[tokio::test]
async fn a_page_whose_only_frame_is_from_another_site_is_explored() {
    let dir = root_with_recipe_and_account();
    let (mut first, _) = slot(buttons_page(LEAVE_PAGE, &["Earlier", "Older"]), exploring("Leave"));
    page_read_in(first.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, 1).await;
    assert_eq!(mapped_area(dir.path(), "Leave").unwrap().explored_at, None);

    let (mut framed, _) = slot(framed_page(&["Save"], true), exploring_anew("Leave"));
    let (status, text) =
        page_read_in(framed.as_mut().unwrap(), Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("frame contents could not be read"), "{text}");
    assert_eq!(on_leave_page(dir.path()), ["Help", "Save"], "a whole read kept what it did not show");
    assert_eq!(sighted(dir.path(), "Leave"), ["Earlier", "Help", "Save"], "a whole read dropped a sighting");
    assert!(mapped_area(dir.path(), "Leave").unwrap().explored_at.is_some(), "the area was not stamped explored");
}

// ------------------------------------------------- several actions at once

use v2_lib::ai_bridge::{discover_actions_in, BATCH_EMPTY, BATCH_TOO_LONG, MAX_BATCH};

/// A page on which every locator finds one ready element, and whose address
/// and one button move on with each click: `/hr/leave/start` showing
/// "Start" before any, `/hr/leave/first` showing "First" after the first,
/// and `/hr/leave/second` showing "Second" after that. Its address carries
/// a query string, which nothing may keep.
fn stepping_page() -> ScriptedDriver {
    ScriptedDriver::new(stepping_answers()).with_net_record()
}

/// What `stepping_page` answers, for a page that wraps it.
fn stepping_answers(
) -> impl FnMut(&str, &Value) -> Result<Value, v2_lib::browser::cdp::CdpError> + Send + 'static {
    let page = FakePage::default();
    let clicks = std::sync::atomic::AtomicUsize::new(0);
    move |method: &str, params: &Value| {
        if method == "Input.dispatchMouseEvent" && params["type"] == "mouseReleased" {
            clicks.fetch_add(1, Ordering::SeqCst);
        }
        let (path, name) = match clicks.load(Ordering::SeqCst) {
            0 => ("start", "Start"),
            1 => ("first", "First"),
            _ => ("second", "Second"),
        };
        match method {
            "Accessibility.getFullAXTree" => Ok(buttons_tree(&[name.to_string()])),
            "Runtime.evaluate" if params["expression"] == "location.href" => Ok(json!({ "result": {
                "value": format!("https://hr.example.internal/hr/leave/{path}?token=t0p-secret")
            } })),
            _ => page.answer(method, params),
        }
    }
}

fn click(css: &str) -> Action {
    Action::Click { selector: css.into() }
}

/// An action that fails at once: there is no such area to return to.
fn nowhere() -> Action {
    serde_json::from_value(json!({ "kind": "return_to_area", "area": "Nowhere At All" })).unwrap()
}

/// The page as `get_autorun_page` prints it with one button, `name`.
async fn page_with(name: &str) -> String {
    read_page(&mut buttons_page(LEAVE_PAGE, &[name]), DEFAULT_LIMIT, None).await.1
}

/// The lines of a batch's answer before its page.
fn batch_lines(text: &str) -> Vec<&str> {
    text.split("\n\n").next().unwrap().lines().collect()
}

/// `page: false` leaves the batch's page out of its text answer, saying
/// where to read it: the page is still read after the last action, and
/// what it shows recorded as seen.
#[tokio::test]
async fn a_batch_asked_for_no_page_leaves_it_out() {
    use v2_lib::ai_bridge::{discover_actions_paged, PAGE_LEFT_OUT};
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let (status, text) = discover_actions_paged(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        &[click("#a"), click("#b")],
        None,
        None,
        true,
        &no_stop(),
        false,
    )
    .await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert_eq!(lines.len(), 2, "{text}");
    assert!(text.ends_with(&format!("\n\nPage: {PAGE_LEFT_OUT}")), "{text}");
    assert!(!text.contains("\"Second\""), "the page came back: {text}");
    let b = browser.as_ref().unwrap();
    assert_eq!(b.d.calls_to("Accessibility.getFullAXTree").len(), 3, "the page was not read");
    assert!(sighted(dir.path(), "Leave").contains(&"Second".to_string()), "what the page showed was not recorded");
}

/// The actions run in order, each answered by one line, and the page is
/// answered once, from where the last action left it, ending as a page read
/// during a discovery does. No host or query string comes back.
#[tokio::test]
async fn a_batch_runs_in_order_and_answers_the_page_once() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let (status, text) =
        discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &[click("#a"), click("#b")], None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");

    let lines = batch_lines(&text);
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].starts_with("1. ok: ") && lines[0].contains("/hr/leave/first"), "{text}");
    assert!(lines[1].starts_with("2. ok: ") && lines[1].contains("/hr/leave/second"), "{text}");

    let last = page_with("Second").await;
    assert!(text.ends_with(&format!("{last}\n\nRecorded 1 element as seen on Leave.")), "{text}");
    assert_eq!(text.matches(&last).count(), 1, "the page came back more than once: {text}");
    assert!(!text.contains("\"First\""), "an action's own page came back: {text}");
    assert!(!text.contains("hr.example.internal") && !text.contains("t0p-secret"), "{text}");

    // Each action read its page, and the answer read the last one again.
    let b = browser.as_ref().unwrap();
    assert_eq!(b.d.calls_to("Accessibility.getFullAXTree").len(), 3);
}

/// By default the batch stops at the first action that fails: the line
/// says why, the actions after it are not run and a line says how many,
/// and the page is where the browser stopped.
#[tokio::test]
async fn a_batch_stops_on_the_first_failure() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let actions = [click("#a"), nowhere(), click("#b"), click("#c")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");

    let lines = batch_lines(&text);
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].starts_with("1. ok: "), "{text}");
    assert!(lines[1].starts_with("2. failed: ") && lines[1].contains("Nowhere At All"), "{text}");
    assert_eq!(lines[2], "Stopped at the failure: 2 actions were not run.");
    assert!(text.ends_with(&format!("{}\n\nRecorded 1 element as seen on Leave.", page_with("First").await)), "{text}");
    let b = browser.as_ref().unwrap();
    let released = b.d.calls_to("Input.dispatchMouseEvent").iter().filter(|p| p["type"] == "mouseReleased").count();
    assert_eq!(released, 1, "an action after the failure ran");
}

/// With `stop_on_failure` false, a failed action is answered and the rest
/// still run.
#[tokio::test]
async fn a_batch_can_carry_on_past_a_failure() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let actions = [click("#a"), nowhere(), click("#b")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, false, &no_stop()).await;
    assert_eq!(status, 200, "{text}");

    let lines = batch_lines(&text);
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].starts_with("1. ok: "), "{text}");
    assert!(lines[1].starts_with("2. failed: "), "{text}");
    assert!(lines[2].starts_with("3. ok: ") && lines[2].contains("/hr/leave/second"), "{text}");
    assert!(!text.contains("not run"), "{text}");
    assert!(text.ends_with(&format!("{}\n\nRecorded 1 element as seen on Leave.", page_with("Second").await)), "{text}");
}

/// More than 20 actions is refused before anything else is looked at, by
/// the route and by the batch itself.
#[tokio::test]
async fn a_batch_of_more_than_twenty_is_refused() {
    let _g = crate::serial::autorun();
    let one = json!({ "kind": "click", "selector": "#a" });
    let body = json!({ "actions": vec![one.clone(); 21] }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, BATCH_TOO_LONG));
    assert_eq!(BATCH_TOO_LONG, "at most 20 actions in one call");
    assert_eq!(BATCH_TOO_LONG, format!("at most {MAX_BATCH} actions in one call"), "the cap and its sentence differ");

    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let many: Vec<Action> = (0..21).map(|_| click("#a")).collect();
    let (status, out) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &many, None, None, true, &no_stop()).await;
    assert_eq!((status, out.as_str()), (400, BATCH_TOO_LONG));
    assert!(browser.as_ref().unwrap().d.calls.is_empty(), "the browser was touched");
}

/// An empty batch is refused, and so is one whose actions are not a list.
#[tokio::test]
async fn an_empty_batch_is_refused() {
    let _g = crate::serial::autorun();
    let body = json!({ "actions": [] }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, BATCH_EMPTY));
    let body = json!({ "actions": { "kind": "click", "selector": "#a" } }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("list of actions"), "{out}");

    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let (status, out) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &[], None, None, true, &no_stop()).await;
    assert_eq!((status, out.as_str()), (400, BATCH_EMPTY));
}

/// Each action is checked as a single action is, before the browser is
/// touched: one refused action refuses the batch, naming which. A
/// `stop_on_failure` that is not true or false is refused, and so is a
/// draft no action uses.
#[tokio::test]
async fn a_batch_with_a_refused_action_runs_none() {
    let _g = crate::serial::autorun();
    let body = json!({ "actions": [
        { "kind": "click", "selector": "#a" },
        { "kind": "sign_in", "account": "admin" }
    ] })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, "action 2: sign_in is not a thing an assistant does - the person signs in");

    let body = json!({ "actions": [{ "kind": "click", "selector": "#a" }], "stop_on_failure": "yes" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("stop_on_failure"), "{out}");

    let draft = serde_json::to_value(pick_a_date()).unwrap();
    let body = json!({ "actions": [{ "kind": "click", "selector": "#a" }], "draft": draft }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("use_component"), "{out}");
}

/// A `use_component` is expanded before the browser is touched too: a
/// component that is not saved (and sent with no draft) refuses the whole
/// batch, naming its action, so the click before it never runs. The same
/// batch without it gets past every check, to the browser that is not
/// there.
#[tokio::test]
async fn a_batch_with_an_unsaved_component_runs_none() {
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let click = json!({ "kind": "click", "selector": "#a" });
    let unsaved = serde_json::to_value(pick_a_date_use()).unwrap();
    let before = every_file(dir.path());

    let body = json!({ "actions": [click.clone(), unsaved.clone()] }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    let want = format!("action 2: {}", v2_lib::ai_bridge::not_saved_try_draft("pick a  DATE"));
    assert_eq!((status, out.as_str()), (400, want.as_str()));
    assert!(!out.contains("Secret-Day-17"), "a typed input came back: {out}");
    assert_eq!(every_file(dir.path()), before, "a refused batch changed a file");

    // A draft of another component does not stand in for it.
    let mut other = pick_a_date();
    other.name = "Another one".into();
    let another = json!({ "kind": "use_component", "component": "Another one",
                          "inputs": { "field": { "css": "#day" }, "day": "1" } });
    let body = json!({ "actions": [click.clone(), unsaved, another], "draft": serde_json::to_value(other).unwrap() })
        .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, want.as_str()));

    let body = json!({ "actions": [click] }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (409, NO_DISCOVERY), "the batch did not reach the browser stage");
}

/// Outside a discovery the batch is refused as a single action is.
#[tokio::test]
async fn a_batch_needs_a_discovery() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), None);
    let (status, out) =
        discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &[click("#a")], None, None, true, &no_stop()).await;
    assert_eq!((status, out.as_str()), (409, NO_DISCOVERY));
    assert!(browser.as_ref().unwrap().d.calls.is_empty(), "the browser was touched");
}

/// A mapping run's guarded browser on which clicks make the page send
/// saves: three in all, taken as the clicks come.
async fn saving_mapping_browser() -> Option<FakeBrowser> {
    use v2_lib::browser::cdp::Driver;
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list?page=2");
    d.guard_saves(&[]).await.unwrap();
    for (method, url) in [
        ("POST", "https://hr.example.internal/hr/leave/save?id=5&token=t0p-secret"),
        ("DELETE", "https://hr.example.internal/hr/leave/delete/7"),
        ("PUT", "https://hr.example.internal/hr/leave/update"),
    ] {
        d.saves_on_call.push(("Input.dispatchMouseEvent".into(), method.into(), url.into()));
    }
    slot(d, mapping(exploring("Leave"), &["Leave"])).0
}

/// In a mapping run every save the batch's actions set off is blocked and
/// counted exactly as the same actions sent one at a time: the run's count
/// is the same, no action fails for it, and the answer says the total.
#[tokio::test]
async fn a_batch_blocks_saves_like_single_actions() {
    let dir = root_with_recipe_and_account();
    let save = click("#save");

    let mut one_by_one = saving_mapping_browser().await;
    let mut singles = 0;
    for _ in 0..2 {
        let (status, body) = discover_action_in(&mut one_by_one, dir.path(), ORG, PROJECT, &save, None, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(parsed(&body)["ok"], true, "{body}");
        singles += parsed(&body)["blocked"].as_u64().unwrap();
    }
    let single_run = the_run(&one_by_one).blocked_writes;
    assert!(single_run > 0, "nothing was blocked");
    assert_eq!(u64::from(single_run), singles);

    let mut batch = saving_mapping_browser().await;
    let (status, text) =
        discover_actions_in(&mut batch, dir.path(), ORG, PROJECT, &[save.clone(), save.clone()], None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert!(lines[0].starts_with("1. ok: ") && lines[1].starts_with("2. ok: "), "a blocked save failed an action: {text}");
    assert_eq!(the_run(&batch).blocked_writes, single_run, "the batch counted differently: {text}");
    assert_eq!(lines[2], format!("Saves blocked by the mapping run: {singles}."), "{text}");
    assert!(!text.contains("t0p-secret") && !text.contains("hr.example.internal"), "{text}");
}

/// Each action's own page read records what it showed, as a single action's
/// does: a page only the first action saw is recorded, not just the last.
#[tokio::test]
async fn a_batch_records_what_each_action_read() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let (status, text) =
        discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &[click("#a"), click("#b")], None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");

    assert_eq!(sighted(dir.path(), "Leave"), ["First", "Second"]);
    let area = mapped_area(dir.path(), "Leave").unwrap();
    let pages: Vec<&str> = area.pages.iter().map(|p| p.path.as_str()).collect();
    assert!(pages.contains(&"/hr/leave/first") && pages.contains(&"/hr/leave/second"), "{pages:?}");
    let file = std::fs::read_to_string(map_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(!file.contains("t0p-secret") && !file.contains('?'), "{file}");
}

/// An action the browser never gets to (a component not saved, with no
/// draft) is a failed line like any other, not the end of the batch. The
/// route refuses such a batch before running any of it
/// (`a_batch_with_an_unsaved_component_runs_none`); this is the batch's own
/// defence should one reach it.
#[tokio::test]
async fn a_refused_action_in_a_batch_is_one_failed_line() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let actions = [pick_a_date_use(), click("#a")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, false, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert_eq!(lines[0], format!("1. failed: {}", v2_lib::ai_bridge::not_saved_try_draft("pick a DATE")), "{text}");
    assert!(lines[1].starts_with("2. ok: "), "{text}");
    assert!(text.ends_with(&format!("{}\n\nRecorded 1 element as seen on Leave.", page_with("First").await)), "{text}");
}

// --------------------------------- a batch's area, a gone browser, a stop

use v2_lib::ai_bridge::{ended_line, BROWSER_GONE};

/// No stop asked for: what a batch gets in place of the route's flag.
fn no_stop() -> AtomicBool {
    AtomicBool::new(false)
}

/// `area` moves the discovery before the first action, even when that
/// action is refused before the browser is touched (a component not saved):
/// what the batch then reads is filed under the named area, not the one the
/// discovery was in.
#[tokio::test]
async fn a_batch_moves_to_its_area_before_its_first_action() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let (status, text) = discover_actions_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        &[pick_a_date_use(), click("#a")],
        None,
        Some("Payroll"),
        true,
        &no_stop(),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert!(batch_lines(&text)[0].starts_with("1. failed: "), "{text}");
    assert!(text.ends_with("\n\nRecorded 1 element as seen on Payroll."), "{text}");
    assert_eq!(browser.as_ref().unwrap().discovery.as_ref().unwrap().area.as_deref(), Some("Payroll"));
    assert_eq!(sighted(dir.path(), "Payroll"), ["Start"]);
    assert!(sighted(dir.path(), "Leave").is_empty(), "the batch filed under the area it left");
}

/// A page that works as `stepping_page` until its second click, when the
/// browser goes: that call and every one after it is `Closed`, and `gone`
/// is set, as the browser's own liveness check would find it.
fn dying_page(gone: Arc<AtomicBool>) -> ScriptedDriver {
    let mut answer = stepping_answers();
    let clicks = std::sync::atomic::AtomicUsize::new(0);
    ScriptedDriver::new(move |method, params| {
        if gone.load(Ordering::SeqCst) {
            return Err(v2_lib::browser::cdp::CdpError::Closed);
        }
        if method == "Input.dispatchMouseEvent" && params["type"] == "mousePressed" {
            if clicks.fetch_add(1, Ordering::SeqCst) == 1 {
                gone.store(true, Ordering::SeqCst);
                return Err(v2_lib::browser::cdp::CdpError::Closed);
            }
        }
        answer(method, params)
    })
    .with_net_record()
}

/// A browser that goes partway through a batch is let go right there, and
/// the answer keeps the lines of the actions that did run, ending with the
/// sentence a single call would have answered.
#[tokio::test]
async fn a_browser_gone_mid_batch_keeps_the_lines_that_ran() {
    let dir = root_with_recipe_and_account();
    let gone = Arc::new(AtomicBool::new(false));
    let (mut browser, closed) = slot(dying_page(gone.clone()), exploring("Leave"));
    browser.as_mut().unwrap().gone = gone.clone();
    let actions = [click("#a"), click("#b"), click("#c")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, false, &no_stop()).await;
    assert_eq!(status, 409, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("1. ok: "), "the line of the action that ran was lost: {text}");
    assert!(lines.iter().any(|l| l.starts_with("2. failed: ")), "{text}");
    assert_eq!(*lines.last().unwrap(), BROWSER_GONE, "{text}");
    assert!(browser.is_none(), "the gone browser is still held");
    assert!(closed.load(Ordering::SeqCst), "it was not let go through its normal close");
}

/// A stop asked for while a batch runs (End discovery, Close browser or a
/// release, before they wait for the browser) is heard before the next
/// action: the batch answers what ran, says how many were not run, reads
/// nothing more, and the stop is used up.
#[tokio::test]
async fn a_batch_gives_way_to_a_stop_before_its_next_action() {
    let dir = root_with_recipe_and_account();
    let stop = Arc::new(AtomicBool::new(false));
    let pressed = stop.clone();
    let mut inner = stepping_answers();
    let d = ScriptedDriver::new(move |method, params| {
        if method == "Input.dispatchMouseEvent" && params["type"] == "mouseReleased" {
            pressed.store(true, Ordering::SeqCst);
        }
        inner(method, params)
    })
    .with_net_record();
    let (mut browser, _) = slot(d, exploring("Leave"));
    let actions = [click("#a"), click("#b"), click("#c")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &stop).await;
    assert_eq!(status, 409, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].starts_with("1. ok: "), "{text}");
    assert_eq!(lines[1], ended_line(2));
    assert_eq!(ended_line(2), "Stopped: 2 not run - End discovery, Close browser, a release or a reset asked the batch to stop.");
    assert!(!stop.load(Ordering::SeqCst), "the stop was not used up");
    let b = browser.as_ref().unwrap();
    assert_eq!(b.d.calls_to("Accessibility.getFullAXTree").len(), 1, "the batch read on after the stop");
}

/// End discovery is not kept waiting behind a long batch: it asks the batch
/// to stop and then waits for the browser, as `end_discovery` does, and gets
/// it after the action under way rather than after all twenty.
#[tokio::test(flavor = "multi_thread")]
async fn end_discovery_is_not_stuck_behind_a_long_batch() {
    let dir = root_with_recipe_and_account();
    let stop = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let seen = started.clone();
    let mut inner = stepping_answers();
    let d = ScriptedDriver::new(move |method, params| {
        if method == "Input.dispatchMouseEvent" && params["type"] == "mouseReleased" {
            seen.store(true, Ordering::SeqCst);
            // Each click takes a while: twenty of them far outlast the wait below.
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
        inner(method, params)
    })
    .with_net_record();
    let session = Arc::new(tokio::sync::Mutex::new(slot(d, exploring("Leave")).0));

    let (held, flag, root) = (session.clone(), stop.clone(), dir.path().to_path_buf());
    let batch = tokio::spawn(async move {
        let mut slot = held.lock().await;
        let many: Vec<Action> = (0..20).map(|_| click("#a")).collect();
        discover_actions_in(&mut slot, &root, ORG, PROJECT, &many, None, None, true, &flag).await
    });
    while !started.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    // The End discovery order: ask the batch to stop, then wait.
    stop.store(true, Ordering::SeqCst);
    let mut slot = tokio::time::timeout(std::time::Duration::from_secs(4), session.lock())
        .await
        .expect("End discovery waited behind the whole batch");
    let (status, ended) = end_discovery_in(&mut slot).await;
    assert_eq!(status, 200, "{ended}");
    assert!(slot.as_ref().is_some_and(|b| b.discovery.is_none()), "the discovery is still going");
    drop(slot);
    let (status, text) = batch.await.unwrap();
    assert_eq!(status, 409, "{text}");
    assert!(text.ends_with("not run - End discovery, Close browser, a release or a reset asked the batch to stop."), "{text}");

    // Each way the person or the assistant ends the discovery asks a batch
    // to give way before it waits for the browser.
    let source = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    for (start, stops) in [
        ("pub(crate) async fn end_discovery(", "crate::ai_bridge::stop_batch_in(n);"),
        ("pub async fn auto_run_close_browser()", "crate::ai_bridge::stop_batch();"),
        ("pub async fn release_autorun_browsers()", "crate::ai_bridge::stop_all_batches();"),
        ("pub async fn release_for_assistant(", "crate::ai_bridge::stop_batch_in(n);"),
        ("pub async fn reset_for_assistant(", "crate::ai_bridge::stop_batch_in(n);"),
        ("pub async fn close_autorun_browsers()", "crate::ai_bridge::stop_all_batches();"),
    ] {
        let body = &source[source.find(start).unwrap_or_else(|| panic!("{start} is gone"))..];
        let lock = ["SESSION.lock()", "session_slot(n).lock()"].iter().filter_map(|l| body.find(l)).min().unwrap();
        let body = &body[..lock];
        assert!(body.contains(stops), "{start} does not stop a batch before it waits");
    }
    // And the route clears an earlier stop before it takes the browser.
    let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
    let route = &bridge[bridge.find("async fn autorun_discover_actions(").unwrap()..];
    let route = &route[..route.find("session_slot(n).lock()").unwrap()];
    assert!(route.contains("batch_stop(n).store(false"), "an old stop would end the next batch");
}

/// A draft is tried only by the `use_component` that names it: another
/// component in the same batch runs as saved.
#[tokio::test]
async fn a_draft_leaves_another_component_to_run_as_saved() {
    let dir = root_with_recipe_and_account();
    put(dir.path(), ORG, PROJECT, pick_a_date()).unwrap();
    let (mut browser, _) = slot(stepping_page(), exploring("Leave"));
    let mut other = pick_a_date();
    other.name = "Pick a time".into();
    let (status, text) = discover_actions_in(
        &mut browser,
        dir.path(),
        ORG,
        PROJECT,
        &[pick_a_date_use()],
        Some(&other),
        None,
        true,
        &no_stop(),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert!(batch_lines(&text)[0].starts_with("1. ok: "), "{text}");
    let state = browser.as_ref().unwrap().discovery.as_ref().unwrap();
    assert_eq!(state.tried, vec![draft_fingerprint(&pick_a_date())], "the saved one was not what ran");
}

// ------------------------------- what the page said, what the app refused

use v2_lib::ai_bridge::describe_action;

/// What a refused save answers: a validation message and a trace id.
const REFUSED_BODY: &str = r#"{"message":"Weightage must total 100.","traceId":"00-4bf9-00"}"#;

/// The message a wizard step shows when its save is not allowed.
const NO_COMPONENT: &str = "At least one assessment component must be enabled.";

/// The path `telling_page`'s message read gives, and the page a rule
/// remembered there is filed under.
const TELLING_PATH: &str = "/hr/eval/42/rules";
const TELLING_PAGE: &str = "/hr/eval/:id/rules";

/// `stepping_page`, whose messages are `shows(clicks, reads)`: what the
/// page shows once `clicks` clicks were made, at the `reads`-th read of its
/// messages since the last click (from 0). Nothing is found on it once
/// `missing_after` clicks were made, and a request it refuses answers
/// `REFUSED_BODY`.
fn telling_page(shows: impl Fn(usize, usize) -> Vec<&'static str> + Send + 'static, missing_after: usize) -> ScriptedDriver {
    let mut inner = stepping_answers();
    let (mut clicks, mut reads) = (0usize, 0usize);
    ScriptedDriver::new(move |method, params| {
        if method == "Input.dispatchMouseEvent" && params["type"] == "mouseReleased" {
            clicks += 1;
            reads = 0;
        }
        match method {
            "Runtime.evaluate" if params["expression"] == v2_lib::browser::page_messages::READ_JS => {
                let messages: Vec<Value> =
                    shows(clicks, reads).into_iter().map(|t| json!({ "text": t, "where": "page" })).collect();
                reads += 1;
                Ok(json!({ "result": { "value": { "messages": messages, "step": null, "path": TELLING_PATH } } }))
            }
            "Network.getResponseBody" => Ok(json!({ "body": REFUSED_BODY, "base64Encoded": false })),
            "Runtime.getProperties" if clicks >= missing_after => Ok(json!({ "result": [] })),
            _ => inner(method, params),
        }
    })
    .with_net_record()
}

/// The first click sends a save the application refuses; its answer
/// arrives a moment after the click.
fn first_click_is_refused(d: &mut ScriptedDriver) {
    const CLICK: &str = "Input.dispatchMouseEvent";
    let url = "https://hr.example.internal/api/eval-rules?x=1";
    let event = |method: &str, params: Value| Event { method: method.into(), params };
    d.on_call_events.push((
        CLICK.into(),
        event(
            "Network.requestWillBeSent",
            json!({ "requestId": "r1", "type": "XHR", "documentURL": "https://hr.example.internal/hr/leave/start",
                    "request": { "url": url, "method": "POST" } }),
        ),
    ));
    d.on_idle_after_call.push((
        CLICK.into(),
        50,
        event(
            "Network.responseReceived",
            json!({ "requestId": "r1", "response": { "status": 400, "url": url, "mimeType": "application/json" } }),
        ),
    ));
    d.on_idle_after_call.push((CLICK.into(), 50, event("Network.loadingFinished", json!({ "requestId": "r1" }))));
}

fn press_tab() -> Action {
    serde_json::from_value(json!({ "kind": "press_key", "key": "Tab" })).unwrap()
}

fn expect_goals() -> Action {
    serde_json::from_value(json!({ "kind": "expect_visible", "selector": "#goals", "timeout_ms": 100 })).unwrap()
}

/// A success toast is reported on the action that raised it, and never
/// stops the batch.
#[tokio::test]
async fn a_success_toast_does_not_stop_a_batch() {
    let dir = root_with_recipe_and_account();
    let saved = |clicks: usize, _| if clicks >= 1 { vec!["Saved."] } else { vec![] };
    let (mut browser, _) = slot(telling_page(saved, usize::MAX), exploring("Leave"));
    let actions = [click("#a"), click("#b"), click("#c")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert_eq!(lines.len(), 3, "all three ran: {text}");
    assert!(lines.iter().all(|l| l.contains(". ok: ")), "{text}");
    assert!(lines[0].ends_with(r#"; the page showed: "Saved.""#), "{text}");
    assert!(!lines[1].contains("the page showed") && !lines[2].contains("the page showed"), "{text}");

    // A single action's answer carries it as `page_said`.
    let (mut browser, _) = slot(telling_page(saved, usize::MAX), exploring("Leave"));
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &click("#a"), None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    assert_eq!(v["page_said"], json!([{ "text": "Saved.", "where": "page" }]), "{body}");
    assert!(v.get("app_refused").is_none(), "{body}");
}

/// A request the application refuses fails the action that sent it - its
/// answer came after the click - and stops the batch.
#[tokio::test]
async fn a_refused_request_stops_the_batch() {
    let dir = root_with_recipe_and_account();
    let mut d = telling_page(|_, _| vec![], usize::MAX);
    first_click_is_refused(&mut d);
    let (mut browser, _) = slot(d, exploring("Leave"));
    let actions = [press_tab(), click("#save"), click("#b")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].starts_with("1. ok: "), "{text}");
    assert!(
        lines[1].starts_with(
            r#"2. failed: the app refused: "Weightage must total 100." (POST /api/eval-rules, 400, trace id 00-4bf9-00)"#
        ),
        "{text}"
    );
    assert_eq!(lines[2], "Stopped at the failure: 1 action was not run.");
    assert!(!text.contains("x=1") && !text.contains("hr.example.internal"), "{text}");
}

/// A single action's answer names each request the application refused.
#[tokio::test]
async fn a_refused_action_answers_what_the_app_refused() {
    let dir = root_with_recipe_and_account();
    let mut d = telling_page(|_, _| vec![], usize::MAX);
    first_click_is_refused(&mut d);
    let (mut browser, _) = slot(d, exploring("Leave"));
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &click("#save"), None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], false, "{body}");
    assert_eq!(
        v["app_refused"],
        json!([{ "method": "POST", "path": "/api/eval-rules", "status": 400,
                 "message": "Weightage must total 100.", "trace_id": "00-4bf9-00" }]),
        "{body}"
    );
    assert!(v.get("page_said").is_none(), "{body}");
}

/// A check that fails after a save the page answered with a message names
/// that message on its own line - the stop line does not say it again.
#[tokio::test]
async fn a_later_failure_points_back_to_the_message() {
    let dir = root_with_recipe_and_account();
    let shows = |clicks: usize, _| if clicks >= 1 { vec![NO_COMPONENT] } else { vec![] };
    let (mut browser, _) = slot(telling_page(shows, 1), exploring("Leave"));
    let actions = [click("#save"), expect_goals(), click("#b")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].contains(&format!("; the page showed: \"{NO_COMPONENT}\"")), "{text}");
    assert!(lines[1].starts_with("2. failed: "), "{text}");
    let after = format!(" - after {} the page showed: \"{NO_COMPONENT}\"", describe_action(&click("#save")));
    assert!(lines[1].contains(&after), "{text}");
    assert_eq!(lines[2], "Stopped at the failure: 1 action was not run.", "said twice: {text}");
}

/// When the failure's own line cannot name it - a quieter action came
/// between - the stop line points back to the action the page answered.
#[tokio::test]
async fn a_failure_after_a_quiet_action_points_back_on_the_stop_line() {
    let dir = root_with_recipe_and_account();
    let shows = |clicks: usize, _| if clicks >= 1 { vec![NO_COMPONENT] } else { vec![] };
    let (mut browser, _) = slot(telling_page(shows, 2), exploring("Leave"));
    let actions = [click("#save"), click("#next"), expect_goals(), click("#c")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert_eq!(lines.len(), 4, "{text}");
    assert!(lines[2].starts_with("3. failed: ") && !lines[2].contains(" - after "), "{text}");
    assert_eq!(
        lines[3],
        format!(
            "Stopped at the failure: 1 action was not run. After action 1 ({}) the page showed: \"{NO_COMPONENT}\".",
            describe_action(&click("#save"))
        )
    );
}

/// A message the page already showed before the action is nothing new:
/// the answer has no `page_said`, and no `app_refused` without a refusal.
#[tokio::test]
async fn an_action_with_nothing_new_has_no_page_said_key() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(telling_page(|_, _| vec!["Welcome back"], usize::MAX), exploring("Leave"));
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &click("#a"), None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    assert!(v.get("page_said").is_none() && v.get("app_refused").is_none(), "{body}");

    let (mut browser, _) = slot(telling_page(|_, _| vec!["Welcome back"], usize::MAX), exploring("Leave"));
    let (_, text) =
        discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &[click("#a"), click("#b")], None, None, true, &no_stop()).await;
    assert!(!text.contains("the page showed"), "{text}");
}

/// A message the page showed only after the action's own read goes to that
/// action, found by the next action's first read.
#[tokio::test]
async fn a_message_shown_late_goes_to_the_action_before() {
    let dir = root_with_recipe_and_account();
    let late = |clicks: usize, reads: usize| {
        if clicks >= 2 || (clicks == 1 && reads >= 1) {
            vec!["Name is required"]
        } else {
            vec![]
        }
    };
    let (mut browser, _) = slot(telling_page(late, usize::MAX), exploring("Leave"));
    let (status, text) =
        discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &[click("#a"), click("#b")], None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert!(lines[0].ends_with(r#"; the page showed: "Name is required""#), "{text}");
    assert!(!lines[1].contains("the page showed"), "{text}");
}

/// The last action has no next one to read the page for it: the call reads
/// the messages once more before it answers.
#[tokio::test]
async fn the_last_action_is_read_once_more_before_the_answer() {
    let dir = root_with_recipe_and_account();
    let late = |clicks: usize, reads: usize| if clicks >= 1 && reads >= 1 { vec!["Name is required"] } else { vec![] };
    let (mut browser, _) = slot(telling_page(late, usize::MAX), exploring("Leave"));
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &click("#a"), None, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["page_said"], json!([{ "text": "Name is required", "where": "page" }]), "{body}");

    let (mut browser, _) = slot(telling_page(late, usize::MAX), exploring("Leave"));
    let (status, text) =
        discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &[click("#a")], None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    assert!(batch_lines(&text)[0].ends_with(r#"; the page showed: "Name is required""#), "{text}");
    let read_js = v2_lib::browser::page_messages::READ_JS;
    let reads = browser.as_ref().unwrap().d.calls_to("Runtime.evaluate").iter().filter(|p| p["expression"] == read_js).count();
    assert_eq!(reads, 3, "before, after, and once more at the end");
}

/// The page sends a save the application refuses only once the first
/// action's own reads are done - as the discovery reads the page's live
/// notes after it - so the refusal is found after that action ended.
fn refused_after_the_first_action(d: &mut ScriptedDriver) {
    let url = "https://hr.example.internal/api/eval-rules";
    let event = |method: &str, params: Value| Event { method: method.into(), params };
    let events = vec![
        event(
            "Network.requestWillBeSent",
            json!({ "requestId": "late-1", "type": "XHR", "documentURL": "https://hr.example.internal/hr/leave/first",
                    "request": { "url": url, "method": "POST" } }),
        ),
        event(
            "Network.responseReceived",
            json!({ "requestId": "late-1", "response": { "status": 400, "url": url, "mimeType": "application/json" } }),
        ),
        event("Network.loadingFinished", json!({ "requestId": "late-1" })),
    ];
    // The live notes are read once before the action and once after it.
    let mut notes_read = 0;
    let after_the_action = move |method: &str, params: &Value| {
        if method == "Runtime.evaluate" && params["expression"].as_str().is_some_and(|e| e.contains("[role=alert]")) {
            notes_read += 1;
        }
        notes_read == 2
    };
    d.events_when = Some((Box::new(after_the_action), events));
}

/// A refusal found only after an action that worked had ended still stops
/// the batch before the next action, and says which action it was. The
/// action's line stays ok, with the refusal on it.
#[tokio::test]
async fn a_refusal_found_after_an_action_ended_stops_the_batch() {
    let dir = root_with_recipe_and_account();
    let mut d = telling_page(|_, _| vec![], usize::MAX);
    refused_after_the_first_action(&mut d);
    let (mut browser, _) = slot(d, exploring("Leave"));
    let actions = [click("#a"), click("#b"), click("#c")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].starts_with("1. ok: ") && lines[0].contains(r#"; the app refused: "Weightage must total 100.""#), "{text}");
    assert_eq!(
        lines[1],
        format!("Stopped: the app refused action 1 ({}) after it ended: 2 not run.", describe_action(&click("#a")))
    );
    let b = browser.as_ref().unwrap();
    let released = b.d.calls_to("Input.dispatchMouseEvent").iter().filter(|p| p["type"] == "mouseReleased").count();
    assert_eq!(released, 1, "an action after the refusal ran");

    // A single action's answer names it, and stays ok.
    let mut d = telling_page(|_, _| vec![], usize::MAX);
    refused_after_the_first_action(&mut d);
    let (mut browser, _) = slot(d, exploring("Leave"));
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &click("#a"), None, None).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    assert_eq!(v["app_refused"][0]["status"], 400, "{body}");
}

/// A request the application refused before the discovery's actions is
/// never one of theirs, however many actions ran: on the whole browser
/// each action's marks are let go, so the last page change's mark is never
/// pushed out - and one that was would read nothing, not the whole record.
#[tokio::test]
async fn an_old_refusal_never_stops_a_batch_after_many_actions() {
    let dir = root_with_recipe_and_account();
    let mut d = telling_page(|_, _| vec![], usize::MAX).with_mark_ring();
    let url = "https://hr.example.internal/api/eval-rules";
    let event = |method: &str, params: Value| Event { method: method.into(), params };
    for e in [
        event(
            "Network.requestWillBeSent",
            json!({ "requestId": "old-1", "type": "XHR", "documentURL": "https://hr.example.internal/hr/leave/start",
                    "request": { "url": url, "method": "POST" } }),
        ),
        event(
            "Network.responseReceived",
            json!({ "requestId": "old-1", "response": { "status": 400, "url": url, "mimeType": "application/json" } }),
        ),
        event("Network.loadingFinished", json!({ "requestId": "old-1" })),
    ] {
        d.net.as_mut().unwrap().observe(&e);
    }
    let (mut browser, _) = slot(d, exploring("Leave"));
    let look = || -> Action {
        serde_json::from_value(json!({ "kind": "expect_visible", "selector": "#a", "timeout_ms": 100 })).unwrap()
    };
    // Twenty actions one at a time first.
    for _ in 0..20 {
        let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &look(), None, None).await;
        assert_eq!(status, 200, "{body}");
    }
    let mut actions = vec![click("#a")];
    actions.extend((0..18).map(|_| look()));
    actions.push(click("#b"));
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert!(!text.contains("the app refused"), "{text}");
    assert_eq!(lines.len(), 20, "{text}");
    assert!(lines.iter().all(|l| l.contains(". ok")), "{text}");
    let map = load_map(dir.path(), ORG, PROJECT).unwrap();
    assert!(rules_of(&map, "Leave").is_empty(), "{:?}", rules_of(&map, "Leave"));
}

/// What the page showed a failed action that its detail does not say is
/// still on its line.
#[tokio::test]
async fn a_failed_action_says_what_the_page_showed_it() {
    let dir = root_with_recipe_and_account();
    let late = |clicks: usize, reads: usize| if clicks >= 1 && reads >= 1 { vec!["Weightage must total 100."] } else { vec![] };
    let mut d = telling_page(late, usize::MAX);
    first_click_is_refused(&mut d);
    let (mut browser, _) = slot(d, exploring("Leave"));
    let (status, text) =
        discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &[click("#save"), click("#b")], None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert!(lines[0].starts_with("1. failed: the app refused: "), "{text}");
    assert!(lines[0].ends_with(r#"; the page showed: "Weightage must total 100.""#), "{text}");
    assert_eq!(lines[0].matches("the page showed").count(), 1, "{text}");
}

// ------------------------------------------- what the app refused, kept

use v2_lib::autorun::discovery_map::{rules_of, RuleSource};

/// A request the application refused in a discovery is kept as a rule of
/// the discovery's area: the application's words, the page it ran on and
/// the action - never its trace id, status or address.
#[tokio::test]
async fn a_refused_request_in_discovery_is_remembered_without_its_trace_id() {
    let dir = root_with_recipe_and_account();
    let mut d = telling_page(|_, _| vec![], usize::MAX);
    first_click_is_refused(&mut d);
    let (mut browser, _) = slot(d, exploring("Leave"));
    let actions = [press_tab(), click("#save"), click("#b")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let file = std::fs::read_to_string(map_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(file.contains("Weightage must total 100."), "{file}");
    assert!(!file.contains("00-4bf9-00"), "{file}");
    assert!(!file.contains("hr.example.internal") && !file.contains('?'), "{file}");
    let map = load_map(dir.path(), ORG, PROJECT).unwrap();
    let rules = rules_of(&map, "Leave");
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert_eq!(rules[0].page, TELLING_PAGE);
    assert_eq!(rules[0].after, describe_action(&click("#save")));
    assert_eq!((rules[0].source, rules[0].count), (RuleSource::Discovery, 1));

    // The same refusal again is the same rule, seen twice.
    let mut d = telling_page(|_, _| vec![], usize::MAX);
    first_click_is_refused(&mut d);
    let (mut browser, _) = slot(d, exploring("Leave"));
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &click("#save"), None, None).await;
    assert_eq!(status, 200, "{body}");
    let map = load_map(dir.path(), ORG, PROJECT).unwrap();
    let rules = rules_of(&map, "Leave");
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert_eq!(rules[0].count, 2);
}

/// A message on its own is never taken for a rule: a success toast is
/// not kept, whatever came after it.
#[tokio::test]
async fn a_success_toast_alone_is_never_remembered() {
    let dir = root_with_recipe_and_account();
    let saved = |clicks: usize, _| if clicks >= 1 { vec!["Saved."] } else { vec![] };
    let (mut browser, _) = slot(telling_page(saved, usize::MAX), exploring("Leave"));
    let actions = [click("#a"), click("#b"), click("#c")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let map = load_map(dir.path(), ORG, PROJECT).unwrap();
    assert!(map.areas.iter().all(|a| a.refusals.is_empty()), "{:?}", map.areas.iter().map(|a| &a.refusals).collect::<Vec<_>>());
}

/// A message followed by a failure in the same batch is kept: the
/// failure is the evidence it was a refusal.
#[tokio::test]
async fn a_message_followed_by_a_failure_is_remembered() {
    let dir = root_with_recipe_and_account();
    let shows = |clicks: usize, _| if clicks >= 1 { vec![NO_COMPONENT] } else { vec![] };
    let (mut browser, _) = slot(telling_page(shows, 1), exploring("Leave"));
    let actions = [click("#save"), expect_goals(), click("#b")];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let map = load_map(dir.path(), ORG, PROJECT).unwrap();
    let rules = rules_of(&map, "Leave");
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert_eq!(rules[0].message, NO_COMPONENT);
    assert_eq!(rules[0].page, TELLING_PAGE);
    assert_eq!(rules[0].after, describe_action(&click("#save")));
}

/// A message two page changes before a failure is not taken for a rule:
/// only the nearest page change before the failure is.
#[tokio::test]
async fn a_message_two_page_changes_before_a_failure_is_not_remembered() {
    let dir = root_with_recipe_and_account();
    let saved = |clicks: usize, _| if clicks >= 1 { vec!["Saved."] } else { vec![] };
    let (mut browser, _) = slot(telling_page(saved, 2), exploring("Leave"));
    let actions = [click("#a"), click("#b"), expect_goals()];
    let (status, text) = discover_actions_in(&mut browser, dir.path(), ORG, PROJECT, &actions, None, None, true, &no_stop()).await;
    assert_eq!(status, 200, "{text}");
    let lines = batch_lines(&text);
    assert!(lines[0].contains(r#"the page showed: "Saved.""#) && lines[2].starts_with("3. failed: "), "{text}");
    let map = load_map(dir.path(), ORG, PROJECT).unwrap();
    assert!(rules_of(&map, "Leave").is_empty(), "{:?}", rules_of(&map, "Leave"));
}

// ----------------------------- a rule the assistant records in a discovery

use v2_lib::ai_bridge::record_validation_rule_in;

/// With a discovery going, the area is its area and the page is the one the
/// browser is on (cut to its path and filed as the map files pages); naming
/// either wins.
#[tokio::test]
async fn record_validation_rule_saves_into_the_discovery_area() {
    let dir = TempDir::new();
    put_path(dir.path(), ORG, PROJECT, leave_apply(MadeBy::Person, vec![css("#apply")], "/hr/leave/new")).unwrap();
    let (mut browser, _) = slot(cycles_page(1, true), exploring("Cycles"));
    let body = json!({ "message": "End date must be after the start date", "after": "clicked Save" }).to_string();
    let (status, out) = record_validation_rule_in(&mut browser, dir.path(), ORG, PROJECT, &body).await;
    assert_eq!((status, out.as_str()), (200, "Remembered for Cycles."));
    let area = mapped_area(dir.path(), "Cycles").expect("nothing was recorded");
    assert_eq!(area.refusals.len(), 1, "{:?}", area.refusals);
    assert_eq!(area.refusals[0].page, "/hr/cycles");
    assert_eq!(area.refusals[0].message, "End date must be after the start date");

    // Removed again from the current page, with the same defaults.
    let gone = json!({ "message": "end date must be after the start date", "remove": true }).to_string();
    let (status, out) = record_validation_rule_in(&mut browser, dir.path(), ORG, PROJECT, &gone).await;
    assert_eq!((status, out.as_str()), (200, "Removed from Cycles."));
    assert!(mapped_area(dir.path(), "Cycles").is_none_or(|a| a.refusals.is_empty()));

    // An area and a page named win over the discovery's.
    let named = json!({ "message": "Name is required", "after": "clicked Save", "area": "Leave Apply", "page": "/hr/leave/new" }).to_string();
    let (status, out) = record_validation_rule_in(&mut browser, dir.path(), ORG, PROJECT, &named).await;
    assert_eq!((status, out.as_str()), (200, "Remembered for Leave Apply."));
    assert_eq!(mapped_area(dir.path(), "Leave Apply").unwrap().refusals[0].page, "/hr/leave/new");

    // A browser that is open for no discovery has no area to default to.
    let (mut open, _) = slot(cycles_page(1, true), opened_for_discovery());
    let (status, out) = record_validation_rule_in(&mut open, dir.path(), ORG, PROJECT, &body).await;
    assert_eq!((status, out.as_str()), (400, "Name the area: no discovery is running."));
}

/// A removal never takes the page from the browser: with no page named it
/// is from every page of the area, wherever the browser is. An add with no
/// page named still takes the page the browser is on.
#[tokio::test]
async fn record_validation_rule_remove_ignores_the_browsers_page() {
    let dir = TempDir::new();
    let (mut browser, _) = slot(cycles_page(1, true), exploring("Cycles"));
    for page in ["/hr/leave", "/hr/cycles/new"] {
        let body = json!({ "message": "Name is required", "after": "clicked Save", "page": page }).to_string();
        let (status, _) = record_validation_rule_in(&mut browser, dir.path(), ORG, PROJECT, &body).await;
        assert_eq!(status, 200);
    }
    let gone = json!({ "message": "Name is required", "remove": true }).to_string();
    let (status, out) = record_validation_rule_in(&mut browser, dir.path(), ORG, PROJECT, &gone).await;
    assert_eq!((status, out.as_str()), (200, "Removed from Cycles."));
    assert!(mapped_area(dir.path(), "Cycles").is_none_or(|a| a.refusals.is_empty()));

    // The same with no browser at all.
    let body = json!({ "message": "Name is required", "after": "clicked Save", "area": "Cycles", "page": "/hr/leave" }).to_string();
    let none: &mut Option<FakeBrowser> = &mut None;
    let (status, _) = record_validation_rule_in(none, dir.path(), ORG, PROJECT, &body).await;
    assert_eq!(status, 200);
    let gone = json!({ "message": "Name is required", "area": "Cycles", "remove": true }).to_string();
    let (status, out) = record_validation_rule_in(none, dir.path(), ORG, PROJECT, &gone).await;
    assert_eq!((status, out.as_str()), (200, "Removed from Cycles."));

    // An add with no page is on the page the browser is on.
    let add = json!({ "message": "Name is required", "after": "clicked Save" }).to_string();
    let (status, _) = record_validation_rule_in(&mut browser, dir.path(), ORG, PROJECT, &add).await;
    assert_eq!(status, 200);
    assert_eq!(mapped_area(dir.path(), "Cycles").unwrap().refusals[0].page, "/hr/cycles");
}

/// The browser's page is read only for an add that names no page: a
/// removal, or an add with its page, never asks the browser where it is.
#[tokio::test]
async fn record_validation_rule_reads_the_page_only_for_an_add_without_one() {
    let dir = TempDir::new();
    let (mut browser, _) = slot(cycles_page(1, true), exploring("Cycles"));
    let reads = |b: &Option<FakeBrowser>| {
        b.as_ref().unwrap().d.calls_to("Runtime.evaluate").iter().filter(|p| p["expression"] == "location.href").count()
    };
    let named = json!({ "message": "Name is required", "after": "clicked Save", "page": "/hr/leave" }).to_string();
    assert_eq!(record_validation_rule_in(&mut browser, dir.path(), ORG, PROJECT, &named).await.0, 200);
    let gone = json!({ "message": "Name is required", "remove": true }).to_string();
    assert_eq!(record_validation_rule_in(&mut browser, dir.path(), ORG, PROJECT, &gone).await.0, 200);
    assert_eq!(reads(&browser), 0, "a removal or a named page read the browser's page");
    let add = json!({ "message": "Name is required", "after": "clicked Save" }).to_string();
    assert_eq!(record_validation_rule_in(&mut browser, dir.path(), ORG, PROJECT, &add).await.0, 200);
    assert_eq!(reads(&browser), 1);
}

// ----------------------------------------------- a start at its saved area

/// A start naming a saved area walks its clicks as a run does: the browser
/// ends on the area's page, the answer says `at_area: true`, and the page it
/// reads is filed under the area.
#[tokio::test]
async fn a_start_naming_a_saved_area_walks_to_it() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#leave"), css("#apply")])).unwrap();
    let (mut browser, closed) = slot(menu_app(false, "/"), opened_for_discovery());

    let (status, body) =
        discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some("leave apply"), &quick()).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["at_area"], true, "{body}");
    assert_eq!(v["path"], "/hr/leave/apply", "{body}");
    assert!(v.get("area_walk").is_none(), "{body}");
    assert!(!closed.load(Ordering::SeqCst));
    let state = browser.as_ref().unwrap().discovery.as_ref().unwrap();
    assert_eq!(state.area.as_deref(), Some("Leave Apply"));
    let area = mapped_area(dir.path(), "Leave Apply").expect("the area's page was not filed");
    assert!(area.pages.iter().any(|p| p.path == "/hr/leave/apply"), "{:?}", area.pages);
}

/// An area that is not saved starts at home, as before, and says so.
#[tokio::test]
async fn a_start_naming_an_unsaved_area_goes_home_and_says_so() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(menu_app(false, "/"), opened_for_discovery());

    let (status, body) =
        discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some("Nowhere"), &quick()).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["at_area"], false, "{body}");
    assert_eq!(
        v["area_walk"],
        "no saved area is called Nowhere; the discovery starts at home and files what it sees under that name"
    );
    assert_eq!(v["path"], "/hr/home/index", "{body}");
    assert_eq!(browser.as_ref().unwrap().discovery.as_ref().unwrap().area.as_deref(), Some("Nowhere"));
}

/// A start with no area names none of this in its answer.
#[tokio::test]
async fn a_start_with_no_area_says_nothing_of_one() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(menu_app(false, "/"), opened_for_discovery());
    let (status, body) = discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", None, &quick()).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert!(v.get("at_area").is_none() && v.get("area_walk").is_none(), "{body}");
}

/// A walk that cannot arrive leaves the discovery started and the browser
/// open where it stopped, with the reason in `area_walk`.
#[tokio::test]
async fn a_start_whose_walk_fails_stays_started_and_says_why() {
    let dir = root_with_recipe_and_account();
    put_path(dir.path(), ORG, PROJECT, persons_leave_apply(vec![css("#leave"), css("#missing")])).unwrap();
    let (mut browser, closed) = slot(menu_app(false, "/"), opened_for_discovery());

    let (status, body) =
        discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some("Leave Apply"), &quick()).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["signed_in"], true, "{body}");
    assert_eq!(v["at_area"], false, "{body}");
    assert!(v["area_walk"].as_str().unwrap_or("").contains("Leave Apply"), "{body}");
    assert!(!closed.load(Ordering::SeqCst), "the browser was closed");
    let b = browser.as_ref().expect("the browser stays");
    let state = b.discovery.as_ref().expect("the discovery stays started");
    assert_eq!(state.area.as_deref(), Some("Leave Apply"));
    assert_eq!(state.account.as_deref(), Some("admin"));
}
