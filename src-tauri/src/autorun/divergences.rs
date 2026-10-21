//! Where a test case and the live application disagree: a count, a text,
//! an order or a state. An assistant reports it (`report_case_divergence`)
//! instead of writing the check to fit the page; a person rules on it in
//! Auto Run.
//!
//! One JSON array per project, beside the quirks file. A `data` or
//! `wording` divergence blocks its case's runs until it is ruled on; an
//! `app` one never blocks (a real bug must surface as a failure). A file
//! that cannot be read blocks nothing and is never overwritten.
//!
//! The rules are pure functions on a list; `update` is the one writer.

use super::case_text::fold;
use super::quirks::day_of;
use super::recipe::project_slug;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum DivergenceKind {
    Data,
    App,
    Wording,
}

impl DivergenceKind {
    pub fn word(self) -> &'static str {
        match self {
            DivergenceKind::Data => "data",
            DivergenceKind::App => "app",
            DivergenceKind::Wording => "wording",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DivergenceStatus {
    Open,
    AwaitingRepair,
    Ruled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Ruling {
    CaseStands,
    DataFixed,
    Rewrite,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Divergence {
    /// "d1", "d2"...: 1 + the highest number in the file.
    pub id: String,
    pub case_id: i32,
    pub step: i32,
    /// Scrubbed (`defects::scrub_note`), at most `MAX_TEXT` characters.
    pub case_says: String,
    pub app_shows: String,
    pub kind: DivergenceKind,
    /// Epoch milliseconds, as a string.
    pub reported_at: String,
    pub status: DivergenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ruling: Option<Ruling>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ruled_at: Option<String>,
}

pub const MAX_TEXT: usize = 300;
pub const MAX_OPEN_PER_CASE: usize = 5;
pub const KEEP_RULED_MS: u64 = 90 * 24 * 60 * 60 * 1000;
pub const MAX_KEPT: usize = 200;
pub const NEEDS_RULING_PREFIX: &str = "needs a ruling: ";
pub const WAITS_FOR_REPAIR_PREFIX: &str = "waits for a repair: ";

const UNREADABLE: &str = "the divergences file could not be read";

// ----------------------------------------------------------------- storage

pub fn path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}-divergences.json", project_slug(org, project)))
}

/// The project's divergences. A missing file is none; one that cannot be
/// read or parsed is an error that names no path.
pub fn load(root: &Path, org: &str, project: &str) -> Result<Vec<Divergence>, String> {
    let p = path(root, org, project);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(UNREADABLE.to_string()),
    };
    serde_json::from_str(&text).map_err(|_| UNREADABLE.to_string())
}

/// Read, change, prune and write the list under one lock. A file that
/// cannot be read refuses the write and is left as it is; so does `f`'s
/// error. The Auto Run screen is told which cases changed.
pub fn update<T>(
    root: &Path,
    org: &str,
    project: &str,
    f: impl FnOnce(&mut Vec<Divergence>) -> Result<T, String>,
) -> Result<T, String> {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let before = load(root, org, project)?;
    let mut list = before.clone();
    let out = f(&mut list)?;
    prune(&mut list, super::sessions::now_ms());
    let p = path(root, org, project);
    std::fs::create_dir_all(p.parent().expect("projects folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&p, &json)?;
    let mut touched: Vec<i32> = before
        .iter()
        .filter(|d| !list.contains(d))
        .chain(list.iter().filter(|d| !before.contains(d)))
        .map(|d| d.case_id)
        .collect();
    touched.sort_unstable();
    touched.dedup();
    super::store::notify_scripts_changed(root, &touched);
    Ok(out)
}

fn ms_of(s: &str) -> u64 {
    s.trim().parse().unwrap_or(0)
}

/// Ruled entries older than `KEEP_RULED_MS` go; then, while over
/// `MAX_KEPT`, the oldest ruled ones.
fn prune(list: &mut Vec<Divergence>, now_ms: u64) {
    list.retain(|d| {
        d.status != DivergenceStatus::Ruled
            || d.ruled_at.as_deref().map(ms_of).is_none_or(|at| now_ms.saturating_sub(at) <= KEEP_RULED_MS)
    });
    while list.len() > MAX_KEPT {
        let oldest = list
            .iter()
            .enumerate()
            .filter(|(_, d)| d.status == DivergenceStatus::Ruled)
            .min_by_key(|(_, d)| d.ruled_at.as_deref().map(ms_of).unwrap_or(0))
            .map(|(i, _)| i);
        match oldest {
            Some(i) => {
                list.remove(i);
            }
            None => break,
        }
    }
}

// ------------------------------------------------------------------- rules

/// What a report did: the id it made, or the one it refreshed.
#[derive(Debug, Clone, PartialEq)]
pub enum Reported {
    New(String),
    Updated(String),
}

fn next_id(list: &[Divergence]) -> String {
    let highest = list.iter().filter_map(|d| d.id.strip_prefix('d')?.parse::<u32>().ok()).max().unwrap_or(0);
    format!("d{}", highest + 1)
}

pub fn report_in(
    list: &mut Vec<Divergence>,
    case_id: i32,
    step: i32,
    case_says: &str,
    app_shows: &str,
    kind: DivergenceKind,
    now_ms: u64,
) -> Result<Reported, String> {
    if case_id < 1 {
        return Err("case_id is a test case's id".to_string());
    }
    if step < 1 {
        return Err("step is the case's step number, 1 or more".to_string());
    }
    let (case_says, app_shows) = (case_says.trim(), app_shows.trim());
    if case_says.is_empty() || app_shows.is_empty() {
        return Err("a divergence needs \"case_says\" (the case's own words) and \"app_shows\" (what the page showed)"
            .to_string());
    }
    let too_long = |s: &str| s.chars().count() > MAX_TEXT || s.contains('\n') || s.contains('\r');
    if too_long(case_says) || too_long(app_shows) {
        return Err("\"case_says\" and \"app_shows\" are each one line of at most 300 characters".to_string());
    }
    let (fold_says, fold_shows) = (fold(case_says), fold(app_shows));
    if let Some(d) = list.iter().find(|d| {
        d.status == DivergenceStatus::Ruled
            && d.ruling == Some(Ruling::CaseStands)
            && d.case_id == case_id
            && d.step == step
            && fold(&d.case_says) == fold_says
            && fold(&d.app_shows) == fold_shows
    }) {
        let day = day_of(d.ruled_at.as_deref().unwrap_or("0"));
        return Err(format!(
            "a person ruled on {day} that the case stands at step {step} - keep the check as the case says; a failure there is the data's or the application's"
        ));
    }
    let now = now_ms.to_string();
    if let Some(d) = list.iter_mut().find(|d| {
        d.status == DivergenceStatus::Open && d.case_id == case_id && d.step == step && d.kind == kind
    }) {
        d.case_says = case_says.to_string();
        d.app_shows = app_shows.to_string();
        d.reported_at = now;
        return Ok(Reported::Updated(d.id.clone()));
    }
    let waiting = list
        .iter()
        .filter(|d| d.case_id == case_id && matches!(d.status, DivergenceStatus::Open | DivergenceStatus::AwaitingRepair))
        .count();
    if waiting >= MAX_OPEN_PER_CASE {
        return Err(format!("case {case_id} already has {MAX_OPEN_PER_CASE} open divergences - wait for the person's ruling"));
    }
    let id = next_id(list);
    list.push(Divergence {
        id: id.clone(),
        case_id,
        step,
        case_says: case_says.to_string(),
        app_shows: app_shows.to_string(),
        kind,
        reported_at: now,
        status: DivergenceStatus::Open,
        ruling: None,
        ruled_at: None,
    });
    Ok(Reported::New(id))
}

/// A person's ruling on an open divergence; answers its case id.
pub fn rule_in(list: &mut [Divergence], id: &str, ruling: Ruling, now_ms: u64) -> Result<i32, String> {
    let Some(d) = list.iter_mut().find(|d| d.id == id) else {
        return Err(format!("no divergence {id}"));
    };
    if d.status != DivergenceStatus::Open {
        return Err("that divergence was already ruled on".to_string());
    }
    if ruling == Ruling::DataFixed && d.kind != DivergenceKind::Data {
        return Err("Data fixed is for a data divergence".to_string());
    }
    d.status = if ruling == Ruling::Rewrite { DivergenceStatus::AwaitingRepair } else { DivergenceStatus::Ruled };
    d.ruling = Some(ruling);
    d.ruled_at = Some(now_ms.to_string());
    Ok(d.case_id)
}

/// What stops the case's runs: the first open `data` or `wording`
/// divergence by report time, else the first one awaiting its repair. An
/// `app` divergence never blocks.
pub fn blocking(list: &[Divergence], case_id: i32) -> Option<&Divergence> {
    let first = |pick: &dyn Fn(&Divergence) -> bool| {
        list.iter().filter(|d| d.case_id == case_id && pick(d)).min_by_key(|d| ms_of(&d.reported_at))
    };
    first(&|d| d.status == DivergenceStatus::Open && d.kind != DivergenceKind::App)
        .or_else(|| first(&|d| d.status == DivergenceStatus::AwaitingRepair))
}

/// The sentence a blocked run (and the assistant) reads.
pub fn run_sentence(d: &Divergence) -> String {
    match d.status {
        DivergenceStatus::AwaitingRepair => format!(
            "{WAITS_FOR_REPAIR_PREFIX}step {}: the person is rewriting the case - repair the script to the case as it now reads",
            d.step
        ),
        _ => format!(
            "{NEEDS_RULING_PREFIX}step {}: the case says \"{}\" but the app shows \"{}\" ({}) - a person rules on it in Auto Run",
            d.step,
            d.case_says,
            d.app_shows,
            d.kind.word()
        ),
    }
}

/// The sentence that blocks `case_id`, if one does. A file that cannot be
/// read blocks nothing (said in the log).
pub fn needs_ruling(root: &Path, org: &str, project: &str, case_id: i32) -> Option<String> {
    match load(root, org, project) {
        Ok(list) => blocking(&list, case_id).map(run_sentence),
        Err(_) => {
            crate::applog::warn("Auto Run: the divergences file could not be read; no case is blocked by it");
            None
        }
    }
}

/// A repair saved for these steps closes the case's awaiting-repair
/// divergences at them; answers those steps.
pub fn close_repaired(list: &mut [Divergence], case_id: i32, steps: &[i32], now_ms: u64) -> Vec<i32> {
    let mut closed = vec![];
    for d in list.iter_mut() {
        if d.case_id == case_id && d.status == DivergenceStatus::AwaitingRepair && steps.contains(&d.step) {
            d.status = DivergenceStatus::Ruled;
            d.ruled_at = Some(now_ms.to_string());
            closed.push(d.step);
        }
    }
    closed
}

/// The case's divergences still waiting on a person or a repair, oldest
/// first.
pub fn open_for(list: &[Divergence], case_id: i32) -> Vec<&Divergence> {
    let mut out: Vec<&Divergence> = list
        .iter()
        .filter(|d| d.case_id == case_id && matches!(d.status, DivergenceStatus::Open | DivergenceStatus::AwaitingRepair))
        .collect();
    out.sort_by_key(|d| ms_of(&d.reported_at));
    out
}

/// What `report_case_divergence` answers.
pub fn tool_answer(case_id: i32, d: &Divergence, updated: bool) -> String {
    let verb = if updated { "Updated" } else { "Reported" };
    match d.kind {
        DivergenceKind::App => format!(
            "{verb} as {}. Case {case_id} is not blocked: keep the check as the case says, so the run fails at step {}, because a real bug must surface as a failure. After that run, mark the step with mark_autorun_suspected_defect, and name this divergence in your report.",
            d.id, d.step
        ),
        _ => format!(
            "{verb} as {}. Case {case_id} needs a ruling: its runs are Blocked with \"{}\" until a person rules in Auto Run. Keep the check as the case says, save the script, and name this divergence in your report.",
            d.id,
            run_sentence(d)
        ),
    }
}
