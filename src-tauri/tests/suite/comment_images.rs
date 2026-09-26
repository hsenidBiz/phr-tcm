//! `collect_attachment_images` extended to find markdown `![alt](url)`
//! image references, not just HTML `src="..."` - Work Manager comments
//! and PR review threads store their attachment images that way. The
//! token-host guard (`attachment_download_url`) stays the only path a URL
//! is ever fetched through; these tests prove it still refuses a foreign
//! host when the reference arrives as markdown rather than HTML.
//!
//! Comments and PR threads go through `collect_comment_images`, not
//! `collect_attachment_images` (the detail drawer's own entry point) -
//! same extraction and guard, but its own higher image-count ceiling,
//! since a whole comment history arrives in one call. Most tests here call
//! `collect_comment_images` for that reason; the two cap tests near the
//! bottom prove each entry point keeps its own limit.

use v2_lib::ado::AdoClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PNG_BYTES: [u8; 4] = [137u8, 80, 78, 71];

#[tokio::test]
async fn markdown_attachment_image_is_downloaded() {
    let server = MockServer::start().await;
    let img_url = format!("{}/org/proj/_apis/wit/attachments/att-9?fileName=x.png", server.uri());
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/attachments/att-9"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "image/png")
                .set_body_bytes(PNG_BYTES.to_vec()),
        )
        .mount(&server)
        .await;

    let source = format!("Review Changes: ![image]({img_url})");
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let out = client.collect_comment_images(&[&source]).await;

    assert_eq!(out.len(), 1);
    assert!(out[0].url.contains("fileName=x.png"));
    assert!(out[0].data.starts_with("data:image/png;base64,"));
}

/// A pasted attachment name survives Azure DevOps' own encoding with a
/// literal, unescaped parenthesis pair - `Screenshot%20(1).png` - which the
/// old `[^)\s]+` capture truncated at the FIRST `)`, well before the real
/// end of the URL. One level of balanced parens must round-trip whole.
#[tokio::test]
async fn a_markdown_url_with_one_level_of_balanced_parens_is_captured_whole() {
    let server = MockServer::start().await;
    let img_url =
        format!("{}/org/proj/_apis/wit/attachments/att-7?fileName=Screenshot%20(1).png", server.uri());
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/attachments/att-7"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "image/png")
                .set_body_bytes(PNG_BYTES.to_vec()),
        )
        .mount(&server)
        .await;

    let source = format!("![image]({img_url})");
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let out = client.collect_comment_images(&[&source]).await;

    assert_eq!(out.len(), 1);
    assert!(
        out[0].url.ends_with("Screenshot%20(1).png"),
        "URL was truncated at the inner ')': {}",
        out[0].url
    );
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
    let out = client.collect_comment_images(&[&source]).await;

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
                .set_body_bytes(PNG_BYTES.to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let md_source = format!("![a]({img_url})");
    let html_source = format!("<img src=\"{img_url}\">");
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let out = client
        .collect_comment_images(&[&md_source, &html_source])
        .await;

    assert_eq!(out.len(), 1);
}

/// Registers 13 distinct attachments on one server, each its own mock
/// returning a tiny PNG, and returns the sources referencing them (one
/// markdown image per source, the way one comment per image would arrive).
async fn mount_distinct_attachments(server: &MockServer, count: usize) -> Vec<String> {
    let mut sources = Vec::new();
    for i in 0..count {
        let img_url = format!("{}/org/proj/_apis/wit/attachments/att-{i}", server.uri());
        Mock::given(method("GET"))
            .and(path(format!("/org/proj/_apis/wit/attachments/att-{i}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "image/png")
                    .set_body_bytes(PNG_BYTES.to_vec()),
            )
            .mount(server)
            .await;
        sources.push(format!("![img{i}]({img_url})"));
    }
    sources
}

/// A whole item's comment history, or a whole PR's thread list, routinely
/// references more distinct images than the drawer's own low cap allows -
/// `collect_comment_images` needs its own, higher one so image 13 does not
/// show "Image unavailable" when it was perfectly fetchable.
#[tokio::test]
async fn collect_comment_images_fetches_more_than_the_drawers_cap() {
    let server = MockServer::start().await;
    let sources = mount_distinct_attachments(&server, 13).await;
    let refs: Vec<&str> = sources.iter().map(String::as_str).collect();

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let out = client.collect_comment_images(&refs).await;

    assert_eq!(out.len(), 13);
}

/// The detail drawer's own entry point keeps its existing, lower cap -
/// this fix widened `collect_comment_images` only, not
/// `collect_attachment_images`.
#[tokio::test]
async fn collect_attachment_images_keeps_the_drawers_lower_cap() {
    let server = MockServer::start().await;
    let sources = mount_distinct_attachments(&server, 13).await;
    let refs: Vec<&str> = sources.iter().map(String::as_str).collect();

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let out = client.collect_attachment_images(&refs).await;

    assert_eq!(out.len(), 12);
}
