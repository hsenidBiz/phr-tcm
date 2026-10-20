//! An assistant's repair of an area's menu path (owner decision
//! 2026-10-20): `save_autorun_area` with `repair: { reason }` replaces the
//! clicks of an area that already exists - a person's included - when that
//! area is failing to be reached. The saving itself (the replays from a
//! fresh home) lives with the other area saves in `ai_bridge`; this module
//! holds the rules around it: the reason, whether the area is failing to be
//! reached by the runs' own record, the repaired area as it is written, and
//! the person's Put old path back.

use super::nav::{self, AssistantRepair, ModulePath, NavFile};
use super::replay::MODULE_STEP;
use super::LocalRun;
use crate::browser::actions::ActionOutcome;
use crate::browser::locator::Target;
use std::path::Path;

/// The most characters a repair's reason may have.
pub const MAX_REASON: usize = 200;

/// Said when the area's saved path still arrives: there is nothing to
/// repair, and nothing was written.
pub const NOTHING_TO_REPAIR: &str = "the area's path still works: nothing to repair";

/// The reason as it is kept: trimmed, one line, at most `MAX_REASON`
/// characters, and in words - an address would land in the log.
pub fn check_reason(reason: &str) -> Result<String, String> {
    let r = reason.trim();
    if r.is_empty() {
        return Err("a repair needs a reason: one line saying what changed in the menu".to_string());
    }
    if r.contains(['\n', '\r']) {
        return Err("a repair's reason is one line".to_string());
    }
    if r.chars().count() > MAX_REASON {
        return Err(format!("a repair's reason is at most {MAX_REASON} characters"));
    }
    if r.contains("://") {
        return Err("a repair's reason says what changed in the menu in words, with no address".to_string());
    }
    Ok(r.to_string())
}

/// Said to a repair that names no area the project has.
pub fn no_area_to_repair(name: &str) -> String {
    format!("there is no area named '{}' to repair - save a new area without `repair`", name.trim())
}

/// Said to a repair that names another module than the area's own.
pub fn other_module(area: &ModulePath) -> String {
    format!(
        "area '{}' is under {}, and a repair keeps it there - send that module, or none",
        area.name(),
        area.module.trim()
    )
}

/// The assistant's answer to a repair that was saved.
pub fn repaired_answer(name: &str, reason: &str) -> String {
    format!("area \"{name}\" repaired: {reason}. The person can put the old path back in Auto Run, Areas.")
}

/// When the area's current path was saved: its latest repair, else its
/// recording.
fn current_since(area: &ModulePath) -> &str {
    area.repaired_by_assistant.as_ref().map_or(area.recorded.as_str(), |r| r.at.as_str())
}

/// What one of a run's "Go to" outcomes says about a trip to `name`:
/// `Some(true)` it arrived, `Some(false)` it did not reach it, `None` it
/// was a trip somewhere else.
fn trip_to(name: &str, o: &ActionOutcome) -> Option<bool> {
    let key = nav::module_key(name);
    if o.ok {
        let went = o.detail.strip_prefix("Go to ")?;
        return (nav::module_key(went) == key).then_some(true);
    }
    let rest = o.detail.strip_prefix(nav::UNREACHED_PREFIX)?;
    let (named, _) = rest.split_once("\":")?;
    (nav::module_key(named) == key).then_some(false)
}

/// Whether the latest trip a saved run or replay made to `area`, since the
/// area's current path was saved, failed to reach it ("Could not reach
/// module ..."). `runs` newest first, as `store::list_runs` gives them. Only
/// the run's own "Go to" step is read - never a script step's words, which
/// can quote the page. A run whose start cannot be read is passed over.
pub fn last_trip_unreached(runs: &[LocalRun], area: &ModulePath) -> bool {
    let since = current_since(area);
    let name = area.name();
    for run in runs {
        let Ok(ms) = run.started_at.trim().parse::<u64>() else { continue };
        let started = crate::commands::queue::iso_utc((ms / 1000) as i64);
        if !since.is_empty() && started.as_str() < since {
            continue;
        }
        let last = run
            .cases
            .iter()
            .rev()
            .flat_map(|c| c.steps.iter().rev().filter(|s| s.step_number == MODULE_STEP))
            .flat_map(|s| s.outcomes.iter().rev())
            .find_map(|o| trip_to(name, o));
        if let Some(arrived) = last {
            return !arrived;
        }
    }
    false
}

/// The area after a repair: the new clicks, where they arrive and the page
/// they begin on, and the note that keeps what it held before. Its name,
/// module, who made it and when it was recorded stay as they were.
pub fn repaired(old: &ModulePath, clicks: Vec<Target>, arrived: String, start: String, reason: String, at: String) -> ModulePath {
    ModulePath {
        clicks,
        arrived,
        start,
        repaired_by_assistant: Some(AssistantRepair {
            previous_clicks: old.clicks.clone(),
            previous_arrived: old.arrived.clone(),
            previous_start: old.start.clone(),
            reason,
            at,
        }),
        ..old.clone()
    }
}

/// Put back the path an assistant's repair replaced (the Areas dialog's
/// Put old path back), and clear the note. Nothing is replayed: the person
/// chose it.
pub fn put_old_path_back(root: &Path, org: &str, project: &str, area: &str) -> Result<NavFile, String> {
    let mut nav = nav::load_nav(root, org, project)?;
    let key = nav::module_key(area);
    let slot = nav
        .modules
        .iter_mut()
        .find(|m| nav::module_key(m.name()) == key)
        .ok_or_else(|| format!("there is no area named \"{}\"", area.trim()))?;
    let Some(repair) = slot.repaired_by_assistant.take() else {
        return Err(format!("area \"{}\" has no repaired path to put back", slot.name()));
    };
    slot.clicks = repair.previous_clicks;
    if !repair.previous_arrived.is_empty() {
        slot.arrived = repair.previous_arrived;
    }
    slot.start = repair.previous_start;
    nav::save_nav(root, org, project, &nav)?;
    Ok(nav)
}
