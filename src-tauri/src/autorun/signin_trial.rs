//! A sign-in trial in progress: which environment it is being tried against,
//! so the Assistants card can say "Trying a sign-in". A trial runs in the
//! check browser, not in an assistant's slot, so it is reported on its own,
//! and there is no Stop for it: nothing cancels a trial safely today.
//!
//! The view carries the environment's name only - never a site address, a
//! login or a path.

use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct SignInTrial {
    pub environment: String,
}

static RUNNING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held for as long as the trial runs; dropping it clears the mark.
pub struct TrialMark {
    environment: String,
}

fn lock() -> std::sync::MutexGuard<'static, Vec<String>> {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn start(environment: &str) -> TrialMark {
    lock().push(environment.to_string());
    TrialMark { environment: environment.to_string() }
}

impl Drop for TrialMark {
    fn drop(&mut self) {
        let mut running = lock();
        if let Some(at) = running.iter().position(|e| *e == self.environment) {
            running.remove(at);
        }
    }
}

/// The trial running now, if any (the latest started when several overlap).
pub fn current() -> Option<SignInTrial> {
    lock().last().map(|environment| SignInTrial { environment: environment.clone() })
}
