//! Reading bugs: a work item's links, and the bug fields for a list of ids.
//! Read only - GET requests, nothing here writes.

use super::endpoints::is_tested_by_forward;
use super::{AdoClient, AdoError, BugTypeInfo};

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
    /// Filled by `sort_bug_links`.
    pub linked_test_cases: Vec<i32>,
    /// Filled by `sort_bug_links`.
    pub linked_pbis: Vec<i32>,
    /// The work items this bug links to as its case or PBI, before they
    /// are sorted by type.
    #[serde(skip)]
    pub raw_links: Vec<i32>,
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

/// Links a bug can have to its test case or its PBI. TCM files both as
/// Related; a process may use a parent link or a Tests link instead.
fn is_case_or_pbi_link(rel: &str) -> bool {
    [
        "System.LinkTypes.Related",
        "System.LinkTypes.Hierarchy-Reverse",
        "Microsoft.VSTS.Common.TestedBy-Reverse",
    ]
    .iter()
    .any(|r| rel.eq_ignore_ascii_case(r))
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
    /// ids that are work items of another type. `bug` is the project's bug
    /// type (`detect_bug_type`), which a process may rename; its
    /// `repro_field` is where the repro steps live.
    /// With `omit_unreadable` an id Azure DevOps cannot read is left out
    /// (`errorPolicy=omit`) instead of failing the whole read: for ids
    /// found by following links. A caller's own ids pass false, so a wrong
    /// id is reported.
    /// Relations come in the same call: `$expand=relations` returns every
    /// field with them, and cannot be combined with `fields=`.
    /// Result order is the batch order (id ascending); a caller sorts.
    pub async fn get_bugs_by_ids(
        &self,
        organization: &str,
        bug: &BugTypeInfo,
        ids: &[i32],
        omit_unreadable: bool,
    ) -> Result<(Vec<BugSummary>, Vec<i32>), AdoError> {
        if ids.is_empty() {
            return Ok((vec![], vec![]));
        }
        let policy = if omit_unreadable { "&errorPolicy=omit" } else { "" };
        let urls: Vec<String> = ids
            .chunks(Self::WORKITEM_BATCH_SIZE)
            .map(|chunk| {
                let csv = chunk.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
                format!(
                    "{}/{}/_apis/wit/workitems?ids={}&$expand=relations{}&api-version=7.1",
                    self.base_url,
                    super::endpoints::percent_encode_segment(organization),
                    csv,
                    policy
                )
            })
            .collect();
        let mut bugs = vec![];
        let mut not_a_bug = vec![];
        for fetched in self.get_json_all(urls).await? {
            for w in fetched["value"].as_array().cloned().unwrap_or_default() {
                // errorPolicy=omit answers null for an item it could not read.
                let Some(id) = w["id"].as_i64().map(|i| i as i32) else { continue };
                let f = &w["fields"];
                if f["System.WorkItemType"].as_str() != Some(bug.wi_type.as_str()) {
                    not_a_bug.push(id);
                    continue;
                }
                let s = |k: &str| f[k].as_str().unwrap_or_default().to_string();
                let text = |k: &str| crate::steps_xml::html_to_text(f[k].as_str().unwrap_or_default());
                let raw_links: Vec<i32> = w["relations"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter(|r| r["rel"].as_str().map(is_case_or_pbi_link).unwrap_or(false))
                    .filter_map(work_item_id)
                    .collect();
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
                    repro_steps: text(&bug.repro_field),
                    system_info: text("Microsoft.VSTS.TCM.SystemInfo"),
                    linked_test_cases: vec![],
                    linked_pbis: vec![],
                    raw_links,
                });
            }
        }
        Ok((bugs, not_a_bug))
    }

    /// Sort each bug's links by what they are: a Test Case goes to
    /// `linked_test_cases`, any other item that is not a bug (a PBI, User
    /// Story, Requirement...) to `linked_pbis`. One batched read of the
    /// linked items' types for all the bugs given; an item that cannot be
    /// read is left out (`errorPolicy=omit`).
    pub async fn sort_bug_links(
        &self,
        organization: &str,
        bug: &BugTypeInfo,
        bugs: &mut [BugSummary],
    ) -> Result<(), AdoError> {
        let mut ids: Vec<i32> = bugs.iter().flat_map(|b| b.raw_links.iter().copied()).collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Ok(());
        }
        let urls: Vec<String> = ids
            .chunks(Self::WORKITEM_BATCH_SIZE)
            .map(|chunk| {
                let csv = chunk.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
                format!(
                    "{}/{}/_apis/wit/workitems?ids={}&fields=System.WorkItemType&errorPolicy=omit&api-version=7.1",
                    self.base_url,
                    super::endpoints::percent_encode_segment(organization),
                    csv
                )
            })
            .collect();
        let mut kinds = std::collections::HashMap::new();
        for fetched in self.get_json_all(urls).await? {
            for w in fetched["value"].as_array().cloned().unwrap_or_default() {
                if let (Some(id), Some(k)) = (w["id"].as_i64(), w["fields"]["System.WorkItemType"].as_str()) {
                    kinds.insert(id as i32, k.to_string());
                }
            }
        }
        for b in bugs.iter_mut() {
            for id in b.raw_links.clone() {
                match kinds.get(&id).map(String::as_str) {
                    Some("Test Case") => b.linked_test_cases.push(id),
                    Some(k) if k != bug.wi_type => b.linked_pbis.push(id),
                    _ => {}
                }
            }
        }
        Ok(())
    }
}
