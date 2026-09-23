//! The optional extras switch: one machine-wide flag, off by default.
//!
//! It is turned on by a key sequence typed on the Settings screen
//! (`src/lib/extrasSequence.ts`, wired in `src/screens/settingsExtras.ts`)
//! and off again by that section's Reset to default. While it is on, a
//! release build offers what a development build always does: the Auto Run
//! tab and the Auto Run AI tools (`ai_tools::autorun_offered`). It is a
//! hidden feature on purpose - nothing user-facing names it.
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
static DIR: OnceLock<PathBuf> = OnceLock::new();

#[derive(Default, Serialize, Deserialize)]
struct Disk {
    #[serde(default)]
    unlocked: bool,
}

/// What `dir` holds. Absent, unreadable or not the expected shape all read
/// as locked - never an error.
pub fn load(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|s| serde_json::from_str::<Disk>(&s).ok())
        .map(|d| d.unlocked)
        .unwrap_or(false)
}

/// Write the switch into `dir` (created if missing), atomically.
pub fn save(dir: &Path, unlocked: bool) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let body = serde_json::to_string(&Disk { unlocked }).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&dir.join(FILE), &body)
}

/// Called once from setup with the app data dir: reads the saved switch
/// and remembers where later changes go.
pub fn init(dir: PathBuf) {
    UNLOCKED.store(load(&dir), Ordering::SeqCst);
    let _ = DIR.set(dir);
}

/// The switch as this process last loaded or set it. Always false in the
/// `--mcp` proxy process, which never runs setup - it learns the app's
/// answer from the bridge's `/tools` instead (`mcp::tool_policy_from`).
pub fn unlocked() -> bool {
    UNLOCKED.load(Ordering::SeqCst)
}

/// Save first, then publish: a switch that could not be saved is not
/// reported as on, only to come back off at the next launch. Before `init`
/// (tests) there is nowhere to save, and it changes in memory only.
pub fn set_unlocked(on: bool) -> Result<(), String> {
    if let Some(dir) = DIR.get() {
        save(dir, on)?;
    }
    UNLOCKED.store(on, Ordering::SeqCst);
    Ok(())
}
