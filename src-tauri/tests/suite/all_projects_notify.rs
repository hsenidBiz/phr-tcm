//! Task 13: mentions and newly assigned work across projects.
//!
//! The global cache has no lock, so every test that reads or writes the
//! assigned baselines uses an organization name of its own.

use v2_lib::assigned_watch::{check_once_all, ASSIGNED_ALL_WIQL, ASSIGNED_WIQL};
use v2_lib::cache::{self, keys};
use v2_lib::work_board::mentions::{Mention, RECENT_MENTIONS_ALL_WIQL};
use v2_lib::ado::AdoClient;
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ME: &str = "me-guid";

fn client(server: &MockServer) -> AdoClient {
    AdoClient::with_base_urls("tok".into(), server.uri(), server.uri())
}

fn anchor() -> String {
    format!("<a href=\"#\" data-vss-mention=\"version:2.0,{ME}\">@Avin</a>")
}

fn comment(id: i32, html: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "text": html,
        "createdBy": { "id": "u-sam", "displayName": "Sam" },
        "createdDate": "2026-09-25T08:00:00Z"
    })
}

fn wiql_answer(ids: &[i32]) -> serde_json::Value {
    let items: Vec<serde_json::Value> = ids.iter().map(|i| serde_json::json!({ "id": i })).collect();
    serde_json::json!({ "workItems": items })
}

// ---------------------------------------------------------------- mentions

/// One organization-scope mentions query, matched on everything it must say
/// and on what it must not: a project clause would pin the answer to one
/// project.
async fn mount_mentions_query(server: &MockServer, org: &str, ids: &[i32]) {
    Mock::given(method("POST"))
        .and(path(format!("/{org}/_apis/wit/wiql")))
        .and(query_param("$top", "20"))
        .and(body_string_contains("[System.Id] IN (@RecentMentions)"))
        .and(body_string_contains("ORDER BY [System.ChangedDate] DESC"))
        .respond_with(ResponseTemplate::new(200).set_body_json(wiql_answer(ids)))
        .expect(1)
        .mount(server)
        .await;
}

async fn mount_org_titles(server: &MockServer, org: &str, value: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/wit/workitems")))
        .and(query_param("errorPolicy", "omit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": value })))
        .mount(server)
        .await;
}

fn titled(id: i32, title: &str, project: &str) -> serde_json::Value {
    serde_json::json!({ "id": id, "fields": {
        "System.Title": title,
        "System.WorkItemType": "Product Backlog Item",
        "System.TeamProject": project
    } })
}

#[test]
fn the_all_projects_mentions_query_names_no_project() {
    assert!(RECENT_MENTIONS_ALL_WIQL.contains("[System.Id] IN (@RecentMentions)"));
    assert!(!RECENT_MENTIONS_ALL_WIQL.contains("TeamProject"), "{RECENT_MENTIONS_ALL_WIQL}");
    assert!(!RECENT_MENTIONS_ALL_WIQL.contains("@project"), "{RECENT_MENTIONS_ALL_WIQL}");
}

#[tokio::test]
async fn all_projects_mentions_ask_at_organization_scope_without_a_project_clause() {
    let server = MockServer::start().await;
    // The only WIQL route is the organization one: a request to any
    // project's route has no mock and answers 404, which would fail the check.
    mount_mentions_query(&server, "notify-m1", &[41]).await;
    mount_org_titles(&server, "notify-m1", serde_json::json!([titled(41, "Leave requests", "Payroll")])).await;
    Mock::given(method("GET"))
        .and(path("/notify-m1/Payroll/_apis/wit/workItems/41/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comments": [comment(7, &format!("<div>{} look</div>", anchor()))]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let found = client(&server).recent_mentions_all("notify-m1", ME).await.unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].project, "Payroll");
    // The query body must not name a project.
    let requests = server.received_requests().await.unwrap();
    let wiql = requests
        .iter()
        .find(|r| r.url.path() == "/notify-m1/_apis/wit/wiql")
        .expect("the query went to the organization");
    let body = String::from_utf8_lossy(&wiql.body).to_string();
    assert!(!body.contains("TeamProject"), "{body}");
    assert!(!body.contains("@project"), "{body}");
}

#[tokio::test]
async fn all_projects_mentions_read_each_items_comments_in_its_own_project() {
    let server = MockServer::start().await;
    mount_mentions_query(&server, "notify-m2", &[41, 52]).await;
    mount_org_titles(
        &server,
        "notify-m2",
        serde_json::json!([titled(41, "Leave requests", "A"), titled(52, "Payslips", "B")]),
    )
    .await;
    for (project, item, comment_id) in [("A", 41, 7), ("B", 52, 9)] {
        Mock::given(method("GET"))
            .and(path(format!("/notify-m2/{project}/_apis/wit/workItems/{item}/comments")))
            .and(query_param("order", "desc"))
            .and(query_param("$top", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "comments": [comment(comment_id, &anchor())]
            })))
            .expect(1)
            .mount(&server)
            .await;
    }

    let found = client(&server).recent_mentions_all("notify-m2", ME).await.unwrap();

    let summary: Vec<(i32, i32, String, String)> = found
        .iter()
        .map(|m| (m.item_id, m.comment_id, m.project.clone(), m.item_title.clone()))
        .collect();
    assert_eq!(
        summary,
        vec![
            (41, 7, "A".to_string(), "Leave requests".to_string()),
            (52, 9, "B".to_string(), "Payslips".to_string()),
        ]
    );
    assert_eq!(found[0].source, "work-item");
    assert_eq!(found[0].item_type, "Product Backlog Item");
}

#[tokio::test]
async fn an_item_whose_project_could_not_be_read_is_skipped() {
    let server = MockServer::start().await;
    mount_mentions_query(&server, "notify-m3", &[41, 52]).await;
    // 52 comes back as an omitted (null) entry: no project to ask in.
    mount_org_titles(
        &server,
        "notify-m3",
        serde_json::json!([titled(41, "Leave requests", "A"), null]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/notify-m3/A/_apis/wit/workItems/41/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comments": [comment(7, &anchor())]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let found = client(&server).recent_mentions_all("notify-m3", ME).await.unwrap();

    assert_eq!(found.iter().map(|m| m.item_id).collect::<Vec<_>>(), vec![41]);
}

#[tokio::test]
async fn an_identity_with_no_id_finds_no_mentions_without_asking() {
    let server = MockServer::start().await;
    let found = client(&server).recent_mentions_all("notify-m4", "  ").await.unwrap();
    assert!(found.is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn per_project_mentions_carry_the_argument_project() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/notify-m5/proj/_apis/wit/wiql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(wiql_answer(&[41])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/notify-m5/proj/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [] })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/notify-m5/proj/_apis/wit/workItems/41/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comments": [comment(7, &anchor())]
        })))
        .mount(&server)
        .await;

    let found: Vec<Mention> = client(&server).recent_mentions("notify-m5", "proj", ME).await.unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].project, "proj");
}

// ----------------------------------------------------------- assigned work

fn assigned(id: i32, project: &str) -> serde_json::Value {
    serde_json::json!({ "id": id, "fields": {
        "System.Title": format!("Item {id}"),
        "System.WorkItemType": "Task",
        "System.State": "Active",
        "System.TeamProject": project
    } })
}

/// The organization-scope assigned-work query and the read of what it found.
/// `items` is (id, project).
async fn mount_assigned(server: &MockServer, org: &str, items: &[(i32, &str)]) {
    let ids: Vec<i32> = items.iter().map(|(i, _)| *i).collect();
    Mock::given(method("POST"))
        .and(path(format!("/{org}/_apis/wit/wiql")))
        .and(query_param("$top", "50"))
        .and(body_string_contains("[System.AssignedTo] = @Me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(wiql_answer(&ids)))
        .mount(server)
        .await;
    let value: Vec<serde_json::Value> = items.iter().map(|(i, p)| assigned(*i, p)).collect();
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/wit/workitems")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": value })))
        .mount(server)
        .await;
}

fn forget(org: &str, home: &str) {
    cache::forget(&keys::assigned_seen_all(org));
    cache::forget(&keys::assigned_seen(org, home));
}

#[test]
fn the_all_projects_assigned_query_is_the_per_project_one() {
    assert_eq!(ASSIGNED_ALL_WIQL, ASSIGNED_WIQL);
    assert!(!ASSIGNED_ALL_WIQL.contains("TeamProject"));
}

#[test]
fn the_all_projects_baseline_has_its_own_key() {
    assert_eq!(keys::assigned_seen_all("acme"), "assigned-seen-all:acme");
}

#[tokio::test]
async fn all_projects_assigned_work_asks_at_organization_scope_and_names_each_project() {
    let server = MockServer::start().await;
    mount_assigned(&server, "notify-a1", &[(1, "A"), (2, "B")]).await;

    let items = client(&server).assigned_to_me_all("notify-a1").await.unwrap();

    assert_eq!(
        items.iter().map(|i| (i.id, i.project.as_str())).collect::<Vec<_>>(),
        vec![(1, "A"), (2, "B")]
    );
    let requests = server.received_requests().await.unwrap();
    let wiql = requests
        .iter()
        .find(|r| r.url.path() == "/notify-a1/_apis/wit/wiql")
        .expect("the query went to the organization");
    assert!(!String::from_utf8_lossy(&wiql.body).contains("TeamProject"));
    let read = requests
        .iter()
        .find(|r| r.url.path() == "/notify-a1/_apis/wit/workitems")
        .expect("the items were read at organization scope");
    let query = read.url.query().unwrap_or_default().to_string();
    assert!(query.contains("System.TeamProject"), "{query}");
}

#[tokio::test]
async fn the_first_all_projects_check_announces_nothing_it_cannot_date() {
    let org = "notify-a2";
    forget(org, "Home");
    let server = MockServer::start().await;
    mount_assigned(&server, org, &[(1, "Home"), (3, "Other")]).await;

    let fresh = check_once_all(&client(&server), org, "Home").await.unwrap();

    assert!(fresh.is_empty());
    let stored = cache::get::<Vec<String>>(&keys::assigned_seen_all(org)).expect("a baseline was stored");
    assert_eq!(stored, vec!["1", "3"]);
}

#[tokio::test]
async fn the_first_all_projects_check_keeps_the_home_projects_news() {
    let org = "notify-a3";
    forget(org, "Home");
    cache::put(&keys::assigned_seen(org, "Home"), &vec!["1".to_string()]);
    let server = MockServer::start().await;
    mount_assigned(&server, org, &[(1, "Home"), (2, "Home"), (3, "Other")]).await;

    let fresh = check_once_all(&client(&server), org, "Home").await.unwrap();

    assert_eq!(fresh.iter().map(|i| i.id).collect::<Vec<_>>(), vec![2]);
    assert_eq!(fresh[0].project, "Home");
    let mut stored = cache::get::<Vec<String>>(&keys::assigned_seen_all(org)).expect("a baseline was stored");
    stored.sort();
    assert_eq!(stored, vec!["1", "2", "3"]);
    assert_eq!(
        cache::get::<Vec<String>>(&keys::assigned_seen(org, "Home")),
        Some(vec!["1".to_string()]),
        "the per-project baseline is left exactly as it was"
    );
}

#[tokio::test]
async fn later_all_projects_checks_announce_new_items_from_any_project() {
    let org = "notify-a4";
    forget(org, "Home");
    cache::put(&keys::assigned_seen_all(org), &vec!["1".to_string(), "3".to_string()]);
    let server = MockServer::start().await;
    mount_assigned(&server, org, &[(1, "Home"), (3, "Other"), (4, "Other"), (5, "Third")]).await;

    let fresh = check_once_all(&client(&server), org, "Home").await.unwrap();

    assert_eq!(
        fresh.iter().map(|i| (i.id, i.project.as_str())).collect::<Vec<_>>(),
        vec![(4, "Other"), (5, "Third")]
    );
    assert_eq!(
        cache::get::<Vec<String>>(&keys::assigned_seen_all(org)),
        Some(vec!["1".to_string(), "3".to_string(), "4".to_string(), "5".to_string()])
    );
    assert_eq!(
        cache::get::<Vec<String>>(&keys::assigned_seen(org, "Home")),
        None,
        "a later combined check never writes the per-project baseline"
    );
}
