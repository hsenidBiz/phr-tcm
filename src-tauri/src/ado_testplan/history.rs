//! The recent-runs outcome sweep behind Run Tests' history dots.

use futures::stream::{self, StreamExt};

use super::{id_i32, CaseHistory, RunOutcome, SUITE_SCAN_CONCURRENCY};
use crate::ado::{AdoClient, AdoError};

impl AdoClient {
    /// Per-test-case outcome history for a plan, via the recent-runs sweep:
    /// GET the plan's runs, take the RUN_HISTORY_RUNS most recent, fetch each
    /// run's results concurrently, and aggregate outcomes per test case -
    /// newest first, capped at RUN_HISTORY_PER_CASE. Unspecified/in-progress
    /// results are skipped; runs that 403/404 are skipped like the suite
    /// scan. Read only.
    pub async fn run_history(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
    ) -> Result<Vec<CaseHistory>, AdoError> {
        const RUN_HISTORY_RUNS: usize = 15;
        const RUN_HISTORY_PER_CASE: usize = 5;

        let url = format!(
            "{}/test/runs?planId={}&api-version=7.1",
            self.tp_base(org, project),
            plan_id
        );
        let data = self.get_json(url).await?;
        let mut runs: Vec<(i32, String)> = data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| {
                Some((
                    r["id"].as_i64()? as i32,
                    r["completedDate"]
                        .as_str()
                        .or_else(|| r["startedDate"].as_str())
                        .unwrap_or_default()
                        .to_string(),
                ))
            })
            .collect();
        // Newest first (ISO dates sort lexicographically; undated runs last).
        runs.sort_by(|a, b| b.1.cmp(&a.1));
        runs.truncate(RUN_HISTORY_RUNS);

        // Fetch each run's results with bounded concurrency (eager futures -
        // closure-captured refs break the tauri command macro).
        let mut per_run: Vec<Option<Result<serde_json::Value, AdoError>>> =
            runs.iter().map(|_| None).collect();
        {
            let futs: Vec<_> = runs
                .iter()
                .enumerate()
                .map(|(i, (run_id, _))| {
                    let url = format!(
                        "{}/test/Runs/{}/results?api-version=7.1",
                        self.tp_base(org, project),
                        run_id
                    );
                    let fut = self.get_json(url);
                    async move { (i, fut.await) }
                })
                .collect();
            let mut in_flight = stream::iter(futs).buffer_unordered(SUITE_SCAN_CONCURRENCY);
            while let Some((i, res)) = in_flight.next().await {
                per_run[i] = Some(res);
            }
        }

        // Aggregate in run order (newest run first) so each case's list is
        // naturally newest-first.
        let mut by_case: std::collections::HashMap<i32, Vec<RunOutcome>> =
            std::collections::HashMap::new();
        for ((run_id, _), res) in runs.iter().zip(per_run) {
            let data = match res.expect("every run index was filled") {
                Ok(d) => d,
                Err(AdoError::Forbidden) | Err(AdoError::NotFound) => continue,
                Err(e) => return Err(e),
            };
            for r in data["value"].as_array().cloned().unwrap_or_default() {
                let outcome = r["outcome"].as_str().unwrap_or_default();
                if outcome.is_empty() || outcome.eq_ignore_ascii_case("unspecified") {
                    continue;
                }
                // testCase.id arrives as a string in run results.
                let case_id = match id_i32(&r["testCase"]["id"]) {
                    Some(id) => id,
                    None => continue,
                };
                let list = by_case.entry(case_id).or_default();
                if list.len() < RUN_HISTORY_PER_CASE {
                    list.push(RunOutcome {
                        outcome: outcome.to_string(),
                        completed_date: r["completedDate"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        run_id: *run_id,
                        result_id: r["id"].as_i64().unwrap_or_default() as i32,
                        run_by: r["runBy"]["displayName"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                    });
                }
            }
        }
        Ok(by_case
            .into_iter()
            .map(|(test_case_id, outcomes)| CaseHistory { test_case_id, outcomes })
            .collect())
    }
}
