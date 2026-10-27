//! Pull requests for the Work Manager's PR panel: repositories, "awaiting
//! your review", "mine", active-on-repo lists, and the review threads on a
//! PR.
//!
//! Almost all reads. The single write is `set_pr_thread_status`, which
//! resolves or reopens one comment thread - the one thing the panel would
//! otherwise send you to the browser for. Voting, completing and
//! abandoning stay in Azure DevOps deliberately.
//!
//! No DELETE, ever, and that half IS enforced: `tests/suite/ado.rs` scans this
//! file's source. The read-only half is not enforceable by a scan, so it
//! is a claim in a comment - keep it true by hand.

use crate::ado::fanout::{self, ProjectCoverage};
use crate::ado::{AdoClient, AdoError};
use crate::cache::{self, keys};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RepoRef {
    pub id: String,
    pub name: String,
    /// The project that owns the repository: what Azure DevOps names, or the
    /// project asked when a per-project answer names none.
    pub project: String,
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
    /// The project that owns the repository: what Azure DevOps names under
    /// `repository.project`, or the project asked when the answer names
    /// none. The detail reads and the thread write of a combined list go
    /// through it.
    pub project: String,
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

/// The same two groups across every project of the organization.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PrOverviewAll {
    pub awaiting: Vec<PullRequest>,
    pub mine: Vec<PullRequest>,
    /// How many projects were read, when they were read one by one. An
    /// organization-wide listing reports `0 of 0` (nothing was left out).
    pub coverage: ProjectCoverage,
}

/// A pull request as the board's chips need it: enough to read its work
/// items and link back to it.
struct LinkedPr {
    id: i32,
    status: String,
    title: String,
    repo_id: String,
    repo_name: String,
    project: String,
    /// When it closed (ISO 8601), empty for one still active.
    closed: String,
}

impl LinkedPr {
    fn from_json(v: &serde_json::Value, status: &str, fallback_project: &str) -> Self {
        let s = |val: &serde_json::Value| val.as_str().unwrap_or_default().to_string();
        LinkedPr {
            id: v["pullRequestId"].as_i64().unwrap_or_default() as i32,
            status: status.to_string(),
            title: s(&v["title"]),
            repo_id: s(&v["repository"]["id"]),
            repo_name: s(&v["repository"]["name"]),
            project: named_project(&v["repository"]["project"], fallback_project),
            closed: s(&v["closedDate"]),
        }
    }
}

/// How many completed pull requests the board reads work items for.
const COMPLETED_PRS_ON_BOARD: usize = 25;

/// The entries of an Azure DevOps list answer.
fn values_of(data: &serde_json::Value) -> Vec<serde_json::Value> {
    data["value"].as_array().cloned().unwrap_or_default()
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
    /// The author's identity id, the one `connected_user` returns - how the
    /// mention scan tells your own comments apart. Empty when Azure DevOps
    /// sent none.
    pub author_id: String,
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
    /// The project the pull request's repository belongs to.
    pub project: String,
    pub web_url: String,
}

/// The project a git answer names under `project` (a repository) or
/// `repository.project` (a pull request), or `fallback` when it names none.
fn named_project(project: &serde_json::Value, fallback: &str) -> String {
    match project["name"].as_str() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => fallback.to_string(),
    }
}

/// The web link of a pull request: deterministic, so built rather than
/// asked for. An empty project (an answer that named none, asked for the
/// whole organization) leaves its segment out.
fn pr_web_url(base_url: &str, org: &str, project: &str, repo: &str, id: i32) -> String {
    let repo = urlencoding::encode(repo);
    if project.is_empty() {
        format!("{base_url}/{org}/_git/{repo}/pullrequest/{id}")
    } else {
        format!("{base_url}/{org}/{project}/_git/{repo}/pullrequest/{id}")
    }
}

/// One entry of the repository list, or `None` when it should not be
/// offered. A disabled repository answers every read with a 404, so it is
/// dropped here rather than listed and tracked into an error. The project
/// is the one the entry names, else `fallback_project`.
fn parse_repo(r: &serde_json::Value, fallback_project: &str) -> Option<RepoRef> {
    if r["isDisabled"].as_bool().unwrap_or(false) {
        return None;
    }
    Some(RepoRef {
        id: r["id"].as_str()?.to_string(),
        name: r["name"].as_str().unwrap_or_default().to_string(),
        project: named_project(&r["project"], fallback_project),
    })
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
    let project = named_project(&v["repository"]["project"], project);
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
        web_url: pr_web_url(base_url, org, &project, &repo, id),
        project,
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
            .filter_map(|r| parse_repo(r, project))
            .collect();
        repos.sort_by_key(|r| r.name.to_lowercase());
        Ok(repos)
    }

    /// Every enabled git repository of the organization, in ONE call (the
    /// organization-level route lists them across projects), sorted by
    /// project then name. Read only.
    pub async fn list_repos_all(&self, org: &str) -> Result<Vec<RepoRef>, AdoError> {
        let url = format!("{}/{}/_apis/git/repositories?api-version=7.1", self.base_url, org);
        let data = self.get_json(url).await?;
        let mut repos: Vec<RepoRef> = data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| parse_repo(r, ""))
            .collect();
        repos.sort_by_key(|r| (r.project.to_lowercase(), r.name.to_lowercase()));
        Ok(repos)
    }

    /// The projects that own at least one enabled repository: the only
    /// ones a pull request fan-out has anything to ask. Kept for
    /// `PROJECTS_WITH_REPOS_TTL` (session tier only).
    async fn projects_with_repos(&self, org: &str) -> Result<Vec<String>, AdoError> {
        let key = keys::projects_with_repos(&self.base_url, org);
        if let Some(hit) = cache::session_fresh::<Vec<String>>(&key, keys::PROJECTS_WITH_REPOS_TTL) {
            return Ok(hit);
        }
        let mut projects: Vec<String> = vec![];
        for repo in self.list_repos_all(org).await? {
            if !repo.project.is_empty() && !projects.contains(&repo.project) {
                projects.push(repo.project);
            }
        }
        cache::session_put(&key, projects.clone());
        Ok(projects)
    }

    /// The pull requests Azure DevOps lists for `query` (a query string
    /// without the api-version) across the organization, each with the
    /// project it was asked of (empty when the whole organization was).
    ///
    /// The organization-level list is tried first. An organization that
    /// refuses it (404, 400 or 405) is asked project by project instead,
    /// and only the projects that own an enabled repository; the answer is
    /// remembered for `PR_SCOPE_TTL` so the refusal is not asked again.
    /// Any other error fails the call. The coverage says how many of those
    /// projects were read; the organization-level list reads all of them
    /// in one go and reports `0 of 0`.
    async fn pull_requests_across(
        &self,
        org: &str,
        query: &str,
    ) -> Result<(Vec<(String, serde_json::Value)>, ProjectCoverage), AdoError> {
        let scope_key = keys::pr_scope(&self.base_url, org);
        let remembered = cache::session_fresh::<String>(&scope_key, keys::PR_SCOPE_TTL);
        if remembered.as_deref() != Some("projects") {
            let url = format!(
                "{}/{}/_apis/git/pullrequests?{}&api-version=7.1",
                self.base_url, org, query
            );
            match self.get_json(url).await {
                Ok(data) => {
                    cache::session_put(&scope_key, "org".to_string());
                    let all = values_of(&data).into_iter().map(|v| (String::new(), v)).collect();
                    return Ok((all, ProjectCoverage { shown: 0, total: 0 }));
                }
                Err(AdoError::NotFound) => self.pr_scope_refused(&scope_key, 404),
                Err(AdoError::Http { status: status @ (400 | 405), .. }) => {
                    self.pr_scope_refused(&scope_key, status)
                }
                Err(e) => return Err(e),
            }
        }
        let (projects, total) = fanout::capped(self.projects_with_repos(org).await?);
        let results = fanout::per_project(&projects, |project| {
            let url = format!(
                "{}/{}/{}/_apis/git/pullrequests?{}&api-version=7.1",
                self.base_url, org, project, query
            );
            async move { self.get_json(url).await.map(|data| values_of(&data)) }
        })
        .await;
        let (read, coverage) = fanout::gather(results, total)?;
        let all = read
            .into_iter()
            .flat_map(|(project, values)| values.into_iter().map(move |v| (project.clone(), v)))
            .collect();
        Ok((all, coverage))
    }

    fn pr_scope_refused(&self, scope_key: &str, status: u16) {
        crate::applog::info(format!(
            "pull requests: organization-wide listing refused ({status}); listing per project instead"
        ));
        cache::session_put(scope_key, "projects".to_string());
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
        let mut prs: Vec<LinkedPr> = vec![];
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
            for v in values_of(&data) {
                prs.push(LinkedPr::from_json(&v, status, project));
            }
        }
        Ok(self.links_of(org, prs, Some(project)).await)
    }

    /// The panel's two groups across every project of the organization.
    /// See `pull_requests_across` for how the projects are reached and what
    /// `coverage` says. Read only.
    pub async fn pr_overview_all(&self, org: &str) -> Result<PrOverviewAll, AdoError> {
        let me = self.my_identity_id(org).await?;
        let (reviewing, _) = self
            .pull_requests_across(org, &format!("searchCriteria.status=active&searchCriteria.reviewerId={me}"))
            .await?;
        let (created, coverage) = self
            .pull_requests_across(org, &format!("searchCriteria.status=active&searchCriteria.creatorId={me}"))
            .await?;
        let parse = |(project, v): (String, serde_json::Value)| {
            parse_pr(&v, &self.base_url, org, &project, &me)
        };
        let awaiting = reviewing.into_iter().map(parse).filter(|pr| pr.my_vote == 0).collect();
        let mine = created.into_iter().map(parse).collect();
        Ok(PrOverviewAll { awaiting, mine, coverage })
    }

    /// `board_pr_links` across every project of the organization: the
    /// active pull requests and the 25 most recent completed ones, then each
    /// one's work items through the pull request's own project. The
    /// completed list stays best-effort. When the organization is asked
    /// project by project, each project answers with its own 25, so the
    /// merged list is cut back to the 25 most recently closed before any
    /// work item is read. Read only.
    pub async fn board_pr_links_all(&self, org: &str) -> Result<Vec<PrLink>, AdoError> {
        let mut prs: Vec<LinkedPr> = vec![];
        for (status, query) in [
            ("active", "searchCriteria.status=active"),
            ("completed", "searchCriteria.status=completed&$top=25"),
        ] {
            // A project the person cannot read is logged by the fan-out
            // itself (`fanout::gather`), so the chips leave it out with a
            // trace, not silently.
            let listed = match self.pull_requests_across(org, query).await {
                Ok((listed, _)) => listed,
                Err(e) if status == "active" => return Err(e),
                Err(_) => continue,
            };
            let mut batch: Vec<LinkedPr> = listed
                .into_iter()
                .map(|(project, v)| LinkedPr::from_json(&v, status, &project))
                .collect();
            if status == "completed" {
                // Newest first; the sort is stable, so equal dates keep the
                // order the projects were read in.
                batch.sort_by(|a, b| b.closed.cmp(&a.closed));
                batch.truncate(COMPLETED_PRS_ON_BOARD);
            }
            prs.extend(batch);
        }
        Ok(self.links_of(org, prs, None).await)
    }

    /// Each pull request's linked work items, as board chips. A pull
    /// request whose work items cannot be read - a disabled repository
    /// among them - just has no chips. The work items are read through
    /// `read_project` when one project was asked (the request a project's
    /// board has always made), else through the pull request's own.
    async fn links_of(&self, org: &str, prs: Vec<LinkedPr>, read_project: Option<&str>) -> Vec<PrLink> {
        let mut links = vec![];
        for pr in prs {
            let read_in = read_project.unwrap_or(&pr.project);
            let scope = if read_in.is_empty() {
                org.to_string()
            } else {
                format!("{org}/{read_in}")
            };
            let url = format!(
                "{}/{}/_apis/git/repositories/{}/pullRequests/{}/workitems?api-version=7.1",
                self.base_url,
                scope,
                urlencoding::encode(&pr.repo_id),
                pr.id
            );
            let Ok(data) = self.get_json(url).await else { continue };
            for r in values_of(&data) {
                // ResourceRef ids arrive as strings.
                let Some(wi) = r["id"].as_str().and_then(|s| s.parse::<i32>().ok()) else {
                    continue;
                };
                links.push(PrLink {
                    work_item_id: wi,
                    pr_id: pr.id,
                    status: pr.status.clone(),
                    title: pr.title.clone(),
                    repo: pr.repo_name.clone(),
                    project: pr.project.clone(),
                    web_url: pr_web_url(&self.base_url, org, &pr.project, &pr.repo_name, pr.id),
                });
            }
        }
        links
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
                    author_id: c["author"]["id"].as_str().unwrap_or("").to_string(),
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
