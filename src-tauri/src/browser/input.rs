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
use serde_json::json;
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
    pub handle: Handle,
    /// Viewport coordinates of the element's centre, in CSS pixels.
    pub x: f64,
    pub y: f64,
}

/// `this` is the element. Scrolls it into view, measures it twice 50 ms
/// apart (a timer, not requestAnimationFrame: a minimised window stops
/// painting and the frame callback would never fire), and hit-tests its
/// centre.
pub const PROBE_JS: &str = r#"async function() {
  this.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
  const a = this.getBoundingClientRect();
  await new Promise((r) => setTimeout(r, 50));
  const b = this.getBoundingClientRect();
  const x = b.left + b.width / 2, y = b.top + b.height / 2;
  const top = document.elementFromPoint(x, y);
  const label = top && top.closest ? top.closest('label') : null;
  const hit = !!top && (top === this || this.contains(top) || (label && label.control === this));
  const say = (e) => !e ? 'nothing' : e.tagName.toLowerCase() + (e.id ? '#' + e.id : '') +
    (typeof e.className === 'string' && e.className.trim() ? '.' + e.className.trim().split(/\s+/).join('.') : '');
  const editable =
    (this instanceof HTMLInputElement && !this.readOnly &&
      !/^(checkbox|radio|file|button|submit|reset|image|range|color|hidden)$/.test(this.type)) ||
    (this instanceof HTMLTextAreaElement && !this.readOnly) ||
    this instanceof HTMLSelectElement || this.isContentEditable;
  return {
    visible: this.checkVisibility({ visibilityProperty: true }) && b.width > 0 && b.height > 0,
    enabled: !this.disabled && this.getAttribute('aria-disabled') !== 'true' && !this.closest('fieldset[disabled]'),
    editable: !!editable,
    stable: a.left === b.left && a.top === b.top && a.width === b.width && a.height === b.height,
    hit, x, y, covered_by: hit ? '' : say(top),
  };
}"#;

/// `this` is the element. Argument: the value. A native select is set
/// here; anything else is focused with its contents selected, so what is
/// typed next replaces them.
pub const FOCUS_JS: &str = r#"function(value) {
  if (this instanceof HTMLSelectElement) {
    const opt = Array.from(this.options).find((o) => o.label.trim() === value.trim() || o.value === value);
    if (!opt) return 'select-missing';
    this.value = opt.value;
    this.dispatchEvent(new Event('input', { bubbles: true }));
    this.dispatchEvent(new Event('change', { bubbles: true }));
    return 'select-ok';
  }
  this.focus();
  if (this.isContentEditable) {
    const r = document.createRange();
    r.selectNodeContents(this);
    const s = getSelection();
    s.removeAllRanges();
    s.addRange(r);
  } else if (typeof this.select === 'function') {
    this.select();
  }
  return 'text';
}"#;

enum Look {
    Ready(Ready),
    NotYet(String),
}

async fn look<D: Driver>(d: &mut D, target: &Target, need_editable: bool) -> Result<Look, CdpError> {
    let handles = resolve(d, target).await?;
    if handles.is_empty() {
        return Ok(Look::NotYet("not found".to_string()));
    }
    if handles.len() > 1 && !target.is_legacy() {
        return Ok(Look::NotYet(format!(
            "matched {} elements - narrow it, or add nth",
            handles.len()
        )));
    }
    let handle = handles.into_iter().next().expect("checked non-empty");
    let p = page::call_value(d, &handle, PROBE_JS, &[]).await?;
    let flag = |k: &str| p[k].as_bool().unwrap_or(false);
    let why = if !flag("visible") {
        "is not visible".to_string()
    } else if !flag("stable") {
        "is still moving".to_string()
    } else if !flag("enabled") {
        "is disabled".to_string()
    } else if need_editable && !flag("editable") {
        "cannot be typed into".to_string()
    } else if !flag("hit") {
        format!("is covered by {}", p["covered_by"].as_str().unwrap_or("another element"))
    } else {
        return Ok(Look::Ready(Ready {
            handle,
            x: p["x"].as_f64().unwrap_or(0.0),
            y: p["y"].as_f64().unwrap_or(0.0),
        }));
    };
    Ok(Look::NotYet(why))
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
    loop {
        page::release(d).await;
        let why = match look(d, target, need_editable).await {
            Ok(Look::Ready(r)) => return Ok(r),
            Ok(Look::NotYet(why)) => why,
            Err(e) if e.is_transient() => e.to_string(),
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

pub async fn click<D: Driver>(d: &mut D, ready: &Ready) -> Result<(), CdpError> {
    for (kind, button, count) in
        [("mouseMoved", "none", 0), ("mousePressed", "left", 1), ("mouseReleased", "left", 1)]
    {
        d.call(
            "Input.dispatchMouseEvent",
            json!({ "type": kind, "x": ready.x, "y": ready.y, "button": button, "clickCount": count }),
        )
        .await?;
    }
    Ok(())
}

fn harness(e: CdpError) -> Blocked {
    Blocked::Harness(e.to_string())
}

pub async fn fill<D: Driver>(d: &mut D, ready: &Ready, value: &str) -> Result<(), Blocked> {
    let kind = page::call_value(d, &ready.handle, FOCUS_JS, &[json!(value)]).await.map_err(harness)?;
    match kind.as_str().unwrap_or("text") {
        "select-ok" => return Ok(()),
        "select-missing" => {
            return Err(Blocked::Page(format!("the list has no option \"{value}\"")));
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
            .map_err(harness)?;
        }
        return Ok(());
    }
    d.call("Input.insertText", json!({ "text": value })).await.map_err(harness)?;
    Ok(())
}
