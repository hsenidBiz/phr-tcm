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
    let prs = client.repo_pull_requests("org", "proj", "repo-guid", "active", 0).await.unwrap();
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

/// Azure DevOps returns the review conversation and its own activity feed
/// from the SAME endpoint. "voted", "updated the source branch" and the
/// like are system entries, and showing them would bury the comments the
/// panel exists to surface - so they are filtered out, and a thread left
/// with nothing human in it is dropped entirely.
///
/// The filter is per COMMENT, not per thread, because a system entry can be
/// appended to a real conversation: filtering by thread would throw away
/// the discussion along with the noise.
#[tokio::test]
async fn pr_threads_keeps_the_conversation_and_drops_the_activity_feed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/git/repositories/demo-web/pullRequests/7/threads"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            // Pure activity - no human comment survives, so no thread.
            { "id": 1, "status": "closed", "lastUpdatedDate": "2026-07-01T00:00:00Z",
              "comments": [ { "id": 1, "commentType": "system", "content": "Sam voted 10",
                              "author": { "displayName": "Sam" } } ] },
            // A real thread that ALSO collected a system entry.
            { "id": 2, "status": "active", "lastUpdatedDate": "2026-07-03T00:00:00Z",
              "threadContext": { "filePath": "/src/a.ts", "rightFileStart": { "line": 42 } },
              "comments": [
                { "id": 2, "content": "Can we surface this error?", "publishedDate": "2026-07-02T00:00:00Z",
                  "author": { "displayName": "Priya", "imageUrl": "http://img/priya" } },
                { "id": 3, "commentType": "system", "content": "Priya updated the source branch",
                  "author": { "displayName": "Priya" } },
                { "id": 4, "content": "Fixed.", "publishedDate": "2026-07-03T00:00:00Z",
                  "lastContentUpdatedDate": "2026-07-03T01:00:00Z",
                  "author": { "displayName": "Sam" } } ] },
            // Deleted threads and deleted comments both vanish.
            { "id": 3, "isDeleted": true, "lastUpdatedDate": "2026-07-04T00:00:00Z",
              "comments": [ { "id": 5, "content": "gone", "author": { "displayName": "Sam" } } ] },
            { "id": 4, "lastUpdatedDate": "2026-07-05T00:00:00Z",
              "comments": [ { "id": 6, "isDeleted": true, "content": "",
                              "author": { "displayName": "Sam" } } ] },
            // No threadContext at all: a pull-request-level comment.
            { "id": 5, "status": "fixed", "lastUpdatedDate": "2026-07-02T00:00:00Z",
              "comments": [ { "id": 7, "content": "Changelog?", "publishedDate": "2026-07-02T00:00:00Z",
                              "author": { "displayName": "Priya" } } ] },
        ]})))
        .mount(&server)
        .await;

    let out = AdoClient::with_base_url("t".into(), server.uri())
        .pr_threads("o", "p", "demo-web", 7)
        .await
        .unwrap();

    let ids: Vec<i32> = out.iter().map(|t| t.id).collect();
    assert_eq!(ids, vec![5, 2], "oldest activity first; 1, 3 and 4 carry nothing human");

    let general = &out[0];
    assert_eq!(general.file_path, "", "a PR-level comment has no file");
    assert_eq!(general.line, 0);
    assert_eq!(general.status, "fixed");

    let anchored = &out[1];
    assert_eq!(anchored.file_path, "/src/a.ts");
    assert_eq!(anchored.line, 42);
    assert_eq!(anchored.comments.len(), 2, "the system entry between them is gone");
    assert_eq!(anchored.comments[0].author, "Priya");
    assert_eq!(anchored.comments[0].avatar, "http://img/priya");
    assert!(!anchored.comments[0].edited);
    assert!(anchored.comments[1].edited, "a later content update marks it edited");
}

/// A thread on code that was DELETED in the pull request has no
/// rightFileStart - only a left one. Falling back matters, or the comment
/// renders as if it were unanchored.
#[tokio::test]
async fn a_thread_on_removed_code_still_reports_its_line() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/git/repositories/r/pullRequests/1/threads"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            { "id": 1, "status": "active", "lastUpdatedDate": "2026-07-01T00:00:00Z",
              "threadContext": { "filePath": "/old.ts", "leftFileStart": { "line": 88 } },
              "comments": [ { "id": 1, "content": "why was this removed?",
                              "author": { "displayName": "Priya" } } ] },
        ]})))
        .mount(&server)
        .await;

    let out = AdoClient::with_base_url("t".into(), server.uri())
        .pr_threads("o", "p", "r", 1)
        .await
        .unwrap();
    assert_eq!(out[0].line, 88);
}

/// The panel's ONE write. It must be a PATCH carrying only a status, and it
/// must report back what Azure DevOps stored rather than what was asked
/// for - so the UI can never claim a state the server declined.
#[tokio::test]
async fn resolving_a_thread_patches_only_the_status_and_reports_what_stuck() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/o/p/_apis/git/repositories/r/pullRequests/1/threads/9"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 9, "status": "closed" })),
        )
        .mount(&server)
        .await;

    let saved = AdoClient::with_base_url("t".into(), server.uri())
        .set_pr_thread_status("o", "p", "r", 1, 9, "fixed")
        .await
        .unwrap();
    assert_eq!(saved, "closed", "the server's answer wins over the request");

    let sent = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(body["status"], "fixed");
    assert_eq!(
        body.as_object().unwrap().len(),
        1,
        "a thread PATCH must carry the status and nothing else: {body}"
    );
}

/// Azure DevOps truncates `description` in the pull request LIST response
/// (around 400 characters, mid-word, with no marker saying it did). The
/// panel therefore cannot show a long description from the list alone - it
/// has to ask for the pull request itself, which returns the whole thing.
#[tokio::test]
async fn pr_description_returns_the_untruncated_body() {
    let server = MockServer::start().await;
    let long = "x".repeat(1200);
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/repositories/web/pullRequests/20"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pullRequestId": 20,
            "title": "Stage notifications",
            "description": long,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let body = client.pr_description("org", "proj", "web", 20).await.unwrap();
    assert_eq!(body.len(), 1200, "the full description, not the list's 400-char cut");
}

/// A pull request with no description at all is not an error - the panel
/// says "No description." and must not be handed a failure to render.
#[tokio::test]
async fn pr_description_of_a_pr_without_one_is_empty_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/repositories/web/pullRequests/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pullRequestId": 7, "title": "Tidy up"
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert_eq!(client.pr_description("org", "proj", "web", 7).await.unwrap(), "");
}

/// The repo name goes in the path and can contain characters that are not
/// URL-safe. `pr_work_items` encodes it; this must too, or a repo with a
/// space in its name 404s.
#[tokio::test]
async fn pr_description_encodes_the_repo_name() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/git/repositories/my%20repo/pullRequests/3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "description": "body"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert_eq!(client.pr_description("org", "proj", "my repo", 3).await.unwrap(), "body");
}
