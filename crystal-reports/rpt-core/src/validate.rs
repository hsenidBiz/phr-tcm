//! The offline load-path validator — "will the real engine accept this file?".
//!
//! Every check here re-implements an invariant the actual Crystal engine enforces when opening
//! a `.rpt`, re-implemented from the engine's observable load-path contract and its
//! error catalog. Each finding quotes the engine's own assert or error string so a rejection
//! is traceable to its source. This is the offline stand-in for the blocked designer smoke
//! test: our writer and reader share the same format assumptions, so an independent opinion
//! is the only way a shared blind spot surfaces before a user hits "cannot open file".
//!
//! Severity policy (documented judgment): **Error** = the PE-open rejection class (the error
//! catalog's "Invalid …" strings); **Warn** = engine-tolerant-but-suspect, or checks that are
//! heuristic offline (display-text reference resolution — the engine resolves pool handles,
//! not strings). Asserts are debug-build expressions: they name the invariants, but release
//! tolerances may differ, so ambiguous cases downgrade to Warn.

use rpt_reader::model;
use rpt_reader::{fields, Rpt, StreamId};

use crate::locate;

/// A validation finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Severity: Error = PE-open rejection class; Warn = suspect but likely tolerated.
    pub severity: Severity,
    /// The check that produced the finding (stable name, e.g. `special_fields`).
    pub check: &'static str,
    /// What is wrong, naming the object/section in IR terms.
    pub message: String,
    /// The engine's own assert text or PE error string this check re-implements.
    pub engine: &'static str,
}

/// Finding severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The PE-open rejection class — the engine would refuse or garble the file.
    Error,
    /// Suspect but tolerated by the engine, or a heuristic offline check.
    Warn,
}

/// The validator's result.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Every finding, in check order.
    pub findings: Vec<Finding>,
}

impl Report {
    /// Only the Error-severity findings.
    pub fn errors(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity == Severity::Error)
    }

    /// Only the Warn-severity findings.
    pub fn warnings(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity == Severity::Warn)
    }
}

struct Checker {
    report: Report,
}

impl Checker {
    fn error(&mut self, check: &'static str, message: String, engine: &'static str) {
        self.report.findings.push(Finding { severity: Severity::Error, check, message, engine });
    }

    fn warn(&mut self, check: &'static str, message: String, engine: &'static str) {
        self.report.findings.push(Finding { severity: Severity::Warn, check, message, engine });
    }
}

/// Run every first-wave check. Cheap: one model walk plus one locate pass.
pub fn validate(rpt: &Rpt) -> Report {
    let mut c = Checker { report: Report::default() };
    let report = rpt.report();
    check_version_gate(&mut c, report);
    check_stream_fully_parsed(rpt, &mut c);
    check_special_fields(rpt, &mut c);
    check_areas_and_groups(&mut c, report);
    check_object_bounds(&mut c, report);
    check_border_and_fill(rpt, &mut c);
    check_value_types(&mut c, report);
    let resolver = Resolver::of(report);
    check_reference_resolution(&mut c, report, &resolver);
    check_subreports(&mut c, report, &resolver);
    check_tables(&mut c, report);
    check_page_setup(&mut c, report);
    check_saved_data_flag(&mut c, report);
    check_summary_operations(&mut c, report);
    c.report
}

/* ---------- 1. version gate ---------- */

fn check_version_gate(c: &mut Checker, report: &model::Report) {
    const NEWER: &str = "Error loading report database - newer version than expected. \
                         Please run a more recent version of the application.";
    let v = &report.authoring_version;
    if v.major > 11 {
        c.error(
            "version_gate",
            format!("authored by {}.{}, newer than this engine (11.5)", v.major, v.minor),
            NEWER,
        );
    } else if v.major < 3 {
        c.warn(
            "version_gate",
            format!("legacy authoring version {}.{}, below the 4.x path", v.major, v.minor),
            "Not a 4.x or earlier version of Crystal Reports.",
        );
    }
    if v.letter != 0 {
        c.warn(
            "version_gate",
            format!("version letter {} is non-zero (0 on every report this engine writes)", v.letter),
            "reportVersion >= curDocVersion (version-word assert family)",
        );
    }
}

/* ---------- 2. record tree parses fully ---------- */

fn check_stream_fully_parsed(rpt: &Rpt, c: &mut Checker) {
    const ENGINE: &str = "tag == TTReportVersion; length == sizeof (VersionInfo) — load-path \
                          record framing must account for every byte";
    if let Some(stream) = rpt.stream(&StreamId::Contents) {
        if !stream.is_fully_parsed() {
            if let Some(why) = stream.decode_error() {
                c.error("stream_parsed", format!("Contents did not decode fully: {why}"), ENGINE);
            } else {
                c.error("stream_parsed", "Contents did not decode fully".to_string(), ENGINE);
            }
        }
    } else {
        c.error("stream_parsed", "no Contents stream".to_string(), ENGINE);
    }
}

/* ---------- 3. special-field index range ---------- */

fn check_special_fields(rpt: &Rpt, c: &mut Checker) {
    const ENGINE: &str = "the engine's 26-entry special-field table (Invalid special var type)";
    let located = locate::locate(rpt);
    for area in &located.areas {
        for sec in &area.sections {
            for obj in &sec.objects {
                let Some(addr) = obj.opener else { continue };
                let Ok(f) = rpt.record_field(
                    rpt_reader::raw::RecordTag(addr.tag),
                    addr.nth,
                    "data_source",
                ) else {
                    continue;
                };
                if let fields::FieldValue::FieldRef { name, kind, index } = &f.value {
                    if *kind == 3 {
                        let idx = index.unwrap_or(u16::MAX);
                        if idx > 0x19 {
                            c.error(
                                "special_fields",
                                format!(
                                    "field object `{}` references special #{idx} ({name}), \
                                     outside the engine's 0..=0x11 range",
                                    obj.name
                                ),
                                ENGINE,
                            );
                        }
                    }
                }
            }
        }
    }
}

/* ---------- 4. areas / sections / group structure ---------- */

fn check_areas_and_groups(c: &mut Checker, report: &model::Report) {
    const EMPTY: &str = "sections.size() > 0 — every area carries at least one section";
    const LEVELS: &str = "groupLevel == getNGroupAreaPairs () + 1 — group areas and group \
                          definitions must be one-to-one and densely levelled";
    let groups = report.data_definition.groups.len();
    let mut header_levels: Vec<usize> = Vec::new();
    let mut footer_count = 0usize;
    for area in &report.report_definition.areas {
        if area.sections.is_empty() {
            c.error("areas_sections", format!("area `{}` has no sections", area.name), EMPTY);
        }
        match area.kind {
            model::AreaSectionKind::GroupHeader => {
                header_levels.push(area.group_level.unwrap_or(usize::MAX));
            }
            model::AreaSectionKind::GroupFooter => footer_count += 1,
            _ => {}
        }
    }
    if groups > 0 || !header_levels.is_empty() {
        let mut sorted = header_levels.clone();
        sorted.sort_unstable();
        let dense = sorted.iter().enumerate().all(|(i, l)| *l == i);
        if !dense {
            c.error(
                "group_structure",
                format!("group-header levels {header_levels:?} are not a dense 0..n sequence"),
                LEVELS,
            );
        }
        if header_levels.len() != groups {
            c.error(
                "group_structure",
                format!("{} group-header area(s) for {} group definition(s)", header_levels.len(), groups),
                LEVELS,
            );
        }
        if footer_count != groups {
            c.error(
                "group_structure",
                format!("{} group-footer area(s) for {} group definition(s)", footer_count, groups),
                LEVELS,
            );
        }
    }
}

/* ---------- 5. object bounds sanity ---------- */

fn check_object_bounds(c: &mut Checker, report: &model::Report) {
    const INT16: &str =
        "textObject->getHeight()/getWidth()/getYOffsetInSection() <= MaxInt16s && >= MinInt16s";
    const OVERHANG: &str = "twipSize <= MaxInt16u && twipSize >= 0 (page-fit family)";
    let content_width = report.print_options.content_width.0;
    for area in &report.report_definition.areas {
        for sec in &area.sections {
            for obj in &sec.objects {
                let b = &obj.bounds;
                for (name, v) in [("left", b.left.0), ("top", b.top.0), ("width", b.width.0), ("height", b.height.0)] {
                    if !(-32768..=32767).contains(&v) {
                        c.error(
                            "object_bounds",
                            format!("object `{}` {} = {v} twips exceeds the Int16 range", obj.name, name),
                            INT16,
                        );
                    }
                }
                if b.width.0 < 0 || b.height.0 < 0 {
                    c.error(
                        "object_bounds",
                        format!("object `{}` has negative width/height", obj.name),
                        OVERHANG,
                    );
                }
                // Page overhang is a Warn: the engine grows sections rather than rejecting.
                if content_width > 0 && b.left.0 + b.width.0 > content_width + 1440 {
                    c.warn(
                        "object_bounds",
                        format!(
                            "object `{}` extends {} twips past the content width",
                            obj.name,
                            b.left.0 + b.width.0 - content_width
                        ),
                        OVERHANG,
                    );
                }
            }
        }
    }
}

/* ---------- 6. border / fill enums ---------- */

fn check_border_and_fill(rpt: &Rpt, c: &mut Checker) {
    const EDGES: &str =
        "{top,bottom,right,left} >= RDNoLine && <= RDDotLine (PE: Invalid line style)";
    const FILL: &str = "fillStyle >= RDNullFill && fillStyle <= RDPatternFill";
    let located = locate::locate(rpt);
    for area in &located.areas {
        for sec in &area.sections {
            for obj in &sec.objects {
                if let Some(addr) = obj.border {
                    for edge in ["left_line_style", "right_line_style", "top_line_style", "bottom_line_style"] {
                        if let Ok(f) = rpt.record_field(rpt_reader::raw::RecordTag(addr.tag), addr.nth, edge) {
                            if let fields::FieldValue::Uint(v) = f.value {
                                if v > 4 {
                                    c.error(
                                        "border_enums",
                                        format!("object `{}` {} = {v}, outside 0..=4", obj.name, edge),
                                        EDGES,
                                    );
                                }
                            }
                        }
                    }
                    if let Ok(f) = rpt.record_field(rpt_reader::raw::RecordTag(addr.tag), addr.nth, "fill_style") {
                        if let fields::FieldValue::Uint(v) = f.value {
                            if v > 5 {
                                c.error(
                                    "border_enums",
                                    format!("object `{}` fill_style = {v}, outside the engine's fill range", obj.name),
                                    FILL,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/* ---------- 7. value-type range ---------- */

fn check_value_types(c: &mut Checker, report: &model::Report) {
    const ENGINE: &str =
        "(valueType >= int8sField && valueType <= dateTimeField) || (valueType >= bitmapField \
         && valueType <= chartField) || valueType == sameAsInputField || valueType == unknownField \
         (PE: Invalid value type)";
    let objects = report.report_definition.areas.iter()
        .flat_map(|a| a.sections.iter().flat_map(|s| s.objects.iter()));
    for obj in objects {
        if let model::ReportObjectKind::Field(f) = &obj.kind {
            if matches!(f.value_type, model::FieldValueType::Other(_)) {
                c.error(
                    "value_types",
                    format!("field object `{}` ({}) has a value type outside the engine's union", obj.name, f.data_source),
                    ENGINE,
                );
            }
        }
    }
    for def in &report.data_definition.field_definitions {
        if matches!(def.value_type, model::FieldValueType::Other(_)) {
            c.error(
                "value_types",
                format!("field definition `{}` has a value type outside the engine's union", def.name),
                ENGINE,
            );
        }
    }
}

/* ---------- 8. reference resolution (heuristic: Warn) ---------- */

/// Name sets references can resolve against. Display-text matching only — the engine resolves
/// pool handles, so unresolved here is *suspect*, not fatal (hence Warn).
struct Resolver {
    db_fields: Vec<String>,          // "{alias.field}"
    formulas: Vec<String>,           // "@name" / bare
    parameters: Vec<String>,         // "?name" / "@name" / bare
    specials: Vec<String>,           // spaceless canonical names
    summaries: Vec<String>,
    group_names: Vec<String>,
}

impl Resolver {
    fn of(report: &model::Report) -> Resolver {
        let mut r = Resolver {
            db_fields: Vec::new(),
            formulas: Vec::new(),
            parameters: Vec::new(),
            specials: crate::edit::special_names(),
            summaries: Vec::new(),
            group_names: Vec::new(),
        };
        for t in &report.database.tables {
            for f in &t.data_fields {
                r.db_fields.push(format!("{}.{}", t.alias, f.name));
            }
        }
        for def in &report.data_definition.field_definitions {
            match &def.kind {
                model::FieldKindData::Database(_) => {}
                model::FieldKindData::Formula(_) => r.formulas.push(def.name.trim_start_matches('@').to_string()),
                model::FieldKindData::Parameter(_) => r.parameters.push(def.name.trim_start_matches('@').to_string()),
                model::FieldKindData::Summary(_) => r.summaries.push(def.name.clone()),
                _ => {}
            }
        }
        for g in &report.data_definition.groups {
            r.group_names.push(g.condition_field.clone());
        }
        r
    }

    fn resolves(&self, raw: &str) -> bool {
        let s = raw.trim();
        // Summary/GroupName display forms ("Sum ({a}, {b})", "GroupName ({x})") reduce to
        // their first braced operand; percentages carry "PercentOf..." prefixes likewise.
        if !s.starts_with('{') {
            if let Some(open) = s.find('{') {
                if let Some(close) = s[open..].find('}') {
                    return self.resolves(&s[open..open + close + 1]);
                }
            }
        }
        let debraced = s.trim_start_matches('{').trim_end_matches('}');
        let bare = debraced.rsplit('.').next().unwrap_or(debraced);
        if let Some(p) = debraced.strip_prefix('@') {
            return self.formulas.iter().any(|f| f.eq_ignore_ascii_case(p));
        }
        if let Some(p) = debraced.strip_prefix('?') {
            return self.parameters.iter().any(|f| f.eq_ignore_ascii_case(p));
        }
        if debraced.starts_with('#') {
            return true; // running totals: pool-resolved, no offline truth
        }
        if self.specials.iter().any(|n| n.eq_ignore_ascii_case(&debraced.replace(' ', ""))) {
            return true;
        }
        if self.summaries.iter().any(|s| s.eq_ignore_ascii_case(bare)) {
            return true;
        }
        if debraced.contains('.') {
            return self.db_fields.iter().any(|f| f.eq_ignore_ascii_case(debraced));
        }
        // Bare name: any pool.
        self.db_fields.iter().any(|f| f.rsplit('.').next() == Some(bare))
            || self.formulas.iter().any(|f| f.eq_ignore_ascii_case(bare))
            || self.parameters.iter().any(|f| f.eq_ignore_ascii_case(bare))
            || self.group_names.iter().any(|g| g.eq_ignore_ascii_case(bare))
    }
}

fn check_reference_resolution(c: &mut Checker, report: &model::Report, resolver: &Resolver) {
    const ENGINE: &str =
        "recordSortFields.indexOf (sortField) != -1 || groupSortFields.indexOf (sortField) != -1 \
         (PE: Invalid field name/number)";
    let flag = |what: &str, raw: &str, c: &mut Checker| {
        if !raw.trim().is_empty() && !resolver.resolves(raw) {
            c.warn("references", format!("{what} `{raw}` resolves against no field pool"), ENGINE);
        }
    };
    for area in &report.report_definition.areas {
        for sec in &area.sections {
            for obj in &sec.objects {
                if let model::ReportObjectKind::Field(f) = &obj.kind {
                    // Specials and GroupName render through canonical names; strip braces for them.
                    let raw = match f.ref_kind {
                        model::FieldRefKind::Special | model::FieldRefKind::GroupName => {
                            f.data_source.trim_start_matches('{').trim_end_matches('}').to_string()
                        }
                        _ => f.data_source.clone(),
                    };
                    if f.ref_kind != model::FieldRefKind::Special {
                        flag("field object data source", &raw, c);
                    }
                }
                if let model::ReportObjectKind::Text(t) = &obj.kind {
                    for emb in &t.embedded_fields {
                        flag("embedded field", emb, c);
                    }
                }
            }
        }
    }
    for g in &report.data_definition.groups {
        flag("group condition field", &g.condition_field, c);
    }
    for s in &report.data_definition.record_sorts {
        flag("sort field", &s.field, c);
    }
    // Selection-formula BODIES are skipped: they are free-form formula language, not bare
    // references — tokenizing them is wave-2 (formula slot work).
}

/* ---------- 10. subreport integrity ---------- */

fn check_subreports(c: &mut Checker, report: &model::Report, resolver: &Resolver) {
    const LINKS: &str =
        "subreportLinkN < linkObject->getNPromptingVarLinkItems (); subreportDataSources.indexOf \
         (dependee) >= 0 (PE: Invalid subreport name / link number)";
    const COUNT: &str = "subreports.size () <= MaxInt16u";
    if report.subreports.len() > 32_767 {
        c.error("subreports", format!("{} subreports exceeds the engine cap", report.subreports.len()), COUNT);
    }
    for sub in &report.subreports {
        // Sub-side resolvers against the nested model.
        let mut sub_db: Vec<String> = Vec::new();
        for t in &sub.report.database.tables {
            for f in &t.data_fields {
                sub_db.push(format!("{}.{}", t.alias, f.name));
            }
        }
        let sub_params: Vec<String> = sub
            .report
            .data_definition
            .field_definitions
            .iter()
            .filter(|d| matches!(d.kind, model::FieldKindData::Parameter(_)))
            .map(|d| d.name.trim_start_matches('@').to_string())
            .collect();
        for link in &sub.links {
            if !resolver.resolves(&link.main_report_field) {
                c.warn(
                    "subreports",
                    format!(
                        "link main field `{}` of subreport `{}` resolves in no main-report pool",
                        link.main_report_field, sub.name
                    ),
                    LINKS,
                );
            }
            let sub_ref = link.subreport_field.trim_start_matches('{').trim_end_matches('}');
            if !sub_ref.is_empty()
                && !sub_db.iter().any(|f| f.eq_ignore_ascii_case(sub_ref))
                && !sub_params.iter().any(|p| sub_ref.eq_ignore_ascii_case(p))
            {
                c.warn(
                    "subreports",
                    format!(
                        "link subreport field `{}` of subreport `{}` resolves in no subreport pool",
                        link.subreport_field, sub.name
                    ),
                    LINKS,
                );
            }
            if let Some(p) = &link.linked_parameter {
                let bare = p.trim_start_matches('@');
                if !sub_params.iter().any(|q| q.eq_ignore_ascii_case(bare)) {
                    c.warn(
                        "subreports",
                        format!(
                            "link parameter `{}` is not a parameter of subreport `{}`",
                            p, sub.name
                        ),
                        LINKS,
                    );
                }
            }
        }
    }
    // Placeholder pairing: every Subreport object's subreport_name must name a subreport.
    let objects = report.report_definition.areas.iter()
        .flat_map(|a| a.sections.iter().flat_map(|s| s.objects.iter()));
    for obj in objects {
        if let model::ReportObjectKind::Subreport(sr) = &obj.kind {
            if !report.subreports.iter().any(|s| s.name == sr.subreport_name) {
                c.error(
                    "subreports",
                    format!(
                        "placeholder `{}` names subreport `{}` which does not exist",
                        obj.name, sr.subreport_name
                    ),
                    LINKS,
                );
            }
        }
    }
}

/* ---------- 11. tables ---------- */

fn check_tables(c: &mut Checker, report: &model::Report) {
    const UNIQUE: &str = "lookupTableByAlias (tableAlias) == NULL — table aliases are unique";
    const CAP: &str = "dataInterface->getNTablesInReport() <= MaxInt16u";
    if report.database.tables.len() > 32_767 {
        c.error("table_aliases", "too many tables (> 32767)".to_string(), CAP);
    }
    let mut seen: Vec<&str> = Vec::new();
    for t in &report.database.tables {
        if seen.iter().any(|s| s.eq_ignore_ascii_case(&t.alias)) {
            c.error(
                "table_aliases",
                format!("table alias `{}` is used more than once", t.alias),
                UNIQUE,
            );
        }
        seen.push(&t.alias);
    }
}

/* ---------- 12. page setup ---------- */

fn check_page_setup(c: &mut Checker, report: &model::Report) {
    const MARGINS: &str = "pageMargins != 0; oldMargins.left != InvalidTwip (PE: Invalid page margins)";
    const KIND: &str = "reportKind >= RDColumnarReport && reportKind <= RDMultiColumnReport";
    let m = &report.print_options.margins;
    for (name, v) in [("left", m.left.0), ("right", m.right.0), ("top", m.top.0), ("bottom", m.bottom.0)] {
        if !(0..=14_400).contains(&v) {
            c.error(
                "page_setup",
                format!("{} margin = {v} twips, outside 0..=14400 (10 inches)", name),
                MARGINS,
            );
        }
    }
    if report.print_options.content_width.0 < 0 || report.print_options.content_height.0 < 0 {
        c.error("page_setup", "negative content dimensions".to_string(), KIND);
    }
}

/* ---------- 13. saved-data flag ---------- */

fn check_saved_data_flag(c: &mut Checker, report: &model::Report) {
    const ENGINE: &str = "bValidSavedRecords == TRUE — the saved-data flag and the saved rows \
                           must agree";
    match (report.has_saved_data, report.saved_data.is_some()) {
        (true, false) => c.error(
            "saved_data",
            "the report claims saved data but none decoded".to_string(),
            ENGINE,
        ),
        (false, true) => c.error(
            "saved_data",
            "saved rows decoded but the report claims none".to_string(),
            ENGINE,
        ),
        _ => {}
    }
}

/* ---------- 14. summary-operation range (wave 2) ---------- */

fn check_summary_operations(c: &mut Checker, report: &model::Report) {
    const ENGINE: &str = "summaryOperation bounds (the SDK enum has 20 members)";
    let scan = |c: &mut Checker, defs: &[model::FieldDef], scope: &str| {
        for f in defs {
            let op = match &f.kind {
                model::FieldKindData::Summary(s) => &s.operation,
                model::FieldKindData::RunningTotal(r) => &r.operation,
                _ => continue,
            };
            if matches!(op, model::SummaryOperation::Other(_)) {
                c.error(
                    "summary_operation_range",
                    format!("{scope} field `{}` carries an operation outside the engine's enum (decoded as Other)", f.name),
                    ENGINE,
                );
            }
        }
    };
    scan(c, &report.data_definition.field_definitions, "main");
    for sub in &report.subreports {
        scan(c, &sub.report.data_definition.field_definitions, "subreport");
    }
}

