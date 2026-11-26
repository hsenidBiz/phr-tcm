//! The Test map page: a self-contained HTML file carrying the tree as
//! JSON and the script that draws it.

use v2_lib::steps_xml::Step;
use v2_lib::test_map::{export_test_map_html, MapCase, MapNode};
use v2_lib::webtheme::PagePalette;

fn tmp_path(name: &str) -> String {
    let dir = std::env::temp_dir().join("tcm-v2-test-map-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{}-{}", std::process::id(), name))
        .to_string_lossy()
        .to_string()
}

fn case(id: Option<i32>, title: &str) -> MapCase {
    MapCase {
        id,
        title: title.into(),
        steps: vec![Step { action: "Open".into(), expected: "Shown".into(), shared: None, attachments: vec![] }],
        preconditions: "Signed in".into(),
        tags: "smoke".into(),
        automation_status: "Not Automated".into(),
    }
}

fn tree() -> Vec<MapNode> {
    vec![MapNode {
        name: "Manage Events".into(),
        count: 3,
        cases: vec![case(Some(81310), "Page navigation")],
        children: vec![MapNode {
            name: "Create".into(),
            count: 2,
            cases: vec![case(Some(81314), "Validation & limits"), case(None, "Fill details</script><b>x</b>")],
            children: vec![],
        }],
    }]
}

#[test]
fn the_page_carries_the_tree_as_json_and_the_script_that_draws_it() {
    let path = tmp_path("map.html");
    export_test_map_html(&tree(), &path, "PBI #42", &PagePalette::default(), Some("test-cases-7.html")).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();

    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(html.contains("<title>Test map (3)</title>"), "the count is the root total: {html}");
    assert!(html.contains("<p class='subtitle'>PBI #42</p>"), "{html}");

    // The data block: present, and a title can never close it early.
    let data = html
        .split("<script type='application/json' id='map-data'>")
        .nth(1)
        .and_then(|rest| rest.split("</script>").next())
        .expect("the data block");
    assert!(data.contains("\"Manage Events\""), "{data}");
    assert!(data.contains("81310"), "{data}");
    assert!(data.contains("\\u003c/script"), "{data}");
    assert!(!html.contains("Fill details</script>"), "{html}");
    let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
    assert_eq!(parsed[0]["children"][0]["cases"][1]["title"], "Fill details</script><b>x</b>");

    // The chrome the script drives, and the script itself.
    for id in ["map-expand", "map-collapse", "viewport", "graph", "detail", "map-list", "map-desc"] {
        assert!(html.contains(&format!("id='{id}'")), "missing #{id}: {html}");
    }
    // The outline reads at normal size, so the zoom controls are gone.
    for id in ["map-in", "map-out", "map-reset", "map-zoom"] {
        assert!(!html.contains(&format!("id='{id}'")), "the zoom control #{id} was removed: {html}");
    }
    assert!(html.contains("getElementById('map-data')"), "the script reads the data block");
    assert!(html.contains("root.testMap = {"), "the graph helpers are embedded");
    assert!(
        html.find("root.testMap = {").unwrap() < html.find("getElementById('map-data')").unwrap(),
        "the helpers load before the page script uses them"
    );
    // The canvas says what it is; the hidden list carries every case for
    // screen readers and the keyboard, numbered the way the script numbers
    // its case nodes (an area's cases before its children).
    assert!(
        html.contains("<canvas id='graph' role='img' aria-label='Test map: 2 areas, 3 test cases' aria-describedby='map-desc'>"),
        "{html}"
    );
    let list = html
        .split("<div id='map-list' class='sr-only'>")
        .nth(1)
        .and_then(|rest| rest.split("<aside id='detail'").next())
        .expect("the hidden list");
    assert!(list.contains("<h3>Manage Events (3)</h3>"), "{list}");
    assert!(list.contains("<h3>Create (2)</h3>"), "{list}");
    // Each case ends with what an upload would do, as its row's tag says.
    assert!(list.contains("<button type='button' class='case' data-i='0'>#81310 Page navigation (update)</button>"), "{list}");
    assert!(list.contains("<button type='button' class='case' data-i='1'>#81314 Validation &amp; limits (update)</button>"), "{list}");
    assert!(
        list.contains("<button type='button' class='case' data-i='2'>Fill details&lt;/script&gt;&lt;b&gt;x&lt;/b&gt; (new)</button>"),
        "{list}"
    );
    assert!(!list.contains("<b>x</b>"), "titles are escaped in the list: {list}");
    // The app's palette and the light/dark switch, like every other report.
    assert!(html.contains("--accent"), "{html}");
    assert!(html.contains("data-scheme=\"light\""), "{html}");
    // The way back to the review page it was opened from.
    assert!(html.contains("<a id='map-back' class='back' href='test-cases-7.html'>"), "{html}");
}

#[test]
fn an_empty_map_says_so_and_still_has_the_chrome() {
    let path = tmp_path("map-empty.html");
    export_test_map_html(&[], &path, "", &PagePalette::default(), None).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("<p id='map-empty' class='empty'>No test cases to map.</p>"), "{html}");
    assert!(html.contains("aria-label='Test map: 0 areas, 0 test cases'"), "{html}");
    assert!(html.contains("<div id='map-list' class='sr-only'></div>"), "{html}");
}

#[test]
fn a_dark_app_opens_a_dark_page_and_a_blank_subtitle_says_the_count() {
    let path = tmp_path("map-dark.html");
    let palette = PagePalette { dark_first: true, ..PagePalette::default() };
    export_test_map_html(&tree(), &path, "", &palette, None).unwrap();
    assert!(!html_has_back(&path), "a map written on its own has nothing to go back to");
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("data-scheme=\"dark\""), "{html}");
    assert!(html.contains("<p class='subtitle'>3 test case(s)</p>"), "{html}");
}

// ---- build_tree: the area paths become the tree -------------------------

fn tc(title: &str, area: &str, id: Option<i32>) -> v2_lib::model::TestCase {
    v2_lib::model::TestCase {
        title: title.into(),
        area: area.into(),
        update_id: id,
        steps: vec![Step { action: "Open".into(), expected: "Shown".into(), shared: None, attachments: vec![] }],
        ..Default::default()
    }
}

#[test]
fn build_tree_nests_area_paths_merges_spellings_and_sorts() {
    use v2_lib::test_map::build_tree;
    let cases = vec![
        tc("Nav", "Manage Events", Some(1)),
        tc("Limits", "Manage Events / Create / Validation", Some(2)),
        tc("Fill", "manage events/create", None),
        tc("Export", "Reports", Some(4)),
        tc("Loose", "", Some(5)),
        tc("Also loose", "  /  ", None),
    ];
    let tree = build_tree(&cases);
    assert_eq!(
        tree.iter().map(|n| (n.name.as_str(), n.count)).collect::<Vec<_>>(),
        vec![("Manage Events", 3), ("Reports", 1), ("Ungrouped", 2)],
        "A-Z, Ungrouped last, counts include descendants"
    );
    let manage = &tree[0];
    assert_eq!(manage.cases.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["Nav"]);
    assert_eq!(manage.cases[0].id, Some(1));
    let create = &manage.children[0];
    assert_eq!(create.name, "Create", "first spelling seen is the one shown");
    assert_eq!(create.count, 2);
    assert_eq!(create.cases.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["Fill"]);
    assert_eq!(create.cases[0].id, None);
    assert_eq!(create.children[0].name, "Validation");
    assert_eq!(create.children[0].cases[0].title, "Limits");
    let loose = &tree[2];
    assert_eq!(loose.cases.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["Loose", "Also loose"]);
}

#[test]
fn has_areas_and_write_beside_need_at_least_one_area() {
    use v2_lib::test_map::{has_areas, write_beside};
    let none = vec![tc("A", "", None), tc("B", " / ", None)];
    assert!(!has_areas(&none));
    assert_eq!(write_beside(&none, "", &PagePalette::default(), "test-cases-1.html", "queue").unwrap(), None);

    let some = vec![tc("A", "", None), tc("B", "Reports", None)];
    assert!(has_areas(&some));
    let name = write_beside(&some, "PBI #9", &PagePalette::default(), "test-cases-draft-1.html", "draft")
        .unwrap()
        .expect("a map file");
    assert_eq!(name, format!("test-map-draft-{}.html", std::process::id()));
    // The queue page's map is its own file: refreshing one page must never
    // overwrite the other's map, or its back-link.
    let queue_name = write_beside(&some, "PBI #9", &PagePalette::default(), "test-cases-1.html", "queue")
        .unwrap()
        .expect("a map file");
    assert_eq!(queue_name, format!("test-map-queue-{}.html", std::process::id()));

    let html = std::fs::read_to_string(std::env::temp_dir().join(&name)).unwrap();
    assert!(html.contains("<p class='subtitle'>PBI #9</p>"), "{html}");
    assert!(html.contains("\"Reports\""), "{html}");
    assert!(html.contains("\"Ungrouped\""), "the area-less case is still on the map: {html}");
    assert!(html.contains("href='test-cases-draft-1.html'"), "links back to the page it sits beside: {html}");
}

fn html_has_back(path: &str) -> bool {
    std::fs::read_to_string(path).unwrap().contains("id='map-back'")
}

#[test]
fn limbs_are_drawn_in_the_muted_token_not_the_hairline_border() {
    let path = tmp_path("map-limbs.html");
    export_test_map_html(&tree(), &path, "", &PagePalette::default(), None).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    // --border is about 1.1:1 on the page: a limb is an essential mark and
    // needs 3:1, which --muted gives in both palettes (see webtheme.rs).
    assert!(!html.contains("strokeStyle = colours.border"), "no stroke may use the hairline");
    assert!(html.contains("strokeStyle = colours.muted"), "limbs use the stronger token");
}

#[test]
fn the_outline_rows_say_their_count_and_their_tag_without_a_click() {
    let path = tmp_path("map-rows.html");
    export_test_map_html(&tree(), &path, "", &PagePalette::default(), None).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    // The graph helpers write "Name · count" for an area and NEW / UPDATE for
    // a case, and the page script draws both on every row.
    assert!(html.contains("node.name + ' \u{b7} ' + node.count"), "an area row always carries its count: {html}");
    assert!(html.contains("'UPDATE'") && html.contains("'NEW'"), "{html}");
    assert!(html.contains("G.areaLabel(n)") && html.contains("G.caseTag(n)"), "the rows draw them: {html}");
    // Only the case title is cut; the area's row is drawn whole.
    assert!(html.contains("G.clipLabel(n.data.title"), "{html}");
    assert!(!html.contains("clipLabel(G.areaLabel"), "an area is never clipped: {html}");
    // Row height 24, elbows 8, and no zoom-to-read thresholds.
    assert!(html.contains("var ROW = 24, INDENT = 24"), "{html}");
    assert!(html.contains("ELBOW_R = 8"), "{html}");
    assert!(!html.contains("labelAlpha"), "labels no longer wait for a zoom: {html}");
    // The dot no longer encodes the count (the count is written).
    assert!(!html.contains("areaRadius"), "{html}");
}

#[test]
fn the_accent_is_only_for_the_hovered_chain_and_the_selected_case() {
    let path = tmp_path("map-accent.html");
    export_test_map_html(&tree(), &path, "", &PagePalette::default(), None).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    // Dots are accent only when lit or selected; area dots are not accent.
    assert!(html.contains("lit || selected ? colours.accent : colours.muted"), "{html}");
    assert!(!html.contains("n.kind === 'area' ? colours.accent"), "no accent fill on every area dot: {html}");
    // The tags stay muted whichever word they carry: one fill for both.
    assert!(html.contains("ctx.fillText(tag, tagX + TAG_PAD") && !html.contains("colours.accent, tagX"), "{html}");
}

#[test]
fn the_legend_names_only_the_tags_the_map_uses() {
    let path = tmp_path("map-legend.html");
    // `tree()` has both an update (#81310) and a new case.
    export_test_map_html(&tree(), &path, "", &PagePalette::default(), None).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    let legend = html.split("<footer class='legend'>").nth(1).and_then(|r| r.split("</footer>").next()).expect("a legend strip");
    assert!(legend.contains(">NEW</span>created on upload") && legend.contains(">UPDATE</span>updates its work item"), "{legend}");
    assert!(legend.contains("Open area") && legend.contains("Folded area") && legend.contains("Test case"), "{legend}");
    assert!(!legend.contains("dot size"), "the dot no longer encodes the count: {legend}");

    let only_updates = vec![MapNode { name: "A".into(), count: 1, cases: vec![case(Some(1), "x")], children: vec![] }];
    let path = tmp_path("map-legend2.html");
    export_test_map_html(&only_updates, &path, "", &PagePalette::default(), None).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    let legend = html.split("<footer class='legend'>").nth(1).and_then(|r| r.split("</footer>").next()).unwrap();
    assert!(legend.contains("UPDATE") && !legend.contains(">NEW<"), "{legend}");

    // An empty map has nothing to explain.
    let path = tmp_path("map-legend3.html");
    export_test_map_html(&[], &path, "", &PagePalette::default(), None).unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("<footer class='legend'>"));
}

#[test]
fn the_canvas_is_described_for_assistive_technology() {
    let path = tmp_path("map-desc.html");
    export_test_map_html(&tree(), &path, "", &PagePalette::default(), None).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    // A title (the canvas's name, 60 characters or fewer) and a one-sentence
    // description it points at; the hidden list beside it carries every case.
    let name = html.split("aria-label='Test map: ").nth(1).unwrap().split('\'').next().unwrap();
    assert!(("Test map: ".len() + name.len()) <= 60, "{name}");
    let desc = html.split("<p id='map-desc' class='sr-only'>").nth(1).unwrap().split("</p>").next().unwrap();
    assert!(desc.starts_with("An outline of the areas"), "{desc}");
    assert!(html.contains("aria-describedby='map-desc'"));
}

#[test]
fn a_cut_title_shows_whole_on_hover_and_folded_areas_have_their_own_mark() {
    let path = tmp_path("map-review.html");
    export_test_map_html(&tree(), &path, "", &PagePalette::default(), None).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    // Hover: the canvas tooltip is the whole label while a cut row is hovered, empty otherwise.
    assert!(html.contains("n.cut = title !== (n.data.title || '')"), "{html}");
    assert!(html.contains("hover && hover.kind === 'case' && hover.cut ? G.caseLabel(hover) : ''"), "{html}");
    assert!(html.contains("canvas.title = tip"), "{html}");
    // A folded area: a ring with a dot inside; the legend names it only once something is folded.
    assert!(html.contains("if (n.folded) { ctx.beginPath(); ctx.arc(px, py, 1.5"), "{html}");
    assert!(html.contains("<li id='k-folded' hidden>"), "{html}");
    assert!(html.contains("document.getElementById('k-folded')"), "{html}");
    // Ctrl + wheel is left to the browser, and a pan lands on whole pixels.
    assert!(html.contains("if (e.ctrlKey) return;"), "{html}");
    assert!(html.contains("tx = Math.round(t.tx); ty = Math.round(t.ty);"), "{html}");
}
