use v2_lib::ado::{AdoClient, AdoError, BlankPolicy};
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
async fn search_wiki_posts_search_text_and_maps_hits() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/myorg/proj/_apis/search/wikisearchresults"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "searchText": "auth flow",
            "$top": 10
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 1,
            "results": [{
                "fileName": "Auth-Flow.md",
                "path": "/Docs/Auth Flow",
                "wiki": {"id": "wiki-1", "name": "Project.wiki"},
                "hits": [
                    {"fieldReferenceName": "content", "highlights": ["...the <hl>auth flow</hl> starts..."]},
                    {"fieldReferenceName": "path", "highlights": ["Auth <hl>Flow</hl>"]}
                ]
            }]
        })))
        .mount(&server)
        .await;

    // With base_url = the mock server's plain http://127.0.0.1:PORT (no
    // "dev.azure.com" substring), the almsearch-host derivation is a no-op
    // and the request still lands on this same stub server.
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let hits = client.search_wiki("myorg", "proj", "auth flow", 10).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].file_name, "Auth-Flow.md");
    assert_eq!(hits[0].path, "/Docs/Auth Flow");
    assert_eq!(hits[0].wiki_id, "wiki-1");
    assert_eq!(hits[0].wiki_name, "Project.wiki");
    assert!(hits[0].highlights.contains("auth flow"));
    assert!(hits[0].highlights.contains("Flow"));
}

#[tokio::test]
async fn search_wiki_defensive_on_missing_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/search/wikisearchresults"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [{ "fileName": "Bare.md" }]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let hits = client.search_wiki("o", "p", "x", 5).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "");
    assert_eq!(hits[0].wiki_id, "");
    assert_eq!(hits[0].highlights, "");
}

#[tokio::test]
async fn get_wiki_page_encodes_path_and_returns_content() {
    let server = MockServer::start().await;
    // wiremock's query_param matcher compares against the DECODED value
    // (it decodes the request's raw query itself); the encoding this test
    // is really pinning is that get_wiki_page's URL is valid enough for
    // wiremock/reqwest to decode the space back correctly at all.
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wiki/wikis/wiki-1/pages"))
        .and(wiremock::matchers::query_param("path", "/Docs/API Guide"))
        .and(wiremock::matchers::query_param("includeContent", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Docs/API Guide",
            "content": "# API Guide\nfull markdown here"
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let page = client
        .get_wiki_page("o", "p", "wiki-1", "/Docs/API Guide")
        .await
        .unwrap();
    assert_eq!(page.path, "/Docs/API Guide");
    assert!(page.content.contains("full markdown here"));
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
        comment: String::new(),
        reviewer_notes: String::new(),
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
        // Neither app-only note may EVER be sent to ADO, in any form.
        .and(NoCommentInBody)
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 777})))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let mut tc = sample_tc();
    tc.comment = "IN-APP-ONLY sentinel".into();
    tc.reviewer_notes = "REVIEWER-ONLY sentinel".into();
    let id = client
        .create_test_case("org", "proj", &tc, Some("Custom.Module"), "Area\\Sub", "It\\1", Some("Custom.Prec"))
        .await
        .unwrap();
    assert_eq!(id, 777);
}

/// Matcher rejecting any request carrying either app-only note's sentinel.
///
/// `comment` and `reviewer_notes` both live on TestCase, both round-trip
/// through the exported JSON, and neither has an Azure DevOps field. The
/// only thing standing between "app-only" and a field mapping added in
/// passing is this matcher, so it names both.
struct NoCommentInBody;
impl wiremock::Match for NoCommentInBody {
    fn matches(&self, request: &wiremock::Request) -> bool {
        let body = String::from_utf8_lossy(&request.body);
        !body.contains("IN-APP-ONLY sentinel") && !body.contains("REVIEWER-ONLY sentinel")
    }
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
        .update_test_case_from_model("org", "proj", 55, &tc, Some("Custom.Module"), Some("Custom.Prec"), None, BlankPolicy::Skip)
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
    assert!(paths.contains(&"/fields/System.Title"));
    assert!(paths.contains(&"/fields/Microsoft.VSTS.TCM.Steps"));
    assert!(paths.contains(&"/fields/Microsoft.VSTS.TCM.AutomationStatus"));
    assert!(!paths.iter().any(|p| p.contains("Tags")), "blank tags must be skipped");
    assert!(!paths.iter().any(|p| p.contains("Custom.Module")));
    assert!(!paths.iter().any(|p| p.contains("Custom.Prec")));
}

#[tokio::test]
async fn update_from_model_writes_the_title() {
    // Regression guard: bulk-import updates rename cases via System.Title.
    // The diff preview promises "Title: old -> new" as an always-written
    // field - an update that omits it reports success while silently
    // keeping the old name in Azure DevOps.
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/wit/workitems/55"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 55})))
        .mount(&server)
        .await;

    let mut tc = sample_tc();
    tc.title = "Renamed by bulk import".into();
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    client
        .update_test_case_from_model("org", "proj", 55, &tc, None, None, None, BlankPolicy::Skip)
        .await
        .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let title_op = body
        .as_array()
        .unwrap()
        .iter()
        .find(|op| op["path"] == "/fields/System.Title")
        .expect("update must PATCH System.Title");
    assert_eq!(title_op["value"], "Renamed by bulk import");
    assert_eq!(title_op["op"], "add");
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
    // The raw blob comes back UNPARSED alongside the lossy read. Every save
    // compares against it to decide whether Steps needs writing at all, so
    // if this ever came back empty, saves would start clobbering steps again
    // without anything else failing.
    assert!(c.steps_xml.contains("<steps"), "raw steps blob: {:?}", c.steps_xml);
    assert!(c.steps_xml.contains("Open"));
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

/// The one sanctioned exception, and it is held to a TIGHTER rule than the
/// files above rather than a looser one.
///
/// `recycle.rs` may issue DELETE - that is its whole reason to exist - but
/// `DELETE _apis/wit/workitems/{id}?destroy=true` erases a work item
/// permanently and irrecoverably, where the plain form moves it to the
/// project's recycle bin and it can be restored. The recoverable one is the
/// only delete this app will ever make, and the difference is a single
/// query parameter, so it is asserted here rather than trusted to review.
#[test]
fn the_only_delete_is_the_recoverable_one() {
    let recycle = include_str!("../src/ado/recycle.rs");

    // It really is the delete path - otherwise this test passes vacuously
    // if someone renames the file or moves the call out of it.
    assert!(
        recycle.contains(".delete("),
        "recycle.rs is supposed to be the file that deletes"
    );

    // Case-insensitive: ?destroy=true, DESTROY, "Destroy" in a builder -
    // none of it. The word appears nowhere, including in prose, so that
    // this assertion can never be weakened by a comment mentioning it.
    let lowered = recycle.to_lowercase();
    assert!(
        !lowered.contains("destroy"),
        "recycle.rs must never mention destroy - a permanent delete is not          a capability this app has"
    );

    // And nothing else may quietly grow one.
    for (name, src) in [
        ("commands/cases.rs", include_str!("../src/commands/cases.rs")),
        ("commands/queue.rs", include_str!("../src/commands/queue.rs")),
        ("commands/board.rs", include_str!("../src/commands/board.rs")),
    ] {
        assert!(
            !src.to_lowercase().contains("destroy"),
            "{name} must not mention destroy"
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

/// Preconditions are typed as plain text and were interpolated straight
/// into `<div>...</div>`, so "value < 10" or "Tom & Jerry" reached Azure
/// DevOps as broken markup.
#[tokio::test]
async fn preconditions_are_html_escaped_on_create_and_update() {
    for (label, method) in [("create", "POST"), ("update", "PATCH")] {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method(method))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "id": 7, "fields": {} }),
            ))
            .mount(&server)
            .await;
        let client = v2_lib::ado::AdoClient::with_base_url("t".into(), server.uri());
        let tc = v2_lib::model::TestCase {
            title: "T".into(),
            steps: vec![v2_lib::steps_xml::Step { action: "a".into(), expected: "b".into() }],
            automation_status: "Planned".into(),
            preconditions: "value < 10 & rising".into(),
            ..Default::default()
        };
        let _ = if method == "POST" {
            client
                .create_test_case("o", "p", &tc, None, "", "", Some("Custom.Pre"))
                .await
                .map(|_| ())
        } else {
            client
                .update_test_case_from_model("o", "p", 7, &tc, None, Some("Custom.Pre"), None, BlankPolicy::Skip)
                .await
        };
        let sent = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&sent[0].body).to_string();
        assert!(
            body.contains("value &lt; 10 &amp; rising"),
            "{label}: preconditions were not escaped:\n{body}"
        );
        assert!(!body.contains("value < 10"), "{label}: raw < reached ADO");
    }
}

// ------------------------------------------------- steps are not clobbered

/// A step as Azure DevOps really holds one: markup and an embedded image
/// that `parse_steps_xml` cannot represent.
const RICH_STEPS: &str = "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\">\
<parameterizedString isformatted=\"true\">&lt;DIV&gt;&lt;B&gt;Click Save&lt;/B&gt;\
&lt;IMG src=\"http://ado/att/1.png\"&gt;&lt;/DIV&gt;</parameterizedString>\
<parameterizedString isformatted=\"true\">Saved</parameterizedString></step></steps>";

async fn captured_patch(tc: &v2_lib::model::TestCase, original: Option<&str>) -> String {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("PATCH"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "id": 55, "fields": {} })),
        )
        .mount(&server)
        .await;
    v2_lib::ado::AdoClient::with_base_url("t".into(), server.uri())
        .update_test_case_from_model("o", "p", 55, tc, None, None, original, BlankPolicy::Skip)
        .await
        .unwrap();
    String::from_utf8_lossy(&server.received_requests().await.unwrap()[0].body).to_string()
}

fn case_from(xml: &str, title: &str) -> v2_lib::model::TestCase {
    v2_lib::model::TestCase {
        title: title.into(),
        steps: v2_lib::steps_xml::parse_steps_xml(xml),
        automation_status: "Planned".into(),
        update_id: Some(55),
        ..Default::default()
    }
}

/// THE bug: the editor reads steps as plain text, so saving a case you only
/// retitled used to PATCH that flattened text back and delete the markup and
/// the screenshot from Azure DevOps.
#[tokio::test]
async fn a_title_only_save_does_not_touch_the_steps() {
    // Exactly what the editor holds after loading: steps parsed from ADO.
    let tc = case_from(RICH_STEPS, "A better title");
    let body = captured_patch(&tc, Some(RICH_STEPS)).await;

    assert!(body.contains("A better title"), "the title must still be written");
    assert!(
        !body.contains("Microsoft.VSTS.TCM.Steps"),
        "Steps must be left out of the patch entirely:\n{body}"
    );
}

/// When the user really does edit a step, it is written - the loss of markup
/// there is unavoidable and intended, because they replaced the text.
#[tokio::test]
async fn an_edited_step_is_still_written() {
    let mut tc = case_from(RICH_STEPS, "T");
    tc.steps[0].action = "Click Save twice".into();
    let body = captured_patch(&tc, Some(RICH_STEPS)).await;
    assert!(body.contains("Microsoft.VSTS.TCM.Steps"), "an edit must write:\n{body}");
    assert!(body.contains("Click Save twice"));
}

/// An imported update has no baseline - the file supplies the steps and is
/// meant to write them, which is the queue submit path.
#[tokio::test]
async fn without_a_baseline_the_steps_are_written_as_before() {
    let tc = case_from(RICH_STEPS, "T");
    let body = captured_patch(&tc, None).await;
    assert!(body.contains("Microsoft.VSTS.TCM.Steps"), "no baseline means write:\n{body}");
}

/// build_steps_xml(&[]) emits a blank placeholder step, so writing it would
/// replace a real step list with one empty row. Nothing should send an empty
/// list, but the cost of being wrong is unrecoverable.
#[tokio::test]
async fn an_empty_step_list_is_never_written_over_a_real_one() {
    let mut tc = case_from(RICH_STEPS, "T");
    tc.steps.clear();
    for baseline in [Some(RICH_STEPS), None] {
        let body = captured_patch(&tc, baseline).await;
        assert!(
            !body.contains("Microsoft.VSTS.TCM.Steps"),
            "an empty list must never reach ADO:\n{body}"
        );
    }
}

/// The round-trip exposure: export existing cases to JSON, retitle them in
/// the file, re-import, submit. The exported JSON only ever held the
/// plain-text read of the steps, so the submit used to write that back and
/// strip the markup - the same defect as a title-only save, reached by a
/// different route.
///
/// The submit path now reads the current Steps for every update row first,
/// so what matters is that a case which survived the round trip still
/// compares EQUAL to what Azure DevOps holds. This proves it does.
#[tokio::test]
async fn a_case_exported_and_reimported_does_not_rewrite_its_steps() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("tcm-roundtrip-{nanos}.json"));

    // What the Edit screen would hold after loading the case from ADO.
    let loaded = case_from(RICH_STEPS, "Original title");
    v2_lib::import_parser::export_queue_to_json(
        std::slice::from_ref(&loaded),
        path.to_str().unwrap(),
    )
    .unwrap();

    // Retitle it in the file, exactly as someone would.
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("Original title", "Renamed in the file")).unwrap();

    let (mut cases, _) = v2_lib::import_parser::parse_file(path.to_str().unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(cases.len(), 1);
    let reimported = cases.remove(0);
    assert_eq!(reimported.title, "Renamed in the file");
    assert_eq!(reimported.update_id, Some(55), "the id must survive, or it would CREATE");

    let body = captured_patch(&reimported, Some(RICH_STEPS)).await;
    assert!(body.contains("Renamed in the file"), "the retitle must be written");
    assert!(
        !body.contains("Microsoft.VSTS.TCM.Steps"),
        "the round trip must not rewrite the steps:\n{body}"
    );
}

/// The editor is not an import. Emptying the tags box and saving used to do
/// nothing at all and still report "Updated" - the blank-skip rule that
/// protects imports was governing the form as well.
#[tokio::test]
async fn the_editor_can_clear_a_field_but_an_import_still_cannot() {
    let blank = v2_lib::model::TestCase {
        title: "T".into(),
        steps: vec![v2_lib::steps_xml::Step { action: "a".into(), expected: "b".into() }],
        automation_status: "Planned".into(),
        tags: String::new(),
        module_value: String::new(),
        preconditions: String::new(),
        update_id: Some(55),
        ..Default::default()
    };

    for (policy, should_write) in [(BlankPolicy::Clear, true), (BlankPolicy::Skip, false)] {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "id": 55, "fields": {} }),
            ))
            .mount(&server)
            .await;
        AdoClient::with_base_url("t".into(), server.uri())
            .update_test_case_from_model(
                "o", "p", 55, &blank,
                Some("Custom.Module"), Some("Custom.Pre"), None, policy,
            )
            .await
            .unwrap();
        let body =
            String::from_utf8_lossy(&server.received_requests().await.unwrap()[0].body).to_string();
        for field in ["System.Tags", "Custom.Module", "Custom.Pre"] {
            assert_eq!(
                body.contains(field),
                should_write,
                "{policy:?} and {field}: expected written={should_write}\n{body}"
            );
        }
        // Clearing preconditions must write empty, not an empty <div> that
        // looks blank but is not.
        if should_write {
            assert!(!body.contains("<div>"), "cleared preconditions must be empty:\n{body}");
        }
    }
}

/// A field holding only spaces is blank to the person who left it that way.
/// Judged as content, a Skip import overwrote real tags with a space, sent a
/// module value that matches no picklist entry, and wrote a precondition of
/// "<div>   </div>" - the "looks blank but is not" state the Clear branch
/// already went out of its way to avoid.
#[tokio::test]
async fn a_field_of_only_spaces_counts_as_blank() {
    let spaces = v2_lib::model::TestCase {
        title: "T".into(),
        steps: vec![v2_lib::steps_xml::Step { action: "a".into(), expected: "b".into() }],
        automation_status: "Planned".into(),
        tags: "   ".into(),
        module_value: "\t ".into(),
        preconditions: " \n ".into(),
        update_id: Some(55),
        ..Default::default()
    };

    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 55, "fields": {} })),
        )
        .mount(&server)
        .await;
    AdoClient::with_base_url("t".into(), server.uri())
        .update_test_case_from_model(
            "o", "p", 55, &spaces,
            Some("Custom.Module"), Some("Custom.Pre"), None, BlankPolicy::Skip,
        )
        .await
        .unwrap();
    let body =
        String::from_utf8_lossy(&server.received_requests().await.unwrap()[0].body).to_string();
    for field in ["System.Tags", "Custom.Module", "Custom.Pre"] {
        assert!(!body.contains(field), "{field} was written from whitespace:\n{body}");
    }
    assert!(!body.contains("<div>"), "whitespace became a non-empty precondition:\n{body}");

    // And a real value still travels trimmed, not padded.
    let padded = v2_lib::model::TestCase { tags: "  smoke  ".into(), ..spaces };
    let server2 = MockServer::start().await;
    Mock::given(method("PATCH"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 55, "fields": {} })),
        )
        .mount(&server2)
        .await;
    AdoClient::with_base_url("t".into(), server2.uri())
        .update_test_case_from_model("o", "p", 55, &padded, None, None, None, BlankPolicy::Skip)
        .await
        .unwrap();
    let body2 =
        String::from_utf8_lossy(&server2.received_requests().await.unwrap()[0].body).to_string();
    assert!(body2.contains(r#""smoke""#), "tags were not trimmed:\n{body2}");
}
