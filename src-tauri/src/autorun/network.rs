//! The unattended run's view of the company network.
//!
//! The environment's database server sits behind the VPN, so "does it
//! answer a TCP connect?" is how a run tells a dropped VPN from a real
//! failure. This module finds that server (`db_target`), knocks on it
//! (`answers`) and, further down, waits for it to come back. Nothing here
//! logs in or runs a query, and nothing here puts a host or port anywhere
//! but into the connect itself.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long one knock may take before the server counts as silent.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How often a paused run knocks again.
pub const RECHECK: Duration = Duration::from_secs(5);
/// How long one pause lasts before the run gives up.
pub const GIVE_UP: Duration = Duration::from_secs(30 * 60);
/// How many times one case is re-run after a network pause.
pub const MAX_NETWORK_RERUNS: u32 = 3;

const DEFAULT_PORT: u16 = 1433;

/// The host and port to knock on for a saved database's `server` and
/// `port`. The port is the saved one, else a `host,port` in `server`, else
/// 1433; a `tcp:` prefix is dropped. A named instance (`host\instance`)
/// with no port anywhere has no knowable port, so it gives `None`, as does
/// a blank server.
pub fn target_of(server: &str, port: Option<u16>) -> Option<(String, u16)> {
    let s = server.trim();
    let s = s.strip_prefix("tcp:").unwrap_or(s).trim();
    let (host, inline) = match s.split_once(',') {
        Some((h, p)) => (h.trim(), p.trim().parse::<u16>().ok()),
        None => (s, None),
    };
    let (host, instance) = match host.split_once('\\') {
        Some((h, _)) => (h.trim(), true),
        None => (host, false),
    };
    if host.is_empty() {
        return None;
    }
    match port.or(inline) {
        Some(p) => Some((host.to_string(), p)),
        None if instance => None,
        None => Some((host.to_string(), DEFAULT_PORT)),
    }
}

/// The host and port of database `db_id`, or `None` for a blank or unknown
/// id or one with no knowable port.
pub fn db_target(store: &dyn crate::db::SecretStore, db_id: &str) -> Option<(String, u16)> {
    if db_id.trim().is_empty() {
        return None;
    }
    let d = crate::db::credentials::databases(store).into_iter().find(|d| d.id == db_id)?;
    target_of(&d.server, d.port)
}

/// Whether a TCP connection to `host:port` succeeds within `timeout`.
pub async fn answers(host: &str, port: u16, timeout: Duration) -> bool {
    matches!(
        tokio::time::timeout(timeout, tokio::net::TcpStream::connect((host, port))).await,
        Ok(Ok(_))
    )
}

/// What one check of the network found, for the run's record of a case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Reachable,
    Unreachable,
    /// Nothing to check: no database, or none with a knowable port.
    Unchecked,
}

/// How a wait for the network ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitEnd {
    Resumed,
    Stopped,
    GaveUp,
}

impl WaitEnd {
    pub fn outcome(self) -> &'static str {
        match self {
            WaitEnd::Resumed => "resumed",
            WaitEnd::Stopped => "stopped",
            WaitEnd::GaveUp => "gave_up",
        }
    }
}

/// How often a wait looks for a Stop.
const STOP_POLL: Duration = Duration::from_millis(50);

/// Sleep for `d`, or until `stop` or `cancel` is set; true when one was.
async fn stopped_within(d: Duration, stop: &AtomicBool, cancel: &AtomicBool) -> bool {
    let end = Instant::now() + d;
    loop {
        if stop.load(Ordering::SeqCst) || cancel.load(Ordering::SeqCst) {
            return true;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        tokio::time::sleep(left.min(STOP_POLL)).await;
    }
}

/// Wait for `check` to come true: once at once, then every `recheck`.
/// `Stopped` as soon as `stop` or `cancel` is set (looked for every 50 ms),
/// `GaveUp` once `limit` has passed.
pub async fn wait_until<F, Fut>(
    mut check: F,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    recheck: Duration,
    limit: Duration,
) -> WaitEnd
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let started = Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) || cancel.load(Ordering::SeqCst) {
            return WaitEnd::Stopped;
        }
        if check().await {
            return WaitEnd::Resumed;
        }
        let left = limit.saturating_sub(started.elapsed());
        if left.is_zero() {
            return WaitEnd::GaveUp;
        }
        if stopped_within(recheck.min(left), stop, cancel).await {
            return WaitEnd::Stopped;
        }
        if started.elapsed() >= limit {
            return WaitEnd::GaveUp;
        }
    }
}

/// What the app shows of a run waiting for the network.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct WaitView {
    pub run_id: String,
    pub before_case_id: i32,
    pub waited_s: u32,
    pub give_up_in_s: u32,
}

struct Active {
    run_id: String,
    before_case_id: i32,
    stop: Arc<AtomicBool>,
    started: Instant,
    limit: Duration,
}

/// The one run waiting for the network, if any. One run goes at a time.
pub struct NetworkWaits {
    slot: Mutex<Option<Active>>,
}

impl Default for NetworkWaits {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkWaits {
    pub const fn new() -> Self {
        NetworkWaits { slot: Mutex::new(None) }
    }

    /// Put run `run_id`'s wait in place, before case `before_case_id`.
    /// Anything already here belongs to a run that is gone and is replaced.
    /// The flag returned is the wait's stop flag.
    pub fn begin(&self, run_id: &str, before_case_id: i32, limit: Duration) -> Arc<AtomicBool> {
        let stop = Arc::new(AtomicBool::new(false));
        if let Ok(mut s) = self.slot.lock() {
            *s = Some(Active {
                run_id: run_id.to_string(),
                before_case_id,
                stop: stop.clone(),
                started: Instant::now(),
                limit,
            });
        }
        stop
    }

    /// The wait is over: clear it, if it is still run `run_id`'s.
    pub fn end(&self, run_id: &str) {
        if let Ok(mut s) = self.slot.lock() {
            if s.as_ref().is_some_and(|a| a.run_id == run_id) {
                *s = None;
            }
        }
    }

    /// Stop run `run_id`'s wait. False when it is not the one waiting.
    pub fn stop(&self, run_id: &str) -> bool {
        match self.slot.lock() {
            Ok(s) => match s.as_ref() {
                Some(a) if a.run_id == run_id => {
                    a.stop.store(true, Ordering::SeqCst);
                    true
                }
                _ => false,
            },
            Err(_) => false,
        }
    }

    /// The app is closing: stop whatever waits. Whether one was waiting.
    pub fn stop_for_exit(&self) -> bool {
        match self.slot.lock() {
            Ok(s) => match s.as_ref() {
                Some(a) => {
                    a.stop.store(true, Ordering::SeqCst);
                    true
                }
                None => false,
            },
            Err(_) => false,
        }
    }

    pub fn current(&self) -> Option<WaitView> {
        let s = self.slot.lock().ok()?;
        let a = s.as_ref()?;
        let waited = a.started.elapsed();
        Some(WaitView {
            run_id: a.run_id.clone(),
            before_case_id: a.before_case_id,
            waited_s: waited.as_secs().min(u64::from(u32::MAX)) as u32,
            give_up_in_s: a.limit.saturating_sub(waited).as_secs().min(u64::from(u32::MAX)) as u32,
        })
    }
}

static WAITS: NetworkWaits = NetworkWaits::new();

/// The app's one registry, which the run waits in and the exit hook stops.
pub fn waits() -> &'static NetworkWaits {
    &WAITS
}
