//! `expect_order`: the texts of a list come in this order.
//!
//! The matches of a locator, in page order, contain the given texts in the
//! given order; other items may sit between them (Playwright's
//! `toContainText([...])`). Each text is compared as `expect_contains_text`
//! compares: whitespace collapsed, case kept, and an item only has to
//! CONTAIN the text.
//!
//! The list is read the way `expect_each` reads one: every item read is
//! followed by a probe that it is still in the page (a detached one makes
//! the look fail with [`LIST_CHANGED`], which the loop looks again on), a
//! list longer than [`ORDER_MAX`] is refused rather than read for ever, and
//! a failure shows items by what was compared - but a text field by the
//! name a person knows it by, never the value typed into it.

use super::cdp::{CdpError, Driver};
use super::each::{CONNECTED_JS, LIST_CHANGED, TOO_SLOW};
use super::expect::{NOT_ON_PAGE, READ_TEXT_JS};
use super::page::{self, Handle};

/// The most items `expect_order` reads.
pub const ORDER_MAX: usize = 200;
/// The fewest texts it takes: for one, `expect_contains_text`.
pub const TEXTS_MIN: usize = 2;
/// The most texts it takes.
pub const TEXTS_MAX: usize = 50;
/// How many of the items a failure shows.
const SEEN_ITEMS: usize = 10;
/// How many characters of an item a failure shows.
const SEEN_CHARS: usize = 40;

/// `this` is an item. What a failure shows it as: what was compared (its
/// text; a `<select>`'s chosen option), but for a text field the name a
/// person knows it by (its aria-label, its label, its placeholder or name,
/// else its tag) - never the value typed into it, a password's included.
pub const SHOWN_JS: &str = r#"function() {
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  if (this instanceof HTMLSelectElement) return norm(this.selectedOptions[0] ? this.selectedOptions[0].label : '');
  if (this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement) {
    const labelled = Array.from(this.labels || []).map((l) => norm(l.innerText)).filter(Boolean).join(' ');
    return norm(this.getAttribute('aria-label')) || labelled || norm(this.getAttribute('placeholder')) ||
      norm(this.getAttribute('name')) || this.tagName.toLowerCase();
  }
  return norm(this.innerText);
}"#;

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The texts as a sentence quotes them: `"a", "b"`.
fn list_of(texts: &[String]) -> String {
    texts.iter().map(|t| format!("{t:?}")).collect::<Vec<_>>().join(", ")
}

/// Why the order does not hold.
enum Fault {
    /// The first text is in none of the items.
    InNone(String),
    /// This text is in no item after the one that held the text before it.
    NotAfter { text: String, previous: String },
}

/// The first text that has no place, going through the texts greedily: each
/// is matched by the earliest item after the previous one that contains it,
/// and each item holds at most one text.
fn find_fault(want: &[String], items: &[String]) -> Option<Fault> {
    let items: Vec<String> = items.iter().map(|i| collapse(i)).collect();
    let mut from = 0;
    let mut previous: Option<String> = None;
    for text in want {
        let text = collapse(text);
        match items.iter().skip(from).position(|i| i.contains(&text)) {
            Some(at) => from += at + 1,
            None => {
                return Some(match previous {
                    None => Fault::InNone(text),
                    Some(previous) => Fault::NotAfter { text, previous },
                });
            }
        }
        previous = Some(text);
    }
    None
}

/// What was seen: the first items, each cut, then how many more there are.
fn seen(shown: &[String], total: usize) -> String {
    let cut = |s: &str| {
        let s = collapse(s);
        if s.chars().count() > SEEN_CHARS {
            format!("{}...", s.chars().take(SEEN_CHARS).collect::<String>())
        } else {
            s
        }
    };
    let first = shown.iter().take(SEEN_ITEMS).map(|s| format!("{:?}", cut(s))).collect::<Vec<_>>().join(", ");
    if total > SEEN_ITEMS {
        format!("{first} and {} more", total - SEEN_ITEMS)
    } else {
        first
    }
}

/// The failure sentence for `fault`, showing `shown` (the first items as the
/// page names them) out of `total` items.
fn sentence(want: &[String], fault: &Fault, shown: &[String], total: usize) -> String {
    let collapsed: Vec<String> = want.iter().map(|t| collapse(t)).collect();
    let list = list_of(&collapsed);
    let saw = seen(shown, total);
    match fault {
        Fault::InNone(text) => format!("expected text order {list} but {text:?} is in none of them; saw {saw}"),
        Fault::NotAfter { text, previous } => {
            format!("expected text order {list} but {text:?} is not after {previous:?}; saw {saw}")
        }
    }
}

/// Do `items` (the texts of the list, in page order) hold `want` in that
/// order, others between allowed? The sentence for a pass, or the failure.
pub fn judge(want: &[String], items: &[String]) -> Result<String, String> {
    judge_shown(want, items, items, items.len())
}

/// [`judge`], a failure showing `shown` (the first items as the page names
/// them) out of `total` rather than the texts compared.
pub(crate) fn judge_shown(want: &[String], items: &[String], shown: &[String], total: usize) -> Result<String, String> {
    match find_fault(want, items) {
        None => {
            let collapsed: Vec<String> = want.iter().map(|t| collapse(t)).collect();
            Ok(format!("reads {} in that order", list_of(&collapsed)))
        }
        Some(fault) => Err(sentence(want, &fault, shown, total)),
    }
}

/// One look at the whole list. `Ok(Ok(detail))` holds; `Ok(Err(why))` does
/// not hold yet; an `Err` is the browser. `read` is set to the texts read,
/// which a hold on the pass is keyed on.
///
/// A call that runs out of time after an item has been read is a list too
/// long for the time given ([`TOO_SLOW`]); before any was, it is the
/// browser's silence and the timeout is passed on (see `each::look_each`).
pub(crate) async fn look_order<D: Driver>(
    d: &mut D,
    what: &str,
    items: &[Handle],
    want: &[String],
    read: &mut String,
) -> Result<Result<String, String>, CdpError> {
    let mut done = 0;
    match read_texts(d, what, items, want, read, &mut done).await {
        Err(CdpError::Timeout { .. }) if done > 0 => Ok(Err(TOO_SLOW.to_string())),
        other => other,
    }
}

/// [`look_order`]; `done` counts the items whose text has been read.
async fn read_texts<D: Driver>(
    d: &mut D,
    what: &str,
    items: &[Handle],
    want: &[String],
    read: &mut String,
    done: &mut usize,
) -> Result<Result<String, String>, CdpError> {
    let n = items.len();
    if n == 0 {
        return Ok(Err(NOT_ON_PAGE.to_string()));
    }
    if n > ORDER_MAX {
        return Ok(Err(format!("matched {n} items, more than the {ORDER_MAX} expect_order reads - narrow the list")));
    }
    let mut texts = Vec::with_capacity(n);
    for item in items {
        let got = page::call_value(d, item, READ_TEXT_JS, &[]).await?;
        *done += 1;
        // An item gone from the page was read for nothing.
        if !page::call_value(d, item, CONNECTED_JS, &[]).await?.as_bool().unwrap_or(false) {
            return Ok(Err(LIST_CHANGED.to_string()));
        }
        texts.push(got.as_str().unwrap_or("").to_string());
    }
    *read = texts.iter().map(|t| collapse(t)).collect::<Vec<_>>().join("\u{1f}");
    if let Ok(said) = judge(want, &texts) {
        return Ok(Ok(format!("{what} {said}")));
    }
    // Shown by what a person calls each item, never a typed value.
    let mut shown = Vec::with_capacity(SEEN_ITEMS);
    for item in items.iter().take(SEEN_ITEMS) {
        shown.push(page::call_value(d, item, SHOWN_JS, &[]).await?.as_str().unwrap_or("").to_string());
    }
    Ok(Err(judge_shown(want, &texts, &shown, n).err().unwrap_or_default()))
}
