//! How many times the assistant has signed an account in while working on
//! one sign-in problem. A trial, an inspect that signs in and the check that
//! follows a saved proposal each take one; at `MOST_SIGN_IN_ATTEMPTS` the
//! answer is to report to the person and stop, so a wrong password or a
//! locked account is never hammered. Memory only: one app session, like
//! `signin`'s refused-reset list. A check that signs the account in closes
//! the problem and the count starts over.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// The most sign-ins the assistant makes for one account and one problem.
pub const MOST_SIGN_IN_ATTEMPTS: u32 = 3;

static ATTEMPTS: Mutex<BTreeMap<String, u32>> = Mutex::new(BTreeMap::new());

fn held() -> MutexGuard<'static, BTreeMap<String, u32>> {
    ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn marker(root: &Path, key: &str) -> String {
    format!("{}|{key}", root.display())
}

/// What the assistant is told once the attempts are used.
pub fn attempts_used(account: &str) -> String {
    format!(
        "the account \"{account}\" has had {MOST_SIGN_IN_ATTEMPTS} sign-in attempts for this sign-in problem - report it to the person and stop; do not try again"
    )
}

/// Open the problem or continue it: the attempt number, 1 to 3. Once all are
/// used it is `Err(attempts_used)` and the count does not grow.
pub fn take(root: &Path, key: &str) -> Result<u32, String> {
    let mut held = held();
    let used = held.entry(marker(root, key)).or_insert(0);
    if *used >= MOST_SIGN_IN_ATTEMPTS {
        return Err(attempts_used(key));
    }
    *used += 1;
    Ok(*used)
}

/// Count an attempt only when a problem is already open: `Ok(true)` when it
/// was counted, `Ok(false)` (nothing opened) when none is.
pub fn take_if_open(root: &Path, key: &str) -> Result<bool, String> {
    let mut held = held();
    match held.get_mut(&marker(root, key)) {
        None => Ok(false),
        Some(used) if *used >= MOST_SIGN_IN_ATTEMPTS => Err(attempts_used(key)),
        Some(used) => {
            *used += 1;
            Ok(true)
        }
    }
}

/// Has a sign-in problem been opened for this account?
pub fn is_open(root: &Path, key: &str) -> bool {
    held().contains_key(&marker(root, key))
}

/// A check signed the account in: the problem is over.
pub fn close(root: &Path, key: &str) {
    held().remove(&marker(root, key));
}

/// Every count, for tests.
pub fn forget_all() {
    held().clear();
}
