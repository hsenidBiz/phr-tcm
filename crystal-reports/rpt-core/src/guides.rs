//! `addGuideline` — add a report-wide guideline at a twip position.
//!
//! A guideline is one `0x10c GuidelineEntry` record: a twip `position` (i32be) plus a
//! `connection_count` (u16be, 0 when nothing is snapped). The guide region's wrapper nesting
//! (`RulerDefinition`/`RulerScale`, `GuidelineCollection`/`GuidelineList`) is irregular across
//! files — entries parse as collection children in some and as top-level siblings in others —
//! so this op clones an existing entry and inserts the copy right beside it (same depth, same
//! context), then patches its position. No framing assumptions about the wrappers.
//!
//! **Verified axis mapping** (2026-10-16): the first guide group in the stream (GuidelineList)
//! holds HORIZONTAL guides — proven empirically on the A4 IndPerfOverview fixture, where the
//! first list's positions include 12168 twips: beyond the content WIDTH (11186) but within the
//! content HEIGHT (16118), i.e. Y coordinates. "horizontal" therefore inserts after the FIRST
//! entry of the run, "vertical" after the last.

use rpt_reader::fields::FieldEdit;
use rpt_reader::raw::RecordNode;
use rpt_reader::{Rpt, StreamId};

/// `0x10c` — one guideline entry.
const T_GUIDELINE_ENTRY: u16 = 0x010c;

/// First entry of the H/V run = horizontal (verified; see module docs).
const HORIZONTAL_AFTER_FIRST: bool = true;

/// Append a guide at `position` twips on the chosen axis.
pub fn add_guideline(rpt: &Rpt, axis: &str, position: i64) -> Result<Vec<u8>, String> {
    let horizontal = match axis.to_ascii_lowercase().as_str() {
        "horizontal" => true,
        "vertical" => false,
        other => return Err(format!("axis must be horizontal or vertical, not `{other}`")),
    };
    if !(-2_147_483_648..=2_147_483_647).contains(&position) {
        return Err(format!("position {position} does not fit the record's i32 field"));
    }

    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();

    // Every 0x10c in document order, with the outer mask at its position.
    let mut entries: Vec<(&RecordNode, u8)> = Vec::new();
    fn scan<'a>(nodes: &'a [RecordNode], out: &mut Vec<(&'a RecordNode, u8)>, outer: u8) {
        for n in nodes {
            if n.rtype == T_GUIDELINE_ENTRY {
                out.push((n, outer));
            }
            scan(&n.children, out, outer ^ (n.rtype & 0xff) as u8);
        }
    }
    // A report with no guides at all takes the synthesis path before any indexing below.
    scan(&tree, &mut entries, 0);
    if entries.is_empty() {
        return add_first_guideline(rpt, &tree, logical, position);
    }

    // Horizontal slots in after the FIRST entry of the run, vertical after the LAST — the H/V
    // pair is stored as consecutive entries (assumption; see module docs).
    let pick_first = horizontal == HORIZONTAL_AFTER_FIRST;
    let (node, node_outer) = if pick_first { entries[0] } else { entries[entries.len() - 1] };
    let at = node.content_end;
    let bytes = crate::framing::remask_run(
        logical.get(node.offset..node.content_end).ok_or("entry span outside the stream")?,
        node_outer,
        node_outer, // same position: identity re-mask
    );

    let new_logical = crate::framing::splice(&logical, &tree, at..at, &bytes)
        .map_err(|e| format!("splice guideline: {e}"))?;
    let mut out = rpt
        .write_contents_logical(&new_logical)
        .map_err(|e| format!("write after guideline insert: {e}"))?;

    // Patch the clone's position — the entry beside the anchor.
    let reopened = crate::open_bytes(&out)?;
    let rs = reopened.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let rtree = rs.record_tree();
    let mut recount: Vec<&RecordNode> = Vec::new();
    fn scan2<'a>(nodes: &'a [RecordNode], out: &mut Vec<&'a RecordNode>) {
        for n in nodes {
            if n.rtype == T_GUIDELINE_ENTRY {
                out.push(n);
            }
            scan2(&n.children, out);
        }
    }
    scan2(&rtree, &mut recount);
    let clone_nth = if pick_first { 1 } else { recount.len().saturating_sub(1) };
    out = reopened
        .patch_record_field_with(
            rpt_reader::raw::RecordTag(T_GUIDELINE_ENTRY),
            clone_nth,
            "position",
            &FieldEdit::Int(position),
            rpt_reader::EditPolicy::Checked,
        )
        .map_err(|e| format!("patch the clone's position: {e}"))?;

    // Verify: exactly one more entry anywhere in the tree carrying the position.
    let check = crate::open_bytes(&out)?;
    let cs = check.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let ctree = cs.record_tree();
    let mut found = 0;
    fn scan3(nodes: &[RecordNode], want: i64, found: &mut i32, logical: &[u8]) {
        for n in nodes {
            if n.rtype == T_GUIDELINE_ENTRY {
                if let Some(read) =
                    rpt_reader::fields::read(n, logical, rpt_reader::raw::Dialect::Contents)
                {
                    if read.fields.iter().any(
                        |f| f.name == "position" && f.value == rpt_reader::fields::FieldValue::Int(want as i32),
                    ) {
                        *found += 1;
                    }
                }
            }
            scan3(&n.children, want, found, logical);
        }
    }
    scan3(&ctree, position, &mut found, cs.logical_bytes());
    if found != 1 {
        return Err(format!("the inserted guideline did not read back ({found} entries at {position})"));
    }
    Ok(out)
}

/// A report with no guides at all: synthesize a root `GuidelineCollection` bracket holding
/// one entry, spliced before the stream's last top-level record (the root end marker).
/// Axis is meaningless for the first guide — the collection pair the engine builds is not
/// yet present; the position is stored and the next add follows the normal path.
fn add_first_guideline(rpt: &Rpt, tree: &[RecordNode], logical: &[u8], position: i64) -> Result<Vec<u8>, String> {
    let mut entry = Vec::new();
    entry.extend_from_slice(&(position as i32).to_be_bytes());
    entry.extend_from_slice(&0u16.to_be_bytes());
    let entry = crate::framing::build_record(T_GUIDELINE_ENTRY, 0x0700, &entry);
    let mut bytes = crate::framing::build_record(0x010f, 0x0700, &entry);
    bytes.extend_from_slice(&crate::framing::build_record(0x0110, 0x0700, &[]));
    let at = tree.last().map(|n| n.offset).unwrap_or(logical.len());
    let new_logical = crate::framing::splice(logical, tree, at..at, &bytes)
        .map_err(|e| format!("splice guideline collection: {e}"))?;
    let out = rpt
        .write_contents_logical(&new_logical)
        .map_err(|e| format!("write after guideline insert: {e}"))?;
    verify_guideline(&out, position)
}

/// Shared read-back check: exactly one entry anywhere in the tree carries `position`.
fn verify_guideline(out: &[u8], position: i64) -> Result<Vec<u8>, String> {
    let check = crate::open_bytes(out)?;
    let cs = check.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let ctree = cs.record_tree();
    let mut found = 0;
    fn scan3(nodes: &[RecordNode], want: i64, found: &mut i32, logical: &[u8]) {
        for n in nodes {
            if n.rtype == T_GUIDELINE_ENTRY {
                if let Some(read) =
                    rpt_reader::fields::read(n, logical, rpt_reader::raw::Dialect::Contents)
                {
                    if read.fields.iter().any(
                        |f| f.name == "position" && f.value == rpt_reader::fields::FieldValue::Int(want as i32),
                    ) {
                        *found += 1;
                    }
                }
            }
            scan3(&n.children, want, found, logical);
        }
    }
    scan3(&ctree, position, &mut found, cs.logical_bytes());
    if found != 1 {
        return Err(format!("the inserted guideline did not read back ({found} entries at {position})"));
    }
    Ok(out.to_vec())
}
