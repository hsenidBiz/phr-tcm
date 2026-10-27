//! The Rust side's one cache. Anything that needs to remember data
//! between calls - or between launches - uses this instead of a map of
//! its own (tests/suite/cache.rs catches the common form: a map held in a
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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::JoinHandle;
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
    /// Bumped on every snapshot taken for the disk, so writes that finish
    /// out of order never put an older state on disk over a newer one.
    generation: u64,
}

/// How long the writer waits after the first change before saving, so a
/// burst of changes (a sweep caching dozens of suites) is one write.
const WRITE_WINDOW: Duration = Duration::from_millis(300);

#[derive(Default)]
struct Wake {
    /// A change has not been written yet.
    dirty: bool,
    /// The store is being dropped: write what is pending, then stop.
    closed: bool,
    /// The writer has taken a change and not finished writing it.
    in_flight: bool,
}

/// What the store and its writer thread share.
struct Shared {
    file: Option<PathBuf>,
    inner: Mutex<Inner>,
    /// The generation last written to disk. Also serialises the writes
    /// themselves (one `.tmp` file), without holding the data lock.
    written: Mutex<u64>,
    /// Successful writes of the file, for tests and diagnostics.
    writes: AtomicU64,
    wake: Mutex<Wake>,
    cv: Condvar,
}

/// Changes land in memory at once; the file is written behind them by one
/// thread per on-disk store, at most once per `WRITE_WINDOW`, so no caller
/// waits on the disk. A sign-in claim and `clear` are written before they
/// return - a wipe must not wait - and so is anything pending when the
/// store is dropped or `flush` is called (the app does at exit). A crash
/// can lose the last window of changes: it is a cache, they refetch.
pub struct Store {
    shared: Arc<Shared>,
    writer: Option<JoinHandle<()>>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Marks the cache as belonging to a sign-in that could not name its
/// account. `owner_tag` always returns exactly 8 lowercase hex digits
/// (`{h:08x}` of a `u32`); this is 7 characters and not hex-only, so no
/// real account can ever collide with it. Recorded (not left as `None`)
/// so a later NAMED sign-in takes the different-owner branch and wipes,
/// rather than reading `None` as "unowned, adopt me".
const UNNAMED_OWNER: &str = "unnamed";

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

/// Wiki page bodies are the one kind of entry that grows with use - one per
/// page ever opened in a spec pane. Drop those not fetched for a week, then
/// keep the newest `WIKI_PAGE_MAX`. Other keys are never touched here.
fn evict_wiki_pages(entries: &mut HashMap<String, Entry>, now: u64) {
    entries.retain(|k, e| {
        !k.starts_with(keys::WIKI_PAGE_PREFIX) || now.saturating_sub(e.at_ms) < keys::WIKI_PAGE_KEEP_MS
    });
    let mut wiki: Vec<(u64, String)> = entries
        .iter()
        .filter(|(k, _)| k.starts_with(keys::WIKI_PAGE_PREFIX))
        .map(|(k, e)| (e.at_ms, k.clone()))
        .collect();
    if wiki.len() <= keys::WIKI_PAGE_MAX {
        return;
    }
    wiki.sort();
    let excess = wiki.len() - keys::WIKI_PAGE_MAX;
    for (_, k) in wiki.into_iter().take(excess) {
        entries.remove(&k);
    }
}

/// A Playwright listing is kept for one clone at a time: putting `just_put`
/// drops every other listing. Other keys are never touched here.
fn evict_pw_lists(entries: &mut HashMap<String, Entry>, just_put: &str) {
    if just_put.starts_with(keys::PW_LIST_PREFIX) {
        entries.retain(|k, _| !k.starts_with(keys::PW_LIST_PREFIX) || k == just_put);
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic elsewhere while holding the lock must not take the cache
        // down with it: the map is still structurally sound.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wake(&self) -> MutexGuard<'_, Wake> {
        self.wake.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The disk half, serialised under the data lock, with its generation.
    /// None for a memory-only store.
    fn snapshot(&self, inner: &mut Inner) -> Option<(u64, String)> {
        self.file.as_ref()?;
        inner.generation += 1;
        let s = serde_json::to_string(&inner.disk).ok()?;
        Some((inner.generation, s))
    }

    /// Write a snapshot AFTER the data lock is released - a slow disk must
    /// not stall every cache read. Through a `.tmp` sibling renamed over
    /// `cache.json`, so a crash mid-write never leaves half a file. A
    /// snapshot older than what is already on disk is skipped.
    fn write(&self, snap: Option<(u64, String)>) -> bool {
        let (Some((generation, s)), Some(path)) = (snap, &self.file) else { return false };
        let mut written = self.written.lock().unwrap_or_else(PoisonError::into_inner);
        if *written >= generation {
            return true;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, s).is_err() || std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
        *written = generation;
        self.writes.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Snapshot the current state and write it now.
    fn write_now(&self) -> bool {
        let snap = self.snapshot(&mut self.lock());
        self.write(snap)
    }

    /// Note a change for the writer thread. The caller must not hold the
    /// data lock (the writer takes it to snapshot).
    fn schedule(&self) {
        if self.file.is_none() {
            return;
        }
        self.wake().dirty = true;
        // All, not one: a `flush` may be waiting on the same condvar.
        self.cv.notify_all();
    }

    /// Write now if a change is pending, after any write the writer thread
    /// has already started - so when this returns, every change made before
    /// it is on disk (the app exits right after).
    fn flush(&self) {
        let mut wake = self.wake();
        while wake.in_flight {
            wake = self.cv.wait(wake).unwrap_or_else(PoisonError::into_inner);
        }
        let pending = std::mem::take(&mut wake.dirty);
        drop(wake);
        if pending {
            self.write_now();
        }
    }

    /// The writer thread: wait for a change, let the window collect any
    /// that follow, write once. Stops after writing what is pending once
    /// the store is closed.
    fn run_writer(&self) {
        let mut wake = self.wake();
        loop {
            while !wake.dirty && !wake.closed {
                wake = self.cv.wait(wake).unwrap_or_else(PoisonError::into_inner);
            }
            if !wake.dirty {
                return; // closed, nothing pending
            }
            // Collect the burst; a close cuts the wait short.
            wake = self
                .cv
                .wait_timeout_while(wake, WRITE_WINDOW, |w| !w.closed)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            if !wake.dirty {
                continue; // a `flush` wrote it during the window
            }
            // Cleared BEFORE the snapshot: a change after this point marks
            // the store dirty again and gets a write of its own.
            wake.dirty = false;
            wake.in_flight = true;
            drop(wake);
            {
                // Clears `in_flight` however the write ends, a panic
                // included, so a waiting `flush` is never stranded.
                let _done = InFlight(self);
                self.write_now();
            }
            wake = self.wake();
        }
    }
}

struct InFlight<'a>(&'a Shared);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.wake().in_flight = false;
        self.0.cv.notify_all();
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let Some(writer) = self.writer.take() else { return };
        self.shared.wake().closed = true;
        self.shared.cv.notify_all();
        let _ = writer.join();
        // The writer exits only with nothing pending, unless it panicked.
        self.shared.flush();
    }
}

impl Store {
    /// Open the cache in `dir`, or memory-only with `None`. A corrupt or
    /// absent file simply starts empty - never an error.
    pub fn open(dir: Option<&Path>) -> Store {
        let shared = Arc::new(Shared {
            file: dir.map(|d| d.join(FILE)),
            inner: Mutex::new(Inner::default()),
            written: Mutex::new(0),
            writes: AtomicU64::new(0),
            wake: Mutex::new(Wake::default()),
            cv: Condvar::new(),
        });
        let Some(dir) = dir else { return Store { shared, writer: None } };
        if let Some(disk) = read_json::<Disk>(&dir.join(FILE)) {
            shared.lock().disk = disk;
            // A restored backup can drop the legacy files beside an
            // already-existing cache.json (restoring onto a machine that
            // has since migrated). Fold anything not already present in so
            // it is not silently left on disk, unread, forever - but never
            // overwrite a current entry with a stale one.
            if let Some(legacy_disk) = legacy::read(dir) {
                {
                    let mut inner = shared.lock();
                    for (key, entry) in legacy_disk.entries {
                        inner.disk.entries.entry(key).or_insert(entry);
                    }
                }
                if shared.write_now() {
                    legacy::remove(dir);
                }
            }
        } else if let Some(disk) = legacy::read(dir) {
            shared.lock().disk = disk;
            if shared.write_now() {
                legacy::remove(dir);
            }
        }
        let for_writer = Arc::clone(&shared);
        let writer = std::thread::Builder::new()
            .name("tcm-cache-writer".into())
            .spawn(move || for_writer.run_writer())
            .ok();
        Store { shared, writer }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.shared.lock()
    }

    /// Note a change: written behind the caller by the writer thread, or
    /// right here when the thread could not be started or has died.
    fn changed(&self) {
        if self.writer.as_ref().is_some_and(|w| !w.is_finished()) {
            self.shared.schedule();
        } else {
            self.shared.write_now();
        }
    }

    /// Write any pending change now. The app calls this at exit; a dropped
    /// store does it itself.
    pub fn flush(&self) {
        self.shared.flush();
    }

    /// How many times the file has been written since this store opened.
    /// For tests and diagnostics.
    pub fn disk_writes(&self) -> u64 {
        self.shared.writes.load(Ordering::Relaxed)
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

    /// Store a freshly fetched value, replacing whatever was there. Also
    /// where old wiki pages are evicted (`evict_wiki_pages`), and other
    /// clones' Playwright listings (`evict_pw_lists`).
    pub fn put<T: Serialize>(&self, key: &str, value: &T) {
        self.put_at(key, value, now_ms())
    }

    /// `put`, stamped as fetched at `at_ms` (epoch ms) instead of now. The
    /// time seam for `fresh`: a test stores a value as a week old and sees
    /// its TTL run out, without a clock of its own anywhere.
    pub fn put_at<T: Serialize>(&self, key: &str, value: &T, at_ms: u64) {
        let Ok(value) = serde_json::to_value(value) else { return };
        {
            let mut inner = self.lock();
            let now = now_ms();
            inner.disk.entries.insert(key.to_string(), Entry { value, at_ms });
            evict_wiki_pages(&mut inner.disk.entries, now);
            evict_pw_lists(&mut inner.disk.entries, key);
        }
        self.changed();
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
        {
            let mut inner = self.lock();
            let Some(entry) = inner.disk.entries.get_mut(key) else { return };
            let Ok(mut value) = serde_json::from_value::<T>(entry.value.clone()) else { return };
            if !f(&mut value) {
                return;
            }
            let Ok(json) = serde_json::to_value(&value) else { return };
            entry.value = json;
        }
        self.changed();
    }

    /// Drop one key from both tiers.
    pub fn forget(&self, key: &str) {
        {
            let mut inner = self.lock();
            inner.session.remove(key);
            if inner.disk.entries.remove(key).is_none() {
                return;
            }
        }
        self.changed();
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
    /// the cache is marked `UNNAMED_OWNER`, never left plain unowned - an
    /// unowned cache is adopted as is by the next named sign-in, and an
    /// unnamed one must not be mistaken for that.
    ///
    /// Answers whether the cache was dropped for a different (or unnamed)
    /// account, so the caller can forget what it keeps outside the cache.
    pub fn claim_for(&self, account: Option<&str>) -> bool {
        let mut inner = self.lock();
        let Some(account) = account else {
            if inner.disk.owner.as_deref() == Some(UNNAMED_OWNER)
                && inner.disk.entries.is_empty()
                && inner.session.is_empty()
            {
                return false;
            }
            inner.disk.entries.clear();
            inner.session.clear();
            inner.disk.owner = Some(UNNAMED_OWNER.to_string());
            drop(inner);
            self.write_through();
            return true;
        };
        let tag = owner_tag(account);
        let mut dropped = false;
        // Compare first, into a plain bool, so no borrow of the owner is
        // alive while the entries are cleared.
        match inner.disk.owner.as_deref().map(|owner| owner == tag) {
            Some(true) => return false,
            Some(false) => {
                inner.disk.entries.clear();
                inner.session.clear();
                dropped = true;
            }
            None => {}
        }
        inner.disk.owner = Some(tag);
        drop(inner);
        self.write_through();
        dropped
    }

    /// Who the cache belongs to, as the short tag it records (never the
    /// address, never a token); empty while it belongs to no one. Work that
    /// is shared between askers is keyed by it, so an ask made under one
    /// account never joins work started under another.
    pub fn owner(&self) -> String {
        self.lock().disk.owner.clone().unwrap_or_default()
    }

    /// Drop every entry in both tiers (the owner is kept).
    pub fn clear(&self) {
        {
            let mut inner = self.lock();
            inner.disk.entries.clear();
            inner.session.clear();
        }
        self.write_through();
    }

    /// Write now, not behind the caller: for the wipes, which must be on
    /// disk before anyone can act on them. Anything pending goes with it.
    fn write_through(&self) {
        self.shared.wake().dirty = false;
        self.shared.write_now();
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

/// See `Store::put_at`: the time seam for `fresh`.
pub fn put_at<T: Serialize>(key: &str, value: &T, at_ms: u64) {
    global().put_at(key, value, at_ms)
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

/// `Store::claim_for` for the global cache: true when a different account
/// than the owner signed in and everything was dropped.
pub fn claim_for(account: Option<&str>) -> bool {
    global().claim_for(account)
}

/// `Store::owner` for the global cache.
pub fn owner() -> String {
    global().owner()
}

pub fn clear() {
    global().clear()
}

/// Write any pending change now: the app calls this as it exits, since the
/// global store is never dropped.
pub fn flush() {
    if let Some(store) = GLOBAL.get() {
        store.flush();
    }
}
