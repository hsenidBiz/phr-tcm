//! One loop for every expectation: look, and if it does not hold yet, look
//! again until the timeout. Pages settle; a record's name appears a moment
//! after Save. A single look turns that moment into a false failure, and a
//! fixed sleep turns it into a slow test that still fails on a bad day.
//!
//! When time runs out, the detail says what was last seen. "Expected X" is
//! not evidence; "expected X but saw Y" is.

use super::actions::{harness, harness_timeout, ActionOutcome};
use super::cdp::{CdpError, Driver};
use super::input::{SHOWN_JS, STILL_LOOKING};
use super::locator::{resolve, resolve_explained, Target, VISIBLE_JS};
use super::page::{self, Handle};
use serde_json::json;
use std::time::{Duration, Instant};

pub enum Check<'a> {
    Visible,
    /// At least one match is visible - what a `when_visible` asks. Two
    /// visible matches (a dialog's OK and Cancel) are shown, not "not
    /// shown": the ambiguity is for the guarded action to report. Visible
    /// here is what a click needs (`input::SHOWN_JS`): one still outside
    /// the window after it is scrolled into view is not shown.
    Shown,
    Hidden,
    Text(&'a str),
    ContainsText(&'a str),
    Count(u32),
    Attribute { name: &'a str, equals: &'a str },
    /// The one match has no attribute `name`.
    NoAttribute { name: &'a str },
    /// The focus is on the one match, or inside it.
    Focused,
    /// The one match is not disabled (`DISABLED_JS`).
    Enabled,
    /// The one match is disabled (`DISABLED_JS`).
    Disabled,
    /// The one match is ticked (true) or not (false), by `CHECKED_JS`.
    Checked(bool),
    /// The one match's computed style `property`, by `READ_STYLE_JS`,
    /// judged by `style::judge`.
    Style { property: &'a str, want: super::style::Want<'a> },
}

/// `this` is the element. Why it is disabled, in a few words, or `""` when
/// it is not, by the one rule `disabled_why_js!` writes down (its own
/// `disabled` property, `aria-disabled="true"` on it or around it, or a
/// disabled fieldset holding a listed form control outside its first
/// legend). A click waits out the same rule (`input::PROBE_JS`).
pub const DISABLED_JS: &str = concat!("function() { return ", disabled_why_js!(), "(this); }");

/// `expect_enabled` on a disabled element, followed by why: one of
/// "its disabled property is set", "it has aria-disabled=\"true\"", "an
/// element around it has aria-disabled=\"true\"" or "a disabled fieldset
/// holds it".
pub const IS_DISABLED: &str = "is disabled: ";
/// `expect_disabled` on an element that is not.
pub const IS_ENABLED: &str = "is enabled";
/// `expect_attribute` with `absent` on an element that has it, followed by
/// the attribute's name and value.
pub const HAS_ATTRIBUTE: &str = "still has ";

/// `this` is the element. Does it, or something inside it, have the focus
/// in its own document (an element in a frame is asked in that frame)?
/// Open shadow roots are followed, so a control a component draws inside
/// its own root still counts as its host's.
pub const FOCUSED_JS: &str = r#"function() {
  let a = this.ownerDocument.activeElement;
  while (a && a.shadowRoot && a.shadowRoot.activeElement) a = a.shadowRoot.activeElement;
  if (!a) return false;
  if (a === this || this.contains(a)) return true;
  let host = a.getRootNode && a.getRootNode().host;
  while (host) { if (host === this || this.contains(host)) return true; host = host.getRootNode && host.getRootNode().host; }
  return false;
}"#;

/// `this` is the element. Is it ticked: `"true"`, `"false"` or `"mixed"`,
/// or null when it says nothing. A checkbox or radio input answers by its
/// live `checked` property (and `indeterminate` for `"mixed"`), which
/// follows every click; the `checked` ATTRIBUTE is only where it started.
/// Anything else answers by its own `aria-checked`, then a label by the
/// box it is for. Any other element holding a box is a container, never
/// read for the first box inside it: `"container"`.
pub const CHECKED_JS: &str = r#"function() {
  const box = (e) => e instanceof HTMLInputElement && (e.type === 'checkbox' || e.type === 'radio');
  const said = (e) => e.indeterminate ? 'mixed' : (e.checked ? 'true' : 'false');
  if (box(this)) return said(this);
  const aria = (this.getAttribute('aria-checked') || '').trim().toLowerCase();
  if (aria === 'true' || aria === 'false' || aria === 'mixed') return aria;
  if (this instanceof HTMLLabelElement && this.control && box(this.control)) return said(this.control);
  const inside = this.querySelector && this.querySelector('input[type=checkbox],input[type=radio],[aria-checked]');
  return inside ? 'container' : null;
}"#;

/// `expect_checked` on a box that is ticked when it should not be.
pub const IS_CHECKED: &str = "is checked";
/// `expect_checked` on a box that is not ticked when it should be.
pub const IS_NOT_CHECKED: &str = "is not checked";
/// `expect_checked` on a box that is partly ticked (`aria-checked="mixed"`
/// or an indeterminate checkbox): neither ticked nor clear.
pub const IS_MIXED: &str = "is partly checked (mixed)";
/// `expect_checked` on an element holding boxes: it is never read for the
/// first one.
pub const MATCHES_A_CONTAINER: &str = "matches a container, point at the checkbox";
/// `expect_checked` on an element with no checked state at all.
pub const NOT_CHECKABLE: &str =
    "has no checked state: it is not a checkbox or radio, and has no aria-checked - point it at the box itself";

/// `expect_focused` on an element that does not have the focus.
pub const NOT_FOCUSED: &str = "does not have the focus";

/// `this` is the element. What a person would read: a field's value, a
/// list's chosen option, otherwise the rendered text.
pub const READ_TEXT_JS: &str = r#"function() {
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  if (this instanceof HTMLSelectElement) return norm(this.selectedOptions[0] ? this.selectedOptions[0].label : '');
  if (this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement) return norm(this.value);
  return norm(this.innerText);
}"#;

/// `this` is the element. Argument: the attribute name. null when absent.
pub const READ_ATTR_JS: &str = r#"function(name) { return this.getAttribute(name); }"#;

/// `this` is the element. Argument: a CSS property name. `{ value, moving
/// }`: its computed value, and whether a running transition or finite
/// animation of the element is changing that property right now - a value
/// read then is an in-between one, so `expect_style` looks again. A
/// transition counts when it is of that property, of `all`, or of a
/// shorthand or longhand of it; an animation when a keyframe sets it (or a
/// shorthand or longhand of it). An endless animation (a pulse) never
/// settles and is not waited for.
pub const READ_STYLE_JS: &str = r#"function(prop) {
  const value = String(getComputedStyle(this).getPropertyValue(prop) || '').trim();
  const camel = prop.startsWith('--') ? prop : prop.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
  const related = (a, b) => a === b || a.startsWith(b + '-') || b.startsWith(a + '-');
  const meta = new Set(['offset', 'computedOffset', 'easing', 'composite']);
  const touches = (an) => {
    if (typeof CSSTransition !== 'undefined' && an instanceof CSSTransition) {
      const p = an.transitionProperty || '';
      return p === 'all' || related(p, prop);
    }
    const e = an.effect;
    if (!e || !e.getKeyframes) return false;
    const t = e.getComputedTiming ? e.getComputedTiming() : {};
    if (t.iterations === Infinity) return false;
    return e.getKeyframes().some((k) => Object.keys(k).some((n) => !meta.has(n) && (n === camel || camel.startsWith(n) || n.startsWith(camel))));
  };
  let moving = false;
  try {
    moving = (this.getAnimations ? this.getAnimations() : []).some((an) => an.playState === 'running' && touches(an));
  } catch (_) {}
  return { value, moving };
}"#;

/// The honest answer when the element IS in the page and cannot be seen.
/// This and the constants below are the words an expectation fails in,
/// read back by `autorun::patterns` - see `input`'s own list.
pub const HIDDEN: &str = "is there but cannot be seen";
/// Nothing on the page matches the target.
pub const NOT_ON_PAGE: &str = "is not on the page";
/// `expect_hidden` still sees one of them.
pub const STILL_VISIBLE: &str = "is still visible";
/// `expect_text`: followed by the wanted text and what was seen.
pub const EXPECTED_TEXT: &str = "expected text ";
/// `expect_contains_text`: followed by the wanted text and what was seen.
pub const EXPECTED_TO_CONTAIN: &str = "expected it to contain ";
/// `expect_count`: `expected N, counted M`.
pub const COUNTED: &str = ", counted ";
/// `expect_attribute` on an element without it: `has no NAME attribute`.
pub const HAS_NO: &str = "has no ";
/// The tail of [`HAS_NO`]'s sentence.
pub const ATTRIBUTE_TAIL: &str = " attribute";

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn visible<D: Driver>(d: &mut D, h: &Handle) -> Result<bool, CdpError> {
    Ok(page::call_value(d, h, VISIBLE_JS, &[]).await?.as_bool().unwrap_or(false))
}

/// Shown the way a click needs it: visible, and inside the window once
/// scrolled there (`input::SHOWN_JS`).
async fn shown<D: Driver>(d: &mut D, h: &Handle) -> Result<bool, CdpError> {
    Ok(page::call_value(d, h, SHOWN_JS, &[]).await?.as_bool().unwrap_or(false))
}

fn only(handles: &[Handle]) -> Result<&Handle, String> {
    match handles {
        [] => Err(NOT_ON_PAGE.to_string()),
        [one] => Ok(one),
        many => Err(super::input::matched_many(many.len())),
    }
}

/// One look. `Ok(Ok(detail))` holds; `Ok(Err(why))` does not hold yet.
pub(crate) async fn look<D: Driver>(
    d: &mut D,
    target: &Target,
    check: &Check<'_>,
) -> Result<Result<String, String>, CdpError> {
    let what = target.describe();
    let found = resolve_explained(d, target).await?;
    // Nothing found because the chain could not enter a frame is not an
    // answer to ANY check - expect_hidden or a count of 0 would otherwise
    // pass on a page Auto Run never saw.
    if let (true, Some(why)) = (found.handles.is_empty(), found.unreachable_frame) {
        return Ok(Err(why));
    }
    let handles = found.handles;
    Ok(match check {
        Check::Visible => match only(&handles) {
            // A locator keeps only what a person could see, so "nothing
            // resolved" cannot on its own tell "not there" from "there
            // but hidden" - and the honest second answer would be
            // unreachable for every structured target. Ask once more
            // including what cannot be seen, and report which it was.
            Err(why) if handles.is_empty() => {
                if resolve(d, &target.including_hidden()).await?.is_empty() {
                    Err(why)
                } else {
                    Err(HIDDEN.to_string())
                }
            }
            Err(why) => Err(why),
            Ok(h) => {
                if visible(d, h).await? {
                    Ok(format!("{what} is visible"))
                } else {
                    Err(HIDDEN.to_string())
                }
            }
        },
        Check::Shown => {
            let mut seen = false;
            for h in &handles {
                if shown(d, h).await? {
                    seen = true;
                    break;
                }
            }
            if seen {
                Ok(format!("{what} is visible"))
            } else if handles.is_empty() {
                Err(NOT_ON_PAGE.to_string())
            } else {
                Err(HIDDEN.to_string())
            }
        }
        Check::Hidden => {
            let mut seen = false;
            for h in &handles {
                if visible(d, h).await? {
                    seen = true;
                    break;
                }
            }
            if seen {
                Err(STILL_VISIBLE.to_string())
            } else {
                Ok(format!("{what} is hidden"))
            }
        }
        Check::Count(n) => {
            if handles.len() as u32 == *n {
                Ok(format!("counted {n}: {what}"))
            } else {
                Err(format!("expected {n}{COUNTED}{}", handles.len()))
            }
        }
        Check::Text(want) => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, READ_TEXT_JS, &[]).await?;
                let (got, want) = (collapse(got.as_str().unwrap_or("")), collapse(want));
                if got == want {
                    Ok(format!("{what} says {want:?}"))
                } else {
                    Err(format!("{EXPECTED_TEXT}{want:?} but saw {got:?}"))
                }
            }
        },
        Check::ContainsText(want) => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, READ_TEXT_JS, &[]).await?;
                let (got, want) = (collapse(got.as_str().unwrap_or("")), collapse(want));
                if got.contains(&want) {
                    Ok(format!("{what} contains {want:?}"))
                } else {
                    Err(format!("{EXPECTED_TO_CONTAIN}{want:?} but saw {got:?}"))
                }
            }
        },
        Check::Attribute { name, equals } => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, READ_ATTR_JS, &[json!(name)]).await?;
                match got.as_str() {
                    None => Err(format!("{HAS_NO}{name}{ATTRIBUTE_TAIL}")),
                    Some(v) if v == *equals => Ok(format!("{what} has {name}={equals:?}")),
                    Some(v) => Err(format!("expected {name}={equals:?} but saw {v:?}")),
                }
            }
        },
        Check::NoAttribute { name } => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, READ_ATTR_JS, &[json!(name)]).await?;
                match got.as_str() {
                    None => Ok(format!("{what} {HAS_NO}{name}{ATTRIBUTE_TAIL}")),
                    Some(v) => Err(format!("{HAS_ATTRIBUTE}{name}={v:?}")),
                }
            }
        },
        Check::Enabled | Check::Disabled => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let why = page::call_value(d, h, DISABLED_JS, &[]).await?;
                let why = why.as_str().unwrap_or("");
                match (matches!(check, Check::Enabled), why.is_empty()) {
                    (true, true) => Ok(format!("{what} {IS_ENABLED}")),
                    (true, false) => Err(format!("{IS_DISABLED}{why}")),
                    (false, false) => Ok(format!("{what} {IS_DISABLED}{why}")),
                    (false, true) => Err(IS_ENABLED.to_string()),
                }
            }
        },
        Check::Checked(want) => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, CHECKED_JS, &[]).await?;
                match (got.as_str(), *want) {
                    (Some("true"), true) => Ok(format!("{what} {IS_CHECKED}")),
                    (Some("false"), false) => Ok(format!("{what} {IS_NOT_CHECKED}")),
                    (Some("true"), false) => Err(IS_CHECKED.to_string()),
                    (Some("false"), true) => Err(IS_NOT_CHECKED.to_string()),
                    (Some("mixed"), _) => Err(IS_MIXED.to_string()),
                    (Some("container"), _) => Err(MATCHES_A_CONTAINER.to_string()),
                    _ => Err(NOT_CHECKABLE.to_string()),
                }
            }
        },
        Check::Style { property, want } => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, READ_STYLE_JS, &[json!(property)]).await?;
                let value = got["value"].as_str().unwrap_or("");
                if got["moving"].as_bool().unwrap_or(false) {
                    Err(format!("{property} is {} ({} so far)", super::style::STILL_CHANGING, super::style::shown(value)))
                } else {
                    super::style::judge(property, value, *want).map(|ok| format!("{what} {ok}"))
                }
            }
        },
        Check::Focused => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let has = page::call_value(d, h, FOCUSED_JS, &[]).await?;
                if has.as_bool().unwrap_or(false) {
                    Ok(format!("{what} has the focus"))
                } else {
                    // Where it is instead is the evidence a failure needs.
                    Err(format!("{NOT_FOCUSED} - {}", super::actions::focus_now(d).await))
                }
            }
        },
    })
}

/// The deadline is pushed down into the driver so no single protocol call
/// can outlive this expectation's budget, and cleared on EVERY path out -
/// which is why the loop is a separate function rather than an early
/// `return` away from a `set_deadline(None)`.
pub async fn expect<D: Driver>(
    d: &mut D,
    target: &Target,
    check: Check<'_>,
    timeout_ms: u64,
    poll_ms: u64,
) -> ActionOutcome {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    d.set_deadline(Some(deadline));
    let out = keep_looking(d, target, check, timeout_ms, poll_ms, deadline).await;
    d.set_deadline(None);
    out
}

async fn keep_looking<D: Driver>(
    d: &mut D,
    target: &Target,
    check: Check<'_>,
    timeout_ms: u64,
    poll_ms: u64,
    deadline: Instant,
) -> ActionOutcome {
    // Whether any look has actually completed - a page that is merely slow
    // to satisfy the check is not the same failure as a browser that has
    // stopped answering, and the two must not be reported the same way.
    let mut looked = false;
    let mut last = STILL_LOOKING.to_string();
    loop {
        page::release(d).await;
        match look(d, target, &check).await {
            Ok(Ok(detail)) => return ActionOutcome::passed(detail),
            Ok(Err(why)) => {
                looked = true;
                last = why;
            }
            // The page refusing mid-navigation is the page answering, just
            // between two documents - it counts as a completed look.
            Err(e) if e.is_transient() => {
                looked = true;
                last = e.to_string();
            }
            // No new information about the page: the browser did not
            // answer THIS call. Keep going; only the deadline decides
            // whether that is the wait ending or the browser's silence.
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return harness(e),
        }
        if Instant::now() >= deadline {
            return if looked {
                ActionOutcome::failed(format!("waited {timeout_ms}ms: {} {last}", target.describe()))
            } else {
                harness_timeout(timeout_ms, &target.describe())
            };
        }
        d.idle(Duration::from_millis(poll_ms)).await;
    }
}
