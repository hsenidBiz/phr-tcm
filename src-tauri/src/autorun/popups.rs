//! The project's known pop-ups (`SignInRecipe::close_when_shown`), closed
//! by the runner right before an action that touches the page.
//!
//! One look per pop-up, no waiting: one that is up has its close control
//! clicked, and the action then runs as written. The action's outcome says
//! so ("closed the cookie banner first"), and says so too when the close
//! did not work - the action then fails, if it fails, the way it always
//! would have ("is covered by div#cookieBanner..."). Nothing is tried
//! twice, and nothing not on the list is ever closed.
//!
//! Called from the runner's one place for a plain action (`runner::plain`),
//! which every unattended run, watched run, try, discovery and replay goes
//! through, so what an assistant sees while writing a script is what a run
//! does.

use super::recipe::ClosePopup;
use crate::browser::actions::{execute_in, shown_now, Action, ActionOutcome, Policy};
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;

/// The longest the close click waits for its control to be usable. The
/// control belongs to a pop-up that was just seen up, so it is there at
/// once or not at all; this only bounds a slow page.
pub const CLOSE_MS: u64 = 2_000;

/// Whether `action` acts on an element a pop-up could sit over. Checks,
/// waits, navigation and tab actions look or go elsewhere; a key press goes
/// to whatever has focus, which clicking a close control would move.
pub fn touches_the_page(action: &Action) -> bool {
    matches!(
        action,
        Action::Click { .. } | Action::ExpectNoChange { .. } | Action::Fill { .. } | Action::Drag { .. } | Action::Upload { .. }
    )
}

/// Said on an action after the runner closed `name` before it.
pub fn closed(name: &str) -> String {
    format!("closed the {name} first")
}

/// Said on an action when `name` was up and clicking its close control
/// did not work.
pub fn not_closed(name: &str, why: &str) -> String {
    format!("the {name} was showing and could not be closed: {why}")
}

/// Before `action`: each listed pop-up that is up right now is closed. The
/// notes to add to the action's outcome, in order - none when nothing was
/// up. A pop-up whose close control the action itself aims at is left to
/// the action (a script's own guard on the banner's x still works). A look
/// the browser does not answer reads as not up: the action goes ahead and
/// reports the browser itself.
pub async fn close_shown<D: Driver>(
    d: &mut D,
    popups: &[ClosePopup],
    action: &Action,
    timing: &Timing,
    policy: &Policy,
) -> Vec<String> {
    if popups.is_empty() || !touches_the_page(action) {
        return Vec::new();
    }
    let aimed_at = action.targets();
    let mut notes = Vec::new();
    for p in popups {
        if aimed_at.iter().any(|t| **t == p.close) {
            continue;
        }
        if !matches!(shown_now(d, &p.shown).await, Ok(Some(true))) {
            continue;
        }
        let name = p.name.trim();
        let short = Timing { action_ms: timing.action_ms.min(CLOSE_MS), ..timing.clone() };
        let out = execute_in(d, &Action::Click { selector: p.close.clone() }, &short, policy).await;
        if out.ok {
            crate::applog::info(format!("Auto Run: closed the {name} before {}", action.kind()));
            notes.push(closed(name));
        } else {
            crate::applog::warn(format!("Auto Run: the {name} was showing and could not be closed: {}", out.detail));
            notes.push(not_closed(name, &out.detail));
        }
    }
    notes
}

/// `notes` added to `out`, in brackets after what it says.
pub fn add_notes(out: &mut ActionOutcome, notes: &[String]) {
    if !notes.is_empty() {
        out.detail.push_str(&format!(" ({})", notes.join("; ")));
    }
}
