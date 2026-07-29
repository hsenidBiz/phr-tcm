//! Bug filing from failures, plus the capture/attachment helpers the
//! runner uses to feed it.

use crate::state::get_fresh_token;
use crate::{ado, capture};

pub mod work_bug {
    #[derive(serde::Serialize, specta::Type)]
    pub struct FiledBug {
        pub id: i32,
        pub url: String,
        /// Screenshots that did NOT make it onto the bug. The work item is
        /// created first and must never be re-filed over a failed upload,
        /// so the only way the tester learns their evidence is missing is
        /// if we say so here.
        pub screenshots_failed: i32,
        pub screenshots_total: i32,
    }
}

#[derive(serde::Serialize, specta::Type)]
pub struct RunAttachmentOut {
    pub file_name: String,
    pub b64: String,
}

/// File a Bug (or Issue on Basic-process projects) for a failed case:
/// Related links to the test case + PBI, screenshots attached. POST only.
#[tauri::command]
#[specta::specta]
pub async fn file_bug(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    title: String,
    repro_text: String,
    test_case_id: i32,
    pbi_id: i32,
    screenshots_b64: Vec<String>,
) -> Result<work_bug::FiledBug, String> {
    use base64::Engine;
    let token = get_fresh_token(&app).await.map_err(|e| e.to_string())?;
    let client = ado::AdoClient::new(token);
    let info = client
        .detect_bug_type(&organization, &project)
        .await
        .map_err(|e| e.to_string())?;
    let repro_html = format!("<div>{}</div>", repro_text.replace('\n', "<br>"));
    let fields = vec![
        ("System.Title".to_string(), title),
        (info.repro_field.clone(), repro_html),
    ];
    let (id, url) = client
        .create_work_item(&organization, &project, &info.wi_type, &fields, &[test_case_id, pbi_id], None)
        .await
        .map_err(|e| e.to_string())?;
    // The bug itself exists from here on, so a screenshot that fails must
    // not fail the call - re-filing would leave a duplicate. Count them
    // instead: a tester who attached three screenshots of a failure needs
    // to know when none of them arrived.
    let mut failed = 0;
    for (i, b64) in screenshots_b64.iter().enumerate() {
        let name = format!("bug-{}-{}.png", id, i + 1);
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else {
            crate::applog::warn(format!("bug {id}: screenshot {name} is not valid base64"));
            failed += 1;
            continue;
        };
        match client
            .upload_wi_attachment(&organization, &project, &name, bytes)
            .await
        {
            Ok(att_url) => {
                if let Err(e) = client
                    .add_wi_attachment_relation(&organization, &project, id, &att_url)
                    .await
                {
                    crate::applog::warn(format!("bug {id}: {name} uploaded but not linked: {e}"));
                    failed += 1;
                }
            }
            Err(e) => {
                crate::applog::warn(format!("bug {id}: {name} did not upload: {e}"));
                failed += 1;
            }
        }
    }
    Ok(work_bug::FiledBug {
        id,
        url,
        screenshots_failed: failed,
        screenshots_total: screenshots_b64.len() as i32,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn capture_screens() -> Result<Vec<capture::ScreenShot>, String> {
    tauri::async_runtime::spawn_blocking(capture::capture_all_monitors)
        .await
        .map_err(|e| e.to_string())?
}

/// Read any file for attaching to a result (name + base64 bytes).
#[tauri::command]
#[specta::specta]
pub fn read_file_b64(path: String) -> Result<RunAttachmentOut, String> {
    use base64::Engine;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.len() > 25 * 1024 * 1024 {
        return Err("File is larger than 25 MB.".into());
    }
    let file_name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "attachment".into());
    Ok(RunAttachmentOut {
        file_name,
        b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

/// Launch the Windows snipping overlay (result lands on the clipboard; the
/// runner polls and attaches it).
#[tauri::command]
#[specta::specta]
pub fn open_snip() -> Result<(), String> {
    tauri_plugin_opener::open_url("ms-screenclip:", None::<&str>).map_err(|e| e.to_string())
}
