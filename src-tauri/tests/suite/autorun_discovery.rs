//! Discovery sessions: the assistant opens the Auto Run browser itself,
//! signs in as a saved account (the app types the password), and explores
//! the live application one action at a time. What it sees, the writes the
//! page sends and what each action led to are kept in the discovery map.
//!
//! The real browser is never opened here. The routes' checks that need no
//! browser run through `route`; everything after the browser opens runs
//! through the `_in` functions, against a fake browser slot that holds a
//! scripted driver and says when it was closed.

use crate::common::{account, quick, ready_probe, FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::ai_bridge::{
    close_browser_in, discover_action_in, discover_start_in, discovery_sighting, end_discovery_in, read_page, route,
    BridgeContext, DiscoveryBrowser, DiscoveryParts,
};
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::discovery_map::{load_map, map_path, AreaMap};
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::recipe::{save_recipe, SignInRecipe};
use v2_lib::autorun::store::set_root;
use v2_lib::browser::actions::{Action, CHECK_TEXT_JS, HIGHLIGHT_JS};
use v2_lib::browser::cdp::Event;
use v2_lib::browser::input::{FOCUS_JS, HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;
use v2_lib::browser::snapshot::DEFAULT_LIMIT;
use v2_lib::commands::autorun::{busy_browser_sentence, DiscoveryState};

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
    Some(DiscoveryState { area: None, account: None })
}

fn exploring(area: &str) -> Option<DiscoveryState> {
    Some(DiscoveryState { area: Some(area.to_string()), account: Some("admin".to_string()) })
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
/// map under the discovery's area, explored by the account KEY.
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
    assert!(area.explored_at.is_some(), "a discovery's landing page stamps the area explored");
    assert_eq!(area.account.as_deref(), Some("admin"), "the account KEY, never the login");
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
        discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, Some("Leave Requests")).await;
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
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &open, None).await;
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
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &fill, None).await;
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
    let (status, out) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None).await;
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

// ------------------------------------------------- what a page read files

/// A page read in a browser a discovery holds stamps its area explored, by
/// the discovery's account KEY - not by whoever the browser last signed in
/// as. Outside a discovery the same read stamps nothing.
#[tokio::test]
async fn a_discovery_page_read_stamps_explored_at_and_the_account_key() {
    let dir = TempDir::new();
    let state = DiscoveryState { area: Some("Leave".into()), account: Some("admin".into()) };
    let at = discovery_sighting(dir.path(), ORG, PROJECT, Some(&state), None, Some("manager"))
        .expect("a project is chosen, so there is somewhere to file it");
    assert!(at.discovering);
    let (status, text) = read_page(&mut signin_app(true), DEFAULT_LIMIT, Some(&at)).await;
    assert_eq!(status, 200, "{text}");
    let area = mapped_area(dir.path(), "Leave").expect("nothing was recorded");
    assert!(area.explored_at.is_some(), "a discovery's page read did not stamp the area");
    assert_eq!(area.account.as_deref(), Some("admin"));

    let other = TempDir::new();
    let at = discovery_sighting(other.path(), ORG, PROJECT, None, None, Some("manager")).unwrap();
    assert!(!at.discovering);
    let (status, _) = read_page(&mut signin_app(true), DEFAULT_LIMIT, Some(&at)).await;
    assert_eq!(status, 200);
    let area = mapped_area(other.path(), "").expect("nothing was recorded");
    assert_eq!(area.explored_at, None, "a page read outside a discovery is not one");

    assert!(discovery_sighting(dir.path(), ORG, "  ", Some(&state), None, None).is_none(), "no project, nowhere to file");
}
