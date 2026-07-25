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

/// One stage inside a build run (the YAML `stages:` list).
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BuildStage {
    pub name: String,
    /// "completed" | "inProgress" | "pending".
    pub state: String,
    /// "succeeded" | "failed" | "canceled" | "skipped" | "" while running.
    pub result: String,
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
        let mut stages: Vec<(i32, BuildStage)> = data["records"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|r| r["type"].as_str() == Some("Stage"))
            .map(|r| {
                (
                    r["order"].as_i64().unwrap_or(0) as i32,
                    BuildStage {
                        name: s(&r["name"]),
                        state: s(&r["state"]),
                        result: s(&r["result"]),
                    },
                )
            })
            .collect();
        stages.sort_by_key(|(order, _)| *order);
        Ok(stages.into_iter().map(|(_, st)| st).collect())
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
