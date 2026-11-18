//! What changed on a page (`autorun::page_changes`): the line diff under its
//! ancestors, and the per-reader memory of the last page given.

use super::serial;
use v2_lib::autorun::page_changes::{diff, remember_whole, remembered, render, same_address, CHANGES_HEADER};
use v2_lib::autorun::page_compact;
use v2_lib::autorun::slots::SlotNo;

fn v(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_new_button_in_a_dialog_shows_under_the_dialog() {
    let old = v(&["RootWebArea \"x\"", " main \"\"", "  dialog \"Confirm delete\"", "   text \"Loading...\""]);
    let new = v(&[
        "RootWebArea \"x\"",
        " main \"\"",
        "  dialog \"Confirm delete\"",
        "   text \"Delete Ann Perera?\"",
        "   button \"Yes\" -> {\"role\":\"button\",\"name\":\"Yes\"}",
    ]);
    let got = render(&diff(&old, &new));
    let want = [
        CHANGES_HEADER,
        " RootWebArea \"x\"",
        "  main \"\"",
        "   dialog \"Confirm delete\"",
        "-   text \"Loading...\"",
        "+   text \"Delete Ann Perera?\"",
        "+   button \"Yes\" -> {\"role\":\"button\",\"name\":\"Yes\"}",
    ]
    .join("\n");
    assert_eq!(got, want);
    let plus: Vec<_> = diff(&old, &new).iter().filter_map(|c| c.new_index).collect();
    assert_eq!(plus, vec![3, 4]);
}

#[test]
fn nothing_changed_is_an_empty_diff() {
    let a = v(&["RootWebArea \"x\"", " main \"\"", "  button \"Save\""]);
    assert!(diff(&a, &a).is_empty());
}

#[test]
fn one_of_two_equal_rows_removed_marks_one_line() {
    let old = v(&[
        "RootWebArea \"x\"",
        " table \"\"",
        "  row \"Ann\"",
        "   button \"Delete\"",
        "  row \"Bo\"",
        "   button \"Delete\"",
    ]);
    let new = v(&["RootWebArea \"x\"", " table \"\"", "  row \"Ann\"", "   button \"Delete\""]);
    let changes = diff(&old, &new);
    let gone: Vec<&str> = changes.iter().filter(|c| c.marker == '-').map(|c| c.line.as_str()).collect();
    assert_eq!(gone, vec!["  row \"Bo\"", "   button \"Delete\""]);
    assert!(changes.iter().all(|c| c.marker != '+'));
}

#[test]
fn an_ancestor_is_printed_once_for_two_changes_under_it() {
    let old = v(&["RootWebArea \"x\"", " main \"\"", "  button \"A\"", "  button \"B\""]);
    let new = v(&["RootWebArea \"x\"", " main \"\"", "  button \"A2\"", "  button \"B2\""]);
    let text = render(&diff(&old, &new));
    assert_eq!(text.matches(" RootWebArea \"x\"").count(), 1);
    assert_eq!(text.matches("  main \"\"").count(), 1);
    assert_eq!(text.lines().filter(|l| l.starts_with('+') || l.starts_with('-')).count(), 4);
}

#[test]
fn memory_is_per_slot_and_reader_and_forget_clears_it() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(1));
    remember_whole(SlotNo(1), "a:", "https://h/p#frag", v(&["x", " y"]));
    assert_eq!(remembered(SlotNo(1), "a:"), Some(("https://h/p".to_string(), v(&["x", " y"]))));
    assert!(remembered(SlotNo(1), "b:").is_none());
    assert!(remembered(SlotNo(2), "a:").is_none());
    page_compact::forget(SlotNo(1));
    assert!(remembered(SlotNo(1), "a:").is_none());
}

#[test]
fn the_fragment_does_not_change_the_address() {
    assert!(same_address("https://h/p#a", "https://h/p#b"));
    assert!(!same_address("https://h/p", "https://h/q"));
}
