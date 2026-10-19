//! HAR 1.2 for the capture store: what the assistant exports, and what it
//! imports from another tool (a browser's "Save all as HAR").
//!
//! The export is redacted exactly as the assistant's own reads are
//! (`capture_redact`): each address and body hidden before it is written,
//! each header already on the capture's allow-list. Its `url` is the
//! application's path and query - never a host. TCM keeps a few things a
//! HAR has no field for (the tab, the failure, the redirect, a body that
//! was dropped or too big) under `_tcm` and `_tcmBody`, as the format
//! allows for custom fields; any other tool ignores them.
//!
//! The import accepts a HAR with `log.entries`, each with a request method
//! and url, a response status and a start time, and nothing else: what is
//! wrong is told in a short sentence that never quotes a value. Everything
//! it takes is redacted again - addresses, queries, bodies - and headers
//! pass the capture's own allow-list, so a HAR from elsewhere (cookies,
//! authorization and all) can never bring a secret in.

use crate::autorun::capture_redact::{
    redact_body, redact_field, redact_header, redact_path_query, redact_text,
};
use crate::browser::net_capture::{
    kept_headers, text_like, Body, Captured, Redirect, MAX_BODY_BYTES, MAX_CAPTURED, MAX_REQUEST_BODY_BYTES,
    MAX_TOTAL_BODY_BYTES,
};
use crate::browser::net_record::origin;
use base64::Engine;
use chrono::{DateTime, SecondsFormat};
use serde_json::{json, Map, Value};

/// `captured`'s header, by lower-case name.
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// HAR's `[{name, value}]`, every value redacted as a header's (`redact_header`).
fn har_headers(headers: &[(String, String)]) -> Value {
    Value::Array(headers.iter().map(|(n, v)| json!({ "name": n, "value": redact_header(n, v) })).collect())
}

/// A body's kind where it is not text: `_tcmBody`.
fn body_tag(body: &Body) -> Option<&'static str> {
    match body {
        Body::Text(_) => None,
        Body::Dropped => Some("dropped"),
        Body::NotText => Some("not-text"),
        Body::TooBig => Some("too-big"),
        Body::OffSite => Some("off-site"),
    }
}

fn tagged(tag: &str) -> Option<Body> {
    Some(match tag {
        "dropped" => Body::Dropped,
        "not-text" => Body::NotText,
        "too-big" => Body::TooBig,
        "off-site" => Body::OffSite,
        _ => return None,
    })
}

/// The query of `pq` as HAR's `queryString`, names and values decoded.
fn query_string(pq: &str) -> Value {
    let query = pq.split('#').next().unwrap_or("").split_once('?').map(|(_, q)| q).unwrap_or("");
    Value::Array(
        query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (n, v) = p.split_once('=').unwrap_or((p, ""));
                let dec = |s: &str| {
                    let spaced = s.replace('+', " ");
                    urlencoding::decode(&spaced).map(|c| c.into_owned()).unwrap_or(spaced)
                };
                json!({ "name": dec(n), "value": dec(v) })
            })
            .collect(),
    )
}

fn started_iso(ms: u64) -> String {
    DateTime::from_timestamp_millis(ms as i64)
        .unwrap_or_default()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn kind_name(kind: &Option<String>) -> Value {
    match kind.as_deref() {
        Some(k) => Value::String(k.to_ascii_lowercase()),
        None => Value::Null,
    }
}

/// What a HAR's `url` needs when nothing is known of the site.
const UNKNOWN_SITE: &str = "https://app.invalid";

fn entry(c: &Captured, site: &str) -> Value {
    let url = redact_path_query(&c.url_path_query);
    let absolute = format!("{site}{}{url}", if url.starts_with('/') { "" } else { "/" });
    let req_mime = header(&c.req_headers, "content-type");
    let mut request = json!({
        "method": c.method,
        "url": absolute,
        "httpVersion": "HTTP/1.1",
        "cookies": [],
        "headers": har_headers(&c.req_headers),
        "queryString": query_string(&url),
        "headersSize": -1,
        "bodySize": match &c.req_body { Some(Body::Text(t)) => t.len() as i64, _ => -1 },
    });
    if let Some(body) = &c.req_body {
        let mut post = json!({ "mimeType": req_mime.unwrap_or("") });
        match body {
            Body::Text(t) => post["text"] = Value::String(redact_body(req_mime, t)),
            other => post["_tcmBody"] = json!(body_tag(other)),
        }
        request["postData"] = post;
    }

    let mime = c.mime.as_deref();
    let mut content = json!({
        "size": match &c.resp_body { Some(Body::Text(t)) => t.len() as i64, _ => 0 },
        "mimeType": mime.unwrap_or(""),
    });
    match &c.resp_body {
        Some(Body::Text(t)) => content["text"] = Value::String(redact_body(mime, t)),
        Some(other) => content["_tcmBody"] = json!(body_tag(other)),
        None => {}
    }
    let response = json!({
        "status": c.status.unwrap_or(0),
        "statusText": "",
        "httpVersion": "HTTP/1.1",
        "cookies": [],
        "headers": har_headers(&c.resp_headers),
        "content": content,
        "redirectURL": c.redirect.as_ref().map(|r| r.to_path.as_str()).unwrap_or(""),
        "headersSize": -1,
        "bodySize": c.resp_size.map(|s| s as i64).unwrap_or(-1),
    });

    let mut tcm = Map::new();
    if c.duration_ms.is_none() {
        tcm.insert("noDuration".into(), json!(true));
    }
    if !c.tab.is_empty() {
        tcm.insert("tab".into(), json!(redact_text(&c.tab)));
    }
    if let Some(f) = &c.failed {
        tcm.insert("failed".into(), json!(redact_text(f)));
    }
    if let Some(r) = &c.redirect {
        tcm.insert(
            "redirect".into(),
            json!({ "status": r.status, "toPath": redact_path_query(&r.to_path), "otherSite": r.other_site }),
        );
    }
    let wait = c.duration_ms.map(|d| d as i64).unwrap_or(0);
    let mut e = json!({
        "startedDateTime": started_iso(c.started_ms),
        "time": c.duration_ms.map(|d| d as i64).unwrap_or(0),
        "request": request,
        "response": response,
        "cache": {},
        "timings": { "send": 0, "wait": wait, "receive": 0 },
        "_resourceType": kind_name(&c.kind),
        "_requestId": redact_text(&c.request_id),
    });
    if !tcm.is_empty() {
        e["_tcm"] = Value::Object(tcm);
    }
    e
}

/// `entries` as a HAR 1.2 document, in the order given, every address, query,
/// header and body redacted (`capture_redact`). `site` is the application's
/// address (any address on it): a HAR's `url` is absolute, so each request's
/// path and query follows its origin.
pub fn to_har(entries: &[&Captured], site: Option<&str>) -> Value {
    let site = site.and_then(origin).unwrap_or_else(|| UNKNOWN_SITE.to_string());
    json!({
        "log": {
            "version": "1.2",
            "creator": { "name": "TCM", "version": env!("CARGO_PKG_VERSION") },
            "entries": entries.iter().map(|c| entry(c, &site)).collect::<Vec<_>>(),
        }
    })
}

/// The application's path and query of a HAR `url`: no scheme, host or
/// fragment.
fn path_query_of(url: &str) -> String {
    let url = url.split('#').next().unwrap_or("");
    let rest = match url.find("://") {
        Some(at) => {
            let after = &url[at + 3..];
            match after.find(['/', '?']) {
                Some(p) if after[p..].starts_with('?') => format!("/{}", &after[p..]),
                Some(p) => after[p..].to_string(),
                None => "/".to_string(),
            }
        }
        None => url.to_string(),
    };
    if rest.starts_with('/') {
        rest
    } else {
        format!("/{rest}")
    }
}

/// HAR's `[{name, value}]` as the object `kept_headers` reads: repeats of a
/// name (in any case) joined by line breaks, which it joins with ", ".
fn header_object(list: &Value) -> Value {
    let mut map: Map<String, Value> = Map::new();
    for h in list.as_array().into_iter().flatten() {
        let (Some(n), Some(v)) = (h["name"].as_str(), h["value"].as_str()) else { continue };
        let key = map.keys().find(|k| k.eq_ignore_ascii_case(n)).cloned().unwrap_or_else(|| n.to_string());
        let joined = match map.get(&key).and_then(Value::as_str) {
            Some(prev) => format!("{prev}\n{v}"),
            None => v.to_string(),
        };
        map.insert(key, Value::String(joined));
    }
    Value::Object(map)
}

fn redacted_headers(headers: Vec<(String, String)>) -> Vec<(String, String)> {
    headers
        .into_iter()
        .map(|(n, v)| {
            let v = redact_header(&n, &v);
            (n, v)
        })
        .collect()
}

/// HAR `params` as form text, every one redacted by its own name whatever
/// the mime says (a multipart or text/plain body is rebuilt the same way).
fn text_of_params(params: &Value) -> Option<String> {
    let list = params.as_array()?;
    Some(
        list.iter()
            .filter_map(|p| {
                let name = p["name"].as_str()?;
                let value = redact_field(name, p["value"].as_str().unwrap_or(""));
                Some(format!("{}={}", urlencoding::encode(name), urlencoding::encode(&value)))
            })
            .collect::<Vec<_>>()
            .join("&"),
    )
}

fn request_body(post: &Value, header_mime: Option<&str>) -> Option<Body> {
    if !post.is_object() {
        return None;
    }
    if let Some(tag) = post["_tcmBody"].as_str().and_then(tagged) {
        return Some(tag);
    }
    let mime = post["mimeType"].as_str().filter(|m| !m.is_empty()).or(header_mime);
    let from_params = post["text"].as_str().is_none();
    let text = post["text"].as_str().map(str::to_string).or_else(|| text_of_params(&post["params"]))?;
    if text.len() > MAX_REQUEST_BODY_BYTES {
        return Some(Body::TooBig);
    }
    Some(Body::Text(redact_body(if from_params { None } else { mime }, &text)))
}

fn response_body(content: &Value, mime: Option<&str>) -> Option<Body> {
    if !content.is_object() {
        return None;
    }
    if let Some(tag) = content["_tcmBody"].as_str().and_then(tagged) {
        return Some(tag);
    }
    let text = content["text"].as_str()?;
    let text = if content["encoding"].as_str() == Some("base64") {
        match base64::engine::general_purpose::STANDARD.decode(text.trim()).ok().and_then(|b| String::from_utf8(b).ok()) {
            Some(t) => t,
            None => return Some(Body::NotText),
        }
    } else {
        text.to_string()
    };
    if mime.is_some_and(|m| !text_like(m)) {
        return Some(Body::NotText);
    }
    if text.len() > MAX_BODY_BYTES {
        return Some(Body::TooBig);
    }
    Some(Body::Text(redact_body(mime, &text)))
}

fn kind_of(name: &str) -> Option<String> {
    match name.to_ascii_lowercase().as_str() {
        "xhr" => Some("XHR".into()),
        "fetch" => Some("Fetch".into()),
        "document" => Some("Document".into()),
        _ => None,
    }
}

fn one(n: usize, e: &Value) -> Result<Captured, String> {
    let bad = |what: &str| format!("entry {n}: {what}");
    let Some(method) = e["request"]["method"].as_str().filter(|m| !m.is_empty()) else {
        return Err(bad("no request method"));
    };
    let Some(url) = e["request"]["url"].as_str() else { return Err(bad("no request url")) };
    let Some(status) = e["response"]["status"].as_u64().filter(|s| *s <= 999) else {
        return Err(bad("no response status"));
    };
    let Some(started) = e["startedDateTime"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()) else {
        return Err(bad("no valid startedDateTime"));
    };

    let req_headers = redacted_headers(kept_headers(&header_object(&e["request"]["headers"])));
    let resp_headers = redacted_headers(kept_headers(&header_object(&e["response"]["headers"])));
    let mime = e["response"]["content"]["mimeType"].as_str().filter(|m| !m.is_empty()).map(str::to_string);
    let req_mime = header(&req_headers, "content-type").map(str::to_string);
    let tcm = &e["_tcm"];
    let redirect = tcm["redirect"].as_object().and_then(|_| {
        Some(Redirect {
            status: u16::try_from(tcm["redirect"]["status"].as_u64()?).ok()?,
            to_path: redact_path_query(tcm["redirect"]["toPath"].as_str()?),
            other_site: tcm["redirect"]["otherSite"].as_bool().unwrap_or(false),
        })
    });
    Ok(Captured {
        id: 0,
        request_id: redact_text(e["_requestId"].as_str().unwrap_or("")),
        tab: redact_text(tcm["tab"].as_str().unwrap_or("")),
        method: method.to_ascii_uppercase(),
        url_path_query: redact_path_query(&path_query_of(url)),
        kind: e["_resourceType"].as_str().and_then(kind_of),
        status: (status != 0).then_some(status as u16),
        resp_body: response_body(&e["response"]["content"], mime.as_deref()),
        mime,
        req_body: request_body(&e["request"]["postData"], req_mime.as_deref()),
        req_headers,
        resp_headers,
        resp_size: e["response"]["bodySize"].as_i64().filter(|s| *s >= 0).map(|s| s as u64),
        started_ms: started.timestamp_millis().max(0) as u64,
        duration_ms: e["time"]
            .as_f64()
            .filter(|t| *t >= 0.0 && tcm["noDuration"] != json!(true))
            .map(|t| t.round() as u64),
        failed: tcm["failed"].as_str().map(redact_text),
        redirect,
    })
}

/// The requests of a HAR document, newest `MAX_CAPTURED` of them at most,
/// numbered from 1 in the order given; every address, query, header and
/// body redacted again. A document that is not a HAR, or has an entry
/// missing what a request needs, is refused with a short sentence.
pub fn from_har(v: &Value) -> Result<Vec<Captured>, String> {
    let log = &v["log"];
    if !log.is_object() {
        return Err("not a HAR file: no log".to_string());
    }
    let Some(entries) = log["entries"].as_array() else { return Err("not a HAR file: no log.entries".to_string()) };
    if let Some(version) = log["version"].as_str() {
        if !version.starts_with("1.") {
            return Err("not a HAR 1.x file".to_string());
        }
    }
    let skip = entries.len().saturating_sub(MAX_CAPTURED);
    let mut out = Vec::with_capacity(entries.len() - skip);
    for (at, e) in entries.iter().enumerate().skip(skip) {
        let mut c = one(at + 1, e)?;
        c.id = (out.len() + 1) as u64;
        out.push(c);
    }
    keep_within_budget(&mut out);
    Ok(out)
}

/// The store's budget for everything kept: while the bodies together are
/// over `MAX_TOTAL_BODY_BYTES`, the oldest request's bodies are dropped
/// (the request stays).
fn keep_within_budget(entries: &mut [Captured]) {
    let size = |b: &Option<Body>| match b {
        Some(Body::Text(t)) => t.len(),
        _ => 0,
    };
    let mut total: usize = entries.iter().map(|c| size(&c.req_body) + size(&c.resp_body)).sum();
    for c in entries.iter_mut() {
        if total <= MAX_TOTAL_BODY_BYTES {
            break;
        }
        total -= size(&c.req_body) + size(&c.resp_body);
        if matches!(c.req_body, Some(Body::Text(_))) {
            c.req_body = Some(Body::Dropped);
        }
        if matches!(c.resp_body, Some(Body::Text(_))) {
            c.resp_body = Some(Body::Dropped);
        }
    }
}
