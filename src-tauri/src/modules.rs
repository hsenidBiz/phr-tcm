//! Modules: five optional areas arranged as a tech tree, chosen per machine.
//!
//! Core (test cases, suites, runs and the board) is always on and is not a
//! module. Database Access and Writing Style hang off Core; Auto Run hangs
//! off Database Access; API Templates and Playwright hang off Auto Run. A
//! module can be on only if its parent is on, and switching one off switches
//! everything under it off. Off hides, never deletes.
//!
//! Stored in `modules.json` in the app data dir, written atomically like
//! `extras.json`, and held in memory too: the AI bridge asks on every
//! `tools/list`. Not part of a backup.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const FILE: &str = "modules.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Module {
    DbAccess,
    WritingStyle,
    AutoRun,
    ApiTemplates,
    Playwright,
}

impl Module {
    pub const ALL: [Module; 5] = [
        Module::DbAccess,
        Module::WritingStyle,
        Module::AutoRun,
        Module::ApiTemplates,
        Module::Playwright,
    ];

    /// The module this one needs; `None` means it hangs off Core.
    pub fn parent(self) -> Option<Module> {
        match self {
            Module::DbAccess | Module::WritingStyle => None,
            Module::AutoRun => Some(Module::DbAccess),
            Module::ApiTemplates | Module::Playwright => Some(Module::AutoRun),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Module::DbAccess => "Database Access",
            Module::WritingStyle => "Writing Style",
            Module::AutoRun => "Auto Run",
            Module::ApiTemplates => "API Templates",
            Module::Playwright => "Playwright",
        }
    }

    pub fn children(self) -> Vec<Module> {
        Module::ALL.into_iter().filter(|m| m.parent() == Some(self)).collect()
    }

    /// Every descendant, however deep.
    pub fn below(self) -> Vec<Module> {
        let mut out = Vec::new();
        for c in self.children() {
            out.push(c);
            out.extend(c.below());
        }
        out
    }
}

/// What a first-time choice holds. A development build gets everything; an
/// existing install keeps what it used (Database Access and Writing Style,
/// plus the Auto Run family if Advanced Features or the extras were on); a
/// fresh install starts with nothing.
pub fn initial(existing_install: bool, advanced: bool, unlocked: bool, dev: bool) -> BTreeSet<Module> {
    if dev {
        return Module::ALL.into_iter().collect();
    }
    let mut on = BTreeSet::new();
    if existing_install {
        on.insert(Module::DbAccess);
        on.insert(Module::WritingStyle);
        if advanced || unlocked {
            on.insert(Module::AutoRun);
            on.insert(Module::ApiTemplates);
            on.insert(Module::Playwright);
        }
    }
    on
}

/// Pure: switch `m` on or off in `on`. On with the parent off is refused;
/// off removes `m` and everything below it.
pub fn apply(on: &BTreeSet<Module>, m: Module, want: bool) -> Result<BTreeSet<Module>, String> {
    let mut next = on.clone();
    if want {
        if let Some(p) = m.parent() {
            if !on.contains(&p) {
                return Err(format!("\"{}\" needs \"{}\" switched on first.", m.name(), p.name()));
            }
        }
        next.insert(m);
    } else {
        next.remove(&m);
        for d in m.below() {
            next.remove(&d);
        }
    }
    Ok(next)
}

#[derive(Serialize, Deserialize)]
struct Disk {
    on: BTreeSet<Module>,
}

fn read(dir: &Path) -> Option<BTreeSet<Module>> {
    let s = std::fs::read_to_string(dir.join(FILE)).ok()?;
    serde_json::from_str::<Disk>(&s).ok().map(|d| d.on)
}

fn write(dir: &Path, on: &BTreeSet<Module>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let body = serde_json::to_string(&Disk { on: on.clone() }).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&dir.join(FILE), &body)
}

/// Save the choice into `dir` (created if missing), atomically.
pub fn save(dir: &Path, on: &BTreeSet<Module>) -> Result<(), String> {
    write(dir, on)
}

/// What `dir` holds. Missing or unreadable: decide from the old flags
/// (never "everything off" on an existing install), save and return it.
pub fn load_or_create(dir: &Path, dev: bool) -> BTreeSet<Module> {
    if let Some(on) = read(dir) {
        return on;
    }
    let existing = dir.join("extras.json").exists() || dir.join("app-settings.json").exists();
    let on = initial(existing, crate::extras::load_advanced(dir), crate::extras::load(dir), dev);
    // A failed save is not fatal: the same answer is rebuilt next launch.
    let _ = write(dir, &on);
    on
}

static STATE: Mutex<BTreeSet<Module>> = Mutex::new(BTreeSet::new());
static DIR: OnceLock<PathBuf> = OnceLock::new();

fn state() -> std::sync::MutexGuard<'static, BTreeSet<Module>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Called once from setup with the app data dir.
pub fn init(dir: PathBuf) {
    *state() = load_or_create(&dir, crate::ai_tools::dev_build());
    let _ = DIR.set(dir);
}

/// Whether `m` is on in this process. Always false in the `--mcp` proxy
/// process, which never runs setup - it learns the app's answer from the
/// bridge's `/tools` instead.
pub fn on(m: Module) -> bool {
    state().contains(&m)
}

pub fn current() -> BTreeSet<Module> {
    state().clone()
}

/// Save first, then publish, like `extras::set_advanced`.
pub fn set(m: Module, want: bool) -> Result<BTreeSet<Module>, String> {
    let dir = DIR.get().ok_or_else(|| "could not save: setup has not finished yet".to_string())?;
    let mut guard = state();
    let next = apply(&guard, m, want)?;
    write(dir, &next)?;
    *guard = next.clone();
    Ok(next)
}
