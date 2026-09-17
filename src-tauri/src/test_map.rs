//! The Test map: a set of cases drawn as a tree of the areas they test, in
//! the browser. The webview builds the tree (one grouping implementation,
//! in `src/lib/testMap.ts`); this side only writes the page.
//!
//! Same page shell as the other reports - the app's palette, the
//! light/dark switch - but static: no revision poll, no comment boxes.
//! Opening it again rewrites the file.

use crate::import_parser::{esc, script_json};
use crate::steps_xml::Step;
use crate::webtheme::PagePalette;

const MAP_CSS: &str = include_str!("../web/test-map.css");
const MAP_JS: &str = include_str!("../web/test-map.js");

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
         <div id='viewport' class='viewport'><div id='canvas' class='canvas'></div></div>\
         <aside id='detail' class='detail' aria-label='Test case' hidden></aside>\
         </div>\
         <script type='application/json' id='map-data'>{data}</script>\
         <script>{js}</script>\
         <script>{switch_js}</script>\
         </body></html>",
        scheme = palette.initial_scheme(),
        vars = palette.css(),
        css = MAP_CSS,
        switch = crate::webtheme::SWITCH_HTML,
        data = script_json(&nodes, "[]"),
        js = MAP_JS,
        switch_js = crate::webtheme::SWITCH_JS,
    );
    std::fs::write(path, html).map_err(|e| e.to_string())
}
