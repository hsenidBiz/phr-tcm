//! Core domain type, ported from v1 app/models/test_case.py.

use crate::steps_xml::Step;
use serde::{Deserialize, Serialize};

/// Azure DevOps rejects System.Title values longer than 255 characters.
pub const MAX_TITLE_LEN: usize = 255;

pub const VALID_STATUSES: [&str; 2] = ["Not Automated", "Planned"];

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
