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
    assert_eq!(clean_title("https:\u{E0041}//x").unwrap_err(), address);
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
fn a_marks_list_over_the_cap_is_cut() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let marks: Vec<v2_lib::autorun::VideoMark> =
        (0..recordings::MAX_MARKS as u32 + 1).map(|i| v2_lib::autorun::VideoMark { at_ms: i, label: "Click".into() }).collect();
    assert_eq!(marks.len(), 2_001);
    let e = recordings::add_with_marks(dir.path(), "many", 10, MP4, marks).unwrap();
    assert_eq!(recordings::MAX_MARKS, 2_000);
    assert_eq!(e.marks.len(), 2_000);
    assert_eq!(list(dir.path())[0].marks.len(), 2_000);
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
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                add(&path, &format!("rec {i}"), i, MP4).unwrap()
            })
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
    let before = add(dir.path(), "before", 1, MP4).unwrap();
    // A valid index that reads fine but cannot be replaced: read-only.
    let index = folder.join("index.json");
    let original = std::fs::read(&index).unwrap();
    let mut perms = std::fs::metadata(&index).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&index, perms.clone()).unwrap();
    let result = add(dir.path(), "t", 1, MP4);
    perms.set_readonly(false);
    std::fs::set_permissions(&index, perms).unwrap();
    assert!(result.is_err(), "replacing a read-only index should fail");
    let mp4s = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .filter(|f| f.file_name().to_string_lossy().ends_with(".mp4"))
        .count();
    assert_eq!(mp4s, 1, "only the first recording's file remains");
    assert_eq!(std::fs::read(&index).unwrap(), original);
    assert_eq!(list(dir.path()), vec![before]);
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


// ----------------------------------------------- a discovery's recording

use crate::autorun_discovery::{exploring, opened_for_discovery, slot, FakeBrowser};
use crate::common::{FakePage, ScriptedDriver};
use base64::Engine;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::ai_bridge::{
    budget_left, cap_step_in, close_browser_in, end_discovery_as, end_discovery_in, end_discovery_releasing, finish_recording_at_exit,
    keep_video, forget_gone_browser, CapStep, recording_kept, recording_start_in, recording_stop_in, EndedBy, ALREADY_RECORDING,
    NOT_RECORDING, RECORDING_LOST, RECORDING_NOT_AVAILABLE, RECORDING_NOT_STARTED, RECORDING_NO_DISCOVERY,
};
use v2_lib::browser::cdp::CdpError;
use v2_lib::commands::autorun_slots::{need_of, Need};

const T0: u64 = 5_000_000;
const TITLE: &str = "Bug 4521: leave balance fix";

/// A browser that records as asked: its product, whether the start and the
/// stop work, and one chunk of video for every stop. `stops` counts the stop
/// calls it was sent.
fn recorder(product: &'static str, start_ok: bool, stop_ok: bool, stops: Arc<AtomicUsize>) -> ScriptedDriver {
    let unread = Arc::new(Mutex::new(false));
    let page = FakePage::default();
    ScriptedDriver::new(move |method, params| match method {
        "Browser.getVersion" => Ok(json!({ "product": product })),
        "Page.startScreenRecording" if start_ok => Ok(json!({})),
        "Page.startScreenRecording" => Err(CdpError::Protocol { method: method.to_string(), message: "no".into() }),
        "Page.stopScreenRecording" => {
            stops.fetch_add(1, Ordering::SeqCst);
            if !stop_ok {
                return Err(CdpError::Protocol { method: method.to_string(), message: "no".into() });
            }
            *unread.lock().unwrap() = true;
            Ok(json!({ "stream": "stream-1" }))
        }
        "IO.read" => {
            let first = std::mem::replace(&mut *unread.lock().unwrap(), false);
            let data = if first { base64::engine::general_purpose::STANDARD.encode(MP4) } else { String::new() };
            Ok(json!({ "data": data, "base64Encoded": true, "eof": true }))
        }
        "IO.close" => Ok(json!({})),
        _ => page.answer(method, params),
    })
}

fn working() -> ScriptedDriver {
    recorder("Edg/157.0.3456.78", true, true, Arc::new(AtomicUsize::new(0)))
}

fn sent(b: &Option<FakeBrowser>, method: &str) -> usize {
    b.as_ref().unwrap().d.methods().iter().filter(|m| m.as_str() == method).count()
}

fn start_body(title: &str) -> String {
    json!({ "title": title }).to_string()
}

#[tokio::test]
async fn a_discovery_records_its_checks_and_keeps_the_video() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = slot(working(), exploring("Leave"));
    let (status, said) = recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await;
    assert_eq!(status, 200, "{said}");
    assert!(said.contains("Recording \"Bug 4521: leave balance fix\" started"), "{said}");
    // The length is read off the monotonic clock: it started 83 s ago.
    let running = b.as_mut().unwrap().discovery.as_mut().unwrap().recording.as_mut().unwrap();
    running.started = std::time::Instant::now() - std::time::Duration::from_secs(83);
    let (status, said) = recording_stop_in(&mut b).await;
    assert_eq!(status, 200, "{said}");
    let listed = list(dir.path());
    assert_eq!(listed.len(), 1);
    assert!((83_000..84_000).contains(&listed[0].duration_ms), "{}", listed[0].duration_ms);
    assert_eq!(said, recording_kept(TITLE, listed[0].duration_ms));
    assert!(said.contains("(1:23)"), "{said}");
    assert_eq!(listed[0].title, TITLE);
    assert_eq!(store::load_video(dir.path(), &listed[0].file).unwrap(), MP4);
}

#[tokio::test]
async fn a_second_start_is_refused_while_one_is_going() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = slot(working(), exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await.0, 200);
    let again = recording_start_in(&mut b, dir.path(), &start_body("another"), T0 + 1).await;
    assert_eq!(again, (409, ALREADY_RECORDING.to_string()));
    assert_eq!(sent(&b, "Page.startScreenRecording"), 1);
}

#[tokio::test]
async fn a_bad_title_or_body_is_a_400() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = slot(working(), exploring("Leave"));
    let (status, said) = recording_start_in(&mut b, dir.path(), &start_body("  "), T0).await;
    assert_eq!((status, said.as_str()), (400, "a recording needs a short title, e.g. \"Bug 4521: leave balance fix\""));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body("see https://x/y"), T0).await.0, 400);
    assert_eq!(recording_start_in(&mut b, dir.path(), "not json", T0).await.0, 400);
    assert_eq!(recording_start_in(&mut b, dir.path(), "{}", T0).await.0, 400);
    assert_eq!(sent(&b, "Page.startScreenRecording"), 0);
}

#[tokio::test]
async fn no_recording_without_a_signed_in_discovery() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let refused = (409, RECORDING_NO_DISCOVERY.to_string());
    let mut empty: Option<FakeBrowser> = None;
    assert_eq!(recording_start_in(&mut empty, dir.path(), &start_body(TITLE), T0).await, refused);
    let (mut not_signed_in, _) = slot(working(), opened_for_discovery());
    assert_eq!(recording_start_in(&mut not_signed_in, dir.path(), &start_body(TITLE), T0).await, refused);
    let (mut persons, _) = slot(working(), None);
    assert_eq!(recording_start_in(&mut persons, dir.path(), &start_body(TITLE), T0).await, refused);
    assert_eq!(sent(&persons, "Page.startScreenRecording"), 0);
}

#[tokio::test]
async fn an_old_browser_says_recording_is_not_available() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let old = recorder("Edg/152.0.1.1", true, true, Arc::new(AtomicUsize::new(0)));
    let (mut b, _) = slot(old, exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await, (409, RECORDING_NOT_AVAILABLE.to_string()));
    assert_eq!(sent(&b, "Page.startScreenRecording"), 0);
    assert!(b.as_mut().unwrap().discovery.is_some());
}

#[tokio::test]
async fn a_start_the_browser_refuses_says_so_and_the_discovery_goes_on() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let refusing = recorder("Edg/157.0.1.1", false, true, Arc::new(AtomicUsize::new(0)));
    let (mut b, _) = slot(refusing, exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await, (409, RECORDING_NOT_STARTED.to_string()));
    assert!(b.as_mut().unwrap().discovery.as_ref().unwrap().recording.is_none());
}

#[tokio::test]
async fn stopping_with_none_going_says_so() {
    let _lock = crate::serial::autorun();
    let _dir = root();
    let (mut b, _) = slot(working(), exploring("Leave"));
    assert_eq!(recording_stop_in(&mut b).await, (409, NOT_RECORDING.to_string()));
    let mut empty: Option<FakeBrowser> = None;
    assert_eq!(recording_stop_in(&mut empty).await, (409, NOT_RECORDING.to_string()));
}

#[tokio::test]
async fn a_stop_the_browser_refuses_keeps_nothing_and_says_so() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let refusing = recorder("Edg/157.0.1.1", true, false, Arc::new(AtomicUsize::new(0)));
    let (mut b, _) = slot(refusing, exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await.0, 200);
    assert_eq!(recording_stop_in(&mut b).await, (200, RECORDING_LOST.to_string()));
    assert!(list(dir.path()).is_empty());
    assert_eq!(recording_stop_in(&mut b).await, (409, NOT_RECORDING.to_string()));
}

async fn recording_slot(
    dir: &std::path::Path,
    assistant: bool,
    stops: Arc<AtomicUsize>,
) -> (Option<FakeBrowser>, Arc<std::sync::atomic::AtomicBool>) {
    let (mut b, closed) = slot(recorder("Edg/157.0.3456.78", true, true, stops), exploring("Leave"));
    b.as_mut().unwrap().assistant = assistant;
    assert_eq!(recording_start_in(&mut b, dir, &start_body(TITLE), T0).await.0, 200);
    (b, closed)
}

fn no_stops() -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(0))
}

#[tokio::test]
async fn ending_the_discovery_keeps_the_recording() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = recording_slot(dir.path(), false, no_stops()).await;
    assert_eq!(end_discovery_in(&mut b).await.0, 200);
    assert_eq!(list(dir.path()).len(), 1);
    let (mut b, _) = recording_slot(dir.path(), true, no_stops()).await;
    assert_eq!(end_discovery_releasing(&mut b).await.0, 200);
    assert_eq!(list(dir.path()).len(), 2);
}

#[tokio::test]
async fn a_persons_end_discovery_keeps_the_recording() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, closed) = recording_slot(dir.path(), true, no_stops()).await;
    assert_eq!(end_discovery_as(&mut b, EndedBy::Person).await.0, 200);
    assert_eq!(list(dir.path()).len(), 1);
    assert!(closed.load(Ordering::SeqCst) && b.is_none());
}

#[tokio::test]
async fn closing_the_browser_keeps_the_recording() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, closed) = recording_slot(dir.path(), true, no_stops()).await;
    assert!(close_browser_in(&mut b).await);
    assert_eq!(list(dir.path()).len(), 1);
    assert!(closed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn a_gone_browser_loses_its_recording_quietly() {
    let _log = crate::serial::log_tail();
    let _lock = crate::serial::autorun();
    let dir = root();
    let stops = no_stops();
    let (mut b, _) = recording_slot(dir.path(), true, stops.clone()).await;
    b.as_ref().unwrap().gone.store(true, Ordering::SeqCst);
    assert!(forget_gone_browser(&mut b).await);
    assert!(list(dir.path()).is_empty());
    assert!(b.is_none());
    assert_eq!(stops.load(Ordering::SeqCst), 0, "a gone browser is sent no stop call");
    let said = v2_lib::applog::recent(200).iter().any(|l| l.message == "Auto Test recording: lost, the browser had gone");
    assert!(said, "the loss is logged");
}

#[tokio::test]
async fn stopping_puts_the_assistants_tab_back() {
    use v2_lib::browser::cdp::Driver;
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = recording_slot(dir.path(), false, no_stops()).await;
    b.as_mut().unwrap().d.open_tab("popup").await.unwrap();
    assert_eq!(b.as_ref().unwrap().d.tab_name(), "popup");
    assert_eq!(recording_stop_in(&mut b).await.0, 200);
    assert_eq!(b.as_ref().unwrap().d.tab_name(), "popup", "the assistant is back on its own tab");
    assert_eq!(list(dir.path()).len(), 1);
    // The stream belongs to the recording's tab: it is read there, and the
    // assistant's tab is only put back after.
    let d = &b.as_ref().unwrap().d.tabs;
    assert!(d.called_in("main").iter().any(|m| m == "IO.read"), "{:?}", d.called_in("main"));
    assert!(!d.called_in("popup").iter().any(|m| m == "IO.read" || m == "IO.close"), "{:?}", d.called_in("popup"));
}

#[tokio::test]
async fn the_exit_stop_keeps_a_recording_that_answers_in_time() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = recording_slot(dir.path(), false, no_stops()).await;
    let video = finish_recording_at_exit(&mut b, std::time::Duration::from_secs(2)).await.expect("read in time");
    // Read, but not written: the caller writes it once its closes are under way.
    assert!(list(dir.path()).is_empty());
    assert!(keep_video(video).await.is_some());
    assert_eq!(list(dir.path()).len(), 1);
    assert!(b.as_mut().unwrap().discovery.as_ref().unwrap().recording.is_none());
}

#[tokio::test]
async fn the_exit_stop_gives_up_at_its_bound_and_says_so() {
    let _log = crate::serial::log_tail();
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = recording_slot(dir.path(), false, no_stops()).await;
    // The browser never answers the stop.
    b.as_mut().unwrap().d.hang_on = Some("Page.stopScreenRecording".to_string());
    let began = std::time::Instant::now();
    assert!(finish_recording_at_exit(&mut b, std::time::Duration::from_millis(100)).await.is_none());
    assert!(began.elapsed() < std::time::Duration::from_secs(2), "the exit is not held up");
    let said = v2_lib::applog::recent(200).iter().any(|l| l.message == "Auto Test recording: lost at exit");
    assert!(said, "the loss at exit is logged");
    assert!(list(dir.path()).is_empty(), "nothing was kept, so \"lost\" is true");
    assert!(b.as_mut().unwrap().discovery.as_ref().unwrap().recording.is_none());
    // Nothing going: nothing to wait for.
    let (mut idle, _) = slot(working(), exploring("Leave"));
    finish_recording_at_exit(&mut idle, std::time::Duration::ZERO).await;
}

#[test]
fn a_spent_exit_budget_is_zero() {
    use std::time::Duration;
    assert_eq!(budget_left(Duration::from_secs(1), Duration::from_millis(400)), Duration::from_millis(600));
    assert_eq!(budget_left(Duration::from_secs(1), Duration::from_secs(1)), Duration::ZERO);
    assert_eq!(budget_left(Duration::from_secs(1), Duration::from_secs(5)), Duration::ZERO);
}

#[tokio::test]
async fn the_cap_timer_waits_while_the_recording_is_not_due() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = recording_slot(dir.path(), false, no_stops()).await;
    // A wall clock that jumped far ahead changes nothing: due is monotonic.
    match cap_step_in(&mut b, T0).await {
        CapStep::Wait(left) => assert!(left > std::time::Duration::from_secs(590), "{left:?}"),
        other => panic!("{other:?}"),
    }
    assert!(list(dir.path()).is_empty());
    // A timer for another recording leaves this one alone.
    assert_eq!(cap_step_in(&mut b, T0 + 1).await, CapStep::Done);
    let (mut none, _) = slot(working(), exploring("Leave"));
    assert_eq!(cap_step_in(&mut none, T0).await, CapStep::Done);
}

#[tokio::test]
async fn the_cap_timer_stops_its_recording_once_the_monotonic_clock_says_so() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = recording_slot(dir.path(), false, no_stops()).await;
    let running = b.as_mut().unwrap().discovery.as_mut().unwrap().recording.as_mut().unwrap();
    running.started = std::time::Instant::now() - std::time::Duration::from_secs(601);
    assert_eq!(cap_step_in(&mut b, T0).await, CapStep::Done);
    assert_eq!(list(dir.path()).len(), 1);
    // The recording is gone: a later look has nothing to do.
    assert_eq!(cap_step_in(&mut b, T0).await, CapStep::Done);
}

#[test]
fn the_recording_routes_act_on_the_browser() {
    assert_eq!(need_of("POST", "/autorun-recording-start"), Some(Need::Act));
    assert_eq!(need_of("POST", "/autorun-recording-stop"), Some(Need::Act));
}

// ------------------------------------------- marks on a discovery's recording

use v2_lib::browser::cdp::Driver as _;
use v2_lib::ai_bridge::{begin_try_mark, discover_action_in, finish_try_mark, note_recording_mark, recording_now};
use v2_lib::autorun::VideoMark;
use v2_lib::browser::actions::Action;

#[tokio::test]
async fn a_discovery_recording_keeps_a_mark_for_each_action_tried() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = slot(working(), exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await.0, 200);
    let click: Action = serde_json::from_value(json!({ "kind": "click", "selector": "#save" })).unwrap();
    let _ = discover_action_in(&mut b, dir.path(), "Acme", "Web", &click, None, None).await;
    assert_eq!(recording_stop_in(&mut b).await.0, 200);
    let e = &list(dir.path())[0];
    assert_eq!(e.marks.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), ["Click"]);
    assert!(e.marks[0].at_ms <= e.duration_ms);
}

#[tokio::test]
async fn a_mark_needs_a_running_recording_and_names_no_secret() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let fill = v2_lib::autorun::action_help::fill_form_from(&json!({ "kind": "fill_form", "fields": { "#pw": "hunter2" } })).unwrap().unwrap();
    let mut none = None;
    assert_eq!(recording_now(&none), None);
    note_recording_mark(&mut none, Some(5), &fill, None);
    assert!(none.is_none());

    let (mut b, _) = slot(working(), exploring("Leave"));
    assert_eq!(recording_now(&b.as_mut().unwrap().discovery), None);
    note_recording_mark(&mut b.as_mut().unwrap().discovery, Some(5), &fill, None);
    assert!(b.as_ref().unwrap().discovery.as_ref().unwrap().recording.is_none());
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await.0, 200);
    let d = &mut b.as_mut().unwrap().discovery;
    let at = recording_now(d);
    assert!(at.is_some());
    note_recording_mark(d, None, &fill, None);
    note_recording_mark(d, at, &fill, Some("Type hunter2"));
    assert_eq!(d.as_ref().unwrap().recording.as_ref().unwrap().marks[0].label, "Fill the form");
    let marks = &d.as_ref().unwrap().recording.as_ref().unwrap().marks;
    assert_eq!(marks.len(), 1);
    assert!(!format!("{marks:?}").contains("hunter2"), "{marks:?}");
}

#[tokio::test]
async fn marks_stop_at_the_cap() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = slot(working(), exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await.0, 200);
    let click: Action = serde_json::from_value(json!({ "kind": "click", "selector": "#save" })).unwrap();
    let d = &mut b.as_mut().unwrap().discovery;
    for _ in 0..recordings::MAX_MARKS + 5 {
        note_recording_mark(d, Some(1), &click, None);
    }
    assert_eq!(d.as_ref().unwrap().recording.as_ref().unwrap().marks.len(), recordings::MAX_MARKS);
}

#[tokio::test]
async fn a_mark_noted_during_a_recording_is_kept_with_the_video() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = slot(working(), exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await.0, 200);
    let click: Action = serde_json::from_value(json!({ "kind": "click", "selector": "#save" })).unwrap();
    let at = recording_now(&b.as_mut().unwrap().discovery);
    note_recording_mark(&mut b.as_mut().unwrap().discovery, at, &click, None);
    assert_eq!(recording_stop_in(&mut b).await.0, 200);
    let e = &list(dir.path())[0];
    assert_eq!(e.marks.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), ["Click"]);
    assert_eq!(e.marks[0].at_ms, at.unwrap());
}

/// The marks and the length share one clock, and a mark is never later than
/// the length.
#[tokio::test]
async fn a_mark_is_never_later_than_the_kept_length() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = slot(working(), exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await.0, 200);
    let running = b.as_mut().unwrap().discovery.as_mut().unwrap().recording.as_mut().unwrap();
    running.marks.push(VideoMark { at_ms: u32::MAX, label: "Click".into() });
    assert_eq!(recording_stop_in(&mut b).await.0, 200);
    let e = &list(dir.path())[0];
    assert!(e.marks[0].at_ms <= e.duration_ms, "{} > {}", e.marks[0].at_ms, e.duration_ms);
}

// The try route's two steps, around the action (`autorun_try`).

async fn recording_now_slot(dir: &std::path::Path) -> Option<FakeBrowser> {
    let (mut b, _) = slot(working(), exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir, &start_body(TITLE), T0).await.0, 200);
    b
}

fn marks_of(b: &Option<FakeBrowser>) -> Vec<String> {
    let d = b.as_ref().unwrap().discovery.as_ref().unwrap();
    d.recording.as_ref().unwrap().marks.iter().map(|m| m.label.clone()).collect()
}

#[tokio::test]
async fn a_try_that_ran_is_marked_and_a_refused_one_is_not() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let mut b = recording_now_slot(dir.path()).await;
    let click: Action = serde_json::from_value(json!({ "kind": "click", "selector": "#save" })).unwrap();
    let browser = b.as_mut().unwrap();
    let at = begin_try_mark(&browser.discovery, &mut browser.d);
    assert!(at.is_some());
    finish_try_mark(&mut browser.discovery, &mut browser.d, at, &click, 200);
    let at = begin_try_mark(&browser.discovery, &mut browser.d);
    finish_try_mark(&mut browser.discovery, &mut browser.d, at, &click, 409);
    assert_eq!(marks_of(&b), ["Click"]);
}

#[tokio::test]
async fn a_try_with_no_recording_is_never_marked() {
    let _lock = crate::serial::autorun();
    let (mut b, _) = slot(working(), exploring("Leave"));
    let click: Action = serde_json::from_value(json!({ "kind": "click", "selector": "#save" })).unwrap();
    let browser = b.as_mut().unwrap();
    browser.d.drawn_label = Some("kept".into());
    let at = begin_try_mark(&browser.discovery, &mut browser.d);
    assert_eq!(at, None);
    assert_eq!(browser.d.drawn_label.as_deref(), Some("kept"), "nothing is dropped without a recording");
    finish_try_mark(&mut browser.discovery, &mut browser.d, at, &click, 200);
    assert!(browser.discovery.as_ref().unwrap().recording.is_none());
}

#[tokio::test]
async fn a_tried_fill_form_gets_one_fill_the_form_mark() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let mut b = recording_now_slot(dir.path()).await;
    let form = v2_lib::autorun::action_help::fill_form_from(&json!({ "kind": "fill_form", "fields": { "#a": "x", "#pw": "hunter2" } })).unwrap().unwrap();
    let browser = b.as_mut().unwrap();
    let at = begin_try_mark(&browser.discovery, &mut browser.d);
    // The fields drew their own boxes while the form ran.
    browser.d.note_drawn_label("Type password");
    finish_try_mark(&mut browser.discovery, &mut browser.d, at, &form, 200);
    assert_eq!(marks_of(&b), ["Fill the form"]);
}

#[tokio::test]
async fn a_label_drawn_before_a_try_is_not_reused_for_it() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let mut b = recording_now_slot(dir.path()).await;
    let click: Action = serde_json::from_value(json!({ "kind": "click", "selector": "#save" })).unwrap();
    let browser = b.as_mut().unwrap();
    browser.d.note_drawn_label("Stale from the try before");
    let at = begin_try_mark(&browser.discovery, &mut browser.d);
    finish_try_mark(&mut browser.discovery, &mut browser.d, at, &click, 200);
    assert_eq!(marks_of(&b), ["Click"]);
}
