//! Share-a-draft-for-review: attachment upload + the ONE approved PBI
//! write (AttachedFile relation), and the recipient's fetch.

use v2_lib::ado::AdoClient;
use v2_lib::ado_share::{draft_file_name, parse_share_link};
use wiremock::matchers::{body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn share_uploads_the_json_and_attaches_it_to_the_pbi() {
    let server = MockServer::start().await;
    let json = r#"{"test_cases":[{"title":"Login - valid credentials","steps":[]}]}"#;
    let name = draft_file_name(144714, json);

    // The reuse pre-check finds nothing on the PBI.
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 144714, "rev": 1, "relations": []
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/acme/Web/_apis/wit/attachments"))
        .and(query_param("fileName", name.as_str()))
        .and(header("Content-Type", "application/octet-stream"))
        .and(body_string_contains("Login - valid credentials"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "aaaa1111-2222-3333-4444-555566667777",
            "url": "https://dev.azure.com/acme/_apis/wit/attachments/aaaa1111"
        })))
        .mount(&server)
        .await;

    // The one approved PBI write: an additive AttachedFile relation with a
    // human-readable comment. json-patch, PATCH verb, nothing else touched.
    Mock::given(method("PATCH"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .and(header("Content-Type", "application/json-patch+json"))
        .and(body_string_contains("AttachedFile"))
        .and(body_string_contains("draft shared for review"))
        .and(body_string_contains(r#""name":"tcm-draft-review-144714-"#))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 144714 })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let link = client.share_draft("acme", "Web", 144714, json).await.unwrap();

    assert_eq!(
        link,
        "tcm-share:acme/Web/144714/aaaa1111-2222-3333-4444-555566667777"
    );
}

/// The upload succeeding but the PBI attach failing must surface an error,
/// not hand out a link to a blob that could be garbage-collected.
#[tokio::test]
async fn share_fails_loudly_when_the_pbi_attach_fails() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 144714, "rev": 1, "relations": []
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/acme/Web/_apis/wit/attachments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "aaaa", "url": "https://x/att/aaaa"
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert!(client.share_draft("acme", "Web", 144714, "{}").await.is_err());
}

#[tokio::test]
async fn fetch_downloads_the_shared_draft_with_the_callers_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/attachments/aaaa1111-2222-3333-4444-555566667777"))
        .and(header("Authorization", "Bearer recipient-token"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"test_cases":[]}"#))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("recipient-token".into(), server.uri(), server.uri());
    let share =
        parse_share_link("tcm-share:acme/Web/144714/aaaa1111-2222-3333-4444-555566667777").unwrap();
    let body = client.fetch_shared_draft(&share).await.unwrap();
    assert!(body.contains("test_cases"));
}

/// One-time use: take_shared_draft downloads then REVOKES - the remove
/// targets exactly the relation index found for our attachment, guarded
/// by a `test` op on /rev so a concurrent change aborts instead of
/// removing the wrong relation.
#[tokio::test]
async fn take_downloads_then_revokes_only_our_relation() {
    let server = MockServer::start().await;
    // The PBI carries an unrelated relation FIRST - ours is index 1.
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 144714, "rev": 7,
            "fields": {
                "System.Title": "Timeline - split weight",
                "System.WorkItemType": "Product Backlog Item"
            },
            "relations": [
                { "rel": "Microsoft.VSTS.Common.TestedBy-Reverse", "url": "https://x/wi/5" },
                { "rel": "AttachedFile",
                  "url": "https://x/_apis/wit/attachments/aaaa1111-2222-3333-4444-555566667777" }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/attachments/aaaa1111-2222-3333-4444-555566667777"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"test_cases":[]}"#))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .and(body_string_contains(r#""op":"test""#))
        .and(body_string_contains(r#""value":7"#))
        .and(body_string_contains(r#""path":"/relations/1""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 144714 })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let share =
        parse_share_link("tcm-share:acme/Web/144714/aaaa1111-2222-3333-4444-555566667777").unwrap();
    let taken = client.take_shared_draft(&share).await.unwrap();
    assert!(taken.json.contains("test_cases"));
    assert!(taken.revoke_warning.is_none(), "revoke succeeded - no warning");
    // The same work-item read supplies the PBI identity, so the recipient
    // can be offered a switch without another lookup.
    assert_eq!(taken.pbi_title, "Timeline - split weight");
    assert_eq!(taken.pbi_work_item_type, "Product Backlog Item");
}

/// A link whose relation is gone is spent - the fetch refuses BEFORE
/// downloading anything.
#[tokio::test]
async fn take_refuses_an_already_used_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 144714, "rev": 9,
            "relations": [
                { "rel": "Microsoft.VSTS.Common.TestedBy-Reverse", "url": "https://x/wi/5" }
            ]
        })))
        .mount(&server)
        .await;
    // NO attachment mock: a download attempt would 404 loudly.

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let share =
        parse_share_link("tcm-share:acme/Web/144714/aaaa1111-2222-3333-4444-555566667777").unwrap();
    let err = client.take_shared_draft(&share).await.unwrap_err();
    assert!(err.contains("already been used"), "got: {err}");
}

/// The recipient may lack edit permission on the PBI: the import still
/// succeeds, with a warning that the link stays live.
#[tokio::test]
async fn take_still_imports_when_the_revoke_is_forbidden() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 144714, "rev": 3,
            "relations": [
                { "rel": "AttachedFile",
                  "url": "https://x/_apis/wit/attachments/aaaa1111-2222-3333-4444-555566667777" }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/attachments/aaaa1111-2222-3333-4444-555566667777"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"test_cases":[]}"#))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let share =
        parse_share_link("tcm-share:acme/Web/144714/aaaa1111-2222-3333-4444-555566667777").unwrap();
    let taken = client.take_shared_draft(&share).await.unwrap();
    assert!(taken.json.contains("test_cases"));
    assert!(taken.revoke_warning.unwrap().contains("could not be revoked"));
}


/// Re-sharing an unchanged queue reuses the attachment already on the
/// PBI: same link back, and NO upload or PBI write happens (no POST/PATCH
/// mocks are mounted - any attempt would fail the call).
#[tokio::test]
async fn share_reuses_an_identical_draft_already_attached() {
    let server = MockServer::start().await;
    let json = r#"{"test_cases":[{"title":"Same content","steps":[]}]}"#;
    let name = draft_file_name(144714, json);
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workitems/144714"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 144714, "rev": 4,
            "relations": [
                { "rel": "AttachedFile",
                  "url": "https://x/_apis/wit/attachments/eeee9999-8888-7777-6666-555544443333",
                  "attributes": { "name": name } }
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let link = client.share_draft("acme", "Web", 144714, json).await.unwrap();
    assert_eq!(
        link,
        "tcm-share:acme/Web/144714/eeee9999-8888-7777-6666-555544443333"
    );

    // A CHANGED queue hashes to a different name - the old attachment does
    // not match, and with no POST mock the upload attempt errors, proving
    // the reuse path was not taken.
    let changed = r#"{"test_cases":[{"title":"Different content","steps":[]}]}"#;
    assert!(client.share_draft("acme", "Web", 144714, changed).await.is_err());
}
