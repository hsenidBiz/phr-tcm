//! `expect_each`: one check on every item of a list.
//!
//! "Every card's title starts with AUTOTEST" is not "the first card's title
//! does". The list is the selector's matches; each item is read (its own
//! text, an attribute, or the one element a `child` locator finds inside
//! it) and judged by the same rule. The first item that fails is named,
//! with its position and its text, so the failure points at a record.
//!
//! A list can change while it is read (a row re-rendered by a refresh), and
//! then an item read earlier says nothing about the page now: every item
//! read is followed by a probe that it is still in the page, and a detached
//! one makes the look fail with [`LIST_CHANGED`], which the loop looks
//! again on. A list longer than [`EACH_MAX`] is refused rather than read
//! for ever.

use super::cdp::{CdpError, Driver};
use super::expect::{COUNTED, HIDDEN, NOT_ON_PAGE, READ_ATTR_JS, READ_TEXT_JS};
use super::input::matched_many;
use super::locator::{resolve_within, Target, VISIBLE_JS};
use super::page::{self, Handle};
use serde_json::json;

/// The most items `expect_each` reads.
pub const EACH_MAX: usize = 200;
/// An item was found detached after it was read: the look is thrown away.
pub const LIST_CHANGED: &str = "the list changed while it was read";
/// `this` is an item. Is it still in the page?
pub const CONNECTED_JS: &str = "function() { return this.isConnected; }";

/// What every item must be.
pub enum EachWant<'a> {
    /// The child is visible (needs a child).
    Visible,
    Equals(&'a str),
    Contains(&'a str),
    /// The text (or attribute) matches `re`; `pattern` is how it is written.
    Matches { pattern: &'a str, re: &'a regex::Regex },
    NotEmpty,
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One value against the wish. `attribute` is the attribute read (`got` is
/// then its raw value, `None` when the element has none); without it `got`
/// is the collapsed text. `Visible` is judged elsewhere and passes here.
pub fn judge_value(want: &EachWant, attribute: Option<&str>, got: Option<&str>) -> Result<(), String> {
    match attribute {
        None => {
            let got = collapse(got.unwrap_or(""));
            match want {
                EachWant::Visible => Ok(()),
                EachWant::Equals(w) => {
                    let w = collapse(w);
                    if got == w {
                        Ok(())
                    } else {
                        Err(format!("expected text {w:?} but saw {got:?}"))
                    }
                }
                EachWant::Contains(w) => {
                    let w = collapse(w);
                    if got.contains(&w) {
                        Ok(())
                    } else {
                        Err(format!("expected it to contain {w:?} but saw {got:?}"))
                    }
                }
                EachWant::Matches { pattern, re } => {
                    if re.is_match(&got) {
                        Ok(())
                    } else {
                        Err(format!("expected text to match {} but saw {got:?}", super::pattern::shown(pattern)))
                    }
                }
                EachWant::NotEmpty => {
                    if got.is_empty() {
                        Err("expected text but saw \"\"".to_string())
                    } else {
                        Ok(())
                    }
                }
            }
        }
        Some(name) => {
            let Some(v) = got else {
                return Err(format!("{}{name}{}", super::expect::HAS_NO, super::expect::ATTRIBUTE_TAIL));
            };
            match want {
                EachWant::Visible => Ok(()),
                EachWant::Equals(w) => {
                    if v == *w {
                        Ok(())
                    } else {
                        Err(format!("expected {name}={w:?} but saw {v:?}"))
                    }
                }
                EachWant::Contains(w) => {
                    if v.contains(w) {
                        Ok(())
                    } else {
                        Err(format!("expected {name}={w:?} as part of it but saw {v:?}"))
                    }
                }
                EachWant::Matches { pattern, re } => {
                    if re.is_match(v) {
                        Ok(())
                    } else {
                        Err(format!("expected {name}={} but saw {v:?}", super::pattern::shown(pattern)))
                    }
                }
                EachWant::NotEmpty => {
                    if v.trim().is_empty() {
                        Err(format!("expected {name}=(not empty) but saw {v:?}"))
                    } else {
                        Ok(())
                    }
                }
            }
        }
    }
}

/// An item's text cut to 60 characters, `...` when cut.
fn label_of(text: &str) -> String {
    let text = collapse(text);
    if text.chars().count() > 60 {
        format!("{}...", text.chars().take(60).collect::<String>())
    } else {
        text
    }
}

/// How the pass says what every item did: the verb after "each".
fn verb(want: &EachWant, attribute: Option<&str>, child: bool) -> String {
    match (want, attribute) {
        (EachWant::Visible, _) => "is visible".to_string(),
        (EachWant::Equals(w), None) if child => format!("reads {w:?}"),
        (EachWant::Equals(w), None) => format!("read {w:?}"),
        (EachWant::Contains(w), None) if child => format!("contains {w:?}"),
        (EachWant::Contains(w), None) => format!("contain {w:?}"),
        (EachWant::Matches { pattern, .. }, None) if child => format!("matches {}", super::pattern::shown(pattern)),
        (EachWant::Matches { pattern, .. }, None) => format!("match {}", super::pattern::shown(pattern)),
        (EachWant::NotEmpty, None) if child => "has text".to_string(),
        (EachWant::NotEmpty, None) => "have text".to_string(),
        (EachWant::Equals(w), Some(a)) if child => format!("has {a}={w:?}"),
        (EachWant::Equals(w), Some(a)) => format!("have {a}={w:?}"),
        (EachWant::Contains(w), Some(a)) if child => format!("has {a} containing {w:?}"),
        (EachWant::Contains(w), Some(a)) => format!("have {a} containing {w:?}"),
        (EachWant::Matches { pattern, .. }, Some(a)) if child => {
            format!("has {a} matching {}", super::pattern::shown(pattern))
        }
        (EachWant::Matches { pattern, .. }, Some(a)) => format!("have {a} matching {}", super::pattern::shown(pattern)),
        (EachWant::NotEmpty, Some(a)) if child => format!("has a {a}"),
        (EachWant::NotEmpty, Some(a)) => format!("have a {a}"),
    }
}

/// What a passing look says.
fn passed(n: usize, what: &str, child: Option<&Target>, attribute: Option<&str>, want: &EachWant) -> String {
    let items = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
    match child {
        None if matches!(want, EachWant::Visible) => format!("{items} of {what} each be visible"),
        None => format!("{items} of {what} each {}", verb(want, attribute, false)),
        Some(c) => format!("{items} of {what}, each with {} that {}", c.describe(), verb(want, attribute, true)),
    }
}

/// One look at the whole list. `Ok(Ok(detail))` holds; `Ok(Err(why))` does
/// not hold yet (the first item that fails, named); an `Err` is the browser.
pub(crate) async fn look_each<D: Driver>(
    d: &mut D,
    what: &str,
    items: &[Handle],
    min: u32,
    child: Option<&Target>,
    attribute: Option<&str>,
    want: &EachWant<'_>,
) -> Result<Result<String, String>, CdpError> {
    let n = items.len();
    if n > EACH_MAX {
        return Ok(Err(format!("matched {n} items, more than the {EACH_MAX} expect_each reads - narrow the list")));
    }
    if (n as u64) < u64::from(min) {
        return Ok(Err(format!("expected at least {min}{COUNTED}{n}")));
    }
    for (i, item) in items.iter().enumerate() {
        // What was read of this item, when that is also its label.
        let mut read: Option<String> = None;
        let verdict: Result<(), String> = 'item: {
            let mut subject = item.clone();
            if let Some(child) = child {
                let found = resolve_within(d, item, child).await?;
                match found.handles.as_slice() {
                    [one] => subject = one.clone(),
                    [] => {
                        let why = if matches!(want, EachWant::Visible)
                            && !resolve_within(d, item, &child.including_hidden()).await?.handles.is_empty()
                        {
                            HIDDEN
                        } else {
                            NOT_ON_PAGE
                        };
                        break 'item Err(format!("{} {why}", child.describe()));
                    }
                    many => break 'item Err(format!("{} {}", child.describe(), matched_many(many.len()))),
                }
            }
            if matches!(want, EachWant::Visible) {
                // A locator may be written to match what cannot be seen.
                if !page::call_value(d, &subject, VISIBLE_JS, &[]).await?.as_bool().unwrap_or(false) {
                    break 'item Err(format!("{} {HIDDEN}", child.map(Target::describe).unwrap_or_default()));
                }
                break 'item Ok(());
            }
            match attribute {
                Some(name) => {
                    let got = page::call_value(d, &subject, READ_ATTR_JS, &[json!(name)]).await?;
                    judge_value(want, Some(name), got.as_str())
                }
                None => {
                    let got = page::call_value(d, &subject, READ_TEXT_JS, &[]).await?;
                    let got = collapse(got.as_str().unwrap_or(""));
                    let verdict = judge_value(want, None, Some(&got));
                    read = Some(got);
                    verdict
                }
            }
        };
        // An item gone from the page was read for nothing: whatever it
        // said (or failed to have) is not the page's answer.
        if !page::call_value(d, item, CONNECTED_JS, &[]).await?.as_bool().unwrap_or(false) {
            return Ok(Err(LIST_CHANGED.to_string()));
        }
        if let Err(why) = verdict {
            let label = match (&read, child, attribute) {
                (Some(text), None, None) => label_of(text),
                _ => label_of(page::call_value(d, item, READ_TEXT_JS, &[]).await?.as_str().unwrap_or("")),
            };
            return Ok(Err(format!("item {} of {n} ({label:?}): {why}", i + 1)));
        }
    }
    Ok(Ok(passed(n, what, child, attribute, want)))
}
