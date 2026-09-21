//! The typed steps a script is made of, and what each does to the browser.
//!
//! Every action answers `{ ok, detail }`. `detail` is written for the
//! human watching, because in this runner the person - not the machine -
//! decides the verdict. An action that cannot tell what happened says so
//! rather than guessing.

use super::cdp::{CdpError, Driver};
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
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ActionOutcome {
    pub ok: bool,
    pub detail: String,
}

impl ActionOutcome {
    pub fn passed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: true, detail: detail.into() }
    }
    pub fn failed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: false, detail: detail.into() }
    }
}

/// A harness failure, said plainly: the app under test did nothing wrong,
/// the browser connection did.
pub(crate) fn harness(e: CdpError) -> ActionOutcome {
    ActionOutcome::failed(format!("the browser did not answer: {e}"))
}

pub(crate) fn blocked(b: Blocked) -> ActionOutcome {
    match b {
        Blocked::Page(why) => ActionOutcome::failed(why),
        Blocked::Harness(why) => ActionOutcome::failed(format!("the browser did not answer: {why}")),
    }
}

fn is_page_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("file://")
}

impl Action {
    /// What can be known to be wrong before a browser is involved. Run on
    /// save, so a bad script is refused where it is written, and again on
    /// execute, so nothing invalid reaches the page.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Action::Navigate { url } if !is_page_url(url) => {
                Err(format!("navigate needs an http, https or file address, not {url:?}"))
            }
            Action::Navigate { .. } => Ok(()),
            Action::Click { selector }
            | Action::Fill { selector, .. }
            | Action::WaitFor { selector, .. } => selector.validate(),
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

async fn navigate<D: Driver>(d: &mut D, url: &str, timing: &Timing) -> ActionOutcome {
    // Older lifecycle events would satisfy the wait below before this
    // page has even started.
    d.forget_events();
    let reply = match d.call("Page.navigate", json!({ "url": url })).await {
        Ok(r) => r,
        Err(e) => return harness(e),
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
            Err(e) => return harness(e),
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

async fn wait_for<D: Driver>(d: &mut D, target: &Target, timeout_ms: u32, timing: &Timing) -> ActionOutcome {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    loop {
        page::release(d).await;
        match resolve(d, target).await {
            Ok(found) if !found.is_empty() => {
                return ActionOutcome::passed(format!("found {}", target.describe()));
            }
            Ok(_) => {}
            Err(e) if e.is_transient() => {}
            Err(e) => return harness(e),
        }
        if Instant::now() >= deadline {
            return ActionOutcome::failed(format!(
                "waited {timeout_ms}ms and never saw {}",
                target.describe()
            ));
        }
        tokio::time::sleep(Duration::from_millis(timing.poll_ms)).await;
    }
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
                return harness(e);
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
                return harness(e);
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
                Err(e) => return harness(e),
            };
            match page::call_value(d, &doc, CHECK_TEXT_JS, &[json!(value)]).await {
                Ok(v) if v.as_bool().unwrap_or(false) => {
                    ActionOutcome::passed(format!("page contains {value}"))
                }
                Ok(_) => ActionOutcome::failed(format!("page does NOT contain {value}")),
                Err(e) => harness(e),
            }
        }
        Action::CheckUrl { contains } => match page::eval_value(d, "location.href").await {
            Ok(v) => {
                let href = v.as_str().unwrap_or("");
                ActionOutcome { ok: href.contains(contains.as_str()), detail: format!("url is {href}") }
            }
            Err(e) => harness(e),
        },
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
    let dialogs = d.take_dialogs();
    if !dialogs.is_empty() {
        out.detail.push_str(&format!(
            " (the page showed {} and it was accepted)",
            dialogs.join("; ")
        ));
    }
    out
}
