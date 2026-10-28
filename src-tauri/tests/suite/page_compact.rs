//! A page answer that does not say a side menu again (`autorun::page_compact`),
//! the page argument that gives it whole, and `get_autorun_page`'s query.

use v2_lib::ai_bridge::{compacted, page_full, page_wanted, query_lines};
use v2_lib::autorun::page_compact::{collapse, forget, known_for, remember, scoped, unchanged_line};
use v2_lib::autorun::slots::SlotNo;
use v2_lib::browser::snapshot::{render_frames_read, AxNode};

/// A page with a side menu of two links and a form under a heading. The memory
/// is process-wide, so a test holds `serial::page_menu` and forgets the slots it uses.
fn page(menu_item: &str) -> String {
    format!(
        "RootWebArea \"Leave\"\n navigation \"Side menu\" -> {{}}\n  link \"Home\" -> {{}}\n  link \"{menu_item}\" -> {{}}\n main \"\" -> {{}}\n  heading \"Apply\" -> {{}}\n  button \"Save\" -> {{}}"
    )
}

fn printed(text: &str) -> Vec<(String, u8)> {
    text.lines().map(|l| (l.to_string(), 0u8)).collect()
}

/// What `collapse` prints of `text` for a reader who has `known`.
async fn shown(known: Vec<String>, text: &str) -> String {
    let kept = scoped(known, async { collapse(printed(text)) }).await;
    kept.into_iter().map(|(t, _)| t).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n")
}

#[tokio::test]
async fn the_same_side_menu_is_one_line_naming_it_the_second_time() {
    let _held = crate::serial::page_menu();
    let _auto = crate::serial::autorun();
    let slot = SlotNo(1);
    forget(slot);
    assert!(known_for(slot, "a:", false).is_empty());
    remember(slot, "a:", &page("Leave"));
    let known = known_for(slot, "a:", false);
    let second = shown(known.clone(), &page("Leave")).await;
    assert!(second.contains(&unchanged_line(1, "Side menu")), "{second}");
    assert!(!second.contains("link \"Home\""), "{second}");
    assert!(second.contains("button \"Save\"") && second.contains("heading \"Apply\""), "the rest stays: {second}");
    // A changed menu is whole, and is what is remembered next.
    let changed = shown(known, &page("Claims")).await;
    assert!(changed.contains("link \"Claims\"") && changed.contains("link \"Home\""), "{changed}");
    remember(slot, "a:", &page("Claims"));
    assert!(shown(known_for(slot, "a:", false), &page("Claims")).await.contains("unchanged since the last answer"));
}

#[tokio::test]
async fn full_and_forget_and_other_readers_get_the_whole_tree() {
    let _held = crate::serial::page_menu();
    let _auto = crate::serial::autorun();
    let slot = SlotNo(1);
    forget(slot);
    remember(slot, "a:", &page("Leave"));
    assert!(known_for(slot, "a:", true).is_empty(), "page full");
    assert!(known_for(slot, "b:", false).is_empty(), "another reader was told nothing");
    assert!(known_for(SlotNo(2), "a:", false).is_empty(), "another slot");
    assert!(shown(known_for(slot, "b:", false), &page("Leave")).await.contains("link \"Home\""));
    forget(slot);
    assert!(known_for(slot, "a:", false).is_empty());
}

#[tokio::test]
async fn only_landmarks_outside_main_collapse_and_each_has_its_own_line() {
    let _held = crate::serial::page_menu();
    let _auto = crate::serial::autorun();
    let slot = SlotNo(1);
    forget(slot);
    let text = "RootWebArea \"x\"\n navigation \"Side menu\" -> {}\n  link \"Home\" -> {}\n navigation \"Top bar\" -> {}\n  link \"Help\" -> {}\n main \"\" -> {}\n  navigation \"Breadcrumb\" -> {}\n   link \"Leave\" -> {}\n  button \"Save\" -> {}";
    remember(slot, "a:", text);
    let out = shown(known_for(slot, "a:", false), text).await;
    assert!(out.contains(&unchanged_line(1, "Side menu")) && out.contains(&unchanged_line(1, "Top bar")), "{out}");
    assert!(!out.contains("link \"Home\"") && !out.contains("link \"Help\""), "{out}");
    assert!(out.contains("navigation \"Breadcrumb\" -> {}") && out.contains("link \"Leave\""), "the breadcrumb stays: {out}");
    // One landmark changing leaves the other collapsed.
    let changed = text.replace("link \"Help\"", "link \"Support\"");
    let out = shown(known_for(slot, "a:", false), &changed).await;
    assert!(out.contains(&unchanged_line(1, "Side menu")) && out.contains("link \"Support\""), "{out}");
    // Collapsed ones are remembered as the reader was given them.
    remember(slot, "a:", &out);
    assert!(shown(known_for(slot, "a:", false), &changed).await.contains(&unchanged_line(1, "Top bar")));
}

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

/// A side menu of 30 links beside a main with 30 buttons.
fn big_page() -> Vec<AxNode> {
    let mut nodes = vec![ax("0", "RootWebArea", "App", &["nav", "main"])];
    let links: Vec<String> = (0..30).map(|i| format!("l{i}")).collect();
    let buttons: Vec<String> = (0..30).map(|i| format!("b{i}")).collect();
    let l: Vec<&str> = links.iter().map(String::as_str).collect();
    let b: Vec<&str> = buttons.iter().map(String::as_str).collect();
    nodes.push(ax("nav", "navigation", "Side menu", &l));
    nodes.push(ax("main", "main", "", &b));
    for i in 0..30 {
        nodes.push(ax(&format!("l{i}"), "link", &format!("Menu {i}"), &[]));
        nodes.push(ax(&format!("b{i}"), "button", &format!("Do {i}"), &[]));
    }
    nodes
}

/// The collapse comes before the line limit: what the menu would have used
/// shows more of the page.
#[tokio::test]
async fn the_collapse_comes_before_the_limit_so_more_of_the_page_shows() {
    let nodes = big_page();
    let plain = render_frames_read(&nodes, &[], 20);
    assert!(plain.text.contains("Menu 5") && !plain.text.contains("Do 0\""), "without the collapse the menu uses the budget");
    let whole = render_frames_read(&nodes, &[], 1000);
    let menu: Vec<&str> = whole.text.lines().filter(|l| l.contains("navigation") || l.contains("Menu ")).collect();
    let read = scoped(vec![menu.join("\n")], async { render_frames_read(&nodes, &[], 20) }).await;
    assert!(read.text.contains(&unchanged_line(1, "Side menu")), "{}", read.text);
    assert!(!read.text.contains("Menu 5"), "{}", read.text);
    assert!(read.text.contains("Do 15"), "the budget went to the page: {}", read.text);
    assert!(read.text.contains("more lines"), "the figure counts what is left: {}", read.text);
    // What it files is still every line it printed or collapsed.
    assert!(read.lines.iter().any(|l| l.name == "Menu 5"), "the menu is still filed");
}

#[tokio::test]
async fn a_route_read_is_remembered_for_its_reader_only() {
    let _held = crate::serial::page_menu();
    let _auto = crate::serial::autorun();
    let slot = SlotNo(1);
    forget(slot);
    let read = |p: String| async move { (200u16, serde_json::json!({ "sentence": "ok", "page": p }).to_string()) };
    let a = compacted(slot, "a:", false, read(page("Leave"))).await;
    assert!(a.1.contains("Side menu"));
    assert!(!known_for(slot, "a:", false).is_empty());
    assert!(known_for(slot, "b:", false).is_empty());
    let failed = compacted(slot, "c:", false, async { (409u16, page("Leave")) }).await;
    assert_eq!(failed.0, 409);
    assert!(known_for(slot, "c:", false).is_empty(), "a failure remembers nothing");
    let batch = compacted(slot, "d:", false, async { (200u16, format!("1. ok: click\n\n{}", page("Leave"))) }).await;
    assert!(batch.1.starts_with("1. ok"));
    assert!(!known_for(slot, "d:", false).is_empty());
    forget(slot);
}

#[test]
fn page_arguments_are_read_from_the_body() {
    assert!(page_full(r#"{"page":"full"}"#) && page_full(r#"{"page":" Full "}"#));
    assert!(!page_full(r#"{"page":true}"#) && !page_full("{}") && !page_full("nonsense"));
    assert!(!page_wanted(r#"{"page":false}"#) && !page_wanted(r#"{"page":"false"}"#));
    assert!(page_wanted(r#"{"page":"full"}"#) && page_wanted(r#"{"page":true}"#) && page_wanted("{}"));
}

#[test]
fn a_query_keeps_the_matching_lines_with_the_lines_they_sit_under() {
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
