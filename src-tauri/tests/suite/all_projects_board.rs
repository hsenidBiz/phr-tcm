//! Task 9: the bounded project fan-out, and the areas of every project.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use v2_lib::ado::fanout::{
    capped, gather, per_project, ProjectCoverage, FANOUT_CONCURRENCY, MAX_FANOUT_PROJECTS,
};
use v2_lib::ado::{AdoClient, AdoError};
use wiremock::matchers::{method, path, path_regex};
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
