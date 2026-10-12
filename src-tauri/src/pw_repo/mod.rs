//! The Playwright tab: the team's PHR-PLAYWRIGHT-AUTOMATION repo, read from
//! its clone folder (the same folder and recognition Export to Playwright
//! uses, `pw_export::clone`).
//!
//! `structure` reads what is in the clone and whether it can be used. It
//! only reads: nothing here writes in the clone or starts a process.
//!
//! Offered exactly where Auto Run is (`ai_tools::autorun_offered`): with
//! Advanced Features off every command of the tab is refused.

pub mod structure;

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
