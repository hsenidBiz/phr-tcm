//! What a no-save script's browser may not send.
//!
//! A script marked `no_save` works on a shared draft, so the page it drives
//! must never be allowed to change it. Its browser intercepts every request
//! (`Fetch.enable`, answered in `cdp`), and this module decides, purely, which
//! requests are saves: a POST, PUT, PATCH or DELETE whose path, lowercased
//! and with the query left out, contains a built-in save word or one of the
//! project's own words. A save is failed inside the browser, so it never
//! reaches the server, and the case fails with `blocked`'s sentence.
//!
//! Nothing here keeps a host, a query string or a body: the sentence names
//! the method and the path only.

use serde_json::{json, Value};

/// The words that make a request with a writing method a save. Fixed: a
/// project can add its own (`check_words`), never remove one of these.
pub const SAVE_WORDS: [&str; 7] = ["save", "update", "delete", "submit", "approve", "publish", "assign"];

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
    let path = path_of(url).to_lowercase();
    SAVE_WORDS.iter().any(|w| path.contains(w))
        || patterns
            .iter()
            .map(|p| p.trim().to_lowercase())
            .any(|p| !p.is_empty() && path.contains(&p))
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

/// What `Fetch.enable` is sent: every request, paused before it is sent.
pub fn fetch_enable_params() -> Value {
    json!({ "patterns": [{ "urlPattern": "*", "requestStage": "Request" }] })
}

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
        if SAVE_WORDS.contains(&word.as_str()) {
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
