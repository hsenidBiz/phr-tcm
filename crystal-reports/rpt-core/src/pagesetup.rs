//! Page-setup synthesis — the `0x0007` page DEVMODE.
//!
//! Orientation and paper size live in a compacted Win32 `DEVMODEW` record a report only
//! carries when it was saved with a printer attached. A report without one (the engine
//! blank, the PMSV10 family) historically refused `setOrientation`/`setPaperSize`; the ops
//! now SYNTHESIZE a minimal record first — `dm_fields` selecting only orientation and paper
//! size (`0x1 | 0x2`, the two fixed slots every record carries; optional members omitted,
//! including `form_name`, whose engine spelling is only attested for Letter) — spliced
//! where the engine writes it in a printer-attached save: right after `0x0005
//! ReportDocumentFlags`, falling back to ahead of the `0x0066` PageSetup record. The record
//! is built for a TOP-LEVEL position (schema `0x0700`), matching the donor placement.

use rpt_reader::{Rpt, StreamId};

use crate::framing;

/// Ensure the report carries a page DEVMODE: when it has one, `bytes` come back unchanged;
/// otherwise a minimal `0x0007` (portrait, Letter) is synthesized and spliced, and the new
/// file bytes are returned for the caller's field patch.
pub(crate) fn ensure_page_devmode(rpt: &Rpt) -> Result<Vec<u8>, String> {
    if crate::locate::locate(rpt).page_devmode.is_some() {
        return rpt.reencode().map_err(|e| format!("reencode: {e}"));
    }
    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();

    let mut body = Vec::new();
    body.extend_from_slice(&0x0000_0003u32.to_be_bytes()); // dm_fields: orientation | paper size
    body.extend_from_slice(&1u16.to_be_bytes()); // orientation: portrait
    body.extend_from_slice(&1u16.to_be_bytes()); // paper_size: Letter
    let record = framing::build_record(0x0007, 0x0700, &body);

    // The engine's placement, per a printer-attached save: after ReportDocumentFlags
    // (`0x0005`), falling back to ahead of PageSetup (`0x0066`).
    let at = tree
        .iter()
        .find(|n| n.rtype == 0x0005)
        .map(|n| n.content_end)
        .or_else(|| tree.iter().find(|n| n.rtype == 0x0066).map(|n| n.offset))
        .ok_or("no anchor for a synthesized page DEVMODE")?;
    let new_logical = framing::splice(&logical, &tree, at..at, &record)
        .map_err(|e| format!("splice page DEVMODE: {e}"))?;
    let out = rpt
        .write_contents_logical(&new_logical)
        .map_err(|e| format!("write page DEVMODE: {e}"))?;

    // Read-back: the record exists and says what we wrote.
    let reopened = crate::open_bytes(&out)?;
    if crate::locate::locate(&reopened).page_devmode.is_none() {
        return Err("the synthesized page DEVMODE does not read back".to_string());
    }
    Ok(out)
}
