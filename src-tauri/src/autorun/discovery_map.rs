//! The per-project discovery map: what Auto Run has seen on the live
//! application, area by area, so a script is written against elements that
//! exist rather than guessed ones.
//!
//! Saved beside the project's other files as `<slug>-map.json`. It keeps
//! page PATHS (no host, query or fragment) and locators only - never a page
//! URL, a typed value or any text that came from a record. An area whose
//! `explored_at` is old, or which has failed since, is stale and wants
//! exploring again. `""` is the bucket for sightings that belong to no area.

use super::recipe::project_slug;
use crate::browser::locator::{LocatorStep, SeenKey, Target};
use crate::browser::snapshot::SnapLine;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const STALE_AFTER_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const MAX_OUTCOMES: usize = 200;

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, Default, PartialEq)]
pub struct DiscoveryMap {
    pub areas: Vec<AreaMap>,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, Default, PartialEq)]
pub struct AreaMap {
    /// `""` is the unattributed bucket.
    pub area: String,
    pub explored_at: Option<u64>,
    pub account: Option<String>,
    pub failed_since: bool,
    pub pages: Vec<PageMap>,
    pub outcomes: Vec<String>,
    pub writes: Vec<WriteEntry>,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, Default, PartialEq)]
pub struct PageMap {
    pub path: String,
    pub title: String,
    pub elements: Vec<SeenElement>,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct SeenElement {
    pub key: SeenKey,
    pub locator: Target,
    pub role: String,
    pub name: String,
    /// `button | field | link | table | dialog | other`, from the role.
    pub kind: String,
    pub required: bool,
    /// When a probe or a try last matched it on the live page, in
    /// milliseconds since the epoch; 0 when only a page read showed it. A
    /// discovery's later read of the page keeps an element it no longer
    /// shows only when this falls inside that discovery.
    #[serde(default)]
    #[specta(type = f64)]
    pub seen_at: u64,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct WriteEntry {
    pub method: String,
    pub path: String,
    /// Milliseconds since the epoch; a JavaScript number holds it exactly.
    #[specta(type = f64)]
    pub at: u64,
    pub step: String,
}

pub fn map_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}-map.json", project_slug(org, project)))
}

/// Every read-change-write of a map holds this: a discovery, a probe, a
/// run's evidence and the person's "forget" can land together.
fn write_lock() -> &'static Mutex<()> {
    static L: Mutex<()> = Mutex::new(());
    &L
}

/// The map's file as the person can find it under the Auto Run folder:
/// `projects/<slug>-map.json`. A refusal names this, never a full path,
/// which would carry the person's user folder.
pub fn map_file_name(org: &str, project: &str) -> String {
    format!("projects/{}-map.json", project_slug(org, project))
}

/// Why the map could not be loaded: its file holds something that is not a
/// map (`Damaged`), or the file could not be read at all.
enum MapError {
    Damaged(String),
    Unreadable(String),
}

fn read_map(root: &Path, org: &str, project: &str) -> Result<DiscoveryMap, MapError> {
    match std::fs::read_to_string(map_path(root, org, project)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| MapError::Damaged(e.to_string()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DiscoveryMap::default()),
        Err(e) => Err(MapError::Unreadable(e.to_string())),
    }
}

pub fn load_map(root: &Path, org: &str, project: &str) -> Result<DiscoveryMap, String> {
    read_map(root, org, project).map_err(|e| {
        let file = map_file_name(org, project);
        match e {
            MapError::Damaged(why) => format!(
                "The discovery map {file} is damaged and could not be read ({why}). Reset map in Auto Run, Setup, Discovery moves it aside and starts an empty one."
            ),
            MapError::Unreadable(why) => format!("The discovery map {file} could not be read: {why}"),
        }
    })
}

/// Move a damaged map aside, as `<slug>-map.corrupt-<now>.json` beside it,
/// so discovery starts an empty one: the person's way out of a file nothing
/// can read. It is never deleted. Hands back the name it was moved to
/// (project-relative), or `None` when there was no file. A map that reads
/// is refused: Forget map clears an area of a healthy one.
pub fn reset_map(root: &Path, org: &str, project: &str, now: u64) -> Result<Option<String>, String> {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    let from = map_path(root, org, project);
    if !from.exists() {
        return Ok(None);
    }
    if read_map(root, org, project).is_ok() {
        return Err("The discovery map can be read, so there is nothing to reset. Forget map clears one area.".to_string());
    }
    let aside = format!("{}-map.corrupt-{now}.json", project_slug(org, project));
    std::fs::rename(&from, from.with_file_name(&aside))
        .map_err(|e| format!("The discovery map {} could not be moved aside: {e}", map_file_name(org, project)))?;
    crate::applog::info(format!("Discovery map: a damaged map was moved aside as projects/{aside}"));
    Ok(Some(format!("projects/{aside}")))
}

/// The one form an area name is compared in: case and runs of spaces
/// folded, as the recorded areas are (`nav::module_key`). `""` is the
/// unattributed bucket.
pub fn area_key(name: &str) -> String {
    super::nav::module_key(name)
}

/// The name an area is filed under: the recorded area's own name when one
/// is named like `name` (`nav::find_area`), else `name` trimmed.
pub fn canonical_area(root: &Path, org: &str, project: &str, name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return String::new();
    }
    super::nav::load_nav(root, org, project)
        .ok()
        .and_then(|nav| super::nav::find_area(&nav, name).map(|m| m.name().to_string()))
        .unwrap_or_else(|| name.to_string())
}

/// Load, change and write back under the lock.
fn update(root: &Path, org: &str, project: &str, change: impl FnOnce(&mut DiscoveryMap)) -> Result<(), String> {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut map = load_map(root, org, project)?;
    change(&mut map);
    let path = map_path(root, org, project);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&map).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path, &text)
}

/// The map's entry for `area`, compared by `area_key`. A new entry is filed
/// under `area` as given, which callers make canonical (`canonical_area`).
fn area_mut<'a>(map: &'a mut DiscoveryMap, area: &str) -> &'a mut AreaMap {
    let key = area_key(area);
    if let Some(i) = map.areas.iter().position(|a| area_key(&a.area) == key) {
        return &mut map.areas[i];
    }
    map.areas.push(AreaMap { area: area.trim().to_string(), ..AreaMap::default() });
    map.areas.last_mut().expect("just pushed")
}

fn page_mut<'a>(area: &'a mut AreaMap, path: &str, title: &str) -> &'a mut PageMap {
    if let Some(i) = area.pages.iter().position(|p| p.path == path) {
        let page = &mut area.pages[i];
        if page.title.is_empty() && !title.is_empty() {
            page.title = title.to_string();
        }
        return page;
    }
    area.pages.push(PageMap { path: path.to_string(), title: title.to_string(), elements: vec![] });
    area.pages.last_mut().expect("just pushed")
}

fn kind_for(role: &str) -> &'static str {
    match role {
        "button" => "button",
        "textbox" | "combobox" | "searchbox" | "spinbutton" | "checkbox" | "radio" | "listbox" | "slider"
        | "switch" => "field",
        "link" => "link",
        "table" | "grid" | "treegrid" => "table",
        "dialog" | "alertdialog" => "dialog",
        _ => "other",
    }
}

/// The path of a page address: no scheme, host, query or fragment.
pub fn path_only(url_or_path: &str) -> String {
    let s = url_or_path.trim();
    let s = s.split(['?', '#']).next().unwrap_or("");
    let rest = match s.find("://") {
        Some(i) => {
            let after = &s[i + 3..];
            after.find('/').map(|j| &after[j..]).unwrap_or("")
        }
        None => s,
    };
    if rest.is_empty() {
        "/".to_string()
    } else {
        rest.to_string()
    }
}

/// Files what one read of a page showed. `discovery` is the start of the
/// discovery under way, or `None` for any other read (a supervised page
/// read while healing, a replay). A discovery's read replaces the page's
/// elements with what it showed, keeping only those a probe or a try
/// matched during this same discovery (`seen_at` at or after its start),
/// since a long page's read is cut off before its end. Any other read only
/// adds.
#[allow(clippy::too_many_arguments)]
pub fn record_seen(
    root: &Path,
    org: &str,
    project: &str,
    area: Option<&str>,
    page_path: &str,
    title: &str,
    lines: &[SnapLine],
    account: Option<&str>,
    discovery: Option<u64>,
    now: u64,
) -> Result<(), String> {
    let path = path_only(page_path);
    let area = canonical_area(root, org, project, area.unwrap_or(""));
    update(root, org, project, |map| {
        let a = area_mut(map, &area);
        // Only what a discovery sees in a named area marks that area
        // explored: the bucket for no area is never explored.
        if discovery.is_some() && !area.is_empty() {
            a.explored_at = Some(now);
            a.failed_since = false;
            a.account = account.map(str::to_string);
        }
        let page = page_mut(a, &path, title);
        let old = match discovery {
            Some(_) => std::mem::take(&mut page.elements),
            None => vec![],
        };
        for line in lines {
            // The last link names the element; earlier links are frames.
            let Some(key) = line.locator.links().last().and_then(LocatorStep::seen_key) else { continue };
            // The full locator is the identity: the same name in two frames
            // is two elements.
            if page.elements.iter().any(|e| e.locator == line.locator) {
                continue;
            }
            let seen_at = old.iter().find(|e| e.locator == line.locator).map_or(0, |e| e.seen_at);
            page.elements.push(SeenElement {
                key,
                locator: line.locator.clone(),
                role: line.role.clone(),
                name: line.name.clone(),
                kind: kind_for(&line.role).to_string(),
                required: line.required,
                seen_at,
            });
        }
        if let Some(started) = discovery {
            for e in old {
                let matched_now = e.seen_at != 0 && e.seen_at >= started;
                if matched_now && !page.elements.iter().any(|n| n.locator == e.locator) {
                    page.elements.push(e);
                }
            }
        }
    })
}

/// Adds each link of a target a probe or try matched, unless the page
/// already has it, and stamps it matched at `now` (`seen_at`) either way.
pub fn record_matched(
    root: &Path,
    org: &str,
    project: &str,
    area: Option<&str>,
    page_path: &str,
    target: &Target,
    now: u64,
) -> Result<(), String> {
    let path = path_only(page_path);
    let area = canonical_area(root, org, project, area.unwrap_or(""));
    update(root, org, project, |map| {
        let a = area_mut(map, &area);
        let page = page_mut(a, &path, "");
        for link in target.links() {
            let Some(key) = link.seen_key() else { continue };
            let mut held = false;
            for e in page.elements.iter_mut().filter(|e| e.key == key) {
                e.seen_at = e.seen_at.max(now);
                held = true;
            }
            if held {
                continue;
            }
            let role = link.role.clone().unwrap_or_default();
            let kind = if role.is_empty() { "other" } else { kind_for(&role) };
            page.elements.push(SeenElement {
                key,
                role,
                name: link.name.clone().unwrap_or_default(),
                kind: kind.to_string(),
                required: false,
                seen_at: now,
                locator: Target::One(link),
            });
        }
    })
}

pub fn record_write(root: &Path, org: &str, project: &str, area: &str, w: WriteEntry) -> Result<(), String> {
    let w = WriteEntry { path: path_only(&w.path), ..w };
    let area = canonical_area(root, org, project, area);
    update(root, org, project, |map| area_mut(map, &area).writes.push(w))
}

pub fn record_outcome(root: &Path, org: &str, project: &str, area: &str, line: &str) -> Result<(), String> {
    let area = canonical_area(root, org, project, area);
    update(root, org, project, |map| {
        let a = area_mut(map, &area);
        a.outcomes.push(line.to_string());
        if a.outcomes.len() > MAX_OUTCOMES {
            let drop = a.outcomes.len() - MAX_OUTCOMES;
            a.outcomes.drain(..drop);
        }
    })
}

pub fn mark_failed(root: &Path, org: &str, project: &str, area: &str) -> Result<(), String> {
    let area = canonical_area(root, org, project, area);
    update(root, org, project, |map| area_mut(map, &area).failed_since = true)
}

pub fn forget_area(root: &Path, org: &str, project: &str, area: &str) -> Result<(), String> {
    let key = area_key(area);
    update(root, org, project, |map| map.areas.retain(|a| area_key(&a.area) != key))
}

/// Never explored, failed since, or explored more than 30 days ago.
pub fn is_stale(a: &AreaMap, now: u64) -> bool {
    match a.explored_at {
        None => true,
        Some(at) => a.failed_since || now.saturating_sub(at) > STALE_AFTER_MS,
    }
}

/// Why `a` is stale at `now`, or `None` when it is not.
pub fn stale_reason(a: &AreaMap, now: u64) -> Option<&'static str> {
    if !is_stale(a, now) {
        None
    } else if a.explored_at.is_none() {
        Some("no map yet")
    } else if a.failed_since {
        Some("a script failed there since it was explored")
    } else {
        Some("explored more than 30 days ago")
    }
}

/// Every key seen in the named areas and in the unattributed bucket.
pub fn seen_keys(map: &DiscoveryMap, areas: &[&str]) -> HashSet<SeenKey> {
    let wanted: Vec<String> = areas.iter().map(|a| area_key(a)).collect();
    map.areas
        .iter()
        .filter(|a| {
            let key = area_key(&a.area);
            key.is_empty() || wanted.contains(&key)
        })
        .flat_map(|a| a.pages.iter())
        .flat_map(|p| p.elements.iter())
        .flat_map(|e| {
            let mut keys: Vec<SeenKey> = e.locator.links().iter().filter_map(LocatorStep::seen_key).collect();
            keys.push(e.key.clone());
            keys
        })
        .collect()
}

pub fn seen_paths(map: &DiscoveryMap) -> HashSet<String> {
    map.areas.iter().flat_map(|a| a.pages.iter()).map(|p| p.path.clone()).collect()
}

/// The live guide's `## Areas to explore`: each of `areas` (the recorded
/// areas, by name) whose map is missing or stale at `now`, with why. Empty
/// when there is nothing to explore, so the guide gains no empty heading.
pub fn explore_section(areas: &[&str], map: &DiscoveryMap, now: u64) -> String {
    let mut lines = String::new();
    for name in areas {
        let key = area_key(name);
        let reason = match map.areas.iter().find(|a| area_key(&a.area) == key) {
            None => "no map yet",
            Some(a) => match stale_reason(a, now) {
                Some(why) => why,
                None => continue,
            },
        };
        lines.push_str(&format!("- {name}: {reason}\n"));
    }
    if lines.is_empty() {
        return lines;
    }
    format!(
        "## Areas to explore\n\n\
         These areas have no map yet, or a stale one. Explore an area with `start_autorun_discovery` \
         before you write or repair a script there (see \"Discovering the app\").\n\n{lines}"
    )
}
