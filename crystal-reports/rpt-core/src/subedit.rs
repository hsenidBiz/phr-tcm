//! Subreport internals editing — the last big editing boundary.
//!
//! A subreport's `Subdocument N/Contents` stream IS a complete report (its own root, areas,
//! objects, formulas), stored raw in the container. So instead of parallel machinery for
//! walking and patching subdocuments, an edit inside a subreport LIFTS the subdocument into
//! a standalone mini-`.rpt` — a minimal compound file whose root `Contents` stream carries
//! the subdocument's bytes — where the ENTIRE existing op vocabulary applies unchanged
//! (locate, IR, verified field patches, donor-clone adds, validation), and then writes the
//! edited stream back into the parent through the verified `write_stream_logical` seam.
//!
//! Object ids in a subreport IR are scoped to that subreport; ops routed to a subreport
//! resolve against its own IR (i.e. the ids `get_subreport_ir` returned).
//!
//! Round-trip safety: the mini re-encodes through the same verified writer, and the parent's
//! subreport model is re-read after the write-back — an edit that does not reach the
//! parent's model fails the operation.

use std::io::Write as _;

use rpt_reader::{Rpt, StreamId};

use crate::edit::{apply_edits_on, ApplyOutcome, EditOp};
use crate::ir::{build_ir, Ir};
use crate::locate;

/// The subdocument storage index a named subreport occupies.
fn subdoc_of(rpt: &Rpt, name: &str) -> Result<u32, String> {
    crate::subreport::subdoc_index_of(rpt, name)
        .ok_or_else(|| format!("`{name}` is not an embedded subreport of this report"))
}

pub(crate) fn lift(rpt: &Rpt, name: &str) -> Result<(Rpt, StreamId), String> {
    let index = subdoc_of(rpt, name)?;
    let id = StreamId::Other(format!("Subdocument {index}/Contents"));
    let raw = rpt
        .stream(&id)
        .ok_or_else(|| format!("subreport `{name}` has no Contents stream"))?
        .raw_bytes()
        .to_vec();
    let mut comp = cfb::CompoundFile::create(std::io::Cursor::new(Vec::new()))
        .map_err(|e| format!("create the mini container: {e}"))?;
    {
        let mut s = comp
            .create_stream("/Contents".to_string())
            .map_err(|e| format!("create the mini Contents stream: {e}"))?;
        s.write_all(&raw).map_err(|e| format!("write the mini Contents: {e}"))?;
    }
    comp.flush().map_err(|e| format!("flush the mini container: {e}"))?;
    let mini = comp.into_inner().into_inner();
    let mini = crate::open_bytes(&mini)
        .map_err(|e| format!("subreport `{name}` did not open as a standalone report: {e}"))?;
    Ok((mini, id))
}

/// The subreport's own IR — its objects, areas, and fields, exactly like the main report's
/// but scoped to the subreport. Object ids in this IR are the ones subreport-scoped edits
/// resolve against.
#[doc(hidden)]
pub fn lift_for_tests(rpt: &Rpt, name: &str) -> Result<(Rpt, StreamId), String> {
    lift(rpt, name)
}

pub fn subreport_ir(rpt: &Rpt, name: &str) -> Result<Ir, String> {
    let (mini, _) = lift(rpt, name)?;
    let located = locate::locate(&mini);
    Ok(build_ir(&mini, &located, name))
}

/// Apply a batch of ops INSIDE the named subreport, returning new parent-file bytes. The
/// batch semantics match the main scope exactly: ops apply in order, any refusal aborts the
/// batch (earlier ops stand), and the parent's subreport model must reflect the change.
pub fn apply_in_subreport(
    rpt: &Rpt,
    name: &str,
    ops: &[EditOp],
) -> Result<Vec<u8>, crate::edit::OpError> {
    let fail = |m: String| crate::edit::OpError(0, m);
    if ops.iter().any(|o| matches!(o, EditOp::InSubreport { .. })) {
        return Err(fail(
            "a subreport cannot itself contain an `in_subreport` op (nested subdocuments are \
             not supported)"
                .to_string(),
        ));
    }
    let (mini, id) = lift(rpt, name).map_err(fail)?;
    let mut outcome = ApplyOutcome::default();
    let edited = apply_edits_on(&mini, ops, &mut outcome)?;
    let edited_rpt = crate::open_bytes(&edited).map_err(|e| fail(e))?;
    let new_raw = edited_rpt
        .stream(&StreamId::Contents)
        .ok_or_else(|| fail("the edited subreport lost its Contents stream".into()))?
        .raw_bytes()
        .to_vec();
    let decoded = rpt_reader::raw::RecordStream::decode(StreamId::Contents, &new_raw);
    let new_logical = decoded.logical_bytes().to_vec();
    let out = rpt
        .write_stream_logical(&id, &new_logical)
        .map_err(|e| fail(format!("write the subreport back: {e}")))?;

    // The parent's subreport model must re-read the edited stream — the same read-back
    // guarantee every main-scope op carries.
    let reopened = crate::open_bytes(&out).map_err(|e| fail(e))?;
    subdoc_of(&reopened, name).map_err(fail)?;
    Ok(out)
}
