//! Per-project quirks: short, attributed notes about the application that
//! a person or an assistant has learned the hard way, so the next script -
//! written by either - does not rediscover the same surprise.
//!
//! Saved beside the project's sign-in recipe, under its own file. One file
//! per project, read by both assistants: the Auto Run guide and the API
//! templates guide end with the same section.
//!
//! The list keeps itself honest. A quirk filed with a repair remembers the
//! case and steps it was about (`sources`); each unattended run afterwards
//! counts whether those steps passed (`confirmed`) or failed the same way
//! again (`doubted`), and the guide shows that next to the note. Only the
//! ACTIVE notes count against the cap; a full list refuses one more and
//! names the best ones to retire. Retired notes stay in the file (out of
//! every guide) so a person can restore one.
//!
//! None of this ever changes a script or starts anything: a quirk is text
//! an assistant reads, and the counts are bookkeeping on that text.

use super::patterns::{step_failure_class, ErrorClass};
use super::recipe::project_slug;
use super::{CaseRecord, CaseScript, StepRecord};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The case and steps a quirk was filed about, with a repair.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct QuirkSource {
    pub case_id: i32,
    pub steps: Vec<u32>,
    /// The error class (`patterns::ErrorClass::key`) of the failure that
    /// led to the repair, when the run that showed it is on this machine.
    /// A later failure of the same class is evidence the note did not
    /// help; with no class known, any failure is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_RETIRED: &str = "retired";
pub const FROM_AUTORUN: &str = "autorun";
pub const FROM_API: &str = "api";

fn active() -> String {
    STATUS_ACTIVE.to_string()
}

fn autorun() -> String {
    FROM_AUTORUN.to_string()
}

/// Every field but `text`, `by` and `at` is `#[serde(default)]`: a file
/// written before they existed loads unchanged, and gains ids on load
/// (written down by the next save).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Quirk {
    /// Short and stable: what the retire tool and the app's own buttons
    /// name a quirk by.
    #[serde(default)]
    pub id: String,
    pub text: String,
    /// "person" or "assistant"
    pub by: String,
    /// Epoch milliseconds as a string.
    pub at: String,
    #[serde(default)]
    pub sources: Vec<QuirkSource>,
    /// Source steps that passed in a run since the note was filed.
    #[serde(default)]
    pub confirmed: u32,
    /// Epoch milliseconds as a string, of the latest of those runs.
    #[serde(default)]
    pub last_confirmed: Option<String>,
    /// Source steps that failed the same way again.
    #[serde(default)]
    pub doubted: u32,
    /// "active" or "retired".
    #[serde(default = "active")]
    pub status: String,
    #[serde(default)]
    pub retired_reason: Option<String>,
    /// Epoch milliseconds as a string.
    #[serde(default)]
    pub retired_at: Option<String>,
    /// Which assistant's work it came from: "autorun" or "api".
    #[serde(default = "autorun")]
    pub from: String,
}

impl Quirk {
    /// A fresh, active note with no evidence yet. The id is filled in by
    /// whatever list it joins (`ensure_ids`).
    pub fn new(text: &str, by: &str, from: &str, now_ms: u64) -> Quirk {
        Quirk {
            id: String::new(),
            text: text.trim().to_string(),
            by: by.to_string(),
            at: now_ms.to_string(),
            sources: Vec::new(),
            confirmed: 0,
            last_confirmed: None,
            doubted: 0,
            status: active(),
            retired_reason: None,
            retired_at: None,
            from: from.to_string(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.status != STATUS_RETIRED
    }

    fn at_ms(&self) -> u64 {
        self.at.parse().unwrap_or(0)
    }
}

/// How many ACTIVE quirks a project keeps. Past this, one has to be
/// retired before another is added - the list is read in full by every
/// assistant, and a long one stops being read.
pub const MAX_QUIRKS: usize = 40;

/// How many retired quirks the file keeps for a person to restore. The
/// oldest retirements are dropped first.
pub const MAX_RETIRED: usize = 40;

/// One quirk is one line, read at a glance.
pub const MAX_QUIRK_CHARS: usize = 300;

/// The MCP tool that retires an assistant's quirk - named in the cap's
/// refusal and in both guides.
pub const RETIRE_TOOL: &str = "retire_autorun_quirk";

/// What the retire tool answers about a person's note.
pub const PERSON_NOTE: &str = "that note was written by a person - ask them to remove it";

pub fn quirks_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}-quirks.json", project_slug(org, project)))
}

/// Every read-change-write of a quirks file holds this: the person's
/// dialog, an assistant's record or retire, and a finished run's evidence
/// can all land at once.
fn write_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn load_quirks(root: &Path, org: &str, project: &str) -> Result<Vec<Quirk>, String> {
    match std::fs::read_to_string(quirks_path(root, org, project)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            let mut list: Vec<Quirk> =
                serde_json::from_str(s).map_err(|e| format!("the project quirks file is not readable: {e}"))?;
            ensure_ids(&mut list);
            Ok(list)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

/// A short id from a seed: `q` and six hex digits of an FNV-1a hash.
/// Deterministic, so an old file's notes get the same ids on every load
/// until a save writes them down.
fn quirk_id(seed: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("q{:06x}", h & 0xff_ffff)
}

/// Gives every quirk without an id (or with one an earlier quirk already
/// has) an id of its own.
pub fn ensure_ids(list: &mut [Quirk]) {
    let mut taken: Vec<String> = Vec::new();
    for i in 0..list.len() {
        let id = list[i].id.trim().to_string();
        if !id.is_empty() && !taken.contains(&id) {
            taken.push(id);
            continue;
        }
        let mut n = 0u32;
        let fresh = loop {
            let candidate = quirk_id(&format!("{}|{}|{n}", list[i].at, list[i].text));
            if !taken.contains(&candidate) && !list.iter().any(|q| q.id == candidate) {
                break candidate;
            }
            n += 1;
        };
        list[i].id = fresh.clone();
        taken.push(fresh);
    }
}

fn validate(quirks: &[Quirk]) -> Result<(), String> {
    let active = quirks.iter().filter(|q| q.is_active()).count();
    if active > MAX_QUIRKS {
        return Err(format!(
            "a project keeps at most {MAX_QUIRKS} active quirks - retire one before adding another"
        ));
    }
    let mut seen: Vec<String> = Vec::new();
    for q in quirks {
        if q.text.trim().is_empty() {
            return Err("a quirk needs some text".to_string());
        }
        if q.text.chars().count() > MAX_QUIRK_CHARS {
            return Err(format!("a quirk is at most {MAX_QUIRK_CHARS} characters"));
        }
        if q.text.contains('\n') {
            return Err("a quirk is one line - remove the line break".to_string());
        }
        if q.status != STATUS_ACTIVE && q.status != STATUS_RETIRED {
            return Err(format!("a quirk is \"{STATUS_ACTIVE}\" or \"{STATUS_RETIRED}\", not \"{}\"", q.status));
        }
        // Case- and whitespace-insensitive, the same rule `record_in` uses
        // to skip a repeat - and across retired notes too, so restoring one
        // can never put the same fact on the list twice.
        let norm = normalized(q.text.trim());
        if seen.contains(&norm) {
            return Err(format!("quirk \"{}\" appears twice", q.text.trim()));
        }
        seen.push(norm);
    }
    Ok(())
}

/// Keeps the newest `MAX_RETIRED` retirements, dropping the oldest.
fn trim_retired(list: &mut Vec<Quirk>) {
    let mut retired: Vec<(u64, String)> = list
        .iter()
        .filter(|q| !q.is_active())
        .map(|q| (q.retired_at.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0), q.id.clone()))
        .collect();
    if retired.len() <= MAX_RETIRED {
        return;
    }
    retired.sort();
    let drop: Vec<String> = retired[..retired.len() - MAX_RETIRED].iter().map(|(_, id)| id.clone()).collect();
    list.retain(|q| q.is_active() || !drop.contains(&q.id));
}

/// Replaces the whole list. Validated first, then written to a temporary
/// file and renamed, so a reader never sees a half-written list and a
/// refused save leaves the earlier one exactly as it was.
pub fn save_quirks(root: &Path, org: &str, project: &str, quirks: &[Quirk]) -> Result<(), String> {
    let mut list = quirks.to_vec();
    ensure_ids(&mut list);
    validate(&list)?;
    trim_retired(&mut list);
    // `slug_part` (behind `project_slug`, via `quirks_path`) never reads as
    // empty any more, so - as with `recipe::save_recipe` - the real refusal,
    // nothing was typed at all, has to be checked on the raw names here.
    if org.trim().is_empty() || project.trim().is_empty() {
        return Err("pick an organization and a project first".to_string());
    }
    let path = quirks_path(root, org, project);
    std::fs::create_dir_all(path.parent().expect("projects folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

/// Load, change, save - under the write lock - and hand back the list as
/// saved. A change that fails writes nothing.
pub fn update_quirks<T>(
    root: &Path,
    org: &str,
    project: &str,
    change: impl FnOnce(&mut Vec<Quirk>) -> Result<T, String>,
) -> Result<(T, Vec<Quirk>), String> {
    let _held = write_lock();
    let mut list = load_quirks(root, org, project)?;
    let out = change(&mut list)?;
    save_quirks(root, org, project, &list)?;
    // What was written, retired trimming included.
    let saved = load_quirks(root, org, project)?;
    Ok((out, saved))
}

/// Collapse runs of whitespace to a single space and lower-case, so "the
/// grid  paginates" and "THE GRID PAGINATES" compare equal regardless of
/// case or how the spacing landed.
fn normalized(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

// ------------------------------------------------------------ the cap

/// The assistant's notes most worth retiring: never confirmed, or more
/// often unhelpful than helpful - oldest first, at most three. A person's
/// note is never a candidate; it is theirs to remove.
pub fn retire_candidates(list: &[Quirk]) -> Vec<&Quirk> {
    let mut out: Vec<&Quirk> = list
        .iter()
        .filter(|q| q.is_active() && q.by == "assistant" && (q.confirmed == 0 || q.doubted > q.confirmed))
        .collect();
    out.sort_by_key(|q| q.at_ms());
    out.truncate(3);
    out
}

/// Why one more quirk does not fit, and which to retire. Worded for an
/// assistant (naming the retire tool) or for the person in the app.
pub fn cap_refusal(list: &[Quirk], for_assistant: bool) -> String {
    let candidates = retire_candidates(list);
    let mut out = if for_assistant {
        format!(
            "this project already has {MAX_QUIRKS} active quirks - retire one with {RETIRE_TOOL} {{ id, reason, replacement? }} before adding another."
        )
    } else {
        format!("This project already has {MAX_QUIRKS} active notes - retire one before adding another.")
    };
    if candidates.is_empty() {
        out.push_str(
            " No assistant note is an obvious candidate (each has been confirmed by a run more often than not) - pick the least useful one.",
        );
    } else {
        let names: Vec<String> = candidates.iter().map(|q| format!("{} \"{}\"", q.id, q.text)).collect();
        out.push_str(&format!(
            " Best candidates - written by an assistant, never confirmed or more often unhelpful than helpful, oldest first: {}.",
            names.join("; ")
        ));
    }
    out
}

fn active_count(list: &[Quirk]) -> usize {
    list.iter().filter(|q| q.is_active()).count()
}

// ------------------------------------------------------------ adding

/// What `record_in` did.
#[derive(Debug, Clone, PartialEq)]
pub enum Recorded {
    /// A new note, with its id.
    Added(String),
    /// The same fact was already active; its id. A source that came with
    /// it is added to that note.
    AlreadyKnown(String),
    /// The same fact had been retired: it is active again rather than
    /// copied. Its id.
    Reactivated(String),
}

impl Recorded {
    pub fn id(&self) -> &str {
        match self {
            Recorded::Added(id) | Recorded::AlreadyKnown(id) | Recorded::Reactivated(id) => id,
        }
    }
}

/// A source joins a note's list once per case: the steps are merged, and
/// a class the note did not know yet is kept.
fn merge_source(q: &mut Quirk, source: QuirkSource) {
    match q.sources.iter_mut().find(|s| s.case_id == source.case_id) {
        Some(s) => {
            for step in source.steps {
                if !s.steps.contains(&step) {
                    s.steps.push(step);
                }
            }
            s.steps.sort_unstable();
            if s.class.is_none() {
                s.class = source.class;
            }
        }
        None => q.sources.push(source),
    }
}

/// Adds one note to a list in memory. A repeat (case- and whitespace-
/// insensitive) of an active note adds nothing new but its sources; a
/// repeat of a retired one brings that one back rather than copying it.
/// Refused when the active list is full, with the candidates to retire.
pub fn record_in(
    list: &mut Vec<Quirk>,
    text: &str,
    by: &str,
    from: &str,
    sources: Vec<QuirkSource>,
    now_ms: u64,
) -> Result<Recorded, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("a quirk needs some text".to_string());
    }
    let for_assistant = by == "assistant";
    if let Some(i) = list.iter().position(|q| normalized(&q.text) == normalized(trimmed)) {
        if list[i].is_active() {
            for s in sources {
                merge_source(&mut list[i], s);
            }
            return Ok(Recorded::AlreadyKnown(list[i].id.clone()));
        }
        if active_count(list) >= MAX_QUIRKS {
            return Err(cap_refusal(list, for_assistant));
        }
        let q = &mut list[i];
        q.status = active();
        q.retired_reason = None;
        q.retired_at = None;
        for s in sources {
            merge_source(q, s);
        }
        return Ok(Recorded::Reactivated(q.id.clone()));
    }
    if active_count(list) >= MAX_QUIRKS {
        return Err(cap_refusal(list, for_assistant));
    }
    let mut q = Quirk::new(trimmed, by, from, now_ms);
    q.sources = sources;
    list.push(q);
    ensure_ids(list);
    Ok(Recorded::Added(list.last().expect("just pushed").id.clone()))
}

/// Records one note in the project's file.
#[allow(clippy::too_many_arguments)]
pub fn record_quirk(
    root: &Path,
    org: &str,
    project: &str,
    text: &str,
    by: &str,
    from: &str,
    sources: Vec<QuirkSource>,
    now_ms: u64,
) -> Result<Recorded, String> {
    update_quirks(root, org, project, |list| record_in(list, text, by, from, sources, now_ms)).map(|(r, _)| r)
}

/// Appends one note from the Auto Run side, unless the same fact is
/// already active, in which case nothing new is written and this returns
/// `false`.
pub fn add_quirk(root: &Path, org: &str, project: &str, text: &str, by: &str, now_ms: u64) -> Result<bool, String> {
    record_quirk(root, org, project, text, by, FROM_AUTORUN, Vec::new(), now_ms)
        .map(|r| !matches!(r, Recorded::AlreadyKnown(_)))
}

// ------------------------------------------------------------ changing

fn find_mut<'a>(list: &'a mut [Quirk], id: &str) -> Result<&'a mut Quirk, String> {
    let id = id.trim();
    list.iter_mut().find(|q| q.id == id).ok_or_else(|| format!("no quirk {id} on this project's list"))
}

/// Retires a note. From an assistant (`by_assistant`), a person's note is
/// refused and a reason is required. `replacement`, when given, is filed
/// in the same change as a new note by the assistant that inherits the
/// retired one's sources (and its `from`); a replacement that repeats a
/// note already on the list joins that one instead. Returns what the
/// replacement became, when there is one.
pub fn retire_in(
    list: &mut Vec<Quirk>,
    id: &str,
    reason: Option<&str>,
    replacement: Option<&str>,
    by_assistant: bool,
    now_ms: u64,
) -> Result<Option<Recorded>, String> {
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());
    if by_assistant && reason.is_none() {
        return Err("a retirement needs a reason - one sentence on why the note no longer helps".to_string());
    }
    if reason.is_some_and(|r| r.contains('\n') || r.chars().count() > MAX_QUIRK_CHARS) {
        return Err(format!("a reason is one line of at most {MAX_QUIRK_CHARS} characters"));
    }
    let q = find_mut(list, id)?;
    if by_assistant && q.by != "assistant" {
        return Err(PERSON_NOTE.to_string());
    }
    if !q.is_active() {
        return Err(format!("quirk {} is already retired", q.id));
    }
    q.status = STATUS_RETIRED.to_string();
    q.retired_reason = reason.map(str::to_string);
    q.retired_at = Some(now_ms.to_string());
    let (sources, from) = (q.sources.clone(), q.from.clone());
    match replacement.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(text) => record_in(list, text, "assistant", &from, sources, now_ms).map(Some),
    }
}

/// Puts a retired note back on the active list - refused when it is full.
pub fn restore_in(list: &mut [Quirk], id: &str) -> Result<(), String> {
    if find_mut(list, id)?.is_active() {
        return Ok(());
    }
    if active_count(list) >= MAX_QUIRKS {
        return Err(cap_refusal(list, false));
    }
    let q = find_mut(list, id)?;
    q.status = active();
    q.retired_reason = None;
    q.retired_at = None;
    Ok(())
}

/// A note's text, changed in place: its author, date and evidence stay.
pub fn edit_in(list: &mut [Quirk], id: &str, text: &str) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("a quirk needs some text".to_string());
    }
    find_mut(list, id)?.text = text.to_string();
    Ok(())
}

/// Removes a note from the file entirely.
pub fn delete_in(list: &mut Vec<Quirk>, id: &str) -> Result<(), String> {
    let before = list.len();
    list.retain(|q| q.id != id.trim());
    if list.len() == before {
        return Err(format!("no quirk {} on this project's list", id.trim()));
    }
    Ok(())
}

// ------------------------------------------------------------ evidence

/// The source a repair's quirk is filed with: the repaired case, its
/// declared steps, and the class of the first failure among those steps
/// in `run` (the newest run of the case), classified against the script
/// that ran - the one on disk before the repair.
pub fn source_for_repair(run: Option<&super::LocalRun>, ran: &CaseScript, case_id: i32, steps: &[i32]) -> QuirkSource {
    let class = run
        .and_then(|r| r.cases.iter().rev().find(|c| c.case_id == case_id))
        .and_then(|case| {
            case.steps
                .iter()
                .filter(|s| steps.contains(&s.step_number))
                .find_map(|s| step_failure_class(s, Some(ran)))
        })
        .map(|c| c.key().to_string());
    let mut steps: Vec<u32> = steps.iter().filter_map(|n| u32::try_from(*n).ok()).collect();
    steps.sort_unstable();
    steps.dedup();
    QuirkSource { case_id, steps, class }
}

/// Every action in the step ran and passed.
fn passed(step: &StepRecord) -> bool {
    !step.outcomes.is_empty() && step.outcomes.iter().all(|o| o.ok)
}

/// Counts what one run says about each active note that has sources: a
/// source step that PASSED confirms the note; one that FAILED with the
/// class of the failure that led to the repair (any failure, when that
/// class is not known) is a run in which it did not help. A step that did
/// not run - not in the run at all, or skipped - says nothing, and neither
/// does a browser that stopped answering. Returns whether anything
/// changed.
///
/// `cases` is the case records of the run being counted; `scripts` the
/// scripts that ran, for the failures' targets.
pub fn apply_run_evidence(quirks: &mut [Quirk], cases: &[CaseRecord], scripts: &[CaseScript], now_ms: u64) -> bool {
    let mut changed = false;
    for q in quirks.iter_mut().filter(|q| q.is_active() && !q.sources.is_empty()) {
        let (mut confirmed, mut doubted) = (0u32, 0u32);
        for src in &q.sources {
            // The newest record of the case in this run, if it ran.
            let Some(case) = cases.iter().rev().find(|c| c.case_id == src.case_id) else { continue };
            let script = scripts.iter().find(|s| s.case_id == src.case_id);
            for &n in &src.steps {
                let Some(step) = case.steps.iter().find(|s| i64::from(s.step_number) == i64::from(n)) else {
                    continue;
                };
                if passed(step) {
                    confirmed += 1;
                    continue;
                }
                let Some(class) = step_failure_class(step, script) else { continue };
                if matches!(class, ErrorClass::Browser | ErrorClass::CannotRun) {
                    continue;
                }
                if src.class.as_deref().is_none_or(|c| c == class.key()) {
                    doubted += 1;
                }
            }
        }
        if confirmed > 0 {
            q.confirmed += confirmed;
            q.last_confirmed = Some(now_ms.to_string());
            changed = true;
        }
        if doubted > 0 {
            q.doubted += doubted;
            changed = true;
        }
    }
    changed
}

/// After a run: counts its evidence into the project's quirks file. Never
/// fails the run - a file that cannot be read or written is logged and
/// left as it was. Returns whether the file was written.
pub fn record_run_evidence(root: &Path, org: &str, project: &str, cases: &[CaseRecord], now_ms: u64) -> bool {
    if org.trim().is_empty() || project.trim().is_empty() || cases.is_empty() {
        return false;
    }
    let _held = write_lock();
    let mut list = match load_quirks(root, org, project) {
        Ok(l) => l,
        Err(e) => {
            crate::applog::warn(format!("Auto Run: the project's quirks were not updated after the run: {e}"));
            return false;
        }
    };
    if !list.iter().any(|q| q.is_active() && !q.sources.is_empty()) {
        return false;
    }
    let scripts: Vec<CaseScript> =
        cases.iter().filter_map(|c| super::store::load_script(root, c.case_id).ok().flatten()).collect();
    if !apply_run_evidence(&mut list, cases, &scripts, now_ms) {
        return false;
    }
    match save_quirks(root, org, project, &list) {
        Ok(()) => true,
        Err(e) => {
            crate::applog::warn(format!("Auto Run: the project's quirks were not updated after the run: {e}"));
            false
        }
    }
}

// ------------------------------------------------------------ the guide

/// An epoch-milliseconds string as a UTC date, `2026-10-01`.
pub fn day_of(ms: &str) -> String {
    let ms: i64 = ms.parse().unwrap_or(0);
    let (y, m, d) = crate::applog::civil(ms.div_euclid(86_400_000));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Who wrote it and what the runs since say, as the guide shows it:
/// `assistant, confirmed 3x, last 2026-10-01` / `assistant, API, did not
/// help 2x` / `person`. Evidence only for a note with sources - one filed
/// on its own is not tied to any step a run could test.
pub fn attribution(q: &Quirk) -> String {
    let assistant = q.by == "assistant";
    let mut parts: Vec<String> = vec![if assistant { "assistant" } else { "person" }.to_string()];
    if assistant && q.from == FROM_API {
        parts.push("API".to_string());
    }
    if !q.sources.is_empty() {
        if q.confirmed > 0 {
            let last = q.last_confirmed.as_deref().map(day_of).unwrap_or_default();
            parts.push(format!("confirmed {}x, last {last}", q.confirmed));
        }
        if q.doubted > 0 {
            parts.push(format!("did not help {}x", q.doubted));
        }
        if q.confirmed == 0 && q.doubted == 0 {
            parts.push("not yet tested by a run".to_string());
        }
    }
    parts.join(", ")
}

/// A Markdown section for an assistant's guide: the ACTIVE notes, each
/// with its id and evidence, or nothing at all when there are none.
/// Shared word for word by the Auto Run guide and the API templates guide.
pub fn quirks_section(quirks: &[Quirk]) -> String {
    let active: Vec<&Quirk> = quirks.iter().filter(|q| q.is_active()).collect();
    if active.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Known quirks of this application\n\n");
    out.push_str(&format!(
        "Each line starts with the note's id, for {RETIRE_TOOL}. \"confirmed\" counts source steps that passed in a run since; \"did not help\" counts ones that failed the same way again.\n\n"
    ));
    for q in active {
        out.push_str(&format!("- [{}] ({}) {}\n", q.id, attribution(q), q.text));
    }
    out
}
