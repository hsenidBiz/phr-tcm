//! The app's own settings file (app-settings.json): read before any page
//! loads, so the close handler and the updater can use it. Anything it
//! cannot read is the defaults - never an error.

use v2_lib::app_settings::{load, save, AppSettings};

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn a_missing_file_is_the_defaults() {
    let d = dir();
    assert_eq!(
        load(d.path()),
        AppSettings { close_to_tray: true, close_notice_shown: false, beta_updates: false }
    );
}

#[test]
fn an_empty_or_corrupt_file_is_the_defaults() {
    let d = dir();
    for body in ["", "{", "not json", "[]", "null"] {
        std::fs::write(d.path().join("app-settings.json"), body).unwrap();
        assert_eq!(load(d.path()), AppSettings::default(), "body {body:?}");
    }
}

#[test]
fn missing_fields_take_their_defaults_and_unknown_fields_are_ignored() {
    let d = dir();
    std::fs::write(d.path().join("app-settings.json"), r#"{"beta_updates":true,"from_the_future":1}"#).unwrap();
    assert_eq!(
        load(d.path()),
        AppSettings { close_to_tray: true, close_notice_shown: false, beta_updates: true }
    );
}

#[test]
fn saved_settings_read_back() {
    let d = dir();
    let s = AppSettings { close_to_tray: false, close_notice_shown: true, beta_updates: true };
    save(d.path(), &s).unwrap();
    assert_eq!(load(d.path()), s);
}

#[test]
fn save_creates_the_folder() {
    let d = dir();
    let nested = d.path().join("a").join("b");
    save(&nested, &AppSettings::default()).unwrap();
    assert_eq!(load(&nested), AppSettings::default());
}
