//! Recording a project's sign-in recipe: the person signs in by hand once
//! in a visible browser, and what they clicked and which fields they typed
//! into become the recipe's steps.
//!
//! A listener added to every document reports, in order: a click on
//! anything that is not a text field; a text field that was typed into
//! (never what was typed - the webview later says which field is the
//! username, the password, or a fixed text); Enter in a field, as the
//! click on its form's sign-in button that Enter stands for; and, while
//! pick mode is on, the one click that says "signed in" - that click is
//! not carried out.
//!
//! Which locator to build is decided by plain functions, tested without a
//! browser. Nothing here saves anything: the command saves a recipe only
//! after it has signed in with it in a fresh browser.

use super::recipe::{has_placeholder, SignInRecipe, PASSWORD, USERNAME};
use super::recorder::{self, exact_role, AxLink, ClickPayload, Ended};
use super::signin::{AFTER_SIGN_IN_STOPPED, MARKER_NEVER_APPEARED, PAGE_DID_NOT_OPEN};
use crate::browser::actions::{execute_in, Action, Policy};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::locator::{LocatorStep, Target};
use crate::browser::page;
use crate::browser::timing::Timing;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The function the page calls to report: `window.<BINDING>(json)`. Its
/// own binding, so a sign-in report can never be read as a module click.
pub const BINDING: &str = "__tcmRecordSignIn";

/// Added to every new document. Held elements live where the module
/// recorder keeps its own (`__tcmRecHeld`, `__tcmRecDoc`), so the same
/// lookup finds them. A field's value is never read: whether a field was
/// typed into is a flag set by the `input` event, and a field is reported
/// by its attributes and its place in the page. The only click it ever
/// stops is the one made in pick mode.
pub const LISTENER_JS: &str = r#"(() => {
  if (window.__tcmSignInArmed) return;
  window.__tcmSignInArmed = true;
  const doc = Math.random().toString(36).slice(2) + Date.now().toString(36);
  const held = [];
  window.__tcmRecDoc = doc;
  window.__tcmRecHeld = held;
  // Typed-into flags, one per field: set by `input`, cleared once the
  // field is reported. A flag, never the field's contents.
  const typed = new WeakMap();
  const TEXT_TYPES = ['text', 'email', 'password', 'search', 'tel', 'url', 'number'];
  let enterButton = null;
  let enterAt = 0;
  const clean = (s) => String(s || '').replace(/\s+/g, ' ').trim().slice(0, 200);
  const raw = (el, n) => String(el.getAttribute(n) || '');
  const send = (p) => {
    p.doc = doc;
    try { window.__tcmRecordSignIn(JSON.stringify(p)); } catch (_) {}
  };
  const hold = (el) => { held.push(el); return held.length - 1; };
  const elementOf = (t) => (t instanceof Element ? t : (t && t.parentElement));
  const entryOf = (t) => {
    let el = elementOf(t);
    if (!el) return null;
    if (el.isContentEditable) {
      while (el.parentElement && el.parentElement.isContentEditable) el = el.parentElement;
      return el;
    }
    if (el.tagName === 'TEXTAREA') return el;
    if (el.tagName === 'INPUT' && TEXT_TYPES.includes(String(el.type).toLowerCase())) return el;
    return null;
  };
  const isPassword = (el) => el.tagName === 'INPUT' && String(el.type).toLowerCase() === 'password';
  const clicked = (ev, el) => {
    const near = el.closest('a,button,summary,[role]') || el;
    // No words at all from a click on, or around, anything a person can
    // type into: a region's text would carry what was typed there.
    const quiet = el.isContentEditable || near.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)
      || !!near.querySelector('input,textarea,select,[contenteditable]');
    send({
      ev: ev,
      i: hold(el),
      tag: near.tagName.toLowerCase(),
      role: near.getAttribute('role') || '',
      label: clean(near.getAttribute('aria-label')),
      text: quiet ? '' : clean(near.innerText || near.textContent),
    });
  };
  const field = (el) => {
    typed.set(el, false);
    send({
      ev: 'field',
      i: hold(el),
      tag: el.tagName.toLowerCase(),
      password: isPassword(el),
      id: raw(el, 'id'),
      name: raw(el, 'name'),
      placeholder: raw(el, 'placeholder'),
    });
  };
  document.addEventListener('input', (e) => {
    if (!e.isTrusted) return;
    const el = entryOf(e.target);
    if (el) typed.set(el, true);
  }, true);
  const typedAway = (e) => {
    if (!e.isTrusted) return;
    const el = entryOf(e.target);
    if (el && typed.get(el)) field(el);
  };
  document.addEventListener('change', typedAway, true);
  document.addEventListener('focusout', typedAway, true);
  document.addEventListener('keydown', (e) => {
    if (!e.isTrusted || e.key !== 'Enter' || e.isComposing) return;
    const el = entryOf(e.target);
    if (!el || el.tagName !== 'INPUT') return;
    if (typed.get(el)) field(el);
    const form = el.form || el.closest('form');
    const button = form && form.querySelector('button[type=submit],input[type=submit],button:not([type])');
    if (!button) {
      send({ ev: 'no_submit' });
      return;
    }
    enterButton = button;
    enterAt = Date.now();
    clicked('click', button);
  }, true);
  document.addEventListener('click', (e) => {
    if (!e.isTrusted) return;
    const el = elementOf(e.target);
    if (!el) return;
    const fromEnter = enterButton && Date.now() - enterAt < 2000 && (el === enterButton || enterButton.contains(el));
    enterButton = null;
    if (fromEnter) return;
    if (window.__tcmRecPick) {
      window.__tcmRecPick = false;
      e.preventDefault();
      e.stopImmediatePropagation();
      clicked('marker', el);
      return;
    }
    if (entryOf(el)) return;
    const label = el.closest('label');
    if (label && label.control && label.control !== el && !label.control.contains(el)) return;
    clicked('click', el);
  }, true);
})()"#;

/// Pick mode on, on whatever document is open now.
pub const PICK_ON_JS: &str = "window.__tcmRecPick = true";
/// Pick mode off: a marker arrived.
pub const PICK_OFF_JS: &str = "window.__tcmRecPick = false";

const POLL: Duration = Duration::from_millis(250);

/// The roles a text field has in the accessibility tree.
const FIELD_ROLES: [&str; 4] = ["textbox", "searchbox", "combobox", "spinbutton"];

pub const UNREADABLE_CLICK: &str = "that click could not be named - click the words of the button or link itself";
pub const UNREADABLE_FIELD: &str =
    "a field you typed into could not be named - it has no label, id, name or placeholder to find it by";
pub const UNREADABLE_MARKER: &str =
    "that click could not be named - press I'm signed in again and click something with words on it";
pub const NO_SUBMIT: &str =
    "Enter was pressed where there is no sign-in button to click instead - record again, and click the sign-in button rather than pressing Enter";
pub const NO_STEPS: &str =
    "nothing was recorded - sign in by clicking and typing in the recording browser, then press Finish";
pub const NO_MARKER: &str =
    "there is no signed-in check - press I'm signed in, then click something only a signed-in person sees";

/// What the listener read about a text field: its attributes, never its
/// value.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
#[serde(default)]
pub struct FieldHints {
    pub tag: String,
    pub password: bool,
    pub id: String,
    pub name: String,
    pub placeholder: String,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct FieldPayload {
    pub doc: String,
    pub i: u32,
    #[serde(flatten)]
    pub hints: FieldHints,
}

/// One report from the page.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum Report {
    Click(ClickPayload),
    Field(FieldPayload),
    Marker(ClickPayload),
    NoSubmit {},
}

/// One recorded step.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Click(Target),
    Field { target: Target, password: bool },
}

impl Step {
    pub fn target(&self) -> &Target {
        match self {
            Step::Click(t) | Step::Field { target: t, .. } => t,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Captured {
    pub steps: Vec<Step>,
    /// The signed-in check, once one was picked. A later pick replaces it.
    pub marker: Option<Target>,
    pub ended: Ended,
}

/// What `capture` tells its caller as it goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen<'a> {
    /// A step was added; its 1-based position.
    Step(u32, &'a Step),
    Marker(&'a Target),
    Unreadable(&'a str),
}

/// CSS.escape (CSSOM), for an id written as `#id`.
pub fn css_escape(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let code = c as u32;
        if c == '\0' {
            out.push('\u{FFFD}');
        } else if (1..=0x1f).contains(&code)
            || code == 0x7f
            || (i == 0 && c.is_ascii_digit())
            || (i == 1 && c.is_ascii_digit() && chars[0] == '-')
        {
            out.push_str(&format!("\\{code:x} "));
        } else if i == 0 && c == '-' && chars.len() == 1 {
            out.push_str("\\-");
        } else if code >= 0x80 || c == '-' || c == '_' || c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    out
}

/// A CSS string in double quotes, for an attribute's value.
pub fn css_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\0' => out.push('\u{FFFD}'),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\{:x} ", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The CSS a field is found by when the accessibility tree cannot name
/// it: its id, else its name, else "the password field", else its
/// placeholder. Built from attributes only.
pub fn field_css(h: &FieldHints) -> Option<String> {
    let tag = if h.tag.trim().is_empty() { "input".to_string() } else { h.tag.trim().to_ascii_lowercase() };
    if !h.id.trim().is_empty() {
        return Some(format!("#{}", css_escape(&h.id)));
    }
    if !h.name.trim().is_empty() {
        return Some(format!("{tag}[name={}]", css_string(&h.name)));
    }
    if h.password {
        return Some("input[type=\"password\"]".to_string());
    }
    if !h.placeholder.trim().is_empty() {
        return Some(format!("{tag}[placeholder={}]", css_string(&h.placeholder)));
    }
    None
}

/// The field's own accessibility node, when it is a text field with a
/// name: `textbox "Email"`, matched exactly the way a recorded click is.
pub fn field_locator_from_ax(chain: &[AxLink]) -> Option<Target> {
    let own = chain.iter().find(|n| !n.ignored)?;
    let name = own.name.split_whitespace().collect::<Vec<_>>().join(" ");
    (FIELD_ROLES.contains(&own.role.as_str()) && !name.is_empty()).then(|| exact_role(&own.role, &name))
}

pub fn field_locator_from_hints(h: &FieldHints) -> Option<Target> {
    field_css(h).map(|css| Target::One(LocatorStep { css: Some(css), ..LocatorStep::default() }))
}

/// Start listening: the binding and the listener on every new document.
/// The first document is opened after this, so it is listened on too.
pub async fn arm<D: Driver>(d: &mut D) -> Result<(), CdpError> {
    d.call("Runtime.enable", json!({})).await?;
    d.call("Runtime.addBinding", json!({ "name": BINDING })).await?;
    d.call("Page.addScriptToEvaluateOnNewDocument", json!({ "source": LISTENER_JS })).await?;
    Ok(())
}

/// Nobody signed in, listening, and at the start address. The words of a
/// failure go to the log - a navigate that would not load names its
/// address - and the person gets a fixed sentence.
pub async fn prepare<D: Driver>(d: &mut D, start_url: &str, timing: &Timing) -> Result<(), String> {
    let origins: Vec<String> = super::recipe::origin_of(start_url).into_iter().collect();
    if let Err(e) = crate::browser::session::clear(d, &origins).await {
        crate::applog::warn(format!("sign-in recording: the browser could not be cleared: {e}"));
        return Err(WOULD_NOT_LISTEN.to_string());
    }
    if let Err(e) = arm(d).await {
        crate::applog::warn(format!("sign-in recording: the listener could not be added: {e}"));
        return Err(WOULD_NOT_LISTEN.to_string());
    }
    let out = execute_in(d, &Action::Navigate { url: start_url.to_string() }, timing, &Policy::only(origins)).await;
    if !out.ok {
        crate::applog::warn(format!("sign-in recording: the start address did not open: {}", out.detail));
        return Err(if out.harness { WOULD_NOT_LISTEN } else { START_DID_NOT_OPEN }.to_string());
    }
    // The document that opened already has the listener; running it once
    // more is harmless (it arms once per document) and covers a browser
    // that loaded it before the script was registered.
    let _ = page::eval_value(d, LISTENER_JS).await;
    Ok(())
}

pub const WOULD_NOT_LISTEN: &str =
    "the browser would not report what you do - try again, and see Settings, Logs if it keeps happening";
pub const START_DID_NOT_OPEN: &str =
    "the start address did not open - check it, and see Settings, Logs for the details";

/// The next report, waiting up to `wait`. `Ok(None)` when there was none,
/// it was another binding's call, or it could not be read.
pub async fn next_report<D: Driver>(d: &mut D, wait: Duration) -> Result<Option<Report>, CdpError> {
    match d.wait_event("Runtime.bindingCalled", wait).await {
        Ok(ev) => {
            if ev.params["name"].as_str() != Some(BINDING) {
                return Ok(None);
            }
            Ok(serde_json::from_str::<Report>(ev.params["payload"].as_str().unwrap_or("")).ok())
        }
        Err(CdpError::Timeout { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The locator for a reported field: the accessibility tree while the
/// field is still there, else its attributes.
pub async fn locate_field<D: Driver>(d: &mut D, field: &FieldPayload) -> Result<Target, String> {
    if let Ok(Some(chain)) = recorder::held_ax_chain(d, &field.doc, field.i).await {
        if let Some(t) = field_locator_from_ax(&chain) {
            return Ok(t);
        }
    }
    field_locator_from_hints(&field.hints).ok_or_else(|| UNREADABLE_FIELD.to_string())
}

/// A field reported again straight after itself (typed into, left, and
/// typed into again) is one step, not two.
fn repeats_last(steps: &[Step], target: &Target) -> bool {
    matches!(steps.last(), Some(Step::Field { target: t, .. }) if t == target)
}

/// Collect steps until `cancel`, a closed browser, or `stop`. While `pick`
/// is set, pick mode is put back on the page each time round - a page
/// change while it is armed would otherwise lose it - until a marker
/// arrives, which turns it off on both sides.
pub async fn capture<D: Driver>(
    d: &mut D,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    pick: &AtomicBool,
    on: &mut (dyn FnMut(Seen) + Send),
) -> Captured {
    let mut steps: Vec<Step> = vec![];
    let mut marker: Option<Target> = None;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Captured { steps, marker, ended: Ended::Cancelled };
        }
        match next_report(d, POLL).await {
            Ok(Some(report)) => {
                match report {
                    Report::Click(click) => match recorder::locate(d, &click).await {
                        Ok(t) => {
                            steps.push(Step::Click(t));
                            on(Seen::Step(steps.len() as u32, steps.last().expect("just pushed")));
                        }
                        Err(_) => on(Seen::Unreadable(UNREADABLE_CLICK)),
                    },
                    Report::Field(field) => match locate_field(d, &field).await {
                        Ok(t) if repeats_last(&steps, &t) => {}
                        Ok(t) => {
                            steps.push(Step::Field { target: t, password: field.hints.password });
                            on(Seen::Step(steps.len() as u32, steps.last().expect("just pushed")));
                        }
                        Err(why) => on(Seen::Unreadable(&why)),
                    },
                    Report::Marker(click) => {
                        pick.store(false, Ordering::SeqCst);
                        let _ = page::eval_value(d, PICK_OFF_JS).await;
                        match recorder::locate(d, &click).await {
                            Ok(t) => {
                                marker = Some(t);
                                on(Seen::Marker(marker.as_ref().expect("just set")));
                            }
                            Err(_) => on(Seen::Unreadable(UNREADABLE_MARKER)),
                        }
                    }
                    Report::NoSubmit {} => on(Seen::Unreadable(NO_SUBMIT)),
                }
                continue;
            }
            Err(CdpError::Closed) | Err(CdpError::Transport(_)) => {
                return Captured { steps, marker, ended: Ended::Closed };
            }
            Ok(None) => {}
            Err(_) => tokio::time::sleep(POLL).await,
        }
        if stop.load(Ordering::SeqCst) {
            return Captured { steps, marker, ended: Ended::Stopped { href: String::new() } };
        }
        if pick.load(Ordering::SeqCst) {
            // Best effort: a page between documents refuses, and the next
            // round tries again.
            let _ = page::eval_value(d, PICK_ON_JS).await;
        }
    }
}

/// A finished recording's steps and marker, or why there are none.
pub fn finish(captured: Captured) -> Result<(Vec<Step>, Option<Target>), String> {
    match captured.ended {
        Ended::Cancelled => Err(recorder::CANCELLED.to_string()),
        Ended::Closed => Err(recorder::BROWSER_CLOSED.to_string()),
        Ended::Stopped { .. } => {
            if captured.steps.is_empty() {
                return Err(NO_STEPS.to_string());
            }
            Ok((captured.steps, captured.marker))
        }
    }
}

/// What a field step fills in.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum FieldRole {
    Username,
    Password,
    Text,
}

/// The review's choice for one field step, in the order the fields were
/// recorded. `text` is the fixed text, for `text` only.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FieldChoice {
    pub role: FieldRole,
    #[serde(default)]
    pub text: String,
}

/// One recorded step as the dialog shows it: locator words only. There is
/// no value to show - none was ever read.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct DraftStepView {
    /// "click" or "field"
    pub kind: String,
    pub readable: String,
    pub password: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct SignInDraftView {
    pub steps: Vec<DraftStepView>,
    /// The signed-in check in words; empty when none was picked.
    pub marker: String,
}

/// A finished recording, kept in memory between Finish and a save that
/// works. The webview only ever sees `view()`; the locators stay here.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub organization: String,
    pub project: String,
    pub start_url: String,
    pub steps: Vec<Step>,
    pub marker: Option<Target>,
}

pub fn step_view(step: &Step) -> DraftStepView {
    match step {
        Step::Click(t) => DraftStepView { kind: "click".into(), readable: t.describe(), password: false },
        Step::Field { target, password } => {
            DraftStepView { kind: "field".into(), readable: target.describe(), password: *password }
        }
    }
}

impl Draft {
    pub fn view(&self) -> SignInDraftView {
        SignInDraftView {
            steps: self.steps.iter().map(step_view).collect(),
            marker: self.marker.as_ref().map(Target::describe).unwrap_or_default(),
        }
    }

    pub fn field_count(&self) -> usize {
        self.steps.iter().filter(|s| matches!(s, Step::Field { .. })).count()
    }

    /// The recipe this recording makes with the review's field choices.
    /// `after_sign_in`, `allowed_origins` and `session_minutes` are kept
    /// from the project's recipe when it has one: a recording records only
    /// the sign-in itself.
    pub fn recipe(&self, fields: &[FieldChoice], existing: Option<&SignInRecipe>) -> Result<SignInRecipe, String> {
        if self.steps.is_empty() {
            return Err(NO_STEPS.to_string());
        }
        let signed_in = self.marker.clone().ok_or_else(|| NO_MARKER.to_string())?;
        if fields.len() != self.field_count() {
            return Err("the field choices do not match the recording - record it again".to_string());
        }
        let mut choices = fields.iter();
        let mut steps = vec![];
        for (n, step) in self.steps.iter().enumerate() {
            let action = match step {
                Step::Click(t) => Action::Click { selector: t.clone() },
                Step::Field { target, .. } => {
                    let choice = choices.next().expect("counted above");
                    let value = match choice.role {
                        FieldRole::Username => USERNAME.to_string(),
                        FieldRole::Password => PASSWORD.to_string(),
                        FieldRole::Text => {
                            if choice.text.trim().is_empty() {
                                return Err(format!(
                                    "step {}: type the fixed text, or choose Username or Password",
                                    n + 1
                                ));
                            }
                            if has_placeholder(&choice.text) {
                                return Err(format!(
                                    "step {}: fixed text cannot hold {{{{username}}}} or {{{{password}}}} - choose Username or Password instead",
                                    n + 1
                                ));
                            }
                            choice.text.clone()
                        }
                    };
                    Action::Fill { selector: target.clone(), value }
                }
            };
            steps.push(super::recipe::RecipeStep::Do(action));
        }
        let recipe = SignInRecipe {
            start_url: self.start_url.clone(),
            steps,
            after_sign_in: existing.map(|r| r.after_sign_in.clone()).unwrap_or_default(),
            signed_in,
            allowed_origins: existing.map(|r| r.allowed_origins.clone()).unwrap_or_default(),
            session_minutes: existing.map(|r| r.session_minutes).unwrap_or(480),
        };
        recipe.validate()?;
        Ok(recipe)
    }
}

pub const CHECK_BROWSER_SILENT: &str =
    "the check could not finish: the browser did not respond - try again, and see Settings, Logs if it keeps happening";
pub const CHECK_START_DID_NOT_OPEN: &str =
    "the check could not open the start address - check it, and see Settings, Logs for the details";

/// The dialog's sentence for a check that did not sign in, from
/// `sign_in`'s own (already redacted) words. The start address is never
/// named; the step that stopped is.
pub fn check_failure(detail: &str, harness: bool) -> String {
    if harness {
        return CHECK_BROWSER_SILENT.to_string();
    }
    if detail.starts_with(PAGE_DID_NOT_OPEN) {
        return CHECK_START_DID_NOT_OPEN.to_string();
    }
    if let Some((_, rest)) = detail.split_once(AFTER_SIGN_IN_STOPPED) {
        return format!(
            "the recorded sign-in worked, but the recipe's after-sign-in steps did not - nothing was saved. After-sign-in step {rest}"
        );
    }
    if detail.starts_with(MARKER_NEVER_APPEARED) {
        return format!(
            "the recorded steps ran, but the signed-in check never appeared - nothing was saved. Check the account's username and password, or pick a different signed-in check ({})",
            detail.trim_start_matches(MARKER_NEVER_APPEARED).split(" never appeared").next().unwrap_or("")
        );
    }
    format!("the check did not sign in - nothing was saved: {detail}")
}
