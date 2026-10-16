//! `rpt-core` — open a Crystal Reports `.rpt`, project it to a JSON IR, apply typed edits, save.
//!
//! Built on the vendored `rpt-reader` (see `vendor/VENDORED.md`), which does the container decode,
//! the AES/zlib stream codec, the TSLV record tree, the field tables, and — crucially — the
//! byte-level write path (`Rpt::reencode`, `Rpt::patch_record_field`), whose edits are verified
//! per record: a field edit is refused unless the record's own field table reproduces the record
//! byte for byte AND the written record reads back with exactly the requested change.
//!
//! The three layers here:
//!
//! - [`locate`] walks the `Contents` record tree in document order (the same grammar the upstream
//!   model builder uses) and records, for every area/section/object, the address of the records
//!   that hold its editable fields — `(tag, nth-of-type)` pairs that `patch_record_field` takes.
//! - [`ir`] pairs that address map with the decoded semantic model and projects both into one JSON
//!   document (`crpt-ir/1`) — the contract between the Rust backend and the editor UI.
//! - [`edit`] is the typed edit vocabulary (`set_text`, `set_bounds`, `set_font`, …) plus the
//!   resolver from IR object ids/names to record addresses, applied one field at a time through
//!   the verified write path.
//!
//! Edits only ever touch the `Contents` stream; every other stream (QESession, PromptManager,
//! subdocuments, images) is spliced through verbatim by the writer. Edit operations never add or
//! remove records, so addresses stay valid across a batch.
//!
//! # Errors
//!
//! Everything fallible returns [`std::result::Result`] with a message suitable for showing a user;
//! the underlying `rpt_reader::Error` chain is preserved in the `source` where one exists.

pub mod blank;
pub mod formula;
pub mod edit;
pub mod framing;
pub mod guides;
pub mod grouping;
pub mod ir;
pub mod locate;
pub mod picture;
pub mod plan;
pub mod preview;
pub mod running_total;
pub mod sections;
pub mod structure;
pub mod subreport;
pub mod validate;
pub mod tables;

pub use edit::{apply_edits, EditOp, ObjSel, SecSel};
pub use ir::{build_ir, AreaIr, BoundsIr, ColorIr, FieldIr, FontIr, Ir, ObjectIr, SectionIr, TextIr, IR_FORMAT};
pub use locate::{Located, LocatedArea, LocatedObject, LocatedSection, RecAddr};
pub use preview::{coerce_params, preview, preview_live, preview_live_report, preview_report, LiveDb, PreviewOutput};

/// The vendored reader crate, re-exported so consumers never depend on `vendor/` paths directly.
pub use rpt_reader;

/// Open a report from a file path.
///
/// # Errors
///
/// Propagates `rpt_reader` open/decode errors (not a report, corrupt container, undecodable
/// stream) as a human-readable message.
pub fn open(path: &str) -> Result<rpt_reader::Rpt, String> {
    rpt_reader::Rpt::open(path).map_err(|e| format!("cannot open `{path}`: {e}"))
}

/// Open a report from in-memory bytes.
///
/// # Errors
///
/// As [`open`], minus the path in the message.
pub fn open_bytes(bytes: &[u8]) -> Result<rpt_reader::Rpt, String> {
    rpt_reader::Rpt::read(bytes).map_err(|e| format!("cannot read report bytes: {e}"))
}

/// Re-encode a report's `Contents` stream unchanged — the no-op write path — returning fresh
/// `.rpt` file bytes. Useful as a save-path sanity check.
///
/// # Errors
///
/// Propagates codec/container errors from the writer.
pub fn reencode(rpt: &rpt_reader::Rpt) -> Result<Vec<u8>, String> {
    rpt.reencode().map_err(|e| format!("reencode failed: {e}"))
}
