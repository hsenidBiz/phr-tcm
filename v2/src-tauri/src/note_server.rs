//! Loopback listener for the HTML report's comment boxes. The "View in
//! browser" page runs in the user's default browser, outside the app - its
//! autosave posts here (127.0.0.1 only, ephemeral port), and the payload is
//! handed to a callback that routes it back into the app's local notes
//! store. Same pattern as the OAuth loopback in auth.rs. Accepts only
//! POST /note with a small JSON body; anything else gets 404.

use std::io::{Read, Write};
use std::net::TcpListener;

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct NotePayload {
    pub org: String,
    pub case_id: i32,
    pub text: String,
}

/// Parse the JSON body of a note POST (size-capped by the caller).
pub fn parse_note(body: &str) -> Option<NotePayload> {
    serde_json::from_str(body).ok()
}

/// Bind 127.0.0.1:0 and serve forever on a background thread, invoking
/// `on_note` for each valid note. Returns the bound port.
pub fn start(on_note: impl Fn(NotePayload) + Send + 'static) -> Result<u16, String> {
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
                                    on_note(note);
                                }
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                // The page fetches with mode:'no-cors'; any response works,
                // but be a good citizen. Always close after one request.
                let _ = stream.write_all(
                    b"HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
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
