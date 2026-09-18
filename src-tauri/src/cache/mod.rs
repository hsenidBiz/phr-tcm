//! The Rust side's one cache. Anything that needs to remember data
//! between calls - or between launches - uses this instead of a map of
//! its own (tests/cache.rs catches the common form: a map held in a
//! static).
//!
//! It lives below both consumers on purpose: the UI's commands and the AI
//! bridge read the SAME entries, so whoever asks first is the only one
//! who ever pays for the request. The webview has its own cache
//! (src/lib/cache.ts) for what only the screens need; the two are
//! separate processes and share nothing.
//!
//! Two tiers:
//! - durable (`get`/`fresh`/`put`/`update`/`forget`): serde values in one
//!   small JSON file, held in memory so repeat reads never touch the disk;
//! - session (`session_fresh`/`session_put`): values kept exactly as they
//!   are, in memory only, for data too big to write on every change or
//!   with `#[serde(skip)]` fields a round trip would lose.
//!
//! Every key and TTL lives in `keys.rs`. Like the webview cache, it drops
//! everything when a different account signs in (`claim_for`).

pub mod keys;

use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

const FILE: &str = "cache.json";

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    value: serde_json::Value,
    /// Unix epoch milliseconds of the last successful fetch.
    at_ms: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Disk {
    /// Non-reversible tag of the account the entries belong to.
    owner: Option<String>,
    entries: HashMap<String, Entry>,
}

#[derive(Default)]
struct Inner {
    disk: Disk,
    session: HashMap<String, (Instant, Box<dyn Any + Send>)>,
}

pub struct Store {
    file: Option<PathBuf>,
    inner: Mutex<Inner>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// FNV-1a: the identity check never needs the address itself on disk.
/// Collisions only cost a needless wipe.
fn owner_tag(account: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in account.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

impl Store {
    /// Open the cache in `dir`, or memory-only with `None`. A corrupt or
    /// absent file simply starts empty - never an error.
    pub fn open(dir: Option<&Path>) -> Store {
        let store = Store {
            file: dir.map(|d| d.join(FILE)),
            inner: Mutex::new(Inner::default()),
        };
        let Some(dir) = dir else { return store };
        if let Some(disk) = read_json::<Disk>(&dir.join(FILE)) {
            store.lock().disk = disk;
            // A restored backup can drop the legacy files beside an
            // already-existing cache.json (restoring onto a machine that
            // has since migrated). Fold anything not already present in so
            // it is not silently left on disk, unread, forever - but never
            // overwrite a current entry with a stale one.
            if let Some(legacy_disk) = legacy::read(dir) {
                let mut inner = store.lock();
                for (key, entry) in legacy_disk.entries {
                    inner.disk.entries.entry(key).or_insert(entry);
                }
                if store.persist(&inner.disk) {
                    legacy::remove(dir);
                }
            }
        } else if let Some(disk) = legacy::read(dir) {
            let mut inner = store.lock();
            inner.disk = disk;
            if store.persist(&inner.disk) {
                legacy::remove(dir);
            }
        }
        store
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic elsewhere while holding the lock must not take the cache
        // down with it: the map is still structurally sound.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Write to a `.tmp` sibling then rename it over `cache.json`. One file
    /// now holds both tags and suites, so a crash mid-write must not lose
    /// both halves to a half-written `cache.json` - the rename is atomic,
    /// a plain write to the real path is not.
    fn persist(&self, disk: &Disk) -> bool {
        let Some(path) = &self.file else { return false };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(s) = serde_json::to_string(disk) else { return false };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, s).is_err() || std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
        true
    }

    /// Whatever is cached, however old.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let inner = self.lock();
        serde_json::from_value(inner.disk.entries.get(key)?.value.clone()).ok()
    }

    /// The cached value if it is younger than `ttl_ms`.
    pub fn fresh<T: DeserializeOwned>(&self, key: &str, ttl_ms: u64) -> Option<T> {
        let inner = self.lock();
        let entry = inner.disk.entries.get(key)?;
        if now_ms().saturating_sub(entry.at_ms) >= ttl_ms {
            return None;
        }
        serde_json::from_value(entry.value.clone()).ok()
    }

    /// Store a freshly fetched value, replacing whatever was there.
    pub fn put<T: Serialize>(&self, key: &str, value: &T) {
        let Ok(value) = serde_json::to_value(value) else { return };
        let mut inner = self.lock();
        inner.disk.entries.insert(key.to_string(), Entry { value, at_ms: now_ms() });
        self.persist(&inner.disk);
    }

    /// Change a cached value in place. `f` returns whether it changed
    /// anything. Leaves the entry's age alone - new local knowledge is not
    /// a refresh - and no-ops on a cold key, so a partial value is never
    /// mistaken for a fetched one.
    ///
    /// `f` runs while the cache's lock is held: it must not call back into
    /// this cache (directly or through the free functions below), or it
    /// will deadlock on itself.
    pub fn update<T: Serialize + DeserializeOwned>(&self, key: &str, f: impl FnOnce(&mut T) -> bool) {
        let mut inner = self.lock();
        let Some(entry) = inner.disk.entries.get_mut(key) else { return };
        let Ok(mut value) = serde_json::from_value::<T>(entry.value.clone()) else { return };
        if !f(&mut value) {
            return;
        }
        let Ok(json) = serde_json::to_value(&value) else { return };
        entry.value = json;
        self.persist(&inner.disk);
    }

    /// Drop one key from both tiers.
    pub fn forget(&self, key: &str) {
        let mut inner = self.lock();
        inner.session.remove(key);
        if inner.disk.entries.remove(key).is_some() {
            self.persist(&inner.disk);
        }
    }

    /// A session value younger than `ttl`, as it was stored.
    pub fn session_fresh<T: Clone + 'static>(&self, key: &str, ttl: Duration) -> Option<T> {
        let inner = self.lock();
        let (at, value) = inner.session.get(key)?;
        if at.elapsed() >= ttl {
            return None;
        }
        // `**value` is the `dyn Any`: downcasting the Box itself would ask
        // "is this a Box?" and always miss.
        (**value).downcast_ref::<T>().cloned()
    }

    /// Keep a value in memory for this run only.
    pub fn session_put<T: Send + 'static>(&self, key: &str, value: T) {
        self.lock()
            .session
            .insert(key.to_string(), (Instant::now(), Box::new(value)));
    }

    /// Hand the cache to the signed-in account. A different account than
    /// the recorded owner drops everything; an unowned cache (first launch
    /// with this guard, carrying migrated data) is adopted as is. `None` -
    /// an account the sign-in could not name - is not provably the owner,
    /// so it is treated like a different account: everything is dropped and
    /// the cache is left unowned for the next named sign-in to adopt.
    pub fn claim_for(&self, account: Option<&str>) {
        let mut inner = self.lock();
        let Some(account) = account else {
            if inner.disk.owner.is_none() && inner.disk.entries.is_empty() && inner.session.is_empty() {
                return;
            }
            inner.disk.entries.clear();
            inner.session.clear();
            inner.disk.owner = None;
            self.persist(&inner.disk);
            return;
        };
        let tag = owner_tag(account);
        // Compare first, into a plain bool, so no borrow of the owner is
        // alive while the entries are cleared.
        match inner.disk.owner.as_deref().map(|owner| owner == tag) {
            Some(true) => return,
            Some(false) => {
                inner.disk.entries.clear();
                inner.session.clear();
            }
            None => {}
        }
        inner.disk.owner = Some(tag);
        self.persist(&inner.disk);
    }

    /// Drop every entry in both tiers (the owner is kept).
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.disk.entries.clear();
        inner.session.clear();
        self.persist(&inner.disk);
    }
}

/// The two single-purpose files this module replaced. Read once, folded
/// into `cache.json`, then removed - so nobody's tags refetch or suites
/// rescan just because the app updated.
mod legacy {
    use std::collections::HashMap;
    use std::path::Path;

    use super::{keys, now_ms, read_json, Disk, Entry};

    const REFERENCE: &str = "reference-cache.json";
    const SUITES: &str = "suite-cache.json";

    #[derive(serde::Deserialize)]
    struct OldEntry {
        values: Vec<String>,
        at_ms: u64,
    }

    pub fn read(dir: &Path) -> Option<Disk> {
        let reference = read_json::<HashMap<String, OldEntry>>(&dir.join(REFERENCE));
        let suites = read_json::<HashMap<String, serde_json::Value>>(&dir.join(SUITES));
        if reference.is_none() && suites.is_none() {
            return None;
        }
        let mut disk = Disk::default();
        for (old, entry) in reference.unwrap_or_default() {
            // "org/project/tags" and "org/project/assigned-seen"; org and
            // project names cannot contain '/'.
            let key = if let Some(scope) = old.strip_suffix("/tags") {
                scope.split_once('/').map(|(o, p)| keys::tags(o, p))
            } else if let Some(scope) = old.strip_suffix("/assigned-seen") {
                scope.split_once('/').map(|(o, p)| keys::assigned_seen(o, p))
            } else {
                None
            };
            let (Some(key), Ok(value)) = (key, serde_json::to_value(entry.values)) else { continue };
            disk.entries.insert(key, Entry { value, at_ms: entry.at_ms });
        }
        let now = now_ms();
        for (old, value) in suites.unwrap_or_default() {
            // "base_url|org|project|pbi"
            let parts: Vec<&str> = old.split('|').collect();
            let [base, org, project, pbi] = parts.as_slice() else { continue };
            let Ok(pbi) = pbi.parse::<i32>() else { continue };
            disk.entries.insert(keys::suite(base, org, project, pbi), Entry { value, at_ms: now });
        }
        Some(disk)
    }

    pub fn remove(dir: &Path) {
        let _ = std::fs::remove_file(dir.join(REFERENCE));
        let _ = std::fs::remove_file(dir.join(SUITES));
    }
}

static DIR: OnceLock<PathBuf> = OnceLock::new();
static GLOBAL: OnceLock<Store> = OnceLock::new();

/// Point the app's cache at its directory. Called once during setup,
/// before anything reads it; later calls are ignored. Without it (the
/// test binaries) the global cache is memory-only.
pub fn init(dir: PathBuf) {
    let _ = DIR.set(dir);
}

fn global() -> &'static Store {
    GLOBAL.get_or_init(|| Store::open(DIR.get().map(PathBuf::as_path)))
}

pub fn get<T: DeserializeOwned>(key: &str) -> Option<T> {
    global().get(key)
}

pub fn fresh<T: DeserializeOwned>(key: &str, ttl_ms: u64) -> Option<T> {
    global().fresh(key, ttl_ms)
}

pub fn put<T: Serialize>(key: &str, value: &T) {
    global().put(key, value)
}

/// See `Store::update`: `f` runs with the cache locked and must not call
/// back into the cache.
pub fn update<T: Serialize + DeserializeOwned>(key: &str, f: impl FnOnce(&mut T) -> bool) {
    global().update(key, f)
}

pub fn forget(key: &str) {
    global().forget(key)
}

pub fn session_fresh<T: Clone + 'static>(key: &str, ttl: Duration) -> Option<T> {
    global().session_fresh(key, ttl)
}

pub fn session_put<T: Send + 'static>(key: &str, value: T) {
    global().session_put(key, value)
}

pub fn claim_for(account: Option<&str>) {
    global().claim_for(account)
}

pub fn clear() {
    global().clear()
}
