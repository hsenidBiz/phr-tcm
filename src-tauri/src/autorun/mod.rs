//! The supervised runner's own data: the action script for a case, and
//! the record of a run.
//!
//! Both live on THIS machine only. Driving the browser and recording what
//! happened (`runner`, `replay`, the `commands/autorun*.rs` IPC surface)
//! never reach Azure DevOps. The one door out is `publish`, used only when
//! a person has reviewed a run and presses Send.

pub mod accounts;
pub mod guide;
pub mod publish;
pub mod recipe;
pub mod replay;
pub mod runner;
pub mod sessions;
pub mod signin;
pub mod store;

use crate::browser::actions::{Action, ActionOutcome};

/// The actions that carry out one numbered step of a test case.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StepScript {
    pub step_number: i32,
    pub actions: Vec<Action>,
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
    pub steps: Vec<StepScript>,
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
}
