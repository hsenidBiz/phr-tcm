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

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct WorkItemDetail {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
    pub assigned_to: String,
    pub assigned_to_unique: String,
    pub activity: String,
    pub tags: String,
    pub area_path: String,
    pub iteration_path: String,
    pub remaining_work: Option<f64>,
    pub completed_work: Option<f64>,
    pub original_estimate: Option<f64>,
    pub start_date: String,
    pub finish_date: String,
    /// Description (or ReproSteps for Bugs) flattened to plain text for the
    /// editor; saving wraps it back into a div like v1's preconditions.
    pub description_text: String,
    /// The same field's RAW HTML, so the editor can convert it to markdown
    /// and preserve the formatting ADO stored (bold, lists, links...).
    pub description_html: String,
    /// Which field the description came from (System.Description or
    /// Microsoft.VSTS.TCM.ReproSteps) so the save writes the right one.
    pub description_field: String,
    /// The process's extra form pages (Bug: RCA, Preventive Measures...)
    /// with every visible field on them, shown as editable tabs.
    pub extra_pages: Vec<ExtraPage>,
    /// Why extra_pages is empty when the layout lookup failed - surfaced in
    /// the drawer so a permissions/endpoint problem is visible, not silent.
    pub extra_pages_error: Option<String>,
    /// Authenticated attachment images from the rich-text fields, downloaded
    /// with the token so the preview can swap URLs for data: URIs (a plain
    /// <img> gets 401). Field values themselves stay byte-faithful.
    pub inline_images: Vec<InlineImage>,
}

/// One downloaded rich-text image: the (entity-unescaped) src URL and the
/// data: URI to show instead.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct InlineImage {
    pub url: String,
    pub data: String,
}

/// One custom form page (an ADO tab) and its editable fields, in form order.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ExtraPage {
    pub name: String,
    pub fields: Vec<ExtraField>,
}

/// A single field on an extra page.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ExtraField {
    pub label: String,
    pub reference_name: String,
    /// Which form section (column in ADO's layout) the field sits in.
    pub section: u32,
    /// "html" (rich text), "pick" (allowed values), or "text".
    pub kind: String,
    /// Allowed values when kind == "pick".
    pub allowed: Vec<String>,
    /// Current raw value (HTML for html fields).
    pub value: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Member {
    pub display_name: String,
    pub unique_name: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TeamRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct WorkComment {
    pub id: i32,
    pub text: String,
    pub created_by: String,
    pub created_date: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct FieldPatch {
    pub reference_name: String,
    pub value: String,
}

/// A WIQL string literal with quotes escaped (v1 _wiql_str).
pub fn wiql_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// A WIQL clause scoping to a team's area(s), ported from v1
/// _team_area_clause: tree fields use UNDER when includeChildren, else '='.
pub fn team_area_clause(field_ref: &str, values: &[(String, bool)]) -> String {
    let field = format!("[{field_ref}]");
    let tree = field_ref.ends_with("AreaPath") || field_ref.ends_with("IterationPath");
    values
        .iter()
        .filter(|(v, _)| !v.is_empty())
        .map(|(v, include_children)| {
            let op = if tree && *include_children { "UNDER" } else { "=" };
            format!("{field} {op} {}", wiql_str(v))
        })
        .collect::<Vec<_>>()
        .join(" OR ")
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

    /// One work item, fully loaded for the detail drawer. Read only.
    pub async fn get_work_item_detail(
        &self,
        org: &str,
        project: &str,
        id: i32,
    ) -> Result<WorkItemDetail, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url, org, project, id
        );
        let data = self.get_json(url).await?;
        let f = &data["fields"];
        let s = |key: &str| f[key].as_str().unwrap_or_default().to_string();
        let wi_type = s("System.WorkItemType");
        let description_field = if wi_type == "Bug" && f["Microsoft.VSTS.TCM.ReproSteps"].is_string()
        {
            "Microsoft.VSTS.TCM.ReproSteps"
        } else {
            "System.Description"
        };
        // The process can put extra form pages on a type (Bug: RCA,
        // Preventive Measures). Discover them from the type's layout so
        // every field on those tabs is shown, whatever the org calls them.
        // A failed lookup means no extra tabs, with the reason surfaced.
        let (extra_pages, extra_pages_error) =
            match self.extra_pages_for(org, project, &wi_type, f).await {
                Ok(pages) => (pages, None),
                Err(e) => (Vec::new(), Some(e)),
            };
        // Attachment images need auth the WebView can't send. Field values
        // stay byte-faithful (edits must round-trip the URLs, not megabytes
        // of base64) - the preview applies this url -> data-uri map instead.
        let mut rich_htmls: Vec<&str> = vec![];
        let desc_html = s(description_field);
        rich_htmls.push(&desc_html);
        for page in &extra_pages {
            for field in &page.fields {
                if field.kind == "html" {
                    rich_htmls.push(&field.value);
                }
            }
        }
        let inline_images = self.collect_attachment_images(&rich_htmls).await;
        Ok(WorkItemDetail {
            id,
            title: s("System.Title"),
            state: s("System.State"),
            assigned_to: f["System.AssignedTo"]["displayName"].as_str().unwrap_or_default().to_string(),
            assigned_to_unique: f["System.AssignedTo"]["uniqueName"].as_str().unwrap_or_default().to_string(),
            activity: s("Microsoft.VSTS.Common.Activity"),
            tags: s("System.Tags"),
            area_path: s("System.AreaPath"),
            iteration_path: s("System.IterationPath"),
            remaining_work: f["Microsoft.VSTS.Scheduling.RemainingWork"].as_f64(),
            completed_work: f["Microsoft.VSTS.Scheduling.CompletedWork"].as_f64(),
            original_estimate: f["Microsoft.VSTS.Scheduling.OriginalEstimate"].as_f64(),
            start_date: s("Microsoft.VSTS.Scheduling.StartDate"),
            finish_date: s("Microsoft.VSTS.Scheduling.FinishDate"),
            description_text: crate::steps_xml::html_to_text(&desc_html),
            description_html: desc_html,
            description_field: description_field.to_string(),
            inline_images,
            work_item_type: wi_type,
            extra_pages,
            extra_pages_error,
        })
    }

    /// The type's custom form pages beyond the first (main) one, with every
    /// visible field control they contain - the same tabs ADO's own form
    /// shows (Bug: RCA, Preventive Measures). Fields are included even when
    /// empty so the drawer can fill them in.
    ///
    /// The layout lives in the org-level PROCESSES api (there is no
    /// project-scoped layout endpoint), so this chains: project capabilities
    /// (process id) -> work item type (reference name) -> process layout.
    /// Every hop reports its failure so the drawer can show why.
    async fn extra_pages_for(
        &self,
        org: &str,
        project: &str,
        wi_type: &str,
        item_fields: &serde_json::Value,
    ) -> Result<Vec<ExtraPage>, String> {
        // Capabilities work with the project NAME (the properties api wants
        // a GUID) and carry the current process template id.
        let proj_url = format!(
            "{}/{}/_apis/projects/{}?includeCapabilities=true&api-version=7.1",
            self.base_url,
            org,
            urlencoding::encode(project)
        );
        let proj = self
            .get_json(proj_url)
            .await
            .map_err(|e| format!("project lookup failed: {e}"))?;
        let process_id = proj["capabilities"]["processTemplate"]["templateTypeId"]
            .as_str()
            .ok_or("project capabilities carried no process template id")?
            .to_string();

        let wit_url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes/{}?api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(wi_type)
        );
        let wit = self
            .get_json(wit_url)
            .await
            .map_err(|e| format!("work item type lookup failed: {e}"))?;
        let Some(wit_ref) = wit["referenceName"].as_str().map(String::from) else {
            return Err("work item type carried no referenceName".into());
        };

        let layout_url = format!(
            "{}/{}/_apis/work/processes/{}/workItemTypes/{}/layout?api-version=7.1-preview.1",
            self.base_url,
            org,
            process_id,
            urlencoding::encode(&wit_ref)
        );
        let layout = self
            .get_json(layout_url)
            .await
            .map_err(|e| format!("process layout failed: {e}"))?;
        let Some(pages) = layout["pages"].as_array() else {
            return Err("process layout carried no pages".into());
        };

        let mut out = Vec::new();
        // The layout lists the main Details page first plus system pages
        // (history/links/attachments). The extra tabs are every "custom"
        // page after that first one.
        let mut seen_main = false;
        for page in pages {
            if page["pageType"].as_str() != Some("custom")
                || page["visible"].as_bool() == Some(false)
            {
                continue;
            }
            if !seen_main {
                seen_main = true; // the drawer already covers the main form
                continue;
            }
            let name = page["label"].as_str().unwrap_or_default().to_string();
            let mut fields = Vec::new();
            let mut section_no: u32 = 0;
            for section in page["sections"].as_array().into_iter().flatten() {
                let mut section_used = false;
                for group in section["groups"].as_array().into_iter().flatten() {
                    let group_label = group["label"].as_str().unwrap_or_default();
                    for control in group["controls"].as_array().into_iter().flatten() {
                        if let Some(mut f) = self
                            .extra_field_from_control(
                                org,
                                project,
                                wi_type,
                                control,
                                group_label,
                                item_fields,
                            )
                            .await
                        {
                            f.section = section_no;
                            section_used = true;
                            fields.push(f);
                        }
                    }
                }
                // Only sections that contributed fields advance the column
                // count (ADO pads layouts with empty sections).
                if section_used {
                    section_no += 1;
                }
            }
            if !name.is_empty() && !fields.is_empty() {
                out.push(ExtraPage { name, fields });
            }
        }
        Ok(out)
    }

    /// Map one layout control to an editable field (None for non-field
    /// controls: links, attachments, extensions, history, hidden).
    async fn extra_field_from_control(
        &self,
        org: &str,
        project: &str,
        wi_type: &str,
        control: &serde_json::Value,
        group_label: &str,
        item_fields: &serde_json::Value,
    ) -> Option<ExtraField> {
        if control["visible"].as_bool() == Some(false)
            || control["isContribution"].as_bool() == Some(true)
        {
            return None;
        }
        let control_type = control["controlType"].as_str().unwrap_or_default();
        let reference_name = control["id"].as_str().unwrap_or_default().to_string();
        if reference_name.is_empty() || !reference_name.contains('.') || reference_name == "System.History" {
            return None;
        }
        let kind = match control_type {
            "HtmlFieldControl" => "html".to_string(),
            "FieldControl" | "DateTimeControl" => "text".to_string(),
            _ => return None, // LinksControl, AttachmentsControl, extensions...
        };
        // Rich-text controls usually sit in their own group whose label is
        // what ADO's form displays; the control label is the fallback.
        let label = [control["label"].as_str().unwrap_or_default(), group_label]
            .into_iter()
            .find(|l| !l.is_empty())
            .unwrap_or(reference_name.as_str())
            .to_string();
        // Plain field controls may be picklists - ask the field definition.
        let (kind, allowed) = if kind == "text" && control_type == "FieldControl" {
            let url = format!(
                "{}/{}/{}/_apis/wit/workitemtypes/{}/fields/{}?$expand=allowedValues&api-version=7.1",
                self.base_url,
                org,
                project,
                urlencoding::encode(wi_type),
                reference_name
            );
            let allowed: Vec<String> = match self.get_json(url).await {
                Ok(def) => def["allowedValues"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                Err(_) => Vec::new(),
            };
            if allowed.is_empty() {
                ("text".to_string(), Vec::new())
            } else {
                ("pick".to_string(), allowed)
            }
        } else {
            (kind, Vec::new())
        };
        let value = match &item_fields[&reference_name] {
            serde_json::Value::Null => String::new(),
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        Some(ExtraField { label, reference_name, section: 0, kind, allowed, value })
    }

    /// A work item's comments, newest first, ported from v1
    /// get_work_item_comments incl. the avatar fallback chain
    /// (_links.avatar.href -> imageUrl -> empty = initials disc). Read only.
    pub async fn get_work_item_comments(
        &self,
        org: &str,
        project: &str,
        wi_id: i32,
    ) -> Result<Vec<WorkComment>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workItems/{}/comments?order=desc&api-version=7.1-preview.4",
            self.base_url, org, project, wi_id
        );
        let data = self.get_json(url).await?;
        Ok(data["comments"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|c| {
                let cb = &c["createdBy"];
                let avatar = cb["_links"]["avatar"]["href"]
                    .as_str()
                    .or_else(|| cb["imageUrl"].as_str())
                    .unwrap_or_default()
                    .to_string();
                WorkComment {
                    id: c["id"].as_i64().unwrap_or_default() as i32,
                    text: crate::steps_xml::html_to_text(c["text"].as_str().unwrap_or_default()),
                    created_by: cb["displayName"].as_str().unwrap_or_default().to_string(),
                    created_date: c["createdDate"].as_str().unwrap_or_default().to_string(),
                    avatar_url: avatar,
                }
            })
            .collect())
    }

    /// POST a comment; no DELETE.
    pub async fn add_work_item_comment(
        &self,
        org: &str,
        project: &str,
        wi_id: i32,
        text: &str,
    ) -> Result<(), AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workItems/{}/comments?api-version=7.1-preview.4",
            self.base_url, org, project, wi_id
        );
        self.post_json(url, &serde_json::json!({"text": text})).await?;
        Ok(())
    }

    /// Download the attachment images referenced by rich-text HTML (a plain
    /// <img> gets 401 - the WebView sends no bearer header) and return
    /// url -> data-uri pairs for the preview to apply. Best-effort per
    /// image; caps guard pathological fields.
    pub async fn collect_attachment_images(&self, htmls: &[&str]) -> Vec<InlineImage> {
        use base64::Engine;
        let re = regex::Regex::new(r#"src=["']([^"']+)["']"#).unwrap();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for html in htmls {
            if !html.contains("<img") {
                continue;
            }
            for cap in re.captures_iter(html) {
                if out.len() >= 12 {
                    return out;
                }
                // The src sits in an HTML attribute, so & is entity-encoded.
                let url = cap[1].replace("&amp;", "&");
                if !url.contains("/_apis/wit/attachments/") || !seen.insert(url.clone()) {
                    continue;
                }
                let Ok(resp) = self
                    .http
                    .get(&url)
                    .bearer_auth(&self.token)
                    .header("Accept", "application/octet-stream")
                    .send()
                    .await
                else {
                    continue;
                };
                if !resp.status().is_success() {
                    continue;
                }
                let mime = resp
                    .headers()
                    .get("Content-Type")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|c| c.split(';').next())
                    .filter(|m| m.starts_with("image/"))
                    .unwrap_or("image/png")
                    .to_string();
                let Ok(bytes) = resp.bytes().await else { continue };
                if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
                    continue; // keep the broken link rather than a 10MB blob
                }
                out.push(InlineImage {
                    url,
                    data: format!(
                        "data:{};base64,{}",
                        mime,
                        base64::engine::general_purpose::STANDARD.encode(&bytes)
                    ),
                });
            }
        }
        out
    }

    /// Fetch an avatar as base64 PNG-ish bytes. Best-effort like v1
    /// get_avatar_image: any problem -> None so the UI falls back to
    /// initials. Handles the Graph endpoint's base64-JSON body variant.
    pub async fn get_avatar_b64(&self, url: &str) -> Option<String> {
        use base64::Engine;
        if url.is_empty() {
            return None;
        }
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .header("Accept", "image/png,image/*;q=0.8")
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let is_json = resp
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .map(|c| c.contains("application/json"))
            .unwrap_or(false);
        let bytes = resp.bytes().await.ok()?;
        if bytes.is_empty() {
            return None;
        }
        if is_json {
            let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            return v["value"].as_str().map(String::from);
        }
        Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
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
