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
