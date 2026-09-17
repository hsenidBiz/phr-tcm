//! The Test map: a set of cases drawn as a graph of the areas they test, in
//! the browser - areas and cases as nodes, membership as edges, laid out by
//! a force simulation. The webview builds the tree (one grouping
//! implementation, in `src/lib/testMap.ts`); this side only writes the page.
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

/// Write the page to `path`. The tree travels inside it as a JSON script
/// block (never executed, `</` escaped, so a title cannot close it) and a
/// script draws it on load.
pub fn export_test_map_html(
    nodes: &[MapNode],
    path: &str,
    subtitle: &str,
    palette: &PagePalette,
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
         <header class='bar'><h1>Test map</h1><p class='subtitle'>{subtitle}</p>\
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
