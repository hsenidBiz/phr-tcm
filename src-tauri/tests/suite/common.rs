//! Test doubles shared by the browser tests. Declared once in main.rs for
//! the whole suite, and not every module uses every helper.
#![allow(dead_code)]

use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use v2_lib::autorun::accounts::Account;
use v2_lib::autorun::recipe::SignInRecipe;
use v2_lib::browser::actions::{CHECK_TEXT_JS, HIGHLIGHT_JS, RESOLVE_URL_JS};
use v2_lib::browser::cdp::{
    no_new_tab, no_tab, tab_did_not_close, tab_taken, CdpError, Driver, Event, MAIN_CANNOT_CLOSE, MAIN_TAB,
};
use v2_lib::browser::dialogs::DialogBook;
use v2_lib::browser::page_errors::PageErrorBook;
use v2_lib::browser::downloads::DownloadEntry;
use v2_lib::browser::expect::{READ_ATTR_JS, READ_TEXT_JS};
use v2_lib::browser::input::{FOCUS_JS, HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;
use v2_lib::browser::net_record::{NetEntry, NetRecord};
use v2_lib::browser::timing::Timing;

type Handler =
    Box<dyn FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, CdpError> + Send>;

/// A browser that answers from a closure and remembers every call.
pub struct ScriptedDriver {
    pub calls: Vec<(String, serde_json::Value)>,
    handler: Handler,
    /// Events already waiting.
    pub events: VecDeque<Event>,
    /// Events that appear once a call to the named method has been made -
    /// how a test says "the load event follows Page.navigate".
    pub on_call_events: Vec<(String, Event)>,
    /// Events that appear after EVERY call to the named method - how a
    /// test says "the load event follows Page.navigate" when that method
    /// is called more than once (`on_call_events` fires once and is
    /// consumed, which cannot express two navigations).
    pub on_every_call_events: Vec<(String, Event)>,
    pub dialogs: Vec<String>,
    /// Every value handed to `set_deadline`, in order. A wait loop must
    /// leave `Some(&None)` here on every path out, or a later action
    /// inherits a budget that has already run out.
    pub deadlines: Vec<Option<Instant>>,
    /// When no event is waiting, `wait_event` says the browser closed
    /// instead of timing out - a window the person shut.
    pub closed_when_drained: bool,
    /// What `page_log` reports - the lines a real page's requests and
    /// console would have given.
    pub page_log: Vec<String>,
    /// A network record fed from the events this driver emits after a
    /// call, as a real browser's is (`with_net_record`). While it is on,
    /// `Network.*` events go to it and not to `events`, as they do in
    /// `Cdp`. `None` keeps the trait's defaults: no record at all.
    pub net: Option<NetRecord>,
    /// A save the guard stopped, noticed once a call to the named method
    /// has been made - how a test says "the click made the page save".
    pub block_after: Option<(String, String)>,
    /// The sentence `take_save_blocked` hands over, once.
    pub save_blocked: Option<String>,
    /// Every folder `enable_downloads` was handed, in order.
    pub download_dirs: Vec<std::path::PathBuf>,
    /// What `downloads` reports: a test pushes the entries a real browser
    /// would have followed.
    pub downloads: Vec<DownloadEntry>,
    /// A download that the browser is heard to start once a call to the
    /// named method is made, dated then - how a test says "the browser read
    /// this download's start while that call was answered". Fires once.
    pub downloads_on_call: Vec<(String, DownloadEntry)>,
    /// Its tabs, as a test models them (`Tabs`): which tab each call went
    /// to, and what the tab actions did.
    pub tabs: Tabs,
    /// The run's dialog book, as a real browser's (`Cdp`) keeps it.
    pub book: DialogBook,
    /// A dialog the page opens once a call to the named method is made:
    /// (method, kind, message). Answered at once, as `Cdp` answers one - the
    /// answer is recorded as a `Page.handleJavaScriptDialog` call in the tab
    /// the dialog opened in - and, when nobody expected it, noted for
    /// `take_dialogs`. Fires once.
    pub dialogs_on_call: Vec<(String, String, String)>,
    /// The run's page errors, fed every event this driver emits after a
    /// call, as `Cdp` feeds its own.
    pub page_errors: PageErrorBook,
}

/// A small model of a browser's tabs for `ScriptedDriver`: `main`, the
/// tabs a script named, and tabs the page opened that nobody named yet.
#[derive(Default)]
pub struct Tabs {
    /// Named tabs other than `main`, in the order they were named.
    pub open: Vec<String>,
    /// The current tab; empty is `main`.
    pub current: String,
    /// The current tab's name once it closed by itself.
    pub gone: Option<String>,
    /// Named tabs that closed by themselves, not yet claimed.
    pub closed_by_page: Vec<String>,
    /// Tabs the page opened that no `expect_tab` claimed yet.
    pub unnamed: usize,
    /// A call to this method makes the page open a tab.
    pub opens_on: Option<String>,
    /// A call to this method makes the page close this tab itself.
    pub closes_on: Option<(String, String)>,
    /// Every call, with the tab it went to.
    pub calls: Vec<(String, String)>,
    /// How often `close_other_tabs` ran.
    pub closed_others: usize,
    /// How often `step_began` ran.
    pub steps_begun: usize,
}

impl Tabs {
    pub fn now(&self) -> String {
        if let Some(g) = &self.gone {
            return g.clone();
        }
        if self.current.is_empty() { MAIN_TAB.to_string() } else { self.current.clone() }
    }
    fn knows(&self, name: &str) -> bool {
        name == MAIN_TAB || self.open.iter().any(|t| t == name)
    }
    /// The methods called in `tab`, in order.
    pub fn called_in(&self, tab: &str) -> Vec<String> {
        self.calls.iter().filter(|(t, _)| t == tab).map(|(_, m)| m.clone()).collect()
    }
}

impl ScriptedDriver {
    pub fn new(
        handler: impl FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, CdpError>
            + Send
            + 'static,
    ) -> Self {
        ScriptedDriver {
            calls: vec![],
            handler: Box::new(handler),
            events: VecDeque::new(),
            on_call_events: vec![],
            on_every_call_events: vec![],
            dialogs: vec![],
            deadlines: vec![],
            closed_when_drained: false,
            page_log: vec![],
            net: None,
            block_after: None,
            save_blocked: None,
            download_dirs: vec![],
            downloads: vec![],
            downloads_on_call: vec![],
            tabs: Tabs::default(),
            book: DialogBook::default(),
            dialogs_on_call: vec![],
            page_errors: PageErrorBook::default(),
        }
    }

    /// Keep a network record of the events this driver emits.
    pub fn with_net_record(mut self) -> Self {
        self.net = Some(NetRecord::default());
        self
    }

    pub fn methods(&self) -> Vec<String> {
        self.calls.iter().map(|(m, _)| m.clone()).collect()
    }

    pub fn calls_to(&self, method: &str) -> Vec<serde_json::Value> {
        self.calls.iter().filter(|(m, _)| m == method).map(|(_, p)| p.clone()).collect()
    }

    /// Did the wait loop that just ran hand its deadline back? Asserted on
    /// every exit path: success, page failure and harness failure alike.
    pub fn deadline_was_cleared(&self) -> bool {
        matches!(self.deadlines.last(), Some(None))
    }
}

impl Driver for ScriptedDriver {
    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CdpError> {
        self.tabs.calls.push((self.tabs.now(), method.to_string()));
        if let Some(name) = &self.tabs.gone {
            return Err(CdpError::Tab(no_tab(name)));
        }
        if self.tabs.opens_on.as_deref() == Some(method) {
            self.tabs.unnamed += 1;
        }
        if let Some((m, tab)) = self.tabs.closes_on.clone() {
            if m == method {
                self.tabs.closes_on = None;
                self.tabs.open.retain(|t| *t != tab);
                self.tabs.closed_by_page.push(tab.clone());
                if self.tabs.current == tab {
                    self.tabs.gone = Some(tab);
                }
            }
        }
        self.calls.push((method.to_string(), params.clone()));
        if self.block_after.as_ref().is_some_and(|(m, _)| m == method) {
            self.save_blocked = self.block_after.take().map(|(_, s)| s);
        }
        let mut fired = vec![];
        self.on_call_events.retain(|(m, ev)| {
            if m == method {
                fired.push(ev.clone());
                false
            } else {
                true
            }
        });
        for (m, ev) in &self.on_every_call_events {
            if m == method {
                fired.push(ev.clone());
            }
        }
        for ev in fired {
            self.page_errors.observe(&ev);
            if let Some(net) = self.net.as_mut() {
                net.observe(&ev);
                if ev.method.starts_with("Network.") {
                    continue;
                }
            }
            self.events.push_back(ev);
        }
        let mut heard = vec![];
        self.downloads_on_call.retain(|(m, e)| {
            if m == method {
                heard.push(e.clone());
                false
            } else {
                true
            }
        });
        for mut e in heard {
            e.started_at = Instant::now();
            self.downloads.push(e);
        }
        let reply = (self.handler)(method, &params);
        let mut opened = vec![];
        self.dialogs_on_call.retain(|(m, kind, message)| {
            if m == method {
                opened.push((kind.clone(), message.clone()));
                false
            } else {
                true
            }
        });
        for (kind, message) in opened {
            let answer = self.book.opened(&kind, &message);
            self.tabs.calls.push((self.tabs.now(), "Page.handleJavaScriptDialog".to_string()));
            self.calls.push(("Page.handleJavaScriptDialog".to_string(), answer));
            if self.book.seen().last().is_some_and(|s| s.claimed_by.is_none()) {
                self.dialogs.push(format!("{kind}: {message}"));
            }
        }
        reply
    }

    async fn wait_event(&mut self, method: &str, _limit: Duration) -> Result<Event, CdpError> {
        match self.events.iter().position(|e| e.method == method) {
            Some(i) => Ok(self.events.remove(i).expect("position was just found")),
            None if self.closed_when_drained => Err(CdpError::Closed),
            None => Err(CdpError::Timeout { what: method.to_string(), ms: 0 }),
        }
    }

    fn forget_events(&mut self) {
        self.events.clear();
    }

    fn take_dialogs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.dialogs)
    }

    fn dialog_book(&mut self) -> Option<&mut DialogBook> {
        Some(&mut self.book)
    }

    fn page_error_book(&mut self) -> Option<&mut PageErrorBook> {
        Some(&mut self.page_errors)
    }

    fn page_log(&self) -> Vec<String> {
        self.page_log.clone()
    }

    fn net_mark(&self) -> u64 {
        self.net.as_ref().map_or(0, NetRecord::mark)
    }

    fn net_since(&self, mark: u64) -> Vec<NetEntry> {
        self.net.as_ref().map_or_else(Vec::new, |n| n.since(mark))
    }

    fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadlines.push(deadline);
    }

    fn take_save_blocked(&mut self) -> Option<String> {
        self.save_blocked.take()
    }

    async fn enable_downloads(&mut self, dir: &std::path::Path) -> Result<(), CdpError> {
        self.download_dirs.push(dir.to_path_buf());
        Ok(())
    }

    fn downloads(&self) -> Vec<DownloadEntry> {
        self.downloads.clone()
    }

    /// Guarding since the last `Fetch.enable` that no `Fetch.disable`
    /// followed.
    fn is_guarding_saves(&self) -> bool {
        self.calls.iter().rev().find(|(m, _)| m == "Fetch.enable" || m == "Fetch.disable").is_some_and(|(m, _)| m == "Fetch.enable")
    }

    fn step_began(&mut self) {
        self.tabs.steps_begun += 1;
    }
    fn tab_name(&self) -> String {
        self.tabs.now()
    }
    fn missing_tab(&self) -> Option<String> {
        self.tabs.gone.clone()
    }
    async fn expect_tab(&mut self, name: &str, _url_contains: Option<&str>, within: Duration) -> Result<String, CdpError> {
        if self.tabs.knows(name) {
            return Err(CdpError::Tab(tab_taken(name)));
        }
        if self.tabs.unnamed == 0 {
            return Err(CdpError::Tab(no_new_tab(within)));
        }
        self.tabs.unnamed -= 1;
        self.tabs.open.push(name.to_string());
        Ok("https://hr.example/report".to_string())
    }
    async fn open_tab(&mut self, name: &str) -> Result<(), CdpError> {
        if self.tabs.knows(name) {
            return Err(CdpError::Tab(tab_taken(name)));
        }
        self.tabs.open.push(name.to_string());
        self.tabs.current = name.to_string();
        self.tabs.gone = None;
        Ok(())
    }
    async fn switch_tab(&mut self, name: &str) -> Result<(), CdpError> {
        if !self.tabs.knows(name) {
            return Err(CdpError::Tab(no_tab(name)));
        }
        self.tabs.current = if name == MAIN_TAB { String::new() } else { name.to_string() };
        self.tabs.gone = None;
        Ok(())
    }
    async fn close_tab(&mut self, name: &str) -> Result<bool, CdpError> {
        if name == MAIN_TAB {
            return Err(CdpError::Tab(MAIN_CANNOT_CLOSE.to_string()));
        }
        if !self.tabs.knows(name) {
            return Err(CdpError::Tab(no_tab(name)));
        }
        self.tabs.open.retain(|t| t != name);
        let was = self.tabs.current == name;
        if was {
            self.tabs.current.clear();
        }
        Ok(was)
    }
    async fn expect_tab_closed(&mut self, name: &str, within: Duration) -> Result<(), CdpError> {
        if name == MAIN_TAB {
            return Err(CdpError::Tab(MAIN_CANNOT_CLOSE.to_string()));
        }
        if let Some(i) = self.tabs.closed_by_page.iter().position(|t| t == name) {
            self.tabs.closed_by_page.remove(i);
            if self.tabs.gone.as_deref() == Some(name) {
                self.tabs.gone = None;
                self.tabs.current.clear();
            }
            return Ok(());
        }
        if self.tabs.knows(name) {
            return Err(CdpError::Tab(tab_did_not_close(name, within)));
        }
        Err(CdpError::Tab(no_tab(name)))
    }
    async fn close_other_tabs(&mut self) {
        self.tabs.closed_others += 1;
        self.tabs.open.clear();
        self.tabs.closed_by_page.clear();
        self.tabs.unnamed = 0;
        self.tabs.current.clear();
        self.tabs.gone = None;
    }
}

/// The actionability probe's answer for an element that is fully ready:
/// visible, onscreen, enabled, editable, unobstructed, and (since a
/// `FakePage`'s single static answer repeats) holding still across the
/// two looks `wait_ready` needs to call it ready.
pub fn ready_probe() -> Value {
    json!({
        "visible": true, "onscreen": true, "enabled": true, "editable": true,
        "hit": true, "x": 10.0, "y": 20.0, "covered_by": "", "rect": [0.0, 0.0, 80.0, 24.0]
    })
}

/// A page described by what it would answer. Every locator finds `found`
/// elements (none until the `appears_on_look`-th look), and each function
/// the runner calls gets the matching field back.
pub struct FakePage {
    pub found: usize,
    /// 1 = there from the first look.
    pub appears_on_look: usize,
    /// Answers to the actionability probe, in turn; the last repeats.
    pub probes: Vec<Value>,
    pub visible: bool,
    pub fill_kind: &'static str,
    /// Does the field still hold the focus when the text is about to be
    /// sent? False is a page that moved it in between.
    pub has_focus: bool,
    pub body_has_text: bool,
    pub href: &'static str,
    /// What the page makes of a relative `navigate` url.
    pub resolved_url: &'static str,
    pub navigate_reply: Value,
    /// Answers to "what does it say", in turn; the last repeats.
    pub texts: Vec<&'static str>,
    pub attribute: Option<&'static str>,
    /// How many `Runtime.getProperties` / `PROBE_JS` / `READ_TEXT_JS` calls
    /// `answer` has already replied to. `Cell`, not a plain field, so
    /// `answer` can take `&self` - letting a test hold onto a `FakePage`
    /// and feed it calls one at a time instead of only through `driver`,
    /// which consumes it. `pub` like every other field here: a struct
    /// update (`FakePage { found: 0, ..FakePage::default() }`) needs to
    /// see every field it does not name, from every test file that uses
    /// one.
    pub looks: Cell<usize>,
    pub probed: Cell<usize>,
    pub read: Cell<usize>,
}

impl Default for FakePage {
    fn default() -> Self {
        FakePage {
            found: 1,
            appears_on_look: 1,
            probes: vec![ready_probe()],
            visible: true,
            fill_kind: "text",
            has_focus: true,
            body_has_text: true,
            href: "https://app.example/home",
            resolved_url: "https://app.example/dashboard",
            navigate_reply: json!({ "frameId": "F", "loaderId": "L" }),
            texts: vec!["Saved"],
            attribute: None,
            looks: Cell::new(0),
            probed: Cell::new(0),
            read: Cell::new(0),
        }
    }
}

impl FakePage {
    /// One call, answered the way the page described by `self` would
    /// answer it. `driver` is this, wrapped in a `ScriptedDriver` that owns
    /// the page outright; a test that needs to mix real answers with a
    /// scripted failure (a browser that goes silent partway through a
    /// look, say) calls this directly instead, keeping its own `FakePage`
    /// around to delegate to one call at a time.
    pub fn answer(&self, method: &str, params: &Value) -> Result<Value, CdpError> {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => {
                json!({ "result": { "objectId": "doc" } })
            }
            "Runtime.evaluate" => json!({ "result": { "value": self.href } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => {
                let i = self.probed.get().min(self.probes.len() - 1);
                self.probed.set(self.probed.get() + 1);
                json!({ "result": { "value": self.probes[i] } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS => json!({ "result": { "value": self.visible } }),
            "Runtime.callFunctionOn" if f == HIGHLIGHT_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if f == FOCUS_JS => json!({ "result": { "value": self.fill_kind } }),
            "Runtime.callFunctionOn" if f == HAS_FOCUS_JS => {
                json!({ "result": { "value": self.has_focus } })
            }
            "Runtime.callFunctionOn" if f == RESOLVE_URL_JS => {
                json!({ "result": { "value": self.resolved_url } })
            }
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => {
                json!({ "result": { "value": self.body_has_text } })
            }
            "Runtime.callFunctionOn" if f == READ_TEXT_JS => {
                let i = self.read.get().min(self.texts.len() - 1);
                self.read.set(self.read.get() + 1);
                json!({ "result": { "value": self.texts[i] } })
            }
            "Runtime.callFunctionOn" if f == READ_ATTR_JS => json!({ "result": { "value": self.attribute } }),
            // Any locator function: an array of elements.
            "Runtime.callFunctionOn" => json!({ "result": { "objectId": "arr" } }),
            "Runtime.getProperties" => {
                self.looks.set(self.looks.get() + 1);
                let n = if self.looks.get() >= self.appears_on_look { self.found } else { 0 };
                json!({ "result": (0..n)
                    .map(|i| json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } }))
                    .collect::<Vec<_>>() })
            }
            "Page.navigate" => self.navigate_reply.clone(),
            _ => json!({}),
        })
    }

    pub fn driver(self) -> ScriptedDriver {
        ScriptedDriver::new(move |method, params| self.answer(method, params))
    }
}

/// Every record from `dir`'s `{kind}-*.jsonl` files (as `activity_log::init`
/// was pointed at it), parsed and in file-then-line order. A bad line is
/// skipped rather than panicking the test that called this - the point is
/// to read back what `activity_log::record` wrote, not to validate it.
pub fn activity_records(dir: &std::path::Path, kind: &str) -> Vec<Value> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            name.starts_with(&format!("{kind}-")) && name.ends_with(".jsonl")
        })
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .flat_map(|text| {
            text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect::<Vec<_>>()
        })
        .collect()
}

/// The password `stateful_app`'s account signs in with. Used by both
/// `autorun_signin.rs` and `autorun_runner.rs` - one stateful fake, never
/// copied.
pub const PASSWORD: &str = "s3cret-Value";

pub fn quick() -> Timing {
    Timing { action_ms: 400, expect_ms: 150, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

pub fn account() -> Account {
    Account { key: "admin".into(), label: "Administrator".into(), username: "kim".into(), password: PASSWORD.into() }
}

/// The recipe `stateful_app` answers to: fill username, fill password,
/// click go, and an optional "another session" prompt that never appears
/// against this fake.
pub fn recipe() -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": [
            { "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" },
            { "kind": "fill", "selector": { "css": "#pass" }, "value": "{{password}}" },
            { "kind": "click", "selector": { "css": "#go" } },
            { "kind": "when_visible", "selector": { "css": "#other-session" }, "within_ms": 60,
              "then": [ { "kind": "click", "selector": { "css": "#other-session" } } ] }
        ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

/// A page with a login form. `#marker` exists only once `signed_in` is
/// set, which happens when the password has been typed and `#go` clicked,
/// or from the start when `cookie_is_good` and cookies were restored.
pub struct StatefulApp {
    pub signed_in: Arc<AtomicBool>,
    pub typed_password: Arc<AtomicBool>,
    pub restored: Arc<AtomicBool>,
    pub clicks: Arc<AtomicUsize>,
}

pub fn stateful_app(cookie_is_good: bool, broken_selector: Option<&'static str>) -> (ScriptedDriver, StatefulApp) {
    let state = StatefulApp {
        signed_in: Arc::new(AtomicBool::new(false)),
        typed_password: Arc::new(AtomicBool::new(false)),
        restored: Arc::new(AtomicBool::new(false)),
        clicks: Arc::new(AtomicUsize::new(0)),
    };
    let (signed_in, typed, restored, clicks) =
        (state.signed_in.clone(), state.typed_password.clone(), state.restored.clone(), state.clicks.clone());
    let mut last_selector = String::new();
    let ready = json!({ "visible": true, "onscreen": true, "enabled": true, "editable": true, "hit": true,
        "x": 5.0, "y": 5.0, "covered_by": "", "rect": [0.0, 0.0, 10.0, 10.0] });
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Network.setCookies" => {
                restored.store(true, Ordering::SeqCst);
                json!({})
            }
            "Network.clearBrowserCookies" => {
                signed_in.store(false, Ordering::SeqCst);
                restored.store(false, Ordering::SeqCst);
                json!({})
            }
            "Page.navigate" => {
                if cookie_is_good && restored.load(Ordering::SeqCst) {
                    signed_in.store(true, Ordering::SeqCst);
                }
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Network.getAllCookies" => json!({ "cookies": [
                { "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true }
            ] }),
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": ready } }),
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last_selector = params["arguments"][0]["value"].as_str().unwrap().to_string();
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.callFunctionOn" => json!({ "result": { "value": "text" } }),
            "Runtime.getProperties" => {
                let there = match last_selector.as_str() {
                    "#marker" => signed_in.load(Ordering::SeqCst),
                    "#other-session" => false,
                    s if Some(s) == broken_selector => false,
                    _ => true,
                };
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Input.insertText" => {
                if params["text"] == PASSWORD {
                    typed.store(true, Ordering::SeqCst);
                }
                json!({})
            }
            "Input.dispatchMouseEvent" => {
                if params["type"] == "mouseReleased" {
                    clicks.fetch_add(1, Ordering::SeqCst);
                    if typed.load(Ordering::SeqCst) {
                        signed_in.store(true, Ordering::SeqCst);
                    }
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
    (d, state)
}

/// The sign-in recipe `menu_app` answers to: one css click, then a marker
/// that is always there. Its home page is the HR application's.
pub fn menu_recipe() -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/hr/home/index",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

/// What `menu_app` saw, in order: `navigate <path>`, `click <name or
/// css>`, `check <value>`; and where its page is now.
pub struct MenuApp {
    pub log: Arc<Mutex<Vec<String>>>,
    pub path: Arc<Mutex<String>>,
}

/// An application with a click-only menu. Each entry is (role, accessible
/// name, the path a click on it lands on). Every css locator is found, so
/// `menu_recipe` always signs in; its `#go` click lands on `landing`. A
/// menu click's new path shows only after `lag` more reads of the address,
/// the way an application that routes asynchronously behaves. `check_text`
/// passes for the value "yes" only.
pub fn menu_app(
    entries: &[(&'static str, &'static str, &'static str)],
    landing: &'static str,
    lag: usize,
) -> (ScriptedDriver, MenuApp) {
    stalling_menu_app(entries, landing, lag, 0)
}

/// `menu_app` whose first `stalls` menu clicks land nowhere - a page still
/// busy loading behind its menu, the way a first trip to a module can
/// stall. The clicks are still logged.
pub fn stalling_menu_app(
    entries: &[(&'static str, &'static str, &'static str)],
    landing: &'static str,
    lag: usize,
    stalls: usize,
) -> (ScriptedDriver, MenuApp) {
    let mut stalls = stalls;
    let app = MenuApp { log: Arc::new(Mutex::new(vec![])), path: Arc::new(Mutex::new("/".to_string())) };
    let (log, path) = (app.log.clone(), app.path.clone());
    let entries: Vec<(String, String, String)> =
        entries.iter().map(|(r, n, p)| (r.to_string(), n.to_string(), p.to_string())).collect();
    let mut last_css = String::new();
    let mut last_probed = String::new();
    let mut pending: Option<(String, usize)> = None;
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Page.navigate" => {
                let p = v2_lib::autorun::nav::path_of(params["url"].as_str().unwrap_or(""));
                log.lock().unwrap().push(format!("navigate {p}"));
                *path.lock().unwrap() = p;
                pending = None;
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => {
                if let Some((dest, left)) = pending.take() {
                    if left == 0 {
                        *path.lock().unwrap() = dest;
                    } else {
                        pending = Some((dest, left - 1));
                    }
                }
                json!({ "result": { "value": format!("https://hr.example.internal{}", path.lock().unwrap()) } })
            }
            "Runtime.evaluate" => json!({ "result": { "value": null } }),
            "Accessibility.queryAXTree" => {
                let role = params["role"].as_str().unwrap_or("");
                let nodes: Vec<Value> = entries
                    .iter()
                    .enumerate()
                    .filter(|(_, (r, _, _))| r == role)
                    .map(|(i, (r, n, _))| {
                        json!({ "nodeId": format!("n{i}"), "role": { "value": r }, "name": { "value": n }, "backendDOMNodeId": 100 + i })
                    })
                    .collect();
                json!({ "nodes": nodes })
            }
            "DOM.resolveNode" => json!({ "object": { "objectId": format!("ax-{}", params["backendNodeId"]) } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => {
                last_probed = params["objectId"].as_str().unwrap_or("").to_string();
                json!({ "result": { "value": ready_probe() } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => {
                let want = params["arguments"][0]["value"].as_str().unwrap_or("").to_string();
                log.lock().unwrap().push(format!("check {want}"));
                json!({ "result": { "value": want == "yes" } })
            }
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                json!({ "result": [ { "name": "0", "value": { "objectId": format!("css:{last_css}") } } ] })
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                if let Some(css) = last_probed.strip_prefix("css:") {
                    log.lock().unwrap().push(format!("click {css}"));
                    if css == "#go" {
                        *path.lock().unwrap() = landing.to_string();
                    }
                } else if let Some(id) = last_probed.strip_prefix("ax-").and_then(|s| s.parse::<usize>().ok()) {
                    if let Some((_, name, dest)) = id.checked_sub(100).and_then(|i| entries.get(i)) {
                        log.lock().unwrap().push(format!("click {name}"));
                        if stalls > 0 {
                            stalls -= 1;
                        } else if lag == 0 {
                            *path.lock().unwrap() = dest.clone();
                        } else {
                            pending = Some((dest.clone(), lag));
                        }
                    }
                }
                json!({})
            }
            "Page.captureScreenshot" => json!({ "data": "/9j/4AAQ" }),
            _ => json!({}),
        })
    });
    d.on_every_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }) },
    ));
    (d, app)
}

/// A database that answers a flow check from a script and remembers the SQL
/// of every call. An answer is chosen by a marker substring of the SQL
/// (a fixture puts `/*stage-id*/` in each check); the first marker the SQL
/// contains wins. SQL no marker matches comes back as an error, so a test
/// that forgot to script a stage sees "could not run" rather than a
/// silent "not done".
///
/// A clone shares the record of calls, so a test can hand one copy to a
/// handler that takes its database by value and still read what was asked.
#[derive(Clone)]
pub struct FakeStageDb {
    answers: Vec<(String, Result<bool, String>)>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl FakeStageDb {
    pub fn new() -> Self {
        FakeStageDb { answers: Vec::new(), calls: Arc::new(Mutex::new(Vec::new())) }
    }

    pub fn answer(mut self, marker: &str, result: Result<bool, String>) -> Self {
        self.answers.push((marker.to_string(), result));
        self
    }

    /// The SQL of every check asked, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl v2_lib::api_templates::gate::StageDb for FakeStageDb {
    fn label(&self) -> String {
        "fake-server/fake-db".to_string()
    }

    async fn read(&self, sql: &str) -> Result<bool, String> {
        self.calls.lock().unwrap().push(sql.to_string());
        match self.answers.iter().find(|(m, _)| sql.contains(m.as_str())) {
            Some((_, r)) => r.clone(),
            None => Err("FakeStageDb: no answer scripted for this SQL".to_string()),
        }
    }
}

/// The design doc "API template flows" §3 example flow, with `/*stage-id*/`
/// on the end of every check so `FakeStageDb` can tell the checks apart.
/// Competencies is optional.
pub fn cycle_flow_json() -> Value {
    let check = |sql: &str, id: &str| format!("{sql} /*{id}*/");
    json!({
        "id": "pms-performance-cycle",
        "title": "Performance cycle wizard",
        "module": "PMS / Performance Cycle",
        "subject": { "name": "cycleId", "type": "number" },
        "sources": ["Pages/PerformanceCycle/Index.cshtml.cs:40"],
        "stages": [
            { "id": "setup", "title": "Cycle setup", "creates": true,
              "check": check("SELECT 1 FROM PeoplesHR.perf_cycle WHERE cycle_id = {{cycleId}}", "setup") },
            { "id": "rules", "title": "Evaluation rules", "requires": ["setup"],
              "check": check("SELECT 1 FROM PeoplesHR.perf_cycle_step_progress WHERE cycle_id = {{cycleId}} AND step_key = 'EvalRules' AND is_complete = 1", "rules") },
            { "id": "competencies", "title": "Competencies", "requires": ["rules"], "optional": true,
              "check": check("SELECT 1 FROM PeoplesHR.perf_cycle_competency WHERE cycle_id = {{cycleId}}", "competencies") },
            { "id": "participants", "title": "Participants", "requires": ["rules"],
              "check": check("SELECT 1 FROM PeoplesHR.perf_cycle_participant WHERE cycle_id = {{cycleId}}", "participants") },
            { "id": "publish", "title": "Publish", "requires": ["participants"],
              "check": check("SELECT 1 FROM PeoplesHR.perf_cycle WHERE cycle_id = {{cycleId}} AND status = 'Published'", "publish") }
        ]
    })
}

/// A draft template performing `stage` of `cycle_flow_json`'s flow on an
/// existing record: one step, and the `cycleId` number param every such
/// template declares.
pub fn template_on_stage(id: &str, title: &str, stage: &str) -> Value {
    json!({
        "id": id,
        "title": title,
        "module": "PMS / Performance Cycle",
        "effect": "edit",
        "description": format!("{title}, on an existing cycle."),
        "sources": ["Pages/PerformanceCycle/Index.cshtml.cs:120"],
        "antiforgery": { "page": "/hr/pmsv10/performancecycle?mode=edit" },
        "params": [ { "name": "cycleId", "type": "number" } ],
        "steps": [
            { "name": title, "method": "POST",
              "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveStage" },
              "form": { "CycleId": "{{cycleId}}" },
              "expect": { "status": 200, "json": { "success": true } } }
        ],
        "outputs": [],
        "stage": { "flow": "pms-performance-cycle", "id": stage }
    })
}

/// `template_on_stage`, as a template the app has proven and saved.
pub fn saved_on_stage(id: &str, title: &str, stage: &str) -> v2_lib::api_templates::ApiTemplate {
    let mut t: v2_lib::api_templates::ApiTemplate =
        serde_json::from_value(template_on_stage(id, title, stage)).expect("fixture should deserialize");
    t.proven = Some(v2_lib::api_templates::Proven {
        at: "2026-09-01 09:00:00".into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: Default::default(),
        environment: None,
    });
    t
}

/// The live app as a save sees it: every target and `navigate` path the
/// scripts in `body` (a save route body) use, recorded as seen in the
/// discovery map - so a test about another gate saves its new scripts past
/// the seen check. Entries that do not parse as a script are passed over;
/// the route says why.
pub fn see_scripts(root: &std::path::Path, org: &str, project: &str, body: &str) {
    use v2_lib::autorun::discovery_map::{record_matched, record_seen};
    use v2_lib::browser::actions::Action;
    let Ok(v) = serde_json::from_str::<Value>(body) else { return };
    let list = v.get("scripts").cloned().unwrap_or(v);
    let scripts: Vec<v2_lib::autorun::CaseScript> = list
        .as_array()
        .map(|a| a.iter().filter_map(|s| serde_json::from_value(s.clone()).ok()).collect())
        .unwrap_or_default();
    for script in &scripts {
        for action in script.steps.iter().flat_map(|s| s.actions.iter()).flat_map(Action::each) {
            if let Action::Navigate { url } = action {
                record_seen(root, org, project, None, url, "", &[], None, None, 0).unwrap();
            }
            for target in action.targets() {
                record_matched(root, org, project, None, "/", target, 0).unwrap();
            }
        }
    }
}

// ---- a step that used a component, as a run recorded it ----

/// What the component step types into its field: never to be seen in a
/// report.
pub const COMPONENT_TYPED: &str = "s3cret-day-42";

/// "Pick a date": click the field, type the day into it, then Done.
pub fn pick_a_date() -> v2_lib::autorun::components::Component {
    serde_json::from_value(json!({
        "name": "Pick a date", "description": "d", "version": 2,
        "inputs": [
            { "name": "field", "kind": "target", "description": "" },
            { "name": "day", "kind": "text", "description": "" }
        ],
        "actions": [
            { "kind": "click", "selector": { "input": "field" } },
            { "kind": "fill", "selector": { "input": "field" }, "value": "{{day}}" },
            { "kind": "click", "selector": { "text": "Done" } }
        ]
    }))
    .unwrap()
}

pub fn pick_a_date_file() -> v2_lib::autorun::components::ComponentFile {
    v2_lib::autorun::components::ComponentFile { components: vec![pick_a_date()] }
}

/// Case `case_id`'s script: step 3 clicks New, uses "Pick a date", then
/// checks the page says Saved.
pub fn component_script(case_id: i32) -> v2_lib::autorun::CaseScript {
    serde_json::from_value(json!({
        "case_id": case_id, "title": format!("case {case_id}"),
        "steps": [{ "step_number": 3, "actions": [
            { "kind": "click", "selector": "#new" },
            { "kind": "use_component", "component": "Pick a date",
              "inputs": { "field": { "css": "#start" }, "day": COMPONENT_TYPED } },
            { "kind": "check_text", "value": "Saved" }
        ] }]
    }))
    .unwrap()
}

/// Case `case_id` as a run recorded `component_script`'s step 3: the
/// component's typing failed with `typing_failed`, and the check after it
/// failed too.
pub fn component_case(case_id: i32, typing_failed: &str) -> v2_lib::autorun::CaseRecord {
    serde_json::from_value(json!({
        "case_id": case_id, "title": format!("case {case_id}"), "verdict": "", "note": "",
        "proposed": "Failed", "reason": format!("step 3: {typing_failed}"),
        "steps": [{ "step_number": 3, "components": [{ "name": "Pick a date", "version": 2 }], "outcomes": [
            { "ok": true, "detail": "clicked #new" },
            { "ok": true, "detail": "clicked #start", "component": "Pick a date" },
            { "ok": false, "detail": typing_failed, "component": "Pick a date" },
            { "ok": true, "detail": "clicked Done", "component": "Pick a date" },
            { "ok": false, "detail": "page does NOT contain Saved" }
        ] }]
    }))
    .unwrap()
}

pub fn component_run(cases: Vec<v2_lib::autorun::CaseRecord>) -> v2_lib::autorun::LocalRun {
    let mut run: v2_lib::autorun::LocalRun = serde_json::from_value(json!({
        "id": "run-1786000200000", "pbi_id": 42, "started_at": "1786000200000", "mode": "unattended", "cases": []
    }))
    .unwrap();
    run.cases = cases;
    run
}
