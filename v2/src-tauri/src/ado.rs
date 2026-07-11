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
    http: reqwest::Client,
    token: String,
    base_url: String, // "https://dev.azure.com" in prod, mock server in tests
    vssps_base_url: String, // "https://app.vssps.visualstudio.com" in prod
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

    async fn get_json(&self, url: String) -> Result<serde_json::Value, AdoError> {
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
    async fn post_json_query(
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
