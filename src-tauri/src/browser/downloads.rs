//! The files an Auto Run browser downloads, and the names they are kept
//! under.
//!
//! The browser saves each download under its guid in the folder it was
//! given (`Cdp::enable_downloads`); once it completes, the driver renames
//! it to the page's suggested name. That name comes from the page, so it is
//! never trusted: `sanitise_name` makes it a plain file name, which cannot
//! leave the folder, and `unique_name` makes sure a second download of the
//! same name never overwrites the first.

use std::path::{Path, PathBuf};
use std::time::Instant;

/// The longest name a download is kept under, in characters, extension
/// included. Well inside Windows' 255 per component, with room for the
/// folder above it under the 260-character path limit most tools still
/// assume.
pub const MAX_NAME_CHARS: usize = 150;

/// Where a download stands, as the browser last said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    InProgress,
    Completed,
    Canceled,
}

/// One download, in the order the browser started them.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadEntry {
    /// The browser's own id for it.
    pub guid: String,
    /// The page's suggested name, sanitised. Two downloads of one name share
    /// it; their `path`s differ.
    pub name: String,
    /// The file on disk: under its guid until it completes, then under its
    /// own name (`name`, numbered when that was taken).
    pub path: PathBuf,
    pub started_at: Instant,
    pub state: DownloadState,
    /// Bytes received so far; the file's size once it completed.
    pub bytes: u64,
}

/// Characters Windows refuses in a file name, beside the control
/// characters.
const REFUSED: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Names Windows keeps for devices, with or without an extension.
fn is_reserved(stem: &str) -> bool {
    let s = stem.trim_end_matches(' ').to_ascii_uppercase();
    if matches!(s.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let b = s.as_bytes();
    b.len() == 4 && (s.starts_with("COM") || s.starts_with("LPT")) && (b'1'..=b'9').contains(&b[3])
}

/// The extension, dot included, when the name has one: the part from the
/// last dot, unless that dot is the first character (`.env` is all name).
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// A suggested name as a plain file name: only its last path part, every
/// character Windows refuses replaced with `_`, the trailing dots and
/// spaces Windows would drop anyway dropped, a device name given a `_` in
/// front, and at most `MAX_NAME_CHARS` characters with its extension kept.
/// Nothing left is `download`.
pub fn sanitise_name(suggested: &str) -> String {
    let last = suggested.rsplit(['\\', '/']).next().unwrap_or("");
    let replaced: String =
        last.chars().map(|c| if c.is_control() || REFUSED.contains(&c) { '_' } else { c }).collect();
    let mut name = replaced.trim_end_matches(['.', ' ']).trim_start_matches(' ').to_string();
    if name.is_empty() {
        return "download".to_string();
    }
    // The part before the FIRST dot is what Windows reads as the device:
    // `nul.tar.gz` is as reserved as `nul`.
    let device = name.split('.').next().unwrap_or("");
    if is_reserved(device) {
        name.insert(0, '_');
    }
    if name.chars().count() > MAX_NAME_CHARS {
        let (stem, ext) = split_ext(&name);
        let ext_len = ext.chars().count();
        name = if ext_len < MAX_NAME_CHARS / 2 {
            let kept: String = stem.chars().take(MAX_NAME_CHARS - ext_len).collect();
            format!("{kept}{ext}")
        } else {
            // An "extension" this long is not one: cut the whole name.
            name.chars().take(MAX_NAME_CHARS).collect()
        };
    }
    name
}

/// Whether anything at all sits at `path`, a dangling link included.
fn taken(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// `name`, or the first of `name (2)`, `name (3)` and so on (the number
/// before the extension) that nothing in `dir` is called.
pub fn unique_name(dir: &Path, name: &str) -> String {
    if !taken(&dir.join(name)) {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    (2u32..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| !taken(&dir.join(candidate)))
        .expect("an unbounded range always finds a free number")
}
