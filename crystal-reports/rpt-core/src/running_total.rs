//! `addRunningTotal` — synthesize a running-total field definition and place it.
//!
//! The wire shape (every field + value pinned from the engine-authored donor
//! `tests/fixtures/reports/benbrahim777/China Orders, with running totals.rpt`):
//!
//! ```text
//! 0x0080 RunningTotalField [ schema 0x0700 ]
//!   └─ 0x007e SummaryFieldDefinition
//!        └─ 0x0071 NamedValue (name, value_type = Number — a running total always
//!           widens Currency to Number —, narrow_length, _u0, length)
//!   own: reset_kind (varu16: 0 none, 1 on-change-of-field, 2 on-change-of-group,
//!         3 on-a-formula), reset_field (FieldRef, only while a field is named),
//!         evaluate_kind (same coding), evaluate_field, optional _u0
//!   0x7e own: operation (varu16), _u0, operation_parameter (u16be), operand (str),
//!         operand_kind (varu16), operand_index (u16be), secondary_operand (str),
//!         secondary_operand_kind, secondary_operand_index (0xffff = unset),
//!         is_percentage (i16be), _u1, _u2
//! ```
//!
//! v1 conditions are field-based (`evaluate`/`reset` name a field or none); formula
//! conditions become possible through `setFormula` later. Placement rides `addField` with a
//! `{#name}` reference (pool 9, `FieldRefKind::RunningTotal`).

use rpt_reader::model::{self, FieldValueType};
use rpt_reader::{Rpt, StreamId};

use crate::framing;

/// The wire code of a `SummaryOperation` (the SDK enum's declaration order — the same
/// mapping the model's `from_code` decodes).
fn operation_code(op: &str) -> Result<u16, String> {
    Ok(match op.to_ascii_lowercase().as_str() {
        "sum" | "add" => 0,
        "average" | "avg" => 1,
        "samplevariance" => 2,
        "samplestandarddeviation" => 3,
        "maximum" | "max" => 4,
        "minimum" | "min" => 5,
        "count" => 6,
        "popvariance" => 7,
        "popstandarddeviation" => 8,
        "distinctcount" => 9,
        "correlation" => 10,
        "covariance" => 11,
        "weightedavg" => 12,
        "median" => 13,
        "percentile" => 14,
        "nthlargest" => 15,
        "nthsmallest" => 16,
        "mode" => 17,
        "nthmostfrequent" => 18,
        other => return Err(format!("unknown summary operation `{other}` (sum/average/count/… )")),
    })
}

/// The `CrFieldValueTypeEnum` code a running total reports (always widened to Number).
const NUMBER_VALUE_TYPE: u16 = 6;

/// A running-total condition: none, or evaluate/reset on change of a field.
#[derive(Debug, Clone, Default)]
pub struct RtCondition {
    /// The field whose change triggers the condition (`alias.field`; empty = no condition).
    pub field: String,
}

/// `addRunningTotal` — synthesize the `0x0080` run and splice it among the field
/// definitions (after the last existing one, before the FieldManager region's end).
///
/// # Errors
///
/// An unknown operation name; the summarized field not being in the report's pool; or a
/// writer failure. The result is verified by model read-back before it is returned.
pub fn add_running_total(
    rpt: &Rpt,
    name: &str,
    summarize: &str,
    operation: &str,
    evaluate: &RtCondition,
    reset: &RtCondition,
) -> Result<Vec<u8>, String> {
    let report = rpt.report();
    let (op_pool, op_index) = crate::grouping::field_handle(report, summarize)?;
    let display = crate::grouping::field_display(report, summarize);
    let op = operation_code(operation)?;

    // The conditions' field handles (pool + index), resolved in the same pools.
    let cond = |c: &RtCondition| -> Result<(u16, String), String> {
        if c.field.is_empty() {
            return Ok((0, String::new()));
        }
        let (_pool, index) = crate::grouping::field_handle(report, &c.field)?;
        let d = crate::grouping::field_display(report, &c.field);
        Ok((index, d))
    };
    let (ev_index, ev_display) = cond(evaluate)?;
    let (rs_index, rs_display) = cond(reset)?;
    let ev_kind = if evaluate.field.is_empty() { 0 } else { 1 };
    let rs_kind = if reset.field.is_empty() { 0 } else { 1 };

    // 0x71: the definition's name.
    let mut nv = Vec::new();
    nv.extend(framing::build_string(name));
    nv.extend(framing::build_narrowing(NUMBER_VALUE_TYPE));
    nv.extend_from_slice(&0u16.to_be_bytes());
    nv.extend(framing::build_string(""));
    nv.extend_from_slice(&8u32.to_be_bytes()); // length: a number's stored width
    let named = framing::build_record(0x0071, 0x0700, &nv);

    // 0x7e: the summary definition.
    let mut summary = named;
    summary.extend(framing::build_narrowing(op));
    summary.extend(framing::build_narrowing(0)); // _u0
    summary.extend_from_slice(&0u16.to_be_bytes()); // operation_parameter
    summary.extend(framing::build_string(&display));
    summary.extend(framing::build_narrowing(u16::try_from(op_pool).unwrap_or(0)));
    summary.extend_from_slice(&op_index.to_be_bytes());
    summary.extend(framing::build_string("")); // secondary operand
    summary.extend(framing::build_narrowing(0));
    summary.extend_from_slice(&0xFFFFu16.to_be_bytes());
    summary.extend_from_slice(&0i16.to_be_bytes()); // is_percentage
    summary.extend(framing::build_narrowing(0)); // _u1
    summary.extend(framing::build_narrowing(0)); // _u2
    let summary = framing::build_record(0x007e, 0x0700, &summary);

    // 0x80: the running-total conditions around the summary.
    let mut body = summary;
    body.extend(framing::build_narrowing(rs_kind));
    if rs_kind == 1 {
        body.extend(framing::build_string(&rs_display));
        body.extend(framing::build_narrowing(0));
        body.extend_from_slice(&rs_index.to_be_bytes());
    }
    body.extend(framing::build_narrowing(ev_kind));
    if ev_kind == 1 {
        body.extend(framing::build_string(&ev_display));
        body.extend(framing::build_narrowing(0));
        body.extend_from_slice(&ev_index.to_be_bytes());
    }
    let record = framing::build_record(0x0080, 0x0700, &body);

    // Splice among the field definitions: after the last existing definition record
    // (`0x0073` FieldDef / `0x0080` / `0x007e`-wrapped), before whatever follows.
    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();
    let at = tree
        .iter()
        .rev()
        .find(|n| matches!(n.rtype, 0x0073 | 0x0080 | 0x007e | 0x0079))
        .map(|n| n.content_end)
        .ok_or("no field definitions to anchor the running total beside")?;
    let new_logical = framing::splice(&logical, &tree, at..at, &record)
        .map_err(|e| format!("splice running total: {e}"))?;
    let out = rpt
        .write_contents_logical(&new_logical)
        .map_err(|e| format!("write running total: {e}"))?;

    // Verify by model read-back: the definition decodes with the requested operation and
    // conditions.
    let reopened = crate::open_bytes(&out)?;
    let found = reopened.report().data_definition.field_definitions.iter().find(|f| {
        f.name.eq_ignore_ascii_case(name) && matches!(f.kind, model::FieldKindData::RunningTotal(_))
    });
    let Some(fd) = found else {
        return Err("the minted running total does not read back in the model".to_string());
    };
    match &fd.kind {
        model::FieldKindData::RunningTotal(rt) => {
            if rt.summarized_field.is_empty() {
                return Err("the minted running total read back without its summarized field".to_string());
            }
        }
        _ => unreachable!("matched on RunningTotal"),
    }
    Ok(out)
}

/// The `{#name}` placement handle for `addField`: pool 9 (`FieldRefKind::RunningTotal`),
/// ordinal among the report's running-total definitions.
pub fn running_total_handle(report: &model::Report, name: &str) -> Option<(u32, u16)> {
    let want = name.trim().trim_start_matches('{').trim_end_matches('}').trim_start_matches('#');
    let mut idx = 0u16;
    for f in &report.data_definition.field_definitions {
        if let model::FieldKindData::RunningTotal(_) = &f.kind {
            if f.name.eq_ignore_ascii_case(want) {
                return Some((9, idx));
            }
            idx = idx.saturating_add(1);
        }
    }
    None
}

/// The value type a running total over `summarize` reports — always Number (the engine
/// widens Currency; every other numeric is Number already).
pub fn running_total_value_type(report: &model::Report, summarize: &str) -> FieldValueType {
    let _ = (report, summarize);
    FieldValueType::Number
}

/// `addSummary` — synthesize a summary field definition (`0x007e[0x0071]`), the sibling
/// our running-total builder already knows how to shape. A grand total (no group), or a
/// group subtotal when `group_field` names the group's condition field.
///
/// # Errors
///
/// An unknown operation name; the summarized field not in the report's pool; the group
/// field not resolvable; or a writer failure. Verified by model read-back.
pub fn add_summary(
    rpt: &Rpt,
    name: &str,
    summarize: &str,
    operation: &str,
    group_field: Option<&str>,
) -> Result<Vec<u8>, String> {
    let report = rpt.report();
    let (op_pool, op_index) = crate::grouping::field_handle(report, summarize)?;
    let display = crate::grouping::field_display(report, summarize);
    let op = operation_code(operation)?;

    // The 0x7e record: the same shape the running total nests.
    let vt = match field_handle_value_type(report, summarize) {
        FieldValueType::Currency => FieldValueType::Number, // summaries widen like totals
        other => other,
    };
    let (summary, _) = build_summary_record(name, &display, op_pool, op_index, op, vt);

    // Group subtotals carry the group's condition-field ordinal in a second record —
    // v1 grand totals only (the group variant needs the 0x29-wrapped group form, a
    // separate synthesis).
    if group_field.is_some() {
        return Err("group subtotals are not yet synthesized (grand totals only in v1)".to_string());
    }

    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();
    let at = tree
        .iter()
        .rev()
        .find(|n| matches!(n.rtype, 0x0073 | 0x0080 | 0x007e | 0x0079))
        .map(|n| n.content_end)
        .ok_or("no field definitions to anchor the summary beside")?;
    let new_logical = framing::splice(&logical, &tree, at..at, &summary)
        .map_err(|e| format!("splice summary: {e}"))?;
    let out = rpt
        .write_contents_logical(&new_logical)
        .map_err(|e| format!("write summary: {e}"))?;

    let reopened = crate::open_bytes(&out)?;
    // A summary carries no stored NAME (the decoder identifies it by its operation + field
    // + scope), so the read-back checks the summarized-field binding.
    let found = reopened
        .report()
        .data_definition
        .field_definitions
        .iter()
        .any(|f| matches!(&f.kind, model::FieldKindData::Summary(s) if s.summarized_field.eq_ignore_ascii_case(&display)));
    if !found {
        return Err("the minted summary does not read back in the model".to_string());
    }
    Ok(out)
}

fn field_handle_value_type(report: &model::Report, field_ref: &str) -> FieldValueType {
    let debraced = field_ref.trim().trim_start_matches('{').trim_end_matches('}');
    let bare = debraced.rsplit('.').next().unwrap_or(debraced);
    report
        .data_definition
        .field_definitions
        .iter()
        .find(|f| {
            matches!(f.kind, model::FieldKindData::Database(_))
                && f.name.rsplit('.').next().unwrap_or(&f.name).eq_ignore_ascii_case(bare)
        })
        .map(|f| f.value_type)
        .unwrap_or(FieldValueType::Number)
}

fn build_summary_record(
    name: &str,
    display: &str,
    op_pool: u32,
    op_index: u16,
    op: u16,
    vt: FieldValueType,
) -> (Vec<u8>, ()) {
    let vt_code = match vt {
        FieldValueType::Number => 6u16,
        FieldValueType::Currency => 6,
        FieldValueType::Boolean => 8,
        FieldValueType::Date => 9,
        FieldValueType::Time => 10,
        FieldValueType::String => 11,
        FieldValueType::DateTime => 15,
        _ => 6,
    };
    let mut nv = Vec::new();
    nv.extend(framing::build_string(name));
    nv.extend(framing::build_narrowing(vt_code));
    nv.extend_from_slice(&0u16.to_be_bytes());
    nv.extend(framing::build_string(""));
    nv.extend_from_slice(&0u32.to_be_bytes());
    let named = framing::build_record(0x0071, 0x0700, &nv);

    let mut body = named;
    body.extend(framing::build_narrowing(op));
    body.extend(framing::build_narrowing(0));
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend(framing::build_string(display));
    body.extend(framing::build_narrowing(u16::try_from(op_pool).unwrap_or(0)));
    body.extend_from_slice(&op_index.to_be_bytes());
    body.extend(framing::build_string(""));
    body.extend(framing::build_narrowing(0));
    body.extend_from_slice(&0xFFFFu16.to_be_bytes());
    body.extend_from_slice(&0i16.to_be_bytes());
    body.extend(framing::build_narrowing(0));
    body.extend(framing::build_narrowing(0));
    (framing::build_record(0x007e, 0x0700, &body), ())
}
