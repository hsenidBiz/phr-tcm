//! Background check for work items newly assigned to the signed-in user.
//!
//! The rule that matters here is "newly": the first poll after launch must
//! not announce the twelve items someone has been carrying for a month.
//! So the first run only *learns* what is already assigned, and every run
//! after it reports the difference. The known set is kept on disk, so a
//! restart doesn't re-announce everything either.
//!
//! Read only - one WIQL query and one work-item batch read, no writes.

use std::collections::BTreeSet;

use crate::ado::{AdoClient, AdoError};

/// How often the check runs. Slow on purpose: an assignment is not
/// something anyone needs within seconds, and the app already paces its
/// Azure DevOps calls (see ado/throttle.rs).
pub const POLL_SECS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct AssignedItem {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
}

/// Ids already announced, per org/project. Stored beside the other
/// reference data (see refcache.rs) so it survives a restart.
fn seen_key(org: &str, project: &str) -> String {
    format!("{org}/{project}/assigned-seen")
}

/// The items in `current` that were not in `seen`, plus the new seen set.
///
/// `first_run` is the whole point: with nothing stored yet we cannot tell
/// "assigned five minutes ago" from "assigned last March", so nothing is
/// reported and the baseline is simply recorded.
pub fn newly_assigned(
    current: &[AssignedItem],
    seen: Option<Vec<String>>,
) -> (Vec<AssignedItem>, Vec<String>) {
    let ids: Vec<String> = current.iter().map(|i| i.id.to_string()).collect();
    let Some(seen) = seen else {
        return (vec![], ids); // first run: learn, don't announce
    };
    let known: BTreeSet<&str> = seen.iter().map(String::as_str).collect();
    let fresh = current
        .iter()
        .filter(|i| !known.contains(i.id.to_string().as_str()))
        .cloned()
        .collect();
    // Keep only what is still assigned, so an item that comes back later
    // is announced again rather than being remembered forever.
    (fresh, ids)
}

impl AdoClient {
    /// Work items currently assigned to the caller, newest first. Read only.
    pub async fn assigned_to_me(
        &self,
        org: &str,
        project: &str,
    ) -> Result<Vec<AssignedItem>, AdoError> {
        // Closed work is not news; States vary by process, so this filters
        // on the category-independent System.State values ADO always has.
        let wiql = "SELECT [System.Id] FROM WorkItems \
                    WHERE [System.AssignedTo] = @Me \
                    AND [System.State] NOT IN ('Closed', 'Done', 'Removed', 'Resolved') \
                    ORDER BY [System.ChangedDate] DESC";
        let ids = self.query_work_items(org, project, wiql, 50).await?;
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let ids_csv = ids.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems?ids={}&fields=System.Id,System.Title,System.WorkItemType,System.State&api-version=7.1",
            self.base_url,
            org,
            project,
            ids_csv
        );
        let data = self.get_json(url).await?;
        Ok(data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|w| {
                let f = &w["fields"];
                AssignedItem {
                    id: w["id"].as_i64().unwrap_or_default() as i32,
                    title: f["System.Title"].as_str().unwrap_or_default().to_string(),
                    work_item_type: f["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
                    state: f["System.State"].as_str().unwrap_or_default().to_string(),
                }
            })
            .collect())
    }
}

/// Poll once and report what is new, updating the stored baseline.
pub async fn check_once(
    client: &AdoClient,
    org: &str,
    project: &str,
) -> Result<Vec<AssignedItem>, AdoError> {
    let current = client.assigned_to_me(org, project).await?;
    let key = seen_key(org, project);
    let (fresh, next) = newly_assigned(&current, crate::refcache::any(&key));
    crate::refcache::put(&key, &next);
    Ok(fresh)
}
