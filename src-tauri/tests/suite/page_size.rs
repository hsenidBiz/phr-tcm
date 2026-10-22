//! The browser page size: a per-machine setting that ships as 1280 x 720.
//!
//! The size in force is process-wide (`browser::viewport::current`, read from
//! the app's settings), and this suite runs every module's tests on parallel
//! threads of one process. A test that CHANGES it would change what the golden
//! exports, the guide tests and the launch tests read meanwhile, so those run
//! in a process of their own (`in_own_process`, the same trick as
//! `updater.rs`'s). The tests that only read the default, or that are refused
//! before anything is saved, run in this one.

use serde_json::json;
use std::collections::BTreeMap;
use v2_lib::app_settings::{self, load, save, AppSettings};
use v2_lib::autorun::guide::autorun_guide;
use v2_lib::autorun::CaseScript;
use v2_lib::browser::launch::{background_args, launch_args};
use v2_lib::browser::viewport::{
    current, device_metrics_params, validate, window_size_arg, PageSize, DEFAULT_PAGE, DEVICE_SCALE, MAX_PAGE, MIN_PAGE,
};
use v2_lib::commands::app_settings::{get_page_size, reset_page_size, set_page_size};
use v2_lib::pw_export::translate::{raw_spec, RawSpecInput};

const REFUSAL: &str = "The page size must be between 800 x 600 and 3840 x 2160.";
const FILE: &str = "app-settings.json";

fn size(width: u32, height: u32) -> PageSize {
    PageSize { width, height }
}

/// Run `body` in a fresh copy of this test binary, so what it changes in
/// the process (the settings, the size in force) is seen by no other test.
fn in_own_process(name: &str, body: impl FnOnce()) {
    const IN_CHILD: &str = "TCM_TEST_PAGE_SIZE_CHILD";
    let path = format!("page_size::{name}");
    if std::env::var_os(IN_CHILD).is_none() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([path.as_str(), "--exact", "--test-threads=1", "--nocapture"])
            .env(IN_CHILD, "1")
            .output()
            .expect("start a copy of the test binary");
        let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "the check failed in its own process:\n{said}");
        // A renamed test would filter to nothing and "pass" having run no check.
        assert!(said.contains("1 passed"), "the child ran no check - is {path} still this test's path?\n{said}");
        return;
    }
    body();
}

fn spec() -> String {
    let script: CaseScript = serde_json::from_value(json!({
        "case_id": 1,
        "title": "A case",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/hr" }] }]
    }))
    .unwrap();
    let (by_name, texts) = (BTreeMap::new(), BTreeMap::new());
    raw_spec(&RawSpecInput {
        script: &script,
        md_path: "suites/sl/admin/m/f/test-cases/f.md".into(),
        feature_title: "F".into(),
        after_sign_in: &[],
        area_clicks: &[],
        area_clicks_by_name: &by_name,
        step_texts: &texts,
        origins: &["https://app.example".to_string()],
        fixme: None,
    })
    .unwrap()
}

const AUTORUN_SENTENCE: &str = "Every browser the app drives shows a 1280 x 720 page at scale 1, the same as\nPlaywright's default, so a layout you see while writing a script is the layout a\nrun sees.";
const API_SENTENCE: &str = "Every browser the app drives shows a 1280 x 720 page at scale 1, the same as\nPlaywright's default, so a layout you see while writing a script is the layout\na run sees.";

fn on_disk(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join(FILE)).unwrap_or_default()
}

// ---- The default ------------------------------------------------------

#[test]
fn it_ships_as_1280_by_720_and_a_fresh_install_has_it() {
    assert_eq!(DEFAULT_PAGE, size(1280, 720));
    assert_eq!((MIN_PAGE, MAX_PAGE), (size(800, 600), size(3840, 2160)));
    assert_eq!(AppSettings::default().page_size, DEFAULT_PAGE);
    // Nothing in this process has saved or loaded a size.
    assert_eq!(current(), DEFAULT_PAGE);
    assert_eq!(get_page_size(), DEFAULT_PAGE);
    assert_eq!(
        device_metrics_params(),
        json!({ "width": 1280, "height": 720, "deviceScaleFactor": DEVICE_SCALE, "mobile": false })
    );
    assert_eq!(window_size_arg(), "--window-size=1296,860");
}

#[test]
fn an_old_settings_file_with_no_size_loads_unchanged() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(FILE), r#"{"beta_updates":true,"close_to_tray":false}"#).unwrap();
    let s = load(d.path());
    assert_eq!(s.page_size, DEFAULT_PAGE);
    assert!(s.beta_updates && !s.close_to_tray);
}

#[test]
fn an_unreadable_size_reads_as_the_default_and_costs_nothing_else() {
    let d = tempfile::tempdir().unwrap();
    for bad in [
        r#""wide""#,
        "null",
        "[1280,720]",
        r#"{"width":"1280","height":720}"#,
        r#"{"width":1280}"#,
        r#"{"width":-1,"height":720}"#,
        // Readable, but not a size the setting accepts.
        r#"{"width":10,"height":10}"#,
        r#"{"width":4000,"height":720}"#,
    ] {
        std::fs::write(d.path().join(FILE), format!(r#"{{"beta_updates":true,"page_size":{bad}}}"#)).unwrap();
        let s = load(d.path());
        assert_eq!(s.page_size, DEFAULT_PAGE, "page_size {bad}");
        assert!(s.beta_updates, "page_size {bad} cost the other settings");
    }
}

#[test]
fn a_saved_size_reads_back() {
    let d = tempfile::tempdir().unwrap();
    let s = AppSettings { page_size: size(1920, 1080), ..AppSettings::default() };
    save(d.path(), &s).unwrap();
    assert_eq!(load(d.path()).page_size, size(1920, 1080));
    assert!(on_disk(d.path()).contains(r#""page_size":{"width":1920,"height":1080}"#), "{}", on_disk(d.path()));
}

// ---- The range --------------------------------------------------------

#[test]
fn the_edges_of_the_range_are_allowed() {
    for ok in [size(800, 600), size(3840, 2160), size(1280, 720), size(800, 2160), size(3840, 600)] {
        assert_eq!(validate(ok), Ok(ok));
    }
}

#[test]
fn a_size_outside_the_range_is_refused_with_the_exact_sentence_and_nothing_is_saved() {
    for bad in [size(799, 720), size(1280, 599), size(3841, 720), size(1280, 2161), size(0, 0), size(u32::MAX, u32::MAX)] {
        assert_eq!(validate(bad), Err(REFUSAL.to_string()), "{bad:?}");
        // The command refuses before it touches the settings.
        assert_eq!(set_page_size(bad), Err(REFUSAL.to_string()), "{bad:?}");
    }
    assert_eq!(app_settings::current().page_size, DEFAULT_PAGE);
    assert_eq!(current(), DEFAULT_PAGE);
}

// ---- What it changes --------------------------------------------------

#[test]
fn the_guides_and_the_export_say_nothing_new_at_the_default() {
    let guide = autorun_guide();
    assert!(guide.contains(AUTORUN_SENTENCE), "the Auto Run guide's sentence changed at the default");
    let api = v2_lib::api_templates::guide::text(&[], None);
    assert!(api.contains(API_SENTENCE), "the API template guide's sentence changed at the default");
    let out = spec();
    assert!(!out.contains("test.use("), "{out}");
    assert!(!out.contains("viewport"), "{out}");
}

#[test]
fn a_saved_size_is_loaded_at_start_up_and_every_user_of_it_follows() {
    in_own_process("a_saved_size_is_loaded_at_start_up_and_every_user_of_it_follows", || {
        let d = tempfile::tempdir().unwrap();
        let s = AppSettings { page_size: size(1600, 900), ..AppSettings::default() };
        save(d.path(), &s).unwrap();
        app_settings::init(d.path().to_path_buf());

        assert_eq!(current(), size(1600, 900));
        assert_eq!(get_page_size(), size(1600, 900));
        assert_eq!(
            device_metrics_params(),
            json!({ "width": 1600, "height": 900, "deviceScaleFactor": 1.0, "mobile": false })
        );
        // The page plus the browser's frame (16 x 140).
        assert_eq!(window_size_arg(), "--window-size=1616,1040");
        assert!(launch_args(9333, std::path::Path::new("C:/tmp/p")).contains(&"--window-size=1616,1040".to_string()));
        assert_eq!(background_args(), ["--headless=new".to_string(), "--window-size=1616,1040".to_string()]);

        // The guides name the size in force, and no longer call it Playwright's.
        let guide = autorun_guide();
        assert!(!guide.contains("1280 x 720 page at scale 1, the same as"), "still the default wording");
        let now = "1600 x 900 page at scale 1, a size set in Settings rather than Playwright's default of 1280 x 720";
        assert!(guide.contains(now), "the Auto Run guide does not name the size in force");
        let api = v2_lib::api_templates::guide::text(&[], None);
        assert!(api.contains(now), "the API template guide does not name the size in force");
        assert!(!api.contains("1280 x 720 page at scale 1, the same as"));

        // The export asks Playwright for the same page, once, near the top.
        let out = spec();
        let line = "test.use({ viewport: { width: 1600, height: 900 }, deviceScaleFactor: 1 });";
        assert_eq!(out.matches("test.use(").count(), 1, "{out}");
        assert!(out.contains(&format!("\n{line}\n")), "{out}");
        assert!(out.find(line).unwrap() > out.find("import { test, expect }").unwrap(), "{out}");
        assert!(out.find(line).unwrap() < out.find("test.describe(").unwrap(), "{out}");
    });
}

#[test]
fn saving_resetting_and_refusing_through_the_commands() {
    in_own_process("saving_resetting_and_refusing_through_the_commands", || {
        let d = tempfile::tempdir().unwrap();
        app_settings::init(d.path().to_path_buf());
        assert_eq!(current(), DEFAULT_PAGE);

        // Saved, in force at once, and on disk for the next launch.
        assert_eq!(set_page_size(size(1024, 768)), Ok(size(1024, 768)));
        assert_eq!(get_page_size(), size(1024, 768));
        assert_eq!(current(), size(1024, 768));
        assert_eq!(load(d.path()).page_size, size(1024, 768));
        assert_eq!(window_size_arg(), "--window-size=1040,908");
        assert_eq!(device_metrics_params()["width"], 1024);

        // A refusal keeps what was there, in memory and on disk.
        let before = on_disk(d.path());
        assert_eq!(set_page_size(size(799, 768)), Err(REFUSAL.to_string()));
        assert_eq!(current(), size(1024, 768));
        assert_eq!(on_disk(d.path()), before);

        // The other settings are left as they were.
        assert!(load(d.path()).close_to_tray);

        // Reset puts the shipped size back, in memory and on disk.
        assert_eq!(reset_page_size(), DEFAULT_PAGE);
        assert_eq!(current(), DEFAULT_PAGE);
        assert_eq!(load(d.path()).page_size, DEFAULT_PAGE);
        assert_eq!(
            device_metrics_params(),
            json!({ "width": 1280, "height": 720, "deviceScaleFactor": 1.0, "mobile": false })
        );
        assert!(autorun_guide().contains(AUTORUN_SENTENCE));
        assert!(!spec().contains("test.use("));
    });
}
