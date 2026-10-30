//! What a no-save script's browser may not send.
//!
//! A script marked `no_save` works on a shared draft, so the page it drives
//! must never be allowed to change it. Its browser intercepts every request
//! that can save (`Fetch.enable`, answered in `cdp`; `PAUSED_TYPES`), and
//! this module decides, purely, which requests are saves: a POST, PUT,
//! PATCH or DELETE whose path, lowercased and with the query left out,
//! contains a built-in save word or one of the project's own words, or has
//! a word of its own that is "add", "create" or "insert" (`adds`). A save
//! is failed inside the browser, so it never reaches the server, and the
//! case fails with `blocked`'s sentence.
//!
//! Nothing here keeps a host, a query string or a body: the sentence names
//! the method and the path only.

use serde_json::{json, Value};

/// The words that make a request with a writing method a save. Fixed: a
/// project can add its own (`check_words`), never remove one of these.
pub const SAVE_WORDS: [&str; 7] = ["save", "update", "delete", "submit", "approve", "publish", "assign"];

/// Save words matched only as a word of their own (`adds`): as substrings
/// they would stop a POST to an address book, a padding setting, a
/// `GetCreatedLeaves` read or a `/recreate` preview.
pub const WORD_SAVES: [&str; 3] = ["add", "create", "insert"];

/// A control named with one of these WORDS is a save-click as well (besides
/// `SAVE_WORDS` as substrings, which covers "Save & Continue"): they name
/// the button that commits a wizard step, a row or a confirm dialog. Matched
/// as whole words (a trailing "s" allowed), so "Facebook" is not "ok".
/// "Next" is here on the owner's ruling: a wizard whose Next posts its step
/// saves too. "Search" is not.
pub const CLICK_WORDS: [&str; 17] = [
    "continue", "finish", "done", "complete", "apply", "confirm", "proceed", "post", "send", "generate", "process",
    "reject", "remove", "next", "ok", "yes", "okay",
];

/// Words that make a writing request a read in disguise: many sites POST
/// their lists, lookups and telemetry. Matched as whole words of the path
/// (a trailing "s" allowed), never as substrings: "login" and "catalog" hold
/// "log" and are not reads. A path that also has a save word of its own
/// (`SAVE_WORDS`, `WORD_SAVES`) as a word is a save all the same.
pub const READ_WORDS: [&str; 18] = [
    "search", "list", "lookup", "find", "get", "load", "query", "autocomplete", "suggest", "filter", "log", "track",
    "telemetry", "ping", "heartbeat", "autosave", "fetch", "read",
];

/// The window is read late by up to this much: a request is judged when its
/// pause is read, which can be a moment after the browser sent it.
pub const SAVE_CLICK_SLACK_MS: u64 = 750;

/// How long after a click on a save-named control any writing request is
/// stopped, whatever its path (a wizard's `POST /hr/PMSV10/wizard` has no
/// save word in it).
pub const SAVE_CLICK_WINDOW_MS: u64 = 5_000;

/// The methods that can write. Every other method (GET, HEAD, OPTIONS)
/// always goes through.
pub const SAVE_METHODS: [&str; 4] = ["POST", "PUT", "PATCH", "DELETE"];

/// How every sentence the no-save guard fails a step with begins: a save it
/// stopped (`BLOCKED_START`), and a tab it could not guard
/// (`cdp::TAB_HELD_UNGUARDED`, `cdp::TAB_OPEN_UNGUARDED`).
pub const GUARD_START: &str = "this script must not save, but ";

/// How the sentence of a blocked save begins (`blocked`).
pub const BLOCKED_START: &str = "this script must not save, but the page tried to send ";
/// How the sentence of a blocked save ends.
pub const BLOCKED_END: &str = " - it was stopped before it reached the server";
/// How a no-save case's refusal begins when interception could not start.
pub const SETUP_FAILED: &str = "the no-save guard could not be set up: ";

/// The most project words kept, and the longest one.
pub const MAX_WORDS: usize = 50;
pub const MAX_WORD_LEN: usize = 60;

/// The path of an address as the matching sees it: no scheme, host, query or
/// fragment. `/` when nothing follows the host. Words in it are matched as
/// written (`is_save`).
fn raw_path_of(url: &str) -> String {
    let no_fragment = url.split('#').next().unwrap_or("");
    let no_query = no_fragment.split('?').next().unwrap_or("");
    let rest = match no_query.find("://") {
        Some(i) => &no_query[i + 3..],
        None => no_query,
    };
    match rest.find('/') {
        Some(i) => rest[i..].to_string(),
        None => "/".to_string(),
    }
}

/// Does a path segment look like a token (a session id, a code, a ticket)?
/// 20 or more characters of base64url or hex that hold a digit (a long
/// word is a name, not a token), or 16 or more with no vowel at all.
fn looks_like_token(seg: &str) -> bool {
    let n = seg.chars().count();
    let url_safe = seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (n >= 20 && url_safe && seg.chars().any(|c| c.is_ascii_digit()))
        || (n >= 16 && !seg.chars().any(|c| "aeiouAEIOU".contains(c)))
}

/// ASP.NET's cookieless session segment: `(S(...))`, `(F(...))`, `(A(...))`
/// (the session id, the forms ticket, the anonymous id).
fn is_cookieless(seg: &str) -> bool {
    let b = seg.as_bytes();
    b.len() > 4 && b[0] == b'(' && matches!(b[1], b'S' | b's' | b'F' | b'f' | b'A' | b'a') && b[2] == b'(' && seg.ends_with("))")
}

/// The path of an address, to be said to a person or written to the log: no
/// scheme, host, query or fragment, and nothing in it that could be a
/// session - no `;parameter` (`;jsessionid=...`), no ASP.NET cookieless
/// segment, and a token-like segment shown as `…`. `/` when nothing follows
/// the host.
pub fn path_of(url: &str) -> String {
    let raw = raw_path_of(url);
    let kept: Vec<String> = raw
        .split('/')
        .skip(1)
        .filter(|seg| !is_cookieless(seg))
        .map(|seg| {
            let seg = seg.split(';').next().unwrap_or("");
            if looks_like_token(seg) { "…".to_string() } else { seg.to_string() }
        })
        .collect();
    format!("/{}", kept.join("/"))
}

/// Is this request a save? `patterns` are the project's own words, matched
/// like the built-in ones: case-insensitive substrings of the path.
pub fn is_save(method: &str, url: &str, patterns: &[String]) -> bool {
    let method = method.trim().to_ascii_uppercase();
    if !SAVE_METHODS.contains(&method.as_str()) {
        return false;
    }
    let path = raw_path_of(url);
    if adds(&path) {
        return true;
    }
    let path = path.to_lowercase();
    SAVE_WORDS.iter().any(|w| path.contains(w))
        || patterns
            .iter()
            .map(|p| p.trim().to_lowercase())
            .any(|p| !p.is_empty() && path.contains(&p))
}

/// Does `path` hold a word of `WORD_SAVES` as a word of its own: at the
/// start of a path segment, after a `-`, `_` or `.`, or at a camelCase hump
/// (`CreateCycle`, `insertRow`, `getAddForm`), and ended by the end of the
/// segment, a separator, a digit, or - when the word is written in small or
/// capital-then-small letters - a capital (`AddEmployee`). A word written all
/// in capitals goes on through capitals (`ADDRESS` is one word). Judged
/// before the path is lowercased. `GetCreatedLeaves`, `createdBy`,
/// `/recreate`, `address` and `padding` hold no such word.
pub fn adds(path: &str) -> bool {
    path.split('/').any(|segment| {
        let chars: Vec<char> = segment.chars().collect();
        (0..chars.len()).filter(|&i| starts_a_word(&chars, i)).any(|i| WORD_SAVES.iter().any(|w| word_at(&chars, i, w)))
    })
}

/// Does a word start at `i`: the segment's start, after a separator, or a
/// camelCase hump (a capital after a small letter or a digit, or the last
/// capital of a run followed by a small letter: the `C` of `APICreate`)?
fn starts_a_word(c: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let (prev, here) = (c[i - 1], c[i]);
    if matches!(prev, '-' | '_' | '.') {
        return true;
    }
    if !here.is_ascii_uppercase() {
        return false;
    }
    prev.is_ascii_lowercase()
        || prev.is_ascii_digit()
        || (prev.is_ascii_uppercase() && c.get(i + 1).is_some_and(|n| n.is_ascii_lowercase()))
}

/// Is `word` written at `i` and ended there (see `adds`)?
fn word_at(c: &[char], i: usize, word: &str) -> bool {
    let n = word.chars().count();
    let Some(written) = c.get(i..i + n) else { return false };
    let written: String = written.iter().collect();
    if !written.eq_ignore_ascii_case(word) {
        return false;
    }
    let all_caps = written.chars().all(|ch| ch.is_ascii_uppercase());
    match c.get(i + n) {
        None => true,
        Some(&next) => {
            next.is_ascii_digit() || matches!(next, '-' | '_' | '.') || (next.is_ascii_uppercase() && !all_caps)
        }
    }
}

/// Is this click on a control named like a save? Any link of the target
/// whose accessible name, text or css (id and class tokens) holds a
/// `SAVE_WORDS` or `CLICK_WORDS` word or one of the project's words as a
/// phrase, or a `WORD_SAVES` word as a word of its own.
pub fn click_saves(target: &crate::browser::locator::Target, patterns: &[String]) -> bool {
    selector_words(target).into_iter().any(|text| words_save(text, patterns))
}

/// The same judgement on the words of the element a click landed on: its
/// accessible name, text, `value`, `aria-label` and `title` (its nearest
/// button's or link's too). Never its id or class: too noisy.
pub fn element_saves(texts: &[String], patterns: &[String]) -> bool {
    texts.iter().any(|t| words_save(t, patterns))
}

/// Does pressing this key submit a form (Enter, Space)?
pub fn key_submits(key: &str) -> bool {
    let last = key.rsplit('+').next().unwrap_or("").trim().to_lowercase();
    let last = if key.trim() == "+" { String::new() } else { last };
    matches!(last.as_str(), "enter" | "numpadenter" | "return" | "space" | "spacebar" | "") && !key.trim().is_empty()
}

fn selector_words(target: &crate::browser::locator::Target) -> Vec<&str> {
    use crate::browser::locator::Target;
    let mut phrases: Vec<&str> = Vec::new();
    match target {
        Target::Legacy(s) => phrases.push(s.as_str()),
        Target::One(s) => phrases.extend([s.name.as_deref(), s.text.as_deref(), s.css.as_deref()].into_iter().flatten()),
        Target::Chain(c) => {
            for s in c {
                phrases.extend([s.name.as_deref(), s.text.as_deref(), s.css.as_deref()].into_iter().flatten());
            }
        }
    }
    phrases
}

fn words_save(text: &str, patterns: &[String]) -> bool {
    {
        let lower = text.to_lowercase();
        // Words as path-like segments for `adds`: anything that is not a
        // name character separates.
        let segmented: String =
            text.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '/' }).collect();
        // camelCase humps and separators split words: "btnFinish" is btn, finish.
        let mut spaced = String::new();
        let mut prev_lower = false;
        for c in text.chars() {
            if c.is_ascii_uppercase() && prev_lower {
                spaced.push(' ');
            }
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
            spaced.push(if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' });
        }
        adds(&segmented)
            || SAVE_WORDS.iter().any(|w| lower.contains(w))
            || spaced.split_whitespace().any(|t| CLICK_WORDS.iter().any(|w| t == *w || t.strip_suffix('s') == Some(*w)))
            || patterns.iter().map(|p| p.trim().to_lowercase()).any(|p| !p.is_empty() && lower.contains(&p))
    }
}

/// Does the guard stop this request: a save by its path (`is_save`), or any
/// writing request while the window after a save-named click is open.
pub fn verdict(method: &str, url: &str, patterns: &[String], save_click_open: bool) -> bool {
    is_save(method, url, patterns)
        || (save_click_open && SAVE_METHODS.contains(&method.trim().to_ascii_uppercase().as_str()))
}

/// What a no-save answer says about writes that got past the guard: method
/// and path only.
pub fn let_through_line(posted: &[(String, String)]) -> Option<String> {
    if posted.is_empty() {
        return None;
    }
    let list: Vec<String> = posted.iter().map(|(m, p)| format!("{} {}", m.trim().to_ascii_uppercase(), p)).collect();
    Some(format!(
        "went through the no-save guard (no save word in the path): {} - if one of them saves, a person adds a save word on the Auto Run Setup tab",
        list.join(", ")
    ))
}

/// The case's failure when the page tried to save: the method and the path,
/// never the host or the query.
pub fn blocked(method: &str, url: &str) -> String {
    format!("{BLOCKED_START}{} {}{BLOCKED_END}", method.trim().to_ascii_uppercase(), path_of(url))
}

/// Did the guard stop this request? The guard fails a request with
/// `errorReason: "BlockedByClient"`, which Chrome reports as
/// `net::ERR_BLOCKED_BY_CLIENT`.
pub fn stopped_by_guard(e: &crate::browser::net_record::NetEntry) -> bool {
    matches!(&e.state, crate::browser::net_record::NetState::Failed(r) if r.contains("ERR_BLOCKED_BY_CLIENT"))
}

/// Did any of these requests write, and get past the guard?
pub fn sent_a_save(sent: &[crate::browser::net_record::NetEntry]) -> bool {
    sent.iter().any(|e| SAVE_METHODS.contains(&e.method.as_str()) && !stopped_by_guard(e))
}

/// The paths (query left out) of the writing requests among `sent` that the
/// guard did not stop, each once.
pub fn posted_paths(sent: &[crate::browser::net_record::NetEntry]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in sent.iter().filter(|e| SAVE_METHODS.contains(&e.method.as_str()) && !stopped_by_guard(e)) {
        let path = path_of(&e.path_query);
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// Is this one of `blocked`'s sentences?
pub fn is_blocked(detail: &str) -> bool {
    detail.starts_with(BLOCKED_START) && detail.ends_with(BLOCKED_END)
}

/// A no-save case's refusal when interception could not be switched on.
pub fn setup_failed(why: &str) -> String {
    format!("{SETUP_FAILED}{why}")
}

/// The resource types (CDP's `Network.ResourceType`) whose requests are
/// paused: every type that can carry a POST, PUT, PATCH or DELETE. A form
/// post is a `Document` (an iframe's too), a beacon a `Ping`, which the
/// sign-in guard judges by type (`cdp`). `WebSocket` cannot be paused by
/// `Fetch`, and Edge refuses a `FedCM` pattern (the whole `Fetch.enable`
/// with it), so neither is here.
pub const PAUSED_TYPES: [&str; 6] = ["Document", "XHR", "Fetch", "Ping", "EventSource", "Other"];

/// Never paused: a page only reads these, with a GET, so `is_save` would
/// let every one through anyway. `Prefetch`, `SignedExchange` and
/// `Preflight`, also left out, are a GET or an OPTIONS too, and `is_save`
/// lets every GET, HEAD and OPTIONS through, so pausing them would only
/// cost time. A `CSPViolationReport` is never a save.
pub const UNPAUSED_TYPES: [&str; 7] = ["Image", "Stylesheet", "Script", "Font", "Media", "Manifest", "TextTrack"];

/// What `Fetch.enable` is sent: every request of a `PAUSED_TYPES` type,
/// paused before it is sent.
pub fn fetch_enable_params() -> Value {
    let patterns: Vec<Value> = PAUSED_TYPES
        .iter()
        .map(|t| json!({ "urlPattern": "*", "resourceType": t, "requestStage": "Request" }))
        .collect();
    json!({ "patterns": patterns })
}

/// What `Fetch.enable` is sent when a browser refused `fetch_enable_params`
/// (one that does not know a type): every request, paused. Slower, never
/// less guarded. Refused too, the guard fails closed.
pub fn fetch_enable_all_params() -> Value {
    json!({ "patterns": [{ "urlPattern": "*", "requestStage": "Request" }] })
}

/// What the log says when a browser refused the typed patterns, followed by
/// the browser's own reason (about the patterns, never an address).
pub const TYPED_REFUSED: &str =
    "Auto Run's Must not save guard pauses every request in this browser, which refused the request types: ";

/// A project's own save words as they are kept: trimmed, lowercased, in the
/// order given, once each. Refused with a sentence that names the word.
pub fn check_words(words: &[String]) -> Result<Vec<String>, String> {
    let mut kept: Vec<String> = Vec::with_capacity(words.len());
    for raw in words {
        let word = raw.trim().to_lowercase();
        if word.is_empty() {
            return Err("a save word cannot be blank".to_string());
        }
        if word.chars().count() > MAX_WORD_LEN {
            return Err(format!("\"{word}\" is too long - a save word is at most {MAX_WORD_LEN} characters"));
        }
        if word.chars().any(char::is_whitespace) {
            return Err(format!("\"{word}\" has a space in it - a save word is matched against an address path, which has none"));
        }
        if word.contains('?') || word.contains('#') {
            return Err(format!("\"{word}\" cannot match - a save word is matched against the path, never the query or a fragment"));
        }
        if SAVE_WORDS.contains(&word.as_str()) || WORD_SAVES.contains(&word.as_str()) {
            return Err(format!("\"{word}\" is already a built-in save word"));
        }
        if !kept.contains(&word) {
            kept.push(word);
        }
    }
    if kept.len() > MAX_WORDS {
        return Err(format!("a project can have at most {MAX_WORDS} save words of its own"));
    }
    Ok(kept)
}

/// Does a request with this method and address (a query left out, a host
/// too) read rather than save: a writing method whose path names a
/// `READ_WORDS` word and no save word of its own? Judged by the path's
/// words (camelCase humps and separators split them): `POST /api/GetList`
/// and `/lookup/search` read; `/SaveSearch` saves.
pub fn looks_like_read(method: &str, url: &str) -> bool {
    if !SAVE_METHODS.contains(&method.trim().to_ascii_uppercase().as_str()) {
        return false;
    }
    let path = raw_path_of(url);
    // A long word is unambiguous written run together ("AutoComplete" would
    // split into "auto" and "complete", "AutoSave" into one holding "save").
    let lower = path.to_ascii_lowercase();
    if READ_WORDS.iter().any(|w| w.len() >= 8 && lower.contains(w)) {
        return true;
    }
    let mut spaced = String::new();
    let mut prev_lower = false;
    for c in path.chars() {
        if c.is_ascii_uppercase() && prev_lower {
            spaced.push(' ');
        }
        prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        spaced.push(if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' });
    }
    let is = |t: &str, w: &str| t == w || t.strip_suffix('s') == Some(w);
    let words: Vec<&str> = spaced.split_whitespace().collect();
    let saves = words.iter().any(|t| SAVE_WORDS.iter().chain(WORD_SAVES.iter()).any(|w| is(t, w)));
    !saves && words.iter().any(|t| READ_WORDS.iter().any(|w| is(t, w)))
}
