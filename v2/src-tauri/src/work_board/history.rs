//! Work-item revision history: what changed, when, and by whom.
//!
//! Azure DevOps returns one entry per revision with a raw `fields` map of
//! oldValue/newValue pairs. Most of that map is bookkeeping the user never
//! wants to see (rev counters, authorization stamps, the changed-date that
//! duplicates the entry's own timestamp), and the interesting values arrive
//! in three different shapes - plain scalars, identity objects, and HTML
//! blobs. This module reduces all of it to a flat list of readable changes
//! so the UI can just render.
//!
//! Read only: GET, no writes of any kind.

use super::{FieldChange, WorkRevision};
use crate::ado::{AdoClient, AdoError};

/// Bookkeeping fields: ADO stamps these on every single revision, and
/// showing them would bury the one change the user actually made.
const NOISE: &[&str] = &[
    "System.Rev",
    "System.AuthorizedDate",
    "System.RevisedDate",
    "System.ChangedDate",
    "System.ChangedBy",
    "System.AuthorizedAs",
    "System.PersonId",
    "System.Watermark",
    "System.CommentCount",
];

/// Long text lives in its own view; the timeline says it changed and by
/// roughly how much rather than dumping a page of HTML into a row.
const LONG_TEXT: &[&str] = &[
    "System.Description",
    "System.History",
    "Microsoft.VSTS.TCM.ReproSteps",
    "Microsoft.VSTS.TCM.SystemInfo",
    "Microsoft.VSTS.Common.AcceptanceCriteria",
];

/// Friendly names for the fields that come up constantly. Anything not
/// here falls back to de-camel-casing the reference name's last segment,
/// which handles the org's custom fields acceptably.
fn label_for(reference: &str) -> String {
    let known = match reference {
        "System.State" => Some("State"),
        "System.Reason" => Some("Reason"),
        "System.Title" => Some("Title"),
        "System.AssignedTo" => Some("Assigned To"),
        "System.AreaPath" => Some("Area Path"),
        "System.IterationPath" => Some("Iteration"),
        "System.Tags" => Some("Tags"),
        "System.Description" => Some("Description"),
        "System.History" => Some("Comment"),
        "Microsoft.VSTS.Common.Priority" => Some("Priority"),
        "Microsoft.VSTS.Common.Severity" => Some("Severity"),
        "Microsoft.VSTS.Common.Activity" => Some("Activity"),
        "Microsoft.VSTS.Common.ResolvedReason" => Some("Resolved Reason"),
        "Microsoft.VSTS.Scheduling.RemainingWork" => Some("Remaining Work"),
        "Microsoft.VSTS.Scheduling.CompletedWork" => Some("Completed Work"),
        "Microsoft.VSTS.Scheduling.OriginalEstimate" => Some("Original Estimate"),
        "Microsoft.VSTS.Scheduling.StartDate" => Some("Start Date"),
        "Microsoft.VSTS.Scheduling.FinishDate" => Some("Finish Date"),
        "Microsoft.VSTS.Scheduling.TargetDate" => Some("Target Date"),
        "Microsoft.VSTS.TCM.ReproSteps" => Some("Repro Steps"),
        _ => None,
    };
    if let Some(k) = known {
        return k.to_string();
    }
    de_camel(reference.rsplit('.').next().unwrap_or(reference))
}

/// "RemainingWork" -> "Remaining Work". Leaves already-spaced names and
/// runs of capitals ("QA", "ID") alone.
pub fn de_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        let prev = if i > 0 { chars[i - 1] } else { '\0' };
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        let boundary = c.is_uppercase()
            && i > 0
            && prev != ' '
            && (prev.is_lowercase() || (next.is_lowercase() && next != '\0'));
        if boundary {
            out.push(' ');
        }
        out.push(*c);
    }
    out
}

/// One raw field value as display text. Identities become their display
/// name, HTML becomes plain text, everything else stringifies without the
/// JSON quoting.
fn value_text(v: &serde_json::Value, reference: &str) -> String {
    if v.is_null() {
        return String::new();
    }
    if let Some(name) = v["displayName"].as_str() {
        return name.to_string();
    }
    let raw = match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if LONG_TEXT.contains(&reference) {
        let text = crate::steps_xml::html_to_text(&raw);
        return truncate(text.trim(), 240);
    }
    truncate(raw.trim(), 240)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

/// A readable name for a relation, preferring ADO's own link name
/// ("Commit", "Pull Request") over the raw rel string.
fn relation_name(rel: &serde_json::Value) -> String {
    if let Some(name) = rel["attributes"]["name"].as_str() {
        return format!("{name} link");
    }
    let raw = rel["rel"].as_str().unwrap_or_default();
    let friendly = match raw {
        "System.LinkTypes.Related" => "Related link",
        "System.LinkTypes.Hierarchy-Forward" => "Child link",
        "System.LinkTypes.Hierarchy-Reverse" => "Parent link",
        "System.LinkTypes.Duplicate-Forward" => "Duplicate link",
        "System.LinkTypes.Dependency-Forward" => "Successor link",
        "System.LinkTypes.Dependency-Reverse" => "Predecessor link",
        "Microsoft.VSTS.Common.TestedBy-Forward" => "Tested By link",
        "Microsoft.VSTS.Common.TestedBy-Reverse" => "Tests link",
        "AttachedFile" => "Attachment",
        "ArtifactLink" => "Artifact link",
        "" => "Link",
        other => return format!("{} link", de_camel(other.rsplit('.').next().unwrap_or(other))),
    };
    friendly.to_string()
}

/// Turn one raw update entry into a revision, or `None` when nothing
/// user-visible happened (a bare rev bump, which ADO emits routinely).
pub fn parse_revision(u: &serde_json::Value) -> Option<WorkRevision> {
    let fields = &u["fields"];
    let mut changes: Vec<FieldChange> = vec![];
    if let Some(map) = fields.as_object() {
        for (reference, change) in map {
            if NOISE.contains(&reference.as_str()) {
                continue;
            }
            let old = value_text(&change["oldValue"], reference);
            let new = value_text(&change["newValue"], reference);
            if old == new {
                continue;
            }
            changes.push(FieldChange {
                reference_name: reference.clone(),
                label: label_for(reference),
                old,
                new,
            });
        }
    }
    // Stable, and with the fields people scan for first at the top.
    changes.sort_by_key(|c| {
        let rank = match c.reference_name.as_str() {
            "System.State" => 0,
            "System.Reason" => 1,
            "System.AssignedTo" => 2,
            "System.Title" => 3,
            _ => 4,
        };
        (rank, c.label.clone())
    });

    let links_added: Vec<String> = u["relations"]["added"]
        .as_array()
        .map(|a| a.iter().map(relation_name).collect())
        .unwrap_or_default();
    let links_removed: Vec<String> = u["relations"]["removed"]
        .as_array()
        .map(|a| a.iter().map(relation_name).collect())
        .unwrap_or_default();

    // A comment shows up as a CommentCount bump (filtered above as noise)
    // and/or a System.History field; the Discussion tab renders the text,
    // so history only notes that one was added.
    let comment_added = fields["System.CommentCount"].is_object()
        || fields["System.History"]["newValue"].is_string();
    changes.retain(|c| c.reference_name != "System.History");

    if changes.is_empty() && links_added.is_empty() && links_removed.is_empty() && !comment_added {
        return None;
    }

    let state = changes.iter().find(|c| c.reference_name == "System.State");
    let by = &u["revisedBy"];
    Some(WorkRevision {
        rev: u["rev"].as_i64().unwrap_or_default() as i32,
        by: by["displayName"].as_str().unwrap_or_default().to_string(),
        avatar_url: by["_links"]["avatar"]["href"]
            .as_str()
            .or_else(|| by["imageUrl"].as_str())
            .unwrap_or_default()
            .to_string(),
        at: revision_date(u),
        state_from: state.map(|c| c.old.clone()).unwrap_or_default(),
        state_to: state.map(|c| c.new.clone()).unwrap_or_default(),
        fields: changes,
        links_added,
        links_removed,
        comment_added,
    })
}

/// `revisedDate` is the date the revision was SUPERSEDED, so the newest
/// entry carries ADO's "never" sentinel (year 9999). The date a user means
/// is System.ChangedDate's new value; fall back to revisedDate only when
/// that is absent.
fn revision_date(u: &serde_json::Value) -> String {
    let changed = u["fields"]["System.ChangedDate"]["newValue"]
        .as_str()
        .unwrap_or_default();
    if !changed.is_empty() {
        return changed.to_string();
    }
    let revised = u["revisedDate"].as_str().unwrap_or_default();
    if revised.starts_with("9999") {
        return String::new();
    }
    revised.to_string()
}

impl AdoClient {
    /// A work item's revision history, newest first. Read only.
    pub async fn get_work_item_history(
        &self,
        org: &str,
        project: &str,
        wi_id: i32,
    ) -> Result<Vec<WorkRevision>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workItems/{}/updates?api-version=7.1&$top=200",
            self.base_url, org, project, wi_id
        );
        let data = self.get_json(url).await?;
        let mut out: Vec<WorkRevision> = data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(parse_revision)
            .collect();
        // ADO returns oldest first; the drawer reads newest first.
        out.reverse();
        Ok(out)
    }
}
