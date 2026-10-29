//! The render-geometry bridge — "edit through the renderer".
//!
//! Every draw-op the layout engine emits carries its source (`ObjectRef`: section, object
//! name, kind, and a per-placement instance id that groups one placed object's wrapped
//! lines and adornments). [`preview_hits`] projects that attribution into one hit box per
//! object instance per page, in printable-relative twips — the geometry a front end
//! overlays on the rendered PDF (pdf.js canvas or scaled absolute boxes) for
//! click-to-select and drag-to-edit. Edits themselves flow through the ordinary op
//! vocabulary, so undo/redo, validation, and diff all compose.
//!
//! Subreports: a child's ops keep their OWN section/object names on the parent page (the
//! merge only shifts geometry and remaps instance ids), so a click inside a rendered
//! subreport attributes to that subreport's object — resolve it against the subreport IRs
//! and route the edit through [`crate::subedit`] (`EditOp::InSubreport` / the plan
//! `subreport` key). [`resolve_scope`] does that resolution: unique match wins, ambiguity
//! (the same object name in several scopes) is an error naming them.

use std::collections::BTreeMap;

use rpt_reader::Rpt;
use rpt_pages::PagedDocument;
use serde::{Deserialize, Serialize};

use crate::ir::{build_ir, Ir};

/// One selectable/editable box on a rendered page: an object instance's true on-page
/// rectangle (wrapped lines and border/fill unioned), printable-relative twips.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitBox {
    /// The section the object was placed in.
    pub section: String,
    /// The object's designer name (`None` for unnamed synthetic ops).
    pub object: Option<String>,
    /// The object taxonomy: `Text`, `Field`, `Line`, `Box`, `Picture`, `Chart`, …
    pub kind: String,
    /// The placement id (stable within a render; groups one object instance's ops).
    pub instance: Option<u32>,
    /// Box left, printable-relative twips.
    pub x: i32,
    /// Box top, printable-relative twips.
    pub y: i32,
    /// Box width in twips.
    pub w: i32,
    /// Box height in twips (the object's REAL rendered height — can-grow and wrapped lines
    /// included, unlike the stored design bounds).
    pub h: i32,
    /// The rendered text, for text-bearing ops (wrapped lines joined with a newline).
    pub text: Option<String>,
}

/// One rendered page's selectable surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageHits {
    /// 1-based page number.
    pub number: u32,
    /// Paper width in twips.
    pub width_twips: i32,
    /// Paper height in twips.
    pub height_twips: i32,
    /// Printable-area origin x (draw-op coordinates are printable-relative; add this for
    /// paper coordinates).
    pub origin_x: i32,
    /// Printable-area origin y.
    pub origin_y: i32,
    /// The page's object boxes in paint order.
    pub objects: Vec<HitBox>,
}

/// [`preview_hits`]'s result: every page's selectable surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitsOutput {
    /// The pages in document order.
    pub pages: Vec<PageHits>,
}

/// Render the report (the saved-data preview path) and project its pages into hit boxes.
///
/// # Errors
///
/// Only decode/render-writer class failures; fidelity issues arrive in the preview's
/// diagnostics, not here (this surface is geometry, not judgement).
pub fn preview_hits(rpt: &Rpt) -> Result<HitsOutput, String> {
    let doc = rpt_render::render(rpt.report());
    Ok(hits_of(&doc))
}

/// The projection itself, over an already-rendered document.
pub fn hits_of(doc: &PagedDocument) -> HitsOutput {
    let mut pages = Vec::new();
    for p in &doc.pages {
        // Group ops into one box per instance id (nameless instances get their own box).
        let mut groups: BTreeMap<(Option<u32>, String, Option<String>, String), Group> = BTreeMap::new();
        let mut order: Vec<(Option<u32>, String, Option<String>, String)> = Vec::new();
        for op in &p.ops {
            let Some(src) = op.source() else { continue };
            let (bounds, text) = match op {
                rpt_pages::DrawOp::Text(t) => (Some(t.bounds), Some(t.text.clone())),
                rpt_pages::DrawOp::Rect(r) => (Some(r.bounds), None),
                rpt_pages::DrawOp::Ellipse(e) => (Some(e.bounds), None),
                rpt_pages::DrawOp::Image(i) => (Some(i.bounds), None),
                rpt_pages::DrawOp::Line(l) => {
                    let (x0, x1) = (l.from.x.0.min(l.to.x.0), l.from.x.0.max(l.to.x.0));
                    let (y0, y1) = (l.from.y.0.min(l.to.y.0), l.from.y.0.max(l.to.y.0));
                    (
                        Some(rpt_reader::model::Rect {
                            left: rpt_reader::model::Twips(x0),
                            top: rpt_reader::model::Twips(y0),
                            width: rpt_reader::model::Twips(x1 - x0),
                            height: rpt_reader::model::Twips(y1 - y0),
                        }),
                        None,
                    )
                }
                rpt_pages::DrawOp::Polygon(g) => {
                    let mut it = g.points.iter();
                    let Some(first) = it.next() else { continue };
                    let (mut x0, mut x1) = (first.x.0, first.x.0);
                    let (mut y0, mut y1) = (first.y.0, first.y.0);
                    for pt in g.points.iter().skip(1) {
                        x0 = x0.min(pt.x.0);
                        x1 = x1.max(pt.x.0);
                        y0 = y0.min(pt.y.0);
                        y1 = y1.max(pt.y.0);
                    }
                    (
                        Some(rpt_reader::model::Rect {
                            left: rpt_reader::model::Twips(x0),
                            top: rpt_reader::model::Twips(y0),
                            width: rpt_reader::model::Twips(x1 - x0),
                            height: rpt_reader::model::Twips(y1 - y0),
                        }),
                        None,
                    )
                }
            };
            let Some(b) = bounds else { continue };
            let key = (
                src.instance,
                src.section.clone(),
                src.object_name.clone(),
                format!("{:?}", src.kind),
            );
            let g = groups.entry(key.clone()).or_insert_with(|| {
                order.push(key.clone());
                Group {
                    section: src.section.clone(),
                    object: src.object_name.clone(),
                    kind: format!("{:?}", src.kind),
                    instance: src.instance,
                    x0: b.left.0,
                    y0: b.top.0,
                    x1: b.left.0 + b.width.0,
                    y1: b.top.0 + b.height.0,
                    texts: Vec::new(),
                }
            });
            g.x0 = g.x0.min(b.left.0);
            g.y0 = g.y0.min(b.top.0);
            g.x1 = g.x1.max(b.left.0 + b.width.0);
            g.y1 = g.y1.max(b.top.0 + b.height.0);
            if let Some(t) = text {
                g.texts.push(t);
            }
        }
        let objects = order
            .into_iter()
            .filter_map(|k| groups.remove(&k))
            .map(|g| HitBox {
                section: g.section,
                object: g.object,
                kind: g.kind,
                instance: g.instance,
                x: g.x0,
                y: g.y0,
                w: g.x1 - g.x0,
                h: g.y1 - g.y0,
                text: if g.texts.is_empty() { None } else { Some(g.texts.join("\n")) },
            })
            .collect();
        pages.push(PageHits {
            number: p.number,
            width_twips: p.size.width.0,
            height_twips: p.size.height.0,
            origin_x: p.origin.x.0,
            origin_y: p.origin.y.0,
            objects,
        });
    }
    HitsOutput { pages }
}

struct Group {
    section: String,
    object: Option<String>,
    kind: String,
    instance: Option<u32>,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    texts: Vec<String>,
}

/// The topmost box on `page` (1-based) containing the printable-relative point, in paint
/// order with a hair of tolerance for zero-thickness lines. `None` when the point hits
/// nothing attributable.
pub fn hit_test(hits: &HitsOutput, page: usize, x: i32, y: i32) -> Option<HitBox> {
    const TOLERANCE: i32 = 20; // twips; a hairline needs charity to be clickable
    let p = hits.pages.iter().find(|p| p.number as usize == page)?;
    p.objects
        .iter()
        .rev()
        .find(|b| {
            x >= b.x - TOLERANCE
                && x <= b.x + b.w + TOLERANCE
                && y >= b.y - TOLERANCE
                && y <= b.y + b.h + TOLERANCE
        })
        .cloned()
}

/// Which editing scope an object name belongs to: the main report's IR, or a subreport's
/// (its name). A name unique across scopes resolves to its scope; the same name in several
/// scopes is an error naming them (disambiguate by section at the call site).
pub fn resolve_scope(rpt: &Rpt, object: &str) -> Result<Scope, String> {
    let main = build_ir(rpt, &crate::locate::locate(rpt), "");
    let mut found: Vec<Scope> = Vec::new();
    if main.objects.iter().any(|o| o.name.eq_ignore_ascii_case(object)) {
        found.push(Scope::Main);
    }
    for sub in &rpt.report().subreports {
        if let Ok(ir) = crate::subedit::subreport_ir(rpt, &sub.name) {
            if ir.objects.iter().any(|o| o.name.eq_ignore_ascii_case(object)) {
                found.push(Scope::Subreport(sub.name.clone()));
            }
        }
    }
    match found.len() {
        1 => Ok(found.pop().unwrap()),
        0 => Err(format!("`{object}` does not resolve in the main report or any subreport")),
        _ => {
            let names: Vec<String> = found.iter().map(|s| s.to_string()).collect();
            Err(format!("`{object}` is ambiguous across scopes: {names:?}"))
        }
    }
}

/// An editing scope: the main report or one embedded subreport.
#[derive(Debug, Clone)]
pub enum Scope {
    /// The main report.
    Main,
    /// The named embedded subreport.
    Subreport(String),
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Scope::Main => write!(f, "main"),
            Scope::Subreport(n) => write!(f, "subreport {n}"),
        }
    }
}

/// Group boxes per OBJECT across a page (an object drawn in several bands yields several
/// boxes; this coalesces them for selection highlighting).
pub fn boxes_of_object(hits: &HitsOutput, object: &str) -> Vec<(u32, HitBox)> {
    let mut out = Vec::new();
    for p in &hits.pages {
        for b in &p.objects {
            if b.object.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(object)) {
                out.push((p.number, b.clone()));
            }
        }
    }
    out
}

/// Keep the IR import honest for callers that thread projections.
#[allow(unused_imports)]
use crate::ir::Ir as _IrDoc;
#[allow(unused)]
fn _ir_doc(_: &Ir) {}
