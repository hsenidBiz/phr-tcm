//! What a page says back after an action: the toasts, alerts and field
//! errors it shows now that it did not show before. A refusal the page
//! gives (a rule that stops a save, a field it will not accept) is only
//! ever a message on the page, so discovery reads the page before and after
//! an action and reports what is new (`new_messages`).
//!
//! The read is `page_messages.js`, one expression evaluated in the page and
//! in every same-origin frame of it. It reads text the page itself shows
//! and the page's path, and nothing else: no host, query or fragment, no
//! request, no value typed into a field.

use super::cdp::Driver;
use super::page;

/// The most new messages one action reports.
pub const MAX_MESSAGES: usize = 5;
/// The longest a reported message is, in characters.
pub const MAX_MESSAGE_CHARS: usize = 300;

/// The page's read: an expression that evaluates to `{ messages, step, path }`.
pub const READ_JS: &str = include_str!("page_messages.js");

/// One message the page is showing, and where: `"page"`, or
/// `"frame <title, name or id>"` for one inside a frame.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PageMessage {
    pub text: String,
    #[serde(rename = "where")]
    pub place: String,
}

/// Everything one read of the page found.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PageRead {
    pub messages: Vec<PageMessage>,
    /// The active item of the page's step indicator, when it has one.
    pub step: Option<String>,
    /// The page's path (`location.pathname`): no host, query or fragment.
    #[serde(default)]
    pub path: Option<String>,
}

/// Read what the page shows now. A page that cannot be read (closed,
/// navigating, an answer of the wrong shape) reads as showing nothing: the
/// read is a help to the action it follows, never a reason for it to fail.
pub async fn read_page_messages<D: Driver>(d: &mut D) -> PageRead {
    match page::eval_value(d, READ_JS).await {
        Ok(v) => serde_json::from_value(v).unwrap_or_default(),
        Err(_) => PageRead::default(),
    }
}

/// A message with its runs of white space made single spaces, so a line
/// break or an indent the page changes is not a different message.
fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The messages in `after` that `before` did not already have, at most
/// `MAX_MESSAGES` of them, each cut to `MAX_MESSAGE_CHARS` characters.
/// Messages are matched by their collapsed text and counted: two "Save
/// failed" after one before is one new "Save failed", so a refusal the page
/// gives again is not lost behind the one it gave first.
pub fn new_messages(before: &[PageMessage], after: &[PageMessage]) -> Vec<PageMessage> {
    let mut had: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for m in before {
        *had.entry(collapsed(&m.text)).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    for m in after {
        let text = collapsed(&m.text);
        if text.is_empty() {
            continue;
        }
        if let Some(n) = had.get_mut(&text) {
            if *n > 0 {
                *n -= 1;
                continue;
            }
        }
        out.push(PageMessage { text: text.chars().take(MAX_MESSAGE_CHARS).collect(), place: m.place.clone() });
        if out.len() == MAX_MESSAGES {
            break;
        }
    }
    out
}
