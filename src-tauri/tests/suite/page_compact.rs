//! A page answer that does not say the side menu again (`autorun::page_compact`),
//! the page argument that gives it whole, and `get_autorun_page`'s query.

use v2_lib::ai_bridge::{page_full, page_wanted, query_lines, with_compact_page};
use v2_lib::autorun::page_compact::{compact, forget, UNCHANGED};
use v2_lib::autorun::slots::SlotNo;

/// A page with a side menu of two links and a form under a heading. The memory is
/// process-wide, so a test holds `serial::page_menu` and forgets the slots it uses.
fn page(menu_item: &str) -> String {
    format!(
        "RootWebArea \"Leave\"\n navigation \"Side menu\" -> {{}}\n  link \"Home\" -> {{}}\n  link \"{menu_item}\" -> {{}}\n main \"\" -> {{}}\n  heading \"Apply\" -> {{}}\n  button \"Save\" -> {{}}"
    )
}

#[test]
fn the_same_side_menu_is_one_line_the_second_time() {
    let _held = crate::serial::page_menu();
    let slot = SlotNo(1);
    forget(slot);
    let first = compact(slot, &page("Leave"), false);
    assert!(first.contains("link \"Leave\""), "the first answer has the menu: {first}");
    let second = compact(slot, &page("Leave"), false);
    assert!(second.contains(&format!(" {UNCHANGED}")), "{second}");
    assert!(!second.contains("link \"Home\"") && !second.contains("Side menu"), "{second}");
    assert!(second.contains("button \"Save\"") && second.contains("heading \"Apply\""), "the rest of the page stays: {second}");
}

#[test]
fn a_changed_menu_is_answered_whole_and_then_remembered() {
    let _held = crate::serial::page_menu();
    let slot = SlotNo(1);
    forget(slot);
    compact(slot, &page("Leave"), false);
    let changed = compact(slot, &page("Claims"), false);
    assert!(changed.contains("link \"Claims\""), "{changed}");
    let again = compact(slot, &page("Claims"), false);
    assert!(again.contains(UNCHANGED), "{again}");
}

#[test]
fn page_full_gives_the_whole_tree_and_forget_starts_over() {
    let _held = crate::serial::page_menu();
    let slot = SlotNo(1);
    forget(slot);
    compact(slot, &page("Leave"), false);
    let full = compact(slot, &page("Leave"), true);
    assert!(full.contains("link \"Home\"") && !full.contains(UNCHANGED), "{full}");
    forget(slot);
    assert!(compact(slot, &page("Leave"), false).contains("link \"Home\""));
}

#[test]
fn each_slot_has_its_own_memory_and_text_without_a_menu_changes_nothing() {
    let _held = crate::serial::page_menu();
    forget(SlotNo(1));
    forget(SlotNo(2));
    compact(SlotNo(1), &page("Leave"), false);
    assert!(compact(SlotNo(2), &page("Leave"), false).contains("link \"Home\""), "another slot was told nothing yet");
    let plain = "RootWebArea \"x\"\n button \"Save\" -> {}";
    assert_eq!(compact(SlotNo(1), plain, false), plain);
    // Nothing in the page said the menu, so slot 1 still remembers it.
    assert!(compact(SlotNo(1), &page("Leave"), false).contains(UNCHANGED));
}

#[test]
fn a_json_answer_has_its_page_field_compacted_and_a_failure_is_left_alone() {
    let _held = crate::serial::page_menu();
    let slot = SlotNo(1);
    forget(slot);
    let answer = |p: &str| (200u16, serde_json::json!({ "sentence": "ok", "page": p }).to_string());
    let (_, first) = with_compact_page(answer(&page("Leave")), "{}", slot);
    assert!(serde_json::from_str::<serde_json::Value>(&first).unwrap()["page"].as_str().unwrap().contains("link \"Home\""));
    let (_, second) = with_compact_page(answer(&page("Leave")), "{}", slot);
    let v: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert!(v["page"].as_str().unwrap().contains(UNCHANGED), "{second}");
    assert_eq!(v["sentence"], "ok");
    let (_, whole) = with_compact_page(answer(&page("Leave")), r#"{"page":"full"}"#, slot);
    assert!(whole.contains("link \\\"Home\\\""), "{whole}");
    let refused = (409u16, page("Leave"));
    assert_eq!(with_compact_page(refused.clone(), "{}", slot), refused);
    // The batch's text answer: lines, then the page.
    let (_, text) = with_compact_page((200, format!("1. ok: click\n\n{}", page("Leave"))), "{}", slot);
    assert!(text.contains(UNCHANGED) && text.starts_with("1. ok: click"), "{text}");
}

#[test]
fn page_arguments_are_read_from_the_body() {
    let _held = crate::serial::page_menu();
    assert!(page_full(r#"{"page":"full"}"#) && page_full(r#"{"page":" Full "}"#));
    assert!(!page_full(r#"{"page":true}"#) && !page_full("{}") && !page_full("nonsense"));
    assert!(!page_wanted(r#"{"page":false}"#) && !page_wanted(r#"{"page":"false"}"#));
    assert!(page_wanted(r#"{"page":"full"}"#) && page_wanted(r#"{"page":true}"#) && page_wanted("{}"));
}

#[test]
fn a_query_keeps_the_matching_lines_with_the_lines_they_sit_under() {
    let _held = crate::serial::page_menu();
    let text = page("Leave");
    let out = query_lines(&text, "SAVE", 120);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "RootWebArea \"Leave\"", "{out}");
    assert_eq!(lines[1], " main \"\" -> {}");
    assert_eq!(lines[2], "  button \"Save\" -> {}");
    assert!(lines[3].starts_with("1 lines contain \"SAVE\""), "{out}");
    assert!(out.contains("Recorded nothing as seen"), "{out}");
    assert!(!out.contains("Side menu"), "{out}");

    let none = query_lines(&text, "zebra", 120);
    assert!(none.starts_with("no line of the page contains \"zebra\""), "{none}");

    let cut = query_lines(&text, "link", 1);
    assert!(cut.contains("more lines"), "{cut}");
}
