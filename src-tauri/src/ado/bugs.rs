//! Reading bugs: a work item's links, and the bug fields for a list of ids.
//! Read only - GET requests, nothing here writes.

use super::endpoints::is_tested_by_forward;
use super::{AdoClient, AdoError};

/// One bug as the assistant is shown it. Rich-text fields are plain text.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct BugSummary {
    pub id: i32,
    pub title: String,
    pub state: String,
    pub reason: String,
    pub severity: String,
    pub priority: Option<i64>,
    pub assigned_to: String,
    pub tags: Vec<String>,
    pub area_path: String,
    pub iteration_path: String,
    pub created: String,
    pub changed: String,
    pub repro_steps: String,
    pub system_info: String,
    pub linked_test_cases: Vec<i32>,
    pub linked_pbis: Vec<i32>,
}

/// The work item id at the end of a relation's url, or None for a link that
/// is not to a work item (an attachment, a hyperlink, a commit).
fn work_item_id(rel: &serde_json::Value) -> Option<i32> {
    let url = rel["url"].as_str()?;
    if !url.contains("/workitems/") {
        return None;
    }
    url.rsplit('/').next()?.parse().ok()
}

/// Is a relation a test-case link of a bug? TCM files a bug with a Related
/// link to its case; a case's own Tests link points at the work item too.
fn is_case_link(rel: &str) -> bool {
    rel.eq_ignore_ascii_case("System.LinkTypes.Related")
        || rel.eq_ignore_ascii_case("Microsoft.VSTS.Common.TestedBy-Reverse")
}

impl AdoClient {
    /// Every work item linked from `id`, as (relation, id), in link order.
    /// One GET with relations.
    pub async fn linked_ids(&self, organization: &str, id: i32) -> Result<Vec<(String, i32)>, AdoError> {
        let url = format!(
            "{}/{}/_apis/wit/workitems/{}?$expand=relations&api-version=7.1",
            self.base_url,
            super::endpoints::percent_encode_segment(organization),
            id
        );
        let data = self.get_json(url).await?;
        Ok(data["relations"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| Some((r["rel"].as_str()?.to_string(), work_item_id(r)?)))
            .collect())
    }

    /// Is this relation name the one that hangs test cases off a PBI?
    pub fn is_tested_by_link(rel: &str) -> bool {
        is_tested_by_forward(rel)
    }

    /// The bug fields of `ids` (batched at `WORKITEM_BATCH_SIZE`), plus the
    /// ids that are work items of another type. `bug_type` is the project's
    /// name for a bug (`detect_bug_type`), which a process may rename.
    /// Relations come in the same call: `$expand=relations` returns every
    /// field with them, and cannot be combined with `fields=`.
    /// Result order is the batch order (id ascending); a caller sorts.
    pub async fn get_bugs_by_ids(
        &self,
        organization: &str,
        bug_type: &str,
        ids: &[i32],
    ) -> Result<(Vec<BugSummary>, Vec<i32>), AdoError> {
        if ids.is_empty() {
            return Ok((vec![], vec![]));
        }
        let urls: Vec<String> = ids
            .chunks(Self::WORKITEM_BATCH_SIZE)
            .map(|chunk| {
                let csv = chunk.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
                format!(
                    "{}/{}/_apis/wit/workitems?ids={}&$expand=relations&api-version=7.1",
                    self.base_url,
                    super::endpoints::percent_encode_segment(organization),
                    csv
                )
            })
            .collect();
        let mut bugs = vec![];
        let mut not_a_bug = vec![];
        for fetched in self.get_json_all(urls).await? {
            for w in fetched["value"].as_array().cloned().unwrap_or_default() {
                let id = w["id"].as_i64().unwrap_or_default() as i32;
                let f = &w["fields"];
                if f["System.WorkItemType"].as_str() != Some(bug_type) {
                    not_a_bug.push(id);
                    continue;
                }
                let s = |k: &str| f[k].as_str().unwrap_or_default().to_string();
                let text = |k: &str| crate::steps_xml::html_to_text(f[k].as_str().unwrap_or_default());
                let mut linked_test_cases = vec![];
                let mut linked_pbis = vec![];
                for r in w["relations"].as_array().cloned().unwrap_or_default() {
                    let (Some(rel), Some(lid)) = (r["rel"].as_str(), work_item_id(&r)) else { continue };
                    if rel.eq_ignore_ascii_case("System.LinkTypes.Hierarchy-Reverse") {
                        linked_pbis.push(lid);
                    } else if is_case_link(rel) {
                        linked_test_cases.push(lid);
                    }
                }
                bugs.push(BugSummary {
                    id,
                    title: s("System.Title"),
                    state: s("System.State"),
                    reason: s("System.Reason"),
                    severity: s("Microsoft.VSTS.Common.Severity"),
                    priority: f["Microsoft.VSTS.Common.Priority"].as_i64(),
                    assigned_to: f["System.AssignedTo"]["displayName"]
                        .as_str()
                        .or_else(|| f["System.AssignedTo"].as_str())
                        .unwrap_or_default()
                        .to_string(),
                    tags: s("System.Tags")
                        .split(';')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect(),
                    area_path: s("System.AreaPath"),
                    iteration_path: s("System.IterationPath"),
                    created: s("System.CreatedDate"),
                    changed: s("System.ChangedDate"),
                    repro_steps: text("Microsoft.VSTS.TCM.ReproSteps"),
                    system_info: text("Microsoft.VSTS.TCM.SystemInfo"),
                    linked_test_cases,
                    linked_pbis,
                });
            }
        }
        Ok((bugs, not_a_bug))
    }
}
