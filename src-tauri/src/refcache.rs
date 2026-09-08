//! Disk-backed cache for slow-moving project reference data - today the
//! project's tag names.
//!
//! It lives in Rust, below both consumers, on purpose. The app's own
//! caching is React Query in the webview, which the AI bridge cannot see
//! and which dies on restart; a cache down here means the UI and an AI
//! assistant read the SAME list, and whoever asks first is the only one
//! who ever pays for the request.
//!
//! Everything is kept in one small JSON file so keys can be arbitrary
//! strings, and held in memory so repeat reads never touch the disk.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Tags change when someone types a new one - slow enough that a stale
/// read is harmless, fast enough that a week would annoy.
pub const TAGS_TTL_MS: u64 = 6 * 60 * 60 * 1000;

static DIR: OnceLock<PathBuf> = OnceLock::new();

static MEM: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub values: Vec<String>,
    /// Unix epoch milliseconds of the last successful fetch.
    pub at_ms: u64,
}

pub fn tags_key(org: &str, project: &str) -> String {
    format!("{org}/{project}/tags")
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Point the cache at its storage directory. Called once during app
/// setup; tests call it with a temp directory. Later calls are ignored,
/// so a test can never repoint a running app's cache.
pub fn init(dir: PathBuf) {
    let _ = DIR.set(dir);
}

fn file() -> Option<PathBuf> {
    DIR.get().map(|d| d.join("reference-cache.json"))
}

fn mem() -> &'static Mutex<HashMap<String, Entry>> {
    MEM.get_or_init(|| {
        // First touch loads whatever the last run left behind. A corrupt
        // or absent file simply starts empty - never an error, the cache
        // is an optimization.
        let loaded = file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<HashMap<String, Entry>>(&s).ok())
            .unwrap_or_default();
        Mutex::new(loaded)
    })
}

fn persist(map: &HashMap<String, Entry>) {
    let Some(path) = file() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string(map) {
        let _ = std::fs::write(path, s);
    }
}

/// Whatever is cached, however old. This is what the AI bridge reads: an
/// assistant asking for tags must never trigger a request the app has
/// already made.
pub fn any(key: &str) -> Option<Vec<String>> {
    mem().lock().unwrap().get(key).map(|e| e.values.clone())
}

/// Cached values younger than `ttl_ms`.
pub fn fresh(key: &str, ttl_ms: u64) -> Option<Vec<String>> {
    let map = mem().lock().unwrap();
    let e = map.get(key)?;
    (now_ms().saturating_sub(e.at_ms) < ttl_ms).then(|| e.values.clone())
}

/// Store a freshly fetched list, replacing whatever was there.
pub fn put(key: &str, values: &[String]) {
    let mut map = mem().lock().unwrap();
    map.insert(
        key.to_string(),
        Entry {
            values: values.to_vec(),
            at_ms: now_ms(),
        },
    );
    persist(&map);
}

/// Fold in values the app just learned about locally - tags carried by
/// test cases it created, which provably exist now. Case-insensitively
/// deduplicated, sorted, and it does NOT touch `at_ms`: this is new
/// knowledge, not a refresh, so the next real fetch still happens on
/// schedule. No-ops when nothing is cached yet, so a cold cache is never
/// seeded with a partial list.
pub fn merge(key: &str, extra: &[String]) {
    let mut map = mem().lock().unwrap();
    let Some(entry) = map.get_mut(key) else { return };
    let mut added = false;
    for v in extra {
        let v = v.trim();
        if v.is_empty() || entry.values.iter().any(|e| e.eq_ignore_ascii_case(v)) {
            continue;
        }
        entry.values.push(v.to_string());
        added = true;
    }
    if added {
        entry.values.sort_by_key(|v| v.to_lowercase());
        persist(&map);
    }
}

/// Test-only reset so cases don't leak into each other.
pub fn clear() {
    let mut map = mem().lock().unwrap();
    map.clear();
    persist(&map);
}
