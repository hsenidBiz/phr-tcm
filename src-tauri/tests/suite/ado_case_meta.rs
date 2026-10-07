//! `get_case_meta`: state, area path and iteration path for a set of test
//! cases, read with the work-item batch route (POST, 200 ids per call).

use v2_lib::ado::AdoClient;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> AdoClient {
    AdoClient::with_base_url("tok".into(), server.uri())
}

#[tokio::test]
async fn case_meta_reads_state_area_and_iteration() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/_apis/wit/workitemsbatch"))
        .and(body_json(serde_json::json!({
            "ids": [11, 12],
            "fields": ["System.Id", "System.State", "System.AreaPath", "System.IterationPath"],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 2,
            "value": [
                {"id": 11, "fields": {
                    "System.Id": 11, "System.State": "Ready",
                    "System.AreaPath": r"HRM\Perf", "System.IterationPath": r"HRM\Sprint 4"}},
                {"id": 12, "fields": {"System.Id": 12, "System.State": "Design"}},
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let meta = client(&server).get_case_meta("org", &[11, 12]).await.unwrap();
    assert_eq!(meta.len(), 2);
    assert_eq!(meta[0].id, 11);
    assert_eq!(meta[0].state, "Ready");
    assert_eq!(meta[0].area_path, r"HRM\Perf");
    assert_eq!(meta[0].iteration_path, r"HRM\Sprint 4");
    // Missing fields are empty strings, not errors; order is preserved.
    assert_eq!(meta[1].id, 12);
    assert_eq!(meta[1].state, "Design");
    assert_eq!(meta[1].area_path, "");
    assert_eq!(meta[1].iteration_path, "");
}

#[tokio::test]
async fn case_meta_makes_one_post_per_200_ids() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/_apis/wit/workitemsbatch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
        .expect(2)
        .mount(&server)
        .await;
    let ids: Vec<i32> = (1..=201).collect();
    let meta = client(&server).get_case_meta("org", &ids).await.unwrap();
    assert!(meta.is_empty());
}

#[tokio::test]
async fn case_meta_with_no_ids_makes_no_request() {
    let server = MockServer::start().await;
    let meta = client(&server).get_case_meta("org", &[]).await.unwrap();
    assert!(meta.is_empty());
}
