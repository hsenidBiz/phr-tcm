//! The JSON IR (`crpt-ir/1`) — one document that joins the decoded semantic model (what the user
//! sees) with the record addresses (what an edit needs), projected for the editor UI.
//!
//! The model's areas are canonically sorted, the walk's are in document order, so the two are
//! paired by name (`area name` → `section name` → object name, with occurrence disambiguation for
//! empty/duplicate names). Every pair that fails to resolve produces a warning in the IR rather
//! than an error — an unpaired object is simply not editable, never a failed open.

use serde::{Deserialize, Serialize};

use rpt_reader::{model, Rpt};

use crate::locate::{Located, RecAddr};

/// The IR schema version. Bump on any breaking shape change.
pub const IR_FORMAT: &str = "crpt-ir/1";

/// A whole report, ready for the UI. Deterministic for the same input file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ir {
    /// Always [`IR_FORMAT`].
    pub format: String,
    /// The file the IR was read from (informational).
    pub file: String,
    /// Document properties from the OLE SummaryInformation stream.
    pub summary: SummaryIr,
    /// The stream envelope's encryption version word (property of the file, not the layout).
    pub version: u16,
    /// The Crystal Reports version that wrote the file, as it stores it.
    pub authoring_version: String,
    /// Page setup: margins, orientation, paper — as stored (model `PrintOptions` shape).
    pub print_options: serde_json::Value,
    /// Areas (canonically ordered: report header → page header → groups → detail → footers).
    pub areas: Vec<AreaIr>,
    /// Every object in one flat, id-addressed list; sections reference ids.
    pub objects: Vec<ObjectIr>,
    /// Names of the report's subreports (their internals live in their own streams).
    pub subreports: Vec<String>,
    /// The report's groups, outermost first (the `addGroup` view).
    pub groups: Vec<GroupIr>,
    /// The record sort order (group sorts first, then explicit sorts — the `addSort` view).
    pub sorts: Vec<SortIr>,
    /// The database fields the report's data source exposes — the `addField` allowlist
    /// (offline: fields the report already references; the live DB is not contacted).
    pub available_fields: Vec<AvailableFieldIr>,
    /// Field/formula/parameter names from the data definition (useful for the UI palette).
    pub field_names: Vec<String>,
    /// Everything that failed to pair or decode; edits against unpaired items error.
    pub warnings: Vec<String>,
}

/// Document properties from the OLE SummaryInformation stream.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SummaryIr {
    /// The report title.
    pub title: Option<String>,
    /// The subject.
    pub subject: Option<String>,
    /// The author.
    pub author: Option<String>,
    /// Free-form comments.
    pub comments: Option<String>,
    /// The last author to save the report.
    pub last_author: Option<String>,
}

/// One area of the report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaIr {
    /// The area's name (e.g. `ReportHeaderArea1`).
    pub name: String,
    /// The area kind (ReportHeader/PageHeader/GroupHeader/Detail/GroupFooter/ReportFooter/PageFooter).
    pub kind: String,
    /// For group areas: the 0-based group nesting level (outermost = 0).
    pub group_level: Option<usize>,
    /// The area's sections.
    pub sections: Vec<SectionIr>,
}

/// One section of an area.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionIr {
    /// The section's name (e.g. `ReportHeaderSection1`).
    pub name: String,
    /// The section height in twips (1440 = 1 inch).
    pub height: i64,
    /// The section width in twips (0 unless a layout resolved the page geometry).
    pub width: i64,
    /// Whether the section is suppressed.
    pub suppress: bool,
    /// Ids of this section's objects, in model order.
    pub object_ids: Vec<String>,
    /// Address of the section record (`0x8c`) — height/name live here.
    pub address: Option<RecAddr>,
}

/// One report object — the flat unit the UI selects and edits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectIr {
    /// Stable id within this IR snapshot (`o0`, `o1`, …). Ids are valid until the file is edited.
    pub id: String,
    /// The object's designer name (may be empty).
    pub name: String,
    /// The object kind: Text, FieldHeading, Field, Box, Line, Picture, Subreport, Chart, CrossTab, …
    pub kind: String,
    /// The containing area's name.
    pub area: String,
    /// The containing section's name.
    pub section: String,
    /// Bounding box in twips, section-relative.
    pub bounds: BoundsIr,
    /// For text-bearing kinds: content, primary font and color.
    pub text: Option<TextIr>,
    /// For field objects: which field/formula/parameter they display.
    pub field: Option<FieldIr>,
    /// For subreports: the subreport's name.
    pub subreport: Option<String>,
    /// The record addresses each edit kind needs. `None` = that edit is unavailable.
    pub address: AddressIr,
}

/// A bounding box in twips (1440 twips = 1 inch = 72 points).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct BoundsIr {
    /// Left edge, section-relative.
    pub left: i64,
    /// Top edge, section-relative.
    pub top: i64,
    /// Width.
    pub width: i64,
    /// Height.
    pub height: i64,
}

/// Text content, primary font and color of a text-bearing object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextIr {
    /// The literal text (single-run objects: identical to `display`).
    pub text: String,
    /// The full rendered text including embedded field placeholders.
    pub display: String,
    /// True when the text has no embedded field references — the shape `set_text` rewrites.
    pub editable: bool,
    /// The primary (first-run) font.
    pub font: FontIr,
    /// The primary text color.
    pub color: ColorIr,
}

/// A font, as the designer shows it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FontIr {
    /// Face name (e.g. `Arial`).
    pub name: String,
    /// Size in points.
    pub size_pt: f64,
    /// Bold (weight ≥ 700).
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Struck through.
    pub strikethrough: bool,
    /// The raw font weight (400 = normal, 700 = bold).
    pub weight: i64,
}

/// An RGBA color. Alpha is 255 in every file seen.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ColorIr {
    /// Red 0–255.
    pub r: u8,
    /// Green 0–255.
    pub g: u8,
    /// Blue 0–255.
    pub b: u8,
    /// Alpha 0–255.
    pub a: u8,
}

/// Which data a field object displays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldIr {
    /// The stored reference text (e.g. `{Command.name}`, `{@formula}`, `{?param}`, `Page N of M`).
    pub data_source: String,
    /// The pool it comes from (DatabaseField/Formula/Parameter/Special/Summary/GroupName/…).
    pub ref_kind: String,
}

/// One group, as the grouping operations see it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupIr {
    /// The grouped field's display form (`{alias.field}` / `@formula`).
    pub field_ref: String,
    /// `ascending` / `descending` (a group's direction is its sort's).
    pub direction: String,
    /// The group header section's name (the `addText`-into-group target), when present.
    pub header_section: Option<String>,
    /// The group footer section's name.
    pub footer_section: Option<String>,
}

/// One record sort.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortIr {
    /// The sorted field's display form.
    pub field_ref: String,
    /// `ascending` / `descending`.
    pub direction: String,
}

/// One bindable field of the data source (the `addField` vocabulary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableFieldIr {
    /// The bindable expression — pass this as `addField`'s fieldRef.
    pub formula_form: String,
    /// The bare field name.
    pub name: String,
    /// The table alias the field belongs to.
    pub table_alias: String,
    /// The value type (String/Number/DateTime/Boolean/…).
    pub value_type: String,
}

/// The record addresses behind one object's editable fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AddressIr {
    /// `0x9e` — name, width, height.
    pub name: Option<RecAddr>,
    /// `0xbe` — left, top.
    pub position: Option<RecAddr>,
    /// `0xc2` — text.
    pub text: Option<RecAddr>,
    /// `0x08` — font.
    pub font: Option<RecAddr>,
    /// `0x0100` — text color.
    pub color: Option<RecAddr>,
}

/// Project `rpt` (plus its [`Located`] addresses) to the JSON IR.
pub fn build_ir(rpt: &Rpt, located: &Located, file: &str) -> Ir {
    let report = rpt.report();
    let mut warnings = Vec::new();

    let summary = rpt
        .summary_info()
        .map(|s| SummaryIr {
            title: s.title.clone(),
            subject: s.subject.clone(),
            author: s.author.clone(),
            comments: s.comments.clone(),
            last_author: s.last_author.clone(),
        })
        .unwrap_or_default();

    let mut objects: Vec<ObjectIr> = Vec::new();
    let mut areas: Vec<AreaIr> = Vec::new();

    for area in &report.report_definition.areas {
        let mut sections_ir = Vec::new();
        for section in &area.sections {
            let mut object_ids = Vec::new();
            for obj in &section.objects {
                let id = format!("o{}", objects.len());
                let (obj_ir, warn) = project_object(obj, &area.name, &section.name, &id, located);
                if let Some(w) = warn {
                    warnings.push(format!("object `{}` ({id}): {w}", obj.name));
                }
                object_ids.push(id);
                objects.push(obj_ir);
            }
            sections_ir.push(SectionIr {
                name: section.name.clone(),
                height: section.height.0 as i64,
                width: section.width.0 as i64,
                suppress: section.format.base.suppress,
                object_ids,
                address: find_section(located, &area.name, &section.name),
            });
        }
        areas.push(AreaIr {
            name: area.name.clone(),
            kind: format!("{:?}", area.kind),
            group_level: area.group_level,
            sections: sections_ir,
        });
    }

    let field_names = report
        .data_definition
        .field_definitions
        .iter()
        .map(|f| f.name.clone())
        .collect();

    let direction_of = |d: &model::SortDirection| {
        match d {
            model::SortDirection::AscendingOrder => "ascending",
            model::SortDirection::DescendingOrder => "descending",
            _ => "unknown",
        }
        .to_string()
    };
    let groups = report
        .data_definition
        .groups
        .iter()
        .enumerate()
        .map(|(level, g)| {
            let header_section = areas
                .iter()
                .find(|a| a.kind == "GroupHeader" && a.group_level == Some(level))
                .and_then(|a| a.sections.first().map(|s| s.name.clone()));
            let footer_section = areas
                .iter()
                .find(|a| a.kind == "GroupFooter" && a.group_level == Some(level))
                .and_then(|a| a.sections.first().map(|s| s.name.clone()));
            GroupIr {
                field_ref: crate::grouping::field_display(report, &g.condition_field),
                direction: direction_of(&g.sort.direction),
                header_section,
                footer_section,
            }
        })
        .collect();
    let sorts = report
        .data_definition
        .record_sorts
        .iter()
        .map(|s| SortIr {
            field_ref: crate::grouping::field_display(report, &s.field),
            direction: direction_of(&s.direction),
        })
        .collect();

    // Offline available fields: the tables' columns the model already knows, keyed by the
    // tables' own aliases; parameters and formulas by their definitions.
    let mut available_fields: Vec<AvailableFieldIr> = Vec::new();
    for t in &report.database.tables {
        for f in &t.data_fields {
            available_fields.push(AvailableFieldIr {
                formula_form: format!("{{{}.{}}}", t.alias, f.name),
                name: f.name.clone(),
                table_alias: t.alias.clone(),
                value_type: format!("{:?}", f.value_type),
            });
        }
    }
    for f in &report.data_definition.field_definitions {
        let formula_form = match &f.kind {
            model::FieldKindData::Formula(_) => format!("@{}", f.name.trim_start_matches('@')),
            model::FieldKindData::Parameter(_) => format!("?{}", f.name.trim_start_matches('@')),
            _ => continue,
        };
        available_fields.push(AvailableFieldIr {
            formula_form,
            name: f.name.clone(),
            table_alias: String::new(),
            value_type: format!("{:?}", f.value_type),
        });
    }

    Ir {
        format: IR_FORMAT.to_string(),
        file: file.to_string(),
        summary,
        version: report.version,
        authoring_version: format!("{:?}", report.authoring_version),
        print_options: serde_json::to_value(&report.print_options).unwrap_or(serde_json::Value::Null),
        areas,
        objects,
        subreports: report.subreports.iter().map(|s| s.name.clone()).collect(),
        groups,
        sorts,
        available_fields,
        field_names,
        warnings,
    }
}

/// Project one model object, pairing it with a located object by name + occurrence.
fn project_object(
    obj: &model::ReportObject,
    area: &str,
    section: &str,
    id: &str,
    located: &Located,
) -> (ObjectIr, Option<String>) {
    let kind_label: String;
    let mut text: Option<TextIr> = None;
    let mut field: Option<FieldIr> = None;
    let mut subreport: Option<String> = None;
    match &obj.kind {
        model::ReportObjectKind::Text(t) => {
            kind_label = "Text".to_string();
            text = Some(text_ir(&t.text, &t.display, &t.font_color, t.embedded_fields.is_empty()));
        }
        model::ReportObjectKind::FieldHeading(h) => {
            kind_label = "FieldHeading".to_string();
            text = Some(text_ir(&h.text, &h.text, &h.font_color, true));
        }
        model::ReportObjectKind::Field(f) => {
            kind_label = "Field".to_string();
            text = Some(text_ir(&f.data_source, &f.data_source, &f.font_color, false));
            field = Some(FieldIr {
                data_source: f.data_source.clone(),
                ref_kind: format!("{:?}", f.ref_kind),
            });
        }
        model::ReportObjectKind::Box(_) => kind_label = "Box".to_string(),
        model::ReportObjectKind::Line(_) => kind_label = "Line".to_string(),
        model::ReportObjectKind::Picture(_) => kind_label = "Picture".to_string(),
        model::ReportObjectKind::Subreport(s) => {
            kind_label = "Subreport".to_string();
            subreport = Some(s.subreport_name.clone());
        }
        model::ReportObjectKind::Chart(_) => kind_label = "Chart".to_string(),
        model::ReportObjectKind::CrossTab(_) => kind_label = "CrossTab".to_string(),
        model::ReportObjectKind::BlobField(_) => kind_label = "BlobField".to_string(),
        model::ReportObjectKind::OlapGrid => kind_label = "OlapGrid".to_string(),
        model::ReportObjectKind::Map => kind_label = "Map".to_string(),
        model::ReportObjectKind::Flash => kind_label = "Flash".to_string(),
        model::ReportObjectKind::Deferred(code) => {
            kind_label = format!("Deferred{code:#06x}");
        }
        model::ReportObjectKind::Unknown => kind_label = "Unknown".to_string(),
    }

    // Pair with the walk's object by section + name.
    let located_obj = find_object(located, area, section, &obj.name);
    let mut warning = None;
    let address = match &located_obj {
        Some(lo) => AddressIr {
            name: lo.name_rec,
            position: lo.position,
            text: lo.text,
            font: lo.font,
            color: lo.color,
        },
        None => {
            warning = Some("no record address located; not editable".to_string());
            AddressIr::default()
        }
    };

    let obj_ir = ObjectIr {
        id: id.to_string(),
        name: obj.name.clone(),
        kind: kind_label,
        area: area.to_string(),
        section: section.to_string(),
        bounds: BoundsIr {
            left: obj.bounds.left.0 as i64,
            top: obj.bounds.top.0 as i64,
            width: obj.bounds.width.0 as i64,
            height: obj.bounds.height.0 as i64,
        },
        text,
        field,
        subreport,
        address,
    };
    (obj_ir, warning)
}

/// Project text content + font/color into the IR shape.
fn text_ir(text: &str, display: &str, fc: &model::FontColor, editable: bool) -> TextIr {
    let font = &fc.font;
    let c = &fc.color;
    TextIr {
        text: text.to_string(),
        display: display.to_string(),
        editable,
        font: FontIr {
            name: font.name.clone(),
            size_pt: font.size_pt as f64,
            bold: font.bold,
            italic: font.italic,
            underline: font.underline,
            strikethrough: font.strikethrough,
            weight: font.weight as i64,
        },
        color: ColorIr { r: c.r, g: c.g, b: c.b, a: c.a },
    }
}

/// Find the located section by (area, section) name.
fn find_section(located: &Located, area: &str, section: &str) -> Option<RecAddr> {
    located
        .areas
        .iter()
        .find(|a| a.name == area)
        .and_then(|a| a.sections.iter().find(|s| s.name == section))
        .and_then(|s| s.section_rec)
}

/// Find the located object by (area, section, name). On duplicate names in one section the first
/// wins — object names are unique report-wide in the designer, so this is an edge case at most.
fn find_object<'a>(
    located: &'a Located,
    area: &str,
    section: &str,
    name: &str,
) -> Option<&'a crate::locate::LocatedObject> {
    let s = located
        .areas
        .iter()
        .find(|a| a.name == area)
        .and_then(|a| a.sections.iter().find(|s| s.name == section))?;
    if name.is_empty() {
        return s.objects.iter().find(|o| o.name.is_empty());
    }
    s.objects.iter().find(|o| o.name == name)
}
