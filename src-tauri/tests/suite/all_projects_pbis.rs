use std::sync::Mutex;
use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::BridgeContext;
use v2_lib::autorun::recipe::{project_slug, projects_with_files};
use v2_lib::commands::ai_bridge::push_context_with;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mount_wiql(server: &MockServer, path_str: &str, ids: &[i64]) {
    let items: Vec<_> = ids.iter().map(|i| serde_json::json!({ "id": i })).collect();
    Mock::given(method("POST"))
        .and(path(path_str))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": items
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn all_projects_pbi_search_posts_org_scoped_wiql_without_a_project_clause() {
    let server = MockServer::start().await;
    mount_wiql(&server, "/myorg/_apis/wit/wiql", &[]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let hits = client.search_pbis_all("myorg", "login", 20).await.unwrap();
    assert!(hits.is_empty());

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert_eq!(req.url.path(), "/myorg/_apis/wit/wiql");
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    let query = body["query"].as_str().unwrap();
    assert!(!query.contains("@project"), "no project macro: {query}");
    assert!(!query.contains("TeamProject"), "no project clause: {query}");
    assert!(
        query.contains("[System.WorkItemType] = 'Product Backlog Item'"),
        "{query}"
    );
    assert_eq!(
        query,
        "SELECT [System.Id] FROM workitems WHERE [System.Title] CONTAINS 'login' \
         AND [System.WorkItemType] = 'Product Backlog Item' \
         ORDER BY [System.ChangedDate] DESC"
    );
    for r in &requests {
        let segs: Vec<_> = r.url.path_segments().unwrap().collect();
        assert_eq!(
            segs.get(1).copied(),
            Some("_apis"),
            "no segment between the organization and _apis: {}",
            r.url.path()
        );
    }
}

#[tokio::test]
async fn all_projects_pbi_hits_carry_their_project() {
    let server = MockServer::start().await;
    mount_wiql(&server, "/myorg/_apis/wit/wiql", &[42, 7]).await;
    Mock::given(method("GET"))
        .and(path("/myorg/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 7, "fields": {"System.Title": "Seven", "System.WorkItemType": "Product Backlog Item", "System.TeamProject": "Payroll"}},
                {"id": 42, "fields": {"System.Title": "The Answer", "System.WorkItemType": "Product Backlog Item", "System.TeamProject": "HRM"}}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let hits = client.search_pbis_all("myorg", "42", 20).await.unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, 42, "WIQL order is kept");
    assert_eq!(hits[0].project, "HRM");
    assert_eq!(hits[0].title, "The Answer");
    assert_eq!(hits[1].id, 7);
    assert_eq!(hits[1].project, "Payroll");

    let requests = server.received_requests().await.unwrap();
    let get = requests.iter().find(|r| r.method.as_str() == "GET").unwrap();
    let q = get.url.query().unwrap_or_default();
    assert!(q.contains("System.TeamProject"), "{q}");
}

#[tokio::test]
async fn all_projects_pbi_search_escapes_single_quotes() {
    let server = MockServer::start().await;
    mount_wiql(&server, "/o/_apis/wit/wiql", &[]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    client.search_pbis_all("o", "it's", 20).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(
        body["query"]
            .as_str()
            .unwrap()
            .contains("[System.Title] CONTAINS 'it''s'"),
        "{body}"
    );
}

#[tokio::test]
async fn per_project_pbi_hits_carry_the_argument_project() {
    let server = MockServer::start().await;
    mount_wiql(&server, "/o/p/_apis/wit/wiql", &[5]).await;
    Mock::given(method("GET"))
        .and(path("/o/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 5, "fields": {"System.Title": "Five", "System.WorkItemType": "Product Backlog Item"}}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let hits = client.search_pbis("o", "p", "x", 20).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].project, "p");
}

// ------------------------------------------------ home project from Auto Run files

fn touch(root: &std::path::Path, name: &str) {
    let dir = root.join("projects");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), "{}").unwrap();
}

#[test]
fn autorun_files_name_the_projects_that_have_them() {
    let dir = tempfile::tempdir().unwrap();
    let slug = project_slug("acme", "HRM");
    touch(dir.path(), &format!("{slug}.nav.json"));
    touch(dir.path(), &format!("{slug}-map.json"));
    let found = projects_with_files(dir.path(), "acme", &["HRM".to_string(), "Payroll".to_string()]);
    assert_eq!(found, vec!["HRM".to_string()]);
}

#[test]
fn autorun_files_match_the_whole_slug_not_a_prefix_of_another() {
    let dir = tempfile::tempdir().unwrap();
    let slug = project_slug("acme", "HRM");
    for name in [".json", ".nav.json", "-map.json", "-components.json", "-quirks.json", "-mapping.json", "-divergences.json", ".pwexport.json"] {
        touch(dir.path(), &format!("{slug}{name}"));
    }
    assert!(projects_with_files(dir.path(), "acme", &["HR".to_string()]).is_empty());
    // Another organization's project of the same name is not this one.
    assert!(projects_with_files(dir.path(), "other", &["HRM".to_string()]).is_empty());
}

#[test]
fn no_projects_folder_means_no_projects() {
    let dir = tempfile::tempdir().unwrap();
    let found = projects_with_files(dir.path(), "acme", &["HRM".to_string()]);
    assert!(found.is_empty());
    assert!(!dir.path().join("projects").exists(), "listing creates nothing");
}

#[test]
fn bridge_context_takes_the_all_projects_switch() {
    let dir = tempfile::tempdir().unwrap();
    for env_id in ["some-env", ""] {
        let ctx = Mutex::new(BridgeContext::default());
        let next = BridgeContext { org: "acme".into(), all_projects: true, ..BridgeContext::default() };
        push_context_with(&ctx, Some(dir.path()), next, env_id);
        assert!(ctx.lock().unwrap().all_projects, "env {env_id:?}");
    }
    let ctx = Mutex::new(BridgeContext { all_projects: true, ..BridgeContext::default() });
    push_context_with(&ctx, None, BridgeContext::default(), "");
    assert!(!ctx.lock().unwrap().all_projects, "a push with the switch off turns it off");
}
