//! The video kept of a failed Auto Test case: the browser's own recorder
//! (`browser::recording`) started where the case's own work starts - before
//! its sign-in, so the video shows the whole flow - and stopped with the steps,
//! its file kept beside the pictures only for a case proposed Failed or
//! Blocked. The browser is a fake that answers the recorder's calls; the
//! real one is `browser_live`'s `a_failed_case_keeps_a_video_in_a_real_browser`.

use crate::common::{self, FakePage, ScriptedDriver};
use base64::Engine;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use v2_lib::autorun::replay::{run_selection, Browsers};
use v2_lib::autorun::{store, CaseRecord, CaseScript, LocalRun};
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::recording::{self, Support};

const MP4: &[u8] = b"\x00\x00\x00\x18ftypmp42-the-video-bytes";

/// How the fake recorder answers.
#[derive(Clone)]
struct Recorder {
    product: &'static str,
    start: Result<(), &'static str>,
    stop: Result<(), &'static str>,
    read: Result<(), &'static str>,
}

impl Recorder {
    fn working() -> Recorder {
        Recorder { product: "Edg/157.0.3456.78", start: Ok(()), stop: Ok(()), read: Ok(()) }
    }
}

fn refuse(method: &str, why: &str) -> CdpError {
    CdpError::Protocol { method: method.to_string(), message: why.to_string() }
}

/// A page that answers the recorder as `rec` says and everything else as
/// the page would. `passes` says whether the case's one check passes.
fn driver(rec: Recorder, passes: bool) -> ScriptedDriver {
    driver_on(rec, FakePage { body_has_text: passes, ..FakePage::default() })
}

fn driver_on(rec: Recorder, page: FakePage) -> ScriptedDriver {
    let sent = Arc::new(Mutex::new(false));
    ScriptedDriver::new(move |method, params| match method {
        "Browser.getVersion" => Ok(json!({ "product": rec.product })),
        "Page.startScreenRecording" => rec.start.map(|()| json!({})).map_err(|w| refuse(method, w)),
        "Page.stopScreenRecording" => rec.stop.map(|()| json!({ "stream": "stream-1" })).map_err(|w| refuse(method, w)),
        "IO.read" => rec.read.map_err(|w| refuse(method, w)).map(|()| {
            let first = !std::mem::replace(&mut *sent.lock().unwrap(), true);
            let data = if first { base64::engine::general_purpose::STANDARD.encode(MP4) } else { String::new() };
            json!({ "data": data, "base64Encoded": true, "eof": true })
        }),
        "IO.close" => Ok(json!({})),
        _ => page.answer(method, params),
    })
}

struct Fake {
    drivers: Vec<ScriptedDriver>,
    closed: Vec<ScriptedDriver>,
    wanted: bool,
}

impl Browsers for Fake {
    type D = ScriptedDriver;
    fn keep_failed_video(&self) -> bool {
        self.wanted
    }
    async fn open(&mut self) -> Result<ScriptedDriver, String> {
        if self.drivers.is_empty() {
            return Err("no browser".into());
        }
        Ok(self.drivers.remove(0))
    }
    async fn close(&mut self, d: ScriptedDriver) {
        self.closed.push(d);
    }
}

fn script(case_id: i32) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": case_id,
        "title": "c",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "x" }] }]
    }))
    .unwrap()
}

async fn go(root: &Path, wanted: bool, drivers: Vec<ScriptedDriver>, cases: &[i32]) -> (LocalRun, Vec<ScriptedDriver>) {
    let mut cs = vec![];
    for id in cases {
        store::save_script(root, &script(*id)).unwrap();
        cs.push((*id, "c".to_string()));
    }
    let mut browsers = Fake { drivers, closed: vec![], wanted };
    let mut run = LocalRun { video_unavailable: false,
        id: "run-v".into(),
        pbi_id: 42,
        started_at: "1700000000000".into(),
        cases: vec![],
        mode: "unattended".into(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cs, &common::quick(), &cancel, &mut |_| {}).await.unwrap();
    (run, browsers.closed)
}

fn pos(d: &ScriptedDriver, method: &str) -> Option<usize> {
    d.methods().iter().position(|m| m == method)
}

fn videos_on_disk(root: &Path) -> Vec<String> {
    std::fs::read_dir(root.join("shots"))
        .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".mp4")).collect())
        .unwrap_or_default()
}

// ---- Which browsers can record ----------------------------------------------------

#[test]
fn the_major_version_is_read_from_the_product() {
    assert_eq!(recording::major_of("Edg/157.0.3456.78"), Some(157));
    assert_eq!(recording::major_of("HeadlessChrome/153.0.1.2"), Some(153));
    assert_eq!(recording::major_of("Chrome/99"), Some(99));
    assert_eq!(recording::major_of("Edg/"), None);
    assert_eq!(recording::major_of("nonsense"), None);
}

#[tokio::test]
async fn a_browser_from_153_on_supports_recording_and_an_older_one_does_not() {
    for (product, yes) in [("Edg/157.0.1.2", true), ("Chrome/153.0.0.0", true), ("Edg/152.0.1.2", false), ("Edg/120.0.1.2", false)] {
        let mut d = driver(Recorder { product, ..Recorder::working() }, true);
        let s = recording::support(&mut d).await;
        assert_eq!(s.is_yes(), yes, "{product}: {s:?}");
    }
}

#[tokio::test]
async fn a_probe_that_fails_or_is_unreadable_is_taken_as_unsupported() {
    let mut refusing = ScriptedDriver::new(|m, _| Err(refuse(m, "no such method")));
    assert!(matches!(recording::support(&mut refusing).await, Support::No(_)));
    let mut junk = ScriptedDriver::new(|_, _| Ok(json!({ "product": "who knows" })));
    assert!(matches!(recording::support(&mut junk).await, Support::No(_)));
    let mut empty = ScriptedDriver::new(|_, _| Ok(json!({})));
    assert!(matches!(recording::support(&mut empty).await, Support::No(_)));
}

#[test]
fn recording_is_asked_for_small_files() {
    let p = recording::start_params();
    assert_eq!(p["maxWidth"], 1280);
    assert_eq!(p["maxHeight"], 720);
    assert_eq!(p["frameRate"], 30);
    assert_eq!(p["audio"], false);
}

// ---- A case's recording ---------------------------------------------------------------

#[tokio::test]
async fn a_failed_case_keeps_its_video_and_a_passed_one_keeps_none() {
    let dir = tempfile::tempdir().unwrap();
    let (run, closed) = go(dir.path(), true, vec![driver(Recorder::working(), false)], &[1]).await;
    let case = &run.cases[0];
    assert_eq!(case.proposed, "Failed");
    let name = case.video.as_deref().expect("a failed case keeps its video");
    assert!(store::safe_video_name(name), "{name}");
    assert_eq!(std::fs::read(dir.path().join("shots").join(name)).unwrap(), MP4);
    // Started before the step ran (and, with an account, before its sign-in:
    // `the_recording_starts_before_the_sign_in...` below), stopped after it,
    // read once, let go.
    let d = &closed[0];
    let (start, step, stop, read, close) = (
        pos(d, "Page.startScreenRecording").expect("started"),
        pos(d, "Runtime.callFunctionOn").expect("the step ran"),
        pos(d, "Page.stopScreenRecording").expect("stopped"),
        pos(d, "IO.read").expect("read"),
        pos(d, "IO.close").expect("closed"),
    );
    assert!(start < step && step < stop && stop < read && read < close, "{:?}", d.methods());

    let dir = tempfile::tempdir().unwrap();
    let (run, closed) = go(dir.path(), true, vec![driver(Recorder::working(), true)], &[2]).await;
    assert_eq!(run.cases[0].proposed, "Passed");
    assert_eq!(run.cases[0].video, None);
    assert!(videos_on_disk(dir.path()).is_empty());
    // Stopped, so the browser is not left recording, and let go unread.
    assert!(pos(&closed[0], "Page.stopScreenRecording").is_some());
    assert!(pos(&closed[0], "IO.read").is_none());
    assert!(pos(&closed[0], "IO.close").is_some());
}

#[tokio::test]
async fn with_the_setting_off_nothing_is_asked_of_the_browser() {
    let dir = tempfile::tempdir().unwrap();
    let (run, closed) = go(dir.path(), false, vec![driver(Recorder::working(), false)], &[1]).await;
    assert_eq!(run.cases[0].proposed, "Failed");
    assert_eq!(run.cases[0].video, None);
    assert_eq!(run.cases[0].notice, None);
    assert!(!run.video_unavailable);
    let m = closed[0].methods();
    assert!(m.iter().all(|x| !x.starts_with("Page.startScreen") && !x.starts_with("Page.stopScreen") && x != "Browser.getVersion"), "{m:?}");
}

#[tokio::test]
async fn an_old_browser_records_nothing_and_says_so_once() {
    let dir = tempfile::tempdir().unwrap();
    let old = Recorder { product: "Edg/140.0.1.1", ..Recorder::working() };
    let (run, closed) = go(dir.path(), true, vec![driver(old.clone(), false), driver(old, false)], &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Failed" && c.video.is_none()));
    // Said once, on the run, and never as a case's notice (the screens show a
    // notice as "Not checked").
    assert!(run.video_unavailable);
    assert!(run.cases.iter().all(|c| c.notice.is_none()), "{:?}", run.cases.iter().map(|c| &c.notice).collect::<Vec<_>>());
    // The browser was asked its version once, and never asked to record.
    assert_eq!(closed.iter().flat_map(|d| d.methods()).filter(|m| m == "Browser.getVersion").count(), 1);
    assert!(closed.iter().all(|d| pos(d, "Page.startScreenRecording").is_none()));
    assert!(!recording::NOT_AVAILABLE.contains("://") && !recording::NOT_AVAILABLE.contains('\\'));
}

#[tokio::test]
async fn a_recording_that_goes_wrong_never_changes_the_case_result() {
    let broken = [
        Recorder { start: Err("not now"), ..Recorder::working() },
        Recorder { stop: Err("no stream"), ..Recorder::working() },
        Recorder { read: Err("read failed"), ..Recorder::working() },
    ];
    let dir = tempfile::tempdir().unwrap();
    let (plain, _) = go(dir.path(), false, vec![driver(Recorder::working(), false)], &[1]).await;
    for rec in broken {
        let dir = tempfile::tempdir().unwrap();
        let (run, _) = go(dir.path(), true, vec![driver(rec, false)], &[1]).await;
        let case = &run.cases[0];
        assert_eq!(case.proposed, "Failed");
        assert_eq!(case.reason, plain.cases[0].reason, "{}", case.reason);
        assert_eq!(case.video, None);
        assert!(videos_on_disk(dir.path()).is_empty());
    }
    // And a passing case still passes when its recording fails.
    let dir = tempfile::tempdir().unwrap();
    let (run, _) = go(dir.path(), true, vec![driver(Recorder { start: Err("x"), ..Recorder::working() }, true)], &[1]).await;
    assert_eq!(run.cases[0].proposed, "Passed");
}

// ---- Where videos live ----------------------------------------------------------------

#[test]
fn a_video_name_is_checked_before_it_is_used() {
    for good in ["video-1700000000000-000001.mp4", "video-1-2.mp4"] {
        assert!(store::safe_video_name(good), "{good}");
    }
    for bad in [
        "",
        "video-1.mp4",
        "video-1-2-3.mp4",
        "video-a-2.mp4",
        "../shots/video-1-2.mp4",
        "video-1-2.mp4/../x",
        "video-1-2.jpg",
        "shot-1-2.jpg",
        "video-1-2.mp4 ",
        "..\\video-1-2.mp4",
        "video--2.mp4",
    ] {
        assert!(!store::safe_video_name(bad), "{bad:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(store::load_video(dir.path(), "../runs/x.json").is_err());
    assert_eq!(store::video_size(dir.path(), "../x"), None);
}

#[test]
fn a_saved_video_is_found_read_and_sized_like_a_picture() {
    let dir = tempfile::tempdir().unwrap();
    let name = store::save_video(dir.path(), MP4).unwrap();
    assert!(store::shot_exists(dir.path(), &name));
    assert_eq!(store::load_video(dir.path(), &name).unwrap(), MP4);
    assert_eq!(store::video_size(dir.path(), &name), Some(MP4.len() as u64));
    // A picture's name is not a video's.
    assert!(store::load_video(dir.path(), "shot-1-2.jpg").is_err());
    assert!(!store::shot_exists(dir.path(), "video-9-9.mp4"));
    // Two videos in one millisecond still get two names.
    let other = store::save_video(dir.path(), MP4).unwrap();
    assert_ne!(name, other);
}

#[test]
fn the_screens_are_told_what_a_video_can_be_used_for() {
    use v2_lib::commands::autorun::{video_info_at, PLAY_MAX_BYTES};
    let dir = tempfile::tempdir().unwrap();
    let name = store::save_video(dir.path(), MP4).unwrap();
    let info = video_info_at(dir.path(), &name).unwrap();
    assert_eq!((info.size as usize, info.playable, info.attachable), (MP4.len(), true, true));
    assert!(video_info_at(dir.path(), "video-1-1.mp4").is_none());
    assert!(video_info_at(dir.path(), "../x").is_none());
    assert!(PLAY_MAX_BYTES <= store::MAX_VIDEO_BYTES);
}

fn run_naming(video: &str, published: bool) -> LocalRun {
    let case = CaseRecord { video: Some(video.to_string()), ..common::component_case(1, "x") };
    LocalRun { video_unavailable: false,
        id: "r".into(),
        pbi_id: 1,
        started_at: "1".into(),
        cases: vec![case],
        mode: "unattended".into(),
        published: published.then(|| v2_lib::autorun::PublishedRun { run_id: 1, web_url: "w".into(), at: "1".into() }),
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    }
}

#[test]
fn old_videos_are_pruned_with_the_pictures_but_an_unpublished_runs_are_spared() {
    let dir = tempfile::tempdir().unwrap();
    let names: Vec<String> = (0..5).map(|_| store::save_video(dir.path(), MP4).unwrap()).collect();
    store::save_run(dir.path(), &run_naming(&names[0], false)).unwrap();
    store::prune_videos_to(dir.path(), 2);
    let left = videos_on_disk(dir.path());
    // The oldest is named by an unpublished run, so it stays; of the rest the newest two do.
    assert!(left.contains(&names[0]), "{left:?}");
    assert!(left.contains(&names[3]) && left.contains(&names[4]), "{left:?}");
    assert_eq!(left.len(), 3, "{left:?}");

    // Once the run is published it protects nothing.
    store::save_run(dir.path(), &run_naming(&names[0], true)).unwrap();
    store::prune_videos_to(dir.path(), 2);
    assert_eq!(videos_on_disk(dir.path()).len(), 2);
}

#[test]
fn clearing_the_runs_clears_their_videos_too() {
    let dir = tempfile::tempdir().unwrap();
    let name = store::save_video(dir.path(), MP4).unwrap();
    store::save_run(dir.path(), &run_naming(&name, false)).unwrap();
    store::clear_runs(dir.path()).unwrap();
    assert!(videos_on_disk(dir.path()).is_empty());
    assert!(!store::shot_exists(dir.path(), &name));
}

#[test]
fn a_run_file_with_a_video_reads_back_and_one_without_writes_no_field() {
    let dir = tempfile::tempdir().unwrap();
    let name = store::save_video(dir.path(), MP4).unwrap();
    store::save_run(dir.path(), &run_naming(&name, false)).unwrap();
    let back = store::load_run(dir.path(), "r").unwrap().unwrap();
    assert_eq!(back.cases[0].video.as_deref(), Some(name.as_str()));
    let plain = serde_json::to_value(CaseRecord { video: None, ..common::component_case(1, "x") }).unwrap();
    assert!(plain.get("video").is_none(), "{plain}");
}

// ---- What reads the video ---------------------------------------------------------------

#[test]
fn the_report_links_the_video_beside_the_pictures_and_never_embeds_it() {
    use v2_lib::autorun::report::build_with_downloads;
    let run = run_naming("video-1700000000000-000001.mp4", false);
    let html = build_with_downloads(&run, &[], "now", &|_| true, &|_| None);
    assert!(html.contains("href=\"../shots/video-1700000000000-000001.mp4\" data-video="), "{html}");
    // A tile captioned Video: the file's name is not shown as text.
    assert!(html.contains("<figcaption>Video</figcaption>"), "{html}");
    assert!(!html.contains(">video-1700000000000-000001.mp4<"), "{html}");
    assert!(!html.contains("<video") && !html.contains("<embed") && !html.contains("<iframe"), "{html}");
    // A video that is gone is said to be, not linked.
    let gone = build_with_downloads(&run, &[], "now", &|_| false, &|_| None);
    assert!(!gone.contains("href=\"../shots/video"), "{gone}");
    assert!(gone.contains("no longer on this machine"), "{gone}");
    // A name that could point elsewhere is never linked.
    let bad = build_with_downloads(&run_naming("../runs/x.json", false), &[], "now", &|_| true, &|_| None);
    assert!(!bad.contains("href=\"../shots/../runs"), "{bad}");
    assert!(bad.contains("is not a video name"), "{bad}");
}

#[test]
fn an_assistant_is_told_the_videos_name_and_never_a_path() {
    use v2_lib::autorun::failures::describe_failures_in;
    let dir = tempfile::tempdir().unwrap();
    let mut run = run_naming("video-1700000000000-000001.mp4", false);
    run.cases[0].verdict = "Failed".into();
    run.cases[0].proposed = "Failed".into();
    let text = describe_failures_in(Some(dir.path()), &run, &[], &Default::default());
    let line = text.lines().find(|l| l.contains("video")).unwrap_or_else(|| panic!("{text}"));
    assert_eq!(line, "video of the case: video-1700000000000-000001.mp4");
    assert!(!text.contains("shots"), "{text}");
}

// ---- Raise bug --------------------------------------------------------------------------

mod attach {
    use super::*;
    use v2_lib::ado::AdoClient;
    use v2_lib::commands::bugs::attach_video_with;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn server(uploads: u64) -> (MockServer, AdoClient) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/o/p/_apis/wit/attachments"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "url": "https://x/a" })))
            .expect(uploads)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/o/p/_apis/wit/workitems/9"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 9 })))
            .mount(&server)
            .await;
        let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
        (server, client)
    }

    #[tokio::test]
    async fn the_video_is_uploaded_to_the_bug_and_linked() {
        let dir = tempfile::tempdir().unwrap();
        let name = store::save_video(dir.path(), MP4).unwrap();
        let (server, client) = server(1).await;
        attach_video_with(&client, dir.path(), "o", "p", 9, &name).await.unwrap();
        let reqs = server.received_requests().await.unwrap();
        let upload = reqs.iter().find(|r| r.method.as_str() == "POST").unwrap();
        assert_eq!(upload.body, MP4);
        assert!(upload.url.query().unwrap_or_default().contains("bug-9-video.mp4"), "{:?}", upload.url);
        assert!(reqs.iter().any(|r| r.method.as_str() == "PATCH"));
        // Only POST and PATCH: never a delete.
        assert!(reqs.iter().all(|r| matches!(r.method.as_str(), "POST" | "PATCH")));
    }

    #[tokio::test]
    async fn a_name_that_is_not_a_video_or_is_gone_uploads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (_server, client) = server(0).await;
        for bad in ["../runs/x.json", "video-1-1.mp4", "shot-1-2.jpg"] {
            let why = attach_video_with(&client, dir.path(), "o", "p", 9, bad).await.unwrap_err();
            assert!(!why.contains("://") && !why.contains('\\'), "{why}");
        }
    }
}


// ---- Blocked, the retry and the network rerun ---------------------------------------

#[tokio::test]
async fn a_blocked_case_keeps_its_video_too() {
    let dir = tempfile::tempdir().unwrap();
    // A project that does not allow opening a page by address: the case cannot
    // be carried out, which is Blocked, not Failed.
    let nav = v2_lib::autorun::nav::NavFile { direct_urls: false, ..Default::default() };
    v2_lib::autorun::nav::save_nav(dir.path(), "Acme", "Web", &nav).unwrap();
    let blocked: CaseScript = serde_json::from_value(json!({
        "case_id": 1, "title": "c",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "navigate", "url": "http://app.test/x" }] }]
    }))
    .unwrap();
    store::save_script(dir.path(), &blocked).unwrap();
    let mut browsers = Fake { drivers: vec![driver(Recorder::working(), true)], closed: vec![], wanted: true };
    let mut run = new_run();
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, dir.path(), "Acme", "Web", &mut run, &[(1, "c".into())], &common::quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(run.cases[0].proposed, "Blocked", "{:?}", run.cases[0]);
    let name = run.cases[0].video.as_deref().expect("a blocked case keeps its video");
    assert!(dir.path().join("shots").join(name).is_file());
}

fn new_run() -> LocalRun {
    LocalRun {
        video_unavailable: false,
        id: "run-v".into(),
        pbi_id: 42,
        started_at: "1700000000000".into(),
        cases: vec![],
        mode: "unattended".into(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    }
}

/// A gate that says the network is gone once the case has failed once.
struct DropsOnce(Vec<v2_lib::autorun::network::Network>);

impl v2_lib::autorun::replay::ResetGate for DropsOnce {
    async fn wait(&mut self, _r: &v2_lib::autorun::plan::Pause, _rest: &[i32], _c: &AtomicBool) -> bool {
        true
    }
    async fn network(&mut self) -> v2_lib::autorun::network::Network {
        if self.0.is_empty() { v2_lib::autorun::network::Network::Reachable } else { self.0.remove(0) }
    }
    async fn wait_for_network(&mut self, _before: i32, _c: &AtomicBool) -> v2_lib::autorun::network::WaitEnd {
        v2_lib::autorun::network::WaitEnd::Resumed
    }
}

fn two_step_script() -> CaseScript {
    serde_json::from_value(json!({
        "case_id": 1, "title": "c",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "http://app.test/x" }] },
            { "step_number": 2, "actions": [{ "kind": "check_text", "value": "x" }] }
        ]
    }))
    .unwrap()
}

async fn plan_run(
    root: &Path,
    drivers: Vec<ScriptedDriver>,
    gate: &mut DropsOnce,
    retry: bool,
) -> LocalRun {
    use v2_lib::autorun::preconditions::{NoDb, PreconditionDb, NEED_DB};
    use v2_lib::autorun::replay::{run_cases_planned, CaseToRun, NeverPauses};
    let _ = NeverPauses;
    store::save_script(root, &two_step_script()).unwrap();
    let mut browsers = Fake { drivers, closed: vec![], wanted: true };
    let mut run = new_run();
    let no_db: PreconditionDb<NoDb> = PreconditionDb::Missing(NEED_DB.to_string());
    run_cases_planned(
        &mut browsers, root, "Acme", "Web", &mut run,
        &[CaseToRun { case_id: 1, title: "c".into(), module: None }],
        None, retry, &common::quick(), &AtomicBool::new(false), &no_db, &[], gate, &mut |_| {},
    )
    .await
    .unwrap();
    run
}

#[tokio::test]
async fn a_case_run_again_after_the_network_came_back_leaves_only_the_kept_videos_file() {
    use v2_lib::autorun::network::Network;
    let dir = tempfile::tempdir().unwrap();
    // Reachable before the case, gone after it failed, back for the second go.
    let mut gate = DropsOnce(vec![Network::Reachable, Network::Unreachable]);
    let failing = || driver(Recorder::working(), false);
    let run = plan_run(dir.path(), vec![failing(), failing()], &mut gate, false).await;
    assert_eq!(run.cases.len(), 1);
    let name = run.cases[0].video.clone().expect("the final record keeps its video");
    assert_eq!(videos_on_disk(dir.path()), vec![name], "the first go's video was left behind");
}

#[tokio::test]
async fn a_transient_retry_discards_the_first_tries_video() {
    let dir = tempfile::tempdir().unwrap();
    let mut gate = DropsOnce(vec![]);
    // The first go's page would not load (a network error: transient); the
    // second go fails the check for good and is the record.
    let glitch = FakePage { navigate_reply: json!({ "frameId": "F", "errorText": "net::ERR_CONNECTION_RESET" }), ..FakePage::default() };
    let first = driver_on(Recorder::working(), glitch);
    let second = driver(Recorder::working(), false);
    let run = plan_run(dir.path(), vec![first, second], &mut gate, true).await;
    let case = &run.cases[0];
    assert!(case.retried.is_some(), "the first go was not taken for transient: {case:?}");
    let name = case.video.clone().expect("the second go's video is kept");
    assert_eq!(videos_on_disk(dir.path()), vec![name], "the first try's video was left behind");
}

// ---- Where in the case the recording starts ---------------------------------------

fn script_as(case_id: i32, account: &str) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": case_id,
        "title": "c",
        "account": account,
        "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "x" }] }]
    }))
    .unwrap()
}

/// The owner decided (2026-11-23) the failed-case video shows the whole flow
/// from the sign-in to the error; it used to start after the sign-in, so no
/// typed account name was on it. The recording now starts before the first
/// call the sign-in makes and stops after the last step.
#[tokio::test]
async fn the_recording_starts_before_the_sign_in_and_stops_after_the_last_step() {
    let _leases = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    v2_lib::autorun::recipe::save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    v2_lib::autorun::accounts::save_accounts(root, &[common::account()]).unwrap();
    store::save_script(root, &script_as(1, "admin")).unwrap();
    let mut browsers = Fake { drivers: vec![driver(Recorder::working(), false)], closed: vec![], wanted: true };
    let mut run = LocalRun { video_unavailable: false, id: "run-v".into(), pbi_id: 42, started_at: "1700000000000".into(), cases: vec![], mode: "unattended".into(), published: None, environment: None, resets: vec![], network_pauses: vec![] };
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "c".to_string())], &common::quick(), &cancel, &mut |_| {}).await.unwrap();

    let case = &run.cases[0];
    assert!(matches!(case.proposed.as_str(), "Failed" | "Blocked"), "{}", case.proposed);
    assert!(case.video.is_some(), "a failed or blocked case keeps its video");
    let d = &browsers.closed[0];
    let m = d.methods();
    let start = pos(d, "Page.startScreenRecording").expect("started");
    let sign_in = pos(d, "Page.navigate").expect("the sign-in opened its page");
    let stop = pos(d, "Page.stopScreenRecording").expect("stopped");
    assert!(start < sign_in, "the recording starts before the sign-in, not after it: {m:?}");
    assert!(m[..start].iter().all(|x| !x.starts_with("Input.") && x != "Page.navigate"), "{m:?}");
    assert!(stop > start, "{m:?}");
    // Started once.
    assert_eq!(m.iter().filter(|x| *x == "Page.startScreenRecording").count(), 1);
}

/// A case stopped before it begins is never recorded: nothing of it ran, so
/// nothing is asked of the recorder.
#[tokio::test]
async fn a_case_stopped_before_it_starts_is_not_recorded() {
    let _leases = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    v2_lib::autorun::recipe::save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    v2_lib::autorun::accounts::save_accounts(root, &[common::account()]).unwrap();
    store::save_script(root, &script_as(1, "admin")).unwrap();
    let mut browsers = Fake { drivers: vec![driver(Recorder::working(), false)], closed: vec![], wanted: true };
    let mut run = LocalRun { video_unavailable: false, id: "run-v".into(), pbi_id: 42, started_at: "1700000000000".into(), cases: vec![], mode: "unattended".into(), published: None, environment: None, resets: vec![], network_pauses: vec![] };
    let cancel = AtomicBool::new(true);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "c".to_string())], &common::quick(), &cancel, &mut |_| {}).await.unwrap();
    for d in &browsers.closed {
        assert!(pos(d, "Page.startScreenRecording").is_none(), "{:?}", d.methods());
        assert!(pos(d, "Page.stopScreenRecording").is_none());
    }
    assert!(run.cases.iter().all(|c| c.video.is_none()));
    assert!(videos_on_disk(root).is_empty());
}

/// A case blocked before it starts (it has a divergence waiting for a
/// person's ruling) is never recorded either.
#[tokio::test]
async fn a_case_blocked_before_it_starts_is_not_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    v2_lib::autorun::divergences::update(root, "Acme", "Web", |list| {
        v2_lib::autorun::divergences::report_in(
            list,
            1,
            1,
            "x",
            "nothing",
            v2_lib::autorun::divergences::DivergenceKind::Data,
            1_786_000_000_000,
        )
    })
    .unwrap();
    let case = script(1);
    store::save_script(root, &case).unwrap();
    let mut browsers = Fake { drivers: vec![driver(Recorder::working(), false)], closed: vec![], wanted: true };
    let mut run = LocalRun { video_unavailable: false, id: "run-v".into(), pbi_id: 42, started_at: "1700000000000".into(), cases: vec![], mode: "unattended".into(), published: None, environment: None, resets: vec![], network_pauses: vec![] };
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "c".to_string())], &common::quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(run.cases[0].proposed, "Blocked", "{}", run.cases[0].reason);
    assert!(run.cases[0].video.is_none());
    for d in &browsers.closed {
        assert!(pos(d, "Page.startScreenRecording").is_none(), "{:?}", d.methods());
    }
}

/// A reused browser skips the sign-in: the recording starts at the reuse
/// check (the first thing the case does), before anything else is asked of
/// the page.
#[tokio::test]
async fn a_reused_case_starts_its_recording_before_the_reuse_check() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let case = script_as(1, "admin");
    let mut d = driver(Recorder::working(), false);
    let mut lease = v2_lib::autorun::lease::Held::new(v2_lib::autorun::lease::Holder::Case { run: String::new() }, std::time::Duration::from_millis(300));
    let cancel = AtomicBool::new(false);
    let record = v2_lib::autorun::replay::run_case_reusing(
        &mut d, root, "Acme", "Web", &mut lease, &case, Some("admin"), None, &common::quick(), &cancel, true, Some(12), &mut |_| {},
    )
    .await;
    assert_eq!(record.steps[0].step_number, v2_lib::autorun::replay::SIGN_IN_STEP);
    assert_eq!(d.methods().first().map(String::as_str), Some("Page.startScreenRecording"), "{:?}", d.methods());
    assert!(pos(&d, "Page.stopScreenRecording").is_some());
}
