use v2_lib::ado::wit_batch::{create_uri, temp_id_op, update_uri, BatchMethod, BatchRequest};
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
        BatchRequest { method: BatchMethod::Patch, uri: create_uri("Web"), body: serde_json::json!([{"op": "add", "path": "/fields/System.Title", "value": "A"}]) },
        BatchRequest { method: BatchMethod::Patch, uri: update_uri(55), body: serde_json::json!([{"op": "add", "path": "/fields/System.Title", "value": "B"}]) },
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
        BatchRequest { method: BatchMethod::Patch, uri: create_uri("p"), body: serde_json::json!([]) },
        BatchRequest { method: BatchMethod::Patch, uri: create_uri("p"), body: serde_json::json!([]) },
    ];
    match client.wit_batch("o", &reqs).await {
        Err(AdoError::Http { body, .. }) => assert!(body.contains("1 of the 2"), "{body}"),
        other => panic!("expected an error, got {other:?}"),
    }
}

/// When the answer is short, ADO usually said WHY in the one item it did
/// send (a whole-batch refusal comes back as a 200 carrying a single
/// error item). That sentence must reach the log and the failure list -
/// 69 creates once failed with nothing but "http 0" on either (2026-09-22).
#[tokio::test]
async fn a_short_answer_carries_what_ado_said() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/wit/$batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 1,
            "value": [{"code": 400, "headers": {}, "body": "{\"message\": \"VS403474: The batch was refused as a whole.\"}"}]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let reqs = vec![
        BatchRequest { method: BatchMethod::Patch, uri: create_uri("p"), body: serde_json::json!([]) },
        BatchRequest { method: BatchMethod::Patch, uri: create_uri("p"), body: serde_json::json!([]) },
    ];
    match client.wit_batch("o", &reqs).await {
        Err(AdoError::Http { body, .. }) => {
            assert!(body.contains("1 of the 2"), "{body}");
            assert!(body.contains("VS403474: The batch was refused as a whole."), "{body}");
        }
        other => panic!("expected an error, got {other:?}"),
    }
}

/// A 200 that is not the batch envelope at all - a top-level error object -
/// is reported with that object's message, not as "0 of the 2" alone.
#[tokio::test]
async fn a_non_envelope_answer_carries_its_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/wit/$batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "$id": "1", "message": "TF400898: An Internal Error Occurred.", "typeKey": "Exception"
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let reqs = vec![BatchRequest { method: BatchMethod::Patch, uri: create_uri("p"), body: serde_json::json!([]) }];
    match client.wit_batch("o", &reqs).await {
        Err(AdoError::Http { body, .. }) => {
            assert!(body.contains("0 of the 1"), "{body}");
            assert!(body.contains("TF400898"), "{body}");
        }
        other => panic!("expected an error, got {other:?}"),
    }
}

/// The text a person sees for a failed call. `Display` of `Http` is the
/// bare "http 400" - the app's own name for the status - and the sentence
/// ADO sent explaining itself lives in `body`. The failure list showed the
/// former, so 69 cases failed with "http 0" and no reason.
#[test]
fn the_failure_text_prefers_what_ado_said() {
    let e = AdoError::Http { status: 0, body: "Azure DevOps answered 1 of the 69 requests".into() };
    assert_eq!(e.user_text(), "Azure DevOps answered 1 of the 69 requests");
    let e = AdoError::Http { status: 400, body: "TF401320: Rule Error for field Title".into() };
    assert_eq!(e.user_text(), "TF401320: Rule Error for field Title");
    // No body: the status is still better than nothing.
    let e = AdoError::Http { status: 502, body: "   ".into() };
    assert_eq!(e.user_text(), "http 502");
    // Every other variant reads as it always did.
    assert_eq!(AdoError::Unauthorized.user_text(), "unauthorized");
    assert_eq!(AdoError::Network("x".into()).user_text(), "network: x");
}

/// A whole-batch refusal nests its sentence one level down, with a capital
/// M: `{"count":1,"value":{"Message":"TF237201: ..."}}`. That is the shape
/// the 1000-link limit came back in (2026-09-22), and reading only
/// `message` showed "Azure DevOps returned HTTP 500" instead of it.
#[test]
fn a_refusal_nested_under_value_message_is_read() {
    use v2_lib::ado::wit_batch::BatchItem;
    let item = BatchItem {
        code: 500,
        body: serde_json::json!({"count": 1, "value": {"Message": "TF237201: Cannot add a new link because one of the work items being linked will exceed the 1000 link limit. "}}),
    };
    assert_eq!(
        item.message(),
        "TF237201: Cannot add a new link because one of the work items being linked will exceed the 1000 link limit."
    );
    // The flat capital-M form too.
    let item = BatchItem { code: 400, body: serde_json::json!({"Message": "VS402: no."}) };
    assert_eq!(item.message(), "VS402: no.");
}

/// A `"DELETE"` sub-request would travel inside an outer POST, past the
/// transport's verb allow-list and past the `.delete(` source scan. The
/// type now only has the two verbs a batch may carry.
#[test]
fn a_batch_item_can_only_be_patch_or_post() {
    assert_eq!(serde_json::to_value(BatchMethod::Patch).unwrap(), serde_json::json!("PATCH"));
    assert_eq!(serde_json::to_value(BatchMethod::Post).unwrap(), serde_json::json!("POST"));
    assert_eq!(BatchMethod::Patch.to_string(), "PATCH");
    let src = include_str!("../../src/ado/wit_batch.rs");
    assert!(!src.contains("\"DELETE\""), "wit_batch.rs must not name DELETE");
}
