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

/// Waits until the configured gap since the previous request has elapsed.
///
/// The lock is deliberately held across the sleep: that serialises callers
/// into a queue, which is the point - concurrent screens should share one
/// budget, not each get their own. At "full" it returns without locking so
/// the fast path stays free.
pub async fn pace() {
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
