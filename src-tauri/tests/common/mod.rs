//! Test doubles shared by the browser tests. Each integration test file is
//! its own crate, so not every file uses every helper.
#![allow(dead_code)]

use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use v2_lib::autorun::accounts::Account;
use v2_lib::autorun::recipe::SignInRecipe;
use v2_lib::browser::actions::{CHECK_TEXT_JS, HIGHLIGHT_JS, RESOLVE_URL_JS};
use v2_lib::browser::cdp::{CdpError, Driver, Event};
use v2_lib::browser::expect::{READ_ATTR_JS, READ_TEXT_JS};
use v2_lib::browser::input::{FOCUS_JS, HAS_FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;
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
        }
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
        self.calls.push((method.to_string(), params.clone()));
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
        self.events.extend(fired);
        (self.handler)(method, &params)
    }

    async fn wait_event(&mut self, method: &str, _limit: Duration) -> Result<Event, CdpError> {
        match self.events.iter().position(|e| e.method == method) {
            Some(i) => Ok(self.events.remove(i).expect("position was just found")),
            None => Err(CdpError::Timeout { what: method.to_string(), ms: 0 }),
        }
    }

    fn forget_events(&mut self) {
        self.events.clear();
    }

    fn take_dialogs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.dialogs)
    }

    fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadlines.push(deadline);
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

/// The password `stateful_app`'s account signs in with. Used by both
/// `autorun_signin.rs` and `autorun_runner.rs` - one stateful fake, never
/// copied.
pub const PASSWORD: &str = "s3cret-Value";

pub fn quick() -> Timing {
    Timing { action_ms: 400, expect_ms: 150, nav_ms: 300, poll_ms: 10, highlight_ms: 0 }
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
