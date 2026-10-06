//! Typed tauri-specta events the backend streams to the frontend.
//! Every event here must stay listed in `specta_builder()`'s
//! `collect_events![]` — an unregistered event panics on emit.

/// Emitted once per queue item while submit_queue runs.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct SubmitProgress {
    pub index: u32,
    pub total: u32,
    pub title: String,
    /// "created" | "updated" | "failed"
    pub action: String,
}

/// Emitted by submit_queue when the PBI had NO area-matched test plan and
/// one was created on the fly, before the upload proceeds - the frontend
/// tells the user so plans never appear out of nowhere.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct PlanCreated {
    pub plan_name: String,
}

/// Emitted by submit_queue when the PBI had no requirement suite and one
/// could not be created - typically no permission on the plan(s) for its
/// area. The cases still upload and link to the PBI; the frontend says
/// so, loudly, because a warning in the log was how 197 cases once landed
/// with no suite and nobody knew.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct SuiteNotCreated {
    pub reason: String,
}

/// Emitted after an upload when the suite's spec order or the suggested
/// run order could not be saved. The upload itself still succeeded; the
/// reason says which order is missing and that Suite Management can set it.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct RunOrderNotSaved {
    pub reason: String,
}

/// Emitted while test plans are being scanned for suites, so Run Tests and
/// the Suites browser can show "Scanning plans X of Y" instead of a bare
/// skeleton.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct SuiteScanProgress {
    pub done: u32,
    pub total: u32,
}

/// Emitted when work items have been newly assigned to the signed-in
/// user. The frontend decides how to surface them: a toast when the app
/// has focus, an OS notification when it doesn't.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct WorkAssigned {
    pub items: Vec<crate::assigned_watch::AssignedItem>,
}

/// Emitted when the JSON file an import is following changes on disk -
/// once per real content change, never on a save that rewrote the same
/// bytes. `stamp` is the new fingerprint (see filewatch.rs).
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct WatchedFileChanged {
    pub path: String,
    pub stamp: String,
}

/// Emitted while an update package downloads, so the banner can show a bar
/// instead of a spinner that says nothing about how long is left.
///
/// `total` is exact - the size the release feed gives. `downloaded` is that
/// share of it implied by `percent`, which Velopack floors to the nearest
/// 5%, so it steps rather than counts. Bytes are f64 because u64 is not
/// exportable over these bindings; an installer is nowhere near the limit
/// where a double stops being exact.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct UpdateProgress {
    pub percent: i32,
    pub downloaded: f64,
    pub total: f64,
}

/// Emitted when `begin_test_case_writing` settles on where the finished
/// JSON will be written. The frontend starts watching that path straight
/// away, so the file folds into the queue the moment the assistant writes
/// it instead of waiting to be imported by hand.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct IntakeOutputPath {
    pub path: String,
}

/// Emitted when the HTML report's comment box autosaves a note back over
/// the loopback listener - the frontend writes it into local storage.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct CaseNoteSaved {
    pub org: String,
    pub case_id: i32,
    pub text: String,
}

/// Emitted when a comment typed in the report page has been written into a
/// DRAFT case. The file (when there is one) is already updated; this is
/// what keeps the queue in the app showing the same text.
///
/// `stamp` is the file's new fingerprint, so the frontend can move its
/// watch snapshot forward - the watcher stays silent about our own write,
/// so nothing else would.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct DraftCommentSaved {
    /// The file it was written into, empty for a case with no file.
    pub path: String,
    pub stamp: String,
    /// Identity, matching the frontend's `caseKey` rule.
    pub id: Option<i32>,
    pub title: String,
    /// The row's occurrence key, as the page had it (see NotePayload::key).
    pub key: String,
    /// The PBI the page was made for.
    pub pbi_id: Option<i32>,
    pub text: String,
}

/// Emitted when the whole-set comment for one file has been written.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct DraftGeneralCommentSaved {
    pub path: String,
    pub stamp: String,
    pub text: String,
}

/// Emitted when Azure DevOps has asked the app to slow down and
/// `ado::throttle::note_server_delay` has started a fresh hold on it - see
/// that function for why this is not a 429 handler. ADO's limit is per
/// *user*, not per app, so by the time this fires the same slowdown may
/// already be reaching the user's browser tabs and git operations with no
/// explanation. The frontend says so, and points at the Settings pacing
/// control that can hand some of the shared budget back.
///
/// Only fired for a hold that is new or longer than the one already in
/// force - see the `if extend` guard around the emit - so one long import
/// getting repeatedly told to slow down raises this once per hold, not
/// once per response.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct SlowdownRequested {
    /// Seconds the pacer is holding requests for, after the 30s cap.
    pub secs: u32,
}

/// Emitted as an unattended run moves: once when a case's browser is
/// opening, once per step as it starts, once when the case is done.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ReplayProgress {
    pub run_id: String,
    /// 0-based position in the selection.
    pub index: u32,
    pub total: u32,
    pub case_id: i32,
    pub title: String,
    /// "opening", "signing_in", "module", "step" or "done"
    pub phase: String,
    /// Meaningful for "step", "signing_in" (0) and "module" (-1).
    pub step_number: i32,
    /// How many steps the script has - the same on every phase of a case.
    pub steps: u32,
    /// Meaningful for "done".
    pub proposed: String,
}

/// Emitted as the supervised browser replays a case to a step: once before
/// each step runs, `step` of `of` (the step before the one replayed to).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct AutorunReplayProgress {
    pub case_id: i32,
    pub step: i32,
    pub of: i32,
}

/// Emitted when a replay changes the supervised browser under the panes:
/// it opened one (`opened`, nobody signed in yet), or signed it in as
/// `account` - the account's key, never its login. A pane showing another
/// case hears that its own sign-in no longer holds.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct AutorunSessionChanged {
    pub opened: bool,
    pub account: Option<String>,
}

/// Emitted when the assistant asks to replay case `case_id` (`title`) up
/// to step `step` and its script must not save: the app shows the Allow
/// prompt for request `id` (`autorun::replay_ask`), and nothing runs until
/// the person answers it with `auto_run_answer_replay_request`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct AutorunReplayRequest {
    pub id: String,
    pub case_id: i32,
    pub title: String,
    pub step: i32,
}

/// Emitted when an unattended run pauses at a reset point: before case
/// `before_case_id` runs, a person reverts `names` (`changed_by` gives, per
/// name, the cases that changed it), then answers with
/// `auto_run_answer_reset`. `remaining` is every case still to run, the
/// next one first. Case ids and names only: never a host, an address or a
/// password. The screen supplies the titles from its own case list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct AutorunResetNeeded {
    pub run_id: String,
    pub before_case_id: i32,
    pub names: Vec<String>,
    pub changed_by: Vec<(String, Vec<i32>)>,
    pub remaining: Vec<i32>,
}

/// Emitted when replay request `id` stops waiting: answered, timed out, or
/// its caller gone. The Allow prompt for it closes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct AutorunReplayRequestEnded {
    pub id: String,
}

/// Emitted while a module path or a sign-in is being recorded: one per
/// captured click, one per text field typed into (a sign-in only), one
/// when the signed-in check is picked (a sign-in only), one per click or
/// field that could not be named, and one if the recording browser went
/// away. Carries locator words only, never a login and never anything
/// typed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct RecordingEvent {
    /// "click", "field", "marker", "unreadable" or "closed"
    pub kind: String,
    /// 1-based position of a captured step ("click" or "field"); 0
    /// otherwise.
    pub index: u32,
    /// The step in words (`link "Leave"`, `textbox "Email"`), for "click",
    /// "field" and "marker".
    pub readable: String,
    /// Why, for "unreadable" and "closed".
    pub detail: String,
    /// A "field" that is a password field. Says which field, never what
    /// was typed into it.
    pub password: bool,
}

/// Emitted when an assistant's prove or run of an API template changed
/// what is saved - the template itself, or its run history - so the API
/// Templates tab reloads its list.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ApiTemplatesChanged {
    /// The template's id.
    pub id: String,
}

/// Emitted while How To Use downloads, so Settings can show "12 of 31 MB".
/// Bytes; u32 because specta refuses u64, and the zip is capped at 200 MB.
/// `total` is the zip's size (the server's, else the release's json).
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct GuideProgress {
    pub received: u32,
    pub total: u32,
}


/// Emitted after each delete of a Clean up of test-made drafts, so the
/// dialog shows each result as it comes. `outcome` is `deleted`, or the
/// sentence the delete failed with.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct AutorunCleanupProgress {
    pub done: u32,
    pub total: u32,
    pub id: String,
    pub outcome: String,
}
