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

fn sample_tc() -> v2_lib::model::TestCase {
    v2_lib::model::TestCase {
        title: "My case".into(),
        steps: vec![v2_lib::steps_xml::Step {
            action: "Do".into(),
            expected: "Done".into(),
        }],
        tags: "smoke".into(),
        automation_status: "Planned".into(),
        module_value: "Auth".into(),
        preconditions: "Logged out".into(),
        update_id: None,
    }
}

#[tokio::test]
async fn create_test_case_posts_json_patch() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/workitems/$Test%20Case"))
        .and(header("Content-Type", "application/json-patch+json"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!([
            {"op": "add", "path": "/fields/System.Title", "value": "My case"}
        ])))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 777})))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let id = client
        .create_test_case("org", "proj", &sample_tc(), Some("Custom.Module"), "Area\\Sub", "It\\1", Some("Custom.Prec"))
        .await
        .unwrap();
    assert_eq!(id, 777);
}

#[tokio::test]
async fn update_from_model_skips_blank_fields() {
    // SAFETY: a blank imported column must never wipe existing ADO data.
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/wit/workitems/55"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 55})))
        .mount(&server)
        .await;

    let mut tc = sample_tc();
    tc.tags = String::new();
    tc.module_value = String::new();
    tc.preconditions = String::new();
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    client
        .update_test_case_from_model("org", "proj", 55, &tc, Some("Custom.Module"), Some("Custom.Prec"))
        .await
        .unwrap();

    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let paths: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|op| op["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"/fields/Microsoft.VSTS.TCM.Steps"));
    assert!(paths.contains(&"/fields/Microsoft.VSTS.TCM.AutomationStatus"));
    assert!(!paths.iter().any(|p| p.contains("Tags")), "blank tags must be skipped");
    assert!(!paths.iter().any(|p| p.contains("Custom.Module")));
    assert!(!paths.iter().any(|p| p.contains("Custom.Prec")));
}

#[tokio::test]
async fn link_to_pbi_adds_testedby_reverse_relation() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/wit/workitems/777"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!([
            {"op": "add", "path": "/relations/-", "value": {"rel": "Microsoft.VSTS.Common.TestedBy-Reverse"}}
        ])))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 777})))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    client.link_to_pbi("org", "proj", 777, 100).await.unwrap();
}

#[tokio::test]
async fn test_case_fields_filter_and_sort_like_v1() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitemtypes/Test%20Case/fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"name": "Zebra Module", "referenceName": "Custom.Module"},
                {"name": "State", "referenceName": "System.State"},
                {"name": "Title", "referenceName": "System.Title"},
                {"name": "Locked", "referenceName": "Custom.Locked", "readOnly": true},
                {"name": "Apples", "referenceName": "Custom.Apples"}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let fields = client.get_test_case_fields("org", "proj").await.unwrap();
    let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
    // System.State and readOnly dropped; System.Title kept; sorted by name.
    assert_eq!(names, vec!["Apples", "Title", "Zebra Module"]);
}

#[tokio::test]
async fn full_cases_parse_steps_and_optional_refs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems/100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 100,
            "relations": [
                {"rel": "Microsoft.VSTS.Common.TestedBy-Forward", "url": "https://x/_apis/wit/workItems/201"}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 201,
                "fields": {
                    "System.Title": "TC one",
                    "System.Tags": "smoke",
                    "Microsoft.VSTS.TCM.Steps": "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open</parameterizedString><parameterizedString isformatted=\"true\">Shown</parameterizedString></step></steps>",
                    "Custom.Module": "Auth",
                    "Custom.Prec": "<div>Logged out</div>"
                }
            }]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let cases = client
        .get_pbi_test_cases_full("org", 100, Some("Custom.Module"), Some("Custom.Prec"))
        .await
        .unwrap();
    assert_eq!(cases.len(), 1);
    let c = &cases[0];
    assert_eq!(c.steps.len(), 1);
    assert_eq!(c.steps[0].action, "Open");
    assert_eq!(c.steps[0].expected, "Shown");
    assert_eq!(c.module_value, "Auth");
    assert_eq!(c.preconditions, "Logged out"); // html flattened
    assert_eq!(c.automation_status, "Not Automated"); // empty -> default
}

#[tokio::test]
async fn classification_paths_walk_names_not_path_field() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/classificationnodes/areas"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "HRM",
            "path": "\\HRM\\Area", // must be ignored - carries the extra segment
            "children": [
                {"name": "Gamma Guardians", "path": "\\HRM\\Area\\Gamma Guardians",
                 "children": [{"name": "Sprint 9"}]}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let paths = client
        .get_classification_paths("org", "proj", "areas")
        .await
        .unwrap();
    assert_eq!(
        paths,
        vec!["HRM", "HRM\\Gamma Guardians", "HRM\\Gamma Guardians\\Sprint 9"]
    );
    // Discovery failure degrades to empty, never an error.
    let bad = AdoClient::with_base_urls("tok".into(), "http://127.0.0.1:1".into(), "x".into());
    assert!(bad.get_classification_paths("o", "p", "areas").await.unwrap().is_empty());
}

/// The tool must never destroy data: no DELETE requests, ever.
#[test]
fn client_source_has_no_delete_calls() {
    // Every file that builds HTTP requests or extends AdoClient — a new
    // impl file must be added here (compile error via include_str! if one
    // of these moves without the test following it).
    let sources = [
        include_str!("../src/auth.rs"),
        include_str!("../src/ado_git.rs"),
        include_str!("../src/ado/mod.rs"),
        include_str!("../src/ado/transport.rs"),
        include_str!("../src/ado/endpoints.rs"),
        include_str!("../src/ado_testplan/mod.rs"),
        include_str!("../src/ado_testplan/plans.rs"),
        include_str!("../src/ado_testplan/runs.rs"),
        include_str!("../src/ado_testplan/history.rs"),
        include_str!("../src/work_board/mod.rs"),
        include_str!("../src/work_board/board.rs"),
        include_str!("../src/work_board/detail.rs"),
        include_str!("../src/work_board/layout.rs"),
    ];
    for src in sources {
        assert!(
            !src.contains(".delete(") && !src.contains("Method::DELETE"),
            "AdoClient must never issue DELETE requests"
        );
    }
}

#[tokio::test]
async fn field_values_in_use_dedupes_and_sorts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/wiql"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "query": "SELECT [System.Id] FROM workitems WHERE [System.TeamProject] = @project AND [System.WorkItemType] = 'Test Case' AND [Custom.Module] <> '' ORDER BY [System.ChangedDate] DESC"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": [{"id": 1}, {"id": 2}, {"id": 3}, {"id": 4}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 1, "fields": {"Custom.Module": "Payments"}},
                {"id": 2, "fields": {"Custom.Module": "auth"}},
                {"id": 3, "fields": {"Custom.Module": "  payments "}}, // dup after trim, case-insensitive
                {"id": 4, "fields": {}}                                  // field missing -> skipped
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let values = client.field_values_in_use("o", "p", "Custom.Module").await.unwrap();
    assert_eq!(values, vec!["auth".to_string(), "Payments".to_string()]);
}

#[tokio::test]
async fn field_values_in_use_rejects_unsafe_field_refs() {
    // Never started server: an unsafe ref must short-circuit without any request.
    let client = AdoClient::with_base_urls("tok".into(), "http://127.0.0.1:1".into(), "http://127.0.0.1:1".into());
    let values = client.field_values_in_use("o", "p", "Bad] FROM x; --").await.unwrap();
    assert!(values.is_empty());
}

/// The move must trust the state ADO persisted, not the state we asked for -
/// server rules (e.g. required dates) can keep or rewrite the transition.
#[tokio::test]
async fn set_state_returns_the_state_ado_actually_saved() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/o/p/_apis/wit/workitems/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 7,
            "fields": { "System.State": "To Do" } // rule kept the old state
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let actual = client.set_work_item_state("o", "p", 7, "In Progress").await.unwrap();
    assert_eq!(actual, "To Do");
}

/// A 400 rule rejection must surface ADO's human-readable message, not the
/// raw JSON body.
#[tokio::test]
async fn set_state_extracts_rule_message_from_400() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/o/p/_apis/wit/workitems/7"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "message": "TF401320: Rule Error: Start Date is required.",
            "typeName": "RuleValidationException"
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client.set_work_item_state("o", "p", 7, "In Progress").await.unwrap_err();
    match err {
        AdoError::Http { status, body } => {
            assert_eq!(status, 400);
            assert_eq!(body, "TF401320: Rule Error: Start Date is required.");
        }
        other => panic!("expected Http error, got {other:?}"),
    }
}
