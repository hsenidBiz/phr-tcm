//! The application refusing an action: a request the page made of its own
//! server that came back 400-499 while an action was carrying out, and
//! what the server said about it.
//!
//! A form that will not save usually says why in a banner the page can be
//! read for (`browser::page`), but the clearest reason is the server's own:
//! an ASP.NET problem body, a `message`, a list of field errors. This reads
//! that out of the response body, so a failed step can name what the
//! application refused rather than leave the tester to find it in a trace.
//!
//! Only the request's method, path (no query, no host), status, the
//! message and the trace id are kept. The message is redacted like any
//! other quoted body (`api_checks::shown_body`) before it is shown.

use crate::autorun::api_checks::{body_of, redact, shown_body, Body, REDACTED};
use crate::autorun::report::without_query;
use crate::browser::cdp::Driver;
use crate::browser::net_record::{NetEntry, NetState};
use serde_json::Value;

/// What the line a person reads starts with.
pub const APP_REFUSED: &str = "the app refused: ";

/// How many refusals one action reports: the first few say what is wrong,
/// and a page that fires a dozen would bury them.
const MAX_REFUSED: usize = 3;

/// The longest message kept, once collapsed.
const MAX_MESSAGE: usize = 300;

/// How much of a body that is not JSON is taken as the message.
const PLAIN_TEXT: usize = 200;

/// The longest trace id kept.
const MAX_TRACE: usize = 100;

/// A request the application refused.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RefusedRequest {
    pub method: String,
    /// Where it went, with no host and no query.
    pub path: String,
    pub status: u16,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

/// Whether `e` is the application refusing something the page asked: its
/// own XHR or fetch, to its own site, not a poll, not one of TCM's own
/// (`own`, request ids), finished, and answered 400-499 other than the
/// three that mean something else (401 signed out, 403 not allowed, 404 not
/// there - the page and its menus handle those themselves).
pub fn counts(e: &NetEntry, own: &[String]) -> bool {
    matches!(e.kind.as_deref(), Some("XHR" | "Fetch"))
        && e.same_site
        && !e.polling
        && e.state == NetState::Finished
        && !own.contains(&e.id)
        && matches!(e.status, Some(400..=499))
        && !matches!(e.status, Some(401 | 403 | 404))
}

/// The status as a person says it, for when the body says nothing.
fn status_line(status: u16) -> String {
    match status {
        400 => "400 Bad Request".to_string(),
        409 => "409 Conflict".to_string(),
        422 => "422 Unprocessable Entity".to_string(),
        other => other.to_string(),
    }
}

/// `s` as a message, or `None` when it is empty or a hidden value.
fn text_of(v: &Value) -> Option<String> {
    let s = v.as_str()?.trim();
    (!s.is_empty() && s != REDACTED).then(|| s.to_string())
}

/// The first message in a field-errors value: an object of field to a list
/// or a string, or a list of either.
fn first_error(v: &Value) -> Option<String> {
    match v {
        Value::String(_) => text_of(v),
        Value::Array(items) => items.iter().find_map(first_error),
        Value::Object(map) => map.values().find_map(first_error).or_else(|| text_of(map.get("message")?)),
        _ => None,
    }
}

/// The message of a JSON body, by the keys servers use, most specific first.
fn message_of(v: &Value) -> Option<String> {
    let Value::Object(map) = v else { return text_of(v) };
    for key in ["message", "Message", "error_description"] {
        if let Some(m) = map.get(key).and_then(text_of) {
            return Some(m);
        }
    }
    if let Some(e) = map.get("error") {
        if let Some(m) = text_of(e).or_else(|| e.get("message").and_then(text_of)) {
            return Some(m);
        }
    }
    for key in ["detail", "title"] {
        if let Some(m) = map.get(key).and_then(text_of) {
            return Some(m);
        }
    }
    map.get("errors").and_then(first_error)
}

fn trace_of(v: &Value) -> Option<String> {
    let id = ["traceId", "TraceId", "trace_id", "traceIdentifier"].iter().find_map(|k| v.get(*k).and_then(text_of))?;
    Some(id.chars().take(MAX_TRACE).collect())
}

/// `text` redacted as any quoted body is, collapsed, and cut to `cap`
/// characters; `None` when nothing is left.
fn shown(text: &str, cap: usize) -> Option<String> {
    let s: String = shown_body(text).chars().take(cap).collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// What the application said about refusing a request answered `status`
/// with `body`: its message, and the trace id when it gave one.
///
/// A JSON body is redacted first and the message picked from what is left,
/// so a value hidden for its key can never become the message. Text that is
/// not JSON gives its first characters; a page (HTML), no body, or a body
/// Chrome no longer holds gives the status.
pub fn message_and_trace(status: u16, body: Option<&str>) -> (String, Option<String>) {
    let none = || status_line(status);
    let Some(text) = body.map(str::trim).filter(|t| !t.is_empty()) else { return (none(), None) };
    match serde_json::from_str::<Value>(text) {
        Ok(v) => {
            let v = redact(&v);
            let message = message_of(&v).and_then(|m| shown(&m, MAX_MESSAGE));
            (message.unwrap_or_else(none), trace_of(&v))
        }
        Err(_) if text.starts_with('<') => (none(), None),
        Err(_) => (shown(text, PLAIN_TEXT).unwrap_or_else(none), None),
    }
}

/// The requests started since `mark` that the application refused
/// (`counts`), at most three, in the order they started, each with what
/// its body said. A body is read only for a request that counts.
pub async fn refused_since<D: Driver>(d: &mut D, mark: u64, own: &[String]) -> Vec<RefusedRequest> {
    let refused: Vec<NetEntry> = d.net_since(mark).into_iter().filter(|e| counts(e, own)).take(MAX_REFUSED).collect();
    let mut out = Vec::with_capacity(refused.len());
    for e in refused {
        let status = e.status.unwrap_or_default();
        // A browser that stops answering leaves the status to speak.
        let body = body_of(d, &e.id).await.ok();
        let text = match &body {
            Some(Body::Text(t)) => Some(t.as_str()),
            _ => None,
        };
        let (message, trace_id) = message_and_trace(status, text);
        out.push(RefusedRequest {
            method: e.method.clone(),
            path: without_query(&e.path_query).to_string(),
            status,
            message,
            trace_id,
        });
    }
    out
}

/// The line a failed step or a discovery answer carries for one refusal.
pub fn app_refused_line(r: &RefusedRequest) -> String {
    let trace = r.trace_id.as_deref().map(|t| format!(", trace id {t}")).unwrap_or_default();
    format!("{APP_REFUSED}\"{}\" ({} {}, {}{trace})", r.message, r.method, r.path, r.status)
}
