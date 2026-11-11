//! What assistants SAVED, PROPOSED, TRIED or CHANGED, one line per write call
//! the bridge answered: the data behind Auto Run's assistant timeline.
//!
//! A record holds the time, the assistant's label, the group, a fixed
//! sentence from the table below, whether the call was accepted, the case id
//! (from its well-known body field only) and the environment and
//! organisation it ran in. Never a body field, a query, a URL, a path, a
//! login, a statement or a value: the sentence is chosen here, not echoed.
//! Records go to `activity_log`'s `assistant-<date>.jsonl` files.
//!
//! The table is curated and lists writes and tries only. Left out on purpose:
//! every GET and read-only POST (captures, page reads, db-lookup/db-query,
//! optimize/transform/validate/merge/check-coverage/begin, list routes);
//! every `/crystal-*` route (never recorded); session plumbing
//! (`/agent-bye`, `/autorun-release`, `/autorun-reset`, `/autorun-replay-stop`,
//! `/autorun-discover-action(s)` - steps inside a discovery that
//! `/autorun-discover-start` already names); browser overrides
//! (`/autorun-headers`, `/autorun-cookie(s)`, `/autorun-cookies-clear`,
//! `/autorun-capture`); `/autorun-probe` and `/autorun-inspect` (they only
//! look); `/api-template-flow-progress` (a progress read);
//! `/api-template-send` (a raw request, whose target must not be named).
//! There is no API template save route on the bridge, so no row for one.

use crate::autorun::agents::Caller;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ActivityGroup {
    Saved,
    Proposed,
    Tried,
    ChangedData,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct AssistantRecord {
    pub at: String,
    pub label: String,
    pub group: ActivityGroup,
    pub action: String,
    pub ok: bool,
    #[serde(default)]
    pub case_id: Option<i32>,
    pub env: String,
    pub org: String,
}

/// Said for a call that names no assistant.
pub const DEFAULT_LABEL: &str = "Assistant";

/// The most records the timeline answers with.
pub const TIMELINE_CAP: usize = 500;

fn removes(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("remove").and_then(|r| r.as_bool()))
        .unwrap_or(false)
}

/// The group and sentence for a bridge call, or None for a call the timeline
/// does not show. `path` carries no query.
pub fn route_action(method: &str, path: &str, body: &str) -> Option<(ActivityGroup, &'static str)> {
    use ActivityGroup::*;
    if method != "POST" {
        return None;
    }
    let toggled = |saved: &'static str, removed: &'static str| if removes(body) { removed } else { saved };
    Some(match path {
        "/autorun-script" => (Saved, "Saved a script"),
        "/autorun-script-delete" => (Saved, "Removed a script"),
        "/autorun-component-save" => (Saved, toggled("Saved a component", "Removed a component")),
        "/autorun-component-retire" => (Saved, "Removed a component"),
        "/autorun-discover-area" => (Saved, "Saved an area"),
        "/autorun-setup-save" => (Saved, toggled("Saved a setup", "Removed a setup")),
        "/autorun-quirk" => (Saved, "Saved a quirk"),
        "/autorun-quirk-retire" => (Saved, "Removed a quirk"),
        "/autorun-validation-rule" => (Saved, toggled("Saved a validation rule", "Removed a validation rule")),
        "/test-file-save" => (Saved, "Saved a test file"),
        "/api-template-fixture-save" => (Saved, "Saved an API fixture"),
        "/api-template-flow-save" => (Saved, "Saved an API flow"),
        "/autorun-order" => (Saved, "Saved the run order"),
        "/accounts-propose" => (Proposed, "Proposed accounts"),
        "/autorun-propose-sign-in" => (Proposed, "Proposed a sign-in"),
        "/autorun-divergence" => (Proposed, "Reported a divergence"),
        "/autorun-divergence-ruling" => (Proposed, "Ruled on a divergence"),
        "/autorun-defect" => (Proposed, "Marked a suspected defect"),
        "/autorun-defect-clear" => (Proposed, "Cleared a suspected defect"),
        "/autorun-try" => (Tried, "Tried an action"),
        "/autorun-replay" => (Tried, "Replayed to a step"),
        "/autorun-setup-run" => (Tried, "Ran a setup"),
        "/autorun-check-sign-in" => (Tried, "Checked sign-in"),
        "/autorun-inspect-sign-in" => (Tried, "Inspected a sign-in"),
        "/autorun-discover-start" => (Tried, "Started discovery"),
        "/autorun-discover-end" => (Tried, "Ended discovery"),
        "/api-template-prove" => (Tried, "Proved an API template"),
        "/api-template-run" => (Tried, "Ran an API template"),
        "/api-template-fixture-run" => (Tried, "Ran an API fixture"),
        "/autorun-explore-change" => (ChangedData, "Recorded an explore change"),
        "/api-template-explore-undo" => (ChangedData, "Undid explore creates"),
        _ => return None,
    })
}

/// The label the person gave the assistant's connection, else "Assistant".
pub fn caller_label(caller: &Caller) -> String {
    caller
        .key()
        .and_then(crate::autorun::agents::info)
        .map(|i| i.label)
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_LABEL.to_string())
}

/// Records one answered bridge call, when the table names it.
pub fn record_call(caller: &Caller, org: &str, env: &str, method: &str, target: &str, body: &str, status: u16) {
    let path = target.split_once('?').map(|(p, _)| p).unwrap_or(target);
    let Some((group, action)) = route_action(method, path, body) else { return };
    let mut entry = serde_json::json!({
        "label": caller_label(caller),
        "group": group,
        "action": action,
        "ok": status < 400,
        "env": env,
        "org": org,
    });
    if let Some(case) = crate::ai_bridge::named_case_id(body) {
        entry["case_id"] = serde_json::json!(case);
    }
    crate::activity_log::record(crate::activity_log::Kind::Assistant, entry);
}

/// This organisation's and environment's records from the last `days` days,
/// newest first, at most `TIMELINE_CAP`.
pub fn timeline_in(dir: &Path, org: &str, env: &str, days: u32, now: SystemTime) -> Vec<AssistantRecord> {
    let since = crate::applog::stamp_of(now - Duration::from_secs(u64::from(days) * 86_400));
    let mut out: Vec<AssistantRecord> = Vec::new();
    for back in 0..=u64::from(days) {
        let date = crate::applog::stamp_of(now - Duration::from_secs(back * 86_400))[..10].to_string();
        let path = dir.join(crate::activity_log::file_name(crate::activity_log::Kind::Assistant, &date));
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        for line in text.lines() {
            let Ok(rec) = serde_json::from_str::<AssistantRecord>(line) else { continue };
            if rec.at >= since && rec.org == org && rec.env == env {
                out.push(rec);
            }
        }
    }
    // Files are read newest day first but each is oldest first: reverse, then
    // a stable sort by time keeps later lines of one second ahead of earlier.
    out.reverse();
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out.truncate(TIMELINE_CAP);
    out
}
