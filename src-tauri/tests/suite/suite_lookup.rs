//! A PBI's requirement suite is asked of its own test cases before any plan
//! is walked.
//!
//! The response shape these mocks pin is Get Suites By Test Case Id:
//! `GET {org}/_apis/testplan/suites?testCaseId=N`, answering
//! `{"count": n, "value": [TestSuite]}` where each TestSuite carries `id`,
//! `name`, `suiteType`, `requirementId` (requirement suites), `plan`
//! (`{id, name}`) and `project` (`{id, name}`).

use v2_lib::ado::AdoClient;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pbi_with_cases(ids: &[i32]) -> serde_json::Value {
    serde_json::json!({
        "id": 42,
        "relations": ids.iter().map(|i| serde_json::json!({
            "rel": "Microsoft.VSTS.Common.TestedBy-Forward",
            "url": format!("https://dev.azure.com/org/_apis/wit/workItems/{i}"),
        })).collect::<Vec<_>>(),
    })
}

async fn mount_pbi(server: &MockServer, ids: &[i32]) {
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pbi_with_cases(ids)))
        .mount(server)
        .await;
}

async fn mount_case_suites(server: &MockServer, case: i32, suites: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/org/_apis/testplan/suites"))
        .and(query_param("testCaseId", case.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": suites.as_array().map(|a| a.len()).unwrap_or(0),
            "value": suites,
        })))
        .mount(server)
        .await;
}

fn requirement_suite(id: i32, pbi: i32, plan: i32, project: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "name": format!("Suite {id}"), "suiteType": "requirementTestSuite",
        "requirementId": pbi,
        "plan": {"id": plan, "name": format!("Plan {plan}")},
        "project": {"id": "p-guid", "name": project},
    })
}

async fn plan_list_calls(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/org/proj/_apis/testplan/plans")
        .count()
}

#[tokio::test]
async fn hit_on_the_first_case_walks_no_plan() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100, 101]).await;
    mount_case_suites(
        &server,
        100,
        serde_json::json!([
            {"id": 5, "name": "Static", "suiteType": "staticTestSuite",
             "plan": {"id": 9, "name": "Plan 9"}, "project": {"name": "proj"}},
            requirement_suite(91, 42, 9, "proj"),
        ]),
    )
    .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42).await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (9, 91));
    assert_eq!(found.plan_name, "Plan 9");
    assert!(!found.created_plan);

    let ensured = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap();
    assert_eq!((ensured.plan_id, ensured.suite_id), (9, 91));
    let finder = client.find_pbi_requirement_suite("org", "proj", 42, "").await.unwrap().unwrap();
    assert_eq!(finder.suite_id, 91);
    assert_eq!(plan_list_calls(&server).await, 0, "no plan list may be requested");
}

#[tokio::test]
async fn other_pbis_and_other_projects_do_not_count() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100, 101]).await;
    // Case 100: a static suite, another PBI's requirement suite, and this
    // PBI's requirement suite in another project.
    mount_case_suites(
        &server,
        100,
        serde_json::json!([
            {"id": 5, "name": "Static", "suiteType": "staticTestSuite",
             "plan": {"id": 9, "name": "Plan 9"}, "project": {"name": "proj"}},
            requirement_suite(60, 77, 9, "proj"),
            requirement_suite(61, 42, 8, "other"),
        ]),
    )
    .await;
    mount_case_suites(&server, 101, serde_json::json!([requirement_suite(92, 42, 10, "PROJ")])).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42).await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (10, 92), "the second case holds the right one");
}

#[tokio::test]
async fn a_suite_that_names_no_project_is_confirmed_by_its_plan() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100]).await;
    mount_case_suites(
        &server,
        100,
        serde_json::json!([{
            "id": 91, "name": "S", "suiteType": "requirementTestSuite", "requirementId": 42,
            "plan": {"id": 9, "name": "Plan 9"}
        }]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans/9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 9, "name": "Confirmed Plan", "areaPath": "proj"
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42).await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (9, 91));
    assert_eq!(found.plan_name, "Confirmed Plan");
}

#[tokio::test]
async fn no_tested_by_links_falls_back_to_the_walk() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[]).await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 9, "name": "Area Plan", "areaPath": "Proj\\Auth"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 91, "name": "S", "suiteType": "requirementTestSuite", "requirementId": 42}]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert!(client.suite_from_cases("org", "proj", 42).await.unwrap().is_none());
    let ensured = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap();
    assert_eq!((ensured.plan_id, ensured.suite_id), (9, 91));
    assert_eq!(plan_list_calls(&server).await, 1, "the walk happened as before");
}

#[tokio::test]
async fn a_403_on_one_case_skips_to_the_next() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100, 101]).await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/testplan/suites"))
        .and(query_param("testCaseId", "100"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    mount_case_suites(&server, 101, serde_json::json!([requirement_suite(92, 42, 10, "proj")])).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42).await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (10, 92));
}

#[tokio::test]
async fn only_the_first_three_cases_are_asked() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100, 101, 102, 103]).await;
    for c in [100, 101, 102, 103] {
        mount_case_suites(&server, c, serde_json::json!([])).await;
    }

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert!(client.suite_from_cases("org", "proj", 42).await.unwrap().is_none());
    let asked = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/org/_apis/testplan/suites")
        .count();
    assert_eq!(asked, 3);
}

// ---- a found suite is remembered across restarts ----

fn suite_for(plan_id: i32, suite_id: i32) -> v2_lib::ado_testplan::EnsuredSuite {
    v2_lib::ado_testplan::EnsuredSuite {
        plan_id,
        plan_name: format!("Plan {plan_id}"),
        suite_id,
        created_plan: false,
    }
}

#[test]
fn a_remembered_suite_is_trusted_for_thirty_days() {
    use v2_lib::ado_testplan::{cached_suite, forget_suite};
    use v2_lib::cache::{keys, now_ms, put_at};

    let base = "http://remember-ttl.test";
    let key = keys::suite(base, "org", "proj", 42);
    let day = 24 * 60 * 60 * 1000u64;
    forget_suite(base, "org", "proj", 42);

    put_at(&key, &suite_for(9, 91), now_ms() - 29 * day);
    assert_eq!(cached_suite(base, "org", "proj", 42), Some(suite_for(9, 91)), "29 days old is still good");
    put_at(&key, &suite_for(9, 91), now_ms() - 31 * day);
    assert!(cached_suite(base, "org", "proj", 42).is_none(), "31 days old is looked up again");
    assert_eq!(keys::SUITE_TTL_MS, 30 * day);
    forget_suite(base, "org", "proj", 42);
}

#[test]
fn a_remembered_suite_survives_a_restart_and_is_per_pbi_and_wiped_with_the_account() {
    use v2_lib::ado_testplan::EnsuredSuite;
    use v2_lib::cache::{keys, Store};

    let dir = std::env::temp_dir().join(format!(
        "tcm-suite-lookup-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let base = "http://restart.test";
    {
        let store = Store::open(Some(&dir));
        store.claim_for(Some("first@example.com"));
        store.put(&keys::suite(base, "org", "proj", 42), &suite_for(9, 91));
    }
    // A new process opens the same folder: the suite is there, with no
    // request made, for that PBI only.
    let store = Store::open(Some(&dir));
    store.claim_for(Some("first@example.com"));
    let got = store.fresh::<EnsuredSuite>(&keys::suite(base, "org", "proj", 42), keys::SUITE_TTL_MS);
    assert_eq!(got, Some(suite_for(9, 91)));
    assert!(store.get::<EnsuredSuite>(&keys::suite(base, "org", "proj", 43)).is_none());
    assert!(store.get::<EnsuredSuite>(&keys::suite(base, "org", "other", 42)).is_none());
    // A different account signing in drops it.
    store.claim_for(Some("second@example.com"));
    assert!(store.get::<EnsuredSuite>(&keys::suite(base, "org", "proj", 42)).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- one walk serves every screen that asks ----

const WALK_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

/// A project of one plan holding the requirement suites of PBIs 42 and 43,
/// whose plan list answers slowly, so lookups started together overlap.
async fn slow_project(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(WALK_DELAY)
                .set_body_json(serde_json::json!({
                    "value": [{"id": 9, "name": "Area Plan", "areaPath": "Proj\\Auth", "rootSuite": {"id": 1}}]
                })),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 1, "name": "Area Plan", "suiteType": "staticTestSuite"},
                {"id": 91, "name": "S42", "suiteType": "requirementTestSuite", "requirementId": 42, "parentSuite": {"id": 1}},
                {"id": 92, "name": "S43", "suiteType": "requirementTestSuite", "requirementId": 43, "parentSuite": {"id": 1}},
            ]
        })))
        .mount(server)
        .await;
}

async fn mount_pbi_without_cases(server: &MockServer, id: i32) {
    Mock::given(method("GET"))
        .and(path(format!("/org/_apis/wit/workitems/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": id})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn two_lookups_for_one_pbi_walk_the_plans_once() {
    let server = MockServer::start().await;
    mount_pbi_without_cases(&server, 42).await;
    slow_project(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    let (a, b) = tokio::join!(
        client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth"),
        client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth"),
    );
    assert_eq!(a.unwrap().unwrap().suite_id, 91);
    assert_eq!(b.unwrap().unwrap().suite_id, 91);
    assert_eq!(plan_list_calls(&server).await, 1, "one walk served both");

    // Nothing is kept once it is done: the next lookup walks again.
    client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth").await.unwrap();
    assert_eq!(plan_list_calls(&server).await, 2);
}

#[tokio::test]
async fn run_tests_joins_the_background_finder_already_walking() {
    let server = MockServer::start().await;
    mount_pbi_without_cases(&server, 42).await;
    slow_project(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    let heard = std::sync::Mutex::new(vec![]);
    let (finder, ensured) = tokio::join!(
        client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth"),
        client.ensure_requirement_suite_cb("org", "proj", 42, "Proj\\Auth", "", |d, t| {
            heard.lock().unwrap().push((d, t));
        }),
    );
    assert_eq!(finder.unwrap().unwrap().suite_id, 91);
    assert_eq!(ensured.unwrap().suite_id, 91);
    assert_eq!(plan_list_calls(&server).await, 1, "the finder's walk answered Run Tests too");
    assert_eq!(heard.lock().unwrap().last(), Some(&(1, 1)), "and Run Tests still saw the scan's progress");
}

#[tokio::test]
async fn lookups_for_different_pbis_do_not_wait_for_each_other() {
    let server = MockServer::start().await;
    mount_pbi_without_cases(&server, 42).await;
    mount_pbi_without_cases(&server, 43).await;
    slow_project(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    let started = std::time::Instant::now();
    let (a, b) = tokio::join!(
        client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth"),
        client.find_pbi_requirement_suite("org", "proj", 43, "Proj\\Auth"),
    );
    assert_eq!(a.unwrap().unwrap().suite_id, 91);
    assert_eq!(b.unwrap().unwrap().suite_id, 92);
    assert_eq!(plan_list_calls(&server).await, 2, "each PBI had its own walk");
    assert!(started.elapsed() < WALK_DELAY * 2, "they ran side by side, not one after the other");
}

#[tokio::test]
async fn a_failed_walk_is_shared_and_not_kept() {
    let server = MockServer::start().await;
    mount_pbi_without_cases(&server, 42).await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(403).set_delay(WALK_DELAY))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    let (a, b) = tokio::join!(
        client.find_pbi_requirement_suite("org", "proj", 42, ""),
        client.find_pbi_requirement_suite("org", "proj", 42, ""),
    );
    assert!(matches!(a, Err(v2_lib::ado::AdoError::Forbidden)));
    assert!(matches!(b, Err(v2_lib::ado::AdoError::Forbidden)), "the waiting lookup got the same failure");
    assert_eq!(plan_list_calls(&server).await, 1);

    let again = client.find_pbi_requirement_suite("org", "proj", 42, "").await;
    assert!(matches!(again, Ok(None)), "the failure was not remembered");
    assert_eq!(plan_list_calls(&server).await, 2);
}

#[tokio::test]
async fn the_plans_tree_is_read_once_for_screens_asking_together() {
    let server = MockServer::start().await;
    slow_project(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    let (first, second) = (std::sync::Mutex::new(vec![]), std::sync::Mutex::new(vec![]));
    let (a, b) = tokio::join!(
        client.list_plans_with_suites_cb("org", "proj", |d, t| first.lock().unwrap().push((d, t))),
        client.list_plans_with_suites_cb("org", "proj", |d, t| second.lock().unwrap().push((d, t))),
    );
    assert_eq!(a.unwrap().len(), 1);
    assert_eq!(b.unwrap().len(), 1);
    assert_eq!(plan_list_calls(&server).await, 1);
    assert_eq!(first.lock().unwrap().last(), Some(&(1, 1)));
    assert_eq!(second.lock().unwrap().last(), Some(&(1, 1)), "both saw the one walk's progress");
}
