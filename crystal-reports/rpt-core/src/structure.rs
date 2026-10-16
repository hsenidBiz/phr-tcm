//! Structural edits: cloning, relocating and deleting whole record runs in the `Contents`
//! stream — the machinery behind addText/addLine/addBox/addField/addSpecialField and
//! removeObject.
//!
//! The engine writes each report object as a **contiguous run of sibling records** — its opener
//! (or the `0xaa` wrapper for drawings) through its end marker — between the section's format
//! records and the area's trailing decorations. Because TSLV records are self-contained frames
//! (no absolute offsets) and the running XOR mask unwinds inside a closed run, a run can be
//! copied verbatim and re-inserted anywhere at the same nesting depth after a uniform re-mask
//! (see `framing`).
//!
//! Adding an object is therefore *donor cloning*: take an existing object of the same kind from
//! the same report, copy its run, re-mask it to the target location, splice it at the target
//! section's anchor, then patch the clone's name/geometry/text through the verified field
//! writer. Removing an object is the inverse splice, plus the section's `object_count` fixup in
//! both directions.

use rpt_reader::raw::RecordNode;
use rpt_reader::{fields, fields::FieldEdit, model, Rpt, StreamId};

use crate::framing;
use crate::locate::{InsertAnchor, Located, LocatedObject, RecAddr, RunInfo};

/// A donor-clone failure: what was wanted, what the report could offer.
#[derive(Debug)]
pub struct NoDonor {
    /// Human-readable explanation.
    pub message: String,
}

/// Everything a clone needs to know about its donor.
pub struct Donor {
    /// The donor object's located entry (its `run` is the bytes to copy).
    pub object: LocatedObject,
    /// The donor's containing area name.
    pub area: String,
    /// The donor's containing section name.
    pub section: String,
}

/// Find an object by name, case-insensitively (the designer's own lookup is case-insensitive).
/// Returns the located object plus its container names.
pub fn find_object_by_name(located: &Located, name: &str) -> Result<Donor, String> {
    let mut found: Vec<Donor> = Vec::new();
    for area in &located.areas {
        for section in &area.sections {
            for obj in &section.objects {
                if obj.name.eq_ignore_ascii_case(name) {
                    found.push(Donor {
                        object: obj.clone(),
                        area: area.name.clone(),
                        section: section.name.clone(),
                    });
                }
            }
        }
    }
    match found.len() {
        0 => Err(format!("no object named `{name}`")),
        1 => Ok(found.pop().expect("len checked")),
        _ => Err(format!(
            "`{name}` matches {} objects; names must be unique",
            found.len()
        )),
    }
}

/// Find a donor for a clone: the first object whose run is known and that passes `filter`.
pub fn find_donor<F>(located: &Located, filter: F) -> Result<Donor, String>
where
    F: Fn(&LocatedObject) -> bool,
{
    for area in &located.areas {
        for section in &area.sections {
            for obj in &section.objects {
                if obj.run.is_some() && filter(obj) {
                    return Ok(Donor {
                        object: obj.clone(),
                        area: area.name.clone(),
                        section: section.name.clone(),
                    });
                }
            }
        }
    }
    Err("no object of the needed kind to clone from in this report".to_string())
}

/// The Contents logical bytes and record tree of an open report.
pub struct Contents {
    /// The logical (inflated) bytes.
    pub logical: Vec<u8>,
    /// The parsed record tree.
    pub tree: Vec<RecordNode>,
}

impl Contents {
    /// Read the Contents stream of `rpt`.
    pub fn of(rpt: &Rpt) -> Result<Contents, String> {
        let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
        Ok(Contents {
            logical: stream.logical_bytes().to_vec(),
            tree: stream.record_tree(),
        })
    }
}

/// Splice `bytes` into the logical stream at `anchor` (an insertion point between records)
/// and return the new `.rpt` file bytes.
pub fn insert_at(rpt: &Rpt, c: &Contents, anchor: &InsertAnchor, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let new_logical = framing::splice(&c.logical, &c.tree, anchor.at..anchor.at, bytes)?;
    rpt.write_contents_logical(&new_logical)
        .map_err(|e| format!("write after insert: {e}"))
}

/// Delete the byte range of `run` from the logical stream and return the new file bytes.
pub fn delete_run(rpt: &Rpt, c: &Contents, run: &RunInfo) -> Result<Vec<u8>, String> {
    let new_logical = framing::splice(&c.logical, &c.tree, run.range.clone(), &[])?;
    rpt.write_contents_logical(&new_logical)
        .map_err(|e| format!("write after delete: {e}"))
}

/// Extract an object's run bytes, re-masked from the donor's context to the anchor's.
pub fn clone_run_bytes(c: &Contents, run: &RunInfo, anchor: &InsertAnchor) -> Result<Vec<u8>, String> {
    let raw = c
        .logical
        .get(run.range.clone())
        .ok_or_else(|| format!("run {:?} outside the stream", run.range))?;
    Ok(framing::remask_run(raw, run.outer_mask, anchor.outer_mask))
}

/// Adjust a section's `object_count` by `delta` (after insert/delete), via the verified writer.
pub fn adjust_object_count(bytes: &[u8], section_rec: RecAddr, delta: i64) -> Result<Vec<u8>, String> {
    let rpt = crate::open_bytes(bytes)?;
    let current = read_field_i64(&rpt, section_rec, "object_count")?;
    let new = (current + delta).max(0);
    if new == current {
        return Ok(bytes.to_vec());
    }
    rpt.patch_record_field_with(
        rpt_reader::raw::RecordTag(section_rec.tag),
        section_rec.nth,
        "object_count",
        &FieldEdit::Int(new),
        rpt_reader::EditPolicy::Checked,
    )
    .map_err(|e| format!("object_count {current} -> {new}: {e}"))
}

/// Read one integer field of a record by address.
pub fn read_field_i64(rpt: &Rpt, addr: RecAddr, field: &str) -> Result<i64, String> {
    let f = rpt
        .record_field(rpt_reader::raw::RecordTag(addr.tag), addr.nth, field)
        .map_err(|e| format!("cannot read {} `{}`: {e}", addr.key(), field))?;
    match f.value {
        fields::FieldValue::Uint(v) => Ok(i64::from(v)),
        fields::FieldValue::Int(v) => Ok(i64::from(v)),
        other => Err(format!("{} `{}` is not an integer: {other:?}", addr.key(), field)),
    }
}

/// The model object mirroring a located one (paired by container names + object name).
pub fn model_object<'m>(
    report: &'m model::Report,
    area: &str,
    section: &str,
    name: &str,
) -> Result<&'m model::ReportObject, String> {
    for a in &report.report_definition.areas {
        if !a.name.eq_ignore_ascii_case(area) {
            continue;
        }
        for s in &a.sections {
            if !s.name.eq_ignore_ascii_case(section) {
                continue;
            }
            for o in &s.objects {
                if o.name.eq_ignore_ascii_case(name) {
                    return Ok(o);
                }
            }
        }
    }
    Err(format!("no model object `{name}` in {area}/{section}"))
}

/// The embedded seed report: the blank plus one Text, one Line and one Box — the donor
/// fallback when the target report has no object of a needed kind (a fresh blank has none).
/// Internal only; built with the editor's own cross-report copyObject and load-path checked.
static SEED: &[u8] = include_bytes!("seed.rpt");

/// Locate a donor in the embedded seed report, for targets with no object of the kind.
pub fn seed_donor(filter: impl Fn(&LocatedObject) -> bool) -> Result<(Donor, Vec<u8>), String> {
    let rpt = crate::open_bytes(SEED)?;
    let located = crate::locate::locate(&rpt);
    let donor = find_donor(&located, &filter)?;
    let c = Contents::of(&rpt)?;
    let run = donor.object.run.as_ref().ok_or("seed donor run not located")?;
    let bytes = c
        .logical
        .get(run.range.clone())
        .ok_or("seed donor run outside its stream")?
        .to_vec();
    Ok((donor, bytes))
}

/// A seed donor drawing (Line or Box), filtered by the `0xec` shape kind (1 = box, 2 = line).
pub fn seed_donor_shape(want_shape: i64) -> Result<(Donor, Vec<u8>), String> {
    let rpt = crate::open_bytes(SEED)?;
    let located = crate::locate::locate(&rpt);
    for area in &located.areas {
        for sec in &area.sections {
            for obj in &sec.objects {
                if obj.opener_tag != 0x00a9 || obj.run.is_none() {
                    continue;
                }
                if let Some(border) = obj.border {
                    if read_field_i64(&rpt, border, "shape_kind").ok() == Some(want_shape) {
                        let donor = Donor {
                            object: obj.clone(),
                            area: area.name.clone(),
                            section: sec.name.clone(),
                        };
                        let c = Contents::of(&rpt)?;
                        let run = donor.object.run.as_ref().ok_or("seed run not located")?;
                        let bytes = c
                            .logical
                            .get(run.range.clone())
                            .ok_or("seed run outside its stream")?
                            .to_vec();
                        return Ok((donor, bytes));
                    }
                }
            }
        }
    }
    Err("no drawing of that shape in the seed".to_string())
}

/* ---------------- container surgery (subreports) ---------------- */

/// The highest `Subdocument N` ordinal in the report's container (0 when none).
pub fn max_subdocument_index(rpt: &Rpt) -> u32 {
    rpt.streams()
        .filter_map(|(id, _)| match id {
            StreamId::Other(path) => path
                .strip_prefix("Subdocument ")
                .and_then(|rest| rest.split('/').next())
                .and_then(|n| n.parse::<u32>().ok()),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// Copy every stream and storage of `source_file_bytes` (a standalone `.rpt`) into
/// `target_bytes` under a fresh `Subdocument {index}` storage — except the OLE
/// SummaryInformation, which real subdocuments do not carry. Returns the new container bytes.
pub fn import_subdocument(target_bytes: &[u8], index: u32, source_file_bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut source = cfb::CompoundFile::open(std::io::Cursor::new(source_file_bytes.to_vec()))
        .map_err(|e| format!("open the subreport file: {e}"))?;
    let mut target = cfb::CompoundFile::open(std::io::Cursor::new(target_bytes.to_vec()))
        .map_err(|e| format!("open the report: {e}"))?;

    // Every entry path under the source root, files only, in walk order (parents first).
    let entries: Vec<std::path::PathBuf> = source
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_path_buf())
        .collect();
    for path in entries {
        let name = path
            .components()
            .last()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or_default()
            .to_string();
        if name.starts_with("\u{5}Summary") {
            continue;
        }
        let mut bytes = Vec::new();
        source
            .open_stream(&path)
            .map_err(|e| format!("read the subreport stream `{}`: {e}", path.display()))?
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        source
            .open_stream(&path)
            .map_err(|e| e.to_string())?
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let dest = std::path::Path::new("/")
            .join(format!("Subdocument {index}"))
            .join(path.strip_prefix("/").unwrap_or(&path));
        if let Some(parent) = dest.parent() {
            if !target.exists(parent) {
                target
                    .create_storage_all(parent)
                    .map_err(|e| format!("create `{}`: {e}", parent.display()))?;
            }
        }
        let mut stream = target
            .create_stream(&dest)
            .map_err(|e| format!("create `{}`: {e}", dest.display()))?;
        stream.write_all(&bytes).map_err(|e| e.to_string())?;
        stream.flush().map_err(|e| e.to_string())?;
    }
    target.flush().map_err(|e| format!("flush the report: {e}"))?;
    Ok(target.into_inner().into_inner())
}

/* ---------------- Embedding storage surgery (addPicture) ---------------- */

/// The highest `Embedding N` ordinal in the report's container (0 when none).
pub fn max_embedding_index(rpt: &Rpt) -> u32 {
    rpt.streams()
        .filter_map(|(id, _)| match id {
            // `Embedding N` storages classify as their own StreamId variant; its payload is
            // the path beneath the storage ("N\CONTENTS" — platform separator).
            StreamId::Embedding(path) => {
                let digits: String = path
                    .trim()
                    .trim_start_matches(|c: char| !c.is_ascii_digit())
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                digits.parse::<u32>().ok()
            }
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// Create `Embedding {index}` as a copy of the donor storage `Embedding {donor}` with its
/// `CONTENTS` stream replaced by `image` — preserving any `\x01Ole`/`\x01CompObj` metadata
/// streams the engine may want. Returns the new container bytes.
pub fn create_embedding(
    target_bytes: &[u8],
    donor: u32,
    index: u32,
    image: &[u8],
) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut target = cfb::CompoundFile::open(std::io::Cursor::new(target_bytes.to_vec()))
        .map_err(|e| format!("open the report container: {e}"))?;
    let entries: Vec<std::path::PathBuf> = target
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_path_buf())
        .collect();
    // Walk paths use the platform separator; match by components, not string prefix.
    let donor_storage = format!("Embedding {donor}");
    let mut copied: Vec<String> = Vec::new();
    for path in entries {
        let mut comps = path.components().skip_while(|c| std::path::Component::RootDir.eq(c));
        let head = comps.next();
        if head.is_none() {
            continue;
        }
        // Strip any OLE control-char prefix on the storage name ("Embedding…") for matching.
        let head_str = head.as_ref().unwrap().as_os_str().to_string_lossy().into_owned();
        if !head_str.trim_matches(char::is_control).eq_ignore_ascii_case(&donor_storage) {
            continue;
        }
        let rest: std::path::PathBuf = comps.collect();
        if rest.as_os_str().is_empty() {
            continue;
        }
        let rest = rest.to_string_lossy().trim_matches(char::is_control).to_string();
        let mut bytes = Vec::new();
        {
            let mut s = target.open_stream(&path).map_err(|e| e.to_string())?;
            s.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            s.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        }
        let dest = std::path::Path::new("/")
            .join(format!("Embedding {index}"))
            .join(std::path::Path::new(&rest).components().collect::<std::path::PathBuf>());
        if let Some(parent) = dest.parent() {
            if !target.exists(parent) {
                target.create_storage_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let body = if rest.eq_ignore_ascii_case("CONTENTS") { image.to_vec() } else { bytes };
        let mut s = target.create_stream(&dest).map_err(|e| format!("create `{}`: {e}", dest.display()))?;
        s.write_all(&body).map_err(|e| e.to_string())?;
        s.flush().map_err(|e| e.to_string())?;
        copied.push(rest);
    }
    if copied.is_empty() {
        return Err(format!("the donor storage `Embedding {donor}` has no streams to copy"));
    }
    target.flush().map_err(|e| e.to_string())?;
    Ok(target.into_inner().into_inner())
}

/* ---------------- cross-report copyObject ---------------- */

/// `copyObject` — clone a single-run Text / Line / Box object from another report's file into
/// a section of this one. Those kinds' core records carry no field-pool handles, so the copy
/// is bytes-safe (see docs/vibey-operations.md for the condition-formula caveat).
pub fn copy_object(
    target: &Rpt,
    from_report: &str,
    object_name: &str,
    section: &crate::edit::SecSel,
    new_name: Option<&str>,
) -> Result<Vec<u8>, String> {
    let donor_rpt = crate::open(from_report)?;
    let donor_located = crate::locate::locate(&donor_rpt);
    let donor = find_object_by_name(&donor_located, object_name)?;
    let run = donor.object.run.as_ref().ok_or("donor run not located")?;

    // Kind gate: opener tags 0xa5 text (single-run, no embedded fields), 0xa9 drawing.
    match donor.object.opener_tag {
        0x00a5 => {
            if donor.object.embedded_fields > 0 {
                return Err(
                    "the text object has embedded field references; copying it would carry \
                     donor-pool handles (not supported in v1)"
                        .to_string(),
                );
            }
        }
        0x00a9 => {}
        other => {
            return Err(format!(
                "copyObject v1 supports single-run Text, Line and Box objects; this object's \
                 opener is {other:#06x} (pool/stream handles would not survive the copy)"
            ));
        }
    }

    // Read the donor run bytes from the DONOR's Contents.
    let donor_contents = Contents::of(&donor_rpt)?;
    let raw = donor_contents
        .logical
        .get(run.range.clone())
        .ok_or("donor run outside its stream")?;

    // Target section anchor (append after its last object, else the section's insert anchor).
    let target_located = crate::locate::locate(target);
    let sec = crate::edit::find_section_by_names(&target_located, section)?;
    let anchor = crate::edit::append_anchor_pub(sec)?;
    let bytes = crate::framing::remask_run(raw, run.outer_mask, anchor.outer_mask);

    let target_contents = Contents::of(target)?;
    let out = insert_at(target, &target_contents, &anchor, &bytes)?;
    let out = adjust_object_count(&out, sec.section_rec.ok_or("section record missing")?, 1)?;

    // Optional rename, then verify presence + kind by model.
    let mut current = out;
    if let Some(n) = new_name {
        let reopened = crate::open_bytes(&current)?;
        let rel = crate::locate::locate(&reopened);
        let sec2 = crate::edit::find_section_by_names(&rel, section)?;
        let clone = sec2.objects.last().ok_or("clone missing after insert")?;
        let name_addr = clone.name_rec.ok_or("clone has no name record")?;
        current = crate::edit::patch_pub(
            &current,
            &name_addr,
            "name",
            rpt_reader::fields::FieldEdit::Text(n.to_string()),
        )?;
    }

    let check = crate::open_bytes(&current)?;
    let located2 = crate::locate::locate(&check);
    let sec2 = crate::edit::find_section_by_names(&located2, section)?;
    let want_name = new_name.unwrap_or(donor.object.name.as_str());
    let clone = sec2
        .objects
        .last()
        .ok_or("the copied object did not land last in its section")?;
    if !clone.name.eq_ignore_ascii_case(want_name) || clone.opener_tag != donor.object.opener_tag {
        return Err(format!(
            "the copy read back as `{}` (opener {:#06x}), expected `{want_name}` ({:#06x})",
            clone.name, clone.opener_tag, donor.object.opener_tag
        ));
    }
    Ok(current)
}
