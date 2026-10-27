//! Task 12: repositories and pull requests across projects.
//!
//! Every organization name here is this file's own: the scope answer and the
//! repository-owning projects are remembered for the session, keyed by the
//! mock server's address and the organization, and a finished test's port can
//! come round again in this one process - so each test also empties those two
//! keys before it starts.

use v2_lib::ado::fanout::ProjectCoverage;
use v2_lib::ado::{AdoClient, AdoError};
use v2_lib::cache::{self, keys};
use wiremock::matchers::{method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn forget_memos(server: &MockServer, org: &str) {
    cache::forget(&keys::pr_scope(&server.uri(), org));
    cache::forget(&keys::projects_with_repos(&server.uri(), org));
}

async fn start(org: &str) -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    forget_memos(&server, org);
    let client = AdoClient::with_base_url("t".into(), server.uri());
    (server, client)
}

async fn mount_identity(server: &MockServer, org: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/connectionData")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authenticatedUser": {"id": "me-guid"}
        })))
        .mount(server)
        .await;
}

fn repo(id: &str, name: &str, project: &str, disabled: bool) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "isDisabled": disabled,
        "project": {"name": project},
    })
}

async fn mount_repos(server: &MockServer, org: &str, repos: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/git/repositories")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": repos })))
        .mount(server)
        .await;
}

/// A pull request. `project` is the name Azure DevOps puts under
/// `repository.project`, or `None` for an answer that carries none.
fn pr(id: i32, repo: &str, repo_id: &str, project: Option<&str>, my_vote: i32) -> serde_json::Value {
    let mut repository = serde_json::json!({"id": repo_id, "name": repo});
    if let Some(p) = project {
        repository["project"] = serde_json::json!({"name": p});
    }
    serde_json::json!({
        "pullRequestId": id,
        "title": format!("PR {id}"),
        "status": "active",
        "createdBy": {"displayName": "Someone"},
        "sourceRefName": "refs/heads/feature/x",
        "targetRefName": "refs/heads/main",
        "repository": repository,
        "reviewers": [{"id": "me-guid", "displayName": "Avin", "vote": my_vote}],
    })
}

fn prs_body(list: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({ "value": list })
}

/// The organization-wide pull request list, one answer per search criterion.
async fn mount_org_prs(
    server: &MockServer,
    org: &str,
    reviewing: Vec<serde_json::Value>,
    created: Vec<serde_json::Value>,
) {
    let route = format!("/{org}/_apis/git/pullrequests");
    Mock::given(method("GET"))
        .and(path(route.clone()))
        .and(query_param("searchCriteria.reviewerId", "me-guid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(reviewing)))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(route))
        .and(query_param("searchCriteria.creatorId", "me-guid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(created)))
        .mount(server)
        .await;
}

/// One project's pull request list, one answer per search criterion.
async fn mount_project_prs(
    server: &MockServer,
    org: &str,
    project: &str,
    reviewing: Vec<serde_json::Value>,
    created: Vec<serde_json::Value>,
) {
    let route = format!("/{org}/{project}/_apis/git/pullrequests");
    Mock::given(method("GET"))
        .and(path(route.clone()))
        .and(query_param("searchCriteria.reviewerId", "me-guid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(reviewing)))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(route))
        .and(query_param("searchCriteria.creatorId", "me-guid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(created)))
        .mount(server)
        .await;
}

/// The organization-wide list answering `status` with `code` and no body.
async fn mount_org_refused(server: &MockServer, org: &str, code: u16, times: u64) {
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/git/pullrequests")))
        .respond_with(ResponseTemplate::new(code))
        .expect(times)
        .mount(server)
        .await;
}

/// No request may reach any project's pull request list.
async fn forbid_project_prs(server: &MockServer, org: &str) {
    Mock::given(method("GET"))
        .and(path_regex(format!(r"^/{org}/[^/]+/_apis/git/pullrequests$")))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(server)
        .await;
}

fn ids(list: &[v2_lib::ado_git::PullRequest]) -> Vec<i32> {
    list.iter().map(|p| p.id).collect()
}

// ------------------------------------------------------------ repositories

#[tokio::test]
async fn every_repository_comes_from_one_organization_wide_call() {
    let org = "prs-repos-org";
    let (server, client) = start(org).await;
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/git/repositories")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                repo("r3", "zeta", "Billing", false),
                repo("r2", "Beta", "Payroll", false),
                repo("r9", "old", "Payroll", true),
                repo("r1", "alpha", "Payroll", false),
                repo("r4", "web", "api", false),
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let repos = client.list_repos_all(org).await.unwrap();

    let listed: Vec<(&str, &str)> = repos.iter().map(|r| (r.project.as_str(), r.name.as_str())).collect();
    assert_eq!(
        listed,
        vec![("api", "web"), ("Billing", "zeta"), ("Payroll", "alpha"), ("Payroll", "Beta")],
        "sorted by project then name, and the disabled repository is not listed"
    );
    assert_eq!(repos[0].id, "r4");
    server.verify().await;
}

#[test]
fn the_scope_keys_and_lifetimes_are_the_documented_ones() {
    assert_eq!(keys::pr_scope("https://x", "org"), "pr-scope:https://x|org");
    assert_eq!(keys::PR_SCOPE_TTL, std::time::Duration::from_secs(6 * 60 * 60));
    assert_ne!(keys::projects_with_repos("https://x", "org"), keys::org_projects("https://x", "org"));
}

// ------------------------------------------------------------ the overview

#[tokio::test]
async fn the_overview_uses_organization_scope_when_azure_devops_accepts_it() {
    let org = "prs-orgscope";
    let (server, client) = start(org).await;
    mount_identity(&server, org).await;
    mount_org_prs(
        &server,
        org,
        vec![
            pr(5, "api", "g1", Some("Payroll"), 0),
            pr(6, "web", "g2", Some("Billing"), 10),
        ],
        vec![pr(7, "web", "g2", Some("Billing"), 0)],
    )
    .await;
    forbid_project_prs(&server, org).await;

    let overview = client.pr_overview_all(org).await.unwrap();

    assert_eq!(ids(&overview.awaiting), vec![5], "only the pull request still waiting for a vote");
    assert_eq!(overview.awaiting[0].project, "Payroll");
    assert_eq!(
        overview.awaiting[0].web_url,
        format!("{}/{org}/Payroll/_git/api/pullrequest/5", server.uri())
    );
    assert_eq!(ids(&overview.mine), vec![7]);
    assert_eq!(overview.mine[0].project, "Billing");
    assert_eq!(
        keys_scope(&server, org).as_deref(),
        Some("org"),
        "the accepted scope is remembered"
    );
    server.verify().await;
}

fn keys_scope(server: &MockServer, org: &str) -> Option<String> {
    cache::session_fresh::<String>(&keys::pr_scope(&server.uri(), org), keys::PR_SCOPE_TTL)
}

#[tokio::test]
async fn a_refused_organization_scope_falls_back_to_the_projects_that_own_repositories() {
    // The log line is read back, so no other test may flood the tail meanwhile.
    let _log = crate::serial::log_tail();
    let org = "prs-fallback";
    let (server, client) = start(org).await;
    mount_identity(&server, org).await;
    mount_repos(
        &server,
        org,
        vec![
            repo("g1", "api", "A", false),
            repo("g2", "web", "C", false),
            repo("g3", "old", "B", true),
        ],
    )
    .await;
    mount_org_refused(&server, org, 404, 1).await;
    mount_project_prs(
        &server,
        org,
        "A",
        vec![pr(1, "api", "g1", Some("A"), 0), pr(9, "api", "g1", Some("A"), 5)],
        vec![pr(2, "api", "g1", Some("A"), 0)],
    )
    .await;
    // Project C's answers name no project, so the one asked is the project.
    mount_project_prs(&server, org, "C", vec![], vec![pr(3, "web", "g2", None, 0)]).await;
    // B owns only a disabled repository: it is never asked.
    Mock::given(method("GET"))
        .and(path_regex(format!(r"^/{org}/B/_apis/git/pullrequests$")))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let overview = client.pr_overview_all(org).await.unwrap();

    assert_eq!(ids(&overview.awaiting), vec![1]);
    assert_eq!(ids(&overview.mine), vec![2, 3]);
    assert_eq!(overview.mine[0].project, "A");
    assert_eq!(overview.mine[1].project, "C");
    assert_eq!(
        overview.mine[1].web_url,
        format!("{}/{org}/C/_git/web/pullrequest/3", server.uri())
    );
    assert_eq!(overview.coverage, ProjectCoverage { shown: 2, total: 2 });
    assert_eq!(keys_scope(&server, org).as_deref(), Some("projects"));
    let logged = v2_lib::applog::recent(6000)
        .into_iter()
        .any(|l| l.message == "pull requests: organization-wide listing refused (404); listing per project instead");
    assert!(logged, "the refusal is written to the log");
    server.verify().await;
}

#[tokio::test]
async fn a_400_or_a_405_also_means_list_per_project() {
    for (n, code) in [400u16, 405].into_iter().enumerate() {
        let org = format!("prs-refused-{n}");
        let (server, client) = start(&org).await;
        mount_identity(&server, &org).await;
        mount_repos(&server, &org, vec![repo("g1", "api", "A", false)]).await;
        mount_org_refused(&server, &org, code, 1).await;
        mount_project_prs(&server, &org, "A", vec![pr(1, "api", "g1", Some("A"), 0)], vec![]).await;

        let overview = client.pr_overview_all(&org).await.unwrap();

        assert_eq!(ids(&overview.awaiting), vec![1], "status {code}");
        assert_eq!(keys_scope(&server, &org).as_deref(), Some("projects"), "status {code}");
        server.verify().await;
    }
}

#[tokio::test]
async fn any_other_refusal_of_organization_scope_fails_the_call_and_is_not_remembered() {
    let org = "prs-forbidden";
    let (server, client) = start(org).await;
    mount_identity(&server, org).await;
    mount_org_refused(&server, org, 403, 1).await;
    forbid_project_prs(&server, org).await;

    let err = client.pr_overview_all(org).await.unwrap_err();

    assert!(matches!(err, AdoError::Forbidden), "{err:?}");
    assert_eq!(keys_scope(&server, org), None);
    server.verify().await;
}

#[tokio::test]
async fn the_scope_answer_is_remembered_for_the_session() {
    let org = "prs-remembered";
    let (server, client) = start(org).await;
    mount_identity(&server, org).await;
    // Read once: the repository list is remembered too.
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/git/repositories")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "value": [repo("g1", "api", "A", false)] })),
        )
        .expect(1)
        .mount(&server)
        .await;
    // The refusal is asked once, however many times the overview is read.
    mount_org_refused(&server, org, 404, 1).await;
    mount_project_prs(&server, org, "A", vec![pr(1, "api", "g1", Some("A"), 0)], vec![]).await;

    let first = client.pr_overview_all(org).await.unwrap();
    let second = client.pr_overview_all(org).await.unwrap();

    assert_eq!(ids(&first.awaiting), vec![1]);
    assert_eq!(ids(&second.awaiting), vec![1]);
    server.verify().await;
}

#[tokio::test]
async fn a_project_the_person_cannot_read_is_skipped_and_counted() {
    let org = "prs-skipped";
    let (server, client) = start(org).await;
    mount_identity(&server, org).await;
    mount_repos(
        &server,
        org,
        vec![repo("g1", "api", "A", false), repo("g2", "web", "C", false)],
    )
    .await;
    mount_org_refused(&server, org, 404, 1).await;
    mount_project_prs(&server, org, "A", vec![pr(1, "api", "g1", Some("A"), 0)], vec![]).await;
    Mock::given(method("GET"))
        .and(path(format!("/{org}/C/_apis/git/pullrequests")))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let overview = client.pr_overview_all(org).await.unwrap();

    assert_eq!(ids(&overview.awaiting), vec![1]);
    assert_eq!(overview.coverage, ProjectCoverage { shown: 1, total: 2 });
}

// ------------------------------------------------------------ the argument project

#[tokio::test]
async fn per_project_pull_requests_carry_the_argument_project_without_a_repository_project() {
    let org = "prs-argument";
    let (server, client) = start(org).await;
    mount_identity(&server, org).await;
    mount_project_prs(
        &server,
        org,
        "proj",
        vec![pr(1, "api", "g1", None, 0)],
        vec![pr(2, "api", "g1", Some("Named"), 0)],
    )
    .await;
    Mock::given(method("GET"))
        .and(path(format!("/{org}/proj/_apis/git/repositories")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": "g1", "name": "api"},
                repo("g2", "web", "Named", false),
                repo("g3", "gone", "proj", true),
            ]
        })))
        .mount(&server)
        .await;

    let overview = client.pr_overview(org, "proj").await.unwrap();
    assert_eq!(overview.awaiting[0].project, "proj");
    assert_eq!(
        overview.awaiting[0].web_url,
        format!("{}/{org}/proj/_git/api/pullrequest/1", server.uri())
    );
    assert_eq!(overview.mine[0].project, "Named");

    let repos = client.list_repos(org, "proj").await.unwrap();
    let listed: Vec<(&str, &str)> = repos.iter().map(|r| (r.name.as_str(), r.project.as_str())).collect();
    assert_eq!(listed, vec![("api", "proj"), ("web", "Named")]);
}

// ------------------------------------------------------------ board links

fn work_item_refs(ids: &[&str]) -> serde_json::Value {
    let value: Vec<_> = ids.iter().map(|i| serde_json::json!({ "id": i })).collect();
    serde_json::json!({ "value": value })
}

async fn mount_status_list(
    server: &MockServer,
    org: &str,
    status: &str,
    answer: ResponseTemplate,
) {
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/git/pullrequests")))
        .and(query_param("searchCriteria.status", status))
        .respond_with(answer)
        .mount(server)
        .await;
}

#[tokio::test]
async fn board_links_name_each_pull_requests_own_project() {
    let org = "prs-links";
    let (server, client) = start(org).await;
    mount_status_list(
        &server,
        org,
        "active",
        ResponseTemplate::new(200).set_body_json(prs_body(vec![
            pr(5, "api", "g1", Some("Payroll"), 0),
            pr(6, "web app", "g2", Some("Billing"), 0),
        ])),
    )
    .await;
    mount_status_list(
        &server,
        org,
        "completed",
        ResponseTemplate::new(200)
            .set_body_json(prs_body(vec![pr(7, "api", "g1", Some("Payroll"), 0)])),
    )
    .await;
    // Each work-item read goes through the pull request's own project.
    for (project, repo_id, id, items) in [
        ("Payroll", "g1", 5, vec!["100"]),
        ("Billing", "g2", 6, vec!["200"]),
        ("Payroll", "g1", 7, vec!["101"]),
    ] {
        Mock::given(method("GET"))
            .and(path(format!(
                "/{org}/{project}/_apis/git/repositories/{repo_id}/pullRequests/{id}/workitems"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(work_item_refs(&items)))
            .expect(1)
            .mount(&server)
            .await;
    }
    forbid_project_prs(&server, org).await;

    let links = client.board_pr_links_all(org).await.unwrap();

    let shape: Vec<(i32, i32, &str, &str)> = links
        .iter()
        .map(|l| (l.work_item_id, l.pr_id, l.status.as_str(), l.project.as_str()))
        .collect();
    assert_eq!(
        shape,
        vec![
            (100, 5, "active", "Payroll"),
            (200, 6, "active", "Billing"),
            (101, 7, "completed", "Payroll"),
        ]
    );
    assert_eq!(
        links[1].web_url,
        format!("{}/{org}/Billing/_git/web%20app/pullrequest/6", server.uri())
    );
    server.verify().await;
}

#[tokio::test]
async fn board_links_skip_a_pull_request_whose_work_items_cannot_be_read_and_a_failed_completed_list() {
    let org = "prs-links-skip";
    let (server, client) = start(org).await;
    mount_status_list(
        &server,
        org,
        "active",
        ResponseTemplate::new(200).set_body_json(prs_body(vec![
            pr(5, "api", "g1", Some("Payroll"), 0),
            pr(6, "gone", "g2", Some("Billing"), 0),
        ])),
    )
    .await;
    mount_status_list(&server, org, "completed", ResponseTemplate::new(403)).await;
    Mock::given(method("GET"))
        .and(path(format!("/{org}/Payroll/_apis/git/repositories/g1/pullRequests/5/workitems")))
        .respond_with(ResponseTemplate::new(200).set_body_json(work_item_refs(&["100"])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{org}/Billing/_apis/git/repositories/g2/pullRequests/6/workitems")))
        .respond_with(ResponseTemplate::new(404).set_body_string(
            "{\"message\":\"TF401019: The Git repository with name or identifier g2 does not exist\"}",
        ))
        .mount(&server)
        .await;

    let links = client.board_pr_links_all(org).await.unwrap();

    assert_eq!(links.len(), 1);
    assert_eq!((links[0].work_item_id, links[0].pr_id), (100, 5));
}

#[tokio::test]
async fn board_links_list_per_project_when_organization_scope_is_refused() {
    let org = "prs-links-fallback";
    let (server, client) = start(org).await;
    mount_repos(&server, org, vec![repo("g1", "api", "A", false), repo("g3", "off", "B", true)]).await;
    mount_org_refused(&server, org, 404, 1).await;
    // No project in the answer: the project asked is the project.
    Mock::given(method("GET"))
        .and(path(format!("/{org}/A/_apis/git/pullrequests")))
        .and(query_param("searchCriteria.status", "active"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(vec![pr(5, "api", "g1", None, 0)])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{org}/A/_apis/git/pullrequests")))
        .and(query_param("searchCriteria.status", "completed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(vec![])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{org}/A/_apis/git/repositories/g1/pullRequests/5/workitems")))
        .respond_with(ResponseTemplate::new(200).set_body_json(work_item_refs(&["100"])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(format!(r"^/{org}/B/")))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let links = client.board_pr_links_all(org).await.unwrap();

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].project, "A");
    assert_eq!(
        links[0].web_url,
        format!("{}/{org}/A/_git/api/pullrequest/5", server.uri())
    );
    server.verify().await;
}

#[tokio::test]
async fn per_project_board_links_carry_the_argument_project() {
    let org = "prs-links-arg";
    let (server, client) = start(org).await;
    for (status, list) in [
        ("active", vec![pr(5, "api", "g1", None, 0)]),
        ("completed", vec![]),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/{org}/proj/_apis/git/pullrequests")))
            .and(query_param("searchCriteria.status", status))
            .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(list)))
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(format!("/{org}/proj/_apis/git/repositories/g1/pullRequests/5/workitems")))
        .respond_with(ResponseTemplate::new(200).set_body_json(work_item_refs(&["100"])))
        .mount(&server)
        .await;

    let links = client.board_pr_links(org, "proj").await.unwrap();

    assert_eq!(links[0].project, "proj");
    assert_eq!(links[0].web_url, format!("{}/{org}/proj/_git/api/pullrequest/5", server.uri()));
}

/// A completed pull request that closed on `day` of January 2026.
fn closed_pr(id: i32, repo_id: &str, project: &str, day: u32) -> serde_json::Value {
    let mut v = pr(id, "api", repo_id, Some(project), 0);
    v["status"] = serde_json::json!("completed");
    v["closedDate"] = serde_json::json!(format!("2026-01-{day:02}T10:00:00Z"));
    v
}

#[tokio::test]
async fn board_links_on_the_per_project_path_keep_the_25_most_recently_closed_in_total() {
    let org = "prs-links-top25";
    let (server, client) = start(org).await;
    mount_repos(
        &server,
        org,
        vec![
            repo("ga", "api", "A", false),
            repo("gb", "api", "B", false),
            repo("gc", "api", "C", false),
        ],
    )
    .await;
    mount_org_refused(&server, org, 404, 1).await;
    // Three projects with 20 completed pull requests each. Project P's
    // pull requests have ids P*100+1..=P*100+20 and close on days 1..=20,
    // so the 25 newest are the days 20 down to 13 of all three and one of day 12.
    let projects = [("A", "ga", 1), ("B", "gb", 2), ("C", "gc", 3)];
    for (project, repo_id, n) in projects {
        Mock::given(method("GET"))
            .and(path(format!("/{org}/{project}/_apis/git/pullrequests")))
            .and(query_param("searchCriteria.status", "active"))
            .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(vec![])))
            .mount(&server)
            .await;
        let completed: Vec<_> = (1..=20).map(|d| closed_pr(n * 100 + d as i32, repo_id, project, d)).collect();
        Mock::given(method("GET"))
            .and(path(format!("/{org}/{project}/_apis/git/pullrequests")))
            .and(query_param("searchCriteria.status", "completed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(completed)))
            .mount(&server)
            .await;
    }
    // Every work-item read is counted; each pull request has one item.
    Mock::given(method("GET"))
        .and(path_regex(format!(r"^/{org}/[ABC]/_apis/git/repositories/g[abc]/pullRequests/\d+/workitems$")))
        .respond_with(ResponseTemplate::new(200).set_body_json(work_item_refs(&["900"])))
        .expect(25)
        .mount(&server)
        .await;

    let links = client.board_pr_links_all(org).await.unwrap();

    assert_eq!(links.len(), 25, "25 completed pull requests in total, not 25 per project");
    // Days 20 down to 13 are 8 days x 3 projects = 24, and one more from
    // day 12 fills the 25. Nothing older is read.
    let day = |id: i32| id % 100;
    let days: Vec<i32> = links.iter().map(|l| day(l.pr_id)).collect();
    assert_eq!(days.iter().filter(|d| **d >= 13).count(), 24, "{days:?}");
    assert_eq!(days.iter().filter(|d| **d == 12).count(), 1, "{days:?}");
    server.verify().await;
}

#[tokio::test]
async fn a_remembered_organization_scope_is_decided_again_when_a_later_call_is_refused() {
    let org = "prs-org-then-refused";
    let (server, client) = start(org).await;
    mount_identity(&server, org).await;
    mount_repos(&server, org, vec![repo("g1", "api", "A", false)]).await;
    // An earlier call was answered organization-wide and remembered so.
    cache::session_put(&keys::pr_scope(&server.uri(), org), "org".to_string());
    assert_eq!(keys_scope(&server, org).as_deref(), Some("org"));
    // Now the organization list is refused: one ask per search criterion
    // (the reviewer list, then the creator list - the second one already
    // goes by project, so the refusal is asked only once).
    mount_org_refused(&server, org, 404, 1).await;
    mount_project_prs(
        &server,
        org,
        "A",
        vec![pr(1, "api", "g1", Some("A"), 0)],
        vec![pr(2, "api", "g1", Some("A"), 0)],
    )
    .await;

    let overview = client.pr_overview_all(org).await.unwrap();

    assert_eq!(ids(&overview.awaiting), vec![1]);
    assert_eq!(ids(&overview.mine), vec![2]);
    assert_eq!(overview.coverage, ProjectCoverage { shown: 1, total: 1 });
    assert_eq!(keys_scope(&server, org).as_deref(), Some("projects"));
    server.verify().await;
}

#[tokio::test]
async fn the_boards_pull_request_chips_log_a_project_they_cannot_read() {
    // The log line is read back, so no other test may flood the tail meanwhile.
    let _log = crate::serial::log_tail();
    let org = "prs-links-forbidden";
    let (server, client) = start(org).await;
    mount_repos(
        &server,
        org,
        vec![repo("ga", "api", "A", false), repo("gb", "web", "Hidden", false)],
    )
    .await;
    mount_org_refused(&server, org, 404, 1).await;
    for status in ["active", "completed"] {
        Mock::given(method("GET"))
            .and(path(format!("/{org}/A/_apis/git/pullrequests")))
            .and(query_param("searchCriteria.status", status))
            .respond_with(ResponseTemplate::new(200).set_body_json(prs_body(if status == "active" {
                vec![pr(5, "api", "ga", Some("A"), 0)]
            } else {
                vec![]
            })))
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(format!("/{org}/Hidden/_apis/git/pullrequests")))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{org}/A/_apis/git/repositories/ga/pullRequests/5/workitems")))
        .respond_with(ResponseTemplate::new(200).set_body_json(work_item_refs(&["100"])))
        .mount(&server)
        .await;

    let links = client.board_pr_links_all(org).await.unwrap();

    assert_eq!(links.len(), 1);
    let logged = v2_lib::applog::recent(6000)
        .into_iter()
        .find(|l| l.message.contains("skipped Hidden"));
    let logged = logged.expect("the skipped project is written to the log");
    assert!(!logged.message.contains("http"), "no URL in the log line: {}", logged.message);
}
