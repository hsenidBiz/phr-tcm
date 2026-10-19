//! Driving a real, visible browser for the test runner.
//!
//! Deliberately NOT a bundled automation framework: the app talks the
//! Chrome DevTools Protocol to the Edge or Chrome already on the machine.
//! That keeps the installer lean (Velopack ships deltas; a bundled browser
//! would wreck them) and means tests run in the browser people actually
//! use, not a webview stand-in.
//!
//! Bottom to top: `cdp` is the protocol client (deadlines, kept events,
//! dialogs); `page` holds element handles and calls functions on them;
//! `locator` says which element; `input` waits until it can be used and
//! uses it for real; `expect` looks until something holds; `actions` is
//! what a script is written in. Beside them, `page_log` keeps what the page
//! was doing (failed requests, console errors) for a failure to explain,
//! and `net_record` keeps every request for a step that checks one;
//! `save_guard` says which requests a no-save script's page may not send;
//! `downloads` names the files the browser saved.
//!
//! `tests/suite/browser_live.rs` runs the whole stack against a real headless
//! browser; everything else is tested against a scripted fake.

/// One JavaScript function expression, `(e) => string`: why the element `e`
/// is disabled, in a few words, or `''` when it is not. The one rule
/// `expect::DISABLED_JS` (`expect_enabled`, `expect_disabled`) and
/// `input::PROBE_JS` (what a click or a fill waits out) both read, written
/// once here so the two can never disagree:
///
/// - its own `disabled` property is set;
/// - the nearest of it and the elements around it that carries
///   `aria-disabled` at all says `"true"` (trimmed, case ignored), as
///   Playwright's `toBeDisabled` reads it: any other value there, such as
///   `"false"`, ends the walk as enabled;
/// - it is a listed form control (button, input, select, textarea,
///   fieldset, or a form-associated custom element) and a disabled fieldset
///   around it, any of them, holds it outside that fieldset's first
///   legend. HTML disables nothing else that way: a link or a
///   `div role="button"` in a disabled fieldset stays usable.
macro_rules! disabled_why_js {
    () => {
        r#"((e) => {
  const ariaOff = (n) => (n.getAttribute('aria-disabled') || '').trim().toLowerCase() === 'true';
  if (e.disabled === true) return 'its disabled property is set';
  for (let p = e; p; p = p.parentElement) {
    if (!p.hasAttribute('aria-disabled')) continue;
    if (ariaOff(p)) return p === e ? 'it has aria-disabled="true"' : 'an element around it has aria-disabled="true"';
    break;
  }
  const listed = /^(BUTTON|INPUT|SELECT|TEXTAREA|FIELDSET)$/.test(e.tagName) ||
    !!(e.constructor && e.constructor.formAssociated);
  if (!listed) return '';
  for (let f = e.parentElement && e.parentElement.closest('fieldset'); f; f = f.parentElement && f.parentElement.closest('fieldset')) {
    if (!f.hasAttribute('disabled')) continue;
    const legend = Array.from(f.children).find((c) => c.tagName === 'LEGEND');
    if (!(legend && legend.contains(e))) return 'a disabled fieldset holds it';
  }
  return '';
})"#
    };
}

pub mod launch;
pub mod tree;
pub mod cdp;
pub mod page_log;
pub mod net_record;
pub mod net_capture;
pub mod save_guard;
pub mod downloads;
pub mod timing;
pub mod page;
pub mod session;
pub mod locator;
pub mod input;
pub mod expect;
pub mod keys;
pub mod drag;
pub mod dialogs;
pub mod table;
pub mod page_errors;
pub mod page_messages;
pub mod actions;
pub mod repeat;
pub mod snapshot;
pub mod inspect;
