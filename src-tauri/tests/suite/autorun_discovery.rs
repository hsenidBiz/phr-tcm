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
use v2_lib::ai_bridge::{
    close_browser_in, discover_action_in, discover_area_in, discover_start_in, discovery_sighting, end_discovery_in,
    read_page, refuse_while_discovering, route, BridgeContext, DiscoveryBrowser, DiscoveryParts, NO_DISCOVERY,
};
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
use v2_lib::commands::autorun::{busy_browser_sentence, DiscoveryState, MappingPlace, MappingRun};

const ORG: &str = "acme";
const PROJECT: &str = "Web";

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::AtomicU64;
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-autorun-discovery-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ctx() -> BridgeContext {
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
fn root_with_recipe_and_account() -> TempDir {
    let dir = TempDir::new();
    save_recipe(dir.path(), ORG, PROJECT, &recipe()).unwrap();
    save_accounts(dir.path(), &[account()]).unwrap();
    dir
}

/// The Auto Run browser as the `_in` functions see it: a driver, its lease,
/// its signed-in account and the discovery under way. `closed` says
/// whether the slot closed it.
struct FakeBrowser {
    d: ScriptedDriver,
    lease: Held,
    account: Option<String>,
    discovery: Option<DiscoveryState>,
    closed: Arc<AtomicBool>,
}

impl DiscoveryBrowser for FakeBrowser {
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
}

/// A browser slot holding `d`, opened for a discovery (no account yet)
/// when `discovering`, else the person's own browser.
fn slot(d: ScriptedDriver, discovery: Option<DiscoveryState>) -> (Option<FakeBrowser>, Arc<AtomicBool>) {
    let closed = Arc::new(AtomicBool::new(false));
    let b = FakeBrowser { d, lease: Held::supervised(), account: None, discovery, closed: closed.clone() };
    (Some(b), closed)
}

fn opened_for_discovery() -> Option<DiscoveryState> {
    Some(DiscoveryState { area: None, account: None, started_at: 1, tried: Vec::new(), mapping: None })
}

fn exploring(area: &str) -> Option<DiscoveryState> {
    Some(DiscoveryState {
        area: Some(area.to_string()),
        account: Some("admin".to_string()),
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
    })
}

/// `state`, made a mapping run over `modules`.
fn mapping(state: Option<DiscoveryState>, modules: &[&str]) -> Option<DiscoveryState> {
    let modules: Vec<String> = modules.iter().map(|m| m.to_string()).collect();
    state.map(|s| DiscoveryState { mapping: Some(MappingRun::new(&modules)), ..s })
}

fn mapped_area(root: &std::path::Path, area: &str) -> Option<AreaMap> {
    load_map(root, ORG, PROJECT).unwrap().areas.into_iter().find(|a| a.area == area)
}

/// A small application: a sign-in page whose `#go` click lands on
/// `/hr/home/index`, and a page with a Name field and a Save button. Its
/// address always carries a query string, which nothing may keep. `#marker`
/// is found only when `marker_shows`.
fn signin_app(marker_shows: bool) -> ScriptedDriver {
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
fn leave_page(moves_on: &'static str, after: &'static str) -> ScriptedDriver {
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

fn sent(id: &str, method: &str, url: &str) -> Event {
    Event {
        method: "Network.requestWillBeSent".into(),
        params: json!({ "requestId": id, "request": { "url": url, "method": method } }),
    }
}

fn parsed(body: &str) -> Value {
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

/// Ending closes a discovery's browser, and ending again - or with no
/// browser, or with the person's own browser open - is fine and closes
/// nothing more.
#[tokio::test]
async fn end_is_idempotent() {
    let (mut browser, closed) = slot(FakePage::default().driver(), exploring("Leave"));
    let (status, _) = end_discovery_in(&mut browser);
    assert_eq!(status, 200);
    assert!(closed.load(Ordering::SeqCst));
    assert!(browser.is_none());
    let (status, _) = end_discovery_in(&mut browser);
    assert_eq!(status, 200);

    let (mut theirs, closed) = slot(FakePage::default().driver(), None);
    let (status, _) = end_discovery_in(&mut theirs);
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
    let path = Arc::new(Mutex::new(at.to_string()));
    let signed = Arc::new(AtomicBool::new(signed_in));
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
        Some(DiscoveryState { area: None, account: Some("admin".into()), started_at: 1, tried: Vec::new(), mapping: None });
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

/// Finding 4: End discovery ends the discovery the way
/// `/autorun-discover-end` does, and is fine with nothing to end.
#[tokio::test]
async fn the_end_discovery_command_ends_like_the_route() {
    let _g = crate::serial::autorun();
    for _ in 0..2 {
        assert_eq!(v2_lib::commands::autorun::auto_run_end_discovery().await, Ok(()));
    }
    assert!(!v2_lib::commands::autorun::auto_run_discovery_active());
    let source = include_str!("../../src/commands/autorun.rs");
    let cmd = &source[source.find("pub async fn auto_run_end_discovery").unwrap()..];
    assert!(cmd[..cmd.find('}').unwrap()].contains("end_discovery().await"), "not the route's own ending");
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
    assert!(body.contains("Not There is not saved"), "{body}");

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
    let (status, _) =
        discover_area_in(&mut browser, dir.path(), ORG, PROJECT, "Payroll", "Payroll", vec![css("#missing")], &quick())
            .await;
    assert_eq!(status, 409);
    // The page tries a save after the last action: nothing drains it but the end.
    browser.as_mut().unwrap().d.saves_stopped.push(("POST".into(), "/api/SaveLastVisited".into()));

    assert_eq!(load_summary(dir.path(), ORG, PROJECT), Ok(None), "kept before the run ended");
    let (status, body) = end_discovery_in(&mut browser);
    assert_eq!(status, 200, "{body}");
    assert!(closed.load(Ordering::SeqCst), "the browser was left open");
    assert!(browser.is_none());

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
    let (status, body) = end_discovery_in(&mut browser);
    assert_eq!((status, body.as_str()), (200, "no discovery is going"));
    assert_eq!(load_summary(dir.path(), ORG, PROJECT), Ok(Some(answered)));

    // An ordinary discovery keeps none, and answers in words.
    let other = root_with_recipe_and_account();
    let (mut browser, _) = slot(menu_app(true, "/hr/leave"), exploring("Leave"));
    let (status, body) = end_discovery_in(&mut browser);
    assert_eq!((status, body.as_str()), (200, "the discovery is over and its browser is closed"));
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
    let (status, body) = end_discovery_in(&mut browser);
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
