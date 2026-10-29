//! `create_report()` — a blank `.rpt` from nothing: the end of the donor-file restriction.
//!
//! Every editing op donor-clones from an opened report (the same wall Vibey's SDK workflow
//! hit — always start from an existing file). This module removes the wall: an
//! **engine-authored blank** is embedded as bytes and re-issued, with the caller's options
//! applied through the same verified ops the editor uses everywhere else.
//!
//! Why an embedded specimen rather than synthesis: the one untested surface of from-scratch is
//! whether the real engine accepts the file, and an authentic engine-authored blank (0 objects,
//! the standard five areas, an empty query session) is the strongest possible answer — better
//! than re-deriving the save path could tell us. The blank is
//! `VibeyReports/out/reports/_shell_logo.rpt` (CR 11.0) with its one residual QESession table
//! removed via the normal `remove_table` op; it passes the load-path validator 0/0 and
//! round-trips byte-identically. A pure-synthesis follow-up (phase B) is documented in
//! `docs/engine-notes.md` and would diff its output against this golden skeleton.
//!
//! Orientation / paper size: a fresh blank carries no page-DEVMODE record (saved with no
//! printer), so these options SYNTHESIZE one first (`pagesetup.rs`) and then patch it —
//! the same path as editing any DEVMODE-less report.

use serde::{Deserialize, Serialize};

use crate::EditOp;
use rpt_reader::Rpt;

/// The embedded engine-authored blank (see module docs). ~7 KB.
static BLANK: &[u8] = include_bytes!("blank.rpt");

/// Page margins in twips for [`NewReportOptions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Margins {
    /// Left margin, twips.
    pub left: i64,
    /// Right margin, twips.
    pub right: i64,
    /// Top margin, twips.
    pub top: i64,
    /// Bottom margin, twips.
    pub bottom: i64,
}

/// Options for [`create_report`]. Everything is optional; the blank's own values apply
/// otherwise.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewReportOptions {
    /// Document title (SummaryInformation PID_TITLE).
    pub title: Option<String>,
    /// Document author (PID_AUTHOR).
    pub author: Option<String>,
    /// Free-form comments (PID_COMMENTS).
    pub comments: Option<String>,
    /// `portrait` / `landscape` — refused when the blank lacks a DEVMODE record (it does;
    /// see module docs).
    pub orientation: Option<String>,
    /// Windows DMPAPER id (1=letter, 9=A4, …) — same synthesis path as orientation.
    pub paper_size: Option<u16>,
    /// Page margins in twips.
    pub margins: Option<Margins>,
}

/// Create a fresh, blank `.rpt` and return its bytes — openable by the editor, the reader,
/// and (as far as the offline load-path validator can attest) the real engine.
///
/// # Errors
///
/// Refuses (message) if any requested option cannot be applied — notably orientation/paper
/// without a DEVMODE record — or if the result would fail the load-path validator (it never
/// should; that check is the creation gate).
pub fn create_report(opts: &NewReportOptions) -> Result<Vec<u8>, String> {
    let rpt: Rpt = crate::open_bytes(BLANK)?;
    let mut current = rpt.reencode().map_err(|e| format!("reencode blank: {e}"))?;

    if let Some(m) = &opts.margins {
        current = apply(
            &current,
            EditOp::SetMargins {
                left: Some(m.left),
                right: Some(m.right),
                top: Some(m.top),
                bottom: Some(m.bottom),
            },
        )?;
    }
    if let Some(o) = &opts.orientation {
        current = apply(&current, EditOp::SetOrientation { orientation: o.clone() })?;
    }
    if let Some(p) = opts.paper_size {
        current = apply(&current, EditOp::SetPaperSize { paper: p })?;
    }

    // Title/author/comments live in the OLE SummaryInformation property set — an optional
    // stream for the reader, so a fresh minimal set is written whenever any is given.
    if opts.title.is_some() || opts.author.is_some() || opts.comments.is_some() {
        let opened = crate::open_bytes(&current)?;
        current = write_summary_information(&opened, &opts.title, &opts.author, &opts.comments)?;
    }

    // Creation gate: the produced file must pass the load-path validator.
    let check = crate::open_bytes(&current)?;
    let vr = crate::validate::validate(&check);
    if let Some(f) = vr.errors().next() {
        return Err(format!("the created file failed validation: {}", f.message));
    }
    Ok(current)
}

fn apply(bytes: &[u8], op: EditOp) -> Result<Vec<u8>, String> {
    let rpt = crate::open_bytes(bytes)?;
    let mut outcome = crate::edit::ApplyOutcome::default();
    crate::edit::apply_edits_on(&rpt, std::slice::from_ref(&op), &mut outcome)
        .map_err(|e| e.to_string())
}

/// Write a minimal SummaryInformation property set (title/author/comments as given),
/// replacing whatever the blank carried. The stream is optional for the reader, so the
/// replace is safe. Layout follows the reader's own proven test builder (VT_LPWSTR UTF-16
/// values; 48-byte header; section = size+count+pairs+values) — the shape its parser is
/// tested against.
fn write_summary_information(
    rpt: &Rpt,
    title: &Option<String>,
    author: &Option<String>,
    comments: &Option<String>,
) -> Result<Vec<u8>, String> {
    const VT_LPWSTR: u32 = 31;
    const PID_TITLE: u32 = 2;
    const PID_AUTHOR: u32 = 4;
    const PID_COMMENTS: u32 = 6;

    let mut props: Vec<(u32, &str)> = Vec::new();
    if let Some(t) = title {
        props.push((PID_TITLE, t.as_str()));
    }
    if let Some(a) = author {
        props.push((PID_AUTHOR, a.as_str()));
    }
    if let Some(c) = comments {
        props.push((PID_COMMENTS, c.as_str()));
    }
    props.sort_by_key(|(id, _)| *id);

    let table_end = 8 + props.len() * 8;
    let mut values: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::new();
    for (_, s) in &props {
        offsets.push((table_end + values.len()) as u32);
        let units: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        values.extend_from_slice(&VT_LPWSTR.to_le_bytes());
        values.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for u in units {
            values.extend_from_slice(&u.to_le_bytes());
        }
    }
    let mut section: Vec<u8> = Vec::new();
    let size = table_end + values.len();
    section.extend_from_slice(&(size as u32).to_le_bytes());
    section.extend_from_slice(&(props.len() as u32).to_le_bytes());
    for ((pid, _), off) in props.iter().zip(&offsets) {
        section.extend_from_slice(&pid.to_le_bytes());
        section.extend_from_slice(&off.to_le_bytes());
    }
    section.extend_from_slice(&values);

    let mut ps: Vec<u8> = Vec::new();
    ps.extend_from_slice(&0xFFFEu16.to_le_bytes());
    ps.extend_from_slice(&[0; 2]); // version
    ps.extend_from_slice(&[0; 4]); // sysid
    ps.extend_from_slice(&[0; 16]); // clsid
    ps.extend_from_slice(&1u32.to_le_bytes()); // num sets
    ps.extend_from_slice(&[0; 16]); // FMTID
    ps.extend_from_slice(&48u32.to_le_bytes()); // section offset
    debug_assert_eq!(ps.len(), 48);
    ps.extend_from_slice(&section);

    replace_stream(rpt.original_bytes(), "\u{5}SummaryInformation", &ps)
}

/// Replace (or create) a top-level stream in the container, copying every other entry.
fn replace_stream(original: &[u8], name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut comp = cfb::CompoundFile::open(std::io::Cursor::new(original.to_vec()))
        .map_err(|e| format!("open container: {e}"))?;
    let path: std::path::PathBuf = std::iter::once("/")
        .chain(std::iter::once(name))
        .collect();
    if comp.is_stream(&path) {
        // copy the old stream out (ignored), then truncate-rewrite via delete+create
        let mut old = Vec::new();
        {
            let mut s = comp.open_stream(&path).map_err(|e| e.to_string())?;
            s.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            s.read_to_end(&mut old).map_err(|e| e.to_string())?;
        }
        let _ = old;
        // cfb has no truncate; rewrite by set_len + seek + write.
        let mut s = comp.open_stream(&path).map_err(|e| e.to_string())?;
        s.set_len(bytes.len() as u64).map_err(|e| e.to_string())?;
        s.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        s.write_all(bytes).map_err(|e| e.to_string())?;
        s.flush().map_err(|e| e.to_string())?;
    } else {
        let mut s = comp.create_stream(&path).map_err(|e| e.to_string())?;
        s.write_all(bytes).map_err(|e| e.to_string())?;
        s.flush().map_err(|e| e.to_string())?;
    }
    comp.flush().map_err(|e| e.to_string())?;
    Ok(comp.into_inner().into_inner())
}
