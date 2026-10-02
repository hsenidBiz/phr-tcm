//! `expect_response`: did the page, during this script step, ask its
//! server the thing the case expects - and was it answered right? And
//! `api_request`: the page asks its own site a GET question (`GET_FN`), and
//! the answer is judged the same way.
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
//! JSON check, and only an excerpt of it goes into a failure - with every
//! value under a key named like a secret hidden (`shown_body`) and
//! anti-forgery tokens scrubbed. A field mismatch hides the same values.

use crate::api_templates::exec::{encode_query, excerpt, partial_match_shown, scrub_tokens};
use crate::api_templates::runner::FETCH_GRACE;
use crate::autorun::report::without_query;
use crate::browser::actions::{harness, harness_timeout, Action, ActionOutcome, CANNOT_RUN};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::net_record::{NetEntry, NetState};
use crate::browser::page;
use crate::browser::timing::Timing;
use base64::Engine;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::LazyLock;
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
/// A body Chrome gave back in a form that could not be decoded.
pub const BODY_UNREADABLE: &str = "the response body could not be read";
/// `the response to <METHOD> <path> was over 64 KB`: too long to judge.
pub const OVER_CAP: &str = " was over 64 KB";
/// `GET <path> was redirected to <path>` - an `api_request` answered by
/// another page (an ended session's sign-in page, typically).
pub const REDIRECTED: &str = " was redirected to ";
/// What a secret's value is shown as.
pub const REDACTED: &str = "[redacted]";
/// Between a failure's sentence and the excerpt of the body it read.
pub const BODY_BEGAN: &str = " - the response began: ";

/// Chrome's reason for a request the page itself called off (a navigation
/// leaving, an `AbortController`) - not the server failing.
const ABORTED: &str = "net::ERR_ABORTED";
/// At most this much of a body is judged - the API templates' own cap.
const MAX_BODY_CHARS: usize = 65_536;

/// The in-page GET an `api_request` sends: `fetch` by the page itself, so
/// the browser attaches the site's cookies; no anti-forgery token (a GET
/// does not need one). Aborted after `limitMs`. At most 64 KB of the body
/// comes back, with `over` saying there was more; `finalPath` is the path
/// and query the answer came from (the query is never shown), and
/// `sameOrigin` whether it came from the page's own site. A request that
/// did not complete comes back as `{ error }`, never a throw. The API
/// templates' `FETCH_FN` is the pattern.
pub const GET_FN: &str = r#"async function (url, limitMs) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), limitMs || 30000);
  try {
    const r = await fetch(url, { credentials: "same-origin", headers: { Accept: "application/json" }, signal: ctrl.signal });
    const whole = await r.text();
    const at = new URL(r.url || url, location.href);
    return { status: r.status, contentType: r.headers.get("content-type"), finalPath: at.pathname + at.search,
             sameOrigin: at.origin === location.origin, redirected: r.redirected,
             text: whole.slice(0, 65536), over: whole.length > 65536 };
  } catch (e) {
    return { error: e && e.name === "AbortError" ? "timeout" : String(e) };
  } finally {
    clearTimeout(timer);
  }
}"#;

/// A response body as a check holds it.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// The whole body, at most 64 KB.
    Text(String),
    /// Its first 64 KB: there was more, so it is not judged as JSON.
    Over(String),
    /// Chrome no longer holds it.
    Gone,
    /// Chrome gave it back, but it could not be decoded.
    Unreadable,
}

impl Body {
    /// `text` as a body, `Over` when it is longer than the cap.
    fn capped(text: String) -> Body {
        if text.chars().count() > MAX_BODY_CHARS {
            Body::Over(text.chars().take(MAX_BODY_CHARS).collect())
        } else {
            Body::Text(text)
        }
    }

    /// What there is to show of it.
    fn text(&self) -> Option<&str> {
        match self {
            Body::Text(t) | Body::Over(t) => Some(t),
            Body::Gone | Body::Unreadable => None,
        }
    }
}

/// The words a key holding a secret is named with - any part of the name,
/// in any case: `access_token`, `Password`, `sessionId`, `Set-Cookie`.
const SECRET_WORDS: [&str; 6] = ["token", "password", "secret", "authorization", "cookie", "session"];

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SECRET_WORDS.iter().any(|w| key.contains(w))
}

/// `v` with the value of every key named like a secret replaced by
/// `REDACTED`, at any depth. A string is a place JSON hides too: one that
/// holds a JSON object or array is redacted the same way, and any other
/// has `hide_members` run over it.
fn redact(v: &Value) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, x)| {
                    let x = if secret_key(k) { Value::String(REDACTED.to_string()) } else { redact(x) };
                    (k.clone(), x)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact).collect()),
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(inner @ (Value::Object(_) | Value::Array(_))) => {
                Value::String(serde_json::to_string(&redact(&inner)).unwrap_or_default())
            }
            _ => Value::String(hide_members(s)),
        },
        other => other.clone(),
    }
}

/// The key of a JSON member named like a secret, in text that does not
/// parse (a body cut at the cap, JSON inside a page or inside a string):
/// plain, `"token":`, or escaped once more, `\"token\":` - JSON written
/// into a JSON string. The first group is the escape, when there is one.
static SECRET_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(\\?)"[^"\\]*(?:token|password|secret|authorization|cookie|session)[^"\\]*\\?"\s*:\s*"#).unwrap()
});

/// `text` with the value of every member named like a secret replaced by
/// `REDACTED`, whatever the value is - a string, a scalar, or a whole
/// object or array - and up to the end of the text when it was cut off
/// inside one.
fn hide_members(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some(key) = SECRET_KEY.captures_at(text, pos) {
        let (Some(whole), Some(escape)) = (key.get(0), key.get(1)) else { break };
        let escaped = !escape.is_empty();
        out.push_str(&text[pos..whole.end()]);
        let quote = if escaped { "\\\"" } else { "\"" };
        out.push_str(&format!("{quote}{REDACTED}{quote}"));
        pos = value_end(text, whole.end(), escaped).max(whole.end());
    }
    out.push_str(&text[pos..]);
    out
}

/// The characters of JSON text from a position, with one layer of escaping
/// taken off when it is `escaped` (JSON written into a JSON string, which
/// then ends at the first quote that is not escaped). Each comes with the
/// byte just after it.
struct Layer<'a> {
    text: &'a str,
    pos: usize,
    escaped: bool,
}

impl Iterator for Layer<'_> {
    type Item = (char, usize);

    fn next(&mut self) -> Option<(char, usize)> {
        let mut rest = self.text[self.pos..].chars();
        let c = rest.next()?;
        if self.escaped {
            if c == '"' {
                return None;
            }
            if c == '\\' {
                let d = rest.next()?;
                self.pos += 1 + d.len_utf8();
                return Some((d, self.pos));
            }
        }
        self.pos += c.len_utf8();
        Some((c, self.pos))
    }
}

/// Where the JSON value starting at `at` ends (the byte after it): a
/// string to its closing quote, an object or array to its matching
/// bracket - strings and escapes inside it respected - and anything else
/// to the next separator. The end of the text, when it was cut off first.
fn value_end(text: &str, at: usize, escaped: bool) -> usize {
    let mut chars = Layer { text, pos: at, escaped };
    let Some((first, after_first)) = chars.next() else { return chars.pos };
    // A `\` arm's guard takes the character it escapes, so that character
    // is never read as a quote or a bracket.
    match first {
        '"' => {
            while let Some((c, after)) = chars.next() {
                match c {
                    '\\' if chars.next().is_none() => break,
                    '"' => return after,
                    _ => {}
                }
            }
            chars.pos
        }
        '{' | '[' => {
            let (mut depth, mut in_string) = (1usize, false);
            while let Some((c, after)) = chars.next() {
                match c {
                    '\\' if in_string && chars.next().is_none() => break,
                    '"' => in_string = !in_string,
                    '{' | '[' if !in_string => depth += 1,
                    '}' | ']' if !in_string => {
                        depth -= 1;
                        if depth == 0 {
                            return after;
                        }
                    }
                    _ => {}
                }
            }
            chars.pos
        }
        c if c == ',' || c == '}' || c == ']' || c.is_whitespace() => at,
        _ => {
            let mut end = after_first;
            for (c, after) in chars.by_ref() {
                if c == ',' || c == '}' || c == ']' || c.is_whitespace() {
                    break;
                }
                end = after;
            }
            end
        }
    }
}

/// What a failure may quote of a body, for both kinds of check: a JSON body
/// with every value under a key named like a secret hidden (text that does
/// not parse gets the same, member by member), anti-forgery tokens
/// scrubbed, then the 500-character excerpt.
pub fn shown_body(body: &str) -> String {
    let hidden = match serde_json::from_str::<Value>(body) {
        Ok(v) => serde_json::to_string(&redact(&v)).unwrap_or_default(),
        Err(_) => hide_members(body),
    };
    excerpt(&scrub_tokens(&hidden, None))
}

/// A value of the answer as a field mismatch quotes it: hidden whole when
/// any key it sits under is named like a secret, and hidden within.
fn show_answer(keys: &[&str], v: &Value) -> String {
    if keys.iter().any(|k| secret_key(k)) {
        format!("\"{REDACTED}\"")
    } else {
        serde_json::to_string(&redact(v)).unwrap_or_default()
    }
}

/// A failure's sentence, and the excerpt of the body when there is one.
fn failed_showing(why: String, body: Option<&str>) -> ActionOutcome {
    let shown = body.map(shown_body).unwrap_or_default();
    if shown.is_empty() {
        ActionOutcome::failed(why)
    } else {
        ActionOutcome::failed(format!("{why}{BODY_BEGAN}{shown}"))
    }
}

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
    let body = match body {
        Some(text) => Body::Text(text.to_string()),
        None => Body::Gone,
    };
    judge_body(entry, status, json, &body)
}

fn judge_body(entry: &NetEntry, status: u16, json: Option<&Value>, body: &Body) -> Result<(), String> {
    let who = named(entry);
    match &entry.state {
        NetState::Failed(why) if why.trim() == ABORTED => return Err(format!("{who}{CANCELLED}")),
        NetState::Failed(why) => return Err(format!("{who}{NET_FAILED}{why}")),
        NetState::Pending => return Err(format!("{who} had not finished")),
        NetState::Finished => {}
    }
    judge_answer(&who, entry.status, status, json, body)
}

/// The answer's status, then (with `json`) its body as a partial match -
/// shared by both kinds of check. `who` is `<METHOD> <path>`.
fn judge_answer(who: &str, got: Option<u16>, status: u16, json: Option<&Value>, body: &Body) -> Result<(), String> {
    match got {
        Some(got) if got == status => {}
        Some(got) => return Err(format!("{who}{ANSWERED}{got}, expected {status}")),
        None => return Err(format!("{who}{ANSWERED}without a status, expected {status}")),
    }
    let Some(expected) = json else { return Ok(()) };
    let text = match body {
        Body::Text(text) => text,
        Body::Over(_) => return Err(format!("{RESPONSE_TO}{who}{OVER_CAP}")),
        Body::Gone => return Err(BODY_GONE.to_string()),
        Body::Unreadable => return Err(BODY_UNREADABLE.to_string()),
    };
    let Ok(actual) = serde_json::from_str::<Value>(text) else {
        return Err(format!("{RESPONSE_TO}{who} was not JSON"));
    };
    // The mismatch quotes the answer's own values: secrets hidden first.
    partial_match_shown(expected, &actual, &show_answer)
        .map_err(|why| format!("{RESPONSE_TO}{who}: {}", scrub_tokens(&why, None)))
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
    // Read only for a JSON check; otherwise never looked at.
    let body = match (json, &entry.state) {
        (Some(_), NetState::Finished) => match body_of(d, &entry.id).await {
            Ok(b) => b,
            Err(out) => return out,
        },
        _ => Body::Gone,
    };
    match judge_body(&entry, *status, json.as_ref(), &body) {
        Ok(()) => ActionOutcome::passed(format!("{}{ANSWERED}{status}", named(&entry))),
        Err(why) => failed_showing(why, body.text()),
    }
}

/// Carry out an `api_request`: the page sends the GET itself (`GET_FN`,
/// on a fresh handle on the document), within the run's action timing,
/// and the answer is judged as an `expect_response`'s is. An answer from
/// another page - an ended session's sign-in page answers 200 too - fails,
/// naming that page's path.
pub async fn api_request<D: Driver>(d: &mut D, a: &Action, timing: &Timing) -> ActionOutcome {
    let Action::ApiRequest { path, query, expect } = a else {
        return ActionOutcome::failed(format!("{CANNOT_RUN}this is not an api_request"));
    };
    // The path again (safe, on this site, no query of its own), before
    // anything is sent.
    if let Err(why) = a.validate() {
        return ActionOutcome::failed(format!("{CANNOT_RUN}{why}"));
    }
    let who = format!("GET {path}");
    let query = encode_query(query);
    let url = if query.is_empty() { path.clone() } else { format!("{path}?{query}") };
    let doc = match page::document(d).await {
        Ok(h) => h,
        Err(CdpError::Protocol { .. }) => return ActionOutcome::failed(format!("{who}{NET_FAILED}the page could not send it")),
        Err(e) => return harness(e),
    };
    // The page aborts its own request at the limit; the DevTools call
    // waits a little longer, so it is the page that says so.
    let args = [Value::String(url.clone()), Value::from(timing.action_ms)];
    let limit = Duration::from_millis(timing.action_ms) + FETCH_GRACE;
    let got = match page::call_value_within(d, &doc, GET_FN, &args, limit).await {
        Ok(v) => v,
        // Whatever the page threw may quote the address: not repeated.
        Err(CdpError::Protocol { .. }) => return ActionOutcome::failed(format!("{who}{NET_FAILED}the page could not send it")),
        Err(e) => return harness(e),
    };
    if let Some(err) = got.get("error") {
        let err = match err {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        return ActionOutcome::failed(format!("{who}{NET_FAILED}{}", fetch_error(&err, &url, path)));
    }
    let Some(status) = got["status"].as_u64().and_then(|s| u16::try_from(s).ok()) else {
        return ActionOutcome::failed(format!("{who}{NET_FAILED}the page gave no answer"));
    };
    let text = got["text"].as_str().unwrap_or("").to_string();
    // Answered by another page, or by another site even on the same path.
    // No excerpt: a sign-in page writes the address it was sent from -
    // query and all - into its own text, and the path says enough.
    let landed = path_only(got["finalPath"].as_str().unwrap_or(""));
    let to = if landed.is_empty() { "another page" } else { landed };
    if got["sameOrigin"].as_bool() == Some(false) {
        return ActionOutcome::failed(format!("{who}{REDIRECTED}{to} on another site"));
    }
    if got["redirected"].as_bool() == Some(true) && landed != path.as_str() {
        return ActionOutcome::failed(format!("{who}{REDIRECTED}{to}"));
    }
    let body = if got["over"].as_bool() == Some(true) { Body::Over(text) } else { Body::Text(text) };
    match judge_answer(&who, Some(status), expect.status, expect.json.as_ref(), &body) {
        Ok(()) => ActionOutcome::passed(format!("{who}{ANSWERED}{status}")),
        Err(why) => failed_showing(why, body.text()),
    }
}

/// An address's path: no scheme or host, no query or fragment.
fn path_only(address: &str) -> &str {
    let rest = match address.find("://") {
        Some(i) => {
            let after = &address[i + 3..];
            after.find('/').map_or("", |j| &after[j..])
        }
        None => address,
    };
    without_query(rest)
}

/// The page's reason a GET did not complete, safe to keep: the address it
/// sent named by its path, and any other address in it cut to its path -
/// the browser's words can quote the whole address, query and all.
fn fetch_error(err: &str, url: &str, path: &str) -> String {
    let named = err.replace(url, path);
    let words: Vec<&str> = named.split_whitespace().map(path_only).filter(|w| !w.is_empty()).collect();
    excerpt(&scrub_tokens(&words.join(" "), None))
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

/// The response body, as text, `Over` past `MAX_BODY_CHARS`. `Gone` when
/// Chrome no longer holds it (it refuses the request), `Unreadable` when
/// its base64 does not decode; a browser that does not answer at all is a
/// harness failure.
async fn body_of<D: Driver>(d: &mut D, request_id: &str) -> Result<Body, ActionOutcome> {
    let got = match d.call("Network.getResponseBody", json!({ "requestId": request_id })).await {
        Ok(v) => v,
        Err(CdpError::Protocol { .. }) => return Ok(Body::Gone),
        Err(e) => return Err(harness(e)),
    };
    let Some(raw) = got["body"].as_str() else { return Ok(Body::Gone) };
    let text = if got["base64Encoded"].as_bool() == Some(true) {
        match base64::engine::general_purpose::STANDARD.decode(raw) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => return Ok(Body::Unreadable),
        }
    } else {
        raw.to_string()
    };
    Ok(Body::capped(text))
}
