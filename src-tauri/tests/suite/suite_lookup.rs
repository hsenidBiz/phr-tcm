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
    let found = client.suite_from_cases("org", "proj", 42, "").await.unwrap().unwrap();
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
    let found = client.suite_from_cases("org", "proj", 42, "").await.unwrap().unwrap();
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
    let found = client.suite_from_cases("org", "proj", 42, "").await.unwrap().unwrap();
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
    assert!(client.suite_from_cases("org", "proj", 42, "").await.unwrap().is_none());
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
    let found = client.suite_from_cases("org", "proj", 42, "").await.unwrap().unwrap();
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
    assert!(client.suite_from_cases("org", "proj", 42, "").await.unwrap().is_none());
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
    slow_project_for(server, WALK_DELAY).await;
}

/// The same, answering the plan list after `delay`.
async fn slow_project_for(server: &MockServer, delay: std::time::Duration) {
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(delay)
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
        client.ensure_requirement_suite_cb("org", "proj", 42, "Proj\\Auth", "", |p| {
            heard.lock().unwrap().push((p.done, p.total));
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
    slow_project_for(&server, std::time::Duration::from_secs(3)).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    // Both plan-list requests must reach the server while neither lookup
    // has answered: that is "side by side", with no clock to judge it by.
    let _p = crate::serial::pacer();
    let (c1, c2) = (client.clone(), client.clone());
    let a = tokio::spawn(async move { c1.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth").await });
    let b = tokio::spawn(async move { c2.find_pbi_requirement_suite("org", "proj", 43, "Proj\\Auth").await });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while plan_list_calls(&server).await < 2 {
        assert!(std::time::Instant::now() < deadline, "the second lookup never started its own walk");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!a.is_finished() && !b.is_finished(), "the walks were still running when both had started");
    assert_eq!(a.await.unwrap().unwrap().unwrap().suite_id, 91);
    assert_eq!(b.await.unwrap().unwrap().unwrap().suite_id, 92);
    assert_eq!(plan_list_calls(&server).await, 2, "each PBI had its own walk");
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
        client.list_plans_with_suites_cb("org", "proj", |p| first.lock().unwrap().push((p.done, p.total))),
        client.list_plans_with_suites_cb("org", "proj", |p| second.lock().unwrap().push((p.done, p.total))),
    );
    assert_eq!(a.unwrap().len(), 1);
    assert_eq!(b.unwrap().len(), 1);
    assert_eq!(plan_list_calls(&server).await, 1);
    assert_eq!(first.lock().unwrap().last(), Some(&(1, 1)));
    assert_eq!(second.lock().unwrap().last(), Some(&(1, 1)), "both saw the one walk's progress");
}

// ---- each screen hears its own walk, and a long plan says so ----

#[test]
fn a_scan_event_says_which_walk_it_belongs_to() {
    use v2_lib::ado::single_flight::ScanProgress;
    use v2_lib::events::{ScanKind, SuiteScanProgress};

    let p = ScanProgress { done: 3, total: 9, pages: Some(4) };
    assert_eq!(
        serde_json::to_value(SuiteScanProgress::of(ScanKind::Plans, "proj", None, p)).unwrap(),
        serde_json::json!({"done": 3, "total": 9, "pages": 4, "kind": "plans", "pbi_id": null, "project": "proj"}),
    );
    assert_eq!(
        serde_json::to_value(SuiteScanProgress::of(ScanKind::Suite, "proj", Some(42), ScanProgress::plans(1, 2)))
            .unwrap(),
        serde_json::json!({"done": 1, "total": 2, "pages": null, "kind": "suite", "pbi_id": 42, "project": "proj"}),
    );
}

const PAGE_DELAY: std::time::Duration = std::time::Duration::from_millis(150);

/// Plans 7 and 8. Plan `paged` answers its suite list in `pages` pages
/// (each after `PAGE_DELAY`); the other answers one page after
/// `other_delay`.
async fn two_plans_one_paged(server: &MockServer, paged: i32, pages: usize, other_delay: std::time::Duration) {
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 7, "name": "Plan 7", "areaPath": "Proj\\Auth", "rootSuite": {"id": 1}},
                {"id": 8, "name": "Plan 8", "areaPath": "Proj\\Auth", "rootSuite": {"id": 1}},
            ]
        })))
        .mount(server)
        .await;
    let other = if paged == 7 { 8 } else { 7 };
    Mock::given(method("GET"))
        .and(path(format!("/org/proj/_apis/testplan/Plans/{other}/suites")))
        .respond_with(
            ResponseTemplate::new(200).set_delay(other_delay).set_body_json(serde_json::json!({
                "value": [{"id": other * 10, "name": "Folder", "suiteType": "staticTestSuite", "parentSuite": {"id": 1}}]
            })),
        )
        .mount(server)
        .await;
    let page_path = format!("/org/proj/_apis/testplan/Plans/{paged}/suites");
    // Later pages first: the first mock that matches answers.
    for n in (2..=pages).rev() {
        let mut r = ResponseTemplate::new(200).set_delay(PAGE_DELAY).set_body_json(serde_json::json!({
            "value": [{"id": paged * 10 + n as i32, "name": "Folder", "suiteType": "staticTestSuite", "parentSuite": {"id": 1}}]
        }));
        if n < pages {
            r = r.insert_header("x-ms-continuationtoken", format!("t{}", n + 1).as_str());
        }
        Mock::given(method("GET"))
            .and(path(page_path.clone()))
            .and(query_param("continuationToken", format!("t{n}")))
            .respond_with(r)
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(page_path))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(PAGE_DELAY)
                .insert_header("x-ms-continuationtoken", "t2")
                .set_body_json(serde_json::json!({
                    "value": [{"id": paged * 10 + 1, "name": "Folder", "suiteType": "staticTestSuite", "parentSuite": {"id": 1}}]
                })),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn the_plans_walk_reports_the_pages_of_the_one_plan_it_waits_on() {
    // Paging is paced like any request: another module's hold would
    // reorder the pages against the plan that is waited on.
    let _p = crate::serial::pacer();
    let server = MockServer::start().await;
    two_plans_one_paged(&server, 8, 3, std::time::Duration::ZERO).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    let heard = std::sync::Mutex::new(vec![]);
    let tree = client
        .list_plans_with_suites_cb("org", "proj", |p| heard.lock().unwrap().push(p))
        .await
        .unwrap();
    assert_eq!(tree.len(), 2);
    let heard = heard.lock().unwrap();
    let pages: Vec<u32> = heard.iter().filter_map(|p| p.pages).collect();
    assert_eq!(pages, vec![1, 2], "page 1 and 2 had a next page; the last page ends the plan");
    assert!(
        heard.iter().filter(|p| p.pages.is_some()).all(|p| (p.done, p.total) == (1, 2)),
        "reported only once every other plan had finished: {heard:?}"
    );
}

#[tokio::test]
async fn pages_are_not_reported_while_other_plans_are_still_being_read() {
    let _p = crate::serial::pacer();
    let server = MockServer::start().await;
    // Plan 7 pages three times (about 450 ms), but plan 8 is slower still
    // by a wide margin: the walk is waiting on plan 8, which has no pages
    // to report.
    two_plans_one_paged(&server, 7, 3, std::time::Duration::from_millis(3000)).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    let heard = std::sync::Mutex::new(vec![]);
    client
        .list_plans_with_suites_cb("org", "proj", |p| heard.lock().unwrap().push(p))
        .await
        .unwrap();
    let heard = heard.lock().unwrap();
    assert!(heard.iter().all(|p| p.pages.is_none()), "{heard:?}");
    assert_eq!(heard.last().map(|p| (p.done, p.total)), Some((2, 2)));
}

#[tokio::test]
async fn the_suite_search_reports_the_pages_of_a_large_plan_too() {
    let _p = crate::serial::pacer();
    let server = MockServer::start().await;
    mount_pbi_without_cases(&server, 42).await;
    two_plans_one_paged(&server, 8, 3, std::time::Duration::ZERO).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    let heard = std::sync::Mutex::new(vec![]);
    let found = client
        .find_pbi_requirement_suite_cb("org", "proj", 42, "Proj\\Auth", |p| heard.lock().unwrap().push(p))
        .await
        .unwrap();
    assert!(found.is_none());
    let pages: Vec<u32> = heard.lock().unwrap().iter().filter_map(|p| p.pages).collect();
    assert_eq!(pages, vec![1, 2]);
    assert_eq!(heard.lock().unwrap().last().map(|p| (p.done, p.total)), Some((2, 2)));
}

// ---- a PBI with a suite in several plans gets the plan a person would pick ----

async fn mount_plan(server: &MockServer, id: i32, area: &str, state: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/org/proj/_apis/testplan/plans/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id, "name": format!("Plan {id}"), "areaPath": area, "state": state
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn the_plan_for_the_pbis_area_wins_even_when_listed_second() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100]).await;
    mount_case_suites(
        &server,
        100,
        serde_json::json!([requirement_suite(71, 42, 7, "proj"), requirement_suite(91, 42, 9, "proj")]),
    )
    .await;
    mount_plan(&server, 7, "Proj\\Other", "Active").await;
    mount_plan(&server, 9, "Proj\\Auth", "Active").await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42, "Proj\\Auth\\Login").await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (9, 91));
    assert_eq!(found.plan_name, "Plan 9");
}

#[tokio::test]
async fn without_an_area_match_an_active_plan_wins_then_the_newest() {
    // Plan 9 is newer but finished; plan 7 is active: 7 wins.
    let server = MockServer::start().await;
    mount_pbi(&server, &[100]).await;
    mount_case_suites(
        &server,
        100,
        serde_json::json!([requirement_suite(91, 42, 9, "proj"), requirement_suite(71, 42, 7, "proj")]),
    )
    .await;
    mount_plan(&server, 7, "Proj\\A", "Active").await;
    mount_plan(&server, 9, "Proj\\B", "Inactive").await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42, "Proj\\Nowhere").await.unwrap().unwrap();
    assert_eq!(found.plan_id, 7);

    // Both active: the newest (highest id) wins.
    let server = MockServer::start().await;
    mount_pbi(&server, &[100]).await;
    mount_case_suites(
        &server,
        100,
        serde_json::json!([requirement_suite(71, 42, 7, "proj"), requirement_suite(91, 42, 9, "proj")]),
    )
    .await;
    mount_plan(&server, 7, "Proj\\A", "Active").await;
    mount_plan(&server, 9, "Proj\\B", "Active").await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42, "").await.unwrap().unwrap();
    assert_eq!(found.plan_id, 9);
}

// ---- a shortcut that does not answer never stops the walk ----

async fn plans_holding_suite_91(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 9, "name": "Area Plan", "areaPath": "Proj\\Auth", "rootSuite": {"id": 1}}]
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 91, "name": "S", "suiteType": "requirementTestSuite", "requirementId": 42}]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_500_from_the_suites_by_case_call_still_finds_the_suite_by_walking() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100]).await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/testplan/suites"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    plans_holding_suite_91(&server).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth").await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (9, 91));
    assert_eq!(plan_list_calls(&server).await, 1);
}

#[tokio::test]
async fn a_400_reading_the_pbi_still_finds_the_suite_by_walking() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad"))
        .mount(&server)
        .await;
    plans_holding_suite_91(&server).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth").await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (9, 91));
}

#[tokio::test]
async fn a_500_confirming_a_suite_with_no_project_still_finds_the_suite_by_walking() {
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
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    plans_holding_suite_91(&server).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth").await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (9, 91));
    assert_eq!(plan_list_calls(&server).await, 1);
}

#[tokio::test]
async fn a_bad_sign_in_on_the_shortcut_is_not_hidden_by_a_walk() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100]).await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/testplan/suites"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    plans_holding_suite_91(&server).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let got = client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth").await;
    assert!(matches!(got, Err(v2_lib::ado::AdoError::Unauthorized)));
    assert_eq!(plan_list_calls(&server).await, 0);
}

// ---- shared work: scoped to who asked, and robust to its askers ----

#[tokio::test]
async fn another_account_never_joins_a_walk_started_for_this_one() {
    let server = MockServer::start().await;
    mount_pbi_without_cases(&server, 42).await;
    slow_project(&server).await;
    let first = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri()).for_account("aaaa1111");
    let second = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri()).for_account("bbbb2222");
    let same = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri()).for_account("aaaa1111");

    let (a, b, c) = tokio::join!(
        first.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth"),
        second.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth"),
        same.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth"),
    );
    assert_eq!(a.unwrap().unwrap().suite_id, 91);
    assert_eq!(b.unwrap().unwrap().suite_id, 91);
    assert_eq!(c.unwrap().unwrap().suite_id, 91);
    assert_eq!(plan_list_calls(&server).await, 2, "one walk per account, the same account sharing one");

    // The plans tree is scoped the same way.
    let (x, y) = tokio::join!(
        first.list_plans_with_suites_cb("org", "proj", |_| {}),
        second.list_plans_with_suites_cb("org", "proj", |_| {}),
    );
    assert_eq!((x.unwrap().len(), y.unwrap().len()), (1, 1));
    assert_eq!(plan_list_calls(&server).await, 4);
}

#[tokio::test]
async fn a_walk_for_another_area_is_not_joined() {
    let server = MockServer::start().await;
    mount_pbi_without_cases(&server, 42).await;
    slow_project(&server).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());

    let (a, b) = tokio::join!(
        client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth"),
        client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Billing"),
    );
    assert_eq!(a.unwrap().unwrap().suite_id, 91);
    assert_eq!(b.unwrap().unwrap().suite_id, 91);
    assert_eq!(plan_list_calls(&server).await, 2, "the area decides the ranking, so each area walks");
}

mod flights {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use v2_lib::ado::single_flight::Flights;
    use v2_lib::ado::AdoError;

    #[tokio::test]
    async fn work_that_panics_fails_every_waiter_and_the_next_ask_starts_fresh() {
        static PANICKY: Flights<u32> = Flights::new();

        let slow_panic = || async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if true {
                panic!("the work fell over");
            }
            Ok::<u32, AdoError>(1)
        };
        let (a, b) = tokio::join!(
            PANICKY.run("k", None, |_tx| slow_panic(), |_| {}),
            PANICKY.run("k", None, |_tx| async { Ok::<u32, AdoError>(2) }, |_| {}),
        );
        assert!(matches!(a, Err(AdoError::Network(_))), "{a:?}");
        assert!(matches!(b, Err(AdoError::Network(_))), "the joined waiter heard it too: {b:?}");
        assert_eq!(PANICKY.running(), 0, "the entry is gone");

        let fresh = PANICKY.run("k", None, |_tx| async { Ok::<u32, AdoError>(7) }, |_| {}).await;
        assert_eq!(fresh.unwrap(), 7, "the next ask ran its own work");
    }

    #[tokio::test]
    async fn work_goes_on_when_the_only_asker_goes_away() {
        static ORPHANED: Flights<u32> = Flights::new();

        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        let asked = tokio::time::timeout(
            Duration::from_millis(20),
            ORPHANED.run(
                "k",
                None,
                move |_tx| async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    flag.store(true, Ordering::SeqCst);
                    Ok::<u32, AdoError>(5)
                },
                |_| {},
            ),
        )
        .await;
        assert!(asked.is_err(), "the asker gave up");

        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !done.load(Ordering::SeqCst) {
            assert!(std::time::Instant::now() < deadline, "the work stalled with nobody asking");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        while ORPHANED.running() > 0 {
            assert!(std::time::Instant::now() < deadline, "the finished work left its entry");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
