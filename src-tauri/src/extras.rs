//! The optional extras switch: one machine-wide flag, off by default.
//!
//! It is turned on by a key sequence typed on the Settings screen
//! (`src/lib/extrasSequence.ts`, wired in `src/screens/settingsExtras.ts`)
//! and off again by that section's Reset to default. While it is on, a
//! release build offers what a development build always does: the Auto Run
//! tab and the Auto Run AI tools (`ai_tools::autorun_offered`). It is a
//! hidden feature on purpose - nothing user-facing names it.
//!
//! Beside it, a second flag a person can see: Settings' Enable Advanced
//! Features switch (`advanced`). It turns on the same things - Auto Run, API
//! Templates and their AI tools - and nothing else; the Extras card stays
//! with `unlocked` alone. `features_on` is what the gates read. The two are
//! saved in the same file and each save keeps the other's value.
//!
//! Its own small file rather than `crate::cache`: the cache is wiped
//! whenever a different account signs in, and this belongs to the machine,
//! not to an account. Held in memory as well, because the AI bridge asks on
//! every `tools/list` and must not touch the disk to answer.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const FILE: &str = "extras.json";

static UNLOCKED: AtomicBool = AtomicBool::new(false);
static ADVANCED: AtomicBool = AtomicBool::new(false);
static DIR: OnceLock<PathBuf> = OnceLock::new();

#[derive(Default, Serialize, Deserialize)]
struct Disk {
    #[serde(default)]
    unlocked: bool,
    /// Absent from a file written before the switch existed: reads as off.
    #[serde(default)]
    advanced: bool,
}

/// The whole file. Absent, unreadable or not the expected shape all read
/// as both off - never an error.
fn read(dir: &Path) -> Disk {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|s| serde_json::from_str::<Disk>(&s).ok())
        .unwrap_or_default()
}

fn write(dir: &Path, disk: &Disk) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let body = serde_json::to_string(disk).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&dir.join(FILE), &body)
}

/// What `dir` holds for the extras switch.
pub fn load(dir: &Path) -> bool {
    read(dir).unlocked
}

/// What `dir` holds for Enable Advanced Features.
pub fn load_advanced(dir: &Path) -> bool {
    read(dir).advanced
}

/// Write the extras switch into `dir` (created if missing), atomically,
/// keeping the saved Enable Advanced Features value.
pub fn save(dir: &Path, unlocked: bool) -> Result<(), String> {
    write(dir, &Disk { unlocked, ..read(dir) })
}

/// Write Enable Advanced Features into `dir` (created if missing),
/// atomically, keeping the saved extras switch.
pub fn save_advanced(dir: &Path, advanced: bool) -> Result<(), String> {
    write(dir, &Disk { advanced, ..read(dir) })
}

/// Called once from setup with the app data dir: reads the saved switch
/// and remembers where later changes go.
pub fn init(dir: PathBuf) {
    let disk = read(&dir);
    UNLOCKED.store(disk.unlocked, Ordering::SeqCst);
    ADVANCED.store(disk.advanced, Ordering::SeqCst);
    let _ = DIR.set(dir);
}

/// The switch as this process last loaded or set it. Always false in the
/// `--mcp` proxy process, which never runs setup - it learns the app's
/// answer from the bridge's `/tools` instead (`mcp::tool_policy_from`).
pub fn unlocked() -> bool {
    UNLOCKED.load(Ordering::SeqCst)
}

/// Save first, then publish: a switch that could not be saved is not
/// reported as on, only to come back off at the next launch. `init` has to
/// have run first - without it there is nowhere to save, and reporting
/// success would leave the person thinking a switch is on that is gone at
/// the next launch.
pub fn set_unlocked(on: bool) -> Result<(), String> {
    let dir = DIR.get().ok_or_else(|| "could not save: setup has not finished yet".to_string())?;
    save(dir, on)?;
    UNLOCKED.store(on, Ordering::SeqCst);
    Ok(())
}

/// Enable Advanced Features as this process last loaded or set it. Like
/// `unlocked`, always false in the `--mcp` proxy process.
pub fn advanced() -> bool {
    ADVANCED.load(Ordering::SeqCst)
}

/// Save first, then publish - the same rule, for the same reasons, as
/// `set_unlocked`.
pub fn set_advanced(on: bool) -> Result<(), String> {
    let dir = DIR.get().ok_or_else(|| "could not save: setup has not finished yet".to_string())?;
    save_advanced(dir, on)?;
    ADVANCED.store(on, Ordering::SeqCst);
    Ok(())
}

/// Whether Auto Run, API Templates and their AI tools are on for this
/// machine: either flag turns them on.
pub fn features_on() -> bool {
    unlocked() || advanced()
}
