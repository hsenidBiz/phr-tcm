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
    /// The project the item lives in (per-project reads: the argument).
    pub project: String,
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

/// The assignment watch is for WORK - PBIs, bugs, tasks, features. Test
/// management artifacts are excluded: creating or updating test cases
/// through this very app assigns them to you, so every submit used to
/// come straight back as an "assigned to you" notification about your
/// own edit.
pub const ASSIGNED_WIQL: &str = "SELECT [System.Id] FROM WorkItems \
    WHERE [System.AssignedTo] = @Me \
    AND [System.State] NOT IN ('Closed', 'Done', 'Removed', 'Resolved') \
    AND [System.WorkItemType] NOT IN ('Test Case', 'Test Suite', 'Test Plan', 'Shared Steps', 'Shared Parameter') \
    ORDER BY [System.ChangedDate] DESC";

/// The same query for the whole organization. It names no project already,
/// so the only difference is the route it is posted to.
pub const ASSIGNED_ALL_WIQL: &str = ASSIGNED_WIQL;

impl AdoClient {
    /// Work items currently assigned to the caller, newest first. Read only.
    pub async fn assigned_to_me(
        &self,
        org: &str,
        project: &str,
    ) -> Result<Vec<AssignedItem>, AdoError> {
        // Closed work is not news; States vary by process, so this filters
        // on the category-independent System.State values ADO always has.
        let ids = self.query_work_items(org, project, ASSIGNED_WIQL, 50).await?;
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
        Ok(parse_assigned(&data, Some(project)))
    }

    /// `assigned_to_me` across every project: the same query asked of the
    /// organization (it has no project clause), and the items read at
    /// organization scope with `System.TeamProject` so each names its
    /// project. Read only.
    pub async fn assigned_to_me_all(&self, org: &str) -> Result<Vec<AssignedItem>, AdoError> {
        let ids = self.query_work_items_org(org, ASSIGNED_ALL_WIQL, 50).await?;
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let ids_csv = ids.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        let url = format!(
            "{}/{}/_apis/wit/workitems?ids={}&fields=System.Id,System.Title,System.WorkItemType,System.State,System.TeamProject&api-version=7.1",
            self.base_url, org, ids_csv
        );
        let data = self.get_json(url).await?;
        Ok(parse_assigned(&data, None))
    }
}

/// The items in one work-item batch answer. `project` is the argument a
/// per-project read was made with; without it each item's own
/// `System.TeamProject` is used.
fn parse_assigned(data: &serde_json::Value, project: Option<&str>) -> Vec<AssignedItem> {
    data["value"]
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
                project: match project {
                    Some(p) => p.to_string(),
                    None => f["System.TeamProject"].as_str().unwrap_or_default().to_string(),
                },
            }
        })
        .collect()
}

/// `check_once` across every project. Its baseline is its own
/// (`assigned_seen_all`) so the per-project one is never touched. The first
/// time it runs there is nothing stored for the organization as a whole, so
/// it starts from what is known: the home project's stored baseline (that
/// project keeps announcing its news) plus every current item outside the
/// home project, whose assignment date is unknown and so is learned rather
/// than announced. With no home baseline either, everything is learned.
pub async fn check_once_all(
    client: &AdoClient,
    org: &str,
    home: &str,
) -> Result<Vec<AssignedItem>, AdoError> {
    let current = client.assigned_to_me_all(org).await?;
    let key = crate::cache::keys::assigned_seen_all(org);
    let seen = match crate::cache::get::<Vec<String>>(&key) {
        Some(seen) => Some(seen),
        None => crate::cache::get::<Vec<String>>(&crate::cache::keys::assigned_seen(org, home))
            .map(|home_seen| first_combined_baseline(&current, home, home_seen)),
    };
    let (fresh, next) = newly_assigned(&current, seen);
    crate::cache::put(&key, &next);
    Ok(fresh)
}

/// The baseline for a first combined check when the home project has one:
/// its stored ids plus the ids of every current item in another project.
fn first_combined_baseline(current: &[AssignedItem], home: &str, home_seen: Vec<String>) -> Vec<String> {
    let mut baseline = home_seen;
    baseline.extend(
        current
            .iter()
            .filter(|i| !i.project.eq_ignore_ascii_case(home))
            .map(|i| i.id.to_string()),
    );
    baseline
}

/// Poll once and report what is new, updating the stored baseline.
pub async fn check_once(
    client: &AdoClient,
    org: &str,
    project: &str,
) -> Result<Vec<AssignedItem>, AdoError> {
    let current = client.assigned_to_me(org, project).await?;
    // The announced ids are kept in the app's cache, on disk, so a restart
    // doesn't re-announce everything.
    let key = crate::cache::keys::assigned_seen(org, project);
    let (fresh, next) = newly_assigned(&current, crate::cache::get::<Vec<String>>(&key));
    crate::cache::put(&key, &next);
    Ok(fresh)
}
