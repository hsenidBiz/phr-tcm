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

/// Past `MAX_SIDE` (4000) lines on a side, once the common head and tail are
/// set aside, nothing is aligned: every line left on either side is marked.
fn rows(n: usize, first: &str, last: &str) -> Vec<String> {
    let mut lines = v(&["RootWebArea \"x\"", " main \"\""]);
    lines.push(format!("  button \"{first}\""));
    lines.extend((1..n - 1).map(|i| format!("  button \"Do {i}\"")));
    lines.push(format!("  button \"{last}\""));
    lines
}

#[test]
fn a_side_past_the_cutoff_marks_every_line_between_head_and_tail() {
    // 4000 lines a side: aligned, so only the two edited ends differ.
    let (old, new) = (rows(4000, "A", "Z"), rows(4000, "A2", "Z2"));
    let changes = diff(&old, &new);
    assert_eq!(changes.iter().filter(|c| c.marker == '-').count(), 2);
    assert_eq!(changes.iter().filter(|c| c.marker == '+').count(), 2);
    // 4001 lines a side: the same edit marks the whole middle.
    let (old, new) = (rows(4001, "A", "Z"), rows(4001, "A2", "Z2"));
    let changes = diff(&old, &new);
    assert_eq!(changes.iter().filter(|c| c.marker == '-').count(), 4001);
    assert_eq!(changes.iter().filter(|c| c.marker == '+').count(), 4001);
    // Either side alone is enough.
    let few = v(&["RootWebArea \"x\"", " main \"\"", "  button \"Only\""]);
    let changes = diff(&old, &few);
    assert_eq!(changes.iter().filter(|c| c.marker == '-').count(), 4001);
    assert_eq!(changes.iter().filter(|c| c.marker == '+').count(), 1);
    let changes = diff(&few, &new);
    assert_eq!(changes.iter().filter(|c| c.marker == '-').count(), 1);
    assert_eq!(changes.iter().filter(|c| c.marker == '+').count(), 4001);
}

#[tokio::test]
async fn a_page_past_the_cutoff_answers_the_usual_page() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(3));
    let a = names(4001);
    let mut b = a.clone();
    b[0] = "First".into();
    b[4000] = "Last".into();
    read(Mode::Changes, "https://h/p", &buttons(&a), 120).await;
    let second = read(Mode::Changes, "https://h/p", &buttons(&b), 120).await;
    // Two edited ends would be a short changes answer if they were aligned.
    assert_eq!(second.text, render_frames_read(&buttons(&b), &[], 120).text);
    assert!(!second.text.starts_with(CHANGES_HEADER), "{}", second.text);
    page_compact::forget(SlotNo(3));
}

#[test]
fn a_line_that_moves_to_another_container_is_under_its_new_parent() {
    let old = v(&["RootWebArea \"x\"", " list \"A\"", "  item \"x\"", " list \"B\"", "  item \"y\""]);
    let new = v(&["RootWebArea \"x\"", " list \"A\"", " list \"B\"", "  item \"y\"", "  item \"x\""]);
    let want = [
        CHANGES_HEADER,
        " RootWebArea \"x\"",
        "  list \"A\"",
        "-  item \"x\"",
        "  list \"B\"",
        "+  item \"x\"",
    ]
    .join("
");
    assert_eq!(render(&diff(&old, &new)), want);
}

/// Two containers that read the same: the line moved from the first to the
/// second must not look as if it stayed in the first.
#[test]
fn a_move_between_identical_containers_is_under_each_container() {
    let old = v(&["RootWebArea \"x\"", " group \"Row\"", "  button \"Move\"", " group \"Row\"", "  button \"Stay\""]);
    let new = v(&["RootWebArea \"x\"", " group \"Row\"", " group \"Row\"", "  button \"Stay\"", "  button \"Move\""]);
    let want = [
        CHANGES_HEADER,
        " RootWebArea \"x\"",
        "  group \"Row\"",
        "-  button \"Move\"",
        "  group \"Row\"",
        "+  button \"Move\"",
    ]
    .join("
");
    assert_eq!(render(&diff(&old, &new)), want);
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

// ------------------------------------------------- the scope around a read

use v2_lib::ai_bridge::page_whole;
use v2_lib::autorun::page_changes::{note_address, scoped, Mode, UNCHANGED_PAGE};
use v2_lib::browser::snapshot::{render_frames_read, AxNode};

fn ax(id: &str, role: &str, name: &str, children: &[&str]) -> AxNode {
    AxNode {
        id: id.into(),
        role: role.into(),
        name: name.into(),
        value: None,
        ignored: false,
        children: children.iter().map(|s| s.to_string()).collect(),
        focusable: false,
        disabled: false,
        backend: None,
        required: false,
        protected: false,
    }
}

/// A main holding one button per name.
fn buttons(names: &[String]) -> Vec<AxNode> {
    let ids: Vec<String> = (0..names.len()).map(|i| format!("b{i}")).collect();
    let kids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut nodes = vec![ax("0", "RootWebArea", "App", &["main"]), ax("main", "main", "", &kids)];
    for (i, n) in names.iter().enumerate() {
        nodes.push(ax(&format!("b{i}"), "button", n, &[]));
    }
    nodes
}

fn names(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Do {i}")).collect()
}

/// Read `nodes` at `address` inside a scope for slot 3's reader `a:`, as a 200.
async fn read(mode: Mode, address: &str, nodes: &[AxNode], limit: usize) -> v2_lib::browser::snapshot::PageRead {
    let mut got = None;
    scoped(SlotNo(3), "a:", mode, async {
        note_address(address);
        got = Some(render_frames_read(nodes, &[], limit));
        (200, String::new())
    })
    .await;
    got.unwrap()
}

#[tokio::test]
async fn a_second_read_with_one_new_line_answers_changes() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(3));
    let a = names(30);
    let mut b = a.clone();
    b.push("Brand new".into());
    read(Mode::Changes, "https://h/p", &buttons(&a), 120).await;
    let second = read(Mode::Changes, "https://h/p", &buttons(&b), 120).await;
    assert!(second.text.starts_with(CHANGES_HEADER), "{}", second.text);
    assert!(second.text.contains("+  button \"Brand new\""), "{}", second.text);
    assert!(!second.text.contains("Do 5"), "{}", second.text);
    page_compact::forget(SlotNo(3));
}

#[tokio::test]
async fn the_same_page_twice_answers_unchanged() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(3));
    let a = buttons(&names(30));
    read(Mode::Changes, "https://h/p", &a, 120).await;
    assert_eq!(read(Mode::Changes, "https://h/p", &a, 120).await.text, UNCHANGED_PAGE);
    page_compact::forget(SlotNo(3));
}

#[tokio::test]
async fn a_changed_address_answers_the_usual_page() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(3));
    let a = buttons(&names(30));
    read(Mode::Changes, "https://h/p", &a, 120).await;
    let other = read(Mode::Changes, "https://h/q", &a, 120).await;
    assert_eq!(other.text, render_frames_read(&a, &[], 120).text);
    // The fragment is not a different page.
    assert_eq!(read(Mode::Changes, "https://h/q#top", &a, 120).await.text, UNCHANGED_PAGE);
    page_compact::forget(SlotNo(3));
}

#[tokio::test]
async fn a_page_changed_everywhere_answers_the_usual_page() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(3));
    read(Mode::Changes, "https://h/p", &buttons(&names(10)), 120).await;
    let b: Vec<String> = (0..10).map(|i| format!("Other {i}")).collect();
    let second = read(Mode::Changes, "https://h/p", &buttons(&b), 120).await;
    assert_eq!(second.text, render_frames_read(&buttons(&b), &[], 120).text);
    page_compact::forget(SlotNo(3));
}

#[tokio::test]
async fn remember_mode_prints_the_usual_page_and_remembers_it() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(3));
    let a = buttons(&names(30));
    let first = read(Mode::Remember, "https://h/p", &a, 120).await;
    assert_eq!(first.text, render_frames_read(&a, &[], 120).text);
    assert_eq!(read(Mode::Changes, "https://h/p", &a, 120).await.text, UNCHANGED_PAGE);
    page_compact::forget(SlotNo(3));
}

#[tokio::test]
async fn a_plus_line_past_the_cap_is_filed() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(3));
    let a = names(130);
    let mut b = a.clone();
    b.insert(124, "Far down".into());
    read(Mode::Changes, "https://h/p", &buttons(&a), 120).await;
    let second = read(Mode::Changes, "https://h/p", &buttons(&b), 120).await;
    assert!(second.text.contains("Far down"), "{}", second.text);
    assert!(second.lines.iter().any(|l| l.name == "Far down"), "the changed line past the cap was not filed");
    page_compact::forget(SlotNo(3));
}

/// An empty page is what the reader was given: the page that comes back is
/// compared with nothing, not called unchanged.
#[tokio::test]
async fn an_empty_page_in_between_is_remembered() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(3));
    let a = buttons(&names(30));
    read(Mode::Changes, "https://h/p", &a, 120).await;
    let empty = read(Mode::Changes, "https://h/p", &[], 120).await;
    assert!(empty.text.contains("nothing a locator could name"), "{}", empty.text);
    let back = read(Mode::Changes, "https://h/p", &a, 120).await;
    assert_ne!(back.text, UNCHANGED_PAGE, "page A after an empty page was called unchanged");
    assert!(back.text.contains("Do 5"), "{}", back.text);
    page_compact::forget(SlotNo(3));
}

#[tokio::test]
async fn a_failed_scope_remembers_nothing() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(3));
    let a = buttons(&names(30));
    scoped(SlotNo(3), "a:", Mode::Changes, async {
        note_address("https://h/p");
        render_frames_read(&a, &[], 120);
        (500, String::new())
    })
    .await;
    assert!(remembered(SlotNo(3), "a:").is_none());
    page_compact::forget(SlotNo(3));
}

#[test]
fn page_whole_reads_the_option() {
    assert!(page_whole(r#"{"page":"whole"}"#));
    assert!(page_whole(r#"{"page":" Whole "}"#));
    assert!(!page_whole(r#"{"page":"full"}"#));
    assert!(!page_whole("{}"));
}

// A changes answer names a landmark only as an ancestor of a change: it must
// not replace what the reader is remembered to have of the other landmarks.

fn two_menus(top_links: usize) -> Vec<AxNode> {
    let top: Vec<String> = (0..top_links).map(|i| format!("t{i}")).collect();
    let top_ids: Vec<&str> = top.iter().map(String::as_str).collect();
    let mut nodes = vec![
        ax("0", "RootWebArea", "App", &["side", "top", "main"]),
        ax("side", "navigation", "Side menu", &["s0", "s1"]),
        ax("top", "navigation", "Top bar", &top_ids),
        ax("main", "main", "", &["b0"]),
        ax("s0", "link", "Home", &[]),
        ax("s1", "link", "Leave", &[]),
        ax("b0", "button", "Save", &[]),
    ];
    for (i, id) in top.iter().enumerate() {
        nodes.push(ax(id, "link", &format!("Top {i}"), &[]));
    }
    nodes
}

async fn route_read(address: &str, nodes: &[AxNode]) -> String {
    use v2_lib::ai_bridge::compacted_with;
    let (_, text) = compacted_with(SlotNo(4), "a:", false, Some(Mode::Changes), async {
        note_address(address);
        (200, render_frames_read(nodes, &[], 120).text)
    })
    .await;
    text
}

#[tokio::test]
async fn a_changes_answer_does_not_make_the_reader_forget_the_other_menus() {
    let _g = serial::page_menu();
    page_compact::forget(SlotNo(4));
    let first = route_read("https://h/p", &two_menus(2)).await;
    assert!(first.contains("Home") && first.contains("Top 0"), "{first}");
    let changed = route_read("https://h/p", &two_menus(3)).await;
    assert!(changed.starts_with(CHANGES_HEADER), "{changed}");
    assert!(changed.contains(" navigation \"Top bar\""), "the change sits under its landmark: {changed}");
    let elsewhere = route_read("https://h/q", &two_menus(3)).await;
    assert!(elsewhere.contains(&page_compact::unchanged_line(1, "Side menu")), "{elsewhere}");
    assert!(elsewhere.contains("Top 2") && !elsewhere.contains("Top bar\" unchanged"), "the changed menu prints whole: {elsewhere}");
    page_compact::forget(SlotNo(4));
}
