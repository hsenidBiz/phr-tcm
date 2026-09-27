//! Closing the main window hides it to the tray - only the main window,
//! only when the switch is on, and only when the tray actually exists
//! (otherwise nobody could get the window back).

use v2_lib::tray::{close_action, start_hidden, CloseAction};

#[test]
fn the_close_decision() {
    use CloseAction::*;
    // (window, close_to_tray, tray_ok) -> action
    let table = [
        ("main", true, true, Hide),
        ("main", false, true, Close),
        ("main", true, false, Close),
        ("main", false, false, Close),
        ("runner", true, true, Close),
        ("runner", false, true, Close),
        ("anything-else", true, true, Close),
    ];
    for (label, on, tray_ok, want) in table {
        assert_eq!(close_action(label, on, tray_ok), want, "{label} on={on} tray_ok={tray_ok}");
    }
}

#[test]
fn a_hidden_start_is_asked_for_with_the_hidden_argument_only() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(start_hidden(args(&["v2.exe", "--hidden"])));
    assert!(!start_hidden(args(&["v2.exe"])));
    assert!(!start_hidden(args(&["v2.exe", "--hidden-thing"])));
    assert!(!start_hidden(args(&["v2.exe", "hidden"])));
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

/// Uninstalling removes the Start with Windows entry, so sign-in does not
/// keep launching a deleted exe. The value the uninstall hook deletes must
/// be the one the autostart plugin writes: both use `AUTOSTART_APP_NAME`,
/// which is also the product name - the plugin's own default, so an entry
/// written before the name was set explicitly is removed too. Nothing here
/// touches the real registry.
#[test]
fn the_uninstaller_removes_the_value_start_with_windows_writes() {
    use v2_lib::tray::{AUTOSTART_APPROVED_KEY, AUTOSTART_APP_NAME, AUTOSTART_RUN_KEY};

    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    assert_eq!(conf["productName"].as_str(), Some(AUTOSTART_APP_NAME));

    // The plugin is told the same name the uninstall hook deletes.
    let lib = read("src/lib.rs");
    assert!(lib.contains(".app_name(tray::AUTOSTART_APP_NAME)"), "the plugin writes AUTOSTART_APP_NAME");
    let tray = read("src/tray.rs");
    assert!(tray.contains("delete_value(AUTOSTART_APP_NAME)"), "the hook deletes AUTOSTART_APP_NAME");

    // The two keys auto-launch 0.5.0 writes under HKEY_CURRENT_USER
    // (src/windows.rs: AL_REGKEY and TASK_MANAGER_OVERRIDE_REGKEY).
    assert_eq!(AUTOSTART_RUN_KEY, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run");
    assert_eq!(AUTOSTART_APPROVED_KEY, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");

    // And Velopack's uninstall hook is what calls it.
    let main = read("src/main.rs");
    assert!(main.contains(".on_before_uninstall_fast_callback(|_version| v2_lib::tray::remove_autostart_entry())"));
}

/// Only a start at sign-in that asked for it stays hidden, and only while
/// Start minimized is on; every other launch opens the window.
#[test]
fn a_launch_stays_hidden_only_when_asked_and_start_minimized_is_on() {
    use v2_lib::tray::launch_hidden;
    assert!(launch_hidden(true, true));
    assert!(!launch_hidden(true, false));
    assert!(!launch_hidden(false, true));
    assert!(!launch_hidden(false, false));
}
