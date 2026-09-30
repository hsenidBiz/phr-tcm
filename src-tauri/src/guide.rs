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
use std::sync::atomic::{AtomicU32, Ordering};

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

/// The zip is refused above this, by the published size and by bytes received.
pub const MAX_ZIP_BYTES: u64 = 200 * 1024 * 1024;

// The four sentences the guide's screens show. They name no URL or path;
// the raw reason goes to the log.
pub const NOT_DOWNLOADED: &str = "Download How to Use from Settings first.";
pub const DOWNLOAD_FAILED: &str =
    "Could not download How to Use. Check your connection and try again - Settings, Logs has the details.";
pub const DAMAGED: &str = "The downloaded guide was damaged, so it was not kept. Try again.";
pub const NOT_PUBLISHED: &str = "How to Use is not available for this version.";

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes))
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
    Ok(hex(hasher.finalize()))
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
    write_record(help_root, &rec).ok()?;
    Some(rec)
}

/// Write `help/installed.json` (temp file then rename, so it is never half
/// written).
fn write_record(help_root: &Path, rec: &Installed) -> Result<(), String> {
    let json = serde_json::to_string(rec).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&help_root.join(RECORD), &json)
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

/// Whether a zip entry name may be unpacked, and the folder/file names it is
/// made of. Split on BOTH `/` and `\` (some Windows zippers write
/// backslashes); it must not be empty, absolute, name a drive (any `:`), or
/// climb out with `..`.
fn safe_entry(name: &str) -> Option<Vec<&str>> {
    let mut parts: Vec<&str> = name.split(['/', '\\']).collect();
    // A trailing separator marks a folder.
    if parts.last() == Some(&"") {
        parts.pop();
    }
    if parts.is_empty() || parts[0].is_empty() {
        return None;
    }
    if parts.iter().any(|p| *p == ".." || p.contains(':')) {
        return None;
    }
    Some(parts.into_iter().filter(|p| !p.is_empty() && *p != ".").collect())
}

/// Unpack `zip_path` into `into`. Every entry name is checked first, and one
/// unsafe name refuses the whole zip before anything is written. `Err`
/// carries the reason for the log.
pub fn unpack(zip_path: &Path, into: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("open zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("read zip: {e}"))?;

    let mut plan = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(|e| format!("zip entry {i}: {e}"))?;
        let name = entry.name().to_string();
        let parts = safe_entry(&name).ok_or_else(|| format!("unsafe zip entry name {name:?}"))?;
        let is_dir = entry.is_dir() || name.ends_with(['/', '\\']);
        plan.push((parts.iter().collect::<PathBuf>(), is_dir));
    }

    std::fs::create_dir_all(into).map_err(|e| format!("create {}: {e}", into.display()))?;
    for (i, (rel, is_dir)) in plan.into_iter().enumerate() {
        let target = into.join(rel);
        if is_dir {
            std::fs::create_dir_all(&target).map_err(|e| format!("create {}: {e}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        let mut entry = archive.by_index(i).map_err(|e| format!("zip entry {i}: {e}"))?;
        let mut out = std::fs::File::create(&target).map_err(|e| format!("write {}: {e}", target.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("write {}: {e}", target.display()))?;
    }
    Ok(())
}

static INCOMING: AtomicU32 = AtomicU32::new(0);

/// A folder being filled; removed on drop unless kept.
struct Incoming {
    path: PathBuf,
    keep: bool,
}

impl Drop for Incoming {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Check and install a downloaded guide: the zip must be the published size
/// and checksum, must unpack safely and hold an `index.html`. It is unpacked
/// beside the old guide as `help/.incoming-<n>`, renamed to
/// `help/<fingerprint>/`, recorded in `installed.json`, and only then is
/// everything else under `help/` removed. Any failure leaves the old guide
/// and its record as they were, no `.incoming-*` folder behind, and returns
/// the "damaged" sentence (the reason goes to the log). Returns the new
/// `index.html`.
pub fn install(help_root: &Path, zip_path: &Path, published: &Published) -> Result<PathBuf, String> {
    install_inner(help_root, zip_path, published).map_err(|reason| {
        crate::applog::warn(format!("guide install refused: {reason}"));
        DAMAGED.to_string()
    })
}

fn install_inner(help_root: &Path, zip_path: &Path, published: &Published) -> Result<PathBuf, String> {
    let folder = &published.fingerprint;
    if !plain_folder_name(folder) || folder.starts_with('.') {
        return Err(format!("fingerprint {folder:?} is not a plain folder name"));
    }
    let size = std::fs::metadata(zip_path).map_err(|e| format!("stat zip: {e}"))?.len();
    if size > MAX_ZIP_BYTES || size != published.size {
        return Err(format!("zip is {size} bytes, published {}", published.size));
    }
    let got = file_hex(zip_path).map_err(|e| format!("hash zip: {e}"))?;
    if !got.eq_ignore_ascii_case(&published.sha256) {
        return Err("zip checksum does not match".to_string());
    }

    std::fs::create_dir_all(help_root).map_err(|e| format!("create help folder: {e}"))?;
    let n = INCOMING.fetch_add(1, Ordering::Relaxed);
    let mut incoming = Incoming {
        path: help_root.join(format!(".incoming-{}-{n}", std::process::id())),
        keep: false,
    };
    // A leftover of a crashed run under the same name would poison the unpack.
    let _ = std::fs::remove_dir_all(&incoming.path);
    unpack(zip_path, &incoming.path)?;
    if !incoming.path.join("index.html").is_file() {
        return Err("the zip has no index.html at its root".to_string());
    }

    let target = help_root.join(folder);
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("replace {}: {e}", target.display()))?;
    }
    std::fs::rename(&incoming.path, &target).map_err(|e| format!("rename into place: {e}"))?;
    incoming.keep = true;

    let rec = Installed {
        fingerprint: Some(folder.clone()),
        folder: folder.clone(),
        at: crate::applog::stamp(),
    };
    if let Err(e) = write_record(help_root, &rec) {
        let _ = std::fs::remove_dir_all(&target);
        return Err(format!("write record: {e}"));
    }

    // The new guide is live; what is left under help/ is the old guide,
    // legacy version folders and strays. Failing to remove one is harmless.
    if let Ok(rd) = std::fs::read_dir(help_root) {
        for entry in rd.flatten() {
            let name = entry.file_name();
            if name == RECORD || name.to_string_lossy() == folder.as_str() {
                continue;
            }
            let path = entry.path();
            let removed = if entry.file_type().is_ok_and(|t| t.is_dir()) {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(e) = removed {
                crate::applog::warn(format!("could not remove {}: {e}", path.display()));
            }
        }
    }
    Ok(target.join("index.html"))
}
