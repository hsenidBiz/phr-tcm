//! Page errors as a check: the uncaught script errors and the 5xx answers
//! every tab of a run met, kept for the runner to judge once per step
//! (`autorun::runner`, a script's `page_errors`).
//!
//! The driver hands every event it reads here (`observe`), from any tab,
//! before the page log takes it. What is kept is said the way the step will
//! say it: a script error's message cut to `cut`'s length with no address
//! beyond its path, and a request as its method and path - never a host or
//! a query, where a token travels.
//!
//! The run's own requests are not the page's errors. The runner names the
//! ones it sent (`exclude`) - an `api_request`'s fetch, found by its path in
//! the network record - and they are passed over when the errors are taken.

use super::cdp::Event;
use super::dialogs::cut;
use std::collections::{HashMap, HashSet, VecDeque};

/// The most errors kept between two takes.
const MAX_SEEN: usize = 100;
/// The most request methods remembered, for the answer that follows.
const MAX_METHODS: usize = 500;

/// One error the page had.
#[derive(Debug, Clone, PartialEq)]
pub enum PageError {
    /// An uncaught script error (`Runtime.exceptionThrown`).
    Script { message: String },
    /// A request answered 500 to 599.
    Response { id: String, status: u16, method: String, path: String },
}

impl PageError {
    /// How a step says it: `the page had an error: <message>` or
    /// `a request was answered <status>: <method> <path>`.
    pub fn sentence(&self) -> String {
        match self {
            PageError::Script { message } => format!("the page had an error: {message}"),
            PageError::Response { status, method, path, .. } => {
                format!("a request was answered {status}: {method} {path}")
            }
        }
    }

    /// The text an ignore phrase is looked for in: a script error's message,
    /// or a request's path.
    fn matched_text(&self) -> &str {
        match self {
            PageError::Script { message } => message,
            PageError::Response { path, .. } => path,
        }
    }
}

/// The run's page errors since the last take, across every tab.
#[derive(Debug, Default)]
pub struct PageErrorBook {
    methods: HashMap<String, String>,
    order: VecDeque<String>,
    seen: Vec<PageError>,
    mine: HashSet<String>,
}

impl PageErrorBook {
    /// Read one event, from any tab.
    pub fn observe(&mut self, ev: &Event) {
        let p = &ev.params;
        match ev.method.as_str() {
            "Network.requestWillBeSent" => {
                let id = p["requestId"].as_str().unwrap_or("").to_string();
                let method = p["request"]["method"].as_str().unwrap_or("GET").to_string();
                if !self.methods.contains_key(&id) {
                    self.order.push_back(id.clone());
                }
                self.methods.insert(id, method);
                while self.order.len() > MAX_METHODS {
                    if let Some(old) = self.order.pop_front() {
                        self.methods.remove(&old);
                    }
                }
            }
            "Network.responseReceived" => {
                let status = p["response"]["status"].as_u64().unwrap_or(0);
                let url = p["response"]["url"].as_str().unwrap_or("");
                if (500..=599).contains(&status) && url.starts_with("http") {
                    let id = p["requestId"].as_str().unwrap_or("").to_string();
                    let method = self.methods.get(&id).cloned().unwrap_or_else(|| "GET".to_string());
                    self.push(PageError::Response { id, status: status as u16, method, path: cut(&path_of(url)) });
                }
            }
            "Runtime.exceptionThrown" => {
                let d = &p["exceptionDetails"];
                let text = d["exception"]["description"].as_str().or(d["text"].as_str()).unwrap_or("an error");
                // A stack trace's first line is the message.
                let first = text.lines().next().unwrap_or("");
                self.push(PageError::Script { message: cut(&scrubbed(first)) });
            }
            _ => {}
        }
    }

    fn push(&mut self, e: PageError) {
        if self.seen.len() >= MAX_SEEN {
            self.seen.remove(0);
        }
        self.seen.push(e);
    }

    /// These requests are the run's own: never counted.
    pub fn exclude(&mut self, ids: impl IntoIterator<Item = String>) {
        self.mine.extend(ids);
    }

    /// How many are held; for a caller that drops what came after
    /// (`drop_after`).
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// Forget every error after the first `n` - what the run's own sign-in
    /// met, which is not the page's doing.
    pub fn drop_after(&mut self, n: usize) {
        self.seen.truncate(n);
    }

    /// Every error since the last take, the run's own requests left out, and
    /// forget them.
    pub fn take(&mut self) -> Vec<PageError> {
        let mine = std::mem::take(&mut self.mine);
        std::mem::take(&mut self.seen)
            .into_iter()
            .filter(|e| !matches!(e, PageError::Response { id, .. } if mine.contains(id)))
            .collect()
    }
}

/// The errors an ignore phrase does not cover: a phrase found, ignoring
/// case, in a script error's message or a request's path.
pub fn counted(errors: Vec<PageError>, ignore: &[String]) -> Vec<PageError> {
    let phrases: Vec<String> = ignore.iter().map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
    errors
        .into_iter()
        .filter(|e| {
            let text = e.matched_text().to_lowercase();
            !phrases.iter().any(|p| text.contains(p.as_str()))
        })
        .collect()
}

/// The first error's sentence, and `(and <n> more)` for the rest.
pub fn summary(errors: &[PageError]) -> Option<String> {
    let first = errors.first()?.sentence();
    Some(match errors.len() {
        1 => first,
        n => format!("{first} (and {} more)", n - 1),
    })
}

/// What a step's log says of its page errors, after its own words.
pub const NOTE: &str = " (page errors: ";

/// The note listing every error: ` (page errors: <one>; <two>)`.
pub fn note(errors: &[PageError]) -> String {
    let all: Vec<String> = errors.iter().map(PageError::sentence).collect();
    format!("{NOTE}{})", all.join("; "))
}

/// An address's path: no scheme, host, query or fragment.
fn path_of(url: &str) -> String {
    let no_query = url.split(['?', '#']).next().unwrap_or("");
    match no_query.find("://") {
        Some(i) => {
            let rest = &no_query[i + 3..];
            rest.find('/').map_or_else(|| "/".to_string(), |j| rest[j..].to_string())
        }
        None => no_query.to_string(),
    }
}

/// A message with every address in it cut to its path.
fn scrubbed(text: &str) -> String {
    text.split_whitespace()
        .map(|w| if w.contains("://") { path_of(w) } else { w.to_string() })
        .collect::<Vec<_>>()
        .join(" ")
}
