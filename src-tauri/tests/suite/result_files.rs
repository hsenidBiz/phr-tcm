//! Listing every attachment on a test result, streaming one download to a
//! file, and writing a session file from base64.

use v2_lib::ado::{AdoClient, AdoError};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LIST: &str = r"/Runs/7/Results/9/attachments$";
const ONE: &str = r"/Runs/7/Results/9/attachments/(\d+)$";

fn client(server: &MockServer) -> AdoClient {
    AdoClient::with_base_url("tok".into(), server.uri())
}

async fn mock_list(server: &MockServer, files: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path_regex(LIST))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": files })))
        .mount(server)
        .await;
}

/// Files in `dir` other than `keep` - a leftover temp file shows up here.
fn strays(dir: &std::path::Path, keep: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != keep)
        .collect()
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfakepng";

#[tokio::test]
async fn attachments_are_listed_with_size_and_kind_and_images_get_thumbnails() {
    let server = MockServer::start().await;
    mock_list(
        &server,
        serde_json::json!([
            {"id": 1, "fileName": "shot.png", "size": 69},
            {"id": 2, "fileName": "rec.webm", "size": 5000000},
            {"id": 3, "fileName": "log.txt", "size": 10}
        ]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/attachments/1$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PNG))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/attachments/2$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"video"))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/attachments/3$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"txt"))
        .expect(0)
        .mount(&server)
        .await;

    let list = client(&server).get_result_attachments("o", "p", 7, 9).await.unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(
        list.iter().map(|a| a.kind.as_str()).collect::<Vec<_>>(),
        ["image", "video", "file"]
    );
    assert_eq!(list[0].size_bytes, 69);
    assert_eq!(list[1].size_bytes, 5_000_000);
    assert!(list[0].thumb_b64.is_some());
    assert!(list[1].thumb_b64.is_none() && list[2].thumb_b64.is_none());
}

#[tokio::test]
async fn a_thumbnail_that_fails_still_lists_the_file() {
    let server = MockServer::start().await;
    mock_list(&server, serde_json::json!([{"id": 1, "fileName": "shot.png", "size": 69}])).await;
    Mock::given(method("GET"))
        .and(path_regex(r"/attachments/1$"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let list = client(&server).get_result_attachments("o", "p", 7, 9).await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].thumb_b64.is_none());
}

#[tokio::test]
async fn a_download_is_streamed_to_the_chosen_path() {
    let server = MockServer::start().await;
    let body: Vec<u8> = (0..300 * 1024).map(|i| (i % 251) as u8).collect();
    Mock::given(method("GET"))
        .and(path_regex(ONE))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let n = client(&server)
        .download_result_attachment("o", "p", 7, 9, 5, body.len() as u32, &dest)
        .await
        .unwrap();
    assert_eq!(n, body.len() as u64);
    assert_eq!(std::fs::read(&dest).unwrap(), body);
    assert!(strays(dir.path(), "out.bin").is_empty(), "a temp file was left");
}

#[tokio::test]
async fn a_failed_download_leaves_no_file() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(ONE))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let err = client(&server)
        .download_result_attachment("o", "p", 7, 9, 5, 10, &dest)
        .await
        .unwrap_err();
    assert!(matches!(err, AdoError::NotFound), "{err:?}");
    assert!(!dest.exists());
    assert!(strays(dir.path(), "out.bin").is_empty() || !dest.exists());
    assert!(strays(dir.path(), "").is_empty());

    std::fs::write(&dest, b"old").unwrap();
    let _ = client(&server)
        .download_result_attachment("o", "p", 7, 9, 5, 10, &dest)
        .await;
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    assert_eq!(strays(dir.path(), "out.bin"), Vec::<String>::new());
}

#[tokio::test]
async fn a_download_cut_off_part_way_leaves_no_file() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = sock.read(&mut buf).await;
        let head = "HTTP/1.1 200 OK
Content-Length: 1000000
Content-Type: application/octet-stream

";
        sock.write_all(head.as_bytes()).await.unwrap();
        sock.write_all(&vec![7u8; 1000]).await.unwrap();
        sock.flush().await.unwrap();
        // Close with most of the body unsent.
    });
    let client = AdoClient::with_base_url("tok".into(), format!("http://{addr}"));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    std::fs::write(&dest, b"old").unwrap();
    let err = client
        .download_result_attachment("o", "p", 7, 9, 5, 1_000_000, &dest)
        .await
        .unwrap_err();
    assert!(matches!(err, AdoError::Network(_)), "{err:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    assert!(strays(dir.path(), "out.bin").is_empty(), "a temp file was left");
}

#[tokio::test]
async fn a_large_image_gets_no_thumbnail_and_no_request() {
    let server = MockServer::start().await;
    mock_list(&server, serde_json::json!([{"id": 1, "fileName": "big.png", "size": 3_000_000}])).await;
    Mock::given(method("GET"))
        .and(path_regex(r"/attachments/1$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PNG))
        .expect(0)
        .mount(&server)
        .await;
    let list = client(&server).get_result_attachments("o", "p", 7, 9).await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].thumb_b64.is_none());
    assert_eq!(list[0].kind, "image");
}

#[tokio::test]
async fn only_the_first_twelve_images_get_thumbnails() {
    let server = MockServer::start().await;
    let files: Vec<_> = (1..=13)
        .map(|i| serde_json::json!({"id": i, "fileName": format!("s{i}.png"), "size": 69}))
        .collect();
    mock_list(&server, serde_json::Value::Array(files)).await;
    Mock::given(method("GET"))
        .and(path_regex(ONE))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PNG))
        .mount(&server)
        .await;
    let list = client(&server).get_result_attachments("o", "p", 7, 9).await.unwrap();
    assert_eq!(list.len(), 13);
    assert_eq!(list.iter().filter(|a| a.thumb_b64.is_some()).count(), 12);
    assert!(list[12].thumb_b64.is_none());
}

#[tokio::test]
async fn an_expired_session_is_unauthorized_not_saved() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(ONE))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let err = client(&server)
        .download_result_attachment("o", "p", 7, 9, 5, 10, &dest)
        .await
        .unwrap_err();
    assert!(matches!(err, AdoError::Unauthorized), "{err:?}");
    assert!(!dest.exists());
}

#[tokio::test]
async fn a_download_error_names_no_url() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let client = AdoClient::with_base_url("tok".into(), format!("http://{addr}"));
    let dir = tempfile::tempdir().unwrap();
    let err = client
        .download_result_attachment("o", "p", 7, 9, 5, 10, &dir.path().join("x"))
        .await
        .unwrap_err();
    let AdoError::Network(s) = err else { panic!("expected Network, got {err:?}") };
    assert!(!s.contains("://"), "{s}");
}

#[test]
fn a_session_file_is_written_by_rust() {
    use base64::Engine;
    use v2_lib::commands::bugs::save_attachment_b64;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("rec.webm");
    let ok = base64::engine::general_purpose::STANDARD.encode(b"abc");
    save_attachment_b64(dest.to_string_lossy().into(), ok).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"abc");

    let bad = dir.path().join("bad.webm");
    let err = save_attachment_b64(bad.to_string_lossy().into(), "!!not base64!!".into()).unwrap_err();
    assert!(!err.contains("://"));
    assert!(!bad.exists());
    assert!(strays(dir.path(), "rec.webm").is_empty(), "a temp file was left");
}

#[tokio::test]
async fn screenshots_for_the_preview_are_unchanged() {
    use base64::Engine;
    let server = MockServer::start().await;
    mock_list(
        &server,
        serde_json::json!([
            {"id": 1, "fileName": "shot.png", "size": 69},
            {"id": 2, "fileName": "rec.webm", "size": 5000000},
            {"id": 3, "fileName": "log.txt", "size": 10}
        ]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/attachments/1$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PNG))
        .mount(&server)
        .await;
    let shots = client(&server).get_result_screenshots("o", "p", 7, 9).await.unwrap();
    assert_eq!(shots, vec![base64::engine::general_purpose::STANDARD.encode(PNG)]);
}
