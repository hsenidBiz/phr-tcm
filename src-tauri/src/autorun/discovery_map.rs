//! The per-project discovery map: what Auto Run has seen on the live
//! application, area by area, so a script is written against elements that
//! exist rather than guessed ones.
//!
//! Saved beside the project's other files as `<slug>-map.json`. It keeps
//! page PATHS (no host, query or fragment) and locators only - never a page
//! URL, a typed value or any text that came from a record - except an
//! area's `refusals`, which keep the application's words as it showed them,
//! record values included (the test environment's data). An area whose
//! `explored_at` is old, or which has failed since, is stale and wants
//! exploring again. `""` is the bucket for sightings that belong to no area.
//!
//! An area's `refusals` are the application's validation rules as Auto Run
//! met them: a request the application refused, or a message the page
//! showed that a failure then followed. Each is kept once per page, wizard
//! step and message (`remember_refusal`), with the action that caused it,
//! and never a trace id, a status or an address. They are rules of the application,
//! not of the map, so exploring the area again keeps them; at most
//! `MAX_RULES_PER_AREA` are kept, the least recently seen going first.
//!
//! Each area also keeps its `sightings`: every distinct link it has seen,
//! once each by its key (`SeenKey`), with when it was last seen. A page's
//! `elements` are what the page holds now and a read replaces them, so a
//! wizard that keeps one address for every step forgets the earlier steps
//! as it moves on; its sightings do not. A sighting is never dropped for
//! the number of newer ones, nor because a later read did not show it: a
//! wizard's earlier step on the same address, a dropdown's item and a toast
//! are all gone from a later read and were real, so a later discovery's
//! whole-page read replaces the page's `elements` and keeps every sighting.
//! A sighting goes only when its area is forgotten, when it was not seen
//! for `STALE_AFTER_MS` (30 days; let go at the area's next recording,
//! and left out by a save's check before then, `without_stale`), or
//! past `MAX_SIGHTINGS`, a safety ceiling on the file's size, the least
//! recently seen first. An area's `wholes` are kept and let go the same
//! way.
//!
//! A sighting and an element also keep how many elements they `matched`
//! on the live page. `1` is filed only for exactly the locator that
//! matched one element: a single link with no `nth`, `exact` or `visible`
//! (`plain_link`). Such a link's key names every element it could match,
//! so a link that only matched one inside a scope, or once narrowed, says
//! nothing about how many its key matches: each link of a chain, and a
//! narrowed link, is filed as `NOT_ONE`. The whole locator, chain or
//! narrowed, is kept in the area's `wholes` with the number it matched, so
//! that exact locator can be vouched for as a whole.
//!
//! A page read counts, for each line, the listed lines in the same frame
//! its locator would match: the same role, and a name holding its name
//! (`locator::name_matches`). Two rows' "Edit" buttons are two; "Save"
//! beside "Save draft" is two. A text line counts every line whose words
//! hold its text, a text line or a control's name, as a text locator finds
//! every element whose visible text holds it: "Stage 1" beside "Stage 10"
//! is two, and "Save" beside a button "Save draft" is two. A read that knows every line of the whole
//! page (`snapshot::PageRead::page_lines`) counts each line it listed
//! against all of them, so a read the line limit cut short, or one that
//! listed only one element's subtree (`within`), still files the page's
//! own count: a line counted once inside a container may have a twin
//! outside it, and that twin is counted. Such a read never explores the
//! page. A read that does not know the whole page (a frame it could not
//! read) may not have listed every match, so there a count of one is
//! filed as `NOT_ONE`. A probe, a try or the record-on-page check of a
//! refused save matches a locator against the page and files the number
//! it found.
//!
//! A probe that matched nothing files a plain locator's link, or a chain or
//! a narrowed locator whole (never its links alone), as seen matching none
//! (`matched` 0): the seen check takes that only for a use that passes with
//! nothing there (`expect_hidden`, an `expect_count` of 0, a
//! `when_visible`'s own locator). A count of some replaces it. A match of
//! none beside a count of some is kept apart (`zero_seen`): it never
//! stamps the count seen again, so a button the app removed, probed and
//! found gone again and again, still ages out after 30 days, and then
//! stands as seen matching none if that probe is itself fresh. It adds no
//! element.
//!
//! A sighting and an element also keep `all`: how many elements the key
//! matched hidden ones included, which is what a legacy string target
//! acts on (`querySelector` takes the first match, hidden or not). It is
//! left out when it equals `matched`, so a map written before it was kept
//! reads it as `matched`. A count that is not known is `NOT_ONE`.
//!
//! Once a locator has been seen matching exactly one element it stays
//! `1`; a later sighting matching several never undoes it. A map written
//! before this was kept loads as `1`. The seen check lets a check that can
//! pass with several matches use a locator seen matching several, and
//! holds every other use to one (`seen_check`).

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
    /// Each chain or narrowed locator a probe or a try matched, whole, with
    /// how many elements it matched (see the module's notes). Kept and let
    /// go as `sightings` are. A map written before these were kept has
    /// none.
    #[serde(default)]
    pub wholes: Vec<WholeSighting>,
    /// The application's rules met in the area (see the module's notes). A
    /// map written before these were kept has none.
    #[serde(default)]
    pub refusals: Vec<RefusalRule>,
    /// The links the area's saved scripts use, as written: read in memory
    /// by the seen check (`seen_check::add_saved_scripts`), never saved.
    #[serde(skip)]
    pub saved_links: Vec<LocatorStep>,
    /// The links the area's saved scripts use only in a check that can pass
    /// with several matches (`seen_check::takes_many`), as written: read in
    /// memory by the seen check for such checks alone, never saved.
    #[serde(skip)]
    pub saved_check_links: Vec<LocatorStep>,
    /// The links the area's saved scripts use only where a run passes with
    /// nothing there (`seen_check::takes_none`: `expect_hidden`, an
    /// `expect_count` of 0, a `when_visible`'s own locator), as written:
    /// read in memory by the seen check for such uses alone, never saved.
    #[serde(skip)]
    pub saved_none_links: Vec<LocatorStep>,
    /// The chains and narrowed locators the area's saved scripts use
    /// outside such a check, whole, as written: read in memory by the seen
    /// check, which takes them only written the same way, never saved.
    #[serde(skip)]
    pub saved_wholes: Vec<Vec<LocatorStep>>,
    /// The chains (a list, then the child looked for inside it) the area's
    /// saved `expect_each` scripts use, whole, as written: they vouch only
    /// for the same chain, never for their links alone. Read in memory,
    /// never saved.
    #[serde(skip)]
    pub saved_each_wholes: Vec<Vec<LocatorStep>>,
    /// Of `saved_links`, those a saved script wrote as a legacy string
    /// target: the only ones the seen check takes for a legacy use, as a
    /// structured css locator says nothing of its hidden matches. Read in
    /// memory, never saved.
    #[serde(skip)]
    pub saved_legacy_links: Vec<LocatorStep>,
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
    /// How many elements it matched (see the module's notes): `1` once it
    /// was seen matching exactly one, else the number the last sighting
    /// matched. A map written before this was kept loads as `1`.
    #[serde(default = "one_match")]
    pub matched: u32,
    /// How many it matched hidden ones included, when that is not
    /// `matched` (see the module's notes): read through `all_matched`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<u32>,
    /// When a probe last matched none of it, while it holds a count of
    /// some: kept apart, so it never makes the count look seen again (see
    /// the module's notes). Left out when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = Option<f64>)]
    pub zero_seen: Option<u64>,
}

impl Sighting {
    /// How many it matched hidden ones included: `all`, else `matched`.
    pub fn all_matched(&self) -> u32 {
        self.all.unwrap_or(self.matched)
    }
}

/// `all` as it is kept: left out when it is `matched`.
fn kept_all(matched: u32, all: u32) -> Option<u32> {
    (all != matched).then_some(all)
}

/// A chain or a narrowed locator seen whole: what that exact locator
/// matched.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct WholeSighting {
    pub locator: Target,
    /// The page (`page_path`) it was last seen on.
    pub page: String,
    /// When it was last seen, in milliseconds since the epoch.
    #[specta(type = f64)]
    pub last_seen: u64,
    /// How many elements it matched, kept as a sighting keeps it.
    pub matched: u32,
    /// When a probe last matched none of it, kept as a sighting keeps it
    /// (`Sighting::zero_seen`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = Option<f64>)]
    pub zero_seen: Option<u64>,
}

/// Is a stamp `at` within `STALE_AFTER_MS` of `now`?
fn fresh(at: u64, now: u64) -> bool {
    now.saturating_sub(at) <= STALE_AFTER_MS
}

/// Ages one seen record at `now`: its count of some (`matched`, stamped
/// `last_seen`) and a match of none kept beside it (`zero_seen`). Answers
/// whether anything of it is still fresh: a count of some gone stale with
/// a fresh match of none left becomes that match of none.
fn age(last_seen: &mut u64, matched: &mut u32, zero_seen: &mut Option<u64>, now: u64) -> bool {
    if zero_seen.is_some_and(|z| !fresh(z, now)) {
        *zero_seen = None;
    }
    if fresh(*last_seen, now) {
        return true;
    }
    match zero_seen.take() {
        Some(z) => {
            *last_seen = z;
            *matched = 0;
            true
        }
        None => false,
    }
}

/// `map` as the seen check reads it at `now`: every sighting and whole
/// aged (`age`), what is not fresh left out, and with them each page
/// element whose key's sighting of some aged out in this pass. An element
/// whose key has no sighting at all comes from a map written before
/// sightings were kept: it stays while its area was explored within
/// `STALE_AFTER_MS`, and goes with an area never explored or explored
/// longer ago. The next write of the area lets go of the same sightings,
/// wholes and elements (`trim_sightings`, `trim_wholes`); this is for a
/// save's check of an area that has not been written since.
pub fn without_stale(map: &mut DiscoveryMap, now: u64) {
    for a in &mut map.areas {
        let sighted: HashSet<SeenKey> = a.sightings.iter().map(|s| s.key.clone()).collect();
        let gone = age_sightings(a, now);
        a.wholes.retain_mut(|w| age(&mut w.last_seen, &mut w.matched, &mut w.zero_seen, now));
        let explored = a.explored_at.is_some_and(|at| fresh(at, now));
        for p in &mut a.pages {
            p.elements.retain(|e| !gone.contains(&e.key) && (sighted.contains(&e.key) || explored));
        }
    }
}

/// `age` over the area's sightings, the stale ones dropped. Answers the
/// keys that held a count of some before and no longer do, which no page
/// element may stand for any more.
fn age_sightings(a: &mut AreaMap, now: u64) -> HashSet<SeenKey> {
    let mut gone = HashSet::new();
    a.sightings.retain_mut(|s| {
        let some = s.matched != 0;
        let kept = age(&mut s.last_seen, &mut s.matched, &mut s.zero_seen, now);
        if s.matched == 0 {
            s.all = None;
        }
        if some && (!kept || s.matched == 0) {
            gone.insert(s.key.clone());
        }
        kept
    });
    gone
}

/// Takes off every page of the area the elements filed as one of `keys`.
fn forget_elements(a: &mut AreaMap, keys: &HashSet<SeenKey>) {
    if keys.is_empty() {
        return;
    }
    for p in &mut a.pages {
        p.elements.retain(|e| !keys.contains(&e.key));
    }
}

/// What a sighting or an element written before `matched` was kept loads
/// as: one element, the only kind that was recorded then.
fn one_match() -> u32 {
    1
}

/// The `matched` a sighting holds after one more sighting matching `new`:
/// one when either is one, else the newer number, unless that one is not
/// known (`NOT_ONE`) and the older is. A sighting matching none (`0`, a
/// probe that found nothing) never undoes a count of some, and a count of
/// some replaces it.
fn merged_matches(old: u32, new: u32) -> u32 {
    if old == 0 {
        new
    } else if new == 0 {
        old
    } else if old == 1 || new == 1 {
        1
    } else if new == NOT_ONE {
        old
    } else {
        new
    }
}

/// Was a sighting or an element seen matching only several elements, never
/// exactly one?
pub fn only_many(matched: u32) -> bool {
    matched > 1
}

/// What a link is filed as when its count is not known: a scope link of a
/// chain, a narrowed link, a line of a read cut short, or the hidden
/// matches of a structured locator (see the module's notes). It is never
/// one, and is told apart from a real count of several (`counted`).
pub const NOT_ONE: u32 = u32::MAX;

/// Is `matched` a number actually counted, rather than `NOT_ONE`?
pub fn counted(matched: u32) -> bool {
    matched != NOT_ONE
}

/// Is `link` a whole locator on its own that names exactly what its key
/// names: no `nth`, no `exact` and no `visible` narrowing it?
pub fn plain_link(link: &LocatorStep) -> bool {
    link.nth.is_none() && !link.exact && link.visible.is_none()
}

/// Is `target` one plain link (`plain_link`), so that the number it
/// matched is the number its key matches? A legacy string is one css link.
pub fn plain_target(target: &Target) -> bool {
    match target {
        Target::Legacy(_) => true,
        Target::One(l) => plain_link(l),
        Target::Chain(v) => v.len() == 1 && plain_link(&v[0]),
    }
}

/// Takes off page `path` the elements filed as `target` that no page read
/// could print (`a_read_can_print_target`): a page's elements are what it
/// holds now, and a probe or a try that finds nothing there is the only
/// evidence such an element has gone. A role locator a read prints is left
/// to the read. Sightings are never taken off (see the module's notes).
fn forget_unprintable(a: &mut AreaMap, path: &str, target: &Target) {
    if let Some(page) = a.pages.iter_mut().find(|p| p.path == path) {
        page.elements.retain(|e| e.locator != *target || a_read_can_print_target(&e.locator));
    }
}

/// Can a page read print a link with this key? A read prints a role and a
/// name for each element (never a role with no locator,
/// `snapshot::NO_LOCATOR_ROLES`), and a css step only for the iframe a
/// line sits in (`snapshot::frame_step`). A text locator, and any other
/// css, only a probe or a try ever matches.
pub fn a_read_can_print(key: &SeenKey) -> bool {
    match key {
        SeenKey::Role { role, .. } => {
            !crate::browser::snapshot::NO_LOCATOR_ROLES.iter().any(|r| crate::browser::locator::fold_name(r) == *role)
        }
        SeenKey::Css(css) => css.starts_with("iframe"),
        SeenKey::Text(_) => false,
    }
}

/// Can a page read print this whole locator? Every link plain
/// (`plain_link`) and printable (`a_read_can_print`), ending in a role.
pub fn a_read_can_print_target(target: &Target) -> bool {
    let links = target.links();
    links.last().is_some_and(|l| l.role.is_some())
        && links.iter().all(|l| plain_link(l) && l.seen_key().is_some_and(|k| a_read_can_print(&k)))
}

/// How many listed lines in the same frame as `line` its locator would
/// match: the same role, and a name holding its name, as a role locator
/// matches (`locator::name_matches`, not exact). A line whose locator is
/// not a role and a name counts as one, except a text line
/// (`snapshot::TEXT_ROLE`): a text locator finds every element whose
/// visible text holds it, a control's own words included, so it counts
/// every listed line in its frame whose words hold its text, a text line
/// or a control's name alike (`"exact"`: whose words are its text).
fn read_count(line: &SnapLine, lines: &[SnapLine]) -> u32 {
    let links = line.locator.links();
    let Some((own, frames)) = links.split_last() else { return 1 };
    let in_frame = |other: &SnapLine| {
        let theirs = other.locator.links();
        theirs.split_last().is_some_and(|(_, f)| f == frames)
    };
    let n = if let Some(text) = own.text.as_deref() {
        lines.iter().filter(|o| in_frame(o) && crate::browser::locator::name_matches(&o.name, text, own.exact)).count()
    } else {
        let Some(role) = own.role.as_deref() else { return 1 };
        let want = own.name.as_deref().unwrap_or("");
        lines
            .iter()
            .filter(|other| {
                let theirs = other.locator.links();
                let Some((o, f)) = theirs.split_last() else { return false };
                f == frames
                    && o.role.as_deref() == Some(role)
                    && (want.is_empty() || crate::browser::locator::name_matches(o.name.as_deref().unwrap_or(""), want, own.exact))
            })
            .count()
    };
    u32::try_from(n.max(1)).unwrap_or(u32::MAX)
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
    /// How many elements it matched, kept as a sighting keeps it
    /// (`Sighting::matched`).
    #[serde(default = "one_match")]
    pub matched: u32,
    /// Hidden ones included, kept as a sighting keeps it (`Sighting::all`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<u32>,
}

impl SeenElement {
    /// How many it matched hidden ones included: `all`, else `matched`.
    pub fn all_matched(&self) -> u32 {
        self.all.unwrap_or(self.matched)
    }

    /// Takes one more sighting matching `matched`, `all` hidden included.
    fn merge(&mut self, matched: u32, all: u32) {
        let old_all = self.all_matched();
        self.matched = merged_matches(self.matched, matched);
        self.all = kept_all(self.matched, merged_matches(old_all, all));
    }
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
    super::data_changed::notify(root, crate::events::AutorunDataKind::Map);
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
    crate::ai_tools::atomic_write(&path, &text)?;
    super::data_changed::notify(root, crate::events::AutorunDataKind::Map);
    Ok(())
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
/// A discovery's read of the WHOLE page (`whole`) explores that page: the
/// page's elements it did not show go, unless seen since the discovery
/// started; the area's sightings all stay (see the module's notes). A read
/// the line limit cut short (`whole` false) never counts as exploring the
/// page: it adds and refreshes what it returned and drops no element, because the lines past the cut were never
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
    record_read_counted(root, org, project, area, page_path, title, lines, None, account, discovery, whole, now)
}

/// [`record_read`], each listed line counted against `whole_page`: every line of
/// the whole page that carries a locator, printed or not, when the read
/// knows it (`snapshot::PageRead::page_lines`, every frame read). Then a
/// read the limit cut short, or one that listed only an element's subtree
/// (`within`), files each line it listed with the number of elements its
/// locator matches on the whole page: a line counted once inside a
/// container may have a twin outside it, and counting against the page
/// sees that twin. Such a read still never explores the page (`whole` is
/// false: no `explored_at`, nothing dropped), as only a read that listed
/// the whole page can say what is no longer there. With it `None` the
/// count is taken among the listed lines, and a count of one from a read
/// that is not whole is filed as `NOT_ONE`, as it always was.
#[allow(clippy::too_many_arguments)]
pub fn record_read_counted(
    root: &Path,
    org: &str,
    project: &str,
    area: Option<&str>,
    page_path: &str,
    title: &str,
    lines: &[SnapLine],
    whole_page: Option<&[SnapLine]>,
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
        // How many elements each line's locator matches: on the whole page
        // when the read knows it, else among the listed lines, where a
        // count of one from a read cut short is not known to be one.
        let counts: Vec<u32> = lines
            .iter()
            .map(|l| match whole_page {
                Some(all) => read_count(l, all),
                None => match read_count(l, lines) {
                    1 if !whole => NOT_ONE,
                    n => n,
                },
            })
            .collect();
        for (line, &count) in lines.iter().zip(&counts) {
            // The last link names the element; earlier links are frames.
            let Some(key) = line.locator.links().last().and_then(LocatorStep::seen_key) else { continue };
            // The full locator is the identity: the same name in two frames
            // is two elements.
            if let Some(e) = page.elements.iter_mut().find(|e| e.locator == line.locator) {
                e.merge(count, count);
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
                matched: count,
                all: None,
            });
        }
        if let Some(started) = explores {
            for e in old {
                let matched_now = e.seen_at != 0 && e.seen_at >= started;
                // What no read can print is not gone for not being printed,
                // until it has not been matched for `STALE_AFTER_MS`.
                let fresh = now.saturating_sub(e.seen_at) <= STALE_AFTER_MS;
                let kept = matched_now || (e.seen_at != 0 && fresh && !a_read_can_print_target(&e.locator));
                if kept && !page.elements.iter().any(|n| n.locator == e.locator) {
                    page.elements.push(e);
                }
            }
        }
        // A discovery's read refreshes what it shows; any other read only
        // adds what is new, so a read that shows nothing new writes nothing.
        // Each link on its own: a plain one-link line with its count, every
        // link of a chain (a frame and what is inside it) as `NOT_ONE`.
        for (line, &count) in lines.iter().zip(&counts) {
            let link_count = if plain_target(&line.locator) { count } else { NOT_ONE };
            for link in line.locator.links() {
                if let Some(key) = link.seen_key() {
                    note_sighting(a, key, link, &path, now, discovery.is_some(), link_count, link_count);
                }
            }
        }
        // No sighting is dropped for not being shown: a wizard's earlier
        // step, a dropdown's item and a toast are gone from a later read
        // and were real. Only time and the ceiling let one go.
        trim_sightings(a, now);
        trim_wholes(a, now);
    })?;
    Ok(recorded)
}

/// Adds `link` to the area's sightings, matching `matched` elements, or,
/// when its key is there already and `refresh` is set, stamps it seen at
/// `now` on `page`. A sighting matching one makes it one either way
/// (`merged_matches`).
#[allow(clippy::too_many_arguments)]
fn note_sighting(
    a: &mut AreaMap,
    key: SeenKey,
    link: LocatorStep,
    page: &str,
    now: u64,
    refresh: bool,
    matched: u32,
    all: u32,
) {
    match a.sightings.iter_mut().find(|s| s.key == key) {
        // A match of none beside a count of some is kept apart: it never
        // stamps the count seen again, so what is gone still ages out.
        Some(s) if matched == 0 && s.matched != 0 => {
            s.zero_seen = Some(s.zero_seen.unwrap_or(0).max(now));
        }
        // A read that is no discovery stamps only one gone stale: it saw
        // it, and a fresh one it leaves as it is, so it writes nothing.
        Some(s) => {
            if (refresh || !fresh(s.last_seen, now)) && now >= s.last_seen {
                s.last_seen = now;
                s.link = link;
                s.page = page.to_string();
            }
            let old_all = s.all_matched();
            s.matched = merged_matches(s.matched, matched);
            s.all = kept_all(s.matched, merged_matches(old_all, all));
        }
        None => a.sightings.push(Sighting {
            key,
            link,
            page: page.to_string(),
            last_seen: now,
            matched,
            all: kept_all(matched, all),
            zero_seen: None,
        }),
    }
}

/// Lets go of the area's sightings not seen for more than `STALE_AFTER_MS`
/// before `now`, then keeps it within `MAX_SIGHTINGS`: past it, the least
/// recently seen go, and among those seen at the same time the earliest
/// kept goes first. The page elements of each key let go go too.
fn trim_sightings(a: &mut AreaMap, now: u64) {
    let mut let_go = age_sightings(a, now);
    let over = a.sightings.len().saturating_sub(MAX_SIGHTINGS);
    if over > 0 {
        let mut order: Vec<usize> = (0..a.sightings.len()).collect();
        order.sort_by_key(|&i| (a.sightings[i].last_seen, i));
        let gone: HashSet<usize> = order.into_iter().take(over).collect();
        let mut i = 0;
        a.sightings.retain(|s| {
            let keep = !gone.contains(&i);
            if !keep {
                let_go.insert(s.key.clone());
            }
            i += 1;
            keep
        });
    }
    // An element stands for its key's sighting: one let go takes its
    // elements with it, so an element with no sighting is only ever one
    // a map kept before sightings were.
    forget_elements(a, &let_go);
}

/// Lets go of the area's `wholes` as `trim_sightings` lets go of its
/// sightings: not seen for `STALE_AFTER_MS`, or past `MAX_SIGHTINGS`.
fn trim_wholes(a: &mut AreaMap, now: u64) {
    a.wholes.retain_mut(|w| age(&mut w.last_seen, &mut w.matched, &mut w.zero_seen, now));
    let over = a.wholes.len().saturating_sub(MAX_SIGHTINGS);
    if over == 0 {
        return;
    }
    let mut order: Vec<usize> = (0..a.wholes.len()).collect();
    order.sort_by_key(|&i| (a.wholes[i].last_seen, i));
    let gone: HashSet<usize> = order.into_iter().take(over).collect();
    let mut i = 0;
    a.wholes.retain(|_| {
        let keep = !gone.contains(&i);
        i += 1;
        keep
    });
}

/// Adds each link of a target a probe or try matched, unless the page
/// already has it, and stamps it matched at `now` (`seen_at`) either way:
/// [`record_matched_count`] for a target that matched exactly one element.
pub fn record_matched(
    root: &Path,
    org: &str,
    project: &str,
    area: Option<&str>,
    page_path: &str,
    target: &Target,
    now: u64,
) -> Result<(), String> {
    record_matched_count(root, org, project, area, page_path, target, 1, now)
}

/// [`record_matched_counts`] knowing only the count `target` itself gives:
/// a legacy string counts hidden matches already, so its `all` is that
/// count; a structured one-link css locator counts only visible ones, and
/// its key is a legacy string's too, so its hidden matches are left not
/// known (`NOT_ONE`); any other locator's `all` is never read for a legacy
/// use, and is that count.
#[allow(clippy::too_many_arguments)]
pub fn record_matched_count(
    root: &Path,
    org: &str,
    project: &str,
    area: Option<&str>,
    page_path: &str,
    target: &Target,
    matched: usize,
    now: u64,
) -> Result<(), String> {
    let structured_css = !target.is_legacy() && plain_target(target) && target.links().first().is_some_and(|l| l.css.is_some());
    let all = if structured_css { NOT_ONE as usize } else { matched };
    record_matched_counts(root, org, project, area, page_path, target, matched, all, now)
}

/// [`record_matched`] for a target that matched `matched` elements, `all`
/// of them with hidden ones included (`NOT_ONE` when that is not known).
/// A plain one-link target (`plain_target`) files those numbers on its
/// link; any other files each link as `NOT_ONE` and keeps the whole target
/// in the area's `wholes` with `matched`. A link already seen matching one
/// stays one (see the module's notes).
///
/// `0` files each link, and a chain or a narrowed target whole, as seen
/// matching none (`matched` 0): what a check that passes with nothing
/// there may use (`seen_check`), never one that needs it present. It adds
/// no element, and takes off this page the elements no page read could
/// print of it (`forget_unprintable`). A sighting already seen matching
/// some keeps its count.
#[allow(clippy::too_many_arguments)]
pub fn record_matched_counts(
    root: &Path,
    org: &str,
    project: &str,
    area: Option<&str>,
    page_path: &str,
    target: &Target,
    matched: usize,
    all: usize,
    now: u64,
) -> Result<(), String> {
    let none = matched == 0;
    let matched = u32::try_from(matched).unwrap_or(NOT_ONE);
    let all = if none { 0 } else { u32::try_from(all.max(1)).unwrap_or(NOT_ONE) };
    let plain = plain_target(target);
    let link_count = if plain { matched } else { NOT_ONE };
    let link_all = if plain { all } else { NOT_ONE };
    let path = self::page_path(page_path);
    let area = canonical_area(root, org, project, area.unwrap_or(""));
    update(root, org, project, |map| {
        let a = area_mut(map, &area);
        // A chain or a narrowed target that matched none says nothing of
        // its links: only the whole is filed.
        if plain || !none {
            for link in target.links() {
                if let Some(key) = link.seen_key() {
                    note_sighting(a, key, link, &path, now, true, link_count, link_all);
                }
            }
        }
        trim_sightings(a, now);
        if !plain {
            // The whole locator: what that exact locator matched.
            match a.wholes.iter_mut().find(|w| w.locator == *target) {
                Some(w) if none && w.matched != 0 => w.zero_seen = Some(w.zero_seen.unwrap_or(0).max(now)),
                Some(w) => {
                    if now >= w.last_seen {
                        w.last_seen = now;
                        w.page = path.clone();
                    }
                    w.matched = merged_matches(w.matched, matched);
                }
                None => a.wholes.push(WholeSighting {
                    locator: target.clone(),
                    page: path.clone(),
                    last_seen: now,
                    matched,
                    zero_seen: None,
                }),
            }
        }
        trim_wholes(a, now);
        if none {
            forget_unprintable(a, &path, target);
            return;
        }
        let page = page_mut(a, &path, "");
        for link in target.links() {
            let Some(key) = link.seen_key() else { continue };
            let mut held = false;
            for e in page.elements.iter_mut().filter(|e| e.key == key) {
                e.seen_at = e.seen_at.max(now);
                // Only an element that is a plain link of this key takes the
                // link's count: a chain keyed by it says nothing of the key.
                if plain_target(&e.locator) {
                    e.merge(link_count, link_all);
                }
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
                matched: link_count,
                all: kept_all(link_count, link_all),
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

// ------------------------------------------ the application's refusals

/// The most rules one area keeps: past it, the least recently seen goes.
pub const MAX_RULES_PER_AREA: usize = 30;

/// One of the application's rules, as Auto Run met it in an area (see the
/// module's notes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RefusalRule {
    /// The page it was met on (`page_path`).
    pub page: String,
    /// The page's wizard step then, when it shows one: a wizard keeps one
    /// address for every step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// The action that caused it, in a few words (`ai_bridge::describe_action`).
    pub after: String,
    /// The application's words, as it showed them.
    pub message: String,
    pub source: RuleSource,
    /// The case whose replay or run met it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_id: Option<i32>,
    /// When it was first met, in milliseconds since the epoch.
    #[specta(type = f64)]
    pub first_seen: u64,
    /// When it was last met, in milliseconds since the epoch.
    #[specta(type = f64)]
    pub last_seen: u64,
    /// How many times it was met.
    pub count: u32,
}

/// Who met a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    /// A discovery, or an assistant's try.
    Discovery,
    /// A replay or a run, in a step that failed.
    Run,
    /// The assistant, which judged a message the page showed to be one.
    Assistant,
}

/// A message as rules are told apart by: white space collapsed, case
/// folded.
fn rule_key(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// A message as a rule keeps it: white space collapsed, and at most as
/// long as a message the page is read for.
fn kept_message(message: &str) -> String {
    let collapsed = message.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(crate::browser::page_messages::MAX_MESSAGE_CHARS).collect()
}

/// Keep `rule` in `area`'s entry, made when there is none (callers make
/// the name canonical, `canonical_area`). One met before on the same page
/// and wizard step - a wizard keeps one address for every step - with the
/// same message (`rule_key`) counts once more and takes the newer action.
/// Past `MAX_RULES_PER_AREA` the least recently seen
/// goes. A rule with no area, or no words, is never kept: the guide never
/// shows the bucket for no area.
pub fn remember_refusal(map: &mut DiscoveryMap, area: &str, rule: RefusalRule) {
    let message = kept_message(&rule.message);
    if area_key(area).is_empty() || message.is_empty() {
        return;
    }
    let key = rule_key(&message);
    let a = area_mut(map, area);
    match a.refusals.iter_mut().find(|r| r.page == rule.page && r.step == rule.step && rule_key(&r.message) == key) {
        Some(r) => {
            r.first_seen = r.first_seen.min(rule.first_seen);
            r.last_seen = r.last_seen.max(rule.last_seen);
            r.count = r.count.saturating_add(1);
            r.after = rule.after;
        }
        None => a.refusals.push(RefusalRule { message, count: rule.count.max(1), ..rule }),
    }
    let over = a.refusals.len().saturating_sub(MAX_RULES_PER_AREA);
    for _ in 0..over {
        // The least recently seen, the earliest kept among equals.
        let oldest = (0..a.refusals.len()).min_by_key(|&i| (a.refusals[i].last_seen, i)).expect("over the cap");
        a.refusals.remove(oldest);
    }
}

/// Take out of `area` the rules with this message (`rule_key`): on `page`
/// only, or on every page when it is `None`. Says whether any went.
pub fn forget_refusal(map: &mut DiscoveryMap, area: &str, page: Option<&str>, message: &str) -> bool {
    let (area, key) = (area_key(area), rule_key(message));
    let Some(a) = map.areas.iter_mut().find(|a| area_key(&a.area) == area) else {
        return false;
    };
    let before = a.refusals.len();
    a.refusals.retain(|r| rule_key(&r.message) != key || page.is_some_and(|p| r.page != p));
    a.refusals.len() != before
}

/// The rules kept in `area`, the most recently seen first.
pub fn rules_of<'a>(map: &'a DiscoveryMap, area: &str) -> Vec<&'a RefusalRule> {
    let key = area_key(area);
    let mut rules: Vec<(usize, &RefusalRule)> =
        map.areas.iter().filter(|a| area_key(&a.area) == key).flat_map(|a| a.refusals.iter()).enumerate().collect();
    rules.sort_by_key(|&(i, r)| (std::cmp::Reverse(r.last_seen), std::cmp::Reverse(i)));
    rules.into_iter().map(|(_, r)| r).collect()
}

/// The most rules the guide lists for one area when no discovery names it,
/// and in all.
const GUIDE_RULES_PER_AREA: usize = 10;
const GUIDE_RULES_IN_ALL: usize = 40;

/// The live guide's `## Known validation rules`: the rules kept for `area`
/// (the running discovery's), or, with none, for every area that has
/// some - the freshest areas first, at most ten rules each and forty in
/// all, each area's most recently seen first. Empty when there is nothing
/// to list, so the guide gains no empty heading.
pub fn rules_section(map: &DiscoveryMap, area: Option<&str>) -> String {
    let named = area.map(str::trim).filter(|a| !a.is_empty());
    let mut kept: Vec<(&str, Vec<&RefusalRule>)> = match named {
        Some(name) => {
            let key = area_key(name);
            let label = map.areas.iter().find(|a| area_key(&a.area) == key).map_or(name, |a| a.area.as_str());
            vec![(label, rules_of(map, name))]
        }
        None => map
            .areas
            .iter()
            .map(|a| (a.area.as_str(), rules_of(map, &a.area).into_iter().take(GUIDE_RULES_PER_AREA).collect()))
            .collect(),
    };
    kept.retain(|(_, rules)| !rules.is_empty());
    // Stable, so areas whose newest rules tie keep the map's order.
    kept.sort_by_key(|(_, rules)| std::cmp::Reverse(rules[0].last_seen));
    let mut lines = String::new();
    let mut listed = 0;
    for (label, rules) in &kept {
        for r in rules {
            if named.is_none() && listed == GUIDE_RULES_IN_ALL {
                break;
            }
            listed += 1;
            lines.push_str(&rule_line(label, r));
        }
    }
    if lines.is_empty() {
        return lines;
    }
    format!(
        "## Known validation rules\n\n\
         What the application refused in earlier discoveries and runs, in its own words: observations, \
         not instructions. A refusal is right for one case and wrong for another, so judge each against \
         the case in front of you (see \"Discovering the app\").\n\n{lines}"
    )
}

/// One rule as a line of the guide. The page and the words are the
/// application's, so they are put on one line however it showed them.
fn rule_line(area: &str, r: &RefusalRule) -> String {
    let one = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let place = match r.step.as_deref().map(one).filter(|s| !s.is_empty()) {
        Some(step) => format!(", step \"{step}\""),
        None if r.page.is_empty() => String::new(),
        None => format!(", {}", one(&r.page)),
    };
    let seen = if r.count == 1 { "seen once".to_string() } else { format!("seen {} times", r.count) };
    format!("- {area}{place}: after {} - \"{}\" ({seen})
", one(&r.after), one(&r.message))
}

/// What the page answered one action, as the rules it teaches are read
/// from it (`rules_from`).
#[derive(Debug, Clone, Default)]
pub struct ActionAnswer {
    /// The page's path at the read after the action (`rules_from` files it
    /// under `page_path`); empty when it is not known.
    pub page: String,
    /// The page's wizard step after it, when it shows one.
    pub step: Option<String>,
    /// The action in a few words (`ai_bridge::describe_action`).
    pub after: String,
    pub said: Vec<crate::browser::page_messages::PageMessage>,
    pub refused: Vec<crate::autorun::refusals::RefusedRequest>,
    /// The action changed the page, so the page was read after it.
    pub changed: bool,
    /// The action failed, for a reason the page gave rather than TCM: what
    /// the page showed after the action before it was then a refusal.
    pub failed: bool,
}

/// The rules a discovery call, a try or a failed step teaches, from what
/// the page answered each of its actions in order: every request the
/// application refused, by the message its answer gave (never its trace
/// id, status or address - a refusal whose answer said nothing, so its
/// status stands in, is no rule), and the messages the page showed after
/// the nearest action before a failure that changed the page. A message
/// with no failure after it is never one on its own: it may as well have
/// been "Saved". Nor is a failure of an action that itself changed the page
/// (a click whose target was not there) evidence for the message before
/// it: that failure is the action's own business.
pub fn rules_from(answers: &[ActionAnswer], source: RuleSource, case_id: Option<i32>, now: u64) -> Vec<RefusalRule> {
    let mut followed = vec![false; answers.len()];
    for f in (0..answers.len()).filter(|&f| answers[f].failed && !answers[f].changed) {
        if let Some(j) = answers[..f].iter().rposition(|a| a.changed) {
            followed[j] = true;
        }
    }
    let mut out = Vec::new();
    for (i, a) in answers.iter().enumerate() {
        let said = a.said.iter().filter(|_| followed[i]).map(|m| m.text.as_str());
        let refused = a.refused.iter().filter(|r| r.from_body).map(|r| r.message.as_str());
        let page = if a.page.is_empty() { String::new() } else { page_path(&a.page) };
        // The same words twice from one action are one rule, met once.
        let mut met: Vec<String> = Vec::new();
        for message in refused.chain(said) {
            let key = rule_key(message);
            if key.is_empty() || met.contains(&key) {
                continue;
            }
            met.push(key);
            out.push(RefusalRule {
                page: page.clone(),
                step: a.step.clone(),
                after: a.after.clone(),
                message: message.to_string(),
                source,
                case_id,
                first_seen: now,
                last_seen: now,
                count: 1,
            });
        }
    }
    out
}

/// Keep `rules` in `area` of the map, in one load, change and write.
/// Nothing is written for no area or no rules.
pub fn remember_refusals(root: &Path, org: &str, project: &str, area: &str, rules: Vec<RefusalRule>) -> Result<(), String> {
    let area = canonical_area(root, org, project, area);
    if area.is_empty() || rules.is_empty() {
        return Ok(());
    }
    update(root, org, project, |map| {
        for rule in rules {
            remember_refusal(map, &area, rule);
        }
    })
}

/// `forget_refusal` in the saved map, in one load, change and write: says
/// whether any rule went. Nothing is written when none did.
pub fn forget_refusal_saved(
    root: &Path,
    org: &str,
    project: &str,
    area: &str,
    page: Option<&str>,
    message: &str,
) -> Result<bool, String> {
    let area = canonical_area(root, org, project, area);
    if area.is_empty() {
        return Ok(false);
    }
    let mut gone = false;
    update(root, org, project, |map| gone = forget_refusal(map, &area, page, message))?;
    Ok(gone)
}

/// `remember_refusals` for an action or a run that goes on whatever comes
/// of it: a map that could not be written is said in the log - how many
/// rules, and the area, never their words - and nothing fails. No project,
/// no area or no rules keeps nothing.
pub fn remember_refusals_logged(root: &Path, org: &str, project: &str, area: Option<&str>, rules: Vec<RefusalRule>) {
    let area = area.unwrap_or("").trim();
    if project.trim().is_empty() || area.is_empty() || rules.is_empty() {
        return;
    }
    let n = rules.len();
    if remember_refusals(root, org, project, area, rules).is_err() {
        let what = if n == 1 { "1 rule".to_string() } else { format!("{n} rules") };
        crate::applog::warn(format!("Discovery map: {what} the application showed in {area} could not be kept"));
    }
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
