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
                // Deliberately out of order, and jobs hang off a PHASE - the
                // real ADO shape, which is why jobs are matched by walking
                // ancestors rather than by a direct parentId == stage.
                { "id": "t2", "parentId": "j1", "type": "Task", "name": "Run Unit Test",
                  "state": "completed", "result": "failed", "order": 2,
                  "startTime": "2026-07-24T09:03:00Z", "finishTime": "2026-07-24T09:04:00Z",
                  "log": { "id": 42, "url": "https://x/logs/42" },
                  "issues": [{ "type": "error", "message": "3 tests failed" }] },
                { "id": "j1", "parentId": "ph1", "type": "Job", "name": "Build_solution",
                  "state": "completed", "result": "failed", "order": 1 },
                { "id": "ph1", "parentId": "st1", "type": "Phase", "name": "Phase 1", "order": 1 },
                { "id": "t1", "parentId": "j1", "type": "Task", "name": "Restore The Solution",
                  "state": "completed", "result": "succeeded", "order": 1 },
                { "id": "st2", "type": "Stage", "name": "Deploy", "state": "inProgress",
                  "result": null, "order": 2 },
                { "id": "st1", "type": "Stage", "name": "Stage", "state": "completed",
                  "result": "succeeded", "order": 1 },
                // Orphan: its stage record is absent, so it is dropped.
                { "id": "j9", "parentId": "missing", "type": "Job", "name": "Ghost", "order": 9 }
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
    // Jobs nest under their stage through the intermediate Phase record,
    // and their tasks come back in `order` - this is what the dialog needs
    // to point at the step that actually failed.
    let jobs = &ci.stages[0].jobs;
    assert_eq!(jobs.len(), 1, "the orphaned job is dropped, not guessed at");
    assert_eq!(jobs[0].name, "Build_solution");
    assert_eq!(
        jobs[0].tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        vec!["Restore The Solution", "Run Unit Test"]
    );
    assert_eq!(jobs[0].tasks[1].result, "failed");
    assert_eq!(jobs[0].tasks[1].issues, vec!["3 tests failed"]);
    assert_eq!(jobs[0].tasks[1].log_id, 42, "the step's log is fetchable on demand");
    assert_eq!(jobs[0].tasks[0].log_id, 0, "no log record -> 0, not a panic");
    assert!(ci.stages[1].jobs.is_empty(), "the in-progress stage has no jobs yet");
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
        .and(query_param("$top", "25"))
        .and(query_param("$skip", "0"))
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
    let done = client.repo_pull_requests("o", "p", "r1", "completed", 0).await.unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].status, "completed");
    assert_eq!(done[0].closed, "2026-07-24T08:00:00Z");
    assert_eq!(done[0].merge_commit, "abc123", "the pipeline lookup needs this");
    assert_eq!(
        done[0].repo_id, "11111111-2222-3333-4444-555555555555",
        "the Build API filters by repository GUID, not the name"
    );

    // Anything unrecognised falls back to active rather than being injected.
    let fallback = client.repo_pull_requests("o", "p", "r1", "bogus&x=1", 0).await.unwrap();
    assert!(fallback.is_empty());
}

/// Build logs come back as plain text, not JSON - the dialog streams them
/// into its log pane while a step is still running.
#[tokio::test]
async fn build_log_returns_plain_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds/901/logs/42"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("Starting: Run Unit Test\nPassed! - Failed: 0, Passed: 203\n"),
        )
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let log = client.build_log("o", "p", 901, 42).await.unwrap();
    assert!(log.contains("Starting: Run Unit Test"));
    assert!(log.lines().count() >= 2);
}

/// The skip offset reaches ADO untouched - page 2 asks for $skip=25.
#[tokio::test]
async fn repo_pull_requests_pages_with_skip() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/git/repositories/r1/pullrequests"))
        .and(query_param("$top", "25"))
        .and(query_param("$skip", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "pullRequestId": 7, "title": "Page two", "status": "completed",
                        "repository": { "name": "r", "id": "g" },
                        "createdBy": { "displayName": "Dev" } }]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let page = client.repo_pull_requests("o", "p", "r1", "completed", 25).await.unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, 7);
}

/// The revalidation path: known build ids in, current deployments out -
/// including a release created long after the build finished. A build with
/// no Release Management data degrades to an empty list, never an error.
#[tokio::test]
async fn builds_deployments_picks_up_late_releases() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/release/releases"))
        .and(query_param("artifactVersionId", "901"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "name": "Release-500",
                "_links": { "web": { "href": "https://x/release/500" } },
                "environments": [
                    { "name": "Production", "status": "succeeded", "modifiedOn": "2026-07-26T10:00:00Z" }
                ]
            }]
        })))
        .mount(&server)
        .await;
    // 902 has no mock at all -> 404 -> empty, not an error.

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let fresh = client.builds_deployments("o", "p", &[901, 902]).await.unwrap();
    assert_eq!(fresh.len(), 2);
    assert_eq!(fresh[0].build_id, 901);
    assert_eq!(fresh[0].deployments[0].environment, "Production");
    assert_eq!(fresh[0].deployments[0].release, "Release-500");
    assert_eq!(fresh[1].build_id, 902);
    assert!(fresh[1].deployments.is_empty());
}

/// The pill on a list row. One call for the whole repository, and the
/// folding rules the panel depends on:
///   - anything still running wins over anything finished;
///   - anything that did not SUCCEED is an error, canceled included;
///   - a PR with no validation build is absent, not "succeeded" - showing
///     nothing for it is the caller's choice, but the data must not claim
///     it passed;
///   - a build on a real branch is not a PR validation build.
#[tokio::test]
async fn pr_build_states_folds_a_repos_builds_onto_its_pull_requests() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            // 101: the same pipeline re-queued while an older run is green
            // -> the newest run is what counts, so it is running.
            { "id": 10, "definition": { "id": 1 }, "sourceBranch": "refs/pull/101/merge",
              "status": "completed", "result": "succeeded" },
            { "id": 11, "definition": { "id": 1 }, "sourceBranch": "refs/pull/101/merge",
              "status": "inProgress", "result": "" },
            // 102: green only.
            { "id": 12, "definition": { "id": 1 }, "sourceBranch": "refs/pull/102/merge",
              "status": "completed", "result": "succeeded" },
            // 103: TWO pipelines. One is green, the other's latest is red -
            // the PR is failing, and a green sibling must not hide that.
            { "id": 13, "definition": { "id": 1 }, "sourceBranch": "refs/pull/103/merge",
              "status": "completed", "result": "succeeded" },
            { "id": 14, "definition": { "id": 2 }, "sourceBranch": "refs/pull/103/merge",
              "status": "completed", "result": "failed" },
            // 104: canceled is not success.
            { "id": 15, "definition": { "id": 1 }, "sourceBranch": "refs/pull/104/merge",
              "status": "completed", "result": "canceled" },
            // 105 has no build at all and must not appear.
            // A branch build, and another PR's build, are both ignored.
            { "id": 16, "definition": { "id": 1 }, "sourceBranch": "refs/heads/main",
              "status": "completed", "result": "failed" },
            { "id": 17, "definition": { "id": 1 }, "sourceBranch": "refs/pull/999/merge",
              "status": "completed", "result": "failed" },
        ]})))
        .mount(&server)
        .await;

    let out = AdoClient::with_base_url("t".into(), server.uri())
        .pr_build_states("o", "p", "repo-guid", &[101, 102, 103, 104, 105])
        .await
        .unwrap();

    let got: Vec<(i32, &str)> = out.iter().map(|s| (s.pr_id, s.state.as_str())).collect();
    assert_eq!(
        got,
        vec![(101, "running"), (102, "succeeded"), (103, "failed"), (104, "failed")],
        "105 has no validation build and must be absent, not succeeded"
    );

    // One request for the whole list - the reason this exists at all.
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn no_pull_requests_asks_azure_devops_nothing() {
    let server = MockServer::start().await;
    let out = AdoClient::with_base_url("t".into(), server.uri())
        .pr_build_states("o", "p", "repo-guid", &[])
        .await
        .unwrap();
    assert!(out.is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// The bug this pill shipped with: it folded a pull request's WHOLE build
/// history together, so one old red run marked it failed forever. Fixing
/// the build and re-running changed nothing, because the failure was still
/// in the list - which is the opposite of what a status pill is for.
#[tokio::test]
async fn a_failure_that_has_since_been_re_run_green_is_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            // Deliberately newest-first, the order Azure DevOps returns:
            // the fix must not depend on reading them in any order.
            { "id": 902, "definition": { "id": 7 }, "sourceBranch": "refs/pull/55/merge",
              "status": "completed", "result": "succeeded" },
            { "id": 901, "definition": { "id": 7 }, "sourceBranch": "refs/pull/55/merge",
              "status": "completed", "result": "failed" },
        ]})))
        .mount(&server)
        .await;

    let out = AdoClient::with_base_url("t".into(), server.uri())
        .pr_build_states("o", "p", "repo-guid", &[55])
        .await
        .unwrap();
    assert_eq!(
        out.iter().map(|s| (s.pr_id, s.state.as_str())).collect::<Vec<_>>(),
        vec![(55, "succeeded")],
        "the older red run is history, not the current state"
    );
}

/// And the other direction: a pipeline that WAS green and has just broken
/// must show the error, however many green runs preceded it.
#[tokio::test]
async fn a_pipeline_that_has_just_broken_shows_the_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/build/builds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            { "id": 801, "definition": { "id": 3 }, "sourceBranch": "refs/pull/56/merge",
              "status": "completed", "result": "succeeded" },
            { "id": 802, "definition": { "id": 3 }, "sourceBranch": "refs/pull/56/merge",
              "status": "completed", "result": "succeeded" },
            { "id": 803, "definition": { "id": 3 }, "sourceBranch": "refs/pull/56/merge",
              "status": "completed", "result": "failed" },
        ]})))
        .mount(&server)
        .await;

    let out = AdoClient::with_base_url("t".into(), server.uri())
        .pr_build_states("o", "p", "repo-guid", &[56])
        .await
        .unwrap();
    assert_eq!(out[0].state, "failed");
}
