//! Cookies whose path differs from a step's only in letter case.
//!
//! Cookie paths are case-sensitive (RFC 6265 §5.1.4): a cookie the
//! application keeps on `/hr/pmsv10` is never sent to `/hr/PMSV10/...`. A
//! hosted PMSV10 did exactly that with its anti-forgery cookie while a
//! template called the capitalised path, so every save went without it and
//! came back an empty 400 - which says nothing about why. The runner reads
//! the browser's jar before each step and, when a step would miss a cookie
//! this way, sends it in the cookie's letter case instead (the application's
//! routing ignores case; only the cookie cares).
//!
//! A cookie is kept here by name, domain and path only. Its value is never
//! copied, so nothing built from this can leak one.

use serde_json::Value;

/// A cookie as the browser holds it - without its value.
#[derive(Debug, Clone, PartialEq)]
pub struct JarCookie {
    pub name: String,
    pub domain: String,
    pub path: String,
}

/// The cookies in a CDP `Network.getAllCookies` / `getCookies` answer (its
/// `cookies` array), by name, domain and path. Anything that is not an
/// array of objects reads as an empty jar.
pub fn jar_cookies(cookies: &Value) -> Vec<JarCookie> {
    let Some(all) = cookies.as_array() else { return vec![] };
    all.iter()
        .map(|c| JarCookie {
            name: c["name"].as_str().unwrap_or_default().to_string(),
            domain: c["domain"].as_str().unwrap_or_default().to_string(),
            path: c["path"].as_str().unwrap_or("/").to_string(),
        })
        .collect()
}

/// The cookies on `host` that would cover `path` if letter case were
/// ignored, but do not cover it as written - the ones a request to `path`
/// silently goes without. `path` is a request path, without its query.
pub fn case_blind_cookies<'a>(jar: &'a [JarCookie], host: &str, path: &str) -> Vec<&'a JarCookie> {
    jar.iter()
        .filter(|c| on_host(&c.domain, host))
        .filter(|c| covers(&c.path.to_ascii_lowercase(), &path.to_ascii_lowercase()) && !covers(&c.path, path))
        .collect()
}

/// `path` with its leading `cookie_path.len()` characters written as the
/// cookie has them: the path the runner sends instead. Only meaningful for
/// a cookie `case_blind_cookies` returned for this path.
pub fn in_cookie_case(path: &str, cookie_path: &str) -> String {
    match path.get(cookie_path.len()..) {
        Some(rest) => format!("{cookie_path}{rest}"),
        None => path.to_string(),
    }
}

/// A cookie on `host` - on a path other than `/`, which covers every path
/// in any case - that `from` carries as written and `to` would not: what
/// sending `to` instead would give up. Adapting must never trade one of the
/// application's cookies for another.
pub fn lost_by_adapting<'a>(jar: &'a [JarCookie], host: &str, from: &str, to: &str) -> Option<&'a JarCookie> {
    jar.iter().find(|c| on_host(&c.domain, host) && c.path != "/" && covers(&c.path, from) && !covers(&c.path, to))
}

/// RFC 6265 domain-match. A domain with a leading `.` is a domain cookie:
/// that host and every host under it. One without is host-only (§5.3):
/// that exact host and no other - the browser never sends it to a
/// subdomain.
fn on_host(domain: &str, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    match domain.strip_prefix('.') {
        Some(parent) => {
            let parent = parent.to_ascii_lowercase();
            !parent.is_empty() && (host == parent || host.ends_with(&format!(".{parent}")))
        }
        None => !domain.is_empty() && host == domain.to_ascii_lowercase(),
    }
}

/// RFC 6265 path-match, case-sensitive as browsers apply it: the paths are
/// equal, or the cookie's path is a prefix of the request's that ends at a
/// `/` - so `/hr/pm` does not cover `/hr/pmsv10`.
fn covers(cookie_path: &str, path: &str) -> bool {
    path == cookie_path
        || (path.starts_with(cookie_path)
            && (cookie_path.ends_with('/') || path[cookie_path.len()..].starts_with('/')))
}
