//! The latest replay of each case that stopped on a failure, kept on this
//! machine so `get_autorun_failures` and `mark_autorun_suspected_defect`
//! see it as they see a run.
//!
//! A replay to a step is not a run: nothing about it is reviewed or sent.
//! But when it stops on a failure, that failure is the newest word on the
//! case, and an assistant healing the step reads and marks it straight
//! after. One file per case, `replays/<case_id>.json` under the Auto Run
//! folder, written by every replay that stopped at a step (the person's
//! or the assistant's) and removed by one of that case that reached its
//! step, whose failure it no longer is. Read back as a run of its own
//! (`as_run`) whenever it is newer than the case's newest run, or the case
//! has none (`latest_for_case`).

use super::failures;
use super::replay::{MODULE_STEP, SIGN_IN_STEP};
use super::replay_to::{ReplayEnd, ReplayPhase};
use super::{CaseRecord, LocalRun, StepRecord};
use crate::browser::actions::ActionOutcome;
use std::path::{Path, PathBuf};

/// `LocalRun::mode` of a replay read back as a run.
pub const REPLAY_MODE: &str = "replay";

/// A replay that stopped on a failure: where, why, and what ran.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReplayRecord {
    pub case_id: i32,
    pub title: String,
    /// The step the replay was asked to stop before.
    pub to_step: i32,
    /// When it ended, milliseconds since the epoch.
    pub at: u64,
    /// Where it stopped: "sign_in", "area" or "step".
    pub phase: String,
    /// The step it stopped at (1 for the sign-in and the area).
    pub step: i32,
    pub why: String,
    pub outcomes: Vec<ActionOutcome>,
    /// The account KEY the case ran as, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Where the kept replays are: `replays/` under the Auto Run folder.
pub fn dir(root: &Path) -> PathBuf {
    root.join("replays")
}

/// Let go of the replay kept for case `case_id`, if any: its script was
/// cleared. Best effort; a file that will not go is logged by case only.
pub fn forget(root: &Path, case_id: i32) {
    match std::fs::remove_file(file(root, case_id)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => crate::applog::warn(format!(
            "Auto Run: the replay kept for case {case_id} could not be removed: {}",
            e.kind()
        )),
    }
}

fn file(root: &Path, case_id: i32) -> PathBuf {
    dir(root).join(format!("{case_id}.json"))
}

/// What a replay of case `case_id` to `to_step` that ended with `end` keeps:
/// a stop on a failure (a closed browser after one included) is written; a
/// replay that reached its step removes the one kept before; any other end
/// (stopped, blocked, refused) leaves what is kept as it is. Never fails a
/// replay: a file that cannot be written is logged, by case only.
pub fn note(root: &Path, case_id: i32, title: &str, to_step: i32, end: &ReplayEnd, account: Option<String>, now_ms: u64) {
    let (phase, step, why, outcomes) = match end {
        ReplayEnd::StoppedAt { phase, step, why, outcomes } => (*phase, *step, why, outcomes),
        ReplayEnd::BrowserGone { step, why: Some(why), outcomes } => (ReplayPhase::Step, *step, why, outcomes),
        ReplayEnd::Ready { .. } => {
            forget(root, case_id);
            return;
        }
        _ => return,
    };
    let phase = match phase {
        ReplayPhase::SignIn => "sign_in",
        ReplayPhase::Area => "area",
        ReplayPhase::Step => "step",
    };
    let record = ReplayRecord {
        case_id,
        title: title.to_string(),
        to_step,
        at: now_ms,
        phase: phase.to_string(),
        step,
        why: why.clone(),
        outcomes: outcomes.clone(),
        account,
    };
    // Written beside its place and renamed into it, as the other stores
    // write: a reader never meets half a file.
    let path = file(root, case_id);
    let tmp = path.with_extension("json.tmp");
    let written = std::fs::create_dir_all(dir(root))
        .map_err(|e| e.to_string())
        .and_then(|()| serde_json::to_string_pretty(&record).map_err(|e| e.to_string()))
        .and_then(|json| std::fs::write(&tmp, json).map_err(|e| e.to_string()))
        .and_then(|()| std::fs::rename(&tmp, &path).map_err(|e| e.to_string()));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    if written.is_err() {
        crate::applog::warn(format!("Auto Run: the replay failure of case {case_id} could not be kept"));
    }
}

/// The failing replay kept for case `case_id`, if any. One that does not
/// read is none.
pub fn load(root: &Path, case_id: i32) -> Option<ReplayRecord> {
    let text = std::fs::read_to_string(file(root, case_id)).ok()?;
    serde_json::from_str(&text).ok()
}

/// A kept replay as a run of its own, with one failed case: the step it
/// stopped at (the sign-in and the area under their own step numbers, as a
/// run keeps them), its outcomes, and the failure as the case's reason.
/// Its id says what it was: "a replay to step N".
pub fn as_run(r: &ReplayRecord) -> LocalRun {
    let step_number = match r.phase.as_str() {
        "sign_in" => SIGN_IN_STEP,
        "area" => MODULE_STEP,
        _ => r.step,
    };
    // A step that failed before any action ran still says why.
    let outcomes = if r.outcomes.iter().any(|o| !o.ok) { r.outcomes.clone() } else { vec![ActionOutcome::failed(r.why.clone())] };
    let reason = match r.phase.as_str() {
        "sign_in" => format!("sign-in: {}", r.why),
        "area" => format!("module: {}", r.why),
        _ => format!("step {}: {}", r.step, r.why),
    };
    LocalRun {
        id: format!("a replay to step {}", r.to_step),
        pbi_id: 0,
        started_at: r.at.to_string(),
        cases: vec![CaseRecord {
            case_id: r.case_id,
            title: r.title.clone(),
            verdict: String::new(),
            note: String::new(),
            steps: vec![StepRecord {
                step_number,
                outcomes,
                screenshot: None,
                downloads: Vec::new(),
                tab: None,
                dialog: None,
                components: Vec::new(),
                duration_ms: None,
            }],
            proposed: "Failed".to_string(),
            reason,
            duration_ms: None,
            account: r.account.clone(),
            retried: None,
            notice: None,
            page_errors_seen: 0,
            phases: None,
        }],
        mode: REPLAY_MODE.to_string(),
        published: None,
        environment: None,
        resets: Vec::new(),
    }
}

/// The newest word on any case: the newest kept failing replay of any case,
/// read as a run (`as_run`), when it is newer than the newest run on this
/// machine or there is no run; that run otherwise.
pub fn latest(root: &Path) -> Option<LocalRun> {
    let newest_replay = std::fs::read_dir(dir(root))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id: i32 = name.strip_suffix(".json")?.parse().ok()?;
            load(root, id)
        })
        .max_by_key(|r| r.at);
    let run = failures::latest_run(root, None);
    match (run, newest_replay) {
        (None, Some(r)) => Some(as_run(&r)),
        (Some(run), Some(r)) if r.at > run.started_at.parse::<u64>().unwrap_or(0) => Some(as_run(&r)),
        (run, _) => run,
    }
}

/// The newest word on case `case_id`: its kept failing replay, read as a
/// run (`as_run`), when no run on this machine has the case or the replay
/// is newer than the newest one that does; that run otherwise.
pub fn latest_for_case(root: &Path, case_id: i32) -> Option<LocalRun> {
    let run = failures::latest_run(root, Some(case_id));
    match (run, load(root, case_id)) {
        (None, Some(r)) => Some(as_run(&r)),
        (Some(run), Some(r)) if r.at > run.started_at.parse::<u64>().unwrap_or(0) => Some(as_run(&r)),
        (run, _) => run,
    }
}
