use v2_lib::ado::{AdoClient, AdoError};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_projects_parses_and_authenticates() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/myorg/_apis/projects"))
        .and(header("Authorization", "Bearer tok123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 1,
            "value": [{"id": "abc", "name": "Proj One"}]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("tok123".into(), server.uri());
    let projects = client.get_projects("myorg").await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "Proj One");
}

#[tokio::test]
async fn maps_401_to_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("bad".into(), server.uri());
    let err = client.get_projects("o").await.unwrap_err();
    assert!(matches!(err, AdoError::Unauthorized));
}

#[tokio::test]
async fn maps_429_with_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "17"))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let err = client.get_projects("o").await.unwrap_err();
    assert!(matches!(err, AdoError::RateLimited { retry_after_secs: 17 }));
}

/// The tool must never destroy data: no DELETE requests, ever.
#[test]
fn client_source_has_no_delete_calls() {
    let src = include_str!("../src/ado.rs");
    assert!(
        !src.contains(".delete(") && !src.contains("Method::DELETE"),
        "AdoClient must never issue DELETE requests"
    );
}
