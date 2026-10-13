//! The Playwright tab: the team's PHR-PLAYWRIGHT-AUTOMATION repo, read from
//! its clone folder (the same folder and recognition Export to Playwright
//! uses, `pw_export::clone`).
//!
//! `structure` reads what is in the clone and whether it can be used. It
//! only reads: nothing there writes in the clone or starts a process.
//!
//! `process` runs a repo command (`npx playwright …` or `npm run users …`)
//! in a Windows job of its own, one at a time, streaming its output.
//!
//! `runner` lists the tests, writes the run list, dry-counts and runs them
//! the way the repo's `npm run qa` does.
//!
//! `users` lists and changes the repo's test accounts through its `npm run
//! users` CLI, and keeps every password out of the log and the output.
//!
//! `sync` keeps the export record and says, for each case, whether the
//! clone's copy of its script is current.
//!
//! Offered exactly where Auto Run is (`ai_tools::autorun_offered`): with
//! Advanced Features off every command of the tab is refused.

pub mod process;
pub mod runner;
pub mod structure;
pub mod sync;
pub mod users;

/// The refusal every Playwright tab command gives where it is not offered.
pub const NOT_OFFERED: &str = "not available in this build";

/// `Err(NOT_OFFERED)` unless the tab is offered (`ai_tools::autorun_offered`).
pub fn ensure_offered(offered: bool) -> Result<(), String> {
    if offered {
        Ok(())
    } else {
        Err(NOT_OFFERED.to_string())
    }
}
