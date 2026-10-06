//! `drag`: pick one element up and drop it on another, as a person's mouse
//! would.
//!
//! Pages drag in one of two ways, and the driver cannot tell which before it
//! starts, so one gesture serves both:
//!
//! - **Mouse or pointer events** (most sortable lists, grid row
//!   reordering): press on `from`'s centre, move 10 px to start the drag,
//!   move in 10 steps to the drop point with an animation frame between
//!   each, release. Chrome raises pointer events from the mouse events.
//! - **The browser's own drag and drop** (`draggable`): the browser starts
//!   a drag of its own once the mouse moves, which a protocol client cannot
//!   steer. Drag interception (`Input.setInterceptDrags`) is on for the
//!   whole gesture, so the browser hands that drag over
//!   (`Input.dragIntercepted`) instead; the driver then completes it at the
//!   drop point with `Input.dispatchDragEvent` - `dragEnter`, `dragOver`,
//!   `drop` - and lets go of the mouse.
//!
//! Interception is turned off again on every way out, a failure or a
//! timeout included: left on, it would swallow the person's own drags.
//!
//! An action does not check its own effect: a drag the page ignored still
//! passes. The script follows it with a check of the new order.

use super::actions::{blocked, failed_by, ActionOutcome, DropAt, HIGHLIGHT_JS};
use super::cdp::{CdpError, Driver};
use super::input::{self, Blocked};
use super::locator::{resolve_explained, Target};
use super::page::{self, Handle};
use super::timing::Timing;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// How long a drag may take when it names no `within_ms`.
pub const DRAG_WAIT_MS: u32 = 10_000;
/// The longest a drag may take.
pub const DRAG_WAIT_MAX_MS: u32 = 60_000;
/// How far the first move goes, to start the drag.
pub const NUDGE_PX: f64 = 10.0;
/// How many moves take the mouse from there to the drop point.
pub const STEPS: u32 = 10;

/// `from` was not found, or could not be used.
pub fn nothing_to_drag(from: &str) -> String {
    format!("there was nothing to drag at {from}")
}

/// `to` was not found, or could not be used.
pub fn nowhere_to_drop(to: &str) -> String {
    format!("there was nowhere to drop at {to}")
}

/// What a drag that ran out of time starts with.
pub const DID_NOT_FINISH: &str = "the drag did not finish within ";

/// `the drag did not finish within <n> seconds`.
pub fn did_not_finish(within_ms: u32) -> String {
    let s = if within_ms % 1000 == 0 {
        (within_ms / 1000).to_string()
    } else {
        format!("{:.1}", f64::from(within_ms) / 1000.0)
    };
    format!("{DID_NOT_FINISH}{s} seconds")
}

/// Added to a drag's outcome when the browser would not stop intercepting
/// drags afterwards.
pub const DRAGS_STILL_HELD: &str =
    " (the browser did not confirm it stopped intercepting drags - if dragging misbehaves, close the browser and open it again)";

/// `this` is the element. Argument: whether to scroll it into view first
/// (only as far as needed, so an element already showing stays put). Its
/// box in the TOP window's coordinates - shifted out through every
/// enclosing same-origin frame the way `input::PROBE_JS` shifts its click
/// point - and the top window's size.
pub const RECT_JS: &str = r#"function(scroll) {
  if (scroll) this.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
  const b = this.getBoundingClientRect();
  let w = window, ox = 0, oy = 0;
  while (w.frameElement) {
    const fe = w.frameElement, fr = fe.getBoundingClientRect(), cs = w.parent.getComputedStyle(fe);
    ox += fr.left + fe.clientLeft + (parseFloat(cs.paddingLeft) || 0);
    oy += fr.top + fe.clientTop + (parseFloat(cs.paddingTop) || 0);
    w = w.parent;
  }
  return { left: b.left + ox, top: b.top + oy, width: b.width, height: b.height, vw: w.innerWidth, vh: w.innerHeight };
}"#;

/// One animation frame: what a drag waits between two moves, so a page
/// that updates on `requestAnimationFrame` sees each one.
pub const NEXT_FRAME_JS: &str = "new Promise((r) => requestAnimationFrame(() => r(true)))";

/// An element's box, as `RECT_JS` measures it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
    pub vw: f64,
    pub vh: f64,
}

impl Rect {
    fn read(v: &Value) -> Rect {
        let at = |k: &str| v[k].as_f64().unwrap_or(0.0);
        Rect { left: at("left"), top: at("top"), width: at("width"), height: at("height"), vw: at("vw"), vh: at("vh") }
    }

    pub fn centre(&self) -> (f64, f64) {
        (self.left + self.width / 2.0, self.top + self.height / 2.0)
    }

    /// Where on it to drop: a quarter of the way down for `before`, three
    /// quarters for `after`, the middle for `onto`.
    pub fn drop_point(&self, at: DropAt) -> (f64, f64) {
        let down = match at {
            DropAt::Before => 0.25,
            DropAt::After => 0.75,
            DropAt::Onto => 0.5,
        };
        (self.left + self.width / 2.0, self.top + self.height * down)
    }

    fn shows(&self, (x, y): (f64, f64)) -> bool {
        x >= 0.0 && y >= 0.0 && x < self.vw && y < self.vh
    }
}

/// The point `NUDGE_PX` from `from`, toward `to` (straight down when they
/// are the same point).
pub fn nudge(from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return (from.0, from.1 + NUDGE_PX);
    }
    (from.0 + dx / len * NUDGE_PX, from.1 + dy / len * NUDGE_PX)
}

async fn rect<D: Driver>(d: &mut D, on: &Handle, scroll: bool) -> Result<Rect, CdpError> {
    Ok(Rect::read(&page::call_value(d, on, RECT_JS, &[json!(scroll)]).await?))
}

/// Carry out a `drag`. `within_ms` bounds the gesture itself; finding the
/// two elements takes the ordinary action wait first.
pub async fn drag<D: Driver>(
    d: &mut D,
    from: &Target,
    to: &Target,
    at: DropAt,
    within_ms: u32,
    timing: &Timing,
) -> ActionOutcome {
    let (from_words, to_words) = (from.describe(), to.describe());
    let nothing = |why: &str| ActionOutcome::failed(format!("{}: {why}", nothing_to_drag(&from_words)));
    let nowhere = |why: &str| ActionOutcome::failed(format!("{}: {why}", nowhere_to_drop(&to_words)));

    // Both are waited for as a click waits for its element (and scrolled
    // into view by the look).
    match input::wait_ready(d, from, false, timing).await {
        Ok(_) => {}
        Err(Blocked::Page(why)) => return nothing(&why),
        Err(b) => return blocked(b),
    }
    let to_ready = match input::wait_ready(d, to, false, timing).await {
        Ok(r) => r,
        Err(Blocked::Page(why)) => return nowhere(&why),
        Err(b) => return blocked(b),
    };
    // A wait lets go of every handle it did not return, so `from` is found
    // again here - without letting go of `to`'s.
    let from_handle = match resolve_explained(d, from).await {
        Ok(r) if r.handles.len() == 1 || (!r.handles.is_empty() && from.is_legacy()) => r.handles[0].clone(),
        Ok(r) if r.handles.is_empty() => {
            return nothing(r.unreachable_frame.as_deref().unwrap_or(input::NOT_FOUND));
        }
        Ok(r) => return nothing(&input::matched_many(r.handles.len())),
        Err(e) => return failed_by(e),
    };
    if let Err(e) = page::call_value(d, &from_handle, HIGHLIGHT_JS, &[]).await {
        return failed_by(e);
    }
    if timing.highlight_ms > 0 {
        d.idle(Duration::from_millis(timing.highlight_ms)).await;
    }

    // Both into view - each only as far as it needs, so the second does
    // not push the first back out - then both measured where they now are.
    let measured = async {
        rect(d, &from_handle, true).await?;
        rect(d, &to_ready.handle, true).await?;
        Ok::<_, CdpError>((rect(d, &from_handle, false).await?, rect(d, &to_ready.handle, false).await?))
    }
    .await;
    let (from_box, to_box) = match measured {
        Ok(b) => b,
        Err(e) => return failed_by(e),
    };
    let start = from_box.centre();
    let drop = to_box.drop_point(at);
    if !from_box.shows(start) {
        return nothing(input::OFFSCREEN);
    }
    if !to_box.shows(drop) {
        return nowhere(input::OFFSCREEN);
    }

    let deadline = Instant::now() + Duration::from_millis(u64::from(within_ms));
    d.set_deadline(Some(deadline));
    let mut held = false;
    let result = match d.call("Input.setInterceptDrags", json!({ "enabled": true })).await {
        // Switched off below anyway: a browser that refused may still have
        // switched it on.
        Err(e) => Err(ended(e, within_ms)),
        Ok(_) => gesture(d, start, drop, deadline, within_ms, &mut held).await,
    };
    d.set_deadline(None);
    // A gesture cut short leaves the button down; let it go where it was
    // headed, so the page is not left mid-drag.
    if held {
        let _ = mouse(d, "mouseReleased", drop, "left", 1, 0).await;
    }
    let switched_off = match d.call("Input.setInterceptDrags", json!({ "enabled": false })).await {
        Ok(_) => true,
        Err(e) => {
            crate::applog::warn(format!("drag: drag interception could not be switched off: {e}"));
            false
        }
    };
    let mut out = match result {
        Ok(()) => ActionOutcome::passed(format!("dragged {from_words} {} {to_words}", at.word())),
        Err(out) => out,
    };
    if !switched_off {
        out.detail.push_str(DRAGS_STILL_HELD);
    }
    out
}

/// A call that failed partway through the gesture: running out of time is
/// the drag not finishing; anything else is what it always is.
fn ended(e: CdpError, within_ms: u32) -> ActionOutcome {
    match e {
        CdpError::Timeout { .. } => ActionOutcome::failed(did_not_finish(within_ms)),
        other => failed_by(other),
    }
}

async fn mouse<D: Driver>(
    d: &mut D,
    kind: &str,
    (x, y): (f64, f64),
    button: &str,
    count: u32,
    buttons: u32,
) -> Result<Value, CdpError> {
    d.call(
        "Input.dispatchMouseEvent",
        json!({ "type": kind, "x": x, "y": y, "button": button, "clickCount": count, "buttons": buttons }),
    )
    .await
}

/// The data of a drag the browser handed over, if it has.
async fn intercepted<D: Driver>(d: &mut D) -> Result<Option<Value>, CdpError> {
    match d.wait_event("Input.dragIntercepted", Duration::ZERO).await {
        Ok(ev) => Ok(Some(ev.params["data"].clone())),
        Err(CdpError::Timeout { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// One animation frame. The page refusing (a document going away mid-drag)
/// is no reason to stop: the moves themselves say whether it still can.
async fn next_frame<D: Driver>(d: &mut D) -> Result<(), CdpError> {
    match page::eval_value(d, NEXT_FRAME_JS).await {
        Ok(_) | Err(CdpError::Protocol { .. }) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Press, nudge, move, and either release (the mouse path) or finish the
/// browser's own drag (the drag-and-drop path). `held` is whether the
/// button is down, for the caller to let go of on a way out.
async fn gesture<D: Driver>(
    d: &mut D,
    start: (f64, f64),
    drop: (f64, f64),
    deadline: Instant,
    within_ms: u32,
    held: &mut bool,
) -> Result<(), ActionOutcome> {
    let late = || ActionOutcome::failed(did_not_finish(within_ms));
    let fail = |e: CdpError| ended(e, within_ms);
    // A drag handed over by an earlier gesture must not stand in for this
    // one's.
    d.forget_events();
    mouse(d, "mouseMoved", start, "none", 0, 0).await.map_err(fail)?;
    mouse(d, "mousePressed", start, "left", 1, 1).await.map_err(fail)?;
    *held = true;
    let first = nudge(start, drop);
    mouse(d, "mouseMoved", first, "left", 0, 1).await.map_err(fail)?;
    if let Some(data) = intercepted(d).await.map_err(fail)? {
        return finish_native(d, drop, data, held).await.map_err(fail);
    }
    for i in 1..=STEPS {
        if Instant::now() >= deadline {
            return Err(late());
        }
        next_frame(d).await.map_err(fail)?;
        let f = f64::from(i) / f64::from(STEPS);
        let p = (first.0 + (drop.0 - first.0) * f, first.1 + (drop.1 - first.1) * f);
        mouse(d, "mouseMoved", p, "left", 0, 1).await.map_err(fail)?;
        if let Some(data) = intercepted(d).await.map_err(fail)? {
            return finish_native(d, drop, data, held).await.map_err(fail);
        }
    }
    if Instant::now() >= deadline {
        return Err(late());
    }
    // One more frame for a drag the browser hands over late.
    next_frame(d).await.map_err(fail)?;
    if let Some(data) = intercepted(d).await.map_err(fail)? {
        return finish_native(d, drop, data, held).await.map_err(fail);
    }
    mouse(d, "mouseReleased", drop, "left", 1, 0).await.map_err(fail)?;
    *held = false;
    Ok(())
}

/// The browser's own drag, completed at the drop point, then the mouse let
/// go.
async fn finish_native<D: Driver>(
    d: &mut D,
    drop: (f64, f64),
    data: Value,
    held: &mut bool,
) -> Result<(), CdpError> {
    for kind in ["dragEnter", "dragOver", "drop"] {
        d.call(
            "Input.dispatchDragEvent",
            json!({ "type": kind, "x": drop.0, "y": drop.1, "data": data, "modifiers": 0 }),
        )
        .await?;
    }
    mouse(d, "mouseReleased", drop, "left", 1, 0).await?;
    *held = false;
    Ok(())
}
