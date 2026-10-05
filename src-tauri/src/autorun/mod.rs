//! The supervised runner's own data: the action script for a case, and
//! the record of a run.
//!
//! Both live on THIS machine only. Driving the browser and recording what
//! happened (`runner`, `replay`, the `commands/autorun*.rs` IPC surface)
//! never reach Azure DevOps. The one door out is `publish`, used only when
//! a person has reviewed a run and presses Send.

pub mod accounts;
pub mod api_checks;
pub mod defects;
pub mod edits;
pub mod failures;
pub mod floor;
pub mod guide;
pub mod nav;
pub mod patterns;
pub mod publish;
pub mod quirks;
pub mod recipe;
pub mod recorder;
pub mod replay;
pub mod report;
pub mod runner;
pub mod sessions;
pub mod signin;
pub mod signin_recorder;
pub mod store;
pub mod transient;

use crate::browser::actions::{Action, ActionOutcome};

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// The actions that carry out one numbered step of a test case.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StepScript {
    pub step_number: i32,
    pub actions: Vec<Action>,
    /// Why this step's expected result is not checked by the script, when
    /// it is not. The floor accepts a step with an expected result and no
    /// check only when this says why, and the app shows the sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unchecked: Option<String>,
}

/// How one test case is driven. Keyed by the Azure DevOps case id so a
/// script and its case stay together, but the script never leaves here.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CaseScript {
    pub case_id: i32,
    pub title: String,
    /// The account this case runs as: a key from the tester's own accounts
    /// list, never a login. Absent means the script signs nobody in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The recorded area the run takes this case to before step 1, by name
    /// (`nav::find_area`). Absent or blank means the module's default area
    /// (`nav::find_path`): the one named like the case's Module, or its
    /// only area when none is. A name the project has not recorded is refused when
    /// the script is saved, and refuses the case at run time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    pub steps: Vec<StepScript>,
    /// How many times an assistant has repaired this script since a person
    /// last saved it from the editor. Absent when 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub repairs: u32,
    /// The `why` of the most recent repair, so a person opening the editor
    /// can see what an assistant changed without having to find the applog
    /// line. Set alongside `repairs` on a repair, cleared to `None` by the
    /// editor's own save - together with `repairs`, so a person saving
    /// from the app always starts from a clean slate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_repair: Option<String>,
    /// An assistant's finding that the script is right and the application
    /// did not do what the case expects, at one step. Set only through
    /// `store::set_suspected_defect` (the assistant's
    /// `mark_autorun_suspected_defect`); every save keeps the one already
    /// on disk and ignores whatever it was sent. Never changes `steps`,
    /// `repairs` or `last_repair`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspected_defect: Option<SuspectedDefect>,
}

/// One case's suspected application defect: the step, and what the
/// application did against what the case expects. One per case; a new
/// mark replaces the old one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SuspectedDefect {
    pub step_number: i32,
    /// At most 300 characters, stored after the address and token scrub
    /// API-check excerpts get (`defects::check_mark`).
    pub note: String,
    /// Epoch milliseconds as a string, like `LocalRun::started_at`.
    pub marked_at: String,
}

impl CaseScript {
    /// The area this script names, trimmed; `None` when it names none or a
    /// blank one - both mean the module's default area (`nav::find_path`).
    pub fn area_name(&self) -> Option<&str> {
        self.area.as_deref().map(str::trim).filter(|a| !a.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StepRecord {
    pub step_number: i32,
    pub outcomes: Vec<ActionOutcome>,
    /// A picture of the page when the step ended (unattended runs only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
}

/// One case in a run. `verdict` is the HUMAN's word - "", "Passed",
/// "Failed", "Blocked". The machine never fills it in: the action
/// outcomes are evidence shown to the person, not a vote. `proposed` is
/// what the machine WOULD say, for an unattended run - a suggestion the
/// review screen shows, never a substitute for `verdict`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CaseRecord {
    pub case_id: i32,
    pub title: String,
    pub verdict: String,
    pub note: String,
    pub steps: Vec<StepRecord>,
    /// What the machine would say: "", "Passed", "Failed" or "Blocked".
    /// A proposal, never a verdict.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proposed: String,
    /// One sentence on why it proposes that.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i32>,
    /// The account the case ran as (a key, never a login).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Set when an unattended case failed in a way that looked transient
    /// and was run once more (`transient`): the first try's failure
    /// sentence. The steps above are the final try's only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retried: Option<String>,
}

/// Recorded once a run has been sent to Azure DevOps, so a stale review
/// screen can never erase the fact that it happened.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PublishedRun {
    pub run_id: i32,
    pub web_url: String,
    /// Epoch milliseconds as a string, like `started_at`.
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LocalRun {
    pub id: String,
    pub pbi_id: i32,
    /// Epoch milliseconds as a string - specta forbids u64 across IPC,
    /// and the frontend formats it anyway.
    pub started_at: String,
    pub cases: Vec<CaseRecord>,
    /// "" for a supervised run (as always), "unattended" for a replay.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    /// Set once the run has been sent to Azure DevOps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<PublishedRun>,
    /// The name of the environment the run was made in, as it was then.
    /// `None` for a run saved before environments existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
}
