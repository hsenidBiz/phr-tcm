//! The Boards "Add Test" route - the internal portal controller the app
//! falls back to when Azure DevOps refuses to create a requirement suite
//! for the account's access level (design
//! docs/superpowers/specs/2026-09-22-requirement-suite-boards-fallback-design.md).
//!
//! Everything here is wiremock: the route, the two ids its URL needs, and
//! the fallback that strings them together and ends with the suite the
//! plan holds. The body was confirmed by the probe on 2026-09-22 (a 200
//! that made plan 157958 and suite 157960); these tests pin the shape the
//! code sends, so the day the controller changes there is one place to
//! change.

use v2_lib::ado::{AdoClient, AdoError};
use v2_lib::ado_testplan::boards::{
    boards_body, probe_report, BoardsOutcome, BOARDS_ROUTE_VERSION,
};
use v2_lib::commands::misc::probe_allowed;
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

/// A mock server no other test has used. `MockServer::start()` hands out
/// servers from a pool, URL and all, and the project id and team are
/// cached per server URL for the whole run (design: they never change
/// within a session). A pooled server could arrive with the previous
/// test's team already cached, so a mock expecting one team lookup saw
/// none - the flaky `the_two_spellings_of_an_area_share_one_cached_team`.
/// A server of its own has a fresh port, so its cache starts empty.
async fn unshared_server() -> MockServer {
    MockServer::builder().start().await
}

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

/// The controller names its own parameters when one is missing: the first
/// probe named `planId`, the second `suiteId`; the third probe, with both
/// sent as 0, answered 200. One case id stands for the upload.
#[test]
fn the_body_is_the_three_integers_the_controller_named() {
    assert_eq!(
        boards_body(0, PBI, 157941),
        serde_json::json!({"planId": 0, "suiteId": 0, "requirementId": 145386, "testCaseId": 157941})
    );
    assert_eq!(
        boards_body(157942, PBI, 157801),
        serde_json::json!({"planId": 157942, "suiteId": 0, "requirementId": 145386, "testCaseId": 157801})
    );
    // The version the route was watched at. It is a constant so a bump
    // shows up as one edit with the date beside it.
    assert_eq!(BOARDS_ROUTE_VERSION, "5");
}

#[tokio::test]
async fn boards_route_posts_the_body_and_reads_the_plan_id() {
    let server = unshared_server().await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .and(query_param("teamId", GAMMA_ID))
        .and(query_param("__v", "5"))
        .and(body_json(boards_body(0, PBI, 157941)))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "requirementId": PBI,
            "testPlanId": 157942,
            "testPoints": [],
            "testSuiteId": 157944,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let (plan_id, suite_id) = client
        .boards_add_to_requirement_suite(ORG, PROJECT_ID, GAMMA_ID, 0, PBI, &[157941])
        .await
        .unwrap();
    assert_eq!(plan_id, 157942);
    assert_eq!(suite_id, Some(157944), "the reply names the suite outright (seen 2026-09-22)");
    server.verify().await;
}

/// A reply without `testPlanId` is the endpoint having changed shape. The
/// suite is then found under a plan nobody named, so there is nothing to
/// guess from - say what came back instead.
#[tokio::test]
async fn boards_route_without_a_plan_id_is_an_error_not_a_guess() {
    let server = unshared_server().await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client
        .boards_add_to_requirement_suite(ORG, PROJECT_ID, GAMMA_ID, 0, PBI, &[157941])
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
    let forbidding = unshared_server().await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "message": "You are not authorized to access this API.",
        })))
        .mount(&forbidding)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), forbidding.uri(), forbidding.uri());
    let err = client
        .boards_add_to_requirement_suite(ORG, PROJECT_ID, GAMMA_ID, 0, PBI, &[157941])
        .await
        .unwrap_err();
    assert!(matches!(err, AdoError::Forbidden), "got {err:?}");

    let complaining = unshared_server().await;
    Mock::given(method("POST"))
        .and(path(route_path()))
        .respond_with(
            ResponseTemplate::new(400).set_body_string("The parameter testCaseIds is required"),
        )
        .mount(&complaining)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), complaining.uri(), complaining.uri());
    let err = client
        .boards_add_to_requirement_suite(ORG, PROJECT_ID, GAMMA_ID, 0, PBI, &[157941])
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
    let server = unshared_server().await;
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
    let server = unshared_server().await;
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
    let server = unshared_server().await;
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

/// One team the account cannot read must not sink the lookup. The
/// fallback only runs at all after Azure DevOps has already refused
/// something, so a 403 on one team's settings is the expected shape of
/// this account's day - and the team that DOES cover the area is still
/// sitting there in the list.
#[tokio::test]
async fn a_team_the_account_cannot_read_is_skipped_not_fatal() {
    let server = unshared_server().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(teams_reply(&[(ALPHA_ID, "Alpha"), (GAMMA_ID, "Gamma Guardians")])),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/{ORG}/{PROJECT_ID}/{ALPHA_ID}/_apis/work/teamsettings/teamfieldvalues"
        )))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "message": "You are not authorized to access this API.",
        })))
        .mount(&server)
        .await;
    mount_team_scope(&server, GAMMA_ID, "HRM\\Gamma Guardians", true).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert_eq!(
        client
            .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians\\Sub")
            .await
            .unwrap(),
        GAMMA_ID
    );
}

/// Every team unreadable is the same answer as no team covering: the
/// project's default team, not an error that strands the upload.
#[tokio::test]
async fn every_team_unreadable_still_lands_on_the_default_team() {
    let server = unshared_server().await;
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
    Mock::given(method("GET"))
        .and(path(format!(
            "/{ORG}/{PROJECT_ID}/{ALPHA_ID}/_apis/work/teamsettings/teamfieldvalues"
        )))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/{ORG}/{PROJECT_ID}/{GAMMA_ID}/_apis/work/teamsettings/teamfieldvalues"
        )))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert_eq!(
        client
            .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians")
            .await
            .unwrap(),
        DEFAULT_TEAM_ID
    );
}

/// An empty project id would go into the route's URL as nothing at all
/// (`{base}/{org}//_api/...`) and come back a 404 nobody can read. Say
/// what went wrong instead of sending a URL with a hole in it.
#[tokio::test]
async fn an_unreadable_project_id_is_said_not_sent() {
    let server = unshared_server().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"name": PROJECT})),
        )
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client.project_id(ORG, PROJECT).await.unwrap_err();
    match err {
        AdoError::Http { status, body } => {
            assert_eq!(status, 0);
            assert!(body.contains("without an id"), "{body}");
        }
        other => panic!("expected the unreadable-project error, got {other:?}"),
    }
}

/// `HRM\Gamma Guardians` and `HRM/Gamma Guardians` are one area, so they
/// are one cache entry - otherwise the spelling a caller happened to use
/// decides whether the team lookup is paid for again.
#[tokio::test]
async fn the_two_spellings_of_an_area_share_one_cached_team() {
    let server = unshared_server().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(teams_reply(&[(GAMMA_ID, "Gamma Guardians")])),
        )
        .expect(1)
        .mount(&server)
        .await;
    mount_team_scope(&server, GAMMA_ID, "HRM\\Gamma Guardians", true).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert_eq!(
        client
            .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians")
            .await
            .unwrap(),
        GAMMA_ID
    );
    assert_eq!(
        client
            .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM/Gamma Guardians")
            .await
            .unwrap(),
        GAMMA_ID
    );
    server.verify().await;
}

/// The whole fallback, end to end, for a reply that names only the plan:
/// the two ids, the route, and then the suite the named plan holds. (The
/// reply seen on 2026-09-22 names the suite too; the sibling test below
/// covers that, and this one is the path a reply that stopped would take.)
#[tokio::test]
async fn the_fallback_ends_with_the_suite_the_plan_holds() {
    let server = unshared_server().await;
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
        .and(body_json(boards_body(0, PBI, 157941)))
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
        .boards_fallback(ORG, PROJECT, PBI, "HRM\\Gamma Guardians", 0, &[157941, 157801])
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
    let server = unshared_server().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client
        .boards_fallback(ORG, PROJECT, PBI, "HRM\\Gamma Guardians", 0, &[])
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
    let server = unshared_server().await;
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
        .boards_fallback(ORG, PROJECT, PBI, "HRM\\Gamma Guardians", 0, &[157941])
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

/// What the development-build probe writes back. The route's body is a
/// guess until this has been run once against a real PBI (design §4.1),
/// so the report's whole job is to say which of the four answers came
/// back and what it means for the next edit: wire it, rename a field,
/// stop, or read the error as it is.
#[test]
fn probe_report_reads_the_four_answers() {
    let landed = Ok(BoardsOutcome {
        suite: EnsuredSuite {
            plan_id: 157942,
            plan_name: "Gamma Guardians_Stories_26R2_SP04".into(),
            suite_id: 157944,
            created_plan: false,
        },
        project_id: PROJECT_ID.into(),
        team_id: GAMMA_ID.into(),
    });
    assert_eq!(
        probe_report(&landed),
        format!(
            "200: plan 157942 \"Gamma Guardians_Stories_26R2_SP04\", suite 157944, \
             project id {PROJECT_ID}, team id {GAMMA_ID} - wire it"
        )
    );

    // The useful failure: a 400 names the field the guessed body got
    // wrong, so the body is carried into the report rather than left in
    // the log. Capped at what `refused()` keeps, because a controller
    // that answers with a page of HTML should not fill the panel.
    let long = "x".repeat(900);
    let wrong_field = probe_report(&Err(AdoError::Http { status: 400, body: long }));
    assert_eq!(
        wrong_field,
        format!(
            "400: the body's field names are wrong - Azure DevOps said: {}",
            "x".repeat(600)
        )
    );

    // The design's one unverified assumption: a bearer token needs no
    // anti-forgery header. A 403 is that assumption failing.
    assert_eq!(
        probe_report(&Err(AdoError::Forbidden)),
        "403: the route refused a bearer token - the design stops here (§4.1 assumption)"
    );

    // The controller answered 500 to the first real call; whatever it
    // says is the only clue, so every other HTTP status shows its body too.
    assert_eq!(
        probe_report(&Err(AdoError::Http { status: 500, body: "Object reference not set".into() })),
        "500: Azure DevOps said: Object reference not set"
    );
    // A status of 0 is this app's own sentence, shown as written.
    assert_eq!(
        probe_report(&Err(AdoError::Http { status: 0, body: "the Boards route needs at least one test case id".into() })),
        "the Boards route needs at least one test case id"
    );

    // Anything else is repeated as it is - there is nothing to read into
    // a token that expired or a host that never answered.
    assert_eq!(probe_report(&Err(AdoError::Unauthorized)), "unauthorized");
}

/// The probe reaches an undocumented endpoint on purpose, so a shipped
/// build refuses it. The gate takes the flag rather than reading
/// `cfg!(debug_assertions)`, which is the only way both answers can be
/// tested from one build.
#[test]
fn the_probe_runs_in_a_development_build_only() {
    assert_eq!(probe_allowed(true), Ok(()));
    let refused = probe_allowed(false).unwrap_err();
    assert!(refused.contains("only in a development build"), "{refused}");
}

/// The projects call answered, but without the default team - the field
/// the fallback uses when no team's area covers the PBI.
fn project_reply_without_a_default_team() -> serde_json::Value {
    serde_json::json!({"id": PROJECT_ID, "name": PROJECT})
}

/// An unreadable default team only matters to a PBI that needs it. A PBI
/// whose area a team does cover never looks at the field, and failing
/// that upload over it would be failing over something it never used.
#[tokio::test]
async fn an_unreadable_default_team_is_an_error_only_where_it_is_needed() {
    // Nothing covers the PBI's area, so the default team is the answer -
    // and there isn't one.
    let needed = unshared_server().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(project_reply_without_a_default_team()),
        )
        .mount(&needed)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(ResponseTemplate::new(200).set_body_json(teams_reply(&[(ALPHA_ID, "Alpha")])))
        .mount(&needed)
        .await;
    mount_team_scope(&needed, ALPHA_ID, "HRM\\Alpha", true).await;

    let client = AdoClient::with_base_urls("tok".into(), needed.uri(), needed.uri());
    let err = client
        .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians")
        .await
        .unwrap_err();
    match err {
        AdoError::Http { status, body } => {
            assert_eq!(status, 0);
            assert!(body.contains("without a default team"), "{body}");
        }
        other => panic!("expected the missing-default-team error, got {other:?}"),
    }

    // Same project reply, but a team owns the area: the missing field is
    // never read, so it is never a problem.
    let covered = unshared_server().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(project_reply_without_a_default_team()),
        )
        .mount(&covered)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(teams_reply(&[(GAMMA_ID, "Gamma Guardians")])),
        )
        .mount(&covered)
        .await;
    mount_team_scope(&covered, GAMMA_ID, "HRM\\Gamma Guardians", true).await;

    let client = AdoClient::with_base_urls("tok".into(), covered.uri(), covered.uri());
    assert_eq!(
        client
            .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians")
            .await
            .unwrap(),
        GAMMA_ID
    );
}

/// The whole team list refused. The fallback only runs after this account
/// was already refused something, so that is an answer about permission,
/// not a fault - and Boards itself falls back to the default team. A
/// refusal is logged and the upload carries on; a 500 is not an answer
/// about permission and still stops it.
#[tokio::test]
async fn a_team_list_the_account_cannot_read_falls_back_to_the_default_team() {
    let refused = unshared_server().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_reply()))
        .mount(&refused)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(ResponseTemplate::new(403).set_body_string(
            r#"{"message":"You are not authorized to access this API"}"#,
        ))
        .mount(&refused)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), refused.uri(), refused.uri());
    assert_eq!(
        client
            .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians")
            .await
            .unwrap(),
        DEFAULT_TEAM_ID
    );

    let broken = unshared_server().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_reply()))
        .mount(&broken)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(ResponseTemplate::new(500).set_body_string("it broke"))
        .mount(&broken)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), broken.uri(), broken.uri());
    match client
        .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians")
        .await
        .unwrap_err()
    {
        AdoError::Http { status, .. } => assert_eq!(status, 500),
        other => panic!("expected the 500 to stop it, got {other:?}"),
    }
}

/// The reply seen on 2026-09-22 names the suite too (`testSuiteId`), so
/// the plan's suites are not listed at all.
#[tokio::test]
async fn a_reply_that_names_the_suite_is_not_followed_by_a_listing() {
    let server = unshared_server().await;
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
            "requirementId": PBI,
            "testPlanId": 157958,
            "testPoints": [{"outcome": "Active", "testCaseId": 157957, "testPointId": 251866}],
            "testSuiteId": 157960,
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/{PROJECT}/_apis/testplan/Plans/157958/suites")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/{PROJECT}/_apis/testplan/plans/157958")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 157958,
            "name": "Gamma Guardians_Stories_26R2_SP03",
            "areaPath": "HRM\\Gamma Guardians",
            "rootSuite": {"id": 157959},
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let outcome = client
        .boards_fallback(ORG, PROJECT, PBI, "HRM\\Gamma Guardians", 0, &[157957])
        .await
        .unwrap();
    assert_eq!(outcome.suite.plan_id, 157958);
    assert_eq!(outcome.suite.suite_id, 157960);
    assert_eq!(outcome.suite.plan_name, "Gamma Guardians_Stories_26R2_SP03");
    server.verify().await;
}

/// The team scopes are read several at a time, not one after another:
/// eight teams that each take 300 ms answer well under the 2.4 s a serial
/// walk would need, and the covering team still wins, whichever request
/// finished first.
#[tokio::test]
async fn the_team_scopes_are_read_several_at_a_time() {
    let server = unshared_server().await;
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_reply()))
        .mount(&server)
        .await;
    let ids: Vec<String> = (0..8).map(|i| format!("0000000{i}-0000-0000-0000-000000000000")).collect();
    let teams: Vec<(&str, &str)> = ids.iter().map(|id| (id.as_str(), "team")).collect();
    Mock::given(method("GET"))
        .and(path(format!("/{ORG}/_apis/projects/{PROJECT_ID}/teams")))
        .respond_with(ResponseTemplate::new(200).set_body_json(teams_reply(&teams)))
        .mount(&server)
        .await;
    for (i, id) in ids.iter().enumerate() {
        // Only the last team covers the area, so an early finisher cannot
        // win by finishing first.
        let value = if i == 7 { "HRM\\Gamma Guardians" } else { "HRM\\Elsewhere" };
        Mock::given(method("GET"))
            .and(path(format!("/{ORG}/{PROJECT_ID}/{id}/_apis/work/teamsettings/teamfieldvalues")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(scope_reply(value, true))
                    .set_delay(std::time::Duration::from_millis(300)),
            )
            .mount(&server)
            .await;
    }

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let started = std::time::Instant::now();
    let team = client
        .team_for_area(ORG, PROJECT, PROJECT_ID, "HRM\\Gamma Guardians\\Sub")
        .await
        .unwrap();
    let took = started.elapsed();
    assert_eq!(team, ids[7]);
    assert!(
        took < std::time::Duration::from_millis(1500),
        "eight 300 ms reads took {took:?} - they ran one after another"
    );
}
