//! Real input, and the wait that comes before it.
//!
//! The first runner called `el.click()` on whatever `querySelector` found.
//! That clicks things no person could: hidden, disabled, mid-animation, or
//! sitting under a modal. A pass produced that way proves nothing. Here an
//! action waits until the element could genuinely be used, then sends the
//! same mouse and keyboard events a person's hardware would.
//!
//! Typing is the one place where that is not literal: text is delivered
//! as a single text commit (`Input.insertText`, which raises `beforeinput`
//! and `input` and NO key events), and only clearing a field sends a real
//! Backspace, so a page that reacts to keydown (type-ahead search, some
//! autocompletes) will not see keys.

use super::cdp::{browser_silent, CdpError, Driver};
use super::locator::{resolve_explained, Target};
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
/// one measurement per look keeps the probe a single cheap round trip,
/// and lets Rust judge whether the element is holding still by comparing
/// the `rect` this returns ACROSS looks, a `poll_ms` apart - a decision
/// that needs more than one look anyway, so nothing is gained by waiting
/// for a second measurement inside the page. That also sidesteps timer
/// throttling: a minimised or occluded window can throttle `setTimeout`
/// (and `requestAnimationFrame` no better), which would make such a wait
/// take a second or more. `launch.rs` passes the switches that turn that
/// throttling off for a browser this app starts, but a browser the person
/// attached some other way could still throttle. The click point is the
/// middle of the part of the rect actually inside the viewport - and inside
/// every enclosing frame's visible box, so a button half hidden by a
/// frame's edge is clicked in its visible half - and is only hit-tested
/// when some of it is on screen at all.
///
/// It is aimed at the first box the element is really drawn in
/// (`getClientRects`), not the middle of its bounding box: words that wrap
/// have a box whose middle can sit past the end of a short later line,
/// where only the element around them is (PeoplesHR's "Performance
/// Management System", 2026-10-01). A block element has one box, the same
/// as its bounding box, so for it nothing changes. `rect` stays the
/// bounding box, which is what the holding-still comparison reads.
pub const PROBE_JS: &str = r#"function() {
  this.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
  const b = this.getBoundingClientRect();
  const drawn = Array.from(this.getClientRects()).find((c) =>
    c.width > 0 && c.height > 0 && c.right > 0 && c.left < innerWidth && c.bottom > 0 && c.top < innerHeight);
  const a = drawn || b;
  const l = Math.max(a.left, 0), r = Math.min(a.right, innerWidth), t = Math.max(a.top, 0), bt = Math.min(a.bottom, innerHeight);
  const onscreen = r > l && bt > t;
  // Inside a same-origin frame everything here is measured in the frame.
  // Walk out to the top window: shift the box by each frame's place on its
  // page (plus its border and padding), and clip it to each frame's visible
  // box. `out` keeps each enclosing page with the offset from this frame's
  // coordinates to that page's, for the cover checks below.
  let w = window, ox = 0, oy = 0;
  let cl = l, cr = r, ct = t, cb = bt;
  const out = [];
  while (w.frameElement) {
    const fe = w.frameElement, fr = fe.getBoundingClientRect(), pw = w.parent;
    // The frame's viewport starts inside its border AND its padding.
    const cs = pw.getComputedStyle(fe);
    const dx = fr.left + fe.clientLeft + (parseFloat(cs.paddingLeft) || 0);
    const dy = fr.top + fe.clientTop + (parseFloat(cs.paddingTop) || 0);
    ox += dx; oy += dy;
    cl = Math.max(cl + dx, fr.left, 0); cr = Math.min(cr + dx, fr.right, pw.innerWidth);
    ct = Math.max(ct + dy, fr.top, 0); cb = Math.min(cb + dy, fr.bottom, pw.innerHeight);
    out.push({ fe, pw, ox, oy });
    w = pw;
  }
  const allOnscreen = onscreen && cr > cl && cb > ct;
  // The point is the middle of what is LEFT once every frame has clipped
  // the box, so a button half hidden by a frame's edge is aimed at in its
  // visible half. `lx`/`ly` is that point in this frame's own coordinates.
  const lx = allOnscreen ? (cl + cr) / 2 - ox : (l + r) / 2;
  const ly = allOnscreen ? (ct + cb) / 2 - oy : (t + bt) / 2;
  const top = onscreen ? document.elementFromPoint(lx, ly) : null;
  const label = top && top.closest ? top.closest('label') : null;
  // Words that let a click through (pointer-events: none) to the row around
  // them: the row is what the point belongs to, and what a person's click on
  // those words reaches - it is not covering them. Only an ancestor counts;
  // anything else there really is in the way.
  const through = !!top && top !== this && top.contains(this) && getComputedStyle(this).pointerEvents === 'none';
  const hit = onscreen && !!top && (top === this || this.contains(top) || through || (label && label.control === this));
  const say = (e) => !e ? 'another element' : e.tagName.toLowerCase() + (e.id ? '#' + e.id : '') +
    (typeof e.className === 'string' && e.className.trim() ? '.' + e.className.trim().split(/\s+/).join('.') : '');
  const editable =
    (this instanceof HTMLInputElement && !this.readOnly &&
      !/^(checkbox|radio|file|button|submit|reset|image|hidden)$/.test(this.type)) ||
    (this instanceof HTMLTextAreaElement && !this.readOnly) ||
    this instanceof HTMLSelectElement || this.isContentEditable;
  // Each enclosing page must have that frame on top at the point - an
  // overlay on the page over the frame covers the element too.
  let outer = null;
  for (const o of out) {
    const there = o.pw.document.elementFromPoint(lx + o.ox, ly + o.oy);
    if (!there || (there !== o.fe && !o.fe.contains(there))) { outer = there || false; break; }
  }
  const allHit = hit && allOnscreen && outer === null;
  return {
    visible: this.checkVisibility({ visibilityProperty: true }) && b.width > 0 && b.height > 0,
    enabled: !this.disabled && this.getAttribute('aria-disabled') !== 'true' && !this.closest('fieldset[disabled]'),
    editable: !!editable,
    onscreen: allOnscreen, hit: allHit, x: lx + ox, y: ly + oy,
    rect: [b.left + ox, b.top + oy, b.width, b.height],
    covered_by: !allOnscreen || allHit ? '' : (outer !== null ? say(outer || null) : say(top)),
  };
}"#;

/// `this` is the element. Argument: the value.
///
/// A native select is set directly. A contenteditable has its contents put
/// in the selection. Everything else is focused and `select()`ed, so that
/// what is typed next replaces what was there - which is exactly what a
/// person does, and it means the page sees ONE input event carrying the
/// new value, never an intermediate empty one a validator could react to.
///
/// Measured on Edge 153 rather than assumed: `select()` selects in every
/// text-like input, `number` and `email` included. Only `selectionStart`
/// reads back `null` there, and nothing here needs to know the difference,
/// so there is no special case for them. `tests/suite/browser_live.rs` counts
/// the input events the page receives, so a special case cannot come back
/// unnoticed.
///
/// A date-like input is the one exception: typing into it is locale
/// dependent (the field wants its parts in the order the machine's locale
/// puts them, and a script writes `2026-03-05` whatever that order is), so
/// its value is set through the native setter and the page is told with
/// the same two events a person's edit would raise.
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
  this.focus();
  if (this instanceof HTMLInputElement && /^(date|time|month|week|datetime-local|color|range)$/.test(this.type)) {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(this, value);
    this.dispatchEvent(new Event('input', { bubbles: true }));
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
  try { this.select(); } catch (e) { /* typed into as it stands */ }
  return 'text';
}"#;

/// `this` is the element. `FOCUS_JS` focuses it in one round trip, and
/// the text is sent in a SEPARATE one that goes wherever focus is by
/// then - so a page that moves focus in between (an autofocusing dialog,
/// a focus trap) would take the typing and the action would still report
/// "filled <target>", a false pass. This is checked immediately before
/// anything is sent.
pub const HAS_FOCUS_JS: &str = r#"function() {
  const a = document.activeElement;
  return a === this || (this.isContentEditable && this.contains(a));
}"#;

// The words a click, a fill or a wait reports a failure in. Named so the
// one place that reads failures back - `autorun::patterns`, which groups
// the same failure across cases - classifies them by these constants,
// never by a second copy of the wording that could drift away from them.

/// Nothing on the page matches the target.
pub const NOT_FOUND: &str = "not found";
/// The one element is there, and hidden.
pub const NOT_VISIBLE: &str = "is not visible";
/// Drawn, but outside the window even after scrolling it into view.
pub const OFFSCREEN: &str = "is outside the visible part of the page";
/// The element (or its fieldset) is disabled.
pub const DISABLED: &str = "is disabled";
/// A fill aimed at something that takes no typing.
pub const NOT_EDITABLE: &str = "cannot be typed into";
/// Followed by what is in the way (`tag#id.class`, or "another element").
pub const COVERED_BY: &str = "is covered by ";
/// It never held still for two looks in a row.
pub const STILL_MOVING: &str = "is still moving";
/// The tail of [`matched_many`].
pub const MATCHED_MANY_TAIL: &str = " elements - narrow it, or add nth";
/// Followed by the reason the second look found.
pub const MOVED_BEFORE_CLICK: &str = "moved or was covered just before the click: ";
/// A fill whose field gave the focus away.
pub const LOST_FOCUS: &str = "lost focus before it could be typed into";
/// A select with no such option (followed by the option, quoted).
pub const NO_OPTION: &str = "the list has no option ";
/// A select whose option is disabled (followed by the option, quoted).
pub const OPTION: &str = "the option ";
/// The page itself refused a protocol call (followed by its message).
pub const PAGE_REFUSED: &str = "the page refused: ";

/// A target that names more than one element.
pub fn matched_many(n: usize) -> String {
    format!("matched {n}{MATCHED_MANY_TAIL}")
}

/// Why a probed element cannot be used right now, from its own flags:
/// visible, onscreen, enabled, and - only when the caller means to type
/// into it - editable, then hit. Shared between `look` (deciding whether
/// a wait is over) and `click` (re-checking the instant before it acts),
/// so the wording can never drift between the two.
fn reason(p: &Value, need_editable: bool) -> Option<String> {
    let flag = |k: &str| p[k].as_bool().unwrap_or(false);
    if !flag("visible") {
        Some(NOT_VISIBLE.to_string())
    } else if !flag("onscreen") {
        Some(OFFSCREEN.to_string())
    } else if !flag("enabled") {
        Some(DISABLED.to_string())
    } else if need_editable && !flag("editable") {
        Some(NOT_EDITABLE.to_string())
    } else if !flag("hit") {
        Some(format!("{COVERED_BY}{}", p["covered_by"].as_str().unwrap_or("another element")))
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
    let found = resolve_explained(d, target).await?;
    let handles = found.handles;
    if handles.is_empty() {
        let why = found.unreachable_frame.unwrap_or_else(|| NOT_FOUND.to_string());
        return Ok(Look::NotYet { why, rect: None });
    }
    if handles.len() > 1 && !target.is_legacy() {
        return Ok(Look::NotYet {
            why: matched_many(handles.len()),
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
        return Ok(Look::NotYet { why: STILL_MOVING.to_string(), rect: Some(rect) });
    }
    Ok(Look::Ready(Ready {
        handle,
        x: p["x"].as_f64().unwrap_or(0.0),
        y: p["y"].as_f64().unwrap_or(0.0),
    }))
}

/// What a wait loop says it last saw when its budget ran out inside a
/// call rather than between two of them.
pub(crate) const STILL_LOOKING: &str = "was still being checked when the time ran out";

/// Look, and keep looking, until the element can be used or the action
/// timeout runs out. The last reason seen is the one reported.
///
/// The deadline is pushed down into the driver so no single protocol call
/// can outlive this wait, and cleared on EVERY path out - which is why
/// the loop itself is a separate function rather than an early `return`
/// away from a `set_deadline(None)`.
pub async fn wait_ready<D: Driver>(
    d: &mut D,
    target: &Target,
    need_editable: bool,
    timing: &Timing,
) -> Result<Ready, Blocked> {
    let deadline = Instant::now() + Duration::from_millis(timing.action_ms);
    d.set_deadline(Some(deadline));
    let out = keep_looking(d, target, need_editable, timing, deadline).await;
    d.set_deadline(None);
    out
}

async fn keep_looking<D: Driver>(
    d: &mut D,
    target: &Target,
    need_editable: bool,
    timing: &Timing,
    deadline: Instant,
) -> Result<Ready, Blocked> {
    // Whether any look has actually completed - a page that is merely slow
    // to become usable is not the same failure as a browser that has
    // stopped answering, and the two must not be reported the same way.
    let mut looked = false;
    let mut prev_rect: Option<[f64; 4]> = None;
    let mut last = STILL_LOOKING.to_string();
    loop {
        page::release(d).await;
        match look(d, target, need_editable, prev_rect).await {
            Ok(Look::Ready(r)) => return Ok(r),
            Ok(Look::NotYet { why, rect }) => {
                looked = true;
                prev_rect = rect;
                last = why;
            }
            // The page refusing mid-navigation is the page answering, just
            // between two documents - it counts as a completed look.
            Err(e) if e.is_transient() => {
                looked = true;
                prev_rect = None;
                last = e.to_string();
            }
            // No new information about the page: the browser did not
            // answer THIS call. Keep going; only the deadline decides
            // whether that is the wait ending or the browser's silence.
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return Err(Blocked::Harness(e.to_string())),
        }
        if Instant::now() >= deadline {
            return Err(if looked {
                Blocked::Page(format!("waited {}ms: {} {}", timing.action_ms, target.describe(), last))
            } else {
                Blocked::Harness(browser_silent(timing.action_ms, &target.describe()))
            });
        }
        d.idle(Duration::from_millis(timing.poll_ms)).await;
    }
}

/// A thrown page error or a vanished element is the page's own doing;
/// anything else (a dead socket, a timeout, a failed send) is the
/// browser connection's. Used wherever a `CdpError` needs to become a
/// `Blocked`.
fn blame(e: CdpError) -> Blocked {
    match e {
        CdpError::Protocol { message, .. } => Blocked::Page(format!("{PAGE_REFUSED}{message}")),
        CdpError::Tab(sentence) => Blocked::Page(sentence),
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
        return Err(Blocked::Page(format!("{MOVED_BEFORE_CLICK}{why}")));
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
            return Err(Blocked::Page(format!("{NO_OPTION}\"{value}\"")));
        }
        "select-disabled" => {
            return Err(Blocked::Page(format!("{OPTION}\"{value}\" {DISABLED}")));
        }
        _ => {}
    }
    // Nothing is sent to a field that no longer has the focus: the text
    // would land somewhere else and this would still say "filled".
    let kept = page::call_value(d, &ready.handle, HAS_FOCUS_JS, &[]).await.map_err(blame)?;
    if !kept.as_bool().unwrap_or(false) {
        return Err(Blocked::Page(format!("{LOST_FOCUS} - something else on the page took it")));
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
