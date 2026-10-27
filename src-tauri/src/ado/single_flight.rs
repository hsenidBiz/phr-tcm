//! One walk for however many screens ask at once.
//!
//! Finding a PBI's suite, or reading every plan's suites, costs hundreds of
//! requests on a large project, and three parts of the app (Run Tests, the
//! background finder, the plans tree) can ask for the same answer in the
//! same minute. `Flights::run` lets the first asker start the work and
//! every later asker for the same key wait for that one result instead of
//! repeating it; a different key never waits for another.
//!
//! This is coordination, not a cache. An entry exists only while its work
//! is running and is removed the moment it finishes, whether it succeeded
//! or failed, so nothing is remembered and an error is never served to a
//! later asker. (Remembering an answer is `crate::cache`'s job.)
//!
//! The work runs on a task of its own, not inside whoever asked first: an
//! asker that goes away (a timeout, a closed connection) does not stall the
//! others, and work that panics comes back to every waiter as an error and
//! leaves nothing behind, so the next ask starts fresh.

use std::future::Future;
use std::sync::Mutex;

use futures::future::{BoxFuture, FutureExt, Shared};
use tokio::sync::watch;

use super::AdoError;

/// Where a walk is, as its progress callback reports it: plans read of
/// plans to read, and, when the walk is waiting on one large plan alone,
/// how many pages of that plan's suite list it has read so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub done: u32,
    pub total: u32,
    pub pages: Option<u32>,
}

impl ScanProgress {
    pub fn plans(done: u32, total: u32) -> Self {
        Self { done, total, pages: None }
    }
}

type Outcome<V> = Result<V, AdoError>;

struct Flight<V: Clone> {
    key: String,
    result: Shared<BoxFuture<'static, Outcome<V>>>,
    progress: watch::Receiver<ScanProgress>,
}

/// The running work of one kind, keyed by what it is for. Held in a
/// `static` by each kind of work (a `Vec` rather than a map: a handful of
/// entries at most, and none outlive their work).
pub struct Flights<V: Clone> {
    open: Mutex<Vec<Flight<V>>>,
}

/// Takes the work's entry out when the work's task ends, however it ends:
/// returning, or unwinding from a panic.
struct Finish<V: Clone + Send + Sync + 'static> {
    flights: &'static Flights<V>,
    key: String,
}

impl<V: Clone + Send + Sync + 'static> Drop for Finish<V> {
    fn drop(&mut self) {
        self.flights.finish(&self.key);
    }
}

impl<V: Clone + Send + Sync + 'static> Flights<V> {
    pub const fn new() -> Self {
        Self { open: Mutex::new(Vec::new()) }
    }

    /// The result of the work for `key`: started by `make` when nothing
    /// for `key` is running, otherwise the running work's. `also_join`
    /// names a second key whose running work answers this ask too (checked
    /// first); it never starts work under that key. `make` gets the sender
    /// the work reports its progress on; every asker's `progress` hears it.
    pub async fn run<Fut>(
        &'static self,
        key: &str,
        also_join: Option<&str>,
        make: impl FnOnce(watch::Sender<ScanProgress>) -> Fut,
        mut progress: impl FnMut(ScanProgress),
    ) -> Outcome<V>
    where
        Fut: Future<Output = Outcome<V>> + Send + 'static,
    {
        let (mut result, mut heard) = {
            let mut open = self.lock();
            let running = also_join
                .into_iter()
                .chain(std::iter::once(key))
                .find_map(|k| open.iter().find(|f| f.key == k));
            match running {
                Some(f) => (f.result.clone(), f.progress.clone()),
                None => {
                    let (tx, rx) = watch::channel(ScanProgress::default());
                    let work = make(tx);
                    let guard = Finish { flights: self, key: key.to_string() };
                    let task = tokio::spawn(async move {
                        let _guard = guard;
                        work.await
                    });
                    let result = async move {
                        match task.await {
                            Ok(out) => out,
                            // The work panicked (or was cancelled): every
                            // waiter hears it as a failed read, and the
                            // guard has already cleared the entry.
                            Err(_) => Err(AdoError::Network("the lookup stopped unexpectedly".to_string())),
                        }
                    }
                    .boxed()
                    .shared();
                    open.push(Flight { key: key.to_string(), result: result.clone(), progress: rx.clone() });
                    (result, rx)
                }
            }
        };

        // An asker that joins late still hears where the walk is.
        let mut told = *heard.borrow_and_update();
        if told.total > 0 {
            progress(told);
        }
        loop {
            tokio::select! {
                out = &mut result => {
                    // The last report can land with the result itself.
                    let last = *heard.borrow();
                    if last != told && last.total > 0 {
                        progress(last);
                    }
                    return out;
                }
                Ok(()) = heard.changed() => {
                    told = *heard.borrow_and_update();
                    progress(told);
                }
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Flight<V>>> {
        self.open.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn finish(&self, key: &str) {
        let gone = {
            let mut open = self.lock();
            open.iter().position(|f| f.key == key).map(|i| open.remove(i))
        };
        // Dropped here, outside the lock.
        drop(gone);
    }

    /// How many pieces of work are running, for tests.
    pub fn running(&self) -> usize {
        self.lock().len()
    }
}
