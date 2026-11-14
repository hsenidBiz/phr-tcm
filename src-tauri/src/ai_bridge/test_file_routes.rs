//! A project's Test files: the listing the accounts answer carries, and `save_test_file`.

use super::*;

/// This project's Test files - names and sizes, for an assistant to pick
/// from. One that cannot be read is logged by `test_files::list` and reads
/// as none.
pub(crate) fn project_test_files(root: &std::path::Path, ctx: &BridgeContext) -> Vec<crate::test_files::TestFile> {
    if ctx.org.trim().is_empty() || ctx.project.trim().is_empty() {
        return Vec::new();
    }
    crate::test_files::list(&crate::test_files::folder(root, &ctx.org, &ctx.project)).unwrap_or_default()
}

/// Said in `get_accounts`' answer, in place of the Test files, with no
/// project open: Test files belong to one.
pub const TEST_FILES_NEED_A_PROJECT: &str = "open a project in the app first - Test files belong to a project";

/// This project's Test files for `get_accounts`' answer, each by name and
/// human size and nothing else - never a path or a date. `Err` is the
/// sentence said in their place: no project open, or a folder that could
/// not be read.
pub(crate) fn test_files_answer(root: &std::path::Path, ctx: &BridgeContext) -> Result<Vec<serde_json::Value>, String> {
    if ctx.org.trim().is_empty() || ctx.project.trim().is_empty() {
        return Err(TEST_FILES_NEED_A_PROJECT.to_string());
    }
    // `list`'s own sentence already says what failed and where to look.
    let files = crate::test_files::list(&crate::test_files::folder(root, &ctx.org, &ctx.project))?;
    Ok(files
        .into_iter()
        .map(|f| serde_json::json!({ "name": f.name, "size": crate::test_files::human_size(u64::from(f.size)) }))
        .collect())
}

/// Said when `save_test_file` is given no content, or more than one.
pub const TEST_FILE_ONE_SOURCE: &str = "Give the file's content as exactly one of text, base64 or from_path.";

/// Said when `save_test_file` is given no name and no `from_path` to take it from.
pub const TEST_FILE_NEEDS_A_NAME: &str = "Give the file a name, or a from_path to take it from.";

/// Said when `base64` is not standard base64.
pub const TEST_FILE_BAD_BASE64: &str = "base64 is not standard base64 - encode the file's bytes and save it again.";

/// Said when `from_path` is not a plain local absolute path: a relative one, a network share
/// (`\\server\share`) or a device path (`\\?\`, `\\.\`).
pub const TEST_FILE_PATH_NOT_LOCAL: &str = "from_path must be a file on this computer's own drives.";

/// `POST /test-file-save`: a file the assistant made, saved into the project's Test files
/// (`test_files::save_bytes`). Exactly one of `text`, `base64` or `from_path` carries the
/// content. Every refusal is a 400 with a sentence that names the file and no path; the
/// raw error goes to the app log. It writes only inside the project's Test files folder.
pub(crate) fn test_file_save(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use base64::Engine as _;
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (400, "that is not readable JSON.".to_string()),
    };
    let text_of = |key: &str| match &v[key] {
        serde_json::Value::String(s) => Some(s.clone()),
        _ => None,
    };
    let (text, b64, from_path) = (text_of("text"), text_of("base64"), text_of("from_path"));
    if [text.is_some(), b64.is_some(), from_path.is_some()].iter().filter(|x| **x).count() != 1 {
        return (400, TEST_FILE_ONE_SOURCE.to_string());
    }
    let replace = v["replace"].as_bool() == Some(true);
    let malformed = v["malformed"].as_bool() == Some(true);
    // Judged on the text alone, before any file system call: a share is never opened.
    if from_path.as_deref().is_some_and(|p| !crate::test_files::is_plain_local_path(p)) {
        return (400, TEST_FILE_PATH_NOT_LOCAL.to_string());
    }
    if ctx.org.trim().is_empty() || ctx.project.trim().is_empty() {
        return (400, TEST_FILES_NEED_A_PROJECT.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let folder = crate::test_files::folder(&root, &ctx.org, &ctx.project);
    let given_name = text_of("name").map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let name = match (&given_name, &from_path) {
        (Some(n), _) => n.clone(),
        (None, Some(p)) => crate::test_files::picked_name(std::path::Path::new(p)),
        (None, None) => return (400, TEST_FILE_NEEDS_A_NAME.to_string()),
    };
    let bytes = if let Some(t) = text {
        t.into_bytes()
    } else if let Some(b) = b64 {
        match base64::engine::general_purpose::STANDARD.decode(b.trim()) {
            Ok(bytes) => bytes,
            Err(_) => return (400, TEST_FILE_BAD_BASE64.to_string()),
        }
    } else {
        let path = std::path::PathBuf::from(from_path.unwrap_or_default());
        // The source is itself a kind of file a test uploads, and the saved name keeps its
        // extension: a file cannot be copied in under another kind's name.
        let source_name = crate::test_files::picked_name(&path);
        if !crate::test_files::is_uploadable(&source_name) {
            return (400, crate::test_files::not_uploadable(&source_name));
        }
        let ext = crate::test_files::extension_of(&source_name).unwrap_or_default();
        if crate::test_files::extension_of(&name).as_deref() != Some(ext.as_str()) {
            return (400, format!("The saved name must keep the file's own extension (.{ext})."));
        }
        match std::fs::metadata(&path) {
            Ok(m) if m.is_file() => {}
            Ok(_) => return (400, format!("\"{name}\" is not a file")),
            Err(e) => {
                crate::applog::warn(format!("test files: \"{name}\" could not be read for the assistant: {e}"));
                return (400, format!("\"{name}\" could not be read - check the path and save it again"));
            }
        }
        match crate::test_files::read_capped(&path, &name) {
            Ok(bytes) => bytes,
            Err(why) => return (400, why),
        }
    };
    let replaced = replace && crate::test_files::resolve(&folder, &name).is_ok_and(|p| p.exists());
    match crate::test_files::save_bytes(&folder, &name, &bytes, replace, malformed) {
        Ok(saved) => (
            200,
            serde_json::json!({
                "saved": saved.name,
                "size": crate::test_files::human_size(u64::from(saved.size)),
                "replaced": replaced,
                "use": "Refer to it by its name: an Auto Run upload action's file, or a template's files.",
            })
            .to_string(),
        ),
        Err(why) => (400, why),
    }
}

