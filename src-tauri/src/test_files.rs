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
/// without an extension (`CON`, `con.txt`) - the superscript-digit ports
/// and the console's own `CONIN$` and `CONOUT$` included.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "COM\u{b9}", "COM\u{b2}", "COM\u{b3}",
    "LPT\u{b9}", "LPT\u{b2}", "LPT\u{b3}", "CONIN$", "CONOUT$",
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

/// The extensions a test uploads: the content-type table's (`content_type`) and nothing else.
pub const UPLOADABLE: &[&str] = &[
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "txt", "csv", "json", "xml", "doc", "docx", "xls", "xlsx", "ppt",
    "pptx", "zip",
];

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase())
}

/// Whether `name` is a kind of file a test uploads (its extension, in any case, is in `UPLOADABLE`).
pub fn is_uploadable(name: &str) -> bool {
    extension(name).is_some_and(|ext| UPLOADABLE.contains(&ext.as_str()))
}

/// Said for any other name.
pub fn not_uploadable(name: &str) -> String {
    format!("\"{name}\" is not a file a test uploads: Test files only hold documents an upload step or a template's files send (.pdf, .png, .jpg, .jpeg, .gif, .webp, .txt, .csv, .json, .xml, .doc, .docx, .xls, .xlsx, .ppt, .pptx, .zip). Keep notes, scripts and reports out of Test files.")
}

/// Whether `bytes` really are a file of `name`'s kind, checked for an assistant's file only: the
/// first bytes for binary kinds - pdf "%PDF-"; png 89 50 4E 47; jpg/jpeg FF D8 FF; gif "GIF8";
/// webp "RIFF"....+"WEBP" at 8; docx/xlsx/pptx/zip "PK"; doc/xls/ppt D0 CF 11 E0 A1 B1 1A E1.
/// txt/csv/json/xml hold anything (a Windows-1252 or UTF-16 export, or an unparseable json, is a
/// real test input).
pub fn looks_like(name: &str, bytes: &[u8]) -> bool {
    let Some(ext) = extension(name) else { return false };
    match ext.as_str() {
        "pdf" => bytes.starts_with(b"%PDF-"),
        "png" => bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]),
        "jpg" | "jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "gif" => bytes.starts_with(b"GIF8"),
        "webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        "docx" | "xlsx" | "pptx" | "zip" => bytes.starts_with(b"PK"),
        "doc" | "xls" | "ppt" => bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]),
        "txt" | "csv" | "json" | "xml" => true,
        _ => false,
    }
}

/// Said when it does not.
pub fn not_what_it_says(name: &str) -> String {
    format!("\"{name}\" does not hold what its extension says - write a real file of that kind (an .xlsx is a zipped workbook, not text) and save it again. If you meant a broken file for a negative test, pass malformed: true.")
}

/// Every save of a test file holds this from its "already there" check through the rename: the
/// person's add and several assistants' saves can land at once. Coordination, not a cache, and
/// never held across an `.await` (`commit` is synchronous).
fn write_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

fn already_there(name: &str) -> String {
    format!("\"{name}\" is already in Test files - replace it, or rename your copy first")
}

/// The checks and the atomic write both ways in share, for bytes in hand: valid name, uploadable,
/// size cap, `looks_like` when `check_content` and - unless `replace` - no file of that name
/// already there. The bytes are written beside the final name and moved into place, so a reader
/// never sees half of them. Returns the saved file's size on success; the caller logs it in its
/// own words.
fn commit(folder: &Path, name: &str, bytes: &[u8], replace: bool, check_content: bool) -> Result<TestFile, String> {
    let dest = resolve(folder, name)?;
    if !is_uploadable(name) {
        return Err(not_uploadable(name));
    }
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_big(name));
    }
    if check_content && !looks_like(name, bytes) {
        return Err(not_what_it_says(name));
    }
    // From the "already there" check to the rename, one save at a time: two assistants' saves of
    // one name must not both pass the check, and the later one silently replace the earlier.
    let _one_at_a_time = write_lock();
    if dest.exists() && !replace {
        return Err(already_there(name));
    }
    std::fs::create_dir_all(folder).map_err(|e| failed("added", name, e))?;
    // A leading dot is not a test file name, so `list` never shows this. Its own name per call,
    // so no two writers can ever share a part file, locked or not.
    static NEXT_PART: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT_PART.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let part = folder.join(format!(".{name}.part-{}-{n}", std::process::id()));
    if let Err(e) = std::fs::write(&part, bytes) {
        let _ = std::fs::remove_file(&part);
        return Err(failed("added", name, e));
    }
    if let Err(e) = std::fs::rename(&part, &dest) {
        let _ = std::fs::remove_file(&part);
        return Err(failed("added", name, e));
    }
    crate::autorun::data_changed::notify(folder, crate::events::AutorunDataKind::TestFiles);
    let meta = std::fs::metadata(&dest).map_err(|e| failed("read", name, e))?;
    Ok(entry(name, &meta))
}

/// Saves `bytes` into `folder` as `name`: `add`'s checks and write for bytes an assistant made,
/// and the magic-byte check of a binary kind (`looks_like`) unless `malformed` - a deliberately
/// broken file for a negative upload test, to which the extension rule still applies.
/// Logs the name and size, never a path.
pub fn save_bytes(folder: &Path, name: &str, bytes: &[u8], replace: bool, malformed: bool) -> Result<TestFile, String> {
    let saved = commit(folder, name, bytes, replace, !malformed)?;
    let size = human_size(u64::from(saved.size));
    if malformed {
        crate::applog::info(format!(
            "test files: the assistant saved \"{name}\" as a deliberately malformed file ({size})"
        ));
    } else {
        crate::applog::info(format!("test files: the assistant saved \"{name}\" ({size})"));
    }
    Ok(saved)
}

/// Whether `p` is a plain local absolute path: a drive-letter path on Windows (`C:\x`, `C:/x`),
/// never a UNC share (`\\server\share`), a device path (`\\?\`, `\\.\`) or a root-relative one.
/// Judged on the text alone, before anything touches the file system.
pub fn is_plain_local_path(p: &str) -> bool {
    if p.starts_with("\\\\") || p.starts_with("//") || !Path::new(p).has_root() {
        return false;
    }
    #[cfg(windows)]
    {
        let b = p.as_bytes();
        b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// `name`'s extension without the dot, lowercased: what the saved name must keep of its source.
pub fn extension_of(name: &str) -> Option<String> {
    extension(name)
}

/// Copies the file at `source` into `folder` under its own name. Refused,
/// each with a sentence naming the file: a name `valid_test_file_name`
/// refuses, one that is not a kind of file a test uploads, a file over
/// `MAX_BYTES`, one that does not hold what its extension says, or - unless
/// `replace` - a file of that name already there (the screen asks the person
/// first). The copy is written beside its final name and moved into place,
/// so a reader never sees half of it; the size cap is checked on what is
/// actually read.
pub fn add(folder: &Path, source: &Path, replace: bool) -> Result<TestFile, String> {
    let name = picked_name(source);
    let dest = resolve(folder, &name)?;
    if !is_uploadable(&name) {
        return Err(not_uploadable(&name));
    }
    if dest.exists() && !replace {
        return Err(already_there(&name));
    }
    let meta = std::fs::metadata(source).map_err(|e| failed("read", &name, e))?;
    if !meta.is_file() {
        return Err(format!("\"{name}\" is not a file"));
    }
    if meta.len() > MAX_BYTES {
        return Err(too_big(&name));
    }
    let bytes = read_capped(source, &name)?;
    // A person's file is taken as it is: only the kind of file is held to the allowlist. A
    // Windows-1252 or UTF-16 export, or a deliberately broken file, is a real test input.
    let saved = commit(folder, &name, &bytes, replace, false)?;
    crate::applog::info(format!("test files: added \"{name}\" ({})", human_size(u64::from(saved.size))));
    Ok(saved)
}

/// At most `MAX_BYTES + 1` bytes of the file at `source`: one that grew past the cap since it was
/// looked at is refused on what was read.
pub fn read_capped(source: &Path, name: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(source)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| failed("read", name, e))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_big(name));
    }
    Ok(bytes)
}

/// Deletes this machine's copy of `name` - a local file, nothing else.
pub fn remove(folder: &Path, name: &str) -> Result<(), String> {
    let path = resolve(folder, name)?;
    match std::fs::remove_file(&path) {
        Ok(()) => {
            crate::applog::info(format!("test files: removed \"{name}\""));
            crate::autorun::data_changed::notify(folder, crate::events::AutorunDataKind::TestFiles);
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
    read_capped(&path, name)
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
