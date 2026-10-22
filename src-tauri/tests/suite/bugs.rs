//! The repro steps of a Bug go to Azure DevOps as HTML. The text is Markdown
//! inside TCM, so a step's `**Save**` must arrive as bold, not as asterisks.

use v2_lib::commands::bugs::repro_html;

#[test]
fn bold_in_a_step_is_bold_in_the_bug_not_asterisks() {
    let html = repro_html("Test case #7: Save\n\nSteps:\n1. Click **Save** -> expected: it is *saved*");
    assert!(html.contains("<strong>Save</strong>"), "{html}");
    assert!(html.contains("<em>saved</em>"), "{html}");
    assert!(!html.contains("**"), "{html}");
}

#[test]
fn lines_and_code_survive_in_the_bug() {
    let html = repro_html("Steps:\n1. Run:\n```sql\nSELECT 1\n```\nDone\nsecond line");
    assert!(html.contains("<pre><code>"), "{html}");
    assert!(html.contains("SELECT 1"), "{html}");
    assert!(html.contains("Done<br>second line"), "{html}");
}

#[test]
fn what_the_tester_typed_cannot_become_markup() {
    let html = repro_html("see <img src=x onerror=alert(1)> and <cycleId> & more");
    assert!(!html.contains("<img"), "{html}");
    assert!(html.contains("&lt;cycleId&gt;"), "{html}");
    assert!(html.contains("&amp; more"), "{html}");
}

#[test]
fn plain_text_with_line_breaks_still_reads_the_same() {
    let html = repro_html("Test case #7: Save\n\nSteps:\nOpen the page");
    assert!(html.contains("Test case #7: Save"), "{html}");
    assert!(html.contains("Steps:<br>Open the page"), "{html}");
}

/// The repro text the Bug dialog builds: each step a list item, its other
/// lines indented under it. A fenced query inside a step closes inside that
/// step, so the code block holds only the query, and the next step is a
/// list item of its own (written as one line, the fence's closing line
/// carried the expected result and every later step into the `<pre>`).
#[test]
fn a_fenced_query_in_a_step_keeps_the_next_step_out_of_the_code_block() {
    let text = "Test case #201: Valid login\n\nSteps:\n1. Run the query:\n\n   ```sql\n   SELECT 1\n   ```\n   Expected: one row\n2. Open the report\n   Expected: It shows the row";
    let html = repro_html(text);
    assert_eq!(html.matches("<pre>").count(), 1, "{html}");
    assert!(html.contains("<pre><code>SELECT 1</code></pre>"), "{html}");
    let items: Vec<&str> = html.split("<li>").skip(1).collect();
    assert_eq!(items.len(), 2, "{html}");
    assert!(items[0].contains("Expected: one row") && items[0].contains("<pre>"), "{html}");
    assert!(items[1].contains("Open the report") && items[1].contains("Expected: It shows the row"), "{html}");
    assert!(!items[1].contains("<pre>"), "{html}");
}

// ---- Screenshots on a bug ------------------------------------------------
//
// A pasted screenshot can reach the Rust side empty or cut off (a clipboard
// that offered the picture before it was ready). Azure DevOps takes any
// file, so the bug would carry a broken image and the toast would still say
// "Filed bug". The upload refuses such a file and counts it as failed.

use base64::Engine;
use v2_lib::ado::AdoClient;
use v2_lib::commands::bugs::{attach_screenshots, bug_image_ext};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82];

fn whole_png() -> Vec<u8> {
    let mut v = PNG_SIG.to_vec();
    v.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89]);
    v.extend_from_slice(&IEND);
    v
}

fn whole_jpeg() -> Vec<u8> {
    vec![0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xff, 0xd9]
}

#[test]
fn a_whole_picture_of_each_kind_is_accepted_with_its_own_extension() {
    assert_eq!(bug_image_ext(&whole_png()), Ok("png"));
    assert_eq!(bug_image_ext(&whole_jpeg()), Ok("jpg"));
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&[1, 0, 1, 0, 0, 0, 0, 0x3b]);
    assert_eq!(bug_image_ext(&gif), Ok("gif"));
    let mut bmp = vec![0u8; 30];
    bmp[0] = b'B';
    bmp[1] = b'M';
    bmp[2] = 30;
    assert_eq!(bug_image_ext(&bmp), Ok("bmp"));
    let mut webp = b"RIFF".to_vec();
    webp.extend_from_slice(&[8, 0, 0, 0]);
    webp.extend_from_slice(b"WEBPVP8 ");
    assert_eq!(bug_image_ext(&webp), Ok("webp"));
}

#[test]
fn an_empty_or_cut_off_picture_is_refused() {
    assert_eq!(bug_image_ext(&[]), Err("the image is empty"));
    let png = whole_png();
    assert!(bug_image_ext(&png[..png.len() - 20]).is_err(), "a PNG without its end");
    assert!(bug_image_ext(&png[..5]).is_err());
    let jpeg = whole_jpeg();
    assert!(bug_image_ext(&jpeg[..jpeg.len() - 4]).is_err(), "a JPEG without its end");
    let mut bmp = vec![0u8; 30];
    bmp[0] = b'B';
    bmp[1] = b'M';
    bmp[2] = 200; // declares 200 bytes, holds 30
    assert!(bug_image_ext(&bmp).is_err(), "a BMP shorter than it says");
    let mut webp = b"RIFF".to_vec();
    webp.extend_from_slice(&[0xff, 0, 0, 0]);
    webp.extend_from_slice(b"WEBPVP8 ");
    assert!(bug_image_ext(&webp).is_err(), "a WebP shorter than it says");
    assert!(bug_image_ext(b"just some text, not a picture at all").is_err());
}

#[tokio::test]
async fn an_empty_and_a_cut_off_screenshot_are_never_uploaded_and_count_as_failed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/attachments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "url": "https://x/att/1" })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/o/p/_apis/wit/workitems/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 7 })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("tok".into(), server.uri());

    let png = whole_png();
    let shots = vec![
        b64(&png),            // whole: goes up
        String::new(),        // a paste that had not arrived: 0 bytes
        b64(&png[..png.len() - 20]), // cut off
        b64(&whole_jpeg()),   // whole, and keeps its own extension
    ];
    let failed = attach_screenshots(&client, "o", "p", 7, &shots).await;
    assert_eq!(failed, 2, "the empty one and the cut-off one are failures");

    let reqs = server.received_requests().await.unwrap();
    let uploads: Vec<String> = reqs
        .iter()
        .filter(|r| r.method.as_str() == "POST")
        .map(|r| r.url.query().unwrap_or_default().to_string())
        .collect();
    assert_eq!(uploads.len(), 2, "only the two whole pictures were uploaded: {uploads:?}");
    assert!(uploads[0].contains("fileName=bug-7-1.png"), "{uploads:?}");
    assert!(uploads[1].contains("fileName=bug-7-4.jpg"), "{uploads:?}");
    assert_eq!(reqs.iter().filter(|r| r.method.as_str() == "PATCH").count(), 2);
    assert!(
        reqs.iter().all(|r| r.method.as_str() != "DELETE"),
        "Azure DevOps stays GET/POST/PATCH"
    );
}
