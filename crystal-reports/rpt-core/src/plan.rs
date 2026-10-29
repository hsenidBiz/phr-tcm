//! The VibeyReports `LayoutPlan` adapter: parse a plan JSON exactly as the C# worker does
//! (camelCase, case-insensitive) and lower every `LayoutOperation` onto the native
//! [`EditOp`] vocabulary, preserving the action names and required-field rules of
//! `VibeyReports.Contracts` (see the upstream repo's `LayoutPlan.cs` / `LayoutPlanValidator.cs`).
//!
//! Validation here is the structural subset that keeps edits safe — required fields per action,
//! known vocabularies, mutually-exclusive hosts — mirroring the C# validator's per-action rules.
//! Existence checks (does the target object/section exist) happen naturally at apply time, with
//! messages in the same spirit as the worker's.

use serde::Deserialize;

use crate::edit::{ApplyOutcome, EditOp, ObjSel, SecSel};

/// One VibeyReports layout operation, field-compatible with
/// `VibeyReports.Contracts.LayoutOperation`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LayoutOperation {
    /// The action name (`"move"`, `"setFont"`, …).
    pub action: String,
    /// Existing object name (or table alias / subreport name for the three name-spaces).
    pub target: Option<String>,
    /// Section name.
    pub section: Option<String>,
    /// Name for a newly created object (or table alias for addTable).
    pub new_name: Option<String>,
    pub left_twips: Option<i64>,
    pub right_twips: Option<i64>,
    pub top_twips: Option<i64>,
    pub bottom_twips: Option<i64>,
    pub width_twips: Option<i64>,
    pub height_twips: Option<i64>,
    pub font_name: Option<String>,
    pub font_size_pt: Option<f64>,
    pub bold: Option<bool>,
    /// Left, Right, Centre, Justified.
    pub alignment: Option<String>,
    /// Literal text for addText.
    pub text: Option<String>,
    /// Formula source text for setFormula / setRecordSelection.
    pub source: Option<String>,
    /// addRunningTotal: the aggregate (sum/average/count/…).
    pub operation: Option<String>,
    /// Tier-A property batch: rotation degrees (0/90/180/270).
    pub degrees: Option<i16>,
    /// Tier-A: vertical alignment (top/center/bottom).
    pub vertical_alignment: Option<String>,
    /// Tier-A: keep object together.
    pub keep: Option<bool>,
    /// Tier-A: word wrap on/off.
    pub wrap: Option<bool>,
    /// Tier-A: max displayed lines (0 = unlimited).
    pub lines: Option<u16>,
    /// Tier-A: text interpretation (plain/html/rtf).
    pub interpretation: Option<String>,
    /// Tier-A: indents in twips.
    pub first_indent: Option<i64>,
    pub left_indent: Option<i64>,
    pub right_indent: Option<i64>,
    /// Tier-A: reading order (ltr/rtl).
    pub reading_order: Option<String>,
    /// Tier-A: boolean output (true_false/one_zero/t_f).
    pub boolean_output: Option<String>,
    /// Tier-A: section flag cluster.
    pub keep_together: Option<bool>,
    pub reset_page_number: Option<bool>,
    pub print_at_bottom: Option<bool>,
    pub underlay: Option<bool>,
    /// setParameter: the optional-prompt flag.
    pub optional_prompt: Option<bool>,
    /// When set, the action applies INSIDE this embedded subreport (its own object
    /// scope) — e.g. `{"action":"setFont","subreport":"StageWise",…}`.
    pub subreport: Option<String>,
    /// addRunningTotal: evaluate on change of this field (empty = no condition).
    pub evaluate_field: Option<String>,
    /// addRunningTotal: reset on change of this field (empty = no condition).
    pub reset_field: Option<String>,
    /// Bindable field expression for addField/addGroup/addSort.
    pub field_ref: Option<String>,
    /// ascending / descending.
    pub direction: Option<String>,
    pub group_index: Option<usize>,
    pub sort_index: Option<usize>,
    /// "#RRGGBB".
    pub color: Option<String>,
    /// Absolute path of the .rpt to import, for addSubreport.
    pub report_path: Option<String>,
    pub main_report_field: Option<String>,
    pub subreport_field: Option<String>,
    pub linked_parameter: Option<String>,
    /// Database object name for addTable / setTableLocation.
    pub table_name: Option<String>,
    pub new_page_before: Option<bool>,
    pub new_page_after: Option<bool>,
    /// addSpecialField's allowlist member.
    pub special_type: Option<String>,
    pub decimal_places: Option<i64>,
    pub thousands_separator: Option<bool>,
    pub suppress_if_zero: Option<bool>,
    pub can_grow: Option<bool>,
    pub suppress: Option<bool>,
    pub suppress_if_blank: Option<bool>,
    /// setBorder per-side styles: none/single/double/dashed/dotted.
    pub left: Option<String>,
    /// setHyperlink's kind vocabulary member (website/email/html/none/reportPart/anotherObject).
    #[serde(default)] pub kind: Option<String>,
    /// setZOrder's target: front or back.
    #[serde(default)] pub z: Option<String>,
    /// addGuideline's axis: horizontal or vertical.
    #[serde(default)] pub axis: Option<String>,
    /// addPicture's image file path.
    #[serde(default)] pub image_path: Option<String>,
    /// copyObject's source report path.
    #[serde(default)] pub from_report: Option<String>,
    pub right: Option<String>,
    pub top: Option<String>,
    pub bottom: Option<String>,
}

/// The plan itself: `{"planVersion": 1, "operations": [...]}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPlan {
    /// Accepted plan shape version.
    #[serde(default)]
    pub plan_version: i32,
    /// The operations, applied in order.
    pub operations: Vec<LayoutOperation>,
}

/// Lower a parsed plan to native edit ops, with per-action validation.
///
/// # Errors
///
/// One message per offending operation, prefixed `[op N]`, joined by ` | ` — the same shape the
/// C# `InvalidPlanException` reports.
pub fn lower(plan: &LayoutPlan) -> Result<Vec<EditOp>, String> {
    let mut ops = Vec::new();
    let mut errors = Vec::new();
    for (i, o) in plan.operations.iter().enumerate() {
        match lower_one(o) {
            Ok(op) => ops.push(match &o.subreport {
                Some(sub) => EditOp::InSubreport { subreport: sub.clone(), ops: vec![op] },
                None => op,
            }),
            Err(e) => errors.push(format!("[op {i}] {e}")),
        }
    }
    if errors.is_empty() {
        Ok(ops)
    } else {
        Err(errors.join(" | "))
    }
}

fn need<T>(v: &Option<T>, what: &str) -> Result<T, String>
where
    T: Clone,
{
    v.as_ref().map(Clone::clone).ok_or_else(|| format!("`{what}` is required for this action"))
}

fn obj(o: &LayoutOperation) -> Result<ObjSel, String> {
    Ok(ObjSel { name: Some(need(&o.target, "target")?), ..Default::default() })
}

fn sec(o: &LayoutOperation) -> Result<SecSel, String> {
    Ok(SecSel { area: None, section: need(&o.section, "section")? })
}

fn lower_one(o: &LayoutOperation) -> Result<EditOp, String> {
    let action = o.action.as_str();
    match action {
        "move" => Ok(EditOp::SetBounds {
            object: obj(o)?,
            left: Some(need(&o.left_twips, "leftTwips")?),
            top: Some(need(&o.top_twips, "topTwips")?),
            width: None,
            height: None,
        }),
        "resize" => Ok(EditOp::SetBounds {
            object: obj(o)?,
            left: None,
            top: None,
            width: Some(need(&o.width_twips, "widthTwips")?),
            height: Some(need(&o.height_twips, "heightTwips")?),
        }),
        "setFont" => Ok(EditOp::SetFont {
            object: obj(o)?,
            name: Some(need(&o.font_name, "fontName")?),
            size_pt: None,
            bold: None,
            italic: None,
            underline: None,
            strikeout: None,
            weight: None,
        }),
        "setFontSize" => Ok(EditOp::SetFont {
            object: obj(o)?,
            name: None,
            size_pt: Some(need(&o.font_size_pt, "fontSizePt")?),
            bold: None,
            italic: None,
            underline: None,
            strikeout: None,
            weight: None,
        }),
        "setBold" => Ok(EditOp::SetFont {
            object: obj(o)?,
            name: None,
            size_pt: None,
            bold: Some(need(&o.bold, "bold")?),
            italic: None,
            underline: None,
            strikeout: None,
            weight: None,
        }),
        "setAlignment" => Ok(EditOp::SetAlignment {
            object: obj(o)?,
            alignment: need(&o.alignment, "alignment")?,
        }),
        "addText" => Ok(EditOp::AddText {
            section: sec(o)?,
            name: need(&o.new_name, "newName")?,
            left: need(&o.left_twips, "leftTwips")?,
            top: need(&o.top_twips, "topTwips")?,
            width: need(&o.width_twips, "widthTwips")?,
            height: need(&o.height_twips, "heightTwips")?,
            text: need(&o.text, "text")?,
        }),
        "addLine" => Ok(EditOp::AddLine {
            section: sec(o)?,
            name: need(&o.new_name, "newName")?,
            left: need(&o.left_twips, "leftTwips")?,
            top: need(&o.top_twips, "topTwips")?,
            width: need(&o.width_twips, "widthTwips")?,
            height: need(&o.height_twips, "heightTwips")?,
        }),
        "addBox" => Ok(EditOp::AddBox {
            section: sec(o)?,
            name: need(&o.new_name, "newName")?,
            left: need(&o.left_twips, "leftTwips")?,
            top: need(&o.top_twips, "topTwips")?,
            width: need(&o.width_twips, "widthTwips")?,
            height: need(&o.height_twips, "heightTwips")?,
        }),
        "resizeSection" => Ok(EditOp::SetSectionHeight {
            section: sec(o)?,
            height: need(&o.height_twips, "heightTwips")?,
        }),
        "addField" => Ok(EditOp::AddField {
            section: sec(o)?,
            name: need(&o.new_name, "newName")?,
            field_ref: need(&o.field_ref, "fieldRef")?,
            left: need(&o.left_twips, "leftTwips")?,
            top: need(&o.top_twips, "topTwips")?,
            width: need(&o.width_twips, "widthTwips")?,
            height: need(&o.height_twips, "heightTwips")?,
        }),
        "removeObject" => Ok(EditOp::RemoveObject { object: obj(o)? }),
        "setTextColor" | "setFillColor" | "setLineColor" => {
            let color = need(&o.color, "color")?;
            match action {
                "setTextColor" => {
                    let (r, g, b) = hex(&color)?;
                    Ok(EditOp::SetTextColor { object: obj(o)?, r, g, b })
                }
                "setFillColor" => Ok(EditOp::SetFillColor { object: obj(o)?, color }),
                _ => Ok(EditOp::SetLineColor { object: obj(o)?, color }),
            }
        }
        "setSectionBackground" => Ok(EditOp::SetSectionBackground {
            section: sec(o)?,
            color: need(&o.color, "color")?,
        }),
        "addSubreport" => Ok(EditOp::AddSubreport {
            section: sec(o)?,
            name: need(&o.new_name, "newName")?,
            report_path: need(&o.report_path, "reportPath")?,
            left: need(&o.left_twips, "leftTwips")?,
            top: need(&o.top_twips, "topTwips")?,
            width: need(&o.width_twips, "widthTwips")?,
            height: need(&o.height_twips, "heightTwips")?,
        }),
        "setSubreportLink" => Ok(EditOp::SetSubreportLink {
            target: need(&o.target, "target")?,
            main_report_field: need(&o.main_report_field, "mainReportField")?,
            subreport_field: need(&o.subreport_field, "subreportField")?,
            linked_parameter: need(&o.linked_parameter, "linkedParameter")?,
        }),
        "removeTable" => Ok(EditOp::RemoveTable { alias: need(&o.target, "target")? }),
        "addTable" => Ok(EditOp::AddTable {
            target: need(&o.target, "target")?,
            new_name: need(&o.new_name, "newName")?,
            table_name: need(&o.table_name, "tableName")?,
        }),
        "setTableLocation" => Ok(EditOp::SetTableLocation {
            target: need(&o.target, "target")?,
            table_name: need(&o.table_name, "tableName")?,
        }),
        "setSectionBreak" => Ok(EditOp::SetSectionBreak {
            section: sec(o)?,
            new_page_before: o.new_page_before,
            new_page_after: o.new_page_after,
        }),
        "addSpecialField" => Ok(EditOp::AddSpecialField {
            section: sec(o)?,
            name: need(&o.new_name, "newName")?,
            special_type: need(&o.special_type, "specialType")?,
            left: need(&o.left_twips, "leftTwips")?,
            top: need(&o.top_twips, "topTwips")?,
            width: need(&o.width_twips, "widthTwips")?,
            height: need(&o.height_twips, "heightTwips")?,
        }),
        "setNumberFormat" => Ok(EditOp::SetNumberFormat {
            object: obj(o)?,
            decimal_places: o.decimal_places,
            thousands_separator: o.thousands_separator,
            suppress_if_zero: o.suppress_if_zero,
        }),
        "setCanGrow" => Ok(EditOp::SetCanGrow { object: obj(o)?, can_grow: need(&o.can_grow, "canGrow")? }),
        "setSuppress" => {
            let (target, section) = match (&o.target, &o.section) {
                (Some(_), Some(_)) => return Err("setSuppress takes target or section, not both".to_string()),
                (None, None) => return Err("setSuppress needs a target or a section".to_string()),
                (Some(_), None) => (Some(obj(o)?), None),
                (None, Some(_)) => (None, Some(sec(o)?)),
            };
            if o.suppress_if_blank.is_some() && target.is_some() {
                return Err("suppressIfBlank applies to sections only".to_string());
            }
            Ok(EditOp::SetSuppress {
                object: target,
                section,
                suppress: need(&o.suppress, "suppress")?,
                suppress_if_blank: o.suppress_if_blank,
            })
        }
        "addGroup" => Ok(EditOp::AddGroup {
            field_ref: need(&o.field_ref, "fieldRef")?,
            direction: o.direction.clone(),
        }),
        "addRunningTotal" => Ok(EditOp::AddRunningTotal {
            name: need(&o.new_name, "newName")?,
            summarize: need(&o.field_ref, "fieldRef")?,
            operation: need(&o.operation, "operation")?,
            evaluate_field: o.evaluate_field.clone(),
            reset_field: o.reset_field.clone(),
        }),
        "setSectionVisibility" => Ok(EditOp::SetSectionVisibility {
            section: SecSel { area: None, section: need(&o.section, "section")? },
            source: need(&o.source, "source")?,
        }),
        "setTextRotation" => Ok(EditOp::SetTextRotation {
            object: ObjSel { id: None, name: o.target.clone(), index: None }, degrees: need(&o.degrees, "degrees")?,
        }),
        "setVerticalAlignment" => Ok(EditOp::SetVerticalAlignment {
            object: ObjSel { id: None, name: o.target.clone(), index: None }, alignment: need(&o.vertical_alignment, "verticalAlignment")?,
        }),
        "setKeepTogether" => Ok(EditOp::SetKeepObjectTogether {
            object: ObjSel { id: None, name: o.target.clone(), index: None }, keep: need(&o.keep, "keep")?,
        }),
        "setWordWrap" => Ok(EditOp::SetWordWrap {
            object: ObjSel { id: None, name: o.target.clone(), index: None }, wrap: need(&o.wrap, "wrap")?,
        }),
        "setMaxLines" => Ok(EditOp::SetMaxLines {
            object: ObjSel { id: None, name: o.target.clone(), index: None }, lines: need(&o.lines, "lines")?,
        }),
        "setTextInterpretation" => Ok(EditOp::SetTextInterpretation {
            object: ObjSel { id: None, name: o.target.clone(), index: None }, format: need(&o.interpretation, "interpretation")?,
        }),
        "setIndents" => Ok(EditOp::SetIndents {
            object: ObjSel { id: None, name: o.target.clone(), index: None }, first: o.first_indent, left: o.left_indent, right: o.right_indent,
        }),
        "setReadingOrder" => Ok(EditOp::SetReadingOrder {
            object: ObjSel { id: None, name: o.target.clone(), index: None }, order: need(&o.reading_order, "readingOrder")?,
        }),
        "setBooleanFormat" => Ok(EditOp::SetBooleanFormat {
            object: ObjSel { id: None, name: o.target.clone(), index: None }, output: need(&o.boolean_output, "booleanOutput")?,
        }),
        "setSectionFlags" => Ok(EditOp::SetSectionFlags {
            section: SecSel { area: None, section: need(&o.section, "section")? },
            keep_together: o.keep_together,
            reset_page_number_after: o.reset_page_number,
            print_at_bottom_of_page: o.print_at_bottom,
            underlay_section: o.underlay,
        }),
        "addSummary" => Ok(EditOp::AddSummary {
            name: need(&o.new_name, "newName")?,
            summarize: need(&o.field_ref, "fieldRef")?,
            operation: need(&o.operation, "operation")?,
        }),
        "setParameter" => Ok(EditOp::SetParameter {
            name: need(&o.target, "target")?,
            prompt_text: o.text.clone(),
            optional: o.optional_prompt,
        }),
        "removeSort" => Ok(EditOp::RemoveSort {
            field_ref: need(&o.field_ref, "fieldRef")?,
        }),
        "setGroupSelection" => Ok(EditOp::SetGroupSelection {
            source: need(&o.source, "source")?,
        }),
        "setFormula" => Ok(EditOp::SetFormula {
            formula: need(&o.target, "target")?,
            source: need(&o.source, "source")?,
        }),
        "setRecordSelection" => Ok(EditOp::SetRecordSelection {
            source: need(&o.source, "source")?,
        }),
        "addSort" => Ok(EditOp::AddSort {
            field_ref: need(&o.field_ref, "fieldRef")?,
            direction: need(&o.direction, "direction")?,
        }),
        "setBorder" => {
            if o.left.is_none() && o.right.is_none() && o.top.is_none() && o.bottom.is_none() && o.color.is_none() {
                return Err("setBorder needs at least one side or a colour".to_string());
            }
            Ok(EditOp::SetBorder {
                object: obj(o)?,
                left: o.left.clone(),
                right: o.right.clone(),
                top: o.top.clone(),
                bottom: o.bottom.clone(),
                color: o.color.clone(),
            })
        }
        // --- Tier 0 extensions (beyond the Vibey contract; same naming style) ---
        "setTooltip" => Ok(EditOp::SetTooltip {
            object: obj(o)?,
            text: need(&o.text, "text")?,
        }),
        "setHyperlink" => Ok(EditOp::SetHyperlink {
            object: obj(o)?,
            kind: need(&o.kind, "kind")?,
            text: o.text.clone(),
        }),
        "setZOrder" => Ok(EditOp::SetZOrder {
            object: obj(o)?,
            z: need(&o.z, "z")?,
        }),
        "setCssClass" => Ok(EditOp::SetCssClass {
            object: obj(o)?,
            css_class: need(&o.text, "text")?,
        }),
        "addPicture" => Ok(EditOp::AddPicture {
            section: sec(o)?,
            name: need(&o.new_name, "newName")?,
            image_path: need(&o.image_path, "imagePath")?,
            left: need(&o.left_twips, "leftTwips")?,
            top: need(&o.top_twips, "topTwips")?,
            width: need(&o.width_twips, "widthTwips")?,
            height: need(&o.height_twips, "heightTwips")?,
        }),
        "addGuideline" => Ok(EditOp::AddGuideline {
            axis: need(&o.axis, "axis")?,
            position: need(&o.left_twips, "leftTwips")?,
        }),
        "copyObject" => Ok(EditOp::CopyObject {
            from_report: need(&o.from_report, "fromReport")?,
            object: obj(o)?,
            section: sec(o)?,
            new_name: o.new_name.clone(),
        }),
        "setMargins" => Ok(EditOp::SetMargins {
            left: o.left_twips,
            right: o.right_twips,
            top: o.top_twips,
            bottom: o.bottom_twips,
        }),
        "addSection" => Ok(EditOp::AddSection {
            area: need(&o.target, "target (the area name)")?,
            after: o.section.clone(),
            name: o.new_name.clone(),
            height: o.height_twips,
        }),
        "removeSection" => Ok(EditOp::RemoveSection {
            area: o.target.clone(),
            section: need(&o.section, "section")?,
        }),
        other => Err(format!("unknown action `{other}`")),
    }
}

fn hex(s: &str) -> Result<(u8, u8, u8), String> {
    let h = s.strip_prefix('#').ok_or_else(|| format!("colour `{s}` must start with `#`"))?;
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("colour `{s}` is not #RRGGBB"));
    }
    let v = u32::from_str_radix(h, 16).map_err(|_| format!("colour `{s}` is not #RRGGBB"))?;
    Ok((((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8))
}

/// Parse a plan JSON (Vibey camelCase), lower, and apply it to the report at `path`.
///
/// # Errors
///
/// Parse/validation failures as one joined message; apply failures name the failing op.
pub fn apply_plan_json(path: &str, plan_json: &str) -> Result<(Vec<u8>, ApplyOutcome), String> {
    let plan: LayoutPlan = serde_json::from_str(plan_json).map_err(|e| format!("bad plan JSON: {e}"))?;
    let ops = lower(&plan)?;
    let rpt = crate::open(path)?;
    let mut outcome = ApplyOutcome::default();
    let bytes = crate::edit::apply_edits_on(&rpt, &ops, &mut outcome).map_err(|e| e.to_string())?;
    Ok((bytes, outcome))
}
