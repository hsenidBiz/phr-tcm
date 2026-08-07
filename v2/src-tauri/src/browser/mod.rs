//! Driving a real, visible browser for the supervised test runner.
//!
//! Deliberately NOT a bundled automation framework: the app talks the
//! Chrome DevTools Protocol to the Edge already on the machine. That
//! keeps the installer lean (Velopack ships deltas; a bundled browser
//! would wreck them) and means tests run in the browser people actually
//! use, not a webview stand-in.

pub mod launch;
pub mod cdp;
pub mod actions;
