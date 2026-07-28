//! Loopback listener for the HTML report's comment boxes. The "View in
//! browser" page runs in the user's default browser, outside the app - its
//! autosave posts here (127.0.0.1 only, ephemeral port), and the payload is
//! handed to a callback that routes it to wherever that kind of comment
//! belongs. Same pattern as the OAuth loopback in auth.rs. Accepts only
//! POST /note with a small JSON body; anything else gets 404.
//!
//! The reply carries the real outcome. A draft comment can genuinely fail
//! to save - the file moved, an assistant rewrote it, the case is no longer
//! in it - and a page that says "Saved" regardless is worse than no
//! indicator at all. That is why the response is readable CORS rather than
//! the opaque `no-cors` this started as: the page is `file://`, so its
//! origin is `null`, and a text/plain POST is a simple request that needs
//! no preflight.

use std::io::{Read, Write};
use std::net::TcpListener;

/// What kind of comment a POST carries. Flat rather than a tagged enum so
/// the field defaults keep older generated pages (which sent no `kind`)
/// working against a newer app.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct NotePayload {
    /// "ado" (default) | "case" | "general".
    #[serde(default = "ado")]
    pub kind: String,
    /// ADO notes only: the org their work item ids are scoped to.
    #[serde(default)]
    pub org: String,
    /// ADO notes only.
    #[serde(default)]
    pub case_id: i32,
    /// Draft comments: the JSON file to write into.
    #[serde(default)]
    pub path: String,
    /// Draft case comments: the work item id, when the case has one.
    #[serde(default)]
    pub id: Option<i32>,
    /// Draft case comments: the title, which identifies an id-less case.
    #[serde(default)]
    pub title: String,
    pub text: String,
}

fn ado() -> String {
    "ado".into()
}

/// Parse the JSON body of a note POST (size-capped by the caller).
pub fn parse_note(body: &str) -> Option<NotePayload> {
    serde_json::from_str(body).ok()
}

/// The reply body for a save, as the page reads it.
pub fn reply_body(outcome: &Result<(), String>) -> String {
    match outcome {
        Ok(()) => "{\"ok\":true}".into(),
        Err(e) => serde_json::json!({ "ok": false, "error": e }).to_string(),
    }
}

/// Bind 127.0.0.1:0 and serve forever on a background thread, invoking
/// `on_note` for each valid note and reporting its result back to the page.
/// Returns the bound port.
pub fn start(
    on_note: impl Fn(NotePayload) -> Result<(), String> + Send + 'static,
) -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    std::thread::Builder::new()
        .name("note-server".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // Small requests only: read up to 64KB, parse head + body.
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let mut outcome = Err("the app did not understand that request".to_string());
                loop {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if buf.len() > 64 * 1024 {
                                break;
                            }
                            if let Some(body) = body_if_complete(&buf) {
                                if let Some(note) = request_note(&buf, &body) {
                                    outcome = on_note(note);
                                }
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let body = reply_body(&outcome);
                // Always close after one request.
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                         Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                         Connection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(port)
}

/// Once the whole body (per Content-Length) has arrived, return it.
fn body_if_complete(buf: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(buf);
    let (head, body) = text.split_once("\r\n\r\n")?;
    let len: usize = head
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(str::trim).map(String::from))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    (body.len() >= len).then(|| body[..len].to_string())
}

/// The note, if this is a POST /note request with a valid body.
fn request_note(buf: &[u8], body: &str) -> Option<NotePayload> {
    let text = String::from_utf8_lossy(buf);
    let first = text.lines().next()?;
    (first.starts_with("POST /note ")).then(|| parse_note(body)).flatten()
}
