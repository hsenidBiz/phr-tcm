//! Reads are retried after a failure that is likely to pass; writes are
//! never sent twice. And the local pacer spaces writes only (backlog A1.2
//! and A1.4).

use v2_lib::ado::{throttle, AdoClient, AdoError};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn projects_body() -> serde_json::Value {
    serde_json::json!({ "count": 1, "value": [{ "id": "p1", "name": "Web" }] })
}

/// Clears any hold on entry and exit, under the suite's pacer lock, and
/// puts the level back to Full speed when done.
struct Gate {
    _pacer: std::sync::MutexGuard<'static, ()>,
}
impl Drop for Gate {
    fn drop(&mut self) {
        throttle::clear_backoff();
        throttle::set_level("full");
    }
}
fn gate() -> Gate {
    let pacer = crate::serial::pacer();
    throttle::clear_backoff();
    throttle::set_level("full");
    Gate { _pacer: pacer }
}

#[tokio::test]
async fn a_get_that_fails_with_503_is_retried_and_succeeds() {
    let _g = gate();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(projects_body()))
        .expect(1)
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let projects = client.get_projects("o").await.expect("the retry succeeds");
    assert_eq!(projects.len(), 1);
}

#[tokio::test]
async fn a_get_gives_up_after_three_tries() {
    let _g = gate();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(502))
        .expect(3)
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let err = client.get_projects("o").await.unwrap_err();
    assert!(matches!(err, AdoError::Http { status: 502, .. }), "{err:?}");
}

#[tokio::test]
async fn a_400_is_not_retried() {
    let _g = gate();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    assert!(client.get_projects("o").await.is_err());
}

#[tokio::test]
async fn a_429_asking_for_a_long_wait_is_not_retried() {
    let _g = gate();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "25"))
        .expect(1)
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let err = client.get_projects("o").await.unwrap_err();
    assert!(matches!(err, AdoError::RateLimited { retry_after_secs: 25 }), "{err:?}");
}

#[tokio::test]
async fn a_write_is_never_sent_twice() {
    let _g = gate();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/p/_apis/wit/workitems/$Test%20Case"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("t".into(), server.uri(), server.uri());
    let tc = v2_lib::model::TestCase {
        title: "Case".into(),
        steps: vec![],
        tags: String::new(),
        automation_status: "Not Automated".into(),
        module_value: String::new(),
        preconditions: String::new(),
        update_id: None,
        priority: None,
        ..Default::default()
    };
    assert!(client.create_test_case("o", "p", &tc, None, "", "", None).await.is_err());
}

#[tokio::test]
async fn a_read_only_post_is_retried_like_a_get() {
    let _g = gate();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/wit/workitemsbatch"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/_apis/wit/workitemsbatch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 7, "fields": { "System.State": "Design" } }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let meta = client.get_case_meta("o", &[7]).await.expect("retried");
    assert_eq!(meta[0].state, "Design");
}

#[tokio::test]
async fn the_pacer_does_not_space_reads() {
    let _g = gate();
    throttle::set_level("gentle"); // 800 ms between writes
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(projects_body()))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let started = std::time::Instant::now();
    for _ in 0..3 {
        client.get_projects("o").await.unwrap();
    }
    assert!(
        started.elapsed() < std::time::Duration::from_millis(800),
        "three reads at Gentle took {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn the_pacer_still_spaces_writes() {
    let _g = gate();
    throttle::set_level("balanced"); // 200 ms between writes
    let started = std::time::Instant::now();
    for _ in 0..3 {
        throttle::pace(true).await;
    }
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(390),
        "three writes at Balanced took {:?}",
        started.elapsed()
    );
}
