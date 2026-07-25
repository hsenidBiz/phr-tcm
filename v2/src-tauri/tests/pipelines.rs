//! PR pipeline + deployment reads, and the completed-PR query they hang off.

use v2_lib::ado::AdoClient;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A merged PR has two builds - validation on refs/pull/{id}/merge and the
/// post-merge CI run matched by merge commit - and each carries its stages
/// plus the release environments it reached.
#[tokio::test]
async fn pr_builds_collects_validation_ci_stages_and_environments() {
    let server = MockServer::start().await;

    // Validation build (queried by PR merge branch).
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds"))
        .and(query_param("branchName", "refs/pull/42/merge"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 900, "buildNumber": "20260724.1", "status": "completed",
                "result": "succeeded", "startTime": "2026-07-24T08:00:00Z",
                "finishTime": "2026-07-24T08:04:00Z",
                "definition": { "name": "HRM-PMS-NET (Build)" },
                "_links": { "web": { "href": "https://x/build/900" } }
            }]
        })))
        .mount(&server)
        .await;

    // Recent repo builds: only the one on the merge commit is this PR's CI run.
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds"))
        .and(query_param("$top", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "id": 901, "buildNumber": "2026.7.24-12", "status": "completed",
                  "result": "succeeded", "sourceVersion": "abc123",
                  "startTime": "2026-07-24T09:00:00Z",
                  "definition": { "name": "HRM-PMS-NET" },
                  "_links": { "web": { "href": "https://x/build/901" } } },
                { "id": 902, "buildNumber": "other", "sourceVersion": "zzz999",
                  "definition": { "name": "Unrelated" } }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds/901/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "records": [
                { "type": "Job", "name": "Build_solution", "state": "completed",
                  "result": "succeeded", "order": 1 },
                { "type": "Stage", "name": "Deploy", "state": "inProgress",
                  "result": null, "order": 2 },
                { "type": "Stage", "name": "Stage", "state": "completed",
                  "result": "succeeded", "order": 1 }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds/900/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "records": [] })))
        .mount(&server)
        .await;

    // Release Management: the CI build reached QA, Production is pending.
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/release/releases"))
        .and(query_param("artifactVersionId", "901"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "name": "Release-482",
                "_links": { "web": { "href": "https://x/release/482" } },
                "environments": [
                    { "name": "QA", "status": "succeeded", "modifiedOn": "2026-07-24T10:00:00Z" },
                    { "name": "Production", "status": "notStarted", "createdOn": "2026-07-24T09:30:00Z" }
                ]
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/release/releases"))
        .and(query_param("artifactVersionId", "900"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [] })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let builds = client.pr_builds("o", "p", "repo1", 42, "abc123").await.unwrap();

    assert_eq!(builds.len(), 2, "validation + CI only - not the unrelated build");
    // Newest first: the CI run started after the validation run.
    let ci = &builds[0];
    assert_eq!(ci.id, 901);
    assert_eq!(ci.name, "HRM-PMS-NET");
    assert!(!ci.is_validation);
    // Stage records only, ordered by `order` - the Job record is not a stage.
    assert_eq!(
        ci.stages.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["Stage", "Deploy"]
    );
    assert_eq!(ci.stages[1].state, "inProgress");
    assert_eq!(
        ci.deployments
            .iter()
            .map(|d| (d.environment.as_str(), d.status.as_str()))
            .collect::<Vec<_>>(),
        vec![("QA", "succeeded"), ("Production", "notStarted")]
    );
    assert_eq!(ci.deployments[0].release, "Release-482");
    assert_eq!(ci.deployments[0].on, "2026-07-24T10:00:00Z");
    // createdOn is the fallback when an environment was never modified.
    assert_eq!(ci.deployments[1].on, "2026-07-24T09:30:00Z");

    let validation = &builds[1];
    assert_eq!(validation.id, 900);
    assert!(validation.is_validation);
    assert!(validation.deployments.is_empty());
}

/// An open PR has no merge commit yet, so only validation builds exist -
/// and an org without Release Management still gets its builds back.
#[tokio::test]
async fn pr_builds_open_pr_skips_ci_and_survives_missing_release_management() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 700, "buildNumber": "1", "status": "inProgress", "result": null,
                "definition": { "name": "CI" }
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds/700/timeline"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    // No release mock at all - those GETs 404, and must not fail the call.

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let builds = client.pr_builds("o", "p", "repo1", 7, "").await.unwrap();
    assert_eq!(builds.len(), 1);
    assert_eq!(builds[0].status, "inProgress");
    assert_eq!(builds[0].result, "", "a running build has no result yet");
    assert!(builds[0].stages.is_empty(), "an expired timeline degrades to no stages");
    assert!(builds[0].deployments.is_empty());
}

/// Completed PRs are a separate, capped query, and the status is
/// whitelisted so an unexpected value can never extend the query string.
#[tokio::test]
async fn repo_pull_requests_completed_is_capped_and_status_is_whitelisted() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/git/repositories/r1/pullrequests"))
        .and(query_param("searchCriteria.status", "completed"))
        .and(query_param("$top", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "pullRequestId": 20620, "title": "Merged thing", "status": "completed",
                "closedDate": "2026-07-24T08:00:00Z",
                "lastMergeCommit": { "commitId": "abc123" },
                "repository": {
                    "name": "HRM-PMS-NET",
                    "id": "11111111-2222-3333-4444-555555555555"
                },
                "createdBy": { "displayName": "Dev" }
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/git/repositories/r1/pullrequests"))
        .and(query_param("searchCriteria.status", "active"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [] })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let done = client.repo_pull_requests("o", "p", "r1", "completed").await.unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].status, "completed");
    assert_eq!(done[0].closed, "2026-07-24T08:00:00Z");
    assert_eq!(done[0].merge_commit, "abc123", "the pipeline lookup needs this");
    assert_eq!(
        done[0].repo_id, "11111111-2222-3333-4444-555555555555",
        "the Build API filters by repository GUID, not the name"
    );

    // Anything unrecognised falls back to active rather than being injected.
    let fallback = client.repo_pull_requests("o", "p", "r1", "bogus&x=1").await.unwrap();
    assert!(fallback.is_empty());
}
