//! The suggested run order: a small file attached to a PBI so every
//! tester who opens it sees the same default order (design
//! docs/superpowers/specs/2026-09-23-run-order-design.md §4.2, §7).
//!
//! Shape mirrors `ado_share.rs`: the file travels through Azure DevOps as
//! an `AttachedFile` relation, so there is no server of our own and access
//! control is the PBI's own permissions. Unlike a share link this file is
//! never revoked - "replacing" it is an upload of the new file plus ONE
//! PATCH that adds the new relation and removes the old one(s). The old
//! blob stays in Azure DevOps unreferenced; this app never deletes
//! anything there (scanned by tests/ado.rs - no DELETE anywhere).

use crate::ado::{AdoClient, AdoError};
use serde::{Deserialize, Serialize};

pub const RUN_ORDER_FILE_NAME: &str = "tcm-run-order.json";
pub const RUN_ORDER_COMMENT: &str = "Test Case Manager run order";
pub const RUN_ORDER_FORMAT: &str = "tcm-run-order";
pub const RUN_ORDER_VERSION: u32 = 1;

/// One case's place in the suggested order. `group` is the case's area
/// where known (the grouping the tree view uses) - optional because not
/// every writer of this file knows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RunOrderCase {
    pub id: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RunOrderFile {
    pub format: String,
    pub version: u32,
    /// The signed-in account, as the app shows it in the context bar.
    pub saved_by: String,
    /// RFC 3339, UTC, e.g. "2026-09-23T10:15:00Z".
    pub saved_at: String,
    pub cases: Vec<RunOrderCase>,
}

/// What reading a PBI's run order found. Never an error on its own - a
/// missing or damaged file falls back to spec order (design §6); only a
/// transport/auth failure on the PBI read itself is an `Err`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RunOrderRead {
    None,
    Found { file: RunOrderFile },
    Unreadable { reason: String },
}

/// Parse one downloaded file. Wrong `format`, a `version` other than the
/// one this app writes, or invalid JSON all become a one-sentence reason -
/// never a URL, since this is shown to a person, not logged (that is
/// `applog`'s job, done by the caller).
pub fn parse_run_order(text: &str) -> Result<RunOrderFile, String> {
    let file: RunOrderFile =
        serde_json::from_str(text).map_err(|_| "the run-order file is damaged and could not be read".to_string())?;
    if file.format != RUN_ORDER_FORMAT {
        return Err("this file is not a Test Case Manager run-order file".to_string());
    }
    if file.version != RUN_ORDER_VERSION {
        return Err(format!(
            "this run-order file is a newer version ({}) than this app understands",
            file.version
        ));
    }
    Ok(file)
}

/// The newest of several by `saved_at`. RFC 3339 strings in UTC (the only
/// shape this app ever writes) compare correctly as plain text.
pub fn newest(files: Vec<RunOrderFile>) -> Option<RunOrderFile> {
    files.into_iter().max_by(|a, b| a.saved_at.cmp(&b.saved_at))
}

/// UTC "YYYY-MM-DDTHH:MM:SSZ" (RFC 3339) for `saved_at`. Reuses applog's
/// day-number algorithm rather than pulling in a date crate - the same
/// reasoning `applog::stamp` documents, just formatted for Azure DevOps
/// instead of the log.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, mo, d) = crate::applog::civil(days);
    let (h, mi, s) = ((rem / 3600) as u32, ((rem % 3600) / 60) as u32, (rem % 60) as u32);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Is this relation "our run-order file"? Exact name match only - a
/// differently-named `AttachedFile` (a screenshot, a shared draft) is left
/// alone, both when reading and when deciding what to remove on save.
fn is_run_order_relation(r: &serde_json::Value) -> bool {
    r["rel"].as_str() == Some("AttachedFile") && r["attributes"]["name"].as_str() == Some(RUN_ORDER_FILE_NAME)
}

impl AdoClient {
    /// GET the PBI with relations; every `AttachedFile` relation named
    /// `tcm-run-order.json` is downloaded and parsed. None found ->
    /// `None`. Some parse, some do not -> the newest that parsed. None
    /// parse -> `Unreadable{reason}` (the first reason hit, in relation
    /// order). A download failure (network, permission) counts the same
    /// as a parse failure here - either way the file could not be read,
    /// and design §6 says that falls back to spec order rather than
    /// failing the whole read. Only the PBI read itself, which every
    /// other outcome depends on, propagates as `Err`.
    pub async fn read_run_order(&self, org: &str, project: &str, pbi_id: i32) -> Result<RunOrderRead, AdoError> {
        let wi_url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?$expand=relations&api-version=7.1",
            self.base_url, org, project, pbi_id
        );
        let wi = self.get_json(wi_url).await?;
        let urls: Vec<String> = wi["relations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| is_run_order_relation(r))
            .filter_map(|r| r["url"].as_str().map(String::from))
            .collect();
        if urls.is_empty() {
            return Ok(RunOrderRead::None);
        }

        let mut parsed = Vec::with_capacity(urls.len());
        let mut first_reason: Option<String> = None;
        for url in urls {
            let outcome = match self.get_text(url).await {
                Ok(text) => parse_run_order(&text),
                Err(e) => Err(e.user_text()),
            };
            match outcome {
                Ok(file) => parsed.push(file),
                Err(reason) => {
                    if first_reason.is_none() {
                        first_reason = Some(reason);
                    }
                }
            }
        }

        match newest(parsed) {
            Some(file) => Ok(RunOrderRead::Found { file }),
            None => Ok(RunOrderRead::Unreadable {
                reason: first_reason.unwrap_or_else(|| "the run-order file could not be read".to_string()),
            }),
        }
    }

    /// Upload `file` as `tcm-run-order.json`, then ONE PATCH on the PBI
    /// that removes every existing run-order relation (highest index
    /// first, so removing one never shifts the index of the next) and
    /// adds the new one. The `test` op on `/rev` aborts the whole patch if
    /// the PBI changed since the read just above, so a concurrent save
    /// can never remove the wrong relation - same guard `revoke_share`
    /// uses for the same reason.
    pub async fn save_run_order(&self, org: &str, project: &str, pbi_id: i32, file: &RunOrderFile) -> Result<(), AdoError> {
        let body = serde_json::to_string(file)
            .map_err(|e| AdoError::Network(format!("could not encode the run-order file: {e}")))?;
        let attachment_url = self.upload_wi_attachment(org, project, RUN_ORDER_FILE_NAME, body.into_bytes()).await?;

        let wi_url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?$expand=relations&api-version=7.1",
            self.base_url, org, project, pbi_id
        );
        let wi = self.get_json(wi_url).await?;
        let relations = wi["relations"].as_array().cloned().unwrap_or_default();
        let mut remove_indices: Vec<usize> = relations
            .iter()
            .enumerate()
            .filter(|(_, r)| is_run_order_relation(r))
            .map(|(i, _)| i)
            .collect();
        remove_indices.sort_unstable_by(|a, b| b.cmp(a));

        let mut patch: Vec<serde_json::Value> =
            vec![serde_json::json!({ "op": "test", "path": "/rev", "value": wi["rev"] })];
        for idx in remove_indices {
            patch.push(serde_json::json!({ "op": "remove", "path": format!("/relations/{idx}") }));
        }
        patch.push(serde_json::json!({
            "op": "add",
            "path": "/relations/-",
            "value": {
                "rel": "AttachedFile",
                "url": attachment_url,
                "attributes": { "name": RUN_ORDER_FILE_NAME, "comment": RUN_ORDER_COMMENT }
            }
        }));

        let patch_url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url, org, project, pbi_id
        );
        self.send_json_patch(reqwest::Method::PATCH, patch_url, &serde_json::Value::Array(patch))
            .await?;
        Ok(())
    }
}
