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
//! | A supervised Auto Run browser (main or an assistant's own, by slot) | From a sign-in until it is closed or signs in as another account | Refused at once, by an asker of any slot |
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
    /// The supervised Auto Run browser in this slot (`autorun::slots`; 0 is
    /// the main one). Which slot is how a refusal can name the assistant
    /// whose browser holds an account (`ai_bridge::with_holder_named`); every
    /// slot's browser is worded alike to whoever is refused (`seen_by`).
    Browser { slot: u8 },
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
            (Holder::Browser { .. }, _) => BROWSER_WORDS,
            (Holder::Template, _) => "an API template run",
            (Holder::Case { run }, Holder::Case { run: mine }) if run == mine => "another case in this run",
            (Holder::Case { .. }, _) => "an unattended run",
            (Holder::Setup, _) => "Auto Run setup",
        }
    }

    /// The holder's browser as a log line names it: the assistant's browser,
    /// the main Auto Run browser, or the run that owns it. Never an id or a
    /// pid.
    pub fn browser_words(&self) -> String {
        match self {
            Holder::Browser { slot } => match crate::autorun::slots::SlotNo::from_number(*slot) {
                Some(n) => crate::commands::autorun_slots::whose(n),
                None => "an Auto Run browser".to_string(),
            },
            Holder::Case { .. } => "an unattended run's browser".to_string(),
            Holder::Template => "an API template run's browser".to_string(),
            Holder::Setup => "Auto Run setup's browser".to_string(),
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

/// How the supervised Auto Run browser is named as a holder.
pub const BROWSER_WORDS: &str = "the Auto Run browser";

/// The sentence a request for the account `key` gets while the supervised
/// Auto Run browser holds it, word for word.
pub fn in_use_by_browser(key: &str) -> String {
    in_use(key, BROWSER_WORDS)
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

/// Whether the supervised Auto Run browser or an Auto Run setup browser holds
/// an account right now - the two holders only a person closes. Only looks.
pub fn a_browser_is_held() -> bool {
    table().values().any(|e| matches!(e.holder, Holder::Browser { .. } | Holder::Setup))
}

/// Who holds the account `key` in `env` right now. Only looks.
pub fn holder_of(env: &str, key: &str) -> Option<Holder> {
    table().get(&(env.to_string(), key.to_string())).map(|e| e.holder.clone())
}

/// Every account held in `env` right now, as (account key, holder), in no
/// particular order. Only looks.
pub fn holders_in(env: &str) -> Vec<(String, Holder)> {
    table().iter().filter(|((e, _), _)| e == env).map(|((_, key), entry)| (key.clone(), entry.holder.clone())).collect()
}

/// The first lease refusal sentence inside `text` (`in_use`), as (account
/// key, holder words): `the account <key> was in use by <holder> - try again
/// when it is free`, with a key that has no space and a holder this module
/// names. `None` when there is none.
pub fn find_in_use(text: &str) -> Option<(String, String)> {
    let (key, holder, _) = locate_in_use(text)?;
    Some((key.to_string(), holder.to_string()))
}

/// `find_in_use`, with where the sentence ends in `text` as well: (key,
/// holder words, the byte after its last character).
pub fn locate_in_use(text: &str) -> Option<(&str, &'static str, usize)> {
    for (at, _) in text.match_indices(IN_USE_START) {
        let rest = &text[at + IN_USE_START.len()..];
        let Some(key_len) = rest.find(char::is_whitespace) else { continue };
        let key = &rest[..key_len];
        let Some(after_by) = rest[key_len..].strip_prefix(IN_USE_BY) else { continue };
        for holder in HOLDER_WORDS {
            let Some(after_holder) = after_by.strip_prefix(holder) else { continue };
            if !key.is_empty() && after_holder.starts_with(IN_USE_END) {
                let end = text.len() - after_holder.len() + IN_USE_END.len();
                return Some((key, holder, end));
            }
        }
    }
    None
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

/// What `Held::check_sign_in` made of the browser's cookies against the
/// app's last sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieCheck {
    /// Each HttpOnly cookie of the sign-in is there with the same value.
    Same,
    /// No sign-in has been noted (or it left no HttpOnly cookies).
    NoPrint,
    /// These cookies changed or went, by name (`name (changed)`, `name (gone)`).
    Changed(Vec<String>),
    /// The browser did not answer the cookie read.
    Unread,
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
    /// The sign-in the app last made in this browser, as its HttpOnly
    /// cookies were (`browser::session::CookiePrint`): how a replay tells that the
    /// browser still holds that sign-in, and not one a step typed or the
    /// person made by hand. Memory only; gone with the lease, and whenever
    /// the browser's signed-in account is cleared (`forget_sign_in`).
    signed_in: Option<crate::browser::session::CookiePrint>,
    /// The cookies the assistant set for testing in this browser, by name
    /// in lower case (`autorun::overrides`): the sign-in's print leaves them
    /// out (`same_sign_in`), so a test cookie never makes the sign-in look
    /// changed. Kept for the browser's life - a test cookie a reset deleted
    /// may have replaced one the sign-in left.
    ignored: std::collections::BTreeSet<String>,
}

impl Held {
    pub fn new(holder: Holder, wait: Duration) -> Self {
        Held { holder, wait, lease: None, signed_in: None, ignored: std::collections::BTreeSet::new() }
    }

    /// The main Auto Run browser's, and the person's paths': a held account
    /// is refused at once, never waited for.
    pub fn supervised() -> Self {
        Held::supervised_in(crate::autorun::slots::SlotNo::MAIN)
    }

    /// The Auto Run browser in `slot`'s: a held account is refused at once,
    /// never waited for, and it holds its own as that slot's.
    pub fn supervised_in(slot: crate::autorun::slots::SlotNo) -> Self {
        Held::new(Holder::Browser { slot: slot.0 }, Duration::ZERO)
    }

    /// This browser as a log line names it (`Holder::browser_words`).
    pub fn browser_words(&self) -> String {
        self.holder.browser_words()
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
        self.signed_in = None;
    }

    /// The sign-in the app just made in this browser, as its HttpOnly
    /// cookies are (`session::cookie_print`); `None` when it failed or left
    /// none.
    pub fn note_sign_in(&mut self, print: Option<crate::browser::session::CookiePrint>) {
        self.signed_in = print;
    }

    /// The browser no longer holds the app's last sign-in: its account was
    /// cleared (a session ended, a sign-in that failed).
    pub fn forget_sign_in(&mut self) {
        self.signed_in = None;
    }

    /// The app's last sign-in in this browser, if it still stands.
    pub fn sign_in_print(&self) -> Option<&crate::browser::session::CookiePrint> {
        self.signed_in.as_ref()
    }

    /// The assistant set a test cookie named `name` in this browser: the
    /// sign-in's print leaves it out from now on.
    pub fn ignore_in_print(&mut self, name: &str) {
        self.ignored.insert(name.to_ascii_lowercase());
    }

    /// The cookies the sign-in's print leaves out (`ignore_in_print`).
    pub fn ignored_in_print(&self) -> &std::collections::BTreeSet<String> {
        &self.ignored
    }

    /// Does the browser still hold the app's last sign-in in it - its
    /// HttpOnly cookies for `origins` there with the same values, the
    /// assistant's test cookies left out (`session::changed_cookies`)? None
    /// noted: no. When they differ, the app log says which changed or went,
    /// by name only - never a value.
    pub async fn same_sign_in<D: crate::browser::cdp::Driver>(&self, d: &mut D, origins: &[String]) -> bool {
        matches!(self.check_sign_in(d, origins).await, CookieCheck::Same)
    }

    /// `same_sign_in`, saying which way it was not: no print noted, cookies
    /// that changed or went (named, never their values), or a browser that
    /// did not answer the cookie read.
    pub async fn check_sign_in<D: crate::browser::cdp::Driver>(&self, d: &mut D, origins: &[String]) -> CookieCheck {
        let Some(print) = &self.signed_in else { return CookieCheck::NoPrint };
        match crate::browser::session::changed_cookies(d, origins, print, &self.ignored).await {
            Some(changed) if changed.is_empty() => CookieCheck::Same,
            Some(changed) => {
                crate::applog::info(format!("Auto Run: the sign-in's cookies changed: {}", changed.join(", ")));
                CookieCheck::Changed(changed)
            }
            None => CookieCheck::Unread,
        }
    }

    /// The account this browser holds, if any.
    pub fn account(&self) -> Option<&str> {
        self.lease.as_ref().map(Lease::key)
    }

    /// Does this browser hold `key`'s lease in `root`'s active environment -
    /// the one `hold` would keep as it is? An environment that cannot be
    /// read holds nothing.
    pub fn holds(&self, root: &Path, key: &str) -> bool {
        let Ok(env) = crate::environments::active_id(root) else { return false };
        self.lease.as_ref().is_some_and(|l| l.env == env && l.key == key)
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
        self.signed_in = None;
        let key = key.to_string();
        // Off the async threads: ending a browser process waits for it to go.
        let _ = tauri::async_runtime::spawn_blocking(move || crate::api_templates::held::give_way(&env, &key)).await;
        Ok(())
    }
}
