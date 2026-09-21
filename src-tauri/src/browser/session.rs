//! A signed-in browser, written down and put back.
//!
//! Every Auto Run case starts from a fresh, throwaway profile, so that one
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

pub async fn capture<D: Driver>(
    d: &mut D,
    origins: &[String],
    now_ms: u64,
) -> Result<SavedSession, CdpError> {
    let hosts: Vec<String> = origins.iter().filter_map(|o| host_of(o)).collect();
    let reply = d.call("Network.getAllCookies", json!({})).await?;
    let cookies: Vec<Value> = reply["cookies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| {
            let domain = c["domain"].as_str().unwrap_or("");
            hosts.iter().any(|h| cookie_belongs(domain, h))
        })
        .cloned()
        .collect();

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
        d.call("Storage.clearDataForOrigin", json!({ "origin": origin, "storageTypes": "local_storage" })).await?;
    }
    Ok(())
}
