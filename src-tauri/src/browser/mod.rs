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

pub mod launch;
pub mod cdp;
pub mod page_log;
pub mod net_record;
pub mod save_guard;
pub mod downloads;
pub mod timing;
pub mod page;
pub mod session;
pub mod locator;
pub mod input;
pub mod expect;
pub mod actions;
pub mod snapshot;
