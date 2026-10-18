//! The page read behind a refusal: what a page newly shows after an action.
//! The read itself is JavaScript, tested in `src/lib/pageMessages.test.ts`;
//! here are the comparison and the page that cannot be read.

use crate::common::ScriptedDriver;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::page_messages::{
    new_messages, read_page_messages, PageMessage, PageRead, MAX_MESSAGES, MAX_MESSAGE_CHARS,
};

fn msg(text: &str) -> PageMessage {
    PageMessage { text: text.to_string(), place: "page".to_string() }
}

#[test]
fn a_message_on_the_page_before_is_not_new() {
    let before = [msg("Welcome back")];
    let after = [msg("Welcome back"), msg("Saved.")];
    assert_eq!(new_messages(&before, &after), vec![msg("Saved.")]);
}

#[test]
fn whitespace_does_not_make_a_message_new() {
    let before = [msg("Saved.\n  ")];
    let after = [msg("Saved.")];
    assert!(new_messages(&before, &after).is_empty());
}

#[test]
fn at_most_five_messages_of_at_most_300_chars() {
    let long = "x".repeat(400);
    let after: Vec<PageMessage> = std::iter::once(msg(&long))
        .chain((1..7).map(|i| msg(&format!("Problem {i}"))))
        .collect();
    let got = new_messages(&[], &after);
    assert_eq!(got.len(), MAX_MESSAGES);
    assert_eq!(got.iter().map(|m| m.text.chars().count()).max(), Some(MAX_MESSAGE_CHARS));
}

#[test]
fn a_refusal_given_again_is_new() {
    let before = [msg("Save failed")];
    let after = [msg("Save failed"), msg("Save failed")];
    assert_eq!(new_messages(&before, &after), vec![msg("Save failed")]);
}

#[test]
fn a_message_shown_twice_is_two_new_messages() {
    let after = [msg("Save failed"), msg("Save failed")];
    assert_eq!(new_messages(&[], &after), vec![msg("Save failed"), msg("Save failed")]);
}

#[tokio::test]
async fn an_unreadable_page_reads_as_no_messages() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    assert_eq!(read_page_messages(&mut d).await, PageRead { messages: vec![], step: None });
}

#[tokio::test]
async fn the_pages_answer_is_read_as_messages_and_a_step() {
    let mut d = ScriptedDriver::new(|_, _| {
        Ok(serde_json::json!({ "result": { "value": {
            "messages": [{ "text": "Name is required", "where": "frame Eval Rules" }],
            "step": "Assessment Components"
        } } }))
    });
    let got = read_page_messages(&mut d).await;
    assert_eq!(
        got,
        PageRead {
            messages: vec![PageMessage { text: "Name is required".into(), place: "frame Eval Rules".into() }],
            step: Some("Assessment Components".into()),
        }
    );
}

#[tokio::test]
async fn an_answer_that_is_not_a_read_is_no_messages() {
    let mut d = ScriptedDriver::new(|_, _| Ok(serde_json::json!({ "result": { "value": "nonsense" } })));
    assert_eq!(read_page_messages(&mut d).await, PageRead::default());
}
