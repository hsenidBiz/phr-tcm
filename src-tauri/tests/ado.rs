use v2_lib::ado::{AdoClient, AdoError, BlankPolicy};
use wiremock::matchers::{header, method, path, query_param};
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
        spec_order: None,
        tester_order: None,
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
        .update_test_case_from_model("org", "proj", 55, &tc, Some("Custom.Module"), Some("Custom.Prec"), None, None, BlankPolicy::Skip)
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

/// The decision table behind tag REMOVAL. Azure DevOps merges tag writes
/// made with the plain `add` op - it appends the listed tags and never
/// removes one - so which op the update sends has to depend on what the
/// work item currently holds. This was the "removed a tag through Import
/// File, nothing happened" bug, and the editor's clear-all-tags shared it.
#[test]
fn tags_write_ops_picks_the_op_that_can_actually_remove() {
    use v2_lib::ado::tags_write_ops;
    let kinds = |ops: &[serde_json::Value]| -> Vec<String> {
        ops.iter().map(|o| o["op"].as_str().unwrap().to_string()).collect()
    };

    // Item HAS tags: replace sets the exact final list...
    let ops = tags_write_ops("smoke", Some("smoke; regression"));
    assert_eq!(kinds(&ops), ["replace"]);
    assert_eq!(ops[0]["value"], "smoke");
    // ...and "" clears everything (the editor's clear-all).
    let ops = tags_write_ops("", Some("smoke; regression"));
    assert_eq!(kinds(&ops), ["replace"]);
    assert_eq!(ops[0]["value"], "");
    // Nothing changed: nothing sent.
    assert!(tags_write_ops("smoke", Some("smoke")).is_empty());

    // Item has NO tags: nothing to remove - plain add creates the field,
    // and clearing an already-clear field needs no op at all.
    assert_eq!(kinds(&tags_write_ops("smoke", Some(""))), ["add"]);
    assert!(tags_write_ops("", Some("  ")).is_empty());

    // Current value unknown (the baseline read failed): add guarantees the
    // path exists, replace makes it exact - in that order, one document.
    assert_eq!(kinds(&tags_write_ops("smoke", None)), ["add", "replace"]);
    // ...but never a blind clear, which could fail the whole PATCH on a
    // tagless item and lose the title/steps update with it.
    assert!(tags_write_ops("", None).is_empty());
}

/// End to end through the client: an update that DROPS a tag sends the
/// `replace` op, not the merging `add` the other fields ride on.
#[tokio::test]
async fn update_from_model_removes_a_tag_with_the_replace_op() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/wit/workitems/55"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 55})))
        .mount(&server)
        .await;

    let mut tc = sample_tc();
    tc.tags = "smoke".into(); // "regression" was removed in the import file
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    client
        .update_test_case_from_model(
            "org", "proj", 55, &tc, None, None, None,
            Some("smoke; regression"),
            BlankPolicy::Skip,
        )
        .await
        .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let tag_ops: Vec<&serde_json::Value> = body
        .as_array()
        .unwrap()
        .iter()
        .filter(|op| op["path"] == "/fields/System.Tags")
        .collect();
    assert_eq!(tag_ops.len(), 1, "{body}");
    assert_eq!(tag_ops[0]["op"], "replace", "an add here merges and cannot remove: {body}");
    assert_eq!(tag_ops[0]["value"], "smoke");
    // Every other field still uses the create-or-replace add.
    for op in body.as_array().unwrap() {
        if op["path"] != "/fields/System.Tags" {
            assert_eq!(op["op"], "add", "{body}");
        }
    }
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
        .update_test_case_from_model("org", "proj", 55, &tc, None, None, None, None, BlankPolicy::Skip)
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

/// `comment` and `reviewer_notes` are the app's own. They round-trip
/// through the exported JSON and must never become an ADO field.
///
/// The `NoCommentInBody` matcher below is mounted on exactly one mock -
/// the POST that creates a case. Every update test builds from
/// `sample_tc()`, which leaves both fields empty, so a stray
/// `fields.push(("Custom.ReviewerNotes", ...))` in
/// `update_test_case_from_model` would have shipped green. This scans the
/// source the way the DELETE rule is scanned: a wiremock matcher cannot
/// survive someone adding a new write function, and a scan can.
#[test]
fn app_only_fields_never_reach_a_request_body() {
    let src = include_str!("../src/ado/endpoints.rs");

    // Every TestCase field the request builders may read. Adding an entry
    // is a deliberate decision to send something to Azure DevOps.
    const MAPPABLE: [&str; 7] = [
        "title",
        "steps",
        "tags",
        "automation_status",
        "module_value",
        "preconditions",
        "update_id",
    ];

    let bytes = src.as_bytes();
    for (i, _) in src.match_indices("tc.") {
        // "etc." and friends are not field reads.
        if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
            continue;
        }
        let rest = &src[i + 3..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let field = &rest[..end];
        assert!(
            MAPPABLE.contains(&field),
            "endpoints.rs reads tc.{field}. If that is one of the app-only fields it must \
             never reach ADO; if it is genuinely a new ADO field, add it to MAPPABLE on purpose."
        );
    }

    // And the app-only names appear nowhere in request-building code at
    // all, under any spelling of the binding.
    assert!(
        !src.contains("reviewer_notes"),
        "reviewer_notes must not appear in request-building code"
    );
}

/// The one sanctioned exception, and it is held to a TIGHTER rule than the
/// files above rather than a looser one.
///
/// `deletion.rs` may issue DELETE - that is its whole reason to exist. It
/// targets the Test Management endpoint (`_apis/test/testcase/{id}`), the
/// only deletion Azure DevOps offers for test artifacts, and a PERMANENT
/// one - authorized explicitly on 2026-08-11 after the work-item endpoint
/// refused test cases outright. What stays banned is the work-item
/// endpoint's own permanent-erase query parameter: the wit route must
/// never grow back, and the parameter's name appearing nowhere in the
/// file - prose included - is what keeps the ban unsoftenable.
#[test]
fn the_only_delete_is_the_test_management_one() {
    let deletion = include_str!("../src/ado/deletion.rs");

    // It really is the delete path - otherwise this test passes vacuously
    // if someone renames the file or moves the call out of it.
    assert!(
        deletion.contains(".delete("),
        "deletion.rs is supposed to be the file that deletes"
    );

    // And it is the test-management endpoint, not the work-item one.
    assert!(
        deletion.contains("_apis/test/testcases/"),
        "the delete must go through the Test Management API"
    );
    assert!(
        !deletion.contains("_apis/wit/workitems"),
        "the work-item delete endpoint must not come back - it refuses test artifacts, and its query parameter is the banned one"
    );

    // Case-insensitive: the wit permanent-erase parameter, in any casing,
    // in code or prose - none of it, so this assertion can never be
    // weakened by a comment mentioning it.
    let lowered = deletion.to_lowercase();
    assert!(
        !lowered.contains("destroy"),
        "deletion.rs must never mention the wit erase parameter by name"
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
                .update_test_case_from_model("o", "p", 7, &tc, None, Some("Custom.Pre"), None, None, BlankPolicy::Skip)
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
        .update_test_case_from_model("o", "p", 55, tc, None, None, original, None, BlankPolicy::Skip)
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
                Some("Custom.Module"), Some("Custom.Pre"), None,
                // The editor always has the case it started from, so a
                // clear always knows the current tags. (With an UNKNOWN
                // baseline a clear is deliberately not attempted - see
                // tags_write_ops.)
                Some("old-tag"),
                policy,
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
            // And the tags clear rides the op that can actually remove -
            // an `add` here merges nothing and leaves every tag in place.
            let ops: serde_json::Value = serde_json::from_str(&body).unwrap();
            let tag_op = ops
                .as_array()
                .unwrap()
                .iter()
                .find(|op| op["path"] == "/fields/System.Tags")
                .unwrap();
            assert_eq!(tag_op["op"], "replace", "{body}");
            assert_eq!(tag_op["value"], "", "{body}");
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
            Some("Custom.Module"), Some("Custom.Pre"), None, None, BlankPolicy::Skip,
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
        .update_test_case_from_model("o", "p", 55, &padded, None, None, None, None, BlankPolicy::Skip)
        .await
        .unwrap();
    let body2 =
        String::from_utf8_lossy(&server2.received_requests().await.unwrap()[0].body).to_string();
    assert!(body2.contains(r#""smoke""#), "tags were not trimmed:\n{body2}");
}

/// Audit finding R-3: `TestedBy-Forward` (a PBI's test cases) and
/// `TestedBy-Reverse` ("Tests", pointing the other way) are DIFFERENT
/// links, but the filter matched the substring "testedby", so both passed.
/// Asking for the test cases of an id that was itself a Test Case followed
/// the reverse link and returned the PBI dressed as a test case.
#[tokio::test]
async fn only_the_forward_tested_by_link_yields_test_cases() {
    let server = MockServer::start().await;
    // A work item carrying BOTH directions: 900 is a real test case of it,
    // 500 is the PBI that this item tests.
    Mock::given(method("GET"))
        .and(path("/o/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 42,
            "relations": [
                { "rel": "Microsoft.VSTS.Common.TestedBy-Forward",
                  "url": "https://dev.azure.com/o/_apis/wit/workItems/900" },
                { "rel": "Microsoft.VSTS.Common.TestedBy-Reverse",
                  "url": "https://dev.azure.com/o/_apis/wit/workItems/500" },
            ]
        })))
        .mount(&server)
        .await;
    // Only 900 may be fetched. A request for 500 would mean the reverse
    // link leaked through - so the batch stub answers for 900 alone.
    Mock::given(method("GET"))
        .and(path("/o/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 900, "fields": { "System.Title": "Real test case" } }]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let cases = client.get_pbi_test_cases_full("o", 42, None, None).await.unwrap();
    assert_eq!(cases.len(), 1, "only the forward link is a test case");
    assert_eq!(cases[0].id, 900);

    let asked = server.received_requests().await.unwrap();
    let batch = asked
        .iter()
        .find(|r| r.url.path().ends_with("/_apis/wit/workitems"))
        .expect("the batch fetch happened");
    let ids = batch.url.query_pairs().find(|(k, _)| k == "ids").unwrap().1.to_string();
    assert!(!ids.contains("500"), "the reverse ('Tests') link must not be followed: ids={ids}");
}

/// Audit finding R-6: org and project were interpolated into URLs raw
/// while other screens encoded them. A percent sign is legal in an ADO
/// project name and is not UI-restricted, so "50% Done" produced an
/// invalid escape sequence on the wire.
#[tokio::test]
async fn org_and_project_are_percent_encoded_in_urls() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": []
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let _ = client.search_pbis("my org", "50% Done", "login", 10).await;

    let asked = server.received_requests().await.unwrap();
    let raw = asked[0].url.as_str();
    assert!(raw.contains("my%20org"), "space in the org must be encoded: {raw}");
    assert!(raw.contains("50%25%20Done"), "percent AND space must be encoded: {raw}");
    assert!(
        !raw.contains("50% Done"),
        "the raw name must not reach the wire: {raw}"
    );
}

/// Audit finding R-2: wiki SEARCH reports the backing FILE path
/// ("/Auth-Flow.md", ADO's gitItemPath namespace) while the page-fetch
/// parameter documents a PAGE path ("/Auth Flow"). Handing a search hit
/// straight back therefore asked for a page whose name ends in ".md".
#[tokio::test]
async fn a_wiki_search_hit_is_normalised_before_the_page_is_fetched() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Auth-Flow", "content": "# Auth"
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());

    // Exactly what search_wiki hands back.
    client.get_wiki_page("o", "p", "wiki-1", "/Auth-Flow.md").await.unwrap();
    // A path with no leading slash is rooted rather than sent relative.
    client.get_wiki_page("o", "p", "wiki-1", "Home").await.unwrap();

    let asked = server.received_requests().await.unwrap();
    let sent: Vec<String> = asked
        .iter()
        .map(|r| r.url.query_pairs().find(|(k, _)| k == "path").unwrap().1.to_string())
        .collect();
    assert_eq!(sent[0], "/Auth-Flow", "the .md extension must be dropped");
    assert_eq!(sent[1], "/Home", "a bare name is rooted");
}

/// What a person actually has is the URL in their address bar. It carries
/// the wiki name and the page id, and the id route is exact - there is no
/// path namespace left to guess at - so `wiki_id` becomes redundant and a
/// blank one must not stop the call.
#[tokio::test]
async fn a_wiki_url_is_fetched_by_its_page_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/PeoplesHR/HRM/_apis/wiki/wikis/HRM.wiki/pages/9486"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Issue Meal", "content": "# Issue Meal"
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());

    let page = client
        .get_wiki_page(
            "PeoplesHR",
            "HRM",
            "",
            "https://dev.azure.com/PeoplesHR/HRM/_wiki/wikis/HRM.wiki/9486/Issue-Meal",
        )
        .await
        .unwrap();
    assert_eq!(page.path, "/Issue Meal");
}

/// The other URL shape ADO hands out: no id, a `pagePath` query instead.
#[tokio::test]
async fn a_wiki_url_carrying_a_pagepath_uses_that_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wiki/wikis/HRM.wiki/pages"))
        .and(query_param("path", "/Issue Meal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Issue Meal", "content": "# Issue Meal"
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());

    let page = client
        .get_wiki_page(
            "o",
            "p",
            "",
            "https://dev.azure.com/o/p/_wiki/wikis/HRM.wiki?pagePath=%2FIssue%20Meal",
        )
        .await
        .unwrap();
    assert_eq!(page.content, "# Issue Meal");
}

/// A search hit pasted back verbatim. ADO's file namespace writes a space
/// as `-` and a real hyphen as `%2D`, so the reversal IS unambiguous - the
/// ambiguity the old code refused to guess at only exists once the `%2D`
/// has been decoded away. Verbatim is still tried first; the reversal is
/// what happens after that 404s.
#[tokio::test]
async fn a_search_path_is_reinterpreted_only_after_the_verbatim_one_404s() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("path", "/HRMWiki/DAB-%2D-Data-API-Builder/High-Level-Design"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("path", "/HRMWiki/DAB - Data API Builder/High Level Design"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/HRMWiki/DAB - Data API Builder/High Level Design",
            "content": "# High Level Design"
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());

    let page = client
        .get_wiki_page("o", "p", "w1", "/HRMWiki/DAB-%2D-Data-API-Builder/High-Level-Design.md")
        .await
        .unwrap();
    assert_eq!(page.content, "# High Level Design");
}

/// The guard on the whole idea: a page whose title really contains a hyphen
/// resolves on the first request, and the reversal must never run. Without
/// this, "Auth-Flow" quietly becomes "Auth Flow" - the exact silent swap the
/// old code avoided by converting nothing at all.
#[tokio::test]
async fn a_path_that_resolves_verbatim_is_never_reinterpreted() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("path", "/Auth-Flow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Auth-Flow", "content": "# Auth"
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());

    let page = client.get_wiki_page("o", "p", "w1", "/Auth-Flow.md").await.unwrap();
    assert_eq!(page.path, "/Auth-Flow");
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "resolving verbatim must not trigger a second, reinterpreted fetch"
    );
}
