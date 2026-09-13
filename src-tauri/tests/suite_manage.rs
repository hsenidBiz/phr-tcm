//! The Manage Test Cases calls: suite entry order, static child suites and
//! adding cases to a suite. GET / POST / PATCH only.

use v2_lib::ado::AdoClient;
use v2_lib::ado_testplan::{ordered_case_ids, SuiteEntry};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn entry(id: i32, seq: i32, kind: &str) -> SuiteEntry {
    SuiteEntry { id, sequence_number: seq, entry_type: kind.to_string() }
}

#[test]
fn ordered_case_ids_puts_named_cases_first_then_the_rest_in_place() {
    let current = vec![
        entry(3, 0, "suite"),
        entry(8, 1, "testCase"),
        entry(9, 2, "testCase"),
        entry(10, 3, "testCase"),
    ];
    // Named ids lead in the order given; 9 was not named and keeps its
    // place after them; 77 is not in the suite and is dropped; a repeat
    // of 8 counts once.
    assert_eq!(ordered_case_ids(&current, &[10, 8, 77, 8]), vec![10, 8, 9]);
    // Nothing named: the current order comes back untouched.
    assert_eq!(ordered_case_ids(&current, &[]), vec![8, 9, 10]);
}

#[tokio::test]
async fn suite_entries_come_back_sorted_by_sequence() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/suiteentry/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"suiteId": 5, "sequenceNumber": 2, "id": 9, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 0, "id": 3, "suiteEntryType": "suite"},
                {"suiteId": 5, "sequenceNumber": 1, "id": 8, "suiteEntryType": "testCase"}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let entries = client.get_suite_entries("org", "proj", 5).await.unwrap();
    assert_eq!(entries, vec![entry(3, 0, "suite"), entry(8, 1, "testCase"), entry(9, 2, "testCase")]);
}

#[tokio::test]
async fn reorder_sends_cases_after_the_child_suites_as_plain_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/suiteentry/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"suiteId": 5, "sequenceNumber": 0, "id": 3, "suiteEntryType": "suite"},
                {"suiteId": 5, "sequenceNumber": 1, "id": 8, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 2, "id": 9, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 3, "id": 10, "suiteEntryType": "testCase"}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/testplan/suiteentry/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"suiteId": 5, "sequenceNumber": 0, "id": 3, "suiteEntryType": "suite"},
                {"suiteId": 5, "sequenceNumber": 3, "id": 9, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 1, "id": 10, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 2, "id": 8, "suiteEntryType": "testCase"}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let order = client.reorder_suite_cases("org", "proj", 5, &[10, 8]).await.unwrap();
    // The server's answer, read back in sequence order, child suites left out.
    assert_eq!(order, vec![10, 8, 9]);

    let reqs = server.received_requests().await.unwrap();
    let patch = reqs.iter().find(|r| r.method.as_str() == "PATCH").expect("one PATCH");
    let ct = patch.headers.get("content-type").expect("content-type").to_str().unwrap();
    assert!(ct.starts_with("application/json"), "plain JSON body, got {ct}");
    assert!(!ct.contains("json-patch"), "suiteentry takes an entry array, not json-patch");
    let body: serde_json::Value = serde_json::from_slice(&patch.body).unwrap();
    // One child suite sits at 0, so the cases start at sequence 1; the
    // unnamed case 9 keeps its place after the named ones.
    assert_eq!(
        body,
        serde_json::json!([
            {"id": 10, "sequenceNumber": 1, "suiteEntryType": "testCase"},
            {"id": 8, "sequenceNumber": 2, "suiteEntryType": "testCase"},
            {"id": 9, "sequenceNumber": 3, "suiteEntryType": "testCase"}
        ])
    );
}

#[tokio::test]
async fn reorder_with_no_cases_in_the_suite_sends_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/suiteentry/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"suiteId": 5, "sequenceNumber": 0, "id": 3, "suiteEntryType": "suite"}]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let order = client.reorder_suite_cases("org", "proj", 5, &[10]).await.unwrap();
    assert!(order.is_empty());
    let patches = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "PATCH")
        .count();
    assert_eq!(patches, 0);
}

#[tokio::test]
async fn create_static_suite_posts_the_documented_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 87,
            "name": "Smoke",
            "suiteType": "staticTestSuite",
            "parentSuite": {"id": 85, "name": "root"}
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let suite = client.create_static_suite("org", "proj", 9, 85, "Smoke").await.unwrap();
    assert_eq!(suite.id, 87);
    assert_eq!(suite.name, "Smoke");
    assert_eq!(suite.suite_type, "staticTestSuite");
    assert_eq!(suite.parent_id, Some(85));
    assert_eq!(suite.requirement_id, None);

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"suiteType": "staticTestSuite", "name": "Smoke", "parentSuite": {"id": 85}})
    );
}

#[tokio::test]
async fn add_test_cases_posts_work_item_ids_and_returns_what_landed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/9/Suites/87/TestCase"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"workItem": {"id": 201, "name": "Valid login"}},
                {"workItem": {"id": 202, "name": "Bad password"}}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let added = client
        .add_test_cases_to_suite("org", "proj", 9, 87, &[201, 202])
        .await
        .unwrap();
    assert_eq!(added, vec![201, 202]);

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(
        body,
        serde_json::json!([{"workItem": {"id": 201}}, {"workItem": {"id": 202}}])
    );
}

#[tokio::test]
async fn add_test_cases_with_nothing_to_add_does_not_call_the_server() {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let added = client.add_test_cases_to_suite("org", "proj", 9, 87, &[]).await.unwrap();
    assert!(added.is_empty());
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
