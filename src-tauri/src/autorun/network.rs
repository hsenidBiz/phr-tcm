//! The unattended run's view of the company network.
//!
//! The environment's database server sits behind the VPN, so "does it
//! answer a TCP connect?" is how a run tells a dropped VPN from a real
//! failure. This module finds that server (`db_target`), knocks on it
//! (`answers`) and, further down, waits for it to come back. Nothing here
//! logs in or runs a query, and nothing here puts a host or port anywhere
//! but into the connect itself.

use std::time::Duration;

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
