//! Pipeline + deployment reads for a pull request: which builds ran for it
//! and which environments the result reached.
//!
//! This org's shape (verified against the Pipelines hub): YAML build
//! pipelines run twice per PR - once as validation while the PR is open
//! (`refs/pull/{id}/merge`), once as CI on the target branch after the
//! merge (the merge commit) - and deployments live in **Classic Release
//! Management**, which is a different host (`vsrm.dev.azure.com`) keyed off
//! the build artifact. So: builds -> timeline (stages) -> releases
//! (environments).
//!
//! GET only, and every lookup past the build list is best-effort: a missing
//! timeline or an org with no Release Management just yields fewer details,
//! never an error that hides the builds. No writes, no DELETE, ever
//! (scanned by tests/ado.rs).

use crate::ado::{AdoClient, AdoError};
use serde::Serialize;

/// A single step inside a job - the level ADO's log view shows, and the
/// level a failure is actually pinned to.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TimelineTask {
    pub name: String,
    /// "completed" | "inProgress" | "pending".
    pub state: String,
    /// "succeeded" | "failed" | "skipped" | "abandoned" | "" while running.
    pub result: String,
    pub started: String,
    pub finished: String,
    /// Error/warning text ADO attached to this step - the summary you get
    /// without fetching anything.
    pub issues: Vec<String>,
    /// Timeline log id, when this step produced output. 0 = none yet (a
    /// pending step, or one whose logs have been cleaned up).
    pub log_id: i32,
}

/// A job inside a stage, holding the steps.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BuildJob {
    pub name: String,
    pub state: String,
    pub result: String,
    pub started: String,
    pub finished: String,
    pub tasks: Vec<TimelineTask>,
}

/// One stage inside a build run (the YAML `stages:` list).
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BuildStage {
    pub name: String,
    /// "completed" | "inProgress" | "pending".
    pub state: String,
    /// "succeeded" | "failed" | "canceled" | "skipped" | "" while running.
    pub result: String,
    pub started: String,
    pub finished: String,
    pub jobs: Vec<BuildJob>,
}

/// One environment a release carried this build into.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Deployment {
    /// Release name, e.g. "Release-482".
    pub release: String,
    /// Environment/stage name, e.g. "QA" or "Production".
    pub environment: String,
    /// "succeeded" | "inProgress" | "notStarted" | "rejected" | "canceled"
    /// | "queued" | "scheduled" | "partiallySucceeded".
    pub status: String,
    /// When the environment last changed state (may be empty).
    pub on: String,
    pub web_url: String,
}

/// Deployments for one build, for the cache-revalidation command.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BuildDeployments {
    pub build_id: i32,
    pub deployments: Vec<Deployment>,
}

/// How far a pull request's validation got, for the row in the list.
/// Only ever "running", "failed" or "succeeded"; a PR with no validation
/// build is omitted rather than guessed at.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PrBuildState {
    pub pr_id: i32,
    pub state: String,
}

/// `refs/pull/1234/merge` -> 1234. Anything else is not a PR validation
/// branch: a build on `refs/heads/main` must never be read as one.
pub(crate) fn pr_id_from_branch(branch: &str) -> Option<i32> {
    branch
        .strip_prefix("refs/pull/")?
        .strip_suffix("/merge")?
        .parse()
        .ok()
}

/// A build run tied to a pull request, with its stages and deployments.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PrBuild {
    pub id: i32,
    /// Pipeline definition name, e.g. "HRM-PMS-NET".
    pub name: String,
    /// Run number, e.g. "2026.7.24-12".
    pub number: String,
    /// "completed" | "inProgress" | "notStarted" | "cancelling".
    pub status: String,
    /// "succeeded" | "failed" | "canceled" | "partiallySucceeded" | "".
    pub result: String,
    /// True when this ran as PR validation, false for the post-merge CI run.
    pub is_validation: bool,
    pub started: String,
    pub finished: String,
    pub web_url: String,
    pub stages: Vec<BuildStage>,
    pub deployments: Vec<Deployment>,
}

fn s(v: &serde_json::Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

impl AdoClient {
    /// Release Management lives on its own host; in prod that is
    /// `vsrm.dev.azure.com`. Derived from `base_url` the same way the
    /// search host is, so wiremock (a bare 127.0.0.1 base) is a no-op and
    /// keeps pointing at the mock server.
    fn vsrm_base(&self) -> String {
        self.base_url.replacen("dev.azure.com", "vsrm.dev.azure.com", 1)
    }

    /// Builds for a PR: validation runs on `refs/pull/{id}/merge`, plus the
    /// post-merge CI run identified by the PR's merge commit. Newest first.
    /// Read only.
    pub async fn pr_builds(
        &self,
        org: &str,
        project: &str,
        repo_id: &str,
        pr_id: i32,
        merge_commit: &str,
    ) -> Result<Vec<PrBuild>, AdoError> {
        let mut builds = self
            .builds_for_branch(org, project, repo_id, &format!("refs/pull/{pr_id}/merge"), true)
            .await?;

        // The CI run after the merge is on the target branch, so it can only
        // be matched by commit. Skipped for open PRs (no merge commit yet).
        if !merge_commit.is_empty() {
            let ci = self
                .builds_for_commit(org, project, repo_id, merge_commit)
                .await
                .unwrap_or_default();
            builds.extend(ci);
        }

        for b in &mut builds {
            b.stages = self.build_stages(org, project, b.id).await.unwrap_or_default();
            b.deployments = self.build_deployments(org, project, b.id).await.unwrap_or_default();
        }
        builds.sort_by(|a, b| b.started.cmp(&a.started));
        Ok(builds)
    }

    async fn builds_for_branch(
        &self,
        org: &str,
        project: &str,
        repo_id: &str,
        branch: &str,
        is_validation: bool,
    ) -> Result<Vec<PrBuild>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/build/builds?repositoryId={}&repositoryType=TfsGit&branchName={}&$top=10&api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo_id),
            urlencoding::encode(branch),
        );
        let data = self.get_json(url).await?;
        Ok(parse_builds(&data, is_validation))
    }

    /// Where each pull request's validation build got to, for the LIST.
    ///
    /// The expanded row fetches a PR's builds in full, which is several
    /// calls; doing that for every row on screen is exactly the traffic
    /// this app works to avoid. So this asks once per REPOSITORY and sorts
    /// the answer out locally - the same trick `builds_for_commit` uses,
    /// because Azure DevOps will not filter builds by a set of branches.
    ///
    /// One PR can trigger several pipelines. They are folded the way a
    /// reader folds them: anything still going makes the PR "running",
    /// otherwise anything that did not succeed makes it "failed". A PR with
    /// no validation build at all is simply absent from the result - it is
    /// not the same as passing, and the caller must not treat it as either.
    pub async fn pr_build_states(
        &self,
        org: &str,
        project: &str,
        repo_id: &str,
        pr_ids: &[i32],
    ) -> Result<Vec<PrBuildState>, AdoError> {
        if pr_ids.is_empty() {
            return Ok(vec![]);
        }
        let url = format!(
            "{}/{}/{}/_apis/build/builds?repositoryId={}&repositoryType=TfsGit&$top=200&api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo_id),
        );
        let data = self.get_json(url).await?;

        let mut running: std::collections::HashSet<i32> = Default::default();
        let mut failed: std::collections::HashSet<i32> = Default::default();
        let mut passed: std::collections::HashSet<i32> = Default::default();
        for b in data["value"].as_array().cloned().unwrap_or_default() {
            let Some(pr_id) = b["sourceBranch"].as_str().and_then(pr_id_from_branch) else {
                continue;
            };
            if !pr_ids.contains(&pr_id) {
                continue;
            }
            let status = b["status"].as_str().unwrap_or("");
            let result = b["result"].as_str().unwrap_or("");
            if status != "completed" {
                running.insert(pr_id);
            } else if result == "succeeded" {
                passed.insert(pr_id);
            } else {
                // canceled and partiallySucceeded land here too. Neither is
                // a green run, and the panel only has the two words.
                failed.insert(pr_id);
            }
        }

        let mut out: Vec<PrBuildState> = vec![];
        for &id in pr_ids {
            let state = if running.contains(&id) {
                "running"
            } else if failed.contains(&id) {
                "failed"
            } else if passed.contains(&id) {
                "succeeded"
            } else {
                continue; // no validation build - say nothing about it
            };
            out.push(PrBuildState { pr_id: id, state: state.into() });
        }
        Ok(out)
    }

    /// The post-merge CI build. ADO has no "builds for commit" filter, so
    /// this pulls the repo's recent builds and matches `sourceVersion`.
    async fn builds_for_commit(
        &self,
        org: &str,
        project: &str,
        repo_id: &str,
        commit: &str,
    ) -> Result<Vec<PrBuild>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/build/builds?repositoryId={}&repositoryType=TfsGit&$top=100&api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo_id),
        );
        let data = self.get_json(url).await?;
        let all = data["value"].as_array().cloned().unwrap_or_default();
        let matching: Vec<serde_json::Value> = all
            .into_iter()
            .filter(|b| b["sourceVersion"].as_str() == Some(commit))
            .collect();
        Ok(parse_builds(
            &serde_json::json!({ "value": matching }),
            false,
        ))
    }

    /// Stage records from the build timeline. Best-effort: a build whose
    /// timeline has expired just shows no stages.
    async fn build_stages(
        &self,
        org: &str,
        project: &str,
        build_id: i32,
    ) -> Result<Vec<BuildStage>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/build/builds/{}/timeline?api-version=7.1",
            self.base_url, org, project, build_id
        );
        let data = self.get_json(url).await?;
        Ok(build_stage_tree(&data))
    }

    /// Fresh deployments for a set of already-known builds. This is the
    /// cache-revalidation path: a finished build's runs/stages/logs are
    /// immutable and stay cached, but a Classic release can be created
    /// against it LATER - so the frontend re-asks only this cheap question
    /// on top of its cached history and folds the answer in. Read only.
    pub async fn builds_deployments(
        &self,
        org: &str,
        project: &str,
        build_ids: &[i32],
    ) -> Result<Vec<BuildDeployments>, AdoError> {
        let mut out = vec![];
        for &id in build_ids {
            out.push(BuildDeployments {
                build_id: id,
                deployments: self.build_deployments(org, project, id).await.unwrap_or_default(),
            });
        }
        Ok(out)
    }

    /// Classic releases that consumed this build as their artifact, with
    /// each environment's status. Best-effort: orgs without Release
    /// Management (or a build nothing deployed) simply have none.
    async fn build_deployments(
        &self,
        org: &str,
        project: &str,
        build_id: i32,
    ) -> Result<Vec<Deployment>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/release/releases?artifactVersionId={}&$expand=environments&api-version=7.1",
            self.vsrm_base(),
            org,
            project,
            build_id
        );
        let data = self.get_json(url).await?;
        let mut out = vec![];
        for rel in data["value"].as_array().cloned().unwrap_or_default() {
            let release = s(&rel["name"]);
            let web_url = rel["_links"]["web"]["href"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            for env in rel["environments"].as_array().cloned().unwrap_or_default() {
                out.push(Deployment {
                    release: release.clone(),
                    environment: s(&env["name"]),
                    status: s(&env["status"]),
                    // ADO uses either depending on how the env was reached.
                    on: if env["modifiedOn"].is_string() {
                        s(&env["modifiedOn"])
                    } else {
                        s(&env["createdOn"])
                    },
                    web_url: web_url.clone(),
                });
            }
        }
        Ok(out)
    }
}

/// Rebuilds the timeline's Stage -> Job -> Task tree.
///
/// ADO returns one flat record list linked by `parentId`, and the real
/// hierarchy is Stage -> Phase -> Job -> Task: a job's parent is a *phase*,
/// not the stage, so jobs are attached by walking parents up to the nearest
/// Stage rather than by a direct id match. Everything is ordered by the
/// records' own `order`, and orphans (a job whose stage record is missing)
/// are dropped rather than guessed at.
pub fn build_stage_tree(data: &serde_json::Value) -> Vec<BuildStage> {
    use std::collections::HashMap;

    let records = data["records"].as_array().cloned().unwrap_or_default();
    let by_id: HashMap<String, &serde_json::Value> = records
        .iter()
        .filter_map(|r| r["id"].as_str().map(|id| (id.to_string(), r)))
        .collect();

    // Walk up parents until a record of `want` is found.
    let ancestor = |rec: &serde_json::Value, want: &str| -> Option<String> {
        let mut cur = rec["parentId"].as_str().map(str::to_string);
        // Timelines are shallow; the bound just guarantees termination if
        // ADO ever hands back a cycle.
        for _ in 0..10 {
            let id = cur?;
            let parent = by_id.get(&id)?;
            if parent["type"].as_str() == Some(want) {
                return Some(id);
            }
            cur = parent["parentId"].as_str().map(str::to_string);
        }
        None
    };

    let order_of = |r: &serde_json::Value| r["order"].as_i64().unwrap_or(0);
    let issues_of = |r: &serde_json::Value| -> Vec<String> {
        r["issues"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|i| i["message"].as_str().map(str::to_string))
            .collect()
    };

    // Tasks grouped under their job id.
    let mut tasks_by_job: HashMap<String, Vec<(i64, TimelineTask)>> = HashMap::new();
    for r in records.iter().filter(|r| r["type"].as_str() == Some("Task")) {
        let Some(job_id) = r["parentId"].as_str() else { continue };
        tasks_by_job.entry(job_id.to_string()).or_default().push((
            order_of(r),
            TimelineTask {
                name: s(&r["name"]),
                state: s(&r["state"]),
                result: s(&r["result"]),
                started: s(&r["startTime"]),
                finished: s(&r["finishTime"]),
                issues: issues_of(r),
                log_id: r["log"]["id"].as_i64().unwrap_or(0) as i32,
            },
        ));
    }

    // Jobs grouped under the stage they ultimately belong to.
    let mut jobs_by_stage: HashMap<String, Vec<(i64, BuildJob)>> = HashMap::new();
    for r in records.iter().filter(|r| r["type"].as_str() == Some("Job")) {
        let Some(stage_id) = ancestor(r, "Stage") else { continue };
        let mut tasks = r["id"]
            .as_str()
            .and_then(|id| tasks_by_job.remove(id))
            .unwrap_or_default();
        tasks.sort_by_key(|(o, _)| *o);
        jobs_by_stage.entry(stage_id).or_default().push((
            order_of(r),
            BuildJob {
                name: s(&r["name"]),
                state: s(&r["state"]),
                result: s(&r["result"]),
                started: s(&r["startTime"]),
                finished: s(&r["finishTime"]),
                tasks: tasks.into_iter().map(|(_, t)| t).collect(),
            },
        ));
    }

    let mut stages: Vec<(i64, BuildStage)> = records
        .iter()
        .filter(|r| r["type"].as_str() == Some("Stage"))
        .map(|r| {
            let mut jobs = r["id"]
                .as_str()
                .and_then(|id| jobs_by_stage.remove(id))
                .unwrap_or_default();
            jobs.sort_by_key(|(o, _)| *o);
            (
                order_of(r),
                BuildStage {
                    name: s(&r["name"]),
                    state: s(&r["state"]),
                    result: s(&r["result"]),
                    started: s(&r["startTime"]),
                    finished: s(&r["finishTime"]),
                    jobs: jobs.into_iter().map(|(_, j)| j).collect(),
                },
            )
        })
        .collect();
    stages.sort_by_key(|(o, _)| *o);
    stages.into_iter().map(|(_, st)| st).collect()
}

impl AdoClient {
    /// Plain-text output for one step of a build, the same content ADO's
    /// log pane shows. Safe to poll while a build runs: ADO returns the
    /// log as it stands, so a running step just returns fewer lines each
    /// time. Read only.
    pub async fn build_log(
        &self,
        org: &str,
        project: &str,
        build_id: i32,
        log_id: i32,
    ) -> Result<String, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/build/builds/{}/logs/{}?api-version=7.1",
            self.base_url, org, project, build_id, log_id
        );
        self.get_text(url).await
    }
}

fn parse_builds(data: &serde_json::Value, is_validation: bool) -> Vec<PrBuild> {
    data["value"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|b| PrBuild {
            id: b["id"].as_i64().unwrap_or_default() as i32,
            name: s(&b["definition"]["name"]),
            number: s(&b["buildNumber"]),
            status: s(&b["status"]),
            result: s(&b["result"]),
            is_validation,
            started: s(&b["startTime"]),
            finished: s(&b["finishTime"]),
            web_url: b["_links"]["web"]["href"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            stages: vec![],
            deployments: vec![],
        })
        .collect()
}
