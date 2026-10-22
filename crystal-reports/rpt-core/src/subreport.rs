//! `addSubreport` and `setSubreportLink` — the subreport half of the cross-stream tier.
//!
//! A subreport is two things in the file: a `Subdocument N` **storage** in the container,
//! holding the imported report's own streams verbatim (its `Contents` root record carries the
//! subreport's name in `document_name`), and a **placeholder object** (`0xa3`) in the main
//! `Contents` whose `subdocument_index` points at that storage. Importing is therefore a
//! container copy plus a donor-cloned placeholder with the index patched.
//!
//! A link is one `0x0106 SubreportLink` record in the main `Contents`, a later sibling of the
//! `0xa3` it belongs to: the subreport parameter's index, the main field's name and pool handle,
//! a flag word, and the subreport-side field's pool handle.

use rpt_reader::raw::{Dialect, RecordTag};
use rpt_reader::{fields, fields::FieldEdit, model, Rpt, StreamId};

use crate::framing;
use crate::structure;

/// `0x64` — the report root record (carries `document_name`, which is the subreport's name).
const T_REPORT_ROOT: u16 = 0x0064;
/// `0xa3` — the subreport placeholder opener.
const T_SUBREPORT_OBJECT: u16 = 0x00a3;
/// `0x106` — one main-report-to-subreport link record.
const T_SUBREPORT_LINK: u16 = 0x0106;
/// `0x7a` — a parameter declaration (carries the engine parameter index).
const T_PARAMETER: u16 = 0x007a;

/// `addSubreport`: import `report_path` under a fresh `Subdocument N`, name it `name`, and
/// place a placeholder object (donor-cloned) at the requested geometry.
pub fn add_subreport(
    rpt: &Rpt,
    section: &crate::edit::SecSel,
    name: &str,
    report_path: &str,
    left: i64,
    top: i64,
    width: i64,
    height: i64,
) -> Result<Vec<u8>, String> {
    if !std::path::Path::new(report_path).exists() {
        return Err(format!("subreport file not found: `{report_path}`"));
    }
    let source_bytes = std::fs::read(report_path)
        .map_err(|e| format!("cannot read `{report_path}`: {e}"))?;

    // 1. Name the imported report: its root record's document_name becomes the subreport name
    //    (the engine resolves SubreportObject.SubreportName from there).
    let source_rpt = crate::open_bytes(&source_bytes)?;
    let named = patch_forced(
        &source_rpt
            .reencode()
            .map_err(|e| format!("reencode the subreport: {e}"))?,
        RecordTag(T_REPORT_ROOT),
        0,
        "document_name",
        FieldEdit::Text(name.to_string()),
    )?;

    // 2. Copy the (renamed) report's streams under a fresh Subdocument N.
    let index = structure::max_subdocument_index(rpt) + 1;
    let with_subdoc = structure::import_subdocument(rpt.original_bytes(), index, &named)?;

    // 3. Place the placeholder: donor-clone an existing subreport object in the target, then
    //    patch its subdocument index, name and geometry.
    let opened = crate::open_bytes(&with_subdoc)?;
    let located = crate::locate::locate(&opened);
    if !located
        .areas
        .iter()
        .any(|a| a.sections.iter().any(|s| s.objects.iter().any(|o| o.opener_tag == T_SUBREPORT_OBJECT)))
    {
        return Err(
            "this report has no Subreport object to clone a placeholder from; placing the first \
             needs a synthesized opener (not supported yet)"
                .to_string(),
        );
    }
    let finished = crate::edit::add_object_inner(
        &opened,
        section,
        name,
        left,
        top,
        width,
        height,
        |obj| obj.opener_tag == T_SUBREPORT_OBJECT,
        &mut |current, addrs| {
            patch_on(
                current,
                RecordTag(addrs.opener.tag),
                addrs.opener.nth,
                "subdocument_index",
                FieldEdit::Int(i64::from(index)),
            )
        },
    )?;

    // 4. Verify: the reopened model lists the new subreport under this name, and the placed
    //    placeholder resolves to it.
    let check = crate::open_bytes(&finished)?;
    let report = check.report();
    if !report.subreports.iter().any(|s| s.name == name) {
        return Err(format!(
            "the imported subreport `{name}` did not register (Subdocument {index} unreadable?)"
        ));
    }
    Ok(finished)
}

/// `setSubreportLink`: append a `0x106` link record after the target subreport's placeholder.
#[allow(clippy::too_many_arguments)]
/* ---------------- helpers ---------------- */

/// The subdocument storage index of the named subreport, located from the placeholder objects.
fn subdoc_index_of(rpt: &Rpt, subreport_name: &str) -> Option<u32> {
    let report = rpt.report();
    let obj_name = report
        .report_definition
        .areas
        .iter()
        .flat_map(|a| a.sections.iter().flat_map(|s| s.objects.iter()))
        .find_map(|o| match &o.kind {
            model::ReportObjectKind::Subreport(sr) if sr.subreport_name.eq_ignore_ascii_case(subreport_name) => {
                Some(sr.subdoc_index)
            }
            _ => None,
        })?;
    Some(obj_name)
}

fn read_subdoc_index(rpt: &Rpt, obj: &crate::locate::LocatedObject) -> Result<u32, String> {
    let opener = obj.opener.ok_or("subreport object has no opener")?;
    let f = rpt
        .record_field(RecordTag(opener.tag), opener.nth, "subdocument_index")
        .map_err(|e| format!("reading subdocument_index: {e}"))?;
    match f.value {
        fields::FieldValue::Uint(v) => Ok(v),
        other => Err(format!("subdocument_index is not an integer: {other:?}")),
    }
}

/// The engine parameter index of `parameter` in the given subdocument, from its own `0x7a`
/// records (name matched on the `@name`/bare name the records carry).
fn subreport_parameter_index(rpt: &Rpt, subdoc: u32, parameter: &str) -> Option<u16> {
    let id = StreamId::Other(format!("Subdocument {subdoc}/Contents"));
    let raw = rpt.stream(&id)?;
    // Nested streams are stored raw (undecrypted); decode the payload explicitly, as the
    // reader's own subreport path does.
    let decoded = rpt_reader::raw::RecordStream::decode(StreamId::Contents, raw.raw_bytes());
    let logical = decoded.logical_bytes().to_vec();
    let tree = decoded.record_tree();
    let want = parameter.trim_start_matches('@');
    for node in &tree {
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if n.rtype == T_PARAMETER {
                let idx = fields::read(n, &logical, Dialect::Contents).and_then(|read| {
                    read.fields
                        .iter()
                        .find(|f| f.name == "parameter_index")
                        .and_then(|f| match f.value {
                            fields::FieldValue::Uint(v) => Some(v as u16),
                            _ => None,
                        })
                });
                // The parameter's own name is the `0x71` NamedValue child's `name` field
                // ("@performance_cycle_id"); the record's own string is the human prompt text.
                let name = n.children.iter().find(|c| c.rtype == 0x0071).and_then(|child| {
                    fields::read(child, &logical, Dialect::Contents).and_then(|read| {
                        read.fields
                            .iter()
                            .find(|f| f.name == "name")
                            .and_then(|f| match &f.value {
                                fields::FieldValue::Text(t) => Some(t.clone()),
                                _ => None,
                            })
                    })
                });
                if let (Some(idx), Some(name)) = (idx, name) {
                    let bare = name.trim_start_matches('@');
                    if bare.eq_ignore_ascii_case(want) {
                        return Some(idx);
                    }
                }
            }
            stack.extend(n.children.iter().rev());
        }
    }
    None
}

/// A field's (pool, index) handle inside the subreport's own report model — the same ordinal
/// rule as the main report's pools.
fn subreport_field_handle(sub: &model::Subreport, field: &str) -> Option<(u32, u16)> {
    let debraced = field.trim().trim_start_matches('{').trim_end_matches('}');
    let bare = debraced.rsplit('.').next().unwrap_or(debraced);
    let formula_name = debraced.strip_prefix('@').unwrap_or(debraced);
    let mut db_idx = 0u16;
    let mut fx_idx = 0u16;
    for f in &sub.report.data_definition.field_definitions {
        match &f.kind {
            model::FieldKindData::Database(_) => {
                if f.name.eq_ignore_ascii_case(bare) {
                    return Some((0, db_idx));
                }
                db_idx = db_idx.saturating_add(1);
            }
            model::FieldKindData::Formula(_) => {
                if f.name.eq_ignore_ascii_case(formula_name) {
                    return Some((1, fx_idx));
                }
                fx_idx = fx_idx.saturating_add(1);
            }
            _ => {}
        }
    }
    None
}

fn patch_on(bytes: &[u8], addr: rpt_reader::raw::RecordTag, nth: usize, field: &str, value: FieldEdit) -> Result<Vec<u8>, String> {
    let rpt = crate::open_bytes(bytes)?;
    rpt.patch_record_field_with(addr, nth, field, &value, rpt_reader::EditPolicy::Checked)
        .map_err(|e| format!("patch {field}: {e}"))
}

/// A forced field patch with our own read-back verification — for records whose tables do not
/// reproduce them byte-for-byte (the report root carries unmodelled trailing strings).
fn patch_forced(bytes: &[u8], tag: RecordTag, nth: usize, field: &str, value: FieldEdit) -> Result<Vec<u8>, String> {
    let rpt = crate::open_bytes(bytes)?;
    let out = rpt
        .patch_record_field_with(tag, nth, field, &value, rpt_reader::EditPolicy::Forced)
        .map_err(|e| format!("patch {field}: {e}"))?;
    let written = crate::open_bytes(&out)?;
    let read = written
        .record_field(tag, nth, field)
        .map_err(|e| format!("verify {field}: {e}"))?;
    let ok = match (&value, &read.value) {
        (FieldEdit::Text(s), fields::FieldValue::Text(t)) => s == t,
        _ => false,
    };
    if ok {
        Ok(out)
    } else {
        Err(format!("{field} did not read back ({:?})", read.value))
    }
}

/* ---------------- sub-side binding (Record Selection formula) ---------------- */

/// The main report's declared type for a field, by display name (bare `field` of
/// `alias.field`), for typing a synthesized `Pm-` parameter like the field it mirrors.
fn main_field_value_type(report: &rpt_reader::model::Report, display: &str) -> rpt_reader::model::FieldValueType {
    let bare = display.rsplit('.').next().unwrap_or(display);
    report
        .data_definition
        .field_definitions
        .iter()
        .find(|f| {
            matches!(f.kind, rpt_reader::model::FieldKindData::Database(_))
                && f.name.rsplit('.').next().unwrap_or(&f.name).eq_ignore_ascii_case(bare)
        })
        .map(|f| f.value_type)
        .unwrap_or(rpt_reader::model::FieldValueType::String)
}

/// The `CrFieldValueTypeEnum` wire code for a field type (`0` Int8s … `15` DateTime; the
/// codes `FieldValueType::from_code` decodes).
fn value_type_code(vt: rpt_reader::model::FieldValueType) -> u16 {
    use rpt_reader::model::FieldValueType;
    match vt {
        FieldValueType::Int8s => 0,
        FieldValueType::Int16s => 2,
        FieldValueType::Int32s => 4,
        FieldValueType::Int32u => 5,
        FieldValueType::Number => 6,
        FieldValueType::Currency => 7,
        FieldValueType::Boolean => 8,
        FieldValueType::Date => 9,
        FieldValueType::Time => 10,
        FieldValueType::String => 11,
        FieldValueType::PersistentMemo => 13,
        FieldValueType::Blob => 14,
        FieldValueType::DateTime => 15,
        _ => 11,
    }
}

/// The next free ENGINE parameter index in the subdocument (one past the highest existing).
fn next_subdoc_param_index(rpt: &Rpt, subdoc: u32) -> u16 {
    let stream_id = StreamId::Other(format!("Subdocument {subdoc}/Contents"));
    let Some(raw) = rpt.stream(&stream_id) else { return 0 };
    let decoded = rpt_reader::raw::RecordStream::decode(StreamId::Contents, raw.raw_bytes());
    let logical = decoded.logical_bytes();
    let mut next = 0u16;
    for n in decoded.record_tree().iter().filter(|n| n.rtype == 0x007a) {
        if let Some(read) = rpt_reader::fields::read(n, logical, rpt_reader::raw::Dialect::Contents) {
            if let Some(rpt_reader::fields::FieldValue::Uint(v)) =
                read.fields.iter().find(|x| x.name == "parameter_index").map(|x| x.value.clone())
            {
                next = next.max(u16::try_from(v).unwrap_or(0).saturating_add(1));
            }
        }
    }
    next
}

/// Build a `Pm-<main field>` parameter record (`0x007a`, schema 0x0701): the field table's
/// required prefix — name record, engine index, prompt, empty bound-field handle, type, no
/// values, no bounds, empty edit mask, report-parameter kind. The position the engine writes
/// parameters (immediately before `FieldManagerEnd 0x0070`) is the caller's splice anchor.
fn build_pm_parameter_run(pm_name: &str, vt: u16, parameter_index: u16) -> Vec<u8> {
    let mut nv = Vec::new();
    nv.extend(framing::build_string(pm_name));
    nv.extend(framing::build_narrowing(vt));
    nv.extend_from_slice(&0u16.to_be_bytes()); // narrow_length
    nv.extend(framing::build_string(""));
    nv.extend_from_slice(&0u32.to_be_bytes()); // length
    let named = framing::build_record(0x0071, 0x0700, &nv);

    let mut body = named;
    body.extend_from_slice(&parameter_index.to_be_bytes());
    body.extend(framing::build_string(pm_name)); // prompt_text — the engine's own form
    body.extend_from_slice(&0i16.to_be_bytes()); // _u0
    body.extend(framing::build_string("")); // bound field: the empty form (no field)
    body.extend(framing::build_narrowing(0));
    body.extend_from_slice(&0xFFFFu16.to_be_bytes());
    body.extend(framing::build_narrowing(vt)); // value_type
    body.extend_from_slice(&0u16.to_be_bytes()); // value_count
    body.extend_from_slice(&0i16.to_be_bytes()); // has_bounds
    body.extend_from_slice(&0f64.to_be_bytes()); // number_min
    body.extend_from_slice(&0f64.to_be_bytes()); // number_max
    body.extend(framing::build_string("")); // edit_mask
    body.extend(framing::build_narrowing(0)); // parameter_type: report parameter
    framing::build_record(0x007a, 0x0701, &body)
}

pub fn set_subreport_link(
    rpt: &Rpt,
    target: &str,
    main_report_field: &str,
    subreport_field: &str,
    linked_parameter: &str,
) -> Result<Vec<u8>, String> {
    // 1. Resolve the subreport (by its own name, not the placeholder object's name) and its
    //    subdocument index.
    let report = rpt.report();
    let sub = report
        .subreports
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(target))
        .ok_or_else(|| {
            let known: Vec<String> = report.subreports.iter().map(|s| s.name.clone()).collect();
            format!(
                "`{target}` is not a subreport in this report. Embedded subreports: {}",
                if known.is_empty() { "(none)".to_string() } else { known.join(", ") }
            )
        })?;
    let subdoc_index = subdoc_index_of(rpt, target)
        .ok_or_else(|| format!("the storage index of subreport `{target}` could not be located"))?;

    // 2. The linked parameter's engine index, from the subreport's own parameter records.
    // The engine names links after a `Pm-<main field>` parameter on the subreport; when the
    // subreport does not carry one, the sub-side write below SYNTHESIZES it (the SDK would
    // create one), typed like the main field. The main-side 0x106 needs only its ENGINE
    // INDEX, computable without an intermediate write: the parameter's would-be index.
    let main_display_for_pm = crate::grouping::field_display(report, main_report_field);
    let pm_name = format!("Pm-{main_display_for_pm}");
    let param_index = subreport_parameter_index(rpt, subdoc_index, linked_parameter)
        .or_else(|| subreport_parameter_index(rpt, subdoc_index, &pm_name))
        .or_else(|| {
            // Creation case: the linked parameter IS the `Pm-` name the engine would mint.
            linked_parameter.trim_start_matches('@').eq_ignore_ascii_case(&pm_name)
                .then(|| next_subdoc_param_index(rpt, subdoc_index))
        })
        .ok_or_else(|| format!("`{linked_parameter}` is not a parameter of subreport `{target}`"))?;

    // 3. The main field's pool handle.
    let (main_kind, main_index) = crate::grouping::field_handle(report, main_report_field)
        .map_err(|e| format!("mainReportField {e}"))?;
    let main_display = crate::grouping::field_display(report, main_report_field);

    // 4. The subreport-side field's handle, in the subreport's own pools.
    let (sub_kind, sub_index) =
        subreport_field_handle(sub, subreport_field).ok_or_else(|| {
            format!("`{subreport_field}` is not a field of subreport `{target}`")
        })?;

    // 5. Synthesize the 0x106 record and splice it after the placeholder's run (it must be a
    //    later sibling, with no other 0xa3 in between).
    let located = crate::locate::locate(rpt);
    let mut anchor: Option<(usize, u8)> = None;
    'find: for area in &located.areas {
        for sec in &area.sections {
            for obj in &sec.objects {
                if obj.opener_tag == T_SUBREPORT_OBJECT {
                    let idx = read_subdoc_index(rpt, obj)?;
                    anchor = obj.run.as_ref().map(|r| (r.range.end, r.outer_mask));
                    if idx == subdoc_index {
                        break 'find;
                    }
                }
            }
        }
    }
    let (at, _outer) = anchor.ok_or("the subreport placeholder object could not be located")?;

    let mut content = Vec::new();
    content.extend_from_slice(&param_index.to_be_bytes());
    content.extend(framing::build_string(&main_display));
    content.extend(framing::build_narrowing(u16::try_from(main_kind).unwrap_or(0)));
    content.extend_from_slice(&main_index.to_be_bytes());
    content.extend_from_slice(&0i16.to_be_bytes()); // link_flag 0: the subreport handle is stored
    content.extend(framing::build_narrowing(u16::try_from(sub_kind).unwrap_or(0)));
    content.extend_from_slice(&sub_index.to_be_bytes());
    let record = framing::build_record(T_SUBREPORT_LINK, 0x0700, &content);

    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();
    let new_logical = framing::splice(&logical, &tree, at..at, &record)
        .map_err(|e| format!("splice link: {e}"))?;
    let out = rpt
        .write_contents_logical(&new_logical)
        .map_err(|e| format!("write after link insert: {e}"))?;

    // 6. Sub-side half: create-or-extend the subreport's Record Selection formula with the
    //    binding `{sub.dbfield}={?Pm-<main field>}` — an UPGRADE over Vibey's main-side-only
    //    behavior. A missing `Pm-` parameter is CREATED inside that write (typed like the
    //    main field); a sub-side failure propagates.
    write_sub_side_binding(&out, subdoc_index, target, &main_display, subreport_field, linked_parameter)
}


/* ---------------- helpers ---------------- */
fn write_sub_side_binding(
    bytes: &[u8],
    subdoc: u32,
    subreport_name: &str,
    main_display: &str,
    subreport_field: &str,
    linked_parameter: &str,
) -> Result<Vec<u8>, String> {
    let _ = linked_parameter; // resolved by the caller for the main-side half
    let rpt = crate::open_bytes(bytes)?;

    // Resolve the sub-side db-field pool index inside the SUBREPORT's own model.
    let report = rpt.report();
    let sub = report
        .subreports
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(subreport_name))
        .ok_or("the subreport vanished before the sub-side write")?;
    let sub_bare = subreport_field
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .rsplit('.')
        .next()
        .unwrap_or(subreport_field);
    let mut sub_db_idx = 0u16;
    let mut found = false;
    for f in &sub.report.data_definition.field_definitions {
        if let rpt_reader::model::FieldKindData::Database(_) = &f.kind {
            if f.name.eq_ignore_ascii_case(sub_bare) {
                found = true;
                break;
            }
            sub_db_idx = sub_db_idx.saturating_add(1);
        }
    }
    if !found {
        return Err(format!(
            "`{subreport_field}` is not a field of subreport `{subreport_name}` (sub-side binding)"
        ));
    }

    // The Pm- parameter's POOL ordinal (the kind-5 FieldRef index) — read from the
    // subdocument's OWN `0x7a` records (the engine's parameter pool), below: a synthesized
    // Pm- parameter is raw-only until a PromptManager entry joins to it.
    let pm_name = format!("Pm-{main_display}");
    

    // Decode the subdocument Contents (nested streams are stored raw).
    let stream_id = StreamId::Other(format!("Subdocument {subdoc}/Contents"));
    let raw = rpt
        .stream(&stream_id)
        .ok_or("the subdocument's Contents stream vanished")?;
    let decoded = rpt_reader::raw::RecordStream::decode(StreamId::Contents, raw.raw_bytes());
    let logical = decoded.logical_bytes().to_vec();
    let tree = decoded.record_tree();

    let mut param_ordinal: Option<u16> = None;
    let mut n_params = 0u16;
    for n in tree.iter().filter(|n| n.rtype == 0x007a) {
        let named = n
            .children
            .iter()
            .find(|c| c.rtype == 0x0071)
            .and_then(|c| {
                rpt_reader::fields::read(c, &logical, rpt_reader::raw::Dialect::Contents).and_then(|r| {
                    r.fields.iter().find(|x| x.name == "name").map(|x| match &x.value {
                        rpt_reader::fields::FieldValue::Text(t) => t.clone(),
                        _ => String::new(),
                    })
                })
            })
            .unwrap_or_default();
        if named.trim_start_matches('@').eq_ignore_ascii_case(&pm_name) {
            param_ordinal = Some(n_params);
        }
        n_params = n_params.saturating_add(1);
    }

    // A missing `Pm-` parameter is CREATED here — inside the same subdocument edit the
    // binding writes, so the subdoc is written exactly once (chained container writes were
    // observed to drop nested-stream edits). Its pool ordinal is the new position; its type
    // mirrors the main field.
    let (param_ordinal, logical, tree) = match param_ordinal {
        Some(o) => (o, logical, tree),
        None => {
            let vt = value_type_code(main_field_value_type(
                &crate::open_bytes(bytes)?.report(),
                main_display,
            ));
            let next_index = next_subdoc_param_index(&crate::open_bytes(bytes)?, subdoc);
            let run = build_pm_parameter_run(&pm_name, vt, next_index);
            let at = tree
                .iter()
                .find(|n| n.rtype == 0x0070)
                .map(|n| n.offset)
                .ok_or("the subdocument has no FieldManagerEnd to anchor the parameter before")?;
            let new_logical = framing::splice(&logical, &tree, at..at, &run)
                .map_err(|e| format!("splice parameter: {e}"))?;
            let decoded2 = rpt_reader::raw::RecordStream::from_logical_bytes(StreamId::Contents, &new_logical);
            let tree2 = decoded2.record_tree();
            let ordinal = tree2.iter().filter(|n| n.rtype == 0x007a).count().saturating_sub(1) as u16;
            (ordinal, new_logical, tree2)
        }
    };


    // The existing Record Selection run (formula_kind == 1), if any: its span + text + refs.
    let existing: Option<(std::ops::Range<usize>, String, Vec<(String, u16, u16)>)> = tree
        .iter()
        .filter(|n| n.rtype == 0x0077)
        .find_map(|w| {
            let f = w.children.iter().find(|c| c.rtype == 0x0076)?;
            let read = rpt_reader::fields::read(f, &logical, rpt_reader::raw::Dialect::Contents)?;
            let kind = read
                .fields
                .iter()
                .find(|x| x.name == "formula_kind")
                .and_then(|x| match x.value {
                    rpt_reader::fields::FieldValue::Uint(v) => Some(v),
                    _ => None,
                })?;
            if kind != 1 {
                return None; // not the Record Selection formula
            }
            let text = read
                .fields
                .iter()
                .find(|x| x.name == "text")
                .and_then(|x| match &x.value {
                    rpt_reader::fields::FieldValue::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let mut refs = Vec::new();
            for x in &read.fields {
                if x.name == "field" {
                    if let rpt_reader::fields::FieldValue::FieldRef { name, kind, index } = &x.value {
                        refs.push((name.clone(), u16::try_from(*kind).unwrap_or(0), index.unwrap_or(0)));
                    }
                }
            }
            Some((w.offset..w.content_end, text, refs))
        });

    // Merged text and refs for the rebuilt run.
    let sub_ref_name = sub_bare_alias_field(subreport_field);
    let new_line = format!("{{{sub_ref_name}}}={{?{pm_name}}}");
    let (run_range, text, refs) = match existing {
        Some((range, prev_text, mut prev_refs)) => {
            let text = if prev_text.trim().is_empty() {
                new_line.clone()
            } else {
                format!("{}\r\nand\r\n{}", prev_text.trim_end(), new_line)
            };
            prev_refs.push((sub_ref_name.to_string(), 0, sub_db_idx));
            prev_refs.push((format!("?{pm_name}"), 5, param_ordinal));
            (range, text, prev_refs)
        }
        None => {
            let refs = vec![
                (sub_ref_name.to_string(), 0, sub_db_idx),
                (format!("?{pm_name}"), 5, param_ordinal),
            ];
            (usize::MAX..usize::MAX, new_line.clone(), refs)
        }
    };

    // Synthesize: 0x71 NamedValue ("Record Selection", Boolean) — the FORMULA table's first
    // content — then refs, text, kinds, per the decoded donor's wire order.
    let mut nv = Vec::new();
    nv.extend(crate::framing::build_string("Record Selection"));
    nv.extend(crate::framing::build_narrowing(8)); // value_type = Boolean
    nv.extend_from_slice(&2u16.to_be_bytes()); // narrow_length
    nv.extend(crate::framing::build_string(""));
    nv.extend_from_slice(&2u32.to_be_bytes()); // length
    let named = crate::framing::build_record(0x0071, 0x0700, &nv);

    let mut body = named;
    body.extend_from_slice(&(refs.len() as u16).to_be_bytes());
    for (name, kind, index) in &refs {
        body.extend(crate::framing::build_string(name));
        body.extend(crate::framing::build_narrowing(*kind));
        body.extend_from_slice(&index.to_be_bytes());
    }
    body.extend(crate::framing::build_string(&text));
    body.extend(crate::framing::build_narrowing(1)); // formula_kind = record selection
    body.extend(crate::framing::build_narrowing(0)); // format_formula_kind
    for _ in 0..7 {
        body.extend_from_slice(&0u16.to_be_bytes()); // the seven tail words
    }
    let formula = crate::framing::build_record(0x0076, 0x0700, &body);
    let run = crate::framing::build_record(0x0077, 0x0700, &formula);

    let new_logical = if run_range.start != usize::MAX {
        crate::framing::splice(&logical, &tree, run_range, &run)
            .map_err(|e| format!("splice sub-side binding: {e}"))?
    } else {
        // Create: after MultiColumnFormat / before the first RecordSortField or area pair —
        // the position the engine itself writes the run (per the decoded donors).
        let at = tree
            .iter()
            .find(|n| n.rtype == 0x006c)
            .map(|n| n.content_end)
            .or_else(|| tree.iter().find(|n| n.rtype == 0x0029).map(|n| n.offset))
            .or_else(|| tree.iter().find(|n| n.rtype == 0x0082).map(|n| n.offset))
            .ok_or("no anchor for the sub-side binding in the subdocument")?;
        crate::framing::splice(&logical, &tree, at..at, &run)
            .map_err(|e| format!("splice sub-side binding: {e}"))?
    };
    rpt.write_stream_logical(&stream_id, &new_logical)
        .map_err(|e| format!("write sub-side binding: {e}"))
}

/// The bare `alias.field` form of a possibly-braced field reference: `{alias.field}` →
/// `alias.field` (no braces) — the wire form a reference block text carries.
fn sub_bare_alias_field(raw: &str) -> &str {
    let t = raw.trim();
    let t = t.strip_prefix('{').unwrap_or(t);
    t.strip_suffix('}').unwrap_or(t)
}
