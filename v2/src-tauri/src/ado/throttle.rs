//! Global pacing for every Azure DevOps request.
//!
//! ADO's rate limit is per *user*, not per app, so the app competes with
//! the same person's browser tabs, `git fetch`es and anything else signed
//! in as them. Once the account crosses the threshold ADO delays requests
//! org-wide, which is the banner users see. The app can't see that budget,
//! but it can choose to consume less of it - so the pace is a setting.
//!
//! One process-wide minimum interval between requests, not a per-client
//! one: `AdoClient` is constructed per command, so a per-client limiter
//! would let ten concurrent commands each fire immediately.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The longest server-requested delay we will actually sleep. ADO
/// documents delays "up to 30 seconds"; a longer value than that is more
/// likely a bad header than a real instruction, and sleeping on it would
/// look like the app had hung.
const MAX_BACKOFF_SECS: u64 = 30;

/// Minimum gap between any two ADO requests. 0 = unthrottled.
static MIN_INTERVAL_MS: AtomicU64 = AtomicU64::new(DEFAULT_MS);

/// Balanced: noticeably gentler than a burst, still brisk for one user.
const DEFAULT_MS: u64 = 200;

/// The three levels offered in Settings. Anything unrecognised falls back
/// to Balanced rather than accidentally unthrottling.
pub fn interval_for(level: &str) -> u64 {
    match level {
        "full" => 0,
        "gentle" => 800,
        _ => DEFAULT_MS,
    }
}

pub fn set_level(level: &str) {
    MIN_INTERVAL_MS.store(interval_for(level), Ordering::Relaxed);
}

pub fn current_interval_ms() -> u64 {
    MIN_INTERVAL_MS.load(Ordering::Relaxed)
}

fn last_send() -> &'static tokio::sync::Mutex<Option<Instant>> {
    static GATE: OnceLock<tokio::sync::Mutex<Option<Instant>>> = OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// When the server has asked us to hold off until - shared by every
/// request, like the pacer itself.
fn backoff_until() -> &'static std::sync::Mutex<Option<Instant>> {
    static UNTIL: OnceLock<std::sync::Mutex<Option<Instant>>> = OnceLock::new();
    UNTIL.get_or_init(|| std::sync::Mutex::new(None))
}

/// Record a delay Azure DevOps ASKED us to take.
///
/// The important part is that this is not a 429 handler. ADO throttles
/// *before* it rejects: the docs say a delayed request "still returns HTTP
/// 200", carrying `Retry-After` (and `X-RateLimit-Delay`) to say how long
/// it was held and how long to wait next time. Ignoring that until a 429
/// arrives means racing an already-annoyed server at full pace - the exact
/// thing the pacer exists to avoid. Honouring it on every response is what
/// Microsoft's own guidance asks for.
///
/// Takes the LATER of any existing deadline and this one, so overlapping
/// responses cannot shorten a hold that is already in force. Zero and
/// absurd values are ignored - a header saying "wait 0" is not a request
/// to wait, and one saying "wait an hour" is more likely broken than true.
pub fn note_server_delay(secs: u64) {
    if secs == 0 {
        return;
    }
    let capped = secs.min(MAX_BACKOFF_SECS);
    let until = Instant::now() + Duration::from_secs(capped);
    if let Ok(mut slot) = backoff_until().lock() {
        let extend = slot.map(|cur| until > cur).unwrap_or(true);
        if extend {
            *slot = Some(until);
            crate::applog::warn(format!(
                "Azure DevOps asked us to slow down: holding requests for {capped}s                 {}",
                if capped < secs { format!(" (it asked for {secs}s, capped)") } else { String::new() }
            ));
        }
    }
}

/// How long the server-requested hold still has to run, if any.
fn remaining_backoff() -> Option<Duration> {
    let mut slot = backoff_until().lock().ok()?;
    let until = (*slot)?;
    let now = Instant::now();
    if until <= now {
        *slot = None; // expired - stop checking
        return None;
    }
    Some(until - now)
}

/// Test/diagnostic helper: clear any hold in force.
pub fn clear_backoff() {
    if let Ok(mut slot) = backoff_until().lock() {
        *slot = None;
    }
}

/// Waits until the configured gap since the previous request has elapsed.
///
/// The lock is deliberately held across the sleep: that serialises callers
/// into a queue, which is the point - concurrent screens should share one
/// budget, not each get their own. At "full" it returns without locking so
/// the fast path stays free.
pub async fn pace() {
    // A server-requested hold outranks the local pace - including at
    // "full", where the user asked US to go flat out, not the server to
    // tolerate it. Slept before the interval gate so the two add rather
    // than overlap.
    if let Some(hold) = remaining_backoff() {
        tokio::time::sleep(hold).await;
    }
    let want = MIN_INTERVAL_MS.load(Ordering::Relaxed);
    if want == 0 {
        return;
    }
    let want = Duration::from_millis(want);
    let mut last = last_send().lock().await;
    if let Some(prev) = *last {
        let elapsed = prev.elapsed();
        if elapsed < want {
            tokio::time::sleep(want - elapsed).await;
        }
    }
    *last = Some(Instant::now());
}
