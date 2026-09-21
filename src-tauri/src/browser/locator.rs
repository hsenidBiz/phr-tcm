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
}

/// What an action points at. A plain string keeps the meaning it has
/// always had, so every script saved before locators existed still runs.
// `Chain` is declared before `One`: serde tries untagged variants in this
// order, and a struct with every field defaulted (as `LocatorStep` is)
// also deserializes from an empty JSON array - so `Chain` must get first
// look at an array or `[]` would silently become `One(LocatorStep::default())`
// instead of the empty chain the "a locator list is empty" validation
// error expects.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(untagged)]
pub enum Target {
    Legacy(String),
    Chain(Vec<LocatorStep>),
    One(LocatorStep),
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

fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|v| v.trim().is_empty())
}

impl LocatorStep {
    fn validate(&self) -> Result<(), String> {
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

    fn describe(&self) -> String {
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

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Target::Legacy(s) if s.trim().is_empty() => Err("a selector is empty".to_string()),
            Target::Legacy(_) => Ok(()),
            Target::Chain(v) if v.is_empty() => Err("a locator list is empty".to_string()),
            _ => self.steps().iter().try_for_each(LocatorStep::validate),
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

/// Every element the target matches right now. Empty is an answer, not an
/// error: callers decide whether "nothing yet" means wait or fail.
pub async fn resolve<D: Driver>(d: &mut D, target: &Target) -> Result<Vec<Handle>, CdpError> {
    let doc = page::document(d).await?;
    if let Target::Legacy(sel) = target {
        return page::call_elements(d, &doc, LEGACY_JS, &[json!(sel)]).await;
    }
    let mut roots = vec![doc];
    for step in target.steps() {
        let mut next = vec![];
        for root in &roots {
            next.extend(find_in(d, root, step).await?);
        }
        if let Some(n) = step.nth {
            next = next.into_iter().nth(n as usize).into_iter().collect();
        }
        roots = next;
        if roots.is_empty() {
            break;
        }
    }
    Ok(roots)
}
