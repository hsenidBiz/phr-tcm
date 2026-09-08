//! The watched-import-file notifier: real filesystem, real OS events.
//!
//! These are timing tests by nature, so the waits are generous - a slow
//! machine should make them slower, never flaky.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use v2_lib::filewatch::{start_with, stop, watched_paths, FileWatchState};

/// Long enough for the coalesce window plus OS delivery.
const SETTLE: Duration = Duration::from_secs(5);
/// How long "nothing should happen" waits before believing it.
const QUIET: Duration = Duration::from_millis(1200);

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir(tag: &str) -> TempDir {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tcm-filewatch-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

fn armed(path: &Path) -> (FileWatchState, mpsc::Receiver<String>) {
    let state = FileWatchState::default();
    let (tx, rx) = mpsc::channel();
    start_with(&state, &path.to_string_lossy(), move |stamp| {
        let _ = tx.send(stamp);
    })
    .unwrap();
    // Give the OS watch a moment to actually be in place before the test
    // starts writing, or the first edit can be missed.
    std::thread::sleep(Duration::from_millis(300));
    (state, rx)
}

#[test]
fn an_edit_notifies_once_with_the_new_fingerprint() {
    let dir = temp_dir("edit");
    let file = dir.0.join("cases.json");
    std::fs::write(&file, r#"{"test_cases":[]}"#).unwrap();
    let (_state, rx) = armed(&file);

    std::fs::write(&file, r#"{"test_cases":[{"title":"A"}]}"#).unwrap();

    let stamp = rx.recv_timeout(SETTLE).expect("the edit was reported");
    assert_eq!(stamp, v2_lib::filewatch::stamp(&file).unwrap());
    // The burst of events one save raises must collapse to a single report.
    assert!(
        rx.recv_timeout(QUIET).is_err(),
        "one save must not report twice"
    );
}

/// The reason the PARENT DIRECTORY is watched: assistants and editors save
/// by writing a temp file and renaming it over the target, which replaces
/// the file a direct watch would be bound to. A second such save must
/// still be seen - that is what a dead watch would fail.
#[test]
fn a_write_and_rename_save_is_seen_every_time() {
    let dir = temp_dir("rename");
    let file = dir.0.join("cases.json");
    std::fs::write(&file, "one").unwrap();
    let (_state, rx) = armed(&file);

    for body in ["two", "three"] {
        let tmp = dir.0.join(format!("cases.json.{body}.tmp"));
        std::fs::write(&tmp, body).unwrap();
        std::fs::rename(&tmp, &file).unwrap();
        let stamp = rx.recv_timeout(SETTLE).unwrap_or_else(|_| {
            panic!("rename-save of {body:?} was not reported - the watch went deaf")
        });
        assert_eq!(stamp, v2_lib::filewatch::stamp(&file).unwrap());
    }
}

#[test]
fn a_save_that_changes_nothing_is_not_reported() {
    let dir = temp_dir("noop");
    let file = dir.0.join("cases.json");
    std::fs::write(&file, "same").unwrap();
    let (_state, rx) = armed(&file);

    std::fs::write(&file, "same").unwrap();

    assert!(
        rx.recv_timeout(QUIET).is_err(),
        "identical bytes are not a change"
    );
}

#[test]
fn edits_to_other_files_in_the_folder_are_ignored() {
    let dir = temp_dir("sibling");
    let file = dir.0.join("cases.json");
    std::fs::write(&file, "mine").unwrap();
    let (_state, rx) = armed(&file);

    std::fs::write(dir.0.join("notes.json"), "someone else's").unwrap();

    assert!(
        rx.recv_timeout(QUIET).is_err(),
        "a directory watch must still only report OUR file"
    );
}

#[test]
fn stopping_ends_the_watch() {
    let dir = temp_dir("stop");
    let file = dir.0.join("cases.json");
    std::fs::write(&file, "before").unwrap();
    let (state, rx) = armed(&file);

    stop(&state, &file.to_string_lossy());
    std::thread::sleep(Duration::from_millis(200));
    std::fs::write(&file, "after").unwrap();

    assert!(
        rx.recv_timeout(QUIET).is_err(),
        "a stopped watch reports nothing"
    );
}

#[test]
fn watching_a_path_with_no_folder_fails_instead_of_panicking() {
    let state = FileWatchState::default();
    assert!(start_with(&state, "cases.json", |_| {}).is_err());
}

/// Several JSON files can feed one queue, so dropping one watch must not
/// disturb the others.
#[test]
fn watches_are_independent() {
    let dir = temp_dir("multi");
    let a = dir.0.join("alpha.json");
    let b = dir.0.join("beta.json");
    std::fs::write(&a, "a1").unwrap();
    std::fs::write(&b, "b1").unwrap();

    let state = FileWatchState::default();
    let (tx_a, rx_a) = mpsc::channel();
    let (tx_b, rx_b) = mpsc::channel();
    start_with(&state, &a.to_string_lossy(), move |s| {
        let _ = tx_a.send(s);
    })
    .unwrap();
    start_with(&state, &b.to_string_lossy(), move |s| {
        let _ = tx_b.send(s);
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));

    assert_eq!(watched_paths(&state).len(), 2);

    // Drop alpha only.
    stop(&state, &a.to_string_lossy());
    std::thread::sleep(Duration::from_millis(200));
    std::fs::write(&a, "a2").unwrap();
    std::fs::write(&b, "b2").unwrap();

    assert!(rx_a.recv_timeout(QUIET).is_err(), "the dropped watch is silent");
    assert!(rx_b.recv_timeout(SETTLE).is_ok(), "the surviving watch still reports");
    assert_eq!(watched_paths(&state), vec![b.to_string_lossy().to_string()]);
}

#[test]
fn re_watching_a_path_replaces_only_that_watch() {
    let dir = temp_dir("rewatch");
    let file = dir.0.join("cases.json");
    std::fs::write(&file, "one").unwrap();
    let state = FileWatchState::default();
    for _ in 0..3 {
        start_with(&state, &file.to_string_lossy(), |_| {}).unwrap();
    }
    assert_eq!(watched_paths(&state).len(), 1, "no duplicate watches pile up");
}

#[test]
fn stopping_an_unwatched_path_is_harmless() {
    let state = FileWatchState::default();
    stop(&state, r"C:\nothing\here.json"); // must not panic
    assert!(watched_paths(&state).is_empty());
}

/// A comment typed in the report page is written back into the watched
/// file BY THE APP. That write is not news to the app that made it -
/// reporting it would show the user a change report for their own typing -
/// so `write_watched` claims the fingerprint and the watch stays quiet.
#[test]
fn the_apps_own_write_is_not_reported_back() {
    let dir = temp_dir("selfwrite");
    let file = dir.0.join("cases.json");
    std::fs::write(&file, r#"{"test_cases":[]}"#).unwrap();
    let (state, rx) = armed(&file);

    let path = file.to_string_lossy().to_string();
    let stamp = v2_lib::filewatch::write_watched(&state, &path, r#"{"comments":"mine"}"#).unwrap();
    assert_eq!(stamp, v2_lib::filewatch::stamp(&file).unwrap());
    assert!(
        rx.recv_timeout(QUIET).is_err(),
        "the app's own write must not come back as a change"
    );

    // ...and the watch is still live: the NEXT outside edit is reported.
    std::fs::write(&file, r#"{"comments":"theirs"}"#).unwrap();
    let seen = rx.recv_timeout(SETTLE).expect("an outside edit is still seen");
    assert_eq!(seen, v2_lib::filewatch::stamp(&file).unwrap());
}

/// Only ONE write is absorbed. If an assistant happened to write the same
/// bytes twice, the second is a real change from the app's point of view.
#[test]
fn only_the_claimed_write_is_absorbed() {
    let dir = temp_dir("claimonce");
    let file = dir.0.join("cases.json");
    std::fs::write(&file, "start").unwrap();
    let (state, rx) = armed(&file);
    let path = file.to_string_lossy().to_string();

    v2_lib::filewatch::write_watched(&state, &path, "ours").unwrap();
    assert!(rx.recv_timeout(QUIET).is_err());

    std::fs::write(&file, "someone else").unwrap();
    assert!(rx.recv_timeout(SETTLE).is_ok(), "the next edit is reported");
}

/// The watch is on the DIRECTORY, not the file, so every event beside our
/// file also reset the 200 ms coalescing window. A folder with any
/// background traffic - a log being appended, a sync client, a download
/// target - never went quiet for 200 ms, the drain never ended, and the
/// change was never emitted: the followed file silently stopped updating
/// the queue.
#[test]
fn a_busy_folder_cannot_starve_the_change_notification() {
    let dir = std::env::temp_dir().join(format!("tcm-busy-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let target = dir.join("cases.json");
    std::fs::write(&target, r#"{"test_cases":[]}"#).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let state = v2_lib::filewatch::FileWatchState::default();
    v2_lib::filewatch::start_with(&state, target.to_str().unwrap(), move |p| {
        let _ = tx.send(p);
    })
    .unwrap();

    // A noisy neighbour in the same folder, writing faster than the 200 ms
    // window - which is what used to hold the drain open indefinitely.
    let noisy = dir.clone();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_writer = stop.clone();
    let writer = std::thread::spawn(move || {
        let mut n = 0u32;
        while !stop_writer.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = std::fs::write(noisy.join("build.log"), format!("line {n}"));
            n += 1;
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    std::thread::sleep(std::time::Duration::from_millis(300));
    std::fs::write(&target, r#"{"test_cases":[{"title":"New","steps":[]}]}"#).unwrap();

    // MAX_COALESCE is 2s, so this must arrive well inside 10 even while the
    // folder never goes quiet.
    let got = rx.recv_timeout(std::time::Duration::from_secs(10));
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = writer.join();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(got.is_ok(), "the change was never emitted while the folder stayed busy");
}
