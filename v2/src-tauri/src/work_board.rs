//! Work Manager board core, ported from v1 app/models/work_item.py and the
//! board-fetch/_state_for_column logic in mywork_screen.py. Columns derive
//! from each state's process *category*, never hardcoded state names - with
//! v1's two documented exceptions: a state literally named "Later" always
//! lands in Done, and unknown processes fall back to a name heuristic.

use crate::ado::{AdoClient, AdoError};
use serde::Serialize;
use std::collections::HashMap;

pub const COLUMNS: [&str; 3] = ["To Do", "In Progress", "Done"];

/// Work-item types that never belong on the board.
pub const EXCLUDED_TYPES: [&str; 5] = [
    "Test Case",
    "Test Suite",
    "Test Plan",
    "Shared Steps",
    "Shared Parameter",
];

const MAX_ITEMS: u32 = 500;

/// Fields the board fetch asks for (rich-text fields deliberately excluded -
/// they are heavy and only the detail editor needs them).
const BOARD_FIELDS: &str = "System.Id,System.Title,System.WorkItemType,System.State,System.AssignedTo,System.ChangedDate,System.Tags,Microsoft.VSTS.Common.Priority";

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct StateInfo {
    pub name: String,
    pub color: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BoardItem {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
    pub state_color: String,
    /// "To Do" | "In Progress" | "Done"; None = hidden (Removed).
    pub column: Option<String>,
    pub assigned_to: String,
    pub tags: String,
    pub priority: Option<i32>,
    pub changed_date: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BoardData {
    pub items: Vec<BoardItem>,
    pub states_by_type: HashMap<String, Vec<StateInfo>>,
}

fn column_for_category(category: &str) -> Option<&'static str> {
    match category {
        "Proposed" => Some("To Do"),
        "InProgress" => Some("In Progress"),
        // Resolved and Completed both land in Done.
        "Resolved" | "Completed" => Some("Done"),
        _ => None, // Removed -> hidden
    }
}

/// Board column for an item, ported from v1 WorkItem.column().
pub fn column_for_state(
    wi_type: &str,
    state: &str,
    states_by_type: &HashMap<String, Vec<StateInfo>>,
) -> Option<String> {
    let name = state.trim().to_lowercase();
    // A state literally named "Later" always lands in Done - parked items
    // sit with the finished work regardless of process category.
    if name == "later" {
        return Some("Done".to_string());
    }
    if let Some(states) = states_by_type.get(wi_type) {
        if let Some(s) = states.iter().find(|s| s.name == state) {
            if !s.category.is_empty() {
                return column_for_category(&s.category).map(String::from);
            }
        }
    }
    // Unknown process/type: name heuristic, same order as v1.
    match name.as_str() {
        "new" | "to do" | "proposed" | "open" | "approved" | "design" => {
            Some("To Do".to_string())
        }
        "removed" => None,
        "done" | "closed" | "completed" | "resolved" => Some("Done".to_string()),
        _ => Some("In Progress".to_string()),
    }
}

fn column_categories(col: &str) -> &'static [&'static str] {
    match col {
        "To Do" => &["Proposed"],
        "In Progress" => &["InProgress"],
        "Done" => &["Completed", "Resolved"],
        _ => &[],
    }
}

/// The state a drop on `col` should move an item of `wi_type` to, ported
/// from v1 _state_for_column: prefer a state named exactly like the column
/// within the column's own categories (so Task/Bug dropped on In Progress
/// becomes "In Progress", not merely the first InProgress state like
/// "Active"); otherwise the first state of the column's categories in
/// workflow order. None when the process defines no such state.
pub fn state_for_column(
    wi_type: &str,
    col: &str,
    states_by_type: &HashMap<String, Vec<StateInfo>>,
) -> Option<String> {
    let states = states_by_type.get(wi_type)?;
    let cats = column_categories(col);
    let target = col.trim().to_lowercase();
    for s in states {
        if cats.contains(&s.category.as_str()) && s.name.trim().to_lowercase() == target {
            return Some(s.name.clone());
        }
    }
    for cat in cats {
        for s in states {
            if s.category == *cat {
                return Some(s.name.clone());
            }
        }
    }
    None
}

impl AdoClient {
    /// Run a WIQL query and return matching ids (query only). Ported from v1
    /// query_work_items.
    pub async fn query_work_items(
        &self,
        org: &str,
        project: &str,
        wiql: &str,
        top: u32,
    ) -> Result<Vec<i32>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/wiql?$top={}&api-version=7.1",
            self.base_url, org, project, top
        );
        let body = self
            .post_json_query(url, &serde_json::json!({ "query": wiql }))
            .await?;
        Ok(body["workItems"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|w| w["id"].as_i64().map(|i| i as i32))
            .collect())
    }

    /// States defined for a work-item type on this project's process. Read only.
    pub async fn get_work_item_states(
        &self,
        org: &str,
        project: &str,
        wi_type: &str,
    ) -> Result<Vec<StateInfo>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes/{}/states?api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(wi_type)
        );
        let data = self.get_json(url).await?;
        Ok(data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|s| StateInfo {
                name: s["name"].as_str().unwrap_or_default().to_string(),
                color: s["color"].as_str().unwrap_or_default().to_string(),
                category: s["category"].as_str().unwrap_or_default().to_string(),
            })
            .collect())
    }

    /// The whole personal board in one call, ported from v1 _fetch_work with
    /// scope "me": WIQL for @Me minus test artifacts, batch field fetch
    /// (chunks of 200), states per distinct type (a type whose states can't
    /// be read falls back to the name heuristic). Read only.
    pub async fn fetch_board(&self, org: &str, project: &str) -> Result<BoardData, AdoError> {
        let excluded = EXCLUDED_TYPES
            .iter()
            .map(|t| format!("'{t}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let wiql = format!(
            "SELECT [System.Id] FROM workitems WHERE [System.TeamProject] = @project \
             AND [System.WorkItemType] NOT IN ({excluded}) \
             AND [System.AssignedTo] = @Me \
             ORDER BY [System.ChangedDate] DESC"
        );
        let ids = self.query_work_items(org, project, &wiql, MAX_ITEMS).await?;

        let mut raw_items: Vec<serde_json::Value> = vec![];
        for chunk in ids.chunks(200) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/{}/_apis/wit/workitems?ids={}&fields={}&api-version=7.1",
                self.base_url, org, project, ids_csv, BOARD_FIELDS
            );
            let data = self.get_json(url).await?;
            raw_items.extend(data["value"].as_array().cloned().unwrap_or_default());
        }

        let mut states_by_type: HashMap<String, Vec<StateInfo>> = HashMap::new();
        for w in &raw_items {
            let wtype = w["fields"]["System.WorkItemType"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if !wtype.is_empty() && !states_by_type.contains_key(&wtype) {
                let states = self
                    .get_work_item_states(org, project, &wtype)
                    .await
                    .unwrap_or_default();
                states_by_type.insert(wtype, states);
            }
        }

        // Preserve WIQL order (ChangedDate DESC), not batch-GET order.
        let by_id: HashMap<i64, &serde_json::Value> = raw_items
            .iter()
            .filter_map(|w| Some((w["id"].as_i64()?, w)))
            .collect();
        let items = ids
            .iter()
            .filter_map(|id| {
                let w = by_id.get(&(*id as i64))?;
                let f = &w["fields"];
                let wtype = f["System.WorkItemType"].as_str().unwrap_or_default().to_string();
                let state = f["System.State"].as_str().unwrap_or_default().to_string();
                let state_color = states_by_type
                    .get(&wtype)
                    .and_then(|ss| ss.iter().find(|s| s.name == state))
                    .map(|s| s.color.clone())
                    .unwrap_or_default();
                Some(BoardItem {
                    id: *id,
                    title: f["System.Title"].as_str().unwrap_or_default().to_string(),
                    column: column_for_state(&wtype, &state, &states_by_type),
                    work_item_type: wtype,
                    state,
                    state_color,
                    assigned_to: f["System.AssignedTo"]["displayName"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    tags: f["System.Tags"].as_str().unwrap_or_default().to_string(),
                    priority: f["Microsoft.VSTS.Common.Priority"].as_i64().map(|i| i as i32),
                    changed_date: f["System.ChangedDate"].as_str().unwrap_or_default().to_string(),
                })
            })
            .collect();

        Ok(BoardData { items, states_by_type })
    }
}
