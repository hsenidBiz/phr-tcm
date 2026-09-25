//! Work-item @mentions of the signed-in user, the bell's work-item source:
//! which items are read, which comments count, and what a failure skips.

use v2_lib::ado::{AdoClient, AdoError};
use v2_lib::work_board::mentions::{excerpt, mentions_me, Mention, EXCERPT_CHARS};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ME: &str = "me-guid";

/// A mention the way Azure DevOps stores one in a comment's HTML.
fn anchor(id: &str, name: &str) -> String {
    format!("<a href=\"#\" data-vss-mention=\"version:2.0,{id}\">@{name}</a>")
}

fn comment(id: i32, author_id: &str, author: &str, html: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "text": html,
        "createdBy": { "id": author_id, "displayName": author },
        "createdDate": "2026-09-25T08:00:00Z"
    })
}

/// The WIQL, matched on everything the query must say.
async fn mount_query(server: &MockServer, ids: &[i32]) {
    let items: Vec<serde_json::Value> = ids.iter().map(|i| serde_json::json!({ "id": i })).collect();
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .and(query_param("$top", "20"))
        .and(body_string_contains("[System.TeamProject] = @project"))
        .and(body_string_contains("[System.Id] IN (@RecentMentions)"))
        .and(body_string_contains("ORDER BY [System.ChangedDate] DESC"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "workItems": items })))
        .mount(server)
        .await;
}

async fn mount_titles(server: &MockServer, value: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems"))
        .and(query_param("errorPolicy", "omit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": value })))
        .mount(server)
        .await;
}

async fn mount_comments(server: &MockServer, item: i32, comments: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/org/proj/_apis/wit/workItems/{item}/comments")))
        .and(query_param("order", "desc"))
        .and(query_param("$top", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "comments": comments })))
        .mount(server)
        .await;
}

fn client(server: &MockServer) -> AdoClient {
    AdoClient::with_base_urls("tok".into(), server.uri(), server.uri())
}

#[tokio::test]
async fn a_comment_mentioning_you_is_found_with_its_item_and_excerpt() {
    let server = MockServer::start().await;
    mount_query(&server, &[41]).await;
    mount_titles(
        &server,
        serde_json::json!([{ "id": 41, "fields": {
            "System.Title": "Leave requests", "System.WorkItemType": "Product Backlog Item"
        } }]),
    )
    .await;
    // The anchor carries the id in upper case: the match ignores case.
    let html = format!("<div>{} can you   check<br>this?</div>", anchor("ME-GUID", "Avin"));
    mount_comments(&server, 41, vec![comment(7, "u-sam", "Sam", &html)]).await;

    let found = client(&server).recent_mentions("org", "proj", ME).await.unwrap();

    assert_eq!(
        found,
        vec![Mention {
            source: "work-item".into(),
            item_id: 41,
            item_type: "Product Backlog Item".into(),
            item_title: "Leave requests".into(),
            comment_id: 7,
            author: "Sam".into(),
            excerpt: "@Avin can you check this?".into(),
            created_date: "2026-09-25T08:00:00Z".into(),
        }]
    );
}

#[tokio::test]
async fn your_own_comments_and_mentions_of_someone_else_are_ignored() {
    let server = MockServer::start().await;
    mount_query(&server, &[41]).await;
    mount_titles(&server, serde_json::json!([])).await;
    mount_comments(
        &server,
        41,
        vec![
            comment(1, "ME-GUID", "Avin", &anchor(ME, "Avin")),
            comment(2, "u-sam", "Sam", &anchor("kim-guid", "Kim")),
            comment(3, "u-sam", "Sam", &anchor(ME, "Avin")),
        ],
    )
    .await;

    let found = client(&server).recent_mentions("org", "proj", ME).await.unwrap();

    assert_eq!(found.iter().map(|m| m.comment_id).collect::<Vec<_>>(), vec![3]);
    assert_eq!(found[0].item_type, "Work item", "an item whose title could not be read");
    assert_eq!(found[0].item_title, "");
}

#[test]
fn the_excerpt_strips_html_collapses_whitespace_and_stops_at_140_characters() {
    assert_eq!(excerpt("<div><b>Tom</b> &amp; Jerry,\n\n  <i>please</i></div>"), "Tom & Jerry, please");
    let long = format!("<p>{}</p>", "word ".repeat(60));
    let cut = excerpt(&long);
    assert_eq!(cut.chars().count(), EXCERPT_CHARS);
    assert!(cut.ends_with('…'), "{cut}");
    assert!(!cut.contains('<'));
}

#[test]
fn a_mention_matches_the_exact_anchor_ignoring_case() {
    assert!(mentions_me(&anchor("ME-GUID", "Avin"), "me-guid"));
    assert!(!mentions_me(&anchor("me-guid-2", "Other"), "me-guid"));
    assert!(!mentions_me("<div>@Avin in plain text</div>", "me-guid"));
}

#[tokio::test]
async fn one_items_failed_comment_read_skips_only_that_item() {
    let server = MockServer::start().await;
    mount_query(&server, &[41, 42]).await;
    mount_titles(&server, serde_json::json!([])).await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workItems/41/comments"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    mount_comments(&server, 42, vec![comment(9, "u-sam", "Sam", &anchor(ME, "Avin"))]).await;

    let found = client(&server).recent_mentions("org", "proj", ME).await.unwrap();

    assert_eq!(found.iter().map(|m| (m.item_id, m.comment_id)).collect::<Vec<_>>(), vec![(42, 9)]);
}

#[tokio::test]
async fn a_failed_query_fails_the_check() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let err = client(&server).recent_mentions("org", "proj", ME).await.unwrap_err();

    assert!(matches!(err, AdoError::Http { status: 500, .. }), "{err:?}");
}

/// Review focus 3: every mention anchor starts with the same prefix, so an
/// empty id would match every mention of anyone. It matches nothing, and
/// asks Azure DevOps nothing.
#[tokio::test]
async fn an_identity_without_an_id_matches_nothing() {
    let server = MockServer::start().await;

    let found = client(&server).recent_mentions("org", "proj", "  ").await.unwrap();

    assert!(found.is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(!mentions_me(&anchor("", "Nobody"), ""));
}

/// Read once per organization per session, memory only.
#[tokio::test]
async fn the_signed_in_identity_is_read_once_per_organization() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/connectionData"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authenticatedUser": { "id": "u-ada", "providerDisplayName": "Ada Lovelace" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server);

    assert_eq!(c.connected_user_cached("org").await.unwrap().id, "u-ada");
    assert_eq!(c.connected_user_cached("org").await.unwrap().id, "u-ada");
}
