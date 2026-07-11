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
}

impl AdoClient {
    pub fn new(access_token: String) -> Self {
        Self::with_base_url(access_token, "https://dev.azure.com".to_string())
    }

    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            token: access_token,
            base_url,
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
