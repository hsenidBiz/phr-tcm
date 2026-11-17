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

#[test]
fn read_file_b64_refuses_a_folder_without_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let err = v2_lib::commands::bugs::read_file_b64(dir.path().to_string_lossy().into()).err().unwrap();
    assert_eq!(err, "Folders can't be attached - attach the files inside it.");
}

#[test]
fn read_file_b64_refuses_an_oversize_file_without_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("secret-name.bin");
    std::fs::File::create(&big).unwrap().set_len(25 * 1024 * 1024 + 1).unwrap();
    let err = v2_lib::commands::bugs::read_file_b64(big.to_string_lossy().into()).err().unwrap();
    assert_eq!(err, "File is larger than 25 MB.");
    assert!(!err.contains("secret-name"));
}

#[test]
fn a_result_attachment_is_checked_in_rust_without_naming_a_path() {
    use base64::Engine;
    use v2_lib::commands::bugs::{refuse_attachment, refuse_result_attachment, MAX_RESULT_FILE};
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    // An allowed file goes through: a log, and a real picture.
    assert!(refuse_result_attachment("run.log", &b64(b"hello")).is_ok());
    let whole_png: Vec<u8> = [&PNG[..8], &[0, 0, 0, 0], b"IEND".as_slice(), &[0xae, 0x42, 0x60, 0x82]].concat();
    assert!(refuse_result_attachment("shot.png", &b64(&whole_png)).is_ok());
    // Over the cap: refused from the size alone, no data decoded.
    let over = "A".repeat((MAX_RESULT_FILE as usize / 3 + 4) * 4);
    let err = refuse_result_attachment("big.bin", &over).unwrap_err();
    assert_eq!(err, "File is larger than 100 MB.");
    assert!(refuse_attachment("x.bin", 25 * 1024 * 1024 + 1, 25 * 1024 * 1024).is_err());
    // Empty, and a name that is a path or a folder: refused, the path never in the sentence.
    assert_eq!(refuse_result_attachment("empty.txt", "").unwrap_err(), "The file is empty.");
    let windows_path = ["C:", "secret", "dir", "a.txt"].join(&String::from('\u{5c}'));
    let drive_relative = format!("C{}secret.txt", ':');
    for name in [windows_path.as_str(), drive_relative.as_str(), "../../etc/passwd", "folder/"] {
        let err = refuse_result_attachment(name, &b64(b"x")).unwrap_err();
        assert!(!err.contains("secret") && !err.contains("etc"), "{err}");
    }
    // A picture that is not a picture (a disallowed type for that name) is refused.
    let err = refuse_result_attachment("fake.png", &b64(b"MZ not an image")).unwrap_err();
    assert!(err.starts_with("The picture can't be attached"), "{err}");
    assert!(refuse_result_attachment("fake.png", "!!!not base64!!!").is_err());
}

#[test]
fn read_file_b64_refuses_an_empty_file_and_passes_an_allowed_one() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty.txt");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(v2_lib::commands::bugs::read_file_b64(empty.to_string_lossy().into()).err().unwrap(), "The file is empty.");
    let ok = dir.path().join("notes.txt");
    std::fs::write(&ok, b"hello").unwrap();
    let got = v2_lib::commands::bugs::read_file_b64(ok.to_string_lossy().into()).unwrap();
    assert_eq!(got.file_name, "notes.txt");
}

#[test]
fn a_jpeg_with_data_after_its_end_is_allowed_for_a_result_but_a_cut_one_is_not() {
    use base64::Engine;
    use v2_lib::commands::bugs::refuse_result_attachment;
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let motion: Vec<u8> = [&[0xff, 0xd8, 0xff, 0xe0, 1, 2, 3][..], &[0xff, 0xd9], b"ftypmp4 video bytes"].concat();
    assert!(refuse_result_attachment("photo.jpg", &b64(&motion)).is_ok());
    let cut = [0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 4];
    assert!(refuse_result_attachment("photo.jpg", &b64(&cut)).is_err());
}

#[test]
fn a_bug_screenshot_over_the_cap_is_counted_as_failed() {
    use v2_lib::commands::bugs::{refuse_attachment, MAX_PICKED_FILE};
    assert!(refuse_attachment("bug-1-1.png", MAX_PICKED_FILE + 1, MAX_PICKED_FILE).is_err());
    assert!(refuse_attachment("bug-1-1.png", 10, MAX_PICKED_FILE).is_ok());
}
