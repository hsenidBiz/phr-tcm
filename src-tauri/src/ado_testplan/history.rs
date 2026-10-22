//! The recent-runs outcome sweep behind Run Tests' history dots.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use futures::stream::{self, StreamExt};

use super::{id_i32, is_no_verdict, CaseHistory, RunOutcome, SUITE_SCAN_CONCURRENCY};
use crate::ado::{AdoClient, AdoError};
use crate::cache::keys;

/// Outcomes kept per case.
const RUN_HISTORY_PER_CASE: usize = 5;
/// Runs walked when the caller names no cases (nothing to stop on).
const RUN_HISTORY_UNTARGETED_RUNS: usize = 15;
/// Runs walked at most when cases are named.
const RUN_HISTORY_MAX_RUNS: usize = 150;

/// One verdict row of a run: which case, and the outcome as shown in the UI.
/// The session cache keeps a completed run's whole `Vec<CaseOutcome>`.
#[derive(Clone)]
struct CaseOutcome {
    test_case_id: i32,
    outcome: RunOutcome,
}

struct PlanRun {
    id: i32,
    date: String,
    completed: bool,
}

/// A run's verdict rows. Results with no verdict, or no parsable
/// `testCase.id`, are dropped; several rows for one case (configurations)
/// are kept, in API order.
fn run_case_outcomes(run_id: i32, rows: &[serde_json::Value]) -> Vec<CaseOutcome> {
    let mut out = vec![];
    for r in rows {
        let outcome = r["outcome"].as_str().unwrap_or_default();
        if is_no_verdict(outcome) {
            continue;
        }
        // testCase.id arrives as a string in run results.
        let Some(test_case_id) = id_i32(&r["testCase"]["id"]) else {
            continue;
        };
        out.push(CaseOutcome {
            test_case_id,
            outcome: RunOutcome {
                outcome: outcome.to_string(),
                completed_date: r["completedDate"].as_str().unwrap_or_default().to_string(),
                run_id,
                result_id: r["id"].as_i64().unwrap_or_default() as i32,
                run_by: r["runBy"]["displayName"].as_str().unwrap_or_default().to_string(),
            },
        });
    }
    out
}

impl AdoClient {
    /// Per-test-case outcome history for a plan, newest first, capped at
    /// RUN_HISTORY_PER_CASE. Pages the plan's run list, sorts it newest
    /// first and walks it in batches of SUITE_SCAN_CONCURRENCY, reading each
    /// run's results page by page, until every case in `test_case_ids` has 5
    /// outcomes, the runs run out, or 150 runs were walked. An empty list
    /// walks the newest 15 runs and returns every case seen. A completed
    /// run's outcomes are kept in the session cache, so a repeat sweep reads
    /// only new or unfinished runs; runs that 403/404 are skipped (and never
    /// kept). A later non-auth failure ends the sweep with what was gathered
    /// (logged, summary says partial); with nothing gathered it is returned.
    /// One info line per sweep says what bit. Read only.
    pub async fn run_history(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        test_case_ids: &[i32],
    ) -> Result<Vec<CaseHistory>, AdoError> {
        let started = Instant::now();
        let list_url = format!(
            "{}/test/runs?planId={}&api-version=7.1",
            self.tp_base(org, project),
            plan_id
        );
        let (rows, list_pages) = self.get_skip_paged(list_url, "test runs").await?;
        let mut runs: Vec<PlanRun> = rows
            .iter()
            .filter_map(|r| {
                Some(PlanRun {
                    id: r["id"].as_i64()? as i32,
                    date: r["completedDate"]
                        .as_str()
                        .or_else(|| r["startedDate"].as_str())
                        .unwrap_or_default()
                        .to_string(),
                    completed: r["state"]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("Completed")),
                })
            })
            .collect();
        // Newest first (ISO dates sort lexicographically; undated runs last).
        runs.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
        let listed = runs.len();

        let targets: HashSet<i32> = test_case_ids.iter().copied().collect();
        let targeted = !targets.is_empty();
        let limit = if targeted { RUN_HISTORY_MAX_RUNS } else { RUN_HISTORY_UNTARGETED_RUNS };

        let mut by_case: HashMap<i32, Vec<RunOutcome>> = HashMap::new();
        let (mut walked, mut fetched, mut cached, mut skipped, mut multi_page) = (0usize, 0, 0, 0, 0);
        let mut all_full = false;
        let mut partial = false;

        'sweep: for chunk in runs[..listed.min(limit)].chunks(SUITE_SCAN_CONCURRENCY) {
            // Cache hits first; the misses are fetched concurrently (eager
            // futures - closure-captured refs break the tauri command macro).
            let mut per_run: Vec<Option<Result<(Vec<serde_json::Value>, usize), AdoError>>> =
                chunk.iter().map(|_| None).collect();
            let mut hits: Vec<Option<Vec<CaseOutcome>>> = Vec::with_capacity(chunk.len());
            let mut futs = vec![];
            for (i, run) in chunk.iter().enumerate() {
                let key = keys::run_outcomes(&self.base_url, org, project, run.id);
                let hit = crate::cache::session_fresh::<Vec<CaseOutcome>>(&key, keys::RUN_OUTCOMES_TTL);
                if hit.is_none() {
                    let url = format!(
                        "{}/test/Runs/{}/results?api-version=7.1",
                        self.tp_base(org, project),
                        run.id
                    );
                    let fut = self.get_skip_paged(url, "run results");
                    futs.push(async move { (i, fut.await) });
                }
                hits.push(hit);
            }
            {
                let mut in_flight = stream::iter(futs).buffer_unordered(SUITE_SCAN_CONCURRENCY);
                while let Some((i, res)) = in_flight.next().await {
                    per_run[i] = Some(res);
                }
            }

            // Aggregate in the chunk's newest-first order.
            for (i, run) in chunk.iter().enumerate() {
                walked += 1;
                let outcomes = if let Some(hit) = hits[i].take() {
                    cached += 1;
                    hit
                } else {
                    match per_run[i].take().expect("every missed run was fetched") {
                        Err(AdoError::Forbidden) | Err(AdoError::NotFound) => {
                            skipped += 1;
                            continue;
                        }
                        Err(AdoError::Unauthorized) => return Err(AdoError::Unauthorized),
                        Err(e) => {
                            // Something other than a refusal (5xx, throttling,
                            // network). With no run read, or none of the read
                            // runs holding an asked case, the caller needs the
                            // error (an empty answer would overwrite a good
                            // saved history); otherwise keep what was read.
                            if fetched + cached == 0 || by_case.is_empty() {
                                return Err(e);
                            }
                            crate::applog::warn(format!(
                                "run history: plan {plan_id}: a run could not be read after {} run(s) \
                                 were read - the history may be incomplete",
                                fetched + cached
                            ));
                            partial = true;
                            break 'sweep;
                        }
                        Ok((rows, pages)) => {
                            fetched += 1;
                            if pages > 1 {
                                multi_page += 1;
                            }
                            let outcomes = run_case_outcomes(run.id, &rows);
                            if run.completed {
                                crate::cache::session_put(
                                    &keys::run_outcomes(&self.base_url, org, project, run.id),
                                    outcomes.clone(),
                                );
                            }
                            outcomes
                        }
                    }
                };
                for o in outcomes {
                    if targeted && !targets.contains(&o.test_case_id) {
                        continue;
                    }
                    let list = by_case.entry(o.test_case_id).or_default();
                    if list.len() < RUN_HISTORY_PER_CASE {
                        list.push(o.outcome);
                    }
                }
            }

            if targeted
                && targets
                    .iter()
                    .all(|id| by_case.get(id).is_some_and(|l| l.len() >= RUN_HISTORY_PER_CASE))
            {
                all_full = true;
                break;
            }
        }

        let reason = if partial {
            "partial (a run failed)"
        } else if !targeted {
            "untargeted (newest 15)"
        } else if all_full {
            "every asked case has 5"
        } else if walked >= listed {
            "runs exhausted"
        } else {
            "cap of 150 runs"
        };
        let full = by_case.values().filter(|l| l.len() >= RUN_HISTORY_PER_CASE).count();
        crate::applog::info(format!(
            "run history: plan {plan_id}: {listed} runs listed ({list_pages} page(s)), {walked} walked \
             ({fetched} read, {cached} from cache, {skipped} skipped, {multi_page} with more than one \
             page of results), {covered}/{asked} asked cases with history, {full} with 5, stopped: \
             {reason}, {ms} ms",
            covered = by_case.len(),
            asked = targets.len(),
            ms = started.elapsed().as_millis(),
        ));

        Ok(by_case
            .into_iter()
            .map(|(test_case_id, outcomes)| CaseHistory { test_case_id, outcomes })
            .collect())
    }
}
