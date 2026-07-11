use v2_lib::ado::{AdoClient, AdoError};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_projects_parses_and_authenticates() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/myorg/_apis/projects"))
        .and(header("Authorization", "Bearer tok123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 1,
            "value": [{"id": "abc", "name": "Proj One"}]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("tok123".into(), server.uri());
    let projects = client.get_projects("myorg").await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "Proj One");
}

#[tokio::test]
async fn maps_401_to_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("bad".into(), server.uri());
    let err = client.get_projects("o").await.unwrap_err();
    assert!(matches!(err, AdoError::Unauthorized));
}

#[tokio::test]
async fn maps_429_with_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "17"))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let err = client.get_projects("o").await.unwrap_err();
    assert!(matches!(err, AdoError::RateLimited { retry_after_secs: 17 }));
}

#[tokio::test]
async fn list_orgs_two_hop_discovery() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_apis/profile/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "member-1"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/_apis/accounts"))
        .and(wiremock::matchers::query_param("memberId", "member-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 2,
            "value": [
                {"accountName": "orgB", "accountUri": "https://dev.azure.com/orgB"},
                {"accountName": "orgA", "accountUri": "https://dev.azure.com/orgA"}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let orgs = client.list_orgs().await.unwrap();
    let names: Vec<_> = orgs.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, vec!["orgA", "orgB"], "sorted case-insensitively");
}

#[tokio::test]
async fn search_pbis_builds_wiql_and_fetches_titles() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/myorg/proj/_apis/wit/wiql"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "query": "SELECT [System.Id] FROM workitems WHERE [System.TeamProject] = @project AND ([System.Id] = 42 OR [System.Title] CONTAINS '42') AND [System.WorkItemType] = 'Product Backlog Item' ORDER BY [System.ChangedDate] DESC"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": [{"id": 42}, {"id": 7}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/myorg/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 7, "fields": {"System.Title": "Seven", "System.WorkItemType": "Product Backlog Item"}},
                {"id": 42, "fields": {"System.Title": "The Answer", "System.WorkItemType": "Product Backlog Item"}}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let hits = client.search_pbis("myorg", "proj", "42", 20).await.unwrap();
    // WIQL order preserved (42 first), not the batch-GET response order.
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, 42);
    assert_eq!(hits[0].title, "The Answer");
    assert_eq!(hits[1].id, 7);
}

#[tokio::test]
async fn search_pbis_escapes_single_quotes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/wiql"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "query": "SELECT [System.Id] FROM workitems WHERE [System.TeamProject] = @project AND [System.Title] CONTAINS 'it''s' AND [System.WorkItemType] = 'Product Backlog Item' ORDER BY [System.ChangedDate] DESC"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": []
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let hits = client.search_pbis("o", "p", "it's", 20).await.unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn pbi_test_cases_follow_testedby_relations() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems/100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 100,
            "relations": [
                {"rel": "Microsoft.VSTS.Common.TestedBy-Forward", "url": "https://x/_apis/wit/workItems/201"},
                {"rel": "System.LinkTypes.Hierarchy-Forward", "url": "https://x/_apis/wit/workItems/999"},
                {"rel": "Microsoft.VSTS.Common.TestedBy-Forward", "url": "https://x/_apis/wit/workItems/202"}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 201, "fields": {"System.Title": "TC one", "System.Tags": "smoke; ui",
                    "Microsoft.VSTS.TCM.AutomationStatus": "Planned"}},
                {"id": 202, "fields": {"System.Title": "TC two"}}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let cases = client.get_pbi_test_cases("org", 100).await.unwrap();
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].id, 201);
    assert_eq!(cases[0].tags, "smoke; ui");
    assert_eq!(cases[0].automation_status, "Planned");
    assert_eq!(cases[1].title, "TC two");
    assert_eq!(cases[1].automation_status, "");
}

/// The tool must never destroy data: no DELETE requests, ever.
#[test]
fn client_source_has_no_delete_calls() {
    let src = include_str!("../src/ado.rs");
    assert!(
        !src.contains(".delete(") && !src.contains("Method::DELETE"),
        "AdoClient must never issue DELETE requests"
    );
}
