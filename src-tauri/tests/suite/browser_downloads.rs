//! Downloads an Auto Run browser keeps: the names they land under, and the
//! driver's following of them. The names are pure; the following is fed
//! canned DevTools frames, so none of this starts a browser
//! (`browser_live.rs` does that).

use std::collections::VecDeque;
use std::path::Path;
use v2_lib::browser::cdp::{Cdp, Driver, Transport};
use v2_lib::browser::downloads::{sanitise_name, unique_name, DownloadState, MAX_NAME_CHARS};

// ---------------------------------------------------------------- names

#[test]
fn a_plain_name_is_kept_as_it_is() {
    assert_eq!(sanitise_name("Template.xlsx"), "Template.xlsx");
    assert_eq!(sanitise_name("error log 2026-10-05.csv"), "error log 2026-10-05.csv");
}

/// Review Focus 2: whatever the page suggests, the file lands inside the
/// run's own folder. Path parts go first, so `..` can never climb out.
#[test]
fn path_parts_are_stripped() {
    assert_eq!(sanitise_name("..\\..\\x.xlsx"), "x.xlsx");
    assert_eq!(sanitise_name("../../x.xlsx"), "x.xlsx");
    assert_eq!(sanitise_name("C:\\x"), "x");
    assert_eq!(sanitise_name("/etc/passwd"), "passwd");
    assert_eq!(sanitise_name("\\\\server\\share\\report.csv"), "report.csv");
}

#[test]
fn characters_windows_refuses_become_underscores() {
    assert_eq!(sanitise_name("a:b?.csv"), "a_b_.csv");
    assert_eq!(sanitise_name("x*y\"z<w>v|u.txt"), "x_y_z_w_v_u.txt");
    assert_eq!(sanitise_name("tab\there.txt"), "tab_here.txt");
}

#[test]
fn windows_reserved_names_get_an_underscore_in_front() {
    assert_eq!(sanitise_name("con.txt"), "_con.txt");
    assert_eq!(sanitise_name("CON"), "_CON");
    assert_eq!(sanitise_name("nul.tar.gz"), "_nul.tar.gz");
    assert_eq!(sanitise_name("Com1.csv"), "_Com1.csv");
    assert_eq!(sanitise_name("lpt9.log"), "_lpt9.log");
    // Only the name itself is reserved, not a name that starts with one.
    assert_eq!(sanitise_name("console.txt"), "console.txt");
    assert_eq!(sanitise_name("com10.txt"), "com10.txt");
}

#[test]
fn trailing_dots_and_spaces_go_since_windows_drops_them_anyway() {
    assert_eq!(sanitise_name("report.csv. . "), "report.csv");
}

#[test]
fn an_empty_result_becomes_download() {
    assert_eq!(sanitise_name(""), "download");
    assert_eq!(sanitise_name("   "), "download");
    assert_eq!(sanitise_name(".."), "download");
    assert_eq!(sanitise_name("a/b/"), "download");
}

#[test]
fn a_long_name_is_cut_to_the_cap_keeping_its_extension() {
    let long = format!("{}.xlsx", "a".repeat(400));
    let got = sanitise_name(&long);
    assert_eq!(got.chars().count(), MAX_NAME_CHARS);
    assert!(got.ends_with(".xlsx"), "{got}");
    assert_eq!(MAX_NAME_CHARS, 150);
    // Counted in characters, never cut through one.
    let wide = format!("{}.csv", "é".repeat(400));
    let got = sanitise_name(&wide);
    assert_eq!(got.chars().count(), MAX_NAME_CHARS);
    assert!(got.ends_with(".csv"), "{got}");
}

/// A fresh temp folder per test.
fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn a_free_name_is_used_as_it_is() {
    let dir = temp();
    assert_eq!(unique_name(dir.path(), "a.xlsx"), "a.xlsx");
}

/// Review Focus 1: a second file of the same name never overwrites the
/// first. The number goes before the extension.
#[test]
fn a_taken_name_gets_a_number_before_its_extension() {
    let dir = temp();
    std::fs::write(dir.path().join("a.xlsx"), "1").unwrap();
    assert_eq!(unique_name(dir.path(), "a.xlsx"), "a (2).xlsx");
    std::fs::write(dir.path().join("a (2).xlsx"), "2").unwrap();
    assert_eq!(unique_name(dir.path(), "a.xlsx"), "a (3).xlsx");
}

#[test]
fn a_taken_name_with_no_extension_gets_its_number_at_the_end() {
    let dir = temp();
    std::fs::write(dir.path().join("README"), "1").unwrap();
    assert_eq!(unique_name(dir.path(), "README"), "README (2)");
    std::fs::write(dir.path().join(".env"), "1").unwrap();
    assert_eq!(unique_name(dir.path(), ".env"), ".env (2)");
}

// ------------------------------------------------------- the driver

/// Hands back canned frames in order and keeps what was sent; reports the
/// socket closed once it runs dry.
struct Frames {
    incoming: VecDeque<String>,
    sent: Vec<serde_json::Value>,
}

impl Frames {
    fn new(frames: Vec<String>) -> Self {
        Frames { incoming: frames.into(), sent: vec![] }
    }
}

impl Transport for Frames {
    async fn send(&mut self, text: String) -> Result<(), String> {
        self.sent.push(serde_json::from_str(&text).unwrap());
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, String>> {
        self.incoming.pop_front().map(Ok)
    }
}

fn reply(id: u64) -> String {
    serde_json::json!({ "id": id, "result": {} }).to_string()
}

fn refusal(id: u64) -> String {
    serde_json::json!({ "id": id, "error": { "code": -32000, "message": "Not allowed" } }).to_string()
}

fn begin(guid: &str, name: &str) -> String {
    serde_json::json!({
        "method": "Browser.downloadWillBegin",
        "params": { "frameId": "F", "guid": guid, "url": "blob:x", "suggestedFilename": name }
    })
    .to_string()
}

fn progress(guid: &str, state: &str, bytes: u64) -> String {
    serde_json::json!({
        "method": "Browser.downloadProgress",
        "params": { "guid": guid, "totalBytes": bytes, "receivedBytes": bytes, "state": state }
    })
    .to_string()
}

/// A connection with downloads on: the first frame answers
/// `Browser.setDownloadBehavior`, the rest are read by one later call
/// (id 2), whose reply comes last.
async fn following(dir: &Path, events: Vec<String>) -> Cdp<Frames> {
    let mut frames = vec![reply(1)];
    frames.extend(events);
    frames.push(reply(2));
    let mut cdp = Cdp::over(Frames::new(frames));
    cdp.enable_downloads(dir).await.unwrap();
    cdp.call("Runtime.evaluate", serde_json::json!({ "expression": "1" })).await.unwrap();
    cdp
}

#[tokio::test]
async fn enabling_asks_the_browser_to_save_every_download_under_its_guid_in_the_folder() {
    let dir = temp();
    let folder = dir.path().join("downloads").join("run-1");
    let mut cdp = Cdp::over(Frames::new(vec![reply(1)]));
    cdp.enable_downloads(&folder).await.unwrap();
    let sent = &cdp.transport().sent;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["method"], "Browser.setDownloadBehavior");
    assert_eq!(sent[0]["params"]["behavior"], "allowAndName");
    assert_eq!(sent[0]["params"]["eventsEnabled"], true);
    assert_eq!(sent[0]["params"]["downloadPath"], folder.to_string_lossy().as_ref());
    assert!(folder.is_dir(), "the folder is made before the browser is pointed at it");
}

#[tokio::test]
async fn a_page_target_that_refuses_the_browser_call_is_asked_through_the_page_domain() {
    let dir = temp();
    let mut cdp = Cdp::over(Frames::new(vec![refusal(1), reply(2)]));
    cdp.enable_downloads(dir.path()).await.unwrap();
    let sent = &cdp.transport().sent;
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1]["method"], "Page.setDownloadBehavior");
    assert_eq!(sent[1]["params"]["behavior"], "allow");
    assert_eq!(sent[1]["params"]["downloadPath"], dir.path().to_string_lossy().as_ref());
}

#[tokio::test]
async fn refused_both_ways_is_an_error() {
    let dir = temp();
    let mut cdp = Cdp::over(Frames::new(vec![refusal(1), refusal(2)]));
    assert!(cdp.enable_downloads(dir.path()).await.is_err());
}

/// Read inside whichever call is in flight, as a paused request is: a
/// download that starts and ends during a click is never missed.
#[tokio::test]
async fn a_completed_download_is_renamed_from_its_guid_to_its_suggested_name() {
    let dir = temp();
    std::fs::write(dir.path().join("g-1"), "Name,Age\n").unwrap();
    let cdp = following(dir.path(), vec![begin("g-1", "report.csv"), progress("g-1", "completed", 9)]).await;
    let all = cdp.downloads();
    assert_eq!(all.len(), 1);
    let d = &all[0];
    assert_eq!(d.guid, "g-1");
    assert_eq!(d.name, "report.csv");
    assert_eq!(d.state, DownloadState::Completed);
    assert_eq!(d.bytes, 9);
    assert_eq!(d.path, dir.path().join("report.csv"));
    assert_eq!(std::fs::read_to_string(&d.path).unwrap(), "Name,Age\n");
    assert!(!dir.path().join("g-1").exists(), "the guid file is gone");
}

/// Review Focus 1: two downloads of one name both survive.
#[tokio::test]
async fn two_downloads_of_one_name_both_survive() {
    let dir = temp();
    std::fs::write(dir.path().join("g-1"), "first").unwrap();
    std::fs::write(dir.path().join("g-2"), "second").unwrap();
    let cdp = following(
        dir.path(),
        vec![
            begin("g-1", "Template.xlsx"),
            begin("g-2", "Template.xlsx"),
            progress("g-1", "completed", 5),
            progress("g-2", "completed", 6),
        ],
    )
    .await;
    let all = cdp.downloads();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].path, dir.path().join("Template.xlsx"));
    assert_eq!(all[1].path, dir.path().join("Template (2).xlsx"));
    assert_eq!(all[0].name, "Template.xlsx");
    assert_eq!(all[1].name, "Template.xlsx");
    assert_eq!(std::fs::read_to_string(&all[0].path).unwrap(), "first");
    assert_eq!(std::fs::read_to_string(&all[1].path).unwrap(), "second");
}

/// Review Focus 2, through the driver.
#[tokio::test]
async fn a_hostile_suggested_name_lands_inside_the_folder() {
    let dir = temp();
    let folder = dir.path().join("inner");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("g-1"), "x").unwrap();
    std::fs::write(folder.join("g-2"), "y").unwrap();
    let cdp = following(
        &folder,
        vec![
            begin("g-1", "..\\..\\x.xlsx"),
            progress("g-1", "completed", 1),
            begin("g-2", "con.txt"),
            progress("g-2", "completed", 1),
        ],
    )
    .await;
    let all = cdp.downloads();
    assert_eq!(all[0].path, folder.join("x.xlsx"));
    assert_eq!(all[1].path, folder.join("_con.txt"));
    assert!(!dir.path().join("x.xlsx").exists());
}

#[tokio::test]
async fn progress_and_cancel_are_followed_in_start_order() {
    let dir = temp();
    let cdp = following(
        dir.path(),
        vec![
            begin("g-1", "big.csv"),
            begin("g-2", "gone.csv"),
            progress("g-1", "inProgress", 100),
            progress("g-2", "canceled", 0),
        ],
    )
    .await;
    let all = cdp.downloads();
    assert_eq!(all.iter().map(|d| d.guid.as_str()).collect::<Vec<_>>(), ["g-1", "g-2"]);
    assert_eq!(all[0].state, DownloadState::InProgress);
    assert_eq!(all[0].bytes, 100);
    assert_eq!(all[0].path, dir.path().join("g-1"), "still under its guid until it completes");
    assert_eq!(all[1].state, DownloadState::Canceled);
    assert!(all[0].started_at <= all[1].started_at);
}

/// The fallback's events come from the Page domain, in the same shape.
#[tokio::test]
async fn the_page_domain_events_are_followed_too() {
    let dir = temp();
    std::fs::write(dir.path().join("g-1"), "x").unwrap();
    let page_begin = begin("g-1", "a.csv").replace("Browser.downloadWillBegin", "Page.downloadWillBegin");
    let page_done = progress("g-1", "completed", 1).replace("Browser.downloadProgress", "Page.downloadProgress");
    let cdp = following(dir.path(), vec![page_begin, page_done]).await;
    let all = cdp.downloads();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].state, DownloadState::Completed);
    assert_eq!(all[0].path, dir.path().join("a.csv"));
}

/// A browser that sends both families for one download is one download.
#[tokio::test]
async fn one_guid_is_one_download_whichever_domain_says_so() {
    let dir = temp();
    std::fs::write(dir.path().join("g-1"), "x").unwrap();
    let page_begin = begin("g-1", "a.csv").replace("Browser.downloadWillBegin", "Page.downloadWillBegin");
    let page_done = progress("g-1", "completed", 1).replace("Browser.downloadProgress", "Page.downloadProgress");
    let cdp = following(
        dir.path(),
        vec![begin("g-1", "a.csv"), page_begin, progress("g-1", "completed", 1), page_done],
    )
    .await;
    let all = cdp.downloads();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].path, dir.path().join("a.csv"));
    assert!(!dir.path().join("a (2).csv").exists());
}

/// The frames can arrive end first. The end is kept for its download and
/// applied, rename included, the moment the begin arrives.
#[tokio::test]
async fn an_end_read_before_its_begin_is_applied_when_the_begin_arrives() {
    let dir = temp();
    std::fs::write(dir.path().join("g-1"), "Name,Age\n").unwrap();
    let cdp = following(
        dir.path(),
        vec![
            progress("g-1", "completed", 9),
            progress("g-2", "canceled", 0),
            begin("g-1", "report.csv"),
            begin("g-2", "gone.csv"),
        ],
    )
    .await;
    let all = cdp.downloads();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].state, DownloadState::Completed);
    assert_eq!(all[0].path, dir.path().join("report.csv"));
    assert_eq!(all[0].bytes, 9);
    assert_eq!(std::fs::read_to_string(&all[0].path).unwrap(), "Name,Age\n");
    assert!(!dir.path().join("g-1").exists(), "the guid file is gone");
    assert_eq!(all[1].state, DownloadState::Canceled);
}

/// A progress report read before its begin is not an end, and is dropped:
/// the download starts in progress.
#[tokio::test]
async fn a_progress_report_read_before_its_begin_leaves_it_in_progress() {
    let dir = temp();
    let cdp = following(dir.path(), vec![progress("g-1", "inProgress", 50), begin("g-1", "big.csv")]).await;
    let all = cdp.downloads();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].state, DownloadState::InProgress);
}

#[tokio::test]
async fn nothing_is_followed_before_downloads_are_enabled() {
    let mut cdp = Cdp::over(Frames::new(vec![begin("g-1", "a.csv"), reply(1)]));
    cdp.call("Runtime.evaluate", serde_json::json!({})).await.unwrap();
    assert!(cdp.downloads().is_empty());
}

/// The default for a test's fake: nothing sent, nothing kept.
struct Bare;

impl Driver for Bare {
    async fn call(&mut self, method: &str, _params: serde_json::Value) -> Result<serde_json::Value, v2_lib::browser::cdp::CdpError> {
        panic!("a fake with no downloads of its own was asked for {method}");
    }
    async fn wait_event(
        &mut self,
        method: &str,
        _limit: std::time::Duration,
    ) -> Result<v2_lib::browser::cdp::Event, v2_lib::browser::cdp::CdpError> {
        panic!("not expected: {method}");
    }
    fn forget_events(&mut self) {}
    fn take_dialogs(&mut self) -> Vec<String> {
        vec![]
    }
    fn set_deadline(&mut self, _deadline: Option<std::time::Instant>) {}
}

#[tokio::test]
async fn a_fake_driver_turns_downloads_on_without_a_call_and_keeps_none() {
    let dir = temp();
    let mut d = Bare;
    Driver::enable_downloads(&mut d, dir.path()).await.unwrap();
    assert!(Driver::downloads(&d).is_empty());
}
