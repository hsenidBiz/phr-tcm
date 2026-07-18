//! Pull-request reads for the Work Manager's PR panel: repositories,
//! "awaiting your review", "mine", and active-on-repo lists. GET only -
//! voting, completing and abandoning stay in Azure DevOps; this module
//! never writes anything, and no DELETE, ever (scanned by tests/ado.rs).

use crate::ado::{AdoClient, AdoError};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RepoRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PrReviewer {
    pub display_name: String,
    /// ADO vote: 10 approved, 5 approved w/ suggestions, 0 no vote,
    /// -5 waiting for author, -10 rejected.
    pub vote: i32,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PullRequest {
    pub id: i32,
    pub title: String,
    pub repo: String,
    pub author: String,
    pub source_branch: String,
    pub target_branch: String,
    pub created: String,
    pub is_draft: bool,
    pub has_conflicts: bool,
    /// The signed-in user's vote on this PR (0 when not a reviewer).
    pub my_vote: i32,
    pub reviewers: Vec<PrReviewer>,
    pub web_url: String,
}

/// The actionable slices for the PR panel's top groups.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PrOverview {
    /// Active PRs where the user is a reviewer and has not voted yet.
    pub awaiting: Vec<PullRequest>,
    /// Active PRs the user created.
    pub mine: Vec<PullRequest>,
}

fn branch(refname: &str) -> String {
    refname.strip_prefix("refs/heads/").unwrap_or(refname).to_string()
}

fn parse_pr(
    v: &serde_json::Value,
    base_url: &str,
    org: &str,
    project: &str,
    my_id: &str,
) -> PullRequest {
    let s = |val: &serde_json::Value| val.as_str().unwrap_or_default().to_string();
    let repo = s(&v["repository"]["name"]);
    let id = v["pullRequestId"].as_i64().unwrap_or_default() as i32;
    let reviewers: Vec<PrReviewer> = v["reviewers"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| PrReviewer {
            display_name: s(&r["displayName"]),
            vote: r["vote"].as_i64().unwrap_or(0) as i32,
        })
        .collect();
    let my_vote = v["reviewers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["id"].as_str() == Some(my_id))
        .and_then(|r| r["vote"].as_i64())
        .unwrap_or(0) as i32;
    PullRequest {
        title: s(&v["title"]),
        author: s(&v["createdBy"]["displayName"]),
        source_branch: branch(&s(&v["sourceRefName"])),
        target_branch: branch(&s(&v["targetRefName"])),
        created: s(&v["creationDate"]),
        is_draft: v["isDraft"].as_bool().unwrap_or(false),
        has_conflicts: v["mergeStatus"].as_str() == Some("conflicts"),
        my_vote,
        reviewers,
        // The list responses carry no web link - ADO's PR URLs are fully
        // deterministic, so build it.
        web_url: format!(
            "{}/{}/{}/_git/{}/pullrequest/{}",
            base_url,
            org,
            project,
            urlencoding::encode(&repo),
            id
        ),
        repo,
        id,
    }
}

impl AdoClient {
    /// The signed-in user's org-scoped identity id (connectionData) - the
    /// id searchCriteria.creatorId/reviewerId expect. Read only.
    async fn my_identity_id(&self, org: &str) -> Result<String, AdoError> {
        let url = format!(
            "{}/{}/_apis/connectionData?api-version=7.1-preview.1",
            self.base_url, org
        );
        let data = self.get_json(url).await?;
        Ok(data["authenticatedUser"]["id"].as_str().unwrap_or_default().to_string())
    }

    /// The project's git repositories, sorted by name. Read only.
    pub async fn list_repos(&self, org: &str, project: &str) -> Result<Vec<RepoRef>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/git/repositories?api-version=7.1",
            self.base_url, org, project
        );
        let data = self.get_json(url).await?;
        let mut repos: Vec<RepoRef> = data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| {
                Some(RepoRef {
                    id: r["id"].as_str()?.to_string(),
                    name: r["name"].as_str().unwrap_or_default().to_string(),
                })
            })
            .collect();
        repos.sort_by_key(|r| r.name.to_lowercase());
        Ok(repos)
    }

    async fn pull_requests_where(
        &self,
        org: &str,
        project: &str,
        criteria: &str,
        my_id: &str,
    ) -> Result<Vec<PullRequest>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/git/pullrequests?searchCriteria.status=active&{}&api-version=7.1",
            self.base_url, org, project, criteria
        );
        let data = self.get_json(url).await?;
        Ok(data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|v| parse_pr(v, &self.base_url, org, project, my_id))
            .collect())
    }

    /// The panel's actionable groups: PRs awaiting the user's review (vote
    /// still 0) first, then the user's own active PRs. Read only.
    pub async fn pr_overview(&self, org: &str, project: &str) -> Result<PrOverview, AdoError> {
        let me = self.my_identity_id(org).await?;
        let reviewing = self
            .pull_requests_where(org, project, &format!("searchCriteria.reviewerId={me}"), &me)
            .await?;
        let awaiting = reviewing
            .into_iter()
            .filter(|pr| pr.my_vote == 0)
            .collect();
        let mine = self
            .pull_requests_where(org, project, &format!("searchCriteria.creatorId={me}"), &me)
            .await?;
        Ok(PrOverview { awaiting, mine })
    }

    /// All active PRs on one repository. Read only.
    pub async fn repo_pull_requests(
        &self,
        org: &str,
        project: &str,
        repo_id: &str,
    ) -> Result<Vec<PullRequest>, AdoError> {
        // Best-effort identity for my_vote highlighting; anonymous fallback
        // just means my_vote stays 0.
        let me = self.my_identity_id(org).await.unwrap_or_default();
        let url = format!(
            "{}/{}/{}/_apis/git/repositories/{}/pullrequests?searchCriteria.status=active&api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo_id)
        );
        let data = self.get_json(url).await?;
        Ok(data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|v| parse_pr(v, &self.base_url, org, project, &me))
            .collect())
    }
}
