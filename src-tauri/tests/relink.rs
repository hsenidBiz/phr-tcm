//! Moving a test case's PBI link - the fix for a case that landed in the
//! wrong PBI. One rev-guarded PATCH: remove the old TestedBy link, add
//! the new, never observable half-moved.

use v2_lib::ado::{AdoClient, AdoError};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A case whose relations carry a TestedBy link to PBI 100 (index 1 -
/// after an unrelated attachment relation, so the index actually has to
/// be FOUND, not assumed zero).
fn case_with_link(server: &MockServer) -> serde_json::Value {
    serde_json::json!({
        "id": 42,
        "rev": 7,
        "relations": [
            { "rel": "AttachedFile", "url": format!("{}/o/_apis/wit/attachments/xyz", server.uri()) },
            { "rel": "Microsoft.VSTS.Common.TestedBy-Reverse",
              "url": format!("{}/o/p/_apis/wit/workitems/100", server.uri()) },
        ]
    })
}

#[tokio::test]
async fn a_relink_removes_the_found_index_and_adds_the_new_pbi_in_one_patch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(case_with_link(&server)))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/o/p/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 42 })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    client.relink_test_case("o", "p", 42, 100, 200).await.unwrap();

    let sent = server.received_requests().await.unwrap();
    let patch = sent
        .iter()
        .find(|r| r.method == wiremock::http::Method::PATCH)
        .expect("a PATCH was sent");
    let body: serde_json::Value = serde_json::from_slice(&patch.body).unwrap();
    let ops = body.as_array().unwrap();

    // Guarded on the revision read moments before - a concurrent edit
    // refuses the whole PATCH instead of removing a shifted index.
    assert_eq!(ops[0]["op"], "test");
    assert_eq!(ops[0]["path"], "/rev");
    assert_eq!(ops[0]["value"], 7);
    // The remove names the FOUND index (1), not a guessed 0.
    assert_eq!(ops[1]["op"], "remove");
    assert_eq!(ops[1]["path"], "/relations/1");
    // And the add points at the destination PBI with the same relation.
    assert_eq!(ops[2]["op"], "add");
    assert_eq!(ops[2]["value"]["rel"], "Microsoft.VSTS.Common.TestedBy-Reverse");
    assert!(ops[2]["value"]["url"].as_str().unwrap().ends_with("/200"));
    assert_eq!(ops.len(), 3, "nothing else rides along");
}

/// A case with no link to the named source PBI is refused BEFORE any
/// write - linking it to a second PBI would compound the very confusion
/// the user is trying to fix.
#[tokio::test]
async fn a_case_not_linked_to_the_source_pbi_is_refused_without_writing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(case_with_link(&server)))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let err = client.relink_test_case("o", "p", 42, 999, 200).await.unwrap_err();
    let AdoError::Http { body, .. } = &err else {
        panic!("expected the structured refusal, got {err:?}");
    };
    assert!(body.contains("no link to PBI #999"), "{body}");
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method != wiremock::http::Method::PATCH),
        "nothing may be written for a case that was never in the source PBI"
    );
}
