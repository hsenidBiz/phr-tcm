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
    /// Repository GUID. The git APIs accept the name in their URL path, but
    /// the Build API's `repositoryId=` query param demands the id - passing
    /// a name there 400s, which is what broke the pipeline lookup.
    pub repo_id: String,
    pub author: String,
    pub source_branch: String,
    pub target_branch: String,
    pub created: String,
    /// The PR description (ADO truncates long ones in list responses).
    pub description: String,
    pub is_draft: bool,
    pub has_conflicts: bool,
    /// "active" | "completed" | "abandoned".
    pub status: String,
    /// When a completed/abandoned PR closed; empty while active.
    pub closed: String,
    /// Merge commit on the target branch, once completed - the handle the
    /// pipeline lookup uses to find the post-merge CI build.
    pub merge_commit: String,
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

/// A work item linked to a PR, shown as a rich chip in the PR detail the
/// way Azure DevOps renders "Related Work Items": type + id + title + state.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PrWorkItem {
    pub id: i32,
    pub work_item_type: String,
    pub title: String,
    pub state: String,
    /// Hex (no '#') for the state dot, from the type's process states.
    pub state_color: String,
    pub url: String,
}

/// One work-item -> pull-request association, for the board's PR chips.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PrLink {
    pub work_item_id: i32,
    pub pr_id: i32,
    /// "active" | "completed" | "abandoned".
    pub status: String,
    pub title: String,
    /// Repository name - the chip label, so "database ●" and "web ✓" read
    /// apart when one item carries PRs in several repos.
    pub repo: String,
    pub web_url: String,
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
    let repo_id = s(&v["repository"]["id"]);
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
        description: s(&v["description"]),
        is_draft: v["isDraft"].as_bool().unwrap_or(false),
        has_conflicts: v["mergeStatus"].as_str() == Some("conflicts"),
        status: s(&v["status"]),
        closed: s(&v["closedDate"]),
        merge_commit: s(&v["lastMergeCommit"]["commitId"]),
        repo_id,
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

    /// Work-item -> PR associations for the board's chips. The workitems
    /// batch-GET can't expand relations, so this goes the other way: one
    /// project-wide list of active (+ recently completed) PRs, then each
    /// PR's linked work items - a handful of small GETs instead of one
    /// per board card. Best-effort per PR; failures just mean no chip.
    /// Read only.
    pub async fn board_pr_links(&self, org: &str, project: &str) -> Result<Vec<PrLink>, AdoError> {
        let mut prs: Vec<(i32, String, String, String, String)> = vec![]; // id, status, title, repo_id, repo_name
        for (status, extra) in [("active", ""), ("completed", "&$top=25")] {
            let url = format!(
                "{}/{}/{}/_apis/git/pullrequests?searchCriteria.status={}{}&api-version=7.1",
                self.base_url, org, project, status, extra
            );
            // Completed-PR history is a nice-to-have - ignore its failure.
            let data = match self.get_json(url).await {
                Ok(d) => d,
                Err(e) if status == "active" => return Err(e),
                Err(_) => continue,
            };
            for v in data["value"].as_array().cloned().unwrap_or_default() {
                prs.push((
                    v["pullRequestId"].as_i64().unwrap_or_default() as i32,
                    status.to_string(),
                    v["title"].as_str().unwrap_or_default().to_string(),
                    v["repository"]["id"].as_str().unwrap_or_default().to_string(),
                    v["repository"]["name"].as_str().unwrap_or_default().to_string(),
                ));
            }
        }

        let mut links = vec![];
        for (pr_id, status, title, repo_id, repo_name) in prs {
            let url = format!(
                "{}/{}/{}/_apis/git/repositories/{}/pullRequests/{}/workitems?api-version=7.1",
                self.base_url,
                org,
                project,
                urlencoding::encode(&repo_id),
                pr_id
            );
            let Ok(data) = self.get_json(url).await else { continue };
            for r in data["value"].as_array().cloned().unwrap_or_default() {
                // ResourceRef ids arrive as strings.
                let Some(wi) = r["id"].as_str().and_then(|s| s.parse::<i32>().ok()) else {
                    continue;
                };
                links.push(PrLink {
                    work_item_id: wi,
                    pr_id,
                    status: status.clone(),
                    title: title.clone(),
                    repo: repo_name.clone(),
                    web_url: format!(
                        "{}/{}/{}/_git/{}/pullrequest/{}",
                        self.base_url,
                        org,
                        project,
                        urlencoding::encode(&repo_name),
                        pr_id
                    ),
                });
            }
        }
        Ok(links)
    }

    /// The work items linked to one PR, with the fields DevOps shows on its
    /// "Related Work Items" chips (type, title, state + state colour).
    /// `repo` is the repository name or id (ADO git endpoints accept
    /// either). Read only.
    pub async fn pr_work_items(
        &self,
        org: &str,
        project: &str,
        repo: &str,
        pr_id: i32,
    ) -> Result<Vec<PrWorkItem>, AdoError> {
        let refs_url = format!(
            "{}/{}/{}/_apis/git/repositories/{}/pullRequests/{}/workitems?api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo),
            pr_id
        );
        let refs = self.get_json(refs_url).await?;
        let ids: Vec<i32> = refs["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| r["id"].as_str().and_then(|s| s.parse().ok()))
            .collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let ids_csv = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let items_url = format!(
            "{}/{}/_apis/wit/workitems?ids={}&fields=System.Id,System.Title,System.WorkItemType,System.State&api-version=7.1",
            self.base_url, org, ids_csv
        );
        let fetched = self.get_json(items_url).await?;

        // State colours come from each type's process states (one call per
        // distinct type - usually just Bug). Best-effort: a failed lookup
        // just leaves the dot uncoloured.
        let mut colors: std::collections::HashMap<(String, String), String> =
            std::collections::HashMap::new();
        let mut seen_types = std::collections::HashSet::new();
        let raw: Vec<serde_json::Value> =
            fetched["value"].as_array().cloned().unwrap_or_default();
        for w in &raw {
            let wtype = w["fields"]["System.WorkItemType"].as_str().unwrap_or_default().to_string();
            if wtype.is_empty() || !seen_types.insert(wtype.clone()) {
                continue;
            }
            if let Ok(states) = self.get_work_item_states(org, project, &wtype).await {
                for s in states {
                    colors.insert((wtype.clone(), s.name), s.color);
                }
            }
        }

        Ok(raw
            .iter()
            .map(|w| {
                let f = &w["fields"];
                let id = w["id"].as_i64().unwrap_or_default() as i32;
                let wtype = f["System.WorkItemType"].as_str().unwrap_or_default().to_string();
                let state = f["System.State"].as_str().unwrap_or_default().to_string();
                PrWorkItem {
                    state_color: colors.get(&(wtype.clone(), state.clone())).cloned().unwrap_or_default(),
                    id,
                    work_item_type: wtype,
                    title: f["System.Title"].as_str().unwrap_or_default().to_string(),
                    state,
                    url: format!("{}/{}/{}/_workitems/edit/{}", self.base_url, org, project, id),
                }
            })
            .collect())
    }

    /// PRs on one repository, by ADO status ("active" | "completed" |
    /// "abandoned" | "all"). Completed lists are capped - the history is
    /// unbounded and the panel only ever shows recent ones. Read only.
    pub async fn repo_pull_requests(
        &self,
        org: &str,
        project: &str,
        repo_id: &str,
        status: &str,
    ) -> Result<Vec<PullRequest>, AdoError> {
        // Best-effort identity for my_vote highlighting; anonymous fallback
        // just means my_vote stays 0.
        let me = self.my_identity_id(org).await.unwrap_or_default();
        let status = match status {
            "completed" | "abandoned" | "all" => status,
            _ => "active",
        };
        let top = if status == "active" { "" } else { "&$top=50" };
        let url = format!(
            "{}/{}/{}/_apis/git/repositories/{}/pullrequests?searchCriteria.status={}{}&api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo_id),
            status,
            top
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
