//! `collect_attachment_images` extended to find markdown `![alt](url)`
//! image references, not just HTML `src="..."` - Work Manager comments
//! and PR review threads store their attachment images that way. The
//! token-host guard (`attachment_download_url`) stays the only path a URL
//! is ever fetched through; these tests prove it still refuses a foreign
//! host when the reference arrives as markdown rather than HTML.

use v2_lib::ado::AdoClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn markdown_attachment_image_is_downloaded() {
    let server = MockServer::start().await;
    let img_url = format!("{}/org/proj/_apis/wit/attachments/att-9?fileName=x.png", server.uri());
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/attachments/att-9"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "image/png")
                .set_body_bytes(vec![137u8, 80, 78, 71]),
        )
        .mount(&server)
        .await;

    let source = format!("Review Changes: ![image]({img_url})");
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let out = client.collect_attachment_images(&[&source]).await;

    assert_eq!(out.len(), 1);
    assert!(out[0].url.contains("fileName=x.png"));
    assert!(out[0].data.starts_with("data:image/png;base64,"));
}

/// The half that matters: a comment's markdown is written by whoever can
/// post it, so the URL inside `![...](...)` is exactly as attacker
/// controlled as an HTML `src`. A foreign host must never see the token -
/// `expect(0)` fails the test the moment a request reaches it, whether or
/// not the guard actually stops it.
#[tokio::test]
async fn markdown_image_on_an_untrusted_host_is_never_fetched() {
    let server = MockServer::start().await;
    let evil = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_apis/wit/attachments/1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&evil)
        .await;

    let source = format!("![image]({}/_apis/wit/attachments/1)", evil.uri());
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let out = client.collect_attachment_images(&[&source]).await;

    assert!(out.is_empty());
    // Verified on drop, but spelled out here so a failure names the test
    // that meant it rather than a bare panic at scope exit.
    evil.verify().await;
}

/// The same attachment referenced twice - once as markdown in one source,
/// once as HTML in another, the way one comment quotes another's image -
/// is fetched once. `expect(1)` fails if the second mention triggers a
/// second request.
#[tokio::test]
async fn duplicate_urls_across_sources_are_fetched_once() {
    let server = MockServer::start().await;
    let img_url = format!("{}/org/proj/_apis/wit/attachments/att-1?fileName=x.png", server.uri());
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/attachments/att-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "image/png")
                .set_body_bytes(vec![137u8, 80, 78, 71]),
        )
        .expect(1)
        .mount(&server)
        .await;

    let md_source = format!("![a]({img_url})");
    let html_source = format!("<img src=\"{img_url}\">");
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let out = client
        .collect_attachment_images(&[&md_source, &html_source])
        .await;

    assert_eq!(out.len(), 1);
}
