//! `addSection` / `removeSection` — insert and delete sections inside an existing area.
//!
//! The grammar (mapped from the multi-section World Sales fixture): each section is one
//! self-contained **band bracket** `BandOpen → 0x8c → 0x9d chain → 0xff/0xfe → [objects] →
//! BandEnd`, sitting side-by-side inside its area. The Area record's `section_count` is
//! load-bearing — the engine loops on it rather than scanning — so both ops adjust it.
//!
//! Adding clones a sibling section's whole bracket (band tags are automatically correct — the
//! donor is in the same area) with the donor's object runs excised, then patches name, height
//! and `object_count = 0`. Removing deletes the bracket — refused for a non-empty section
//! (engine assert 0x56a078 requires an empty one), for an area's last section
//! (`sections.size() > 0`), and when the bracket hosts the area's guides (`RulerScale`) — and
//! decrements `end_section_index` on every drawing above the removed section's canonical index
//! (the only stored section reference anywhere in the format).

use rpt_reader::fields::FieldEdit;
use rpt_reader::model;
use rpt_reader::{Rpt, StreamId};

use crate::locate::{RecAddr};
use crate::structure;

/// `0xa9` — a drawing object (the only kind storing a section index).
const T_DRAWING: u16 = 0x00a9;

/// `addSection {area, after?, name?, height?}` — insert a fresh, empty section into `area`,
/// positioned after the section named `after` (or last when omitted).
pub fn add_section(
    rpt: &Rpt,
    area_name: &str,
    after: Option<&str>,
    name: Option<&str>,
    height: Option<i64>,
) -> Result<Vec<u8>, String> {
    let located = crate::locate::locate(rpt);
    let area = located
        .areas
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(area_name))
        .ok_or_else(|| format!("no area named `{area_name}`"))?;
    if area.sections.is_empty() {
        return Err(format!("area `{area_name}` has no section to clone from", ));
    }
    let donor_pos = match after {
        Some(a) => area
            .sections
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(a))
            .ok_or_else(|| format!("no section `{a}` in area `{area_name}`"))?,
        None => area.sections.len() - 1,
    };
    let donor = &area.sections[donor_pos];
    let band = donor.band_run.as_ref().ok_or("the donor section's band bracket was not located")?;

    // Clone the bracket with everything between the format records and the BandEnd marker
    // excised — the donor's objects AND any per-area RulerScale the donor's bracket hosts
    // (BandEnd is a 4-byte empty marker, so the tail to keep is the last 4 bytes).
    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();
    let ruler_start = tree
        .iter()
        .find(|n| n.rtype == 0x010a && n.offset >= band.range.start && n.content_end <= band.range.end)
        .map(|n| n.offset);
    let cut = donor
        .objects
        .first()
        .and_then(|o| o.run.as_ref())
        .map(|r| r.range.start)
        .or(ruler_start);
    let mut clone: Vec<u8> = logical
        .get(band.range.clone())
        .ok_or("band bracket outside the stream")?
        .to_vec();
    if let Some(cut) = cut {
        let lo = cut - band.range.start;
        let hi = clone.len() - 4; // keep the BandEnd marker
        clone.drain(lo..hi);
    }

    // Splice right after the donor's BandEnd; identity re-mask (same area, same depth).
    let c = structure::Contents::of(rpt)?;
    let anchor = crate::locate::InsertAnchor { at: band.range.end, outer_mask: band.outer_mask };
    let mut out = structure::insert_at(rpt, &c, &anchor, &clone)?;

    // section_count +1 (the engine loops on it), and shift drawing end-indices at/after the
    // insertion point (the new section takes a canonical slot, later sections move up).
    let area_rec = area.name_rec.ok_or("area record not located")?;
    out = adjust_section_count(&out, area_rec, 1)?;
    let new_idx = donor_pos as i64
        + canonical_prefix(rpt.report(), &area.name, donor_pos)
        .ok_or("canonical offset of the donor section not found")?;
    let mut shifted = drawings_where(rpt, new_idx, std::cmp::Ordering::Greater, |v| v + 1);
    shifted.extend(drawings_where(rpt, new_idx, std::cmp::Ordering::Equal, |v| v + 1));
    for (addr, _, v) in shifted {
        out = patch(&out, addr, "end_section_index", FieldEdit::Int(v))?;
    }

    // Reopen: the new section sits right after the donor; patch identity fields.
    let opened = crate::open_bytes(&out)?;
    let rel = crate::locate::locate(&opened);
    let area2 = rel
        .areas
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(area_name))
        .ok_or("area vanished after insert")?;
    let new_sec = area2.sections.get(donor_pos + 1).ok_or("the cloned section did not land after its donor")?;
    let sec_rec = new_sec.section_rec.ok_or("cloned section record missing")?;

    let donor_height = donor
        .section_rec
        .and_then(|r| read_i64(&opened, r, "height").ok())
        .unwrap_or(0);
    let auto_name = next_section_name(rpt.report());
    let want_name = name.map(str::to_string).unwrap_or(auto_name);
    let want_height = height.unwrap_or(donor_height);

    out = patch(&out, sec_rec, "name", FieldEdit::Text(want_name.clone()))?;
    out = patch(&out, sec_rec, "height", FieldEdit::Int(want_height))?;
    out = patch(&out, sec_rec, "object_count", FieldEdit::Int(0))?;

    // Verify: the model lists the new section, empty, and the validator still passes.
    let check = crate::open_bytes(&out)?;
    let m = check.report();
    let a = m.report_definition.areas.iter()
        .find(|a| a.name.eq_ignore_ascii_case(area_name)).ok_or("area missing in model")?;
    let added = a
        .sections
        .iter()
        .find(|s| s.name == want_name)
        .ok_or_else(|| format!("the new section `{want_name}` did not register"))?;
    if !added.objects.is_empty() {
        return Err("the cloned section carried objects".to_string());
    }
    let vr = crate::validate::validate(&check);
    if let Some(f) = vr.errors().next() {
        return Err(format!("validator rejected the result: {}", f.message));
    }
    Ok(out)
}

/// `removeSection {area?, section}` — delete an empty section's band bracket, fix the
/// canonical `end_section_index` on drawings above it.
pub fn remove_section(rpt: &Rpt, area_name: Option<&str>, section_name: &str) -> Result<Vec<u8>, String> {
    let located = crate::locate::locate(rpt);
    let sel = crate::edit::SecSel {
        area: area_name.map(str::to_string),
        section: section_name.to_string(),
    };
    let sec = crate::edit::find_section_by_names(&located, &sel)?;
    let area = located
        .areas
        .iter()
        .find(|a| a.sections.iter().any(|s| s.name == sec.name))
        .ok_or("the section's area vanished")?;

    if area.sections.len() <= 1 {
        return Err(format!(
            "refused: `{}` is the last section of area `{}` — an area must keep at least one \
             (engine: sections.size() > 0)",
            sec.name, area.name
        ));
    }
    if !sec.objects.is_empty() {
        return Err(format!(
            "refused: section `{}` holds {} object(s); remove them first (engine path requires \
             an empty section)",
            sec.name,
            sec.objects.len()
        ));
    }
    let band = sec.band_run.as_ref().ok_or("the section's band bracket was not located")?;

    // Guides: the per-area RulerScale may live inside this bracket — refuse rather than
    // silently destroy it.
    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let tree = stream.record_tree();
    let has_ruler = tree.iter().any(|root| {
        root.offset >= band.range.start
            && root.content_end <= band.range.end
            && root.rtype == 0x010a
    });
    if has_ruler {
        return Err(format!(
            "refused: section `{}` hosts the area's guides (RulerScale); move them before removing",
            sec.name
        ));
    }

    // Canonical global index of the section (the model's areas are canonically sorted).
    let report = rpt.report();
    let canonical_idx = canonical_section_index(report, &area.name, &sec.name).map(|i| i as i64)
        .ok_or("the section has no canonical index")?;

    // Drawings: refuse spans INTO the section; collect above-index fixups.
    if let Some((_, name, _)) =
        drawings_where(rpt, canonical_idx, std::cmp::Ordering::Equal, |v| v).first()
    {
        return Err(format!(
            "refused: drawing `{name}` spans into section `{}` (its end_section_index names it)",
            sec.name
        ));
    }
    let fixups: Vec<(RecAddr, i64)> = drawings_where(rpt, canonical_idx, std::cmp::Ordering::Greater, |v| v - 1)
        .into_iter()
        .map(|(addr, _, v)| (addr, v))
        .collect();

    // Delete the bracket, adjust the count, fix the drawings.
    let c = structure::Contents::of(rpt)?;
    let mut out = structure::delete_run(rpt, &c, band)?;
    let area_rec = area.name_rec.ok_or("area record not located")?;
    out = adjust_section_count(&out, area_rec, -1)?;
    for (addr, v) in fixups {
        out = patch(&out, addr, "end_section_index", FieldEdit::Int(v))?;
    }

    // Verify: the section is gone and the validator still passes.
    let check = crate::open_bytes(&out)?;
    let m = check.report();
    let still = m.report_definition.areas.iter()
        .flat_map(|a| a.sections.iter())
        .any(|s| s.name == sec.name);
    if still {
        return Err(format!("section `{}` survived its own removal", sec.name));
    }
    let vr = crate::validate::validate(&check);
    if let Some(f) = vr.errors().next() {
        return Err(format!("validator rejected the result: {}", f.message));
    }
    Ok(out)
}

/* ---------------- helpers ---------------- */

/// How many sections precede `section_pos`-in-area `area_name` in canonical order.
fn canonical_prefix(report: &model::Report, area_name: &str, section_pos: usize) -> Option<i64> {
    let mut idx = 0i64;
    for a in &report.report_definition.areas {
        if a.name.eq_ignore_ascii_case(area_name) {
            return Some(idx + section_pos as i64);
        }
        idx += a.sections.len() as i64;
    }
    None
}

/// Every drawing whose end_section_index compares to `pivot` exactly as `ord`, mapped
/// through `fix`; entries carry the object name for refusal messages.
fn drawings_where(
    rpt: &Rpt,
    pivot: i64,
    ord: std::cmp::Ordering,
    fix: impl Fn(i64) -> i64,
) -> Vec<(RecAddr, String, i64)> {
    let mut out = Vec::new();
    let located = crate::locate::locate(rpt);
    for a in &located.areas {
        for s in &a.sections {
            for obj in &s.objects {
                if obj.opener_tag != T_DRAWING {
                    continue;
                }
                let Some(addr) = obj.opener else { continue };
                let Ok(f) = rpt.record_field(
                    rpt_reader::raw::RecordTag(addr.tag),
                    addr.nth,
                    "end_section_index",
                ) else {
                    continue;
                };
                let v = match f.value {
                    rpt_reader::fields::FieldValue::Int(v) => i64::from(v),
                    rpt_reader::fields::FieldValue::Uint(v) => i64::from(v),
                    _ => continue,
                };
                if v.cmp(&pivot) == ord {
                    out.push((addr, obj.name.clone(), fix(v)));
                }
            }
        }
    }
    out
}

/// Patch one field on a record by address through the verified writer.
fn patch(bytes: &[u8], addr: RecAddr, field: &str, value: FieldEdit) -> Result<Vec<u8>, String> {
    crate::edit::patch_pub(bytes, &addr, field, value)
}

fn read_i64(rpt: &Rpt, addr: RecAddr, field: &str) -> Result<i64, String> {
    let f = rpt
        .record_field(rpt_reader::raw::RecordTag(addr.tag), addr.nth, field)
        .map_err(|e| format!("read {field}: {e}"))?;
    match f.value {
        rpt_reader::fields::FieldValue::Int(v) => Ok(i64::from(v)),
        rpt_reader::fields::FieldValue::Uint(v) => Ok(i64::from(v)),
        other => Err(format!("{field} is not an integer: {other:?}")),
    }
}

/// The engine's section naming: a report-global `SectionN` counter with gaps that persist.
/// The next free name = max N + 1.
fn next_section_name(report: &model::Report) -> String {
    let max = report
        .report_definition
        .areas
        .iter()
        .flat_map(|a| a.sections.iter())
        .filter_map(|s| s.name.strip_prefix("Section").and_then(|n| n.parse::<u32>().ok()))
        .max()
        .unwrap_or(0);
    format!("Section{}", max + 1)
}

/// The section's index in the flattened canonical order (the model's areas are already
/// canonically sorted by kind + group level; sections follow in document order within each).
fn canonical_section_index(report: &model::Report, area: &str, section: &str) -> Option<usize> {
    let mut idx = 0;
    for a in &report.report_definition.areas {
        for s in &a.sections {
            if a.name.eq_ignore_ascii_case(area) && s.name.eq_ignore_ascii_case(section) {
                return Some(idx);
            }
            idx += 1;
        }
    }
    None
}

/// Read-modify-write the Area record's `section_count` (the engine loops on it rather than
/// scanning) through the verified writer — the section twin of `adjust_object_count`.
fn adjust_section_count(bytes: &[u8], area_rec: RecAddr, delta: i64) -> Result<Vec<u8>, String> {
    let rpt = crate::open_bytes(bytes)?;
    let current = read_i64(&rpt, area_rec, "section_count")?;
    let new = current + delta;
    if new < 0 {
        return Err(format!("section_count would go negative ({current} {delta:+})"));
    }
    patch(bytes, area_rec, "section_count", FieldEdit::Int(new))
}

