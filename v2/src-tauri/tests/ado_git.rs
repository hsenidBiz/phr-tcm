use v2_lib::ado::AdoClient;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pr_json(id: i32, my_vote: i32) -> serde_json::Value {
    serde_json::json!({
        "pullRequestId": id,
        "title": format!("PR {id}"),
        "isDraft": false,
        "creationDate": "2026-07-18T01:00:00Z",
        "mergeStatus": "succeeded",
        "description": "What and why",
        "createdBy": {"displayName": "Someone"},
        "sourceRefName": "refs/heads/feature/x",
        "targetRefName": "refs/heads/main",
        "repository": {"id": "repo-guid", "name": "web app"},
        "reviewers": [
            {"id": "me-guid", "displayName": "Avin", "vote": my_vote},
            {"id": "other", "displayName": "Sam", "vote": 10}
        ]
    })
}

fn mount_connection_data(server: &MockServer) -> impl std::future::Future<Output = ()> + '_ {
    Mock::given(method("GET"))
        .and(path("/org/_apis/connectionData"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authenticatedUser": {"id": "me-guid"}
        })))
        .mount(server)
}

#[tokio::test]
async fn pr_overview_filters_awaiting_to_unvoted_and_parses_fields() {
    let server = MockServer::start().await;
    mount_connection_data(&server).await;
    // Reviewer query: two PRs, one already voted on (10) -> only the
    // unvoted one lands in `awaiting`.
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/pullrequests"))
        .and(query_param("searchCriteria.reviewerId", "me-guid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [pr_json(1, 0), pr_json(2, 10)]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/pullrequests"))
        .and(query_param("searchCriteria.creatorId", "me-guid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [pr_json(3, -5)]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let o = client.pr_overview("org", "proj").await.unwrap();

    assert_eq!(o.awaiting.len(), 1);
    assert_eq!(o.awaiting[0].id, 1);
    assert_eq!(o.mine.len(), 1);
    let pr = &o.mine[0];
    assert_eq!(pr.id, 3);
    assert_eq!(pr.source_branch, "feature/x");
    assert_eq!(pr.target_branch, "main");
    assert_eq!(pr.my_vote, -5);
    assert_eq!(pr.description, "What and why");
    assert_eq!(pr.reviewers.len(), 2);
    assert!(!pr.has_conflicts);
    // Deterministic web URL, repo name encoded.
    assert!(pr.web_url.ends_with("/org/proj/_git/web%20app/pullrequest/3"));
}

#[tokio::test]
async fn repo_pull_requests_flags_conflicts_and_lists_repos_sorted() {
    let server = MockServer::start().await;
    mount_connection_data(&server).await;
    let mut conflicted = pr_json(9, 0);
    conflicted["mergeStatus"] = serde_json::json!("conflicts");
    conflicted["isDraft"] = serde_json::json!(true);
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/repositories/repo-guid/pullrequests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [conflicted]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/repositories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": "b", "name": "zeta"},
                {"id": "a", "name": "Alpha"}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let prs = client.repo_pull_requests("org", "proj", "repo-guid", "active").await.unwrap();
    assert!(prs[0].has_conflicts);
    assert!(prs[0].is_draft);

    let repos = client.list_repos("org", "proj").await.unwrap();
    assert_eq!(
        repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["Alpha", "zeta"]
    );
}

#[tokio::test]
async fn board_pr_links_map_work_items_across_statuses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/pullrequests"))
        .and(query_param("searchCriteria.status", "active"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "pullRequestId": 7, "title": "Fix login",
                "repository": {"id": "r1", "name": "web"}
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/pullrequests"))
        .and(query_param("searchCriteria.status", "completed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "pullRequestId": 5, "title": "Old work",
                "repository": {"id": "r1", "name": "web"}
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/repositories/r1/pullRequests/7/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": "101"}, {"id": "102"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/repositories/r1/pullRequests/5/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": "101"}]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let links = client.board_pr_links("org", "proj").await.unwrap();
    assert_eq!(links.len(), 3);
    // Work item 101 carries both an active and a completed PR.
    let for_101: Vec<_> = links.iter().filter(|l| l.work_item_id == 101).collect();
    assert_eq!(for_101.len(), 2);
    assert!(for_101.iter().any(|l| l.status == "active" && l.pr_id == 7));
    assert!(for_101.iter().any(|l| l.status == "completed" && l.pr_id == 5));
    assert!(links.iter().all(|l| l.repo == "web"));
    assert!(links[0].web_url.contains("/_git/web/pullrequest/"));
}

#[tokio::test]
async fn pr_work_items_render_like_devops() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/repositories/web/pullRequests/20/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": "143783"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 143783,
                "fields": {
                    "System.Title": "Participants - department inconsistencies",
                    "System.WorkItemType": "Bug",
                    "System.State": "In Progress"
                }
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitemtypes/Bug/states"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"name": "In Progress", "color": "007acc", "category": "InProgress"}]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let items = client.pr_work_items("org", "proj", "web", 20).await.unwrap();
    assert_eq!(items.len(), 1);
    let wi = &items[0];
    assert_eq!(wi.id, 143783);
    assert_eq!(wi.work_item_type, "Bug");
    assert_eq!(wi.state, "In Progress");
    assert_eq!(wi.state_color, "007acc"); // resolved from the type's states
    assert!(wi.url.ends_with("/org/proj/_workitems/edit/143783"));
}
