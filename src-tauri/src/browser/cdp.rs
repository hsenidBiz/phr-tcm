//! A minimal Chrome DevTools Protocol client: connect to the page's
//! socket and evaluate JavaScript in it. Evaluation alone covers finding
//! elements, clicking, typing, reading text and highlighting, which is
//! everything the supervised runner does.

use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

/// One request on the wire.
pub fn frame(id: u64, method: &str, params: serde_json::Value) -> String {
    serde_json::json!({ "id": id, "method": method, "params": params }).to_string()
}

/// Is this frame the answer to `id`? `None` means "not ours" - an event,
/// another request's reply, or something unparseable. `Some(Err(..))` is
/// a real protocol error and must never be flattened into an empty
/// success: in a test runner an empty result reads as "found nothing".
pub fn reply_for(id: u64, raw: &str) -> Option<Result<serde_json::Value, String>> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("id")?.as_u64()? != id {
        return None;
    }
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown DevTools error");
        return Some(Err(msg.to_string()));
    }
    Some(Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null)))
}

type Socket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

pub struct Cdp {
    socket: Socket,
    next_id: u64,
}

impl Cdp {
    /// Ask the browser which socket its page is on, then open it.
    pub async fn connect(port: u16) -> Result<Cdp, String> {
        let url = format!("http://127.0.0.1:{port}/json/list");
        let body = reqwest::get(&url)
            .await
            .map_err(|e| format!("the browser did not answer on port {port}: {e}"))?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let tabs: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let ws = tabs
            .as_array()
            .and_then(|a| a.iter().find(|t| t["type"] == "page"))
            .and_then(|t| t["webSocketDebuggerUrl"].as_str())
            .ok_or_else(|| "the browser reported no page to drive".to_string())?
            .to_string();
        let (socket, _) = tokio_tungstenite::connect_async(&ws)
            .await
            .map_err(|e| format!("could not open the DevTools socket: {e}"))?;
        Ok(Cdp { socket, next_id: 1 })
    }

    /// Run an expression in the page and return its DevTools result.
    /// `awaitPromise` so an async expression resolves before we answer.
    pub async fn eval(&mut self, expression: &str) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let params = serde_json::json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true,
        });
        self.socket
            .send(Message::Text(frame(id, "Runtime.evaluate", params)))
            .await
            .map_err(|e| format!("the DevTools socket closed: {e}"))?;

        // Skip past the events the browser volunteers until our own
        // reply arrives.
        while let Some(msg) = self.socket.next().await {
            let msg = msg.map_err(|e| format!("the DevTools socket failed: {e}"))?;
            let Message::Text(raw) = msg else { continue };
            if let Some(answer) = reply_for(id, &raw) {
                return answer;
            }
        }
        Err("the browser closed before answering".to_string())
    }
}
