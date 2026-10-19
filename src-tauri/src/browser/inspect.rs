//! Looking at one element without touching it: what `inspect_autorun_element`
//! shows the assistant when it diagnoses a failing step or checks an edge
//! case (a field turning red, a button that only looks disabled).
//!
//! Four parts, any of them: the element's computed styles, its event
//! listeners (and those of the element at its click point when another
//! one covers it), its running animations and transitions, and its
//! structure (tag, attributes, text, children, frame).
//!
//! Read only. Nothing here clicks, types, focuses or scrolls: the click
//! point is measured where the element stands (`input::PROBE_JS` told not
//! to scroll), the listeners come from `DOMDebugger.getEventListeners`
//! and only their type, flags and place are read (never a handler's source,
//! nor an `on<event>` attribute's), and a script's address comes from V8's
//! best-effort coverage, which reads counters and resets nothing. That
//! coverage lists only scripts that have run: a handler written in the
//! page's markup that has not run yet is named as its `on<event>`
//! attribute, and any other it does not list says its address is not known. The `Debugger` domain is never
//! enabled: it would pause the page on a `debugger;` statement.
//!
//! What could hold a secret is hidden on the way out: a password field's
//! value never leaves the page, an attribute named like a secret (and the
//! value of a field named like one) shows as `(hidden)`, and text, values
//! and addresses go through the capture redaction (`capture_redact`).

use super::cdp::{CdpError, Driver};
use super::input::{matched_many, NOT_FOUND, NO_FREE_SPOT, PAGE_REFUSED, PROBE_JS};
use super::locator::{resolve_explained, Target};
use super::page::{self, Handle};
use crate::autorun::capture_redact::{redact_path_query, redact_text, secret_name};
use crate::browser::net_capture::HIDDEN;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// The most characters of an element's text shown.
pub const TEXT_MAX: usize = 500;

/// The most characters of an element's text read out of the page: more
/// than is shown, so the redaction judges a token whole before the cut.
const TEXT_READ: usize = 4000;

/// The most listeners listed for one element, and running animations.
const MAX_LISTED: usize = 100;

/// Said for a listener whose script V8's coverage did not list.
pub const SCRIPT_NOT_KNOWN: &str = "(address not known)";

/// Said for a listener whose script has no address: an `onclick="..."`
/// attribute, or code the page built with `eval` or `new Function`.
pub const SCRIPT_NO_ADDRESS: &str = "(no address: an inline handler or a script the page built)";

/// One of the four things an inspect can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Styles,
    Listeners,
    Animations,
    Structure,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Styles, Part::Listeners, Part::Animations, Part::Structure];

    pub fn word(self) -> &'static str {
        match self {
            Part::Styles => "styles",
            Part::Listeners => "listeners",
            Part::Animations => "animations",
            Part::Structure => "structure",
        }
    }

    fn from_word(word: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|p| p.word().eq_ignore_ascii_case(word.trim()))
    }
}

/// What an inspect was asked to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub parts: Vec<Part>,
    /// Only these computed properties; every one when empty.
    pub properties: Vec<String>,
}

const SHOW_SAYS: &str =
    "show is a list of any of: styles, listeners, animations, structure (all four when left out)";
const PROPERTIES_SAY: &str =
    "properties is a list of CSS property names, such as [\"color\", \"border-color\"] (every computed value when left out)";

/// `show` and `properties` out of an inspect's arguments.
pub fn read_ask(args: &Value) -> Result<Ask, String> {
    let parts = match args.get("show") {
        None | Some(Value::Null) => Part::ALL.to_vec(),
        Some(Value::Array(items)) if !items.is_empty() => {
            let mut parts = Vec::new();
            for item in items {
                let part = item.as_str().and_then(Part::from_word).ok_or_else(|| SHOW_SAYS.to_string())?;
                if !parts.contains(&part) {
                    parts.push(part);
                }
            }
            parts
        }
        _ => return Err(SHOW_SAYS.to_string()),
    };
    let properties = match args.get("properties") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| i.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| PROPERTIES_SAY.to_string())?,
        _ => return Err(PROPERTIES_SAY.to_string()),
    };
    Ok(Ask { parts, properties })
}

/// `this` is the element. Argument: the property names asked for, or an
/// empty list for every computed one. Name to value, read from the
/// element's own window (an element in a frame is styled by the frame).
pub const STYLES_JS: &str = r#"function(names) {
  const cs = this.ownerDocument.defaultView.getComputedStyle(this);
  const out = {};
  if (names && names.length) {
    for (const n of names) out[n] = cs.getPropertyValue(n);
  } else {
    for (let i = 0; i < cs.length; i++) out[cs[i]] = cs.getPropertyValue(cs[i]);
  }
  return out;
}"#;

/// `this` is the element. Its own running animations and transitions
/// (`getAnimations`, which only reads), and every computed `transition-*`
/// and `animation-*` value.
pub const ANIMATIONS_JS: &str = r#"function() {
  const cs = this.ownerDocument.defaultView.getComputedStyle(this);
  const count = (v) => (v === Infinity ? 'infinite' : v);
  const running = this.getAnimations().slice(0, 100).map((a) => {
    const t = a.effect && a.effect.getTiming ? a.effect.getTiming() : {};
    return {
      kind: (a.constructor && a.constructor.name) || 'Animation',
      name: a.animationName || a.transitionProperty || a.id || '',
      duration: t.duration === undefined ? 0 : t.duration,
      delay: t.delay || 0,
      iterations: count(t.iterations === undefined ? 1 : t.iterations),
      play_state: a.playState,
    };
  });
  const computed = {};
  for (let i = 0; i < cs.length; i++) {
    const n = cs[i];
    if (/^(transition|animation)-/.test(n)) computed[n] = cs.getPropertyValue(n);
  }
  return { running, computed };
}"#;

/// `this` is the element. Argument: how many characters of its text to
/// read. Its tag, its attributes as [name, value] (a password field's
/// `value` as null: what was typed there never leaves the page), its text,
/// its children counted by tag, the frames it sits in (outermost first)
/// and its document's path.
pub const STRUCTURE_JS: &str = r#"function(max) {
  const password = this.tagName === 'INPUT' && String(this.type).toLowerCase() === 'password';
  const attributes = Array.from(this.attributes || []).map((a) =>
    [a.name, password && a.name.toLowerCase() === 'value' ? null : a.value]);
  const raw = password ? '' : (this.innerText !== undefined ? this.innerText : this.textContent);
  const text = String(raw || '').replace(/\s+/g, ' ').trim().slice(0, max);
  const children = {};
  for (const c of Array.from(this.children || [])) {
    const t = c.tagName.toLowerCase();
    children[t] = (children[t] || 0) + 1;
  }
  const say = (e) => e.tagName.toLowerCase() + (e.id ? '#' + e.id : '') +
    (e.getAttribute('name') ? '[name=' + e.getAttribute('name') + ']' : '');
  const frames = [];
  let w = this.ownerDocument.defaultView;
  while (w && w.frameElement) { frames.unshift(say(w.frameElement)); w = w.parent; }
  return { tag: this.tagName.toLowerCase(), attributes, text, children, frames, path: this.ownerDocument.location.pathname };
}"#;

/// `this` is the top document. Arguments: a point in the top window, as
/// `input::PROBE_JS` gives the click point. The deepest element there,
/// walking into each same-origin frame the point lands on (shifted by the
/// frame's place, border and padding, as the probe shifts it): what a
/// click at that point would land on.
pub const COVER_AT_JS: &str = r#"function(x, y) {
  let e = this.elementFromPoint(x, y);
  for (let depth = 0; depth < 4 && e && (e.tagName === 'IFRAME' || e.tagName === 'FRAME'); depth++) {
    let inner = null;
    try { inner = e.contentDocument; } catch (err) { inner = null; }
    if (!inner) break;
    const r = e.getBoundingClientRect();
    const cs = e.ownerDocument.defaultView.getComputedStyle(e);
    x -= r.left + e.clientLeft + (parseFloat(cs.paddingLeft) || 0);
    y -= r.top + e.clientTop + (parseFloat(cs.paddingTop) || 0);
    const next = inner.elementFromPoint(x, y);
    if (!next) break;
    e = next;
  }
  return e ? [e] : [];
}"#;

/// Why an inspect stopped: a refusal for the assistant, or the browser.
enum Stop {
    Refused(u16, String),
    Browser(CdpError),
}

impl From<CdpError> for Stop {
    fn from(e: CdpError) -> Self {
        Stop::Browser(e)
    }
}

/// Inspect the one element `target` matches on the page as it is now. The
/// locator is resolved as a try resolves it: nothing found and more than
/// one found are refused in the try's own words. `Err` is the status and
/// the sentence: 404 not found, 409 matched many or the page refused, 503
/// the browser did not answer.
pub async fn inspect<D: Driver>(d: &mut D, target: &Target, ask: &Ask) -> Result<Value, (u16, String)> {
    page::release(d).await;
    let out = look(d, target, ask).await;
    page::release(d).await;
    out.map_err(|stop| match stop {
        Stop::Refused(status, why) => (status, why),
        Stop::Browser(CdpError::Protocol { message, .. }) => (409, format!("{PAGE_REFUSED}{message}")),
        Stop::Browser(CdpError::Tab(sentence)) => (409, sentence),
        Stop::Browser(e) => (503, format!("the browser did not answer: {e}")),
    })
}

async fn look<D: Driver>(d: &mut D, target: &Target, ask: &Ask) -> Result<Value, Stop> {
    let described = target.describe();
    let found = resolve_explained(d, target).await?;
    let mut handles = found.handles;
    if handles.is_empty() {
        let why = found.unreachable_frame.unwrap_or_else(|| format!("{described} {NOT_FOUND}"));
        return Err(Stop::Refused(404, why));
    }
    if handles.len() > 1 {
        return Err(Stop::Refused(409, format!("{described} {}", matched_many(handles.len()))));
    }
    let el = handles.remove(0);
    let mut out = Map::new();
    out.insert("element".into(), json!(described));
    // A name is the page's words: a token in one is hidden like any text.
    let named = super::snapshot::page_read_label(d, &el).await.map(|l| redact_text(&l));
    out.insert("named".into(), json!(named));

    // The click point, measured where the element stands: never settled
    // across frames, never scrolled to.
    let p = page::call_value(d, &el, PROBE_JS, &[json!(false), json!(true)]).await?;
    let flag = |k: &str| p[k].as_bool().unwrap_or(false);
    let mut cover: Option<(String, Option<Handle>)> = None;
    let point = if !flag("visible") {
        "not checked: the element is not visible".to_string()
    } else if !flag("onscreen") {
        "not checked: the element is outside the visible part of the page, and an inspect never scrolls".to_string()
    } else if flag("nospot") {
        format!("the element {NO_FREE_SPOT}")
    } else if flag("hit") {
        "a click at its centre reaches it".to_string()
    } else {
        let (x, y) = (p["x"].as_f64().unwrap_or(0.0), p["y"].as_f64().unwrap_or(0.0));
        let said = redact_text(p["covered_by"].as_str().filter(|s| !s.is_empty()).unwrap_or("another element"));
        let found = cover_at(d, &el, x, y).await?;
        let name = match &found {
            Some(h) => match super::snapshot::page_read_label(d, h).await {
                Some(label) => format!("{} ({said})", redact_text(&label)),
                None => said,
            },
            None => said,
        };
        let sentence = format!("a click at its centre lands on {name}");
        cover = Some((name, found));
        sentence
    };
    out.insert("click_point".into(), json!(point));
    out.insert("covered_by".into(), cover.as_ref().map_or(Value::Null, |(name, _)| json!(name)));

    for part in &ask.parts {
        let shown = match part {
            Part::Styles => {
                let raw = page::call_value(d, &el, STYLES_JS, &[json!(ask.properties)]).await?;
                Value::Object(shown_values(&raw))
            }
            Part::Listeners => listeners(d, &el, cover.as_ref()).await?,
            Part::Animations => {
                let raw = page::call_value(d, &el, ANIMATIONS_JS, &[]).await?;
                shown_animations(&raw)
            }
            Part::Structure => {
                let raw = page::call_value(d, &el, STRUCTURE_JS, &[json!(TEXT_READ)]).await?;
                shown_structure(&raw)
            }
        };
        out.insert(part.word().into(), shown);
    }
    out.insert(
        "note".into(),
        json!("Read only: nothing was clicked, typed, focused or scrolled. `listeners.element` are the element's own; `listeners.ancestors` counts the click, pointer, mouse, touch and submit listeners above it, up to the document and the window - where a page that delegates events (React, jQuery's .on with a selector) listens."),
    );
    Ok(Value::Object(out))
}

/// The element a click at (`x`, `y`) lands on (`COVER_AT_JS`), unless it
/// is `el` itself.
async fn cover_at<D: Driver>(d: &mut D, el: &Handle, x: f64, y: f64) -> Result<Option<Handle>, CdpError> {
    let doc = page::document(d).await?;
    let Some(cover) = page::call_elements(d, &doc, COVER_AT_JS, &[json!(x), json!(y)]).await?.into_iter().next() else {
        return Ok(None);
    };
    let same = page::backend_id(d, &cover).await? == page::backend_id(d, el).await?;
    Ok((!same).then_some(cover))
}

/// Name to value, each value through the redaction.
fn shown_values(raw: &Value) -> Map<String, Value> {
    raw.as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), json!(redact_text(v.as_str().unwrap_or("")))))
        .collect()
}

/// The element's listeners, and the cover's when one was found: each with
/// its flags and where its handler was written, as a path with no host
/// and no query, and a line and column counted from 1.
async fn listeners<D: Driver>(
    d: &mut D,
    el: &Handle,
    cover: Option<&(String, Option<Handle>)>,
) -> Result<Value, CdpError> {
    let own = listeners_of(d, el).await?;
    let theirs = match cover {
        Some((name, Some(h))) => Some((name.clone(), h.clone(), listeners_of(d, h).await?)),
        _ => None,
    };
    let any_script = own.iter().chain(theirs.iter().flat_map(|(_, _, l)| l.iter())).any(|l| l["scriptId"].is_string());
    let scripts = if any_script { script_addresses(d).await } else { HashMap::new() };
    let mut out = Map::new();
    out.insert("element".into(), json!(shown_listeners(d, el, &own, &scripts).await?));
    if let Some((name, h, list)) = theirs {
        out.insert("cover".into(), json!({ "name": name, "listeners": shown_listeners(d, &h, &list, &scripts).await? }));
    }
    let above = ancestor_listeners(d, el).await?;
    let capturing: Vec<String> = above
        .iter()
        .filter(|a| a["listeners"].as_array().into_iter().flatten().any(|l| l["capture"] == true))
        .filter_map(|a| a["name"].as_str().map(str::to_string))
        .collect();
    out.insert("ancestors".into(), json!(above));
    if !capturing.is_empty() {
        out.insert("ancestors_note".into(), json!(capture_above(&capturing)));
    }
    Ok(Value::Object(out))
}

/// Said when an element has capture-phase listeners above it (`names`,
/// nearest first).
pub fn capture_above(names: &[String]) -> String {
    format!(
        "Capture-phase listeners above the element run before its own: {}. One that stops the event (stopPropagation) or cancels it (preventDefault) is the usual reason a click does nothing.",
        names.join(", ")
    )
}

/// The most ancestors looked at, nearest first, besides the window.
const MAX_ANCESTORS: usize = 64;

/// `this` is the element. Every node above it, nearest first, up to its
/// document (through a shadow root to its host), then its window.
pub const ANCESTORS_JS: &str = r#"function() {
  const out = [];
  let p = this.parentNode;
  while (p && out.length < 64) {
    if (p.nodeType === 11) { p = p.host; continue; }
    out.push(p);
    p = p.parentNode;
  }
  out.push(this.ownerDocument.defaultView);
  return out;
}"#;

/// `this` is an ancestor: `tag#id`, or `tag.class` when it has no id, or
/// `document` / `window`.
pub const NODE_NAME_JS: &str = r#"function() {
  if (this.window === this) return 'window';
  if (this.nodeType === 9) return 'document';
  const tag = String(this.tagName || this.nodeName || '').toLowerCase();
  if (this.id) return tag + '#' + this.id;
  const cls = typeof this.className === 'string' ? this.className.trim().split(/\s+/).filter(Boolean)[0] : '';
  return cls ? tag + '.' + cls : tag;
}"#;

/// The event types counted on the element's ancestors: those a click
/// passes through.
fn click_kind(t: &str) -> bool {
    matches!(t, "click" | "dblclick" | "auxclick" | "submit")
        || t.starts_with("pointer")
        || t.starts_with("mouse")
        || t.starts_with("touch")
}

/// The ancestors of `el` (up to the document, and the window) that listen
/// for a click kind of event (`click_kind`), nearest first: each named, with
/// each type and capture flag counted. Handlers are released and never
/// read, as the element's own are.
async fn ancestor_listeners<D: Driver>(d: &mut D, el: &Handle) -> Result<Vec<Value>, CdpError> {
    let above = page::call_elements(d, el, ANCESTORS_JS, &[]).await?;
    let mut out = Vec::new();
    for h in above.iter().take(MAX_ANCESTORS + 1) {
        let r = d.call("DOMDebugger.getEventListeners", json!({ "objectId": h, "objectGroup": page::GROUP })).await?;
        let mut counts: std::collections::BTreeMap<(String, bool), usize> = std::collections::BTreeMap::new();
        for l in r["listeners"].as_array().into_iter().flatten() {
            let kind = l["type"].as_str().unwrap_or("");
            if click_kind(kind) {
                *counts.entry((kind.to_string(), l["useCapture"].as_bool().unwrap_or(false))).or_default() += 1;
            }
        }
        if counts.is_empty() {
            continue;
        }
        let name = page::call_value(d, h, NODE_NAME_JS, &[]).await?;
        let listeners: Vec<Value> = counts
            .into_iter()
            .map(|((kind, capture), count)| json!({ "type": kind, "capture": capture, "count": count }))
            .collect();
        out.push(json!({ "name": redact_text(name.as_str().unwrap_or("an ancestor")), "listeners": listeners }));
    }
    Ok(out)
}

/// One node's own listeners. Chrome hands each handler back as an object
/// (its source in the description): they go in the app's object group, so
/// the inspect's release lets them go, and nothing of them is ever read.
async fn listeners_of<D: Driver>(d: &mut D, on: &Handle) -> Result<Vec<Value>, CdpError> {
    let r = d.call("DOMDebugger.getEventListeners", json!({ "objectId": on, "objectGroup": page::GROUP })).await?;
    Ok(r["listeners"].as_array().into_iter().flatten().take(MAX_LISTED).cloned().collect())
}

/// `this` is the element. Argument: event types. Those the element carries
/// an `on<type>` attribute for: a handler written in the page's markup,
/// which V8 lists no script for until it has run.
pub const ATTRIBUTE_HANDLERS_JS: &str = r#"function(types) {
  return types.filter((t) => typeof t === 'string' && this.hasAttribute('on' + t));
}"#;

/// `list` (the listeners of `on`) as shown: a listener whose script V8 does
/// not list is named as the node's `on<type>` attribute when it has one.
async fn shown_listeners<D: Driver>(
    d: &mut D,
    on: &Handle,
    list: &[Value],
    scripts: &HashMap<String, String>,
) -> Result<Vec<Value>, CdpError> {
    let unknown: Vec<&str> = list
        .iter()
        .filter(|l| !l["scriptId"].as_str().is_some_and(|id| scripts.contains_key(id)))
        .filter_map(|l| l["type"].as_str())
        .collect();
    let in_markup: Vec<String> = if unknown.is_empty() {
        Vec::new()
    } else {
        page::call_value(d, on, ATTRIBUTE_HANDLERS_JS, &[json!(unknown)])
            .await?
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_string))
            .collect()
    };
    Ok(list.iter().map(|l| shown_listener(l, scripts, &in_markup)).collect())
}

/// Script id to address, from V8's best-effort coverage: a read of its
/// counters that resets nothing and pauses nothing. A browser that wants
/// the profiler on first has it on for the one read. Empty when it cannot
/// be read: the listeners then say their address is not known.
async fn script_addresses<D: Driver>(d: &mut D) -> HashMap<String, String> {
    const COVERAGE: &str = "Profiler.getBestEffortCoverage";
    let r = match d.call(COVERAGE, json!({})).await {
        Ok(r) => r,
        Err(CdpError::Protocol { message, .. }) if message.to_lowercase().contains("enable") => {
            if d.call("Profiler.enable", json!({})).await.is_err() {
                return HashMap::new();
            }
            let r = d.call(COVERAGE, json!({})).await;
            let _ = d.call("Profiler.disable", json!({})).await;
            match r {
                Ok(r) => r,
                Err(_) => return HashMap::new(),
            }
        }
        Err(_) => return HashMap::new(),
    };
    r["result"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some((s["scriptId"].as_str()?.to_string(), s["url"].as_str().unwrap_or("").to_string())))
        .collect()
}

fn shown_listener(raw: &Value, scripts: &HashMap<String, String>, in_markup: &[String]) -> Value {
    let kind = raw["type"].as_str().unwrap_or("");
    let script = match raw["scriptId"].as_str().and_then(|id| scripts.get(id)) {
        None if in_markup.iter().any(|t| t == kind) => format!("(its on{} attribute)", redact_text(kind)),
        None => SCRIPT_NOT_KNOWN.to_string(),
        Some(url) if url.trim().is_empty() => SCRIPT_NO_ADDRESS.to_string(),
        Some(url) => redact_path_query(&super::actions::path_only(url)),
    };
    let from_one = |k: &str| raw[k].as_u64().map_or(Value::Null, |n| json!(n + 1));
    json!({
        "type": redact_text(kind),
        "use_capture": raw["useCapture"].as_bool().unwrap_or(false),
        "once": raw["once"].as_bool().unwrap_or(false),
        "passive": raw["passive"].as_bool().unwrap_or(false),
        "script": script,
        "line": from_one("lineNumber"),
        "column": from_one("columnNumber"),
    })
}

fn shown_animations(raw: &Value) -> Value {
    let running: Vec<Value> = raw["running"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_LISTED)
        .map(|a| {
            json!({
                "kind": a["kind"].as_str().unwrap_or("Animation"),
                "name": redact_text(a["name"].as_str().unwrap_or("")),
                "duration": a["duration"].clone(),
                "delay": a["delay"].clone(),
                "iterations": a["iterations"].clone(),
                "play_state": a["play_state"].as_str().unwrap_or(""),
            })
        })
        .collect();
    json!({ "running": running, "computed": shown_values(&raw["computed"]) })
}

/// Said for an `on<event>` attribute's value: a handler's source is never
/// shown, in the structure as in the listeners.
pub const HANDLER_NOT_SHOWN: &str = "(a handler: its source is not shown)";

/// `onclick`, `onchange`...: an attribute that holds a handler's source.
fn is_handler_attribute(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.len() > 2 && lower.starts_with("on") && lower[2..].chars().all(|c| c.is_ascii_alphanumeric())
}

/// The element's structure as the assistant sees it: an attribute named
/// like a secret hidden, an `on<event>` attribute's handler not shown, and the `value` of a password field or of a field
/// named like a secret (its name, id or autocomplete); every other value,
/// the text (cut to `TEXT_MAX`) and the frames through the redaction.
fn shown_structure(raw: &Value) -> Value {
    let attrs: Vec<(String, Option<String>)> = raw["attributes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some((a[0].as_str()?.to_string(), a[1].as_str().map(str::to_string))))
        .collect();
    let attr = |name: &str| attrs.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).and_then(|(_, v)| v.as_deref());
    let password = attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("password"));
    let secret_field = ["name", "id", "autocomplete"].iter().any(|n| attr(n).is_some_and(secret_name));
    let attributes: Map<String, Value> = attrs
        .iter()
        .map(|(name, value)| {
            let hide = secret_name(name)
                || value.is_none()
                || (name.eq_ignore_ascii_case("value") && (password || secret_field));
            let shown = if hide {
                HIDDEN.to_string()
            } else if is_handler_attribute(name) {
                HANDLER_NOT_SHOWN.to_string()
            } else {
                redact_text(value.as_deref().unwrap_or(""))
            };
            (name.clone(), json!(shown))
        })
        .collect();
    let text = redact_text(raw["text"].as_str().unwrap_or(""));
    let text = if text.chars().count() > TEXT_MAX {
        format!("{}...", text.chars().take(TEXT_MAX).collect::<String>())
    } else {
        text
    };
    let frames: Vec<String> =
        raw["frames"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(redact_text)).collect();
    let frame = if frames.is_empty() {
        "the page itself".to_string()
    } else {
        format!("inside {}, showing {}", frames.join(" > "), redact_path_query(raw["path"].as_str().unwrap_or("")))
    };
    let children: Map<String, Value> = raw["children"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), json!(v.as_u64().unwrap_or(0))))
        .collect();
    json!({
        "tag": raw["tag"].as_str().unwrap_or(""),
        "attributes": attributes,
        "text": text,
        "children": children,
        "frame": frame,
    })
}
