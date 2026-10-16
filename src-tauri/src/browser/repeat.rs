//! What `repeat_while_visible` asks of the page: which of its selector's
//! matches sit in a protected row, and which row the pass works on.
//!
//! A match's ROW is found the same way here and in the Playwright export
//! (`ROW_OF_JS`): the nearest of the match itself and its ancestors that is
//! a `tr`, an element with `role="row"`, or an `li`; failing that, the
//! nearest ancestor (never the match itself) carrying a `data-*-id`
//! attribute (`data-id`, `data-goal-id`). The structural row is preferred so
//! an icon that carries its own `data-icon-id` never counts as the row. A
//! row is protected when it, or any element inside it, reads exactly one of
//! the `except` texts, ignoring case and runs of whitespace: its text, or a
//! field's value.
//!
//! The pass's row is marked with the attribute `ROW_SCOPE` names (every
//! other mark in that document is cleared first). A `then` locator that
//! starts with `{ "css": "[data-autorun-row]" }` is resolved inside it, by
//! the locator rules every action uses - so the element acted on is the
//! one in the row being processed, never a fresh first match.

use super::cdp::{CdpError, Driver};
use super::locator::{resolve, LocatorStep, Target};
use super::page::{self, Handle};
use serde_json::json;

/// The css a `then` locator starts with to be resolved inside the row the
/// pass is working on.
pub const ROW_SCOPE: &str = "[data-autorun-row]";

/// The attribute the pass's row carries.
pub const ROW_ATTR: &str = "data-autorun-row";

/// The rule's own words, for a sentence and the guide.
pub const ROW_RULE: &str =
    "a list row: the nearest tr, [role=row] or li holding the match (the match itself included), else the nearest element above it with a data-*-id attribute";

macro_rules! row_of_js {
    () => {
        r#"((e) => {
  for (let p = e; p && p.nodeType === 1; p = p.parentElement) {
    if (p.matches('tr, [role=row], li')) return p;
  }
  for (let p = e.parentElement; p && p.nodeType === 1; p = p.parentElement) {
    if (Array.from(p.attributes).some((a) => /^data-(?:[\w-]+-)?id$/i.test(a.name))) return p;
  }
  return null;
})"#
    };
}

macro_rules! kept_of_js {
    () => {
        r#"((row, except) => {
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim().toLowerCase();
  const want = except.map(norm);
  const all = [row, ...Array.from(row.querySelectorAll('*'))];
  for (const el of all) {
    const field = el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement;
    const said = norm(field ? el.value : el.innerText);
    const i = want.indexOf(said);
    if (i >= 0) return except[i];
  }
  return null;
})"#
    };
}

/// `(element) => row element | null` - the rule in the module's notes.
pub const ROW_OF_JS: &str = row_of_js!();

/// `(row, except) => the except text the row reads | null`.
pub const KEPT_OF_JS: &str = kept_of_js!();

/// `this` is a match. Argument: the except texts. `{ row, kept }`: whether
/// it sits in a row, and the protected text that row reads, or null.
pub const KEPT_JS: &str = concat!(
    "function(except) {\n  const rowOf = ",
    row_of_js!(),
    ";\n  const keptOf = ",
    kept_of_js!(),
    ";\n  const row = rowOf(this);\n  if (!row) return { row: false, kept: null };\n  return { row: true, kept: except.length ? keptOf(row, except) : null };\n}"
);

/// `this` is a match: its row is marked, every other mark in its document
/// cleared. Whether it had a row.
pub const MARK_JS: &str = concat!(
    "function() {\n  const rowOf = ",
    row_of_js!(),
    ";\n  const doc = this.ownerDocument || document;\n  doc.querySelectorAll('[data-autorun-row]').forEach((e) => e.removeAttribute('data-autorun-row'));\n  const row = rowOf(this);\n  if (!row) return false;\n  row.setAttribute('data-autorun-row', '');\n  return true;\n}"
);

/// `this` is the document: every mark cleared.
pub const UNMARK_JS: &str = r#"function() {
  const doc = this.ownerDocument || this;
  doc.querySelectorAll('[data-autorun-row]').forEach((e) => e.removeAttribute('data-autorun-row'));
  return true;
}"#;

/// One visible match of the loop's selector.
#[derive(Debug, Clone)]
pub struct Match {
    pub handle: Handle,
    /// Whether it sits in a row (`ROW_RULE`).
    pub row: bool,
    /// The protected text its row reads, as `except` wrote it.
    pub kept: Option<String>,
}

/// Every match of `selector` right now, each with its row's verdict. One
/// look, no waiting.
pub async fn look<D: Driver>(d: &mut D, selector: &Target, except: &[String]) -> Result<Vec<Match>, CdpError> {
    page::release(d).await;
    let mut out = Vec::new();
    for handle in resolve(d, selector).await? {
        let v = page::call_value(d, &handle, KEPT_JS, &[json!(except)]).await?;
        let row = v["row"].as_bool().unwrap_or(false);
        let kept = v["kept"].as_str().map(str::to_string);
        out.push(Match { handle, row, kept });
    }
    Ok(out)
}

/// Marks `m`'s row as the one the pass works on. Whether it had one.
pub async fn mark<D: Driver>(d: &mut D, m: &Match) -> Result<bool, CdpError> {
    Ok(page::call_value(d, &m.handle, MARK_JS, &[]).await?.as_bool().unwrap_or(false))
}

/// Clears the mark. Housekeeping: a failure is ignored.
pub async fn unmark<D: Driver>(d: &mut D) {
    if let Ok(doc) = page::document(d).await {
        let _ = page::call_value(d, &doc, UNMARK_JS, &[]).await;
    }
}

/// Is the marked row still showing?
pub async fn row_still_there<D: Driver>(d: &mut D) -> Result<bool, CdpError> {
    let target = Target::One(LocatorStep { css: Some(ROW_SCOPE.to_string()), ..LocatorStep::default() });
    Ok(!resolve(d, &target).await?.is_empty())
}

/// Does `t` start inside the pass's row?
pub fn is_row_scoped(t: &Target) -> bool {
    match t {
        Target::Legacy(s) => s.trim_start().starts_with(ROW_SCOPE),
        _ => t.links().first().is_some_and(|l| l.css.as_deref().map(str::trim) == Some(ROW_SCOPE)),
    }
}

/// What `t` names inside the row, without the row link: `None` for a
/// target that is not row-scoped, or one that names the row itself.
pub fn within_row(t: &Target) -> Option<Target> {
    if !is_row_scoped(t) {
        return None;
    }
    match t {
        Target::Legacy(s) => {
            let rest = s.trim_start()[ROW_SCOPE.len()..].trim();
            (!rest.is_empty()).then(|| Target::Legacy(rest.to_string()))
        }
        _ => {
            let rest: Vec<LocatorStep> = t.links().into_iter().skip(1).collect();
            match rest.len() {
                0 => None,
                1 => rest.into_iter().next().map(Target::One),
                _ => Some(Target::Chain(rest)),
            }
        }
    }
}
