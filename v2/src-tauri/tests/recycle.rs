//! The delete path: permission-gated, recycle bin only, reported per item.

use v2_lib::ado::{AdoClient, AdoError};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Project + root area + a permission answer, which is what the gate
/// needs. All three evaluations (work-item delete, manage plans, manage
/// suites) answer `allowed` together.
async fn with_permission(server: &MockServer, allowed: bool) {
    with_split_permission(server, allowed, allowed).await;
}

/// As `with_permission`, but the work-item right and the manage-test
/// rights can answer differently - the field failure was exactly that
/// split (work-item delete yes, test artifacts no).
async fn with_split_permission(server: &MockServer, work_item: bool, manage_test: bool) {
    Mock::given(method("GET"))
        .and(path("/o/_apis/projects/p"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": "proj-guid-1" })),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/classificationnodes/areas"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "identifier": "area-guid-1" })),
        )
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/security/permissionevaluationbatch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "evaluations": [
                { "value": work_item },
                { "value": manage_test },
                { "value": manage_test },
            ]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_delete_goes_to_the_recycle_bin_and_nowhere_else() {
    let server = MockServer::start().await;
    with_permission(&server, true).await;
    Mock::given(method("DELETE"))
        .and(path("/o/p/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 42 })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let out = client.delete_test_cases_to_recycle_bin("o", "p", &[42]).await.unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].deleted);

    // The URL that actually went out carries api-version and NOTHING else.
    // The permanent-erase form differs from this by one query parameter, so
    // asserting the real request is the only check that means anything.
    let sent = server.received_requests().await.unwrap();
    let del = sent
        .iter()
        .find(|r| r.method == wiremock::http::Method::DELETE)
        .expect("a DELETE was sent");
    let q = del.url.query().unwrap_or_default();
    assert_eq!(q, "api-version=7.1", "the delete carried an unexpected parameter: {q}");
}

/// The whole safety argument rests on this: an inconclusive permission
/// answer must read as NO, because the alternative offers the user a
/// capability the app cannot actually deliver.
#[tokio::test]
async fn permission_fails_closed_on_every_uncertain_answer() {
    // Explicitly denied.
    let denied = MockServer::start().await;
    with_permission(&denied, false).await;
    let c = AdoClient::with_base_url("t".into(), denied.uri());
    assert!(!c.can_delete_work_items("o", "p").await);
    assert!(matches!(
        c.delete_test_cases_to_recycle_bin("o", "p", &[42]).await.unwrap_err(),
        AdoError::Forbidden
    ));
    // And nothing was sent.
    assert!(denied
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| r.method != wiremock::http::Method::DELETE));

    // The evaluation is missing entirely.
    let empty = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": "g" })),
        )
        .mount(&empty)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&empty)
        .await;
    assert!(!AdoClient::with_base_url("t".into(), empty.uri())
        .can_delete_work_items("o", "p")
        .await);

    // The permission service itself is unreachable or erroring.
    let broken = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&broken)
        .await;
    assert!(!AdoClient::with_base_url("t".into(), broken.uri())
        .can_delete_work_items("o", "p")
        .await);

    // The project carried no id to build a token from.
    let nonode = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&nonode)
        .await;
    assert!(!AdoClient::with_base_url("t".into(), nonode.uri())
        .can_delete_work_items("o", "p")
        .await);
}

/// One failure must not strand the rest, and the caller has to be able to
/// say WHICH survived rather than reporting a count.
#[tokio::test]
async fn one_failure_does_not_stop_the_others_and_is_named() {
    let server = MockServer::start().await;
    with_permission(&server, true).await;
    Mock::given(method("DELETE"))
        .and(path("/o/p/_apis/wit/workitems/1"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/o/p/_apis/wit/workitems/2"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/o/p/_apis/wit/workitems/3"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let out = AdoClient::with_base_url("t".into(), server.uri())
        .delete_test_cases_to_recycle_bin("o", "p", &[1, 2, 3])
        .await
        .unwrap();
    assert_eq!(out.len(), 3);
    assert!(out[0].deleted && out[2].deleted, "the others still went");
    assert!(!out[1].deleted);
    assert_eq!(out[1].id, 2, "the failure names the item");
    assert!(matches!(out[1].error, Some(AdoError::Forbidden)), "and says why");
}

/// The reason Azure DevOps gives has to REACH the caller.
///
/// This is the branch that broke in the field. A 400 - which is what Azure
/// DevOps uses to refuse the request rather than the caller - carries its
/// explanation in the body, and the outcome used to flatten the error with
/// `to_string()`. `AdoError::Http`'s Display is `"http {status}"`, so a
/// user hit a refusal Azure DevOps had described in a full sentence and
/// could only report "it gives http 400". The old assertion here was
/// `!error.is_empty()`, which "http 400" satisfies - so the test agreed the
/// failure "says why" while saying nothing.
#[tokio::test]
async fn an_unmapped_failure_carries_azure_devops_own_explanation() {
    let server = MockServer::start().await;
    with_permission(&server, true).await;
    Mock::given(method("DELETE"))
        .and(path("/o/p/_apis/wit/workitems/7"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "message": "VS402625: Work item 7 cannot be deleted because it is in use."
        })))
        .mount(&server)
        .await;

    let out = AdoClient::with_base_url("t".into(), server.uri())
        .delete_test_cases_to_recycle_bin("o", "p", &[7])
        .await
        .unwrap();

    assert!(!out[0].deleted);
    let Some(AdoError::Http { status, body }) = &out[0].error else {
        panic!("a 400 must arrive as Http, structured - got {:?}", out[0].error);
    };
    assert_eq!(*status, 400);
    // The BODY is the whole point: the frontend's describeAdoError lifts
    // `message` out of it. A status with an empty body is the bug.
    assert!(
        body.contains("cannot be deleted because it is in use"),
        "the explanation was dropped; the user gets a bare status again: {body:?}"
    );
}

/// A throttled delete must not be reported as a permission problem or as an
/// unexplained status - it is the one failure that is worth simply retrying.
#[tokio::test]
async fn a_throttled_delete_is_reported_as_rate_limiting() {
    let server = MockServer::start().await;
    with_permission(&server, true).await;
    Mock::given(method("DELETE"))
        .and(path("/o/p/_apis/wit/workitems/9"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "7"))
        .mount(&server)
        .await;

    let out = AdoClient::with_base_url("t".into(), server.uri())
        .delete_test_cases_to_recycle_bin("o", "p", &[9])
        .await
        .unwrap();
    assert!(matches!(out[0].error, Some(AdoError::RateLimited { retry_after_secs: 7 })));
}


/// What the permission request actually ASKS.
///
/// The first version of this gate evaluated the classification-node (area
/// path) namespace and its bit 8 - both real, both internally consistent,
/// and both about "may this user delete this AREA NODE" rather than "may
/// this user delete work items". Azure DevOps answered it confidently, so
/// nothing failed: a default Contributor simply never saw the button.
///
/// Every test here mocked the endpoint by PATH only and never looked at the
/// body, so all of them passed against the wrong question. This one reads
/// the body, which is the only way that class of mistake shows up.
#[tokio::test]
async fn the_permission_asked_for_is_work_item_delete_on_the_project() {
    let server = MockServer::start().await;
    with_permission(&server, true).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    assert!(client.can_delete_work_items("o", "p").await);

    let sent = server.received_requests().await.unwrap();
    let eval = sent
        .iter()
        .find(|r| r.url.path().ends_with("/permissionevaluationbatch"))
        .expect("the permission was evaluated");
    let body: serde_json::Value = serde_json::from_slice(&eval.body).unwrap();
    let e = &body["evaluations"][0];

    // The PROJECT namespace, not the classification-node one.
    assert_eq!(
        e["securityNamespaceId"].as_str(),
        Some("52d39943-cb85-4d7f-8fa8-c6baac873819"),
        "wrong security namespace - this decides WHICH permission is being asked about"
    );
    // WORK_ITEM_DELETE in that namespace.
    assert_eq!(e["permissions"].as_i64(), Some(8192));
    // Token built from the project's GUID, not the name in the URL.
    assert_eq!(
        e["token"].as_str(),
        Some("$PROJECT:vstfs:///Classification/TeamProject/proj-guid-1")
    );

    // And the test-artifact half: Manage test plans / Manage test suites
    // on the root AREA node, in the CSS namespace. Work-item delete alone
    // is the ordinary-work-item permission; Azure DevOps gates deleting a
    // Test Case on these, and checking only the first showed the button
    // to users the service would refuse.
    for (i, bit) in [(1, 64), (2, 128)] {
        let e = &body["evaluations"][i];
        assert_eq!(
            e["securityNamespaceId"].as_str(),
            Some("83e28ad4-2d72-4ceb-97b0-c7726d5502c3"),
            "evaluation {i} must ask the CSS (area) namespace"
        );
        assert_eq!(e["permissions"].as_i64(), Some(bit));
        assert_eq!(e["token"].as_str(), Some("vstfs:///Classification/Node/area-guid-1"));
    }
    // The literal ACL answer. `true` here asks Azure DevOps to pass anyone
    // in an Administrators group whatever their ACL says - the only field
    // in this body that can bias a fail-closed check toward yes, and the
    // only one that used to go unasserted.
    assert_eq!(
        body["alwaysAllowAdministrators"].as_bool(),
        Some(false),
        "the gate must not ask for the administrator bypass"
    );
}

/// The false positive from the field: a default Contributor HAS
/// project-level work-item delete, but Azure DevOps refuses Test Case
/// deletion without a manage-test right - so the old single-bit check
/// showed a Delete button the service would refuse. Both halves are
/// required now, and either manage bit satisfies the second half.
#[tokio::test]
async fn work_item_delete_alone_is_not_enough_for_test_cases() {
    let server = MockServer::start().await;
    with_split_permission(&server, true, false).await;
    assert!(
        !AdoClient::with_base_url("t".into(), server.uri())
            .can_delete_work_items("o", "p")
            .await,
        "work-item delete without a manage-test right must hide the button"
    );

    // The mirror: manage-test rights without work-item delete is no, too.
    let mirror = MockServer::start().await;
    with_split_permission(&mirror, false, true).await;
    assert!(!AdoClient::with_base_url("t".into(), mirror.uri())
        .can_delete_work_items("o", "p")
        .await);
}

/// The root area answering without an identifier is an uncertain path,
/// and uncertain reads as no.
#[tokio::test]
async fn a_root_area_without_an_identifier_fails_closed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/_apis/projects/p"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": "proj-guid-1" })),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/classificationnodes/areas"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;
    assert!(!AdoClient::with_base_url("t".into(), server.uri())
        .can_delete_work_items("o", "p")
        .await);
}
