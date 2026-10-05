//! How long the runner waits, in one place.

#[derive(Debug, Clone, PartialEq)]
pub struct Timing {
    /// A click or fill waiting for its element to be usable.
    pub action_ms: u64,
    /// An expectation waiting to come true.
    pub expect_ms: u64,
    /// A navigation waiting for the page to load.
    pub nav_ms: u64,
    /// Between looks.
    pub poll_ms: u64,
    /// The outline stays up this long before the action, so a watcher can
    /// see WHERE it is about to land.
    pub highlight_ms: u64,
    /// An account someone else is signed in as, waited for before a sign-in
    /// that may wait (`autorun::lease`): an unattended case, an API
    /// template run. The supervised browser never waits.
    pub lease_wait_ms: u64,
}

impl Timing {
    pub fn lease_wait(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.lease_wait_ms)
    }
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            action_ms: 15_000,
            expect_ms: 10_000,
            nav_ms: 30_000,
            poll_ms: 100,
            highlight_ms: 350,
            lease_wait_ms: crate::autorun::lease::WAIT.as_millis() as u64,
        }
    }
}

/// A screenshot is best effort evidence, not part of the action's own
/// result - it must never hold the run up waiting for a browser that is
/// already in trouble.
pub const SHOT_TIMEOUT_MS: u64 = 5_000;
