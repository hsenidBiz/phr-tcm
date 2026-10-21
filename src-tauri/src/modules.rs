//! Modules: six optional areas arranged as a tech tree, chosen per machine.
//!
//! Core (test cases, suites, runs and the board) is always on and is not a
//! module. Database Access and Writing Style hang off Core; Auto Run hangs
//! off Database Access; API Templates and Playwright hang off Auto Run;
//! Crystal Reports hangs off Core on its own. A module can be on only if its parent is on, and switching one off switches
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
    CrystalReports,
}

impl Module {
    pub const ALL: [Module; 6] = [
        Module::DbAccess,
        Module::WritingStyle,
        Module::AutoRun,
        Module::ApiTemplates,
        Module::Playwright,
        Module::CrystalReports,
    ];

    /// The module this one needs; `None` means it hangs off Core.
    pub fn parent(self) -> Option<Module> {
        match self {
            Module::DbAccess | Module::WritingStyle | Module::CrystalReports => None,
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
            Module::CrystalReports => "Crystal Reports",
        }
    }

    /// The stored and wire name: `db_access`, `writing_style`, `auto_run`,
    /// `api_templates`, `playwright`, `crystal_reports` - what serde writes, spelled out so a
    /// URL or a log line needs no serialiser.
    pub fn key(self) -> &'static str {
        match self {
            Module::DbAccess => "db_access",
            Module::WritingStyle => "writing_style",
            Module::AutoRun => "auto_run",
            Module::ApiTemplates => "api_templates",
            Module::Playwright => "playwright",
            Module::CrystalReports => "crystal_reports",
        }
    }

    /// The module whose stored or wire name is `key`; `None` for anything
    /// else (an unknown name from a newer app is ignored, never guessed at).
    pub fn from_key(key: &str) -> Option<Module> {
        Module::ALL.into_iter().find(|m| m.key() == key)
    }

    /// What every gate says when it refuses because this module is off.
    pub fn off_sentence(self) -> String {
        self.off_sentence_for(crate::extras::unlocked())
    }

    /// `off_sentence` with the unlock state passed in. While the held-back
    /// module is locked, nothing an assistant or person can read names it.
    pub fn off_sentence_for(self, unlocked: bool) -> String {
        if !unlocked && self == HIDDEN_UNTIL_UNLOCKED {
            return NOT_AVAILABLE.to_string();
        }
        format!("{} is switched off in Modules", self.name())
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
/// fresh install starts with nothing. Crystal Reports is new, so no old flag
/// switches it on: only a development build starts with it.
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

/// The names are read as plain strings and mapped through `Module::from_key`,
/// so a module a newer app wrote (unknown here) is dropped while every module
/// this app knows is kept - the file is never failed whole.
#[derive(Serialize, Deserialize)]
struct Disk {
    on: Vec<String>,
}

/// Drop any module whose parent is off, so a hand-edited or damaged file -
/// or a `/tools` answer the `--mcp` proxy reads - can never hold a state the
/// tree forbids.
pub fn normalise(on: BTreeSet<Module>) -> BTreeSet<Module> {
    // ALL lists parents before children.
    Module::ALL.into_iter().filter(|m| on.contains(m)).fold(BTreeSet::new(), |mut out, m| {
        if m.parent().map_or(true, |p| out.contains(&p)) {
            out.insert(m);
        }
        out
    })
}

fn read(dir: &Path) -> Option<BTreeSet<Module>> {
    let s = std::fs::read_to_string(dir.join(FILE)).ok()?;
    serde_json::from_str::<Disk>(&s).ok().map(|d| normalise(d.on.iter().filter_map(|n| Module::from_key(n)).collect()))
}

/// The choice saved in `dir`, mended; `None` when there is no file or it
/// cannot be read. Creates and writes nothing: this is what the `--mcp`
/// proxy reads when the app cannot be asked.
pub fn saved(dir: &Path) -> Option<BTreeSet<Module>> {
    read(dir)
}

/// The bundle identifier (`tauri.conf.json`), which Tauri appends to the
/// roaming app-data folder to make the app data dir.
pub const APP_IDENTIFIER: &str = "com.avinalwis.testcasemanager.v2";

/// Where the app keeps its data, given the roaming app-data folder
/// (`%APPDATA%`): what `app_data_dir()` resolves to, for a process that has
/// no Tauri handle. `None` without that folder.
pub fn data_dir_in(roaming: Option<std::ffi::OsString>) -> Option<PathBuf> {
    roaming.filter(|r| !r.is_empty()).map(|r| PathBuf::from(r).join(APP_IDENTIFIER))
}

/// The modules saved on this machine, for the `--mcp` proxy (see `saved`).
pub fn saved_on_this_machine() -> Option<BTreeSet<Module>> {
    saved(&data_dir_in(std::env::var_os("APPDATA"))?)
}

/// Whether `dir` holds any app data from an earlier launch. A person who
/// never changed a setting may have only the cache, so any of these counts.
pub fn existing_install(dir: &Path) -> bool {
    ["extras.json", "app-settings.json", "cache.json", "databases.json", "writing-style.json", "autorun"]
        .iter()
        .any(|n| dir.join(n).exists())
}

fn write(dir: &Path, on: &BTreeSet<Module>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let body = serde_json::to_string(&Disk { on: on.iter().map(|m| m.key().to_string()).collect() }).map_err(|e| e.to_string())?;
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
    let on = initial(existing_install(dir), crate::extras::load_advanced(dir), crate::extras::load(dir), dev);
    // A failed save is not fatal (the same answer is rebuilt next launch) but is logged.
    if let Err(e) = write(dir, &on) {
        crate::applog::warn(format!("modules: could not save modules.json: {e}"));
    }
    on
}

/// `None` until setup has run.
static STATE: Mutex<Option<BTreeSet<Module>>> = Mutex::new(None);
static DIR: OnceLock<PathBuf> = OnceLock::new();

fn state() -> std::sync::MutexGuard<'static, Option<BTreeSet<Module>>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a process reads before setup has run: everything in a development
/// build (what a development build's first `modules.json` holds), nothing
/// in a release build. Only processes that never run setup see it - the
/// `--mcp` proxy, which never asks (it learns the app's answer from the
/// bridge's `/tools`), and the test binary, whose route tests reach every
/// gate the way a development build does.
pub fn before_setup(dev: bool) -> BTreeSet<Module> {
    if dev {
        Module::ALL.into_iter().collect()
    } else {
        BTreeSet::new()
    }
}

/// Called once from setup with the app data dir.
pub fn init(dir: PathBuf) {
    *state() = Some(load_or_create(&dir, crate::ai_tools::dev_build()));
    let _ = DIR.set(dir);
}

/// Whether `m` is on in this process - what every Rust gate asks.
pub fn on(m: Module) -> bool {
    current().contains(&m)
}

/// The module only a machine that unlocked it in Settings is offered.
pub const HIDDEN_UNTIL_UNLOCKED: Module = Module::CrystalReports;

/// What is offered: the saved choice, less the module that is held back
/// while this machine has not unlocked it. The saved choice itself is never
/// touched, so it comes back the moment the machine unlocks.
pub fn effective(saved: &BTreeSet<Module>, unlocked: bool) -> BTreeSet<Module> {
    let mut on = saved.clone();
    if !unlocked {
        on.remove(&HIDDEN_UNTIL_UNLOCKED);
    }
    on
}

/// The sentence for switching on a module this machine is not offered.
pub const NOT_AVAILABLE: &str = "That module is not available on this machine.";

/// `Err(NOT_AVAILABLE)` for switching the held-back module on while locked.
pub fn refuse_if_locked(m: Module, want: bool, unlocked: bool) -> Result<(), String> {
    if want && !unlocked && m == HIDDEN_UNTIL_UNLOCKED {
        return Err(NOT_AVAILABLE.to_string());
    }
    Ok(())
}

/// The saved choice in this process, before the unlock rule.
fn saved_now() -> BTreeSet<Module> {
    state().clone().unwrap_or_else(|| before_setup(crate::ai_tools::dev_build()))
}

/// The modules on in this process.
pub fn current() -> BTreeSet<Module> {
    effective(&saved_now(), crate::extras::unlocked())
}

/// `Err(m.off_sentence())` unless `on`: the refusal an IPC command gives
/// while its module is off. `on` is passed in so both answers are testable.
pub fn require(m: Module, on: bool) -> Result<(), String> {
    if on {
        Ok(())
    } else {
        Err(m.off_sentence())
    }
}

/// Pure half of `set`: switch `m` in `state`, saving into `dir` first and
/// changing `state` only once the save has worked - a switch that could
/// not be saved is not reported as made, only to come back at the next
/// launch.
pub fn set_in(state: &mut BTreeSet<Module>, dir: &Path, m: Module, want: bool) -> Result<BTreeSet<Module>, String> {
    let next = apply(state, m, want)?;
    write(dir, &next)?;
    *state = next.clone();
    Ok(next)
}

/// Save first, then publish, like `extras::set_unlocked` (`set_in`).
pub fn set(m: Module, want: bool) -> Result<BTreeSet<Module>, String> {
    let unlocked = crate::extras::unlocked();
    refuse_if_locked(m, want, unlocked)?;
    let dir = DIR.get().ok_or_else(|| "could not save: setup has not finished yet".to_string())?;
    let mut guard = state();
    let mut now = guard.clone().unwrap_or_else(|| before_setup(crate::ai_tools::dev_build()));
    let next = set_in(&mut now, dir, m, want)?;
    *guard = Some(next.clone());
    Ok(effective(&next, unlocked))
}
