//! A signed-in browser kept between single API template runs: at most one
//! per (environment id, account key), process-wide, so the next run as that
//! account can skip the launch and the sign-in.
//!
//! The store only keeps entries. It never opens, drives or closes a
//! browser. Every entry it hands back - a stale one from `take`, a replaced
//! one from `put`, the ones `expired` and `drain_all` drain - is the
//! caller's to close.
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
