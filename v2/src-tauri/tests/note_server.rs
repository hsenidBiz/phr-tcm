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
    let port = start("secret".into(), move |n| {
        tx.send(n).unwrap();
        Ok(())
    })
    .unwrap();

    let resp = post(port, "/note", r#"{"token":"secret","org":"acme","case_id":42,"text":"fix step 3"}"#);
    assert!(resp.starts_with("HTTP/1.1 200"));
    assert!(resp.contains("\"ok\":true"));
    let note = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(
        note,
        NotePayload {
            token: "secret".into(),
            kind: "ado".into(),
            org: "acme".into(),
            case_id: 42,
            path: String::new(),
            id: None,
            title: String::new(),
            text: "fix step 3".into(),
        }
    );

    // Wrong path or junk body: responds politely, never delivers a note.
    post(port, "/other", r#"{"token":"secret","org":"acme","case_id":1,"text":"x"}"#);
    post(port, "/note", "not json");
    assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err());
}

/// The payload got LOOSER when draft comments arrived, and deliberately:
/// `text` is the only field every kind of comment has. A draft case has no
/// org and no work item id; a whole-set comment has neither of those nor a
/// title. So "missing org" stopped being malformed - which is what this
/// test used to assert.
#[test]
fn only_the_text_is_required_of_a_note() {
    assert!(parse_note(r#"{"org":"a","case_id":1,"text":"t"}"#).is_some());
    assert!(parse_note(r#"{"kind":"general","path":"C:/w/a.json","text":"t"}"#).is_some());
    // No text at all is still malformed, as is anything that isn't JSON.
    assert!(parse_note(r#"{"org":"a","case_id":1}"#).is_none());
    assert!(parse_note("").is_none());
    assert!(parse_note("not json").is_none());
}

/// Pages generated before draft comments existed post no `kind` at all.
/// They must keep working: the app they were generated from is the app
/// they post back to, and it may well have updated underneath them.
#[test]
fn a_note_with_no_kind_is_still_an_azure_devops_note() {
    let note = parse_note(r#"{"org":"acme","case_id":42,"text":"t"}"#).unwrap();
    assert_eq!(note.kind, "ado");
    assert_eq!(note.org, "acme");
    assert_eq!(note.case_id, 42);
}

/// The listener is on loopback with `Access-Control-Allow-Origin: *`, so any
/// page the user visits can reach it if it guesses the port - and a note
/// names the file to write. The secret only exists in the page this app
/// generated, so a request without it is not one of ours.
#[test]
fn a_note_without_the_secret_is_refused() {
    let (tx, rx) = mpsc::channel();
    let port = start("the-real-secret".into(), move |n| {
        tx.send(n).unwrap();
        Ok(())
    })
    .unwrap();

    for body in [
        r#"{"kind":"general","path":"C:/w/a.json","text":"x"}"#,
        r#"{"token":"","kind":"general","path":"C:/w/a.json","text":"x"}"#,
        r#"{"token":"guessed","kind":"general","path":"C:/w/a.json","text":"x"}"#,
    ] {
        let resp = post(port, "/note", body);
        assert!(resp.contains("\"ok\":false"), "accepted a note without the secret: {resp}");
    }
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(300)).is_err(),
        "a refused note must never reach the handler"
    );

    // The real page still works.
    let ok = post(
        port,
        "/note",
        r#"{"token":"the-real-secret","kind":"general","path":"C:/w/a.json","text":"x"}"#,
    );
    assert!(ok.contains("\"ok\":true"), "the app's own page must still save: {ok}");
    assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok());
}
