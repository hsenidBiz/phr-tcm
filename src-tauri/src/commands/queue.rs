//! The test-case queue: import, export, HTML views, and the serial
//! create/update submit loop (single-flight; it runs to the end once started).

use tauri::Manager;
use tauri_specta::Event;

use crate::events::{
    CaseNoteSaved, DraftCommentSaved, DraftGeneralCommentSaved, PlanCreated, SubmitProgress,
};
use crate::filewatch::NOTE_WRITE;
use crate::state::{get_fresh_token, SubmitCancel};
use crate::{ado, import_parser, model, note_server};

#[derive(serde::Serialize, specta::Type)]
pub struct ImportResult {
    pub cases: Vec<model::TestCase>,
    pub warnings: Vec<String>,
    /// The file's `specs` list, verbatim - resolved when a page is written.
    pub specs: Vec<String>,
}

#[derive(Debug, serde::Serialize, specta::Type)]
pub struct SubmitItemResult {
    pub index: u32,
    pub title: String,
    /// "created" | "updated" | "failed" | "unknown". "unknown": the batch
    /// failed and Azure DevOps could not be asked whether this create
    /// landed, or it had not finished yet. It may exist - check
    /// (`reconcile_upload`) before uploading it again.
    pub action: String,
    pub id: Option<i32>,
    pub error: Option<String>,
}

/// Cases per `$batch` call. See the loop in `submit_queue` for why it is
/// not the API's maximum of 200.
/// Azure DevOps' own ceiling per `$batch` call. One call per 200 cases:
/// there is no Stop any more, so nothing is gained by chunking smaller,
/// and the progress bar sweeps while a call is in flight.
const BATCH_SIZE: usize = crate::ado::wit_batch::MAX_PER_BATCH;

#[tauri::command]
#[specta::specta]
pub fn parse_import_file(path: String) -> Result<ImportResult, String> {
    let parsed = import_parser::parse_file(&path)?;
    Ok(ImportResult { cases: parsed.cases, warnings: parsed.warnings, specs: parsed.specs })
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
    let parsed = parsed?;
    let (cases, mut warnings) = (parsed.cases, parsed.warnings);
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

/// Whether the app's own commands may rewrite `path` as a draft.
///
/// Not `writable()`: the post-upload id stamp writes the files a queue came
/// from after a PBI switch has already unwatched them, and refusing it
/// leaves files that re-import as duplicates. So: a watched file, or an
/// existing `.json` file that already holds a draft (`test_cases`, or a
/// bare case array). Anything else is not ours to overwrite.
pub fn draft_write_allowed(path: &str, watched: &[String]) -> Result<(), String> {
    if watched.iter().any(|p| p == path) {
        return Ok(());
    }
    let p = std::path::Path::new(path);
    let is_json = p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if !is_json || !p.is_file() {
        return Err("that is not a draft file this app can write to".into());
    }
    let text = std::fs::read_to_string(p).map_err(|e| format!("could not read the file: {e}"))?;
    match serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) {
        Ok(v) if v.is_array() || v.get("test_cases").is_some_and(|t| t.is_array()) => Ok(()),
        _ => Err("that file is not a test case draft, so the app will not write to it".into()),
    }
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
        let json = import_parser::read_json_text(std::path::Path::new(&n.path))?;
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
        key: n.key,
        pbi_id: n.pbi_id,
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
    let json = import_parser::read_json_text(std::path::Path::new(path))?;
    let patched = import_parser::comments::patch_general_comment(&json, text)?;
    crate::filewatch::write_watched(&watch_state(app), path, &patched)
}

/// The whole-set comment held in a JSON file, for prefilling the panel.
/// A file that has none - or can't be read - simply has no comment.
#[tauri::command]
#[specta::specta]
pub fn read_general_comment(path: String) -> String {
    import_parser::read_json_text(std::path::Path::new(&path))
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

/// Save the `specs` list from the Import File tab's Attach control. Returns
/// the file's new fingerprint so the caller can move its watch forward.
#[tauri::command]
#[specta::specta]
pub fn save_specs(app: tauri::AppHandle, path: String, specs: Vec<String>) -> Result<String, String> {
    // Same guard as the comment saves: one read-patch-write at a time.
    let _serialised = NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    writable(&app, &path)?;
    let json = import_parser::read_json_text(std::path::Path::new(&path))?;
    let patched = import_parser::specs::patch_specs(&json, &specs)?;
    crate::filewatch::write_watched(&watch_state(&app), &path, &patched)
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
    draft_write_allowed(&path, &crate::filewatch::watched_paths(&watch_state(&app)))?;
    let json = import_parser::read_json_text(std::path::Path::new(&path))?;
    let target = import_parser::comments::CaseTarget { id, title };
    let patched = import_parser::comments::patch_case_comment(&json, &target, &text)?;
    crate::filewatch::write_watched(&watch_state(&app), &path, &patched)
}

/// Write a queue edit back into the draft file its cases came from, so the
/// file says what the queue says. `edits` holds one entry per queue row the
/// file owns, IN QUEUE ORDER: the row before the edit (how the file finds
/// its own copy, since a rename changes the title) and after it (`None` when
/// the edit removed it). Order matters: the Nth same-titled row claims the
/// Nth same-titled entry. The file is patched (`apply_draft_edits`): cases
/// it holds that the queue never showed, keys the app does not model, and
/// the author's spellings all survive. Returns the file's new fingerprint so
/// the caller can move its watch snapshot forward - the watcher stays silent
/// about our own write, so nothing else would.
#[tauri::command]
#[specta::specta]
pub fn save_draft_cases(
    app: tauri::AppHandle,
    path: String,
    edits: Vec<model::DraftEdit>,
) -> Result<String, String> {
    // Same guard as the comment writers: a bulk edit and a comment box
    // autosave can reach the same file, and read-patch-write from both at
    // once loses one of them silently.
    let _serialised = NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    draft_write_allowed(&path, &crate::filewatch::watched_paths(&watch_state(&app)))?;
    let old = import_parser::read_json_text(std::path::Path::new(&path))?;
    let out = import_parser::apply_draft_edits(&old, &edits)?;
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
    // The Test map beside the page, when the cases have areas to map; the
    // page's "View as Tree" links to it.
    let page_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tree = crate::test_map::write_beside(&queue, &subtitle, &palette, &page_name, crate::note_server::REPORT_QUEUE)?;
    import_parser::export_queue_page(
        &queue,
        &path_str,
        &subtitle,
        note_ctx.as_ref().map(import_parser::CommentCtx::Ado),
        &palette,
        tree.as_deref(),
        &[],
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
#[allow(clippy::too_many_arguments)]
pub async fn view_draft_html(
    app: tauri::AppHandle,
    queue: Vec<model::TestCase>,
    subtitle: String,
    owners: Vec<String>,
    keys: Vec<String>,
    pbi_id: i32,
    files: Vec<import_parser::DraftFile>,
    palette: crate::webtheme::PagePalette,
) -> Result<(), String> {
    let path_str = render_draft_html(&app, queue, subtitle, owners, keys, pbi_id, files, palette).await?;
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
#[allow(clippy::too_many_arguments)]
pub async fn refresh_draft_html(
    app: tauri::AppHandle,
    queue: Vec<model::TestCase>,
    subtitle: String,
    owners: Vec<String>,
    keys: Vec<String>,
    pbi_id: i32,
    files: Vec<import_parser::DraftFile>,
    palette: crate::webtheme::PagePalette,
) -> Result<(), String> {
    render_draft_html(&app, queue, subtitle, owners, keys, pbi_id, files, palette).await.map(|_| ())
}

#[allow(clippy::too_many_arguments)]
async fn render_draft_html(
    app: &tauri::AppHandle,
    queue: Vec<model::TestCase>,
    subtitle: String,
    owners: Vec<String>,
    keys: Vec<String>,
    pbi_id: i32,
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
    // The specs beside the cases. Files are read here; wiki pages need the
    // user's token - without a session they render as "not signed in".
    let entries = crate::spec_pane::spec_entries(&files);
    let client = if entries.iter().any(|(e, _)| crate::spec_pane::is_wiki_entry(e)) {
        match get_fresh_token(app).await {
            Ok(token) => Some(ado::AdoClient::new(token)),
            Err(_) => {
                crate::applog::warn("Spec pane: no sign-in token, wiki specs show as not signed in".to_string());
                None
            }
        }
    } else {
        None
    };
    let specs = crate::spec_pane::render_all(&entries, client.as_ref()).await;
    let ctx = ensure_note_server(app).map(|port| import_parser::DraftNoteCtx {
        port,
        token: note_token().to_string(),
        owners,
        keys,
        pbi_id,
        files,
    });
    let page_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tree = crate::test_map::write_beside(&queue, &subtitle, &palette, &page_name, crate::note_server::REPORT_DRAFT)?;
    import_parser::export_queue_page(
        &queue,
        &path_str,
        &subtitle,
        ctx.as_ref().map(import_parser::CommentCtx::Draft),
        &palette,
        tree.as_deref(),
        &specs,
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
    // The rows the screen left out as unchanged, with where each sat, so
    // the order set after the upload still counts them. Ordering only.
    order_hint: Vec<crate::run_order::OrderHint>,
) -> Result<Vec<SubmitItemResult>, String> {
    // One submit at a time. A second call would run a second loop over the
    // same queue, and two loops create every case twice; this tool only
    // deletes Test Cases, and only for someone Azure DevOps says may, so
    // those duplicates may well be permanent. The guard releases on every
    // exit path, including a panic.
    let single = app.state::<SubmitCancel>();
    let Some(_running) = single.claim() else {
        crate::applog::warn("refused a second submit while one was already running");
        return Err("A submit is already running. Wait for it to finish.".into());
    };

    // The lower bound for "what did this upload create", should a batch
    // fail. Early by a margin: the local clock may run ahead of ADO's.
    let upload_since = iso_utc(now_secs() - SINCE_MARGIN_SECS);

    // Best-effort board visibility (ported from v1 CreationWorker._ensure_suite):
    // make sure the PBI's requirement-based suite exists before creating, so
    // linked cases surface on the board's test count. Failures never block
    // creation. The PBI's own paths double as the default area/iteration for
    // created cases (unset picker = "Same as PBI").
    let mut pbi_area = String::new();
    let mut pbi_iteration = String::new();
    // A suite refusal that is worth a second route, held until the batch
    // has run. See where it is read, after the loop.
    let mut suite_pending: Option<String> = None;
    // The PBI's requirement suite, once either route has it. Kept so the
    // upload can set the suite to spec order afterwards (design §4.1);
    // no suite means nothing is ordered.
    let mut resolved_suite: Option<crate::ado_testplan::EnsuredSuite> = None;
    // The Steps field as Azure DevOps currently holds it, for every row that
    // is an UPDATE. See `steps_patch`: without it, a case exported to JSON,
    // retitled and re-imported writes its steps back from the plain-text
    // read and strips their formatting and screenshots. One batched read
    // covers the whole queue, and creates cost nothing.
    let mut steps_before: std::collections::HashMap<i32, String> = Default::default();
    // System.Tags as ADO holds it, from the same batched read: the op that
    // can actually REMOVE a tag depends on the current value - see
    // `tags_write_ops`. Without this baseline a dropped tag stays put.
    let mut tags_before: std::collections::HashMap<i32, String> = Default::default();
    if let Ok(token) = get_fresh_token(&app).await {
        let client = ado::AdoClient::new(token);
        let update_ids: Vec<i32> = queue.iter().filter_map(|tc| tc.update_id).collect();
        if !update_ids.is_empty() {
            match client
                .get_test_cases_by_ids(&organization, &update_ids, None, None)
                .await
            {
                Ok(current) => {
                    tags_before = current.iter().map(|c| (c.id, c.tags.clone())).collect();
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
        // A queue of NOTHING but updates never touches the selected PBI:
        // updates PATCH their own work items where they already live, and
        // the area/iteration defaults below are only read by creates. The
        // suite-ensure would at best be a wasted round trip and at worst
        // CREATE a test plan on a mis-selected PBI nobody meant to touch.
        let has_creates = queue.iter().any(|tc| tc.update_id.is_none());
        if has_creates {
            if let Ok((area, iteration)) = client
                .get_work_item_paths(&organization, &project, pbi_id)
                .await
            {
                // Resolving the suite scans every test plan in the project,
                // about a minute on a large org. Once found the ids are
                // stable, so a suite Run Tests, the AI bridge or an earlier
                // upload already resolved is taken as read. A suite deleted
                // in Azure DevOps since costs nothing here - the cases still
                // link to the PBI - and Run Tests re-detects it on its 404.
                let cached = crate::ado_testplan::cached_suite(&client.base_url, &organization, &project, pbi_id);
                if let Some(s) = &cached {
                    crate::applog::info(format!(
                        "requirement suite for #{pbi_id} already resolved (plan {} '{}', suite {}) - not scanning again",
                        s.plan_id, s.plan_name, s.suite_id
                    ));
                }
                // Still best-effort (a failure never blocks creation), but
                // when the PBI had no test plan at all, the plan gets
                // created FIRST and the user is told before the upload
                // proceeds.
                let ensured = match cached {
                    Some(s) => Ok(s),
                    None => {
                        client
                            .ensure_requirement_suite(&organization, &project, pbi_id, &area, &iteration)
                            .await
                    }
                };
                match ensured {
                    Ok(ensured) => {
                        crate::ado_testplan::remember_suite(&client.base_url, &organization, &project, pbi_id, &ensured);
                        resolved_suite = Some(ensured.clone());
                        if ensured.created_plan {
                            let _ = PlanCreated { plan_name: ensured.plan_name }.emit(&app);
                        }
                    }
                    // Still never blocks the upload - the cases are linked to
                    // the PBI either way - but it is no longer a line in the
                    // log only. Named 403s carry their own sentence; the
                    // rest get the error as it is.
                    Err(e) => {
                        let reason = match &e {
                            ado::AdoError::Http { status: 403, body } => body.clone(),
                            ado::AdoError::Forbidden => "you don't have permission to create test suites in this project's plans".to_string(),
                            other => format!("{other}"),
                        };
                        crate::applog::warn(format!("no requirement suite for #{pbi_id}: {reason}"));
                        // A 403 here is the access level, not the request:
                        // the same account creates the suite fine from
                        // Boards, so there is a second route worth taking.
                        // It adds EXISTING work items to a suite, so it
                        // needs ids this upload has not created yet - which
                        // is why the sentence waits for the batch instead
                        // of going to the screen now.
                        if matches!(&e, ado::AdoError::Http { status: 403, .. }) {
                            suite_pending = Some(reason);
                        } else {
                            let _ = crate::events::SuiteNotCreated { reason }.emit(&app);
                        }
                    }
                }
                pbi_area = area;
                pbi_iteration = iteration;
            }
        }
    }
    // Cloned, not moved: the PBI's own area is what picks the team the
    // Boards fallback runs as, and that happens after the batch.
    let effective_area = area_path
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| pbi_area.clone());
    let effective_iteration = iteration_path
        .filter(|s| !s.is_empty())
        .unwrap_or(pbi_iteration);

    let total = queue.len() as u32;
    crate::applog::info(format!(
        "Submitting {total} test case(s) to {organization}/{project} PBI #{pbi_id} in batches of {BATCH_SIZE}"
    ));
    // Chunks of BATCH_SIZE, each one HTTP call executed on the server in
    // order. An upload runs to the end once started: the case in flight
    // could never be taken back (created cases cannot be deleted), so a
    // Stop only ever left a half-done set, and it is gone.
    let mut results: Vec<SubmitItemResult> = Vec::with_capacity(queue.len());
    let mut chunk_start = 0usize;
    while chunk_start < queue.len() {
        let end = (chunk_start + BATCH_SIZE).min(queue.len());
        if chunk_start > 0 {
            // Between chunks only: the request-rate setting, and nothing
            // more. Every request also passes the global pacer and holds
            // for whatever Retry-After Azure DevOps sends.
            let gap = crate::ado::throttle::current_interval_ms();
            if gap > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(gap)).await;
            }
        }
        let mut chunk: Vec<Option<SubmitItemResult>> = (chunk_start..end).map(|_| None).collect();
        match get_fresh_token(&app).await {
            Err(e) => {
                for i in chunk_start..end {
                    chunk[i - chunk_start] = Some(failed_item(i, &queue[i], e.to_string()));
                }
            }
            Ok(token) => {
                let client = ado::AdoClient::new(token);
                // A case that fails validation is reported without being
                // sent; the rest of the chunk still goes.
                let mut sent_idx: Vec<usize> = vec![];
                let mut reqs: Vec<crate::ado::wit_batch::BatchRequest> = vec![];
                for i in chunk_start..end {
                    let tc = &queue[i];
                    match queue_item_request(
                        &client,
                        &organization,
                        &project,
                        pbi_id,
                        tc,
                        module_ref.as_deref(),
                        preconditions_ref.as_deref(),
                        &effective_area,
                        &effective_iteration,
                        tc.update_id.and_then(|id| steps_before.get(&id)).map(String::as_str),
                        tc.update_id.and_then(|id| tags_before.get(&id)).map(String::as_str),
                        i - chunk_start + 1,
                    ) {
                        Ok(req) => {
                            reqs.push(req);
                            sent_idx.push(i);
                        }
                        Err(msg) => chunk[i - chunk_start] = Some(failed_item(i, tc, msg)),
                    }
                }
                if !reqs.is_empty() {
                    match client.wit_batch(&organization, &reqs).await {
                        Ok(items) => {
                            for r in map_batch_results(&queue, &sent_idx, &items) {
                                let i = r.index as usize;
                                chunk[i - chunk_start] = Some(r);
                            }
                        }
                        // The whole call failed (network, 401, 429 past its
                        // back-off, timeout). The server may still have run
                        // some of it: ask what it created before calling a
                        // create failed - retrying a create that landed makes
                        // a duplicate. See `resolve_failed_batch`.
                        Err(e) => {
                            let already_claimed: Vec<i32> = results
                                .iter()
                                .chain(chunk.iter().flatten())
                                .filter_map(|r| r.id)
                                .collect();
                            for r in resolve_failed_batch(
                                &client,
                                &organization,
                                &project,
                                pbi_id,
                                &upload_since,
                                &queue,
                                &sent_idx,
                                &e,
                                &already_claimed,
                            )
                            .await
                            {
                                let i = r.index as usize;
                                chunk[i - chunk_start] = Some(r);
                            }
                        }
                    }
                }
            }
        }
        for (k, r) in chunk.into_iter().enumerate() {
            let i = chunk_start + k;
            let item = r.unwrap_or_else(|| failed_item(i, &queue[i], "not processed".into()));
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
            } else if item.action == "unknown" {
                crate::applog::warn(format!(
                    "Submit outcome unknown for '{}': {}",
                    item.title,
                    item.error.as_deref().unwrap_or("no reason given")
                ));
            }
            results.push(item);
        }
        chunk_start = end;
    }
    let failed = results.iter().filter(|r| r.action == "failed").count();
    let unknown = results.iter().filter(|r| r.action == "unknown").count();
    crate::applog::info(format!(
        "Submit finished: {} of {total} processed, {failed} failed, {unknown} unknown",
        results.len()
    ));
    // Tags carried by cases that actually landed provably exist in the
    // project now, so fold them into the cache rather than waiting for a
    // refresh to rediscover what we just created ourselves. "unknown" is
    // not "landed".
    let created: Vec<String> = results
        .iter()
        .filter(|r| r.action == "created" || r.action == "updated")
        .filter_map(|r| queue.get(r.index as usize))
        .flat_map(|tc| tc.tags.split(';').map(|t| t.trim().to_string()))
        .filter(|t| !t.is_empty())
        .collect();
    if !created.is_empty() {
        crate::cache::update::<Vec<String>>(
            &crate::cache::keys::tags(&organization, &project),
            |tags| crate::commands::discovery::add_new_tags(tags, &created),
        );
    }
    // The suite refusal held back above. The Boards route adds work items
    // that already exist, so only now is there anything to add.
    if let Some(sentence) = suite_pending {
        // Only rows that landed: a failed row's id is either missing or
        // not a case Azure DevOps has.
        let ids: Vec<i32> = results
            .iter()
            .filter(|r| r.action != "failed")
            .filter_map(|r| r.id)
            .collect();
        if ids.is_empty() {
            // Nothing landed, so there is nothing to put in a suite and no
            // route to try. The refusal is the whole story, as it was
            // before this fallback existed.
            let _ = crate::events::SuiteNotCreated { reason: sentence }.emit(&app);
        } else {
            // The log line is decided here, where what actually happened is
            // still known: a route that answered and a route that was never
            // reached send the next reader to different places.
            let attempt = match get_fresh_token(&app).await {
                Ok(token) => {
                    let client = ado::AdoClient::new(token);
                    client
                        .boards_fallback(&organization, &project, pbi_id, &pbi_area, 0, &ids)
                        .await
                        .map(|out| (client.base_url.clone(), out))
                        .map_err(|e| {
                            format!("the Boards route did not create a suite for #{pbi_id} either: {e}")
                        })
                }
                Err(e) => Err(format!(
                    "the Boards route was not tried for #{pbi_id}: the access token could not be refreshed ({e})"
                )),
            };
            match attempt {
                // `boards_fallback` has already logged the plan and the
                // suite it ended on. Nothing is emitted: a suite that was
                // FOUND says nothing today either, and `PlanCreated` is the
                // documented create's own event - the plan this route made
                // is the team's sprint plan, which the log names.
                Ok((base_url, out)) => {
                    crate::ado_testplan::remember_suite(&base_url, &organization, &project, pbi_id, &out.suite);
                    resolved_suite = Some(out.suite);
                }
                // Tried once and never again: a fallback that retries is a
                // fallback nobody can diagnose. The first sentence stands
                // word for word - it is still the true reason - with one
                // line saying the second route did not work either. What
                // exactly went wrong is a job for the log, not for someone
                // who only wanted their cases in a suite.
                Err(why) => {
                    crate::applog::warn(why);
                    let _ = crate::events::SuiteNotCreated {
                        reason: format!(
                            "{sentence} The Boards route did not work either - Settings, Logs has what it said."
                        ),
                    }
                    .emit(&app);
                }
            }
        }
    }
    // Spec order in the suite and the suggested run order on the PBI, when
    // this upload created cases into a suite it knows. Best-effort by
    // design (§6): `results` is already final and nothing below changes it
    // or turns the upload into a failure - a miss is one toast each.
    if let Some(suite) = &resolved_suite {
        if results.iter().any(|r| r.action == "created") {
            let landed: Vec<crate::run_order::Landed> = results
                .iter()
                .filter(|r| r.action != "failed")
                .filter_map(|r| {
                    r.id.map(|id| crate::run_order::Landed {
                        index: r.index as usize,
                        id,
                        created: r.action == "created",
                    })
                })
                .collect();
            let notes = match get_fresh_token(&app).await {
                Ok(token) => {
                    let client = ado::AdoClient::new(token);
                    let saved_by = crate::commands::run_order::saved_by(&app);
                    crate::run_order::order_after_upload(
                        &client,
                        &organization,
                        &project,
                        pbi_id,
                        suite.suite_id,
                        &landed,
                        &queue,
                        &order_hint,
                        &saved_by,
                        crate::run_order::SETTLE_DELAY,
                    )
                    .await
                }
                Err(e) => {
                    crate::applog::warn(format!(
                        "suite {} for #{pbi_id} not ordered: the access token could not be refreshed ({e})",
                        suite.suite_id
                    ));
                    vec![format!("The spec order could not be set in Azure DevOps: {}", e.user_text())]
                }
            };
            for reason in notes {
                let _ = crate::events::RunOrderNotSaved { reason }.emit(&app);
            }
        }
    }
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
/// One case's place in a batch: the create document (with the PBI link
/// folded in, so a create is one request) or the update document, ready
/// to send. A case that fails validation never becomes a request.
#[allow(clippy::too_many_arguments)]
pub fn queue_item_request(
    client: &ado::AdoClient,
    organization: &str,
    project: &str,
    pbi_id: i32,
    tc: &model::TestCase,
    m_ref: Option<&str>,
    p_ref: Option<&str>,
    area_path: &str,
    iteration_path: &str,
    original_steps_xml: Option<&str>,
    original_tags: Option<&str>,
    temp_id: usize,
) -> Result<crate::ado::wit_batch::BatchRequest, String> {
    use crate::ado::wit_batch::{create_uri, temp_id_op, update_uri, BatchMethod, BatchRequest};
    tc.is_valid()?;
    Ok(match tc.update_id {
        Some(existing_id) => BatchRequest {
            method: BatchMethod::Patch,
            uri: update_uri(existing_id),
            body: serde_json::Value::Array(client.update_test_case_doc(
                tc,
                m_ref,
                p_ref,
                original_steps_xml,
                original_tags,
                ado::BlankPolicy::Skip,
            )),
        },
        // The batch API creates with PATCH against the type's URL, as its
        // own reference does - a POST there is refused. The document
        // starts with the batch-local temporary id (see `temp_id_op`).
        None => {
            let mut doc = vec![temp_id_op(temp_id)];
            doc.extend(client.create_test_case_doc(
                organization,
                project,
                tc,
                m_ref,
                area_path,
                iteration_path,
                p_ref,
                Some(pbi_id),
            ));
            BatchRequest {
                method: BatchMethod::Patch,
                uri: create_uri(project),
                body: serde_json::Value::Array(doc),
            }
        }
    })
}

/// What each answer in a batch means for its case. `sent_idx[k]` is the
/// queue index of the k-th request: a case that failed validation was never
/// sent, so answer k is NOT row `chunk_start + k`. `wit_batch` guarantees
/// exactly one item per request, in order.
pub fn map_batch_results(
    queue: &[model::TestCase],
    sent_idx: &[usize],
    items: &[crate::ado::wit_batch::BatchItem],
) -> Vec<SubmitItemResult> {
    sent_idx
        .iter()
        .zip(items)
        .map(|(&i, item)| {
            let tc = &queue[i];
            if !item.ok() {
                failed_item(i, tc, item.message())
            } else if let Some(id) = item.id() {
                SubmitItemResult {
                    index: i as u32,
                    title: tc.title.clone(),
                    action: if tc.update_id.is_some() { "updated" } else { "created" }.into(),
                    id: Some(id),
                    error: None,
                }
            } else {
                // A create with no id is not a create (see create_test_case):
                // report it, never call it success.
                failed_item(
                    i,
                    tc,
                    "Azure DevOps accepted the test case but its answer carried no work item id".into(),
                )
            }
        })
        .collect()
}

/// Shared by `match_reconciled` and `failed_batch_results`: the pairs, and
/// the trimmed titles that were too ambiguous to pair at all (see below).
///
/// Titles match exactly, ignoring only outer whitespace. A title is only
/// paired when the found items with it do not outnumber the creates with
/// it: if they do, a colleague's case (or a resend of a title from a
/// different chunk) may be sitting in that pool, and guessing which found
/// row is genuinely ours risks reporting the wrong one `created` while the
/// real create is quietly lost - so NONE of that title's creates are
/// claimed. Otherwise the server executed a batch in order, so the lowest
/// id with the title goes to the earliest create with it.
fn reconcile_pairs(
    creates: &[(usize, String)],
    found: &[(i32, String)],
) -> (Vec<(usize, i32)>, std::collections::HashSet<String>) {
    use std::collections::HashMap;
    let mut pools: HashMap<&str, Vec<i32>> = HashMap::new();
    for (id, t) in found {
        pools.entry(t.trim()).or_default().push(*id);
    }
    for ids in pools.values_mut() {
        ids.sort();
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (_, t) in creates {
        *counts.entry(t.trim()).or_default() += 1;
    }
    let ambiguous: std::collections::HashSet<String> = pools
        .iter()
        .filter(|(title, ids)| ids.len() > *counts.get(**title).unwrap_or(&0))
        .map(|(title, _)| (*title).to_string())
        .collect();
    let mut cursor: HashMap<&str, usize> = HashMap::new();
    let mut out = Vec::new();
    for (idx, title) in creates {
        let t = title.trim();
        if ambiguous.contains(t) {
            continue;
        }
        if let Some(ids) = pools.get(t) {
            let k = cursor.entry(t).or_insert(0);
            if let Some(&id) = ids.get(*k) {
                out.push((*idx, id));
                *k += 1;
            }
        }
    }
    (out, ambiguous)
}

/// Pair the creates of a failed batch with the Test Cases a lookup found.
/// `creates` is (queue index, title) in queue order; `found` is (id, title).
/// See `reconcile_pairs` for the exactly-one-title rule this applies.
pub fn match_reconciled(creates: &[(usize, String)], found: &[(i32, String)]) -> Vec<(usize, i32)> {
    reconcile_pairs(creates, found).0
}

fn log_claimed(pairs: &[(usize, i32)]) {
    if pairs.is_empty() {
        return;
    }
    crate::applog::info(format!(
        "reconcile matched {} case(s): {}",
        pairs.len(),
        pairs.iter().map(|(i, id)| format!("queue[{i}] -> #{id}")).collect::<Vec<_>>().join(", ")
    ));
}

/// How far before the upload's start the reconcile lookup reaches.
pub const SINCE_MARGIN_SECS: i64 = 300;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// UTC `YYYY-MM-DDTHH:MM:SSZ` - the form `created_since_wiql` takes.
pub fn iso_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = crate::applog::civil(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn unknown_item(index: usize, tc: &model::TestCase, error: &str) -> SubmitItemResult {
    SubmitItemResult {
        index: index as u32,
        title: tc.title.clone(),
        action: "unknown".into(),
        id: None,
        error: Some(format!(
            "Outcome unknown ({error}). Azure DevOps may have created this case - check before uploading it again."
        )),
    }
}

/// The results for a batch whose call failed. `found` is what the lookup
/// returned, or `None` when the lookup itself failed. Updates are always
/// `failed` (a PATCH is safe to retry). A create the lookup found - and
/// could pair unambiguously (`reconcile_pairs`) - is `created` with that
/// id. Any other create is `unknown` when there was no answer, when the
/// server may still be running the batch (`still_running`, after a
/// timeout), or when its title was too ambiguous to claim (something with
/// that title exists, so it is not a plain `failed`). Otherwise it is
/// `failed`.
pub fn failed_batch_results(
    queue: &[model::TestCase],
    sent_idx: &[usize],
    error: &str,
    found: Option<&[(i32, String)]>,
    still_running: bool,
) -> Vec<SubmitItemResult> {
    let creates: Vec<(usize, String)> = sent_idx
        .iter()
        .filter(|&&i| queue[i].update_id.is_none())
        .map(|&i| (i, queue[i].title.clone()))
        .collect();
    let (pairs, ambiguous) = found.map(|f| reconcile_pairs(&creates, f)).unwrap_or_default();
    log_claimed(&pairs);
    let matched: std::collections::HashMap<usize, i32> = pairs.into_iter().collect();
    sent_idx
        .iter()
        .map(|&i| {
            let tc = &queue[i];
            if tc.update_id.is_some() {
                failed_item(i, tc, error.to_string())
            } else if let Some(&id) = matched.get(&i) {
                SubmitItemResult {
                    index: i as u32,
                    title: tc.title.clone(),
                    action: "created".into(),
                    id: Some(id),
                    error: None,
                }
            } else if found.is_none() || still_running || ambiguous.contains(tc.title.trim()) {
                unknown_item(i, tc, error)
            } else {
                failed_item(i, tc, error.to_string())
            }
        })
        .collect()
}

/// A failed `$batch`, resolved: when it held creates, ask Azure DevOps what
/// it made (one lookup) and report per case. See `failed_batch_results`.
/// `already_claimed` is every work item id this upload has already reported
/// (earlier chunks, plus this chunk's own so far) - removed from what the
/// lookup found before matching, so a title repeated across chunks cannot
/// pair a later chunk's failed create with an earlier chunk's own success.
#[allow(clippy::too_many_arguments)]
pub async fn resolve_failed_batch(
    client: &ado::AdoClient,
    organization: &str,
    project: &str,
    pbi_id: i32,
    since: &str,
    queue: &[model::TestCase],
    sent_idx: &[usize],
    err: &ado::AdoError,
    already_claimed: &[i32],
) -> Vec<SubmitItemResult> {
    // `user_text`, not `to_string`: a refused batch's own sentence, not
    // "http 0" (main's failure-list fix).
    let msg = err.user_text();
    let still_running = matches!(err, ado::AdoError::Network(m) if m == ado::NET_TIMEOUT);
    let titles: Vec<String> = sent_idx
        .iter()
        .filter(|&&i| queue[i].update_id.is_none())
        .map(|&i| queue[i].title.clone())
        .collect();
    if titles.is_empty() {
        return failed_batch_results(queue, sent_idx, &msg, Some(&[]), false);
    }
    let found = match client
        .find_created_test_cases(organization, project, pbi_id, since, &titles)
        .await
    {
        Ok(f) => {
            let f: Vec<(i32, String)> =
                f.into_iter().filter(|(id, _)| !already_claimed.contains(id)).collect();
            crate::applog::info(format!(
                "batch failed ({msg}); {} of {} create(s) found in Azure DevOps",
                f.len(),
                titles.len()
            ));
            Some(f)
        }
        Err(e) => {
            crate::applog::warn(format!(
                "batch failed ({msg}) and the lookup of what it created failed too ({e}) - {} create(s) held as unknown",
                titles.len()
            ));
            None
        }
    };
    failed_batch_results(queue, sent_idx, &msg, found.as_deref(), still_running)
}

/// One create the reconcile lookup found: the title it was queued under and
/// the work item it became.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ReconciledCase {
    pub title: String,
    pub id: i32,
}

/// The answer to a Check. `found` is an unambiguous match, treated exactly
/// like a created case. `ambiguous` lists checked titles that had more
/// unclaimed matches in Azure DevOps than rows being checked - which one is
/// genuinely this upload's cannot be told apart (see `reconcile_pairs`), so
/// that row is neither cleared nor claimed: it stays held. Any title in
/// neither list was not found at all, and its hold is lifted.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ReconcileAnswer {
    pub found: Vec<ReconciledCase>,
    pub ambiguous: Vec<String>,
}

/// The lookup behind `reconcile_upload`, with the client passed in.
/// `titles` has one entry per held row (repeats allowed); each found case
/// is paired once (`reconcile_pairs`). `exclude_ids` - the ids the hold
/// recorded (the PBI's cases before the upload, and every id the upload
/// reported) plus every update id in the queue - are dropped from what the
/// lookup found before the exactly-one rule is applied.
pub async fn reconcile_with(
    client: &ado::AdoClient,
    organization: &str,
    project: &str,
    pbi_id: i32,
    since: &str,
    titles: &[String],
    exclude_ids: &[i32],
) -> Result<ReconcileAnswer, String> {
    if ado::endpoints::wiql_datetime(since).is_none() {
        return Err("The time to check from is not a valid date.".into());
    }
    // Removed before pairing, as `resolve_failed_batch` does with the ids
    // it already reported: an id this upload already stamped on another
    // row, or one linked before the upload began, is never this row's.
    let found: Vec<(i32, String)> = client
        .find_created_test_cases(organization, project, pbi_id, since, titles)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|(id, _)| !exclude_ids.contains(id))
        .collect();
    let creates: Vec<(usize, String)> = titles.iter().cloned().enumerate().collect();
    let (pairs, ambiguous) = reconcile_pairs(&creates, &found);
    log_claimed(&pairs);
    Ok(ReconcileAnswer {
        found: pairs.into_iter().map(|(k, id)| ReconciledCase { title: titles[k].clone(), id }).collect(),
        ambiguous: ambiguous.into_iter().collect(),
    })
}

/// Check what an interrupted upload created: the rows the queue holds as
/// "outcome unknown" (C2). Read only.
#[tauri::command]
#[specta::specta]
pub async fn reconcile_upload(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    since: String,
    titles: Vec<String>,
    exclude_ids: Vec<i32>,
) -> Result<ReconcileAnswer, String> {
    let token = get_fresh_token(&app).await.map_err(|e| e.to_string())?;
    let client = ado::AdoClient::new(token);
    reconcile_with(&client, &organization, &project, pbi_id, &since, &titles, &exclude_ids).await
}

fn failed_item(index: usize, tc: &model::TestCase, error: String) -> SubmitItemResult {
    SubmitItemResult {
        index: index as u32,
        title: tc.title.clone(),
        action: "failed".into(),
        id: None,
        error: Some(error),
    }
}
