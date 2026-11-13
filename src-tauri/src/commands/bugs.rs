//! Bug filing from failures, plus the capture/attachment helpers the
//! runner uses to feed it.

use crate::state::get_fresh_token;
use crate::ado;

pub mod work_bug {
    #[derive(Debug, serde::Serialize, specta::Type)]
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

/// The fields a Bug of this project's process must be given beyond its
/// title and repro steps.
#[derive(serde::Serialize, specta::Type)]
pub struct BugFields {
    pub wi_type: String,
    pub fields: Vec<crate::work_board::ExtraField>,
}

/// One value the tester gave for a Bug field.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct BugFieldValue {
    pub reference_name: String,
    pub value: String,
}

/// Why a bug was not filed: the process rules named fields (nothing was
/// created), or Azure DevOps / the network failed some other way.
#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(tag = "kind", content = "detail")]
pub enum FileBugError {
    RuleViolations(Vec<ado::rule_errors::FieldViolation>),
    Ado(ado::AdoError),
}

impl From<ado::AdoError> for FileBugError {
    fn from(e: ado::AdoError) -> Self {
        if let ado::AdoError::Http { status, body } = &e {
            if (400..500).contains(status) {
                let v = ado::rule_errors::rule_violations(body);
                if !v.is_empty() {
                    return FileBugError::RuleViolations(v);
                }
            }
        }
        FileBugError::Ado(e)
    }
}

/// The fields the Bug dialog must ask for: the type's required fields that
/// ADO does not fill, plus `extra_refs` (fields a rejection named).
#[tauri::command]
#[specta::specta]
pub async fn bug_fields(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    extra_refs: Vec<String>,
) -> Result<BugFields, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let info = client.detect_bug_type(&organization, &project).await?;
    let fields = client
        .bug_field_definitions(&organization, &project, &info.wi_type, &info.repro_field, &extra_refs)
        .await?;
    Ok(BugFields { wi_type: info.wi_type, fields })
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
    field_values: Vec<BugFieldValue>,
) -> Result<work_bug::FiledBug, FileBugError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    file_bug_with(
        &client,
        &organization,
        &project,
        &title,
        &repro_text,
        test_case_id,
        pbi_id,
        &screenshots_b64,
        &field_values,
    )
    .await
}

/// The body of `file_bug`, testable against a mock server. Blank values,
/// the title and repro fields, and anything that is not a plain dotted
/// field reference are dropped: a value can never steer the JSON-patch path
/// or override what the dialog itself supplies.
pub async fn file_bug_with(
    client: &ado::AdoClient,
    organization: &str,
    project: &str,
    title: &str,
    repro_text: &str,
    test_case_id: i32,
    pbi_id: i32,
    screenshots_b64: &[String],
    field_values: &[BugFieldValue],
) -> Result<work_bug::FiledBug, FileBugError> {
    let info = client.detect_bug_type(organization, project).await?;
    let mut fields = vec![
        ("System.Title".to_string(), title.to_string()),
        (info.repro_field.clone(), repro_html(repro_text)),
    ];
    for v in field_values {
        let r = v.reference_name.as_str();
        if v.value.trim().is_empty()
            || r == "System.Title"
            || r == info.repro_field
            || !ado::rule_errors::is_plain_ref(r)
            || fields.iter().any(|(have, _)| have == r)
        {
            continue;
        }
        fields.push((r.to_string(), v.value.clone()));
    }
    let (id, url) = client
        .create_work_item(organization, project, &info.wi_type, &fields, &[test_case_id, pbi_id], None)
        .await?;
    // The bug itself exists from here on, so a screenshot that fails must
    // not fail the call - re-filing would leave a duplicate. Count them
    // instead: a tester who attached three screenshots of a failure needs
    // to know when none of them arrived.
    let failed = attach_screenshots(client, organization, project, id, screenshots_b64).await;
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
    // Look before reading: a folder, or a file over the limit, is refused
    // without being loaded, and neither sentence names the path.
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        return Err("Folders can't be attached - attach the files inside it.".into());
    }
    if meta.len() > 25 * 1024 * 1024 {
        return Err("File is larger than 25 MB.".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let file_name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "attachment".into());
    Ok(RunAttachmentOut {
        file_name,
        b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

/// Write `bytes` to `dest` through a unique `.part` file and a rename, so a
/// failure never leaves half a file at `dest`.
pub fn write_file_atomically(dest: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let part = ado::transport::temp_part_path(dest);
    let guard = ado::transport::PartGuard(part.clone());
    let result = std::fs::write(&part, bytes).and_then(|()| std::fs::rename(&part, dest));
    if result.is_ok() {
        guard.disarm();
    }
    result
}

/// Write a file the page holds as base64 (a recording from this session)
/// to a path the person chose.
#[tauri::command(async)]
#[specta::specta]
pub fn save_attachment_b64(dest_path: String, b64: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|_| "The file data is damaged.".to_string())?;
    write_file_atomically(std::path::Path::new(&dest_path), &bytes).map_err(|e| {
        crate::applog::warn(format!("saving a session file failed: {e}"));
        format!("Could not save the file: {e}")
    })
}

/// Launch the Windows snipping overlay (result lands on the clipboard; the
/// runner polls and attaches it).
#[tauri::command]
#[specta::specta]
pub fn open_snip() -> Result<(), String> {
    tauri_plugin_opener::open_url("ms-screenclip:", None::<&str>).map_err(|e| e.to_string())
}
