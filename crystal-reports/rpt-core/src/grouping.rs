//! `addSort` and `addGroup` — the data-definition edits.
//!
//! A record sort is one `0x29 RecordSortField` record (field text, pool kind, pool index,
//! direction); adding one is a clone-or-synthesize of that small record spliced after the last
//! existing sort, then rebound. A group is a `0xe5 Group` definition **plus** the layout the
//! engine builds for it — a Group Area Pair with its header/footer areas, bands and sections —
//! so adding one clones an existing group's whole footprint from the same report (every report
//! the layout vocabulary targets has at least one group) and rebinds it as the innermost group.

use rpt_reader::raw::RecordTag;
use rpt_reader::{fields::FieldEdit, model, Rpt, StreamId};

use crate::framing;

/// The `0x29` sort-record tag.
const T_SORT: u16 = 0x0029;
/// The `0xe5` group-definition tag.
const T_GROUP: u16 = 0x00e5;

/// Sort directions (the record's narrowing enum).
fn direction_code(direction: &str) -> Result<i64, String> {
    match direction.to_ascii_lowercase().as_str() {
        "ascending" => Ok(0),
        "descending" => Ok(1),
        other => Err(format!("direction must be ascending or descending, not `{other}`")),
    }
}

/// The (pool kind, pool index) a field reference resolves to, for sorts, group conditions and
/// field-object bindings. Database fields index their 0-based position among the report's DB
/// field definitions (the pool the records store); formulas among the formula definitions.
/// A field is matchable by its bare name (the last dot-segment of the reference) — the report's
/// own definitions carry bare names.
pub fn field_handle(report: &model::Report, field_ref: &str) -> Result<(u32, u16), String> {
    let debraced = field_ref.trim().trim_start_matches('{').trim_end_matches('}');
    let bare = debraced.rsplit('.').next().unwrap_or(debraced);
    let formula_name = debraced.strip_prefix('@').unwrap_or(debraced);
    let mut db_idx = 0u16;
    let mut fx_idx = 0u16;
    let mut pm_idx = 0u16;
    let bare_unmarked = bare.trim_start_matches('@');
    for f in &report.data_definition.field_definitions {
        let field_bare = f.name.trim_start_matches('@');
        let matches_db = field_bare.eq_ignore_ascii_case(bare_unmarked);
        let matches_fx = field_bare.eq_ignore_ascii_case(bare_unmarked)
            || field_bare.eq_ignore_ascii_case(formula_name.trim_start_matches('@'))
            || f.name.eq_ignore_ascii_case(debraced);
        match &f.kind {
            model::FieldKindData::Database(_) => {
                if matches_db {
                    return Ok((0, db_idx));
                }
                db_idx = db_idx.saturating_add(1);
            }
            model::FieldKindData::Formula(_) => {
                if matches_fx {
                    return Ok((1, fx_idx));
                }
                fx_idx = fx_idx.saturating_add(1);
            }
            model::FieldKindData::Parameter(_) => {
                if matches_fx {
                    return Ok((6, pm_idx));
                }
                pm_idx = pm_idx.saturating_add(1);
            }
            _ => {}
        }
    }
    Err(format!(
        "field `{field_ref}` is not in the report's field pool; a field becomes bindable offline          only once the report references it (the engine builds unreferenced columns from the live          database, which an offline editor cannot reach)"
    ))
}

/// The canonical display text for a field reference, from the report's own tables first
/// (`{alias.field}`), then any stored formula form, else the caller's string verbatim.
pub fn field_display(report: &model::Report, field_ref: &str) -> String {
    let debraced = field_ref.trim().trim_start_matches('{').trim_end_matches('}');
    let bare = debraced.rsplit('.').next().unwrap_or(debraced);
    let formula_name = debraced.strip_prefix('@').unwrap_or(debraced);
    for t in &report.database.tables {
        for f in &t.data_fields {
            if f.name.eq_ignore_ascii_case(bare) {
                // The wire's reference text is the RAW name — the model renders braces itself.
                return format!("{}.{}", t.alias, f.name);
            }
        }
    }
    for f in &report.data_definition.field_definitions {
        if let Some(form) = &f.formula_form {
            if form.eq_ignore_ascii_case(field_ref) || form.eq_ignore_ascii_case(debraced) {
                return form.clone();
            }
        }
        if matches!(f.kind, model::FieldKindData::Parameter(_))
            && f.name.eq_ignore_ascii_case(formula_name)
        {
            return format!("@{}", f.name.trim_start_matches('@'));
        }
    }
    field_ref.to_string()
}

/// `addSort`: clone (or synthesize) a `0x29` sort record, rebind, splice after the last sort.
pub fn add_sort(rpt: &Rpt, field_ref: &str, direction: &str) -> Result<Vec<u8>, String> {
    let report = rpt.report();
    let (pool, index) = field_handle(report, field_ref)?;
    let display = field_display(report, field_ref);
    let dir = direction_code(direction)?;

    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();
    let at = match tree.iter().filter(|n| n.rtype == T_SORT).next_back() {
        Some(n) => n.content_end,
        // No explicit sorts yet: synthesize the engine's own shape and place it after the last
        // group definition — sorts follow groups in the data-definition grammar.
        None => {
            let insert_after = tree
                .iter()
                .filter(|n| n.rtype == T_GROUP)
                .next_back()
                .map(|n| n.content_end)
                .ok_or_else(|| {
                    "this report has neither record-sort nor group records to anchor a new sort against"
                        .to_string()
                })?;
            let mut content = framing::build_string(&display);
            content.extend(framing::build_narrowing(u16::try_from(pool).unwrap_or(0)));
            content.extend_from_slice(&index.to_be_bytes());
            content.extend(framing::build_narrowing(u16::try_from(dir).unwrap_or(0)));
            let record = framing::build_record(T_SORT, 0x0700, &content);
            let new_logical = framing::splice(logical, &tree, insert_after..insert_after, &record)
                .map_err(|e| format!("splice synthesized sort: {e}"))?;
            let out = rpt
                .write_contents_logical(&new_logical)
                .map_err(|e| format!("write after sort insert: {e}"))?;
            let reopened = crate::open_bytes(&out)?;
            let nth = count_tag(&reopened, T_SORT).saturating_sub(1);
            return verify_sort(&out, nth, &display, dir);
        }
    };

    // Clone the last sort record verbatim (its whole span), then rebind it.
    let last_node = tree
        .iter()
        .rev()
        .find(|n| n.rtype == T_SORT)
        .ok_or("no sort record to clone")?;
    let run = last_node.offset..last_node.content_end;
    let bytes = logical.get(run.clone()).ok_or("sort record span outside the stream")?.to_vec();

    // The clone is inserted at the same nesting level (top level): same outer mask (0).
    let new_logical =
        framing::splice(logical, &tree, at..at, &bytes).map_err(|e| format!("splice sort: {e}"))?;
    let out = rpt
        .write_contents_logical(&new_logical)
        .map_err(|e| format!("write after sort insert: {e}"))?;

    // Rebind the new sort (now the LAST 0x29) through the verified writer.
    let reopened = crate::open_bytes(&out)?;
    let nth = count_tag(&reopened, T_SORT).saturating_sub(1);
    let mut current = out;
    current = patch_sort(&current, nth, &display, pool, index, dir)?;
    verify_sort(&current, nth, &display, dir)
}

/// `addGroup`: clone an existing group's definition + area pair, rebind as the innermost group.
pub fn add_group(rpt: &Rpt, field_ref: &str, direction: Option<&str>) -> Result<Vec<u8>, String> {
    let report = rpt.report();
    if report.data_definition.groups.is_empty() {
        return Err(
            "this report has no existing group to clone a new one from; adding a first group \
             needs a full area-pair synthesis (not supported yet)"
                .to_string(),
        );
    }
    let (pool, index) = field_handle(report, field_ref)?;
    let display = field_display(report, field_ref);
    let dir = direction.map(direction_code).transpose()?;

    // 1. Clone the LAST group definition record (the innermost group's).
    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();
    let last_group = tree.iter().rev().find(|n| n.rtype == T_GROUP).ok_or("no group record to clone")?;
    let def_run = last_group.offset..last_group.content_end;
    let group_bytes = logical.get(def_run.clone()).ok_or("group record span outside stream")?.to_vec();
    let insert_at = last_group.content_end;
    let new_logical = framing::splice(logical, &tree, insert_at..insert_at, &group_bytes)
        .map_err(|e| format!("splice group def: {e}"))?;
    let mut current = rpt
        .write_contents_logical(&new_logical)
        .map_err(|e| format!("write after group def insert: {e}"))?;

    // 2. Clone the innermost group's layout footprint: its Group Header area bracket and its
    //    Group Footer area bracket (each an Area record through its AreaEnd child), re-masked
    //    and re-named for the new group. The innermost group's header is the last group-header
    //    area before Detail; its footer the last group-footer area before ReportFooter.
    let opened = crate::open_bytes(&current)?;
    let located = crate::locate::locate(&opened);
    let report2 = opened.report();
    let group_header = report2
        .report_definition
        .areas
        .iter()
        .filter(|a| matches!(a.kind, model::AreaSectionKind::GroupHeader))
        .next_back()
        .and_then(|a| located.areas.iter().find(|l| l.name == a.name));
    let group_footer = report2
        .report_definition
        .areas
        .iter()
        .filter(|a| matches!(a.kind, model::AreaSectionKind::GroupFooter))
        .next_back()
        .and_then(|a| located.areas.iter().find(|l| l.name == a.name));
    let (gh, gf) = (group_header, group_footer);
    let gh_run = gh.and_then(|a| a.run.clone()).ok_or("group header area run not located")?;
    let gf_run = gf.and_then(|a| a.run.clone()).ok_or("group footer area run not located")?;

    // Insertion points: the header bracket goes right where the innermost header bracket ends
    // (a new innermost header nests INSIDE it, so actually before Detail — after the last
    // group header is exactly that); the footer after the last group footer bracket.
    let header_name = gh.map(|a| a.name.clone()).unwrap_or_default();
    let footer_name = gf.map(|a| a.name.clone()).unwrap_or_default();

    let stream2 = opened.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical2 = stream2.logical_bytes();
    let tree2 = stream2.record_tree();
    let gh_bytes = crate::framing::remask_run(
        logical2.get(gh_run.range.clone()).ok_or("header run outside stream")?,
        gh_run.outer_mask,
        gh_run.outer_mask,
    );
    let gf_bytes = crate::framing::remask_run(
        logical2.get(gf_run.range.clone()).ok_or("footer run outside stream")?,
        gf_run.outer_mask,
        gf_run.outer_mask,
    );
    // Footer first (higher offset) so the header splice does not move it.
    let with_footer = framing::splice(&logical2, &tree2, gf_run.range.end..gf_run.range.end, &gf_bytes)
        .map_err(|e| format!("splice group footer: {e}"))?;
    let tree3 = opened
        .stream(&StreamId::Contents)
        .map(|s| s.record_tree())
        .unwrap_or_default();
    let _ = tree3;
    // Recompute the tree for the header splice against the footer-spliced stream.
    let reparsed = parse_tree_of(&with_footer)?;
    let with_both = framing::splice(&with_footer, &reparsed, gh_run.range.end..gh_run.range.end, &gh_bytes)
        .map_err(|e| format!("splice group header: {e}"))?;
    current = opened
        .write_contents_logical(&with_both)
        .map_err(|e| format!("write after group layout insert: {e}"))?;

    // 3. Rebind the new group definition (now the LAST 0xe5): condition field + ordinal + direction.
    let reopened = crate::open_bytes(&current)?;
    let nth = count_tag(&reopened, T_GROUP).saturating_sub(1);
    current = patch_group_condition(&current, nth, &display, pool, index)?;
    let new_level = report.data_definition.groups.len() as i64;
    if let Ok(fields) = reopened.record_fields(RecordTag(T_GROUP), nth) {
        if fields.fields.iter().any(|f| f.name == "condition_ordinal") {
            current = patch_on_field(&current, RecordTag(T_GROUP), nth, "condition_ordinal", FieldEdit::Int(new_level))?;
        }
    }
    if let Some(d) = dir {
        current = patch_on_field(&current, RecordTag(T_GROUP), nth, "direction", FieldEdit::Int(d))?;
    }

    // 3b. Group identity: the clone still carries the DONOR's `Group #(N-1) Name` /
    // `@Group #(N-1) Order` fields and its order-formula handle — re-number them to the new
    // group and mint the new group's `Group #N Order` formula wrapper. Without this, a report
    // gaining its second group carries two level-clashing "#1" identities.
    let group_no = new_level as i64 + 1;
    {
        let opened = crate::open_bytes(&current)?;
        let nth_group = count_tag(&opened, T_GROUP).saturating_sub(1);
        let present = |field: &str| {
            opened
                .record_fields(RecordTag(T_GROUP), nth_group)
                .map(|rf| rf.fields.iter().any(|f| f.name == field))
                .unwrap_or(false)
        };
        if present("group_name_field") {
            current = patch_on_field(&current, RecordTag(T_GROUP), nth_group, "group_name_field", FieldEdit::Text(format!("Group #{group_no} Name")))?;
        }
        if present("group_name_field_kind") {
            current = patch_on_field(&current, RecordTag(T_GROUP), nth_group, "group_name_field_kind", FieldEdit::Int(4))?;
        }
        if present("group_name_field_index") {
            current = patch_on_field(&current, RecordTag(T_GROUP), nth_group, "group_name_field_index", FieldEdit::Int(new_level))?;
        }
        if present("order_marker") {
            current = patch_on_field(&current, RecordTag(T_GROUP), nth_group, "order_marker", FieldEdit::Text(format!("@Group #{group_no} Order")))?;
        }
    }
    // 3c. Mint `Group #N Order`: clone the innermost group's order wrapper and rebind it to
    // the new group's condition field. Inserted AFTER THE LAST formula wrapper in the stream,
    // so its pool ordinal (wrapper count before insertion — the index space the 0xe5's
    // `order_formula_index` and kind-1 refs address) shifts nothing.
    current = mint_group_order(&current, &display, pool, index, new_level)?;

    // 4. Rename the cloned areas and their sections (they still carry the donor's names) and
    //    bump the nesting level — the same naming rule the engine applies
    //    (VibeyReports' GroupSectionNaming: field name, alphanumerics only, + band [+ Section1]).
    let bare = display
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .rsplit('.')
        .next()
        .unwrap_or(&display);
    let stem: String = bare.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let opened = crate::open_bytes(&current)?;
    let located2 = crate::locate::locate(&opened);
    for (donor_name, band) in [(header_name.clone(), "Header"), (footer_name.clone(), "Footer")] {
        let clones: Vec<usize> = located2
            .areas
            .iter()
            .enumerate()
            .filter(|(_, a)| a.name.eq_ignore_ascii_case(&donor_name))
            .map(|(i, _)| i)
            .collect();
        // The clone is the LATER of the duplicated areas (inserted after the donor's bracket).
        let Some(&clone_idx) = clones.last() else { continue };
        let area_name = format!("{stem}{band}");
        let section_name = format!("{stem}{band}Section1");
        let area = &located2.areas[clone_idx];
        if let Some(nr) = area.name_rec {
            current = patch_on_field(&current, RecordTag(nr.tag), nr.nth, "name", FieldEdit::Text(area_name.clone()))?;
        }
        for lr in &area.level_recs {
            current = patch_on_field(&current, RecordTag(lr.tag), lr.nth, "group_level", FieldEdit::Int(new_level))?;
        }
        for sec in &area.sections {
            if let Some(sr) = sec.section_rec {
                current = patch_on_field(&current, RecordTag(sr.tag), sr.nth, "name", FieldEdit::Text(section_name.clone()))?;
            }
        }
    }
    Ok(current)
}

/// A `Group #N Order` name (`Group #1 Order`) → its 1-based number.
fn group_order_number(name: &str) -> Option<i64> {
    let rest = name.trim().strip_prefix("Group #")?;
    if !rest.ends_with(" Order") {
        return None;
    }
    rest.trim_end_matches(" Order").parse().ok()
}

/// The name of the `0x71` child of a `0x76` inside a `0x77` wrapper.
fn wrapper_formula_name(node: &rpt_reader::raw::RecordNode, logical: &[u8]) -> Option<String> {
    let f = node.children.iter().find(|c| c.rtype == 0x0076)?;
    let child = f.children.iter().find(|c| c.rtype == 0x0071)?;
    let read = rpt_reader::fields::read(child, logical, rpt_reader::raw::Dialect::Contents)?;
    read.fields.iter().find(|x| x.name == "name").and_then(|x| match &x.value {
        rpt_reader::fields::FieldValue::Text(t) => Some(t.clone()),
        _ => None,
    })
}

/// Mint the new group's `Group #N Order` formula wrapper: rebuild it FRESH from the innermost
/// existing order wrapper's decoded fields (name, refs, and text rebound to the new group's
/// condition field; every kind word carried over), inserted after the LAST top-level formula
/// wrapper so its pool ordinal — the index space `order_formula_index` and kind-1 refs address —
/// shifts nothing. No nested-record patching: the whole run is built, then spliced once.
fn mint_group_order(
    bytes: &[u8],
    display: &str,
    pool: u32,
    index: u16,
    new_level: i64,
) -> Result<Vec<u8>, String> {
    let opened = crate::open_bytes(bytes)?;
    let stream = opened.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();

    let wrappers: Vec<&rpt_reader::raw::RecordNode> =
        tree.iter().filter(|n| n.rtype == 0x0077).collect();
    if wrappers.is_empty() {
        return Err("this report has no formula wrappers to derive a group order from".to_string());
    }
    let mint_ordinal = wrappers.len() as i64;
    let donor = wrappers
        .iter()
        .rev()
        .find(|w| wrapper_formula_name(w, logical).as_deref().and_then(group_order_number).is_some())
        .ok_or("no `Group #N Order` formula to derive; the report's groups carry none")?;

    let donor_76 = donor.children.iter().find(|c| c.rtype == 0x0076).ok_or("donor has no formula")?;
    let donor_71 = donor_76.children.iter().find(|c| c.rtype == 0x0071).ok_or("donor has no name record")?;
    let read76 = rpt_reader::fields::read(donor_76, logical, rpt_reader::raw::Dialect::Contents)
        .ok_or("the donor order formula refused to read")?;
    let mut child_fields = rpt_reader::fields::read(donor_71, logical, rpt_reader::raw::Dialect::Contents)
        .ok_or("the donor order formula's name record refused to read")?
        .fields
        .to_vec();
    // Name → `Group #N Order` (the first field of the 0x71 is its name).
    if let Some(first) = child_fields.first_mut() {
        first.value = rpt_reader::fields::FieldValue::Text(format!("Group #{} Order", new_level + 1));
    }
    let tail_fields: Vec<_> = read76.fields.iter().skip_while(|x| x.path != "text").skip(1).cloned().collect();

    let refs = vec![(display.to_string(), u16::try_from(pool).unwrap_or(0), index)];
    let text = format!("if IsNull ({{{display}}})
then 2
else 2");
    let run = crate::formula::build_formula_run(child_fields, &refs, &text, &tail_fields)?;
    let insert_at = wrappers.last().unwrap().content_end;
    let spliced = framing::splice(&logical, &tree, insert_at..insert_at, &run)
        .map_err(|e| format!("splice group order wrapper: {e}"))?;
    let mut current = opened
        .write_contents_logical(&spliced)
        .map_err(|e| format!("write group order wrapper: {e}"))?;

    // Read the mint back: a top-level wrapper named `Group #N Order` whose ref and body carry
    // the new condition field.
    {
        let chk = crate::open_bytes(&current)?;
        let s2 = chk.stream(&StreamId::Contents).ok_or("no Contents stream")?;
        let l2 = s2.logical_bytes();
        let tree2 = s2.record_tree();
        let minted = tree2.iter().filter(|n| n.rtype == 0x0077).find_map(|w| {
            let name = wrapper_formula_name(w, l2)?;
            (name == format!("Group #{} Order", new_level + 1)).then_some(w)
        });
        let Some(minted) = minted else {
            return Err("the minted group order wrapper does not read back".to_string());
        };
        let f = minted.children.iter().find(|c| c.rtype == 0x0076).ok_or("minted has no formula")?;
        let read = rpt_reader::fields::read(f, l2, rpt_reader::raw::Dialect::Contents)
            .ok_or("the minted order formula refused to read")?;
        let ok_text = read.fields.iter().any(|x| x.path == "text" && matches!(&x.value, rpt_reader::fields::FieldValue::Text(t) if t == &text));
        let ok_ref = read.fields.iter().any(|x| matches!(&x.value, rpt_reader::fields::FieldValue::FieldRef { name, .. } if name == display));
        if !ok_text || !ok_ref {
            return Err(format!("the minted group order wrapper read back wrong (text={ok_text} ref={ok_ref})"));
        }
    }

    // Point the new group's 0xe5 at the minted formula.
    let opened4 = crate::open_bytes(&current)?;
    let nth_group = count_tag(&opened4, T_GROUP).saturating_sub(1);
    let has_kind = opened4
        .record_fields(RecordTag(T_GROUP), nth_group)
        .map(|rf| rf.fields.iter().any(|f| f.name == "order_formula_kind"))
        .unwrap_or(false);
    if has_kind {
        current = patch_on_field(&current, RecordTag(T_GROUP), nth_group, "order_formula_kind", FieldEdit::Int(1))?;
        current = patch_on_field(&current, RecordTag(T_GROUP), nth_group, "order_formula_index", FieldEdit::Int(mint_ordinal))?;
    }
    Ok(current)
}

/// Count records of a tag in Contents (pre-order).
fn count_tag(rpt: &Rpt, tag: u16) -> usize {
    rpt.stream(&StreamId::Contents)
        .map(|s| {
            s.record_tree()
                .iter()
                .filter(|n| n.rtype == tag)
                .count()
        })
        .unwrap_or(0)
}

/// Patch the four fields of the nth sort record.
fn patch_sort(bytes: &[u8], nth: usize, display: &str, pool: u32, index: u16, dir: i64) -> Result<Vec<u8>, String> {
    let mut current = bytes.to_vec();
    current = patch_on_field(&current, RecordTag(T_SORT), nth, "field", FieldEdit::Text(display.to_string()))?;
    current = patch_on_field(&current, RecordTag(T_SORT), nth, "field_kind", FieldEdit::Int(i64::from(pool)))?;
    current = patch_on_field(&current, RecordTag(T_SORT), nth, "field_index", FieldEdit::Int(i64::from(index)))?;
    patch_on_field(&current, RecordTag(T_SORT), nth, "direction", FieldEdit::Int(dir))
}

/// Patch a group definition's condition-field triple.
fn patch_group_condition(bytes: &[u8], nth: usize, display: &str, pool: u32, index: u16) -> Result<Vec<u8>, String> {
    let mut current = bytes.to_vec();
    current = patch_on_field(&current, RecordTag(T_GROUP), nth, "condition_field", FieldEdit::Text(display.to_string()))?;
    current = patch_on_field(&current, RecordTag(T_GROUP), nth, "condition_field_kind", FieldEdit::Int(i64::from(pool)))?;
    patch_on_field(&current, RecordTag(T_GROUP), nth, "condition_field_index", FieldEdit::Int(i64::from(index)))
}

fn patch_on_field(bytes: &[u8], tag: RecordTag, nth: usize, field: &str, value: FieldEdit) -> Result<Vec<u8>, String> {
    let rpt = crate::open_bytes(bytes)?;
    rpt.patch_record_field_with(tag, nth, field, &value, rpt_reader::EditPolicy::Checked)
        .map_err(|e| format!("patch {field}: {e}"))
}

/// Read back the nth sort and confirm it says what we wrote.
fn verify_sort(bytes: &[u8], nth: usize, display: &str, dir: i64) -> Result<Vec<u8>, String> {
    let rpt = crate::open_bytes(bytes)?;
    let f = rpt
        .record_field(RecordTag(T_SORT), nth, "field")
        .map_err(|e| format!("verify: {e}"))?;
    match &f.value {
        rpt_reader::fields::FieldValue::Text(t) if t == display => {}
        other => return Err(format!("sort field read back {other:?}, expected `{display}`")),
    }
    let d = rpt
        .record_field(RecordTag(T_SORT), nth, "direction")
        .map_err(|e| format!("verify: {e}"))?;
    match &d.value {
        rpt_reader::fields::FieldValue::Uint(v) if i64::from(*v) == dir => {}
        rpt_reader::fields::FieldValue::Int(v) if i64::from(*v) == dir => {}
        other => return Err(format!("sort direction read back {other:?}, expected {dir}")),
    }
    Ok(bytes.to_vec())
}

/// Parse a logical stream into a record tree (for re-splicing after a mutation).
fn parse_tree_of(logical: &[u8]) -> Result<Vec<rpt_reader::raw::RecordNode>, String> {
    let stream = rpt_reader::raw::RecordStream::from_logical_bytes(StreamId::Contents, logical);
    Ok(stream.record_tree())
}
