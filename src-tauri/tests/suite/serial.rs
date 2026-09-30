//! Locks for the process-wide state these tests share.
//!
//! When every file under tests/ was a binary of its own, "process-wide"
//! meant one file's tests: a lock local to that file was enough, and a
//! test that assumed nothing else touched a static was right. In one
//! binary two modules can race on the same static, so the lock for each
//! piece of shared state lives here and every module that touches it takes
//! the same one. Tests that only ever touch their own temp dirs, mock
//! servers and values need none of this.

use std::sync::{Mutex, MutexGuard};

/// A test that fails while holding a lock must fail on its own - not
/// poison every later test that shares the lock and bury the real failure
/// under a dozen `PoisonError`s.
fn hold(lock: &'static Mutex<()>) -> MutexGuard<'static, ()> {
    lock.lock().unwrap_or_else(|e| e.into_inner())
}

/// The Azure DevOps pacer (`ado::throttle`): its level, and the hold a
/// server's Retry-After puts on every request. Taken by the tests that set
/// a hold or the level, and by the ones that time requests - a hold banked
/// by another module would otherwise land in the middle of their timing.
pub fn pacer() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    hold(&L)
}

/// The script store's root (`autorun::store::set_root`), the one-at-a-time
/// run and recording claims, and the supervised browser slot. The bridge's
/// page routes answer from all three, so a claim held by one module's test
/// would change another module's answer.
pub fn autorun() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    hold(&L)
}

/// The `SQLCMD_OVERRIDE` environment variable, which decides where (or
/// whether) sqlcmd is found for everything in the process.
pub fn sqlcmd_env() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    hold(&L)
}

/// The in-memory log tail (`applog::recent`). One test fills it past its
/// bound on purpose, which pushes out any line another test logged and is
/// about to look for.
pub fn log_tail() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    hold(&L)
}

/// The report revision counters (`note_server::revision`): a bump between
/// reading one and rendering a page from it makes the page one ahead.
pub fn report_revisions() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    hold(&L)
}

/// `activity_log`'s own directory (`activity_log::init`). Taken by any test
/// that points it at a tempdir and reads that tempdir back, AND by every
/// other test that reaches `db::query`'s `run_query` / `run_batch_query` /
/// `run_lookup` - those call `activity_log::record` too now, and without
/// this lock a record from one test's run can land in another test's
/// tempdir mid-assertion (this raced in practice before every db_tests and
/// db_batch test that calls them took the lock).
pub fn activity_log() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    hold(&L)
}

/// The one-at-a-time API template run slot (`api_templates::runner::claim`):
/// a claim one test holds would make another test's claim come back `None`.
pub fn api_template_run() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    hold(&L)
}

/// The guide's once-per-run published answer (`guide::forget_published`)
/// and its one-at-a-time download claim: a cached answer from one test
/// would be another test's answer, and a claim held by one would refuse
/// another's download.
pub fn guide() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    hold(&L)
}
