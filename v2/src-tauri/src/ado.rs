//! Azure DevOps REST client.
//!
//! SAFETY INVARIANT: this client exposes GET (and later POST/PATCH) only.
//! Never add a DELETE method — enforced by tests/ado.rs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Org {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PbiHit {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TestCaseSummary {
    pub id: i32,
    pub title: String,
    pub tags: String,
    pub automation_status: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct FieldRef {
    pub name: String,
    pub reference_name: String,
}

/// A fully-loaded Test Case for the editor: steps parsed from the XML blob,
/// preconditions flattened to plain text. `id` doubles as update_id when the
/// editor saves.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TestCaseFull {
    pub id: i32,
    pub title: String,
    pub tags: String,
    pub automation_status: String,
    pub steps: Vec<crate::steps_xml::Step>,
    /// Real ADO step ids (document order, aligned with `steps`) - the runner
    /// needs them to build iterationDetails.
    pub step_ids: Vec<String>,
    pub module_value: String,
    pub preconditions: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BugTypeInfo {
    pub wi_type: String,
    pub repro_field: String,
    pub has_severity: bool,
}

#[derive(Debug, thiserror::Error, Serialize, specta::Type)]
#[serde(tag = "kind", content = "detail")]
pub enum AdoError {
    #[error("unauthorized")]
    Unauthorized,
    // u32 (not u64): specta forbids BigInt-style types crossing IPC.
    #[error("rate limited, retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u32 },
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("http {status}")]
    Http { status: u16, body: String },
    #[error("network: {0}")]
    Network(String),
}

pub struct AdoClient {
    pub(crate) http: reqwest::Client,
    pub(crate) token: String,
    pub(crate) base_url: String, // "https://dev.azure.com" in prod, mock server in tests
    pub(crate) vssps_base_url: String, // "https://app.vssps.visualstudio.com" in prod
}

impl AdoClient {
    pub fn new(access_token: String) -> Self {
        Self::with_base_urls(
            access_token,
            "https://dev.azure.com".to_string(),
            "https://app.vssps.visualstudio.com".to_string(),
        )
    }

    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        let vssps = base_url.clone();
        Self::with_base_urls(access_token, base_url, vssps)
    }

    pub fn with_base_urls(access_token: String, base_url: String, vssps_base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            token: access_token,
            base_url,
            vssps_base_url,
        }
    }

    /// GET returning (body, x-ms-continuationtoken) for ADO's paginated
    /// testplan endpoints.
    pub(crate) async fn get_json_with_continuation(
        &self,
        url: String,
    ) -> Result<(serde_json::Value, Option<String>), AdoError> {
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        let cont = resp
            .headers()
            .get("x-ms-continuationtoken")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = Self::handle_json(resp).await?;
        Ok((body, cont))
    }

    /// Plain-JSON POST (application/json) - used by testplan/test-run
    /// endpoints. Still no DELETE anywhere in this client.
    pub(crate) async fn post_json(
        &self,
        url: String,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        Self::handle_json(resp).await
    }

    /// Plain-JSON PATCH (application/json, not json-patch).
    pub(crate) async fn patch_plain_json(
        &self,
        url: String,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .http
            .patch(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        Self::handle_json(resp).await
    }

    pub(crate) async fn get_json(&self, url: String) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        match resp.status().as_u16() {
            200..=299 => resp
                .json()
                .await
                .map_err(|e| AdoError::Network(e.to_string())),
            401 => Err(AdoError::Unauthorized),
            403 => Err(AdoError::Forbidden),
            404 => Err(AdoError::NotFound),
            429 => {
                let retry = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(5);
                Err(AdoError::RateLimited { retry_after_secs: retry })
            }
            s => Err(AdoError::Http {
                status: s,
                body: resp.text().await.unwrap_or_default(),
            }),
        }
    }

    /// POST used for WIQL queries only — query-only, creates and modifies
    /// nothing. Still no DELETE anywhere in this client.
    pub(crate) async fn post_json_query(
        &self,
        url: String,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        Self::handle_json(resp).await
    }

    async fn handle_json(resp: reqwest::Response) -> Result<serde_json::Value, AdoError> {
        match resp.status().as_u16() {
            200..=299 => resp
                .json()
                .await
                .map_err(|e| AdoError::Network(e.to_string())),
            401 => Err(AdoError::Unauthorized),
            403 => Err(AdoError::Forbidden),
            404 => Err(AdoError::NotFound),
            429 => {
                let retry = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(5);
                Err(AdoError::RateLimited { retry_after_secs: retry })
            }
            s => Err(AdoError::Http {
                status: s,
                body: resp.text().await.unwrap_or_default(),
            }),
        }
    }

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

    /// Azure DevOps caps the workitems batch-GET (?ids=) endpoint at 200 ids.
    const WORKITEM_BATCH_SIZE: usize = 200;

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

    /// PATCH-shaped request helpers. The only verbs this client will ever
    /// grow are GET, POST and PATCH - no DELETE, ever.
    async fn send_json_patch(
        &self,
        method: reqwest::Method,
        url: String,
        patch: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .http
            .request(method, &url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json-patch+json")
            .json(patch)
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        Self::handle_json(resp).await
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

        let mut cases = Vec::with_capacity(tc_ids.len());
        for chunk in tc_ids.chunks(Self::WORKITEM_BATCH_SIZE) {
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
