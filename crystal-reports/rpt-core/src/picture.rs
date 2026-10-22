//! `addPicture` — place an image file as a static picture object.
//!
//! A static picture's bytes live in an `Embedding N` storage's `CONTENTS` stream (raw image
//! bytes; the reader loads exactly that — `build_model/picture.rs`), pointed at by the
//! `0x00bd` record's `embedding_ordinal` inside the picture object's run. Adding one is
//! therefore: copy a donor picture's whole `Embedding` storage under a fresh ordinal
//! (preserving whatever `\x01Ole`/`\x01CompObj` metadata streams it carries), overwrite
//! `CONTENTS` with the new image bytes, then donor-clone the picture object run and rebind
//! its `0xbd` ordinal.

use rpt_reader::fields::FieldEdit;
use rpt_reader::Rpt;

use crate::structure;

/// `0xae` — the static-picture opener (also a chart's graphic base; donors are filtered by
/// having a `0xbd` sibling, which only static pictures carry).
const T_PICTURE: u16 = 0x00ae;

/// Place `image_path`'s bytes as a new static picture `name` at the requested geometry.
pub fn add_picture(
    rpt: &Rpt,
    section: &crate::edit::SecSel,
    name: &str,
    image_path: &str,
    left: i64,
    top: i64,
    width: i64,
    height: i64,
) -> Result<Vec<u8>, String> {
    let image = std::fs::read(image_path)
        .map_err(|e| format!("cannot read the image `{image_path}`: {e}"))?;
    if image.is_empty() {
        return Err("the image file is empty".to_string());
    }

    // 1. Locate a static-picture donor (an 0xae object whose run carries a 0xbd record).
    let located = crate::locate::locate(rpt);
    let mut donor: Option<(structure::Donor, u32)> = None;
    'find: for area in &located.areas {
        for sec in &area.sections {
            for obj in &sec.objects {
                if obj.opener_tag != T_PICTURE || obj.bracket_tag != 0x00af || obj.run.is_none() {
                    continue;
                }
                // The 0xbd ordinal record inside the run names the donor storage — static
                // pictures carry one; chart graphic bases do not.
                if let Some(ord) = read_embedding_ordinal(rpt, obj) {
                    donor = Some((
                        structure::Donor {
                            object: obj.clone(),
                            area: area.name.clone(),
                            section: sec.name.clone(),
                        },
                        ord,
                    ));
                    break 'find;
                }
            }
        }
    }
    let (_donor, donor_ordinal) = donor.ok_or(
        "this report has no static picture to clone from; addPicture needs an in-report donor",
    )?;
    // 2. Fresh Embedding N = copy of the donor storage with CONTENTS replaced by the image.
    let index = structure::max_embedding_index(rpt) + 1;
    let with_embedding =
        structure::create_embedding(rpt.original_bytes(), donor_ordinal, index, &image)?;

    // 3. Clone the picture object run and rebind name/geometry + the 0xbd ordinal.
    let opened = crate::open_bytes(&with_embedding)?;
    let finished = crate::edit::add_object_inner(
        &opened,
        section,
        name,
        left,
        top,
        width,
        height,
        |obj| {
            obj.opener_tag == T_PICTURE
                && obj.bracket_tag == 0x00af // static picture, not a blob-field one
                && read_embedding_ordinal(&opened, obj).is_some()
        },
        &mut |current, _addrs| {
            // The 0xbd record sits inside the clone's run; address it via a fresh locate.
            let reopened = crate::open_bytes(current)?;
            let rel = crate::locate::locate(&reopened);
            let sec2 = crate::edit::find_section_by_names(&rel, section)?;
            let clone = sec2.objects.last().ok_or("clone missing after insert")?;
            let ord_addr = clone.ordinal.as_ref().ok_or("the clone carries no 0xbd record")?;
            crate::edit::patch_pub(current, ord_addr, "embedding_ordinal", FieldEdit::Int(i64::from(index)))
        },
    )?;

    // 4. Verify: the model loads the picture's bytes from the new Embedding CONTENTS.
    let check = crate::open_bytes(&finished)?;
    let report = check.report();
    let pic = report
        .report_definition
        .areas
        .iter()
        .flat_map(|a| a.sections.iter().flat_map(|s| s.objects.iter()))
        .find(|o| o.name == name && matches!(o.kind, rpt_reader::model::ReportObjectKind::Picture(_)))
        .ok_or("the placed picture did not register in the model")?;
    match o_data(pic) {
        Some(data) if data == image.as_slice() => Ok(finished),
        Some(data) => Err(format!(
            "the picture's Embedding CONTENTS read back {} bytes, expected {}",
            data.len(),
            image.len()
        )),
        None => Err("the picture's bytes did not load from its Embedding storage".to_string()),
    }
}

fn o_data(obj: &rpt_reader::model::ReportObject) -> Option<&[u8]> {
    match &obj.kind {
        rpt_reader::model::ReportObjectKind::Picture(p) => Some(p.data.as_slice()),
        _ => None,
    }
}

/// The embedding ordinal of a picture object, read from the `0xbd` sibling in its run —
/// `None` when the run carries none (chart graphic bases and blob-field wrappers).
fn read_embedding_ordinal(rpt: &Rpt, obj: &crate::locate::LocatedObject) -> Option<u32> {
    // The 0xbd record follows the 0xae opener as a sibling; find it by scanning the run's
    // records via the stream fields API, keyed by the run's byte range.
    let addr = obj.ordinal?;
    let f = rpt
        .record_field(rpt_reader::raw::RecordTag(addr.tag), addr.nth, "embedding_ordinal")
        .ok()?;
    match f.value {
        rpt_reader::fields::FieldValue::Uint(v) => Some(v),
        _ => None,
    }
}

