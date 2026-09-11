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

/// Emitted when an AI assistant has recorded a finding through the bridge.
/// The AI Bridge tab refreshes its list and the bell raises a notification.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct FindingRecorded {
    pub id: String,
    pub org: String,
    pub project: String,
    pub kind: String,
    pub title: String,
}
