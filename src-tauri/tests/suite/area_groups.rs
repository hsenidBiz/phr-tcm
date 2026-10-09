//! Group by area: the tree `import_parser::area_groups` builds (the same
//! cases `src/lib/areaGroups.test.ts` checks on the app's side), and the
//! review page View in browser writes from it while the Queue is grouped.

use v2_lib::import_parser::area_groups::{build, AreaGroup};
use v2_lib::import_parser::{
    export_queue_page, export_queue_page_in, CommentCtx, DraftNoteCtx, PageLayout,
};
use v2_lib::model::TestCase;

fn cases(areas: &[&str]) -> Vec<TestCase> {
    areas
        .iter()
        .enumerate()
        .map(|(i, a)| TestCase { title: format!("Case {i}"), area: (*a).to_string(), ..Default::default() })
        .collect()
}

/// name(count)[indices]{children} - the whole tree in one comparable line.
fn shape(groups: &[AreaGroup]) -> String {
    groups
        .iter()
        .map(|g| {
            let idx = g.indices.iter().map(usize::to_string).collect::<Vec<_>>().join(",");
            let kids = if g.children.is_empty() { String::new() } else { format!("{{{}}}", shape(&g.children)) };
            format!("{}({})[{idx}]{kids}", g.name, g.count)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn nests_one_level_per_segment_three_levels_deep() {
    let tree = build(&cases(&["Events / Create / Form", "Events / Create", "Events"]));
    assert_eq!(shape(&tree), "Events(3)[2]{Create(2)[1]{Form(1)[0]}}");
    let form = &tree[0].children[0].children[0];
    assert_eq!(form.path, "Events / Create / Form");
    assert_eq!(form.key, "events / create / form");
}

#[test]
fn groups_appear_in_the_order_of_their_first_case_not_a_to_z() {
    let tree = build(&cases(&["Zeta", "Alpha", "Zeta / B", "Zeta / A", "Alpha"]));
    assert_eq!(shape(&tree), "Zeta(3)[0]{B(1)[2] A(1)[3]} Alpha(2)[1,4]");
}

#[test]
fn cases_with_no_area_go_under_no_area_last() {
    let tree = build(&cases(&["", "Reports", "  /  ", "Billing"]));
    assert_eq!(shape(&tree), "Reports(1)[1] Billing(1)[3] No area(2)[0,2]");
    assert_eq!(tree[2].key, "");
}

#[test]
fn a_real_area_named_ungrouped_stays_apart_from_the_no_area_bucket() {
    let tree = build(&cases(&["Ungrouped", ""]));
    assert_eq!(shape(&tree), "Ungrouped(1)[0] No area(1)[1]");
    assert_eq!(tree.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(), vec!["ungrouped", ""]);
}

/// An area is the user's own text: in the grouped page its name is escaped
/// as markup and its key as a single-quoted attribute value.
#[test]
fn an_area_with_markup_characters_is_escaped_in_the_grouped_page() {
    let queue = cases(&["Reports <b> & Exports / O'Brien's"]);
    let html = page(&queue, "escaped", PageLayout::ByArea);
    assert!(html.contains("<span class='tc-group-name'>Reports &lt;b&gt; &amp; Exports</span>"), "{html}");
    assert!(html.contains("<span class='tc-group-name'>O'Brien's</span>"), "{html}");
    assert!(html.contains("data-area='reports &lt;b&gt; &amp; exports'"), "{html}");
    assert!(html.contains("data-area='reports &lt;b&gt; &amp; exports / o&#39;brien&#39;s'"), "{html}");
    assert!(!html.contains("<b> &"), "{html}");
}

#[test]
fn segments_match_ignoring_case_and_surrounding_spaces() {
    let tree = build(&cases(&["Display", "display ", " DISPLAY/ Rules", "display / rules "]));
    assert_eq!(shape(&tree), "Display(4)[0,1]{Rules(2)[2,3]}");
    assert_eq!(tree[0].children[0].path, "Display / Rules");
}

#[test]
fn counts_include_nested_cases() {
    let tree = build(&cases(&["A / B / C", "A / B / C", "A / B", "A / D", "A"]));
    assert_eq!(tree[0].count, 5);
    assert_eq!(tree[0].children.iter().map(|c| c.count).collect::<Vec<_>>(), vec![3, 1]);
    assert_eq!(tree[0].all_indices(), vec![4, 2, 0, 1, 3]);
}

fn page(queue: &[TestCase], name: &str, layout: PageLayout) -> String {
    let dir = std::env::temp_dir().join("tcm-v2-area-group-page-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}-{name}.html", std::process::id())).to_string_lossy().to_string();
    export_queue_page_in(queue, &path, "PBI #7", None, &Default::default(), None, &[], layout).unwrap();
    std::fs::read_to_string(&path).unwrap()
}

/// Where each of `needles` first appears, in order - panics on a missing one.
fn positions(html: &str, needles: &[&str]) -> Vec<usize> {
    needles.iter().map(|n| html.find(n).unwrap_or_else(|| panic!("missing {n}: {html}"))).collect()
}

#[test]
fn the_grouped_page_nests_sections_in_queue_order_with_no_area_last() {
    let queue = cases(&["", "Zeta", "Events / Create / Form", "Alpha", "events / Create", "zeta "]);
    let html = page(&queue, "grouped", PageLayout::ByArea);

    // Every section a native disclosure, open, naming its area and count.
    assert!(html.contains(
        "<details class='tc-group' open data-area='zeta' data-level='0'><summary>\
         <span class='tc-group-name'>Zeta</span> <span class='tc-group-count'>(2)</span></summary>\
         <div class='tc-group-body'>"
    ), "{html}");
    assert!(html.contains("data-area='events / create / form' data-level='2'"), "{html}");
    assert!(html.contains("<span class='tc-group-name'>No area</span> <span class='tc-group-count'>(1)</span>"), "{html}");

    // Sections and cases read top to bottom in queue order, No area last;
    // each case keeps its queue position as its number and its key.
    let order = positions(
        &html,
        &[
            "data-area='zeta'",
            "data-key='d1'",
            "data-key='d5'",
            "data-area='events'",
            "data-area='events / create'",
            "data-key='d4'",
            "data-area='events / create / form'",
            "data-key='d2'",
            "data-area='alpha'",
            "data-key='d3'",
            "data-area=''",
            "data-key='d0'",
        ],
    );
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted, "out of order: {html}");
    assert!(html.contains("<span class='seq'>6</span>"), "{html}");

    // Nested: Form's section closes inside Create's, which closes inside
    // Events' - three closings in a row after the last Form case.
    let after_form = &html[html.find("data-key='d2'").unwrap()..];
    let tail = &after_form[..after_form.find("data-area='alpha'").unwrap()];
    assert_eq!(tail.matches("</div></details>").count(), 3, "{tail}");
    // Every section opened is closed (below the search bar, and these
    // cases have no notes or findings, whose blocks close the same way).
    let cases_part = &html[html.find("id='tc-no-match'").unwrap()..];
    let cases_part = &cases_part[..cases_part.find("<script").unwrap()];
    assert_eq!(cases_part.matches("<details class='tc-group'").count(), 6);
    assert_eq!(cases_part.matches("</div></details>").count(), 6, "{cases_part}");
}

#[test]
fn the_flat_page_is_unchanged_when_not_grouped() {
    let queue = cases(&["Zeta", "", "Alpha"]);
    // The same path for both: a draft page's bookmark scope is derived
    // from it, so two paths would differ there and nowhere else.
    let flat = page(&queue, "flat", PageLayout::Flat);
    let dir = std::env::temp_dir().join("tcm-v2-area-group-page-tests");
    let path = dir.join(format!("{}-flat.html", std::process::id())).to_string_lossy().to_string();
    export_queue_page(&queue, &path, "PBI #7", None, &Default::default(), None, &[]).unwrap();
    let plain = std::fs::read_to_string(&path).unwrap();
    if flat != plain {
        let at = flat.bytes().zip(plain.bytes()).position(|(a, b)| a != b).unwrap_or(0);
        panic!("differ at {at}: {:?} vs {:?}", &flat[at.saturating_sub(80)..(at + 80).min(flat.len())], &plain[at.saturating_sub(80)..(at + 80).min(plain.len())]);
    }
    assert!(!flat.contains("<details class='tc-group' open"), "{flat}");
    let order = positions(&flat, &["data-key='d0'", "data-key='d1'", "data-key='d2'"]);
    assert!(order[0] < order[1] && order[1] < order[2], "{flat}");
}

/// A comment box is addressed by its slot in the page's identity list, and
/// that list follows the QUEUE whatever the layout: the box under a case
/// in a grouped page still writes back to that case.
#[test]
fn grouping_keeps_each_comment_box_on_its_own_case() {
    let queue = cases(&["Zeta", "Alpha", "Zeta"]);
    let ctx = DraftNoteCtx {
        port: 1,
        token: "t".into(),
        owners: vec![String::new(); 3],
        keys: vec!["k0".into(), "k1".into(), "k2".into()],
        pbi_id: 7,
        files: vec![],
    };
    let dir = std::env::temp_dir().join("tcm-v2-area-group-page-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}-notes.html", std::process::id())).to_string_lossy().to_string();
    export_queue_page_in(&queue, &path, "PBI #7", Some(CommentCtx::Draft(&ctx)), &Default::default(), None, &[], PageLayout::ByArea)
        .unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    // Case 2 (Zeta) renders before case 1 (Alpha), with its own slot.
    let order = positions(&html, &["data-key='d2'", "data-case='2'", "data-key='d1'", "data-case='1'"]);
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{html}");
    assert!(html.contains("\"key\":\"k2\""), "{html}");
}
