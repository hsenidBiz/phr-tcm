//! Files attached to individual test-case steps.
//!
//! A step's attachments are data on the `Step` itself, so they travel with it
//! through every editor. Azure DevOps stores them as `AttachedFile`
//! relations on the Test Case whose comment names the step:
//! `[TestStep=<id>]:` followed by the attachment's own comment text.
//!
//! UNCONFIRMED against a live case: the marker format below comes from the
//! design, not from reading a real work item. It is isolated in `marker` and
//! `parse_marker` so a correction touches nothing else.
//!
//! `relation_ops` is the one pure function that turns (final step ids,
//! current relations, the draft's attachments, removals, uploaded blobs) into
//! JSON-patch operations. Relations nobody carried or removed are never
//! touched, so an old draft that never read attachments changes nothing.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::model::TestCase;
use crate::steps_xml::Step;

/// Per-file cap for a step attachment.
pub const STEP_ATTACHMENT_MAX_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct StepAttachment {
    /// An attachment Azure DevOps already holds: its GUID (last segment of
    /// the relation URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// A new file: a full local path on this machine, read only at upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// File name as shown (ADO's `name` attribute; for a new file, the
    /// path's file name).
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    /// Text after `[TestStep=N]:`, kept verbatim when re-pointed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

/// The relation comment that ties an attachment to a step.
pub fn marker(step_id: &str, comment: &str) -> String {
    format!("[TestStep={step_id}]:{comment}")
}

/// `(step id, rest of the comment)` when `comment` starts with a step
/// marker (leading whitespace tolerated), else `None`.
pub fn parse_marker(comment: &str) -> Option<(String, String)> {
    let rest = comment.trim_start().strip_prefix("[TestStep=")?;
    let close = rest.find("]:")?;
    let id = &rest[..close];
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((id.to_string(), rest[close + 2..].to_string()))
}

/// The GUID of an attachment URL: its last path segment, query stripped.
pub fn attachment_guid(url: &str) -> Option<String> {
    let no_query = url.split(['?', '#']).next().unwrap_or("");
    let last = no_query.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}

/// One `AttachedFile` relation on a work item.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseRelation {
    /// Position in the work item's full `relations` array.
    pub index: usize,
    pub guid: String,
    pub name: String,
    pub size: Option<u32>,
    pub comment: String,
}

/// A work item's `AttachedFile` relations and the revision they were read at.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaseRelations {
    pub rev: i64,
    pub attachments: Vec<CaseRelation>,
}

/// A blob that has been uploaded for a local file.
#[derive(Debug, Clone, PartialEq)]
pub struct Uploaded {
    pub path: String,
    pub guid: String,
    pub url: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttachmentOps {
    pub ops: Vec<Value>,
    /// Plain sentences for the row; no URL in any of them.
    pub notes: Vec<String>,
}

/// Whether saving `tc` has any attachment work to do.
pub fn has_attachment_work(tc: &TestCase) -> bool {
    !tc.removed_attachments.is_empty() || tc.steps.iter().any(|s| !s.attachments.is_empty())
}

/// JSON-patch operations for a case's step attachments.
///
/// `final_ids[j]` is the id step `j` has in the Steps XML being written.
/// `current` is `None` for a create. Order: one `test /rev` (only when an
/// index-based op follows), `replace`s ascending, `remove`s descending,
/// then `add`s.
pub fn relation_ops(
    final_ids: &[String],
    steps: &[Step],
    removed: &[String],
    current: Option<&CaseRelations>,
    uploaded: &[Uploaded],
) -> AttachmentOps {
    let mut notes: Vec<String> = Vec::new();
    // (index, new comment)
    let mut replaces: Vec<(usize, String)> = Vec::new();
    let mut adds: Vec<Value> = Vec::new();
    let mut carried: Vec<&str> = Vec::new();

    for (j, step) in steps.iter().enumerate() {
        for att in &step.attachments {
            let step_id = final_ids.get(j).map(String::as_str).unwrap_or("");
            if let Some(guid) = att.id.as_deref() {
                carried.push(guid);
                let Some(cur) = current else {
                    notes.push(format!(
                        "\"{}\" was already attached to another case, so it was not added to this one.",
                        att.name
                    ));
                    continue;
                };
                let Some(r) = cur.attachments.iter().find(|r| r.guid == guid) else {
                    notes.push(format!(
                        "\"{}\" is no longer attached to this case in Azure DevOps, so it was left out.",
                        att.name
                    ));
                    continue;
                };
                if step_id.is_empty() {
                    continue;
                }
                // No marker: a case-level attachment. Never marked, even if a
                // hand-written draft names its GUID.
                let Some((old_id, rest)) = parse_marker(&r.comment) else {
                    notes.push(format!(
                        "\"{}\" is attached to the whole case, not a step, so it was left as it is.",
                        att.name
                    ));
                    continue;
                };
                if old_id != step_id && !replaces.iter().any(|(i, _)| *i == r.index) {
                    replaces.push((r.index, marker(step_id, &rest)));
                }
            } else if let Some(path) = att.path.as_deref() {
                if step_id.is_empty() {
                    notes.push(format!("\"{}\" could not be attached to step {}.", att.name, j + 1));
                    continue;
                }
                let Some(up) = uploaded.iter().find(|u| u.path == path) else {
                    notes.push(format!("\"{}\" was not uploaded, so it was not attached.", att.name));
                    continue;
                };
                adds.push(json!({
                    "op": "add",
                    "path": "/relations/-",
                    "value": {
                        "rel": "AttachedFile",
                        "url": up.url,
                        "attributes": {
                            "name": up.name,
                            "comment": marker(step_id, &att.comment),
                        },
                    },
                }));
            }
        }
    }

    // Removals: carried wins over removed.
    let mut removes: Vec<usize> = Vec::new();
    if let Some(cur) = current {
        for g in removed {
            if carried.contains(&g.as_str()) {
                continue;
            }
            if let Some(r) = cur.attachments.iter().find(|r| &r.guid == g) {
                if parse_marker(&r.comment).is_none() {
                    notes.push(format!(
                        "\"{}\" is attached to the whole case, not a step, so it was not removed.",
                        r.name
                    ));
                    continue;
                }
                if !removes.contains(&r.index) {
                    removes.push(r.index);
                }
            }
        }
    }

    replaces.sort_by_key(|(i, _)| *i);
    removes.sort_unstable_by(|a, b| b.cmp(a));

    let mut ops: Vec<Value> = Vec::new();
    if let (Some(cur), false) = (current, replaces.is_empty() && removes.is_empty()) {
        ops.push(json!({"op": "test", "path": "/rev", "value": cur.rev}));
    }
    for (i, comment) in replaces {
        ops.push(json!({
            "op": "replace",
            "path": format!("/relations/{i}/attributes/comment"),
            "value": comment,
        }));
    }
    for i in removes {
        ops.push(json!({"op": "remove", "path": format!("/relations/{i}")}));
    }
    ops.extend(adds);
    AttachmentOps { ops, notes }
}

/// What a create or update document needs to write a case's step
/// attachments. Callers that do no attachment work pass `NONE`.
#[derive(Debug, Clone, Copy)]
pub struct AttachmentInput<'a> {
    /// The case's `AttachedFile` relations as read now; `None` for a create.
    pub current: Option<&'a CaseRelations>,
    /// Blobs already uploaded for the draft's local files.
    pub uploaded: &'a [Uploaded],
    /// Write no attachment ops and no notes: the caller could not read the
    /// relations (or does no attachment work) and says so itself.
    pub skip: bool,
}

impl AttachmentInput<'static> {
    /// No attachment work: the document is exactly what 2.4.2 built.
    pub const NONE: AttachmentInput<'static> = AttachmentInput { current: None, uploaded: &[], skip: true };
}

/// The attachment ops for a document, or a note when they cannot be
/// written. Nothing at all when the case has no attachment work.
pub fn document_ops(tc: &TestCase, final_ids: &[String], attach: &AttachmentInput<'_>) -> AttachmentOps {
    if attach.skip || !has_attachment_work(tc) {
        return AttachmentOps::default();
    }
    if final_ids.len() != tc.steps.len() {
        return AttachmentOps {
            ops: vec![],
            notes: vec![
                "Its step attachments were not saved: its steps could not be matched to the ones in Azure DevOps. \
                 Settings → Logs has the details."
                    .to_string(),
            ],
        };
    }
    relation_ops(final_ids, &tc.steps, &tc.removed_attachments, attach.current, attach.uploaded)
}
