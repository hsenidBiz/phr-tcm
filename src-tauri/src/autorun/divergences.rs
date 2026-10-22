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

use super::case_text::{fold, CaseText};
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

/// Who made a ruling. A ruling saved before the assistant could make one
/// has none, and reads as the person's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuledBy {
    Person,
    Assistant,
}

/// The ruling the assistant names in `rule_case_divergence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRuling {
    CaseRight,
    DataFixed,
    RewriteCase,
}

impl AssistantRuling {
    pub fn ruling(self) -> Ruling {
        match self {
            AssistantRuling::CaseRight => Ruling::CaseStands,
            AssistantRuling::DataFixed => Ruling::DataFixed,
            AssistantRuling::RewriteCase => Ruling::Rewrite,
        }
    }
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
    /// While the case is rewritten (`AwaitingRepair`): the fingerprint of
    /// the case's steps text when the ruling was made, or at the first read
    /// after it for a ruling an earlier build saved. A different one later
    /// means the case was rewritten (`note_case_text`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_fp: Option<String>,
    /// The ruled step's action text, folded, as it read when `case_fp` was
    /// taken: a rewrite that renumbers the step is still found by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_action: Option<String>,
    /// Who ruled; `None` reads as the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ruled_by: Option<RuledBy>,
    /// For an assistant's ruling: the person's own words from the chat that
    /// gave it, scrubbed (`defects::scrub_note`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said: Option<String>,
    /// For an assistant's ruling: the assistant's own note, scrubbed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Divergence {
    fn by_assistant(&self) -> bool {
        self.ruled_by == Some(RuledBy::Assistant)
    }

    /// Can a person take this ruling back? One that waits for a repair, or
    /// one the assistant made that a repair has not closed since: a closed
    /// rewrite (ruled, with the ruling "rewrite") is done, and reopening it
    /// would block the case on a step the rewrite may have renumbered.
    pub fn undoable(&self) -> bool {
        match self.status {
            DivergenceStatus::AwaitingRepair => true,
            DivergenceStatus::Ruled => self.by_assistant() && self.ruling != Some(Ruling::Rewrite),
            DivergenceStatus::Open => false,
        }
    }
}

pub const MAX_TEXT: usize = 300;
pub const SAID_EMPTY: &str = "\"said\" quotes the person's own words from this chat that gave this ruling: a ruling is applied only on what the person said, never with an empty \"said\"";
pub const SAID_TOO_LONG: &str = "\"said\" is one line of at most 300 characters: quote the words that gave the ruling";
pub const NOTE_TOO_LONG: &str = "\"note\" is one line of at most 300 characters";
pub const NO_PICK: &str = "name the divergence to rule: case_id and divergence_id, or divergences";
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
        if d.by_assistant() {
            let said = d.said.as_deref().unwrap_or("");
            return Err(format!(
                "the assistant ruled on {day}, on the person's words \"{said}\", that the case stands at step {step} - keep the check as the case says; a failure there is the data's or the application's"
            ));
        }
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
        case_fp: None,
        step_action: None,
        ruled_by: None,
        said: None,
        note: None,
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

/// The person's words behind an assistant's ruling: trimmed, one line of at
/// most `MAX_TEXT` characters, never empty, and answered scrubbed like a
/// suspected defect's note (an address or token pasted from a page does not
/// stay).
pub fn check_said(said: &str) -> Result<String, String> {
    let said = said.trim();
    if said.is_empty() {
        return Err(SAID_EMPTY.to_string());
    }
    if said.chars().count() > MAX_TEXT || said.contains('\n') || said.contains('\r') {
        return Err(SAID_TOO_LONG.to_string());
    }
    Ok(super::defects::scrub_note(said))
}

/// The assistant's own note: empty is none, one line of at most
/// `MAX_TEXT` characters, scrubbed.
pub fn check_note(note: Option<&str>) -> Result<Option<String>, String> {
    let Some(note) = note.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    if note.chars().count() > MAX_TEXT || note.contains('\n') || note.contains('\r') {
        return Err(NOTE_TOO_LONG.to_string());
    }
    Ok(Some(super::defects::scrub_note(note)))
}

/// The assistant rules, on the person's words, on the divergences it names
/// (`(case_id, divergence_id)` picks). All or none: every pick is checked
/// first, by [`rule_in`]'s own rules and by naming its own case, and any
/// refusal rules nothing and answers each refused pick as `"{id}: {why}"`,
/// one per line. Then each is ruled by [`rule_in`] itself, so the effect is
/// exactly the person's, with who ruled and the words set. A pick named
/// twice counts once; the same divergence named under two cases is refused
/// at the case that is not its own. Answers the cases ruled, in pick order, each once.
pub fn rule_by_assistant_in(
    list: &mut [Divergence],
    picks: &[(i32, String)],
    ruling: Ruling,
    said: &str,
    note: Option<&str>,
    now_ms: u64,
) -> Result<Vec<i32>, String> {
    if picks.is_empty() {
        return Err(NO_PICK.to_string());
    }
    let said = check_said(said)?;
    let note = check_note(note)?;
    // The same divergence named under two cases is two picks: the case of
    // each is checked, so a mistaken pick is refused rather than hidden.
    let mut distinct: Vec<&(i32, String)> = vec![];
    for pick in picks {
        if !distinct.contains(&pick) {
            distinct.push(pick);
        }
    }
    // A trial on a copy: the same rules, so nothing is ruled unless every pick can be.
    let mut trial = list.to_vec();
    let mut refused: Vec<String> = vec![];
    for (case_id, id) in distinct.iter().map(|p| (p.0, p.1.as_str())) {
        let why = match trial.iter().find(|d| d.id == id) {
            Some(d) if d.case_id != case_id => {
                Some(format!("{id} is case {}'s divergence, not case {case_id}'s", d.case_id))
            }
            _ => rule_in(&mut trial, id, ruling, now_ms).err(),
        };
        if let Some(why) = why {
            refused.push(format!("{id}: {why}"));
        }
    }
    if !refused.is_empty() {
        return Err(refused.join("\n"));
    }
    let mut cases: Vec<i32> = vec![];
    for (_, id) in distinct.iter().map(|p| (p.0, p.1.as_str())) {
        let case = rule_in(list, id, ruling, now_ms)?;
        if let Some(d) = list.iter_mut().find(|d| d.id == id) {
            d.ruled_by = Some(RuledBy::Assistant);
            d.said = Some(said.clone());
            d.note = note.clone();
        }
        if !cases.contains(&case) {
            cases.push(case);
        }
    }
    Ok(cases)
}

/// What `rule_case_divergence` answers: one line per pick, then where the
/// person can take the ruling back.
pub fn ruling_answer(list: &[Divergence], picks: &[(i32, String)], ruling: Ruling) -> String {
    let mut seen: Vec<&str> = vec![];
    let mut lines: Vec<String> = vec![];
    for (_, id) in picks {
        if seen.contains(&id.as_str()) {
            continue;
        }
        seen.push(id);
        let Some(d) = list.iter().find(|d| &d.id == id) else { continue };
        let (c, s) = (d.case_id, d.step);
        lines.push(match ruling {
            Ruling::CaseStands => format!(
                "Ruled {id} (case {c} step {s}): the case is right, on the person's words. Its runs are no longer Blocked by it, and the script's check stays as the case says."
            ),
            Ruling::DataFixed => format!(
                "Ruled {id} (case {c} step {s}): the data was fixed, on the person's words. Replay the whole case with replay_autorun_to_step: the script counts as checked only after a replay passes."
            ),
            Ruling::Rewrite => format!(
                "Ruled {id} (case {c} step {s}): the case is being rewritten, on the person's words. It stays Blocked until the rewrite is in Azure DevOps and the script is repaired to the case as it then reads."
            ),
        });
    }
    lines.push("The person can undo each of these on the case card in Auto Run.".to_string());
    lines.join("\n")
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
    close_repaired_with(list, case_id, steps, now_ms, &|_| None)
}

/// [`close_repaired`], also closing the divergence of a step the rewrite
/// renumbered: a declared step whose action text (`action_of`, folded, from
/// the case as it now reads) is the ruled step's action text.
pub fn close_repaired_with(
    list: &mut [Divergence],
    case_id: i32,
    steps: &[i32],
    now_ms: u64,
    action_of: &dyn Fn(i32) -> Option<String>,
) -> Vec<i32> {
    let mut closed = vec![];
    for d in list.iter_mut() {
        if d.case_id != case_id || d.status != DivergenceStatus::AwaitingRepair {
            continue;
        }
        let renumbered = d.step_action.as_ref().is_some_and(|a| steps.iter().any(|s| action_of(*s).as_ref() == Some(a)));
        if steps.contains(&d.step) || renumbered {
            close(d, now_ms);
            closed.push(d.step);
        }
    }
    closed
}

fn close(d: &mut Divergence, now_ms: u64) {
    d.status = DivergenceStatus::Ruled;
    d.ruled_at = Some(now_ms.to_string());
}

/// A person's save of the case's script in the editor is the acceptance of
/// whatever waited for a repair: closes the case's awaiting-repair
/// divergences; answers their steps.
pub fn close_for_editor_save(list: &mut [Divergence], case_id: i32, now_ms: u64) -> Vec<i32> {
    let mut closed = vec![];
    for d in list.iter_mut().filter(|d| d.case_id == case_id && d.status == DivergenceStatus::AwaitingRepair) {
        close(d, now_ms);
        closed.push(d.step);
    }
    closed
}

/// A person takes back a "Rewrite the case" ruling that still waits for its
/// repair, or a ruling the assistant made that no repair has closed since
/// (`Divergence::undoable`): the divergence is open again and needs a
/// ruling; answers its case id. A person's own "the case stands" or "data
/// fixed" ruling is not undone.
pub fn undo_in(list: &mut [Divergence], id: &str) -> Result<i32, String> {
    let Some(d) = list.iter_mut().find(|d| d.id == id) else {
        return Err(format!("no divergence {id}"));
    };
    if !d.undoable() {
        return Err("only a ruling that waits for a repair, or one the assistant made that no repair has closed, can be undone".to_string());
    }
    d.status = DivergenceStatus::Open;
    d.ruling = None;
    d.ruled_at = None;
    d.case_fp = None;
    d.step_action = None;
    d.ruled_by = None;
    d.said = None;
    d.note = None;
    Ok(d.case_id)
}

/// The case as Azure DevOps reads it now, against its awaiting-repair
/// divergences. One with no print yet gets this one (a ruling an earlier
/// build saved, or one made while Azure DevOps could not be read); one whose
/// print differs means the case was rewritten, and it is closed. Answers the
/// steps closed.
pub fn note_case_text(list: &mut [Divergence], case_id: i32, text: &CaseText, now_ms: u64) -> Vec<i32> {
    let fp = text.steps_fingerprint();
    let mut closed = vec![];
    for d in list.iter_mut().filter(|d| d.case_id == case_id && d.status == DivergenceStatus::AwaitingRepair) {
        match d.case_fp.as_deref() {
            None => {
                d.case_fp = Some(fp.clone());
                d.step_action = text.step_action_folded(d.step);
            }
            Some(was) if was != fp => {
                close(d, now_ms);
                closed.push(d.step);
            }
            Some(_) => {}
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

/// The failures answer's "Divergences" lines for these failed or blocked
/// cases (given in the order the run holds them), one per entry:
/// - open: waiting for a person's ruling;
/// - awaiting its repair: the person is rewriting the case;
/// - ruled "the case stands" in the last 90 days: leave the check alone.
///
/// Empty when none of the cases has such an entry.
pub fn failures_lines(list: &[Divergence], case_ids: &[i32], now_ms: u64) -> Vec<String> {
    let mut out = vec![];
    let mut seen: Vec<i32> = vec![];
    for &case_id in case_ids {
        if seen.contains(&case_id) {
            continue;
        }
        seen.push(case_id);
        let mut mine: Vec<&Divergence> = list.iter().filter(|d| d.case_id == case_id).collect();
        mine.sort_by_key(|d| (d.step, ms_of(&d.reported_at)));
        for d in mine {
            match (d.status, d.ruling) {
                (DivergenceStatus::Open, _) => out.push(format!(
                    "- case {case_id} step {} ({}, {}): needs a ruling - the case says \"{}\", the app shows \"{}\". Leave the script as the case says; the person rules in Auto Run.",
                    d.step,
                    d.kind.word(),
                    d.id,
                    d.case_says,
                    d.app_shows
                )),
                (DivergenceStatus::AwaitingRepair, _) => out.push(format!(
                    "- case {case_id} step {} (rewrite, {}): the person is rewriting the case - repair that step's checks to the case as it now reads, declared in edits; the repair closes it.",
                    d.step, d.id
                )),
                (DivergenceStatus::Ruled, Some(Ruling::CaseStands))
                    if now_ms.saturating_sub(d.ruled_at.as_deref().map(ms_of).unwrap_or(0)) <= KEEP_RULED_MS =>
                {
                    let day = day_of(d.ruled_at.as_deref().unwrap_or("0"));
                    if d.by_assistant() {
                        out.push(format!(
                            "- case {case_id} step {} ({}): the assistant ruled on {day}, on the person's words \"{}\", that the case stands - leave the check as it is; a failure there is the data's or the application's (mark_autorun_suspected_defect, or tell the person).",
                            d.step,
                            d.id,
                            d.said.as_deref().unwrap_or("")
                        ));
                    } else {
                        out.push(format!(
                            "- case {case_id} step {} ({}): a person ruled on {day} that the case stands - leave the check as it is; a failure there is the data's or the application's (mark_autorun_suspected_defect, or tell the person).",
                            d.step, d.id
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The full "## Divergences" section, or none when there is nothing to say.
pub fn failures_section(list: &[Divergence], case_ids: &[i32], now_ms: u64) -> Option<String> {
    let lines = failures_lines(list, case_ids, now_ms);
    if lines.is_empty() {
        None
    } else {
        Some(format!("## Divergences\n\n{}", lines.join("\n")))
    }
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
