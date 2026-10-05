//! The person's Allow before an assistant replays a must-not-save script.
//!
//! An assistant's replay of a script marked must not save first asks the
//! person in the app (`AutorunReplayRequest`, shown as a modal) and waits
//! for Allow or Deny. Nothing opens, signs in or runs before Allow. One
//! request waits at a time; it has an id, and only that id is answered, so
//! a late Allow on a request that already timed out starts nothing.

use crate::events::AutorunReplayRequest;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;

/// How long a request waits for the person.
pub const WAIT: Duration = Duration::from_secs(120);

/// Said to a second request while one waits.
pub const ALREADY_WAITING: &str = "a replay request is already waiting for the person";
/// Said when the person pressed Deny.
pub const DECLINED: &str = "the person declined the replay";
/// Said when the person did not answer in time.
pub const NO_ANSWER: &str = "the person did not answer within 2 minutes";
/// Said to an answer for a request that is not waiting: unknown, answered
/// already, or timed out.
pub const NOT_WAITING: &str = "that replay request is no longer waiting";

/// What the app is told: a request to show, or the end of one (answered,
/// timed out, or its caller gone), so the modal closes.
pub enum Notice<'a> {
    Asked(&'a AutorunReplayRequest),
    Ended(&'a str),
}

struct Pending {
    ask: AutorunReplayRequest,
    answer: oneshot::Sender<bool>,
}

/// The one request waiting for the person, if any.
pub struct Asks {
    pending: Mutex<Option<Pending>>,
    next: AtomicU64,
}

impl Default for Asks {
    fn default() -> Self {
        Self::new()
    }
}

/// Clears the request when its wait ends, however it ends, and tells the
/// app so: a dropped wait (the assistant's connection went away) must not
/// leave a modal whose answer reaches nothing.
struct Ending<'a> {
    asks: &'a Asks,
    id: String,
    notify: &'a (dyn Fn(Notice<'_>) + Send + Sync),
}

impl Drop for Ending<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.asks.pending.lock() {
            if slot.as_ref().is_some_and(|p| p.ask.id == self.id) {
                *slot = None;
            }
        }
        (self.notify)(Notice::Ended(&self.id));
    }
}

impl Asks {
    pub const fn new() -> Self {
        Asks { pending: Mutex::new(None), next: AtomicU64::new(1) }
    }

    /// Ask the person whether case `case_id` may be replayed to `step`, and
    /// wait up to `wait` for the answer. `notify` shows the request and
    /// says when it ended. `Ok` is Allow; `Err` is the sentence for Deny, no
    /// answer in time, or another request already waiting.
    pub async fn ask(
        &self,
        case_id: i32,
        title: &str,
        step: i32,
        wait: Duration,
        notify: &(dyn Fn(Notice<'_>) + Send + Sync),
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let ask = {
            let mut slot = self.pending.lock().map_err(|_| ALREADY_WAITING.to_string())?;
            if slot.is_some() {
                return Err(ALREADY_WAITING.to_string());
            }
            let ask = AutorunReplayRequest { id: self.new_id(), case_id, title: title.to_string(), step };
            *slot = Some(Pending { ask: ask.clone(), answer: tx });
            ask
        };
        let _ending = Ending { asks: self, id: ask.id.clone(), notify };
        notify(Notice::Asked(&ask));
        match tokio::time::timeout(wait, rx).await {
            Ok(Ok(true)) => Ok(()),
            Ok(Ok(false)) => Err(DECLINED.to_string()),
            Ok(Err(_)) | Err(_) => Err(NO_ANSWER.to_string()),
        }
    }

    /// The person's answer to request `id`. Refused with `NOT_WAITING` for
    /// any id that is not the one waiting right now.
    pub fn answer(&self, id: &str, allow: bool) -> Result<(), String> {
        let pending = {
            let mut slot = self.pending.lock().map_err(|_| NOT_WAITING.to_string())?;
            match slot.as_ref() {
                Some(p) if p.ask.id == id => slot.take(),
                _ => None,
            }
        };
        let pending = pending.ok_or_else(|| NOT_WAITING.to_string())?;
        // The wait that gave up a moment ago no longer hears it.
        pending.answer.send(allow).map_err(|_| NOT_WAITING.to_string())
    }

    /// The request waiting right now, if any.
    pub fn waiting(&self) -> Option<AutorunReplayRequest> {
        self.pending.lock().ok().and_then(|s| s.as_ref().map(|p| p.ask.clone()))
    }

    /// An id no earlier request had, and not one anybody could guess from
    /// the last: a counter beside a random part.
    fn new_id(&self) -> String {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        format!("replay-{n}-{:016x}", rand::random::<u64>())
    }
}

/// The app's one registry, which the bridge asks through and the answer
/// command answers.
static ASKS: Asks = Asks::new();

pub fn asks() -> &'static Asks {
    &ASKS
}
