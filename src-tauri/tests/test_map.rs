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
        steps: vec![Step { action: "Open".into(), expected: "Shown".into() }],
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
    export_test_map_html(&tree(), &path, "PBI #42", &PagePalette::default()).unwrap();
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
    for id in ["map-expand", "map-collapse", "map-in", "map-out", "map-reset", "map-zoom", "viewport", "canvas", "detail"] {
        assert!(html.contains(&format!("id='{id}'")), "missing #{id}: {html}");
    }
    assert!(html.contains("getElementById('map-data')"), "the script reads the data block");
    // The app's palette and the light/dark switch, like every other report.
    assert!(html.contains("--accent"), "{html}");
    assert!(html.contains("data-scheme=\"light\""), "{html}");
}

#[test]
fn a_dark_app_opens_a_dark_page_and_a_blank_subtitle_says_the_count() {
    let path = tmp_path("map-dark.html");
    let palette = PagePalette { dark_first: true, ..PagePalette::default() };
    export_test_map_html(&tree(), &path, "", &palette).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("data-scheme=\"dark\""), "{html}");
    assert!(html.contains("<p class='subtitle'>3 test case(s)</p>"), "{html}");
}
