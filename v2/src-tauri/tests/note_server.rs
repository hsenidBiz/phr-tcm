use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use v2_lib::note_server::{parse_note, start, NotePayload};

fn post(port: u16, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    resp
}

#[test]
fn loopback_listener_delivers_posted_notes() {
    let (tx, rx) = mpsc::channel();
    let port = start(move |n| tx.send(n).unwrap()).unwrap();

    let resp = post(port, "/note", r#"{"org":"acme","case_id":42,"text":"fix step 3"}"#);
    assert!(resp.starts_with("HTTP/1.1 204"));
    let note = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(
        note,
        NotePayload { org: "acme".into(), case_id: 42, text: "fix step 3".into() }
    );

    // Wrong path or junk body: responds politely, never delivers a note.
    post(port, "/other", r#"{"org":"acme","case_id":1,"text":"x"}"#);
    post(port, "/note", "not json");
    assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err());
}

#[test]
fn parse_note_rejects_malformed_payloads() {
    assert!(parse_note(r#"{"org":"a","case_id":1,"text":"t"}"#).is_some());
    assert!(parse_note(r#"{"case_id":1,"text":"t"}"#).is_none());
    assert!(parse_note("").is_none());
}
