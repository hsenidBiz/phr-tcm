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
//! value under a key named like a secret hidden (`shown_body`), anti-forgery
//! tokens scrubbed, any address in it cut to its path, and bearer tokens and
//! JWTs hidden (`scrub_shown`). A field mismatch hides the same.

use crate::api_templates::exec::{encode_query, excerpt, partial_match_shown, scrub_tokens};
use crate::api_templates::runner::FETCH_GRACE;
use crate::autorun::report::without_query;
use crate::browser::actions::{harness, harness_timeout, Action, ActionOutcome, CANNOT_RUN};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::net_record::{NetEntry, NetRedirect, NetState};
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
/// `<METHOD> <path> was redirected to <path>` - a request answered by
/// sending it on to another page (an ended session's sign-in page,
/// typically).
pub const REDIRECTED: &str = " was redirected to ";
/// What a secret's value is shown as.
pub const REDACTED: &str = "[redacted]";
/// Between a failure's sentence and the excerpt of the body it read.
pub const BODY_BEGAN: &str = " - the response began: ";
/// `<METHOD> <path> started <n> ms after action <k> (<kind>), sooner than not_before_ms <n>` and its
/// other timing forms.
pub const STARTED: &str = " started ";
/// `<METHOD> <path> has no start time from the browser`.
pub const NO_START: &str = "has no start time from the browser";
/// A `same_document` check whose step loaded a new document in the top frame.
pub const RELOADED: &str = "the page reloaded during the step: its top frame loaded a new document";
/// A `same_document` check that could not tell.
pub const RELOAD_UNKNOWN: &str =
    "could not tell whether the page reloaded during the step: the browser did not name the document it showed";
/// A timed check in a step with no action before it to time from.
pub const NOTHING_TO_TIME_FROM: &str =
    "not_before_ms and started_within_ms time the request from the action before the check in its step, and this step has none";
/// What a passing `same_document` check adds to its sentence.
pub const SAME_DOCUMENT: &str = ", in the same document";

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
pub(crate) const SECRET_WORDS: [&str; 6] = ["token", "password", "secret", "authorization", "cookie", "session"];

pub(crate) fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SECRET_WORDS.iter().any(|w| key.contains(w))
}

/// `v` with the value of every key named like a secret replaced by
/// `REDACTED`, at any depth. A string is a place JSON hides too: one that
/// holds a JSON object or array is redacted the same way, and any other
/// has `hide_members` run over it.
pub(crate) fn redact(v: &Value) -> Value {
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
    hide_members_by(text, &SECRET_KEY, |_| true)
}

/// `hide_members` for another way of telling a secret's key: `member` has
/// the shape of `SECRET_KEY` (group 1 the escape, the match ending where
/// the value begins) and may match any key, with group 2 its name; only a
/// member whose name `is_secret` has its value hidden.
pub(crate) fn hide_members_by(text: &str, member: &Regex, is_secret: impl Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some(key) = member.captures_at(text, pos) {
        let (Some(whole), Some(escape)) = (key.get(0), key.get(1)) else { break };
        out.push_str(&text[pos..whole.end()]);
        if !key.get(2).map_or(true, |name| is_secret(name.as_str())) {
            pos = whole.end();
            continue;
        }
        let escaped = !escape.is_empty();
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

/// An address in text: a scheme, `://` (or `:\/\/`, as JSON may write it),
/// and everything up to a quote, a bracket or a space - JSON's `\/` and
/// `\uXXXX` escapes included, so an escaped `&` does not end it early.
static ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b[a-z][a-z0-9+.\-]*:(?:\\?/){2}(?:[^\s"'<>\\]|\\/|\\u[0-9a-f]{4})*"#).unwrap()
});

/// A query string (or a fragment carrying one) left in text once the
/// addresses are cut: `?ReturnUrl=...`, `#access_token=...`.
static QUERY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"[?#][\w.~%\[\]\-]+=(?:[^\s"'<>\\]|\\/|\\u[0-9a-fA-F]{4})*"#).unwrap()
});

/// `Bearer <token>`, whatever key or text it sits in.
pub(crate) static BEARER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bbearer\s+[\w\-.~+/]+=*").unwrap());

/// A JWT: three base64url parts, the first a JSON header (`eyJ`).
static JWT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"eyJ[\w-]+\.[\w-]+\.[\w-]*").unwrap());

/// Text a failure quotes, made safe to keep wherever it came from: every
/// address cut to its path (no host, no query), any other query string
/// dropped, and bearer tokens and JWTs hidden - under a key with an
/// ordinary name too. Anti-forgery tokens are `scrub_tokens`' part.
fn scrub_shown(text: &str) -> String {
    let text = ADDRESS.replace_all(text, |c: &regex::Captures| {
        let address = c[0].replace("\\/", "/");
        match path_only(&address) {
            "" => "/".to_string(),
            path => path.to_string(),
        }
    });
    let text = QUERY.replace_all(&text, "");
    let text = BEARER.replace_all(&text, format!("Bearer {REDACTED}").as_str());
    JWT.replace_all(&text, REDACTED).into_owned()
}

/// What a failure may quote of a body, for both kinds of check: a JSON body
/// with every value under a key named like a secret hidden (text that does
/// not parse gets the same, member by member), anti-forgery tokens
/// scrubbed, addresses and bare tokens made safe (`scrub_shown`), then the
/// 500-character excerpt.
pub fn shown_body(body: &str) -> String {
    let hidden = match serde_json::from_str::<Value>(body) {
        Ok(v) => serde_json::to_string(&redact(&v)).unwrap_or_default(),
        Err(_) => hide_members(body),
    };
    excerpt(&scrub_tokens(&scrub_shown(&hidden), None))
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
    /// The most recent matching request that finished - or that was
    /// redirected: what it answered is known as soon as it is sent on.
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

/// Has it answered? Finished, or redirected - the redirect was its answer,
/// whatever the page it was sent on to is still doing.
fn answered(e: &NetEntry) -> bool {
    e.state == NetState::Finished || e.redirect.is_some()
}

/// Still going, with nothing yet to judge.
fn going(e: &NetEntry) -> bool {
    e.state == NetState::Pending && e.redirect.is_none()
}

/// The request to judge among `entries` (the step's, oldest first): the
/// most recent match that finished or was redirected (`answered`), else
/// the most recent failed one, else a
/// match still going. `url_contains` is matched without regard to case
/// against the path and query - the record holds no host to match.
pub fn pick(entries: &[NetEntry], method: Option<&str>, url_contains: &str) -> Pick {
    let pattern = url_contains.trim().to_lowercase();
    let found: Vec<&NetEntry> = entries.iter().rev().filter(|e| matches(e, method, &pattern)).collect();
    if let Some(e) = found.iter().find(|e| answered(e)) {
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

/// What the step's `expect_response` checks share, armed when the step starts: where the step began in the
/// network record, the requests earlier checks of the step took, and how many of the step's requests had
/// been recorded as each action began.
#[derive(Debug, Clone, Default)]
pub struct StepRequests {
    /// Where the step began in the network record (`Driver::net_mark`).
    pub mark: u64,
    /// The ids of the requests earlier checks of the step took.
    pub taken: Vec<String>,
    /// How many of the step's requests had been recorded as each action began, in order.
    pub seen_at_action: Vec<usize>,
    /// The step's actions' kinds (`patterns::action_kind`), in order.
    pub kinds: Vec<String>,
    /// The step's latest action `times_from` holds for, so far.
    pub acted_at: Option<ActedAt>,
    /// The main frame's `loaderId` at the step's start, read only when a check of the step asks `same_document`.
    pub document: Option<String>,
}

/// The action a timed check measures from: which one it was, when it began and when its last input went out
/// (epoch ms, the scale of a request's `started_ms`).
#[derive(Debug, Clone, PartialEq)]
pub struct ActedAt {
    /// The action's place in the step, from 1.
    pub number: usize,
    pub kind: String,
    pub began_ms: u64,
    pub at_ms: u64,
}

impl StepRequests {
    /// `kinds`: the step's actions' kinds (`patterns::action_kind`), in order.
    pub fn new(mark: u64, kinds: Vec<String>) -> Self {
        StepRequests { mark, taken: Vec::new(), seen_at_action: Vec::new(), kinds, acted_at: None, document: None }
    }

    /// The next action begins with `seen` of the step's requests recorded.
    pub fn action_began(&mut self, seen: usize) {
        self.seen_at_action.push(seen);
    }

    /// The action just carried out (the latest written one `action_began` noted) began at `began_ms` and
    /// sent its last input at `at_ms`. A later inner action of the same component keeps the first `began_ms`.
    pub fn note_action_done(&mut self, began_ms: u64, at_ms: u64) {
        let number = self.seen_at_action.len().max(1);
        match &mut self.acted_at {
            Some(acted) if acted.number == number => acted.at_ms = at_ms,
            _ => {
                let kind = self.kinds.get(number - 1).cloned().unwrap_or_else(|| "action".to_string());
                self.acted_at = Some(ActedAt { number, kind, began_ms, at_ms });
            }
        }
    }
}

/// Whether a request started when a timed check wants, measured from `acted` (`started_ms` is the browser's
/// own start time for it). `Ok` is the measure (`started <n> ms after action <k> (<kind>)`), `Err` the
/// reason, without the request's name.
pub fn judge_start(
    started_ms: Option<u64>,
    acted: &ActedAt,
    not_before_ms: Option<u32>,
    started_within_ms: Option<u32>,
) -> Result<String, String> {
    let Some(start) = started_ms else { return Err(NO_START.to_string()) };
    let (k, kind) = (acted.number, acted.kind.as_str());
    if start < acted.began_ms {
        return Err(format!("started {} ms before action {k} ({kind}) began", acted.began_ms - start));
    }
    // A start between the action's beginning and its last input is the action's own doing: 0 ms after it.
    let after = start.saturating_sub(acted.at_ms);
    if let Some(nb) = not_before_ms.filter(|nb| after < u64::from(*nb)) {
        return Err(format!("started {after} ms after action {k} ({kind}), sooner than not_before_ms {nb}"));
    }
    if let Some(sw) = started_within_ms.filter(|sw| after > u64::from(*sw)) {
        return Err(format!("started {after} ms after action {k} ({kind}), later than started_within_ms {sw}"));
    }
    Ok(format!("started {after} ms after action {k} ({kind})"))
}

/// Is the page's top frame still the document the step began on? `at_start` and `now` are the main frame's
/// `loaderId`s; either missing is "cannot tell", never a pass.
pub fn judge_document(at_start: Option<&str>, now: Option<&str>) -> Result<(), String> {
    match (at_start, now) {
        (Some(a), Some(b)) if a == b => Ok(()),
        (Some(_), Some(_)) => Err(RELOADED.to_string()),
        _ => Err(RELOAD_UNKNOWN.to_string()),
    }
}

/// The main frame's `loaderId` (`Page.getFrameTree`): it changes with every new document the top frame loads,
/// and not with a request the page makes. None on any error or an empty answer.
pub async fn main_document<D: Driver>(d: &mut D) -> Option<String> {
    let tree = d.call("Page.getFrameTree", json!({})).await.ok()?;
    tree["frameTree"]["frame"]["loaderId"].as_str().filter(|id| !id.is_empty()).map(str::to_string)
}

/// A match the page called off itself: not the server failing, and not a request to judge while a later
/// one matches.
fn cancelled(e: &NetEntry) -> bool {
    e.redirect.is_none() && matches!(&e.state, NetState::Failed(why) if why.trim() == ABORTED)
}

/// The request the next check takes: among `entries` (the step's, oldest first) that match and are not in
/// `taken`, passing over one the page cancelled (`ABORTED`) while a later one matches:
/// - the OLDEST that has finished (answered or redirected), whether or not an older one is still going: a
///   request that never finishes (a fetch whose body the page never reads stays going in the browser's
///   record) must not hold up the check while a newer one has answered;
/// - else the oldest that failed;
/// - else the oldest, still going: the check waits only while none has finished.
///
/// None left -> None { seen }.
pub fn pick_in_order(entries: &[NetEntry], method: Option<&str>, url_contains: &str, taken: &[String]) -> Pick {
    let pattern = url_contains.trim().to_lowercase();
    let found: Vec<&NetEntry> = entries.iter().filter(|e| matches(e, method, &pattern)).collect();
    let last = found.len().saturating_sub(1);
    let left: Vec<&NetEntry> = found
        .iter()
        .enumerate()
        .filter(|(i, e)| !taken.contains(&e.id) && !(cancelled(e) && *i < last))
        .map(|(_, e)| *e)
        .collect();
    if let Some(e) = left.iter().find(|e| answered(e)) {
        return Pick::Finished((*e).clone());
    }
    if let Some(e) = left.iter().find(|e| matches!(e.state, NetState::Failed(_))) {
        return Pick::Failed((*e).clone());
    }
    match left.first() {
        Some(e) => Pick::PendingOnly((*e).clone()),
        None => Pick::None { seen: entries.len() },
    }
}

/// How long a lone cancelled request is waited on, in case the page sends it again (a debounced search
/// cancels on each keystroke and resends after its delay): the check then judges the one resent.
pub const RESEND_GRACE_MS: u64 = 1000;

/// Where a sentence about the picked request says which one it was.
pub const WHICH: &str = ": the step's matching request ";

/// ": the step's matching request 2 of 3, sent after action 3 (press_key) began" - the action is the last whose
/// `seen_at_action` is at most the entry's index among the step's requests; before the first,
/// ": the step's matching request 1 of 1, sent as the step began".
pub fn which_request(
    entries: &[NetEntry],
    picked: &NetEntry,
    method: Option<&str>,
    url_contains: &str,
    step: &StepRequests,
) -> String {
    let pattern = url_contains.trim().to_lowercase();
    let found: Vec<&NetEntry> = entries.iter().filter(|e| matches(e, method, &pattern)).collect();
    let place = found.iter().position(|e| e.id == picked.id).map_or(1, |p| p + 1);
    let n = found.len().max(place);
    let index = entries.iter().position(|e| e.id == picked.id).unwrap_or(0);
    let action = step.seen_at_action.iter().rposition(|seen| *seen <= index);
    match action {
        Some(k) => {
            let kind = step.kinds.get(k).map_or("action", String::as_str);
            format!("{WHICH}{place} of {n}, sent after action {} ({kind}) began", k + 1)
        }
        None => format!("{WHICH}{place} of {n}, sent as the step began"),
    }
}

/// `NO_REQUEST` + pattern (path only) + "\" left in <s> s: an earlier expect_response of this step took each of the <n> that matched".
pub fn all_taken(url_contains: &str, waited_ms: u64, n: usize) -> String {
    format!(
        "{NO_REQUEST}{}\" left in {} s: an earlier expect_response of this step took each of the {n} that matched",
        without_query(url_contains.trim()),
        seconds(waited_ms)
    )
}

/// `detail` without the ": the step's matching request ..." an `expect_response` ended its sentence with
/// (the excerpt of a body that may follow it stays): for readers of the sentences that come before it.
pub fn without_which(detail: &str) -> String {
    let Some(at) = detail.find(WHICH) else { return detail.to_string() };
    let after = &detail[at..];
    match after.find(BODY_BEGAN) {
        Some(b) => format!("{}{}", &detail[..at], &after[b..]),
        None => detail[..at].to_string(),
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
        "{NO_REQUEST}{}\" in {} s (this step made {seen} {})",
        without_query(pattern.trim()),
        seconds(waited_ms),
        if seen == 1 { "request" } else { "requests" }
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

/// `<METHOD> <path> was redirected to <path>`, plus ` (on another site)`
/// when it was. The path only: a sign-in page's address carries the
/// address it was sent from, query and all.
fn redirected(who: &str, r: &NetRedirect) -> String {
    let to = if r.to.is_empty() { "another page" } else { r.to.as_str() };
    let site = if r.other_site { " (on another site)" } else { "" };
    format!("{who}{REDIRECTED}{to}{site}")
}

fn judge_body(entry: &NetEntry, status: u16, json: Option<&Value>, body: &Body) -> Result<(), String> {
    let who = named(entry);
    // A redirect is the request's answer: right when that 3xx is the status
    // expected and no JSON is asked for (post-redirect-get) - the body
    // Chrome holds is the next page's, so there is none to check. Wrong
    // otherwise, whatever the page it was sent on to answered.
    if let Some(r) = &entry.redirect {
        return if r.status == status && json.is_none() { Ok(()) } else { Err(redirected(&who, r)) };
    }
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
        .map_err(|why| format!("{RESPONSE_TO}{who}: {}", scrub_tokens(&scrub_shown(&why), None)))
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

/// Whether an earlier action of a step is one a timed `expect_response` can be timed from: it may change
/// the page, and it is not an `api_request` (that sends its own request and no input).
pub fn times_from(a: &Action) -> bool {
    crate::autorun::runner::changes_the_page(a) && !matches!(a, Action::ApiRequest { .. })
}

/// Carry out an `expect_response`, looking only at requests that started in the step (`step.mark` onwards).
/// It takes the step's oldest matching request no earlier check took, records it in `step.taken`, and ends
/// its pass or failure sentence with which request that was and after which action it was sent.
pub async fn expect_response<D: Driver>(d: &mut D, a: &Action, step: &mut StepRequests, timing: &Timing) -> ActionOutcome {
    let Action::ExpectResponse {
        method, url_contains, status, json, timeout_ms, not_before_ms, started_within_ms, same_document, ..
    } = a
    else {
        return ActionOutcome::failed(format!("{CANNOT_RUN}this is not an expect_response"));
    };
    if let Err(why) = a.validate() {
        return ActionOutcome::failed(format!("{CANNOT_RUN}{why}"));
    }
    let timed = not_before_ms.is_some() || started_within_ms.is_some();
    // Timed from the action before the check in its step: with none, there is nothing to measure from.
    if timed && step.acted_at.is_none() {
        return ActionOutcome::failed(format!("{CANNOT_RUN}{NOTHING_TO_TIME_FROM}"));
    }
    let wait_ms = timeout_ms.map(u64::from).unwrap_or(timing.expect_ms);
    // The deadline is pushed down into the driver so no single call can
    // outlive the wait, and cleared on every path out - which is why the
    // loop is a function of its own.
    let deadline = Instant::now() + Duration::from_millis(wait_ms);
    d.set_deadline(Some(deadline));
    let found = watch(d, method.as_deref(), url_contains, step, wait_ms, timing.poll_ms, deadline).await;
    d.set_deadline(None);
    let (entry, finished) = match found {
        Ok(Waited::Judge(e)) => (e, true),
        Ok(Waited::NotFinished(e)) => (e, false),
        Err(out) => return out,
    };
    step.taken.push(entry.id.clone());
    let which = which_request(&d.net_since(step.mark), &entry, method.as_deref(), url_contains, step);
    // When it started is judged before anything it answered, and before it is said not to have finished: a
    // request out of its window is the failure, whatever it said and wherever it got to. A request still
    // going whose start the browser has not reported has nothing to judge yet, so it is just not finished.
    let started = match (&step.acted_at, timed) {
        (Some(acted), true) if finished || entry.started_ms.is_some() => {
            Some(judge_start(entry.started_ms, acted, *not_before_ms, *started_within_ms))
        }
        _ => None,
    };
    if let Some(Err(clause)) = &started {
        return ActionOutcome::failed(format!("{} {clause}{which}", named(&entry)));
    }
    if !finished {
        return ActionOutcome::failed(format!("{}{which}", not_finished(&entry, wait_ms)));
    }
    let measured = match started {
        Some(Ok(measure)) => format!(", {measure}"),
        _ => String::new(),
    };
    // Read only for a JSON check, or for an unexpected 400 - whether it
    // said why is what tells a refused request from a transient one
    // (`transient`) - and never for a redirected request (what Chrome
    // holds is the next page's body); otherwise never looked at.
    let unexpected_400 = entry.status == Some(400) && *status != 400;
    let body = match (json.is_some() || unexpected_400, &entry.state) {
        (true, NetState::Finished) if entry.redirect.is_none() => match body_of(d, &entry.id).await {
            Ok(b) => b,
            Err(out) => return out,
        },
        _ => Body::Gone,
    };
    if let Err(why) = judge_body(&entry, *status, json.as_ref(), &body) {
        return failed_showing(format!("{why}{which}"), body.text());
    }
    // Judged last, once everything else passed: one look first, so a document the page loaded since the
    // last one is recorded, then the document the top frame shows now.
    if *same_document {
        let _ = look(d).await;
        let now = main_document(d).await;
        if let Err(why) = judge_document(step.document.as_deref(), now.as_deref()) {
            return ActionOutcome::failed(format!("{why}{which}"));
        }
    }
    let same = if *same_document { SAME_DOCUMENT } else { "" };
    ActionOutcome::passed(format!("{}{ANSWERED}{status}{measured}{same}{which}", named(&entry)))
}

/// Carry out an `api_request`: the page sends the GET itself (`GET_FN`,
/// on a fresh handle on the document), within the run's action timing,
/// and the answer is judged as an `expect_response`'s is. An answer from
/// another page - an ended session's sign-in page answers 200 too - fails,
/// naming that page's path.
pub async fn api_request<D: Driver>(d: &mut D, a: &Action, timing: &Timing) -> ActionOutcome {
    let Action::ApiRequest { path, query, expect, .. } = a else {
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

/// How the wait for a check's request ended.
enum Waited {
    /// The request to judge.
    Judge(NetEntry),
    /// The request the check took, still going when the time ran out.
    NotFinished(NetEntry),
}

/// Look until a matching request has finished or failed. A failed match
/// ends the wait only when no other match is still going: a page that
/// calls one request off and sends it again is judged on the second. Only
/// requests no earlier check of the step took are looked at, oldest first.
async fn watch<D: Driver>(
    d: &mut D,
    method: Option<&str>,
    url_contains: &str,
    step: &StepRequests,
    wait_ms: u64,
    poll_ms: u64,
    deadline: Instant,
) -> Result<Waited, ActionOutcome> {
    // Whether any call has come back - a page that never makes the request
    // is not the same failure as a browser that has stopped answering.
    let mut answered = false;
    // Since when the only request left to judge has been one the page cancelled.
    let mut cancelled_since: Option<Instant> = None;
    loop {
        match look(d).await {
            Ok(_) => answered = true,
            Err(e) if e.is_transient() => answered = true,
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return Err(harness(e)),
        }
        let entries = d.net_since(step.mark);
        let pattern = url_contains.trim().to_lowercase();
        let still_going =
            entries.iter().any(|e| going(e) && matches(e, method, &pattern) && !step.taken.contains(&e.id));
        let picked = pick_in_order(&entries, method, url_contains, &step.taken);
        match &picked {
            Pick::Finished(e) => return Ok(Waited::Judge(e.clone())),
            Pick::Failed(e) if !still_going && !cancelled(e) => return Ok(Waited::Judge(e.clone())),
            // A page that cancels a request on the keystroke and sends it again after a delay
            // is judged on the second: a lone cancelled one is given a moment (or the time
            // the check has) before it is said to be cancelled.
            Pick::Failed(e) if !still_going => {
                let since = *cancelled_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(RESEND_GRACE_MS) {
                    return Ok(Waited::Judge(e.clone()));
                }
            }
            _ => cancelled_since = None,
        }
        if Instant::now() >= deadline {
            if !answered {
                return Err(harness_timeout(wait_ms, &format!("a request matching \"{}\"", without_query(url_contains.trim()))));
            }
            return match picked {
                // A failed match while another was still going: judged
                // as it stands.
                Pick::Finished(e) | Pick::Failed(e) => Ok(Waited::Judge(e)),
                Pick::PendingOnly(e) => Ok(Waited::NotFinished(e)),
                Pick::None { seen } => {
                    let matched = entries.iter().filter(|e| matches(e, method, &pattern)).count();
                    Err(ActionOutcome::failed(if matched > 0 {
                        all_taken(url_contains, wait_ms, matched)
                    } else {
                        no_request(url_contains, wait_ms, seen)
                    }))
                }
            };
        }
        d.idle(Duration::from_millis(poll_ms)).await;
    }
}

/// The response body, as text, `Over` past `MAX_BODY_CHARS`. `Gone` when
/// Chrome no longer holds it (it refuses the request), `Unreadable` when
/// its base64 does not decode; a browser that does not answer at all is a
/// harness failure.
pub(crate) async fn body_of<D: Driver>(d: &mut D, request_id: &str) -> Result<Body, ActionOutcome> {
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
