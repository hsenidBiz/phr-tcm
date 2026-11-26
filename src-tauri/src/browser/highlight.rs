//! What the Auto Test browser shows a watcher about a step: a box drawn over
//! the element the step is about to use, a small label that says what it is
//! about to do, and, once the step is over, the same box turned green or
//! red.
//!
//! It is on only when `Timing::highlight_ms` is above 0 (the person's Pause
//! before each action choice, for a watched run and the assistant's
//! exploring). With it off nothing here sends a call or waits.
//!
//! Drawn ON the page, never on the element: a fixed-position overlay inside
//! a closed shadow root appended to the top document, so the site's own CSS
//! cannot restyle it, and the element's own style is never touched. An
//! element in a same-origin frame is drawn over the top document, offset by
//! the frames around it; an element in a frame whose parent cannot be read
//! (another site) is drawn in that frame's own document instead.
//!
//! The label is built here, in Rust, already hidden where it would show a
//! secret, and handed to the page as text. A secret never travels into this
//! script. A highlight never changes what a step reports: every call it makes,
//! before the action and after, is ignored when it fails, and the action's
//! own first look at the page reports what is wrong with the page.

use super::actions::{Action, DropAt};
use super::cdp::{CdpError, Driver};
use super::expect::{is_password, NOTED_SECRET};
use super::locator::resolve_explained;
use super::page::{self, Handle};
use super::timing::Timing;
use serde_json::json;
use std::time::Duration;

/// The longest a typed, chosen or checked value is shown before it is cut.
pub const SHOWN_MAX: usize = 24;

/// What a password field's typing says in place of its text.
pub const TYPE_PASSWORD: &str = "Type password";
/// What any other secret's typing says in place of its text.
pub const TYPE_HIDDEN: &str = "Type a hidden value";
/// What a check of a secret says in place of its text.
pub const CHECK_HIDDEN: &str = "Check: hidden value";

/// How long the page may take to answer the call that sets the result: a
/// page held by a dialog must not hold the step up.
const RESULT_WAIT: Duration = Duration::from_millis(1500);

/// The colour a step is drawn in, by what kind of step it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Blue: a click, a key press, a drag, a move to another page.
    Click,
    /// Green: a value put in - typing, filling, choosing, uploading.
    Type,
    /// Amber: a check, a step that asserts something about the page.
    Check,
    /// Grey: hover, scroll and the steps that only look or note.
    Quiet,
}

impl Kind {
    /// The name the injected script knows the colour by.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Click => "click",
            Kind::Type => "type",
            Kind::Check => "check",
            Kind::Quiet => "quiet",
        }
    }

    /// The colour's name, as the owner's decision words it.
    pub fn colour(self) -> &'static str {
        match self {
            Kind::Click => "blue",
            Kind::Type => "green",
            Kind::Check => "amber",
            Kind::Quiet => "grey",
        }
    }
}

/// The kind of every action. Matched without a catch-all, so a new action
/// kind will not compile until it is given a colour.
pub fn kind_of(action: &Action) -> Kind {
    match action {
        Action::Click { .. }
        | Action::PressKey { .. }
        | Action::Drag { .. }
        | Action::PageUntil { .. }
        | Action::Navigate { .. }
        | Action::Reload
        | Action::ExpireSession
        | Action::ReturnToArea { .. }
        | Action::OpenTab { .. }
        | Action::SwitchTab { .. }
        | Action::CloseTab { .. } => Kind::Click,
        Action::Fill { .. } | Action::SelectOption { .. } | Action::Upload { .. } | Action::SignIn { .. } => Kind::Type,
        Action::CheckText { .. }
        | Action::CheckUrl { .. }
        | Action::WaitFor { .. }
        | Action::ExpectVisible { .. }
        | Action::ExpectHidden { .. }
        | Action::ExpectText { .. }
        | Action::ExpectTextMoved { .. }
        | Action::ExpectContainsText { .. }
        | Action::ExpectCount { .. }
        | Action::ExpectAttribute { .. }
        | Action::ExpectStyle { .. }
        | Action::ExpectEnabled { .. }
        | Action::ExpectDisabled { .. }
        | Action::ExpectChecked { .. }
        | Action::ExpectNoChange { .. }
        | Action::ExpectResponse { .. }
        | Action::ApiRequest { .. }
        | Action::ExpectRow { .. }
        | Action::ExpectNoRow { .. }
        | Action::ExpectSorted { .. }
        | Action::ExpectRowCount { .. }
        | Action::ExpectEach { .. }
        | Action::ExpectOrder { .. }
        | Action::ExpectPosition { .. }
        | Action::ExpectFocused { .. }
        | Action::ExpectDownload { .. }
        | Action::ExpectTab { .. }
        | Action::ExpectTabClosed { .. }
        | Action::ExpectDialog { .. } => Kind::Check,
        Action::Hover { .. }
        | Action::UseComponent { .. }
        | Action::RememberUrl { .. }
        | Action::RememberText { .. }
        | Action::WhenVisible { .. }
        | Action::WhenHidden { .. }
        | Action::RepeatWhileVisible { .. } => Kind::Quiet,
    }
}

/// What to draw for one step: its colour, its label, and the label to use
/// instead when the element turns out to be a password field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Say {
    pub kind: Kind,
    pub label: String,
    pub if_password: Option<String>,
}

impl Say {
    pub fn new(kind: Kind, label: impl Into<String>) -> Say {
        Say { kind, label: label.into(), if_password: None }
    }

    /// What `action` says. A typed or checked value is cut to `SHOWN_MAX`
    /// characters, and never shown when the action marks it secret.
    pub fn of(action: &Action) -> Say {
        Say::of_in(action, false)
    }

    /// `of`, for an action that may be the runner's own (`runner_own`: the
    /// app's sign-in and password reset). What the runner types is a
    /// password that was filled in before the step ran, so a fill of its own
    /// is always the hidden value, whatever the field is called.
    pub fn of_in(action: &Action, runner_own: bool) -> Say {
        let kind = kind_of(action);
        if runner_own && matches!(action, Action::Fill { .. }) {
            return Say { kind, label: TYPE_HIDDEN.into(), if_password: None };
        }
        let (label, if_password) = label_of(action);
        Say { kind, label, if_password }
    }
}

/// What a drag says: where it drops, in the words the drop position uses.
pub fn drag_label(at: DropAt) -> String {
    match at {
        DropAt::Onto => "Drag".to_string(),
        at => format!("Drag {}", at.word()),
    }
}

/// A value as the label shows it: whitespace folded, cut to `SHOWN_MAX`
/// characters with an ellipsis.
pub fn shown(value: &str) -> String {
    let flat = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= SHOWN_MAX {
        return flat;
    }
    let mut cut: String = flat.chars().take(SHOWN_MAX).collect();
    cut.push('\u{2026}');
    cut
}

/// Is this typed value one the action itself marks secret? A text noted from
/// a password field (`NOTED_SECRET`), the sign-in recipe's placeholders, or
/// a field named like a secret (the one rule every capture redaction uses,
/// `capture_redact::secret_name`) - never decided by looking at the value.
fn marks_secret(value: &str, field_words: &str) -> bool {
    value.starts_with(NOTED_SECRET)
        || value.to_ascii_lowercase().contains("{{password}}")
        || crate::autorun::capture_redact::secret_name(field_words)
}

/// What an upload says: the bare file name, however the file was named.
pub fn upload_label(file: &str) -> String {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    format!("Upload '{}'", shown(name))
}

/// The label for an action, and the label to use instead when the element
/// is a password input (the rule `expect::is_password` states).
fn label_of(action: &Action) -> (String, Option<String>) {
    let quoted = |v: &str| format!("'{}'", shown(v));
    match action {
        Action::Click { .. } => ("Click".into(), None),
        Action::Hover { .. } => ("Hover".into(), None),
        Action::PressKey { key, times } => match times {
            Some(n) if *n > 1 => (format!("Press {} x{n}", shown(key)), None),
            _ => (format!("Press {}", shown(key)), None),
        },
        Action::Drag { position, .. } => (drag_label(position.unwrap_or_default()), None),
        Action::PageUntil { .. } => ("Click next page".into(), None),
        Action::Fill { selector, value, sensitive } => {
            if *sensitive || marks_secret(value, &selector.describe()) {
                (TYPE_HIDDEN.into(), None)
            } else if value.is_empty() {
                ("Clear".into(), Some(TYPE_PASSWORD.into()))
            } else {
                (format!("Type {}", quoted(value)), Some(TYPE_PASSWORD.into()))
            }
        }
        Action::SelectOption { label, value, .. } => match (label, value) {
            (Some(l), _) => (format!("Choose {}", quoted(l)), None),
            (_, Some(v)) => (format!("Choose {}", quoted(v)), None),
            _ => ("Choose".into(), None),
        },
        Action::Upload { file, .. } => (upload_label(file), None),
        Action::CheckText { value } => {
            if marks_secret(value, "") {
                (CHECK_HIDDEN.into(), None)
            } else {
                (format!("Check: shows {}", quoted(value)), None)
            }
        }
        Action::ExpectContainsText { selector, value, .. } => {
            if marks_secret(value, &selector.describe()) {
                (CHECK_HIDDEN.into(), None)
            } else {
                (format!("Check: shows {}", quoted(value)), Some(CHECK_HIDDEN.into()))
            }
        }
        Action::ExpectText { selector, equals, matches, same_as, .. } => match (equals, matches, same_as) {
            (Some(v), _, _) => {
                if marks_secret(v, &selector.describe()) {
                    (CHECK_HIDDEN.into(), None)
                } else {
                    (format!("Check: text is {}", quoted(v)), Some(CHECK_HIDDEN.into()))
                }
            }
            (_, Some(_), _) => ("Check: text matches".into(), None),
            _ if crate::autorun::capture_redact::secret_name(&selector.describe()) => (CHECK_HIDDEN.into(), None),
            _ => ("Check: text as noted".into(), None),
        },
        Action::ExpectTextMoved { .. } => ("Check: number moved".into(), None),
        Action::CheckUrl { .. } => ("Check: address".into(), None),
        Action::ExpectVisible { .. } => ("Check: visible".into(), None),
        Action::ExpectHidden { .. } => ("Check: hidden".into(), None),
        Action::ExpectCount { .. } | Action::ExpectRowCount { .. } => ("Check: count".into(), None),
        Action::ExpectAttribute { name, .. } => (format!("Check: {}", shown(name)), None),
        Action::ExpectStyle { property, .. } => (format!("Check: {}", shown(property)), None),
        Action::ExpectEnabled { .. } => ("Check: enabled".into(), None),
        Action::ExpectDisabled { .. } => ("Check: disabled".into(), None),
        Action::ExpectChecked { checked, .. } => {
            (if *checked { "Check: ticked" } else { "Check: not ticked" }.into(), None)
        }
        Action::ExpectNoChange { .. } => ("Check: nothing changes".into(), None),
        Action::ExpectFocused { .. } => ("Check: has focus".into(), None),
        Action::ExpectRow { .. } => ("Check: row".into(), None),
        Action::ExpectNoRow { .. } => ("Check: no such row".into(), None),
        Action::ExpectSorted { .. } => ("Check: sorted".into(), None),
        Action::ExpectEach { .. } => ("Check: each item".into(), None),
        Action::ExpectOrder { .. } => ("Check: order".into(), None),
        Action::ExpectPosition { .. } => ("Check: position".into(), None),
        Action::WaitFor { .. } => ("Wait for it".into(), None),
        other => (
            match kind_of(other) {
                Kind::Click => "Click",
                Kind::Type => "Type",
                Kind::Check => "Check",
                Kind::Quiet => "Look",
            }
            .into(),
            None,
        ),
    }
}

/// Does the call before this action draw a box? The one list `execute_in`
/// reads to decide whether a result is owed afterwards.
pub fn draws_before(action: &Action) -> bool {
    matches!(
        action,
        Action::Click { .. }
            | Action::Hover { .. }
            | Action::Fill { .. }
            | Action::SelectOption { .. }
            | Action::Drag { .. }
            | Action::ExpectNoChange { .. }
            | Action::PageUntil { .. }
    )
}

/// Does this check get its box only once it has finished? The look that
/// proves it may be waiting for its element to appear, so there is nothing to
/// draw round before; afterwards the element, if it is one, is found again.
pub fn draws_after(action: &Action) -> bool {
    action.is_check()
        && !matches!(action, Action::ExpectNoChange { .. } | Action::PageUntil { .. })
        && !action.targets().is_empty()
}

/// Everything the page needs to draw, written once: where the box goes
/// (over the top document, offset by same-origin frames) and the overlay
/// state kept under a symbol on the window it is drawn in.
///
/// `this` is the element. Arguments: the colour's key (`Kind::key`), the
/// label and how long the step holds the box before it acts (the pause, in
/// milliseconds). Draws the box and label in a closed shadow root, with inline
/// styles only and `pointer-events: none` (hidden from assistive technology),
/// follows the element while it is shown, and takes itself away after 20
/// seconds if no result ever comes.
///
/// A box drawn in a frame's own window (its parent is another site) is one the
/// result call, which runs in the top window, can never reach. It takes
/// itself away after the pause and 2 seconds instead.
pub const SHOW_JS: &str = r#"function(kind, label, holdMs) {
  try {
    const KEY = Symbol.for('tcm.action-highlight');
    const COLOURS = { click: '#2563eb', type: '#16a34a', check: '#d97706', quiet: '#64748b' };
    const el = this;
    const home = (e) => {
      const own = (e.ownerDocument || e).defaultView;
      try {
        let w = own, x = 0, y = 0;
        while (w !== w.top) {
          const f = w.frameElement;
          if (!f) throw new Error('parent cannot be read');
          const r = f.getBoundingClientRect();
          const cs = w.parent.getComputedStyle(f);
          x += r.left + f.clientLeft + (parseFloat(cs.paddingLeft) || 0);
          y += r.top + f.clientTop + (parseFloat(cs.paddingTop) || 0);
          w = w.parent;
        }
        return { win: w, x: x, y: y };
      } catch (err) {
        return { win: own, x: 0, y: 0 };
      }
    };
    const first = home(el);
    const win = first.win;
    const doc = win.document;
    const old = win[KEY];
    if (old) old.stop();
    const colour = COLOURS[kind] || COLOURS.quiet;
    const host = doc.createElement('div');
    host.setAttribute('aria-hidden', 'true');
    host.setAttribute('style', 'all: initial !important; display: block !important; position: fixed !important; top: 0 !important; left: 0 !important; width: 0 !important; height: 0 !important; overflow: visible !important; pointer-events: none !important; z-index: 2147483647 !important;');
    const root = host.attachShadow({ mode: 'closed' });
    const base = 'position: fixed; pointer-events: none; box-sizing: border-box; margin: 0; ';
    const box = doc.createElement('div');
    box.setAttribute('style', base + 'border: 3px solid ' + colour + '; border-radius: 4px; box-shadow: 0 0 0 1px rgba(255,255,255,0.9), 0 0 0 4px rgba(15,23,42,0.18);');
    const tag = doc.createElement('div');
    tag.setAttribute('style', base + 'background: ' + colour + '; color: #ffffff; font: 600 12px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif; padding: 2px 8px; border-radius: 4px; white-space: nowrap; max-width: 90vw; overflow: hidden; text-overflow: ellipsis; box-shadow: 0 1px 3px rgba(0,0,0,0.35);');
    tag.textContent = String(label);
    root.appendChild(box);
    root.appendChild(tag);
    (doc.documentElement || doc.body).appendChild(host);
    let raf = 0, timer = 0, stopped = false;
    const place = () => {
      if (!el.isConnected) return;
      const h = home(el);
      if (h.win !== win) return;
      const r = el.getBoundingClientRect();
      const left = r.left + h.x, top = r.top + h.y;
      const w = Math.max(r.width, 6), ht = Math.max(r.height, 6);
      box.style.left = (left - 3) + 'px';
      box.style.top = (top - 3) + 'px';
      box.style.width = (w + 6) + 'px';
      box.style.height = (ht + 6) + 'px';
      const tw = tag.offsetWidth, th = tag.offsetHeight;
      let ty = top - 3 - th - 4;
      if (ty < 2) ty = top + ht + 3 + 4;
      ty = Math.max(2, Math.min(ty, win.innerHeight - th - 2));
      const tx = Math.max(4, Math.min(left - 3, win.innerWidth - tw - 4));
      tag.style.left = tx + 'px';
      tag.style.top = ty + 'px';
    };
    const tick = () => { if (stopped) return; place(); raf = win.requestAnimationFrame(tick); };
    const state = {
      stop() {
        stopped = true;
        win.cancelAnimationFrame(raf);
        win.clearTimeout(timer);
        host.remove();
        if (win[KEY] === state) delete win[KEY];
      },
      finish(ok, ms) {
        const c = ok ? '#16a34a' : '#dc2626';
        box.style.borderColor = c;
        tag.style.background = c;
        win.clearTimeout(timer);
        timer = win.setTimeout(() => state.stop(), ms);
      },
    };
    win[KEY] = state;
    const life = win === win.top ? 20000 : (Number(holdMs) || 0) + 2000;
    timer = win.setTimeout(() => state.stop(), life);
    tick();
    return true;
  } catch (err) {
    return false;
  }
}"#;

/// Run in the top frame, with the step's result as the argument: the box
/// turns green (it worked, gone after about 0.8 seconds) or red (it failed,
/// gone after about 2 seconds).
pub const RESULT_JS: &str = r#"function(ok) {
  try {
    const state = window[Symbol.for('tcm.action-highlight')];
    if (!state) return false;
    state.finish(ok, ok ? 800 : 2000);
    return true;
  } catch (err) {
    return false;
  }
}"#;

/// Draw `say` over `handle`. A password field's own label replaces the
/// typed one. The caller ignores a failure (`actions::point_at`).
pub async fn show<D: Driver>(d: &mut D, handle: &Handle, say: &Say, timing: &Timing) -> Result<(), CdpError> {
    let mut label = say.label.as_str();
    if let Some(hidden) = &say.if_password {
        // A failed look at the element cannot say it is not a password:
        // the hidden label is used. Nothing is lost - the call below reports
        // a page that cannot be reached.
        if is_password(d, handle).await.unwrap_or(true) {
            label = hidden;
        }
    }
    page::call_value(d, handle, SHOW_JS, &[json!(say.kind.key()), json!(label), json!(timing.highlight_ms)]).await?;
    Ok(())
}

/// Turn the box green or red, and let it go. Quiet by design: a page that
/// has moved on, a tab that is gone, a dialog that holds the page - none of
/// it is the step's business.
pub async fn conclude<D: Driver>(d: &mut D, timing: &Timing, ok: bool) {
    if timing.highlight_ms == 0 {
        return;
    }
    let expression = format!("({RESULT_JS})({ok})");
    let _ = d
        .call_within("Runtime.evaluate", json!({ "expression": expression, "returnByValue": true }), RESULT_WAIT)
        .await;
}

/// A check that has finished: find its element again and, if it is the one
/// element, draw the check's box over it and then its result. Nothing is
/// drawn when the element is not there, or is several.
pub async fn after_check<D: Driver>(d: &mut D, action: &Action, ok: bool, timing: &Timing) {
    if timing.highlight_ms == 0 || !draws_after(action) {
        return;
    }
    let Some(target) = action.targets().first().copied() else { return };
    let Ok(found) = resolve_explained(d, target).await else { return };
    if let [handle] = found.handles.as_slice() {
        if show(d, handle, &Say::of(action), timing).await.is_ok() {
            d.idle(Duration::from_millis(timing.highlight_ms)).await;
            conclude(d, timing, ok).await;
        }
    }
    page::release(d).await;
}
