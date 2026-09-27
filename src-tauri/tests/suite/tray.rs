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
