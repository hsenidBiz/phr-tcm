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
//! `handoff` builds the prompt that finishes exported cases in the repo
//! (`/gen-test` steps 4, 5, 6 and 7); the app only copies it.
//!
//! `presence` finds which ADO ids the clone already has, and where: what
//! Export skips by default and what Sync's Add new leaves alone.
//!
//! Offered while the Playwright module is on (`modules::on`): with it off
//! every command of the tab, and Export to Playwright, is refused.

pub mod handoff;
pub mod layout;
pub mod presence;
pub mod process;
pub mod runner;
pub mod structure;
pub mod sync;
pub mod users;

/// The refusal every Playwright command gives while the module is off -
/// `Module::Playwright.off_sentence()`, spelled out so it can be a const.
pub const NOT_OFFERED: &str = "Playwright is switched off in Modules";

/// `Err(NOT_OFFERED)` unless the Playwright module is on (`offered`).
pub fn ensure_offered(offered: bool) -> Result<(), String> {
    if offered {
        Ok(())
    } else {
        Err(NOT_OFFERED.to_string())
    }
}

/// How long a Playwright command took, logged at debug level when it is
/// dropped (`timed`): its name and the milliseconds, and nothing else (no
/// path, argument or secret ever reaches the line).
pub struct Timed {
    name: &'static str,
    start: std::time::Instant,
}

/// Starts timing command `name`; the line is logged when the guard drops,
/// so an early `?` return is timed too.
pub fn timed(name: &'static str) -> Timed {
    Timed { name, start: std::time::Instant::now() }
}

/// The line `Timed` logs for command `name` after `elapsed`.
pub fn timing_line(name: &str, elapsed: std::time::Duration) -> String {
    format!("pw-timing: {name} took {} ms", elapsed.as_millis())
}

impl Drop for Timed {
    fn drop(&mut self) {
        crate::applog::debug(timing_line(self.name, self.start.elapsed()));
    }
}
