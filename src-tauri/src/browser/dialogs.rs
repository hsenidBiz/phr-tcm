//! Browser dialogs (`alert`, `confirm`, `prompt`, `beforeunload`): who
//! answers each one, and how a script checks one.
//!
//! A dialog holds the page up until it is answered (measured on Edge: an
//! `alert()` leaves every later call pending), so the driver answers every
//! one the moment it reads it, whatever else is waiting. Which answer it
//! gives is the `DialogBook`'s: one per run, shared by every tab.
//!
//! - **An armed expectation** (`expect_dialog`, armed when its step starts)
//!   claims the next dialog in any tab, first armed first. The dialog is
//!   answered as it asks - OK or Cancel, with the prompt's text - and the
//!   `expect_dialog` judges the text afterwards. A mismatch has still been
//!   answered as asked, so the page is never left stuck.
//! - **Anything else** is accepted, as it always was, and noted as not
//!   expected. The runner says so on the step, or fails the step when the
//!   script asks it to (`fail_on_unexpected_dialog`).
//!
//! A dialog's message is page text, never a secret, but it is cut to
//! `MESSAGE_MAX` characters wherever it is said or kept.

use super::actions::{ActionOutcome, DialogAnswer};
use super::cdp::Driver;
use super::timing::Timing;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long an `expect_dialog` waits when it names no `within_ms`.
pub const DIALOG_WAIT_MS: u32 = 10_000;
/// The longest an `expect_dialog` may wait.
pub const DIALOG_WAIT_MAX_MS: u32 = 60_000;
/// The most of a dialog's message any sentence or record holds.
pub const MESSAGE_MAX: usize = 200;
/// The most dialogs a book remembers between two takes. A page that loops
/// on `alert` must not grow it without end.
const SEEN_MAX: usize = 50;

/// `expect_dialog` given both ways of checking the text.
pub const TEXT_OR_CONTAINS: &str = "expect_dialog takes text or contains, not both";
/// `prompt_text` with `answer: dismiss`.
pub const PROMPT_NEEDS_ACCEPT: &str = "prompt_text needs answer accept";

/// A dialog's message, cut to `MESSAGE_MAX` characters.
pub fn cut(message: &str) -> String {
    message.chars().take(MESSAGE_MAX).collect()
}

/// What an `expect_dialog` that saw none says.
pub const NO_DIALOG: &str = "no dialog appeared within ";

/// `no dialog appeared within <n> seconds`.
pub fn no_dialog(within_ms: u32) -> String {
    format!("{NO_DIALOG}{} seconds", seconds(within_ms))
}

/// Seconds from milliseconds: whole when it is, else one decimal place.
pub fn seconds(ms: u32) -> String {
    if ms % 1000 == 0 {
        (ms / 1000).to_string()
    } else {
        format!("{:.1}", f64::from(ms) / 1000.0)
    }
}

/// `the dialog said "<message>", not "<text>"`.
pub fn said_not(message: &str, text: &str) -> String {
    format!("the dialog said \"{}\", not \"{text}\"", cut(message))
}

/// `the dialog said "<message>", which does not contain "<text>"`.
pub fn said_without(message: &str, text: &str) -> String {
    format!("the dialog said \"{}\", which does not contain \"{text}\"", cut(message))
}

/// What fails a step whose script asked for it: `an unexpected <kind>
/// dialog appeared: "<message>"`.
pub fn unexpected(kind: &str, message: &str) -> String {
    format!("an unexpected {kind} dialog appeared: \"{}\"", cut(message))
}

/// The words between the kind and the message in `accepted`, by which a
/// sentence carrying one is recognised (`autorun::patterns`).
pub const WAS_ACCEPTED: &str = " dialog was accepted: \"";

/// What a step says of a dialog nobody expected, when that does not fail
/// it: `a <kind> dialog was accepted: "<message>"` (`an alert dialog`).
pub fn accepted(kind: &str, message: &str) -> String {
    let a = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
    format!("{a} {kind}{WAS_ACCEPTED}{}\"", cut(message))
}

/// How an armed expectation answers the dialog it claims.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogPlan {
    /// Which expectation: the runner numbers a step's `expect_dialog`s from
    /// 0, in the order they are written.
    pub id: u32,
    pub accept: bool,
    pub prompt_text: Option<String>,
}

/// One dialog the page showed.
#[derive(Debug, Clone, PartialEq)]
pub struct SeenDialog {
    /// `alert`, `confirm`, `prompt` or `beforeunload`.
    pub kind: String,
    /// Cut to `MESSAGE_MAX`.
    pub message: String,
    /// The expectation that claimed it; `None` when nobody expected it.
    pub claimed_by: Option<u32>,
}

/// The run's armed expectations and the dialogs seen since the last take.
#[derive(Debug, Default)]
pub struct DialogBook {
    armed: VecDeque<DialogPlan>,
    seen: Vec<SeenDialog>,
}

impl DialogBook {
    /// Arm these expectations, in order, in place of any still armed.
    pub fn arm(&mut self, plans: Vec<DialogPlan>) {
        self.armed = plans.into();
    }

    /// Let go of every expectation no dialog claimed.
    pub fn disarm(&mut self) {
        self.armed.clear();
    }

    /// Is any expectation still waiting for its dialog?
    pub fn is_armed(&self) -> bool {
        !self.armed.is_empty()
    }

    /// A dialog opened, in any tab: the first armed expectation claims it,
    /// or nobody does. Returns the parameters of the
    /// `Page.handleJavaScriptDialog` that answers it.
    pub fn opened(&mut self, kind: &str, message: &str) -> Value {
        let plan = self.armed.pop_front();
        if self.seen.len() >= SEEN_MAX {
            self.seen.remove(0);
        }
        self.seen.push(SeenDialog {
            kind: kind.to_string(),
            message: cut(message),
            claimed_by: plan.as_ref().map(|p| p.id),
        });
        match plan {
            Some(DialogPlan { accept: true, prompt_text: Some(text), .. }) => {
                json!({ "accept": true, "promptText": text })
            }
            Some(p) => json!({ "accept": p.accept }),
            None => json!({ "accept": true }),
        }
    }

    /// The dialog expectation `id` claimed, if one has.
    pub fn claimed(&self, id: u32) -> Option<&SeenDialog> {
        self.seen.iter().find(|s| s.claimed_by == Some(id))
    }

    /// Every dialog seen since the last take, oldest first.
    pub fn seen(&self) -> &[SeenDialog] {
        &self.seen
    }

    /// Every dialog seen since the last take, handed over and forgotten.
    pub fn take_seen(&mut self) -> Vec<SeenDialog> {
        std::mem::take(&mut self.seen)
    }
}

/// How `expect_dialog` answers: from its `answer` and `prompt_text`.
pub fn plan_of(id: u32, answer: DialogAnswer, prompt_text: &Option<String>) -> DialogPlan {
    DialogPlan { id, accept: answer == DialogAnswer::Accept, prompt_text: prompt_text.clone() }
}

/// What an `expect_dialog` checks, read from the action.
pub struct Expectation<'a> {
    pub text: Option<&'a str>,
    pub contains: Option<&'a str>,
    pub answer: DialogAnswer,
    pub prompt_text: Option<&'a str>,
    pub within_ms: u32,
}

/// Wait for expectation `id` to be claimed, up to `within_ms`, then judge
/// the dialog it claimed. The waiting reads the browser (`idle`), so a
/// dialog that opens meanwhile is answered at once.
pub async fn judge<D: Driver>(d: &mut D, id: u32, want: &Expectation<'_>, timing: &Timing) -> ActionOutcome {
    let deadline = Instant::now() + Duration::from_millis(u64::from(want.within_ms));
    let seen = loop {
        if let Some(seen) = d.dialog_book().and_then(|b| b.claimed(id).cloned()) {
            break seen;
        }
        if Instant::now() >= deadline {
            return ActionOutcome::failed(no_dialog(want.within_ms));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        d.idle(Duration::from_millis(timing.poll_ms).min(left)).await;
    };
    let message = seen.message.trim();
    if let Some(text) = want.text {
        if message != text.trim() {
            return ActionOutcome::failed(said_not(message, text));
        }
    }
    if let Some(text) = want.contains {
        // Case ignored, and every run of whitespace one space on both
        // sides: a message's non-breaking space is a space.
        let collapse = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        if !collapse(message).contains(&collapse(text)) {
            return ActionOutcome::failed(said_without(message, text));
        }
    }
    let pressed = match (want.answer, want.prompt_text) {
        (DialogAnswer::Accept, Some(t)) => format!("typed \"{t}\" and pressed OK"),
        (DialogAnswer::Accept, None) => "pressed OK".to_string(),
        (DialogAnswer::Dismiss, _) => "pressed Cancel".to_string(),
    };
    ActionOutcome::passed(format!("a {} dialog said \"{message}\"; {pressed}", seen.kind))
}
