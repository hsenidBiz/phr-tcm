//! `expect_response`: did the page, during this script step, ask its
//! server the thing the case expects - and was it answered right?
//!
//! Carried out by the runner, which takes the step's mark in the browser's
//! network record (`browser::net_record`) before the step's first action;
//! only requests that started after it are looked at. Chrome's events are
//! read during the app's own DevTools calls, so the wait makes a light
//! call each round until a matching request has finished (or failed) or
//! the time runs out.
//!
//! Every sentence names the request by its method and PATH only: the
//! record keeps the query string for matching, and nothing here copies it
//! out - an address can carry a token there. A body is read only for a
//! JSON check, and only an excerpt of it, tokens scrubbed, goes into a
//! failure.

use crate::api_templates::exec::{excerpt, partial_match, scrub_tokens};
use crate::autorun::report::without_query;
use crate::browser::actions::{harness, harness_timeout, Action, ActionOutcome, CANNOT_RUN};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::net_record::{NetEntry, NetState};
use crate::browser::timing::Timing;
use base64::Engine;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

// The words these checks fail in - read back by `autorun::patterns`.

/// `no request matching "<pattern>" in <s> s (this step made <n> requests)`.
pub const NO_REQUEST: &str = "no request matching \"";
/// `<METHOD> <path> had not finished after <s> s`.
pub const NOT_FINISHED: &str = " had not finished after ";
/// `<METHOD> <path> failed: <errorText>`.
pub const NET_FAILED: &str = " failed: ";
/// `<METHOD> <path> was cancelled by the page`.
pub const CANCELLED: &str = " was cancelled by the page";
/// `<METHOD> <path> answered <status>, expected <expected>`.
pub const ANSWERED: &str = " answered ";
/// `the response to <METHOD> <path> was not JSON`, and `the response to
/// <METHOD> <path>: <field mismatch>`.
pub const RESPONSE_TO: &str = "the response to ";
/// A body Chrome no longer holds.
pub const BODY_GONE: &str = "the response body was no longer available";
/// Between a failure's sentence and the excerpt of the body it read.
pub const BODY_BEGAN: &str = " - the response began: ";

/// Chrome's reason for a request the page itself called off (a navigation
/// leaving, an `AbortController`) - not the server failing.
const ABORTED: &str = "net::ERR_ABORTED";
/// At most this much of a body is judged - the API templates' own cap.
const MAX_BODY_CHARS: usize = 65_536;

/// Which of the step's requests an `expect_response` is about.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    /// The most recent matching request that finished.
    Finished(NetEntry),
    /// No match finished; the most recent one that failed.
    Failed(NetEntry),
    /// Matches exist, and every one is still going: the most recent.
    PendingOnly(NetEntry),
    /// Nothing matched among the `seen` requests the step made.
    None { seen: usize },
}

/// `method` as the matching and the sentences use it.
fn method_of(m: &str) -> String {
    m.trim().to_ascii_uppercase()
}

fn matches(e: &NetEntry, method: Option<&str>, pattern: &str) -> bool {
    let method_ok = method.is_none_or(|m| method_of(m) == method_of(&e.method));
    method_ok && e.path_query.to_lowercase().contains(pattern)
}

/// The request to judge among `entries` (the step's, oldest first): the
/// most recent finished match, else the most recent failed one, else a
/// match still going. `url_contains` is matched without regard to case
/// against the path and query - the record holds no host to match.
pub fn pick(entries: &[NetEntry], method: Option<&str>, url_contains: &str) -> Pick {
    let pattern = url_contains.trim().to_lowercase();
    let found: Vec<&NetEntry> = entries.iter().rev().filter(|e| matches(e, method, &pattern)).collect();
    if let Some(e) = found.iter().find(|e| e.state == NetState::Finished) {
        return Pick::Finished((*e).clone());
    }
    if let Some(e) = found.iter().find(|e| matches!(e.state, NetState::Failed(_))) {
        return Pick::Failed((*e).clone());
    }
    match found.first() {
        Some(e) => Pick::PendingOnly((*e).clone()),
        None => Pick::None { seen: entries.len() },
    }
}

/// `POST /hr/Cycle/Save` - the method and the path, never the query.
fn named(e: &NetEntry) -> String {
    format!("{} {}", method_of(&e.method), without_query(&e.path_query))
}

/// Milliseconds as the seconds a sentence shows: `10`, `0.3`.
fn seconds(ms: u64) -> String {
    if ms % 1000 == 0 {
        (ms / 1000).to_string()
    } else {
        format!("{:.1}", ms as f64 / 1000.0)
    }
}

fn no_request(pattern: &str, waited_ms: u64, seen: usize) -> String {
    format!(
        "{NO_REQUEST}{}\" in {} s (this step made {seen} requests)",
        without_query(pattern.trim()),
        seconds(waited_ms)
    )
}

fn not_finished(e: &NetEntry, waited_ms: u64) -> String {
    format!("{}{NOT_FINISHED}{} s", named(e), seconds(waited_ms))
}

/// Was this request answered as expected? `body` is the response text,
/// when one was read; it is needed only when `json` asks for fields.
/// Pure: the sentences, with no excerpt (the caller adds that).
pub fn judge(entry: &NetEntry, status: u16, json: Option<&Value>, body: Option<&str>) -> Result<(), String> {
    let who = named(entry);
    match &entry.state {
        NetState::Failed(why) if why.trim() == ABORTED => return Err(format!("{who}{CANCELLED}")),
        NetState::Failed(why) => return Err(format!("{who}{NET_FAILED}{why}")),
        NetState::Pending => return Err(format!("{who} had not finished")),
        NetState::Finished => {}
    }
    match entry.status {
        Some(got) if got == status => {}
        Some(got) => return Err(format!("{who}{ANSWERED}{got}, expected {status}")),
        None => return Err(format!("{who}{ANSWERED}without a status, expected {status}")),
    }
    let Some(expected) = json else { return Ok(()) };
    let Some(body) = body else { return Err(BODY_GONE.to_string()) };
    let Ok(actual) = serde_json::from_str::<Value>(body) else {
        return Err(format!("{RESPONSE_TO}{who} was not JSON"));
    };
    // The mismatch can quote the answer's own values.
    partial_match(expected, &actual).map_err(|why| format!("{RESPONSE_TO}{who}: {}", scrub_tokens(&why, None)))
}

/// Read what is waiting from the browser, so a request the page sent
/// before now is recorded before the step takes its mark. Best effort and
/// brief: a browser in trouble shows that in the step's own actions.
pub async fn settle<D: Driver>(d: &mut D, timing: &Timing) {
    d.set_deadline(Some(Instant::now() + Duration::from_millis(timing.poll_ms)));
    let _ = look(d).await;
    d.set_deadline(None);
}

/// The light call each round makes: it carries nothing, but reading its
/// answer reads every event that arrived before it.
async fn look<D: Driver>(d: &mut D) -> Result<Value, CdpError> {
    d.call("Runtime.evaluate", json!({ "expression": "1", "returnByValue": true })).await
}

/// Carry out an `expect_response`, looking only at requests that started at
/// or after `mark`.
pub async fn expect_response<D: Driver>(d: &mut D, a: &Action, mark: u64, timing: &Timing) -> ActionOutcome {
    let Action::ExpectResponse { method, url_contains, status, json, timeout_ms } = a else {
        return ActionOutcome::failed(format!("{CANNOT_RUN}this is not an expect_response"));
    };
    if let Err(why) = a.validate() {
        return ActionOutcome::failed(format!("{CANNOT_RUN}{why}"));
    }
    let wait_ms = timeout_ms.map(u64::from).unwrap_or(timing.expect_ms);
    // The deadline is pushed down into the driver so no single call can
    // outlive the wait, and cleared on every path out - which is why the
    // loop is a function of its own.
    let deadline = Instant::now() + Duration::from_millis(wait_ms);
    d.set_deadline(Some(deadline));
    let found = watch(d, method.as_deref(), url_contains, mark, wait_ms, timing.poll_ms, deadline).await;
    d.set_deadline(None);
    let entry = match found {
        Ok(e) => e,
        Err(out) => return out,
    };
    let body = match (json, &entry.state) {
        (Some(_), NetState::Finished) => match body_of(d, &entry.id).await {
            Ok(b) => b,
            Err(out) => return out,
        },
        _ => None,
    };
    match judge(&entry, *status, json.as_ref(), body.as_deref()) {
        Ok(()) => ActionOutcome::passed(format!("{}{ANSWERED}{status}", named(&entry))),
        Err(why) => {
            let shown = body.as_deref().map(|b| excerpt(&scrub_tokens(b, None))).unwrap_or_default();
            if shown.is_empty() {
                ActionOutcome::failed(why)
            } else {
                ActionOutcome::failed(format!("{why}{BODY_BEGAN}{shown}"))
            }
        }
    }
}

/// Look until a matching request has finished or failed. A failed match
/// ends the wait only when no other match is still going: a page that
/// calls one request off and sends it again is judged on the second.
async fn watch<D: Driver>(
    d: &mut D,
    method: Option<&str>,
    url_contains: &str,
    mark: u64,
    wait_ms: u64,
    poll_ms: u64,
    deadline: Instant,
) -> Result<NetEntry, ActionOutcome> {
    // Whether any call has come back - a page that never makes the request
    // is not the same failure as a browser that has stopped answering.
    let mut answered = false;
    loop {
        match look(d).await {
            Ok(_) => answered = true,
            Err(e) if e.is_transient() => answered = true,
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return Err(harness(e)),
        }
        let entries = d.net_since(mark);
        let pattern = url_contains.trim().to_lowercase();
        let going = entries.iter().any(|e| e.state == NetState::Pending && matches(e, method, &pattern));
        let picked = pick(&entries, method, url_contains);
        match picked {
            Pick::Finished(e) => return Ok(e),
            Pick::Failed(e) if !going => return Ok(e),
            _ => {}
        }
        if Instant::now() >= deadline {
            if !answered {
                return Err(harness_timeout(wait_ms, &format!("a request matching \"{}\"", without_query(url_contains.trim()))));
            }
            return match picked {
                // A failed match while another was still going: judged
                // as it stands.
                Pick::Finished(e) | Pick::Failed(e) => Ok(e),
                Pick::PendingOnly(e) => Err(ActionOutcome::failed(not_finished(&e, wait_ms))),
                Pick::None { seen } => Err(ActionOutcome::failed(no_request(url_contains, wait_ms, seen))),
            };
        }
        tokio::time::sleep(Duration::from_millis(poll_ms)).await;
    }
}

/// The response body, as text, cut to `MAX_BODY_CHARS`. `Ok(None)` when
/// Chrome no longer holds it (it refuses the request); a browser that does
/// not answer at all is a harness failure.
async fn body_of<D: Driver>(d: &mut D, request_id: &str) -> Result<Option<String>, ActionOutcome> {
    let got = match d.call("Network.getResponseBody", json!({ "requestId": request_id })).await {
        Ok(v) => v,
        Err(CdpError::Protocol { .. }) => return Ok(None),
        Err(e) => return Err(harness(e)),
    };
    let Some(raw) = got["body"].as_str() else { return Ok(None) };
    let text = if got["base64Encoded"].as_bool() == Some(true) {
        match base64::engine::general_purpose::STANDARD.decode(raw) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => return Ok(None),
        }
    } else {
        raw.to_string()
    };
    Ok(Some(text.chars().take(MAX_BODY_CHARS).collect()))
}
