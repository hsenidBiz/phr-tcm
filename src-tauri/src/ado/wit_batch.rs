//! The Work Item Tracking `$batch` endpoint: many work item requests in
//! one HTTP call, executed on the server one after another.
//!
//! This is what makes a 200-case upload take seconds rather than minutes.
//! Each case used to be its own round trip (two, for a create - POST then
//! a PATCH to link it to the PBI); here a chunk of cases travels as one
//! request and comes back as one response carrying a status and body per
//! item, so a failure on one case is reported on that case alone.
//!
//! Sequential on the server matters for tester order: the requests in a
//! batch are executed in the order given, so ids - and with them the
//! suite's order - follow the order the app sends.

use super::endpoints::percent_encode_segment;
use super::{AdoClient, AdoError};

/// Azure DevOps refuses a batch larger than this.
pub const MAX_PER_BATCH: usize = 200;

/// One request inside a batch. `uri` is relative to the organization
/// (`/{project}/_apis/wit/...`); the body is a JSON-patch document.
#[derive(Debug, Clone)]
pub struct BatchRequest {
    pub method: &'static str,
    pub uri: String,
    pub body: serde_json::Value,
}

/// One answer inside a batch response. `body` is the parsed JSON when the
/// server sent JSON (it arrives as a STRING inside the envelope), the raw
/// string otherwise.
#[derive(Debug, Clone)]
pub struct BatchItem {
    pub code: u16,
    pub body: serde_json::Value,
}

impl BatchItem {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.code)
    }

    /// The work item id a create or update answered with.
    pub fn id(&self) -> Option<i32> {
        self.body["id"].as_i64().map(|i| i as i32)
    }

    /// What went wrong, in ADO's words where it gave any - the `message`
    /// of an error body is where "Rule Error for field ..." lives.
    pub fn message(&self) -> String {
        if let Some(m) = self.body["message"].as_str() {
            if !m.trim().is_empty() {
                return m.to_string();
            }
        }
        if let serde_json::Value::String(s) = &self.body {
            if !s.trim().is_empty() && !s.starts_with('<') {
                return s.clone();
            }
        }
        format!("Azure DevOps returned HTTP {}", self.code)
    }
}

/// The batch URI that creates a Test Case in `project`.
pub fn create_uri(project: &str) -> String {
    format!(
        "/{}/_apis/wit/workitems/$Test%20Case?api-version=7.1",
        percent_encode_segment(project)
    )
}

/// The batch URI that updates work item `id`. Organization-level, no
/// project segment: that is the shape the batch reference documents, and
/// the project-prefixed form answered 404 for every one of 96 updates on
/// the first real run (2026-09-11) while the same ids updated fine
/// through the single-item endpoint.
pub fn update_uri(id: i32) -> String {
    format!("/_apis/wit/workitems/{id}?api-version=7.1")
}

/// The operation a create inside a batch must START with: a temporary id,
/// negative and unique within the batch, so the server can tell the
/// creates apart. Without it only the first create in a batch succeeded
/// and the other 24 came back 400 (2026-09-11). `n` is 1-based.
pub fn temp_id_op(n: usize) -> serde_json::Value {
    serde_json::json!({"op": "add", "path": "/id", "value": -(n as i64)})
}

impl AdoClient {
    /// Send up to `MAX_PER_BATCH` requests as one call. The answer has
    /// exactly one item per request, in order; a mismatch is an error
    /// rather than a guess about which item is which.
    pub async fn wit_batch(&self, org: &str, reqs: &[BatchRequest]) -> Result<Vec<BatchItem>, AdoError> {
        let url = format!(
            "{}/{}/_apis/wit/$batch?api-version=7.1",
            self.base_url,
            percent_encode_segment(org)
        );
        let body: Vec<serde_json::Value> = reqs
            .iter()
            .map(|r| {
                serde_json::json!({
                    "method": r.method,
                    "uri": r.uri,
                    "headers": {"Content-Type": "application/json-patch+json"},
                    "body": r.body,
                })
            })
            .collect();
        let data = self.post_json(url, &serde_json::Value::Array(body)).await?;
        let items = data["value"].as_array().cloned().unwrap_or_default();
        if items.len() != reqs.len() {
            return Err(AdoError::Http {
                status: 0,
                body: format!(
                    "Azure DevOps answered {} of the {} requests in a batch, so the results cannot be matched to the cases",
                    items.len(),
                    reqs.len()
                ),
            });
        }
        Ok(items
            .iter()
            .enumerate()
            .map(|(k, it)| {
                let code = it["code"].as_u64().unwrap_or(0) as u16;
                let body = match &it["body"] {
                    serde_json::Value::String(s) => {
                        serde_json::from_str(s).unwrap_or(serde_json::Value::String(s.clone()))
                    }
                    other => other.clone(),
                };
                let item = BatchItem { code, body };
                // A refused item goes to the log with what the server sent,
                // because the message shown to the user is only as good as
                // the `message` field, and "HTTP 400" on its own explained
                // nothing about 92 failures.
                if !item.ok() {
                    let raw = it["body"].as_str().map(str::to_string).unwrap_or_else(|| it["body"].to_string());
                    let raw: String = raw.chars().take(600).collect();
                    crate::applog::warn(format!(
                        "batch item {} ({} {}) -> {}: {}",
                        k + 1,
                        reqs[k].method,
                        reqs[k].uri,
                        code,
                        raw
                    ));
                }
                item
            })
            .collect())
    }
}
