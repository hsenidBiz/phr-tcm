//! The Test map: a set of cases drawn as a tree of the areas they test, in
//! the browser - areas and cases as nodes, membership as limbs. The tree is
//! built here from each case's `area` path (`build_tree`) and the page is
//! written beside the review page that links to it (`write_beside`).
//!
//! Same page shell as the other reports - the app's palette, the
//! light/dark switch - but static: no revision poll, no comment boxes.
//! Opening it again rewrites the file.

use crate::import_parser::{esc, script_json};
use crate::steps_xml::Step;
use crate::webtheme::PagePalette;

const MAP_CSS: &str = include_str!("../web/test-map.css");
const MAP_JS: &str = include_str!("../web/test-map.js");
const GRAPH_JS: &str = include_str!("../web/test-map-graph.js");

/// One case as the map shows it: enough for the side panel, nothing the
/// app keeps to itself (no notes, no findings).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct MapCase {
    /// The work item id, or None for a draft not yet created.
    pub id: Option<i32>,
    pub title: String,
    pub steps: Vec<Step>,
    pub preconditions: String,
    pub tags: String,
    pub automation_status: String,
}

/// One area. `count` is this node's cases plus everything beneath it,
/// computed by the webview.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct MapNode {
    pub name: String,
    pub count: u32,
    pub cases: Vec<MapCase>,
    pub children: Vec<MapNode>,
}

/// Areas and cases in the whole tree, for the canvas's accessible name.
fn totals(nodes: &[MapNode]) -> (usize, usize) {
    nodes.iter().fold((0, 0), |(a, c), n| {
        let (ca, cc) = totals(&n.children);
        (a + 1 + ca, c + n.cases.len() + cc)
    })
}

/// The hidden list: every area as a heading, every case as a button. The
/// button's `data-i` is the case's position in this walk - cases before
/// children - which is the order `buildGraph` numbers its case nodes, so
/// the script finds a button's node by index.
fn list_html(nodes: &[MapNode], next: &mut usize, out: &mut String) {
    for n in nodes {
        out.push_str(&format!("<h3>{} ({})</h3>", esc(&n.name), n.count));
        if !n.cases.is_empty() {
            out.push_str("<ol>");
            for c in &n.cases {
                // "#id Title" for a case in Azure DevOps; a new case is its
                // title alone - the page says it is new when it is opened.
                let id = c.id.map(|i| format!("#{i} ")).unwrap_or_default();
                out.push_str(&format!(
                    "<li><button type='button' class='case' data-i='{}'>{id}{}</button></li>",
                    *next,
                    esc(&c.title)
                ));
                *next += 1;
            }
            out.push_str("</ol>");
        }
        list_html(&n.children, next, out);
    }
}

pub const UNGROUPED: &str = "Ungrouped";

/// Whether a set has anything to map: at least one case with an `area`.
/// Without one the tree would only be title groups, which the review page
/// already shows, so no link is offered.
pub fn has_areas(cases: &[crate::model::TestCase]) -> bool {
    cases.iter().any(|c| !crate::import_parser::normalise_area(&c.area).is_empty())
}

struct Draft {
    name: String,
    cases: Vec<MapCase>,
    children: Vec<Draft>,
}

impl Draft {
    /// The child named `name`, made if missing. Names compare without
    /// case, and the first spelling seen is the one shown.
    fn child(&mut self, name: &str) -> &mut Draft {
        let at = self.children.iter().position(|d| d.name.eq_ignore_ascii_case(name));
        let at = at.unwrap_or_else(|| {
            self.children.push(Draft { name: name.to_string(), cases: vec![], children: vec![] });
            self.children.len() - 1
        });
        &mut self.children[at]
    }
    fn finish(self) -> MapNode {
        let mut children: Vec<MapNode> = self.children.into_iter().map(Draft::finish).collect();
        children.sort_by(by_name);
        let count = self.cases.len() as u32 + children.iter().map(|c| c.count).sum::<u32>();
        MapNode { name: self.name, count, cases: self.cases, children }
    }
}

/// A-Z without case, "Ungrouped" last.
fn by_name(a: &MapNode, b: &MapNode) -> std::cmp::Ordering {
    match (a.name == UNGROUPED, b.name == UNGROUPED) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    }
}

fn to_map_case(tc: &crate::model::TestCase) -> MapCase {
    MapCase {
        id: tc.update_id,
        title: tc.title.clone(),
        steps: tc.steps.clone(),
        preconditions: tc.preconditions.clone(),
        tags: tc.tags.clone(),
        automation_status: tc.automation_status.clone(),
    }
}

/// The tree of areas: a case lands under its `area` path ("Manage Events /
/// Create" is two levels), a case without one under "Ungrouped". Cases keep
/// the order they were given; children sort A-Z, "Ungrouped" last.
pub fn build_tree(cases: &[crate::model::TestCase]) -> Vec<MapNode> {
    let mut root = Draft { name: String::new(), cases: vec![], children: vec![] };
    for tc in cases {
        let path = crate::import_parser::normalise_area(&tc.area);
        let mut node = &mut root;
        if path.is_empty() {
            node = node.child(UNGROUPED);
        } else {
            for segment in path.split(" / ") {
                node = node.child(segment);
            }
        }
        node.cases.push(to_map_case(tc));
    }
    root.finish().children
}

/// Write the Test map for these cases next to a review page, when there is
/// one to draw (`has_areas`). Returns the file NAME - the review page links
/// to it relatively, both being in the temp directory - or None, in which
/// case no link is offered. One file per process AND per report kind, like
/// the review pages: the draft page and the queue page each get their own
/// map, so refreshing one never overwrites the other's map or back-link.
///
/// `page_name` is the review page's own file name, so the map can link
/// back to it the same way. `kind` is `note_server::REPORT_DRAFT` or
/// `REPORT_QUEUE`.
pub fn write_beside(
    cases: &[crate::model::TestCase],
    subtitle: &str,
    palette: &PagePalette,
    page_name: &str,
    kind: &str,
) -> Result<Option<String>, String> {
    if !has_areas(cases) {
        return Ok(None);
    }
    let name = format!("test-map-{kind}-{}.html", std::process::id());
    let path = std::env::temp_dir().join(&name);
    export_test_map_html(&build_tree(cases), &path.to_string_lossy(), subtitle, palette, Some(page_name))?;
    Ok(Some(name))
}

/// Write the page to `path`. The tree travels inside it as a JSON script
/// block (never executed, `</` escaped, so a title cannot close it) and a
/// script draws it on load.
///
/// `back_href` is the review page this map was opened from; the header
/// links back to it. None (a map written on its own) shows no link.
pub fn export_test_map_html(
    nodes: &[MapNode],
    path: &str,
    subtitle: &str,
    palette: &PagePalette,
    back_href: Option<&str>,
) -> Result<(), String> {
    let total: u32 = nodes.iter().map(|n| n.count).sum();
    let subtitle = if subtitle.is_empty() {
        format!("{total} test case(s)")
    } else {
        esc(subtitle)
    };
    let (areas, cases) = totals(nodes);
    let mut list = String::new();
    list_html(nodes, &mut 0, &mut list);
    let empty = if cases == 0 { "<p id='map-empty' class='empty'>No test cases to map.</p>" } else { "" };
    let html = format!(
        "<!DOCTYPE html>\n\
         <html lang=\"en\" data-scheme=\"{scheme}\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Test map ({total})</title>\
         <style>{vars}{css}</style></head><body>{switch}\
         <header class='bar'>{back}<h1>Test map</h1><p class='subtitle'>{subtitle}</p>\
         <div class='tools'>\
         <button type='button' id='map-expand'>Expand all</button>\
         <button type='button' id='map-collapse'>Collapse all</button>\
         <span class='sep' aria-hidden='true'></span>\
         <button type='button' id='map-out' aria-label='Zoom out'>-</button>\
         <button type='button' id='map-in' aria-label='Zoom in'>+</button>\
         <button type='button' id='map-reset'>Reset</button>\
         <span id='map-zoom' aria-live='polite'>100%</span>\
         </div></header>\
         <div class='layout'>\
         <div id='viewport' class='viewport'>\
         <canvas id='graph' role='img' aria-label='Test map: {areas} areas, {cases} test cases'></canvas>\
         {empty}</div>\
         <div id='map-list' class='sr-only'>{list}</div>\
         <aside id='detail' class='detail' aria-label='Test case' hidden></aside>\
         </div>\
         <script type='application/json' id='map-data'>{data}</script>\
         <script>{graph_js}</script>\
         <script>{js}</script>\
         <script>{switch_js}</script>\
         </body></html>",
        back = match back_href {
            Some(href) => format!("<a id='map-back' class='back' href='{}'>\u{2190} Test cases</a>", esc(href)),
            None => String::new(),
        },
        scheme = palette.initial_scheme(),
        vars = palette.css(),
        css = MAP_CSS,
        switch = crate::webtheme::SWITCH_HTML,
        areas = areas,
        cases = cases,
        empty = empty,
        list = list,
        data = script_json(&nodes, "[]"),
        graph_js = GRAPH_JS,
        js = MAP_JS,
        switch_js = crate::webtheme::SWITCH_JS,
    );
    std::fs::write(path, html).map_err(|e| e.to_string())
}
