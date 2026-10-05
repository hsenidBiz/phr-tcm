//! An unattended run waiting at a reset point for the person.
//!
//! At a reset point (design §3) the run tells the app (`AutorunResetNeeded`,
//! shown as the Reset needed panel) and waits for Continue or Stop. One
//! run goes at a time, so one reset waits at a time; it is known by its
//! run's id, and only that id is answered. The wait has no time limit. It
//! ends with the person's answer, with the run's own Stop (`cancel`), or
//! with the app closing (`stop_for_exit`), and the last two both answer
//! Stop, so the run is always saved as stopped rather than left waiting.

use super::plan::Reset;
use super::replay::ResetGate;
use crate::events::AutorunResetNeeded;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;

/// Said to an answer for a run that is not waiting at a reset point:
/// unknown, answered already, or stopped.
pub const NOT_WAITING: &str = "that run is not waiting at a reset point";

/// How often a waiting reset looks for the run's own Stop.
const STOP_POLL: Duration = Duration::from_millis(50);

struct Pending {
    run_id: String,
    /// What the panel shows, kept so a screen opened after the event was
    /// sent can find the pause again (`waiting`).
    needed: AutorunResetNeeded,
    answer: oneshot::Sender<bool>,
}

/// The one reset waiting for the person, if any.
pub struct ResetWaits {
    pending: Mutex<Option<Pending>>,
}

impl Default for ResetWaits {
    fn default() -> Self {
        Self::new()
    }
}

/// Clears the waiting reset when its wait ends, however it ends: a dropped
/// wait must not leave an answer that reaches nothing.
struct Ending<'a> {
    waits: &'a ResetWaits,
    run_id: String,
}

impl Drop for Ending<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.waits.pending.lock() {
            if slot.as_ref().is_some_and(|p| p.run_id == self.run_id) {
                *slot = None;
            }
        }
    }
}

/// A reset in place, waiting for its answer. Dropping it clears it.
pub struct Waiting<'a> {
    answer: oneshot::Receiver<bool>,
    _ending: Ending<'a>,
}

impl Waiting<'_> {
    /// The answer: `true` is Continue, `false` is Stop - and so is the
    /// run's own Stop (`cancel`), an exit, or a reset replaced.
    pub async fn answered(self, cancel: &AtomicBool) -> bool {
        let Waiting { answer, _ending } = self;
        tokio::select! {
            a = answer => a.unwrap_or(false),
            () = stop_asked(cancel) => false,
        }
    }
}

/// Returns once `cancel` is set, looking every `STOP_POLL`.
async fn stop_asked(cancel: &AtomicBool) {
    while !cancel.load(Ordering::SeqCst) {
        tokio::time::sleep(STOP_POLL).await;
    }
}

impl ResetWaits {
    pub const fn new() -> Self {
        ResetWaits { pending: Mutex::new(None) }
    }

    /// Put run `run_id`'s reset in place to be answered. From here an
    /// answer reaches it, even before anything awaits it. One run goes at a
    /// time, so anything already here belongs to a run that is gone: it is
    /// replaced, and its wait reads that as Stop.
    pub fn begin(&self, run_id: &str, needed: AutorunResetNeeded) -> Waiting<'_> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut slot) = self.pending.lock() {
            *slot = Some(Pending { run_id: run_id.to_string(), needed, answer: tx });
        }
        // A poisoned lock drops `tx` here, and the wait reads Stop.
        Waiting { answer: rx, _ending: Ending { waits: self, run_id: run_id.to_string() } }
    }

    /// Wait for the answer to run `run_id`'s reset: `true` is Continue,
    /// `false` is Stop. The run's own Stop (`cancel`) answers Stop, and so
    /// does `stop_for_exit`. No time limit.
    pub async fn wait(&self, needed: AutorunResetNeeded, cancel: &AtomicBool) -> bool {
        let run_id = needed.run_id.clone();
        self.begin(&run_id, needed).answered(cancel).await
    }

    /// The person's answer for run `run_id`. Refused with `NOT_WAITING` for
    /// any run that is not the one waiting right now.
    pub fn answer(&self, run_id: &str, continue_run: bool) -> Result<(), String> {
        let pending = {
            let mut slot = self.pending.lock().map_err(|_| NOT_WAITING.to_string())?;
            match slot.as_ref() {
                Some(p) if p.run_id == run_id => slot.take(),
                _ => None,
            }
        };
        let pending = pending.ok_or_else(|| NOT_WAITING.to_string())?;
        pending.answer.send(continue_run).map_err(|_| NOT_WAITING.to_string())
    }

    /// The reset point a run is waiting at right now, as the panel shows
    /// it, if any.
    pub fn waiting(&self) -> Option<AutorunResetNeeded> {
        self.pending.lock().ok().and_then(|s| s.as_ref().map(|p| p.needed.clone()))
    }

    /// The app is closing: a run waiting at a reset point is answered Stop,
    /// so it records the rest as not run and saves itself as stopped.
    /// Returns whether one was waiting.
    pub fn stop_for_exit(&self) -> bool {
        let pending = self.pending.lock().ok().and_then(|mut s| s.take());
        match pending {
            Some(p) => {
                let _ = p.answer.send(false);
                true
            }
            None => false,
        }
    }
}

/// The app's one registry, which the run waits in, the answer command
/// answers and the exit hook stops.
static WAITS: ResetWaits = ResetWaits::new();

pub fn waits() -> &'static ResetWaits {
    &WAITS
}

/// The gate an unattended run started from the app pauses at: it tells the
/// app (`notify`, which emits `AutorunResetNeeded`) and waits in `waits`.
pub struct AppGate<'a> {
    pub waits: &'a ResetWaits,
    pub run_id: String,
    pub notify: &'a (dyn Fn(&AutorunResetNeeded) + Send + Sync),
}

impl ResetGate for AppGate<'_> {
    async fn wait(&mut self, reset: &Reset, remaining: &[i32], cancel: &AtomicBool) -> bool {
        let needed = AutorunResetNeeded {
            run_id: self.run_id.clone(),
            before_case_id: reset.before_case_id,
            names: reset.names.clone(),
            changed_by: reset.changed_by.clone(),
            remaining: remaining.to_vec(),
        };
        // In place before the panel shows, so its answer always lands.
        let waiting = self.waits.begin(&self.run_id, needed.clone());
        (self.notify)(&needed);
        waiting.answered(cancel).await
    }
}
