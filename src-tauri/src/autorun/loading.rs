//! The project's loading indicators (`SignInRecipe::wait_while_shown`),
//! waited out by the runner right before an action that touches or checks
//! the page.
//!
//! One look per indicator, no waiting, when none is up - so a page that is
//! not loading costs one look per action. While one is VISIBLE (an
//! indicator left in the page hidden, as PeoplesHR's `#ehrmLoader` is once
//! a page has loaded, is not loading), the runner polls until it goes, up
//! to the entry's `max_ms`, and only then does the action run and start its
//! own timeout. Still up at `max_ms`, the action fails with
//! `still_loading`. A wait of more than `NOTED_MS` is said on the action
//! ("waited 12.3 s for page loader").
//!
//! Called from the runner's one place for a plain action (`runner::plain`)
//! and from the start of a `when_visible` or `repeat_while_visible` (whose
//! own look would read a covered target as not there), which every
//! unattended run, watched run, try, discovery and replay goes through.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::recipe::LoadingIndicator;
use crate::browser::actions::{shown_now, Action};
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;

/// How long the runner waits for an indicator whose `max_ms` is left out.
pub const DEFAULT_MAX_MS: u32 = 90_000;
/// The shortest `max_ms` a recipe may give.
pub const MIN_MAX_MS: u32 = 1_000;
/// The longest `max_ms` a recipe may give.
pub const MAX_MAX_MS: u32 = 180_000;
/// A wait longer than this is said on the action.
pub const NOTED_MS: u64 = 1_000;
/// The least time between two looks while an indicator is up: a spinner
/// that stays for a minute is not looked at every 40 ms.
const MIN_POLL_MS: u64 = 100;

/// Whether `action` touches or checks the page, so a loading page would
/// cover it or answer it wrongly. Navigation, tab actions, a sign-in, the
/// runner's own trip to an area and the network checks go elsewhere or
/// wait in their own way.
pub fn checks_for_loading(action: &Action) -> bool {
    matches!(
        action,
        Action::Click { .. }
            | Action::Fill { .. }
            | Action::Drag { .. }
            | Action::Upload { .. }
            | Action::PressKey { .. }
            | Action::WaitFor { .. }
            | Action::CheckText { .. }
            | Action::CheckUrl { .. }
            | Action::ExpectVisible { .. }
            | Action::ExpectHidden { .. }
            | Action::ExpectText { .. }
            | Action::ExpectContainsText { .. }
            | Action::ExpectCount { .. }
            | Action::ExpectAttribute { .. }
            | Action::ExpectStyle { .. }
            | Action::ExpectEnabled { .. }
            | Action::ExpectDisabled { .. }
            | Action::ExpectChecked { .. }
            | Action::ExpectFocused { .. }
            | Action::ExpectRow { .. }
            | Action::ExpectNoRow { .. }
            | Action::ExpectSorted { .. }
            | Action::ExpectRowCount { .. }
            | Action::ExpectEach { .. }
            | Action::ExpectOrder { .. }
            | Action::ExpectPosition { .. }
            | Action::WhenVisible { .. }
            | Action::RepeatWhileVisible { .. }
    )
}

/// `ms` as seconds, to a tenth, with no trailing ".0".
fn seconds(ms: u64) -> String {
    let tenths = (ms + 50) / 100;
    if tenths % 10 == 0 {
        format!("{}", tenths / 10)
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

/// The failure of an action whose page was still loading at `max_ms`.
pub fn still_loading(name: &str, max_ms: u32) -> String {
    format!("the page was still loading ({name}) after {} s", seconds(max_ms as u64))
}

/// Said on an action after the runner waited `ms` for `name` to go.
pub fn waited(name: &str, ms: u64) -> String {
    format!("waited {} s for {name}", seconds(ms))
}

/// How a wait ended, when it did not simply let the action run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotReady {
    /// An indicator was still up at its `max_ms`: the action's failure.
    StillLoading(String),
    /// The run's Stop was set while waiting.
    Stopped,
}

/// Before `action`: each listed indicator that is visible right now is
/// waited out, in order. `Ok` holds the notes to add to the action's
/// outcome - none when nothing was up, or nothing for long. An indicator the
/// action itself aims at is left to the action (a script may check the
/// spinner). A look the browser does not answer reads as not up: the action
/// goes ahead and reports the browser itself. `cancel` is the run's Stop,
/// heard at every look.
pub async fn wait_out<D: Driver>(
    d: &mut D,
    loaders: &[LoadingIndicator],
    action: &Action,
    timing: &Timing,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<String>, NotReady> {
    if loaders.is_empty() || !checks_for_loading(action) {
        return Ok(Vec::new());
    }
    let aimed_at = action.targets();
    let poll = Duration::from_millis(timing.poll_ms.max(MIN_POLL_MS));
    let mut notes = Vec::new();
    for l in loaders {
        if aimed_at.iter().any(|t| **t == l.shown) {
            continue;
        }
        let name = l.name.trim();
        let limit = Duration::from_millis(l.limit_ms() as u64);
        let began = Instant::now();
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                return Err(NotReady::Stopped);
            }
            if !matches!(shown_now(d, &l.shown).await, Ok(Some(true))) {
                break;
            }
            if began.elapsed() >= limit {
                crate::applog::warn(format!(
                    "Auto Run: the {name} was still up after {} s before {}",
                    seconds(limit.as_millis() as u64),
                    action.kind()
                ));
                return Err(NotReady::StillLoading(still_loading(name, l.limit_ms())));
            }
            tokio::time::sleep(poll.min(limit.saturating_sub(began.elapsed())).max(Duration::from_millis(1))).await;
        }
        let ms = began.elapsed().as_millis() as u64;
        if ms > NOTED_MS {
            crate::applog::info(format!("Auto Run: waited {} s for the {name} before {}", seconds(ms), action.kind()));
            notes.push(waited(name, ms));
        }
    }
    Ok(notes)
}
