//! Turning a person's clicks in a real browser into the locators a run
//! clicks with.
//!
//! A click listener, added in the capture phase to every document through
//! a DevTools binding, reports each click: a handle on the element itself
//! (kept in the page), and what could be read on the spot - tag, role
//! attribute, aria-label, visible text - for when the click has already
//! taken the page somewhere else. The recorder asks Chrome's accessibility
//! tree for the role and name of the clicked element, or of its nearest
//! ancestor that has one, preferring the roles a menu is made of; when
//! that is not possible it builds the locator from the hints.
//!
//! Which locator to build is decided by plain functions, tested without a
//! browser. Nothing here saves anything: the command saves a path only
//! after it has replayed in a fresh browser.

use super::nav::{self, ModulePath};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::locator::{LocatorStep, Target};
use crate::browser::page;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The function the page calls to report a click: `window.<BINDING>(json)`.
pub const BINDING: &str = "__tcmRecordClick";

/// Added to every new document, and run once on the current one. The
/// clicked element is kept in the page (`__tcmRecHeld`) so the recorder can
/// ask the accessibility tree about it; `doc` tells an index into this
/// document's list apart from the same index on the next document. It only
/// listens: it never stops or changes a click. It reports words a locator
/// can use and nothing a person typed: a field's value is never read, and
/// a click in a field or an editable area sends no text at all.
pub const LISTENER_JS: &str = r#"(() => {
  if (window.__tcmRecArmed) return;
  window.__tcmRecArmed = true;
  const doc = Math.random().toString(36).slice(2) + Date.now().toString(36);
  const held = [];
  window.__tcmRecDoc = doc;
  window.__tcmRecHeld = held;
  const clean = (s) => String(s || '').replace(/\s+/g, ' ').trim().slice(0, 200);
  document.addEventListener('click', (e) => {
    if (!e.isTrusted) return;
    const t = e.target;
    const el = t instanceof Element ? t : (t && t.parentElement);
    if (!el) return;
    held.push(el);
    const near = el.closest('a,button,summary,[role]') || el;
    const typed = el.isContentEditable || near.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName);
    const payload = {
      doc: doc,
      i: held.length - 1,
      tag: near.tagName.toLowerCase(),
      role: near.getAttribute('role') || '',
      label: clean(near.getAttribute('aria-label')),
      text: typed ? '' : clean(near.innerText || near.textContent),
    };
    try { window.__tcmRecordClick(JSON.stringify(payload)); } catch (_) {}
  }, true);
})()"#;

/// `this` is the document. Arguments: the document token, the index. The
/// element as a one-item list, or an empty list when it is gone.
pub const HELD_JS: &str = r#"function(doc, i) {
  const held = window.__tcmRecHeld;
  if (window.__tcmRecDoc !== doc || !held) return [];
  const el = held[i];
  return el && el.isConnected ? [el] : [];
}"#;

/// The roles a menu is made of, preferred over anything else near a click.
pub const PREFERRED_ROLES: [&str; 5] = ["link", "button", "menuitem", "tab", "treeitem"];

/// Roles that carry words but are not something a person clicks by name.
const NOT_A_TARGET: [&str; 8] =
    ["generic", "none", "presentation", "StaticText", "InlineTextBox", "LineBreak", "paragraph", "listitem"];

/// The climb from a click stops at these: past one, a name would describe
/// a whole region, not what was clicked.
const CONTAINERS: [&str; 16] = [
    "RootWebArea", "WebArea", "main", "navigation", "menubar", "menu", "tablist", "tree", "dialog", "banner",
    "contentinfo", "form", "region", "document", "application", "list",
];

const MAX_CLIMB: usize = 6;
const MAX_HINT_TEXT: usize = 80;
const POLL: Duration = Duration::from_millis(250);

pub const UNREADABLE: &str = "that click could not be named - click the words of the menu entry itself";
pub const BROWSER_CLOSED: &str = "the recording browser was closed - nothing was saved";
pub const CANCELLED: &str = "the recording was cancelled - nothing was saved";
pub const NO_CLICKS: &str = "no clicks were recorded - click through the menu in the recording browser, then press Stop";

/// What the listener read on the spot.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
#[serde(default)]
pub struct ClickHints {
    pub tag: String,
    pub role: String,
    pub label: String,
    pub text: String,
}

/// One click as the page reported it.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ClickPayload {
    pub doc: String,
    pub i: u32,
    #[serde(flatten)]
    pub hints: ClickHints,
}

/// One accessibility node on the way up from a click.
#[derive(Debug, Clone, PartialEq)]
pub struct AxLink {
    pub role: String,
    pub name: String,
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// Stop was pressed; where the page was at that moment.
    Stopped { href: String },
    Cancelled,
    /// The recording browser went away.
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Captured {
    pub clicks: Vec<Target>,
    pub ended: Ended,
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `exact`: a menu has "Leave" and "Apply Leave" side by side, and a
/// contains-match on the first would find both.
fn exact_role(role: &str, name: &str) -> Target {
    Target::One(LocatorStep {
        role: Some(role.to_string()),
        name: Some(collapse(name)),
        exact: true,
        ..LocatorStep::default()
    })
}

/// The clicked node first, then each ancestor, from
/// `Accessibility.getPartialAXTree`'s answer. Parents come from `parentId`
/// where Chrome gives one, else from the `childIds` that name the node.
pub fn ax_chain(tree: &Value, backend: i64) -> Vec<AxLink> {
    let nodes: Vec<&Value> = tree["nodes"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let by_id: HashMap<String, &Value> =
        nodes.iter().filter_map(|n| n["nodeId"].as_str().map(|id| (id.to_string(), *n))).collect();
    let mut parent_of: HashMap<String, String> = HashMap::new();
    for n in &nodes {
        let Some(id) = n["nodeId"].as_str() else { continue };
        for child in n["childIds"].as_array().into_iter().flatten() {
            if let Some(c) = child.as_str() {
                parent_of.entry(c.to_string()).or_insert_with(|| id.to_string());
            }
        }
    }
    let mut out = vec![];
    let mut seen = HashSet::new();
    let mut current = nodes.iter().copied().find(|n| n["backendDOMNodeId"].as_i64() == Some(backend));
    while let Some(n) = current {
        let id = n["nodeId"].as_str().unwrap_or("").to_string();
        // A tree that names a node as its own ancestor would climb forever.
        if !seen.insert(id.clone()) {
            break;
        }
        out.push(AxLink {
            role: n["role"]["value"].as_str().unwrap_or("").to_string(),
            name: n["name"]["value"].as_str().unwrap_or("").to_string(),
            ignored: n["ignored"].as_bool().unwrap_or(false),
        });
        let parent = n["parentId"].as_str().map(str::to_string).or_else(|| parent_of.get(&id).cloned());
        current = parent.and_then(|p| by_id.get(&p).copied());
    }
    out
}

/// The nearest node with a preferred role and a name; else the nearest
/// named node whose role a person clicks by; never past a container.
pub fn locator_from_ax(chain: &[AxLink]) -> Option<Target> {
    let near: Vec<&AxLink> =
        chain.iter().take(MAX_CLIMB).take_while(|n| !CONTAINERS.contains(&n.role.as_str())).collect();
    let usable = |n: &AxLink| !n.ignored && !collapse(&n.name).is_empty();
    if let Some(n) = near.iter().find(|n| usable(n) && PREFERRED_ROLES.contains(&n.role.as_str())) {
        return Some(exact_role(&n.role, &n.name));
    }
    near.iter()
        .find(|n| usable(n) && !NOT_A_TARGET.contains(&n.role.as_str()))
        .map(|n| exact_role(&n.role, &n.name))
}

/// When the element is gone or has no role nearby: a role attribute with an
/// aria-label, else the visible words (short ones only).
pub fn locator_from_hints(h: &ClickHints) -> Option<Target> {
    let role = h.role.trim();
    let label = collapse(&h.label);
    if !role.is_empty() && !label.is_empty() {
        return Some(exact_role(role, &label));
    }
    let text = collapse(&h.text);
    if !text.is_empty() && text.chars().count() <= MAX_HINT_TEXT {
        return Some(Target::One(LocatorStep { text: Some(text), exact: true, ..LocatorStep::default() }));
    }
    None
}

/// Start listening: the binding, the listener on every new document, and
/// the listener on the document already open.
pub async fn arm<D: Driver>(d: &mut D) -> Result<(), CdpError> {
    d.call("Runtime.enable", json!({})).await?;
    d.call("Runtime.addBinding", json!({ "name": BINDING })).await?;
    d.call("Page.addScriptToEvaluateOnNewDocument", json!({ "source": LISTENER_JS })).await?;
    page::eval_value(d, LISTENER_JS).await?;
    Ok(())
}

/// The next click the page reported, waiting up to `wait`. `Ok(None)` when
/// there was none (or it was another binding's call).
pub async fn next_click<D: Driver>(d: &mut D, wait: Duration) -> Result<Option<ClickPayload>, CdpError> {
    match d.wait_event("Runtime.bindingCalled", wait).await {
        Ok(ev) => {
            if ev.params["name"].as_str() != Some(BINDING) {
                return Ok(None);
            }
            Ok(serde_json::from_str::<ClickPayload>(ev.params["payload"].as_str().unwrap_or("")).ok())
        }
        Err(CdpError::Timeout { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `Accessibility.getPartialAXTree` around one node, retried once after
/// `Accessibility.enable` - real Edge answers the first call that way.
async fn ax_around<D: Driver>(d: &mut D, backend: i64) -> Result<Value, CdpError> {
    let params = json!({ "backendNodeId": backend, "fetchRelatives": true });
    match d.call("Accessibility.getPartialAXTree", params.clone()).await {
        Ok(v) => Ok(v),
        Err(CdpError::Protocol { message, .. }) if message.to_lowercase().contains("enable") => {
            d.call("Accessibility.enable", json!({})).await?;
            d.call("Accessibility.getPartialAXTree", params).await
        }
        Err(e) => Err(e),
    }
}

async fn from_the_element<D: Driver>(d: &mut D, click: &ClickPayload) -> Result<Option<Target>, CdpError> {
    page::release(d).await;
    let doc = page::document(d).await?;
    let found = page::call_elements(d, &doc, HELD_JS, &[json!(click.doc), json!(click.i)]).await?;
    let Some(el) = found.first() else { return Ok(None) };
    let backend = page::backend_id(d, el).await?;
    let tree = ax_around(d, backend).await?;
    Ok(locator_from_ax(&ax_chain(&tree, backend)))
}

/// The locator for one reported click: from the accessibility tree when
/// the element is still there, else from the hints.
pub async fn locate<D: Driver>(d: &mut D, click: &ClickPayload) -> Result<Target, String> {
    if let Ok(Some(t)) = from_the_element(d, click).await {
        return Ok(t);
    }
    locator_from_hints(&click.hints).ok_or_else(|| UNREADABLE.to_string())
}

/// Where the page is, allowing it a moment if it is between documents.
async fn where_now<D: Driver>(d: &mut D) -> Result<String, CdpError> {
    let mut last = CdpError::Closed;
    for _ in 0..10 {
        match page::eval_value(d, "location.href").await {
            Ok(v) => return Ok(v.as_str().unwrap_or("").to_string()),
            Err(e) if e.is_transient() => {
                last = e;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Collect clicks until `cancel`, a closed browser, or `stop` - clicks
/// already reported when Stop is pressed are kept. `on_click` hears each
/// one as it is named (or why it could not be).
pub async fn capture<D: Driver>(
    d: &mut D,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    on_click: &mut (dyn FnMut(Result<&Target, &str>) + Send),
) -> Captured {
    let mut clicks: Vec<Target> = vec![];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Captured { clicks, ended: Ended::Cancelled };
        }
        match next_click(d, POLL).await {
            Ok(Some(click)) => {
                match locate(d, &click).await {
                    Ok(t) => {
                        on_click(Ok(&t));
                        clicks.push(t);
                    }
                    Err(why) => on_click(Err(&why)),
                }
                // Another click may already be waiting: Stop keeps it.
                continue;
            }
            Err(CdpError::Closed) | Err(CdpError::Transport(_)) => return Captured { clicks, ended: Ended::Closed },
            Ok(None) => {}
            // A refusal while waiting: Stop must still be heard, or a
            // browser that keeps refusing would hold the recording open -
            // and the pause keeps that from spinning.
            Err(_) => tokio::time::sleep(POLL).await,
        }
        if stop.load(Ordering::SeqCst) {
            let ended = match where_now(d).await {
                Ok(href) => Ended::Stopped { href },
                Err(CdpError::Closed) | Err(CdpError::Transport(_)) => Ended::Closed,
                Err(_) => Ended::Stopped { href: String::new() },
            };
            return Captured { clicks, ended };
        }
    }
}

/// A finished recording as a path to check, or why there is none.
pub fn finish(module: &str, captured: Captured, recorded: &str) -> Result<ModulePath, String> {
    match captured.ended {
        Ended::Cancelled => Err(CANCELLED.to_string()),
        Ended::Closed => Err(BROWSER_CLOSED.to_string()),
        Ended::Stopped { href } => {
            if captured.clicks.is_empty() {
                return Err(NO_CLICKS.to_string());
            }
            Ok(ModulePath {
                module: module.trim().to_string(),
                clicks: captured.clicks,
                arrived: nav::path_of(&href),
                recorded: recorded.to_string(),
            })
        }
    }
}
