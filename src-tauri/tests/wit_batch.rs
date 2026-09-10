use v2_lib::ado::wit_batch::{create_uri, update_uri, BatchRequest};
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
        BatchRequest { method: "PATCH", uri: update_uri("Web", 55), body: serde_json::json!([{"op": "add", "path": "/fields/System.Title", "value": "B"}]) },
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
    assert_eq!(arr[1]["uri"], "/Web/_apis/wit/workitems/55?api-version=7.1");
}

/// The org and project segments are encoded like every other URL the app
/// builds - a space or a percent in a name must not become a query.
#[test]
fn batch_uris_encode_the_project() {
    assert_eq!(create_uri("50% Done"), "/50%25%20Done/_apis/wit/workitems/$Test%20Case?api-version=7.1");
    assert_eq!(update_uri("My Proj", 7), "/My%20Proj/_apis/wit/workitems/7?api-version=7.1");
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
