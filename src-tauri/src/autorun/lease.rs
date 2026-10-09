//! One sign-in per account at a time.
//!
//! PeoplesHR allows one session per user: signing an account in anywhere
//! ends that account's session everywhere else. So every sign-in as an Auto
//! Run account first takes a lease on (environment id, account key),
//! process-wide, and holds it while that session is in use. The holders:
//!
//! | Holder | Holds the lease | Asks for a held one |
//! |---|---|---|
//! | An unattended case | From before its sign-in until its browser is closed at the case's end | Waits |
//! | The supervised Auto Run browser | From a sign-in until it is closed or signs in as another account | Refused at once |
//! | An API template run or prove | For its whole run, its browser's close included | Waits |
//! | Auto Run setup: the module-path check, Try and recording, and the recorded sign-in's check | Until its own browser is closed | Refused at once |
//!
//! A waiter waits (up to `WAIT`) only on a holder that ends on its own: a
//! case or a template run. The supervised browser and Auto Run setup hold an
//! account until a person closes them, so waiting on them cannot help, and a
//! waiter is refused at once.
//!
//! A sign-in keeps its lease whichever way it went: one that failed partway
//! may still have signed the account in.
//!
//! A lease is released when it is dropped, and only then: a case that ends,
//! returns early with an error, is stopped (its future dropped) or panics
//! lets its account go as its stack unwinds.
//!
//! Every take by anything other than an API template run bumps the
//! account's generation (`generation`): a template browser kept signed in
//! between runs (`api_templates::held`) is never reused once something
//! else has taken its account.
//!
//! The key is an account key, never a login. Nothing here logs a login or
//! a password: the log lines name the account key only.

use crate::applog;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// How long an unattended case or an API template run waits for an
/// account a case or a template run holds. The supervised browser and Auto
/// Run setup never wait.
pub const WAIT: Duration = Duration::from_secs(60);

/// Between looks while waiting.
const POLL: Duration = Duration::from_millis(250);

/// Who holds a lease, as the sentence names it to whoever is asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// The supervised Auto Run browser.
    Browser,
    /// An API template run or prove.
    Template,
    /// A case of the unattended run with this id.
    Case { run: String },
    /// Auto Run setup: the module-path check, Try and recording, and the
    /// recorded sign-in's check.
    Setup,
}

impl Holder {
    /// This holder as `asking` reads it: a case of the asker's own run is
    /// "another case in this run", one of any other run "an unattended run".
    fn seen_by(&self, asking: &Holder) -> &'static str {
        match (self, asking) {
            (Holder::Browser, _) => "the Auto Run browser",
            (Holder::Template, _) => "an API template run",
            (Holder::Case { run }, Holder::Case { run: mine }) if run == mine => "another case in this run",
            (Holder::Case { .. }, _) => "an unattended run",
            (Holder::Setup, _) => "Auto Run setup",
        }
    }

    /// Whether this holder lets its account go on its own. The supervised
    /// browser and Auto Run setup hold theirs until a person closes them.
    fn ends_on_its_own(&self) -> bool {
        matches!(self, Holder::Case { .. } | Holder::Template)
    }
}

struct Entry {
    ticket: u64,
    holder: Holder,
}

type Table = HashMap<(String, String), Entry>;

/// Per (environment id, account key): see `generation`.
type Generations = HashMap<(String, String), u64>;

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);

/// The registry. Never held across an await or anything that can panic,
/// so a panic elsewhere cannot poison it; a poisoned one is still used.
fn table() -> MutexGuard<'static, Table> {
    static TABLE: OnceLock<Mutex<Table>> = OnceLock::new();
    TABLE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// The account `key` in environment `env`, held until this is dropped.
#[derive(Debug)]
pub struct Lease {
    env: String,
    key: String,
    ticket: u64,
}

impl Lease {
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let mut t = table();
        let at = (std::mem::take(&mut self.env), std::mem::take(&mut self.key));
        // Only its own entry: a ticket is never reused, so a lease can
        // never free one somebody else took after it.
        if t.get(&at).is_some_and(|e| e.ticket == self.ticket) {
            t.remove(&at);
        }
    }
}

/// How the sentence a refused or timed-out request gets begins, joins and
/// ends (`in_use`).
const IN_USE_START: &str = "the account ";
const IN_USE_BY: &str = " was in use by ";
const IN_USE_END: &str = " - try again when it is free";

/// Every holder as a request can name it (`Holder::seen_by`).
const HOLDER_WORDS: [&str; 5] =
    ["the Auto Run browser", "an API template run", "another case in this run", "an unattended run", "Auto Run setup"];

/// The sentence a refused or timed-out request gets.
fn in_use(key: &str, holder: &str) -> String {
    format!("{IN_USE_START}{key}{IN_USE_BY}{holder}{IN_USE_END}")
}

/// Is this exactly a lease refusal's sentence: `the account <key> was in
/// use by <holder> - try again when it is free`, with a key that has no
/// space and a holder this module names? A case refused its account is
/// Blocked, never Failed: the application did nothing wrong.
pub fn is_in_use(detail: &str) -> bool {
    let Some(rest) = detail.strip_prefix(IN_USE_START).and_then(|r| r.strip_suffix(IN_USE_END)) else {
        return false;
    };
    let Some((key, holder)) = rest.split_once(IN_USE_BY) else {
        return false;
    };
    !key.is_empty() && !key.contains(char::is_whitespace) && HOLDER_WORDS.contains(&holder)
}

/// One look: the lease, or who has it as `holder` reads them, and whether
/// they let it go on their own.
fn take(env: &str, key: &str, holder: &Holder) -> Result<Lease, (&'static str, bool)> {
    let mut t = table();
    let at = (env.to_string(), key.to_string());
    if let Some(e) = t.get(&at) {
        return Err((e.holder.seen_by(holder), e.holder.ends_on_its_own()));
    }
    let ticket = NEXT_TICKET.fetch_add(1, Ordering::SeqCst);
    if *holder != Holder::Template {
        *generations().entry(at.clone()).or_insert(0) += 1;
    }
    t.insert(at, Entry { ticket, holder: holder.clone() });
    Ok(Lease { env: env.to_string(), key: key.to_string(), ticket })
}

/// How many times anything other than an API template run has taken the
/// account `key` in `env` (0 for one never taken). A held template browser
/// (`api_templates::held`) records this when it signs in: a different
/// number later means something else signed the account in since, which
/// ended the held browser's session.
fn generations() -> MutexGuard<'static, Generations> {
    static GENERATIONS: OnceLock<Mutex<Generations>> = OnceLock::new();
    GENERATIONS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// The account `key` in `env`'s generation (`generations`). Only looks.
pub fn generation(env: &str, key: &str) -> u64 {
    generations().get(&(env.to_string(), key.to_string())).copied().unwrap_or(0)
}

/// Whether anyone holds the account `key` in `env` right now. Only looks.
pub fn is_held(env: &str, key: &str) -> bool {
    table().contains_key(&(env.to_string(), key.to_string()))
}

/// The lease, now, or the sentence that says who has it.
pub fn try_acquire(env: &str, key: &str, holder: Holder) -> Result<Lease, String> {
    take(env, key, &holder).map_err(|(by, _)| in_use(key, by))
}

/// The lease, waiting up to `wait` for whoever has it to let it go; then
/// the sentence that says who had it. A zero wait looks once, and so does
/// a request for an account held by a holder that never lets it go on its
/// own (`Holder::ends_on_its_own`): it is refused at once.
pub async fn acquire(env: &str, key: &str, holder: Holder, wait: Duration) -> Result<Lease, String> {
    let deadline = Instant::now() + wait;
    let mut waiting = false;
    loop {
        let (by, ends) = match take(env, key, &holder) {
            Ok(lease) => {
                if waiting {
                    applog::info(format!("account lease: {key} is free now"));
                }
                return Ok(lease);
            }
            Err(held) => held,
        };
        let now = Instant::now();
        if now >= deadline || !ends {
            let sentence = in_use(key, by);
            applog::info(format!("account lease: {sentence}"));
            return Err(sentence);
        }
        if !waiting {
            waiting = true;
            applog::info(format!(
                "account lease: {key} is in use by {by} - waiting up to {} s",
                wait.as_secs_f32()
            ));
        }
        tokio::time::sleep(POLL.min(deadline - now)).await;
    }
}

/// The lease one browser holds for the account it is signed in as.
///
/// A sign-in asks `hold` first and goes ahead only when it is `Ok`. From
/// then on the account is this browser's, whichever way the sign-in went,
/// and the account held before it is let go. Dropping this - the case's
/// browser closes, the supervised browser or a setup browser closes - lets
/// the account go.
#[derive(Debug)]
pub struct Held {
    holder: Holder,
    wait: Duration,
    lease: Option<Lease>,
}

impl Held {
    pub fn new(holder: Holder, wait: Duration) -> Self {
        Held { holder, wait, lease: None }
    }

    /// The supervised Auto Run browser's: a held account is refused at
    /// once, never waited for.
    pub fn supervised() -> Self {
        Held::new(Holder::Browser, Duration::ZERO)
    }

    /// Auto Run setup's (a module-path check, Try or recording, or the
    /// recorded sign-in's check): refused at once, never waited for.
    pub fn setup() -> Self {
        Held::new(Holder::Setup, Duration::ZERO)
    }

    /// Lets go of the account this browser holds, if any: its session has
    /// been ended, and someone else may sign in as it.
    pub fn let_go(&mut self) {
        self.lease = None;
    }

    /// The account this browser holds, if any.
    pub fn account(&self) -> Option<&str> {
        self.lease.as_ref().map(Lease::key)
    }

    /// Before a sign-in as `key` in `root`'s active environment: its lease,
    /// kept when this browser already holds it, otherwise taken - waiting as
    /// this holder waits - in place of the one held before. `Err` is the
    /// sentence: nothing may sign in, and what was held stays held.
    ///
    /// A browser a single template run kept signed in as the account
    /// (`api_templates::held`) gives way once the lease is taken, closed
    /// before this returns: this sign-in would end its session anyway, and
    /// one browser process per account is the most there should be.
    pub async fn hold(&mut self, root: &Path, key: &str) -> Result<(), String> {
        let env = crate::environments::active_id(root)?;
        if self.lease.as_ref().is_some_and(|l| l.env == env && l.key == key) {
            return Ok(());
        }
        self.lease = Some(acquire(&env, key, self.holder.clone(), self.wait).await?);
        let key = key.to_string();
        // Off the async threads: ending a browser process waits for it to go.
        let _ = tauri::async_runtime::spawn_blocking(move || crate::api_templates::held::give_way(&env, &key)).await;
        Ok(())
    }
}
