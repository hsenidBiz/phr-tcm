//! Which Auto Run browser a bridge call uses.
//!
//! `slot_for` is asked once, at the top of the bridge's router
//! (`ai_bridge::route_as`), and the slot it answers is the one every handler
//! of that call locks and publishes to. Today every caller gets the main
//! browser; the rules that give an assistant a browser of its own come with
//! the extra browsers.

use crate::autorun::agents::Caller;
use crate::autorun::slots::SlotNo;

/// What a route does with a supervised browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// It may open one: a discovery's start, a replay.
    Open,
    /// It acts on the one the caller has, or ends it: a try, a discovery
    /// action, a release, a reset, a header.
    Act,
    /// It only reads the one the caller has: a page, a probe, the captured
    /// requests.
    Read,
}

/// What the route `method path` needs of a supervised browser. `None`: the
/// route uses no supervised browser (a file, Azure DevOps, the database, the
/// process-wide imported capture sets, or `/api-template-run`, which asks
/// `slot_if_any` itself).
pub fn need_of(method: &str, path: &str) -> Option<Need> {
    match (method, path) {
        ("POST", "/autorun-discover-start" | "/autorun-replay") => Some(Need::Open),
        (
            "POST",
            "/autorun-try"
            | "/autorun-discover-action"
            | "/autorun-discover-actions"
            | "/autorun-discover-area"
            | "/autorun-discover-end"
            | "/autorun-replay-stop"
            | "/autorun-release"
            | "/autorun-reset"
            | "/autorun-cookies-clear"
            | "/autorun-headers"
            | "/autorun-cookies"
            | "/autorun-cookie"
            | "/api-template-send"
            | "/api-template-explore-undo",
        ) => Some(Need::Act),
        ("GET", "/autorun-page" | "/autorun-capture") => Some(Need::Read),
        (
            "POST",
            "/autorun-probe"
            | "/autorun-inspect"
            | "/autorun-requests"
            | "/autorun-requests-search"
            | "/autorun-request"
            | "/autorun-response"
            | "/autorun-response-body"
            | "/autorun-capture"
            | "/autorun-requests-export"
            | "/autorun-component-save"
            | "/autorun-validation-rule"
            | "/autorun-explore-change",
        ) => Some(Need::Read),
        _ => None,
    }
}

/// The browser `caller` uses for a route that `need`s one. For now every
/// caller, a default one or an agent, gets the main browser, so nothing
/// changes for anyone; the refusals (no browser to spare, the account is
/// held) come with the rules that give an assistant a browser of its own.
pub fn slot_for(_caller: &Caller, _need: Need) -> Result<SlotNo, (u16, String)> {
    Ok(SlotNo::MAIN)
}

/// The caller's slot when it has one, else `None`, and never a refusal: for
/// the routes that use a browser only when the caller has one
/// (`/api-template-run`). For now every caller has the main browser.
pub fn slot_if_any(_caller: &Caller) -> Option<SlotNo> {
    Some(SlotNo::MAIN)
}
