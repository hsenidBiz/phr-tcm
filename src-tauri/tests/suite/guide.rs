//! The guide on disk: its fingerprint, its installed record, and whether
//! it is current; then fetching it through a `GuideSource` - a scripted
//! fake, or the real `GithubGuide` pointed at a local server that never
//! answers. Nothing touches the internet.

use std::fs;
use std::path::{Path, PathBuf};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use v2_lib::guide::{
    adopt_legacy, download, fingerprint, forget_published, guide_url, install, installed_index,
    read_installed, state_for, status, unpack, GithubGuide, GuideSource, GuideState, Installed,
    Published,
    DAMAGED, DOWNLOAD_FAILED, MAX_ZIP_BYTES, NOT_PUBLISHED,
};

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

/// The fingerprint of the two-file fixture, printed once and pasted. The
/// release script's JS implementation asserts the SAME literal - if the
/// rule changes, both change together.
const PINNED: &str = "9a2b1db7d7bd429fc1a0a310563276e3e2310674d59155fcec5f89b5c5096b74";

fn fixture(root: &Path) {
    put(root, "index.html", b"<html>");
    put(root, "img/a.jpg", &[1, 2, 3]);
}

#[test]
fn the_fingerprint_follows_the_rule() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());

    // The rule by hand: paths relative to the root, `/` separators, sorted
    // by byte order; for each "<path>\n<sha256 hex>\n"; SHA-256 of the lot.
    let mut all = String::new();
    all.push_str(&format!("img/a.jpg\n{}\n", hex(&[1, 2, 3])));
    all.push_str(&format!("index.html\n{}\n", hex(b"<html>")));
    let by_hand = hex(all.as_bytes());

    let got = fingerprint(dir.path()).unwrap();
    assert_eq!(got, by_hand);
    assert_eq!(got.len(), 64);
    assert_eq!(got, PINNED);
}

#[test]
fn the_fingerprint_ignores_order_and_times() {
    let a = tempfile::tempdir().unwrap();
    fixture(a.path());

    let b = tempfile::tempdir().unwrap();
    put(b.path(), "img/a.jpg", &[1, 2, 3]);
    std::thread::sleep(std::time::Duration::from_millis(30));
    put(b.path(), "index.html", b"<html>");
    let f = fs::OpenOptions::new().write(true).open(b.path().join("img/a.jpg")).unwrap();
    f.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000))
        .unwrap();

    assert_eq!(fingerprint(a.path()).unwrap(), fingerprint(b.path()).unwrap());

    // Content does matter.
    put(b.path(), "img/a.jpg", &[1, 2, 4]);
    assert_ne!(fingerprint(a.path()).unwrap(), fingerprint(b.path()).unwrap());
}

/// Paths sort by their UTF-8 bytes. U+FF5E (EF BD 9E) sorts before U+1F600
/// (F0 9F 98 80) by bytes, but after it by UTF-16 code units (FF5E against
/// the surrogate D83D) - so a UTF-16 sort gives a different value. The
/// literal was computed by hand from byte order, and
/// scripts/guide-fingerprint.test.mjs pins the same one for the same two
/// files. Windows stores names as UTF-16; both names are legal there and
/// come back as the same UTF-8.
const PINNED_ORDER: &str = "da716af538a6719a8017b3d8a89294421242a353dddcfc32002f674cc8df6001";

#[test]
fn paths_sort_by_utf8_bytes_not_utf16_units() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "\u{1F600}.txt", b"y");
    put(dir.path(), "\u{FF5E}.txt", b"x");
    assert_eq!(fingerprint(dir.path()).unwrap(), PINNED_ORDER);
}

fn installed(fp: Option<&str>) -> Installed {
    Installed { fingerprint: fp.map(str::to_string), folder: "x".into(), at: "t".into() }
}

fn published(fp: &str) -> Published {
    Published { fingerprint: fp.into(), sha256: "0".repeat(64), size: 10 }
}

#[test]
fn state_follows_what_is_on_disk_and_published() {
    let a = installed(Some("aaa"));
    let null = installed(None);

    assert_eq!(state_for(None, Some(&published("aaa"))), GuideState::NotDownloaded);
    assert_eq!(state_for(None, None), GuideState::NotDownloaded);
    assert_eq!(state_for(Some(&a), Some(&published("aaa"))), GuideState::Ready);
    assert_eq!(state_for(Some(&a), Some(&published("bbb"))), GuideState::UpdateAvailable);
    assert_eq!(state_for(Some(&a), None), GuideState::Ready);
    assert_eq!(state_for(Some(&null), Some(&published("aaa"))), GuideState::UpdateAvailable);
    assert_eq!(state_for(Some(&null), None), GuideState::Ready);
}

#[test]
fn an_old_guide_is_adopted_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path();
    put(help, "2.0.4/index.html", b"a");
    put(help, "2.0.5-beta.2/index.html", b"b");
    put(help, "2.0.5-beta.10/other.txt", b"no index");
    put(help, "not-a-version/index.html", b"c");

    let got = adopt_legacy(help).expect("adopted");
    assert_eq!(got.folder, "2.0.5-beta.2");
    assert_eq!(got.fingerprint, None);

    let raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(help.join("installed.json")).unwrap()).unwrap();
    assert!(raw["fingerprint"].is_null());
    assert_eq!(raw["folder"], "2.0.5-beta.2");
    assert!(raw["at"].as_str().is_some_and(|s| !s.is_empty()));

    // A release sorts above its own betas, and 10 above 9 as numbers.
    let dir2 = tempfile::tempdir().unwrap();
    put(dir2.path(), "2.0.5-beta.9/index.html", b"a");
    put(dir2.path(), "2.0.5-beta.10/index.html", b"a");
    assert_eq!(adopt_legacy(dir2.path()).unwrap().folder, "2.0.5-beta.10");
    put(dir2.path(), "2.0.5/index.html", b"a");
    fs::remove_file(dir2.path().join("installed.json")).unwrap();
    assert_eq!(adopt_legacy(dir2.path()).unwrap().folder, "2.0.5");
}

/// Only a folder an older install could have made is adopted: exactly
/// `x.y.z` or `x.y.z-beta.N`. Anything else that parses as a version
/// (another pre-release, build metadata, a half-renamed temp folder) is left
/// alone, even when it holds an index.html.
#[test]
fn only_release_and_beta_folders_are_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path();
    put(help, "2.0.4/index.html", b"a");
    for odd in ["2.0.5-beta.6.tmp-1234-0", "2.0.6+build.1", "2.0.7-rc.1", "2.0.8-beta", "2.0.9-beta.x"] {
        put(help, &format!("{odd}/index.html"), b"odd");
    }
    assert_eq!(adopt_legacy(help).expect("adopted").folder, "2.0.4");

    // With only odd folders, nothing is adopted and no record is written.
    let only_odd = tempfile::tempdir().unwrap();
    put(only_odd.path(), "2.0.5-beta.6.tmp-1234-0/index.html", b"odd");
    assert!(adopt_legacy(only_odd.path()).is_none());
    assert!(!only_odd.path().join("installed.json").exists());
}

#[test]
fn nothing_is_adopted_when_a_record_exists() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "2.0.4/index.html", b"a");
    put(dir.path(), "installed.json", br#"{"fingerprint":"abc","folder":"abc","at":"t"}"#);
    assert!(adopt_legacy(dir.path()).is_none());
    // And an empty or missing help folder adopts nothing.
    let empty = tempfile::tempdir().unwrap();
    assert!(adopt_legacy(empty.path()).is_none());
    assert!(adopt_legacy(&empty.path().join("nope")).is_none());
}

#[test]
fn read_installed_needs_the_folder_and_its_index() {
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path();
    assert!(read_installed(help).is_none());

    put(help, "installed.json", br#"{"fingerprint":"abc","folder":"abc","at":"t"}"#);
    assert!(read_installed(help).is_none(), "record without its folder");

    fs::create_dir_all(help.join("abc")).unwrap();
    assert!(read_installed(help).is_none(), "folder without index.html");

    put(help, "abc/index.html", b"<html>");
    let (rec, folder) = read_installed(help).expect("ready");
    assert_eq!(rec.fingerprint.as_deref(), Some("abc"));
    assert_eq!(folder, help.join("abc"));

    // An adopted guide: null fingerprint, folder is the old version.
    put(help, "installed.json", br#"{"fingerprint":null,"folder":"2.0.4","at":"t"}"#);
    put(help, "2.0.4/index.html", b"<html>");
    let (rec, folder) = read_installed(help).unwrap();
    assert_eq!(rec.fingerprint, None);
    assert_eq!(folder, help.join("2.0.4"));

    // A folder name that tries to leave help/ is not trusted.
    put(help, "installed.json", br#"{"fingerprint":null,"folder":"..","at":"t"}"#);
    assert!(read_installed(help).is_none());

    // Garbage is not a record.
    put(help, "installed.json", b"{not json");
    assert!(read_installed(help).is_none());
}

// ---- Safe unpack and install --------------------------------------------

fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
    use std::io::Write;
    let mut w = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap();
}

fn published_for(zip_path: &Path, fingerprint: &str) -> Published {
    let bytes = fs::read(zip_path).unwrap();
    Published { fingerprint: fingerprint.into(), sha256: hex(&bytes), size: bytes.len() as u64 }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match fs::read_dir(dir) {
        Ok(rd) => rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

fn incoming_left(help: &Path) -> Vec<String> {
    names_in(help).into_iter().filter(|n| n.starts_with(".incoming-")).collect()
}

/// An installed old guide: `help/old/index.html` and a record naming it.
fn old_guide(help: &Path) -> String {
    put(help, "old/index.html", b"old");
    let rec = r#"{"fingerprint":"oldfp","folder":"old","at":"t"}"#;
    put(help, "installed.json", rec.as_bytes());
    rec.to_string()
}

#[test]
fn a_good_zip_is_installed_and_the_old_guide_removed() {
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    old_guide(&help);
    put(&help, "2.0.4/index.html", b"legacy");
    put(&help, "stray.txt", b"x");

    let zip_path = dir.path().join("how-to-use.zip");
    zip_of(&zip_path, &[("index.html", b"<html>"), ("img/", b""), ("img/a.jpg", &[1, 2, 3])]);
    let fp = "f".repeat(64);
    let index = install(&help, &zip_path, &published_for(&zip_path, &fp)).expect("installed");

    assert_eq!(index, help.join(&fp).join("index.html"));
    assert_eq!(fs::read(&index).unwrap(), b"<html>");
    assert_eq!(fs::read(help.join(&fp).join("img/a.jpg")).unwrap(), [1, 2, 3]);

    let (rec, folder) = read_installed(&help).expect("recorded");
    assert_eq!(rec.fingerprint.as_deref(), Some(fp.as_str()));
    assert_eq!(rec.folder, fp);
    assert_eq!(folder, help.join(&fp));
    assert_eq!(names_in(&help), vec![fp.clone(), "installed.json".to_string()]);

    // Installing the same fingerprint again replaces its folder.
    put(&help, &format!("{fp}/leftover.txt"), b"x");
    install(&help, &zip_path, &published_for(&zip_path, &fp)).expect("reinstalled");
    assert!(!help.join(&fp).join("leftover.txt").exists());
    assert!(incoming_left(&help).is_empty());
}

#[test]
fn a_wrong_checksum_keeps_nothing_and_the_old_guide_stays() {
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let rec = old_guide(&help);

    let zip_path = dir.path().join("how-to-use.zip");
    zip_of(&zip_path, &[("index.html", b"<html>")]);
    let mut p = published_for(&zip_path, &"a".repeat(64));
    p.sha256 = "0".repeat(64);

    assert_eq!(install(&help, &zip_path, &p), Err(DAMAGED.to_string()));
    assert_eq!(fs::read(help.join("old/index.html")).unwrap(), b"old");
    assert_eq!(fs::read_to_string(help.join("installed.json")).unwrap(), rec);
    assert!(incoming_left(&help).is_empty());
    assert!(!help.join("a".repeat(64)).exists());
}

#[test]
fn a_wrong_size_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let rec = old_guide(&help);

    let zip_path = dir.path().join("how-to-use.zip");
    zip_of(&zip_path, &[("index.html", b"<html>")]);
    let mut p = published_for(&zip_path, &"a".repeat(64));
    p.size += 1;

    assert_eq!(install(&help, &zip_path, &p), Err(DAMAGED.to_string()));
    assert_eq!(fs::read_to_string(help.join("installed.json")).unwrap(), rec);
    assert!(help.join("old/index.html").is_file());
    assert!(incoming_left(&help).is_empty());

    // A file that is not there at all is refused the same way.
    assert_eq!(install(&help, &dir.path().join("nope.zip"), &p), Err(DAMAGED.to_string()));
    assert!(incoming_left(&help).is_empty());
}

#[test]
fn entries_escaping_the_folder_are_refused() {
    for bad in
        ["../x", r"..\x", "/abs", r"\abs", r"C:\x", "C:/x", "a/../../x", r"a\..\..\x", ""]
    {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("z.zip");
        // A good entry BEFORE the bad one: nothing may be written at all.
        zip_of(&zip_path, &[("index.html", b"<html>"), (bad, b"evil")]);
        let into = dir.path().join("out").join("site");

        assert!(unpack(&zip_path, &into).is_err(), "{bad:?} should be refused");
        assert!(!into.join("index.html").exists(), "{bad:?}: something was written");
        assert_eq!(names_in(dir.path()), vec!["z.zip".to_string()], "{bad:?}: escaped");

        // And through install: DAMAGED, no incoming folder.
        let help = dir.path().join("help");
        assert_eq!(
            install(&help, &zip_path, &published_for(&zip_path, &"b".repeat(64))),
            Err(DAMAGED.to_string())
        );
        assert!(incoming_left(&help).is_empty(), "{bad:?}");
        assert!(!help.join("b".repeat(64)).exists(), "{bad:?}");
        assert!(!dir.path().join("x").exists());
    }
}

#[test]
fn backslash_entries_unpack_into_folders() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("z.zip");
    zip_of(
        &zip_path,
        &[("index.html", b"<html>"), (r"img\light\a.jpg", &[9, 8, 7]), (r"img\dark\", b"")],
    );
    let into = dir.path().join("site");
    unpack(&zip_path, &into).expect("unpacked");
    assert_eq!(fs::read(into.join("img/light/a.jpg")).unwrap(), [9, 8, 7]);
    assert!(into.join("img/dark").is_dir());
    assert_eq!(names_in(&into), vec!["img".to_string(), "index.html".to_string()]);
}

#[test]
fn a_zip_without_an_index_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let rec = old_guide(&help);

    let zip_path = dir.path().join("how-to-use.zip");
    zip_of(&zip_path, &[("img/a.jpg", &[1]), ("readme.txt", b"hi")]);
    assert_eq!(
        install(&help, &zip_path, &published_for(&zip_path, &"c".repeat(64))),
        Err(DAMAGED.to_string())
    );
    assert_eq!(fs::read_to_string(help.join("installed.json")).unwrap(), rec);
    assert!(help.join("old/index.html").is_file());
    assert!(incoming_left(&help).is_empty());
    assert!(!help.join("c".repeat(64)).exists());
}

#[test]
fn a_fingerprint_that_is_not_a_plain_name_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let zip_path = dir.path().join("z.zip");
    zip_of(&zip_path, &[("index.html", b"<html>")]);
    for fp in ["..", "a/b", r"a\b", "", "C:x"] {
        assert_eq!(install(&help, &zip_path, &published_for(&zip_path, fp)), Err(DAMAGED.to_string()));
    }
    assert!(incoming_left(&help).is_empty());
}

// ---- Fetching: status, download, and what to open -----------------------

/// A scripted release: answers `published` with `answer`, and "downloads"
/// `zip` in two halves (reporting progress after each) - or, with
/// `fail_midway`, writes the first half and then drops the connection.
/// `started`/`gate` let a test hold a download open.
struct FakeGuide {
    answer: Result<Option<Published>, String>,
    /// Answers given before `answer`, one per ask.
    queued: Mutex<VecDeque<Result<Option<Published>, String>>>,
    zip: Vec<u8>,
    fail_midway: bool,
    asked: AtomicU32,
    downloads: AtomicU32,
    written_to: Mutex<Option<PathBuf>>,
    started: Mutex<Option<oneshot::Sender<()>>>,
    gate: Mutex<Option<oneshot::Receiver<()>>>,
}

impl FakeGuide {
    fn new(answer: Result<Option<Published>, String>, zip: Vec<u8>) -> Self {
        FakeGuide {
            answer,
            queued: Mutex::new(VecDeque::new()),
            zip,
            fail_midway: false,
            asked: AtomicU32::new(0),
            downloads: AtomicU32::new(0),
            written_to: Mutex::new(None),
            started: Mutex::new(None),
            gate: Mutex::new(None),
        }
    }
    fn asked(&self) -> u32 {
        self.asked.load(Ordering::SeqCst)
    }
    fn downloads(&self) -> u32 {
        self.downloads.load(Ordering::SeqCst)
    }
    fn written_to(&self) -> PathBuf {
        self.written_to.lock().unwrap().clone().expect("the fake was asked to download")
    }
}

impl GuideSource for FakeGuide {
    async fn published(&self) -> Result<Option<Published>, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let queued = self.queued.lock().unwrap().pop_front();
        queued.unwrap_or_else(|| self.answer.clone())
    }

    async fn download(&self, to: &Path, on_progress: &mut (dyn FnMut(u64, u64) + Send)) -> Result<(), String> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        *self.written_to.lock().unwrap() = Some(to.to_path_buf());
        let started = self.started.lock().unwrap().take();
        if let Some(tx) = started {
            let _ = tx.send(());
        }
        let gate = self.gate.lock().unwrap().take();
        if let Some(rx) = gate {
            let _ = rx.await;
        }
        let total = self.zip.len() as u64;
        let half = self.zip.len() / 2;
        fs::write(to, &self.zip[..half]).map_err(|e| e.to_string())?;
        on_progress(half as u64, total);
        if self.fail_midway {
            return Err("connection reset at 50%".to_string());
        }
        fs::write(to, &self.zip).map_err(|e| e.to_string())?;
        on_progress(total, total);
        Ok(())
    }
}

/// A good guide zip and the json a release would publish for it.
fn release(dir: &Path, fp: &str) -> (Vec<u8>, Published) {
    let zip_path = dir.join("release.zip");
    zip_of(&zip_path, &[("index.html", b"<html>new"), ("img/a.jpg", &[1, 2, 3])]);
    let p = published_for(&zip_path, fp);
    (fs::read(&zip_path).unwrap(), p)
}

#[test]
fn the_urls_point_only_at_the_release() {
    assert_eq!(
        guide_url("2.0.6-beta.1", "how-to-use.json"),
        "https://github.com/hsenidBiz/phr-tcm/releases/download/v2.0.6-beta.1/how-to-use.json"
    );
    assert_eq!(
        guide_url("2.0.6", "how-to-use.zip"),
        "https://github.com/hsenidBiz/phr-tcm/releases/download/v2.0.6/how-to-use.zip"
    );
}

#[tokio::test]
async fn the_status_asks_the_release_once() {
    let _g = crate::serial::guide();
    forget_published();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    old_guide(&help);
    let (zip, p) = release(dir.path(), &"b".repeat(64));
    let size = p.size as u32;
    let fake = FakeGuide::new(Ok(Some(p)), zip);

    for _ in 0..3 {
        let s = status(&help, &fake).await;
        assert_eq!(s.state, GuideState::UpdateAvailable);
        assert_eq!(s.size, Some(size));
    }
    assert_eq!(fake.asked(), 1, "the json is fetched once per run");
    forget_published();
}

#[tokio::test]
async fn offline_is_ready_never_update() {
    let _g = crate::serial::guide();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let offline = FakeGuide::new(Err("no route to host".into()), Vec::new());

    // Nothing on disk: Download is offered, and no error comes back.
    forget_published();
    let s = status(&help, &offline).await;
    assert_eq!((s.state, s.size), (GuideState::NotDownloaded, None));

    // A guide adopted from an older install has no fingerprint, but offline
    // it is Ready like any other - never an Update it cannot fetch.
    forget_published();
    put(&help, "2.0.4/index.html", b"legacy");
    let s = status(&help, &offline).await;
    assert_eq!((s.state, s.size), (GuideState::Ready, None));
    assert_eq!(offline.asked(), 2);

    // The offline answer is kept for the run: asking again does not refetch...
    assert_eq!(status(&help, &offline).await.state, GuideState::Ready);
    assert_eq!(offline.asked(), 2);
    // ...but a Download the person asked for does try again.
    assert_eq!(download(&help, &offline, |_, _| {}).await, Err(DOWNLOAD_FAILED.to_string()));
    assert_eq!(offline.asked(), 3);
    assert_eq!(offline.downloads(), 0);
    assert_eq!(fs::read(help.join("2.0.4/index.html")).unwrap(), b"legacy");
    forget_published();
}

#[tokio::test]
async fn a_download_installs_and_reports_progress() {
    let _g = crate::serial::guide();
    forget_published();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    old_guide(&help);
    let fp = "c".repeat(64);
    let (zip, p) = release(dir.path(), &fp);
    let total = p.size;
    let fake = FakeGuide::new(Ok(Some(p)), zip);

    let mut seen = Vec::new();
    let index = download(&help, &fake, |r, t| seen.push((r, t))).await.expect("downloaded");

    assert_eq!(index, help.join(&fp).join("index.html"));
    assert_eq!(fs::read(&index).unwrap(), b"<html>new");
    assert_eq!(read_installed(&help).unwrap().0.fingerprint.as_deref(), Some(fp.as_str()));
    assert!(!help.join("old").exists(), "the old guide is replaced");
    assert_eq!(seen.last(), Some(&(total, total)));
    assert!(seen.len() >= 2, "progress is reported as it goes: {seen:?}");

    // The zip was a temp file outside help/, and it is gone.
    let tmp = fake.written_to();
    assert!(tmp.starts_with(std::env::temp_dir()), "{}", tmp.display());
    assert!(!tmp.starts_with(&help));
    assert!(!tmp.exists());
    assert!(incoming_left(&help).is_empty());

    // Straight after, the guide reads Ready - without asking the release again.
    let s = status(&help, &fake).await;
    assert_eq!(s.state, GuideState::Ready);
    assert_eq!(fake.asked(), 1);
    assert_eq!(installed_index(&help), Some(index));
    forget_published();
}

#[tokio::test]
async fn a_failed_download_leaves_the_old_guide_and_no_temp_file() {
    let _g = crate::serial::guide();
    forget_published();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let rec = old_guide(&help);
    let (zip, p) = release(dir.path(), &"d".repeat(64));
    let mut fake = FakeGuide::new(Ok(Some(p)), zip);
    fake.fail_midway = true;

    assert_eq!(download(&help, &fake, |_, _| {}).await, Err(DOWNLOAD_FAILED.to_string()));

    assert_eq!(fs::read(help.join("old/index.html")).unwrap(), b"old");
    assert_eq!(fs::read_to_string(help.join("installed.json")).unwrap(), rec);
    assert_eq!(names_in(&help), vec!["installed.json".to_string(), "old".to_string()]);
    assert!(!fake.written_to().exists(), "the half-written zip is removed");
    assert_eq!(installed_index(&help), Some(help.join("old").join("index.html")));
    forget_published();
}

#[tokio::test]
async fn a_damaged_download_is_not_kept_and_no_temp_file_is_left() {
    let _g = crate::serial::guide();
    forget_published();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let rec = old_guide(&help);
    let (mut zip, p) = release(dir.path(), &"e".repeat(64));
    let last = zip.len() - 1;
    zip[last] ^= 0xff; // same size, wrong checksum
    let fake = FakeGuide::new(Ok(Some(p)), zip);

    assert_eq!(download(&help, &fake, |_, _| {}).await, Err(DAMAGED.to_string()));
    assert_eq!(fs::read_to_string(help.join("installed.json")).unwrap(), rec);
    assert!(!fake.written_to().exists());
    assert!(incoming_left(&help).is_empty());
    forget_published();
}

/// A json that does not match its zip (a stale or mis-uploaded one) is not
/// kept for the run after DAMAGED: the next try asks for it again, and a
/// corrected one then installs.
#[tokio::test]
async fn a_damaged_download_asks_for_the_json_again() {
    let _g = crate::serial::guide();
    forget_published();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let rec = old_guide(&help);
    let fp = "7".repeat(64);
    let (zip, good) = release(dir.path(), &fp);
    let mut stale = good.clone();
    stale.sha256 = "0".repeat(64);
    let fake = FakeGuide::new(Ok(Some(good)), zip);
    fake.queued.lock().unwrap().push_back(Ok(Some(stale)));

    assert_eq!(download(&help, &fake, |_, _| {}).await, Err(DAMAGED.to_string()));
    assert_eq!(fake.asked(), 1);
    assert_eq!(fs::read_to_string(help.join("installed.json")).unwrap(), rec);

    let index = download(&help, &fake, |_, _| {}).await.expect("the retry fetched the corrected json");
    assert_eq!(fake.asked(), 2, "the json was asked for again");
    assert_eq!(index, help.join(&fp).join("index.html"));
    forget_published();
}

#[tokio::test]
async fn not_published_says_so() {
    let _g = crate::serial::guide();
    forget_published();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    old_guide(&help);
    let fake = FakeGuide::new(Ok(None), Vec::new());

    // Nothing to compare against: the guide on disk is Ready.
    assert_eq!(status(&help, &fake).await.state, GuideState::Ready);
    assert_eq!(download(&help, &fake, |_, _| {}).await, Err(NOT_PUBLISHED.to_string()));
    assert_eq!(fake.downloads(), 0);
    assert_eq!(fs::read(help.join("old/index.html")).unwrap(), b"old");
    forget_published();
}

#[tokio::test]
async fn an_unusable_published_json_is_ignored() {
    let _g = crate::serial::guide();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    old_guide(&help);
    let (zip, good) = release(dir.path(), &"f".repeat(64));

    let mut upper = good.clone();
    upper.sha256 = upper.sha256.to_uppercase();
    let mut short_fp = good.clone();
    short_fp.fingerprint = "abc".into();
    let mut not_hex = good.clone();
    not_hex.fingerprint = "g".repeat(64);
    let mut too_big = good.clone();
    too_big.size = MAX_ZIP_BYTES + 1;

    for bad in [upper, short_fp, not_hex, too_big] {
        forget_published();
        let fake = FakeGuide::new(Ok(Some(bad)), zip.clone());
        let s = status(&help, &fake).await;
        assert_eq!((s.state, s.size), (GuideState::Ready, None), "treated as could not be fetched");
        assert_eq!(download(&help, &fake, |_, _| {}).await, Err(DOWNLOAD_FAILED.to_string()));
        assert_eq!(fake.downloads(), 0);
    }
    assert_eq!(fs::read(help.join("old/index.html")).unwrap(), b"old");
    forget_published();
}

#[tokio::test]
async fn a_second_download_while_one_runs_is_refused() {
    let _g = crate::serial::guide();
    forget_published();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let fp = "a".repeat(64);
    let (zip, p) = release(dir.path(), &fp);
    let (started_tx, started_rx) = oneshot::channel();
    let (gate_tx, gate_rx) = oneshot::channel();
    let fake = Arc::new(FakeGuide::new(Ok(Some(p)), zip));
    *fake.started.lock().unwrap() = Some(started_tx);
    *fake.gate.lock().unwrap() = Some(gate_rx);

    let first = {
        let (fake, help) = (fake.clone(), help.clone());
        tokio::spawn(async move { download(&help, &*fake, |_, _| {}).await })
    };
    started_rx.await.unwrap();

    assert_eq!(download(&help, &*fake, |_, _| {}).await, Err(DOWNLOAD_FAILED.to_string()));
    gate_tx.send(()).unwrap();
    assert_eq!(first.await.unwrap(), Ok(help.join(&fp).join("index.html")));
    assert_eq!(fake.downloads(), 1, "one download");

    // The claim is released once the first finishes.
    assert!(download(&help, &*fake, |_, _| {}).await.is_ok());
    forget_published();
}

#[test]
fn the_guide_to_open_is_the_installed_one() {
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    assert_eq!(installed_index(&help), None);

    // An older install's guide is adopted and opened.
    put(&help, "2.0.4/index.html", b"legacy");
    assert_eq!(installed_index(&help), Some(help.join("2.0.4").join("index.html")));
}

/// This process's download temp files, if any are left.
fn temp_zips_left() -> Vec<String> {
    let prefix = format!("phr-tcm-how-to-use-{}-", std::process::id());
    names_in(&std::env::temp_dir()).into_iter().filter(|n| n.starts_with(&prefix)).collect()
}

/// A server that takes the connection and then never answers: the real
/// download path gives up after the quiet period (60 s in the app, a
/// fraction of a second here) instead of waiting forever for the headers.
/// The download-failed sentence comes back, the claim is released, and no
/// temp file is left.
#[tokio::test]
async fn a_server_that_never_answers_is_given_up_on() {
    let _g = crate::serial::guide();
    forget_published();
    let dir = tempfile::tempdir().unwrap();
    let help = dir.path().join("help");
    let rec = old_guide(&help);
    let fp = "9".repeat(64);
    let (zip, p) = release(dir.path(), &fp);
    // This run already has the json, so the download goes straight to the zip.
    let fake = FakeGuide::new(Ok(Some(p)), zip);
    assert_eq!(status(&help, &fake).await.state, GuideState::UpdateAvailable);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicU32::new(0));
    let server = {
        let accepted = accepted.clone();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((conn, _)) = listener.accept().await {
                accepted.fetch_add(1, Ordering::SeqCst);
                held.push(conn); // kept open, never written to
            }
        })
    };

    let quiet = Duration::from_millis(300);
    let started = Instant::now();
    let got = tokio::time::timeout(
        Duration::from_secs(20),
        download(&help, &GithubGuide::loopback(port, quiet), |_, _| {}),
    )
    .await
    .expect("the download gave up by itself");
    assert_eq!(got, Err(DOWNLOAD_FAILED.to_string()));
    assert!(started.elapsed() >= quiet, "it waited the quiet period first");
    assert!(accepted.load(Ordering::SeqCst) >= 1, "the request reached the server");
    server.abort();

    assert!(temp_zips_left().is_empty(), "{:?}", temp_zips_left());
    assert_eq!(fs::read_to_string(help.join("installed.json")).unwrap(), rec);
    assert!(incoming_left(&help).is_empty());

    // The claim is released: the next download runs (and installs).
    assert_eq!(download(&help, &fake, |_, _| {}).await, Ok(help.join(&fp).join("index.html")));
    forget_published();
}
