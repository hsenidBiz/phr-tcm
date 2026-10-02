//! What the page was doing, for when a run cannot say why it got stuck.
//!
//! An unattended case that never reached its module used to leave only
//! "text "New Cycle" not found" and a picture of a spinner (PeoplesHR,
//! 2026-10-02): nothing said which request the page was still waiting on,
//! which one had failed, or what it wrote to its console. A run's browser
//! now switches on the Network and Runtime domains (`watch`), and the CDP
//! client hands their events here instead of to its event buffer, so
//! thousands of requests can never push a load event out of it.
//!
//! Only what helps explain a failure is kept: requests that failed,
//! answered 400 or worse, or have not finished, and console errors and
//! warnings. Every address loses its query string - a token travels there
//! (`/hr/pmsv10/updatehub?token=...`) - and everything is bounded.

use super::cdp::{CdpError, Driver, Event};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// Requests remembered at once; the oldest goes first.
const MAX_REQUESTS: usize = 400;
/// Console errors and warnings remembered; the oldest goes first.
const MAX_CONSOLE: usize = 30;
/// Lines a report gives at most, then how many more there were.
const MAX_LINES: usize = 25;
/// Characters of one console message kept.
const MAX_TEXT: usize = 300;

struct Request {
    method: String,
    url: String,
    sent: Instant,
    status: Option<u64>,
    /// `None` while waiting; `Some(None)` finished; `Some(Some(why))` failed.
    ended: Option<Option<String>>,
}

#[derive(Default)]
pub struct PageLog {
    requests: HashMap<String, Request>,
    order: VecDeque<String>,
    console: VecDeque<String>,
}

impl PageLog {
    /// Take the event if it belongs here. `false` leaves it to the caller's
    /// own buffer - everything outside these two domains' log events,
    /// including `Runtime.bindingCalled`, which the recorders wait for.
    pub fn observe(&mut self, ev: &Event) -> bool {
        let p = &ev.params;
        match ev.method.as_str() {
            "Network.requestWillBeSent" => {
                let url = p["request"]["url"].as_str().unwrap_or("");
                if url.starts_with("http") {
                    let id = p["requestId"].as_str().unwrap_or("").to_string();
                    let method = p["request"]["method"].as_str().unwrap_or("GET").to_string();
                    // A redirect arrives under the same id: the request
                    // goes on, now to the new address.
                    if !self.requests.contains_key(&id) {
                        self.order.push_back(id.clone());
                    }
                    self.requests.insert(
                        id,
                        Request { method, url: without_query(url), sent: Instant::now(), status: None, ended: None },
                    );
                    while self.order.len() > MAX_REQUESTS {
                        if let Some(old) = self.order.pop_front() {
                            self.requests.remove(&old);
                        }
                    }
                }
                true
            }
            "Network.responseReceived" => {
                if let Some(r) = self.request(p) {
                    r.status = p["response"]["status"].as_u64();
                }
                true
            }
            "Network.loadingFinished" => {
                if let Some(r) = self.request(p) {
                    r.ended = Some(None);
                }
                true
            }
            "Network.loadingFailed" => {
                let id = p["requestId"].as_str().unwrap_or("");
                // A request the page itself called off (it navigated, or
                // aborted a superseded search) is not a failure.
                if p["canceled"].as_bool() == Some(true) {
                    self.requests.remove(id);
                    self.order.retain(|o| o != id);
                } else if let Some(r) = self.request(p) {
                    r.ended = Some(Some(p["errorText"].as_str().unwrap_or("failed").to_string()));
                }
                true
            }
            m if m.starts_with("Network.") => true,
            "Runtime.consoleAPICalled" => {
                let kind = p["type"].as_str().unwrap_or("");
                if matches!(kind, "error" | "warning" | "assert") {
                    let text = p["args"].as_array().map(|a| a.iter().map(arg_text).collect::<Vec<_>>().join(" "));
                    self.note(format!("console {kind}: {}", clip(&text.unwrap_or_default())));
                }
                true
            }
            "Runtime.exceptionThrown" => {
                let d = &p["exceptionDetails"];
                let text = d["exception"]["description"].as_str().or(d["text"].as_str()).unwrap_or("an error");
                self.note(format!("uncaught error: {}", clip(text)));
                true
            }
            _ => false,
        }
    }

    fn request(&mut self, p: &Value) -> Option<&mut Request> {
        self.requests.get_mut(p["requestId"].as_str().unwrap_or(""))
    }

    fn note(&mut self, line: String) {
        if self.console.len() >= MAX_CONSOLE {
            self.console.pop_front();
        }
        self.console.push_back(line);
    }

    /// One line per thing worth knowing, oldest first: requests that failed,
    /// were answered 400 or worse, or are still waiting, then console errors
    /// and warnings. Empty when the page did nothing worth reporting.
    pub fn report(&self) -> Vec<String> {
        let now = Instant::now();
        let mut lines: Vec<String> = vec![];
        for r in self.order.iter().filter_map(|id| self.requests.get(id)) {
            let what = format!("{} {}", r.method, r.url);
            match (&r.ended, r.status) {
                (Some(Some(why)), _) => lines.push(format!("request failed ({why}): {what}")),
                (_, Some(s)) if s >= 400 => lines.push(format!("request answered {s}: {what}")),
                (None, _) => lines.push(format!(
                    "request still waiting after {}s: {what}",
                    now.saturating_duration_since(r.sent).as_secs()
                )),
                _ => {}
            }
        }
        lines.extend(self.console.iter().cloned());
        if lines.len() > MAX_LINES {
            let more = lines.len() - MAX_LINES;
            lines.truncate(MAX_LINES);
            lines.push(format!("...and {more} more"));
        }
        lines
    }
}

/// Switch on the events `PageLog` reads, for a browser a run drives. Best
/// effort for the caller: a page that refuses only loses the report.
pub async fn watch<D: Driver>(d: &mut D) -> Result<(), CdpError> {
    d.call("Network.enable", json!({})).await?;
    d.call("Runtime.enable", json!({})).await?;
    Ok(())
}

/// The address without its query string or fragment.
pub fn without_query(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or("").to_string()
}

/// A console argument as words: a string's own text, or how the console
/// would describe anything else.
fn arg_text(a: &Value) -> String {
    match &a["value"] {
        Value::String(s) => s.clone(),
        Value::Null => a["description"].as_str().unwrap_or("").to_string(),
        v => v.to_string(),
    }
}

/// At most `MAX_TEXT` characters, on one line, with any address in it
/// cut back to its path.
fn clip(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let scrubbed = one_line
        .split(' ')
        .map(|w| if w.contains("://") { without_query(w) } else { w.to_string() })
        .collect::<Vec<_>>()
        .join(" ");
    if scrubbed.chars().count() > MAX_TEXT {
        format!("{}...", scrubbed.chars().take(MAX_TEXT).collect::<String>())
    } else {
        scrubbed
    }
}
