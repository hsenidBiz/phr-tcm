//! Every request shares a 60 s start-to-last-byte deadline. An attachment
//! upload - the runner's screen recording, or "Attach file" - can
//! legitimately take far longer than that on a slow link, so both uploads
//! carry their own deadline scaled to the body size.

use std::time::Duration;
use v2_lib::ado::{upload_timeout, AdoClient, HTTP_TIMEOUT, UPLOAD_TIMEOUT_CAP};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn a_small_upload_keeps_the_ordinary_deadline() {
    assert_eq!(upload_timeout(0), HTTP_TIMEOUT);
    assert_eq!(upload_timeout(1024 * 1024), HTTP_TIMEOUT);
}

#[test]
fn a_large_upload_gets_one_second_per_128_kib() {
    let forty_mb = 40 * 1024 * 1024;
    assert_eq!(upload_timeout(forty_mb), Duration::from_secs(320));
    assert!(upload_timeout(forty_mb) > HTTP_TIMEOUT);
}

#[test]
fn a_huge_upload_is_capped_at_thirty_minutes() {
    assert_eq!(UPLOAD_TIMEOUT_CAP, Duration::from_secs(30 * 60));
    assert_eq!(upload_timeout(usize::MAX), UPLOAD_TIMEOUT_CAP);
    assert_eq!(upload_timeout(10 * 1024 * 1024 * 1024), UPLOAD_TIMEOUT_CAP);
}

/// The client's ordinary deadline is shrunk to 300 ms so the suite stays
/// fast; the mock answers after 800 ms. An ordinary POST gives up, while
/// both uploads - whose deadline is max(HTTP_TIMEOUT, size-scaled) - wait.
async fn slow_server(route: &str, body: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(800))
                .set_body_json(body),
        )
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn attach_file_outlasts_the_ordinary_deadline() {
    let server = slow_server("/o/p/_apis/wit/attachments", serde_json::json!({ "url": "https://x/att/1" })).await;
    let client = AdoClient::with_base_url_and_timeout("tok".into(), server.uri(), Duration::from_millis(300));
    assert_eq!(
        client.upload_wi_attachment("o", "p", "a.png", vec![1, 2, 3]).await.unwrap(),
        "https://x/att/1"
    );
}

#[tokio::test]
async fn a_result_attachment_outlasts_the_ordinary_deadline() {
    let server = slow_server("/o/p/_apis/test/Runs/7/Results/9/attachments", serde_json::json!({ "id": 1 })).await;
    let client = AdoClient::with_base_url_and_timeout("tok".into(), server.uri(), Duration::from_millis(300));
    client
        .add_result_attachment("o", "p", 7, 9, "AAAA", "rec.webm", "")
        .await
        .unwrap();
}

/// The control: the same client and delay DO time out an ordinary request,
/// so the two tests above pass because of the upload deadline, not a
/// shrunk timeout that never applied.
#[tokio::test]
async fn an_ordinary_request_still_times_out_on_that_client() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/projects"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(800))
                .set_body_json(serde_json::json!({ "count": 0, "value": [] })),
        )
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url_and_timeout("tok".into(), server.uri(), Duration::from_millis(300));
    assert!(
        client.get_projects("org").await.is_err(),
        "a 300 ms client deadline must cut off an 800 ms reply"
    );
}
