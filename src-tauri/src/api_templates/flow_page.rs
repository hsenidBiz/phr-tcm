//! A flow on its own page, in the browser: the stages as large coloured
//! boxes left to right, joined by thick curved connectors that blend from
//! one stage's colour into the next - the API Templates tab's "View flow",
//! as the Test map is the review page's "View as Tree".
//!
//! A stage's colour says what it does: the one effect its templates share
//! (create, edit, delete), or the page's accent when no template performs
//! it yet or its templates disagree - the same rule the tab's arrows use.
//!
//! The browser lays the stages out - a column per depth, each centred -
//! so a long title wraps and its box grows instead of being cut short.
//! Where the boxes land is then known only to the browser, so a small
//! script (`web/flow-page.js`) draws the connectors between them.

use std::collections::HashMap;

use crate::api_templates::flow::{Flow, Stage};
use crate::api_templates::store::SavedTemplate;
use crate::api_templates::{ApiTemplate, Effect};
use crate::import_parser::esc;
use crate::webtheme::PagePalette;

/// A column's width, and the gaps between columns and between the boxes
/// in one - the page's CSS reads these.
pub const COL_W: u32 = 260;
pub const GAP_X: u32 = 96;
pub const GAP_Y: u32 = 24;

const FLOW_JS: &str = include_str!("../../web/flow-page.js");

/// What a stage's colour stands for. A stage whose templates agree is a
/// filled box in that effect's colour; `Open` is outlined in the accent
/// instead, so it never reads as a create stage in a theme whose accent is
/// green.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Create,
    Edit,
    Delete,
    /// No template performs it yet, or its templates disagree.
    Open,
}

impl Tone {
    /// The page palette variable the tone is drawn in.
    pub fn var(self) -> &'static str {
        match self {
            Tone::Create => "--success",
            Tone::Edit => "--warning",
            Tone::Delete => "--danger",
            Tone::Open => "--accent",
        }
    }

    fn class(self) -> &'static str {
        match self {
            Tone::Create => "create",
            Tone::Edit => "edit",
            Tone::Delete => "delete",
            Tone::Open => "open",
        }
    }
}

/// The one effect a stage's templates share, or `Open`.
pub fn tone_of(templates: &[&ApiTemplate]) -> Tone {
    let mut effects = templates.iter().map(|t| t.effect.clone());
    let Some(first) = effects.next() else {
        return Tone::Open;
    };
    if effects.any(|e| e != first) {
        return Tone::Open;
    }
    match first {
        Effect::Create => Tone::Create,
        Effect::Edit => Tone::Edit,
        Effect::Delete => Tone::Delete,
    }
}

/// The stages by column. A stage's column is the length of the longest
/// `requires` path from the creating stage; within a column, declaration
/// order. `requires` naming no stage is ignored and a loop is cut, so a
/// hand-edited flow still draws.
pub fn columns(flow: &Flow) -> Vec<Vec<&Stage>> {
    let by_id: HashMap<&str, &Stage> = flow.stages.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut cols: HashMap<&str, u32> = HashMap::new();
    for s in &flow.stages {
        col_of(s.id.as_str(), &by_id, &mut cols, &mut Vec::new());
    }
    let mut out: Vec<Vec<&Stage>> = Vec::new();
    for s in &flow.stages {
        let c = cols[s.id.as_str()] as usize;
        if out.len() <= c {
            out.resize(c + 1, Vec::new());
        }
        out[c].push(s);
    }
    out
}

fn col_of<'a>(
    id: &'a str,
    by_id: &HashMap<&'a str, &'a Stage>,
    cols: &mut HashMap<&'a str, u32>,
    visiting: &mut Vec<&'a str>,
) -> u32 {
    if let Some(c) = cols.get(id) {
        return *c;
    }
    if visiting.contains(&id) {
        return 0;
    }
    visiting.push(id);
    let c = by_id
        .get(id)
        .map(|s| {
            s.requires
                .iter()
                .filter(|r| by_id.contains_key(r.as_str()))
                .map(|r| col_of(by_id[r.as_str()].id.as_str(), by_id, cols, visiting) + 1)
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    visiting.pop();
    cols.insert(id, c);
    c
}

/// The templates that perform each stage of this flow, by title.
fn by_stage<'a>(flow: &Flow, templates: &'a [SavedTemplate]) -> HashMap<String, Vec<&'a ApiTemplate>> {
    let mut m: HashMap<String, Vec<&ApiTemplate>> = HashMap::new();
    for s in templates {
        if let Some(r) = &s.template.stage {
            if r.flow == flow.id {
                m.entry(r.id.clone()).or_default().push(&s.template);
            }
        }
    }
    for list in m.values_mut() {
        list.sort_by(|a, b| a.title.cmp(&b.title));
    }
    m
}

fn effect_word(e: &Effect) -> &'static str {
    match e {
        Effect::Create => "create",
        Effect::Edit => "edit",
        Effect::Delete => "delete",
    }
}

/// The whole page. Every title the assistant wrote is escaped.
pub fn page_html(flow: &Flow, templates: &[SavedTemplate], palette: &PagePalette) -> String {
    let on = by_stage(flow, templates);
    let cols = columns(flow);
    let col_of: HashMap<&str, usize> =
        cols.iter().enumerate().flat_map(|(c, list)| list.iter().map(move |s| (s.id.as_str(), c))).collect();
    let tone: HashMap<&str, Tone> = flow
        .stages
        .iter()
        .map(|s| (s.id.as_str(), tone_of(on.get(&s.id).map(Vec::as_slice).unwrap_or(&[]))))
        .collect();
    let title_of: HashMap<&str, &str> = flow.stages.iter().map(|s| (s.id.as_str(), s.title.as_str())).collect();

    // Connectors: one gradient per edge, from the stage it leaves to the
    // stage it reaches. Their shape and the gradient's run are filled in by
    // the page's script once the browser has placed the boxes.
    let mut defs = String::new();
    let mut paths = String::new();
    let mut n = 0;
    for s in &flow.stages {
        for r in &s.requires {
            let (Some(from), Some(_)) = (col_of.get(r.as_str()), col_of.get(s.id.as_str())) else {
                continue;
            };
            defs.push_str(&format!(
                "<linearGradient id='g{n}' gradientUnits='userSpaceOnUse' x1='0' y1='0' x2='1' y2='0'>\
                 <stop offset='0' style='stop-color:var({})'/><stop offset='1' style='stop-color:var({})'/></linearGradient>",
                tone[r.as_str()].var(),
                tone[s.id.as_str()].var(),
            ));
            paths.push_str(&format!(
                "<g class='edge' data-from='{}' data-to='{}' data-grad='g{n}' style='animation-delay:{:.2}s'>\
                 <path class='wire' d='' stroke='url(#g{n})'/><path class='spark' d=''/></g>",
                esc(r),
                esc(&s.id),
                *from as f32 * 0.25,
            ));
            n += 1;
        }
    }

    let mut boxes = String::new();
    for column in &cols {
        boxes.push_str("<div class='col'>");
        for s in column {
        let list = on.get(&s.id).map(Vec::as_slice).unwrap_or(&[]);
        let t = tone[s.id.as_str()];
        let under = if s.creates {
            "Creates the record".to_string()
        } else if s.optional {
            "Optional".to_string()
        } else if list.is_empty() {
            "Not built yet".to_string()
        } else if list.len() == 1 {
            "1 template".to_string()
        } else {
            format!("{} templates", list.len())
        };
        let rows = if list.is_empty() {
            "<li class='none'>No template yet</li>".to_string()
        } else {
            list.iter()
                .map(|tpl| {
                    format!(
                        "<li><span class='name'>{}</span><span class='fx'>{}</span></li>",
                        esc(&tpl.title),
                        effect_word(&tpl.effect)
                    )
                })
                .collect()
        };
        boxes.push_str(&format!(
            "<div class='stage {cls}{opt}' data-stage='{id}' style='--tone:var({var})'>\
             <h2>{title}</h2><p class='under'>{under}</p><ul>{rows}</ul></div>",
            cls = t.class(),
            opt = if s.optional { " optional" } else { "" },
            id = esc(&s.id),
            var = t.var(),
            title = esc(&s.title),
        ));
        }
        boxes.push_str("</div>");
    }

    // The same, in words, for a screen reader.
    let words: String = flow
        .stages
        .iter()
        .map(|s| {
            let requires: Vec<&str> = s.requires.iter().map(|r| *title_of.get(r.as_str()).unwrap_or(&r.as_str())).collect();
            let performers: Vec<&str> =
                on.get(&s.id).map(|l| l.iter().map(|t| t.title.as_str()).collect()).unwrap_or_default();
            format!(
                "<li>{}. Requires: {}. {}Templates: {}.</li>",
                esc(&s.title),
                if requires.is_empty() { "nothing".to_string() } else { esc(&requires.join(", ")) },
                if s.optional { "Optional. " } else { "" },
                if performers.is_empty() { "none yet".to_string() } else { esc(&performers.join(", ")) },
            )
        })
        .collect();

    format!(
        "<!DOCTYPE html>\n\
         <html lang=\"en\" data-scheme=\"{scheme}\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title} - flow</title><style>{vars}{css}</style></head><body>{switch}\
         <header class='bar'><h1>{title}</h1>\
         <p class='subtitle'>{module} \u{b7} Tracks {subject} \u{b7} {stages} stage{s}</p>\
         <ul class='key' aria-label='Colours'>\
         <li class='create'>Create</li><li class='edit'>Edit</li><li class='delete'>Delete</li>\
         <li class='open'>No template yet, or mixed</li></ul></header>\
         <main class='viewport'><div id='canvas' class='canvas'>\
         <svg aria-hidden='true'><defs>{defs}</defs>{paths}</svg>\
         {boxes}</div></main>\
         <ol class='sr-only' aria-label='Stages of {title}'>{words}</ol>\
         <script>{switch_js}</script><script>{flow_js}</script></body></html>",
        scheme = palette.initial_scheme(),
        vars = palette.css(),
        css = page_css(),
        flow_js = FLOW_JS,
        switch = crate::webtheme::SWITCH_HTML,
        switch_js = crate::webtheme::SWITCH_JS,
        title = esc(&flow.title),
        module = esc(&flow.module),
        subject = esc(&flow.subject.name),
        stages = flow.stages.len(),
        s = if flow.stages.len() == 1 { "" } else { "s" },
    )
}

/// Write the page into the temp folder, one file per flow (opening it again
/// rewrites it), and return its path.
pub fn write(flow: &Flow, templates: &[SavedTemplate], palette: &PagePalette) -> Result<std::path::PathBuf, String> {
    let path = std::env::temp_dir().join(format!("api-flow-{}-{}.html", flow.id, std::process::id()));
    std::fs::write(&path, page_html(flow, templates, palette)).map_err(|e| format!("the flow page could not be written: {e}"))?;
    Ok(path)
}

fn page_css() -> String {
    format!(
        ".canvas{{position:relative;display:flex;gap:{GAP_X}px;width:max-content;margin:0 auto}}\
         .col{{display:flex;flex-direction:column;justify-content:center;gap:{GAP_Y}px;width:{COL_W}px}}{PAGE_CSS}"
    )
}

const PAGE_CSS: &str = "
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:14px/1.45 system-ui,-apple-system,'Segoe UI',sans-serif}
.bar{padding:28px 40px 8px}
.bar h1{margin:0;font-size:30px;font-weight:800;letter-spacing:.02em;text-transform:uppercase}
.subtitle{margin:4px 0 14px;color:var(--muted)}
.key{display:flex;flex-wrap:wrap;gap:8px 18px;margin:0;padding:0;list-style:none;color:var(--muted);font-size:12px}
.key li{display:flex;align-items:center;gap:6px}
.key li::before{content:'';width:22px;height:6px;border-radius:3px;background:var(--k)}
.key .create{--k:var(--success)}.key .edit{--k:var(--warning)}.key .delete{--k:var(--danger)}
.key .open::before{background:transparent;border:2px solid var(--accent);height:10px;width:18px}
.viewport{overflow:auto;padding:28px 40px 48px}
.canvas svg{position:absolute;inset:0;overflow:visible;pointer-events:none}
.wire{fill:none;stroke-width:10;stroke-linecap:round;opacity:.9}
.spark{fill:none;stroke:var(--bg);stroke-width:2.5;stroke-linecap:round;stroke-dasharray:2 22;opacity:.5}
.stage{position:relative;z-index:1;display:flex;flex-direction:column;padding:12px 14px;border-radius:14px;color:#fff;
 background:linear-gradient(135deg,var(--tone),color-mix(in srgb,var(--tone) 72%,#000));
 box-shadow:0 10px 28px color-mix(in srgb,var(--tone) 35%,transparent),0 2px 6px color-mix(in srgb,#000 25%,transparent)}
.stage.open,.stage.optional{background:color-mix(in srgb,var(--tone) 10%,var(--surface));color:var(--text);border:2.5px solid var(--tone);box-shadow:none}
.stage.optional{border-style:dashed}
.stage h2{margin:0;font-size:14px;font-weight:800;line-height:1.3;letter-spacing:.06em;text-transform:uppercase;overflow-wrap:anywhere}
.under{margin:2px 0 8px;font-size:11px;font-weight:600;letter-spacing:.04em;text-transform:uppercase;opacity:.85}
.stage ul{margin:0;padding:0;list-style:none}
.stage li{display:flex;align-items:flex-start;gap:8px;padding:3px 0;font-size:12px;line-height:1.35}
.stage li .name{flex:1;min-width:0;overflow-wrap:anywhere}
.stage li .fx{flex:none;margin-top:1px;padding:1px 7px;border-radius:999px;font-size:10px;font-weight:700;text-transform:uppercase;background:color-mix(in srgb,#000 22%,transparent)}
.stage.open li .fx,.stage.optional li .fx{background:color-mix(in srgb,var(--tone) 18%,transparent);color:var(--text)}
.stage li.none{opacity:.8;font-style:italic}
.sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}
@media (prefers-reduced-motion:no-preference){
 .edge{animation:flow-breathe 2.4s ease-in-out infinite}
 .spark{animation:flow-run 1.6s linear infinite}
 @keyframes flow-breathe{0%,100%{opacity:.7}50%{opacity:1}}
 @keyframes flow-run{to{stroke-dashoffset:-24}}
}
@media print{.spark{display:none}}
";
