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

// ---- Required fields on a bug ---------------------------------------------
//
// An org whose Bug type has required custom fields refuses a create that
// leaves them out, with a 400 that names every field at once.

use v2_lib::ado::rule_errors::rule_violations;
use v2_lib::ado::AdoError;
use v2_lib::commands::bugs::{file_bug_with, BugFieldValue, FileBugError};
use wiremock::matchers::body_string_contains;

fn rule_body() -> String {
    let flags = "required, hasValues, limitedToValues, allowsOldValue, invalidEmpty";
    let mut errors = vec![serde_json::json!({
        "fieldReferenceName": "Custom.BugType",
        "fieldStatusFlags": flags,
        "errorMessage": "TF401320: Rule Error for field Bug Type. Error code: Required, HasValues, LimitedToValues, AllowsOldValue, InvalidEmpty.",
    })];
    for n in 1..7 {
        errors.push(serde_json::json!({
            "fieldReferenceName": format!("Custom.Field{n}"),
            "fieldStatusFlags": "required, invalidEmpty",
            "errorMessage": format!("TF401320: Rule Error for field Field {n}. Error code: Required, InvalidEmpty."),
        }));
    }
    serde_json::json!({
        "$id": "1",
        "customProperties": {
            "FieldReferenceName": null,
            "FieldStatusFlags": "none",
            "ErrorMessage": "TF401320: Rule Error for field Bug Type. Error code: Required, HasValues, LimitedToValues, AllowsOldValue, InvalidEmpty. 6 additional errors occurred during validation of the work item. Please correct all errors and try again.",
            "FieldStatusCode": 0,
            "RuleValidationErrors": errors,
        },
        "message": "TF401320: Rule Error for field Bug Type.",
    })
    .to_string()
}

#[test]
fn every_rule_violation_in_the_body_is_returned() {
    let v = rule_violations(&rule_body());
    assert_eq!(v.len(), 7, "{v:?}");
    assert_eq!(v[0].reference_name, "Custom.BugType");
    assert!(!v[0].message.starts_with("TF401320"), "{}", v[0].message);
    assert_eq!(v[6].reference_name, "Custom.Field6");
}

#[test]
fn a_message_carrying_a_url_is_replaced_by_the_flag_sentence() {
    let body = serde_json::json!({"customProperties": {"RuleValidationErrors": [{
        "fieldReferenceName": "Custom.BugType",
        "fieldStatusFlags": "required, invalidEmpty",
        "errorMessage": "TF401320: Rule Error. See https://dev.azure.com/x?secret=1",
    }]}})
    .to_string();
    let v = rule_violations(&body);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].message, "This field is required.");
    assert!(!v[0].message.contains("://"));
}

#[test]
fn a_body_that_is_not_a_rule_error_has_no_violations() {
    assert!(rule_violations(r#"{"message":"VS402323: The request is not valid."}"#).is_empty());
    assert!(rule_violations("not json at all").is_empty());
    assert!(rule_violations("").is_empty());
}

fn two_url_client(uri: &str) -> AdoClient {
    AdoClient::with_base_urls("tok".into(), uri.to_string(), uri.to_string())
}

#[tokio::test]
async fn required_fields_skip_what_ado_fills_and_fields_with_defaults() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/workitemtypes/Bug/fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"referenceName": "System.Title", "name": "Title", "alwaysRequired": true},
            {"referenceName": "System.AreaPath", "name": "Area Path", "alwaysRequired": true},
            {"referenceName": "Microsoft.VSTS.TCM.ReproSteps", "name": "Repro Steps", "alwaysRequired": true},
            {"referenceName": "Microsoft.VSTS.Common.Severity", "name": "Severity", "alwaysRequired": true, "defaultValue": "3 - Medium"},
            {"referenceName": "Custom.BugType", "name": "Bug Type", "alwaysRequired": true, "allowedValues": ["UI", "API"]},
            {"referenceName": "Custom.FoundOn", "name": "Found On", "alwaysRequired": true},
            {"referenceName": "Custom.Optional", "name": "Optional", "alwaysRequired": false},
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/fields/Custom.FoundOn"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"type": "dateTime"})))
        .mount(&server)
        .await;
    let client = two_url_client(&server.uri());
    let f = client
        .bug_field_definitions("o", "p", "Bug", "Microsoft.VSTS.TCM.ReproSteps", &[])
        .await
        .unwrap();
    assert_eq!(f.len(), 2, "{f:?}");
    assert_eq!(f[0].reference_name, "Custom.BugType");
    assert_eq!(f[0].kind, "pick");
    assert_eq!(f[0].allowed, vec!["UI", "API"]);
    assert_eq!(f[1].reference_name, "Custom.FoundOn");
    assert_eq!(f[1].kind, "date");
}

#[tokio::test]
async fn extra_refs_are_returned_even_when_not_always_required() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/workitemtypes/Bug/fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"referenceName": "Custom.RootCause", "name": "Root Cause", "alwaysRequired": false},
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/fields/Custom.RootCause"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"type": "string"})))
        .mount(&server)
        .await;
    let client = two_url_client(&server.uri());
    let f = client
        .bug_field_definitions("o", "p", "Bug", "Microsoft.VSTS.TCM.ReproSteps", &["Custom.RootCause".to_string()])
        .await
        .unwrap();
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].reference_name, "Custom.RootCause");
    assert_eq!(f[0].kind, "text");
}

/// The one read `file_bug_with` makes before the create: the type list
/// (Bug exists).
async fn bug_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wit/workitemtypes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [{"name": "Bug"}]})))
        .mount(&server)
        .await;
    server
}

fn value(r: &str, v: &str) -> BugFieldValue {
    BugFieldValue { reference_name: r.into(), value: v.into() }
}

#[tokio::test]
async fn field_values_go_in_the_create_patch() {
    let server = bug_server().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/workitems/$Bug"))
        .and(body_string_contains(r#"{"op":"add","path":"/fields/Custom.BugType","value":"UI"}"#))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 9, "_links": {"html": {"href": "https://x/9"}}})))
        .expect(1)
        .mount(&server)
        .await;
    let client = two_url_client(&server.uri());
    let bug = file_bug_with(&client, "o", "p", "T", "steps", 1, 2, &[], &[value("Custom.BugType", "UI")])
        .await
        .unwrap();
    assert_eq!(bug.id, 9);
}

#[tokio::test]
async fn field_values_never_override_title_or_repro_and_odd_refs_are_dropped() {
    let server = bug_server().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/workitems/$Bug"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 9})))
        .mount(&server)
        .await;
    let client = two_url_client(&server.uri());
    let values = [
        value("System.Title", "HIJACK"),
        value("Microsoft.VSTS.TCM.ReproSteps", "HIJACK"),
        value("Custom.X/../relations", "HIJACK"),
        value("Custom.Y", ""),
        value("Custom.Z", "kept"),
    ];
    file_bug_with(&client, "o", "p", "Dialog title", "steps", 1, 2, &[], &values).await.unwrap();
    let reqs = server.received_requests().await.unwrap();
    let post = reqs.iter().find(|r| r.method.as_str() == "POST").unwrap();
    let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
    let ops = body.as_array().unwrap();
    let on = |p: &str| ops.iter().filter(|o| o["path"] == p).count();
    assert_eq!(on("/fields/System.Title"), 1);
    assert!(ops.iter().any(|o| o["path"] == "/fields/System.Title" && o["value"] == "Dialog title"));
    assert_eq!(on("/fields/Microsoft.VSTS.TCM.ReproSteps"), 1);
    assert!(!String::from_utf8_lossy(&post.body).contains("HIJACK"));
    assert_eq!(on("/fields/Custom.Y"), 0);
    assert_eq!(on("/fields/Custom.Z"), 1);
    assert!(ops.iter().all(|o| !o["path"].as_str().unwrap().contains("..")));
}

#[tokio::test]
async fn a_rule_rejection_creates_nothing_and_returns_the_fields() {
    let server = bug_server().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/workitems/$Bug"))
        .respond_with(ResponseTemplate::new(400).set_body_string(rule_body()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/attachments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"url": "https://x/a"})))
        .expect(0)
        .mount(&server)
        .await;
    let client = two_url_client(&server.uri());
    let shots = vec![b64(&whole_png())];
    let err = file_bug_with(&client, "o", "p", "T", "steps", 1, 2, &shots, &[]).await.unwrap_err();
    match err {
        FileBugError::RuleViolations(v) => assert_eq!(v.len(), 7, "{v:?}"),
        other => panic!("expected rule violations, got {other:?}"),
    }
}

#[tokio::test]
async fn other_failures_stay_ado_errors() {
    let server = bug_server().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/workitems/$Bug"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let client = two_url_client(&server.uri());
    let err = file_bug_with(&client, "o", "p", "T", "steps", 1, 2, &[], &[]).await.unwrap_err();
    assert!(matches!(err, FileBugError::Ado(AdoError::Forbidden)), "{err:?}");
}
