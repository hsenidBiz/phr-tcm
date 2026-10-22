//! The typed edit vocabulary: the v1 ops plus the full VibeyReports layout-operation set,
//! resolved per-op against a freshly located report and applied through the verified write
//! path (`patch_record_field`) or the structural writer (`structure`).
//!
//! Every field patch is verified (record round-trip + read-back). Structural ops (add*/remove*)
//! clone donor runs, re-mask, splice with ancestor length fixup, fix the section's
//! `object_count`, then patch the clone's own fields through the same verified writer and
//! re-open to confirm the model shows the new object.
//!
//! Because adds/removes change record counts, addresses are re-derived per op: each op locates
//! the report fresh. Object selectors are resolved by name (case-insensitive, like the
//! designer), IR id, or flat index against a fresh snapshot.

use serde::Deserialize;
use rpt_reader::fields::{FieldEdit, FieldValue};
use rpt_reader::{model, EditPolicy, Rpt};

use crate::locate::{Located, RecAddr};
use crate::structure;

/// Select an object by IR id, designer name, or flat index (any one of the three).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ObjSel {
    /// IR id from the snapshot (`"o12"`).
    pub id: Option<String>,
    /// The designer name (case-insensitive; must resolve uniquely).
    pub name: Option<String>,
    /// Flat index into the IR object list.
    pub index: Option<usize>,
}

/// Select a section by area + section name (either may alone identify it if unique).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SecSel {
    /// Area name (e.g. `ReportHeaderArea1`).
    pub area: Option<String>,
    /// Section name (e.g. `ReportHeaderSection1`).
    pub section: String,
}

/// One edit operation. All lengths are twips (1440 = 1 inch); font size in points; colors as
/// `#RRGGBB`. The vocabulary is a superset of the v1 set and the VibeyReports layout actions.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    // --- v1 ops (unchanged) ---
    SetText { object: ObjSel, text: String },
    SetObjectName { object: ObjSel, name: String },
    SetBounds { object: ObjSel, left: Option<i64>, top: Option<i64>, width: Option<i64>, height: Option<i64> },
    SetFont {
        object: ObjSel,
        name: Option<String>,
        size_pt: Option<f64>,
        bold: Option<bool>,
        italic: Option<bool>,
        underline: Option<bool>,
        strikeout: Option<bool>,
        weight: Option<i64>,
    },
    SetTextColor { object: ObjSel, r: u8, g: u8, b: u8 },
    SetSectionHeight { section: SecSel, height: i64 },
    SetMargins { left: Option<i64>, right: Option<i64>, top: Option<i64>, bottom: Option<i64> },
    SetOrientation { orientation: String },
    SetPaperSize { paper: u16 },

    // --- VibeyReports layout operations ---
    /// `setAlignment` — Left, Right, Centre/Center, Justified.
    SetAlignment { object: ObjSel, alignment: String },
    /// `setFillColor` — a Box's fill colour.
    SetFillColor { object: ObjSel, color: String },
    /// `setLineColor` — a Line's or Box's outline colour.
    SetLineColor { object: ObjSel, color: String },
    /// `setSectionBackground` — a section's background colour.
    SetSectionBackground { section: SecSel, color: String },
    /// `setSectionBreak` — page-break flags; at least one required.
    SetSectionBreak { section: SecSel, new_page_before: Option<bool>, new_page_after: Option<bool> },
    /// `setCanGrow` — let the object grow vertically to fit its content.
    SetCanGrow { object: ObjSel, can_grow: bool },
    /// `setSuppress` — hide an object (target) or a section (section); exactly one host.
    SetSuppress {
        object: Option<ObjSel>,
        section: Option<SecSel>,
        suppress: bool,
        #[serde(default)] suppress_if_blank: Option<bool>,
    },
    /// `setBorder` — per-side line styles (none/single/double/dashed/dotted) and colour.
    SetBorder {
        object: ObjSel,
        #[serde(default)] left: Option<String>,
        #[serde(default)] right: Option<String>,
        #[serde(default)] top: Option<String>,
        #[serde(default)] bottom: Option<String>,
        #[serde(default)] color: Option<String>,
    },
    /// `setNumberFormat` — a Field's numeric display; the system-default gate is cleared.
    SetNumberFormat {
        object: ObjSel,
        decimal_places: Option<i64>,
        thousands_separator: Option<bool>,
        suppress_if_zero: Option<bool>,
    },
    /// `removeObject` — delete the object (and its records) outright.
    RemoveObject { object: ObjSel },
    /// `addText` — place a literal text object.
    AddText {
        section: SecSel,
        name: String,
        left: i64, top: i64, width: i64, height: i64,
        text: String,
    },
    /// `addLine` — place a straight line (single, 0.75pt).
    AddLine { section: SecSel, name: String, left: i64, top: i64, width: i64, height: i64 },
    /// `addBox` — place a rectangle.
    AddBox { section: SecSel, name: String, left: i64, top: i64, width: i64, height: i64 },
    /// `addField` — place a field the report already knows (`{alias.field}` formula form).
    AddField {
        section: SecSel,
        name: String,
        field_ref: String,
        left: i64, top: i64, width: i64, height: i64,
    },
    /// `addSpecialField` — pageNumber, pageNOfM, printDate, printTime, reportTitle.
    AddSpecialField {
        section: SecSel,
        name: String,
        special_type: String,
        left: i64, top: i64, width: i64, height: i64,
    },
    /// `addGroup` — group on a field (appends as the innermost group).
    AddGroup { field_ref: String, #[serde(default)] direction: Option<String> },
    /// `addSort` — sort records on a field.
    AddSort { field_ref: String, direction: String },
    /// `addRunningTotal` — synthesize a running-total field definition (field-based conditions).
    AddRunningTotal {
        name: String,
        summarize: String,
        operation: String,
        #[serde(default)] evaluate_field: Option<String>,
        #[serde(default)] reset_field: Option<String>,
    },
    /// `setFormula` — replace a named formula's source text (refs re-derived from the source).
    SetFormula { formula: String, source: String },
    /// `setRecordSelection` — replace the record-selection formula's source text.
    SetRecordSelection { source: String },
    /// `addSubreport` — import an existing .rpt as a subreport object.
    AddSubreport {
        section: SecSel,
        name: String,
        report_path: String,
        left: i64, top: i64, width: i64, height: i64,
    },
    /// `setSubreportLink` — link a main-report field to a subreport parameter.
    SetSubreportLink {
        target: String,
        main_report_field: String,
        subreport_field: String,
        linked_parameter: String,
    },
    /// `removeTable` — delete a data-source table (offline; see docs).
    RemoveTable { alias: String },
    /// `addTable` — add a database object under a cloned connection (offline; see docs).
    AddTable { target: String, new_name: String, table_name: String },
    /// `setTableLocation` — repoint a table at another object on its own connection.
    SetTableLocation { target: String, table_name: String },

    // --- Tier 0 extensions (2026-10-15 extraction batch) ---
    /// `setTooltip` — the object's hover text (empty string clears).
    SetTooltip { object: ObjSel, text: String },
    /// `setHyperlink` — kind vocabulary: website/email/html/none/reportPart/anotherObject.
    SetHyperlink { object: ObjSel, kind: String, #[serde(default)] text: Option<String> },
    /// `setCssClass` — the object's CSS class (optional field; refused when absent).
    SetCssClass { object: ObjSel, css_class: String },
    /// `setZOrder` — `front` (top) or `back` (bottom); record order is the layering.
    SetZOrder { object: ObjSel, z: String },
    /// `addPicture` — place an image file as a static picture via a fresh `Embedding N`.
    AddPicture {
        section: SecSel,
        name: String,
        image_path: String,
        left: i64, top: i64, width: i64, height: i64,
    },
    /// `addGuideline` — a report-wide guide at a twip position; axis mapping is the
    /// documented first-list-horizontal assumption.
    AddGuideline { axis: String, position: i64 },
    /// `addSection` — insert a fresh empty section into an area (donor-clone of a sibling's
    /// band bracket).
    AddSection {
        /// The area to grow.
        area: String,
        /// Insert after this section (by name); omitted = last.
        #[serde(default)] after: Option<String>,
        /// The new section's name; default = the engine's global `SectionN` counter + 1.
        #[serde(default)] name: Option<String>,
        /// Height in twips; default = the donor's.
        #[serde(default)] height: Option<i64>,
    },
    /// `removeSection` — delete an empty section (fixes drawing end-section indices).
    RemoveSection {
        /// The containing area (optional when the section name is unique).
        #[serde(default)] area: Option<String>,
        /// The section to remove.
        section: String,
    },
    /// `copyObject` — clone a Text/Line/Box object from another report's file.
    CopyObject {
        from_report: String,
        object: ObjSel,
        section: SecSel,
        #[serde(default)] new_name: Option<String>,
    },
}

/// The result of applying a plan, mirroring VibeyReports' `ApplyResult`.
#[derive(Debug, Default, Clone)]
pub struct ApplyOutcome {
    /// How many operations were applied.
    pub operations_applied: usize,
    /// Canonical names of objects removed by `removeObject`.
    pub removed_objects: Vec<String>,
    /// Aliases removed by `removeTable`.
    pub removed_tables: Vec<String>,
}

/// Apply a batch of edits to the report at `path`, returning the new `.rpt` file bytes.
///
/// # Errors
///
/// A human-readable message naming the failing op index and action.
pub fn apply_edits(path: &str, ops: &[EditOp]) -> Result<Vec<u8>, String> {
    let rpt = crate::open(path)?;
    let mut outcome = ApplyOutcome::default();
    apply_edits_on(&rpt, ops, &mut outcome).map_err(|e| e.to_string())
}

/// Apply a batch to an opened report; each op re-locates fresh (adds/removes move addresses).
pub fn apply_edits_on(rpt: &Rpt, ops: &[EditOp], outcome: &mut ApplyOutcome) -> Result<Vec<u8>, OpError> {
    let mut current = rpt.reencode().map_err(|e| OpError(0, format!("reencode failed: {e}")))?;
    for (n, op) in ops.iter().enumerate() {
        let open = crate::open_bytes(&current).map_err(|e| OpError(n, e))?;
        current = apply_one(&open, op, outcome).map_err(|e| OpError(n, e))?;
        outcome.operations_applied += 1;
    }
    Ok(current)
}

/// An op failure carrying its index, so a batch abort names the culprit.
#[derive(Debug)]
pub struct OpError(pub usize, pub String);

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "op #{}: {}", self.0, self.1)
    }
}

/* ---------------- value mappings ---------------- */

/// `#RRGGBB` → (r, g, b).
fn parse_hex_color(s: &str) -> Result<(u8, u8, u8), String> {
    let hex = s.strip_prefix('#').ok_or_else(|| format!("colour `{s}` must start with `#`"))?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("colour `{s}` is not #RRGGBB"));
    }
    let v = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
    Ok((((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8))
}

/// The COLORREF word the records store: big-endian `[type, blue, green, red]`, type `0`.
fn colorref_word(r: u8, g: u8, b: u8) -> i64 {
    (i64::from(b) << 16) | (i64::from(g) << 8) | i64::from(r)
}

/// Alignment vocabulary → the `Alignment` ordinal the records store.
fn alignment_code(s: &str) -> Result<i64, String> {
    match s.to_ascii_lowercase().as_str() {
        "default" => Ok(0),
        "left" => Ok(1),
        "centre" | "center" => Ok(2),
        "right" => Ok(3),
        "justified" => Ok(4),
        other => Err(format!("alignment must be Left/Right/Centre/Justified, not `{other}`")),
    }
}

/// Border-style vocabulary → `LineStyle` ordinal.
fn line_style_code(s: &str) -> Result<i64, String> {
    match s.to_ascii_lowercase().as_str() {
        "none" => Ok(0),
        "single" => Ok(1),
        "double" => Ok(2),
        "dashed" => Ok(3),
        "dotted" => Ok(4),
        other => Err(format!("border style must be none/single/double/dashed/dotted, not `{other}`")),
    }
}

/// The engine's special-field table: (CrSpecialFieldTypeEnum code, display string). The two
/// Vibey kinds whose file codes are undocumented even upstream are absent and refused.
/// The engine's 18 special-field canonical names (spaceless FormulaForms), validator-facing.
pub fn special_names() -> Vec<String> {
    [
        "PrintDate", "PrintTime", "ModificationDate", "ModificationTime", "DataDate", "DataTime",
        "ReportTitle", "ReportComments", "RecordSelectionFormula", "GroupSelectionFormula",
        "FilePathAndName", "FileAuthor", "FileCreationDate", "RecordNumber", "GroupNumber",
        "TotalPageCount", "PageNofM", "PageNumber", "HPageNumber", "CurrentUserName",
        "CurrentUserID", "CurrentUserTimeZone", "PrintTimeZone", "DataTimeZone", "ContentLocale",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

const SPECIAL_POOL: u32 = 3;
/// The engine's special-field wire table (pool 3), VERIFIED 2026-10-16 against the engine's
/// own special-field table: a 26-entry wire-to-SDK mapping (the published
/// `CRSpecialVarType` ordinals). The earlier
/// 0x0C–0x0F block was cyclically shifted by one and is now the engine's mapping. Wires 18–24
/// exist per the table (25 has no SDK name — refused). See docs/validation-log.md §20.
fn special_field(s: &str) -> Option<(u8, &'static str)> {
    match s.to_ascii_lowercase().as_str() {
        "printdate" => Some((0x00, "Print Date")),
        "printtime" => Some((0x01, "Print Time")),
        "modificationdate" => Some((0x02, "Modification Date")),
        "modificationtime" => Some((0x03, "Modification Time")),
        "datadate" => Some((0x04, "Data Date")),
        "datatime" => Some((0x05, "Data Time")),
        "reporttitle" => Some((0x06, "Report Title")),
        "reportcomments" => Some((0x07, "Report Comments")),
        "recordselection" => Some((0x08, "Record Selection Formula")),
        "groupselection" => Some((0x09, "Group Selection Formula")),
        "filepath" | "filename" => Some((0x0A, "File Path and Name")),
        "fileauthor" => Some((0x0B, "File Author")),
        "filecreationdate" => Some((0x0C, "File Creation Date")),
        "recordnumber" => Some((0x0D, "Record Number")),
        "groupnumber" => Some((0x0E, "Group Number")),
        "totalpagecount" => Some((0x0F, "Total Page Count")),
        "pagenofm" => Some((0x10, "Page N of M")),
        "pagenumber" => Some((0x11, "Page Number")),
        "hpagenumber" => Some((0x12, "H Page Number")),
        "currentusername" => Some((0x13, "Current User Name")),
        "currentuserid" => Some((0x14, "Current User ID")),
        "currentusertimezone" => Some((0x15, "Current User Time Zone")),
        "printtimezone" => Some((0x16, "Print Time Zone")),
        "datatimezone" => Some((0x17, "Data Time Zone")),
        "contentlocale" => Some((0x18, "Content Locale")),
        _ => None,
    }
}

/* ---------------- dispatch ---------------- */

fn apply_one(rpt: &Rpt, op: &EditOp, outcome: &mut ApplyOutcome) -> Result<Vec<u8>, String> {
    match op {
        // --- v1 ops ---
        EditOp::SetText { object, text } => {
            let (_located, obj) = resolve(rpt, object)?;
            let addr = obj.text_rec().ok_or("this object has no text record")?;
            if !obj.text_editable() {
                return Err("the text has embedded field references; rewriting it would drop them \
                            (not supported in v1)".to_string());
            }
            patch(rpt, addr, "text", FieldEdit::Text(text.clone()))
        }
        EditOp::SetObjectName { object, name } => {
            let (_, obj) = resolve(rpt, object)?;
            let addr = obj.name_rec().ok_or("this object has no name record")?;
            patch(rpt, addr, "name", FieldEdit::Text(name.clone()))
        }
        EditOp::SetBounds { object, left, top, width, height } => {
            let (_, obj) = resolve(rpt, object)?;
            if left.is_none() && top.is_none() && width.is_none() && height.is_none() {
                return Err("set_bounds with nothing to change".to_string());
            }
            let mut current = rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?;
            if let Some(v) = left {
                let addr = obj.position().ok_or("this object has no position record")?;
                current = patch_on(&current, addr, "left", FieldEdit::Int(*v))?;
            }
            if let Some(v) = top {
                let addr = obj.position().ok_or("this object has no position record")?;
                current = patch_on(&current, addr, "top", FieldEdit::Int(*v))?;
            }
            if let Some(v) = width {
                let addr = obj.name_rec().ok_or("this object has no name/size record")?;
                current = patch_on(&current, addr, "width", FieldEdit::Int(*v))?;
            }
            if let Some(v) = height {
                let addr = obj.name_rec().ok_or("this object has no name/size record")?;
                current = patch_on(&current, addr, "height", FieldEdit::Int(*v))?;
            }
            Ok(current)
        }
        EditOp::SetFont { object, name, size_pt, bold, italic, underline, strikeout, weight } => {
            let (_, obj) = resolve(rpt, object)?;
            let addr = obj.font().ok_or("this object has no font record")?;
            let mut current = rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?;
            if let Some(face) = name {
                current = patch_on(&current, addr, "face_name", FieldEdit::Text(face.clone()))?;
            }
            if let Some(pt) = size_pt {
                let fields = fields_of(&current, addr)?;
                let has_twips = fields.iter().any(|f| f.name == "size_twips");
                let has_points = fields.iter().any(|f| f.name == "size_points");
                if has_twips {
                    current = patch_on(&current, addr, "size_twips", FieldEdit::Int((pt * 20.0).round() as i64))?;
                }
                if has_points {
                    current = patch_on(&current, addr, "size_points", FieldEdit::Int(pt.round() as i64))?;
                }
                if !has_twips && !has_points {
                    return Err("the font record stores no size field".to_string());
                }
            }
            if let Some(w) = weight {
                current = patch_on(&current, addr, "weight", FieldEdit::Int(*w))?;
            } else if let Some(b) = bold {
                current = patch_on(&current, addr, "weight", FieldEdit::Int(if *b { 700 } else { 400 }))?;
            }
            if let Some(v) = italic {
                current = patch_on(&current, addr, "italic", FieldEdit::Int(*v as i64))?;
            }
            if let Some(v) = underline {
                current = patch_on(&current, addr, "underline", FieldEdit::Int(*v as i64))?;
            }
            if let Some(v) = strikeout {
                current = patch_on(&current, addr, "strikeout", FieldEdit::Int(*v as i64))?;
            }
            Ok(current)
        }
        EditOp::SetTextColor { object, r, g, b } => {
            let (_, obj) = resolve(rpt, object)?;
            let addr = obj.color().ok_or("this object has no colour record")?;
            patch(rpt, addr, "color", FieldEdit::Int(colorref_word(*r, *g, *b)))
        }
        EditOp::SetSectionHeight { section, height } => {
            let located = crate::locate::locate(rpt);
            let addr = section_rec(&located, section)?;
            patch(rpt, addr, "height", FieldEdit::Int(*height))
        }
        EditOp::SetMargins { left, right, top, bottom } => {
            if left.is_none() && right.is_none() && top.is_none() && bottom.is_none() {
                return Err("set_margins with nothing to change".to_string());
            }
            let addr = page_setup_addr(rpt)?;
            let mut current = rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?;
            for (field, val) in
                [("margin_left", left), ("margin_right", right), ("margin_top", top), ("margin_bottom", bottom)]
            {
                if let Some(v) = val {
                    current = patch_page_level(&current, addr, field, FieldEdit::Int(*v))?;
                }
            }
            Ok(current)
        }
        EditOp::SetOrientation { orientation } => {
            let value = match orientation.to_ascii_lowercase().as_str() {
                "portrait" => 1,
                "landscape" => 2,
                other => return Err(format!("orientation must be portrait or landscape, not `{other}`")),
            };
            let addr = page_devmode_addr(rpt)?;
            patch_page_level(&rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?, addr, "orientation", FieldEdit::Int(value))
        }
        EditOp::SetPaperSize { paper } => {
            let addr = page_devmode_addr(rpt)?;
            patch_page_level(&rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?, addr, "paper_size", FieldEdit::Int(i64::from(*paper)))
        }

        // --- Vibey field-patch ops ---
        EditOp::SetAlignment { object, alignment } => {
            let code = alignment_code(alignment)?;
            let (_, obj) = resolve(rpt, object)?;
            let mut current = rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?;
            let mut patched = false;
            // The paragraph format (0xc0) is authoritative for text-bearing objects; the object
            // format (0xfc) carries it for everything. Patch both when both exist.
            if let Some(addr) = obj.text_format() {
                current = patch_on(&current, addr, "horizontal_alignment", FieldEdit::Int(code))?;
                patched = true;
            }
            if let Some(addr) = obj.format() {
                current = patch_on(&current, addr, "horizontal_alignment", FieldEdit::Int(code))?;
                patched = true;
            }
            if patched { Ok(current) } else { Err("this object has no format record to align".to_string()) }
        }
        EditOp::SetFillColor { object, color } => {
            let (r, g, b) = parse_hex_color(color)?;
            let (located, obj) = resolve(rpt, object)?;
            require_kind(rpt, &located, &obj, &["Box"], "has no fill colour")?;
            let addr = obj.border().ok_or("this object has no border record")?;
            patch(rpt, addr, "background_color", FieldEdit::Int(colorref_word(r, g, b)))
        }
        EditOp::SetLineColor { object, color } => {
            let (r, g, b) = parse_hex_color(color)?;
            let (located, obj) = resolve(rpt, object)?;
            require_kind(rpt, &located, &obj, &["Line", "Box"], "has no line colour")?;
            let addr = obj.border().ok_or("this object has no border record")?;
            patch(rpt, addr, "border_color", FieldEdit::Int(colorref_word(r, g, b)))
        }
        EditOp::SetSectionBackground { section, color } => {
            let (r, g, b) = parse_hex_color(color)?;
            let located = crate::locate::locate(rpt);
            let addr = section_format_rec(&located, section)?;
            patch(rpt, addr, "background_color", FieldEdit::Int(colorref_word(r, g, b)))
        }
        EditOp::SetSectionBreak { section, new_page_before, new_page_after } => {
            if new_page_before.is_none() && new_page_after.is_none() {
                return Err("set_section_break with neither new_page_before nor new_page_after".to_string());
            }
            let located = crate::locate::locate(rpt);
            let addr = section_format_rec(&located, section)?;
            let mut current = rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?;
            if let Some(v) = new_page_before {
                current = patch_on(&current, addr, "new_page_before", FieldEdit::Int(*v as i64))?;
            }
            if let Some(v) = new_page_after {
                current = patch_on(&current, addr, "new_page_after", FieldEdit::Int(*v as i64))?;
            }
            Ok(current)
        }
        EditOp::SetCanGrow { object, can_grow } => {
            let (_, obj) = resolve(rpt, object)?;
            let addr = obj.format().ok_or("this object has no format record")?;
            patch(rpt, addr, "can_grow", FieldEdit::Int(*can_grow as i64))
        }
        EditOp::SetSuppress { object, section, suppress, suppress_if_blank } => {
            match (object, section) {
                (Some(_), Some(_)) => Err("setSuppress takes a target or a section, not both".to_string()),
                (None, None) => Err("setSuppress needs a target or a section".to_string()),
                (Some(sel), None) => {
                    let (_, obj) = resolve(rpt, sel)?;
                    let addr = obj.format().ok_or("this object has no format record")?;
                    // The record stores `visible`; suppress is its negation.
                    patch(rpt, addr, "visible", FieldEdit::Int(!*suppress as i64))
                }
                (None, Some(sec)) => {
                    let located = crate::locate::locate(rpt);
                    let addr = section_format_rec(&located, sec)?;
                    let mut current = rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?;
                    current = patch_on(&current, addr, "visible", FieldEdit::Int(!*suppress as i64))?;
                    if let Some(v) = suppress_if_blank {
                        current = patch_on(&current, addr, "suppress_blank_section", FieldEdit::Int(*v as i64))?;
                    }
                    Ok(current)
                }
            }
        }
        EditOp::SetBorder { object, left, right, top, bottom, color } => {
            let (_, obj) = resolve(rpt, object)?;
            let addr = obj.border().ok_or("this object has no border record")?;
            let mut current = rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?;
            for (field, style) in
                [("left_line_style", left), ("right_line_style", right), ("top_line_style", top), ("bottom_line_style", bottom)]
            {
                if let Some(s) = style {
                    let code = line_style_code(s)?;
                    current = patch_on(&current, addr, field, FieldEdit::Int(code))?;
                }
            }
            if let Some(c) = color {
                let (r, g, b) = parse_hex_color(c)?;
                current = patch_on(&current, addr, "border_color", FieldEdit::Int(colorref_word(r, g, b)))?;
            }
            Ok(current)
        }
        EditOp::SetNumberFormat { object, decimal_places, thousands_separator, suppress_if_zero } => {
            if decimal_places.is_none() && thousands_separator.is_none() && suppress_if_zero.is_none() {
                return Err("set_number_format with nothing to change".to_string());
            }
            let (located, obj) = resolve(rpt, object)?;
            require_kind(rpt, &located, &obj, &["Field"], "has no number format")?;
            let common = obj.common_format().ok_or("this field has no common-format record")?;
            let mut current = rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?;
            // The SDK gate: while use_system_defaults is on, the engine discards the explicit
            // numeric format on save (measured in VibeyReports task notes).
            current = patch_on(&current, common, "use_system_defaults", FieldEdit::Int(0))?;
            for addr in obj.numeric_formats() {
                if let Some(dp) = decimal_places {
                    current = patch_on(&current, addr, "decimal_places", FieldEdit::Int(*dp))?;
                    // Rounding must track the requested precision (enum 11 - dp).
                    current = patch_on(&current, addr, "rounding_type", FieldEdit::Int(11 - *dp))?;
                }
                if let Some(v) = thousands_separator {
                    current = patch_on(&current, addr, "thousands_separator", FieldEdit::Int(*v as i64))?;
                }
                if let Some(v) = suppress_if_zero {
                    current = patch_on(&current, addr, "suppress_if_zero", FieldEdit::Int(*v as i64))?;
                }
            }
            if obj.numeric_formats().is_empty() {
                return Err("this field has no numeric-format records".to_string());
            }
            Ok(current)
        }

        // --- structural ops ---
        EditOp::RemoveObject { object } => {
            let name = object.name.as_deref().ok_or("removeObject needs a name selector")?;
            let located = crate::locate::locate(rpt);
            let donor = structure::find_object_by_name(&located, name)?;
            let canonical = donor.object.name.clone();
            let run = donor.object.run.as_ref().ok_or("object run not located")?;
            let sec = find_section_rec(&located, &donor.area, &donor.section)?;
            let c = structure::Contents::of(rpt)?;
            let bytes = structure::delete_run(rpt, &c, run)?;
            let bytes = structure::adjust_object_count(&bytes, sec, -1)?;
            let reopened = crate::open_bytes(&bytes)?;
            if structure::find_object_by_name(&crate::locate::locate(&reopened), &canonical).is_ok() {
                return Err(format!("object `{canonical}` survived its own removal"));
            }
            outcome.removed_objects.push(canonical);
            Ok(bytes)
        }
        EditOp::AddText { section, name, left, top, width, height, text } => {
            add_object_inner(rpt, section, name, *left, *top, *width, *height,
                |obj| obj.opener_tag == 0x00a5 && obj.text_runs == 1 && obj.embedded_fields == 0,
                &mut |current, addrs| {
                    let mut cur = current.to_vec();
                    if let Some(addr) = addrs.text {
                        cur = patch_on(&cur, addr, "text", FieldEdit::Text(text.clone()))?;
                    }
                    Ok(cur)
                })
        }
        EditOp::AddLine { section, name, left, top, width, height } => {
            add_drawing(rpt, section, name, *left, *top, *width, *height, 2)
        }
        EditOp::AddBox { section, name, left, top, width, height } => {
            add_drawing(rpt, section, name, *left, *top, *width, *height, 1)
        }
        EditOp::AddField { section, name, field_ref, left, top, width, height } => {
            let report = rpt.report();
            // A `{#name}` reference places a running total (pool 9); anything else resolves
            // through the report's own field pool.
            let (pool, index, display) =
                match crate::running_total::running_total_handle(report, field_ref) {
                    Some((p, i)) => (p, i, format!("#{}", field_ref.trim().trim_start_matches('{').trim_end_matches('}').trim_start_matches('#'))),
                    None => {
                        let (p, i) = crate::grouping::field_handle(report, field_ref)?;
                        (p, i, crate::grouping::field_display(report, field_ref))
                    }
                };
            add_object_inner(rpt, section, name, *left, *top, *width, *height,
                |obj| obj.opener_tag == 0x009f && obj.font.is_some(),
                &mut move |current, addrs| {
                    let mut cur = current.to_vec();
                    cur = patch_on(&cur, addrs.opener, "data_source", FieldEdit::Ref {
                        text: display.clone(),
                        kind: u32::from(pool),
                        index: Some(index),
                    })?;
                    // The opener's repeated handle, when the record carries one.
                    let reopened = crate::open_bytes(&cur)?;
                    if let Ok(fields) = reopened.record_fields(
                        rpt_reader::raw::RecordTag(addrs.opener.tag), addrs.opener.nth,
                    ) {
                        if fields.fields.iter().any(|f| f.name == "field_index") {
                            cur = patch_on(&cur, addrs.opener, "field_index", FieldEdit::Int(i64::from(index)))?;
                            cur = patch_on(&cur, addrs.opener, "field_kind", FieldEdit::Int(i64::from(pool)))?;
                        }
                    }
                    Ok(cur)
                })
        }
        EditOp::AddSpecialField { section, name, special_type, left, top, width, height } => {
            let (code, display) = special_field(special_type).ok_or_else(|| {
                format!(
                    "specialType `{special_type}` has no known engine code"
                )
            })?;
            add_object_inner(rpt, section, name, *left, *top, *width, *height,
                |obj| obj.opener_tag == 0x009f && obj.font.is_some(),
                &mut move |current, addrs| {
                    patch_on(current, addrs.opener, "data_source", FieldEdit::Ref {
                        text: display.to_string(),
                        kind: u32::from(SPECIAL_POOL),
                        index: Some(u16::from(code)),
                    })
                })
        }
        EditOp::AddGroup { field_ref, direction } => crate::grouping::add_group(rpt, field_ref, direction.as_deref()),
        EditOp::AddSort { field_ref, direction } => crate::grouping::add_sort(rpt, field_ref, direction),
        EditOp::SetFormula { formula, source } => crate::formula::set_formula(rpt, formula, source),
        EditOp::AddRunningTotal { name, summarize, operation, evaluate_field, reset_field } => {
            crate::running_total::add_running_total(
                rpt,
                name,
                summarize,
                operation,
                &crate::running_total::RtCondition { field: evaluate_field.clone().unwrap_or_default() },
                &crate::running_total::RtCondition { field: reset_field.clone().unwrap_or_default() },
            )
        }
        EditOp::SetRecordSelection { source } => crate::formula::set_record_selection(rpt, source),
        EditOp::AddSubreport { section, name, report_path, left, top, width, height } => {
            crate::subreport::add_subreport(rpt, section, name, report_path, *left, *top, *width, *height)
        }
        EditOp::SetSubreportLink { target, main_report_field, subreport_field, linked_parameter } => {
            crate::subreport::set_subreport_link(rpt, target, main_report_field, subreport_field, linked_parameter)
        }
        EditOp::RemoveTable { alias } => crate::tables::remove_table(rpt, alias),
        EditOp::AddTable { target, new_name, table_name } => {
            crate::tables::add_table(rpt, target, new_name, table_name)
        }
        EditOp::SetTableLocation { target, table_name } => crate::tables::set_table_location(rpt, target, table_name),

        // --- Tier 0 extensions ---
        EditOp::SetTooltip { object, text } => {
            let (_, obj) = resolve(rpt, object)?;
            let addr = obj.format().ok_or("this object has no format record")?;
            patch(rpt, addr, "tool_tip_text", FieldEdit::Text(text.clone()))
        }
        EditOp::SetHyperlink { object, kind, text } => {
            let code = hyperlink_code(kind)?;
            let (_, obj) = resolve(rpt, object)?;
            let addr = obj.format().ok_or("this object has no format record")?;
            let mut current = rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?;
            if let Some(t) = text {
                current = patch_on(&current, addr, "hyperlink_text", FieldEdit::Text(t.clone()))?;
            }
            current = patch_on(&current, addr, "hyperlink_type", FieldEdit::Int(code))?;
            Ok(current)
        }
        EditOp::SetCssClass { object, css_class } => {
            let (_, obj) = resolve(rpt, object)?;
            let addr = obj.format().ok_or("this object has no format record")?;
            patch(rpt, addr, "css_class", FieldEdit::Text(css_class.clone()))
        }
        EditOp::SetZOrder { object, z } => set_z_order(rpt, object, z),
        EditOp::AddPicture { section, name, image_path, left, top, width, height } => {
            crate::picture::add_picture(rpt, section, name, image_path, *left, *top, *width, *height)
        }
        EditOp::AddGuideline { axis, position } => crate::guides::add_guideline(rpt, axis, *position),
        EditOp::AddSection { area, after, name, height } => {
            crate::sections::add_section(rpt, area, after.as_deref(), name.as_deref(), *height)
        }
        EditOp::RemoveSection { area, section } => {
            crate::sections::remove_section(rpt, area.as_deref(), section)
        }
        EditOp::CopyObject { from_report, object, section, new_name } => {
            let sel_name = object.name.as_deref().ok_or("copyObject needs a name selector")?;
            crate::structure::copy_object(rpt, from_report, sel_name, section, new_name.as_deref())
        }
    }
}

/// The `0xfc hyperlink_type` word: RAS `CrHyperlinkTypeEnum` ordinals (6 = no hyperlink).
fn hyperlink_code(kind: &str) -> Result<i64, String> {
    match kind.to_ascii_lowercase().as_str() {
        "website" => Ok(0),
        "email" => Ok(1),
        "html" => Ok(2),
        "none" => Ok(6),
        "reportpart" => Ok(7),
        "anotherobject" => Ok(8),
        other => Err(format!(
            "hyperlink kind must be website/email/html/none/reportPart/anotherObject, not `{other}`"
        )),
    }
}

/// `setZOrder` — record order is the designer's layering (later records render on top), so
/// front/back is a run move to the section's object-span end/start. All runs in a section
/// share the outer mask, so the clone re-masks to itself.
fn set_z_order(rpt: &Rpt, object: &ObjSel, z: &str) -> Result<Vec<u8>, String> {
    let front = match z {
        "front" => true,
        "back" => false,
        other => return Err(format!("z must be front or back, not `{other}`")),
    };
    let located = crate::locate::locate(rpt);
    let sel_name = object.name.as_deref().ok_or("setZOrder needs a name selector")?;
    let donor = structure::find_object_by_name(&located, sel_name)?;
    let run = donor.object.run.as_ref().ok_or("object run not located")?;
    let sec = located
        .areas
        .iter()
        .find(|a| a.name == donor.area)
        .and_then(|a| a.sections.iter().find(|s| s.name == donor.section))
        .ok_or("the object's section vanished")?;
    let others: Vec<&crate::locate::LocatedObject> =
        sec.objects.iter().filter(|o| o.name != donor.object.name).collect();
    if others.is_empty() {
        return Ok(rpt.reencode().map_err(|e| format!("reencode failed: {e}"))?);
    }
    // Take the run bytes while everything is still in place, then remove and re-insert at the
    // (recomputed) survivor boundary — a removal ahead of the insertion point shifts offsets.
    let c = structure::Contents::of(rpt)?;
    let last_before = others.last().and_then(|o| o.run.as_ref()).ok_or("target run missing")?;
    let bytes = structure::clone_run_bytes(
        &c,
        run,
        &crate::locate::InsertAnchor { at: last_before.range.end, outer_mask: last_before.outer_mask },
    )?;
    let without = structure::delete_run(rpt, &c, run)?;

    let mid = crate::open_bytes(&without)?;
    let mid_located = crate::locate::locate(&mid);
    let mid_sec = mid_located
        .areas
        .iter()
        .find(|a| a.name == donor.area)
        .and_then(|a| a.sections.iter().find(|s| s.name == donor.section))
        .ok_or("section vanished after removal")?;
    if mid_sec.objects.is_empty() {
        return Err("the section lost every object; refusing the move".to_string());
    }
    let (at, outer) = if front {
        let last = mid_sec.objects.last().and_then(|o| o.run.as_ref()).ok_or("run missing")?;
        (last.range.end, last.outer_mask)
    } else {
        let first = mid_sec.objects.first().and_then(|o| o.run.as_ref()).ok_or("run missing")?;
        (first.range.start, first.outer_mask)
    };
    let bytes = crate::framing::remask_run(&bytes, last_before.outer_mask, outer);
    let mid_c = structure::Contents::of(&mid)?;
    let new_logical = crate::framing::splice(&mid_c.logical, &mid_c.tree, at..at, &bytes)
        .map_err(|e| format!("z-order splice: {e}"))?;
    let out = mid.write_contents_logical(&new_logical).map_err(|e| format!("write after z-order: {e}"))?;

    // Verify: the object is now first/last in its section.
    let check = crate::open_bytes(&out)?;
    let check_located = crate::locate::locate(&check);
    let check_sec = check_located
        .areas
        .iter()
        .find(|a| a.name == donor.area)
        .and_then(|a| a.sections.iter().find(|s| s.name == donor.section))
        .ok_or("section vanished after move")?;
    let names: Vec<&str> = check_sec.objects.iter().map(|o| o.name.as_str()).collect();
    let want: Option<&str> = if front { names.last().copied() } else { names.first().copied() };
    if want != Some(donor.object.name.as_str()) {
        return Err(format!(
            "`{}` did not land at the {}",
            donor.object.name,
            if front { "front" } else { "back" }
        ));
    }
    Ok(out)
}

/* ---------------- resolution helpers ---------------- */

/// A resolved object: its located entry, plus the accessors the ops use.
struct Resolved {
    obj: crate::locate::LocatedObject,
}

impl Resolved {
    fn name_rec(&self) -> Option<RecAddr> { self.obj.name_rec }
    fn position(&self) -> Option<RecAddr> { self.obj.position }
    fn font(&self) -> Option<RecAddr> { self.obj.font }
    fn color(&self) -> Option<RecAddr> { self.obj.color }
    fn format(&self) -> Option<RecAddr> { self.obj.format }
    fn border(&self) -> Option<RecAddr> { self.obj.border }
    fn text_format(&self) -> Option<RecAddr> { self.obj.text_format }
    fn common_format(&self) -> Option<RecAddr> { self.obj.common_format }
    fn numeric_formats(&self) -> Vec<RecAddr> { self.obj.numeric_formats.clone() }
    fn text_rec(&self) -> Option<RecAddr> { self.obj.text }
    fn text_editable(&self) -> bool { self.obj.embedded_fields == 0 && self.obj.text_runs <= 1 }
}

/// Resolve a selector: by name (case-insensitive, canonical like the designer), by IR id
/// against a fresh snapshot, or by flat index. Returns the located object.
fn resolve(rpt: &Rpt, sel: &ObjSel) -> Result<(Located, Resolved), String> {
    let located = crate::locate::locate(rpt);
    if let Some(name) = &sel.name {
        let donor = structure::find_object_by_name(&located, name)?;
        return Ok((located, Resolved { obj: donor.object }));
    }
    if let Some(id) = &sel.id {
        let ir = crate::ir::build_ir(rpt, &located, "");
        let target = ir.objects.iter().find(|o| &o.id == id).ok_or_else(|| format!("no object `{id}`"))?;
        let obj = located
            .areas
            .iter()
            .find(|a| a.name == target.area)
            .and_then(|a| a.sections.iter().find(|s| s.name == target.section))
            .and_then(|s| s.objects.iter().find(|o| o.name == target.name))
            .cloned()
            .ok_or_else(|| format!("object `{id}` ({}) could not be re-located", target.name))?;
        return Ok((located, Resolved { obj }));
    }
    if let Some(i) = sel.index {
        let ir = crate::ir::build_ir(rpt, &located, "");
        let target = ir.objects.get(i).ok_or_else(|| format!("no object at index {i}"))?;
        let obj = located
            .areas
            .iter()
            .find(|a| a.name == target.area)
            .and_then(|a| a.sections.iter().find(|s| s.name == target.section))
            .and_then(|s| s.objects.iter().find(|o| o.name == target.name))
            .cloned()
            .ok_or_else(|| format!("object at index {i} could not be re-located"))?;
        return Ok((located, Resolved { obj }));
    }
    Err("object selector needs one of id, name or index".to_string())
}

/* ---------------- section lookups ---------------- */

fn section_rec(located: &Located, sel: &SecSel) -> Result<RecAddr, String> {
    let sec = find_section(located, sel)?;
    sec.section_rec.ok_or_else(|| format!("section `{}` record not located", sel.section))
}

fn section_format_rec(located: &Located, sel: &SecSel) -> Result<RecAddr, String> {
    let sec = find_section(located, sel)?;
    sec.section_format.ok_or_else(|| {
        format!("section `{}` has no own format record (its area carries the format)", sel.section)
    })
}

pub fn find_section_by_names<'a>(located: &'a Located, sel: &SecSel) -> Result<&'a crate::locate::LocatedSection, String> {
    find_section(located, sel)
}

fn find_section<'a>(located: &'a Located, sel: &SecSel) -> Result<&'a crate::locate::LocatedSection, String> {
    let mut matches = Vec::new();
    for area in &located.areas {
        if let Some(want_area) = &sel.area {
            if !area.name.eq_ignore_ascii_case(want_area) {
                continue;
            }
        }
        for section in &area.sections {
            if section.name.eq_ignore_ascii_case(&sel.section) {
                matches.push((area.name.clone(), section));
            }
        }
    }
    match matches.len() {
        0 => Err(format!(
            "no section `{}`{}",
            sel.section,
            sel.area.as_deref().map(|a| format!(" in area `{a}`")).unwrap_or_default()
        )),
        1 => Ok(matches.pop().expect("len checked").1),
        _ => Err(format!(
            "section `{}` matches {} areas ({}); name it with its area",
            sel.section,
            matches.len(),
            matches.iter().map(|(a, _)| a.clone()).collect::<Vec<_>>().join(", ")
        )),
    }
}

fn find_section_rec(located: &Located, area: &str, section: &str) -> Result<RecAddr, String> {
    section_rec(located, &SecSel { area: Some(area.to_string()), section: section.to_string() })
}

/// Verify the modelled kind of a located object matches one of `kinds`.
fn require_kind(rpt: &Rpt, located: &Located, obj: &Resolved, kinds: &[&str], what: &str) -> Result<(), String> {
    let name = &obj.obj.name;
    for area in &located.areas {
        for section in &area.sections {
            if let Some(m) = section.objects.iter().find(|o| o.name == *name) {
                if let Ok(mo) = structure::model_object(rpt.report(), &area.name, &section.name, &m.name) {
                    let kind = kind_label(&mo.kind);
                    if !kinds.contains(&kind.as_str()) {
                        return Err(format!("object `{name}` ({kind}) {what}"));
                    }
                }
                return Ok(());
            }
        }
    }
    Ok(())
}

fn kind_label(kind: &model::ReportObjectKind) -> String {
    use model::ReportObjectKind::*;
    match kind {
        Field(_) => "Field",
        Text(_) => "Text",
        FieldHeading(_) => "FieldHeading",
        Box(_) => "Box",
        Line(_) => "Line",
        Picture(_) => "Picture",
        Subreport(_) => "Subreport",
        Chart(_) => "Chart",
        CrossTab(_) => "CrossTab",
        BlobField(_) => "BlobField",
        OlapGrid => "OlapGrid",
        Map => "Map",
        Flash => "Flash",
        Deferred(c) => return format!("Deferred{c:#06x}"),
        Unknown => "Unknown",
    }
    .to_string()
}

/* ---------------- writer plumbing ---------------- */

fn patch(rpt: &Rpt, addr: RecAddr, field: &str, value: FieldEdit) -> Result<Vec<u8>, String> {
    rpt.patch_record_field_with(rpt_reader::raw::RecordTag(addr.tag), addr.nth, field, &value, EditPolicy::Checked)
        .map_err(|e| format!("patch {} `{}`: {e}", addr.key(), field))
}

pub fn patch_pub(bytes: &[u8], addr: &RecAddr, field: &str, value: FieldEdit) -> Result<Vec<u8>, String> {
    patch_on(bytes, *addr, field, value)
}

fn patch_on(bytes: &[u8], addr: RecAddr, field: &str, value: FieldEdit) -> Result<Vec<u8>, String> {
    let rpt = crate::open_bytes(bytes)?;
    patch(&rpt, addr, field, value)
}

fn fields_of(bytes: &[u8], addr: RecAddr) -> Result<Vec<rpt_reader::fields::FieldRead>, String> {
    let rpt = crate::open_bytes(bytes)?;
    rpt.record_fields(rpt_reader::raw::RecordTag(addr.tag), addr.nth)
        .map(|f| f.fields)
        .map_err(|e| format!("cannot read {} fields: {e}", addr.key()))
}

fn page_setup_addr(rpt: &Rpt) -> Result<RecAddr, String> {
    crate::locate::locate(rpt)
        .page_setup
        .ok_or_else(|| "no page setup record in this report".to_string())
}

fn page_devmode_addr(rpt: &Rpt) -> Result<RecAddr, String> {
    crate::locate::locate(rpt).page_devmode.ok_or_else(|| {
        "this report stores no page DEVMODE record (saved with no printer); orientation and \
         paper size have nothing to edit"
            .to_string()
    })
}

fn patch_page_level(bytes: &[u8], addr: RecAddr, field: &str, value: FieldEdit) -> Result<Vec<u8>, String> {
    let rpt = crate::open_bytes(bytes)?;
    let out = rpt
        .patch_record_field_with(rpt_reader::raw::RecordTag(addr.tag), addr.nth, field, &value, EditPolicy::Forced)
        .map_err(|e| format!("patch {} `{}`: {e}", addr.key(), field))?;
    let written = crate::open_bytes(&out)?;
    let read_back = read_field(&written, addr, field)?;
    let same = match (&value, &read_back) {
        (FieldEdit::Int(i), FieldValue::Int(j)) => *i == *j as i64,
        (FieldEdit::Int(i), FieldValue::Uint(u)) => *i == *u as i64,
        (FieldEdit::Float(f), FieldValue::Float(g)) => f == g,
        (FieldEdit::Text(s), FieldValue::Text(t)) => s == t,
        _ => false,
    };
    if same {
        Ok(out)
    } else {
        Err(format!(
            "page-level edit did not read back: {} `{}` wrote {:?}, read {:?}",
            addr.key(), field, value, read_back
        ))
    }
}

/// Read one record field's current value.
///
/// # Errors
///
/// When the record or field does not exist.
pub fn read_field(rpt: &Rpt, addr: RecAddr, field: &str) -> Result<FieldValue, String> {
    let f = rpt
        .record_field(rpt_reader::raw::RecordTag(addr.tag), addr.nth, field)
        .map_err(|e| format!("cannot read {} `{}`: {e}", addr.key(), field))?;
    Ok(f.value)
}

/* ---------------- structural add machinery ---------------- */

/// The freshly inserted clone's record addresses, handed to the per-kind finisher.
pub struct CloneAddrs {
    pub name: Option<RecAddr>,
    pub position: Option<RecAddr>,
    pub text: Option<RecAddr>,
    pub opener: RecAddr,
}

/// Where a new object run splices into a section: after the last existing object when there is
/// one (top of the z-order, the designer's own add behaviour), else at the section's anchor.
pub fn append_anchor_pub(sec: &crate::locate::LocatedSection) -> Result<crate::locate::InsertAnchor, String> {
    append_anchor(sec)
}

fn append_anchor(sec: &crate::locate::LocatedSection) -> Result<crate::locate::InsertAnchor, String> {
    if let Some(last) = sec.objects.last().and_then(|o| o.run.as_ref()) {
        Ok(crate::locate::InsertAnchor { at: last.range.end, outer_mask: last.outer_mask })
    } else {
        sec.insert.ok_or_else(|| format!("section `{}` has no insertion anchor", sec.name))
    }
}

/// Clone a donor object into `section`, patch name/geometry, run the per-kind finisher, and
/// verify the model shows the new object. `finish` receives the current bytes and the clone's
/// addresses (the clone is the LAST object of the target section after insertion).
#[allow(clippy::too_many_arguments)]
pub fn add_object_inner(
    rpt: &Rpt,
    section: &SecSel,
    name: &str,
    left: i64,
    top: i64,
    width: i64,
    height: i64,
    donor_filter: impl Fn(&crate::locate::LocatedObject) -> bool,
    finish: &mut dyn FnMut(&[u8], &CloneAddrs) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let located = crate::locate::locate(rpt);
    let sec = find_section(&located, section)?;
    let anchor = append_anchor(sec)?;
    let out = match structure::find_donor(&located, &donor_filter) {
        Ok(donor) => {
            let run = donor.object.run.as_ref().ok_or("donor run not located")?;
            let c = structure::Contents::of(rpt)?;
            let bytes = structure::clone_run_bytes(&c, run, &anchor)?;
            structure::insert_at(rpt, &c, &anchor, &bytes)?
        }
        // A blank report has no objects to clone from: fall back to the embedded seed and
        // re-mask its run across reports (the copyObject rule).
        Err(_) => {
            let (_, seed_bytes) = structure::seed_donor(&donor_filter)?;
            let c = structure::Contents::of(rpt)?;
            structure::insert_at(rpt, &c, &anchor, &crate::framing::remask_run(&seed_bytes, 0, anchor.outer_mask))?
        }
    };
    let out = structure::adjust_object_count(&out, sec.section_rec.ok_or("section record missing")?, 1)?;

    // The clone is now the last object of the target section; patch it.
    let reopened = crate::open_bytes(&out)?;
    let re_located = crate::locate::locate(&reopened);
    let sec2 = find_section(&re_located, section)?;
    let clone = sec2.objects.last().ok_or("clone missing after insert")?;
    let addrs = CloneAddrs {
        name: clone.name_rec,
        position: clone.position,
        text: clone.text,
        opener: clone.opener.ok_or("clone has no opener")?,
    };

    let mut current = out;
    if let Some(addr) = addrs.name {
        current = patch_on(&current, addr, "name", FieldEdit::Text(name.to_string()))?;
        current = patch_on(&current, addr, "width", FieldEdit::Int(width))?;
        current = patch_on(&current, addr, "height", FieldEdit::Int(height))?;
    }
    if let Some(addr) = addrs.position {
        current = patch_on(&current, addr, "left", FieldEdit::Int(left))?;
        current = patch_on(&current, addr, "top", FieldEdit::Int(top))?;
    }
    current = finish(&current, &addrs)?;

    // Verify: the model now shows an object of this name with the requested geometry.
    let final_rpt = crate::open_bytes(&current)?;
    let area_name = re_located
        .areas
        .iter()
        .find(|a| a.sections.iter().any(|s| s.name == sec2.name))
        .map(|a| a.name.clone())
        .unwrap_or_default();
    let mo = structure::model_object(final_rpt.report(), &area_name, &sec2.name, name)?;
    let b = &mo.bounds;
    if (b.left.0 as i64, b.top.0 as i64, b.width.0 as i64, b.height.0 as i64) != (left, top, width, height) {
        return Err(format!(
            "clone `{name}` read back at ({}, {}, {}, {}), expected ({left}, {top}, {width}, {height})",
            b.left.0, b.top.0, b.width.0, b.height.0
        ));
    }
    Ok(current)
}

/// addLine/addBox: a drawing donor of the right shape (1 = box, 2 = line), with the opener's
/// absolute bottom-right corner kept in step with the requested geometry.
fn add_drawing(
    rpt: &Rpt,
    section: &SecSel,
    name: &str,
    left: i64,
    top: i64,
    width: i64,
    height: i64,
    want_shape: i64,
) -> Result<Vec<u8>, String> {
    let located = crate::locate::locate(rpt);
    // Pick the first drawing whose 0xec shape_kind matches.
    let mut donor: Option<structure::Donor> = None;
    'search: for area in &located.areas {
        for sec in &area.sections {
            for obj in &sec.objects {
                if obj.opener_tag != 0x00a9 || obj.run.is_none() {
                    continue;
                }
                if let Some(border) = obj.border {
                    if structure::read_field_i64(rpt, border, "shape_kind")? == want_shape {
                        donor = Some(structure::Donor {
                            object: obj.clone(),
                            area: area.name.clone(),
                            section: sec.name.clone(),
                        });
                        break 'search;
                    }
                }
            }
        }
    }
    let sec = find_section(&located, section)?;
    let anchor = append_anchor(sec)?;
    let out = match donor {
        Some(d) => {
            let run = d.object.run.as_ref().ok_or("donor run not located")?;
            let c = structure::Contents::of(rpt)?;
            let bytes = structure::clone_run_bytes(&c, run, &anchor)?;
            structure::insert_at(rpt, &c, &anchor, &bytes)?
        }
        // Blank report: seed fallback, remasked across reports.
        None => {
            let (_, seed_bytes) = structure::seed_donor_shape(want_shape)?;
            let c = structure::Contents::of(rpt)?;
            structure::insert_at(rpt, &c, &anchor, &crate::framing::remask_run(&seed_bytes, 0, anchor.outer_mask))?
        }
    };
    let out = structure::adjust_object_count(&out, sec.section_rec.ok_or("section record missing")?, 1)?;

    let reopened = crate::open_bytes(&out)?;
    let re_located = crate::locate::locate(&reopened);
    let sec2 = find_section(&re_located, section)?;
    let clone = match re_located
        .areas
        .iter()
        .find(|a| a.sections.iter().any(|s| s.name == sec2.name))
        .and_then(|a| a.sections.iter().find(|s| s.name == sec2.name))
        .and_then(|s| s.objects.last())
        .cloned()
    {
        Some(c) => c,
        None => {
            return Err("clone missing after insert".to_string());
        }
    };

    let mut current = out;
    if let Some(addr) = clone.name_rec {
        current = patch_on(&current, addr, "name", FieldEdit::Text(name.to_string()))?;
        current = patch_on(&current, addr, "width", FieldEdit::Int(width))?;
        current = patch_on(&current, addr, "height", FieldEdit::Int(height))?;
    }
    if let Some(addr) = clone.position {
        current = patch_on(&current, addr, "left", FieldEdit::Int(left))?;
        current = patch_on(&current, addr, "top", FieldEdit::Int(top))?;
    }
    // The drawing opener's absolute corner must track the new geometry (the model derives a
    // box's true geometry from it).
    if let Some(opener) = clone.opener {
        let reopened = crate::open_bytes(&current)?;
        if let Ok(fields) = reopened.record_fields(rpt_reader::raw::RecordTag(opener.tag), opener.nth) {
            if fields.fields.iter().any(|f| f.name == "right") {
                current = patch_on(&current, opener, "right", FieldEdit::Int(left + width))?;
                current = patch_on(&current, opener, "bottom", FieldEdit::Int(top + height))?;
            }
        }
    }

    let final_rpt = crate::open_bytes(&current)?;
    let area_name = re_located
        .areas
        .iter()
        .find(|a| a.sections.iter().any(|s| s.name == sec2.name))
        .map(|a| a.name.clone())
        .unwrap_or_default();
    let mo = structure::model_object(final_rpt.report(), &area_name, &sec2.name, name)?;
    let b = &mo.bounds;
    if (b.left.0 as i64, b.top.0 as i64, b.width.0 as i64, b.height.0 as i64) != (left, top, width, height) {
        return Err(format!(
            "clone `{name}` read back at ({}, {}, {}, {}), expected ({left}, {top}, {width}, {height})",
            b.left.0, b.top.0, b.width.0, b.height.0
        ));
    }
    Ok(current)
}