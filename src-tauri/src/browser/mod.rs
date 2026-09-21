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
//! what a script is written in.
//!
//! `tests/browser_live.rs` runs the whole stack against a real headless
//! browser; everything else is tested against a scripted fake.

pub mod launch;
pub mod cdp;
pub mod timing;
pub mod page;
pub mod locator;
pub mod input;
pub mod expect;
pub mod actions;
