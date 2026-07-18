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
    let prs = client.repo_pull_requests("org", "proj", "repo-guid").await.unwrap();
    assert!(prs[0].has_conflicts);
    assert!(prs[0].is_draft);

    let repos = client.list_repos("org", "proj").await.unwrap();
    assert_eq!(
        repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["Alpha", "zeta"]
    );
}
