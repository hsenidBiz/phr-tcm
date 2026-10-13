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

/// The methods that can write. Every other method (GET, HEAD, OPTIONS)
/// always goes through.
pub const SAVE_METHODS: [&str; 4] = ["POST", "PUT", "PATCH", "DELETE"];

/// How the sentence of a blocked save begins (`blocked`).
pub const BLOCKED_START: &str = "this script must not save, but the page tried to send ";
/// How the sentence of a blocked save ends.
pub const BLOCKED_END: &str = " - it was stopped before it reached the server";
/// How a no-save case's refusal begins when interception could not start.
pub const SETUP_FAILED: &str = "the no-save guard could not be set up: ";

/// The most project words kept, and the longest one.
pub const MAX_WORDS: usize = 50;
pub const MAX_WORD_LEN: usize = 60;

/// The path of an address: no scheme, host, query or fragment. `/` when
/// nothing follows the host.
pub fn path_of(url: &str) -> String {
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

/// Is this request a save? `patterns` are the project's own words, matched
/// like the built-in ones: case-insensitive substrings of the path.
pub fn is_save(method: &str, url: &str, patterns: &[String]) -> bool {
    let method = method.trim().to_ascii_uppercase();
    if !SAVE_METHODS.contains(&method.as_str()) {
        return false;
    }
    let path = path_of(url);
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

/// The case's failure when the page tried to save: the method and the path,
/// never the host or the query.
pub fn blocked(method: &str, url: &str) -> String {
    format!("{BLOCKED_START}{} {}{BLOCKED_END}", method.trim().to_ascii_uppercase(), path_of(url))
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
