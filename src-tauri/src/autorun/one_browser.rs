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
//! and the next case starts a new browser. A browser that will not make
//! contexts at all (a policy can switch them off) gives the rest of the run
//! a browser per case, as it was before contexts.

use crate::autorun::replay::Browsers;
use crate::browser::cdp::{Cdp, CdpError, Transport};
use std::future::Future;

/// What a case's Blocked reason says when its browser would not give it a
/// page. The raw error names a local DevTools address or protocol text,
/// nothing a person can act on: it goes to the log instead.
pub const NO_FRESH_PAGE: &str =
    "it would not open a fresh page for the case - try again, and see Settings, Logs if it keeps happening";

/// How a browser is started, reached and stopped. The command gives the
/// real one (`commands::autorun_replay`); the tests give a fake.
pub trait Launcher {
    /// A started browser.
    type Process;
    /// The socket a connection to it talks over.
    type T: Transport;
    /// Start a browser, and return once it answers. Its error is already
    /// worded for a person.
    fn launch(&mut self) -> impl Future<Output = Result<Self::Process, String>>;
    /// A new connection to the browser itself, with no page driven yet.
    fn connect(&mut self, p: &Self::Process) -> impl Future<Output = Result<Cdp<Self::T>, String>>;
    /// Is its process still running?
    fn alive(&mut self, p: &mut Self::Process) -> bool;
    /// Stop it and delete its profile.
    fn close(&mut self, p: Self::Process);
}

/// Why a case's page could not be made.
enum NoPage {
    /// The browser refused to make a context, or a page in one.
    ContextsRefused(CdpError),
    /// Anything else, raw, for the log.
    Failed(String),
}

/// Is this the browser refusing a context, or a page in one?
fn refuses_contexts(e: &CdpError) -> bool {
    matches!(e, CdpError::Protocol { method, .. } if method == "Target.createBrowserContext" || method == "Target.createTarget")
}

/// The `Browsers` an unattended run hands its cases: one browser, started
/// by the first case and closed when this is dropped at the end of the
/// run, with a context per case.
pub struct OneBrowser<L: Launcher> {
    launcher: L,
    current: Option<L::Process>,
    /// The browser refused contexts: from then on each case has a browser
    /// of its own, closed when the case ends.
    per_case: bool,
}

impl<L: Launcher> OneBrowser<L> {
    pub fn new(launcher: L) -> Self {
        OneBrowser { launcher, current: None, per_case: false }
    }

    /// Close the browser, if one is open: the next case starts another.
    fn retire(&mut self) {
        if let Some(p) = self.current.take() {
            self.launcher.close(p);
        }
    }

    /// A connection to the open browser driving a fresh page: in a new
    /// context of its own, or, once contexts were refused, in the
    /// browser's default context. The page log is switched on.
    async fn new_case(&mut self) -> Result<Cdp<L::T>, NoPage> {
        let Some(p) = self.current.as_ref() else {
            return Err(NoPage::Failed("no browser is open".to_string()));
        };
        let mut cdp = self.launcher.connect(p).await.map_err(NoPage::Failed)?;
        let driven = if self.per_case { cdp.drive_new_page().await } else { cdp.drive_new_context().await };
        match driven {
            Ok(()) => {}
            Err(e) if !self.per_case && refuses_contexts(&e) => return Err(NoPage::ContextsRefused(e)),
            Err(e) => return Err(NoPage::Failed(e.to_string())),
        }
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
    /// started, once. One that refuses contexts outright serves this case
    /// in its default context, and every later case gets a browser of its
    /// own. Any failure is logged as it is, and the case is told
    /// `NO_FRESH_PAGE` (a launch that failed says its own words).
    async fn open(&mut self) -> Result<Cdp<L::T>, String> {
        if let Some(p) = self.current.as_mut() {
            if !self.launcher.alive(p) {
                crate::applog::warn("unattended run: the browser closed during the run - a new one is started");
                self.retire();
            }
        }
        for _ in 0..2 {
            let fresh = self.current.is_none();
            if fresh {
                self.current = Some(self.launcher.launch().await?);
            }
            let why = match self.new_case().await {
                Ok(cdp) => return Ok(cdp),
                Err(NoPage::ContextsRefused(e)) => {
                    crate::applog::warn(format!(
                        "unattended run: the browser will not make a context ({e}) - each case gets a browser of its own for the rest of the run"
                    ));
                    self.per_case = true;
                    match self.new_case().await {
                        Ok(cdp) => return Ok(cdp),
                        Err(NoPage::ContextsRefused(e)) => e.to_string(),
                        Err(NoPage::Failed(e)) => e,
                    }
                }
                Err(NoPage::Failed(e)) => e,
            };
            crate::applog::warn(format!("unattended run: the browser gave no page for the case: {why}"));
            self.retire();
            if fresh {
                break;
            }
        }
        Err(NO_FRESH_PAGE.to_string())
    }

    /// The case's context goes, with its pages, cookies and storage. A
    /// browser that cannot dispose of it is not trusted with another case:
    /// it is closed, and the next case starts a new one. Once contexts were
    /// refused, the case's own browser is closed.
    async fn close(&mut self, mut d: Cdp<L::T>) {
        if self.per_case {
            drop(d);
            self.retire();
            return;
        }
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
