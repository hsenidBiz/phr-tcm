//! Bug filing from failures, plus the capture/attachment helpers the
//! runner uses to feed it.

use crate::state::get_fresh_token;
use crate::ado;

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

/// The repro steps as the HTML the Bug's repro field takes.
///
/// The text is built from the case's steps, and step text is Markdown inside
/// TCM, so `**Save**` has to arrive as bold and not as asterisks. The repro
/// field is a full rich-text field, so it takes the same HTML as the
/// prerequisites: paragraphs, line breaks, bold, italic, lists and code. A
/// single newline is a line break, so plain text reads as it was typed, and
/// anything in angle brackets stays text.
pub fn repro_html(repro_text: &str) -> String {
    crate::case_markdown::prereq_md_to_html(repro_text)
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
    let token = get_fresh_token(&app).await.map_err(|e| e.to_string())?;
    let client = ado::AdoClient::new(token);
    let info = client
        .detect_bug_type(&organization, &project)
        .await
        .map_err(|e| e.to_string())?;
    let fields = vec![
        ("System.Title".to_string(), title),
        (info.repro_field.clone(), repro_html(&repro_text)),
    ];
    let (id, url) = client
        .create_work_item(&organization, &project, &info.wi_type, &fields, &[test_case_id, pbi_id], None)
        .await
        .map_err(|e| e.to_string())?;
    // The bug itself exists from here on, so a screenshot that fails must
    // not fail the call - re-filing would leave a duplicate. Count them
    // instead: a tester who attached three screenshots of a failure needs
    // to know when none of them arrived.
    let failed = attach_screenshots(&client, &organization, &project, id, &screenshots_b64).await;
    Ok(work_bug::FiledBug {
        id,
        url,
        screenshots_failed: failed,
        screenshots_total: screenshots_b64.len() as i32,
    })
}

/// Why a picture is not fit to put on a bug, or the file extension its
/// bytes call for. Only a PNG, JPEG, GIF, BMP or WebP that is whole - its
/// signature at the front and its end at the back - passes: an empty paste
/// (a clipboard that offered the image before it was ready) and the front
/// half of a picture are both refused, because Azure DevOps would take the
/// file and show a broken image. A look at the bytes, not a decode: the
/// structure is what a cut-off transfer breaks.
pub fn bug_image_ext(bytes: &[u8]) -> Result<&'static str, &'static str> {
    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.is_empty() {
        return Err("the image is empty");
    }
    if bytes.starts_with(&PNG) {
        // IEND is the last chunk: length(4) "IEND" crc(4).
        let tail = &bytes[bytes.len().saturating_sub(16)..];
        let whole = bytes.len() >= PNG.len() + 12 && tail.windows(4).any(|w| w == b"IEND");
        return if whole { Ok("png") } else { Err("the PNG image is cut off") };
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        let end = bytes.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
        let whole = bytes[..end].ends_with(&[0xff, 0xd9]);
        return if whole { Ok("jpg") } else { Err("the JPEG image is cut off") };
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return if bytes.last() == Some(&0x3b) { Ok("gif") } else { Err("the GIF image is cut off") };
    }
    if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        // The file size the header declares; a few writers leave it 0.
        let declared = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
        return if declared <= bytes.len() { Ok("bmp") } else { Err("the BMP image is cut off") };
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        let declared = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        return if declared.saturating_add(8) <= bytes.len() {
            Ok("webp")
        } else {
            Err("the WebP image is cut off")
        };
    }
    Err("the data is not a PNG, JPEG, GIF, BMP or WebP image")
}

/// Upload each pasted or captured screenshot to the bug and link it.
/// Returns how many did NOT make it. An empty, cut-off or undecodable
/// image is refused here, whatever the webview let through, and counted as
/// failed - never uploaded and never counted as a success. POST (the
/// upload) and PATCH (the link) only.
pub async fn attach_screenshots(
    client: &ado::AdoClient,
    organization: &str,
    project: &str,
    id: i32,
    screenshots_b64: &[String],
) -> i32 {
    use base64::Engine;
    let mut failed = 0;
    for (i, b64) in screenshots_b64.iter().enumerate() {
        let n = i + 1;
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.trim()) else {
            crate::applog::warn(format!("bug {id}: screenshot {n} is not valid base64"));
            failed += 1;
            continue;
        };
        let ext = match bug_image_ext(&bytes) {
            Ok(ext) => ext,
            Err(why) => {
                crate::applog::warn(format!("bug {id}: screenshot {n} was not uploaded - {why}"));
                failed += 1;
                continue;
            }
        };
        let name = format!("bug-{id}-{n}.{ext}");
        match client.upload_wi_attachment(organization, project, &name, bytes).await {
            Ok(att_url) => {
                if let Err(e) = client
                    .add_wi_attachment_relation(organization, project, id, &att_url)
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
    failed
}

/// Read any file for attaching to a result (name + base64 bytes).
#[tauri::command(async)]
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
