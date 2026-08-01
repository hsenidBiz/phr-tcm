//! Share-a-draft-for-review: attachment upload + the ONE approved PBI
//! write (AttachedFile relation), and the recipient's fetch.

use v2_lib::ado::AdoClient;
use v2_lib::ado_share::{draft_file_name, parse_share_link};
use wiremock::matchers::{body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// What actually crosses a share link.
///
/// `share_queue` serialises the queue with `queue_to_json_string` and
/// `share_draft` uploads that string verbatim; `fetch_shared_queue` writes
/// it back to a temp file and reads it with `parse_file`, the same importer
/// a file import uses. So the payload contract is exactly this pair, and
/// the app-only fields have to survive it - a reviewer note is most of the
/// value of sending a draft to somebody else for review.
#[test]
fn a_shared_draft_carries_the_app_only_fields() {
    let queue = vec![v2_lib::model::TestCase {
        title: "Copy from Previous - hidden on a published cycle".into(),
        steps: vec![v2_lib::steps_xml::Step {
            action: "Open the published cycle.".into(),
            expected: "No Copy from Previous button is shown.".into(),
        }],
        automation_status: "Not Automated".into(),
        reviewer_notes: "Spec: Step10-ManagePerformanceCycle.md 7.7 (AC-3)".into(),
        comment: "Ask Priya whether Restricted counts here.".into(),
        ..Default::default()
    }];

    // 1. Exactly what share_queue hands to share_draft.
    let json = v2_lib::import_parser::queue_to_json_string(&queue).unwrap();

    // 2. share_draft posts it unchanged, so the bytes on the attachment
    //    are these bytes.
    let dir = std::env::temp_dir().join("tcm-v2-share-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}-shared.json", std::process::id()));
    std::fs::write(&path, &json).unwrap();

    // 3. What fetch_shared_queue does with the download.
    let (cases, warnings) =
        v2_lib::import_parser::parse_file(path.to_str().unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(cases.len(), 1);
    assert_eq!(
        cases[0].reviewer_notes,
        "Spec: Step10-ManagePerformanceCycle.md 7.7 (AC-3)",
        "the recipient must get the reviewer notes"
    );
    assert_eq!(
        cases[0].comment,
        "Ask Priya whether Restricted counts here.",
        "and the in-app comment"
    );
}
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
    // Taking the draft does NOT revoke - the caller does that once the
    // importer has actually read it. Revoking here burned the link on a
    // draft that then failed to parse.
    assert!(
        server.received_requests().await.unwrap().iter().all(|r| r.method != wiremock::http::Method::PATCH),
        "the link was revoked before the draft had been imported"
    );
    assert!(client.revoke_share(&share, &taken.pending_revoke).await.is_none(),
        "revoke succeeded - no warning");
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
    assert!(client
        .revoke_share(&share, &taken.pending_revoke)
        .await
        .unwrap()
        .contains("could not be revoked"));
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
