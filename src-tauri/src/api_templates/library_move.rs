//! The one-time fold of every environment's own API templates, flows and
//! fixtures into the shared library (`library`).
//!
//! Before the library, each environment held its own copies under
//! `<env>/templates|fixtures|flows/<slug>/<id>.json`. The fold gives every
//! id one definition in `<shared>/api-library`: the newest copy wins, the
//! others are kept aside for 30 days in `<shared>/removed-api/<ms>/`, and
//! each environment keeps its confirmation of an item only where its own
//! copy IS the winner (`here`).
//!
//! It runs at start-up, holding the switch lock for writing, and is safe to
//! cut short at any point: everything is first built in a staging folder
//! beside the library; only once that is whole is a `.complete` mark
//! written, and a start that finds a mark only finishes the job. A staging
//! folder without a mark is discarded and rebuilt. Nothing is deleted from
//! an environment until the new library is in place.
//!
//! Everything here writes with plain `std::fs`: the lock is held for
//! writing, and a guarded write (`roots::write_file`) takes it for reading.

use super::fixture::Fixture;
use super::flow::Flow;
use super::here::{EnvApiState, ItemState};
use super::library::{fixture_hash, flow_hash, template_hash, ApiKind, LIBRARY_DIR, REMOVED_API_DIR, STATE_DIR};
use super::ApiTemplate;
use crate::autorun::roots::{self, REMOVED_DIR};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLockWriteGuard;

/// Staging folders (and their `.complete` marks) are named
/// `<STAGING_PREFIX><ms>` beside the library.
pub const STAGING_PREFIX: &str = ".api-library-staging-";

const MARK_SUFFIX: &str = ".complete";
const KINDS: [ApiKind; 3] = [ApiKind::Templates, ApiKind::Fixtures, ApiKind::Flows];

/// What the fold did.
#[derive(Debug, Default, PartialEq)]
pub struct MoveReport {
    /// Items now in the library that came from an environment or a removed folder.
    pub moved: usize,
    /// Items two or more environments held in different versions.
    pub conflicts: usize,
    /// Versions (and unreadable files) kept aside in `removed-api`.
    pub kept_aside: usize,
}

/// A sentence for the log. Names no path.
pub fn move_log_line(r: &MoveReport) -> String {
    let aside = if r.kept_aside > 0 { format!(", {} kept aside for 30 days", r.kept_aside) } else { String::new() };
    format!(
        "Auto Run: API templates, flows and fixtures are now shared by every environment ({} moved{aside})",
        r.moved
    )
}

/// Takes the switch lock for writing and folds. `Ok(None)` when there was
/// nothing to do.
pub fn fold_in(shared: &Path, now_ms: u64) -> Result<Option<MoveReport>, String> {
    let held = roots::switch_lock().write().unwrap_or_else(|e| e.into_inner());
    fold_in_held(&held, shared, now_ms)
}

/// `fold_in` for a caller that already holds the lock for writing.
pub fn fold_in_held(
    held: &RwLockWriteGuard<'static, ()>,
    shared: &Path,
    now_ms: u64,
) -> Result<Option<MoveReport>, String> {
    fold_in_with(held, shared, now_ms, &mut |a, b| std::fs::rename(a, b))
}

/// What moves a folder into place; tests make it fail.
pub type Rename<'a> = &'a mut dyn FnMut(&Path, &Path) -> std::io::Result<()>;

/// `fold_in_held` with the folder move passed in (the seam the tests use to
/// make a rename fail).
pub fn fold_in_with(
    _held: &RwLockWriteGuard<'static, ()>,
    shared: &Path,
    now_ms: u64,
    rename: Rename,
) -> Result<Option<MoveReport>, String> {
    let mut total = finish_marked(shared, &mut *rename)?;
    let Some(plan) = plan(shared)? else { return Ok(total) };
    let ms = free_stamp(shared, now_ms);
    let staging = shared.join(format!("{STAGING_PREFIX}{ms}"));
    build(shared, &staging, &plan)?;
    finish(shared, &staging, &mut *rename)?;
    let report = plan.report;
    let t = total.get_or_insert_with(MoveReport::default);
    t.moved += report.moved;
    t.conflicts += report.conflicts;
    t.kept_aside += report.kept_aside;
    Ok(total)
}

// ---- candidates -----------------------------------------------------------

/// Whose copy a candidate is.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Holder {
    Library,
    /// A live environment folder (the seed included).
    Env(String),
    /// A folder under `removed-envs`, by its key.
    Removed(String),
}

impl Holder {
    /// The folder name under `removed-api/<ms>/`.
    fn label(&self) -> &str {
        match self {
            Holder::Library => "library",
            Holder::Env(id) | Holder::Removed(id) => id,
        }
    }

    fn live(&self) -> bool {
        !matches!(self, Holder::Removed(_))
    }

    /// The order ties are broken in: the library, then the lower id.
    fn rank(&self) -> (u8, &str) {
        match self {
            Holder::Library => (0, ""),
            Holder::Env(id) => (1, id),
            Holder::Removed(id) => (2, id),
        }
    }
}

enum Def {
    Template(ApiTemplate),
    Fixture(Fixture),
    Flow(Flow),
}

impl Def {
    fn parse(kind: ApiKind, text: &str) -> Option<Def> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        match kind {
            ApiKind::Templates => serde_json::from_str(text).ok().map(Def::Template),
            ApiKind::Fixtures => serde_json::from_str(text).ok().map(Def::Fixture),
            ApiKind::Flows => serde_json::from_str(text).ok().map(Def::Flow),
        }
    }

    fn hash(&self) -> String {
        match self {
            Def::Template(t) => template_hash(t),
            Def::Fixture(f) => fixture_hash(f),
            Def::Flow(f) => flow_hash(f),
        }
    }

    fn proven_at(&self) -> Option<&str> {
        match self {
            Def::Template(t) => t.proven.as_ref().map(|p| p.at.as_str()),
            _ => None,
        }
    }
}

struct Candidate {
    holder: Holder,
    kind: ApiKind,
    slug: String,
    id: String,
    path: PathBuf,
    /// `None`: the file could not be parsed.
    def: Option<Def>,
    recency: String,
}

fn modified_stamp(path: &Path) -> String {
    std::fs::metadata(path).and_then(|m| m.modified()).map(crate::applog::stamp_of).unwrap_or_default()
}

fn read_runs(root: &Path, kind: ApiKind, slug: &str, id: &str) -> Vec<Value> {
    let path = root.join(kind.folder()).join(slug).join(format!("{id}.runs.json"));
    let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
    serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text)).unwrap_or_default()
}

fn run_at(r: &Value) -> Option<&str> {
    r.get("at").and_then(Value::as_str)
}

fn run_ok(r: &Value) -> bool {
    r.get("ok").and_then(Value::as_bool).unwrap_or(false)
}

/// The newest `ok` line (of the given modes, when asked) in a history.
fn newest_ok<'a>(runs: &'a [Value], modes: Option<&[&str]>) -> Option<&'a str> {
    runs.iter()
        .filter(|r| run_ok(r))
        .filter(|r| {
            modes.is_none_or(|m| m.contains(&r.get("mode").and_then(Value::as_str).unwrap_or("run")))
        })
        .filter_map(run_at)
        .max()
}

/// The newest line of a history and whether it succeeded.
fn newest_run(runs: &[Value]) -> Option<(&str, bool)> {
    runs.iter().filter_map(|r| run_at(r).map(|at| (at, run_ok(r)))).max_by(|a, b| a.0.cmp(b.0))
}

/// How recent a copy is, as a string comparable with `applog::stamp`.
fn recency_of(holder: &Holder, history_root: Option<&Path>, kind: ApiKind, slug: &str, id: &str, def: &Option<Def>, path: &Path) -> String {
    let runs = history_root.map(|r| read_runs(r, kind, slug, id)).unwrap_or_default();
    let found = match (kind, def) {
        (ApiKind::Templates, Some(d)) if *holder == Holder::Library => d.proven_at().map(str::to_string),
        (ApiKind::Templates, Some(d)) => newest_ok(&runs, Some(&["prove", "explore"]))
            .map(str::to_string)
            .or_else(|| d.proven_at().map(str::to_string)),
        (ApiKind::Fixtures, _) => newest_ok(&runs, None).map(str::to_string),
        (ApiKind::Flows, Some(Def::Flow(f))) => f.saved.as_ref().map(|s| s.at.clone()),
        _ => None,
    };
    found.unwrap_or_else(|| modified_stamp(path))
}

/// Every definition file of `kind` under `kind_dir` (`<root>/<kind>` or
/// `<library>/<kind>`), in no particular order.
fn scan(kind_dir: &Path, holder: &Holder, kind: ApiKind, history_root: Option<&Path>, out: &mut Vec<Candidate>) -> Result<(), String> {
    let slugs = match std::fs::read_dir(kind_dir) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    for slug in slugs.flatten() {
        if !slug.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let slug_name = slug.file_name().to_string_lossy().into_owned();
        for file in std::fs::read_dir(slug.path()).map_err(|e| e.to_string())?.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".json") else { continue };
            if name.ends_with(".runs.json") || !file.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let path = file.path();
            let text = std::fs::read_to_string(&path);
            let def = match text {
                Ok(t) if super::valid_id(id) => Def::parse(kind, &t),
                Ok(_) => None,
                // Non-UTF-8 content is as unreadable as a syntax error.
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => None,
                Err(e) => return Err(e.to_string()),
            };
            let recency = recency_of(holder, history_root, kind, &slug_name, id, &def, &path);
            out.push(Candidate { holder: holder.clone(), kind, slug: slug_name.clone(), id: id.to_string(), path, def, recency });
        }
    }
    Ok(())
}

// ---- the plan ---------------------------------------------------------------

type GroupKey = (&'static str, String, String);

/// What the fold will do, decided but not yet written.
struct Plan {
    /// Library files to put in (source file, library-relative destination).
    winners: Vec<(PathBuf, PathBuf)>,
    /// Files to move aside: source, destination under `removed/`.
    asides: Vec<(PathBuf, PathBuf)>,
    /// Library files that stop being there (a loser, or replaced).
    library_drops: Vec<PathBuf>,
    /// Every environment/removed-folder definition file to delete.
    sources: Vec<PathBuf>,
    /// (environment folder, slug) -> the entries the fold sets.
    states: BTreeMap<(String, String), EnvApiState>,
    report: MoveReport,
}

struct Candidates {
    all: Vec<Candidate>,
    envs: Vec<(String, PathBuf)>,
    /// Folders under `removed-envs`: they confirm nothing, but their run
    /// histories show which lines were copied from them.
    removed: Vec<(String, PathBuf)>,
}

fn gather(shared: &Path) -> Result<Candidates, String> {
    let envs: Vec<(String, PathBuf)> = roots::env_roots(shared)
        .into_iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(|n| (n.to_string(), p.clone())))
        .collect();
    let mut removed: Vec<(String, PathBuf)> = vec![];
    if let Ok(entries) = std::fs::read_dir(shared.join(REMOVED_DIR)) {
        for e in entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
            removed.push((e.file_name().to_string_lossy().into_owned(), e.path()));
        }
    }
    let mut all = vec![];
    for kind in KINDS {
        for (id, root) in &envs {
            scan(&root.join(kind.folder()), &Holder::Env(id.clone()), kind, Some(root), &mut all)?;
        }
        for (key, root) in &removed {
            scan(&root.join(kind.folder()), &Holder::Removed(key.clone()), kind, Some(root), &mut all)?;
        }
    }
    // The library only matters where something is folding into it.
    let touched: Vec<GroupKey> = all.iter().map(group_key).collect();
    let mut library = vec![];
    let lib = shared.join(LIBRARY_DIR);
    for kind in KINDS {
        scan(&lib.join(kind.folder()), &Holder::Library, kind, None, &mut library)?;
    }
    all.extend(library.into_iter().filter(|c| touched.contains(&group_key(c))));
    Ok(Candidates { all, envs, removed })
}

fn group_key(c: &Candidate) -> GroupKey {
    (c.kind.folder(), c.slug.clone(), c.id.clone())
}

/// `None` when no environment or removed folder holds a definition.
fn plan(shared: &Path) -> Result<Option<Plan>, String> {
    let found = gather(shared)?;
    if !found.all.iter().any(|c| c.holder != Holder::Library) {
        return Ok(None);
    }
    let names: BTreeMap<String, String> = crate::environments::read_names(shared)
        .ok()
        .flatten()
        .map(|(list, _)| list.into_iter().collect())
        .unwrap_or_default();

    let mut groups: BTreeMap<GroupKey, Vec<&Candidate>> = BTreeMap::new();
    for c in &found.all {
        groups.entry(group_key(c)).or_default().push(c);
    }

    let mut plan = Plan {
        winners: vec![],
        asides: vec![],
        library_drops: vec![],
        sources: found.all.iter().filter(|c| c.holder != Holder::Library).map(|c| c.path.clone()).collect(),
        states: BTreeMap::new(),
        report: MoveReport::default(),
    };
    // Templates and fixtures are decided first so a fixture can ask which
    // version of each template won.
    let mut winning_hash: BTreeMap<GroupKey, String> = BTreeMap::new();

    for (key, members) in &groups {
        let Some(winner) = pick_winner(members) else {
            // Nothing in this group could be read.
            for c in members {
                set_aside(&mut plan, c);
            }
            plan.report.kept_aside += members.len();
            continue;
        };
        let win_hash = winner.def.as_ref().map(Def::hash).unwrap_or_default();
        let live_pool = winner.holder.live();
        // Identical copies: write the one with the newest proof.
        let writer = members
            .iter()
            .filter(|c| c.holder.live() == live_pool && c.def.as_ref().is_some_and(|d| d.hash() == win_hash))
            .max_by(|a, b| {
                (a.def.as_ref().and_then(Def::proven_at), std::cmp::Reverse(a.holder.rank()))
                    .cmp(&(b.def.as_ref().and_then(Def::proven_at), std::cmp::Reverse(b.holder.rank())))
            })
            .copied()
            .unwrap_or(winner);
        let mut differing = 0;
        for c in members {
            if c.def.as_ref().is_some_and(|d| d.hash() == win_hash) {
                continue;
            }
            set_aside(&mut plan, c);
            differing += 1;
        }
        plan.report.kept_aside += differing;
        if members.iter().any(|c| c.def.as_ref().is_some_and(|d| d.hash() != win_hash)) {
            plan.report.conflicts += 1;
        }
        if writer.holder != Holder::Library {
            let dest = PathBuf::from(key.0).join(&key.1).join(format!("{}.json", key.2));
            plan.winners.push((writer.path.clone(), dest));
            plan.report.moved += 1;
        }
        winning_hash.insert(key.clone(), win_hash);
    }

    confirm(&found, &groups, &names, &winning_hash, &mut plan);
    Ok(Some(plan))
}

/// The newest live copy (the library included); a removed folder's copy
/// only when nothing live holds the id.
fn pick_winner<'a>(members: &[&'a Candidate]) -> Option<&'a Candidate> {
    let best = |live: bool| {
        members
            .iter()
            .filter(|c| c.holder.live() == live && c.def.is_some())
            .max_by(|a, b| a.recency.cmp(&b.recency).then_with(|| b.holder.rank().cmp(&a.holder.rank())))
            .copied()
    };
    best(true).or_else(|| best(false))
}

/// Keeps a loser: its file goes under `removed-api/<ms>/<holder>/...`. A
/// library loser also leaves the library.
fn set_aside(plan: &mut Plan, c: &Candidate) {
    if c.def.is_none() {
        crate::applog::warn(format!(
            "Auto Run: the {} definition '{}' could not be read and was kept aside",
            c.kind.folder().trim_end_matches('s'),
            c.id
        ));
    }
    let rel = PathBuf::from(c.holder.label()).join(c.kind.folder()).join(&c.slug).join(format!("{}.json", c.id));
    plan.asides.push((c.path.clone(), rel));
    if c.holder == Holder::Library {
        plan.library_drops.push(PathBuf::from(c.kind.folder()).join(&c.slug).join(format!("{}.json", c.id)));
    }
}

// ---- confirmations --------------------------------------------------------

fn confirm(
    found: &Candidates,
    groups: &BTreeMap<GroupKey, Vec<&Candidate>>,
    names: &BTreeMap<String, String>,
    winning_hash: &BTreeMap<GroupKey, String>,
    plan: &mut Plan,
) {
    for (env_id, root) in &found.envs {
        let own = |kind: ApiKind, slug: &str, id: &str| -> Option<&Candidate> {
            groups
                .get(&(kind.folder(), slug.to_string(), id.to_string()))?
                .iter()
                .find(|c| c.holder == Holder::Env(env_id.clone()) && c.def.is_some())
                .copied()
        };
        for (key, members) in groups {
            let Some(win) = winning_hash.get(key) else { continue };
            let Some(c) = members.iter().find(|c| c.holder == Holder::Env(env_id.clone())) else { continue };
            let Some(def) = c.def.as_ref().filter(|d| d.hash() == *win) else { continue };
            // A line another holder's history carries verbatim was copied
            // along with the data, not earned here: it confirms nothing.
            let runs = earned_runs(found, env_id, root, c.kind, &c.slug, &c.id);
            let entry = match def {
                Def::Template(t) => template_entry(t, &runs, names.get(env_id).map(String::as_str), win),
                Def::Fixture(f) => fixture_entry(f, &runs, win, |tid| {
                    let mine = own(ApiKind::Templates, &c.slug, tid)?;
                    let theirs = winning_hash.get(&(ApiKind::Templates.folder(), c.slug.clone(), tid.to_string()))?;
                    let hash = mine.def.as_ref()?.hash();
                    (hash == *theirs).then_some(hash)
                }),
                Def::Flow(_) => None,
            };
            let Some(entry) = entry else { continue };
            let state = plan.states.entry((env_id.clone(), c.slug.clone())).or_default();
            match c.kind {
                ApiKind::Templates => state.templates.insert(c.id.clone(), entry),
                _ => state.fixtures.insert(c.id.clone(), entry),
            };
        }
    }
}

/// This environment's history of the item without the lines any other
/// holder (a live environment or a removed folder) also carries verbatim.
/// Copy Auto Run data used to copy histories along with the definitions,
/// and stamps are second-resolution with mode, account and outputs in each
/// line, so an equal line is a copy and not a second run.
fn earned_runs(found: &Candidates, env_id: &str, root: &Path, kind: ApiKind, slug: &str, id: &str) -> Vec<Value> {
    let mine = read_runs(root, kind, slug, id);
    if mine.is_empty() {
        return mine;
    }
    let others: Vec<Value> = found
        .envs
        .iter()
        .filter(|(e, _)| e != env_id)
        .chain(found.removed.iter())
        .flat_map(|(_, r)| read_runs(r, kind, slug, id))
        .collect();
    mine.into_iter().filter(|line| !others.contains(line)).collect()
}

fn template_entry(t: &ApiTemplate, runs: &[Value], env_name: Option<&str>, hash: &str) -> Option<ItemState> {
    let proven = t.proven.as_ref()?;
    let proved_here = env_name.is_some() && proven.environment.as_deref() == env_name;
    let worked_since = runs.iter().any(|r| run_ok(r) && run_at(r).is_some_and(|at| at >= proven.at.as_str()));
    (proved_here || worked_since).then(|| ItemState {
        hash: hash.to_string(),
        uses: BTreeMap::new(),
        confirmed_at: Some(proven.at.clone()),
        failure: None,
        failed_here: false,
    })
}

/// `template_hash` of this environment's own copy of a step's template
/// when it is the winning version, else `None`.
fn fixture_entry(f: &Fixture, runs: &[Value], hash: &str, own_winning_template: impl Fn(&str) -> Option<String>) -> Option<ItemState> {
    let (at, ok) = newest_run(runs)?;
    if !ok {
        return None;
    }
    let mut uses = BTreeMap::new();
    for step in &f.steps {
        uses.insert(step.template.clone(), own_winning_template(&step.template)?);
    }
    Some(ItemState { hash: hash.to_string(), uses, confirmed_at: Some(at.to_string()), failure: None, failed_here: false })
}

// ---- staging and finishing -------------------------------------------------

/// A stamp no staging folder or kept-aside folder uses yet.
fn free_stamp(shared: &Path, mut ms: u64) -> u64 {
    while shared.join(format!("{STAGING_PREFIX}{ms}")).exists() || shared.join(REMOVED_API_DIR).join(ms.to_string()).exists() {
        ms += 1;
    }
    ms
}

fn mark_of(staging: &Path) -> PathBuf {
    let mut name = staging.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(MARK_SUFFIX);
    staging.with_file_name(name)
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

fn write_into(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::write(path, bytes).map_err(io)
}

fn copy_into(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::copy(from, to).map(|_| ()).map_err(io)
}

fn relative(shared: &Path, p: &Path) -> Result<String, String> {
    let rel = p.strip_prefix(shared).map_err(|e| e.to_string())?;
    Ok(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
}

fn build(shared: &Path, staging: &Path, plan: &Plan) -> Result<(), String> {
    std::fs::create_dir_all(staging).map_err(io)?;
    let lib = staging.join("library");
    let live = shared.join(LIBRARY_DIR);
    if live.is_dir() {
        roots::copy_tree(&live, &lib, Path::new(""), false, &|_| false)?;
    } else {
        std::fs::create_dir_all(&lib).map_err(io)?;
    }
    for rel in &plan.library_drops {
        let _ = std::fs::remove_file(lib.join(rel));
    }
    for (from, rel) in &plan.winners {
        copy_into(from, &lib.join(rel))?;
    }
    let removed = staging.join("removed");
    for (from, rel) in &plan.asides {
        copy_into(from, &removed.join(rel))?;
    }
    for ((env, slug), fold) in &plan.states {
        let mut merged = existing_state(&roots::env_root(shared, env), slug);
        merged.templates.extend(fold.templates.iter().map(|(k, v)| (k.clone(), v.clone())));
        merged.fixtures.extend(fold.fixtures.iter().map(|(k, v)| (k.clone(), v.clone())));
        let text = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?;
        write_into(&staging.join("state").join(env).join(format!("{slug}.json")), text.as_bytes())?;
    }
    let sources = plan.sources.iter().map(|p| relative(shared, p)).collect::<Result<Vec<_>, _>>()?;
    write_into(&staging.join("sources.json"), serde_json::to_string(&sources).map_err(|e| e.to_string())?.as_bytes())?;
    let r = &plan.report;
    write_into(&mark_of(staging), format!("{} {} {}", r.moved, r.conflicts, r.kept_aside).as_bytes())
}

fn existing_state(env_root: &Path, slug: &str) -> EnvApiState {
    std::fs::read_to_string(env_root.join(STATE_DIR).join(format!("{slug}.json")))
        .ok()
        .and_then(|t| serde_json::from_str(t.strip_prefix('\u{feff}').unwrap_or(&t)).ok())
        .unwrap_or_default()
}

fn rename(from: &Path, to: &Path, mover: Rename) -> Result<(), String> {
    crate::ai_tools::rename_retrying_waits(
        from,
        to,
        &mut *mover,
        &mut std::thread::sleep,
        &roots::MIGRATION_RENAME_WAITS_MS,
    )
}

/// Settles every staging folder a cut-short start left: a marked one is
/// finished, an unmarked one (or a mark with no folder) is deleted.
/// Returns what the finished ones had done.
fn finish_marked(shared: &Path, rename: Rename) -> Result<Option<MoveReport>, String> {
    let Ok(entries) = std::fs::read_dir(shared) else { return Ok(None) };
    let mut marked: Vec<PathBuf> = vec![];
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(STAGING_PREFIX) {
            continue;
        }
        let path = entry.path();
        if let Some(folder) = name.strip_suffix(MARK_SUFFIX) {
            if !shared.join(folder).is_dir() {
                let _ = std::fs::remove_file(&path);
            }
        } else if path.is_dir() && mark_of(&path).is_file() {
            marked.push(path);
        } else if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(io)?;
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    marked.sort();
    let mut total: Option<MoveReport> = None;
    for staging in marked {
        let nums: Vec<usize> = std::fs::read_to_string(mark_of(&staging))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        finish(shared, &staging, &mut *rename)?;
        let t = total.get_or_insert_with(MoveReport::default);
        if let [moved, conflicts, kept_aside] = nums[..] {
            t.moved += moved;
            t.conflicts += conflicts;
            t.kept_aside += kept_aside;
        }
    }
    Ok(total)
}

/// Runs the steps after the mark. Every step is safe to run again, and no
/// step ever deletes a library: the one it replaces ends up kept aside.
fn finish(shared: &Path, staging: &Path, mover: Rename) -> Result<(), String> {
    let removed = staging.join("removed");
    // (a) the new library replaces the old one.
    let new_library = staging.join("library");
    let live = shared.join(LIBRARY_DIR);
    let old = staging.join("library-old");
    if new_library.is_dir() {
        if live.exists() && old.exists() {
            // A earlier try already set the original aside, so what is live
            // now was made since: it is data, not something to overwrite.
            std::fs::create_dir_all(&removed).map_err(io)?;
            rename(&live, &removed.join("library-live"), &mut *mover)?;
        } else if live.exists() {
            rename(&live, &old, &mut *mover)?;
        }
        if let Err(e) = rename(&new_library, &live, &mut *mover) {
            // Never leave the app without a library: put the original back.
            if old.exists() && !live.exists() {
                let _ = rename(&old, &live, &mut *mover);
            }
            return Err(e);
        }
    }
    // The library it replaced waits with the other kept versions.
    if old.is_dir() && live.exists() {
        std::fs::create_dir_all(&removed).map_err(io)?;
        rename(&old, &removed.join("library-old"), &mut *mover)?;
    }
    // (b) each environment's confirmations.
    let states = staging.join("state");
    if let Ok(envs) = std::fs::read_dir(&states) {
        for env in envs.flatten() {
            let env_root = roots::env_root(shared, &env.file_name().to_string_lossy());
            for file in std::fs::read_dir(env.path()).map_err(io)?.flatten() {
                if env_root.is_dir() {
                    let text = std::fs::read_to_string(file.path()).map_err(io)?;
                    let dest = env_root.join(STATE_DIR).join(file.file_name());
                    if let Some(dir) = dest.parent() {
                        std::fs::create_dir_all(dir).map_err(io)?;
                    }
                    crate::ai_tools::atomic_write(&dest, &text)?;
                }
                std::fs::remove_file(file.path()).map_err(io)?;
            }
        }
    }
    // (c) the losers wait for 30 days.
    if removed.is_dir() {
        let ms = staging.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix(STAGING_PREFIX)).unwrap_or("0");
        std::fs::create_dir_all(shared.join(REMOVED_API_DIR)).map_err(io)?;
        rename(&removed, &shared.join(REMOVED_API_DIR).join(ms), &mut *mover)?;
    }
    // (d) the copies the environments held.
    delete_sources(shared, staging)?;
    // (e) done.
    std::fs::remove_dir_all(staging).map_err(io)?;
    let _ = std::fs::remove_file(mark_of(staging));
    Ok(())
}

fn delete_sources(shared: &Path, staging: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(staging.join("sources.json")).unwrap_or_else(|_| "[]".to_string());
    let sources: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    for rel in sources {
        let rel = Path::new(&rel);
        // Only ever inside the shared root.
        if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            continue;
        }
        let path = shared.join(rel);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        // A flows folder left empty goes too; one holding anything stays.
        let slug_dir = path.parent();
        let flows = slug_dir.and_then(Path::parent);
        if flows.and_then(Path::file_name).is_some_and(|n| n == ApiKind::Flows.folder()) {
            if let Some(slug_dir) = slug_dir {
                let _ = std::fs::remove_dir(slug_dir);
            }
            if let Some(flows) = flows {
                let _ = std::fs::remove_dir(flows);
            }
        }
    }
    Ok(())
}
