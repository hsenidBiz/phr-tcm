//! The test-case queue: import, export, HTML views, and the serial
//! create/update submit loop with its cooperative cancel.

use tauri::Manager;
use tauri_specta::Event;

/// Serialises the read-patch-write that every comment save performs.
///
/// Saving a comment reads the whole JSON file, patches one value and writes
/// it back. Two of those interleaving means the second read happens before
/// the first write, and the first comment is gone - the box on the page
/// still shows it, so nobody finds out until the file is reopened.
///
/// This became reachable when the note listener started handling
/// connections off the accept thread: the single-threaded loop used to
/// serialise these by accident, and that was the only thing stopping it.
/// The listener has to stay concurrent - one stalled peer must not block
/// every save - so the guarantee moves here, where it is only ever held
/// across a file read and a file write.
static NOTE_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

use crate::events::{
    CaseNoteSaved, DraftCommentSaved, DraftGeneralCommentSaved, PlanCreated, SubmitProgress,
};
use crate::state::{get_fresh_token, SubmitCancel};
use crate::{ado, import_parser, model, note_server};

#[derive(serde::Serialize, specta::Type)]
pub struct ImportResult {
    pub cases: Vec<model::TestCase>,
    pub warnings: Vec<String>,
}

#[derive(serde::Serialize, specta::Type)]
pub struct SubmitItemResult {
    pub index: u32,
    pub title: String,
    /// "created" | "updated" | "failed"
    pub action: String,
    pub id: Option<i32>,
    pub error: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn parse_import_file(path: String) -> Result<ImportResult, String> {
    let (cases, warnings) = import_parser::parse_file(&path)?;
    Ok(ImportResult { cases, warnings })
}

/// One-shot content fingerprint. Used when a watch is (re)armed, to catch
/// an edit made while the app was closed or the tab was elsewhere - the
/// OS watcher only reports changes from the moment it starts. `None` means
/// the file can't be read right now.
#[tauri::command]
#[specta::specta]
pub fn file_stamp(path: String) -> Option<String> {
    crate::filewatch::stamp(std::path::Path::new(&path))
}

/// Follow `path` for edits, emitting `WatchedFileChanged` per real content
/// change. Several files can be followed at once; re-watching the same
/// path replaces only that watch.
#[tauri::command]
#[specta::specta]
pub fn watch_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::filewatch::FileWatchState>,
    path: String,
) -> Result<(), String> {
    crate::filewatch::start(&app, &state, &path)
}

/// Stop following one file. Unknown paths are a no-op.
#[tauri::command]
#[specta::specta]
pub fn unwatch_file(state: tauri::State<'_, crate::filewatch::FileWatchState>, path: String) {
    crate::filewatch::stop(&state, &path);
}

/// Stop following every file - used when the PBI scope changes.
#[tauri::command]
#[specta::specta]
pub fn unwatch_all_files(state: tauri::State<'_, crate::filewatch::FileWatchState>) {
    crate::filewatch::stop_all(&state);
}

#[tauri::command]
#[specta::specta]
pub fn export_queue_json(path: String, queue: Vec<model::TestCase>) -> Result<(), String> {
    import_parser::export_queue_to_json(&queue, &path)
}

#[tauri::command]
#[specta::specta]
pub fn export_queue_html(path: String, queue: Vec<model::TestCase>, subtitle: String) -> Result<(), String> {
    // A saved-to-disk export is shared/archived - no autosaving note boxes,
    // and no palette either: a file that leaves this machine opens in the
    // neutral light styling rather than in whatever theme the sender
    // happened to be using. The page's own switch still gets the reader to
    // dark in one click.
    import_parser::export_queue_to_html(&queue, &path, &subtitle, None, &Default::default())
}

#[derive(serde::Serialize, specta::Type)]
pub struct SharedQueue {
    /// The PBI the sender drafted against. When it differs from the
    /// recipient's current selection the frontend asks which one to load
    /// into - the queue is stored PER PBI, so loading into the wrong one
    /// hides the cases behind a PBI switch.
    pub pbi_id: i32,
    /// Enough to select that PBI without another lookup.
    pub pbi_title: String,
    pub pbi_work_item_type: String,
    pub organization: String,
    pub project: String,
    pub cases: Vec<model::TestCase>,
    pub warnings: Vec<String>,
}

/// Uploads the draft queue as an ADO attachment on the PBI and returns a
/// pasteable share link. Review-before-upload sharing: the cases do NOT
/// exist in ADO - only this JSON file does.
#[tauri::command]
#[specta::specta]
pub async fn share_queue(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    queue: Vec<model::TestCase>,
) -> Result<String, String> {
    if queue.is_empty() {
        return Err("nothing to share - the queue is empty".into());
    }
    let json = import_parser::queue_to_json_string(&queue)?;
    let token = get_fresh_token(&app).await.map_err(|e| e.to_string())?;
    let link = ado::AdoClient::new(token)
        .share_draft(&organization, &project, pbi_id, &json)
        .await
        .map_err(|e| e.to_string())?;
    crate::applog::info(format!(
        "Shared a draft of {} case(s) for review on PBI #{pbi_id}",
        queue.len()
    ));
    Ok(link)
}

/// Consumes a shared draft by its link (with the CALLER's own sign-in) and
/// runs it through the real importer, exactly like a file import. Links
/// are one-time use: a successful import revokes the share.
#[tauri::command]
#[specta::specta]
pub async fn fetch_shared_queue(
    app: tauri::AppHandle,
    link: String,
) -> Result<SharedQueue, String> {
    let share = crate::ado_share::parse_share_link(&link)?;
    let token = get_fresh_token(&app).await.map_err(|e| e.to_string())?;
    let client = ado::AdoClient::new(token);
    let taken = client.take_shared_draft(&share).await?;
    let json = taken.json;
    // Through the same temp-file + parse_file path as every other import,
    // so shared drafts get identical validation and warnings.
    let path = std::env::temp_dir().join(format!(
        "tcm-shared-{}-{}.json",
        std::process::id(),
        share.attachment_id
    ));
    std::fs::write(&path, &json).map_err(|e| e.to_string())?;
    let parsed = import_parser::parse_file(path.to_str().unwrap_or_default());
    let _ = std::fs::remove_file(&path);
    // The `?` here is why the revoke waits until after it. A link is
    // one-time use, and burning it on a draft the importer then refused
    // left the recipient with nothing to retry and the sender having to
    // share the whole thing again.
    let (cases, mut warnings) = parsed?;
    if let Some(w) = client.revoke_share(&share, &taken.pending_revoke).await {
        warnings.push(w);
    }
    crate::applog::info(format!(
        "Imported a shared draft: {} case(s) for PBI #{} (link revoked)",
        cases.len(),
        share.pbi_id
    ));
    Ok(SharedQueue {
        pbi_id: share.pbi_id,
        pbi_title: taken.pbi_title,
        pbi_work_item_type: taken.pbi_work_item_type,
        organization: share.org,
        project: share.project,
        cases,
        warnings,
    })
}

#[derive(serde::Serialize, specta::Type)]
pub struct MaterializedDraft {
    pub path: String,
    pub stamp: String,
}

/// Write an imported SHARED draft to a real local file and return its path
/// and fingerprint so the caller can arm a watch on it.
///
/// A share-link queue used to have no file at all, so the id write-back
/// after a submit had nowhere to land: the created ids lived only in Azure
/// DevOps, and the next import of the same drafts silently created every
/// case again (43 duplicates in one real incident). One stable path per
/// PBI - a newer share for the same PBI replaces the older copy.
#[tauri::command]
#[specta::specta]
pub fn materialize_shared_draft(
    app: tauri::AppHandle,
    pbi_id: i32,
    cases: Vec<model::TestCase>,
) -> Result<MaterializedDraft, String> {
    let json = import_parser::queue_to_json_string(&cases)?;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("shared-drafts");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("shared-pbi-{pbi_id}.json"));
    let path_str = path.to_string_lossy().to_string();
    let stamp = crate::filewatch::write_watched(&watch_state(&app), &path_str, &json)?;
    crate::applog::info(format!(
        "Materialized a shared draft for PBI #{pbi_id} ({} case(s)) at {path_str}",
        cases.len()
    ));
    Ok(MaterializedDraft { path: path_str, stamp })
}

/// The secret shared with the report pages this run generates. Minted once,
/// never written to disk, and only ever embedded in a page the app itself
/// wrote - see `note_server::start` for what it defends against.
fn note_token() -> &'static str {
    static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TOKEN.get_or_init(|| {
        rand::random::<[u8; 24]>()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    })
}

/// One note listener per app run, started lazily on the first report.
fn ensure_note_server(app: &tauri::AppHandle) -> Option<u16> {
    static PORT: std::sync::OnceLock<Option<u16>> = std::sync::OnceLock::new();
    *PORT.get_or_init(|| {
        let app = app.clone();
        note_server::start(note_token().to_string(), move |n| route_note(&app, n))
            .ok() // no listener -> report still opens, comments just can't save
    })
}

/// A note may only write to a file the app is CURRENTLY WATCHING.
///
/// The token above already keeps out anything that is not one of our pages,
/// but a note carries the path to write, and our own page should not be able
/// to name an arbitrary file either - a stale tab, or a page saved to disk
/// and reopened later, would otherwise still be able to patch whatever path
/// it was holding. The watch list is exactly the set of files the user has
/// pointed the app at.
fn writable(app: &tauri::AppHandle, path: &str) -> Result<(), String> {
    if crate::filewatch::watched_paths(&watch_state(app)).iter().any(|p| p == path) {
        return Ok(());
    }
    Err("this file is no longer open in the app - reopen the report from the queue".into())
}

/// Where a comment posted from a report page belongs. The default arm also
/// catches pages generated before drafts had comments, which send no kind.
fn route_note(app: &tauri::AppHandle, n: note_server::NotePayload) -> Result<(), String> {
    // Poisoning only means a previous save panicked mid-write; the next one
    // still has to work, and it re-reads the file anyway.
    let _serialised = NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    match n.kind.as_str() {
        "case" => save_draft_case_comment(app, n),
        "general" => save_draft_general_comment(app, n),
        _ => {
            let _ = CaseNoteSaved { org: n.org, case_id: n.case_id, text: n.text }.emit(app);
            Ok(())
        }
    }
}

fn watch_state(app: &tauri::AppHandle) -> tauri::State<'_, crate::filewatch::FileWatchState> {
    app.state::<crate::filewatch::FileWatchState>()
}

/// Patch one case's comment into the file it came from, then tell the app
/// so the queue card shows the same text.
///
/// A case with no file (typed in Manual Entry) skips straight to the event:
/// the comment still belongs on the case, it just has nowhere on disk to
/// live until the draft is exported.
fn save_draft_case_comment(
    app: &tauri::AppHandle,
    n: note_server::NotePayload,
) -> Result<(), String> {
    let mut stamp = String::new();
    if !n.path.is_empty() {
        writable(app, &n.path)?;
        let json = std::fs::read_to_string(&n.path)
            .map_err(|e| format!("could not read the file: {e}"))?;
        let target = import_parser::comments::CaseTarget {
            id: n.id,
            title: n.title.clone(),
        };
        let patched = import_parser::comments::patch_case_comment(&json, &target, &n.text)?;
        stamp = crate::filewatch::write_watched(&watch_state(app), &n.path, &patched)?;
    }
    let _ = DraftCommentSaved {
        path: n.path,
        stamp,
        id: n.id,
        title: n.title,
        text: n.text,
    }
    .emit(app);
    Ok(())
}

fn save_draft_general_comment(
    app: &tauri::AppHandle,
    n: note_server::NotePayload,
) -> Result<(), String> {
    let stamp = write_general_comment(app, &n.path, &n.text)?;
    let _ = DraftGeneralCommentSaved {
        path: n.path,
        stamp,
        text: n.text,
    }
    .emit(app);
    Ok(())
}

fn write_general_comment(
    app: &tauri::AppHandle,
    path: &str,
    text: &str,
) -> Result<String, String> {
    writable(app, path)?;
    let json =
        std::fs::read_to_string(path).map_err(|e| format!("could not read the file: {e}"))?;
    let patched = import_parser::comments::patch_general_comment(&json, text)?;
    crate::filewatch::write_watched(&watch_state(app), path, &patched)
}

/// The whole-set comment held in a JSON file, for prefilling the panel.
/// A file that has none - or can't be read - simply has no comment.
#[tauri::command]
#[specta::specta]
pub fn read_general_comment(path: String) -> String {
    std::fs::read_to_string(&path)
        .map(|j| import_parser::comments::general_comment(&j))
        .unwrap_or_default()
}

/// Save the whole-set comment from the app's own panel. Returns the file's
/// new fingerprint so the caller can move its watch snapshot forward.
#[tauri::command]
#[specta::specta]
pub fn save_general_comment(
    app: tauri::AppHandle,
    path: String,
    text: String,
) -> Result<String, String> {
    // Third and last entry into the read-patch-write. The lock lives at the
    // entry points rather than inside write_general_comment, because
    // route_note already holds it by the time it gets there and a std Mutex
    // is not reentrant.
    let _serialised = NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    write_general_comment(&app, &path, &text)
}

/// Save one draft case's comment into the file it came from, from the app.
/// Mirrors what the report page's box does, for the queue card.
#[tauri::command]
#[specta::specta]
pub fn save_draft_comment(
    app: tauri::AppHandle,
    path: String,
    id: Option<i32>,
    title: String,
    text: String,
) -> Result<String, String> {
    // The same guard the note listener takes. A comment typed on the queue
    // card and one typed in the browser page reach the same file by
    // different routes, and read-patch-write from both at once loses one of
    // them silently.
    let _serialised = NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let json = std::fs::read_to_string(&path).map_err(|e| format!("could not read the file: {e}"))?;
    let target = import_parser::comments::CaseTarget { id, title };
    let patched = import_parser::comments::patch_case_comment(&json, &target, &text)?;
    crate::filewatch::write_watched(&watch_state(&app), &path, &patched)
}

/// Replace a draft file's test cases with the given list - the write-back
/// behind bulk edits on the queue, so the file a case came from says what
/// the queue says. Everything ELSE in the file survives: the top-level
/// general comments, and any key this app does not know about, stay
/// exactly as written. Returns the file's new fingerprint so the caller
/// can move its watch snapshot forward - the watcher stays silent about
/// our own write, so nothing else would.
#[tauri::command]
#[specta::specta]
pub fn save_draft_cases(
    app: tauri::AppHandle,
    path: String,
    cases: Vec<model::TestCase>,
) -> Result<String, String> {
    // Same guard as the comment writers: a bulk edit and a comment box
    // autosave can reach the same file, and read-patch-write from both at
    // once loses one of them silently.
    let _serialised = NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let old = std::fs::read_to_string(&path).map_err(|e| format!("could not read the file: {e}"))?;
    let out = import_parser::merge_cases_into_draft(&old, &cases)?;
    crate::filewatch::write_watched(&watch_state(&app), &path, &out)
}

/// Render the queue's HTML report to a temp file and open it in the
/// default browser - v1's "View" behaviour, no save dialog. Cases with a
/// work item id get a comment box that autosaves back into the app via
/// the loopback note listener.
#[tauri::command]
#[specta::specta]
pub fn view_queue_html(
    app: tauri::AppHandle,
    queue: Vec<model::TestCase>,
    subtitle: String,
    organization: String,
    notes: std::collections::HashMap<String, String>,
    palette: crate::webtheme::PagePalette,
) -> Result<(), String> {
    let path_str = render_queue_html(&app, queue, subtitle, organization, notes, palette)?;
    tauri_plugin_opener::open_path(&path_str, None::<&str>).map_err(|e| e.to_string())
}

/// Re-render the queue page WITHOUT opening a browser - the queue report's
/// twin of `refresh_draft_html`, for the same reason: the keep-in-step
/// refresh used to share `view_queue_html` with the button, and the
/// `open_path` at the end of that meant every re-render - each one
/// triggered by nothing more than the app window regaining focus - opened
/// ANOTHER tab on the same file. A page already open learns about the
/// rewrite from its revision poll and pulls the new content itself;
/// nothing here should touch the browser.
#[tauri::command]
#[specta::specta]
pub fn refresh_queue_html(
    app: tauri::AppHandle,
    queue: Vec<model::TestCase>,
    subtitle: String,
    organization: String,
    notes: std::collections::HashMap<String, String>,
    palette: crate::webtheme::PagePalette,
) -> Result<(), String> {
    render_queue_html(&app, queue, subtitle, organization, notes, palette).map(|_| ())
}

fn render_queue_html(
    app: &tauri::AppHandle,
    queue: Vec<model::TestCase>,
    subtitle: String,
    organization: String,
    notes: std::collections::HashMap<String, String>,
    palette: crate::webtheme::PagePalette,
) -> Result<String, String> {
    // Stable per run, for the same reason the draft report is: a name
    // carrying `queue.len()` writes a DIFFERENT file the moment a case is
    // added or removed, so the tab already open could never see the change
    // however hard the page looked for it.
    let path = std::env::temp_dir().join(format!("test-cases-{}.html", std::process::id()));
    let path_str = path.to_string_lossy().to_string();
    let note_ctx = (!organization.is_empty())
        .then(|| ensure_note_server(app))
        .flatten()
        .map(|port| import_parser::NoteCtx {
            port,
            token: note_token().to_string(),
            org: organization,
            notes,
        });
    import_parser::export_queue_to_html(
        &queue,
        &path_str,
        &subtitle,
        note_ctx.as_ref().map(import_parser::CommentCtx::Ado),
        &palette,
    )?;
    // And tell a page already open on these cases that it is behind - and
    // where to pull the fresh content from.
    crate::note_server::set_report_path(crate::note_server::REPORT_QUEUE, &path_str);
    crate::note_server::bump_revision(crate::note_server::REPORT_QUEUE);
    Ok(path_str)
}

/// The same page for a DRAFT queue. Every case gets a comment box - drafts
/// have no work item id to key an app-side note by, and the comment belongs
/// to the case itself here - plus a collapsible column of whole-set
/// comments, one per file the draft was imported from.
///
/// `owners` is the file each queued case came from, aligned with `queue`;
/// an empty entry means the case was typed by hand and has no file.
#[tauri::command]
#[specta::specta]
pub fn view_draft_html(
    app: tauri::AppHandle,
    queue: Vec<model::TestCase>,
    subtitle: String,
    owners: Vec<String>,
    files: Vec<import_parser::DraftFile>,
    palette: crate::webtheme::PagePalette,
) -> Result<(), String> {
    let path_str = render_draft_html(&app, queue, subtitle, owners, files, palette)?;
    tauri_plugin_opener::open_path(&path_str, None::<&str>).map_err(|e| e.to_string())
}

/// Re-render the draft page WITHOUT opening a browser. This is what the
/// background keep-in-step refresh calls: it used to share `view_draft_html`
/// with the button, and the `open_path` at the end of that meant every
/// comment save and every queue change opened ANOTHER tab on the same file.
/// A page already open learns about the rewrite from its revision poll and
/// pulls the new content itself; nothing here should touch the browser.
#[tauri::command]
#[specta::specta]
pub fn refresh_draft_html(
    app: tauri::AppHandle,
    queue: Vec<model::TestCase>,
    subtitle: String,
    owners: Vec<String>,
    files: Vec<import_parser::DraftFile>,
    palette: crate::webtheme::PagePalette,
) -> Result<(), String> {
    render_draft_html(&app, queue, subtitle, owners, files, palette).map(|_| ())
}

fn render_draft_html(
    app: &tauri::AppHandle,
    queue: Vec<model::TestCase>,
    subtitle: String,
    owners: Vec<String>,
    files: Vec<import_parser::DraftFile>,
    palette: crate::webtheme::PagePalette,
) -> Result<String, String> {
    // Stable per run, deliberately: the name used to carry `queue.len()`,
    // so re-exporting after an edit that changed the count wrote a DIFFERENT
    // file and the tab the developer already had open never saw it. One name
    // per process means a re-export lands on the page they are looking at,
    // which is what makes the live update land on the right document.
    let path = std::env::temp_dir()
        .join(format!("test-cases-draft-{}.html", std::process::id()));
    let path_str = path.to_string_lossy().to_string();
    let ctx = ensure_note_server(app).map(|port| import_parser::DraftNoteCtx {
        port,
        token: note_token().to_string(),
        owners,
        files,
    });
    import_parser::export_queue_to_html(
        &queue,
        &path_str,
        &subtitle,
        ctx.as_ref().map(import_parser::CommentCtx::Draft),
        &palette,
    )?;
    // Where an open page can pull the fresh content from, and the signal
    // that it should: the poll sees the revision move, fetches /report,
    // and swaps itself in place.
    crate::note_server::set_report_path(crate::note_server::REPORT_DRAFT, &path_str);
    crate::note_server::bump_revision(crate::note_server::REPORT_DRAFT);
    Ok(path_str)
}

/// Serial creation loop ported from v1 CreationWorker: one item at a time,
/// 500 ms spacing (rate-limit respect), new cases linked to the PBI, updates
/// patched in place. A failed item never aborts the rest.
#[tauri::command]
#[specta::specta]
pub async fn submit_queue(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    queue: Vec<model::TestCase>,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
    area_path: Option<String>,
    iteration_path: Option<String>,
) -> Result<Vec<SubmitItemResult>, String> {
    // One submit at a time. A second call used to clear the cancel flag
    // below - wiping a Cancel already clicked - and then run a second loop
    // over the same queue. Two loops create every case twice, and this tool
    // only deletes Test Cases, and only for someone Azure DevOps says may,
    // so those duplicates may well be permanent. The guard releases
    // on every exit path, including a panic.
    let cancel = app.state::<SubmitCancel>();
    let Some(_running) = cancel.claim() else {
        crate::applog::warn("refused a second submit while one was already running");
        return Err("A submit is already running. Wait for it to finish, or cancel it.".into());
    };

    // Arm the cancel flag BEFORE any awaits: the suite-resolution phase
    // below can take seconds, and a Cancel clicked during it must stick
    // (resetting later would silently swallow it and run the whole queue).
    cancel.0.store(false, std::sync::atomic::Ordering::SeqCst);

    // Best-effort board visibility (ported from v1 CreationWorker._ensure_suite):
    // make sure the PBI's requirement-based suite exists before creating, so
    // linked cases surface on the board's test count. Failures never block
    // creation. The PBI's own paths double as the default area/iteration for
    // created cases (unset picker = "Same as PBI").
    let mut pbi_area = String::new();
    let mut pbi_iteration = String::new();
    // The Steps field as Azure DevOps currently holds it, for every row that
    // is an UPDATE. See `steps_patch`: without it, a case exported to JSON,
    // retitled and re-imported writes its steps back from the plain-text
    // read and strips their formatting and screenshots. One batched read
    // covers the whole queue, and creates cost nothing.
    let mut steps_before: std::collections::HashMap<i32, String> = Default::default();
    if let Ok(token) = get_fresh_token(&app).await {
        let client = ado::AdoClient::new(token);
        let update_ids: Vec<i32> = queue.iter().filter_map(|tc| tc.update_id).collect();
        if !update_ids.is_empty() {
            match client
                .get_test_cases_by_ids(&organization, &update_ids, None, None)
                .await
            {
                Ok(current) => {
                    steps_before = current.into_iter().map(|c| (c.id, c.steps_xml)).collect();
                }
                // Without a baseline the import's own steps are written, as
                // they always were - the file is meant to describe the case.
                // Say so, because it is the one path where markup can still
                // be lost and the log is where that has to be visible.
                Err(e) => crate::applog::warn(format!(
                    "could not read the current steps for {} update(s) ({e}) - their steps                      will be rewritten from the imported text, which drops any formatting                      Azure DevOps holds",
                    update_ids.len()
                )),
            }
        }
        if let Ok((area, iteration)) = client
            .get_work_item_paths(&organization, &project, pbi_id)
            .await
        {
            // Still best-effort (a failure never blocks creation), but when
            // the PBI had no test plan at all, the plan gets created FIRST
            // and the user is told before the upload proceeds.
            if let Ok(ensured) = client
                .ensure_requirement_suite(&organization, &project, pbi_id, &area, &iteration)
                .await
            {
                if ensured.created_plan {
                    let _ = PlanCreated { plan_name: ensured.plan_name }.emit(&app);
                }
            }
            pbi_area = area;
            pbi_iteration = iteration;
        }
    }
    let effective_area = area_path.filter(|s| !s.is_empty()).unwrap_or(pbi_area);
    let effective_iteration = iteration_path
        .filter(|s| !s.is_empty())
        .unwrap_or(pbi_iteration);

    let total = queue.len() as u32;
    crate::applog::info(format!(
        "Submitting {total} test case(s) to {organization}/{project} PBI #{pbi_id}"
    ));
    let mut results: Vec<SubmitItemResult> = vec![];
    for (i, tc) in queue.iter().enumerate() {
        if cancel.0.load(std::sync::atomic::Ordering::SeqCst) {
            break; // unprocessed items stay in the client's queue
        }
        if i > 0 {
            // Per-item spacing on top of the global pacer: bulk creation is
            // the app's heaviest burst, so it stays the most deferential
            // thing it does. Never below 500 ms (v1's proven spacing), and
            // wider when the user has asked for a gentler rate.
            let gap = std::cmp::max(500, crate::ado::throttle::current_interval_ms());
            tokio::time::sleep(std::time::Duration::from_millis(gap)).await;
            // Check again on the far side of the pause. The loop spends
            // most of its life here - half a second per item, more when
            // throttled - so this is where a Cancel usually lands, and
            // checking only at the top of the iteration would create one
            // more case after the click. These cases cannot be deleted.
            if cancel.0.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
        }
        let item = process_queue_item(
            &app,
            &organization,
            &project,
            pbi_id,
            i as u32,
            tc,
            module_ref.as_deref(),
            preconditions_ref.as_deref(),
            &effective_area,
            &effective_iteration,
            tc.update_id.and_then(|id| steps_before.get(&id)).map(String::as_str),
        )
        .await;
        let _ = SubmitProgress {
            index: item.index,
            total,
            title: item.title.clone(),
            action: item.action.clone(),
        }
        .emit(&app);
        if item.action == "failed" {
            crate::applog::error(format!(
                "Submit failed for '{}': {}",
                item.title,
                item.error.as_deref().unwrap_or("unknown error")
            ));
        }
        results.push(item);
    }
    let failed = results.iter().filter(|r| r.action == "failed").count();
    crate::applog::info(format!(
        "Submit finished: {} of {total} processed, {failed} failed",
        results.len()
    ));
    // Tags carried by cases that actually landed provably exist in the
    // project now, so fold them into the cache rather than waiting for a
    // refresh to rediscover what we just created ourselves.
    let created: Vec<String> = results
        .iter()
        .filter(|r| r.action != "failed")
        .filter_map(|r| queue.get(r.index as usize))
        .flat_map(|tc| tc.tags.split(';').map(|t| t.trim().to_string()))
        .filter(|t| !t.is_empty())
        .collect();
    if !created.is_empty() {
        crate::refcache::merge(
            &crate::refcache::tags_key(&organization, &project),
            &created,
        );
    }
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
async fn process_queue_item(
    app: &tauri::AppHandle,
    organization: &str,
    project: &str,
    pbi_id: i32,
    index: u32,
    tc: &model::TestCase,
    m_ref: Option<&str>,
    p_ref: Option<&str>,
    area_path: &str,
    iteration_path: &str,
    original_steps_xml: Option<&str>,
) -> SubmitItemResult {
    let failed = |error: String| SubmitItemResult {
        index,
        title: tc.title.clone(),
        action: "failed".into(),
        id: None,
        error: Some(error),
    };
    if let Err(msg) = tc.is_valid() {
        return failed(msg);
    }
    let token = match get_fresh_token(app).await {
        Ok(t) => t,
        Err(e) => return failed(e.to_string()),
    };
    let client = ado::AdoClient::new(token);
    let outcome = match tc.update_id {
        Some(existing_id) => client
            .update_test_case_from_model(
                organization,
                project,
                existing_id,
                tc,
                m_ref,
                p_ref,
                original_steps_xml,
                // An import: a blank column is the absence of an opinion,
                // never an instruction to erase.
                ado::BlankPolicy::Skip,
            )
            .await
            .map(|_| (existing_id, "updated")),
        None => match client
            .create_test_case(organization, project, tc, m_ref, area_path, iteration_path, p_ref)
            .await
        {
            // The case EXISTS from here on. A failed link must not be
            // reported as a failed create: the row stays in the queue, the
            // user submits again, and Azure DevOps ends up with two copies
            // of a case that cannot be deleted. Report it created, and say
            // the link is what needs attention.
            Ok(new_id) => match client.link_to_pbi(organization, project, new_id, pbi_id).await {
                Ok(()) => Ok((new_id, "created")),
                Err(e) => {
                    crate::applog::warn(format!(
                        "Created #{new_id} '{}' but linking it to PBI #{pbi_id} failed: {e}",
                        tc.title
                    ));
                    return SubmitItemResult {
                        index,
                        title: tc.title.clone(),
                        action: "created".into(),
                        id: Some(new_id),
                        error: Some(format!(
                            "Created, but linking to PBI #{pbi_id} failed: {e}.                              The case exists - link it in Azure DevOps rather than                              submitting again, which would create a second copy."
                        )),
                    };
                }
            },
            Err(e) => Err(e),
        },
    };
    match outcome {
        Ok((id, action)) => SubmitItemResult {
            index,
            title: tc.title.clone(),
            action: action.into(),
            id: Some(id),
            error: None,
        },
        Err(e) => failed(e.to_string()),
    }
}

/// Stop the running submit loop after the in-flight item finishes.
#[tauri::command]
#[specta::specta]
pub fn cancel_submit(state: tauri::State<'_, SubmitCancel>) {
    state.0.store(true, std::sync::atomic::Ordering::SeqCst);
}
