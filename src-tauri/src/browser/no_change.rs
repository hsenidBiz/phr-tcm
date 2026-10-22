//! `expect_no_change`: click a control anyway, and see nothing happen.
//!
//! A case that says "clicking Save while it is disabled does nothing" cannot
//! be written with `click` - a click on a disabled control is refused, as a
//! person's would not be: a person CAN click it, and the page does nothing.
//! This clicks it where a person's click lands (`input::wait_ready_forced`,
//! `input::click_forced`: a disabled or `aria-disabled` control goes ahead,
//! one under an overlay never does), waits, and compares what the page was
//! before the click with what it is after:
//!
//! - the address;
//! - the page's open dialogs (`DIALOGS_JS`), and any browser dialog;
//! - the text and the states of a region (`STATE_JS`): the clicked element
//!   unless the script names another. Never a class or a style, so a card
//!   that only looks different under the pointer is not a change.
//!
//! The export to Playwright compares the same three things, after the same
//! wait, so it passes and fails where this does.

use super::actions::{blocked, failed_by, path_only, point_and_pause, settle, ActionOutcome};
use super::cdp::{CdpError, Driver};
use super::dialogs::seconds;
use super::input;
use super::locator::{resolve_explained, Target};
use super::page;
use super::timing::Timing;
use serde_json::Value;
use std::time::Duration;

/// How long nothing may happen after the click, when the script does not say.
pub const WAIT_MS: u32 = 1000;
/// The shortest `within_ms`.
pub const MIN_MS: u32 = 200;
/// The longest `within_ms`.
pub const MAX_MS: u32 = 10_000;

/// `within_ms` outside `MIN_MS` to `MAX_MS`.
pub const TIME: &str = "expect_no_change: \"within_ms\" is 200 to 10000, how long nothing may happen after the click";
/// What a failure begins with; `autorun::patterns` reads its class from it.
pub const NOTHING_EXPECTED: &str = "expected nothing to happen after the click, but ";

/// `this` is the region: `{ text, states }`, text being its innerText
/// whitespace-collapsed, states every `aria-pressed`, `aria-checked`,
/// `aria-selected`, `aria-expanded`, `aria-current` and `aria-disabled` of
/// it and the elements in it, and the `checked`, `selected`, `disabled` and
/// `value` of its form controls, in document order (each as `[n, name,
/// value]`, `n` the element's place among the region and what it holds, so
/// an element added or taken away is a change too). Never a class or a style.
pub const STATE_JS: &str = r#"function() {
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  const aria = ['aria-pressed', 'aria-checked', 'aria-selected', 'aria-expanded', 'aria-current', 'aria-disabled'];
  const states = [];
  const all = [this, ...Array.from(this.querySelectorAll('*'))];
  for (let i = 0; i < all.length; i++) {
    const e = all[i];
    for (const a of aria) {
      if (e.hasAttribute(a)) states.push([i, a, e.getAttribute(a)]);
    }
    if (e instanceof HTMLInputElement) {
      if (/^(checkbox|radio)$/.test(e.type)) states.push([i, 'checked', e.checked]);
      states.push([i, 'disabled', e.disabled]);
      states.push([i, 'value', e.value]);
    } else if (e instanceof HTMLTextAreaElement || e instanceof HTMLSelectElement) {
      states.push([i, 'disabled', e.disabled]);
      states.push([i, 'value', e.value]);
    } else if (e instanceof HTMLOptionElement) {
      states.push([i, 'selected', e.selected]);
    } else if (e instanceof HTMLButtonElement || e instanceof HTMLFieldSetElement) {
      states.push([i, 'disabled', e.disabled]);
    }
  }
  return { text: norm(this.innerText), states: states };
}"#;

/// The number of open dialogs on the page: visible `[role=dialog]`,
/// `[role=alertdialog]`, `dialog[open]` and `[aria-modal=true]`, each
/// element once.
pub const DIALOGS_JS: &str = r#"function() {
  const shown = (e) => e.checkVisibility({ visibilityProperty: true });
  return Array.from(document.querySelectorAll('[role=dialog],[role=alertdialog],dialog[open],[aria-modal=true]')).filter(shown).length;
}"#;

/// What the page was, as far as this check compares it.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    /// `location.href` whole (compared whole, as the export compares
    /// `cur.url()`); only its path is ever said.
    pub address: String,
    pub dialogs: u32,
    pub browser_dialogs: usize,
    pub text: String,
    /// `Value::Null` when the region was not on the page.
    pub states: Value,
}

fn dialogs_phrase(n: u32, did: &str) -> String {
    if n == 1 {
        format!("a dialog {did}")
    } else {
        format!("{n} dialogs {did}")
    }
}

/// What changed, in words, in this order; empty when nothing did. `region`
/// is the region's `describe()`. Pure.
pub fn changes(before: &Seen, after: &Seen, region: &str) -> Vec<String> {
    let mut out = Vec::new();
    if before.address != after.address {
        out.push(format!(
            "the address went from {} to {}",
            path_only(&before.address),
            path_only(&after.address)
        ));
    }
    if after.dialogs > before.dialogs {
        out.push(dialogs_phrase(after.dialogs - before.dialogs, "opened"));
    } else if after.dialogs < before.dialogs {
        out.push(dialogs_phrase(before.dialogs - after.dialogs, "closed"));
    }
    if after.browser_dialogs > before.browser_dialogs {
        out.push("a browser dialog opened".to_string());
    }
    if after.states.is_null() && !before.states.is_null() {
        out.push(format!("{region} is no longer on the page"));
    } else {
        if before.text != after.text {
            out.push(format!("the text of {region} changed"));
        }
        if before.states != after.states {
            out.push(format!("a state in {region} changed"));
        }
    }
    out
}

/// The pass sentence.
fn nothing_happened(selector: &Target, within_ms: u32) -> String {
    format!("clicked {} anyway and nothing happened in {} s", selector.describe(), seconds(within_ms))
}

/// The region's text and states, or `None` when it is not on the page, or
/// `Err(sentence)` when it names several elements.
async fn read_region<D: Driver>(d: &mut D, region: &Target) -> Result<Result<Option<(String, Value)>, String>, CdpError> {
    let found = resolve_explained(d, region).await?;
    match found.handles.len() {
        0 => Ok(match found.unreachable_frame {
            Some(why) => Err(why),
            None => Ok(None),
        }),
        1 => {
            let v = page::call_value(d, &found.handles[0], STATE_JS, &[]).await?;
            let text = v["text"].as_str().unwrap_or("").to_string();
            Ok(Ok(Some((text, v["states"].clone()))))
        }
        n => Ok(Err(format!("{} {}", region.describe(), input::matched_many(n)))),
    }
}

/// Everything compared, read now. A page that refuses a read mid-navigation
/// is asked again a few times: a click that navigates is a change to say,
/// not a failure to read.
async fn read_seen<D: Driver>(
    d: &mut D,
    region: &Target,
    timing: &Timing,
    release: bool,
) -> Result<Result<Seen, String>, CdpError> {
    let mut tries = 0;
    loop {
        tries += 1;
        match read_once(d, region, release).await {
            Err(e) if e.is_transient() && tries < 5 => d.idle(Duration::from_millis(timing.poll_ms.max(100))).await,
            other => return other,
        }
    }
}

async fn read_once<D: Driver>(d: &mut D, region: &Target, release: bool) -> Result<Result<Seen, String>, CdpError> {
    if release {
        page::release(d).await;
    }
    let address = page::eval_value(d, "location.href").await?.as_str().unwrap_or("").to_string();
    let dialogs = page::eval_value(d, &format!("({DIALOGS_JS})()")).await?.as_u64().unwrap_or(0) as u32;
    let (text, states) = match read_region(d, region).await? {
        Ok(Some(read)) => read,
        Ok(None) => (String::new(), Value::Null),
        Err(sentence) => return Ok(Err(sentence)),
    };
    // Last: the calls above read whatever the browser sent, a dialog among it.
    let browser_dialogs = d.dialog_book().map_or(0, |b| b.seen().len());
    Ok(Ok(Seen { address, dialogs, browser_dialogs, text, states }))
}

/// Click `selector` anyway, wait `within_ms`, and say what changed.
pub async fn expect_no_change<D: Driver>(
    d: &mut D,
    selector: &Target,
    region: Option<&Target>,
    within_ms: Option<u32>,
    timing: &Timing,
) -> ActionOutcome {
    let within = within_ms.unwrap_or(WAIT_MS);
    let watched = region.unwrap_or(selector);
    settle(d, timing).await;
    let ready = match input::wait_ready_forced(d, selector, timing).await {
        Ok(r) => r,
        Err(b) => return blocked(b),
    };
    if let Err(e) = point_and_pause(d, &ready, timing).await {
        return failed_by(e);
    }
    let before = match read_seen(d, watched, timing, false).await {
        Ok(Ok(seen)) => seen,
        Ok(Err(sentence)) => return ActionOutcome::failed(sentence),
        Err(e) => return failed_by(e),
    };
    if before.states.is_null() {
        let why = format!("{} {}", watched.describe(), input::NOT_FOUND);
        return ActionOutcome::failed(why);
    }
    // The readings above left the clicked element's handle alone: the very
    // handle that was made ready is the one that is clicked.
    if let Err(b) = input::click_forced(d, &ready).await {
        return blocked(b);
    }
    d.idle(Duration::from_millis(u64::from(within))).await;
    let after = match read_seen(d, watched, timing, true).await {
        Ok(Ok(seen)) => seen,
        Ok(Err(sentence)) => return ActionOutcome::failed(sentence),
        Err(e) => return failed_by(e),
    };
    let said = changes(&before, &after, &watched.describe());
    if said.is_empty() {
        ActionOutcome::passed(nothing_happened(selector, within))
    } else {
        ActionOutcome::failed(format!("{NOTHING_EXPECTED}{}", said.join(", ")))
    }
}
