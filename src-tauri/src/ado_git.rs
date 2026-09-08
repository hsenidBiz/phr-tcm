//! Pull requests for the Work Manager's PR panel: repositories, "awaiting
//! your review", "mine", active-on-repo lists, and the review threads on a
//! PR.
//!
//! Almost all reads. The single write is `set_pr_thread_status`, which
//! resolves or reopens one comment thread - the one thing the panel would
//! otherwise send you to the browser for. Voting, completing and
//! abandoning stay in Azure DevOps deliberately.
//!
//! No DELETE, ever, and that half IS enforced: `tests/ado.rs` scans this
//! file's source. The read-only half is not enforceable by a scan, so it
//! is a claim in a comment - keep it true by hand.

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

/// One comment in a review thread.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PrComment {
    pub id: i32,
    pub author: String,
    /// The author's avatar URL, or empty.
    pub avatar: String,
    pub content: String,
    /// ISO 8601, as Azure DevOps returns it.
    pub published: String,
    /// True once the author has edited it - Azure DevOps shows this.
    pub edited: bool,
}

/// A review thread: the comment chain plus where it is anchored.
///
/// Azure DevOps mixes SYSTEM threads into the same collection - "voted",
/// "updated the source branch", "linked a work item". Those are activity,
/// not conversation, and this type only ever holds the human ones; see
/// `pr_threads` for the filter and why it is written the way it is.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PrThread {
    pub id: i32,
    /// "active" | "fixed" | "wontFix" | "closed" | "pending" | "byDesign".
    /// Empty when Azure DevOps sends none, which it does for a thread that
    /// has never been resolved either way - those read as active.
    pub status: String,
    /// The file this thread hangs off, or empty for a PR-level comment.
    pub file_path: String,
    /// First line of the anchored range; 0 when there is no range.
    pub line: i32,
    pub comments: Vec<PrComment>,
    /// ISO 8601 of the newest activity, for ordering.
    pub last_updated: String,
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

impl AdoClient {
    /// Page size for repo_pull_requests. The frontend mirrors this to know
    /// when a page is full (i.e. a next page may exist).
    pub const PR_PAGE_SIZE: u32 = 25;
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

    /// The full description of one pull request.
    ///
    /// The LIST endpoint truncates `description` (around 400 characters,
    /// mid-word, with no marker), so the panel has to ask for the pull
    /// request itself before it can show a long one.
    pub async fn pr_description(
        &self,
        org: &str,
        project: &str,
        repo: &str,
        pr_id: i32,
    ) -> Result<String, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/git/repositories/{}/pullRequests/{}?api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo),
            pr_id
        );
        let pr = self.get_json(url).await?;
        Ok(pr["description"].as_str().unwrap_or_default().to_string())
    }

    /// The review conversation on one PR: human comment threads only,
    /// oldest activity first, each with where it is anchored and whether it
    /// is resolved.
    ///
    /// Azure DevOps returns SYSTEM threads from this endpoint too - "voted
    /// approved", "updated the source branch", "linked #123". They are the
    /// activity feed, not the review, and showing them would bury the
    /// comments the panel exists to surface. The filter is on each
    /// comment's `commentType` rather than on the thread, because a thread
    /// can hold both: a system entry can be appended to a real
    /// conversation. So the comments are filtered first and the thread is
    /// dropped only if nothing human is left.
    ///
    /// Deleted comments are skipped the same way - Azure DevOps keeps them
    /// in the payload with `isDeleted` set and the content blanked.
    pub async fn pr_threads(
        &self,
        org: &str,
        project: &str,
        repo: &str,
        pr_id: i32,
    ) -> Result<Vec<PrThread>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/git/repositories/{}/pullRequests/{}/threads?api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo),
            pr_id
        );
        let body = self.get_json(url).await?;
        let mut out: Vec<PrThread> = vec![];
        for t in body["value"].as_array().cloned().unwrap_or_default() {
            if t["isDeleted"].as_bool() == Some(true) {
                continue;
            }
            let comments: Vec<PrComment> = t["comments"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter(|c| {
                    c["isDeleted"].as_bool() != Some(true)
                        // Absent commentType means a normal comment; only
                        // an explicit "system" is activity.
                        && c["commentType"].as_str().unwrap_or("text") != "system"
                })
                .map(|c| PrComment {
                    id: c["id"].as_i64().unwrap_or(0) as i32,
                    author: c["author"]["displayName"].as_str().unwrap_or("").to_string(),
                    avatar: c["author"]["imageUrl"].as_str().unwrap_or("").to_string(),
                    content: c["content"].as_str().unwrap_or("").to_string(),
                    published: c["publishedDate"].as_str().unwrap_or("").to_string(),
                    edited: c["lastContentUpdatedDate"].as_str().is_some_and(|u| {
                        c["publishedDate"].as_str().is_some_and(|p| u != p)
                    }),
                })
                .collect();
            if comments.is_empty() {
                continue;
            }
            let ctx = &t["threadContext"];
            out.push(PrThread {
                id: t["id"].as_i64().unwrap_or(0) as i32,
                status: t["status"].as_str().unwrap_or("").to_string(),
                file_path: ctx["filePath"].as_str().unwrap_or("").to_string(),
                // rightFileStart is the line in the PR's version; a thread
                // on deleted code only has leftFileStart.
                line: ctx["rightFileStart"]["line"]
                    .as_i64()
                    .or_else(|| ctx["leftFileStart"]["line"].as_i64())
                    .unwrap_or(0) as i32,
                comments,
                last_updated: t["lastUpdatedDate"].as_str().unwrap_or("").to_string(),
            });
        }
        out.sort_by(|a, b| a.last_updated.cmp(&b.last_updated));
        Ok(out)
    }

    /// Resolve a review thread, or put it back to active.
    ///
    /// This is the ONE write the pull-request panel makes. Everything else
    /// there is read-only and stays that way: voting, completing, replying
    /// and abandoning all remain in Azure DevOps. A thread status is the
    /// exception because it is the half of reviewing that is bookkeeping
    /// rather than judgement, it is reversible from inside this app, and it
    /// carries no content of its own.
    ///
    /// `status` is passed through to Azure DevOps and must be one of its
    /// thread statuses; the caller is responsible for that, and the command
    /// layer checks it against a fixed list rather than trusting the UI.
    pub async fn set_pr_thread_status(
        &self,
        org: &str,
        project: &str,
        repo: &str,
        pr_id: i32,
        thread_id: i32,
        status: &str,
    ) -> Result<String, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/git/repositories/{}/pullRequests/{}/threads/{}?api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo),
            pr_id,
            thread_id
        );
        let saved = self
            .patch_plain_json(url, &serde_json::json!({ "status": status }))
            .await?;
        // Report what Azure DevOps actually stored, not what we asked for -
        // the same rule set_state follows, so the UI can never claim a
        // state the server declined.
        Ok(saved["status"].as_str().unwrap_or(status).to_string())
    }

    /// One page (PR_PAGE_SIZE) of a repository's PRs, by ADO status
    /// ("active" | "completed" | "abandoned" | "all") and `skip` offset.
    /// Paged on purpose: completed history is unbounded, and even active
    /// lists on a busy repo don't need to arrive all at once. A page
    /// shorter than PR_PAGE_SIZE means there is no next page. Read only.
    pub async fn repo_pull_requests(
        &self,
        org: &str,
        project: &str,
        repo_id: &str,
        status: &str,
        skip: u32,
    ) -> Result<Vec<PullRequest>, AdoError> {
        // Best-effort identity for my_vote highlighting; anonymous fallback
        // just means my_vote stays 0.
        let me = self.my_identity_id(org).await.unwrap_or_default();
        let status = match status {
            "completed" | "abandoned" | "all" => status,
            _ => "active",
        };
        let url = format!(
            "{}/{}/{}/_apis/git/repositories/{}/pullrequests?searchCriteria.status={}&$top={}&$skip={}&api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(repo_id),
            status,
            Self::PR_PAGE_SIZE,
            skip
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
