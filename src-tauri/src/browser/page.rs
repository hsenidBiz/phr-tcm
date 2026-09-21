//! The handful of DevTools calls everything above is built from.
//!
//! An element is held as a `Handle` (a DevTools objectId) in one object
//! group, released before each new attempt so handles never pile up in the
//! page. Functions run ON a handle with their inputs passed as arguments:
//! nothing from a script is ever concatenated into JavaScript source.

use super::cdp::{CdpError, Driver};
use base64::Engine;
use serde_json::{json, Value};

/// Every handle this app creates belongs to this group.
pub const GROUP: &str = "tcm-autorun";

/// A DevTools objectId.
pub type Handle = String;

/// A reply that carries `exceptionDetails` is the page throwing, delivered
/// as a success. It has to be an error here, or a bad selector reads as
/// "nothing matched".
fn thrown(method: &str, reply: &Value) -> Option<CdpError> {
    let ex = reply.get("exceptionDetails")?;
    let message = ex["exception"]["description"]
        .as_str()
        .or_else(|| ex["text"].as_str())
        .unwrap_or("the page threw an error")
        .to_string();
    Some(CdpError::Protocol { method: method.to_string(), message })
}

fn handle_of(method: &str, remote: &Value) -> Result<Handle, CdpError> {
    remote["objectId"].as_str().map(str::to_string).ok_or_else(|| CdpError::Protocol {
        method: method.to_string(),
        message: "the page returned no object".to_string(),
    })
}

fn arguments(values: &[Value]) -> Vec<Value> {
    values.iter().map(|v| json!({ "value": v })).collect()
}

/// Evaluate an expression that this app wrote (never script input) and
/// return its value.
pub async fn eval_value<D: Driver>(d: &mut D, expression: &str) -> Result<Value, CdpError> {
    let r = d
        .call(
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
        )
        .await?;
    if let Some(e) = thrown("Runtime.evaluate", &r) {
        return Err(e);
    }
    Ok(r["result"]["value"].clone())
}

/// A handle on the current document: the root every locator starts from.
pub async fn document<D: Driver>(d: &mut D) -> Result<Handle, CdpError> {
    let r = d
        .call("Runtime.evaluate", json!({ "expression": "document", "objectGroup": GROUP }))
        .await?;
    if let Some(e) = thrown("Runtime.evaluate", &r) {
        return Err(e);
    }
    handle_of("Runtime.evaluate", &r["result"])
}

/// Call `function` with `this` bound to `on`, and return its value.
pub async fn call_value<D: Driver>(
    d: &mut D,
    on: &Handle,
    function: &str,
    values: &[Value],
) -> Result<Value, CdpError> {
    let r = d
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": on,
                "functionDeclaration": function,
                "arguments": arguments(values),
                "returnByValue": true,
                "awaitPromise": true,
            }),
        )
        .await?;
    if let Some(e) = thrown("Runtime.callFunctionOn", &r) {
        return Err(e);
    }
    Ok(r["result"]["value"].clone())
}

/// Call `function` (which must return an array of elements) and return a
/// handle for each, in array order.
pub async fn call_elements<D: Driver>(
    d: &mut D,
    on: &Handle,
    function: &str,
    values: &[Value],
) -> Result<Vec<Handle>, CdpError> {
    let r = d
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": on,
                "functionDeclaration": function,
                "arguments": arguments(values),
                "objectGroup": GROUP,
            }),
        )
        .await?;
    if let Some(e) = thrown("Runtime.callFunctionOn", &r) {
        return Err(e);
    }
    let array = handle_of("Runtime.callFunctionOn", &r["result"])?;
    let props = d
        .call("Runtime.getProperties", json!({ "objectId": array, "ownProperties": true }))
        .await?;
    if let Some(e) = thrown("Runtime.getProperties", &props) {
        return Err(e);
    }
    let mut indexed: Vec<(usize, Handle)> = props["result"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let i = p["name"].as_str()?.parse::<usize>().ok()?;
            let id = p["value"]["objectId"].as_str()?;
            Some((i, id.to_string()))
        })
        .collect();
    indexed.sort_by_key(|(i, _)| *i);
    Ok(indexed.into_iter().map(|(_, h)| h).collect())
}

/// An accessibility node names its element by backend node id; this turns
/// that into a handle functions can be called on.
pub async fn resolve_backend<D: Driver>(d: &mut D, backend_node_id: i64) -> Result<Handle, CdpError> {
    let r = d
        .call("DOM.resolveNode", json!({ "backendNodeId": backend_node_id, "objectGroup": GROUP }))
        .await?;
    handle_of("DOM.resolveNode", &r["object"])
}

/// The reverse of `resolve_backend`: a handle's own backend node id, so
/// handles reached by different paths (two roots that nest) can be told
/// apart from the same element reached twice.
pub async fn backend_id<D: Driver>(d: &mut D, handle: &Handle) -> Result<i64, CdpError> {
    let r = d.call("DOM.describeNode", json!({ "objectId": handle })).await?;
    r["node"]["backendNodeId"].as_i64().ok_or_else(|| CdpError::Protocol {
        method: "DOM.describeNode".to_string(),
        message: "the browser described no node".to_string(),
    })
}

/// Let go of every handle. Housekeeping: a failure here is ignored, never
/// reported as the action's failure.
pub async fn release<D: Driver>(d: &mut D) {
    let _ = d.call("Runtime.releaseObjectGroup", json!({ "objectGroup": GROUP })).await;
}

/// What the person would see right now, as JPEG bytes.
pub async fn screenshot<D: Driver>(d: &mut D) -> Result<Vec<u8>, CdpError> {
    let method = "Page.captureScreenshot";
    let r = d.call(method, json!({ "format": "jpeg", "quality": 60 })).await?;
    let data = r["data"].as_str().ok_or_else(|| CdpError::Protocol {
        method: method.to_string(),
        message: "the browser returned no image".to_string(),
    })?;
    base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| CdpError::Protocol {
        method: method.to_string(),
        message: format!("the image could not be decoded: {e}"),
    })
}
