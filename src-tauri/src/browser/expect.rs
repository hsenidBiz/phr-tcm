//! One loop for every expectation: look, and if it does not hold yet, look
//! again until the timeout. Pages settle; a record's name appears a moment
//! after Save. A single look turns that moment into a false failure, and a
//! fixed sleep turns it into a slow test that still fails on a bad day.
//!
//! When time runs out, the detail says what was last seen. "Expected X" is
//! not evidence; "expected X but saw Y" is.

use super::actions::{harness, ActionOutcome};
use super::cdp::{CdpError, Driver};
use super::locator::{resolve, Target, VISIBLE_JS};
use super::page::{self, Handle};
use serde_json::json;
use std::time::{Duration, Instant};

pub enum Check<'a> {
    Visible,
    Hidden,
    Text(&'a str),
    ContainsText(&'a str),
    Count(u32),
    Attribute { name: &'a str, equals: &'a str },
}

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

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn visible<D: Driver>(d: &mut D, h: &Handle) -> Result<bool, CdpError> {
    Ok(page::call_value(d, h, VISIBLE_JS, &[]).await?.as_bool().unwrap_or(false))
}

fn only(handles: &[Handle]) -> Result<&Handle, String> {
    match handles {
        [] => Err("is not on the page".to_string()),
        [one] => Ok(one),
        many => Err(format!("matched {} elements - narrow it, or add nth", many.len())),
    }
}

/// One look. `Ok(Ok(detail))` holds; `Ok(Err(why))` does not hold yet.
async fn look<D: Driver>(
    d: &mut D,
    target: &Target,
    check: &Check<'_>,
) -> Result<Result<String, String>, CdpError> {
    let what = target.describe();
    let handles = resolve(d, target).await?;
    Ok(match check {
        Check::Visible => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                if visible(d, h).await? {
                    Ok(format!("{what} is visible"))
                } else {
                    Err("is there but cannot be seen".to_string())
                }
            }
        },
        Check::Hidden => {
            let mut seen = false;
            for h in &handles {
                if visible(d, h).await? {
                    seen = true;
                    break;
                }
            }
            if seen {
                Err("is still visible".to_string())
            } else {
                Ok(format!("{what} is hidden"))
            }
        }
        Check::Count(n) => {
            if handles.len() as u32 == *n {
                Ok(format!("counted {n}: {what}"))
            } else {
                Err(format!("expected {n}, counted {}", handles.len()))
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
                    Err(format!("expected text {want:?} but saw {got:?}"))
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
                    Err(format!("expected it to contain {want:?} but saw {got:?}"))
                }
            }
        },
        Check::Attribute { name, equals } => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, READ_ATTR_JS, &[json!(name)]).await?;
                match got.as_str() {
                    None => Err(format!("has no {name} attribute")),
                    Some(v) if v == *equals => Ok(format!("{what} has {name}={equals:?}")),
                    Some(v) => Err(format!("expected {name}={equals:?} but saw {v:?}")),
                }
            }
        },
    })
}

pub async fn expect<D: Driver>(
    d: &mut D,
    target: &Target,
    check: Check<'_>,
    timeout_ms: u64,
    poll_ms: u64,
) -> ActionOutcome {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        page::release(d).await;
        let why = match look(d, target, &check).await {
            Ok(Ok(detail)) => return ActionOutcome::passed(detail),
            Ok(Err(why)) => why,
            Err(e) if e.is_transient() => e.to_string(),
            Err(e) => return harness(e),
        };
        if Instant::now() >= deadline {
            return ActionOutcome::failed(format!(
                "waited {timeout_ms}ms: {} {why}",
                target.describe()
            ));
        }
        tokio::time::sleep(Duration::from_millis(poll_ms)).await;
    }
}
