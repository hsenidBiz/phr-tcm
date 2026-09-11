//! Core domain type, ported from v1 app/models/test_case.py.

use crate::steps_xml::Step;
use serde::{Deserialize, Serialize};

/// Azure DevOps rejects System.Title values longer than 255 characters.
pub const MAX_TITLE_LEN: usize = 255;

pub const VALID_STATUSES: [&str; 2] = ["Not Automated", "Planned"];

pub const FINDING_KINDS: [&str; 3] = ["test_case", "spec", "code"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct CaseFinding {
    /// One of FINDING_KINDS.
    pub kind: String,
    /// What it is about: the spec file and section, the code symbol, or empty for the case itself.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subject: String,
    pub title: String,
    /// Markdown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct TestCase {
    pub title: String,
    pub steps: Vec<Step>,
    /// Semicolon-separated.
    pub tags: String,
    /// "Not Automated" or "Planned".
    pub automation_status: String,
    pub module_value: String,
    pub preconditions: String,
    /// When set, update this existing work item instead of creating a new one.
    pub update_id: Option<i32>,
    /// In-app note that round-trips through the JSON export/import but is
    /// NEVER sent to Azure DevOps (no ADO field mapping reads it).
    /// Absent from serialized output when empty, so tools that round-trip
    /// a draft do not inject a field the caller never wrote (feedback:
    /// a transform must be idempotent in shape).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Context for whoever REVIEWS this case - what in the spec it comes
    /// from, which acceptance criterion it covers, what was deliberately
    /// left out. Written by hand or by an assistant, read in the browser
    /// page during review, and rendered as markdown there.
    ///
    /// Like `comment`, it round-trips through the JSON and is NEVER sent
    /// to Azure DevOps. Two tests hold that: the `NoCommentInBody` matcher
    /// on the create mock, and `app_only_fields_never_reach_a_request_body`,
    /// which scans `ado/endpoints.rs` so the rule also covers the update
    /// path and any write function added later.
    ///
    /// The two fields are separate on purpose: `comment` is the reviewer's
    /// own scratchpad and is editable in the page; this is the reference
    /// material they read while writing one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reviewer_notes: String,
    /// This case's 1-based position when the set is read AGAINST THE SPEC -
    /// cases walking down the document, so a reviewer scrolls the spec and
    /// the file together. Stamped by the optimizer from the order the
    /// draft was written in; app-only, like the two notes above - never
    /// sent to Azure DevOps (`app_only_fields_never_reach_a_request_body`
    /// covers it by scanning the write path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_order: Option<u32>,
    /// This case's 1-based position when the set is run BY A TESTER -
    /// grouped so cases sharing a setup run together and the environment
    /// changes as few times as possible. Stamped by the optimizer's
    /// grouping pass. Both orders live in the same file so neither reading
    /// costs the other; the array order is just whichever one the file was
    /// last saved in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tester_order: Option<u32>,
    /// Problems an assistant found while writing this case: in the spec,
    /// the code, or the case itself. Lives in the draft file, shown only
    /// in the browser page, never sent to Azure DevOps, never written by
    /// a transform. `comment` stays the developer's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<CaseFinding>,
}

impl TestCase {
    /// Ported from v1 TestCase.is_valid(): Ok(()) if ready to submit.
    pub fn is_valid(&self) -> Result<(), String> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err("Title is required.".into());
        }
        if title.chars().count() > MAX_TITLE_LEN {
            return Err(format!(
                "Title is {} characters - Azure DevOps allows at most {}.",
                title.chars().count(),
                MAX_TITLE_LEN
            ));
        }
        if self.steps.is_empty() {
            return Err("At least one step is required.".into());
        }
        for (i, step) in self.steps.iter().enumerate() {
            if step.action.trim().is_empty() {
                return Err(format!("Step {} action is empty.", i + 1));
            }
        }
        if !VALID_STATUSES.contains(&self.automation_status.as_str()) {
            return Err(format!(
                "Invalid automation status: '{}'",
                self.automation_status
            ));
        }
        if self.tags.contains(',') {
            return Err(
                "Tags must be separated with semicolons - Azure DevOps does not allow commas in tag names."
                    .into(),
            );
        }
        Ok(())
    }
}
