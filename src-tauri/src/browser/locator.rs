//! Saying WHICH element, in the words a test case uses.
//!
//! A test case says "click Add Method in the Add Rating Method dialog". A
//! locator says the same thing: a role and a name, inside another role and
//! name. Chrome computes role and name itself (the accessibility tree), so
//! this app does not re-implement the accessible-name rules - it asks.
//!
//! Every step keeps only what a person could see unless it says otherwise.
//! Real applications keep hidden copies of their dialogs and menus in the
//! page; a locator that matched those would click nothing, or the wrong
//! thing, and report success.

use super::cdp::{CdpError, Driver};
use super::page::{self, Handle};
use serde_json::json;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct LocatorStep {
    /// An ARIA role as Chrome reports it: button, link, textbox, dialog,
    /// heading, checkbox, combobox, searchbox, row, cell...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The accessible name. Only with `role`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Visible text; the deepest element carrying it wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
    /// Equal (case-sensitive) instead of contains (case-insensitive).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exact: bool,
    /// `Some(false)` also matches what cannot be seen. Absent means
    /// visible only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Zero-based pick from this step's matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nth: Option<i32>,
    /// A component's placeholder: the caller's input of this name supplies
    /// the whole locator step. Stands alone, and is replaced before a
    /// script runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
}

/// What an action points at. A plain string keeps the meaning it has
/// always had, so every script saved before locators existed still runs.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(untagged)]
pub enum Target {
    Legacy(String),
    One(LocatorStep),
    Chain(Vec<LocatorStep>),
}

/// Hand-written rather than `#[derive(Deserialize)] #[serde(untagged)]`:
/// serde's derived struct visitor also accepts a JSON ARRAY positionally
/// (every `LocatorStep` field has a default), so an untagged derive would
/// let something like `["button", "Save"]` quietly become
/// `One(LocatorStep { role: Some("button"), name: Some("Save"), .. })` - a
/// malformed selector accepted with a meaning nobody wrote. Deciding on
/// the JSON shape first closes that off: an array is only ever a chain of
/// locator objects, never a step read positionally.
impl<'de> serde::Deserialize<'de> for Target {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        let value: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(Target::Legacy(s)),
            serde_json::Value::Object(_) => {
                serde_json::from_value::<LocatorStep>(value).map(Target::One).map_err(D::Error::custom)
            }
            serde_json::Value::Array(items) if items.iter().all(serde_json::Value::is_object) => items
                .into_iter()
                .map(serde_json::from_value::<LocatorStep>)
                .collect::<Result<Vec<_>, _>>()
                .map(Target::Chain)
                .map_err(D::Error::custom),
            _ => Err(D::Error::custom(
                "a selector is a string, a locator object, or a list of locator objects",
            )),
        }
    }
}

impl From<&str> for Target {
    fn from(s: &str) -> Self {
        Target::Legacy(s.to_string())
    }
}

impl From<String> for Target {
    fn from(s: String) -> Self {
        Target::Legacy(s)
    }
}

/// How a locator is remembered once the live page has named the element it
/// found: a role with its accessible name, or the exact text or css it used.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum SeenKey {
    Role { role: String, name: String },
    Text(String),
    Css(String),
}

/// A name the way two people would call it the same: trimmed, every unusual
/// space (no-break, narrow no-break, figure, thin, tab, newline) one space,
/// runs collapsed, lowercase.
pub fn fold_name(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|v| v.trim().is_empty())
}

impl LocatorStep {
    fn validate(&self) -> Result<(), String> {
        if self.input.is_some() {
            // The caller's locator replaces the whole link, so any other
            // field here would be dropped without a word.
            if self.role.is_some()
                || self.name.is_some()
                || self.text.is_some()
                || self.css.is_some()
                || self.exact
                || self.visible.is_some()
                || self.nth.is_some()
            {
                return Err("an input placeholder stands alone".to_string());
            }
            if blank(&self.input) {
                return Err("an input placeholder needs a name".to_string());
            }
            return Ok(());
        }
        if blank(&self.role) || blank(&self.name) || blank(&self.text) || blank(&self.css) {
            return Err("a locator has an empty role, name, text or css".to_string());
        }
        let kinds = [self.role.is_some(), self.text.is_some(), self.css.is_some()]
            .iter()
            .filter(|b| **b)
            .count();
        if kinds == 0 {
            return Err("a locator needs one of role, text or css".to_string());
        }
        if kinds > 1 {
            return Err("a locator takes only one of role, text or css".to_string());
        }
        if self.name.is_some() && self.role.is_none() {
            return Err("name only goes with role".to_string());
        }
        if self.nth.is_some_and(|n| n < 0) {
            return Err("nth counts from 0 and cannot be negative".to_string());
        }
        Ok(())
    }

    /// What this link is called, for comparison with what the page showed.
    /// `exact`, `visible` and `nth` do not change which element is meant by
    /// name, so they are left out. `None` when the link names nothing.
    pub fn seen_key(&self) -> Option<SeenKey> {
        if let Some(role) = &self.role {
            return Some(SeenKey::Role {
                role: fold_name(role),
                name: fold_name(self.name.as_deref().unwrap_or("")),
            });
        }
        if let Some(text) = &self.text {
            return Some(SeenKey::Text(text.trim().to_string()));
        }
        self.css.as_ref().map(|c| SeenKey::Css(c.trim().to_string()))
    }

    fn describe(&self) -> String {
        if let Some(input) = &self.input {
            return format!("input \"{input}\"");
        }
        let mut s = if let Some(role) = &self.role {
            match &self.name {
                Some(name) => format!("{role} \"{name}\""),
                None => role.clone(),
            }
        } else if let Some(text) = &self.text {
            format!("text \"{text}\"")
        } else {
            self.css.clone().unwrap_or_default()
        };
        if let Some(n) = self.nth {
            s.push_str(&format!(" #{}", n + 1));
        }
        s
    }
}

impl Target {
    pub fn is_legacy(&self) -> bool {
        matches!(self, Target::Legacy(_))
    }

    fn steps(&self) -> &[LocatorStep] {
        match self {
            Target::Legacy(_) => &[],
            Target::One(s) => std::slice::from_ref(s),
            Target::Chain(v) => v,
        }
    }

    /// Every link of this target as a step: a legacy string is one css link.
    pub fn links(&self) -> Vec<LocatorStep> {
        match self {
            Target::Legacy(s) => vec![LocatorStep { css: Some(s.clone()), ..LocatorStep::default() }],
            _ => self.steps().to_vec(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Target::Legacy(s) if s.trim().is_empty() => Err("a selector is empty".to_string()),
            Target::Legacy(_) => Ok(()),
            Target::Chain(v) if v.is_empty() => Err("a locator list is empty".to_string()),
            _ => self.steps().iter().try_for_each(LocatorStep::validate),
        }
    }

    /// The same target, matching what cannot be seen as well. Only
    /// `expect_visible` uses it, and only after a visible-only look found
    /// nothing: without it, "is there but cannot be seen" is unreachable
    /// for a structured target, and a hidden element reads as absent. A
    /// legacy string has no visibility filter to relax, so it is returned
    /// unchanged.
    pub fn including_hidden(&self) -> Target {
        let show_all = |s: &LocatorStep| LocatorStep { visible: Some(false), ..s.clone() };
        match self {
            Target::Legacy(s) => Target::Legacy(s.clone()),
            Target::One(s) => Target::One(show_all(s)),
            Target::Chain(v) => Target::Chain(v.iter().map(show_all).collect()),
        }
    }

    /// Innermost first, the way a person says it: `button "Add Method" in
    /// dialog "Add Rating Method"`.
    pub fn describe(&self) -> String {
        match self {
            Target::Legacy(s) => s.clone(),
            _ => self
                .steps()
                .iter()
                .rev()
                .map(LocatorStep::describe)
                .collect::<Vec<_>>()
                .join(" in "),
        }
    }
}

/// Does any link of this target wait for a component input?
pub fn has_input_placeholder(t: &Target) -> bool {
    t.steps().iter().any(|s| s.input.is_some())
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whitespace is collapsed on both sides first: accessible names arrive
/// with stray spaces. Then contains (case-insensitive), or equal when
/// `exact`.
pub fn name_matches(got: &str, want: &str, exact: bool) -> bool {
    let (got, want) = (collapse(got), collapse(want));
    if exact {
        got == want
    } else {
        got.to_lowercase().contains(&want.to_lowercase())
    }
}

/// `this` is the root. Arguments: selector, visibleOnly.
pub const CSS_JS: &str = r#"function(sel, visibleOnly) {
  const seen = (e) => {
    if (!visibleOnly) return true;
    const r = e.getBoundingClientRect();
    return e.checkVisibility({ visibilityProperty: true }) && r.width > 0 && r.height > 0;
  };
  return Array.from(this.querySelectorAll(sel)).filter(seen);
}"#;

/// `this` is the root. Arguments: text, exact, visibleOnly. The DEEPEST
/// element carrying the text wins: the control itself, not the panel it
/// sits in. textContent is a cheap first pass before the costly innerText.
pub const TEXT_JS: &str = r#"function(want, exact, visibleOnly) {
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  const needle = norm(want);
  const lower = needle.toLowerCase();
  const isButtonInput = (e) => e instanceof HTMLInputElement && /^(button|submit|reset)$/.test(e.type);
  const textOf = (e) => norm(isButtonInput(e) ? e.value : e.innerText);
  const hit = (e) => {
    if (!isButtonInput(e) && !norm(e.textContent).toLowerCase().includes(lower)) return false;
    const t = textOf(e);
    return exact ? t === needle : t.toLowerCase().includes(lower);
  };
  const seen = (e) => {
    if (!visibleOnly) return true;
    const r = e.getBoundingClientRect();
    return e.checkVisibility({ visibilityProperty: true }) && r.width > 0 && r.height > 0;
  };
  const skip = /^(SCRIPT|STYLE|HEAD|HTML|BODY|NOSCRIPT|TEMPLATE)$/;
  const all = Array.from(this.querySelectorAll('*'))
    .filter((e) => e instanceof HTMLElement && !skip.test(e.tagName) && hit(e));
  const matched = new Set(all);
  const parents = new Set();
  for (const e of all) {
    for (let p = e.parentElement; p; p = p.parentElement) if (matched.has(p)) parents.add(p);
  }
  return all.filter((e) => !parents.has(e) && seen(e));
}"#;

/// The selector strings scripts have always used: CSS (first match), or
/// `text=words` (last match). No visibility filter, exactly as before.
pub const LEGACY_JS: &str = r#"function(sel) {
  if (sel.startsWith('text=')) {
    const want = sel.slice(5).trim().toLowerCase();
    const all = Array.from(this.querySelectorAll(
      'button,a,[role=button],label,input,textarea,select,td,th,li,summary,h1,h2,h3,span,div'));
    const hits = all.filter((e) => ((e.innerText || e.value || '') + '').trim().toLowerCase().includes(want));
    return hits.length ? [hits[hits.length - 1]] : [];
  }
  const el = this.querySelector(sel);
  return el ? [el] : [];
}"#;

/// `this` is the element.
pub const VISIBLE_JS: &str = r#"function() {
  const r = this.getBoundingClientRect();
  return this.checkVisibility({ visibilityProperty: true }) && r.width > 0 && r.height > 0;
}"#;

async fn by_role<D: Driver>(
    d: &mut D,
    root: &Handle,
    step: &LocatorStep,
    role: &str,
    visible_only: bool,
) -> Result<Vec<Handle>, CdpError> {
    // Role only. The protocol's own name filter is exact-match, and names
    // carry stray whitespace, so the name is matched here.
    let r = d
        .call("Accessibility.queryAXTree", json!({ "objectId": root, "role": role }))
        .await?;
    let mut out = vec![];
    for node in r["nodes"].as_array().into_iter().flatten() {
        if node["ignored"].as_bool().unwrap_or(false) {
            continue;
        }
        if let Some(want) = &step.name {
            let got = node["name"]["value"].as_str().unwrap_or("");
            if !name_matches(got, want, step.exact) {
                continue;
            }
        }
        let Some(backend) = node["backendDOMNodeId"].as_i64() else { continue };
        let handle = page::resolve_backend(d, backend).await?;
        if visible_only
            && !page::call_value(d, &handle, VISIBLE_JS, &[]).await?.as_bool().unwrap_or(false)
        {
            continue;
        }
        out.push(handle);
    }
    Ok(out)
}

async fn find_in<D: Driver>(
    d: &mut D,
    root: &Handle,
    step: &LocatorStep,
) -> Result<Vec<Handle>, CdpError> {
    let visible_only = step.visible.unwrap_or(true);
    if let Some(role) = &step.role {
        return by_role(d, root, step, role, visible_only).await;
    }
    if let Some(text) = &step.text {
        return page::call_elements(d, root, TEXT_JS, &[json!(text), json!(step.exact), json!(visible_only)])
            .await;
    }
    let css = step.css.as_deref().unwrap_or("");
    page::call_elements(d, root, CSS_JS, &[json!(css), json!(visible_only)]).await
}

/// Later handles that name the same backend node as an earlier one are
/// dropped, first-seen order kept. Only needed when a step searched more
/// than one root: nested roots (a dialog inside a dialog, nested rows of
/// the same role) can otherwise reach the same element twice, doubling a
/// count and pointing `nth` at the wrong match.
async fn dedupe_by_backend<D: Driver>(d: &mut D, handles: Vec<Handle>) -> Result<Vec<Handle>, CdpError> {
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    for handle in handles {
        if seen.insert(page::backend_id(d, &handle).await?) {
            out.push(handle);
        }
    }
    Ok(out)
}

/// Why a chain found nothing when it had to pass through a frame whose page
/// cannot be reached: a frame from another site, a sandboxed one, or one
/// with no document yet. `frame` is that frame step's `describe()`.
pub fn frame_unreachable(frame: &str) -> String {
    format!("the frame {frame} {FRAME_UNREACHABLE}")
}

/// The words `frame_unreachable` ends with, which `autorun::patterns`
/// reads a failure's class from.
pub const FRAME_UNREACHABLE: &str = "holds a page from another site (or has not loaded), which Auto Run cannot reach";

/// Every element the target matches right now. Empty is an answer, not an
/// error: callers decide whether "nothing yet" means wait or fail.
pub async fn resolve<D: Driver>(d: &mut D, target: &Target) -> Result<Vec<Handle>, CdpError> {
    Ok(resolve_explained(d, target).await?.handles)
}

/// What a target matched, and - when it matched nothing because the chain
/// had to pass through a frame it could not enter - which frame, in a
/// sentence for the person (`frame_unreachable`).
#[derive(Debug, Default)]
pub struct Resolved {
    pub handles: Vec<Handle>,
    pub unreachable_frame: Option<String>,
}

/// `this` is an element. "frame" for an iframe/frame whose document the
/// page can reach (same origin), "unreachable" for one it cannot (another
/// site, sandboxed, not loaded), "element" for anything else. By tag name,
/// not `instanceof`: the element may belong to another frame's realm.
pub const FRAME_JS: &str = r#"function() {
  if (this.tagName !== 'IFRAME' && this.tagName !== 'FRAME') return 'element';
  let doc = null;
  try { doc = this.contentDocument; } catch (e) { doc = null; }
  return doc ? 'frame' : 'unreachable';
}"#;

/// `this` is a reachable frame element: its document, as the next root.
pub const FRAME_DOC_JS: &str = r#"function() { return [this.contentDocument]; }"#;

/// `resolve`, saying why when a frame stood in the way. A step that matches
/// an iframe hands the NEXT step that frame's document to search - the way
/// a page builds a component inside a same-origin frame (PeoplesHR's
/// employee search). The last step is never swapped, so a chain can still
/// point at the iframe itself.
pub async fn resolve_explained<D: Driver>(d: &mut D, target: &Target) -> Result<Resolved, CdpError> {
    let doc = page::document(d).await?;
    if let Target::Legacy(sel) = target {
        let handles = page::call_elements(d, &doc, LEGACY_JS, &[json!(sel)]).await?;
        return Ok(Resolved { handles, unreachable_frame: None });
    }
    let steps = target.steps();
    let mut unreachable_frame = None;
    let mut roots = vec![doc];
    for (i, step) in steps.iter().enumerate() {
        let mut next = vec![];
        for root in &roots {
            next.extend(find_in(d, root, step).await?);
        }
        if roots.len() > 1 {
            next = dedupe_by_backend(d, next).await?;
        }
        if let Some(n) = step.nth {
            next = next.into_iter().nth(n as usize).into_iter().collect();
        }
        if i + 1 < steps.len() {
            let mut entered = Vec::with_capacity(next.len());
            // Whether a frame at this step was entered, and whether one
            // could not be: a step that entered another frame and found
            // nothing in it is "not found", not "cannot reach".
            let (mut frame_entered, mut blocked) = (false, false);
            for handle in next {
                match page::call_value(d, &handle, FRAME_JS, &[]).await?.as_str() {
                    Some("frame") => {
                        frame_entered = true;
                        // Chrome runs a function in the context its handle
                        // came from. Read through the parent, the frame's
                        // document would make every later search and probe
                        // use the PARENT's globals (`document`, `instanceof
                        // HTMLElement`, `innerWidth`). Resolving the node
                        // again by its backend id hands back a handle that
                        // lives in the frame's own context.
                        for doc in page::call_elements(d, &handle, FRAME_DOC_JS, &[]).await? {
                            let backend = page::backend_id(d, &doc).await?;
                            entered.push(page::resolve_backend(d, backend).await?);
                        }
                    }
                    Some("unreachable") => blocked = true,
                    _ => entered.push(handle),
                }
            }
            if blocked && !frame_entered {
                unreachable_frame.get_or_insert_with(|| frame_unreachable(&step.describe()));
            }
            next = entered;
        }
        roots = next;
        if roots.is_empty() {
            break;
        }
    }
    Ok(Resolved { handles: roots, unreachable_frame })
}
