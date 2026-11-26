//! A flow on its own page, in the browser: the stages as plain boxes left
//! to right, joined by thin curved connectors that end in an arrowhead -
//! the API Templates tab's "View flow", as the Test map is the review
//! page's "View as Tree".
//!
//! A stage's tone says what it does: the one effect its templates share
//! (create, edit, delete), or the page's accent when no template performs
//! it yet or its templates disagree - the same rule the tab's flow map
//! uses. The tone is a 4px left edge and a small tag; the box itself stays
//! neutral, so every line of text reads on the page's own surface.
//!
//! The browser lays the stages out - a column per depth, each centred -
//! so a long title wraps and its box grows instead of being cut short.
//! Where the boxes land is then known only to the browser, so a small
//! script (`web/flow-page.js`) draws the connectors between them.

use std::collections::HashMap;

use crate::api_templates::flow::{Flow, Stage};
use crate::api_templates::here::Here;
use crate::api_templates::store::SavedTemplate;
use crate::api_templates::{ApiTemplate, Effect};
use crate::html_escape::{esc, esc_attr};
use crate::webtheme::PagePalette;

/// A column's width, and the gaps between columns and between the boxes
/// in one - the page's CSS reads these.
pub const COL_W: u32 = 260;
pub const GAP_X: u32 = 96;
pub const GAP_Y: u32 = 24;

const FLOW_JS: &str = include_str!("../../web/flow-page.js");

/// What a stage's tone stands for. A stage whose templates agree has a left
/// edge and a tag in that effect's colour; `Open` is outlined in the accent
/// instead, so it never reads as a create stage in a theme whose accent is
/// green.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Create,
    Edit,
    Delete,
    /// The stage's templates only look something up: muted, since nothing
    /// changes.
    Read,
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
            Tone::Read => "--muted",
            Tone::Open => "--accent",
        }
    }

    /// The word on the stage's tag; `Open` has none.
    fn tag(self) -> Option<&'static str> {
        match self {
            Tone::Create => Some("Create"),
            Tone::Edit => Some("Edit"),
            Tone::Delete => Some("Delete"),
            Tone::Read => Some("Read"),
            Tone::Open => None,
        }
    }

    fn class(self) -> &'static str {
        match self {
            Tone::Create => "create",
            Tone::Edit => "edit",
            Tone::Delete => "delete",
            Tone::Read => "read",
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
        Effect::Read => Tone::Read,
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
fn by_stage<'a>(flow: &Flow, templates: &'a [SavedTemplate]) -> HashMap<String, Vec<&'a SavedTemplate>> {
    let mut m: HashMap<String, Vec<&SavedTemplate>> = HashMap::new();
    for s in templates {
        if let Some(r) = &s.template.stage {
            if r.flow == flow.id {
                m.entry(r.id.clone()).or_default().push(s);
            }
        }
    }
    for list in m.values_mut() {
        list.sort_by(|a, b| a.template.title.cmp(&b.template.title));
    }
    m
}

/// The mark a template carries on this environment, as the app's own
/// screens word it; a confirmed one carries none.
fn here_mark(here: &Here) -> Option<&'static str> {
    match here {
        Here::Confirmed { .. } => None,
        Here::Unconfirmed => Some("unconfirmed here"),
        Here::Failed { .. } => Some("failed here"),
    }
}

fn here_hint(here: &Here) -> String {
    match here {
        Here::Confirmed { .. } => String::new(),
        Here::Unconfirmed => "Not confirmed on this environment yet. Ask the assistant to try it here before relying on it.".into(),
        Here::Failed { failure } => {
            let mut s = "Failed on this environment".to_string();
            if let Some(step) = &failure.step {
                s.push_str(&format!(" at {step}"));
            }
            if let Some(detail) = &failure.detail {
                s.push_str(&format!(": {detail}"));
            }
            s.push_str(". Ask the assistant to make it work here.");
            s
        }
    }
}

/// A failure's reason as one short line: the runner's own `reason`, or for an
/// older record the part of its `detail` after "failed at <step>: ", and in
/// either case nothing from the application's response excerpt on. Cut to
/// 120 characters; the `title` keeps the full sentence.
fn short_reason(f: &crate::api_templates::here::HereFailure) -> Option<String> {
    let raw = f.reason.as_deref().or(f.detail.as_deref())?;
    let mut text = raw.split(crate::api_templates::runner::RESPONSE_MARK).next().unwrap_or("");
    if let Some(at) = text.find("failed at ") {
        // "<captured>; failed at <name> (<handler>): <detail>" - keep the detail.
        text = text[at..].split_once(": ").map_or("", |(_, rest)| rest);
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    Some(if text.chars().count() > 120 {
        format!("{}\u{2026}", text.chars().take(119).collect::<String>().trim_end())
    } else {
        text
    })
}

/// The visible second line under a template that failed here: "failed at
/// <step>: <short reason>". None when the failure names neither.
fn failure_line(here: &Here) -> Option<String> {
    let Here::Failed { failure } = here else { return None };
    let at = failure.step.as_ref().map(|step| {
        if !step.is_empty() && step.chars().all(|c| c.is_ascii_digit()) {
            format!("step {step}")
        } else {
            step.clone()
        }
    });
    match (at, short_reason(failure)) {
        (Some(a), Some(d)) => Some(format!("failed at {a}: {d}")),
        (Some(a), None) => Some(format!("failed at {a}")),
        (None, Some(d)) => Some(format!("failed: {d}")),
        (None, None) => None,
    }
}

fn effect_word(e: &Effect) -> &'static str {
    match e {
        Effect::Create => "create",
        Effect::Edit => "edit",
        Effect::Delete => "delete",
        Effect::Read => "read",
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
        .map(|s| {
            let templates: Vec<&ApiTemplate> =
                on.get(&s.id).map(|l| l.iter().map(|t| &t.template).collect()).unwrap_or_default();
            (s.id.as_str(), tone_of(&templates))
        })
        .collect();
    let title_of: HashMap<&str, &str> = flow.stages.iter().map(|s| (s.id.as_str(), s.title.as_str())).collect();

    // Connectors: one thin muted curve per edge, ending in an arrowhead at
    // the stage it reaches. Their shape is filled in by the page's script
    // once the browser has placed the boxes.
    let mut paths = String::new();
    for s in &flow.stages {
        for r in &s.requires {
            if !col_of.contains_key(r.as_str()) || !col_of.contains_key(s.id.as_str()) {
                continue;
            }
            paths.push_str(&format!(
                "<g class='edge' data-from='{}' data-to='{}'><path class='wire' d='' marker-end='url(#arrow)'/></g>",
                esc(r),
                esc(&s.id),
            ));
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
                .map(|saved| {
                    let tpl = &saved.template;
                    let mark = match here_mark(&saved.here) {
                        Some(word) => format!("<span class='fx' title='{}'>{word}</span>", esc_attr(&here_hint(&saved.here))),
                        None => String::new(),
                    };
                    // The reason is on the page, not only in the hover.
                    let why = match failure_line(&saved.here) {
                        Some(line) => {
                            format!("<span class='why' title='{}'>{}</span>", esc_attr(&here_hint(&saved.here)), esc(&line))
                        }
                        None => String::new(),
                    };
                    format!(
                        "<li><span class='name'>{}</span>{mark}<span class='fx'>{}</span>{why}</li>",
                        esc(&tpl.title),
                        effect_word(&tpl.effect)
                    )
                })
                .collect()
        };
        boxes.push_str(&format!(
            "<div class='stage {cls}{opt}' data-stage='{id}' style='--tone:var({var})'>\
             <div class='head'><h2>{title}</h2>{tag}</div><p class='under'>{under}</p><ul>{rows}</ul></div>",
            tag = t.tag().map(|w| format!("<span class='tag'>{w}</span>")).unwrap_or_default(),
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
            let performers: Vec<String> = on
                .get(&s.id)
                .map(|l| {
                    l.iter()
                        .map(|t| match (here_mark(&t.here), failure_line(&t.here)) {
                            (None, _) => t.template.title.clone(),
                            (Some(word), None) => format!("{} ({word})", t.template.title),
                            (Some(word), Some(line)) => format!("{} ({word}, {line})", t.template.title),
                        })
                        .collect()
                })
                .unwrap_or_default();
            format!(
                "<li>{}. Requires: {}. {}Templates: {}.</li>",
                esc(&s.title),
                if requires.is_empty() { "nothing".to_string() } else { esc(&requires.join(", ")) },
                if s.optional { "Optional. " } else { "" },
                if performers.is_empty() { "none yet".to_string() } else { esc(&performers.join(", ")) },
            )
        })
        .collect();

    // The legend lists only what this flow uses.
    let mut legend = String::new();
    for t in [Tone::Create, Tone::Edit, Tone::Delete, Tone::Read, Tone::Open] {
        if flow.stages.iter().any(|s| tone[s.id.as_str()] == t) {
            let word = match t {
                Tone::Create => "Create",
                Tone::Edit => "Edit",
                Tone::Delete => "Delete",
                Tone::Read => "Read",
                Tone::Open => "Open: no template yet, or mixed",
            };
            legend.push_str(&format!("<li class='{}'>{word}</li>", t.class()));
        }
    }
    if flow.stages.iter().any(|s| s.optional) {
        legend.push_str("<li class='optional'>Optional stage</li>");
    }

    // No stages, nothing to name: no footer either.
    let legend_footer = if legend.is_empty() {
        String::new()
    } else {
        format!("<footer class='legend'><ul class='key' aria-label='Legend'>{legend}</ul></footer>")
    };

    format!(
        "<!DOCTYPE html>\n\
         <html lang=\"en\" data-scheme=\"{scheme}\"><head><meta charset=\"utf-8\">{restore}\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title} - flow</title><style>{vars}{css}</style></head><body>{switch}\
         <header class='bar'><h1>{title}</h1>\
         <p class='subtitle'>{module} \u{b7} Tracks {subject} \u{b7} {stages} stage{s}</p></header>\
         <main class='viewport'><div id='canvas' class='canvas'>\
         <svg aria-hidden='true'><defs>\
         <marker id='arrow' viewBox='0 0 6 6' refX='6' refY='3' markerWidth='6' markerHeight='6' markerUnits='userSpaceOnUse' orient='auto'>\
         <path d='M0 0 L6 3 L0 6 z'/></marker></defs>{paths}</svg>\
         {boxes}</div></main>\
         {legend_footer}\
         <ol class='sr-only' aria-label='Stages of {title}'>{words}</ol>\
         <script>{switch_js}</script><script>{flow_js}</script></body></html>",
        scheme = palette.initial_scheme(),
        restore = crate::webtheme::head_script(),
        vars = palette.css(),
        css = page_css(),
        flow_js = FLOW_JS,
        legend_footer = legend_footer,
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
body{margin:0;background:var(--bg);color:var(--text);font:14px/1.5 system-ui,-apple-system,'Segoe UI',sans-serif}
.bar{padding:24px 40px 4px}
.bar h1{margin:0;font-size:24px;font-weight:600;line-height:1.3}
.subtitle{margin:4px 0 12px;color:var(--muted)}
.legend{padding:12px 40px 24px;border-top:1px solid var(--border)}
.key{display:flex;flex-wrap:wrap;gap:8px 24px;margin:0;padding:0;list-style:none;color:var(--muted);font-size:12px}
.key li{display:flex;align-items:center;gap:8px}
.key li::before{content:'';width:4px;height:16px;border-radius:2px;background:var(--k)}
.key .create{--k:var(--success)}.key .edit{--k:var(--warning)}.key .delete{--k:var(--danger)}.key .read{--k:var(--muted)}
.key .open::before{width:16px;height:16px;border-radius:4px;background:transparent;border:1px solid var(--accent)}
.key .optional::before{width:16px;height:16px;border-radius:4px;background:transparent;border:1px dashed var(--muted)}
.viewport{overflow:auto;padding:24px 40px 40px}
.canvas svg{position:absolute;inset:0;overflow:visible;pointer-events:none}
.wire{fill:none;stroke:var(--muted);stroke-width:1.5}
#arrow path{fill:var(--muted)}
.stage{position:relative;z-index:1;display:flex;flex-direction:column;padding:12px 16px;border-radius:8px;
 background:var(--surface);color:var(--text);border:1px solid var(--border);border-left:4px solid var(--tone)}
.stage.open{border:1px solid var(--accent)}
.stage.optional{border-style:dashed;border-left-style:solid}
.stage.open.optional{border-style:dashed}
.head{display:flex;align-items:flex-start;gap:8px}
.stage h2{flex:1;min-width:0;margin:0;font-size:13px;font-weight:600;line-height:20px;overflow-wrap:anywhere}
.tag{flex:none;margin-top:2px;padding:0 8px;border-radius:4px;font-size:10px;font-weight:600;line-height:16px;text-transform:uppercase;
 color:var(--text);background:color-mix(in srgb,var(--tone) 14%,var(--surface))}
.under{margin:0 0 8px;font-size:12px;color:var(--muted)}
.stage ul{margin:0;padding:0;list-style:none}
.stage li{display:flex;flex-wrap:wrap;align-items:flex-start;gap:0 8px;padding:4px 0;font-size:12px;line-height:16px}
.stage li .name{flex:1;min-width:0;overflow-wrap:anywhere}
.stage li .fx{flex:none;padding:0 8px;border-radius:4px;font-size:10px;font-weight:600;line-height:16px;text-transform:uppercase;color:var(--text);background:var(--surface-2)}
.stage li .why{flex-basis:100%;color:var(--muted);overflow-wrap:anywhere}
.stage li.none{color:var(--muted);font-style:italic}
.sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}
";
