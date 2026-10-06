//! The supervised runner's own data: the action script for a case, and
//! the record of a run.
//!
//! Both live on THIS machine only. Driving the browser and recording what
//! happened (`runner`, `replay`, the `commands/autorun*.rs` IPC surface)
//! never reach Azure DevOps. The one door out is `publish`, used only when
//! a person has reviewed a run and presses Send.

pub mod accounts;
pub mod api_checks;
pub mod approvals;
pub mod cleanup;
pub mod defects;
pub mod downloads;
pub mod edits;
pub mod failures;
pub mod floor;
pub mod guide;
pub mod lease;
pub mod marks;
pub mod nav;
pub mod page_errors;
pub mod patterns;
pub mod plan;
pub mod preconditions;
pub mod publish;
pub mod quirks;
pub mod recipe;
pub mod recorder;
pub mod replay;
pub mod replay_ask;
pub mod reset_wait;
pub mod replay_to;
pub mod report;
pub mod runner;
pub mod sessions;
pub mod setup;
pub mod signin;
pub mod signin_recorder;
pub mod store;
pub mod test_made;
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
    /// The case works on a shared draft and must never change it: while it
    /// runs, its browser stops every save the page tries to send
    /// (`browser::save_guard`) and the case fails. Set in the editor (Must
    /// not save), by an assistant's save or through import; only a person
    /// saving from the editor can turn it off (`edits::check_edits`).
    /// Written only when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_save: bool,
    /// Records the case relies on, built beforehand: each one a stage of an
    /// API template flow that must be done for a value before step 1. The
    /// run checks every one before it signs in (`preconditions`) and
    /// blocks the case when one is not met. Every save validates them.
    /// Written only when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preconditions: Vec<Precondition>,
    /// The draft this case makes for itself before it signs in: a fixture
    /// the run performs on every run of the case, once a person has
    /// approved it in the script editor (`approvals`). Its outputs reach
    /// the steps as `{{setup.<output>}}` (`setup`). A repair can never
    /// add, change or remove it. Written only when there is one, so a
    /// script without one reads exactly as it always did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<Setup>,
    /// Shared state this case leaves changed for the cases after it, by
    /// name (`"cycle published"`). Names compare by `marks::normalise`, and
    /// every save validates them (`marks::check_marks`). Written only when
    /// there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<String>,
    /// Shared state this case needs not yet changed, or reverted, by the
    /// same names as `changes`. Written only when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs_unchanged: Vec<String>,
    /// When the script was last saved, UTC "YYYY-MM-DDTHH:MM:SSZ" - set by
    /// every save (`store::save_scripts_atomically`), whatever was sent. A
    /// repair reads the test case as of this moment to see which steps the
    /// case itself has changed since (`edits::check_edits_following_case`).
    /// Absent on scripts saved before it existed; the file's own modified
    /// time stands in for it then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<String>,
    /// A browser dialog no `expect_dialog` claimed fails the step it
    /// appeared in (`an unexpected <kind> dialog appeared: ...`). Off, it
    /// is accepted and said on the step, as always. Written only when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fail_on_unexpected_dialog: bool,
    /// Whether the page's own errors - uncaught script errors and 5xx
    /// answers - fail the step they appear in (`fail`) or are counted on
    /// the case (`flag`). Absent: they are not looked at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_errors: Option<PageErrors>,
    /// Phrases whose page errors are not counted: found, ignoring case, in
    /// a script error's message or a request's path. At most 10. Written
    /// only when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_page_errors: Vec<String>,
}

/// What a script does with the page's own errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum PageErrors {
    /// The step fails.
    Fail,
    /// The step is judged as usual; the case counts them.
    Flag,
}

/// One record a case relies on: `stage` of `flow` must be done for `value`,
/// the flow's subject as its checks take it (a cycle's id, or its name).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Precondition {
    pub flow: String,
    pub stage: String,
    /// Absent reads as null, so a save can say the value is missing rather
    /// than fail to parse. See `FlowSaved::sample` on why this is declared
    /// to TypeScript as `unknown`.
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub value: serde_json::Value,
    /// One sentence on why the case needs it, said after the Blocked
    /// sentence when the precondition is not met.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// A script's setup: the saved fixture whose run makes this case's own
/// draft, by id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub fixture: String,
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
    /// The files the browser saved during the step, by the names they are
    /// kept under in the run's download folder (`store::downloads_dir`):
    /// names only, never a path (unattended runs only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub downloads: Vec<String>,
    /// The tab the step ran in, when that was not `main` (see
    /// `runner::InRun::tab`). Left out for a step in `main`, so a run file
    /// from before tabs reads the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    /// The browser dialog the step met, when it met one: the first it
    /// claimed with an `expect_dialog`, else the first nobody expected.
    /// Left out when there was none, so older run files read the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialog: Option<StepDialog>,
}

/// A browser dialog a step met: its kind (`alert`, `confirm`, `prompt`,
/// `beforeunload`) and its message, cut to 200 characters. Page text,
/// never a secret.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StepDialog {
    pub kind: String,
    pub message: String,
}

/// One file in a run's download folder, as Past runs and the review list
/// it: its name and size, read from disk when asked (`store::download_files`),
/// never kept in the run file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DownloadFile {
    pub name: String,
    /// Bytes. A `u32` (specta refuses a 64-bit number across IPC); a file
    /// over 4 GB reads as `u32::MAX`.
    pub size: u32,
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
    /// Something the run did not do for this case and the case went on
    /// without, said so a person reviewing it knows: today only that its
    /// preconditions were not checked while Database Read Access was off
    /// (`preconditions::NOT_CHECKED`). Never a reason to block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// How many page errors a script with `"page_errors": "flag"` met in
    /// this case (`page_errors`). Written only when there were any.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub page_errors_seen: u32,
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
    /// Each reset point the run paused at, in run order, with how it ended
    /// (`reset_wait`). Written only when there are any, so a run without
    /// reset points reads exactly as it always did.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resets: Vec<ResetRecord>,
}

/// One reset point a run paused at: before `before_case_id` ran, a person
/// was asked to revert `names` (`changed_by` gives, per name, the cases
/// that changed it). `waited_ms` is the wall time spent paused; `outcome`
/// is "continued" or "stopped".
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ResetRecord {
    pub before_case_id: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_by: Vec<(String, Vec<i32>)>,
    /// Milliseconds. A `u32` (specta refuses a 64-bit number across IPC);
    /// a pause past 49 days reads as `u32::MAX`.
    #[serde(default)]
    pub waited_ms: u32,
    /// "continued" or "stopped".
    #[serde(default)]
    pub outcome: String,
}

/// `ResetRecord::outcome` when the person pressed Continue.
pub const RESET_CONTINUED: &str = "continued";
/// `ResetRecord::outcome` when the run ended at the reset point.
pub const RESET_STOPPED: &str = "stopped";
