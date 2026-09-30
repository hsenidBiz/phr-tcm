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
  const norm = (s) => String(s || '').replace(/\s+/g, ' ').trim();
  const skip = /^(SCRIPT|STYLE|HEAD|HTML|BODY|NOSCRIPT|TEMPLATE)$/;
  // The runner's text rule (locator.rs TEXT_JS), exact and visible only:
  // the deepest visible elements whose words are exactly these. It leaves
  // out TEXT_JS's button-input branch, which reads a field's value - a
  // click's words here come from innerText, which such an input has none of.
  const matches = (root, want) => {
    const lower = want.toLowerCase();
    const all = Array.from(root.querySelectorAll('*')).filter((x) =>
      x instanceof HTMLElement && !skip.test(x.tagName) &&
      norm(x.textContent).toLowerCase().includes(lower) && norm(x.innerText) === want);
    const set = new Set(all), parents = new Set();
    for (const x of all) for (let p = x.parentElement; p; p = p.parentElement) if (set.has(p)) parents.add(p);
    return all.filter((x) => {
      if (parents.has(x)) return false;
      const r = x.getBoundingClientRect();
      return x.checkVisibility({ visibilityProperty: true }) && r.width > 0 && r.height > 0;
    });
  };
  // Counted now, at the click: the click that ends a path takes the page
  // away before anything could ask afterwards.
  const counts = (el, want) => {
    const out = { count: 0, scopes: [] };
    if (!want) return out;
    try {
      const mine = (list) => list.findIndex((m) => m === el || m.contains(el) || el.contains(m));
      out.count = matches(document, want).length;
      if (out.count <= 1) return out;
      for (let a = el.parentElement; a && a !== document.body; a = a.parentElement) {
        if (!a.id || document.querySelectorAll('#' + CSS.escape(a.id)).length !== 1) continue;
        const list = matches(a, want);
        const nth = mine(list);
        out.scopes.push({ id: a.id, count: list.length, nth: nth < 0 ? null : nth });
      }
    } catch (_) {}
    return out;
  };
  document.addEventListener('click', (e) => {
    if (!e.isTrusted) return;
    const t = e.target;
    const el = t instanceof Element ? t : (t && t.parentElement);
    if (!el) return;
    held.push(el);
    const near = el.closest('a,button,summary,[role]') || el;
    const typed = el.isContentEditable || near.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName);
    const text = typed ? '' : clean(near.innerText || near.textContent);
    const counted = counts(el, text);
    const payload = {
      doc: doc,
      i: held.length - 1,
      tag: near.tagName.toLowerCase(),
      role: near.getAttribute('role') || '',
      label: clean(near.getAttribute('aria-label')),
      text: text,
      count: counted.count,
      scopes: counted.scopes,
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
pub(crate) const CONTAINERS: [&str; 16] = [
    "RootWebArea", "WebArea", "main", "navigation", "menubar", "menu", "tablist", "tree", "dialog", "banner",
    "contentinfo", "form", "region", "document", "application", "list",
];

pub(crate) const MAX_CLIMB: usize = 6;
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
    /// How many visible elements the runner's text rule matches for `text`
    /// on the whole page, counted at the moment of the click - the click
    /// that ends a path takes the page away, so it cannot be counted after.
    /// 0 when nothing was counted.
    pub count: u32,
    /// Ancestors of the clicked element with an id unique on the page,
    /// nearest first, each with the same count inside it.
    pub scopes: Vec<ScopeHint>,
}

/// One id around a click: how many matches for the words it holds, and
/// which of them (zero-based, page order) is the clicked one - `None` when
/// the clicked element was not among them.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
#[serde(default)]
pub struct ScopeHint {
    pub id: String,
    pub count: u32,
    pub nth: Option<u32>,
}

/// Said at the click when words shown more than once have nothing around
/// them that tells them apart.
pub fn ambiguous(count: u32) -> String {
    format!(
        "those words are on the page {count} times and nothing around this click tells them apart - click the entry in the menu itself, or record from a page where it appears once"
    )
}

/// Could this id be the same on another machine and another day? Refused:
/// anything a selector would have to escape, and anything that looks built
/// from data - a run of three digits (`row-4711`, `ember123`) or a long
/// hex run with a digit in it (a hash or GUID piece).
pub fn stable_id(id: &str) -> bool {
    let mut chars = id.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return false;
    }
    let (mut digits, mut hex, mut hex_has_digit) = (0, 0, false);
    for c in id.chars() {
        digits = if c.is_ascii_digit() { digits + 1 } else { 0 };
        if c.is_ascii_hexdigit() {
            hex += 1;
            hex_has_digit |= c.is_ascii_digit();
        } else {
            hex = 0;
            hex_has_digit = false;
        }
        if digits >= 3 || (hex >= 8 && hex_has_digit) {
            return false;
        }
    }
    true
}

/// Words the page shows more than once, made to find the clicked one only:
/// scoped to the WIDEST stable id around it that holds them once (the menu
/// tree rather than an inner group of it), else placed by `nth` inside the
/// nearest stable id, else refused. Only exact words from the hints are
/// narrowed - the listener counted words, so its numbers say nothing about
/// a role and name.
pub fn narrow(target: Target, h: &ClickHints) -> Result<Target, String> {
    let words = match &target {
        Target::One(s) if s.exact && s.role.is_none() && s.css.is_none() && s.nth.is_none() => s.text.clone(),
        _ => None,
    };
    let Some(words) = words else { return Ok(target) };
    if h.count <= 1 || collapse(&words) != collapse(&h.text) {
        return Ok(target);
    }
    let stable: Vec<&ScopeHint> = h.scopes.iter().filter(|s| stable_id(&s.id)).collect();
    let within = |s: &ScopeHint, nth: Option<i32>| {
        Target::Chain(vec![
            LocatorStep { css: Some(format!("#{}", s.id)), ..LocatorStep::default() },
            LocatorStep { text: Some(words.clone()), exact: true, nth, ..LocatorStep::default() },
        ])
    };
    if let Some(s) = stable.iter().rev().find(|s| s.count == 1 && s.nth == Some(0)) {
        return Ok(within(s, None));
    }
    if let Some(s) = stable.iter().find(|s| s.count > 1) {
        if let Some(n) = s.nth {
            return Ok(within(s, Some(n as i32)));
        }
    }
    Err(ambiguous(h.count))
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
pub fn exact_role(role: &str, name: &str) -> Target {
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
    ax_chain_with_nodes(tree, backend).into_iter().map(|(link, _)| link).collect()
}

/// `ax_chain`, each link with the DOM node it stands for when the tree
/// names one (`backendDOMNodeId`), so a caller can ask the page about it.
pub fn ax_chain_with_nodes(tree: &Value, backend: i64) -> Vec<(AxLink, Option<i64>)> {
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
        out.push((
            AxLink {
                role: n["role"]["value"].as_str().unwrap_or("").to_string(),
                name: n["name"]["value"].as_str().unwrap_or("").to_string(),
                ignored: n["ignored"].as_bool().unwrap_or(false),
            },
            n["backendDOMNodeId"].as_i64(),
        ));
        let parent = n["parentId"].as_str().map(str::to_string).or_else(|| parent_of.get(&id).cloned());
        current = parent.and_then(|p| by_id.get(&p).copied());
    }
    out
}

/// The nearest node with a preferred role and a name; else the nearest
/// named node whose role a person clicks by; never past a container. Only
/// nodes the tree does not ignore count toward the climb: deep markup
/// (`a > div > span > svg > path`) is mostly ignored wrappers.
pub fn locator_from_ax(chain: &[AxLink]) -> Option<Target> {
    let near: Vec<&AxLink> = chain
        .iter()
        .take_while(|n| !CONTAINERS.contains(&n.role.as_str()))
        .filter(|n| !n.ignored)
        .take(MAX_CLIMB)
        .collect();
    let usable = |n: &AxLink| !n.ignored && !collapse(&n.name).is_empty();
    if let Some(n) = near.iter().find(|n| usable(n) && PREFERRED_ROLES.contains(&n.role.as_str())) {
        return Some(exact_role(&n.role, &n.name));
    }
    near.iter()
        .find(|n| usable(n) && !NOT_A_TARGET.contains(&n.role.as_str()))
        .map(|n| exact_role(&n.role, &n.name))
}

/// When the element is gone or has no role nearby: a role attribute with an
/// aria-label, else the visible words (short ones only). A role attribute
/// is read closer to how the browser reads it: its first token, the role
/// the browser tries first, and `none` or
/// `presentation` is no role at all.
pub fn locator_from_hints(h: &ClickHints) -> Option<Target> {
    let role = h.role.split_whitespace().next().filter(|r| !["none", "presentation"].contains(r)).unwrap_or("");
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

/// The accessibility chain up from an element the page is holding
/// (`__tcmRecHeld[i]` of document `doc`), or `None` when it is gone. Any
/// listener that keeps its elements there can use it.
pub async fn held_ax_chain<D: Driver>(d: &mut D, doc: &str, i: u32) -> Result<Option<Vec<AxLink>>, CdpError> {
    Ok(held_ax_nodes(d, doc, i).await?.map(|nodes| nodes.into_iter().map(|(link, _)| link).collect()))
}

/// `held_ax_chain`, each link with its DOM node (`ax_chain_with_nodes`).
pub async fn held_ax_nodes<D: Driver>(
    d: &mut D,
    doc: &str,
    i: u32,
) -> Result<Option<Vec<(AxLink, Option<i64>)>>, CdpError> {
    page::release(d).await;
    let document = page::document(d).await?;
    let found = page::call_elements(d, &document, HELD_JS, &[json!(doc), json!(i)]).await?;
    let Some(el) = found.first() else { return Ok(None) };
    let backend = page::backend_id(d, el).await?;
    let tree = ax_around(d, backend).await?;
    Ok(Some(ax_chain_with_nodes(&tree, backend)))
}

async fn from_the_element<D: Driver>(d: &mut D, click: &ClickPayload) -> Result<Option<Target>, CdpError> {
    Ok(held_ax_chain(d, &click.doc, click.i).await?.and_then(|chain| locator_from_ax(&chain)))
}

/// The locator for one reported click: from the accessibility tree when
/// the element is still there, else from the hints - narrowed when the
/// page showed those words more than once.
pub async fn locate<D: Driver>(d: &mut D, click: &ClickPayload) -> Result<Target, String> {
    if let Ok(Some(t)) = from_the_element(d, click).await {
        return Ok(t);
    }
    let t = locator_from_hints(&click.hints).ok_or_else(|| UNREADABLE.to_string())?;
    narrow(t, &click.hints)
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
