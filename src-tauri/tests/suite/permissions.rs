//! The "may this user create a test suite?" question. Unlike the delete
//! gate, this one hides the button ONLY on an explicit no - see
//! src/ado/permissions.rs for why the two lean opposite ways.

use v2_lib::ado::AdoClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Area node and a permission answer for the given value.
async fn with_answer(server: &MockServer, allowed: bool) {
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/classificationnodes/areas/Web"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "identifier": "area-guid-1" })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/security/permissionevaluationbatch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "evaluations": [{ "value": allowed }]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_clear_yes_and_a_clear_no_are_both_reported() {
    let yes = MockServer::start().await;
    with_answer(&yes, true).await;
    let client = AdoClient::with_base_url("t".into(), yes.uri());
    assert_eq!(client.may_manage_test_suites("o", "p", Some("Project\\Web")).await, Some(true));

    let no = MockServer::start().await;
    with_answer(&no, false).await;
    let client = AdoClient::with_base_url("t".into(), no.uri());
    assert_eq!(client.may_manage_test_suites("o", "p", Some("Project\\Web")).await, Some(false));
}

/// The question asked is "Manage test suites on THIS area node" - the bit
/// and the namespace are pinned by reading the request body, the only way
/// a wrong-question mistake shows up (see tests/suite/deletion.rs for the time
/// that happened).
#[tokio::test]
async fn the_permission_asked_for_is_manage_test_suites_on_the_plans_area() {
    let server = MockServer::start().await;
    with_answer(&server, true).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    client.may_manage_test_suites("o", "p", Some("Project\\Web")).await;

    let sent = server.received_requests().await.unwrap();
    let eval = sent
        .iter()
        .find(|r| r.url.path().ends_with("/permissionevaluationbatch"))
        .expect("the batch was asked");
    let body: serde_json::Value = serde_json::from_slice(&eval.body).unwrap();
    let e = &body["evaluations"][0];
    assert_eq!(e["securityNamespaceId"], "83e28ad4-2d72-4ceb-97b0-c7726d5502c3", "the CSS namespace");
    assert_eq!(e["permissions"], 128, "MANAGE_TEST_SUITES");
    assert_eq!(e["token"], "vstfs:///Classification/Node/area-guid-1", "the plan's own node");
    assert_eq!(body["alwaysAllowAdministrators"], false, "ask for the literal ACL answer");
}

/// Nothing to go on is NOT a no: the button stays and the refusal on the
/// create itself is the backstop. A failed check must not take away a
/// button the user is entitled to.
#[tokio::test]
async fn an_unanswerable_question_is_unknown_not_no() {
    // No mock at all: the area lookup itself has nothing to answer it.
    let server = MockServer::start().await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    assert_eq!(client.may_manage_test_suites("o", "p", Some("Project\\Web")).await, None);

    // A 200 whose shape says nothing is equally unknown.
    let odd = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/classificationnodes/areas/Web"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "identifier": "a" })))
        .mount(&odd)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/security/permissionevaluationbatch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "evaluations": [] })))
        .mount(&odd)
        .await;
    let client = AdoClient::with_base_url("t".into(), odd.uri());
    assert_eq!(client.may_manage_test_suites("o", "p", Some("Project\\Web")).await, None);
}
