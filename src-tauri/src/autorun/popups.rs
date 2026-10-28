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

use super::recipe::{ClosePopup, SignInRecipe};
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

/// Right after a discovery lands: each of the recipe's pop-ups that is up -
/// or comes up within `within_ms`, since a banner often appears a moment
/// after the page - has its close control clicked, so the first action is
/// never covered. Looks at the timing's `poll_ms` rhythm; one look at least,
/// so `0` is a single look. Only a listed `close` selector is ever clicked,
/// each pop-up at most once. How many it closed. Returns early once every
/// listed pop-up has been closed.
pub async fn close_shown_within<D: Driver>(d: &mut D, recipe: &SignInRecipe, within_ms: u64, timing: &Timing) -> usize {
    let popups = &recipe.close_when_shown;
    if popups.is_empty() {
        return 0;
    }
    let policy = Policy::only(recipe.origins());
    let short = Timing { action_ms: timing.action_ms.min(CLOSE_MS), ..timing.clone() };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(within_ms);
    let mut done = vec![false; popups.len()];
    let mut closed = 0;
    loop {
        for (i, p) in popups.iter().enumerate() {
            if done[i] || !matches!(shown_now(d, &p.shown).await, Ok(Some(true))) {
                continue;
            }
            let name = p.name.trim();
            let out = execute_in(d, &Action::Click { selector: p.close.clone() }, &short, &policy).await;
            // Tried once either way: a close that did not work is not retried.
            done[i] = true;
            if out.ok {
                closed += 1;
                crate::applog::info(format!("Auto Run: closed the {name} before the discovery's first look"));
            } else {
                crate::applog::warn(format!("Auto Run: the {name} was showing and could not be closed: {}", out.detail));
            }
        }
        if done.iter().all(|x| *x) || std::time::Instant::now() >= deadline {
            break;
        }
        d.idle(std::time::Duration::from_millis(timing.poll_ms.max(1))).await;
    }
    closed
}
