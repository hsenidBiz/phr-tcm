//! The board fetch pipeline (WIQL scoping, batched item reads, state
//! resolution) and the team / member / picklist queries around it.

use std::collections::HashMap;

use super::{
    team_area_clause, BoardData, BoardItem, Member, StateInfo, TeamRef, BOARD_FIELDS,
    EXCLUDED_TYPES, MAX_ITEMS,
};
use crate::ado::{AdoClient, AdoError};
use super::column_for_state;

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

    /// Teams in the project (for the board's team-scope selector). Read only.
    pub async fn list_teams(&self, org: &str, project: &str) -> Result<Vec<TeamRef>, AdoError> {
        let url = format!(
            "{}/{}/_apis/projects/{}/teams?api-version=7.1",
            self.base_url,
            org,
            urlencoding::encode(project)
        );
        let data = self.get_json(url).await?;
        Ok(data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|t| TeamRef {
                id: t["id"].as_str().unwrap_or_default().to_string(),
                name: t["name"].as_str().unwrap_or_default().to_string(),
            })
            .collect())
    }

    /// A team's team-field (usually AreaPath) values, ported from v1
    /// get_team_field_values. Returns (field_ref, values) where values fall
    /// back to the default with includeChildren when the list is empty.
    pub async fn get_team_scope(
        &self,
        org: &str,
        project: &str,
        team: &str,
    ) -> Result<(String, Vec<(String, bool)>), AdoError> {
        let url = format!(
            "{}/{}/{}/{}/_apis/work/teamsettings/teamfieldvalues?api-version=7.1",
            self.base_url,
            org,
            urlencoding::encode(project),
            urlencoding::encode(team)
        );
        let data = self.get_json(url).await?;
        let field_ref = data["field"]["referenceName"]
            .as_str()
            .unwrap_or("System.AreaPath")
            .to_string();
        let mut values: Vec<(String, bool)> = data["values"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|v| {
                (
                    v["value"].as_str().unwrap_or_default().to_string(),
                    v["includeChildren"].as_bool().unwrap_or(false),
                )
            })
            .collect();
        if values.is_empty() {
            let default = data["defaultValue"].as_str().unwrap_or_default();
            if !default.is_empty() {
                values.push((default.to_string(), true));
            }
        }
        Ok((field_ref, values))
    }

    /// All members across the project's teams, deduped by uniqueName and
    /// sorted by display name (v1 get_team_members, without the thread pool -
    /// team counts are small). Read only.
    pub async fn list_team_members(&self, org: &str, project: &str) -> Result<Vec<Member>, AdoError> {
        let teams = self.list_teams(org, project).await?;
        let mut by_unique: std::collections::HashMap<String, Member> = Default::default();
        for team in teams {
            let url = format!(
                "{}/{}/_apis/projects/{}/teams/{}/members?api-version=7.1",
                self.base_url,
                org,
                urlencoding::encode(project),
                team.id
            );
            let Ok(data) = self.get_json(url).await else { continue };
            for m in data["value"].as_array().cloned().unwrap_or_default() {
                let identity = &m["identity"];
                let unique = identity["uniqueName"].as_str().unwrap_or_default().to_string();
                if unique.is_empty() {
                    continue;
                }
                by_unique.insert(
                    unique.clone(),
                    Member {
                        display_name: identity["displayName"].as_str().unwrap_or_default().to_string(),
                        unique_name: unique,
                    },
                );
            }
        }
        let mut members: Vec<Member> = by_unique.into_values().collect();
        members.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
        Ok(members)
    }

    /// Allowed (picklist) values for a field on a type - e.g. Activity.
    /// Absent field / no picklist -> empty (v1 get_field_allowed_values).
    pub async fn get_field_allowed_values(
        &self,
        org: &str,
        project: &str,
        wi_type: &str,
        field_ref: &str,
    ) -> Result<Vec<String>, AdoError> {
        // $expand=allowedValues is REQUIRED for custom picklist fields -
        // without it ADO omits allowedValues for them entirely.
        let url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes/{}/fields/{}?$expand=allowedValues&api-version=7.1",
            self.base_url,
            org,
            urlencoding::encode(project),
            urlencoding::encode(wi_type),
            urlencoding::encode(field_ref)
        );
        match self.get_json(url).await {
            Ok(data) => Ok(data["allowedValues"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()),
            Err(_) => Ok(vec![]),
        }
    }

    /// The board in one call, ported from v1 _fetch_work: scope is "me"
    /// (AssignedTo = @Me) or a team (everything under the team's area(s),
    /// whoever it's assigned to). Test artifacts excluded in both. Read only.
    pub async fn fetch_board(
        &self,
        org: &str,
        project: &str,
        team: Option<&str>,
    ) -> Result<BoardData, AdoError> {
        let excluded = EXCLUDED_TYPES
            .iter()
            .map(|t| format!("'{t}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut where_clauses = vec![
            "[System.TeamProject] = @project".to_string(),
            format!("[System.WorkItemType] NOT IN ({excluded})"),
        ];
        match team {
            Some(team) => {
                let (field_ref, values) = self.get_team_scope(org, project, team).await?;
                let clause = team_area_clause(&field_ref, &values);
                if !clause.is_empty() {
                    where_clauses.push(format!("({clause})"));
                }
            }
            None => where_clauses.push("[System.AssignedTo] = @Me".to_string()),
        }
        let wiql = format!(
            "SELECT [System.Id] FROM workitems WHERE {} ORDER BY [System.ChangedDate] DESC",
            where_clauses.join(" AND ")
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
