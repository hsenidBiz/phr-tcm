//! The typed steps a script is made of, and what each does to the browser.
//!
//! Every action answers `{ ok, detail }`. `detail` is written for the
//! human watching, because in this runner the person - not the machine -
//! decides the verdict. An action that cannot tell what happened says so
//! rather than guessing.

use super::cdp::{browser_silent, CdpError, Driver};
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
    /// Change who is signed in. Carried out by the runner (it needs the
    /// tester's accounts and the project's recipe), not by this driver.
    SignIn { account: String },
    /// Put a file from the project's Test files into the page: into the
    /// file input `selector` names, or through the file chooser that
    /// clicking it opens. `file` is a test file's NAME, never a path. The
    /// runner finds the file (it knows the project) and hands this driver
    /// its path - see `upload_in`.
    Upload { selector: Target, file: String },
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

/// A wait loop's deadline ran out and not one look ever completed. Shared
/// by `expect`'s loop and `wait_for` below, so their wording can never
/// drift apart; `wait_ready` (which returns a `Blocked`, not an
/// `ActionOutcome`) builds the equivalent `Blocked::Harness` itself from
/// the same `browser_silent` wording.
pub(crate) fn harness_timeout(waited_ms: u64, target: &str) -> ActionOutcome {
    let mut out = ActionOutcome::failed(browser_silent(waited_ms, target));
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

/// Where an authored `navigate` may go. `None` is no restriction, which is
/// what a project with no sign-in recipe gets.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Policy {
    pub allowed_origins: Option<Vec<String>>,
}

impl Policy {
    pub fn open() -> Self {
        Policy { allowed_origins: None }
    }
    pub fn only(origins: Vec<String>) -> Self {
        Policy { allowed_origins: Some(origins.into_iter().map(|o| o.to_ascii_lowercase()).collect()) }
    }
    pub fn allows(&self, url: &str) -> bool {
        match &self.allowed_origins {
            None => true,
            Some(list) => crate::autorun::recipe::origin_of(url).is_some_and(|o| list.contains(&o)),
        }
    }
}

/// An address the browser can be sent to. `file://` is allowed on
/// purpose: the live fixture is a local file and this tab is a
/// development-only one. Which of these an authored `navigate` may
/// actually use is `Policy`'s job, not this function's.
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
            Action::SignIn { account } if !crate::autorun::accounts::valid_key(account) => {
                Err(format!("sign_in names \"{account}\", which is not a usable account key"))
            }
            Action::SignIn { .. } => Ok(()),
            Action::Upload { file, .. } if !crate::test_files::valid_test_file_name(file) => {
                Err(format!("upload: {}", crate::test_files::bad_name(file)))
            }
            Action::Upload { selector, .. } => selector.validate(),
        }
    }

    /// Does this action JUDGE the page (rather than drive it or wait for it)?
    pub fn is_check(&self) -> bool {
        matches!(
            self,
            Action::CheckText { .. }
                | Action::CheckUrl { .. }
                | Action::ExpectVisible { .. }
                | Action::ExpectHidden { .. }
                | Action::ExpectText { .. }
                | Action::ExpectContainsText { .. }
                | Action::ExpectCount { .. }
                | Action::ExpectAttribute { .. }
        )
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

async fn navigate<D: Driver>(d: &mut D, url: &str, timing: &Timing, policy: &Policy) -> ActionOutcome {
    let url = match absolute(d, url).await {
        Ok(u) => u,
        Err(out) => return out,
    };
    if !policy.allows(&url) {
        // `origin_of` returning `None` here (rather than an origin outside
        // the list) means the address itself cannot be trusted to go
        // where it reads as - naming a made-up origin for it would be
        // worse than not naming one.
        let detail = match crate::autorun::recipe::origin_of(&url) {
            Some(origin) => format!(
                "{origin} is not one of this project's allowed origins - add it to the sign-in recipe if the test really goes there"
            ),
            None => "this address is not one that can be checked against this project's allowed origins - it does not read as a usable http, https or file address".to_string(),
        };
        return ActionOutcome::failed(detail);
    }
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
    // Whether any call has actually come back - a page that is merely slow
    // to show the element is not the same failure as a browser that has
    // stopped answering, and the two must not be reported the same way.
    let mut looked = false;
    loop {
        page::release(d).await;
        match resolve(d, target).await {
            Ok(found) if !found.is_empty() => {
                return ActionOutcome::passed(format!("found {}", target.describe()));
            }
            Ok(_) => looked = true,
            // The page refusing mid-navigation is the page answering, just
            // between two documents - it counts as a completed look.
            Err(e) if e.is_transient() => looked = true,
            // No new information about the page: the browser did not
            // answer THIS call. Keep going; only the deadline decides
            // whether that is the wait ending or the browser's silence.
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return harness(e),
        }
        if Instant::now() >= deadline {
            return if looked { gave_up() } else { harness_timeout(u64::from(timeout_ms), &target.describe()) };
        }
        tokio::time::sleep(Duration::from_millis(timing.poll_ms)).await;
    }
}

fn wait(own: &Option<u32>, timing: &Timing) -> u64 {
    own.map(u64::from).unwrap_or(timing.expect_ms)
}

async fn run<D: Driver>(d: &mut D, action: &Action, timing: &Timing, policy: &Policy) -> ActionOutcome {
    match action {
        Action::Navigate { url } => navigate(d, url.trim(), timing, policy).await,
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
        // The runner intercepts `sign_in` before it ever reaches this
        // executor (it alone has the tester's accounts and the project's
        // recipe). Reaching here means a caller forgot to.
        Action::SignIn { .. } => ActionOutcome::failed("sign_in is carried out by the runner"),
        // The same for `upload`: only the runner knows the project, and so
        // where its Test files are. It calls `upload_in` with the path.
        Action::Upload { .. } => ActionOutcome::failed("upload is carried out by the runner"),
    }
}

/// `this` is the element. Is it a file input, and may it be used?
pub const FILE_INPUT_JS: &str = r#"function() {
  return {
    file: this instanceof HTMLInputElement && this.type === 'file',
    disabled: !!this.disabled || !!this.closest('fieldset[disabled]'),
  };
}"#;

/// What `upload` says when a click opened no file chooser.
pub fn no_chooser(target: &str) -> String {
    format!(
        "clicking {target} did not open a file chooser - point upload at the page's file input or the button that opens it"
    )
}

/// Puts the file at `file` into the page through `selector`, and says so
/// with `shown` (the file's name and size, as the person reads them):
/// `uploaded "cv.pdf" (12.0 KB) to <target>`.
///
/// The ONE element the selector names is found first, by the same rules a
/// click or a fill finds theirs (the only match, waited for up to
/// `action_ms`). A file input gets the file directly
/// (`DOM.setFileInputFiles`, which raises the input's own `input` and
/// `change` events). Anything else - the button a page draws over its
/// hidden input - is clicked the way `click` clicks, with the browser told
/// to hand the file chooser to this driver rather than show it
/// (`Page.setInterceptFileChooserDialog`); the chooser's input then gets
/// the file. That interception is switched off again on every way out.
///
/// The caller has already checked the file exists and is within the cap,
/// before anything here touches the page. The dialogs a page raised are
/// reported the way `execute_in` reports them.
pub async fn upload_in<D: Driver>(
    d: &mut D,
    selector: &Target,
    file: &std::path::Path,
    shown: &str,
    timing: &Timing,
) -> ActionOutcome {
    let path = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()).to_string_lossy().into_owned();
    let mut out = upload(d, selector, &path, shown, timing).await;
    append_dialogs(d, &mut out);
    out
}

async fn upload<D: Driver>(d: &mut D, selector: &Target, path: &str, shown: &str, timing: &Timing) -> ActionOutcome {
    let handle = match find_one(d, selector, timing).await {
        Ok(h) => h,
        Err(out) => return out,
    };
    let kind = match page::call_value(d, &handle, FILE_INPUT_JS, &[]).await {
        Ok(v) => v,
        Err(e) => return failed_by(e),
    };
    let done = || ActionOutcome::passed(format!("uploaded {shown} to {}", selector.describe()));
    if kind["file"].as_bool() != Some(true) {
        return match through_chooser(d, selector, path, timing).await {
            Ok(()) => done(),
            Err(out) => out,
        };
    }
    if kind["disabled"].as_bool() == Some(true) {
        return ActionOutcome::failed(format!("{} is disabled", selector.describe()));
    }
    let backend = match page::backend_id(d, &handle).await {
        Ok(id) => id,
        Err(e) => return failed_by(e),
    };
    match d.call("DOM.setFileInputFiles", json!({ "files": [path], "backendNodeId": backend })).await {
        Ok(_) => done(),
        Err(e) => failed_by(e),
    }
}

/// Clicks `selector` with the file chooser intercepted, and gives the
/// chooser's input the file. Interception is switched off again whatever
/// happened in between - a page left intercepting would swallow the
/// person's own next chooser.
async fn through_chooser<D: Driver>(
    d: &mut D,
    selector: &Target,
    path: &str,
    timing: &Timing,
) -> Result<(), ActionOutcome> {
    if let Err(e) = d.call("Page.setInterceptFileChooserDialog", json!({ "enabled": true })).await {
        // Switched off anyway: a browser that refused may still have
        // switched it on.
        let _ = d.call("Page.setInterceptFileChooserDialog", json!({ "enabled": false })).await;
        return Err(failed_by(e));
    }
    let out = choose(d, selector, path, timing).await;
    let _ = d.call("Page.setInterceptFileChooserDialog", json!({ "enabled": false })).await;
    out
}

async fn choose<D: Driver>(d: &mut D, selector: &Target, path: &str, timing: &Timing) -> Result<(), ActionOutcome> {
    let ready = input::wait_ready(d, selector, false, timing).await.map_err(blocked)?;
    point_and_pause(d, &ready, timing).await.map_err(failed_by)?;
    // A chooser event left over from an earlier upload must not stand in
    // for the one this click opens.
    d.forget_events();
    input::click(d, &ready).await.map_err(blocked)?;
    let ev = match d.wait_event("Page.fileChooserOpened", Duration::from_millis(timing.action_ms)).await {
        Ok(ev) => ev,
        Err(CdpError::Timeout { .. }) => return Err(ActionOutcome::failed(no_chooser(&selector.describe()))),
        Err(e) => return Err(failed_by(e)),
    };
    let Some(backend) = ev.params["backendNodeId"].as_i64() else {
        return Err(ActionOutcome::failed(format!(
            "clicking {} opened a file chooser the browser did not tie to a file input - point upload at the page's file input",
            selector.describe()
        )));
    };
    d.call("DOM.setFileInputFiles", json!({ "files": [path], "backendNodeId": backend }))
        .await
        .map_err(failed_by)?;
    Ok(())
}

/// The one element `target` names, waited for up to `action_ms` the way
/// `wait_ready` waits for its element - but not for it to be usable, since
/// a file input is set rather than clicked. Several matches are refused
/// as a click refuses them (a legacy string selector takes the first). The
/// deadline is cleared on every way out.
async fn find_one<D: Driver>(d: &mut D, target: &Target, timing: &Timing) -> Result<page::Handle, ActionOutcome> {
    let deadline = Instant::now() + Duration::from_millis(timing.action_ms);
    d.set_deadline(Some(deadline));
    let out = keep_finding(d, target, timing, deadline).await;
    d.set_deadline(None);
    out
}

async fn keep_finding<D: Driver>(
    d: &mut D,
    target: &Target,
    timing: &Timing,
    deadline: Instant,
) -> Result<page::Handle, ActionOutcome> {
    let mut looked = false;
    let mut last = input::STILL_LOOKING.to_string();
    loop {
        page::release(d).await;
        match resolve(d, target).await {
            Ok(found) if found.len() == 1 || (!found.is_empty() && target.is_legacy()) => {
                return Ok(found.into_iter().next().expect("checked non-empty"));
            }
            Ok(found) => {
                looked = true;
                last = if found.is_empty() {
                    "not found".to_string()
                } else {
                    format!("matched {} elements - narrow it, or add nth", found.len())
                };
            }
            Err(e) if e.is_transient() => {
                looked = true;
                last = e.to_string();
            }
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return Err(harness(e)),
        }
        if Instant::now() >= deadline {
            return Err(if looked {
                ActionOutcome::failed(format!("waited {}ms: {} {last}", timing.action_ms, target.describe()))
            } else {
                harness_timeout(timing.action_ms, &target.describe())
            });
        }
        tokio::time::sleep(Duration::from_millis(timing.poll_ms)).await;
    }
}

/// A dialog raised BETWEEN two actions is reported with the NEXT one: the
/// client only reads frames off the socket while a call is in flight, so
/// nothing is noticed until something asks again.
fn append_dialogs<D: Driver>(d: &mut D, out: &mut ActionOutcome) {
    let dialogs = d.take_dialogs();
    if !dialogs.is_empty() {
        out.detail.push_str(&format!(" (the page showed {} and it was accepted)", dialogs.join("; ")));
    }
}

/// Run one action with the standard waits.
pub async fn execute<D: Driver>(d: &mut D, action: &Action) -> ActionOutcome {
    execute_with(d, action, &Timing::default()).await
}

pub async fn execute_with<D: Driver>(d: &mut D, action: &Action, timing: &Timing) -> ActionOutcome {
    execute_in(d, action, timing, &Policy::open()).await
}

pub async fn execute_in<D: Driver>(
    d: &mut D,
    action: &Action,
    timing: &Timing,
    policy: &Policy,
) -> ActionOutcome {
    if let Err(why) = action.validate() {
        return ActionOutcome::failed(format!("this action cannot run: {why}"));
    }
    let mut out = run(d, action, timing, policy).await;
    append_dialogs(d, &mut out);
    out
}
