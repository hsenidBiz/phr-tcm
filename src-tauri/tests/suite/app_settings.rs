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
        AppSettings {
            close_to_tray: true,
            close_notice_shown: false,
            beta_updates: false,
            start_minimized: true,
            // Letting AI tools change the database unasked is a choice, never a default.
            db_auto_approve: false,
            stay_signed_in: true,
            playwright_clone: String::new(),
            autorun_highlight: true,
        }
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
        AppSettings {
            close_to_tray: true,
            close_notice_shown: false,
            beta_updates: true,
            start_minimized: true,
            db_auto_approve: false,
            stay_signed_in: true,
            playwright_clone: String::new(),
            autorun_highlight: true,
        }
    );
}

#[test]
fn saved_settings_read_back() {
    let d = dir();
    let s = AppSettings {
        close_to_tray: false,
        close_notice_shown: true,
        beta_updates: true,
        start_minimized: false,
        db_auto_approve: true,
        stay_signed_in: false,
        playwright_clone: "C:/clone".into(),
        autorun_highlight: false,
    };
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

/// Start with Windows registers the app with the argument that makes it
/// start in the tray - the same one `start_hidden` looks for.
#[test]
fn start_with_windows_asks_for_a_hidden_start() {
    use v2_lib::tray::{start_hidden, AUTOSTART_ARGS};
    let launch = std::iter::once("v2.exe".to_string()).chain(AUTOSTART_ARGS.iter().map(|a| a.to_string()));
    assert!(start_hidden(launch));
}

/// A settings file from before Start minimized existed keeps today's
/// behaviour: the sign-in start stays in the tray.
#[test]
fn an_older_file_starts_minimized() {
    let d = dir();
    std::fs::write(d.path().join("app-settings.json"), r#"{"close_to_tray":true,"close_notice_shown":true,"beta_updates":false}"#).unwrap();
    assert!(load(d.path()).start_minimized);
}

/// Stay signed in arrives on for everyone: a settings file from before it
/// existed turns it on, the same as a fresh install.
#[test]
fn an_older_file_stays_signed_in() {
    let d = dir();
    std::fs::write(d.path().join("app-settings.json"), r#"{"close_to_tray":true,"beta_updates":false,"start_minimized":true}"#).unwrap();
    assert!(load(d.path()).stay_signed_in);
}

#[test]
fn highlight_defaults_on() {
    assert!(AppSettings::default().autorun_highlight);
    // A file written before the setting existed reads as on.
    let d = dir();
    std::fs::write(d.path().join("app-settings.json"), r#"{"beta_updates":true}"#).unwrap();
    assert!(load(d.path()).autorun_highlight);
}
