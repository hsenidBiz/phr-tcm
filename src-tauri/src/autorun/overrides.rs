//! The test headers and cookies the assistant sets in its own Auto Run
//! browser (`set_autorun_headers`, `set_autorun_cookie`), so it can test an
//! edge case - a changed header, a feature cookie - and what TCM recorded of
//! them (`Overrides`, kept on the session), so every try, discovery and
//! replay answer names them while they are in place and a reset takes away
//! exactly what was set.
//!
//! Guards (spec decisions 1, 2 and 4):
//! - headers go through `Network.setExtraHTTPHeaders` only, never `Fetch`
//!   (the save guard's interceptor), and never `cookie`, `authorization`,
//!   `proxy-authorization` or `host`;
//! - a cookie of the sign-in's (`protected_cookie`) is never set, and its
//!   value never shown;
//! - cookies are read and written with the raw CDP calls, as JSON, never a
//!   typed parser (one was found to hang on a missing field).

use crate::autorun::capture_redact::{redact_text, secret_name};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::net_capture::HIDDEN;
use crate::browser::session::cookie_belongs;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// What the assistant changed for testing in its Auto Run browser, as TCM
/// set it. Gone with the browser; a reset takes it away
/// (`undo`), and so does a clear of the cookies (`clear_autorun_cookies`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// Extra request headers sent on every tab, by name as the assistant
    /// wrote it (one name per header, compared in any case), with its value.
    pub headers: BTreeMap<String, String>,
    /// The cookies the assistant set, by name, with exactly where and what
    /// it set (`SetCookie`): a reset deletes one only while it is still that.
    pub cookies: BTreeMap<String, SetCookie>,
}

/// One cookie as the assistant set it: on the app's host only (`domain`,
/// never a parent domain), at `path`, holding `value` - the assistant's own
/// value, never one the page or the sign-in wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetCookie {
    pub domain: String,
    pub path: String,
    pub value: String,
}

impl SetCookie {
    /// Whether the browser's cookie `c` is still this one: the same name,
    /// domain, path and value. One the page or a sign-in wrote over it since
    /// is not.
    pub fn is(&self, name: &str, c: &Value) -> bool {
        c["name"].as_str() == Some(name)
            && c["domain"].as_str().is_some_and(|d| d.eq_ignore_ascii_case(&self.domain))
            && c["path"].as_str() == Some(self.path.as_str())
            && c["value"].as_str() == Some(self.value.as_str())
    }
}

impl Overrides {
    pub const fn new() -> Self {
        Overrides { headers: BTreeMap::new(), cookies: BTreeMap::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && self.cookies.is_empty()
    }

    /// `header A, B; cookie c`: what is changed, by name only (never a
    /// value). `None` while nothing is.
    pub fn names(&self) -> Option<String> {
        let mut parts = Vec::new();
        if !self.headers.is_empty() {
            parts.push(format!("header {}", self.headers.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        if !self.cookies.is_empty() {
            parts.push(format!("cookie {}", self.cookies.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }

    /// The line every try, discovery and replay answer ends with while
    /// anything is changed.
    pub fn changed_line(&self) -> Option<String> {
        self.names().map(|n| format!("{CHANGED_FOR_TESTING} {n}."))
    }

    /// The line a replay that passed ends with while anything is changed,
    /// in place of a plain pass.
    pub fn passed_line(&self) -> Option<String> {
        self.names().map(|n| format!("{PASSED_WITH_CHANGES} {n}."))
    }
}

/// How `Overrides::changed_line` begins.
pub const CHANGED_FOR_TESTING: &str = "Changed for testing:";

/// How `Overrides::passed_line` begins.
pub const PASSED_WITH_CHANGES: &str = "Passed with these changes in place:";

/// The key a JSON answer carries the changed line under (`with_note`).
pub const NOTE_KEY: &str = "changed_for_testing";

/// `answer` ending with `overrides`' changed line while anything is
/// changed: a JSON object gets it as its last member (`changed_for_testing`),
/// any other answer on a line of its own after its text.
pub fn with_note(answer: (u16, String), overrides: &Overrides) -> (u16, String) {
    let Some(line) = overrides.changed_line() else {
        return answer;
    };
    let (status, text) = answer;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(mut map)) => {
            map.insert(NOTE_KEY.to_string(), json!(line));
            (status, Value::Object(map).to_string())
        }
        _ => (status, format!("{text}\n{line}")),
    }
}

// ------------------------------------------------------------------ headers

/// Request headers never set for testing: they are the sign-in's, or the
/// browser's own.
pub const REFUSED_HEADERS: [&str; 4] = ["cookie", "authorization", "proxy-authorization", "host"];

/// Request headers that make a server route a request as another method or
/// to another path: the save guard judges the method and the path of the
/// request line, so a POST it lets through could act as a PUT or a DELETE
/// (the method overrides), or reach a save address (`X-Original-URL` and
/// `X-Rewrite-URL`, honoured by IIS URL Rewrite, ASP.NET and some
/// frameworks). Never set for testing either.
pub const ROUTING_OVERRIDE_HEADERS: [&str; 5] =
    ["x-http-method-override", "x-http-method", "x-method-override", "x-original-url", "x-rewrite-url"];

/// Whether `name` is one of `REFUSED_HEADERS` or `ROUTING_OVERRIDE_HEADERS`,
/// in any case.
pub fn refused_header(name: &str) -> bool {
    REFUSED_HEADERS.iter().chain(ROUTING_OVERRIDE_HEADERS.iter()).any(|h| name.trim().eq_ignore_ascii_case(h))
}

/// Said when a refused header is asked for.
pub fn refused_header_sentence(name: &str) -> String {
    if ROUTING_OVERRIDE_HEADERS.iter().any(|h| name.trim().eq_ignore_ascii_case(h)) {
        return format!(
            "{name} is never set as a test header: it makes the server take a request for another path or method, so a request the save guard lets through could save or delete."
        );
    }
    format!(
        "{name} is never set as a test header: cookie, authorization, proxy-authorization and host belong to the sign-in and the browser. Set a cookie that is not the sign-in's with set_autorun_cookie."
    )
}

/// The most test headers in place at once.
pub const MAX_HEADERS: usize = 20;

/// The longest header name and value taken.
const MAX_HEADER_NAME: usize = 100;
const MAX_HEADER_VALUE: usize = 4096;

/// What `set_autorun_headers` takes.
pub const HEADERS_SHAPE: &str = "{ \"headers\": { \"<name>\": \"<value>\" | null } }";

/// A token character of a header or cookie name (RFC 9110's `tchar`).
fn tchar(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
}

/// The changes `set_autorun_headers` asks for, in order: a value to set, or
/// `None` (a `null`) to remove one TCM set. `Err` is the sentence: nothing
/// is changed then.
pub fn header_changes(body: &Value) -> Result<Vec<(String, Option<String>)>, String> {
    let Some(asked) = body.get("headers").and_then(Value::as_object) else {
        return Err(format!("\"headers\" is an object of header names. Expected {HEADERS_SHAPE}."));
    };
    if asked.is_empty() {
        return Err(format!("name at least one header. Expected {HEADERS_SHAPE}."));
    }
    let mut out = Vec::with_capacity(asked.len());
    for (name, value) in asked {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_HEADER_NAME || !name.chars().all(tchar) {
            return Err(format!(
                "\"{}\" is not a header name: letters, digits and - _ . ! # $ % & ' * + ^ ` | ~ only, at most {MAX_HEADER_NAME}.",
                name.chars().take(MAX_HEADER_NAME).collect::<String>()
            ));
        }
        if refused_header(name) {
            return Err(refused_header_sentence(name));
        }
        let value = match value {
            Value::Null => None,
            Value::String(v) => {
                if v.len() > MAX_HEADER_VALUE || v.chars().any(|c| c.is_control() && c != '\t') {
                    return Err(format!(
                        "{name}'s value must be one line of at most {MAX_HEADER_VALUE} characters."
                    ));
                }
                Some(v.clone())
            }
            _ => return Err(format!("{name}'s value is a string, or null to remove it. Expected {HEADERS_SHAPE}.")),
        };
        out.push((name.to_string(), value));
    }
    Ok(out)
}

/// `current` with `changes` made: a name replaces any written in another
/// case, and `None` removes it. `Err` when more than `MAX_HEADERS` would be
/// in place.
pub fn merged(
    current: &BTreeMap<String, String>,
    changes: &[(String, Option<String>)],
) -> Result<BTreeMap<String, String>, String> {
    let mut out = current.clone();
    for (name, value) in changes {
        out.retain(|k, _| !k.eq_ignore_ascii_case(name));
        if let Some(v) = value {
            out.insert(name.clone(), v.clone());
        }
    }
    if out.len() > MAX_HEADERS {
        return Err(format!("at most {MAX_HEADERS} test headers are in place at once; remove some with null first."));
    }
    Ok(out)
}

// ------------------------------------------------------------------ cookies

/// Whether a cookie is the sign-in's, and so never set, deleted by name or
/// shown: an `HttpOnly` one, the ASP.NET sign-in and anti-forgery cookies,
/// or one named with a secret word (`capture_redact::secret_name`, whose
/// `auth` is a word of its own: `authToken` is protected, `author` is not).
/// Names are compared in any case. The same rule with or without a cookie
/// fingerprint (spec decision 2).
pub fn protected_cookie(name: &str, http_only: bool) -> bool {
    if http_only {
        return true;
    }
    let n = name.trim();
    let lower = n.to_ascii_lowercase();
    lower == ".aspxauth"
        || lower.starts_with(".aspnetcore.")
        || lower == "asp.net_sessionid"
        || lower.starts_with("__requestverificationtoken")
        || secret_name(n)
}

/// Said when a protected cookie is asked to be set.
pub fn protected_cookie_sentence(name: &str) -> String {
    format!(
        "{name} is the sign-in's cookie (HttpOnly, the sign-in or anti-forgery cookie, or named like a secret): it is never set or deleted for testing. clear_autorun_cookies signs out for real."
    )
}

/// What `set_autorun_cookie` takes.
pub const COOKIE_SHAPE: &str = "{ \"name\": \"<name>\", \"value\": \"<value>\", \"path\": \"/\" (optional), \"expires\": <seconds since 1970, optional>, \"secure\": true | false (optional), \"same_site\": \"Strict\" | \"Lax\" | \"None\" (optional) }";

/// One cookie `set_autorun_cookie` asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct CookieAsk {
    pub name: String,
    pub value: String,
    pub path: String,
    pub expires: Option<f64>,
    pub secure: Option<bool>,
    pub same_site: Option<&'static str>,
}

const MAX_COOKIE_NAME: usize = 200;
const MAX_COOKIE_VALUE: usize = 4096;

/// The cookie asked for, checked. `Err` is the sentence: nothing is set.
pub fn cookie_ask(body: &Value) -> Result<CookieAsk, String> {
    let name = body.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
    if name.is_empty() || name.len() > MAX_COOKIE_NAME || !name.chars().all(tchar) {
        return Err(format!(
            "name is the cookie's name: letters, digits and - _ . ! # $ % & ' * + ^ ` | ~ only, at most {MAX_COOKIE_NAME}. Expected {COOKIE_SHAPE}."
        ));
    }
    let Some(value) = body.get("value").and_then(Value::as_str) else {
        return Err(format!("value is the cookie's value, a string. Expected {COOKIE_SHAPE}."));
    };
    if value.len() > MAX_COOKIE_VALUE || value.contains(';') || value.chars().any(char::is_control) {
        return Err(format!("{name}'s value must be one line of at most {MAX_COOKIE_VALUE} characters, with no ;."));
    }
    let path = match body.get("path") {
        None | Some(Value::Null) => "/".to_string(),
        Some(Value::String(p)) if p.starts_with('/') && p.len() <= 512 && !p.contains(';') && !p.chars().any(char::is_control) => {
            p.clone()
        }
        Some(_) => return Err(format!("path starts with / (at most 512 characters, no ;). Expected {COOKIE_SHAPE}.")),
    };
    let expires = match body.get("expires") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_f64() {
            Some(n) if n > 0.0 && n.is_finite() => Some(n),
            _ => return Err(format!("expires is a time in seconds since 1970. Expected {COOKIE_SHAPE}.")),
        },
    };
    let secure = match body.get("secure") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(format!("secure is true or false. Expected {COOKIE_SHAPE}.")),
    };
    let same_site = match body.get("same_site") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
            "strict" => Some("Strict"),
            "lax" => Some("Lax"),
            "none" => Some("None"),
            _ => return Err(format!("same_site is Strict, Lax or None. Expected {COOKIE_SHAPE}.")),
        },
        Some(_) => return Err(format!("same_site is Strict, Lax or None. Expected {COOKIE_SHAPE}.")),
    };
    Ok(CookieAsk { name: name.to_string(), value: value.to_string(), path, expires, secure, same_site })
}

/// One cookie as `get_autorun_cookies` shows it: name, domain, path,
/// expiry, flags and size, and the value - `(hidden)` for a protected
/// cookie, and every other one redacted as free text (a token under an
/// innocent name is hidden too). `set_for_testing` when the assistant set it.
pub fn shown_cookie(c: &Value, set_for_testing: bool) -> Value {
    let name = c["name"].as_str().unwrap_or("");
    let http_only = c["httpOnly"].as_bool().unwrap_or(false);
    let protected = protected_cookie(name, http_only);
    let session = c["session"].as_bool().unwrap_or(false) || c["expires"].as_f64().is_none_or(|e| e <= 0.0);
    let value = if protected { HIDDEN.to_string() } else { redact_text(c["value"].as_str().unwrap_or("")) };
    json!({
        "name": name,
        "domain": c["domain"].as_str().unwrap_or(""),
        "path": c["path"].as_str().unwrap_or(""),
        "expires": if session { json!("session") } else { c["expires"].clone() },
        "http_only": http_only,
        "secure": c["secure"].as_bool().unwrap_or(false),
        "same_site": c["sameSite"].as_str().unwrap_or(""),
        "size": c["size"].as_u64().unwrap_or(0),
        "value": value,
        "protected": protected,
        "set_for_testing": set_for_testing,
    })
}

/// The most cookies one answer lists.
pub const MAX_LISTED: usize = 200;

/// Said when the browser will not take a cookie.
pub const COOKIE_REFUSED: &str = "The browser refused that cookie: a same_site None cookie must be secure, and a secure one needs an https page.";

/// The host of an http or https address, lower case: `https://App.example:8443/x?y`
/// is `app.example`. `None` for any other address.
pub fn url_host(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("http://") {
        &url[7..]
    } else {
        return None;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// A browser call that failed, as an answer (`CdpError`'s own words: a
/// closed browser is told by them, `ai_bridge::let_go_if_silent`).
pub fn browser_failed(e: CdpError) -> (u16, String) {
    let status = if matches!(e, CdpError::Protocol { .. }) { 502 } else { 503 };
    (status, format!("the Auto Run browser did not answer: {e}"))
}

/// Every cookie the browser holds for `host` (its own and its parent
/// domains'), any path, as the browser lists them (`Network.getAllCookies`,
/// read as JSON).
pub async fn cookies_for_host<D: Driver>(d: &mut D, host: &str) -> Result<Vec<Value>, CdpError> {
    let reply = d.call("Network.getAllCookies", json!({})).await?;
    Ok(reply["cookies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| cookie_belongs(c["domain"].as_str().unwrap_or(""), host))
        .cloned()
        .collect())
}

/// The `Network.setCookie` parameters for `ask` on the site at `origin`
/// (its own host, never a parent domain).
pub fn set_cookie_params(ask: &CookieAsk, origin: &str) -> Value {
    let mut p = json!({ "name": ask.name, "value": ask.value, "url": format!("{origin}/"), "path": ask.path });
    if let Some(e) = ask.expires {
        p["expires"] = json!(e);
    }
    if let Some(s) = ask.secure {
        p["secure"] = json!(s);
    }
    if let Some(s) = ask.same_site {
        p["sameSite"] = json!(s);
    }
    p
}

/// Said by the cookie tools before TCM's sign-in has confirmed the app's
/// own site (`NetCapture::confirmed_site`): they act only there.
pub const SITE_NOT_CONFIRMED: &str =
    "The app's site is not confirmed yet: sign in through a replay or a discovery first.";

/// The app's own site (`https://host[:port]`), as TCM's sign-in confirmed
/// it in this browser's capture - never whatever page is showing, which may
/// be the identity provider's. `Err` (409 `SITE_NOT_CONFIRMED`) before it is.
pub fn confirmed_origin<D: Driver>(d: &D) -> Result<String, (u16, String)> {
    d.capture()
        .and_then(|c| c.confirmed_site())
        .filter(|site| url_host(site).is_some())
        .map(|site| site.trim_end_matches('/').to_string())
        .ok_or_else(|| (409, SITE_NOT_CONFIRMED.to_string()))
}

/// Take away everything `o` records from the browser: the test headers
/// (an empty set on every tab) and each cookie the assistant set - only
/// while the cookie there is still exactly the one it set (`SetCookie::is`,
/// value included). One the page or a sign-in wrote over since is left
/// alone, and no longer counted as the assistant's. What could not be taken
/// away stays recorded, and is named in what this returns (empty when
/// everything went).
pub async fn undo<D: Driver>(d: &mut D, o: &mut Overrides) -> Vec<String> {
    let mut left = Vec::new();
    if !o.headers.is_empty() {
        match d.set_extra_headers(&BTreeMap::new()).await {
            Ok(()) => o.headers.clear(),
            Err(e) => {
                crate::applog::warn(format!("Auto Run: the test headers could not be removed: {e}"));
                left.push(format!("header {}", o.headers.keys().cloned().collect::<Vec<_>>().join(", ")));
            }
        }
    }
    if o.cookies.is_empty() {
        return left;
    }
    let jar = match d.call("Network.getAllCookies", json!({})).await {
        Ok(reply) => reply["cookies"].as_array().cloned().unwrap_or_default(),
        Err(e) => {
            crate::applog::warn(format!("Auto Run: the test cookies could not be read to remove them: {e}"));
            left.extend(o.cookies.keys().map(|n| format!("cookie {n}")));
            return left;
        }
    };
    let cookies: Vec<(String, SetCookie)> = o.cookies.iter().map(|(n, c)| (n.clone(), c.clone())).collect();
    for (name, set) in cookies {
        if !jar.iter().any(|c| set.is(&name, c)) {
            // Gone, or written over since: not the assistant's any more.
            o.cookies.remove(&name);
            continue;
        }
        let params = json!({ "name": name, "domain": set.domain, "path": set.path });
        match d.call("Network.deleteCookies", params).await {
            Ok(_) => {
                o.cookies.remove(&name);
            }
            Err(e) => {
                crate::applog::warn(format!("Auto Run: the test cookie {name} could not be removed: {e}"));
                left.push(format!("cookie {name}"));
            }
        }
    }
    left
}

/// Said when the person takes the browser and the assistant's test changes
/// would not go: the person's action is refused rather than run with them.
pub const TEST_CHANGES_STUCK: &str =
    "Test changes made by the assistant could not be removed: use Release browser, then try again.";

/// Before the person's own step, start, sign-in or replay in this browser:
/// every test change taken away (`undo`), or `Err(TEST_CHANGES_STUCK)` - the
/// person's action never runs with the assistant's changes in place.
pub async fn take_for_person<D: Driver>(d: &mut D, o: &mut Overrides) -> Result<(), String> {
    if o.is_empty() {
        return Ok(());
    }
    let left = undo(d, o).await;
    if left.is_empty() {
        Ok(())
    } else {
        crate::applog::warn(format!(
            "Auto Run: the person took the browser, but these test changes could not be removed: {}",
            left.join("; ")
        ));
        Err(TEST_CHANGES_STUCK.to_string())
    }
}
