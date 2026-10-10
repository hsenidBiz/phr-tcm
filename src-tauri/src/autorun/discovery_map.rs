//! The per-project discovery map: what Auto Run has seen on the live
//! application, area by area, so a script is written against elements that
//! exist rather than guessed ones.
//!
//! Saved beside the project's other files as `<slug>-map.json`. It keeps
//! page PATHS (no host, query or fragment) and locators only - never a page
//! URL, a typed value or any text that came from a record. An area whose
//! `explored_at` is old, or which has failed since, is stale and wants
//! exploring again. `""` is the bucket for sightings that belong to no area.
//!
//! Each area also keeps its `sightings`: every distinct link it has seen,
//! once each by its key (`SeenKey`), with when it was last seen. A page's
//! `elements` are what the page holds now and a read replaces them, so a
//! wizard that keeps one address for every step forgets the earlier steps
//! as it moves on; its sightings do not. A sighting is never dropped for
//! the number of newer ones. It goes when its area is forgotten, or when a
//! later discovery reads its whole page and it was neither shown by that
//! read nor seen since that discovery started (exploring again replaces, as
//! for `elements`). A read cut short by its line limit drops nothing.
//! `MAX_SIGHTINGS` is only a safety ceiling on the file's size: past it,
//! the least recently seen sightings go first.

use super::recipe::project_slug;
use crate::browser::locator::{LocatorStep, SeenKey, Target};
use crate::browser::snapshot::SnapLine;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const STALE_AFTER_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// The area's `outcomes` are lines of prose saying what an action led to,
/// shown as evidence; the seen check never reads them, so they stay capped.
const MAX_OUTCOMES: usize = 200;
/// The most distinct sightings one area keeps. Far above what an area
/// holds; past it the least recently seen go first (`trim_sightings`).
pub const MAX_SIGHTINGS: usize = 5000;
const MAX_WRITES: usize = 500;

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
    /// Every distinct link seen in the area, once each by its key (see the
    /// module's notes). A map written before these were kept has none.
    #[serde(default)]
    pub sightings: Vec<Sighting>,
    /// The links the area's saved scripts use, as written: read in memory
    /// by the seen check (`seen_check::add_saved_scripts`), never saved.
    #[serde(skip)]
    pub saved_links: Vec<LocatorStep>,
    /// The page the area's recorded route arrives on (`page_path` of its
    /// `arrived`), empty when it is not known: read in memory by the seen
    /// check (`seen_check::add_area_pages`) to prefer a "did you mean" seen
    /// on that page, never saved.
    #[serde(skip)]
    pub area_page: String,
}

/// One distinct link an area has seen.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct Sighting {
    pub key: SeenKey,
    /// The link as it was written when it was last seen.
    pub link: LocatorStep,
    /// The page (`page_path`) it was last seen on.
    pub page: String,
    /// When it was last seen, in milliseconds since the epoch.
    #[specta(type = f64)]
    pub last_seen: u64,
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

/// Load, change and write back under the lock. A change that leaves the
/// map as it was writes nothing: most page reads see nothing new.
fn update(root: &Path, org: &str, project: &str, change: impl FnOnce(&mut DiscoveryMap)) -> Result<(), String> {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut map = load_map(root, org, project)?;
    let before = map.clone();
    change(&mut map);
    if map == before {
        return Ok(());
    }
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

/// The page filed at `path` (a `page_path`). A page an older map filed
/// with its ids is found too, and takes the collapsed path.
fn page_mut<'a>(area: &'a mut AreaMap, path: &str, title: &str) -> &'a mut PageMap {
    if let Some(i) = area.pages.iter().position(|p| p.path == path || page_path(&p.path) == path) {
        let page = &mut area.pages[i];
        if page.path != path {
            page.path = path.to_string();
        }
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
/// Is this path segment an id: all digits, a GUID, or 16 or more hex
/// characters?
fn is_id_segment(seg: &str) -> bool {
    let hex = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit());
    if !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let parts: Vec<&str> = seg.split('-').collect();
    let guid = parts.len() == 5
        && parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12])
        && parts.iter().all(|p| hex(p));
    guid || (seg.len() >= 16 && hex(seg))
}

/// The path a map PAGE is filed under: `path_only`, with each segment that
/// is an id kept as `:id`, so two records of one kind are one page
/// (`/leave/12345/edit` is `/leave/:id/edit`). The save-request log keeps
/// `path_only`.
pub fn page_path(url_or_path: &str) -> String {
    path_only(url_or_path)
        .split('/')
        .map(|seg| if is_id_segment(seg) { ":id" } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

/// Files what a read of the whole page showed: `record_read` with `whole`
/// set.
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
    record_read(root, org, project, area, page_path, title, lines, account, discovery, true, now).map(|_| ())
}

/// Files what one page read showed, `lines` being only the lines it
/// returned, and answers how many distinct elements it recorded as seen.
///
/// A discovery's read of the WHOLE page (`whole`) explores that page: what
/// the map held for it and this read did not show goes, unless it was seen
/// since the discovery started. A read the line limit cut short (`whole`
/// false) never counts as exploring the page: it adds and refreshes what it
/// returned and drops nothing, because the lines past the cut were never
/// looked at, and it leaves the area's `explored_at`, `failed_since` and
/// `account` as they were.
#[allow(clippy::too_many_arguments)]
pub fn record_read(
    root: &Path,
    org: &str,
    project: &str,
    area: Option<&str>,
    page_path: &str,
    title: &str,
    lines: &[SnapLine],
    account: Option<&str>,
    discovery: Option<u64>,
    whole: bool,
    now: u64,
) -> Result<usize, String> {
    let path = self::page_path(page_path);
    let area = canonical_area(root, org, project, area.unwrap_or(""));
    let mut recorded = 0;
    update(root, org, project, |map| {
        let a = area_mut(map, &area);
        // Only what a discovery sees in a named area marks that area
        // explored: the bucket for no area is never explored. A read the
        // limit cut short saw only part of a page, so it never freshens the
        // area either: no stamp, no clearing a failure, no account.
        if discovery.is_some() && whole && !area.is_empty() {
            a.explored_at = Some(now);
            a.failed_since = false;
            a.account = account.map(str::to_string);
        }
        let page = page_mut(a, &path, title);
        let explores = discovery.filter(|_| whole);
        let old = match explores {
            Some(_) => std::mem::take(&mut page.elements),
            None => vec![],
        };
        let mut shown_locators: Vec<&Target> = Vec::new();
        for line in lines {
            if line.locator.links().last().and_then(LocatorStep::seen_key).is_some()
                && !shown_locators.contains(&&line.locator)
            {
                shown_locators.push(&line.locator);
            }
        }
        recorded = shown_locators.len();
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
        if let Some(started) = explores {
            for e in old {
                let matched_now = e.seen_at != 0 && e.seen_at >= started;
                if matched_now && !page.elements.iter().any(|n| n.locator == e.locator) {
                    page.elements.push(e);
                }
            }
        }
        // A discovery's read refreshes what it shows; any other read only
        // adds what is new, so a read that shows nothing new writes nothing.
        let mut shown: HashSet<SeenKey> = HashSet::new();
        for line in lines {
            for link in line.locator.links() {
                if let Some(key) = link.seen_key() {
                    shown.insert(key.clone());
                    note_sighting(a, key, link, &path, now, discovery.is_some());
                }
            }
        }
        if let Some(started) = explores {
            a.sightings.retain(|s| s.page != path || s.last_seen >= started || shown.contains(&s.key));
        }
        trim_sightings(a);
    })?;
    Ok(recorded)
}

/// Adds `link` to the area's sightings, or, when its key is there already
/// and `refresh` is set, stamps it seen at `now` on `page`.
fn note_sighting(a: &mut AreaMap, key: SeenKey, link: LocatorStep, page: &str, now: u64, refresh: bool) {
    match a.sightings.iter_mut().find(|s| s.key == key) {
        Some(s) if refresh && now >= s.last_seen => {
            s.last_seen = now;
            s.link = link;
            s.page = page.to_string();
        }
        Some(_) => {}
        None => a.sightings.push(Sighting { key, link, page: page.to_string(), last_seen: now }),
    }
}

/// Keeps the area within `MAX_SIGHTINGS`: past it, the least recently seen
/// go, and among those seen at the same time the earliest kept goes first.
fn trim_sightings(a: &mut AreaMap) {
    let over = a.sightings.len().saturating_sub(MAX_SIGHTINGS);
    if over == 0 {
        return;
    }
    let mut order: Vec<usize> = (0..a.sightings.len()).collect();
    order.sort_by_key(|&i| (a.sightings[i].last_seen, i));
    let gone: HashSet<usize> = order.into_iter().take(over).collect();
    let mut i = 0;
    a.sightings.retain(|_| {
        let keep = !gone.contains(&i);
        i += 1;
        keep
    });
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
    let path = self::page_path(page_path);
    let area = canonical_area(root, org, project, area.unwrap_or(""));
    update(root, org, project, |map| {
        let a = area_mut(map, &area);
        for link in target.links() {
            if let Some(key) = link.seen_key() {
                note_sighting(a, key, link, &path, now, true);
            }
        }
        trim_sightings(a);
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
    update(root, org, project, |map| {
        let a = area_mut(map, &area);
        a.writes.push(w);
        if a.writes.len() > MAX_WRITES {
            let drop = a.writes.len() - MAX_WRITES;
            a.writes.drain(..drop);
        }
    })
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

/// The elements seen in the named areas and in the unattributed bucket, in
/// map order.
fn elements_in<'a>(map: &'a DiscoveryMap, areas: &[&str]) -> impl Iterator<Item = &'a SeenElement> {
    let wanted: Vec<String> = areas.iter().map(|a| area_key(a)).collect();
    map.areas
        .iter()
        .filter(move |a| {
            let key = area_key(&a.area);
            key.is_empty() || wanted.contains(&key)
        })
        .flat_map(|a| a.pages.iter())
        .flat_map(|p| p.elements.iter())
}

/// The sightings kept in the named areas and in the unattributed bucket,
/// in map order.
fn sightings_in<'a>(map: &'a DiscoveryMap, areas: &[&str]) -> impl Iterator<Item = &'a Sighting> {
    let wanted: Vec<String> = areas.iter().map(|a| area_key(a)).collect();
    map.areas
        .iter()
        .filter(move |a| {
            let key = area_key(&a.area);
            key.is_empty() || wanted.contains(&key)
        })
        .flat_map(|a| a.sightings.iter())
}

/// The pages (`page_path`) each key was seen on, in the named areas and in
/// the unattributed bucket: an element's page, and a sighting's.
pub fn seen_pages(map: &DiscoveryMap, areas: &[&str]) -> std::collections::HashMap<SeenKey, HashSet<String>> {
    let wanted: Vec<String> = areas.iter().map(|a| area_key(a)).collect();
    let mut out: std::collections::HashMap<SeenKey, HashSet<String>> = std::collections::HashMap::new();
    for a in map.areas.iter().filter(|a| {
        let key = area_key(&a.area);
        key.is_empty() || wanted.contains(&key)
    }) {
        for p in &a.pages {
            let page = page_path(&p.path);
            for e in &p.elements {
                let mut keys: Vec<SeenKey> = e.locator.links().iter().filter_map(LocatorStep::seen_key).collect();
                keys.push(e.key.clone());
                for k in keys {
                    out.entry(k).or_default().insert(page.clone());
                }
            }
        }
        for s in &a.sightings {
            out.entry(s.key.clone()).or_default().insert(page_path(&s.page));
        }
    }
    out
}

/// Every key seen in the named areas and in the unattributed bucket.
pub fn seen_keys(map: &DiscoveryMap, areas: &[&str]) -> HashSet<SeenKey> {
    elements_in(map, areas)
        .flat_map(|e| {
            let mut keys: Vec<SeenKey> = e.locator.links().iter().filter_map(LocatorStep::seen_key).collect();
            keys.push(e.key.clone());
            keys
        })
        .chain(sightings_in(map, areas).map(|s| s.key.clone()))
        .collect()
}

/// Every locator seen in the named areas and in the unattributed bucket,
/// whole (a chain stays a chain, so what was seen inside what is kept), in
/// map order.
pub fn seen_locators(map: &DiscoveryMap, areas: &[&str]) -> Vec<Target> {
    elements_in(map, areas).map(|e| e.locator.clone()).collect()
}

/// Every link seen in the named areas and in the unattributed bucket, as
/// it was written when seen, in map order: what the seen check compares a
/// name with beyond its exact key (`seen_keys`), and where it takes the
/// closest seen locator from. An element whose key is none of its links
/// (an older map) adds a link made from the key.
pub fn seen_links(map: &DiscoveryMap, areas: &[&str]) -> Vec<LocatorStep> {
    let mut out = Vec::new();
    for e in elements_in(map, areas) {
        let links = e.locator.links();
        if !links.iter().any(|l| l.seen_key().as_ref() == Some(&e.key)) {
            out.push(match &e.key {
                SeenKey::Role { role, name } => LocatorStep {
                    role: Some(if e.role.is_empty() { role.clone() } else { e.role.clone() }),
                    name: Some(if e.name.is_empty() { name.clone() } else { e.name.clone() }),
                    ..LocatorStep::default()
                },
                SeenKey::Text(t) => LocatorStep { text: Some(t.clone()), ..LocatorStep::default() },
                SeenKey::Css(c) => LocatorStep { css: Some(c.clone()), ..LocatorStep::default() },
            });
        }
        out.extend(links);
    }
    out.extend(sightings_in(map, areas).map(|s| s.link.clone()));
    out
}

/// Every page path the map holds, as `page_path` files it: a page an
/// older map filed with its ids counts too.
pub fn seen_paths(map: &DiscoveryMap) -> HashSet<String> {
    map.areas.iter().flat_map(|a| a.pages.iter()).map(|p| page_path(&p.path)).collect()
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
