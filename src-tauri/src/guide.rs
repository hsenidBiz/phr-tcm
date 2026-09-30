//! The How To Use guide as a download rather than part of the exe: what is
//! on disk, how it is fingerprinted, and whether it is current.
//!
//! On disk, under the app's `help/` folder:
//!
//! - `help/<folder>/index.html` - a guide (a downloaded one is filed under
//!   its fingerprint; one adopted from an older install keeps the version
//!   number it was unpacked under),
//! - `help/installed.json` - which folder is current.
//!
//! The network is behind `GuideSource`: `GithubGuide` fetches from this
//! version's own release and nowhere else, and the tests script a fake. No
//! Tauri commands live here (see `commands/guide.rs`).

use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

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

/// The version an older install's guide folder is named for: exactly
/// `x.y.z` or `x.y.z-beta.N`, the only names it ever made. Anything else
/// that parses as a version (build metadata, another pre-release, a temp
/// folder such as `2.0.5-beta.6.tmp-1234-0`) is not one.
fn legacy_folder_version(name: &str) -> Option<semver::Version> {
    let v = semver::Version::parse(name).ok()?;
    let beta_n = |pre: &str| {
        pre.strip_prefix("beta.").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    (v.build.is_empty() && (v.pre.is_empty() || beta_n(v.pre.as_str()))).then_some(v)
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
            legacy_folder_version(&name).map(|v| (v, name))
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

/// The installed guide's `index.html` - the one Settings opens - after
/// adopting an older install's guide if there is no record yet. None when
/// no guide is on disk.
pub fn installed_index(help_root: &Path) -> Option<PathBuf> {
    adopt_legacy(help_root);
    read_installed(help_root).map(|(_, folder)| folder.join("index.html"))
}

// ---- Fetching ---------------------------------------------------------------

/// The two files a release publishes for the guide.
pub const GUIDE_JSON: &str = "how-to-use.json";
pub const GUIDE_ZIP: &str = "how-to-use.zip";

/// Where a release's guide files are: github.com itself (not the API - the
/// updater's reason for its mirror holds here too), under this version's
/// own tag. The only addresses the guide is ever fetched from.
pub fn guide_url(version: &str, file: &str) -> String {
    format!("https://github.com/hsenidBiz/phr-tcm/releases/download/v{version}/{file}")
}

/// Where the guide comes from. `GithubGuide` in the app; a scripted fake in
/// the tests.
pub trait GuideSource {
    /// This version's `how-to-use.json`. `Ok(None)` when the release has
    /// none (a 404); `Err` carries the raw reason for the log.
    fn published(&self) -> impl Future<Output = Result<Option<Published>, String>>;

    /// Write the zip to `to`, calling `on_progress(received, total)` as the
    /// bytes arrive (`total` is 0 when the server did not say). `Err`
    /// carries the raw reason for the log; `to` may then hold part of it.
    /// `Send` because the download runs inside an async command.
    fn download(
        &self,
        to: &Path,
        on_progress: &mut (dyn FnMut(u64, u64) + Send),
    ) -> impl Future<Output = Result<(), String>>;
}

/// This version's release on GitHub.
pub struct GithubGuide {
    version: String,
    /// None in the app: github.com, over https only. A test's local server
    /// on 127.0.0.1 (plain http) otherwise - see `loopback`.
    loopback: Option<u16>,
    /// How long the zip download may go without receiving anything.
    quiet: Duration,
}

impl GithubGuide {
    /// The release this build came from - betas included.
    pub fn this_version() -> Self {
        GithubGuide { version: env!("CARGO_PKG_VERSION").to_string(), loopback: None, quiet: QUIET_TIMEOUT }
    }

    /// For tests: the same download path against a server on 127.0.0.1
    /// `port`, giving up after `quiet` without data. Only the loopback
    /// address can be named, so this cannot fetch from anywhere else.
    pub fn loopback(port: u16, quiet: Duration) -> Self {
        GithubGuide { version: env!("CARGO_PKG_VERSION").to_string(), loopback: Some(port), quiet }
    }

    fn url(&self, file: &str) -> String {
        match self.loopback {
            None => guide_url(&self.version, file),
            Some(port) => format!("http://127.0.0.1:{port}/v{}/{file}", self.version),
        }
    }
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The whole json fetch, answer included.
const JSON_TIMEOUT: Duration = Duration::from_secs(10);
/// The zip has no total deadline (31 MB on a slow line is minutes), but a
/// connection that goes quiet this long is given up on.
const QUIET_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a request may take.
enum Deadline {
    /// The whole request, answer included.
    Whole(Duration),
    /// No total, but the wait for the answer's headers and each read of its
    /// body must each end within this.
    Quiet(Duration),
}

impl GithubGuide {
    fn client(&self, deadline: Deadline) -> Result<reqwest::Client, String> {
        let builder = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            // github.com is only ever asked over https; only a test's
            // loopback server is plain http.
            .https_only(self.loopback.is_none());
        let builder = match deadline {
            Deadline::Whole(t) => builder.timeout(t),
            Deadline::Quiet(t) => builder.read_timeout(t),
        };
        builder.build().map_err(|e| format!("could not build the HTTP client: {e}"))
    }
}

impl GuideSource for GithubGuide {
    async fn published(&self) -> Result<Option<Published>, String> {
        let url = self.url(GUIDE_JSON);
        let resp = self
            .client(Deadline::Whole(JSON_TIMEOUT))?
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("fetch {GUIDE_JSON}: {e}"))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(format!("{GUIDE_JSON} answered {}", resp.status()));
        }
        let text = resp.text().await.map_err(|e| format!("read {GUIDE_JSON}: {e}"))?;
        serde_json::from_str(&text).map(Some).map_err(|e| format!("parse {GUIDE_JSON}: {e}"))
    }

    async fn download(&self, to: &Path, on_progress: &mut (dyn FnMut(u64, u64) + Send)) -> Result<(), String> {
        let url = self.url(GUIDE_ZIP);
        // Without a read deadline, a server that takes the connection and
        // never answers would hold `send()` - and the one-at-a-time claim -
        // for good.
        let mut resp = self
            .client(Deadline::Quiet(self.quiet))?
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("fetch {GUIDE_ZIP}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("{GUIDE_ZIP} answered {}", resp.status()));
        }
        let total = resp.content_length().unwrap_or(0);
        if total > MAX_ZIP_BYTES {
            return Err(format!("{GUIDE_ZIP} is {total} bytes, over the {MAX_ZIP_BYTES} limit"));
        }
        let mut file = std::fs::File::create(to).map_err(|e| format!("create {}: {e}", to.display()))?;
        let mut received: u64 = 0;
        loop {
            let chunk = tokio::time::timeout(self.quiet, resp.chunk())
                .await
                .map_err(|_| format!("{GUIDE_ZIP}: no data for {} ms", self.quiet.as_millis()))?
                .map_err(|e| format!("read {GUIDE_ZIP}: {e}"))?;
            let Some(chunk) = chunk else { break };
            received += chunk.len() as u64;
            if received > MAX_ZIP_BYTES {
                return Err(format!("{GUIDE_ZIP} passed the {MAX_ZIP_BYTES} byte limit"));
            }
            file.write_all(&chunk).map_err(|e| format!("write {}: {e}", to.display()))?;
            on_progress(received, total);
        }
        file.flush().map_err(|e| format!("write {}: {e}", to.display()))?;
        Ok(())
    }
}

/// This run's answer from the release: None = not asked yet; Some(None) =
/// asked, and there is nothing usable (404, offline, or a json that does
/// not parse or validate).
static PUBLISHED: Mutex<Option<Option<Published>>> = Mutex::new(None);

/// Held while the json is being fetched, so two status calls at once still
/// fetch it once.
static FETCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn published_cache() -> std::sync::MutexGuard<'static, Option<Option<Published>>> {
    PUBLISHED.lock().unwrap_or_else(|e| e.into_inner())
}

/// Forget this run's answer from the release, so the next status or
/// download asks again. After a damaged download (a json that does not
/// match its zip), and in tests.
pub fn forget_published() {
    *published_cache() = None;
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Why a parsed json cannot be used, if it cannot: both hashes must be 64
/// lowercase hex characters and the size within the limit.
fn unusable(p: &Published) -> Option<String> {
    if !is_hex64(&p.fingerprint) {
        return Some(format!("{GUIDE_JSON} has a fingerprint that is not 64 lowercase hex characters"));
    }
    if !is_hex64(&p.sha256) {
        return Some(format!("{GUIDE_JSON} has a sha256 that is not 64 lowercase hex characters"));
    }
    if p.size > MAX_ZIP_BYTES {
        return Some(format!("{GUIDE_JSON} says the zip is {} bytes, over the {MAX_ZIP_BYTES} limit", p.size));
    }
    None
}

/// Ask the release, validate the answer, and keep it for the run. `Ok(None)`
/// is "not published"; `Err` is "could not be fetched" (or unusable), with
/// the reason already logged. Either way the cache then holds None.
async fn fetch_published<S: GuideSource>(source: &S) -> Result<Option<Published>, ()> {
    let answer = match source.published().await {
        Ok(Some(p)) => match unusable(&p) {
            None => Ok(Some(p)),
            Some(reason) => {
                crate::applog::warn(format!("the How To Use guide's release answer is unusable: {reason}"));
                Err(())
            }
        },
        Ok(None) => {
            crate::applog::info("no How To Use guide is published for this version");
            Ok(None)
        }
        Err(e) => {
            crate::applog::warn(format!("could not ask the release for the How To Use guide: {e}"));
            Err(())
        }
    };
    *published_cache() = Some(answer.clone().ok().flatten());
    answer
}

/// Whether a guide is on disk and current. An older install's guide is
/// adopted first. The release is asked at most once per run; offline (or
/// any failure to ask) counts as nothing published, so an installed guide
/// reads Ready and nothing nags.
pub async fn status<S: GuideSource>(help_root: &Path, source: &S) -> GuideStatus {
    adopt_legacy(help_root);
    let published = {
        let _fetching = FETCHING.lock().await;
        let cached = published_cache().clone();
        match cached {
            Some(p) => p,
            None => fetch_published(source).await.ok().flatten(),
        }
    };
    let installed = read_installed(help_root).map(|(rec, _)| rec);
    GuideStatus {
        state: state_for(installed.as_ref(), published.as_ref()),
        size: published.as_ref().and_then(|p| u32::try_from(p.size).ok()),
    }
}

/// One download at a time.
static DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// The download claim; released on drop, whatever path the download ends by.
struct Claim;

impl Drop for Claim {
    fn drop(&mut self) {
        DOWNLOADING.store(false, Ordering::Release);
    }
}

static TEMP_SEQ: AtomicU32 = AtomicU32::new(0);

/// The downloaded zip, in the system temp folder (never under `help/`,
/// which `install` sweeps); removed on drop.
struct TempZip(PathBuf);

impl Drop for TempZip {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Download this version's guide and install it. A second call while one
/// runs is refused. The json is the one this run already has if it is
/// usable; otherwise it is asked for again (the person asked, so a failed
/// first try at startup does not strand them until a restart). The zip goes
/// to a temp file that is removed on every path, then `install` checks and
/// unpacks it. Every failure leaves the guide on disk as it was and returns
/// one of the guide's sentences; the raw reason goes to the log. Returns the
/// new `index.html`.
pub async fn download<S: GuideSource>(
    help_root: &Path,
    source: &S,
    mut on_progress: impl FnMut(u64, u64) + Send,
) -> Result<PathBuf, String> {
    if DOWNLOADING.swap(true, Ordering::AcqRel) {
        crate::applog::warn("a How To Use download was refused: one is already running");
        return Err(DOWNLOAD_FAILED.to_string());
    }
    let _claim = Claim;

    let published = {
        let _fetching = FETCHING.lock().await;
        let cached = published_cache().clone().flatten();
        match cached {
            Some(p) => p,
            None => match fetch_published(source).await {
                Ok(Some(p)) => p,
                Ok(None) => return Err(NOT_PUBLISHED.to_string()),
                Err(()) => return Err(DOWNLOAD_FAILED.to_string()),
            },
        }
    };

    let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp = TempZip(std::env::temp_dir().join(format!("phr-tcm-how-to-use-{}-{n}.zip", std::process::id())));
    let _ = std::fs::remove_file(&temp.0);
    let total = published.size;
    let mut report = |received: u64, told: u64| on_progress(received, if told == 0 { total } else { told });
    if let Err(e) = source.download(&temp.0, &mut report).await {
        crate::applog::warn(format!("the How To Use download failed: {e}"));
        return Err(DOWNLOAD_FAILED.to_string());
    }

    let (root, zip) = (help_root.to_path_buf(), temp.0.clone());
    let installed = tokio::task::spawn_blocking(move || install(&root, &zip, &published))
        .await
        .unwrap_or_else(|e| {
            crate::applog::warn(format!("the How To Use install did not finish: {e}"));
            Err(DOWNLOAD_FAILED.to_string())
        });
    drop(temp);
    if installed.as_deref().is_err_and(|e| e == DAMAGED) {
        // The json may be what was wrong (stale, or uploaded beside the
        // wrong zip): the next try asks for it again.
        forget_published();
    }
    installed
}
