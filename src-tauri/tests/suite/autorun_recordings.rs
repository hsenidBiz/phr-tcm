//! Discovery recordings: kept in `recordings/` with an index, apart from the
//! failed-case videos (never pruned or cleared with them), served through the
//! same video readers.

use v2_lib::autorun::recordings::{self, add, clean_title, list, remove, safe_recording_name};
use v2_lib::autorun::{roots, store};

const MP4: &[u8] = b"\x00\x00\x00\x18ftypmp42-a-recording";

fn root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    store::set_root(dir.path().to_path_buf());
    dir
}

#[test]
fn titles_are_cleaned_and_bad_ones_refused() {
    assert_eq!(clean_title("  Bug 4521:\n leave\tbalance fix ").unwrap(), "Bug 4521: leave balance fix");
    assert_eq!(clean_title(&"x".repeat(120)).unwrap().chars().count(), recordings::MAX_TITLE);
    let blank = "a recording needs a short title, e.g. \"Bug 4521: leave balance fix\"";
    assert_eq!(clean_title("").unwrap_err(), blank);
    assert_eq!(clean_title("   ").unwrap_err(), blank);
    let address = "a recording's title names no address";
    assert_eq!(clean_title("see https://app/x").unwrap_err(), address);
    assert_eq!(clean_title("www.app.com").unwrap_err(), address);
    assert_eq!(clean_title("https:\u{200B}//x").unwrap_err(), address);
    assert_eq!(clean_title("https:\u{202E}//x").unwrap_err(), address);
}

#[test]
fn a_recording_is_kept_listed_newest_first_and_read_back() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let first = add(dir.path(), "first", 1500, MP4).unwrap();
    let second = add(dir.path(), "second", 2500, b"\x00\x00\x00\x18ftypmp42-other").unwrap();
    let listed = list(dir.path());
    assert_eq!(listed, vec![second.clone(), first.clone()]);
    for (e, ms) in [(&first, 1500), (&second, 2500)] {
        assert!(safe_recording_name(&e.file), "{}", e.file);
        assert_eq!(e.duration_ms, ms);
        assert_eq!(e.file, format!("{}.mp4", e.id));
    }
    assert_eq!(store::load_video(dir.path(), &first.file).unwrap(), MP4);
    assert_eq!(store::video_size(dir.path(), &first.file), Some(MP4.len() as u64));
}

#[test]
fn removing_a_recording_deletes_its_file_and_entry() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let e = add(dir.path(), "gone soon", 10, MP4).unwrap();
    remove(dir.path(), &e.id).unwrap();
    assert!(list(dir.path()).is_empty());
    assert!(!recordings::recordings_dir(dir.path()).join(&e.file).exists());
    assert_eq!(remove(dir.path(), &e.id).unwrap_err(), "that recording is gone");
}

#[test]
fn recordings_are_never_pruned_or_cleared_with_failed_case_videos() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let e = add(dir.path(), "kept", 10, MP4).unwrap();
    store::prune_videos_to(dir.path(), 0);
    store::clear_runs(dir.path()).unwrap();
    assert_eq!(list(dir.path()), vec![e.clone()]);
    assert!(recordings::recordings_dir(dir.path()).join(&e.file).is_file());
}

#[test]
fn a_recording_name_is_never_a_failed_case_video_name() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let e = add(dir.path(), "named", 10, MP4).unwrap();
    assert!(!store::safe_video_name(&e.file));
    for bad in ["video-1-1.mp4", "../x.mp4", "recording-1-1.mp4.exe", "recording-1.mp4", "recording-1-1-1.mp4", "recording--1.mp4"] {
        assert!(!safe_recording_name(bad), "{bad}");
    }
    assert!(safe_recording_name("recording-1-1.mp4"));
}

#[test]
fn an_unreadable_index_is_set_aside_and_recording_goes_on() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let folder = recordings::recordings_dir(dir.path());
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("index.json"), "{not json").unwrap();
    assert!(list(dir.path()).is_empty());
    let e = add(dir.path(), "after", 10, MP4).unwrap();
    assert_eq!(list(dir.path()), vec![e]);
    let aside = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .any(|f| f.file_name().to_string_lossy().starts_with("index.unreadable-"));
    assert!(aside, "the unreadable index was not set aside");
}

#[test]
fn a_recording_for_an_environment_no_longer_active_is_not_written() {
    let _lock = crate::serial::autorun();
    let shared = tempfile::tempdir().unwrap();
    let (a, b) = (roots::env_root(shared.path(), "env-0000000a"), roots::env_root(shared.path(), "env-0000000b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    store::set_root(a);
    assert!(add(&b, "stale", 10, MP4).is_err());
    let wrote = std::fs::read_dir(recordings::recordings_dir(&b))
        .map(|d| d.flatten().any(|f| f.file_name().to_string_lossy().ends_with(".mp4")))
        .unwrap_or(false);
    assert!(!wrote, "a video was written for an inactive environment");
}

#[test]
fn an_empty_video_is_not_kept() {
    let _lock = crate::serial::autorun();
    let dir = root();
    assert_eq!(add(dir.path(), "t", 1, &[]).unwrap_err(), "the browser gave no video");
    assert!(list(dir.path()).is_empty());
}

#[test]
fn an_index_that_is_not_utf8_is_set_aside_with_its_bytes() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let folder = recordings::recordings_dir(dir.path());
    std::fs::create_dir_all(&folder).unwrap();
    let bad: &[u8] = b"{\"recordings\":[]}\xff\xfe";
    std::fs::write(folder.join("index.json"), bad).unwrap();
    assert!(list(dir.path()).is_empty());
    assert!(folder.join("index.json").exists(), "list must not set it aside");
    let e = add(dir.path(), "after", 10, MP4).unwrap();
    assert_eq!(list(dir.path()), vec![e]);
    let aside: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .filter(|f| f.file_name().to_string_lossy().starts_with("index.unreadable-"))
        .collect();
    assert_eq!(aside.len(), 1);
    assert_eq!(std::fs::read(aside[0].path()).unwrap(), bad);
}

#[test]
fn concurrent_adds_all_land_in_the_index() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let path = dir.path().to_path_buf();
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let path = path.clone();
            std::thread::spawn(move || add(&path, &format!("rec {i}"), i, MP4).unwrap())
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(list(&path).len(), 8);
    let mp4s = std::fs::read_dir(recordings::recordings_dir(&path))
        .unwrap()
        .flatten()
        .filter(|f| f.file_name().to_string_lossy().ends_with(".mp4"))
        .count();
    assert_eq!(mp4s, 8);
}

#[test]
fn a_failed_index_write_removes_the_file_just_written() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let folder = recordings::recordings_dir(dir.path());
    std::fs::create_dir_all(folder.join("index.json")).unwrap();
    assert!(add(dir.path(), "t", 1, MP4).is_err());
    let mp4s = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .filter(|f| f.file_name().to_string_lossy().ends_with(".mp4"))
        .count();
    assert_eq!(mp4s, 0);
}

#[test]
fn a_listed_entry_whose_file_is_missing_is_left_out() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let keep = add(dir.path(), "keep", 1, MP4).unwrap();
    let lost = add(dir.path(), "lost", 1, MP4).unwrap();
    std::fs::remove_file(recordings::recordings_dir(dir.path()).join(&lost.file)).unwrap();
    assert_eq!(list(dir.path()), vec![keep]);
}
