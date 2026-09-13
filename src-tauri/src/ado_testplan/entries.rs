//! Suite entries (the order of test cases and child suites inside a
//! suite), static child suites ("folders"), and adding cases to a suite.
//! The Manage Test Cases screen's calls. GET, POST and PATCH only.

use super::SuiteRef;
use crate::ado::{AdoClient, AdoError};
use serde::Serialize;

/// One row of a suite's ordering: a test case or a child suite and the
/// position Azure DevOps shows it at.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SuiteEntry {
    pub id: i32,
    pub sequence_number: i32,
    /// "testCase" or "suite", exactly as Azure DevOps names them.
    pub entry_type: String,
}

/// The order to send: every id in `wanted` that really is a case in the
/// suite, in that order (a repeat counts once, an id the suite does not
/// hold is dropped), then the suite's remaining cases in the order they
/// already had. Child suites are never part of it.
pub fn ordered_case_ids(current: &[SuiteEntry], wanted: &[i32]) -> Vec<i32> {
    let present: Vec<i32> = current
        .iter()
        .filter(|e| e.entry_type == "testCase")
        .map(|e| e.id)
        .collect();
    let mut out: Vec<i32> = Vec::with_capacity(present.len());
    for id in wanted {
        if present.contains(id) && !out.contains(id) {
            out.push(*id);
        }
    }
    for id in present {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

fn entries_of(data: &serde_json::Value) -> Vec<SuiteEntry> {
    let mut entries: Vec<SuiteEntry> = data["value"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|e| SuiteEntry {
            id: e["id"].as_i64().unwrap_or_default() as i32,
            sequence_number: e["sequenceNumber"].as_i64().unwrap_or_default() as i32,
            entry_type: e["suiteEntryType"].as_str().unwrap_or_default().to_string(),
        })
        .collect();
    entries.sort_by_key(|e| e.sequence_number);
    entries
}

impl AdoClient {
    /// Every entry of a suite, sorted by its sequence number. Read only.
    pub async fn get_suite_entries(
        &self,
        org: &str,
        project: &str,
        suite_id: i32,
    ) -> Result<Vec<SuiteEntry>, AdoError> {
        let url = format!(
            "{}/testplan/suiteentry/{}?api-version=7.1",
            self.tp_base(org, project),
            suite_id
        );
        let data = self.get_json(url).await?;
        Ok(entries_of(&data))
    }

    /// Put the suite's test cases in `case_ids` order. Child suites keep
    /// their places at the top (Azure DevOps lists them first and this
    /// never names them); cases the caller did not name follow the named
    /// ones in the order they already had. Returns the case order as the
    /// server reports it back. A suite with no cases sends nothing.
    pub async fn reorder_suite_cases(
        &self,
        org: &str,
        project: &str,
        suite_id: i32,
        case_ids: &[i32],
    ) -> Result<Vec<i32>, AdoError> {
        let current = self.get_suite_entries(org, project, suite_id).await?;
        let ordered = ordered_case_ids(&current, case_ids);
        if ordered.is_empty() {
            return Ok(vec![]);
        }
        let suites = current.iter().filter(|e| e.entry_type == "suite").count() as i32;
        let body: Vec<serde_json::Value> = ordered
            .iter()
            .enumerate()
            .map(|(i, id)| {
                serde_json::json!({
                    "id": id,
                    "sequenceNumber": suites + i as i32,
                    "suiteEntryType": "testCase",
                })
            })
            .collect();
        let url = format!(
            "{}/testplan/suiteentry/{}?api-version=7.1",
            self.tp_base(org, project),
            suite_id
        );
        let data = self.patch_json(url, &serde_json::Value::Array(body)).await?;
        Ok(entries_of(&data)
            .into_iter()
            .filter(|e| e.entry_type == "testCase")
            .map(|e| e.id)
            .collect())
    }

    /// A static child suite (a "folder") under `parent_suite_id`. Azure
    /// DevOps only allows one under a static suite or the plan root; the
    /// server's refusal for any other parent comes back as the error.
    pub async fn create_static_suite(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        parent_suite_id: i32,
        name: &str,
    ) -> Result<SuiteRef, AdoError> {
        let body = serde_json::json!({
            "suiteType": "staticTestSuite",
            "name": name,
            "parentSuite": {"id": parent_suite_id},
        });
        let url = format!(
            "{}/testplan/Plans/{}/suites?api-version=7.1",
            self.tp_base(org, project),
            plan_id
        );
        let data = self.post_json(url, &body).await?;
        Ok(SuiteRef {
            id: data["id"].as_i64().unwrap_or_default() as i32,
            name: data["name"].as_str().unwrap_or(name).to_string(),
            suite_type: data["suiteType"].as_str().unwrap_or("staticTestSuite").to_string(),
            requirement_id: None,
            parent_id: Some(parent_suite_id),
        })
    }

    /// Add existing test cases to a suite. A case can sit in many suites,
    /// so this is a copy: it stays wherever it already was. Returns the
    /// ids the server reports as now in the suite.
    pub async fn add_test_cases_to_suite(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        suite_id: i32,
        case_ids: &[i32],
    ) -> Result<Vec<i32>, AdoError> {
        if case_ids.is_empty() {
            return Ok(vec![]);
        }
        let body: Vec<serde_json::Value> = case_ids
            .iter()
            .map(|id| serde_json::json!({"workItem": {"id": id}}))
            .collect();
        let url = format!(
            "{}/testplan/Plans/{}/Suites/{}/TestCase?api-version=7.1",
            self.tp_base(org, project),
            plan_id,
            suite_id
        );
        let data = self.post_json(url, &serde_json::Value::Array(body)).await?;
        Ok(data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v["workItem"]["id"].as_i64().map(|i| i as i32))
            .collect())
    }
}
