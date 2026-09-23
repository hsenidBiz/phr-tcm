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

/// A new file ready to save: this app's format and version, stamped with
/// who saved it and now. The one place both writers (the save command and
/// the upload) fill it, so the two can never disagree on its shape.
pub fn new_file(saved_by: String, cases: Vec<RunOrderCase>) -> RunOrderFile {
    RunOrderFile {
        format: RUN_ORDER_FORMAT.to_string(),
        version: RUN_ORDER_VERSION,
        saved_by,
        saved_at: now_rfc3339(),
        cases,
    }
}

// ── At upload (design §4.1, §4.2) ───────────────────────────────────────

/// How many times, and how far apart, the upload reads the suite while
/// waiting for Azure DevOps to add the new cases to it. About three
/// seconds in all: enough for the usual lag, short enough that an upload
/// never looks stuck on it.
pub const SETTLE_TRIES: u32 = 5;
pub const SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(800);

pub const NOTE_SUITE_BEHIND: &str = "Azure DevOps had not added every new test case to the suite yet, so the order was set for the ones it had. Set it in Suite Management if it looks wrong.";

/// The queue indexes and work item ids that landed (created or updated) in
/// this upload, in queue order. Built by submit_queue from its results.
#[derive(Debug, Clone, PartialEq)]
pub struct Landed {
    pub index: usize,
    pub id: i32,
    pub created: bool,
}

/// Spec order for the landed cases: by spec_order when EVERY landed case has
/// one, else queue (file) order. All or nothing because a partial order has
/// no right place for the cases without one - file order at least is the
/// order someone chose.
pub fn spec_order_ids(landed: &[Landed], queue: &[crate::model::TestCase]) -> Vec<i32> {
    let mut rows: Vec<&Landed> = landed.iter().collect();
    rows.sort_by_key(|l| l.index);
    let keys: Option<Vec<u32>> = rows.iter().map(|l| queue.get(l.index).and_then(|tc| tc.spec_order)).collect();
    if let Some(keys) = keys {
        let mut keyed: Vec<(u32, &Landed)> = keys.into_iter().zip(rows).collect();
        // Stable: two cases sharing a spec_order keep their file order.
        keyed.sort_by_key(|(k, _)| *k);
        return keyed.into_iter().map(|(_, l)| l.id).collect();
    }
    rows.into_iter().map(|l| l.id).collect()
}

/// The suggested order's first part: the landed cases by tester_order, each
/// with group = its area when non-empty. None unless EVERY landed case has a
/// tester_order - a file without one means "no suggestion", and Run Tests
/// then uses spec order (design §4.2).
pub fn tester_order_cases(landed: &[Landed], queue: &[crate::model::TestCase]) -> Option<Vec<RunOrderCase>> {
    let mut rows: Vec<(u32, &Landed, &crate::model::TestCase)> = landed
        .iter()
        .map(|l| {
            let tc = queue.get(l.index)?;
            Some((tc.tester_order?, l, tc))
        })
        .collect::<Option<_>>()?;
    rows.sort_by_key(|(order, l, _)| (*order, l.index));
    Some(
        rows.into_iter()
            .map(|(_, l, tc)| RunOrderCase {
                id: l.id,
                group: (!tc.area.trim().is_empty()).then(|| tc.area.clone()),
            })
            .collect(),
    )
}

/// `first`, then every id of `suite_order` not already in it, in suite order.
pub fn with_rest(first: Vec<RunOrderCase>, suite_order: &[i32]) -> Vec<RunOrderCase> {
    let mut out = first;
    for id in suite_order {
        if !out.iter().any(|c| c.id == *id) {
            out.push(RunOrderCase { id: *id, group: None });
        }
    }
    out
}

/// After an upload into a known suite: set the suite to spec order and,
/// when the file carried a tester order, save the suggested run order.
/// Returns one sentence per thing that could not be done, for the screen.
/// Nothing here fails the upload - the cases are in Azure DevOps already,
/// and either order can be set later from Suite Management (design §6).
/// An upload that created nothing sends nothing, so an arrangement made in
/// Suite Management survives routine edits (§4.1).
#[allow(clippy::too_many_arguments)]
pub async fn order_after_upload(
    client: &AdoClient,
    org: &str,
    project: &str,
    pbi_id: i32,
    suite_id: i32,
    landed: &[Landed],
    queue: &[crate::model::TestCase],
    saved_by: &str,
    settle_delay: std::time::Duration,
) -> Vec<String> {
    let mut notes = Vec::new();
    let created: Vec<i32> = landed.iter().filter(|l| l.created).map(|l| l.id).collect();
    if created.is_empty() {
        return notes;
    }
    let ids = spec_order_ids(landed, queue);
    crate::applog::info(format!(
        "ordering suite {suite_id} for #{pbi_id}: {} uploaded case(s), {} created, spec order {ids:?}",
        ids.len(),
        created.len()
    ));

    // The requirement suite fills itself from the Tested-By links, on
    // Azure DevOps' own schedule; ordering before the new cases arrive
    // would leave them wherever they land.
    let settled = match client
        .settle_suite(org, project, suite_id, &created, SETTLE_TRIES, settle_delay)
        .await
    {
        Ok(ids) => ids,
        Err(e) => {
            crate::applog::warn(format!("could not read suite {suite_id} to order it: {e}"));
            notes.push(format!("The spec order could not be set in Azure DevOps: {}", e.user_text()));
            return notes;
        }
    };
    let behind: Vec<i32> = created.iter().copied().filter(|id| !settled.contains(id)).collect();

    let suite_order = match client.reorder_suite_cases(org, project, suite_id, &ids).await {
        Ok(order) => order,
        Err(e) => {
            crate::applog::warn(format!("could not set the spec order on suite {suite_id}: {e}"));
            notes.push(format!("The spec order could not be set in Azure DevOps: {}", e.user_text()));
            return notes;
        }
    };
    crate::applog::info(format!(
        "suite {suite_id} set to spec order: {} case(s) in the suite",
        suite_order.len()
    ));
    if !behind.is_empty() {
        crate::applog::warn(format!(
            "suite {suite_id} still lacked {} new case(s) after {SETTLE_TRIES} reads: {behind:?}",
            behind.len()
        ));
        notes.push(NOTE_SUITE_BEHIND.to_string());
    }

    let Some(first) = tester_order_cases(landed, queue) else {
        crate::applog::info(format!("no tester order in the upload for #{pbi_id} - no suggested run order saved"));
        return notes;
    };
    let file = new_file(saved_by.to_string(), with_rest(first, &suite_order));
    match client.save_run_order(org, project, pbi_id, &file).await {
        Ok(()) => crate::applog::info(format!(
            "saved the suggested run order for #{pbi_id}: {} case(s)",
            file.cases.len()
        )),
        Err(e) => {
            crate::applog::warn(format!("could not save the suggested run order for #{pbi_id}: {e}"));
            notes.push(format!("The suggested run order could not be saved: {}", e.user_text()));
        }
    }
    notes
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

    /// Read the suite's case entries (get_suite_entries, sequence order) until
    /// every id in `expect` is present or `tries` reads have been made, sleeping
    /// `delay` between reads. Returns the last read's case ids in order.
    pub async fn settle_suite(
        &self,
        org: &str,
        project: &str,
        suite_id: i32,
        expect: &[i32],
        tries: u32,
        delay: std::time::Duration,
    ) -> Result<Vec<i32>, AdoError> {
        let mut read = 0;
        loop {
            let ids: Vec<i32> = self
                .get_suite_entries(org, project, suite_id)
                .await?
                .into_iter()
                .filter(|e| e.entry_type == "testCase")
                .map(|e| e.id)
                .collect();
            read += 1;
            if read >= tries.max(1) || expect.iter().all(|id| ids.contains(id)) {
                return Ok(ids);
            }
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
        }
    }
}
