//! Real input, and the wait that comes before it.
//!
//! The first runner called `el.click()` on whatever `querySelector` found.
//! That clicks things no person could: hidden, disabled, mid-animation, or
//! sitting under a modal. A pass produced that way proves nothing. Here an
//! action waits until the element could genuinely be used, then sends the
//! same mouse and keyboard events a person's hardware would.

use super::cdp::{CdpError, Driver};
use super::locator::{resolve, Target};
use super::page::{self, Handle};
use super::timing::Timing;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Why an action did not happen.
#[derive(Debug, Clone, PartialEq)]
pub enum Blocked {
    /// The page's doing, in words for the person watching.
    Page(String),
    /// The browser connection's doing. The app under test did nothing wrong.
    Harness(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ready {
    /// A DevTools object in the shared object group: the next `wait_ready`
    /// call (or any `page::release`) frees every handle in that group, so
    /// a `Ready` must be used before another wait starts.
    pub handle: Handle,
    /// Viewport coordinates of the element's centre, in CSS pixels.
    pub x: f64,
    pub y: f64,
}

/// `this` is the element. A synchronous, single measurement, on purpose:
/// measured on real Edge, a minimised or occluded window throttles page
/// timers (`setTimeout`, and `requestAnimationFrame` no better), so a
/// probe that waited on one for a second measurement could itself take a
/// second or more and risk the call's own 30s timeout. Whether the
/// element is holding still is instead judged in Rust, by comparing the
/// `rect` this returns across two polls a `poll_ms` apart. The click
/// point is clamped to the part of the rect actually inside the
/// viewport, and only hit-tested when some of it is on screen at all.
pub const PROBE_JS: &str = r#"function() {
  this.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
  const b = this.getBoundingClientRect();
  const l = Math.max(b.left, 0), r = Math.min(b.right, innerWidth), t = Math.max(b.top, 0), bt = Math.min(b.bottom, innerHeight);
  const onscreen = r > l && bt > t;
  const x = (l + r) / 2, y = (t + bt) / 2;
  const top = onscreen ? document.elementFromPoint(x, y) : null;
  const label = top && top.closest ? top.closest('label') : null;
  const hit = onscreen && !!top && (top === this || this.contains(top) || (label && label.control === this));
  const say = (e) => !e ? 'another element' : e.tagName.toLowerCase() + (e.id ? '#' + e.id : '') +
    (typeof e.className === 'string' && e.className.trim() ? '.' + e.className.trim().split(/\s+/).join('.') : '');
  const editable =
    (this instanceof HTMLInputElement && !this.readOnly &&
      !/^(checkbox|radio|file|button|submit|reset|image|hidden)$/.test(this.type)) ||
    (this instanceof HTMLTextAreaElement && !this.readOnly) ||
    this instanceof HTMLSelectElement || this.isContentEditable;
  return {
    visible: this.checkVisibility({ visibilityProperty: true }) && b.width > 0 && b.height > 0,
    enabled: !this.disabled && this.getAttribute('aria-disabled') !== 'true' && !this.closest('fieldset[disabled]'),
    editable: !!editable,
    onscreen, hit, x, y, rect: [b.left, b.top, b.width, b.height],
    covered_by: (onscreen && !hit) ? say(top) : '',
  };
}"#;

/// `this` is the element. Argument: the value. A native select is set
/// directly. A date-like input does not accept typed characters at all,
/// so it is set through its native value setter instead. Anything else
/// is focused with its contents selected, so what is typed next replaces
/// them - except where `select()` is a documented no-op (number, email
/// and friends), where the value is cleared through the same native
/// setter so typing does not concatenate onto what was already there.
pub const FOCUS_JS: &str = r#"function(value) {
  if (this instanceof HTMLSelectElement) {
    const want = String(value).trim();
    const usable = (o) => !o.disabled && !(o.parentElement instanceof HTMLOptGroupElement && o.parentElement.disabled);
    const all = Array.from(this.options);
    const opt = all.find((o) => o.label.trim() === want || o.value === value)
      || all.find((o) => o.label.trim().toLowerCase() === want.toLowerCase());
    if (!opt) return 'select-missing';
    if (!usable(opt)) return 'select-disabled';
    this.value = opt.value;
    this.dispatchEvent(new Event('input', { bubbles: true }));
    this.dispatchEvent(new Event('change', { bubbles: true }));
    return 'select-ok';
  }
  const setNative = (el, v) => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };
  this.focus();
  if (this instanceof HTMLInputElement && /^(date|time|month|week|datetime-local|color|range)$/.test(this.type)) {
    setNative(this, value);
    this.dispatchEvent(new Event('change', { bubbles: true }));
    return 'set';
  }
  if (this.isContentEditable) {
    const r = document.createRange();
    r.selectNodeContents(this);
    const s = getSelection();
    s.removeAllRanges();
    s.addRange(r);
    return 'text';
  }
  let selectable = false;
  try { this.select(); selectable = this.selectionStart !== null && this.selectionStart !== undefined; } catch (e) { selectable = false; }
  if (!selectable && this.value !== '') setNative(this, '');
  return 'text';
}"#;

/// Why a probed element cannot be used right now, from its own flags:
/// visible, onscreen, enabled, and - only when the caller means to type
/// into it - editable, then hit. Shared between `look` (deciding whether
/// a wait is over) and `click` (re-checking the instant before it acts),
/// so the wording can never drift between the two.
fn reason(p: &Value, need_editable: bool) -> Option<String> {
    let flag = |k: &str| p[k].as_bool().unwrap_or(false);
    if !flag("visible") {
        Some("is not visible".to_string())
    } else if !flag("onscreen") {
        Some("is outside the visible part of the page".to_string())
    } else if !flag("enabled") {
        Some("is disabled".to_string())
    } else if need_editable && !flag("editable") {
        Some("cannot be typed into".to_string())
    } else if !flag("hit") {
        Some(format!("is covered by {}", p["covered_by"].as_str().unwrap_or("another element")))
    } else {
        None
    }
}

fn read_rect(p: &Value) -> [f64; 4] {
    let at = |i: usize| p["rect"][i].as_f64().unwrap_or(0.0);
    [at(0), at(1), at(2), at(3)]
}

enum Look {
    Ready(Ready),
    /// `rect` is `Some` only when this look was otherwise ready and is
    /// being held back purely for not yet matching the previous look's
    /// rect - the one case where the next look must remember it.
    NotYet { why: String, rect: Option<[f64; 4]> },
}

async fn look<D: Driver>(
    d: &mut D,
    target: &Target,
    need_editable: bool,
    prev_rect: Option<[f64; 4]>,
) -> Result<Look, CdpError> {
    let handles = resolve(d, target).await?;
    if handles.is_empty() {
        return Ok(Look::NotYet { why: "not found".to_string(), rect: None });
    }
    if handles.len() > 1 && !target.is_legacy() {
        return Ok(Look::NotYet {
            why: format!("matched {} elements - narrow it, or add nth", handles.len()),
            rect: None,
        });
    }
    let handle = handles.into_iter().next().expect("checked non-empty");
    let p = page::call_value(d, &handle, PROBE_JS, &[]).await?;
    if let Some(why) = reason(&p, need_editable) {
        return Ok(Look::NotYet { why, rect: None });
    }
    // Everything the flags alone can say is fine, so the one thing left
    // to check is whether it is still moving: it must be seen holding the
    // same rect across two polls, one `poll_ms` apart, before it counts
    // as ready.
    let rect = read_rect(&p);
    if Some(rect) != prev_rect {
        return Ok(Look::NotYet { why: "is still moving".to_string(), rect: Some(rect) });
    }
    Ok(Look::Ready(Ready {
        handle,
        x: p["x"].as_f64().unwrap_or(0.0),
        y: p["y"].as_f64().unwrap_or(0.0),
    }))
}

/// Look, and keep looking, until the element can be used or the action
/// timeout runs out. The last reason seen is the one reported.
pub async fn wait_ready<D: Driver>(
    d: &mut D,
    target: &Target,
    need_editable: bool,
    timing: &Timing,
) -> Result<Ready, Blocked> {
    let deadline = Instant::now() + Duration::from_millis(timing.action_ms);
    let mut prev_rect: Option<[f64; 4]> = None;
    loop {
        page::release(d).await;
        let why = match look(d, target, need_editable, prev_rect).await {
            Ok(Look::Ready(r)) => return Ok(r),
            Ok(Look::NotYet { why, rect }) => {
                prev_rect = rect;
                why
            }
            Err(e) if e.is_transient() => {
                prev_rect = None;
                e.to_string()
            }
            Err(e) => return Err(Blocked::Harness(e.to_string())),
        };
        if Instant::now() >= deadline {
            return Err(Blocked::Page(format!(
                "waited {}ms: {} {}",
                timing.action_ms,
                target.describe(),
                why
            )));
        }
        tokio::time::sleep(Duration::from_millis(timing.poll_ms)).await;
    }
}

/// A thrown page error or a vanished element is the page's own doing;
/// anything else (a dead socket, a timeout, a failed send) is the
/// browser connection's. Used wherever a `CdpError` needs to become a
/// `Blocked`.
fn blame(e: CdpError) -> Blocked {
    match e {
        CdpError::Protocol { message, .. } => Blocked::Page(format!("the page refused: {message}")),
        other => Blocked::Harness(other.to_string()),
    }
}

/// Re-probes `ready.handle` first: the caller pauses for a highlight
/// between `wait_ready` and `click`, so the coordinate `Ready` carries is
/// stale by design. Clicks at the freshly probed point, never the one
/// `wait_ready` returned.
pub async fn click<D: Driver>(d: &mut D, ready: &Ready) -> Result<(), Blocked> {
    let p = page::call_value(d, &ready.handle, PROBE_JS, &[]).await.map_err(blame)?;
    if let Some(why) = reason(&p, false) {
        return Err(Blocked::Page(format!("moved or was covered just before the click: {why}")));
    }
    let x = p["x"].as_f64().unwrap_or(ready.x);
    let y = p["y"].as_f64().unwrap_or(ready.y);
    for (kind, button, count, buttons) in [
        ("mouseMoved", "none", 0, 0),
        ("mousePressed", "left", 1, 1),
        ("mouseReleased", "left", 1, 0),
    ] {
        d.call(
            "Input.dispatchMouseEvent",
            json!({ "type": kind, "x": x, "y": y, "button": button, "clickCount": count, "buttons": buttons }),
        )
        .await
        .map_err(blame)?;
    }
    Ok(())
}

pub async fn fill<D: Driver>(d: &mut D, ready: &Ready, value: &str) -> Result<(), Blocked> {
    let kind = page::call_value(d, &ready.handle, FOCUS_JS, &[json!(value)]).await.map_err(blame)?;
    match kind.as_str().unwrap_or("text") {
        "select-ok" | "set" => return Ok(()),
        "select-missing" => {
            return Err(Blocked::Page(format!("the list has no option \"{value}\"")));
        }
        "select-disabled" => {
            return Err(Blocked::Page(format!("the option \"{value}\" is disabled")));
        }
        _ => {}
    }
    if value.is_empty() {
        for kind in ["keyDown", "keyUp"] {
            d.call(
                "Input.dispatchKeyEvent",
                json!({
                    "type": kind, "key": "Backspace", "code": "Backspace",
                    "windowsVirtualKeyCode": 8, "nativeVirtualKeyCode": 8
                }),
            )
            .await
            .map_err(blame)?;
        }
        return Ok(());
    }
    d.call("Input.insertText", json!({ "text": value })).await.map_err(blame)?;
    Ok(())
}
