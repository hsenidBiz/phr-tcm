//! What the assistant may learn about a sign-in page without the page's
//! values: counts and shapes only, never an input's `.value`.

use crate::browser::cdp::Driver;

/// An expression evaluating to the number of visible password fields on the
/// page: each has client rects, a computed `visibility` that is not hidden and
/// a `display` that is not none.
pub const PASSWORD_FIELDS_JS: &str = "(() => Array.from(document.querySelectorAll('input[type=password]')).filter(e => { const s = getComputedStyle(e); return e.getClientRects().length > 0 && s.visibility !== 'hidden' && s.display !== 'none'; }).length)()";

/// How many password fields are visible now; anything the browser answers
/// that is not a number is none.
pub async fn visible_password_fields<D: Driver>(d: &mut D) -> usize {
    crate::browser::page::eval_value(d, PASSWORD_FIELDS_JS)
        .await
        .ok()
        .and_then(|v| v.as_u64())
        .map_or(0, |n| n as usize)
}
