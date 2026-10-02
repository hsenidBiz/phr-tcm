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
//! answered 400 or worse, took `SLOW_SECS` or more, or have not finished
//! (each with how many redirects it went through), and console errors and
//! warnings with the script and line they came from. Every address loses
//! its query string - a token travels there (`/hr/pmsv10/updatehub?token=...`)
//! - and everything is bounded.

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
/// A request that finished but took this long is reported too: a page that
/// waited 14 s for one answer and then 2 s for the next explains a spinner
/// as much as one that never answered.
pub const SLOW_SECS: f64 = 5.0;

struct Request {
    method: String,
    url: String,
    /// When the first hop was read, for a request that has not finished.
    sent: Instant,
    /// The browser's own clock (`timestamp`, seconds) for the first hop, so a
    /// finished request's time is measured where it happened, not when its
    /// events happened to be read.
    started: Option<f64>,
    /// How long it took, by the browser's clock, once it finished or failed.
    took: Option<f64>,
    redirects: u32,
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
                    match self.requests.get_mut(&id) {
                        // A redirect arrives under the same id: the request
                        // goes on, now to the new address, and its time
                        // still counts from the first hop.
                        Some(r) => {
                            r.redirects += 1;
                            r.method = method;
                            r.url = without_query(url);
                            r.status = None;
                        }
                        None => {
                            self.order.push_back(id.clone());
                            self.requests.insert(
                                id,
                                Request {
                                    method,
                                    url: without_query(url),
                                    sent: Instant::now(),
                                    started: p["timestamp"].as_f64(),
                                    took: None,
                                    redirects: 0,
                                    status: None,
                                    ended: None,
                                },
                            );
                        }
                    }
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
                    r.took = elapsed(r.started, p);
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
                    r.took = elapsed(r.started, p);
                }
                true
            }
            m if m.starts_with("Network.") => true,
            "Runtime.consoleAPICalled" => {
                let kind = p["type"].as_str().unwrap_or("");
                if matches!(kind, "error" | "warning" | "assert") {
                    let text = p["args"].as_array().map(|a| a.iter().map(arg_text).collect::<Vec<_>>().join(" "));
                    let place = top_frame(&p["stackTrace"]).map(|(url, line, col)| at(&url, line, col)).unwrap_or_default();
                    self.note(format!("console {kind}: {}{place}", clip(&text.unwrap_or_default())));
                }
                true
            }
            "Runtime.exceptionThrown" => {
                let d = &p["exceptionDetails"];
                let text = d["exception"]["description"].as_str().or(d["text"].as_str()).unwrap_or("an error");
                self.note(format!("uncaught error: {}{}", clip(text), where_thrown(d)));
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
    /// were answered 400 or worse, took `SLOW_SECS` or more, or are still
    /// waiting, then console errors and warnings. Empty when the page did
    /// nothing worth reporting.
    pub fn report(&self) -> Vec<String> {
        let now = Instant::now();
        let mut lines: Vec<String> = vec![];
        for r in self.order.iter().filter_map(|id| self.requests.get(id)) {
            let what = format!("{} {}{}", r.method, r.url, redirects(r.redirects));
            let after = r.took.map(|t| format!(" after {}", secs(t))).unwrap_or_default();
            match (&r.ended, r.status) {
                (Some(Some(why)), _) => lines.push(format!("request failed ({why}){after}: {what}")),
                (_, Some(s)) if s >= 400 => lines.push(format!("request answered {s}{after}: {what}")),
                (None, _) => lines.push(format!(
                    "request still waiting after {}: {what}",
                    secs(now.saturating_duration_since(r.sent).as_secs_f64())
                )),
                (Some(None), _) => {
                    if let Some(t) = r.took.filter(|t| *t >= SLOW_SECS) {
                        lines.push(format!("request took {}: {what}", secs(t)));
                    }
                }
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

/// Seconds between a request's first hop and the event `p` (its finish or
/// failure), by the browser's own clock. `None` when either is missing.
fn elapsed(started: Option<f64>, p: &Value) -> Option<f64> {
    Some((p["timestamp"].as_f64()? - started?).max(0.0))
}

/// "4.2s" under ten seconds, "14s" from there.
fn secs(t: f64) -> String {
    if t < 10.0 {
        format!("{t:.1}s")
    } else {
        format!("{}s", t.round() as u64)
    }
}

fn redirects(n: u32) -> String {
    match n {
        0 => String::new(),
        1 => " (after 1 redirect)".to_string(),
        n => format!(" (after {n} redirects)"),
    }
}

/// The first frame of a stack trace: its script address and its 0-based
/// line and column.
fn top_frame(stack: &Value) -> Option<(String, u64, u64)> {
    let f = stack["callFrames"].as_array()?.first()?;
    Some((f["url"].as_str().unwrap_or("").to_string(), f["lineNumber"].as_u64()?, f["columnNumber"].as_u64().unwrap_or(0)))
}

/// " (at script:line:column)", 1-based as a person reads them; a script with
/// no address is one the page added itself (an inline block, or markup
/// inserted with its scripts), which is worth saying too.
fn at(url: &str, line: u64, col: u64) -> String {
    if url.is_empty() {
        format!(" (at line {} of a script the page added itself)", line + 1)
    } else {
        format!(" (at {}:{}:{})", without_query(url), line + 1, col + 1)
    }
}

/// Where an uncaught error was thrown: the exception details' own script
/// and line, or the top of its stack.
fn where_thrown(d: &Value) -> String {
    if let Some(line) = d["lineNumber"].as_u64() {
        let url = d["url"].as_str().unwrap_or("");
        return at(url, line, d["columnNumber"].as_u64().unwrap_or(0));
    }
    top_frame(&d["stackTrace"]).map(|(url, line, col)| at(&url, line, col)).unwrap_or_default()
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
