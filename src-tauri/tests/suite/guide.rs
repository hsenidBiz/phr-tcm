//! The guide on disk: its fingerprint, its installed record, and whether
//! it is current. Pure file logic - nothing here touches the network.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};
use v2_lib::guide::{
    adopt_legacy, fingerprint, install, read_installed, state_for, unpack, GuideState, Installed,
    Published, DAMAGED,
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
