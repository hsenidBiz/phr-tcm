//! The project's Test files: documents a test uploads into the application
//! under test - from an API template's form step (`files`) or an Auto Run
//! case script's `upload` action.
//!
//! They live on this machine only, one folder per organization and project:
//! `<Auto Run data>/test-files/<project slug>/`, the same slug templates,
//! flows and sign-in recipes use (`autorun::recipe::project_slug`). A
//! template or a script refers to a file by its NAME, never by a path, so
//! the same script runs on every machine that has a file of that name in
//! its Test files. Nothing here sends a file anywhere: the upload itself is
//! the browser's.
//!
//! Every sentence a person reads names the file, never a full path; the raw
//! error goes to the app log.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The largest test file: checked when one is added, and again on what is
/// actually read for a run - a file swapped in the folder by hand is held
/// to the same cap.
pub const MAX_BYTES: u64 = 25 * 1024 * 1024;

/// The longest file name, in characters.
pub const MAX_NAME_CHARS: usize = 120;

/// Where the person adds and removes test files, for a sentence.
pub const WHERE: &str = "Test files (Auto Run or API Templates)";

/// One file in a project's Test files.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct TestFile {
    pub name: String,
    /// Bytes. A `u32` (specta refuses a 64-bit number across IPC); a file
    /// over 4 GB - never one this app accepted - reads as `u32::MAX`.
    pub size: u32,
    /// Epoch milliseconds as a string, like a run's `started_at`; empty when
    /// the file system would not say.
    pub modified: String,
}

/// Windows' reserved device names: no file may be called one, with or
/// without an extension (`CON`, `con.txt`).
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A test file name: one file name, never a path. Non-empty and at most
/// `MAX_NAME_CHARS` characters; none of `/ \ : * ? " < > |` and no control
/// character; not `.` or `..`; no leading or trailing space or dot; and not
/// a Windows device name, with or without an extension.
pub fn valid_test_file_name(name: &str) -> bool {
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return false;
    }
    if name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return false;
    }
    if name == "." || name == ".." {
        return false;
    }
    let edge = |c: Option<char>| matches!(c, Some(' ' | '.'));
    if edge(name.chars().next()) || edge(name.chars().last()) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    !RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r))
}

/// Why `name` is not a test file name, as a sentence naming it.
pub fn bad_name(name: &str) -> String {
    format!(
        "\"{name}\" cannot be a test file name - use a plain file name of up to {MAX_NAME_CHARS} characters, \
         without / \\ : * ? \" < > | and not starting or ending with a space or a dot"
    )
}

/// This project's Test files folder. Not created here: `list` reads a
/// missing one as empty, and `add` and the open-folder command create it.
pub fn folder(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("test-files").join(crate::autorun::recipe::project_slug(org, project))
}

/// `folder.join(name)`, only for a name `valid_test_file_name` accepts.
pub fn resolve(folder: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_test_file_name(name) {
        return Err(bad_name(name));
    }
    Ok(folder.join(name))
}

/// The content type a file is sent with, from its extension. A short fixed
/// table: anything else goes as `application/octet-stream`.
pub fn content_type(name: &str) -> &'static str {
    let ext = match name.rsplit_once('.') {
        Some((_, ext)) => ext.to_ascii_lowercase(),
        None => return "application/octet-stream",
    };
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// A size as a sentence shows it: `512 bytes`, `1.5 KB`, `3.2 MB`.
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if bytes == 1 {
        "1 byte".to_string()
    } else if b < KB {
        format!("{bytes} bytes")
    } else if b < MB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / MB)
    }
}

/// The sentence for a file a template step or a script needs and this
/// machine's Test files does not have. `who` says what uploads it: `the
/// step "Attach"`, or `this step`.
pub fn missing(name: &str, who: &str) -> String {
    format!("add \"{name}\" to {WHERE} - {who} uploads it")
}

/// The sentence for a file over the cap.
pub fn too_big(name: &str) -> String {
    format!("\"{name}\" is larger than 25 MB - a test file can be at most 25 MB")
}

/// Logs `e` (with the file's name, never its folder) and gives the sentence
/// a person reads.
fn failed(what: &str, name: &str, e: std::io::Error) -> String {
    crate::applog::warn(format!("test files: {what} \"{name}\": {e}"));
    format!("\"{name}\" could not be {what} - see Settings, Logs")
}

fn modified_ms(meta: &std::fs::Metadata) -> String {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()
}

fn entry(name: &str, meta: &std::fs::Metadata) -> TestFile {
    TestFile { name: name.to_string(), size: u32::try_from(meta.len()).unwrap_or(u32::MAX), modified: modified_ms(meta) }
}

/// Every test file in `folder`, by name (ignoring case). A missing folder
/// is an empty list. Only plain files with a usable name are listed:
/// anything else in the folder (a sub-folder, a half-written copy) is not a
/// test file a template or a script could name.
pub fn list(folder: &Path) -> Result<Vec<TestFile>, String> {
    let entries = match std::fs::read_dir(folder) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            crate::applog::warn(format!("test files: the folder could not be listed: {e}"));
            return Err("the Test files folder could not be read - see Settings, Logs".to_string());
        }
    };
    let mut out: Vec<TestFile> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let meta = e.metadata().ok()?;
            (meta.is_file() && valid_test_file_name(&name)).then(|| entry(&name, &meta))
        })
        .collect();
    out.sort_by_key(|f| f.name.to_lowercase());
    Ok(out)
}

/// The name a file a person picked would be listed under.
pub fn picked_name(source: &Path) -> String {
    source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Copies the file at `source` into `folder` under its own name. Refused,
/// each with a sentence naming the file: a name `valid_test_file_name`
/// refuses, a file over `MAX_BYTES`, or - unless `replace` - a file of that
/// name already there (the screen asks the person first). The copy is
/// written beside its final name and moved into place, so a reader never
/// sees half of it; the size cap is checked on what is actually read.
pub fn add(folder: &Path, source: &Path, replace: bool) -> Result<TestFile, String> {
    let name = picked_name(source);
    let dest = resolve(folder, &name)?;
    if dest.exists() && !replace {
        return Err(format!("\"{name}\" is already in Test files - replace it, or rename your copy first"));
    }
    let meta = std::fs::metadata(source).map_err(|e| failed("read", &name, e))?;
    if !meta.is_file() {
        return Err(format!("\"{name}\" is not a file"));
    }
    if meta.len() > MAX_BYTES {
        return Err(too_big(&name));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(source)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| failed("read", &name, e))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_big(&name));
    }
    std::fs::create_dir_all(folder).map_err(|e| failed("added", &name, e))?;
    // A leading dot is not a test file name, so `list` never shows this.
    let part = folder.join(format!(".{name}.part"));
    if let Err(e) = std::fs::write(&part, &bytes) {
        let _ = std::fs::remove_file(&part);
        return Err(failed("added", &name, e));
    }
    if let Err(e) = std::fs::rename(&part, &dest) {
        let _ = std::fs::remove_file(&part);
        return Err(failed("added", &name, e));
    }
    let meta = std::fs::metadata(&dest).map_err(|e| failed("read", &name, e))?;
    crate::applog::info(format!("test files: added \"{name}\" ({})", human_size(meta.len())));
    Ok(entry(&name, &meta))
}

/// Deletes this machine's copy of `name` - a local file, nothing else.
pub fn remove(folder: &Path, name: &str) -> Result<(), String> {
    let path = resolve(folder, name)?;
    match std::fs::remove_file(&path) {
        Ok(()) => {
            crate::applog::info(format!("test files: removed \"{name}\""));
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(format!("\"{name}\" is no longer in Test files")),
        Err(e) => Err(failed("removed", name, e)),
    }
}

/// A file a run is about to upload: where it is and how big. Checked before
/// anything is touched - a missing one gets `missing`'s sentence (`who` is
/// what uploads it), one over the cap `too_big`'s.
pub fn check_for_run(folder: &Path, name: &str, who: &str) -> Result<(PathBuf, u64), String> {
    let path = resolve(folder, name)?;
    match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() => {
            if meta.len() > MAX_BYTES {
                Err(too_big(name))
            } else {
                Ok((path, meta.len()))
            }
        }
        Ok(_) => Err(missing(name, who)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(missing(name, who)),
        Err(e) => Err(failed("read", name, e)),
    }
}

/// The bytes of `name` for a run, at most `MAX_BYTES + 1` of them read: a
/// file that grew past the cap since it was checked is refused on what was
/// read, not on what the folder said a moment before.
pub fn read_for_run(folder: &Path, name: &str, who: &str) -> Result<Vec<u8>, String> {
    let (path, _) = check_for_run(folder, name, who)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| failed("read", name, e))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_big(name));
    }
    Ok(bytes)
}

/// The live part of an assistant's guide: the names in this project's Test
/// files now, with their sizes - never a path, never the contents. Nothing
/// at all when there are none: both guides say that no such section means
/// the project has no test files yet.
pub fn guide_section(files: &[TestFile]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n## Test files\n\n");
    out.push_str("Files a script or a template may upload, by name (nothing else exists):\n\n");
    for f in files {
        out.push_str(&format!("- `{}` ({})\n", f.name, human_size(u64::from(f.size))));
    }
    out
}
