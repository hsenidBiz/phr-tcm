//! The How To Use guide as a download rather than part of the exe: what is
//! on disk, how it is fingerprinted, and whether it is current.
//!
//! Pure file logic - no network, no Tauri commands. On disk, under the
//! app's `help/` folder:
//!
//! - `help/<folder>/index.html` - a guide (a downloaded one is filed under
//!   its fingerprint; one adopted from an older install keeps the version
//!   number it was unpacked under),
//! - `help/installed.json` - which folder is current.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The record of the guide that is installed. `fingerprint` is None for a
/// guide adopted from an older install, whose contents were never
/// fingerprinted - it then always offers the published one as an update.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Installed {
    pub fingerprint: Option<String>,
    /// The folder's name under `help/`.
    pub folder: String,
    /// When it was recorded (the applog stamp).
    pub at: String,
}

/// What the release published in `how-to-use.json`.
#[derive(Deserialize, Clone, PartialEq)]
pub struct Published {
    pub fingerprint: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Serialize, specta::Type, Clone, Copy, PartialEq, Debug)]
pub enum GuideState {
    NotDownloaded,
    Ready,
    UpdateAvailable,
}

#[derive(Serialize, specta::Type)]
pub struct GuideStatus {
    pub state: GuideState,
    /// Bytes of the published zip, when known. `u32` because specta
    /// refuses `u64`, and the download is capped at 200 MB.
    pub size: Option<u32>,
}

const RECORD: &str = "installed.json";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn file_hex(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn collect_files(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_files(&entry.path(), &rel, out)?;
        } else if kind.is_file() {
            out.push((rel, entry.path()));
        }
    }
    Ok(())
}

/// The fingerprint of a site folder: every file under `root`, path relative
/// to it with `/` separators, sorted by that path (byte order); for each,
/// the text `<path>\n<sha256 hex of its bytes>\n`; the SHA-256 hex of the
/// concatenation. File times and creation order do not matter, so an
/// unchanged guide fingerprints the same release after release. The release
/// script implements the same rule and a test on each side pins them to one
/// value.
pub fn fingerprint(root: &Path) -> std::io::Result<String> {
    let mut files = Vec::new();
    collect_files(root, "", &mut files)?;
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut all = String::new();
    for (rel, abs) in &files {
        all.push_str(rel);
        all.push('\n');
        all.push_str(&file_hex(abs)?);
        all.push('\n');
    }
    Ok(sha256_hex(all.as_bytes()))
}

/// A record's folder, but only if it is a plain folder name directly under
/// `help/` - never a path that could point elsewhere.
fn plain_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
}

/// The installed record and the folder it names - only when that folder
/// holds an `index.html`. A missing, unreadable or dangling record is None,
/// the same as nothing installed.
pub fn read_installed(help_root: &Path) -> Option<(Installed, PathBuf)> {
    let text = std::fs::read_to_string(help_root.join(RECORD)).ok()?;
    let rec: Installed = serde_json::from_str(&text).ok()?;
    if !plain_folder_name(&rec.folder) {
        return None;
    }
    let folder = help_root.join(&rec.folder);
    folder.join("index.html").is_file().then_some((rec, folder))
}

/// An install from before the guide was a download left `help/<version>/`
/// folders and no record. When there is no record, the newest such folder
/// that has an `index.html` is recorded (with no fingerprint) so the guide
/// keeps opening. A beta sorts below its release and numeric parts compare
/// as numbers, as the updater orders versions.
pub fn adopt_legacy(help_root: &Path) -> Option<Installed> {
    let record = help_root.join(RECORD);
    if record.exists() {
        return None;
    }
    let newest = std::fs::read_dir(help_root)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.path().join("index.html").is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            crate::updater::parse_version(&name).map(|v| (v, name))
        })
        .max_by(|a, b| a.0.cmp(&b.0))?;
    let rec = Installed { fingerprint: None, folder: newest.1, at: crate::applog::stamp() };
    let json = serde_json::to_string(&rec).ok()?;
    crate::ai_tools::atomic_write(&record, &json).ok()?;
    Some(rec)
}

/// Whether the guide on disk is current. With nothing published to compare
/// against (offline, or this release has none) an installed guide is Ready;
/// one with no fingerprint cannot be compared and yields to a published one.
pub fn state_for(installed: Option<&Installed>, published: Option<&Published>) -> GuideState {
    match (installed, published) {
        (None, _) => GuideState::NotDownloaded,
        (Some(_), None) => GuideState::Ready,
        (Some(i), Some(p)) if i.fingerprint.as_deref() == Some(p.fingerprint.as_str()) => GuideState::Ready,
        (Some(_), Some(_)) => GuideState::UpdateAvailable,
    }
}
