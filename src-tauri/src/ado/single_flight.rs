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

use std::future::Future;
use std::sync::Mutex;

use futures::future::{BoxFuture, FutureExt, Shared};
use tokio::sync::watch;

/// (done, total) of the walk, as its progress callback reports it.
pub type Progress = (u32, u32);

struct Flight<T: Clone> {
    key: String,
    result: Shared<BoxFuture<'static, T>>,
    progress: watch::Receiver<Progress>,
}

/// The running work of one kind, keyed by what it is for. Held in a
/// `static` by each kind of work (a `Vec` rather than a map: a handful of
/// entries at most, and none outlive their work).
pub struct Flights<T: Clone> {
    open: Mutex<Vec<Flight<T>>>,
}

impl<T: Clone + Send + Sync + 'static> Flights<T> {
    pub const fn new() -> Self {
        Self { open: Mutex::new(Vec::new()) }
    }

    /// The result of the work for `key`: started by `make` when nothing
    /// for `key` is running, otherwise the running work's. `also_join`
    /// names a second key whose running work answers this ask too (checked
    /// first); it never starts work under that key. `make` gets the sender
    /// the work reports (done, total) on; every asker's `progress` hears it.
    pub async fn run<Fut>(
        &'static self,
        key: &str,
        also_join: Option<&str>,
        make: impl FnOnce(watch::Sender<Progress>) -> Fut,
        mut progress: impl FnMut(u32, u32),
    ) -> T
    where
        Fut: Future<Output = T> + Send + 'static,
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
                    let (tx, rx) = watch::channel((0, 0));
                    let work = make(tx);
                    let owned = key.to_string();
                    let result = async move {
                        let out = work.await;
                        self.finish(&owned);
                        out
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
        if told.1 > 0 {
            progress(told.0, told.1);
        }
        loop {
            tokio::select! {
                out = &mut result => {
                    // The last report can land with the result itself.
                    let last = *heard.borrow();
                    if last != told && last.1 > 0 {
                        progress(last.0, last.1);
                    }
                    return out;
                }
                Ok(()) = heard.changed() => {
                    told = *heard.borrow_and_update();
                    progress(told.0, told.1);
                }
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Flight<T>>> {
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
