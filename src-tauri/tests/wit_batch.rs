use v2_lib::ado::wit_batch::{create_uri, temp_id_op, update_uri, BatchRequest};
use v2_lib::ado::{AdoClient, AdoError};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// One call carries every request with the json-patch content type per
/// item, and the answer comes back one item per request, in order, with
/// each body parsed out of the string the envelope wraps it in.
#[tokio::test]
async fn a_batch_is_one_call_and_answers_per_item_in_order() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/my%20org/_apis/wit/$batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 2,
            "value": [
                {"code": 200, "headers": {"Content-Type": "application/json"}, "body": "{\"id\": 901, \"rev\": 1}"},
                {"code": 400, "headers": {"Content-Type": "application/json"}, "body": "{\"message\": \"TF401320: Rule Error for field Title\"}"}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let reqs = vec![
        BatchRequest { method: "PATCH", uri: create_uri("Web"), body: serde_json::json!([{"op": "add", "path": "/fields/System.Title", "value": "A"}]) },
        BatchRequest { method: "PATCH", uri: update_uri(55), body: serde_json::json!([{"op": "add", "path": "/fields/System.Title", "value": "B"}]) },
    ];
    let items = client.wit_batch("my org", &reqs).await.unwrap();
    assert_eq!(items.len(), 2);
    assert!(items[0].ok());
    assert_eq!(items[0].id(), Some(901));
    assert!(!items[1].ok());
    assert_eq!(items[1].message(), "TF401320: Rule Error for field Title");

    let sent = server.received_requests().await.unwrap();
    assert_eq!(sent.len(), 1, "one HTTP call for the whole batch");
    let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["method"], "PATCH");
    assert_eq!(arr[0]["uri"], "/Web/_apis/wit/workitems/$Test%20Case?api-version=7.1");
    assert_eq!(arr[0]["headers"]["Content-Type"], "application/json-patch+json");
    assert_eq!(arr[0]["body"][0]["path"], "/fields/System.Title");
    assert_eq!(arr[1]["uri"], "/_apis/wit/workitems/55?api-version=7.1");
}

/// The two URI shapes the batch reference documents, and nothing else.
/// The first real run (2026-09-11) proved both halves: an update carrying
/// the project segment was 404 for all 96 cases, and a create without a
/// temporary id failed for every case but the first in each batch.
#[test]
fn batch_uris_follow_the_reference() {
    // Creates are project-scoped, and the project is encoded like every
    // other URL the app builds - a space or a percent must not become a query.
    assert_eq!(create_uri("50% Done"), "/50%25%20Done/_apis/wit/workitems/$Test%20Case?api-version=7.1");
    // Updates are organization-level: no project segment at all.
    assert_eq!(update_uri(7), "/_apis/wit/workitems/7?api-version=7.1");
}

/// Every create in a batch starts with a temporary id, negative and unique
/// within the batch, or the server treats the second onwards as bad requests.
#[test]
fn creates_carry_a_negative_temporary_id() {
    assert_eq!(temp_id_op(1), serde_json::json!({"op": "add", "path": "/id", "value": -1}));
    assert_eq!(temp_id_op(25), serde_json::json!({"op": "add", "path": "/id", "value": -25}));
}

/// Fewer answers than requests cannot be matched to cases - it is an
/// error for the whole chunk, never a guess.
#[tokio::test]
async fn a_short_answer_is_an_error_not_a_guess() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/wit/$batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 1, "value": [{"code": 200, "headers": {}, "body": "{\"id\": 1}"}]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let reqs = vec![
        BatchRequest { method: "PATCH", uri: create_uri("p"), body: serde_json::json!([]) },
        BatchRequest { method: "PATCH", uri: create_uri("p"), body: serde_json::json!([]) },
    ];
    match client.wit_batch("o", &reqs).await {
        Err(AdoError::Http { body, .. }) => assert!(body.contains("1 of the 2"), "{body}"),
        other => panic!("expected an error, got {other:?}"),
    }
}
