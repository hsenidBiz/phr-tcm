//! A signed-in browser, written down and put back.
//!
//! Every Auto Run case starts from a fresh profile or browser context, so that one
//! case can never pass only because another signed in. Signing in through
//! the page for every case would make that honesty slow. A saved session
//! keeps both: the profile is still fresh, and the cookies and local
//! storage that mean "signed in" are put back into it. Verified on real
//! Edge: HttpOnly cookies are captured and restored, and local storage
//! seeded by `Page.addScriptToEvaluateOnNewDocument` is there before the
//! page's own scripts look for it.

use super::cdp::{CdpError, Driver};
use super::page;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OriginStorage {
    pub origin: String,
    pub entries: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SavedSession {
    pub saved_at_ms: u64,
    /// As `Network.getAllCookies` returned them.
    pub cookies: Vec<Value>,
    pub local_storage: Vec<OriginStorage>,
}

/// The host of an `https://host[:port]` origin. `file://` has none.
pub fn host_of(origin: &str) -> Option<String> {
    let rest = origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://"))?;
    let host = rest.split(':').next().unwrap_or("");
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The cookie domain rule: the host itself, or a parent domain of it.
pub fn cookie_belongs(cookie_domain: &str, host: &str) -> bool {
    let d = cookie_domain.trim_start_matches('.').to_ascii_lowercase();
    let h = host.to_ascii_lowercase();
    !d.is_empty() && (h == d || h.ends_with(&format!(".{d}")))
}

/// An expression this app wrote, with no input in it.
pub const CAPTURE_STORAGE_JS: &str = r#"(() => {
  try { return { origin: location.origin, entries: Object.entries(localStorage) }; }
  catch (e) { return { origin: location.origin, entries: [] }; }
})()"#;

/// The browser's cookies that belong to `origins`: the set a saved session
/// holds (`capture`). Read only.
async fn own_cookies<D: Driver>(d: &mut D, origins: &[String]) -> Result<Vec<Value>, CdpError> {
    let hosts: Vec<String> = origins.iter().filter_map(|o| host_of(o)).collect();
    let reply = d.call("Network.getAllCookies", json!({})).await?;
    Ok(reply["cookies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| {
            let domain = c["domain"].as_str().unwrap_or("");
            hosts.iter().any(|h| cookie_belongs(domain, h))
        })
        .cloned()
        .collect())
}

/// Which of its own cookies a sign-in left in a browser - by name, domain
/// and path - and a SHA-256 of their name=value pairs. Taken at each sign-in
/// the app makes (`cookie_print`) and kept in memory only, with the
/// browser's lease (`autorun::lease::Held`): never logged, never written,
/// and its `Debug` shows only how many cookies it covers. The same cookies
/// with the same values later (`same_cookies`) are the same sign-in; a value
/// changed or a cookie gone is not. A cookie the page set after the sign-in
/// is not part of it.
#[derive(Clone, PartialEq, Eq)]
pub struct CookiePrint {
    keys: Vec<(String, String, String)>,
    digest: Vec<u8>,
}

impl std::fmt::Debug for CookiePrint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CookiePrint({} cookies)", self.keys.len())
    }
}

fn cookie_key(c: &Value) -> (String, String, String) {
    let s = |k: &str| c[k].as_str().unwrap_or("").to_string();
    (s("name"), s("domain").to_ascii_lowercase(), s("path"))
}

/// The digest of `keys`' values among `cookies`, in `keys`' order; `None`
/// when one of them is not there.
fn digest_of(cookies: &[Value], keys: &[(String, String, String)]) -> Option<Vec<u8>> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for key in keys {
        let c = cookies.iter().find(|c| cookie_key(c) == *key)?;
        for part in [key.0.as_str(), key.1.as_str(), key.2.as_str(), c["value"].as_str().unwrap_or("")] {
            h.update((part.len() as u64).to_le_bytes());
            h.update(part.as_bytes());
        }
    }
    Some(h.finalize().to_vec())
}

/// The print of the sign-in the browser holds now: its own cookies for
/// `origins` (read only). `None` when it holds none - nothing then shows
/// whose sign-in it is - or the browser does not answer.
pub async fn cookie_print<D: Driver>(d: &mut D, origins: &[String]) -> Option<CookiePrint> {
    let cookies = own_cookies(d, origins).await.ok()?;
    let mut keys: Vec<(String, String, String)> = cookies.iter().map(cookie_key).collect();
    keys.sort();
    keys.dedup();
    if keys.is_empty() {
        return None;
    }
    let digest = digest_of(&cookies, &keys)?;
    Some(CookiePrint { keys, digest })
}

/// Does the browser still hold the sign-in `print` was taken of: each of
/// its cookies there, with the same value? Read only; a browser that does
/// not answer does not.
pub async fn same_cookies<D: Driver>(d: &mut D, origins: &[String], print: &CookiePrint) -> bool {
    match own_cookies(d, origins).await {
        Ok(cookies) => digest_of(&cookies, &print.keys).is_some_and(|now| now == print.digest),
        Err(_) => false,
    }
}

pub async fn capture<D: Driver>(
    d: &mut D,
    origins: &[String],
    now_ms: u64,
) -> Result<SavedSession, CdpError> {
    let cookies = own_cookies(d, origins).await?;

    let page_storage = page::eval_value(d, CAPTURE_STORAGE_JS).await?;
    let origin = page_storage["origin"].as_str().unwrap_or("").to_ascii_lowercase();
    let entries: Vec<(String, String)> = page_storage["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e[0].as_str()?.to_string(), e[1].as_str()?.to_string())))
        .collect();
    let local_storage = if !entries.is_empty() && origins.iter().any(|o| o.eq_ignore_ascii_case(&origin)) {
        vec![OriginStorage { origin, entries }]
    } else {
        vec![]
    };
    Ok(SavedSession { saved_at_ms: now_ms, cookies, local_storage })
}

/// What `Network.setCookies` accepts, from what `getAllCookies` gave. A
/// session cookie has no expiry to send; `size`, `priority` and the rest
/// are the browser's own bookkeeping.
fn cookie_param(c: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for key in ["name", "value", "domain", "path"] {
        if let Some(v) = c.get(key).filter(|v| v.is_string()) {
            out.insert(key.to_string(), v.clone());
        }
    }
    for key in ["httpOnly", "secure"] {
        if let Some(v) = c.get(key).filter(|v| v.is_boolean()) {
            out.insert(key.to_string(), v.clone());
        }
    }
    if let Some(v) = c.get("sameSite").filter(|v| v.is_string()) {
        out.insert("sameSite".to_string(), v.clone());
    }
    let session = c["session"].as_bool().unwrap_or(true);
    if !session {
        if let Some(v) = c.get("expires").filter(|v| v.as_f64().is_some_and(|e| e > 0.0)) {
            out.insert("expires".to_string(), v.clone());
        }
    }
    Value::Object(out)
}

/// Seeds local storage before the page's own scripts run, on its own
/// origin only, never overwriting what is already there.
///
/// THE ONE PLACE data becomes JavaScript source in this app:
/// `Page.addScriptToEvaluateOnNewDocument` takes source text and offers no
/// arguments. The data is captured browser state, not something a person
/// or an assistant authored, and it is embedded as a JSON literal, which
/// is valid JavaScript.
pub fn seed_script(storage: &[OriginStorage]) -> String {
    if storage.is_empty() {
        return String::new();
    }
    let data: Vec<Value> = storage.iter().map(|s| json!({ "origin": s.origin, "entries": s.entries })).collect();
    // A raw U+2028/U+2029 inside this JSON string literal (valid JSON,
    // valid inside the JS template literal below) relies on ES2019: before
    // it, those two code points terminated a JS string same as a real
    // newline would. Every browser this drives is ES2019+.
    let data = serde_json::to_string(&data).unwrap_or_else(|_| "[]".to_string());
    format!(
        "(() => {{ try {{ for (const s of {data}) {{ if (location.origin !== s.origin) continue; \
         for (const [k, v] of s.entries) {{ if (localStorage.getItem(k) === null) localStorage.setItem(k, v); }} }} }} catch (e) {{}} }})();"
    )
}

pub async fn restore<D: Driver>(d: &mut D, saved: &SavedSession) -> Result<Vec<String>, CdpError> {
    if !saved.cookies.is_empty() {
        let cookies: Vec<Value> = saved.cookies.iter().map(cookie_param).collect();
        d.call("Network.setCookies", json!({ "cookies": cookies })).await?;
    }
    let source = seed_script(&saved.local_storage);
    if source.is_empty() {
        return Ok(vec![]);
    }
    let r = d.call("Page.addScriptToEvaluateOnNewDocument", json!({ "source": source })).await?;
    Ok(r["identifier"].as_str().map(|s| vec![s.to_string()]).unwrap_or_default())
}

/// Stop seeding. Housekeeping: a failure here is ignored.
pub async fn unseed<D: Driver>(d: &mut D, ids: &[String]) {
    for id in ids {
        let _ = d.call("Page.removeScriptToEvaluateOnNewDocument", json!({ "identifier": id })).await;
    }
}

/// Back to signed out, in the same browser: how one account makes way for
/// another in the middle of a case.
pub async fn clear<D: Driver>(d: &mut D, origins: &[String]) -> Result<(), CdpError> {
    d.call("Network.clearBrowserCookies", json!({})).await?;
    for origin in origins.iter().filter(|o| host_of(o).is_some()) {
        d.call("Storage.clearDataForOrigin", json!({ "origin": origin, "storageTypes": "cookies,local_storage" })).await?;
    }
    Ok(())
}
