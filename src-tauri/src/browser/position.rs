//! `expect_position`: where one element sits against another.
//!
//! A person says "the New button is in the top-right of the bar", not "it is
//! at x=702". Both elements are measured in the window (an element in a
//! frame is moved into the top window's frame of reference), and the
//! relation is judged with a pixel of give for borders and rounding. No
//! pixel size is ever asked for.

use super::cdp::{CdpError, Driver};
use super::page::{self, Handle};

/// Where the first element sits against the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    LeftOf,
    RightOf,
    Above,
    Below,
    Inside,
    InTopLeft,
    InTopRight,
    InBottomLeft,
    InBottomRight,
}

impl Relation {
    /// The relation in words, as a sentence says it: `is {words} {ref}`.
    pub fn words(self) -> &'static str {
        match self {
            Relation::LeftOf => "left of",
            Relation::RightOf => "right of",
            Relation::Above => "above",
            Relation::Below => "below",
            Relation::Inside => "inside",
            Relation::InTopLeft => "in the top-left of",
            Relation::InTopRight => "in the top-right of",
            Relation::InBottomLeft => "in the bottom-left of",
            Relation::InBottomRight => "in the bottom-right of",
        }
    }
}

/// A box in the top window: its corner, and its size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// How far a relation may be off by: borders, and rounding.
pub const TOLERANCE_PX: f64 = 1.0;

/// `this` is the element. `{ x, y, w, h }`: its `getBoundingClientRect()`
/// moved into the top window's frame by adding each frame element's own
/// left and top, up the `parent` chain. A parent from another site cannot
/// be asked: the walk stops there, silently, with what it has.
pub const READ_BOX_JS: &str = r#"function() {
  const r = this.getBoundingClientRect();
  let x = r.left, y = r.top;
  try {
    let win = this.ownerDocument.defaultView;
    while (win && win.parent && win !== win.parent) {
      const frame = win.frameElement;
      if (!frame) break;
      const f = frame.getBoundingClientRect();
      x += f.left + (frame.clientLeft || 0);
      y += f.top + (frame.clientTop || 0);
      win = win.parent;
    }
  } catch (_) {}
  return { x: x, y: y, w: r.width, h: r.height };
}"#;

/// Does `rel` hold of `a` against `b`? A corner is being inside `b` and the
/// centre of `a` strictly in that half of `b` both ways.
pub fn holds(rel: Relation, a: Rect, b: Rect) -> bool {
    let t = TOLERANCE_PX;
    let inside = a.x >= b.x - t && a.y >= b.y - t && a.x + a.w <= b.x + b.w + t && a.y + a.h <= b.y + b.h + t;
    let (cx, cy) = (a.x + a.w / 2.0, a.y + a.h / 2.0);
    let (mx, my) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
    match rel {
        Relation::LeftOf => a.x + a.w <= b.x + t,
        Relation::RightOf => a.x >= b.x + b.w - t,
        Relation::Above => a.y + a.h <= b.y + t,
        Relation::Below => a.y >= b.y + b.h - t,
        Relation::Inside => inside,
        Relation::InTopLeft => inside && cx < mx && cy < my,
        Relation::InTopRight => inside && cx > mx && cy < my,
        Relation::InBottomLeft => inside && cx < mx && cy > my,
        Relation::InBottomRight => inside && cx > mx && cy > my,
    }
}

/// A box in a sentence: `(x, y) WxH`, to whole pixels.
pub fn shown(r: &Rect) -> String {
    format!("({}, {}) {}x{}", r.x.round() as i64, r.y.round() as i64, r.w.round() as i64, r.h.round() as i64)
}

/// The element's box in the top window, or `None` when the page did not
/// answer with one.
pub(crate) async fn read_box<D: Driver>(d: &mut D, h: &Handle) -> Result<Option<Rect>, CdpError> {
    let v = page::call_value(d, h, READ_BOX_JS, &[]).await?;
    let n = |k: &str| v.get(k).and_then(serde_json::Value::as_f64);
    Ok(match (n("x"), n("y"), n("w"), n("h")) {
        (Some(x), Some(y), Some(w), Some(h)) => Some(Rect { x, y, w, h }),
        _ => None,
    })
}
