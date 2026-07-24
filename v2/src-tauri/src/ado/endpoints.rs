//! Work-item-tracking endpoints (orgs, projects, PBIs, test cases, bugs,
//! tags, classification). Every method rides the `transport` helpers; the
//! writes are POST/PATCH create-or-update only — no DELETE, ever.

use super::{
    tc_ids_i32, AdoClient, AdoError, BugTypeInfo, FieldRef, Org, PbiHit, Project,
    TestCaseFull, TestCaseSummary,
};

impl AdoClient {
    /// Two-hop org discovery, ported from v1 get_organizations():
    /// profile id first, then the accounts list. Sorted case-insensitively.
    pub async fn list_orgs(&self) -> Result<Vec<Org>, AdoError> {
        let me = self
            .get_json(format!(
                "{}/_apis/profile/profiles/me?api-version=6.0",
                self.vssps_base_url
            ))
            .await?;
        let member_id = me["id"].as_str().unwrap_or_default().to_string();
        let accounts = self
            .get_json(format!(
                "{}/_apis/accounts?memberId={}&api-version=6.0",
                self.vssps_base_url, member_id
            ))
            .await?;
        let mut orgs: Vec<Org> = accounts["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| {
                Some(Org {
                    name: a["accountName"].as_str()?.to_string(),
                    url: a["accountUri"].as_str().unwrap_or_default().to_string(),
                })
            })
            .collect();
        orgs.sort_by_key(|o| o.name.to_lowercase());
        Ok(orgs)
    }

    /// PBI search ported from v1 search_work_items(): title CONTAINS (quotes
    /// escaped), OR exact id when numeric, PBIs only, most recently changed
    /// first. WIQL order is preserved in the result.
    pub async fn search_pbis(
        &self,
        organization: &str,
        project: &str,
        text: &str,
        top: u32,
    ) -> Result<Vec<PbiHit>, AdoError> {
        let safe = text.trim().replace('\'', "''");
        let mut clause = format!("[System.Title] CONTAINS '{safe}'");
        if let Ok(id) = text.trim().parse::<i64>() {
            clause = format!("([System.Id] = {id} OR {clause})");
        }
        let wiql = format!(
            "SELECT [System.Id] FROM workitems \
             WHERE [System.TeamProject] = @project AND {clause} \
             AND [System.WorkItemType] = 'Product Backlog Item' \
             ORDER BY [System.ChangedDate] DESC"
        );
        let url = format!(
            "{}/{}/{}/_apis/wit/wiql?$top={}&api-version=7.1",
            self.base_url, organization, project, top
        );
        let body = self
            .post_json_query(url, &serde_json::json!({ "query": wiql }))
            .await?;
        let ids: Vec<i64> = body["workItems"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|w| w["id"].as_i64())
            .collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let ids_csv = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let url = format!(
            "{}/{}/_apis/wit/workitems?ids={}&fields=System.Title,System.WorkItemType&api-version=7.1",
            self.base_url, organization, ids_csv
        );
        let fetched = self.get_json(url).await?;
        let by_id: std::collections::HashMap<i64, &serde_json::Value> = fetched["value"]
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(|w| Some((w["id"].as_i64()?, &w["fields"])))
                    .collect()
            })
            .unwrap_or_default();
        Ok(ids
            .iter()
            .filter_map(|i| {
                let fields = by_id.get(i)?;
                Some(PbiHit {
                    id: *i as i32,
                    title: fields["System.Title"].as_str().unwrap_or_default().to_string(),
                    work_item_type: fields["System.WorkItemType"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect())
    }

    /// Distinct values of `field_ref` actually used on the project's Test
    /// Cases - the fallback when the field definition has no picklist (many
    /// orgs keep Modules as plain values, not allowedValues). Scans the 200
    /// most recently changed cases with the field set; values are deduped
    /// case-insensitively (first casing wins) and sorted. Read only.
    pub async fn field_values_in_use(
        &self,
        organization: &str,
        project: &str,
        field_ref: &str,
    ) -> Result<Vec<String>, AdoError> {
        // Field refs come from the project's own field list, but they are
        // interpolated into WIQL - allow only reference-name characters.
        if field_ref.is_empty()
            || !field_ref.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
        {
            return Ok(vec![]);
        }
        let wiql = format!(
            "SELECT [System.Id] FROM workitems \
             WHERE [System.TeamProject] = @project \
             AND [System.WorkItemType] = 'Test Case' \
             AND [{field_ref}] <> '' \
             ORDER BY [System.ChangedDate] DESC"
        );
        let url = format!(
            "{}/{}/{}/_apis/wit/wiql?$top=200&api-version=7.1",
            self.base_url, organization, project
        );
        let body = self
            .post_json_query(url, &serde_json::json!({ "query": wiql }))
            .await?;
        let ids: Vec<i64> = body["workItems"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|w| w["id"].as_i64())
            .collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let mut values: Vec<String> = vec![];
        let mut seen = std::collections::HashSet::new();
        for chunk in ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/_apis/wit/workitems?ids={}&fields={}&api-version=7.1",
                self.base_url,
                organization,
                ids_csv,
                urlencoding::encode(field_ref)
            );
            let fetched = self.get_json(url).await?;
            for wi in fetched["value"].as_array().cloned().unwrap_or_default() {
                if let Some(v) = wi["fields"][field_ref].as_str() {
                    let t = v.trim();
                    if !t.is_empty() && seen.insert(t.to_lowercase()) {
                        values.push(t.to_string());
                    }
                }
            }
        }
        values.sort_by_key(|v| v.to_lowercase());
        Ok(values)
    }

    /// All Test Cases linked to a PBI via TestedBy relations, ported from v1
    /// get_test_cases_for_pbi(). Fetches in batches so there is no overall cap.
    pub async fn get_pbi_test_cases(
        &self,
        organization: &str,
        pbi_id: i32,
    ) -> Result<Vec<TestCaseSummary>, AdoError> {
        let url = format!(
            "{}/{}/_apis/wit/workitems/{}?$expand=relations&api-version=7.1",
            self.base_url, organization, pbi_id
        );
        let data = self.get_json(url).await?;
        let tc_ids: Vec<i64> = data["relations"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|r| {
                r["rel"]
                    .as_str()
                    .map(|s| s.to_lowercase().contains("testedby"))
                    .unwrap_or(false)
            })
            .filter_map(|r| r["url"].as_str()?.rsplit('/').next()?.parse().ok())
            .collect();
        if tc_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut cases = Vec::with_capacity(tc_ids.len());
        for chunk in tc_ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/_apis/wit/workitems?ids={}&fields=System.Id,System.Title,System.Tags,Microsoft.VSTS.TCM.AutomationStatus&api-version=7.1",
                self.base_url, organization, ids_csv
            );
            let fetched = self.get_json(url).await?;
            for w in fetched["value"].as_array().cloned().unwrap_or_default() {
                let fields = &w["fields"];
                cases.push(TestCaseSummary {
                    id: w["id"].as_i64().unwrap_or_default() as i32,
                    title: fields["System.Title"].as_str().unwrap_or_default().to_string(),
                    tags: fields["System.Tags"].as_str().unwrap_or_default().to_string(),
                    automation_status: fields["Microsoft.VSTS.TCM.AutomationStatus"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        }
        Ok(cases)
    }

    /// POST a new Test Case work item, ported from v1 create_test_case.
    /// Only creates work items of type 'Test Case'. Returns the new id.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_test_case(
        &self,
        organization: &str,
        project: &str,
        tc: &crate::model::TestCase,
        module_ref: Option<&str>,
        area_path: &str,
        iteration_path: &str,
        preconditions_ref: Option<&str>,
    ) -> Result<i32, AdoError> {
        let mut patch = vec![
            serde_json::json!({"op": "add", "path": "/fields/System.Title", "value": tc.title}),
            serde_json::json!({"op": "add", "path": "/fields/Microsoft.VSTS.TCM.Steps",
                "value": crate::steps_xml::build_steps_xml(&tc.steps)}),
            serde_json::json!({"op": "add", "path": "/fields/Microsoft.VSTS.TCM.AutomationStatus",
                "value": tc.automation_status}),
        ];
        if !area_path.is_empty() {
            patch.push(serde_json::json!({"op": "add", "path": "/fields/System.AreaPath", "value": area_path}));
        }
        if !iteration_path.is_empty() {
            patch.push(serde_json::json!({"op": "add", "path": "/fields/System.IterationPath", "value": iteration_path}));
        }
        if !tc.tags.is_empty() {
            patch.push(serde_json::json!({"op": "add", "path": "/fields/System.Tags", "value": tc.tags}));
        }
        if let Some(m) = module_ref {
            if !tc.module_value.is_empty() {
                patch.push(serde_json::json!({"op": "add", "path": format!("/fields/{m}"), "value": tc.module_value}));
            }
        }
        if let Some(p) = preconditions_ref {
            if !tc.preconditions.is_empty() {
                patch.push(serde_json::json!({"op": "add", "path": format!("/fields/{p}"),
                    "value": format!("<div>{}</div>", tc.preconditions)}));
            }
        }
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/$Test%20Case?api-version=7.1",
            self.base_url, organization, project
        );
        let data = self
            .send_json_patch(reqwest::Method::POST, url, &serde_json::Value::Array(patch))
            .await?;
        Ok(data["id"].as_i64().unwrap_or_default() as i32)
    }

    /// PATCH System.State and return the state ADO actually persisted.
    /// Server-side rules (required dates, disallowed transitions) can reject
    /// or rewrite the change - the response body is the truth, so callers
    /// must never assume the requested state was applied. On a 400 rule
    /// rejection, ADO's human-readable message replaces the raw JSON body.
    pub async fn set_work_item_state(
        &self,
        organization: &str,
        project: &str,
        wi_id: i32,
        state: &str,
    ) -> Result<String, AdoError> {
        let patch = serde_json::json!([
            {"op": "add", "path": "/fields/System.State", "value": state}
        ]);
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url, organization, project, wi_id
        );
        let data = self
            .send_json_patch(reqwest::Method::PATCH, url, &patch)
            .await
            .map_err(|e| match e {
                AdoError::Http { status: 400, body } => {
                    let msg = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| v["message"].as_str().map(String::from))
                        .unwrap_or(body);
                    AdoError::Http { status: 400, body: msg }
                }
                other => other,
            })?;
        Ok(data["fields"]["System.State"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// PATCH a work item's fields ({reference_name: value}); the 'add' op
    /// creates-or-replaces. Ported from v1 update_work_item_fields.
    pub async fn update_work_item_fields(
        &self,
        organization: &str,
        project: &str,
        wi_id: i32,
        fields: &[(String, String)],
    ) -> Result<(), AdoError> {
        let patch: Vec<serde_json::Value> = fields
            .iter()
            .map(|(r, v)| serde_json::json!({"op": "add", "path": format!("/fields/{r}"), "value": v}))
            .collect();
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url, organization, project, wi_id
        );
        self.send_json_patch(reqwest::Method::PATCH, url, &serde_json::Value::Array(patch))
            .await?;
        Ok(())
    }

    /// SAFETY RULE (ported from v1 update_test_case_from_model): always
    /// overwrites Steps and AutomationStatus, but overwrites Tags / module /
    /// Preconditions only when the imported case provides a value - a blank
    /// spreadsheet column must never wipe existing data.
    pub async fn update_test_case_from_model(
        &self,
        organization: &str,
        project: &str,
        tc_id: i32,
        tc: &crate::model::TestCase,
        module_ref: Option<&str>,
        preconditions_ref: Option<&str>,
    ) -> Result<(), AdoError> {
        let mut fields = vec![
            (
                "Microsoft.VSTS.TCM.Steps".to_string(),
                crate::steps_xml::build_steps_xml(&tc.steps),
            ),
            (
                "Microsoft.VSTS.TCM.AutomationStatus".to_string(),
                tc.automation_status.clone(),
            ),
        ];
        if !tc.tags.is_empty() {
            fields.push(("System.Tags".to_string(), tc.tags.clone()));
        }
        if let Some(m) = module_ref {
            if !tc.module_value.is_empty() {
                fields.push((m.to_string(), tc.module_value.clone()));
            }
        }
        if let Some(p) = preconditions_ref {
            if !tc.preconditions.is_empty() {
                fields.push((p.to_string(), format!("<div>{}</div>", tc.preconditions)));
            }
        }
        self.update_work_item_fields(organization, project, tc_id, &fields)
            .await
    }

    /// PATCH the Test Case to add a TestedBy-Reverse relation to the PBI -
    /// 'Tests' on the Test Case side, 'Tested By' on the PBI side. The PBI
    /// itself is never modified directly. Ported from v1 link_to_pbi.
    pub async fn link_to_pbi(
        &self,
        organization: &str,
        project: &str,
        test_case_id: i32,
        pbi_id: i32,
    ) -> Result<(), AdoError> {
        let pbi_url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}",
            self.base_url, organization, project, pbi_id
        );
        let patch = serde_json::json!([{
            "op": "add",
            "path": "/relations/-",
            "value": {
                "rel": "Microsoft.VSTS.Common.TestedBy-Reverse",
                "url": pbi_url,
                "attributes": {"comment": "Linked by DevOps Test Case Manager"},
            },
        }]);
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url, organization, project, test_case_id
        );
        self.send_json_patch(reqwest::Method::PATCH, url, &patch).await?;
        Ok(())
    }

    /// All writable fields on the Test Case type, ported from v1
    /// get_test_case_fields: readOnly dropped, System.* dropped except
    /// Title/Tags/Description, sorted by display name. Read only.
    pub async fn get_test_case_fields(
        &self,
        organization: &str,
        project: &str,
    ) -> Result<Vec<FieldRef>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes/Test%20Case/fields?api-version=7.1",
            self.base_url, organization, project
        );
        let data = self.get_json(url).await?;
        let mut fields: Vec<FieldRef> = data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|f| {
                let reference_name = f["referenceName"].as_str()?.to_string();
                if f["readOnly"].as_bool().unwrap_or(false) {
                    return None;
                }
                if reference_name.starts_with("System.")
                    && !matches!(
                        reference_name.as_str(),
                        "System.Title" | "System.Tags" | "System.Description"
                    )
                {
                    return None;
                }
                Some(FieldRef {
                    name: f["name"].as_str().unwrap_or(&reference_name).to_string(),
                    reference_name,
                })
            })
            .collect();
        fields.sort_by_key(|f| f.name.to_lowercase());
        Ok(fields)
    }

    /// The v1 Edit-tab field set (same batch endpoint as the summaries) plus
    /// the optional module/preconditions refs, with steps parsed and
    /// preconditions flattened for editing. Read only.
    pub async fn get_pbi_test_cases_full(
        &self,
        organization: &str,
        pbi_id: i32,
        module_ref: Option<&str>,
        preconditions_ref: Option<&str>,
    ) -> Result<Vec<TestCaseFull>, AdoError> {
        let url = format!(
            "{}/{}/_apis/wit/workitems/{}?$expand=relations&api-version=7.1",
            self.base_url, organization, pbi_id
        );
        let data = self.get_json(url).await?;
        let tc_ids: Vec<i64> = data["relations"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|r| {
                r["rel"]
                    .as_str()
                    .map(|s| s.to_lowercase().contains("testedby"))
                    .unwrap_or(false)
            })
            .filter_map(|r| r["url"].as_str()?.rsplit('/').next()?.parse().ok())
            .collect();
        if tc_ids.is_empty() {
            return Ok(vec![]);
        }

        self.get_test_cases_by_ids(organization, &tc_ids_i32(&tc_ids), module_ref, preconditions_ref)
            .await
    }

    /// Full test cases for arbitrary work-item ids (chunked at 200) - the
    /// suite browser's Edit-cases handoff and browser views use this
    /// directly; get_pbi_test_cases_full delegates here. Read only.
    pub async fn get_test_cases_by_ids(
        &self,
        organization: &str,
        ids: &[i32],
        module_ref: Option<&str>,
        preconditions_ref: Option<&str>,
    ) -> Result<Vec<TestCaseFull>, AdoError> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let mut field_list = vec![
            "System.Id",
            "System.Title",
            "System.Tags",
            "Microsoft.VSTS.TCM.AutomationStatus",
            "Microsoft.VSTS.TCM.Steps",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
        if let Some(m) = module_ref {
            field_list.push(m.to_string());
        }
        if let Some(p) = preconditions_ref {
            field_list.push(p.to_string());
        }

        let mut cases = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/_apis/wit/workitems?ids={}&fields={}&api-version=7.1",
                self.base_url,
                organization,
                ids_csv,
                field_list.join(",")
            );
            let fetched = self.get_json(url).await?;
            for w in fetched["value"].as_array().cloned().unwrap_or_default() {
                let f = &w["fields"];
                let str_of = |key: &str| f[key].as_str().unwrap_or_default().to_string();
                cases.push(TestCaseFull {
                    id: w["id"].as_i64().unwrap_or_default() as i32,
                    title: str_of("System.Title"),
                    tags: str_of("System.Tags"),
                    automation_status: {
                        let s = str_of("Microsoft.VSTS.TCM.AutomationStatus");
                        if s.is_empty() { "Not Automated".to_string() } else { s }
                    },
                    steps: crate::steps_xml::parse_steps_xml(&str_of("Microsoft.VSTS.TCM.Steps")),
                    step_ids: crate::steps_xml::parse_step_ids(&str_of("Microsoft.VSTS.TCM.Steps")),
                    module_value: module_ref.map(str_of).unwrap_or_default(),
                    preconditions: preconditions_ref
                        .map(|p| crate::steps_xml::html_to_text(&str_of(p)))
                        .unwrap_or_default(),
                });
            }
        }
        Ok(cases)
    }

    /// Which type bugs are filed as on this project's process, ported from
    /// v1 detect_bug_type: prefer Bug (ReproSteps), fall back to Issue
    /// (Description). Enumeration failure assumes Bug. Read only.
    pub async fn detect_bug_type(
        &self,
        organization: &str,
        project: &str,
    ) -> Result<BugTypeInfo, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes?api-version=7.1",
            self.base_url, organization, project
        );
        let names: Vec<String> = match self.get_json(url).await {
            Ok(data) => data["value"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|wt| wt["name"].as_str().map(String::from))
                .collect(),
            Err(_) => vec![], // enumeration failure -> assume Bug (v1 behaviour)
        };
        let info = if names.iter().any(|n| n == "Bug") || names.is_empty() {
            BugTypeInfo {
                wi_type: "Bug".into(),
                repro_field: "Microsoft.VSTS.TCM.ReproSteps".into(),
                has_severity: true,
            }
        } else if names.iter().any(|n| n == "Issue") {
            BugTypeInfo {
                wi_type: "Issue".into(),
                repro_field: "System.Description".into(),
                has_severity: false,
            }
        } else {
            BugTypeInfo {
                wi_type: "Bug".into(),
                repro_field: "Microsoft.VSTS.TCM.ReproSteps".into(),
                has_severity: true,
            }
        };
        Ok(info)
    }

    /// POST a new work item of `wi_type` with fields and optional Related
    /// links. Returns (id, web_url). Only POST - never DELETEs.
    pub async fn create_work_item(
        &self,
        organization: &str,
        project: &str,
        wi_type: &str,
        fields: &[(String, String)],
        related_ids: &[i32],
        parent_id: Option<i32>,
    ) -> Result<(i32, String), AdoError> {
        let mut patch: Vec<serde_json::Value> = fields
            .iter()
            .map(|(r, v)| serde_json::json!({"op": "add", "path": format!("/fields/{r}"), "value": v}))
            .collect();
        for rel_id in related_ids {
            patch.push(serde_json::json!({
                "op": "add",
                "path": "/relations/-",
                "value": {
                    "rel": "System.LinkTypes.Related",
                    "url": format!("{}/{}/{}/_apis/wit/workitems/{}", self.base_url, organization, project, rel_id),
                },
            }));
        }
        // Hierarchy-Reverse = "my parent is" - lands the new item under its
        // PBI/Feature so boards and backlogs nest it correctly.
        if let Some(pid) = parent_id {
            patch.push(serde_json::json!({
                "op": "add",
                "path": "/relations/-",
                "value": {
                    "rel": "System.LinkTypes.Hierarchy-Reverse",
                    "url": format!("{}/{}/{}/_apis/wit/workitems/{}", self.base_url, organization, project, pid),
                },
            }));
        }
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/${}?api-version=7.1",
            self.base_url,
            organization,
            project,
            urlencoding::encode(wi_type)
        );
        let data = self
            .send_json_patch(reqwest::Method::POST, url, &serde_json::Value::Array(patch))
            .await?;
        let web = data["_links"]["html"]["href"].as_str().unwrap_or_default().to_string();
        Ok((data["id"].as_i64().unwrap_or_default() as i32, web))
    }

    /// Upload raw bytes as a work-item attachment; returns the attachment
    /// URL for an AttachedFile relation. POST only.
    pub async fn upload_wi_attachment(
        &self,
        organization: &str,
        project: &str,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<String, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/attachments?fileName={}&api-version=7.1",
            self.base_url,
            organization,
            project,
            urlencoding::encode(file_name)
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .header("Content-Type", "application/octet-stream")
            .body(bytes)
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        let data = Self::handle_json(resp).await?;
        Ok(data["url"].as_str().unwrap_or_default().to_string())
    }

    /// PATCH an AttachedFile relation onto a work item (bug screenshots).
    pub async fn add_wi_attachment_relation(
        &self,
        organization: &str,
        project: &str,
        wi_id: i32,
        attachment_url: &str,
    ) -> Result<(), AdoError> {
        let patch = serde_json::json!([{
            "op": "add",
            "path": "/relations/-",
            "value": {"rel": "AttachedFile", "url": attachment_url},
        }]);
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url, organization, project, wi_id
        );
        self.send_json_patch(reqwest::Method::PATCH, url, &patch).await?;
        Ok(())
    }

    /// All work-item tag names in the project, sorted (v1 get_tags, for the
    /// manual-entry autocomplete). Read only.
    pub async fn get_tags(&self, organization: &str, project: &str) -> Result<Vec<String>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/tags?api-version=7.1",
            self.base_url, organization, project
        );
        let data = self.get_json(url).await?;
        let mut tags: Vec<String> = data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|t| t["name"].as_str().map(String::from))
            .collect();
        tags.sort_by_key(|t| t.to_lowercase());
        Ok(tags)
    }

    /// The project's Area or Iteration tree flattened to path strings,
    /// ported from v1 get_classification_paths: built from node NAMES, not
    /// the node's `path` field (that carries an extra \Area or \Iteration
    /// segment the stored field values don't have). Failure -> empty so
    /// pickers fall back gracefully. Read only.
    pub async fn get_classification_paths(
        &self,
        organization: &str,
        project: &str,
        structure: &str,
    ) -> Result<Vec<String>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/classificationnodes/{}?$depth=14&api-version=7.1",
            self.base_url, organization, project, structure
        );
        let root = match self.get_json(url).await {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };
        fn walk(node: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
            let name = node["name"].as_str().unwrap_or_default();
            let path = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}\\{name}")
            };
            out.push(path.clone());
            for child in node["children"].as_array().cloned().unwrap_or_default() {
                walk(&child, &path, out);
            }
        }
        let mut paths = vec![];
        walk(&root, "", &mut paths);
        Ok(paths)
    }

    /// Iteration paths WITH their sprint dates (classificationnodes carries
    /// startDate/finishDate in `attributes`), so pickers can render like
    /// Azure DevOps's own iteration dropdown. Non-sprint nodes (the project
    /// root, grouping folders) have no dates. Failure -> empty. Read only.
    pub async fn get_iterations_dated(
        &self,
        organization: &str,
        project: &str,
    ) -> Result<Vec<crate::work_board::IterationRef>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/classificationnodes/iterations?$depth=14&api-version=7.1",
            self.base_url, organization, project
        );
        let root = match self.get_json(url).await {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };
        fn walk(node: &serde_json::Value, prefix: &str, out: &mut Vec<crate::work_board::IterationRef>) {
            let name = node["name"].as_str().unwrap_or_default();
            let path = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}\\{name}")
            };
            out.push(crate::work_board::IterationRef {
                path: path.clone(),
                start_date: node["attributes"]["startDate"].as_str().map(str::to_string),
                finish_date: node["attributes"]["finishDate"].as_str().map(str::to_string),
            });
            for child in node["children"].as_array().cloned().unwrap_or_default() {
                walk(&child, &path, out);
            }
        }
        let mut out = vec![];
        walk(&root, "", &mut out);
        Ok(out)
    }

    /// A work item's area + iteration path (used to home the PBI's test
    /// plan). Read only.
    pub async fn get_work_item_paths(
        &self,
        organization: &str,
        project: &str,
        wi_id: i32,
    ) -> Result<(String, String), AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1&$select=System.AreaPath,System.IterationPath",
            self.base_url, organization, project, wi_id
        );
        let data = self.get_json(url).await?;
        Ok((
            data["fields"]["System.AreaPath"].as_str().unwrap_or_default().to_string(),
            data["fields"]["System.IterationPath"].as_str().unwrap_or_default().to_string(),
        ))
    }

    pub async fn get_projects(&self, organization: &str) -> Result<Vec<Project>, AdoError> {
        let url = format!(
            "{}/{}/_apis/projects?api-version=7.1&$top=500",
            self.base_url, organization
        );
        let body = self.get_json(url).await?;
        let projects = body["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
        Ok(projects)
    }
}
