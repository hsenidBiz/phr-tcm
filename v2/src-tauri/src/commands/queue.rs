//! The test-case queue: import, export, HTML views, and the serial
//! create/update submit loop with its cooperative cancel.

use tauri::Manager;
use tauri_specta::Event;

use crate::events::{CaseNoteSaved, PlanCreated, SubmitProgress};
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

#[tauri::command]
#[specta::specta]
pub fn export_queue(path: String, queue: Vec<model::TestCase>) -> Result<(), String> {
    import_parser::export_queue_to_excel(&queue, &path)
}

#[tauri::command]
#[specta::specta]
pub fn export_queue_json(path: String, queue: Vec<model::TestCase>) -> Result<(), String> {
    import_parser::export_queue_to_json(&queue, &path)
}

#[tauri::command]
#[specta::specta]
pub fn export_queue_html(path: String, queue: Vec<model::TestCase>, subtitle: String) -> Result<(), String> {
    // A saved-to-disk export is shared/archived - no autosaving note boxes.
    import_parser::export_queue_to_html(&queue, &path, &subtitle, None)
}

#[tauri::command]
#[specta::specta]
pub fn write_template(path: String) -> Result<(), String> {
    import_parser::generate_template(&path)
}

#[derive(serde::Serialize, specta::Type)]
pub struct SharedQueue {
    /// The PBI the sender drafted against - the frontend warns when it
    /// differs from the recipient's current selection.
    pub pbi_id: i32,
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
    let (json, revoke_warning) = ado::AdoClient::new(token).take_shared_draft(&share).await?;
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
    let (cases, mut warnings) = parsed?;
    if let Some(w) = revoke_warning {
        warnings.push(w);
    }
    crate::applog::info(format!(
        "Imported a shared draft: {} case(s) for PBI #{} (link revoked)",
        cases.len(),
        share.pbi_id
    ));
    Ok(SharedQueue {
        pbi_id: share.pbi_id,
        organization: share.org,
        project: share.project,
        cases,
        warnings,
    })
}

/// One note listener per app run, started lazily on the first report.
fn ensure_note_server(app: &tauri::AppHandle) -> Option<u16> {
    static PORT: std::sync::OnceLock<Option<u16>> = std::sync::OnceLock::new();
    *PORT.get_or_init(|| {
        let app = app.clone();
        note_server::start(move |n| {
            let _ = CaseNoteSaved { org: n.org, case_id: n.case_id, text: n.text }.emit(&app);
        })
        .ok() // no listener -> report still opens, comments just can't save
    })
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
) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!(
        "test-cases-{}-{}.html",
        std::process::id(),
        queue.len()
    ));
    let path_str = path.to_string_lossy().to_string();
    let note_ctx = (!organization.is_empty())
        .then(|| ensure_note_server(&app))
        .flatten()
        .map(|port| import_parser::NoteCtx { port, org: organization, notes });
    import_parser::export_queue_to_html(&queue, &path_str, &subtitle, note_ctx.as_ref())?;
    tauri_plugin_opener::open_path(&path_str, None::<&str>).map_err(|e| e.to_string())
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
    // Arm the cancel flag BEFORE any awaits: the suite-resolution phase
    // below can take seconds, and a Cancel clicked during it must stick
    // (resetting later would silently swallow it and run the whole queue).
    let cancel = app.state::<SubmitCancel>();
    cancel.0.store(false, std::sync::atomic::Ordering::SeqCst);

    // Best-effort board visibility (ported from v1 CreationWorker._ensure_suite):
    // make sure the PBI's requirement-based suite exists before creating, so
    // linked cases surface on the board's test count. Failures never block
    // creation. The PBI's own paths double as the default area/iteration for
    // created cases (unset picker = "Same as PBI").
    let mut pbi_area = String::new();
    let mut pbi_iteration = String::new();
    if let Ok(token) = get_fresh_token(&app).await {
        let client = ado::AdoClient::new(token);
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
            .update_test_case_from_model(organization, project, existing_id, tc, m_ref, p_ref)
            .await
            .map(|_| (existing_id, "updated")),
        None => match client
            .create_test_case(organization, project, tc, m_ref, area_path, iteration_path, p_ref)
            .await
        {
            Ok(new_id) => client
                .link_to_pbi(organization, project, new_id, pbi_id)
                .await
                .map(|_| (new_id, "created")),
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
