//! The AI bridge while "Treat all projects as one" is on: what it reads in
//! the PBI's own project rather than the home project.

use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{route, BridgeContext};
use wiremock::matchers::{method as wm_method, path as wm_path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ctx(all_projects: bool) -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: Some("Custom.Module".into()),
        preconditions_ref: Some("Custom.Preconditions".into()),
        disabled_tools: vec![],
        working_dir: None,
        db_id: None,
        db_secrets: None,
        db_writes: false,
        api_writes: false,
        network_tools: false,
        all_projects,
        db_no_ask: false,
        gate: Default::default(),
    }
}

async fn ado_stub() -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

/// PBI `pbi` lives in `project`, which holds one plan with its requirement
/// suite and two points (one failed). Nothing is mounted for any other
/// project, so a request there answers 404.
async fn mount_pbi_in(server: &MockServer, project: &str, pbi: i64) {
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/_apis/wit/workitems/{pbi}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": pbi,
            "fields": { "System.TeamProject": project }
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/testplan/plans")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 9, "name": "Payroll Plan", "areaPath": project, "rootSuite": { "id": 90 } }]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/testplan/Plans/9/suites")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 91, "name": format!("{pbi} : Pay run"), "suiteType": "requirementTestSuite", "requirementId": pbi }]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/testplan/Plans/9/Suites/91/TestPoint")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {
                    "id": 7,
                    "testCaseReference": { "id": 201, "name": "Pay run closes" },
                    "configuration": { "name": "Windows 10" },
                    "results": { "outcome": "failed", "lastTestRunId": 3, "lastResultId": 30 }
                },
                {
                    "id": 8,
                    "testCaseReference": { "id": 202, "name": "Pay run opens" },
                    "configuration": { "name": "Windows 10" },
                    "results": { "outcome": "passed", "lastTestRunId": 3, "lastResultId": 31 }
                }
            ]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/test/Runs/3/Results/30")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comment": "Totals do not match.",
            "associatedBugs": [{ "id": "555" }]
        })))
        .mount(server)
        .await;
}

fn paths(reqs: &[wiremock::Request]) -> Vec<String> {
    reqs.iter().map(|r| r.url.path().to_string()).collect()
}

#[tokio::test]
async fn combined_run_results_read_the_pbis_own_project() {
    let (server, client) = ado_stub().await;
    // PBI 4201 is in Payroll while the home project (ctx.project) is Web.
    mount_pbi_in(&server, "Payroll", 4201).await;

    let (status, body) =
        route(&ctx(true), Some(&client), "GET", "/run-results?pbi=4201", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases_in_suite"], 2, "{body}");
    assert_eq!(v["matched"], 1);
    assert_eq!(v["results"][0]["case_id"], 201);
    assert_eq!(v["results"][0]["comment"], "Totals do not match.");

    let seen = paths(&server.received_requests().await.unwrap());
    assert!(
        seen.iter().any(|p| p.starts_with("/acme/Payroll/_apis/testplan/")),
        "the suite scan runs in the PBI's project: {seen:?}"
    );
    assert!(
        seen.iter().any(|p| p == "/acme/Payroll/_apis/test/Runs/3/Results/30"),
        "the result detail is read in the PBI's project: {seen:?}"
    );
    assert!(
        !seen.iter().any(|p| p.starts_with("/acme/Web/")),
        "nothing is asked of the home project: {seen:?}"
    );
}

#[tokio::test]
async fn combined_run_results_ask_for_a_pbis_project_once() {
    let (server, client) = ado_stub().await;
    mount_pbi_in(&server, "Payroll", 4202).await;

    for _ in 0..2 {
        let (status, body) =
            route(&ctx(true), Some(&client), "GET", "/run-results?pbi=4202", "", "2.3.1").await;
        assert_eq!(status, 200, "{body}");
    }
    let seen = paths(&server.received_requests().await.unwrap());
    let reads = seen.iter().filter(|p| *p == "/acme/_apis/wit/workitems/4202").count();
    assert_eq!(reads, 1, "the PBI's project does not change, so it is read once: {seen:?}");
}

#[tokio::test]
async fn switched_off_run_results_stay_in_the_chosen_project() {
    let (server, client) = ado_stub().await;
    // Off, the bridge reads the project it was given and never looks the
    // PBI up: the same requests as beta.7.
    mount_pbi_in(&server, "Web", 4203).await;

    let (status, body) =
        route(&ctx(false), Some(&client), "GET", "/run-results?pbi=4203", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let seen = paths(&server.received_requests().await.unwrap());
    assert!(
        !seen.iter().any(|p| p.starts_with("/acme/_apis/wit/workitems")),
        "no work item lookup when switched off: {seen:?}"
    );
    assert!(seen.iter().any(|p| p.starts_with("/acme/Web/_apis/testplan/")), "{seen:?}");
}

#[tokio::test]
async fn combined_run_results_name_a_pbi_that_is_not_in_the_organization() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems/4204"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(true), Some(&client), "GET", "/run-results?pbi=4204", "", "2.3.1").await;
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("4204"), "{body}");
}

async fn mount_projects(server: &MockServer, names: &[&str]) {
    let value: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(i, n)| serde_json::json!({ "id": format!("p{i}"), "name": n }))
        .collect();
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": value })))
        .mount(server)
        .await;
}

/// `project` holds plan `plan` with one requirement suite `suite` (for PBI
/// `plan * 10`) whose single point is test case `case`.
async fn mount_plan_in(server: &MockServer, project: &str, plan: i64, suite: i64, case: i64) {
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/testplan/plans")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": plan, "name": format!("{project} Plan"), "areaPath": project, "rootSuite": { "id": plan * 100 } }]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/testplan/Plans/{plan}/suites")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "id": plan * 100, "name": format!("{project} Plan"), "suiteType": "staticTestSuite" },
                { "id": suite, "name": format!("{} : Cases", plan * 10), "suiteType": "requirementTestSuite", "requirementId": plan * 10, "parentSuite": { "id": plan * 100 } }
            ]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/testplan/Plans/{plan}/Suites/{suite}/TestPoint")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 1,
                "testCaseReference": { "id": case, "name": "A case" },
                "configuration": { "name": "Windows 10" }
            }]
        })))
        .mount(server)
        .await;
}

async fn mount_case(server: &MockServer, case: i64) {
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": case, "fields": { "System.Title": "A case" } }]
        })))
        .mount(server)
        .await;
}

async fn mount_work_item_project(server: &MockServer, id: i64, project: &str) {
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/_apis/wit/workitems/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id,
            "fields": { "System.TeamProject": project }
        })))
        .mount(server)
        .await;
}

async fn mount_pbi_hits(server: &MockServer, wiql_path: &str) {
    Mock::given(wm_method("POST"))
        .and(wm_path(wiql_path))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": [{ "id": 42 }, { "id": 7 }]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "id": 42, "fields": { "System.Title": "Pay run", "System.WorkItemType": "Product Backlog Item", "System.TeamProject": "Payroll" } },
                { "id": 7, "fields": { "System.Title": "Login", "System.WorkItemType": "Product Backlog Item", "System.TeamProject": "Web" } }
            ]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn combined_pbi_search_names_each_hits_project() {
    let (server, client) = ado_stub().await;
    mount_pbi_hits(&server, "/acme/_apis/wit/wiql").await;

    let (status, body) =
        route(&ctx(true), Some(&client), "GET", "/search-pbis?q=pay", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["pbis"][0]["id"], 42);
    assert_eq!(v["pbis"][0]["project"], "Payroll");
    assert_eq!(v["pbis"][1]["id"], 7);
    assert_eq!(v["pbis"][1]["project"], "Web");
}

#[tokio::test]
async fn switched_off_pbi_search_answers_exactly_as_before() {
    let (server, client) = ado_stub().await;
    // Off, the query is scoped to the home project and the answer has no
    // `project` key at all.
    mount_pbi_hits(&server, "/acme/Web/_apis/wit/wiql").await;

    let (status, body) =
        route(&ctx(false), Some(&client), "GET", "/search-pbis?q=pay", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        r#"{"pbis":[{"id":42,"title":"Pay run","work_item_type":"Product Backlog Item"},{"id":7,"title":"Login","work_item_type":"Product Backlog Item"}]}"#
    );
}

#[tokio::test]
async fn combined_suite_search_lists_every_projects_suites_with_their_project() {
    let (server, client) = ado_stub().await;
    mount_projects(&server, &["Web", "Payroll", "Locked"]).await;
    mount_plan_in(&server, "Web", 8, 801, 301).await;
    mount_plan_in(&server, "Payroll", 9, 901, 302).await;
    // A project the person may not read is skipped, and the answer says so.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Locked/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let (status, body) = route(&ctx(true), Some(&client), "GET", "/suites", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let rows = v["suites"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{body}");
    // Projects in name order: Payroll, then Web.
    assert_eq!(rows[0]["project"], "Payroll");
    assert_eq!(rows[0]["plan_id"], 9);
    assert_eq!(rows[0]["suite_id"], 901);
    assert_eq!(rows[1]["project"], "Web");
    assert_eq!(rows[1]["plan_id"], 8);
    assert_eq!(v["total"], 2);
    assert_eq!(v["projects_shown"], 2, "{body}");
    assert_eq!(v["projects_total"], 3, "{body}");
    assert!(v["note"].as_str().unwrap().contains("2 of 3 projects"), "{body}");

    // The filter still reaches a requirement suite's PBI.
    let (_, body) = route(&ctx(true), Some(&client), "GET", "/suites?q=90", "", "2.3.1").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["suites"].as_array().unwrap().len(), 1, "{body}");
    assert_eq!(v["suites"][0]["project"], "Payroll");
}

#[tokio::test]
async fn combined_suite_search_reads_only_the_capped_projects() {
    let (server, client) = ado_stub().await;
    let many: Vec<String> = (0..102).map(|i| format!("P{i:03}")).collect();
    let names: Vec<&str> = many.iter().map(String::as_str).collect();
    mount_projects(&server, &names).await;
    Mock::given(wm_method("GET"))
        .and(path_regex(r"^/acme/[^/]+/_apis/testplan/plans$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [] })))
        .mount(&server)
        .await;

    let (status, body) = route(&ctx(true), Some(&client), "GET", "/suites", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["projects_shown"], 100, "{body}");
    assert_eq!(v["projects_total"], 102, "{body}");
    assert!(v["note"].as_str().unwrap().contains("100 of 102 projects"), "{body}");
    let seen = paths(&server.received_requests().await.unwrap());
    assert!(
        !seen.iter().any(|p| p.starts_with("/acme/P101/") || p.starts_with("/acme/P100/")),
        "{seen:?}"
    );
}

#[tokio::test]
async fn switched_off_suite_search_answers_exactly_as_before() {
    let (server, client) = ado_stub().await;
    mount_plan_in(&server, "Web", 8, 801, 301).await;

    let (status, body) = route(&ctx(false), Some(&client), "GET", "/suites", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["suites"][0].get("project").is_none(), "{body}");
    assert!(v.get("projects_shown").is_none(), "{body}");
    let seen = paths(&server.received_requests().await.unwrap());
    assert!(!seen.iter().any(|p| p == "/acme/_apis/projects"), "no project list when off: {seen:?}");
}

#[tokio::test]
async fn suite_cases_find_the_plans_project_from_the_cached_trees() {
    let (server, client) = ado_stub().await;
    mount_projects(&server, &["Web", "Payroll"]).await;
    mount_plan_in(&server, "Web", 8, 801, 301).await;
    mount_plan_in(&server, "Payroll", 9, 901, 302).await;
    mount_case(&server, 302).await;
    // The suite search reads every project's tree and keeps them.
    let (status, body) = route(&ctx(true), Some(&client), "GET", "/suites", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");

    let (status, body) =
        route(&ctx(true), Some(&client), "GET", "/suite-cases?plan=9&suite=901", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["project"], "Payroll", "{body}");
    assert_eq!(v["test_cases"][0]["id"], 302);

    let seen = paths(&server.received_requests().await.unwrap());
    assert!(
        seen.iter().any(|p| p == "/acme/Payroll/_apis/testplan/Plans/9/Suites/901/TestPoint"),
        "the points are read in the plan's project: {seen:?}"
    );
    assert!(
        !seen.iter().any(|p| p == "/acme/_apis/wit/workitems/9"),
        "the cached tree answers, so Azure DevOps is not asked: {seen:?}"
    );
    assert!(!seen.iter().any(|p| p.starts_with("/acme/Web/_apis/testplan/Plans/9/")), "{seen:?}");
}

#[tokio::test]
async fn suite_cases_ask_azure_devops_for_a_plans_project_only_once() {
    let (server, client) = ado_stub().await;
    // No tree is cached and the project list is not there either, so the
    // plan is a work item and its project comes from one read.
    mount_plan_in(&server, "Payroll", 9, 901, 302).await;
    mount_case(&server, 302).await;
    mount_work_item_project(&server, 9, "Payroll").await;

    for _ in 0..2 {
        let (status, body) =
            route(&ctx(true), Some(&client), "GET", "/suite-cases?plan=9&suite=901", "", "2.3.1").await;
        assert_eq!(status, 200, "{body}");
    }
    let seen = paths(&server.received_requests().await.unwrap());
    let reads = seen.iter().filter(|p| *p == "/acme/_apis/wit/workitems/9").count();
    assert_eq!(reads, 1, "{seen:?}");
    assert!(
        seen.iter().any(|p| p == "/acme/Payroll/_apis/testplan/Plans/9/Suites/901/TestPoint"),
        "{seen:?}"
    );
}

#[tokio::test]
async fn an_explicit_project_wins() {
    let (server, client) = ado_stub().await;
    mount_plan_in(&server, "Payroll", 9, 901, 302).await;
    mount_case(&server, 302).await;

    let (status, body) = route(
        &ctx(true),
        Some(&client),
        "GET",
        "/suite-cases?plan=9&suite=901&project=Payroll",
        "",
        "2.3.1",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let seen = paths(&server.received_requests().await.unwrap());
    assert!(
        !seen.iter().any(|p| p.starts_with("/acme/_apis/wit/workitems/") || p == "/acme/_apis/projects"),
        "nothing is looked up when the caller names the project: {seen:?}"
    );
    assert!(
        seen.iter().any(|p| p == "/acme/Payroll/_apis/testplan/Plans/9/Suites/901/TestPoint"),
        "{seen:?}"
    );
}

#[tokio::test]
async fn switched_off_suite_cases_stay_in_the_chosen_project() {
    let (server, client) = ado_stub().await;
    mount_plan_in(&server, "Web", 8, 801, 301).await;
    mount_case(&server, 301).await;

    // Off, a `project` in the request is not a thing: the chosen project is
    // read, with no lookup, exactly as before.
    let (status, body) = route(
        &ctx(false),
        Some(&client),
        "GET",
        "/suite-cases?plan=8&suite=801&project=Payroll",
        "",
        "2.3.1",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v.get("project").is_none(), "{body}");
    let seen = paths(&server.received_requests().await.unwrap());
    assert!(
        seen.iter().any(|p| p == "/acme/Web/_apis/testplan/Plans/8/Suites/801/TestPoint"),
        "{seen:?}"
    );
    assert!(!seen.iter().any(|p| p.starts_with("/acme/_apis/wit/workitems/")), "{seen:?}");
}

#[tokio::test]
async fn combined_suite_cases_name_a_plan_that_is_not_in_the_organization() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems/77"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(true), Some(&client), "GET", "/suite-cases?plan=77&suite=1", "", "2.3.1").await;
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("77"), "{body}");
}

fn wiki_hits() -> serde_json::Value {
    serde_json::json!({
        "results": [
            {
                "fileName": "Pay-Run.md",
                "path": "/Docs/Pay-Run",
                "project": { "id": "p1", "name": "Payroll" },
                "wiki": { "id": "w1", "name": "Payroll.wiki" },
                "hits": [{ "highlights": ["the <hl>pay</hl> run"] }]
            },
            {
                "fileName": "Login.md",
                "path": "/Docs/Login",
                "project": { "id": "p2", "name": "Web" },
                "wiki": { "id": "w2", "name": "Web.wiki" },
                "hits": [{ "highlights": ["login"] }]
            }
        ]
    })
}

#[tokio::test]
async fn combined_wiki_search_is_organization_wide_and_names_each_pages_project() {
    let (server, client) = ado_stub().await;
    // The organization-level search, not one project's.
    Mock::given(wm_method("POST"))
        .and(wm_path("/acme/_apis/search/wikisearchresults"))
        .respond_with(ResponseTemplate::new(200).set_body_json(wiki_hits()))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(true), Some(&client), "GET", "/search-wiki?q=pay", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["results"][0]["wiki_id"], "w1");
    assert_eq!(v["results"][0]["project"], "Payroll");
    assert_eq!(v["results"][1]["wiki_id"], "w2");
    assert_eq!(v["results"][1]["project"], "Web");
}

#[tokio::test]
async fn switched_off_wiki_search_stays_in_the_chosen_project_and_names_none() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("POST"))
        .and(wm_path("/acme/Web/_apis/search/wikisearchresults"))
        .respond_with(ResponseTemplate::new(200).set_body_json(wiki_hits()))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(false), Some(&client), "GET", "/search-wiki?q=pay", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["results"][0].get("project").is_none(), "{body}");
    assert_eq!(v["results"][0]["wiki_id"], "w1");
}

#[tokio::test]
async fn the_tool_policy_answer_says_whether_projects_are_combined() {
    for on in [true, false] {
        let (status, body) = route(&ctx(on), None, "GET", "/tools", "", "2.3.1").await;
        assert_eq!(status, 200, "{body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["all_projects"], on, "{body}");
    }
}

async fn mount_wiki_page(server: &MockServer, project: &str) {
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/wiki/wikis/w1/pages")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Docs/Pay-Run",
            "content": format!("# Pay run in {project}")
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_wiki_page_is_read_in_its_hits_project() {
    let (server, client) = ado_stub().await;
    mount_wiki_page(&server, "Payroll").await;

    let (status, body) = route(
        &ctx(true),
        Some(&client),
        "GET",
        "/wiki-page?wiki=w1&path=/Docs/Pay-Run&project=Payroll",
        "",
        "2.3.1",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("Pay run in Payroll"), "{body}");
}

#[tokio::test]
async fn a_pasted_wiki_url_is_read_in_the_project_it_names() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Payroll/_apis/wiki/wikis/Payroll.wiki/pages/12"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Docs/Pay-Run",
            "content": "# Pay run from the url"
        })))
        .mount(&server)
        .await;

    // No `project`: the URL's own project is the one that holds the page.
    let url = "https%3A%2F%2Fdev.azure.com%2Facme%2FPayroll%2F_wiki%2Fwikis%2FPayroll.wiki%2F12%2FPay-Run";
    let (status, body) = route(
        &ctx(true),
        Some(&client),
        "GET",
        &format!("/wiki-page?path={url}"),
        "",
        "2.3.1",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("from the url"), "{body}");
}

#[tokio::test]
async fn a_wiki_page_with_no_project_is_read_in_the_home_project() {
    let (server, client) = ado_stub().await;
    mount_wiki_page(&server, "Web").await;

    let (status, body) = route(
        &ctx(true),
        Some(&client),
        "GET",
        "/wiki-page?wiki=w1&path=/Docs/Pay-Run",
        "",
        "2.3.1",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("Pay run in Web"), "{body}");
}

#[tokio::test]
async fn switched_off_a_wiki_page_ignores_a_project_in_the_request() {
    let (server, client) = ado_stub().await;
    mount_wiki_page(&server, "Web").await;

    let (status, body) = route(
        &ctx(false),
        Some(&client),
        "GET",
        "/wiki-page?wiki=w1&path=/Docs/Pay-Run&project=Payroll",
        "",
        "2.3.1",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("Pay run in Web"), "{body}");
}

/// A writing job (the intake's `ready` answer) in `c`'s organization, whose
/// answers name `pbi` as the reference, or no PBI.
async fn begin_job_naming(c: &BridgeContext, dir: &std::path::Path, pbi: Option<i32>) {
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    let mut answers = serde_json::json!({
        "output_path": "pay.json",
        "spec_paths": [spec.to_string_lossy()],
        "authority": "spec",
        "ordering": "tester",
        "reference_cases": "none",
    });
    if let Some(p) = pbi {
        answers["examples_pbi"] = serde_json::json!(p);
    }
    let (status, body) =
        route(c, None, "POST", "/begin?feature=Pay%20run", &answers.to_string(), "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"ready\""), "{body}");
}

async fn mount_tags(server: &MockServer, org: &str, project: &str, names: &[&str]) {
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/{org}/{project}/_apis/wit/tags")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": names.iter().map(|n| serde_json::json!({ "name": n })).collect::<Vec<_>>()
        })))
        .mount(server)
        .await;
}

/// The cache and the job record have no lock, so the tags test owns an
/// organization name no other test reads.
fn tags_ctx(all_projects: bool, dir: &std::path::Path) -> BridgeContext {
    let mut c = ctx(all_projects);
    c.org = "tagsorg-p6".into();
    c.module_ref = None;
    c.working_dir = Some(dir.to_string_lossy().to_string());
    c
}

#[tokio::test]
async fn combined_tags_follow_the_writing_jobs_pbi_project() {
    for p in ["Web", "Payroll"] {
        v2_lib::cache::forget(&v2_lib::cache::keys::tags("tagsorg-p6", p));
    }
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/tagsorg-p6/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 42, "fields": { "System.TeamProject": "Payroll" }
        })))
        .mount(&server)
        .await;
    mount_tags(&server, "tagsorg-p6", "Web", &["web-only"]).await;
    mount_tags(&server, "tagsorg-p6", "Payroll", &["payroll-only"]).await;

    let dir = tempfile::tempdir().unwrap();
    let on = tags_ctx(true, dir.path());
    // No job yet: the home project's tags.
    let (status, body) = route(&on, Some(&client), "GET", "/tags", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["tags"], serde_json::json!(["web-only"]), "{body}");
    assert_eq!(v["project"], "Web", "{body}");

    // A job that names PBI 42 (in Payroll): that project's tags.
    begin_job_naming(&on, dir.path(), Some(42)).await;
    let (status, body) = route(&on, Some(&client), "GET", "/tags", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["tags"], serde_json::json!(["payroll-only"]), "{body}");
    assert_eq!(v["project"], "Payroll", "{body}");

    // Switched off, the job's PBI is not a thing: the chosen project, and
    // no `project` key in the answer.
    let off = tags_ctx(false, dir.path());
    let (status, body) = route(&off, Some(&client), "GET", "/tags", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["tags"], serde_json::json!(["web-only"]), "{body}");
    assert!(v.get("project").is_none(), "{body}");

    // A later job that names no PBI puts the tags back at home.
    begin_job_naming(&on, dir.path(), None).await;
    let (_, body) = route(&on, Some(&client), "GET", "/tags", "", "2.3.1").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["project"], "Web", "{body}");
}
