//! A signed-in browser kept between single API template runs: at most one
//! per (environment id, account key), process-wide, so the next run as that
//! account can skip the launch and the sign-in.
//!
//! The store itself (`take`, `put`, `expired`, `drain_all`) only keeps
//! entries. It never opens, drives or closes a browser: every entry it
//! hands back - a stale one from `take`, a replaced one from `put`, the
//! ones `expired` and `drain_all` drain - is the caller's to close. The
//! helpers built on it (`keep`, `sweep`, `close_all`) close what they take
//! out, through the driver's own `HeldBrowser::close`.
//!
//! A run keeps its browser with `keep`. The first `keep` of a driver type
//! starts that type's idle sweep (`SWEEP_EVERY`) and enrols it in
//! `close_all`, which the app's exit, an environment change and signing out
//! call. An entry a run has taken out is not in the store, so neither the
//! sweep nor `close_all` can close a browser while a run uses it.
//!
//! **A held driver owns its browser process.** In the app,
//! `RealBrowsers::open` hands out only the `Cdp` connection: the Edge
//! process (`LaunchedBrowser`) stays in `RealBrowsers.current`, and dropping
//! the `RealBrowsers` kills it, which every template run does when it ends.
//! So a `Cdp` alone must never be held: it would point at a browser already
//! gone. The app's held `D` carries the process with the connection (the
//! `LaunchedBrowser` taken over from `RealBrowsers`), and closing one drops
//! the connection and ends the process through `autorun::close_browser`.
//! Closing it through a later run's `Browsers::close` would not do: that
//! `RealBrowsers` has no process of its own to close.
//!
//! An entry is reused only while both hold:
//! - its fingerprint (`fingerprint`: the sign-in recipe, the account key and
//!   its login) matches the run's, so a changed recipe or account never
//!   reuses a browser signed in the old way;
//! - its lease generation still equals `lease::generation` for its key:
//!   anything other than a template run that took the account since (a
//!   case, the supervised browser, setup) signed it in elsewhere, which
//!   ended this browser's session. It has given way, and the next sweep
//!   (`expired`) hands it back to be closed even before its idle time runs
//!   out: one browser process per account at most.
//!
//! An idle held browser holds no lease. It is closed `HELD_IDLE` after the
//! run that kept it ended.
//!
//! The driver type is the caller's: the browser process with its CDP
//! connection in the app, a fake in the tests. Rust has no generic statics, so one map holds every
//! entry as `Box<dyn Any + Send>`, keyed by the driver's `TypeId` as well as
//! (environment, account): each driver type sees only its own entries, and
//! the downcast back to `HeldEntry<D>` cannot meet another type.
//!
//! Log lines name the account key only. A fingerprint is never logged.

use super::runner::Session;
use crate::applog;
use crate::autorun::replay::Browsers;
use crate::autorun::accounts::Account;
use crate::autorun::lease;
use crate::autorun::recipe::SignInRecipe;
use std::any::{Any, TypeId};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// How long a held browser waits, after the run that kept it ends, for
/// another run to reuse it. Each reuse starts it again.
pub const HELD_IDLE: Duration = Duration::from_secs(120);

/// A browser kept signed in, and what it needs to be reused.
pub struct HeldEntry<D> {
    pub driver: D,
    /// What its sign-in left (`runner::open_session`).
    pub session: Session,
    /// `fingerprint` of the recipe and account it signed in with.
    pub fingerprint: u64,
    /// `lease::generation` for its account when it signed in.
    pub generation: u64,
    /// The path of the page the browser is on, if known.
    pub page: Option<String>,
}

/// What `take` found for an account.
pub enum Taken<D> {
    /// Signed in the same way, and nothing else has taken the account
    /// since: reuse it.
    Reuse(HeldEntry<D>),
    /// Held, but signed in another way or given way: close it and sign in
    /// afresh. It is no longer in the store.
    Close(HeldEntry<D>),
    /// Nothing is held for the account.
    Nothing,
}

/// A fingerprint of a sign-in: the recipe as JSON, the account key and its
/// login. Two runs with the same one would sign in the same way. Kept in
/// memory only and never logged.
pub fn fingerprint(recipe: &SignInRecipe, account: &Account) -> u64 {
    let mut h = DefaultHasher::new();
    // A recipe that cannot be written out hashes as an error, never as an
    // empty string every such recipe would share.
    serde_json::to_string(recipe).map_err(|e| e.to_string()).hash(&mut h);
    account.key.hash(&mut h);
    account.username.hash(&mut h);
    h.finish()
}

struct Slot {
    deadline: Instant,
    /// The account key, for log lines.
    key: String,
    /// The entry's `generation`, read without a downcast.
    generation: u64,
    entry: Box<dyn Any + Send>,
}

type Store = HashMap<(TypeId, String, String), Slot>;

/// The store. Never held across an await or anything that can panic, so a
/// panic elsewhere cannot poison it; a poisoned one is still used.
fn store() -> MutexGuard<'static, Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn at<D: 'static>(env: &str, key: &str) -> (TypeId, String, String) {
    (TypeId::of::<D>(), env.to_string(), key.to_string())
}

/// The entry back from its slot. The slot was keyed by `D`'s `TypeId`, so
/// the downcast holds; `None` would mean the map was keyed wrongly.
fn open<D: Send + 'static>(slot: Slot) -> Option<HeldEntry<D>> {
    let key = slot.key;
    match slot.entry.downcast::<HeldEntry<D>>() {
        Ok(b) => Some(*b),
        Err(_) => {
            applog::warn(format!("held browser: the entry for {key} was of another type and was dropped unclosed"));
            None
        }
    }
}

/// The browser held for the account `key` in `env`, taken out of the store.
/// `Reuse` when it signed in with `fingerprint` and its account has not
/// been taken by anything else since; `Close` when it is held but either
/// differs.
pub fn take<D: Send + 'static>(env: &str, key: &str, fingerprint: u64) -> Taken<D> {
    let Some(slot) = store().remove(&at::<D>(env, key)) else {
        return Taken::Nothing;
    };
    let Some(entry) = open::<D>(slot) else {
        return Taken::Nothing;
    };
    if entry.generation != lease::generation(env, key) {
        applog::info(format!("held browser: {key} gave way to another sign-in"));
        Taken::Close(entry)
    } else if entry.fingerprint != fingerprint {
        applog::info(format!("held browser: the sign-in for {key} changed, not reused"));
        Taken::Close(entry)
    } else {
        applog::info(format!("held browser: reused, signed in as {key}"));
        Taken::Reuse(entry)
    }
}

/// Keeps `entry` for the account `key` in `env` until `HELD_IDLE` from now.
/// The entry it replaced, if any, is handed back to be closed.
pub fn put<D: Send + 'static>(env: &str, key: &str, entry: HeldEntry<D>) -> Option<HeldEntry<D>> {
    put_at(env, key, entry, Instant::now())
}

/// `put`, as if now were `now`.
pub fn put_at<D: Send + 'static>(env: &str, key: &str, entry: HeldEntry<D>, now: Instant) -> Option<HeldEntry<D>> {
    let slot = Slot { deadline: now + HELD_IDLE, key: key.to_string(), generation: entry.generation, entry: Box::new(entry) };
    let replaced = store().insert(at::<D>(env, key), slot);
    applog::info(format!("held browser: kept signed in as {key}"));
    replaced.and_then(open::<D>)
}

/// Takes out every entry to be closed: one whose idle time has run out,
/// and one that has given way (its account was taken by anything other
/// than a template run since it signed in).
pub fn expired<D: Send + 'static>() -> Vec<HeldEntry<D>> {
    expired_at(Instant::now())
}

/// `expired`, as if now were `now`.
pub fn expired_at<D: Send + 'static>(now: Instant) -> Vec<HeldEntry<D>> {
    // Lock order: the store, then the lease generations.
    drain::<D>(|(_, env, key), slot| {
        if slot.generation != lease::generation(env, key) {
            Some(format!("held browser: {key} gave way to another sign-in"))
        } else if slot.deadline <= now {
            Some(format!("held browser: {key} idle for {} s, closing", HELD_IDLE.as_secs()))
        } else {
            None
        }
    })
}

/// Takes out every entry, to be closed: the app quits, the active
/// environment changes or the person signs out.
pub fn drain_all<D: Send + 'static>() -> Vec<HeldEntry<D>> {
    drain::<D>(|_, slot| Some(format!("held browser: {} closing", slot.key)))
}

/// Takes out every `D` entry `due` names a log line for, and logs it once
/// the store is let go.
fn drain<D: Send + 'static>(due: impl Fn(&(TypeId, String, String), &Slot) -> Option<String>) -> Vec<HeldEntry<D>> {
    let ty = TypeId::of::<D>();
    let slots: Vec<(String, Slot)> = {
        let mut s = store();
        let keys: Vec<_> =
            s.iter().filter(|(k, _)| k.0 == ty).filter_map(|(k, v)| due(k, v).map(|line| (k.clone(), line))).collect();
        keys.into_iter().filter_map(|(k, line)| s.remove(&k).map(|slot| (line, slot))).collect()
    };
    slots
        .into_iter()
        .filter_map(|(line, slot)| {
            applog::info(line);
            open::<D>(slot)
        })
        .collect()
}

/// How often the idle sweep looks for held browsers to close.
pub const SWEEP_EVERY: Duration = Duration::from_secs(10);

/// A browser that can be held: it owns its browser process, so it outlives
/// the `Browsers` that opened it, and it ends that process itself.
pub trait HeldBrowser: Send + 'static {
    /// Ends the browser. Never panics: a close that fails is logged and
    /// forgotten.
    fn close(self);
}

/// `Browsers` whose browser can be kept after a run, for the next run as
/// the same account to reuse.
pub trait Keeps: Browsers {
    /// What is kept: the connection with the browser process it drives.
    type Kept: HeldBrowser;
    /// Takes the browser `d` drives out of this value, process and all, so
    /// dropping this value no longer ends it. `Err(d)` when it is not kept:
    /// this kind keeps nothing, or its process has already ended. The
    /// caller then closes `d` with `Browsers::close`, as before.
    fn keep(&mut self, d: Self::D) -> Result<Self::Kept, Self::D>;
    /// A kept browser taken back for a run. From here on it is this
    /// value's own: its `close`, or dropping it, ends the process.
    fn adopt(&mut self, kept: Self::Kept) -> Self::D;
}

/// The `Kept` of `Browsers` that never keep a browser: nothing of this
/// type exists, so nothing is ever put or found.
pub enum NotKept {}

impl HeldBrowser for NotKept {
    fn close(self) {
        match self {}
    }
}

/// Keeps `entry` for the account `key` in `env` (`put`) and closes the
/// entry it replaced. The first `keep` of a driver type starts its sweep
/// and enrols it in `close_all`.
pub fn keep<K: HeldBrowser>(env: &str, key: &str, entry: HeldEntry<K>) {
    enrol::<K>();
    if let Some(old) = put(env, key, entry) {
        old.driver.close();
    }
}

/// Closes every `K` entry `expired_at(now)` drains: idle past `HELD_IDLE`,
/// or given way. Never one a run has taken out: it is not in the store.
pub fn sweep_at<K: HeldBrowser>(now: Instant) {
    for e in expired_at::<K>(now) {
        e.driver.close();
    }
}

/// `sweep_at`, now.
pub fn sweep<K: HeldBrowser>() {
    sweep_at::<K>(Instant::now());
}

/// Closes every held browser of every driver type ever kept: the app
/// quits, the active environment changes or the person signs out.
pub fn close_all() {
    // Copied out first: a close is never made under the lock.
    let closers: Vec<fn()> = kinds().iter().map(|(_, close)| *close).collect();
    for close in closers {
        close();
    }
}

fn close_every<K: HeldBrowser>() {
    for e in drain_all::<K>() {
        e.driver.close();
    }
}

/// Each driver type kept so far, with how to close all of its entries.
fn kinds() -> MutexGuard<'static, Vec<(TypeId, fn())>> {
    static KINDS: Mutex<Vec<(TypeId, fn())>> = Mutex::new(Vec::new());
    KINDS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Enrols `K` in `close_all` and starts its sweep, once per type: one task
/// for the life of the app, every `SWEEP_EVERY`. The close itself runs off
/// the async threads, since ending a browser process waits for it to go.
fn enrol<K: HeldBrowser>() {
    let ty = TypeId::of::<K>();
    {
        let mut k = kinds();
        if k.iter().any(|(t, _)| *t == ty) {
            return;
        }
        k.push((ty, close_every::<K>));
    }
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(SWEEP_EVERY).await;
            // A sweep that panicked is reported by its handle and the next
            // one runs as usual.
            let _ = tauri::async_runtime::spawn_blocking(sweep::<K>).await;
        }
    });
}
