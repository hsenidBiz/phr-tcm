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
    /// The secret from the page that was generated for this app run. A
    /// request without it is not from a page this app wrote.
    #[serde(default)]
    pub token: String,
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
    token: String,
    on_note: impl Fn(NotePayload) -> Result<(), String> + Send + Sync + 'static,
) -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let on_note = std::sync::Arc::new(on_note);
    let token = std::sync::Arc::new(token);
    // Bounded so a flood cannot spawn threads without end. Loopback and
    // token-guarded, so this only has to be larger than any honest burst.
    let live = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    const MAX_LIVE: usize = 32;
    std::thread::Builder::new()
        .name("note-server".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // Every connection used to be read to completion ON THIS
                // THREAD, with no timeout. One peer that connected and then
                // said nothing - and this port is on loopback with
                // Access-Control-Allow-Origin: *, so any page the user
                // visits can reach it - blocked the accept loop for good,
                // and every comment save after it hung.
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));
                if live.load(std::sync::atomic::Ordering::SeqCst) >= MAX_LIVE {
                    continue; // drop it; the page retries on the next keystroke
                }
                let (on_note, token, slot) = (on_note.clone(), token.clone(), live.clone());
                live.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let spawned = std::thread::Builder::new().name("note-conn".into()).spawn(move || {
                    let _guard = LiveGuard(slot);
                // Small requests only: read up to 64KB, parse head + body.
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let mut outcome = Err("the app did not understand that request".to_string());
                // Answered instead of `outcome` when this is a version poll.
                let mut version: Option<String> = None;
                loop {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if buf.len() > 64 * 1024 {
                                break;
                            }
                            if let Some(body) = body_if_complete(&buf) {
                                if let Some(asked) = request_version_token(&buf) {
                                    version = Some(if asked == *token {
                                        format!("{{\"revision\":{}}}", revision())
                                    } else {
                                        // Same shape, no number: a page without
                                        // the secret learns nothing and still parses.
                                        "{\"revision\":null}".to_string()
                                    });
                                    break;
                                }
                                if let Some(note) = request_note(&buf, &body) {
                                    // The listener is on loopback with
                                    // Access-Control-Allow-Origin: *, so any
                                    // page the user visits can reach it if it
                                    // guesses the port - and these notes now
                                    // write to files named in the request.
                                    // The secret is only in the page this app
                                    // generated, so a request without it did
                                    // not come from one.
                                    outcome = if note.token == *token {
                                        on_note(note)
                                    } else {
                                        Err("this page is out of date - reopen it from the app".into())
                                    };
                                }
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let body = version.unwrap_or_else(|| reply_body(&outcome));
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
                });
                if spawned.is_err() {
                    live.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(port)
}

/// Releases a connection slot however the handler ends.
struct LiveGuard(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Once the whole body (per Content-Length) has arrived, return it.
/// How many times the report has been re-exported this run.
///
/// The browser page is a file on disk: once it is open, nothing tells
/// it that the queue behind it moved on. It polls this instead, and
/// offers a refresh when the number it was rendered at stops matching.
/// A counter rather than a content hash, because the only question is
/// "is what you are looking at still current" - and a counter cannot
/// collide.
static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Called whenever the report file is rewritten.
pub fn bump_revision() {
    REVISION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

pub fn revision() -> u64 {
    REVISION.load(std::sync::atomic::Ordering::SeqCst)
}

/// The token from a `GET /version?token=...`, if that is what this is.
///
/// Token-checked like the note route, and for the same reason: this port
/// is on loopback with `Access-Control-Allow-Origin: *`, so any page the
/// user visits can reach it if it guesses the port. There is little to
/// learn from a counter, but "only pages this app generated get answers"
/// is a cheaper rule to keep than a list of exceptions to it.
pub fn request_version_token(buf: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(buf);
    let first = text.lines().next()?;
    let rest = first.strip_prefix("GET /version")?;
    let query = rest.split_whitespace().next().unwrap_or("");
    Some(query.strip_prefix("?token=")?.to_string())
}

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
