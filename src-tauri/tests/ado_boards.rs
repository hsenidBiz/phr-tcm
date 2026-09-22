//! The Boards "Add Test" route - the internal portal controller the app
//! falls back to when Azure DevOps refuses to create a requirement suite
//! for the account's access level (design
//! docs/superpowers/specs/2026-09-22-requirement-suite-boards-fallback-design.md).
//!
//! Everything here is wiremock: the route, the two ids its URL needs, and
//! the fallback that strings them together and ends with the suite the
//! plan holds. The body's field names are UNCONFIRMED until the probe
//! answers 200 - these tests pin the shape the code sends, so the day the
//! probe names a different field there is exactly one place to change.

use v2_lib::ado::{AdoClient, AdoError};
use v2_lib::ado_testplan::boards::{boards_body, BoardsOutcome, BOARDS_ROUTE_VERSION};
use v2_lib::ado_testplan::EnsuredSuite;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The org, project and ids from the watched session (design §6).
const ORG: &str = "PeoplesHR";
const PROJECT: &str = "HRM";
const PROJECT_ID: &str = "73d6b311-d948-40cc-8c86-fc600c1edb87";
const GAMMA_ID: &str = "71215fd9-79d5-4742-8756-01821a02e014";
const ALPHA_ID: &str = "11111111-2222-3333-4444-555555555555";
const DEFAULT_TEAM_ID: &str = "99999999-8888-7777-6666-555555555555";
const PBI: i32 = 145386;

fn route_path() -> String {
    format!("/{ORG}/{PROJECT_ID}/_api/_testManagement/AddWitTestCasesToRequirementSuite")
}

/// The reply the projects call makes, carrying both ids the route needs.
fn project_reply() -> serde_json::Value {
    serde_json::json!({
        "id": PROJECT_ID,
        "name": PROJECT,
        "defaultTeam": {"id": DEFAULT_TEAM_ID, "name": "HRM Team"},
    })
}

fn teams_reply(teams: &[(&str, &str)]) -> serde_json::Value {
    serde_json::json!({
        "value": teams
            .iter()
            .map(|(id, name)| serde_json::json!({"id": id, "name": name}))
            .collect::<Vec<_>>()
    })
}

fn scope_reply(value: &str, include_children: bool) -> serde_json::Value {
    serde_json::json!({
        "field": {"referenceName": "System.AreaPath"},
        "values": [{"value": value, "includeChildren": include_children}],
    })
}

async fn mount_team_scope(server: &MockServer, team_id: &str, value: &str, include_children: bool) {
    Mock::given(method("GET"))
        .and(path(format!(
            "/{ORG}/{PROJECT_ID}/{team_id}/_apis/work/teamsettings/teamfieldvalues"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(scope_reply(value, include_children)))
        .mount(server)
        .await;
}

/// The controller's sibling call sends `{"userStoryIds":"[145386]"}` - JSON
/// whose array values are JSON strings (design §4.1). The add call is
/// built the same way, with the field names still unconfirmed.
#[test]
fn the_body_follows_the_controllers_convention() {
    assert_eq!(
        boards_body(PBI, &[157941, 157801]),
        serde_json::json!({"requirementId": 145386, "testCaseIds": "[157941,157801]"})
    );
    assert_eq!(
        boards_body(PBI, &[157941]),
        serde_json::json!({"requirementId": 145386, "testCaseIds": "[157941]"})
    );
    // The version the route was watched at. It is a constant so a bump
    // shows up as one edit with the date beside it.
    assert_eq!(BOARDS_ROUTE_VERSION, "5");
}

#[tokio::test]
async fn boards_route_posts_the_body_and_reads_the_plan_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .and(query_param("teamId", GAMMA_ID))
        .and(query_param("__v", "5"))
        .and(body_json(boards_body(PBI, &[157941])))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "requirementId": PBI,
            "testPlanId": 157942,
            "testPoints": [],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let plan_id = client
        .boards_add_to_requirement_suite(ORG, PROJECT_ID, GAMMA_ID, PBI, &[157941])
        .await
        .unwrap();
    assert_eq!(plan_id, 157942);
    server.verify().await;
}

/// A reply without `testPlanId` is the endpoint having changed shape. The
/// suite is then found under a plan nobody named, so there is nothing to
/// guess from - say what came back instead.
#[tokio::test]
async fn boards_route_without_a_plan_id_is_an_error_not_a_guess() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client
        .boards_add_to_requirement_suite(ORG, PROJECT_ID, GAMMA_ID, PBI, &[157941])
        .await
        .unwrap_err();
    match err {
        AdoError::Http { status, body } => {
            assert_eq!(status, 0);
            assert!(body.contains("testPlanId"), "{body}");
        }
        other => panic!("expected an Http error naming testPlanId, got {other:?}"),
    }
}

/// The probe reads these: a 403 is the access level again (and the route is
/// no use), a 400 names the field the body got wrong.
#[tokio::test]
async fn boards_route_refusals_come_back_as_the_usual_variants() {
    let forbidding = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "message": "You are not authorized to access this API.",
        })))
        .mount(&forbidding)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), forbidding.uri(), forbidding.uri());
    let err = client
        .boards_add_to_requirement_suite(ORG, PROJECT_ID, GAMMA_ID, PBI, &[157941])
        .await
        .unwrap_err();
    assert!(matches!(err, AdoError::Forbidden), "got {err:?}");

    let complaining = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .respond_with(
            ResponseTemplate::new(400).set_body_string("The parameter testCaseIds is required"),
        )
        .mount(&complaining)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), complaining.uri(), complaining.uri());
    let err = client
        .boards_add_to_requirement_suite(ORG, PROJECT_ID, GAMMA_ID, PBI, &[157941])
        .await
        .unwrap_err();
    match err {
        AdoError::Http { status, body } => {
            assert_eq!(status, 400);
            assert!(body.contains("testCaseIds"), "the body the probe reads must survive: {body}");
        }
        other => panic!("expected the 400 body, got {other:?}"),
    }
}

#[tokio::test]
async fn the_project_id_is_read_once_and_cached() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_reply()))
        .expect(1)
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let first = client.project_id(ORG, PROJECT).await.unwrap();
    let second = client.project_id(ORG, PROJECT).await.unwrap();
    assert_eq!(first, PROJECT_ID);
    assert_eq!(first, second);
    server.verify().await;
}

/// Areas nest, and so do team scopes: the project-wide team covers every
/// PBI, so the team that owns the PBI's own sub-area has to win. Paths
/// come back with either slash and in whatever case someone typed.
#[tokio::test]
async fn the_team_is_the_one_whose_area_covers_the_pbi_longest_match_wins() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(teams_reply(&[(ALPHA_ID, "Alpha"), (GAMMA_ID, "Gamma Guardians")])),
        )
        .mount(&server)
        .await;
    mount_team_scope(&server, ALPHA_ID, "HRM", true).await;
    mount_team_scope(&server, GAMMA_ID, "HRM\\Gamma Guardians", true).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert_eq!(
        client
            .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians\\Sub")
            .await
            .unwrap(),
        GAMMA_ID,
        "the deeper scope owns the sub-area"
    );
    assert_eq!(
        client.team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Other").await.unwrap(),
        ALPHA_ID,
        "only the project-wide scope covers it"
    );
    assert_eq!(
        client
            .team_for_area(ORG, PROJECT, PROJECT_ID, "hrm/gamma guardians")
            .await
            .unwrap(),
        GAMMA_ID,
        "slashes and case must not change the answer"
    );
}

/// No team's scope covers the area - the project's own default team is
/// what Boards would fall back to, and it is read from the projects call
/// that already found the project id.
#[tokio::test]
async fn no_covering_team_means_the_default_team() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_reply()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(teams_reply(&[(ALPHA_ID, "Alpha"), (GAMMA_ID, "Gamma Guardians")])),
        )
        .mount(&server)
        .await;
    mount_team_scope(&server, ALPHA_ID, "HRM\\Alpha", true).await;
    mount_team_scope(&server, GAMMA_ID, "HRM\\Gamma Guardians", false).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert_eq!(
        client.team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Zeta").await.unwrap(),
        DEFAULT_TEAM_ID
    );
    // includeChildren false means the scope covers that area and nothing
    // under it, so a child area falls through to the default team too.
    assert_eq!(
        client
            .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians\\Sub")
            .await
            .unwrap(),
        DEFAULT_TEAM_ID
    );
}

/// The whole fallback, end to end: the two ids, the route, and then the
/// suite the named plan holds - because the reply carries the plan id and
/// not the suite id.
#[tokio::test]
async fn the_fallback_ends_with_the_suite_the_plan_holds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_reply()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(teams_reply(&[(GAMMA_ID, "Gamma Guardians")])),
        )
        .mount(&server)
        .await;
    mount_team_scope(&server, GAMMA_ID, "HRM\\Gamma Guardians", true).await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .and(query_param("teamId", GAMMA_ID))
        .and(query_param("__v", "5"))
        .and(body_json(boards_body(PBI, &[157941, 157801])))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "requirementId": PBI,
            "testPlanId": 157942,
            "testPoints": [{"outcome": "Active", "testCaseId": 157801, "testPointId": 251509}],
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/{PROJECT}/_apis/testplan/Plans/157942/suites")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 157943, "name": "root", "suiteType": "staticTestSuite"},
            {
                "id": 157944,
                "name": "145386 : [PMS] - PMS Reports",
                "suiteType": "requirementTestSuite",
                "requirementId": PBI,
                "parentSuite": {"id": 157943},
            }
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/{PROJECT}/_apis/testplan/plans/157942")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 157942,
            "name": "Gamma Guardians_Stories_26R2_SP04",
            "areaPath": "HRM\\Gamma Guardians",
            "rootSuite": {"id": 157943},
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let outcome = client
        .boards_fallback(ORG, PROJECT, PBI, "HRM\\Gamma Guardians", &[157941, 157801])
        .await
        .unwrap();
    assert_eq!(
        outcome,
        BoardsOutcome {
            suite: EnsuredSuite {
                plan_id: 157942,
                plan_name: "Gamma Guardians_Stories_26R2_SP04".into(),
                suite_id: 157944,
                // The route made the plan, but `created_plan` is the app's
                // own "no plan existed, so one was created here" flag and
                // the caller surfaces the Boards route in its own words.
                created_plan: false,
            },
            project_id: PROJECT_ID.into(),
            team_id: GAMMA_ID.into(),
        }
    );
    server.verify().await;

    // The no-DELETE invariant, seen from the wire rather than the source.
    let asked = server.received_requests().await.unwrap();
    assert!(!asked.is_empty());
    for req in &asked {
        assert!(
            matches!(req.method.as_str(), "GET" | "POST"),
            "the Boards fallback sent a {}",
            req.method.as_str()
        );
    }
}

/// Nothing was uploaded, so there is nothing to add: the route is never
/// asked, and no id is resolved on the way to not asking it.
#[tokio::test]
async fn the_fallback_refuses_an_empty_id_list_before_any_request() {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client
        .boards_fallback(ORG, PROJECT, PBI, "HRM\\Gamma Guardians", &[])
        .await
        .unwrap_err();
    match err {
        AdoError::Http { status, body } => {
            assert_eq!(status, 0);
            assert!(body.contains("at least one test case id"), "{body}");
        }
        other => panic!("expected the refusal, got {other:?}"),
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// The route answered 200 and named a plan, but that plan holds no
/// requirement suite for this PBI - the endpoint changed, or it did
/// something other than what it is here to do. Say both ids.
#[tokio::test]
async fn the_fallback_says_when_the_named_plan_has_no_suite() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_reply()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(teams_reply(&[(GAMMA_ID, "Gamma Guardians")])),
        )
        .mount(&server)
        .await;
    mount_team_scope(&server, GAMMA_ID, "HRM\\Gamma Guardians", true).await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "testPlanId": 157942,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/{PROJECT}/_apis/testplan/Plans/157942/suites")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client
        .boards_fallback(ORG, PROJECT, PBI, "HRM\\Gamma Guardians", &[157941])
        .await
        .unwrap_err();
    match err {
        AdoError::Http { status, body } => {
            assert_eq!(status, 0);
            assert!(body.contains("157942"), "{body}");
            assert!(body.contains("#145386"), "{body}");
        }
        other => panic!("expected the missing-suite error, got {other:?}"),
    }
}
