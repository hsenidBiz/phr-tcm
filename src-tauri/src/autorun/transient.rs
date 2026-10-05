//! Which failed unattended cases are worth one more go (spec §6).
//!
//! Pure: it reads a finished case and its script, and decides. A case is
//! transient when its FIRST failure is one of:
//! - an API check (`expect_response`, `api_request`) answered 502, 503 or
//!   504, or 400 with an empty body;
//! - a request the step waited for (an API check's, or a `navigate`'s
//!   page load) failed at the network level: `net::ERR_*`, or, for an
//!   `api_request` (the page's own `fetch`, which never says `net::ERR`),
//!   "Failed to fetch";
//! - the sign-in page would not load, `net::ERR_*` (the case's own
//!   sign-in or a `sign_in` action);
//! - the browser stopped answering (a harness failure).
//!
//! Decided by where the failure sits - which action of which step wrote
//! it - and never by its words alone: a page's dialog or a `check_text`
//! value can say "answered 503" too, and that is still the page failing
//! the test. An assertion that fails is never transient.

use super::api_checks::NET_FAILED;
use super::replay::SIGN_IN_STEP;
use super::signin::PAGE_DID_NOT_OPEN;
use super::{CaseRecord, CaseScript};
use crate::browser::actions::{Action, ActionOutcome, WOULD_NOT_LOAD};

/// The reason a retried case that passed on its second go proposes.
pub const RETRY_PASSED: &str = "passed on a second try after a transient failure: ";
/// Between a retried case's second-go reason and its first go's, closed by
/// `)`: the second go's words first, so a reason still begins the way
/// every reader of it expects (`step N:`, the trip's sentence...).
pub const FIRST_TRY: &str = " (first try: ";
/// After the first go's reason when the second go's browser never opened,
/// followed by why and closed by `)`.
pub const RETRY_NOT_STARTED: &str = " (a second try could not start: the browser did not open: ";

/// Chrome's prefix for a request that failed at the network level.
const NET_ERR: &str = "net::ERR_";
/// The page calling its own request off - not the network failing.
const ABORTED: &str = "net::ERR_ABORTED";
/// What the page's `fetch` throws when the request never got an answer -
/// `api_request`'s word for a network failure (its own limit running out
/// says "timeout" instead, which is a slow server, not this).
const FETCH_FAILED: &str = "TypeError: Failed to fetch";

/// The first failure's sentence - the case's reason - when `case` failed
/// in a way worth one more go; `None` otherwise. Only a case proposed
/// Failed or Blocked qualifies: a stopped case proposes nothing. Without
/// its script only a harness failure can be told.
pub fn is_transient(case: &CaseRecord, script: Option<&CaseScript>) -> Option<String> {
    if !matches!(case.proposed.as_str(), "Failed" | "Blocked") {
        return None;
    }
    let (n, i, first) = case
        .steps
        .iter()
        .flat_map(|s| s.outcomes.iter().enumerate().map(move |(i, o)| (s.step_number, i, o)))
        .find(|(_, _, o)| !o.ok && !o.detail.starts_with("not run:"))?;
    // The case's own sign-in is the runner's, not the script's: no action
    // of the script wrote it, so it is read on its own.
    let transient = first.harness
        || (n == SIGN_IN_STEP && page_would_not_load(&first.detail))
        || {
            let action =
                script.and_then(|sc| sc.steps.iter().find(|s| s.step_number == n)).and_then(|s| s.actions.get(i));
            action.is_some_and(|a| network_glitch(a, first))
        };
    transient.then(|| case.reason.clone())
}

/// The one record a retried case keeps: the second go's steps and
/// proposal, `retried` holding the first go's sentence, and a reason that
/// names it - `RETRY_PASSED` when the second go passed, else the second
/// go's own reason followed by `FIRST_TRY`. The time is both goes'.
pub fn after_retry(first: String, first_ms: Option<i32>, mut second: CaseRecord) -> CaseRecord {
    second.reason = if second.proposed == "Passed" {
        format!("{RETRY_PASSED}{first}")
    } else {
        format!("{}{FIRST_TRY}{first})", second.reason)
    };
    second.duration_ms = match (first_ms, second.duration_ms) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (a, b) => a.or(b),
    };
    second.retried = Some(first);
    second
}

/// The record a retried case keeps when the second go's browser never
/// opened: there is no second go to keep, so the first go's steps,
/// evidence, proposal and time stay; `retried` holds its sentence, and the
/// reason adds that the second try could not start, and why.
pub fn retry_not_started(mut first: CaseRecord, why: &str) -> CaseRecord {
    first.retried = Some(first.reason.clone());
    first.reason = format!("{}{RETRY_NOT_STARTED}{why})", first.reason);
    first
}

/// A failure the network or the server's gateway made, read from the
/// sentence the action that failed writes.
fn network_glitch(action: &Action, out: &ActionOutcome) -> bool {
    match action {
        Action::ExpectResponse { .. } => api_glitch(&out.detail),
        Action::ApiRequest { .. } => api_glitch(&out.detail) || fetch_failed(&out.detail),
        Action::Navigate { .. } => {
            // `<url> would not load: <errorText>`: an address has no spaces,
            // so the first such phrase is the runner's.
            out.detail.find(WOULD_NOT_LOAD).is_some_and(|at| net_error(&out.detail[at + WOULD_NOT_LOAD.len()..]))
        }
        Action::SignIn { .. } => page_would_not_load(&out.detail),
        _ => false,
    }
}

/// `<url> would not load: net::ERR_*`, alone or after the sign-in's own
/// `PAGE_DID_NOT_OPEN` - the start page's load failing at the network
/// level. The address before it has no spaces, so a recipe step's words
/// quoting that phrase (a `check_text` value) do not read as one.
fn page_would_not_load(detail: &str) -> bool {
    let Some((before, why)) = detail.split_once(WOULD_NOT_LOAD) else {
        return false;
    };
    let url = before.strip_prefix(PAGE_DID_NOT_OPEN).unwrap_or(before);
    !url.is_empty() && !url.contains(char::is_whitespace) && net_error(why)
}

/// `GET <path> failed: TypeError: Failed to fetch` - the page's `fetch`
/// got no answer at all.
fn fetch_failed(detail: &str) -> bool {
    detail.split_once(NET_FAILED).is_some_and(|(who, why)| {
        who.starts_with("GET ") && !who[4..].contains(' ') && why.starts_with(FETCH_FAILED)
    })
}

/// An API check's sentence begins `<METHOD> <path> `, neither with a space
/// in it; what follows says how it failed (`api_checks`).
fn api_glitch(detail: &str) -> bool {
    let mut parts = detail.splitn(3, ' ');
    let (Some(_method), Some(_path), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if let Some(why) = rest.strip_prefix("failed: ") {
        return net_error(why);
    }
    let Some(answer) = rest.strip_prefix("answered ") else {
        return false;
    };
    let Some((got, after)) = answer.split_once(", expected ") else {
        return false;
    };
    match got {
        "502" | "503" | "504" => true,
        // Nothing after the expected status: no body was shown. Usually it
        // was empty - a 400 that said why is the server refusing the
        // request. But a body Chrome no longer held (`Body::Gone`) or could
        // not decode (`Body::Unreadable`) shows nothing either, and such a
        // 400 is treated as transient on purpose: telling it apart is not
        // worth it when the cost is at most one extra try.
        "400" => !after.is_empty() && after.chars().all(|c| c.is_ascii_digit()),
        _ => false,
    }
}

fn net_error(why: &str) -> bool {
    why.starts_with(NET_ERR) && !why.starts_with(ABORTED)
}
