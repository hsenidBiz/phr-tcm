//! The typed steps a script is made of, and what each does to the browser.
//!
//! Every action answers `{ ok, detail }`. `detail` is written for the
//! human watching, because in this runner the person - not the machine -
//! decides the verdict. An action that cannot tell what happened says so
//! rather than guessing.

use super::cdp::{CdpError, Driver};
use super::expect::{self, Check};
use super::input::{self, Blocked};
use super::locator::{resolve, Target};
use super::page;
use super::timing::Timing;
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Navigate { url: String },
    Click { selector: Target },
    Fill { selector: Target, value: String },
    WaitFor { selector: Target, timeout_ms: u32 },
    CheckText { value: String },
    CheckUrl { contains: String },
    ExpectVisible {
        selector: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectHidden {
        selector: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectText {
        selector: Target,
        equals: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectContainsText {
        selector: Target,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectCount {
        selector: Target,
        equals: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectAttribute {
        selector: Target,
        name: String,
        equals: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ActionOutcome {
    pub ok: bool,
    pub detail: String,
    /// A file in the autorun `shots` folder, taken when the action failed.
    /// A name, never a path and never the image: run files stay small, and
    /// the webview cannot ask for anything outside that folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
    /// True when the browser connection failed rather than the page, so
    /// callers do not ask a dead browser for a picture. Process-internal
    /// only: never crosses the IPC boundary and never lands in a saved run
    /// file.
    #[serde(skip)]
    #[specta(skip)]
    pub harness: bool,
}

impl ActionOutcome {
    pub fn passed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: true, detail: detail.into(), screenshot: None, harness: false }
    }
    pub fn failed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: false, detail: detail.into(), screenshot: None, harness: false }
    }
}

/// A harness failure, said plainly: the app under test did nothing wrong,
/// the browser connection did.
pub(crate) fn harness(e: CdpError) -> ActionOutcome {
    let mut out = ActionOutcome::failed(format!("the browser did not answer: {e}"));
    out.harness = true;
    out
}

pub(crate) fn blocked(b: Blocked) -> ActionOutcome {
    match b {
        Blocked::Page(why) => ActionOutcome::failed(why),
        Blocked::Harness(why) => {
            let mut out = ActionOutcome::failed(format!("the browser did not answer: {why}"));
            out.harness = true;
            out
        }
    }
}

/// The same division `input::blame` draws, for the sites that produce an
/// `ActionOutcome` directly: a refusal came from the PAGE (a navigation
/// landing mid-action yields "Cannot find context with specified id"),
/// anything else is the browser connection. It matters twice over -
/// "the browser did not answer" about a browser that is alive and well is
/// simply wrong, and `harness` also suppresses the failure screenshot.
pub(crate) fn failed_by(e: CdpError) -> ActionOutcome {
    match e {
        CdpError::Protocol { message, .. } => {
            ActionOutcome::failed(format!("the page refused: {message}"))
        }
        other => harness(other),
    }
}

/// An address the browser can be sent to. `file://` is allowed on
/// purpose: the live fixture is a local file and this tab is a
/// development-only one. Narrowing this to an origin allowlist is planned
/// work, not an oversight.
fn is_page_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("file://")
}

/// Does this start with a URI scheme (RFC 3986: a letter, then letters,
/// digits, `+`, `-` or `.`, then `:`)? Anything that does NOT is a
/// relative reference, which the page resolves at run time.
fn has_scheme(url: &str) -> bool {
    let mut chars = url.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
    }
    false
}

/// What `navigate` accepts when a script is SAVED: a page address, or a
/// relative reference like `/dashboard`. Scripts have always been able to
/// write the latter, and because saving validates every action, refusing
/// one here would refuse a whole bundle for containing a single such
/// script. Anything carrying another scheme (`javascript:`, `data:`,
/// `about:`, `chrome:`) is still refused.
fn is_navigable(url: &str) -> bool {
    let u = url.trim();
    !u.is_empty() && (is_page_url(u) || !has_scheme(u))
}

impl Action {
    /// What can be known to be wrong before a browser is involved. Run on
    /// save, so a bad script is refused where it is written, and again on
    /// execute, so nothing invalid reaches the page.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Action::Navigate { url } if !is_navigable(url) => {
                Err(format!("navigate needs an http, https or file address, not {url:?}"))
            }
            Action::Navigate { .. } => Ok(()),
            Action::Click { selector }
            | Action::Fill { selector, .. }
            | Action::WaitFor { selector, .. }
            | Action::ExpectVisible { selector, .. }
            | Action::ExpectHidden { selector, .. }
            | Action::ExpectText { selector, .. }
            | Action::ExpectCount { selector, .. } => selector.validate(),
            Action::ExpectContainsText { selector, value, .. } => {
                if value.trim().is_empty() {
                    return Err("expect_contains_text has an empty value - everything contains nothing".to_string());
                }
                selector.validate()
            }
            Action::ExpectAttribute { selector, name, .. } => {
                if name.trim().is_empty() {
                    return Err("expect_attribute has an empty name".to_string());
                }
                selector.validate()
            }
            Action::CheckText { value } if value.trim().is_empty() => {
                Err("check_text has an empty value".to_string())
            }
            Action::CheckUrl { contains } if contains.trim().is_empty() => {
                Err("check_url has an empty value".to_string())
            }
            Action::CheckText { .. } | Action::CheckUrl { .. } => Ok(()),
        }
    }
}

/// `this` is the element about to be touched.
pub const HIGHLIGHT_JS: &str = r#"function() {
  const prev = this.style.outline;
  this.style.outline = '3px solid #7c5cff';
  setTimeout(() => { this.style.outline = prev; }, 1200);
  return true;
}"#;

/// `this` is the document. Argument: the words to look for.
pub const CHECK_TEXT_JS: &str = r#"function(want) {
  const hay = (this.body ? this.body.innerText : '') || '';
  return hay.toLowerCase().includes(String(want).toLowerCase());
}"#;

/// `this` is the document. Argument: the relative reference. Resolved in
/// the PAGE against its own address, with the script's value passed as an
/// argument and never concatenated into this source.
pub const RESOLVE_URL_JS: &str = r#"function(rel) { return new URL(rel, location.href).href; }"#;

async fn point_and_pause<D: Driver>(
    d: &mut D,
    ready: &input::Ready,
    timing: &Timing,
) -> Result<(), CdpError> {
    page::call_value(d, &ready.handle, HIGHLIGHT_JS, &[]).await?;
    if timing.highlight_ms > 0 {
        tokio::time::sleep(Duration::from_millis(timing.highlight_ms)).await;
    }
    Ok(())
}

/// A page address as given, or a relative reference made absolute against
/// the page's own address. What comes back has to be an address a browser
/// can be sent to in its own right: `new URL("javascript:x", base)` keeps
/// that scheme, so the check is applied again to the RESULT.
async fn absolute<D: Driver>(d: &mut D, url: &str) -> Result<String, ActionOutcome> {
    if is_page_url(url) {
        return Ok(url.to_string());
    }
    let doc = page::document(d).await.map_err(failed_by)?;
    let resolved = page::call_value(d, &doc, RESOLVE_URL_JS, &[json!(url)])
        .await
        .map_err(failed_by)?;
    let resolved = resolved.as_str().unwrap_or("").to_string();
    if !is_page_url(&resolved) {
        return Err(ActionOutcome::failed(format!(
            "navigate needs an http, https or file address, not {resolved:?}"
        )));
    }
    Ok(resolved)
}

async fn navigate<D: Driver>(d: &mut D, url: &str, timing: &Timing) -> ActionOutcome {
    let url = match absolute(d, url).await {
        Ok(u) => u,
        Err(out) => return out,
    };
    let url = url.as_str();
    // Older lifecycle events would satisfy the wait below before this
    // page has even started.
    d.forget_events();
    let reply = match d.call("Page.navigate", json!({ "url": url })).await {
        Ok(r) => r,
        Err(e) => return failed_by(e),
    };
    if let Some(err) = reply["errorText"].as_str() {
        return ActionOutcome::failed(format!("{url} would not load: {err}"));
    }
    // No loaderId means the same document (a #fragment): nothing loads.
    let Some(loader_id) = reply["loaderId"].as_str().map(str::to_string) else {
        return ActionOutcome::passed(format!("moved to {url}"));
    };
    let frame_id = reply["frameId"].as_str().map(str::to_string);
    let deadline = Instant::now() + Duration::from_millis(timing.nav_ms);
    let timed_out = || {
        ActionOutcome::failed(format!("{url} did not finish loading within {}ms", timing.nav_ms))
    };
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        // The `frameId`/`loaderId` filter can only be applied here: the
        // driver hands back events by method name alone, so a lifecycle
        // event for a stale navigation or a sub-frame is still received
        // and has to be told apart from this navigation's own "load".
        let ev = match d.wait_event("Page.lifecycleEvent", remaining).await {
            Ok(ev) => ev,
            Err(CdpError::Timeout { .. }) => return timed_out(),
            Err(e) => return failed_by(e),
        };
        let is_this_navigation = ev.params["loaderId"].as_str() == Some(loader_id.as_str())
            && ev.params["name"].as_str() == Some("load")
            && frame_id.as_deref().map_or(true, |f| ev.params["frameId"].as_str() == Some(f));
        if is_this_navigation {
            return ActionOutcome::passed(format!("loaded {url}"));
        }
        if Instant::now() >= deadline {
            return timed_out();
        }
    }
}

/// The deadline is pushed down into the driver so no single protocol call
/// can outlive this wait's budget, and cleared on EVERY path out - which
/// is why the loop is a separate function rather than an early `return`
/// away from a `set_deadline(None)`.
async fn wait_for<D: Driver>(d: &mut D, target: &Target, timeout_ms: u32, timing: &Timing) -> ActionOutcome {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    d.set_deadline(Some(deadline));
    let out = keep_waiting(d, target, timeout_ms, timing, deadline).await;
    d.set_deadline(None);
    out
}

async fn keep_waiting<D: Driver>(
    d: &mut D,
    target: &Target,
    timeout_ms: u32,
    timing: &Timing,
    deadline: Instant,
) -> ActionOutcome {
    let gave_up = || {
        ActionOutcome::failed(format!("waited {timeout_ms}ms and never saw {}", target.describe()))
    };
    loop {
        page::release(d).await;
        match resolve(d, target).await {
            Ok(found) if !found.is_empty() => {
                return ActionOutcome::passed(format!("found {}", target.describe()));
            }
            Ok(_) => {}
            Err(e) if e.is_transient() => {}
            // The budget ran out inside a call rather than between two of
            // them: this wait ending, not a browser that has died.
            Err(CdpError::Timeout { .. }) if Instant::now() >= deadline => return gave_up(),
            Err(e) => return harness(e),
        }
        if Instant::now() >= deadline {
            return gave_up();
        }
        tokio::time::sleep(Duration::from_millis(timing.poll_ms)).await;
    }
}

fn wait(own: &Option<u32>, timing: &Timing) -> u64 {
    own.map(u64::from).unwrap_or(timing.expect_ms)
}

async fn run<D: Driver>(d: &mut D, action: &Action, timing: &Timing) -> ActionOutcome {
    match action {
        Action::Navigate { url } => navigate(d, url.trim(), timing).await,
        Action::Click { selector } => {
            let ready = match input::wait_ready(d, selector, false, timing).await {
                Ok(r) => r,
                Err(b) => return blocked(b),
            };
            if let Err(e) = point_and_pause(d, &ready, timing).await {
                return failed_by(e);
            }
            match input::click(d, &ready).await {
                Ok(()) => ActionOutcome::passed(format!("clicked {}", selector.describe())),
                Err(b) => blocked(b),
            }
        }
        Action::Fill { selector, value } => {
            let ready = match input::wait_ready(d, selector, true, timing).await {
                Ok(r) => r,
                Err(b) => return blocked(b),
            };
            if let Err(e) = point_and_pause(d, &ready, timing).await {
                return failed_by(e);
            }
            match input::fill(d, &ready, value).await {
                Ok(()) => ActionOutcome::passed(format!("filled {}", selector.describe())),
                Err(b) => blocked(b),
            }
        }
        Action::WaitFor { selector, timeout_ms } => wait_for(d, selector, *timeout_ms, timing).await,
        Action::CheckText { value } => {
            let doc = match page::document(d).await {
                Ok(h) => h,
                Err(e) => return failed_by(e),
            };
            match page::call_value(d, &doc, CHECK_TEXT_JS, &[json!(value)]).await {
                Ok(v) if v.as_bool().unwrap_or(false) => {
                    ActionOutcome::passed(format!("page contains {value}"))
                }
                Ok(_) => ActionOutcome::failed(format!("page does NOT contain {value}")),
                Err(e) => failed_by(e),
            }
        }
        Action::CheckUrl { contains } => match page::eval_value(d, "location.href").await {
            Ok(v) => {
                let href = v.as_str().unwrap_or("");
                let detail = format!("url is {href}");
                if href.contains(contains.as_str()) {
                    ActionOutcome::passed(detail)
                } else {
                    ActionOutcome::failed(detail)
                }
            }
            Err(e) => failed_by(e),
        },
        Action::ExpectVisible { selector, timeout_ms } => {
            expect::expect(d, selector, Check::Visible, wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectHidden { selector, timeout_ms } => {
            expect::expect(d, selector, Check::Hidden, wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectText { selector, equals, timeout_ms } => {
            expect::expect(d, selector, Check::Text(equals), wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectContainsText { selector, value, timeout_ms } => {
            expect::expect(d, selector, Check::ContainsText(value), wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectCount { selector, equals, timeout_ms } => {
            expect::expect(d, selector, Check::Count(*equals), wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectAttribute { selector, name, equals, timeout_ms } => {
            expect::expect(d, selector, Check::Attribute { name, equals }, wait(timeout_ms, timing), timing.poll_ms).await
        }
    }
}

/// Run one action with the standard waits.
pub async fn execute<D: Driver>(d: &mut D, action: &Action) -> ActionOutcome {
    execute_with(d, action, &Timing::default()).await
}

pub async fn execute_with<D: Driver>(d: &mut D, action: &Action, timing: &Timing) -> ActionOutcome {
    if let Err(why) = action.validate() {
        return ActionOutcome::failed(format!("this action cannot run: {why}"));
    }
    let mut out = run(d, action, timing).await;
    // A dialog raised BETWEEN two actions is reported with the NEXT one:
    // the client only reads frames off the socket while a call is in
    // flight, so nothing is noticed until something asks again.
    let dialogs = d.take_dialogs();
    if !dialogs.is_empty() {
        out.detail.push_str(&format!(
            " (the page showed {} and it was accepted)",
            dialogs.join("; ")
        ));
    }
    out
}
