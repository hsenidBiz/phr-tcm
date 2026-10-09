//! One browser for a whole unattended run, and a fresh browser context for
//! each case in it.
//!
//! A context starts with no cookies and no storage, as a fresh profile
//! does, so one case still cannot pass only because another signed in -
//! but the browser is started, and its profile made and deleted, once per
//! run instead of once per case. Each case gets its own connection to the
//! browser, driving a page in its own context (`Cdp::drive_new_context`),
//! so its tabs, dialogs, downloads, page log and save guard are its own and
//! start empty; a page in any other context is never one of its tabs. The
//! context is disposed when the case ends.
//!
//! A browser that dies mid-run takes only the case it was running with it:
//! that case is recorded as any case whose browser stopped answering is,
//! and the next case starts a new browser.

use crate::autorun::replay::Browsers;
use crate::browser::cdp::{Cdp, Transport};
use std::future::Future;

/// How a browser is started, reached and stopped. The command gives the
/// real one (`commands::autorun_replay`); the tests give a fake.
pub trait Launcher {
    /// A started browser.
    type Process;
    /// The socket a connection to it talks over.
    type T: Transport;
    /// Start a browser, and return once it answers.
    fn launch(&mut self) -> impl Future<Output = Result<Self::Process, String>>;
    /// A new connection to the browser itself, with no page driven yet.
    fn connect(&mut self, p: &Self::Process) -> impl Future<Output = Result<Cdp<Self::T>, String>>;
    /// Is its process still running?
    fn alive(&mut self, p: &mut Self::Process) -> bool;
    /// Stop it and delete its profile.
    fn close(&mut self, p: Self::Process);
}

/// The `Browsers` an unattended run hands its cases: one browser, started
/// by the first case and closed when this is dropped at the end of the
/// run, with a context per case.
pub struct OneBrowser<L: Launcher> {
    launcher: L,
    current: Option<L::Process>,
}

impl<L: Launcher> OneBrowser<L> {
    pub fn new(launcher: L) -> Self {
        OneBrowser { launcher, current: None }
    }

    /// Close the browser, if one is open: the next case starts another.
    fn retire(&mut self) {
        if let Some(p) = self.current.take() {
            self.launcher.close(p);
        }
    }

    /// A connection to the open browser driving a page in a new context of
    /// its own, with the page log switched on.
    async fn new_case(&mut self) -> Result<Cdp<L::T>, String> {
        let Some(p) = self.current.as_ref() else {
            return Err("no browser is open".to_string());
        };
        let mut cdp = self.launcher.connect(p).await?;
        cdp.drive_new_context().await.map_err(|e| e.to_string())?;
        // So a case that cannot reach its module can say what the page was
        // doing. Losing that is no reason not to run the case.
        if let Err(e) = crate::browser::page_log::watch(&mut cdp).await {
            crate::applog::warn(format!("unattended run: the page log could not be switched on: {e}"));
        }
        Ok(cdp)
    }
}

impl<L: Launcher> Browsers for OneBrowser<L> {
    type D = Cdp<L::T>;

    /// The run's browser, started now if there is none (the first case, or
    /// the one after a browser died), and a fresh context in it. An open
    /// browser that will not give a context is closed and one more is
    /// started, once.
    async fn open(&mut self) -> Result<Cdp<L::T>, String> {
        if let Some(p) = self.current.as_mut() {
            if !self.launcher.alive(p) {
                crate::applog::warn("unattended run: the browser closed during the run - a new one is started");
                self.retire();
            }
        }
        let mut last = String::new();
        for _ in 0..2 {
            let fresh = self.current.is_none();
            if fresh {
                self.current = Some(self.launcher.launch().await?);
            }
            match self.new_case().await {
                Ok(cdp) => return Ok(cdp),
                Err(e) => {
                    crate::applog::warn(format!("unattended run: the browser gave no context for the case: {e}"));
                    last = e;
                    self.retire();
                    if fresh {
                        break;
                    }
                }
            }
        }
        Err(last)
    }

    /// The case's context goes, with its pages, cookies and storage. A
    /// browser that cannot dispose of it is not trusted with another case:
    /// it is closed, and the next case starts a new one.
    async fn close(&mut self, mut d: Cdp<L::T>) {
        if let Err(e) = d.dispose_context().await {
            crate::applog::warn(format!(
                "unattended run: the case's browser context could not be closed ({e}) - the next case starts a new browser"
            ));
            drop(d);
            self.retire();
        }
    }
}

/// A run that ends, errors out or panics never leaves its browser behind.
impl<L: Launcher> Drop for OneBrowser<L> {
    fn drop(&mut self) {
        self.retire();
    }
}
