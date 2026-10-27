//! Task 9: the bounded project fan-out, and the areas of every project.
//! Task 10: the organization-wide board fetch.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use v2_lib::ado::fanout::{
    capped, gather, per_project, ProjectCoverage, FANOUT_CONCURRENCY, MAX_FANOUT_PROJECTS,
};
use v2_lib::ado::{AdoClient, AdoError};
use v2_lib::work_board::BOARD_FIELDS_ALL;
use wiremock::matchers::{method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn project_list(names: &[String]) -> serde_json::Value {
    let value: Vec<_> = names
        .iter()
        .map(|n| serde_json::json!({ "id": format!("id-{n}"), "name": n }))
        .collect();
    serde_json::json!({ "value": value })
}

/// An area tree: the root named for the project, one child per name.
fn area_tree(project: &str, children: &[&str]) -> serde_json::Value {
    let kids: Vec<_> = children.iter().map(|c| serde_json::json!({ "name": c })).collect();
    serde_json::json!({ "name": project, "children": kids })
}

async fn mount_projects(server: &MockServer, org: &str, names: &[String]) {
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/projects")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_list(names)))
        .mount(server)
        .await;
}

fn areas_path(org: &str, project: &str) -> String {
    format!("/{org}/{project}/_apis/wit/classificationnodes/areas")
}

async fn mount_areas(server: &MockServer, org: &str, project: &str, children: &[&str]) {
    Mock::given(method("GET"))
        .and(path(areas_path(org, project)))
        .respond_with(ResponseTemplate::new(200).set_body_json(area_tree(project, children)))
        .mount(server)
        .await;
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ------------------------------------------------------------ the cap

#[tokio::test]
async fn the_fan_out_stops_at_the_project_cap_and_says_how_many_it_left_out() {
    let server = MockServer::start().await;
    let org = "cap-org";
    let all: Vec<String> = (0..101).map(|i| format!("P{i:03}")).collect();
    mount_projects(&server, org, &all).await;
    Mock::given(method("GET"))
        .and(path_regex(format!(
            r"^/{org}/[^/]+/_apis/wit/classificationnodes/areas$"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(area_tree("any", &["Team"])))
        .expect(MAX_FANOUT_PROJECTS as u64)
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let areas = client.classification_paths_all(org).await.unwrap();
    assert_eq!(areas.coverage, ProjectCoverage { shown: 100, total: 101 });
    server.verify().await;
}

#[test]
fn the_cap_keeps_the_first_projects_by_name_and_counts_them_all() {
    let mut many: Vec<String> = (0..120).rev().map(|i| format!("p{i:03}")).collect();
    many.push("Alpha".into());
    let (kept, total) = capped(many);
    assert_eq!(total, 121);
    assert_eq!(kept.len(), MAX_FANOUT_PROJECTS);
    assert_eq!(kept[0], "Alpha");
    assert_eq!(kept[1], "p000");
    assert_eq!(kept[99], "p098");

    let (few, total) = capped(names(&["b", "A", "c"]));
    assert_eq!(total, 3);
    assert_eq!(few, names(&["A", "b", "c"]));
}

// ------------------------------------------------------------ the bound

/// Answers every area read after a pause, and writes down when each one
/// arrived, so the test can tell how many were in flight together.
struct Arrivals {
    at: Arc<Mutex<Vec<Instant>>>,
    pause: Duration,
}

impl Respond for Arrivals {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        self.at.lock().unwrap().push(Instant::now());
        let project = req.url.path_segments().unwrap().nth(1).unwrap().to_string();
        ResponseTemplate::new(200)
            .set_body_json(area_tree(&project, &["Team"]))
            .set_delay(self.pause)
    }
}

#[tokio::test]
async fn the_fan_out_runs_a_few_projects_at_a_time() {
    // Timed: a Retry-After hold banked by another module's test would be
    // slept inside this window (crate::serial::pacer).
    let _pacer = crate::serial::pacer();
    v2_lib::ado::throttle::clear_backoff();
    v2_lib::ado::throttle::set_level("full");

    let server = MockServer::start().await;
    let org = "bound-org";
    let all: Vec<String> = (0..12).map(|i| format!("P{i:02}")).collect();
    mount_projects(&server, org, &all).await;
    let at = Arc::new(Mutex::new(vec![]));
    let pause = Duration::from_millis(250);
    Mock::given(method("GET"))
        .and(path_regex(format!(
            r"^/{org}/[^/]+/_apis/wit/classificationnodes/areas$"
        )))
        .respond_with(Arrivals { at: at.clone(), pause })
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let areas = client.classification_paths_all(org).await.unwrap();
    assert_eq!(areas.coverage, ProjectCoverage { shown: 12, total: 12 });

    // A request is in flight for `pause` after it arrives, so the arrivals
    // inside any one `pause`-long window are all in flight together.
    let arrivals = at.lock().unwrap().clone();
    assert_eq!(arrivals.len(), 12);
    let most = arrivals
        .iter()
        .map(|t| arrivals.iter().filter(|u| *u >= t && **u - *t < pause).count())
        .max()
        .unwrap();
    assert!(
        most <= FANOUT_CONCURRENCY,
        "{most} area reads were in flight together"
    );
    assert!(most >= 2, "the reads ran one after another ({most} together)");
}

// ------------------------------------------------------------ errors

#[tokio::test]
async fn a_project_the_user_cannot_read_is_skipped_and_counted_out() {
    let _log = crate::serial::log_tail();
    let server = MockServer::start().await;
    let org = "forbidden-org";
    mount_projects(&server, org, &names(&["Alpha", "Beta", "Gamma"])).await;
    Mock::given(method("GET"))
        .and(path(areas_path(org, "Beta")))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    mount_areas(&server, org, "Alpha", &["Core"]).await;
    mount_areas(&server, org, "Gamma", &["Web"]).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let areas = client.classification_paths_all(org).await.unwrap();
    assert_eq!(areas.paths, names(&["Alpha", "Alpha\\Core", "Gamma", "Gamma\\Web"]));
    assert_eq!(areas.coverage, ProjectCoverage { shown: 2, total: 3 });
    assert!(areas.coverage.shown == areas.coverage.total - 1);

    let skipped: Vec<_> = v2_lib::applog::recent(500)
        .into_iter()
        .filter(|l| l.level == "warn" && l.message.starts_with("all projects: skipped Beta: "))
        .collect();
    assert_eq!(skipped.len(), 1, "the skipped project is logged once");
}

#[tokio::test]
async fn an_expired_sign_in_fails_the_whole_fan_out() {
    let server = MockServer::start().await;
    let org = "expired-org";
    mount_projects(&server, org, &names(&["Alpha", "Beta", "Gamma"])).await;
    Mock::given(method("GET"))
        .and(path(areas_path(org, "Beta")))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    mount_areas(&server, org, "Alpha", &["Core"]).await;
    mount_areas(&server, org, "Gamma", &["Web"]).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let err = client.classification_paths_all(org).await.unwrap_err();
    assert!(matches!(err, AdoError::Unauthorized), "{err:?}");
}

#[test]
fn gather_skips_what_the_user_may_not_read_and_fails_on_anything_else() {
    let skipped = gather(
        vec![
            ("A".to_string(), Ok(1)),
            ("B".to_string(), Err(AdoError::Forbidden)),
            ("C".to_string(), Err(AdoError::NotFound)),
            ("D".to_string(), Err(AdoError::RepositoryUnavailable)),
            ("E".to_string(), Ok(5)),
        ],
        5,
    )
    .unwrap();
    assert_eq!(skipped.0, vec![("A".to_string(), 1), ("E".to_string(), 5)]);
    assert_eq!(skipped.1, ProjectCoverage { shown: 2, total: 5 });

    for e in [
        AdoError::Unauthorized,
        AdoError::RateLimited { retry_after_secs: 3 },
        AdoError::Http { status: 500, body: String::new() },
        AdoError::Network("x".into()),
    ] {
        let failed = gather(vec![("A".to_string(), Ok(1)), ("B".to_string(), Err(e))], 2);
        assert!(failed.is_err());
    }
}

#[tokio::test]
async fn per_project_keeps_the_order_it_was_given() {
    let projects = names(&["slow", "fast", "mid"]);
    let out = per_project(&projects, |p| async move {
        let wait = match p.as_str() {
            "slow" => 60,
            "mid" => 30,
            _ => 0,
        };
        tokio::time::sleep(Duration::from_millis(wait)).await;
        Ok::<_, AdoError>(p.len())
    })
    .await;
    let order: Vec<_> = out.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(order, ["slow", "fast", "mid"]);
}

// ------------------------------------------------------------ the areas

#[tokio::test]
async fn areas_from_projects_with_only_their_root_are_left_out() {
    let server = MockServer::start().await;
    let org = "root-only-org";
    mount_projects(&server, org, &names(&["repos-only", "Work"])).await;
    mount_areas(&server, org, "repos-only", &[]).await;
    mount_areas(&server, org, "Work", &["Team A"]).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let areas = client.classification_paths_all(org).await.unwrap();
    assert_eq!(areas.paths, names(&["Work", "Work\\Team A"]));
    // The root-only project was read, so it is not a skipped one.
    assert_eq!(areas.coverage, ProjectCoverage { shown: 2, total: 2 });
}

#[tokio::test]
async fn the_areas_of_every_project_come_back_sorted_without_regard_to_case() {
    let server = MockServer::start().await;
    let org = "sorted-org";
    mount_projects(&server, org, &names(&["beta", "Alpha"])).await;
    mount_areas(&server, org, "Alpha", &["zeta", "Eta"]).await;
    mount_areas(&server, org, "beta", &["One"]).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let areas = client.classification_paths_all(org).await.unwrap();
    assert_eq!(
        areas.paths,
        names(&["Alpha", "Alpha\\Eta", "Alpha\\zeta", "beta", "beta\\One"])
    );
}

#[tokio::test]
async fn the_project_list_is_read_once_per_session() {
    let server = MockServer::start().await;
    let org = "memo-org-task9";
    Mock::given(method("GET"))
        .and(path(format!("/{org}/_apis/projects")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_list(&names(&["Alpha"]))))
        .expect(1)
        .mount(&server)
        .await;
    mount_areas(&server, org, "Alpha", &["Core"]).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let first = client.classification_paths_all(org).await.unwrap();
    let second = client.classification_paths_all(org).await.unwrap();
    assert_eq!(first.paths, second.paths);
    server.verify().await;
}

#[tokio::test]
async fn the_per_project_area_list_still_swallows_a_failure() {
    let server = MockServer::start().await;
    let org = "swallow-org";
    Mock::given(method("GET"))
        .and(path(areas_path(org, "Beta")))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let paths = client.get_classification_paths(org, "Beta", "areas").await.unwrap();
    assert!(paths.is_empty());
}

// ------------------------------------------------------------ the board

fn board_item(id: i32, title: &str, wtype: &str, state: &str, project: &str) -> serde_json::Value {
    serde_json::json!({ "id": id, "fields": {
        "System.Title": title, "System.WorkItemType": wtype, "System.State": state,
        "System.TeamProject": project,
        "System.ChangedDate": "2026-10-01T00:00:00Z"
    }})
}

fn with_parent(mut item: serde_json::Value, parent: i32) -> serde_json::Value {
    item["fields"]["System.Parent"] = serde_json::json!(parent);
    item
}

async fn mount_org_wiql(server: &MockServer, ids: &[i32]) {
    let hits: Vec<_> = ids.iter().map(|i| serde_json::json!({ "id": i })).collect();
    Mock::given(method("POST"))
        .and(path("/org/_apis/wit/wiql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "workItems": hits })),
        )
        .mount(server)
        .await;
}

/// The items read, answered for whichever `ids` the request names.
async fn mount_org_items(server: &MockServer, ids_csv: &str, items: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems"))
        .and(query_param("ids", ids_csv))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": items })),
        )
        .mount(server)
        .await;
}

fn states_body(states: &[(&str, &str)]) -> serde_json::Value {
    let value: Vec<_> = states
        .iter()
        .map(|(n, c)| serde_json::json!({ "name": n, "color": "aabbcc", "category": c }))
        .collect();
    serde_json::json!({ "value": value })
}

async fn mount_states(
    server: &MockServer,
    project: &str,
    wtype: &str,
    states: &[(&str, &str)],
    expect: u64,
) {
    Mock::given(method("GET"))
        .and(path(format!("/org/{project}/_apis/wit/workitemtypes/{wtype}/states")))
        .respond_with(ResponseTemplate::new(200).set_body_json(states_body(states)))
        .expect(expect)
        .mount(server)
        .await;
}

async fn wiql_queries(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "POST" && r.url.path().ends_with("/_apis/wit/wiql"))
        .map(|r| {
            let body: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
            body["query"].as_str().unwrap().to_string()
        })
        .collect()
}

#[tokio::test]
async fn the_all_projects_board_asks_once_at_organization_scope_with_no_project_clause() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let board = client.fetch_board_all("org", None, None, false).await.unwrap();
    assert!(board.items.is_empty());

    let asked = server.received_requests().await.unwrap();
    assert_eq!(asked.len(), 1, "one WIQL post, nothing else");
    assert_eq!(asked[0].url.path(), "/org/_apis/wit/wiql");
    let queries = wiql_queries(&server).await;
    assert!(!queries[0].contains("@project"), "{}", queries[0]);
    assert!(!queries[0].contains("TeamProject"), "{}", queries[0]);
    assert!(queries[0].contains("[System.AssignedTo] = @Me"), "{}", queries[0]);
    assert!(queries[0].contains("NOT IN ('Test Case'"), "{}", queries[0]);
}

#[tokio::test]
async fn board_items_carry_the_project_azure_devops_names() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[1, 2]).await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems"))
        .and(query_param("fields", BOARD_FIELDS_ALL))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            board_item(2, "Pay run", "Task", "Active", "Payroll"),
            board_item(1, "Leave form", "Task", "Active", "HRM"),
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/org/[^/]+/_apis/wit/workitemtypes/Task/states$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(states_body(&[(
            "Active",
            "InProgress",
        )])))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let board = client.fetch_board_all("org", None, None, false).await.unwrap();
    let got: Vec<_> = board.items.iter().map(|i| (i.id, i.project.as_str())).collect();
    // WIQL order is kept, and each card names its own project.
    assert_eq!(got, [(1, "HRM"), (2, "Payroll")]);
    assert!(BOARD_FIELDS_ALL.ends_with(",System.TeamProject"));
    assert!(BOARD_FIELDS_ALL.contains("System.Parent"));
    assert!(board.states_by_type.is_empty(), "a type's states differ between processes");
}

#[tokio::test]
async fn states_are_read_once_per_project_and_type() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[1, 2, 3, 4]).await;
    mount_org_items(
        &server,
        "1,2,3,4",
        vec![
            board_item(1, "a", "Task", "Active", "A"),
            board_item(2, "b", "Task", "Active", "A"),
            board_item(3, "c", "Task", "Active", "B"),
            board_item(4, "d", "Bug", "Active", "A"),
        ],
    )
    .await;
    mount_states(&server, "A", "Task", &[("Active", "InProgress")], 1).await;
    // Project B's process calls the same state a not-started one.
    mount_states(&server, "B", "Task", &[("Active", "Proposed")], 1).await;
    mount_states(&server, "A", "Bug", &[("Active", "Completed")], 1).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let board = client.fetch_board_all("org", None, None, false).await.unwrap();
    let column = |id: i32| board.items.iter().find(|i| i.id == id).unwrap().column.clone();
    assert_eq!(column(1).as_deref(), Some("In Progress"));
    assert_eq!(column(2).as_deref(), Some("In Progress"));
    assert_eq!(column(3).as_deref(), Some("To Do"));
    assert_eq!(column(4).as_deref(), Some("Done"));
    assert_eq!(board.states_by_project["A"]["Task"].len(), 1);
    assert_eq!(board.states_by_project["B"]["Task"][0].category, "Proposed");
    assert!(board.states_by_project["A"].contains_key("Bug"));
    assert!(!board.states_by_project["B"].contains_key("Bug"));
    server.verify().await;
}

#[tokio::test]
async fn a_pbi_board_finds_children_in_any_project() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    client.fetch_board_all("org", None, Some(9), false).await.unwrap();
    let queries = wiql_queries(&server).await;
    assert_eq!(queries.len(), 1);
    assert!(queries[0].contains("[System.Parent] = 9"), "{}", queries[0]);
    assert!(queries[0].contains("[System.Id] = 9"), "{}", queries[0]);
    assert!(!queries[0].contains("@project") && !queries[0].contains("TeamProject"));
    assert!(!queries[0].contains("@Me"), "a PBI board is not 'my work'");
}

#[tokio::test]
async fn a_pbi_wins_over_an_area_and_neither_takes_a_sprint_clause() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    client
        .fetch_board_all("org", Some("Payroll\\Team X"), Some(9), true)
        .await
        .unwrap();
    client.fetch_board_all("org", None, None, true).await.unwrap();
    let queries = wiql_queries(&server).await;
    assert!(queries[0].contains("[System.Parent] = 9") && !queries[0].contains("AreaPath"));
    assert!(!queries[0].contains("Iteration") && !queries[1].contains("Iteration"));
    // No sprint lookup was made for either: only the two WIQL posts.
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn an_area_board_asks_under_the_area_with_the_name_quoted() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    client
        .fetch_board_all("org", Some("Payroll\\Gamma's Team"), None, false)
        .await
        .unwrap();
    let queries = wiql_queries(&server).await;
    assert!(
        queries[0].contains("[System.AreaPath] UNDER 'Payroll\\Gamma''s Team'"),
        "{}",
        queries[0]
    );
    assert!(!queries[0].contains("@project") && !queries[0].contains("@Me"));
}

#[tokio::test]
async fn an_area_boards_sprint_comes_from_the_areas_own_project_and_team() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/Payroll/Team%20X/_apis/work/teamsettings/iterations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            { "name": "SP2", "path": "Payroll\\SP2",
              "attributes": { "timeFrame": "current", "finishDate": "2026-10-30T00:00:00Z" } }
        ]})))
        .expect(1)
        .mount(&server)
        .await;
    mount_org_wiql(&server, &[]).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    client
        .fetch_board_all("org", Some("Payroll\\Team X"), None, true)
        .await
        .unwrap();
    let queries = wiql_queries(&server).await;
    assert_eq!(queries.len(), 1);
    assert!(queries[0].contains("IterationPath] UNDER"), "{}", queries[0]);
    assert!(queries[0].contains("Payroll\\SP2"), "{}", queries[0]);
    assert!(!queries[0].contains("@CurrentIteration"));
    server.verify().await;
}

#[tokio::test]
async fn an_area_that_is_not_a_team_falls_back_to_the_projects_default_team_sprint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/Payroll/Team%20X/_apis/work/teamsettings/iterations"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/projects/Payroll"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "defaultTeam": { "name": "Payroll Team" }
        })))
        .mount(&server)
        .await;
    // The macro only means something in a team's context, so this is the
    // team-scoped route of the area's own project.
    Mock::given(method("POST"))
        .and(path("/org/Payroll/Payroll%20Team/_apis/wit/wiql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "workItems": [] })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    client
        .fetch_board_all("org", Some("Payroll\\Team X"), None, true)
        .await
        .unwrap();
    let queries = wiql_queries(&server).await;
    assert!(queries[0].contains("[System.IterationPath] = @CurrentIteration"));
    server.verify().await;
}

#[tokio::test]
async fn a_parent_in_another_project_still_names_its_lane_and_its_project() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[20, 21]).await;
    mount_org_items(
        &server,
        "20,21",
        vec![
            with_parent(board_item(20, "Child", "Task", "Active", "HRM"), 99),
            // The second card's parent is on the board, so it is not read.
            with_parent(board_item(21, "Sub", "Task", "Active", "HRM"), 20),
        ],
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems"))
        .and(query_param("ids", "99"))
        .and(query_param("errorPolicy", "omit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            { "id": 99, "fields": {
                "System.Title": "Payroll feature", "System.WorkItemType": "Feature",
                "System.TeamProject": "Payroll" } }
        ]})))
        .expect(1)
        .mount(&server)
        .await;
    mount_states(&server, "HRM", "Task", &[("Active", "InProgress")], 1).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let board = client.fetch_board_all("org", None, None, false).await.unwrap();
    let parent = board.items[0].parent.clone().unwrap();
    assert_eq!(
        (parent.id, parent.title.as_str(), parent.work_item_type.as_str()),
        (99, "Payroll feature", "Feature")
    );
    assert_eq!(board.parent_projects[&99], "Payroll");
    // A parent already on the board answers with its own project.
    assert_eq!(board.items[1].parent.clone().unwrap().title, "Child");
    assert_eq!(board.parent_projects[&20], "HRM");

    let reads: Vec<_> = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.query_pairs().any(|(k, v)| k == "ids" && v == "99"))
        .collect();
    assert_eq!(reads.len(), 1);
    let fields = reads[0].url.query_pairs().find(|(k, _)| k == "fields").unwrap().1.to_string();
    assert_eq!(fields, "System.Title,System.WorkItemType,System.TeamProject");
    server.verify().await;
}

#[tokio::test]
async fn a_parent_that_cannot_be_read_keeps_its_id_and_does_not_fail_the_board() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[20]).await;
    mount_org_items(
        &server,
        "20",
        vec![with_parent(board_item(20, "Child", "Task", "Active", "HRM"), 99)],
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems"))
        .and(query_param("ids", "99"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    mount_states(&server, "HRM", "Task", &[("Active", "InProgress")], 1).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let board = client.fetch_board_all("org", None, None, false).await.unwrap();
    let parent = board.items[0].parent.clone().unwrap();
    assert_eq!((parent.id, parent.title.as_str()), (99, ""));
    assert!(!board.parent_projects.contains_key(&99));
}

#[tokio::test]
async fn per_project_board_items_carry_the_argument_project() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": [{ "id": 5 }]
        })))
        .mount(&server)
        .await;
    // The per-project request is unchanged: no TeamProject asked for.
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems"))
        .and(query_param(
            "fields",
            "System.Id,System.Title,System.WorkItemType,System.State,System.AssignedTo,System.ChangedDate,System.Tags,Microsoft.VSTS.Common.Priority,System.Parent",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            { "id": 5, "fields": {
                "System.Title": "t", "System.WorkItemType": "Task", "System.State": "Active" } }
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitemtypes/Task/states"))
        .respond_with(ResponseTemplate::new(200).set_body_json(states_body(&[(
            "Active",
            "InProgress",
        )])))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let board = client.fetch_board("org", "proj", None, None, false).await.unwrap();
    assert_eq!(board.items[0].project, "proj");
    assert!(board.states_by_project.is_empty());
    assert!(board.parent_projects.is_empty());
    assert_eq!(board.states_by_type["Task"].len(), 1);
}

#[tokio::test]
async fn a_card_with_no_project_named_reads_no_states_and_keeps_the_heuristic_column() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[7]).await;
    mount_org_items(
        &server,
        "7",
        vec![serde_json::json!({ "id": 7, "fields": {
            "System.Title": "t", "System.WorkItemType": "Task", "System.State": "New" } })],
    )
    .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let board = client.fetch_board_all("org", None, None, false).await.unwrap();
    assert_eq!(board.items[0].project, "");
    assert_eq!(board.items[0].column.as_deref(), Some("To Do"));
    // The WIQL post and the item read; no states request for a blank project.
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_project_named_with_a_percent_sign_gets_its_states_read_through_an_encoded_url() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[5]).await;
    mount_org_items(
        &server,
        "5",
        vec![board_item(5, "half done", "Task", "Active", "50% Done")],
    )
    .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/states$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(states_body(&[(
            "Active",
            "InProgress",
        )])))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let board = client.fetch_board_all("org", None, None, false).await.unwrap();
    let asked = server.received_requests().await.unwrap();
    let states = asked
        .iter()
        .find(|r| r.url.path().ends_with("/states"))
        .expect("the project's states were asked");
    let raw = states.url.as_str();
    assert!(raw.contains("/org/50%25%20Done/"), "percent AND space must be encoded: {raw}");
    assert!(!raw.contains("50% Done"), "the raw name must not reach the wire: {raw}");
    // And the answer reached the card: its state read worked.
    assert_eq!(board.states_by_project["50% Done"]["Task"].len(), 1);
    assert_eq!(board.items[0].column.as_deref(), Some("In Progress"));
}

#[tokio::test]
async fn a_second_board_load_reads_no_states_again() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[1, 2]).await;
    mount_org_items(
        &server,
        "1,2",
        vec![
            board_item(1, "a", "Task", "Active", "A"),
            board_item(2, "b", "Bug", "Active", "B"),
        ],
    )
    .await;
    // One read per (project, type) for the whole session, not per load.
    mount_states(&server, "A", "Task", &[("Active", "InProgress")], 1).await;
    mount_states(&server, "B", "Bug", &[("Active", "Completed")], 1).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let first = client.fetch_board_all("org", None, None, false).await.unwrap();
    let second = client.fetch_board_all("org", None, None, false).await.unwrap();
    // The remembered answer paints the same cards the first load did.
    assert_eq!(first.items[0].column, second.items[0].column);
    assert_eq!(second.items[1].column.as_deref(), Some("Done"));
    assert_eq!(second.states_by_project["A"]["Task"].len(), 1);
    server.verify().await;
}

#[tokio::test]
async fn a_failed_state_read_is_not_remembered() {
    let server = MockServer::start().await;
    mount_org_wiql(&server, &[1]).await;
    mount_org_items(&server, "1", vec![board_item(1, "a", "Task", "Active", "A")]).await;
    // Refused once, then answered: the empty answer must not stick.
    Mock::given(method("GET"))
        .and(path("/org/A/_apis/wit/workitemtypes/Task/states"))
        .respond_with(ResponseTemplate::new(403))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_states(&server, "A", "Task", &[("Active", "InProgress")], 1).await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let first = client.fetch_board_all("org", None, None, false).await.unwrap();
    assert!(first.states_by_project["A"]["Task"].is_empty());
    let second = client.fetch_board_all("org", None, None, false).await.unwrap();
    assert_eq!(second.states_by_project["A"]["Task"].len(), 1);
}
