//! A signed-in browser kept between single API template runs: at most one
//! per (environment id, account key), process-wide, so the next run as that
//! account can skip the launch and the sign-in.
//!
//! The store only keeps entries. It never opens, drives or closes a
//! browser: it has no `Browsers`. Every entry it hands back - a stale one
//! from `take`, a replaced one from `put`, the ones `expired` and
//! `drain_all` drain - is the caller's to close through `Browsers::close`.
//!
//! An entry is reused only while both hold:
//! - its fingerprint (`fingerprint`: the sign-in recipe, the account key and
//!   its login) matches the run's, so a changed recipe or account never
//!   reuses a browser signed in the old way;
//! - its lease generation still equals `lease::generation` for its key:
//!   anything other than a template run that took the account since (a
//!   case, the supervised browser, setup) signed it in elsewhere, which
//!   ended this browser's session. It has given way.
//!
//! An idle held browser holds no lease. It is closed `HELD_IDLE` after the
//! run that kept it ended.
//!
//! The driver type is whatever `Browsers::D` is: the CDP driver in the app,
//! a fake in the tests. Rust has no generic statics, so one map holds every
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
    serde_json::to_string(recipe).unwrap_or_default().hash(&mut h);
    account.key.hash(&mut h);
    account.username.hash(&mut h);
    h.finish()
}

struct Slot {
    deadline: Instant,
    /// The account key, for log lines.
    key: String,
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
    slot.entry.downcast::<HeldEntry<D>>().ok().map(|b| *b)
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
    let slot = Slot { deadline: now + HELD_IDLE, key: key.to_string(), entry: Box::new(entry) };
    let replaced = store().insert(at::<D>(env, key), slot);
    applog::info(format!("held browser: kept signed in as {key}"));
    replaced.and_then(open::<D>)
}

/// Takes out every entry whose idle time has run out, to be closed.
pub fn expired<D: Send + 'static>() -> Vec<HeldEntry<D>> {
    expired_at(Instant::now())
}

/// `expired`, as if now were `now`.
pub fn expired_at<D: Send + 'static>(now: Instant) -> Vec<HeldEntry<D>> {
    drain::<D>(|slot| slot.deadline <= now, |key| format!("held browser: {key} idle for {} s, closing", HELD_IDLE.as_secs()))
}

/// Takes out every entry, to be closed: the app quits, the active
/// environment changes or the person signs out.
pub fn drain_all<D: Send + 'static>() -> Vec<HeldEntry<D>> {
    drain::<D>(|_| true, |key| format!("held browser: {key} closing"))
}

fn drain<D: Send + 'static>(due: impl Fn(&Slot) -> bool, line: impl Fn(&str) -> String) -> Vec<HeldEntry<D>> {
    let ty = TypeId::of::<D>();
    let slots: Vec<Slot> = {
        let mut s = store();
        let keys: Vec<_> = s.iter().filter(|(k, v)| k.0 == ty && due(v)).map(|(k, _)| k.clone()).collect();
        keys.iter().filter_map(|k| s.remove(k)).collect()
    };
    slots
        .into_iter()
        .filter_map(|slot| {
            applog::info(line(&slot.key));
            open::<D>(slot)
        })
        .collect()
}
