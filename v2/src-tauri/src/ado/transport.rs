//! HTTP transport for `AdoClient`: the only place requests are built and
//! status codes become `AdoError`s. Verbs stop at GET / POST / PATCH —
//! no DELETE, ever (tests/ado.rs scans this file).

use super::{AdoClient, AdoError};

impl AdoClient {
    /// Azure DevOps caps the workitems batch-GET (?ids=) endpoint at 200 ids.
    pub(crate) const WORKITEM_BATCH_SIZE: usize = 200;

    pub(crate) async fn get_json(&self, url: String) -> Result<serde_json::Value, AdoError> {
        super::throttle::pace().await;
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        Self::handle_json(resp).await
    }

    /// GET returning the raw body as text - build logs are plain text, not
    /// JSON. Same status handling as the JSON path.
    pub(crate) async fn get_text(&self, url: String) -> Result<String, AdoError> {
        super::throttle::pace().await;
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "text/plain")
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        match resp.status().as_u16() {
            200..=299 => resp.text().await.map_err(|e| AdoError::Network(e.to_string())),
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

    /// GET returning (body, x-ms-continuationtoken) for ADO's paginated
    /// testplan endpoints.
    pub(crate) async fn get_json_with_continuation(
        &self,
        url: String,
    ) -> Result<(serde_json::Value, Option<String>), AdoError> {
        super::throttle::pace().await;
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
        super::throttle::pace().await;
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

    /// POST used for WIQL queries only — query-only, creates and modifies
    /// nothing. Still no DELETE anywhere in this client.
    pub(crate) async fn post_json_query(
        &self,
        url: String,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        super::throttle::pace().await;
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
        super::throttle::pace().await;
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

    /// PATCH-shaped request helpers. The only verbs this client will ever
    /// grow are GET, POST and PATCH - no DELETE, ever.
    pub(crate) async fn send_json_patch(
        &self,
        method: reqwest::Method,
        url: String,
        patch: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        super::throttle::pace().await;
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

    pub(crate) async fn handle_json(resp: reqwest::Response) -> Result<serde_json::Value, AdoError> {
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
}
