//! The board fetch pipeline (WIQL scoping, batched item reads, state
//! resolution) and the team / member / picklist queries around it.

use std::collections::HashMap;

use super::{
    wiql_str, BoardData, BoardItem, BoardParent, Member, StateInfo, TeamRef, BOARD_FIELDS,
    BOARD_FIELDS_ALL, EXCLUDED_TYPES, MAX_ITEMS, TITLE_FIELDS,
};
use crate::ado::endpoints::percent_encode_segment;
use crate::ado::{AdoClient, AdoError};
use crate::cache::{self, keys};
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
        self.query_work_items_ctx(org, project, None, wiql, top).await
    }

    /// Same, optionally running in a TEAM context - required for WIQL
    /// macros like @CurrentIteration, which have no meaning project-wide.
    pub async fn query_work_items_ctx(
        &self,
        org: &str,
        project: &str,
        team: Option<&str>,
        wiql: &str,
        top: u32,
    ) -> Result<Vec<i32>, AdoError> {
        let team_seg = team
            .map(|t| format!("/{}", urlencoding::encode(t)))
            .unwrap_or_default();
        let url = format!(
            "{}/{}/{}{}/_apis/wit/wiql?$top={}&api-version=7.1",
            self.base_url, org, project, team_seg, top
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

    /// Run a WIQL query against the whole organization and return matching
    /// ids. With no project in the route the query may match items of any
    /// project, so its WHERE clause must not name one. Query only.
    pub async fn query_work_items_org(
        &self,
        org: &str,
        wiql: &str,
        top: u32,
    ) -> Result<Vec<i32>, AdoError> {
        let url = format!(
            "{}/{}/_apis/wit/wiql?$top={}&api-version=7.1",
            self.base_url, org, top
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

    /// The project's default team name (every project has one) - the team
    /// context @CurrentIteration resolves against. Read only.
    pub async fn default_team(&self, org: &str, project: &str) -> Result<String, AdoError> {
        let url = format!(
            "{}/{}/_apis/projects/{}?api-version=7.1",
            self.base_url,
            org,
            urlencoding::encode(project)
        );
        let data = self.get_json(url).await?;
        Ok(data["defaultTeam"]["name"].as_str().unwrap_or_default().to_string())
    }

    /// The iteration path a team-scoped "this sprint" filter should use.
    /// This org gives every team its own sprints, so @CurrentIteration in
    /// the default-team context matches nothing - instead, read the TEAM's
    /// subscribed iterations: prefer the one ADO marks `current`; when the
    /// next sprint hasn't been created yet, fall back to the most recently
    /// finished one (user rule: show the previous sprint until the new one
    /// exists). Ok(None) when the team doesn't exist under this name or has
    /// no dated iterations - callers then use the old default-team clause.
    pub async fn team_sprint_path(
        &self,
        org: &str,
        project: &str,
        team: &str,
    ) -> Result<Option<String>, AdoError> {
        let url = format!(
            "{}/{}/{}/{}/_apis/work/teamsettings/iterations?api-version=7.1",
            self.base_url,
            org,
            urlencoding::encode(project),
            urlencoding::encode(team)
        );
        let data = match self.get_json(url).await {
            Ok(d) => d,
            // Area name that isn't a team name - not an error, just no info.
            Err(AdoError::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        };
        let iterations = data["value"].as_array().cloned().unwrap_or_default();
        if let Some(current) = iterations
            .iter()
            .find(|it| it["attributes"]["timeFrame"].as_str() == Some("current"))
        {
            return Ok(current["path"].as_str().map(str::to_string));
        }
        // No current sprint (not created in time): latest finished one wins.
        let latest_past = iterations
            .iter()
            .filter(|it| it["attributes"]["timeFrame"].as_str() == Some("past"))
            .max_by_key(|it| it["attributes"]["finishDate"].as_str().unwrap_or("").to_string());
        Ok(latest_past.and_then(|it| it["path"].as_str().map(str::to_string)))
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
            percent_encode_segment(project),
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

    /// `get_work_item_states` for the board across projects, remembered for
    /// `WORK_ITEM_STATES_TTL` (session tier only): that board reads one type
    /// per project present on every load, and a type's states almost never
    /// change. A failed read is not remembered - the card falls back to the
    /// name heuristic this time and the read is tried again next load.
    async fn work_item_states_remembered(
        &self,
        org: &str,
        project: &str,
        wi_type: &str,
    ) -> Vec<StateInfo> {
        let key = keys::work_item_states(&self.base_url, org, project, wi_type);
        if let Some(hit) = cache::session_fresh::<Vec<StateInfo>>(&key, keys::WORK_ITEM_STATES_TTL) {
            return hit;
        }
        match self.get_work_item_states(org, project, wi_type).await {
            Ok(states) => {
                cache::session_put(&key, states.clone());
                states
            }
            Err(_) => Vec::new(),
        }
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
    /// An empty list means the field has no picklist (free text). A failed
    /// lookup is an Err - the two are not the same thing.
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
        // The error is PROPAGATED. Mapping it to an empty list here made a
        // failed lookup indistinguishable from "this field is free text",
        // and callers read empty as "nothing to check against" - so a 403
        // or a dropped connection came out the far end as a clean bill of
        // health. Callers that genuinely want to shrug can still do so, but
        // now they have to say it.
        let data = self.get_json(url).await?;
        Ok(data["allowedValues"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect())
    }

    /// Title and type of each readable id, read in batches of 200 with
    /// `errorPolicy=omit` so a deleted, moved or forbidden id comes back as
    /// a null entry instead of failing its whole batch. Never fails: a batch
    /// that errors is logged and its ids are simply absent from the answer.
    /// Read only.
    pub(crate) async fn read_titles(
        &self,
        org: &str,
        project: &str,
        ids: &[i32],
    ) -> HashMap<i32, (String, String)> {
        let mut out = HashMap::new();
        for chunk in ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/{}/_apis/wit/workitems?ids={}&fields={}&errorPolicy=omit&api-version=7.1",
                self.base_url, org, project, ids_csv, TITLE_FIELDS
            );
            match self.get_json(url).await {
                Ok(data) => {
                    for w in data["value"].as_array().cloned().unwrap_or_default() {
                        // An omitted id is a null entry: nothing to read.
                        let Some(id) = w["id"].as_i64() else { continue };
                        let f = &w["fields"];
                        out.insert(
                            id as i32,
                            (
                                f["System.Title"].as_str().unwrap_or_default().to_string(),
                                f["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
                            ),
                        );
                    }
                }
                Err(e) => crate::applog::warn(format!(
                    "could not read the titles of {} work item(s); they show as ids only: {e}",
                    chunk.len()
                )),
            }
        }
        out
    }

    /// `read_titles` at organization scope: title, type and project of each
    /// readable id, wherever it lives. Same batching and `errorPolicy=omit`;
    /// never fails (a batch that errors is logged and its ids are absent).
    /// Read only.
    pub(crate) async fn read_titles_org(
        &self,
        org: &str,
        ids: &[i32],
    ) -> HashMap<i32, (String, String, String)> {
        let mut out = HashMap::new();
        for chunk in ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/_apis/wit/workitems?ids={}&fields={},System.TeamProject&errorPolicy=omit&api-version=7.1",
                self.base_url, org, ids_csv, TITLE_FIELDS
            );
            match self.get_json(url).await {
                Ok(data) => {
                    for w in data["value"].as_array().cloned().unwrap_or_default() {
                        let Some(id) = w["id"].as_i64() else { continue };
                        let f = &w["fields"];
                        out.insert(
                            id as i32,
                            (
                                f["System.Title"].as_str().unwrap_or_default().to_string(),
                                f["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
                                f["System.TeamProject"].as_str().unwrap_or_default().to_string(),
                            ),
                        );
                    }
                }
                Err(e) => crate::applog::warn(format!(
                    "could not read the titles of {} work item(s); they show as ids only: {e}",
                    chunk.len()
                )),
            }
        }
        out
    }

    /// The board of every project at once: the same scopes as `fetch_board`
    /// ("me", an area, a PBI) asked of the organization, so the WHERE clause
    /// never names a project. Each card carries the project Azure DevOps says
    /// it lives in, states are read once per (project, type) because two
    /// projects can run different processes, and a parent in another project
    /// is read with its project. A sprint filter applies to an area board
    /// only, resolved from the area's own project and team. Read only.
    pub async fn fetch_board_all(
        &self,
        org: &str,
        area: Option<&str>,
        pbi_id: Option<i32>,
        current_sprint: bool,
    ) -> Result<BoardData, AdoError> {
        let excluded = EXCLUDED_TYPES
            .iter()
            .map(|t| format!("'{t}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut where_clauses = vec![format!("[System.WorkItemType] NOT IN ({excluded})")];
        match (pbi_id, area) {
            (Some(id), _) => {
                where_clauses.push(format!("([System.Parent] = {id} OR [System.Id] = {id})"));
            }
            (None, Some(area)) => {
                where_clauses.push(format!("[System.AreaPath] UNDER {}", wiql_str(area)));
            }
            (None, None) => where_clauses.push("[System.AssignedTo] = @Me".to_string()),
        }
        // (project, team) the @CurrentIteration macro must run in; None when
        // the sprint clause is an explicit path (or there is no sprint).
        let mut macro_ctx: Option<(String, String)> = None;
        if let (true, None, Some(area)) = (current_sprint, pbi_id, area) {
            // The area is "<project>\...\<team>": the first segment is the
            // project it lives in, the last one the team (this org's
            // convention). Anything that cannot resolve falls back to the
            // project's default-team @CurrentIteration clause.
            let root = area.split('\\').next().unwrap_or_default();
            let team = area.rsplit('\\').next().unwrap_or_default();
            if !root.is_empty() && !team.is_empty() {
                match self.team_sprint_path(org, root, team).await? {
                    Some(path) => where_clauses
                        .push(format!("[System.IterationPath] UNDER {}", wiql_str(&path))),
                    None => {
                        where_clauses
                            .push("[System.IterationPath] = @CurrentIteration".to_string());
                        macro_ctx = Some((
                            root.to_string(),
                            self.default_team(org, root).await?,
                        ));
                    }
                }
            }
        }
        let wiql = format!(
            "SELECT [System.Id] FROM workitems WHERE {} ORDER BY [System.ChangedDate] DESC",
            where_clauses.join(" AND ")
        );
        let ids = match &macro_ctx {
            Some((project, team)) => {
                self.query_work_items_ctx(org, project, Some(team), &wiql, MAX_ITEMS)
                    .await?
            }
            None => self.query_work_items_org(org, &wiql, MAX_ITEMS).await?,
        };

        let mut raw_items: Vec<serde_json::Value> = vec![];
        for chunk in ids.chunks(200) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/_apis/wit/workitems?ids={}&fields={}&api-version=7.1",
                self.base_url, org, ids_csv, BOARD_FIELDS_ALL
            );
            let data = self.get_json(url).await?;
            raw_items.extend(data["value"].as_array().cloned().unwrap_or_default());
        }

        // One state read per (project, type) present. A card whose project
        // came back blank has no process to ask, so it keeps the name
        // heuristic.
        let mut states_by_project: HashMap<String, HashMap<String, Vec<StateInfo>>> =
            HashMap::new();
        for w in &raw_items {
            let f = &w["fields"];
            let project = f["System.TeamProject"].as_str().unwrap_or_default();
            let wtype = f["System.WorkItemType"].as_str().unwrap_or_default();
            if project.is_empty() || wtype.is_empty() {
                continue;
            }
            if states_by_project.get(project).is_some_and(|m| m.contains_key(wtype)) {
                continue;
            }
            let states = self.work_item_states_remembered(org, project, wtype).await;
            states_by_project
                .entry(project.to_string())
                .or_default()
                .insert(wtype.to_string(), states);
        }

        // Each card's direct parent, as in `fetch_board`, but a parent may
        // live in another project, so its project is kept too.
        let mut known: HashMap<i32, (String, String, String)> = raw_items
            .iter()
            .filter_map(|w| {
                let f = &w["fields"];
                Some((
                    w["id"].as_i64()? as i32,
                    (
                        f["System.Title"].as_str().unwrap_or_default().to_string(),
                        f["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
                        f["System.TeamProject"].as_str().unwrap_or_default().to_string(),
                    ),
                ))
            })
            .collect();
        let mut to_read: Vec<i32> = vec![];
        let mut queued = std::collections::HashSet::new();
        for w in &raw_items {
            if let Some(p) = w["fields"]["System.Parent"].as_i64().map(|p| p as i32) {
                if !known.contains_key(&p) && queued.insert(p) {
                    to_read.push(p);
                }
            }
        }
        known.extend(self.read_titles_org(org, &to_read).await);
        let mut parent_projects: HashMap<i32, String> = HashMap::new();
        let mut parent_of = |f: &serde_json::Value| {
            f["System.Parent"].as_i64().map(|p| {
                let id = p as i32;
                let (title, work_item_type, project) = known.get(&id).cloned().unwrap_or_default();
                if !project.is_empty() {
                    parent_projects.insert(id, project);
                }
                BoardParent { id, title, work_item_type }
            })
        };

        // Preserve WIQL order (ChangedDate DESC), not batch-GET order.
        let by_id: HashMap<i64, &serde_json::Value> = raw_items
            .iter()
            .filter_map(|w| Some((w["id"].as_i64()?, w)))
            .collect();
        let no_states = HashMap::new();
        let items = ids
            .iter()
            .filter_map(|id| {
                let w = by_id.get(&(*id as i64))?;
                let f = &w["fields"];
                let project = f["System.TeamProject"].as_str().unwrap_or_default().to_string();
                let wtype = f["System.WorkItemType"].as_str().unwrap_or_default().to_string();
                let state = f["System.State"].as_str().unwrap_or_default().to_string();
                let own_states = states_by_project.get(&project).unwrap_or(&no_states);
                let state_color = own_states
                    .get(&wtype)
                    .and_then(|ss| ss.iter().find(|s| s.name == state))
                    .map(|s| s.color.clone())
                    .unwrap_or_default();
                Some(BoardItem {
                    id: *id,
                    title: f["System.Title"].as_str().unwrap_or_default().to_string(),
                    column: column_for_state(&wtype, &state, own_states),
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
                    parent: parent_of(f),
                    project,
                })
            })
            .collect();

        Ok(BoardData {
            items,
            states_by_type: HashMap::new(),
            states_by_project,
            parent_projects,
        })
    }

    /// The board in one call, ported from v1 _fetch_work: scope is "me"
    /// (AssignedTo = @Me), an area (everything UNDER that area path,
    /// whoever it's assigned to - areas are the classification tree the
    /// user actually sees, unlike the project's often-stale team list),
    /// or a PBI (everything parented under it, plus the PBI itself - PBI
    /// wins when both arrive). `current_sprint` adds an
    /// @CurrentIteration clause, run in the project's default-team
    /// context (the macro needs a team to resolve against). Test
    /// artifacts excluded in every mode. Read only.
    pub async fn fetch_board(
        &self,
        org: &str,
        project: &str,
        area: Option<&str>,
        pbi_id: Option<i32>,
        current_sprint: bool,
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
        match (pbi_id, area) {
            (Some(id), _) => {
                // Parent links only catch properly-parented items - the UI
                // shows the resulting count so a sparse board is visibly
                // "few children", not silently broken.
                where_clauses.push(format!("([System.Parent] = {id} OR [System.Id] = {id})"));
            }
            (None, Some(area)) => {
                // UNDER includes the area's whole subtree; the path comes
                // from the project's own classification list, quoted via
                // wiql_str like every interpolated string.
                where_clauses.push(format!("[System.AreaPath] UNDER {}", wiql_str(area)));
            }
            (None, None) => where_clauses.push("[System.AssignedTo] = @Me".to_string()),
        }
        let team_ctx = if current_sprint {
            // Area boards resolve the sprint from THAT team's iterations
            // (teams here have their own sprints; the default team's
            // @CurrentIteration matches none of them). The team is the
            // area's last segment - this org's convention. Anything that
            // can't resolve falls back to the old default-team clause.
            let area_team = if pbi_id.is_none() {
                area.and_then(|a| a.rsplit('\\').next()).filter(|s| !s.is_empty())
            } else {
                None
            };
            let team_sprint = match area_team {
                Some(team) => self.team_sprint_path(org, project, team).await?,
                None => None,
            };
            match team_sprint {
                Some(path) => {
                    where_clauses.push(format!("[System.IterationPath] UNDER {}", wiql_str(&path)));
                    None
                }
                None => {
                    where_clauses.push("[System.IterationPath] = @CurrentIteration".to_string());
                    Some(self.default_team(org, project).await?)
                }
            }
        } else {
            None
        };
        let wiql = format!(
            "SELECT [System.Id] FROM workitems WHERE {} ORDER BY [System.ChangedDate] DESC",
            where_clauses.join(" AND ")
        );
        let ids = self
            .query_work_items_ctx(org, project, team_ctx.as_deref(), &wiql, MAX_ITEMS)
            .await?;

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

        // Each card's direct parent, for the board's swimlanes. A parent
        // that is itself on the board is already in hand; the others are
        // read once each, and one that cannot be read keeps its id with an
        // empty title. This read never fails the board.
        let mut known: HashMap<i32, (String, String)> = raw_items
            .iter()
            .filter_map(|w| {
                let f = &w["fields"];
                Some((
                    w["id"].as_i64()? as i32,
                    (
                        f["System.Title"].as_str().unwrap_or_default().to_string(),
                        f["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
                    ),
                ))
            })
            .collect();
        let mut to_read: Vec<i32> = vec![];
        let mut queued = std::collections::HashSet::new();
        for w in &raw_items {
            if let Some(p) = w["fields"]["System.Parent"].as_i64().map(|p| p as i32) {
                if !known.contains_key(&p) && queued.insert(p) {
                    to_read.push(p);
                }
            }
        }
        known.extend(self.read_titles(org, project, &to_read).await);
        let parent_of = |f: &serde_json::Value| {
            f["System.Parent"].as_i64().map(|p| {
                let id = p as i32;
                let (title, work_item_type) = known.get(&id).cloned().unwrap_or_default();
                BoardParent { id, title, work_item_type }
            })
        };

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
                    parent: parent_of(f),
                    project: project.to_string(),
                })
            })
            .collect();

        Ok(BoardData {
            items,
            states_by_type,
            states_by_project: HashMap::new(),
            parent_projects: HashMap::new(),
        })
    }
}
