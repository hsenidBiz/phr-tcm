//! An assistant's repair of an area's menu path (owner decision
//! 2026-10-20): `save_autorun_area` with `repair: { reason }` replaces the
//! clicks of an area that already exists - a person's included - when that
//! area is failing to be reached. Whether it is, is decided live: the
//! area's saved path is replayed from a fresh home the way a run takes it
//! (`nav::reach_module`, two tries with a reload between), and the repair
//! is refused while it still arrives. The browser trips live with the other
//! area saves in `ai_bridge`; this module holds the rules around them: the
//! reason, the repaired area as it is written, and the person's Put old
//! path back.

use super::nav::{self, AssistantRepair, ModulePath, NavFile};
use crate::browser::locator::Target;
use std::path::Path;

/// The most characters a repair's reason may have.
pub const MAX_REASON: usize = 200;

/// Said when the area's saved path still arrives: there is nothing to
/// repair, and nothing was written.
pub const NOTHING_TO_REPAIR: &str = "the area's path still works: nothing to repair";

/// Said when the new clicks are the ones already saved.
pub const SAME_PATH: &str = "the new path is the saved path: nothing to repair";

/// Said for a reason over `MAX_REASON` characters.
pub const LONG_REASON: &str = "a repair's reason is at most 200 characters";

/// The reason as it is kept, logged and answered: one line of at most
/// `MAX_REASON` characters (measured trimmed, before the scrub), then
/// scrubbed as every assistant-written note is - an address cut to its
/// path, a query dropped, tokens and JWTs hidden (`defects::scrub_note`),
/// and an address with no scheme cut to its path too
/// (`mapping_summary::without_addresses`). A reason with nothing left is
/// refused.
pub fn check_reason(reason: &str) -> Result<String, String> {
    let r = reason.trim();
    if r.is_empty() {
        return Err("a repair needs a reason: one line saying what changed in the menu".to_string());
    }
    if r.contains(['\n', '\r']) {
        return Err("a repair's reason is one line".to_string());
    }
    if r.chars().count() > MAX_REASON {
        return Err(LONG_REASON.to_string());
    }
    let scrubbed = super::mapping_summary::without_addresses(&super::defects::scrub_note(r)).trim().to_string();
    if scrubbed.is_empty() {
        return Err("a repair's reason says what changed in the menu, in words".to_string());
    }
    Ok(scrubbed)
}

/// Said to a repair that names no area the project has.
pub fn no_area_to_repair(name: &str) -> String {
    format!("there is no area named '{}' to repair - save a new area without `repair`", name.trim())
}

/// Said to a repair that names another module than the area's own.
pub fn other_module(area: &ModulePath) -> String {
    format!(
        "area '{}' is under {}, and a repair keeps it there - send that module",
        area.name(),
        area.module.trim()
    )
}

/// The assistant's answer to a repair that was saved.
pub fn repaired_answer(name: &str, reason: &str) -> String {
    format!("area \"{name}\" repaired: {reason}. The person can put the old path back in Auto Run, Areas.")
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

/// Write a repair once the browser trips are done, to the areas file as it
/// is NOW, never to the copy read before them: a person may have removed
/// the area, put an old path back or recorded it again meanwhile. `tried`
/// is the area as the trips found it; when the file no longer holds it
/// that way, nothing is written and the refusal says why.
#[allow(clippy::too_many_arguments)]
pub fn save_repair(
    root: &Path,
    org: &str,
    project: &str,
    tried: &ModulePath,
    clicks: Vec<Target>,
    arrived: String,
    start: String,
    reason: String,
    at: String,
) -> Result<NavFile, String> {
    let mut nav = nav::load_nav(root, org, project)?;
    let key = nav::module_key(tried.name());
    let Some(slot) = nav.modules.iter_mut().find(|m| nav::module_key(m.name()) == key) else {
        return Err(format!("area '{}' was removed while it was being repaired - nothing was saved", tried.name()));
    };
    if slot.clicks != tried.clicks
        || slot.arrived != tried.arrived
        || nav::module_key(&slot.module) != nav::module_key(&tried.module)
    {
        return Err(format!(
            "area '{}' changed while it was being repaired - nothing was saved; look at it again",
            slot.name()
        ));
    }
    *slot = repaired(slot, clicks, arrived, start, reason, at);
    nav::save_nav(root, org, project, &nav)?;
    Ok(nav)
}

/// Put back the path an assistant's repair replaced (the Areas dialog's
/// Put old path back), exactly as it was - its clicks, where they arrived
/// and the page they began on - and clear the note. Nothing is replayed:
/// the person chose it.
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
    slot.arrived = repair.previous_arrived;
    slot.start = repair.previous_start;
    nav::save_nav(root, org, project, &nav)?;
    Ok(nav)
}
