//! `AdoSource` against a mock Items API. The trait is synchronous and
//! wiremock is async, so every call goes through `spawn_blocking`.

use std::sync::mpsc;
use velopack::bundle::Manifest;
use velopack::sources::UpdateSource;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
use v2_lib::updater::ado::AdoSource;

const FEED: &str = r#"{"Assets":[{"PackageId":"AzureDevOpsTestCaseManager.V2","Version":"1.23.0","Type":"Full","FileName":"AzureDevOpsTestCaseManager.V2-1.23.0-full.nupkg","SHA1":"A","SHA256":"B","Size":5}]}"#;

/// The branch tip, as `GET refs?filter=heads/main` answers.
fn refs_answer(commit: &str) -> Mock {
    Mock::given(method("GET"))
        .and(path("/refs"))
        .and(query_param("filter", "heads/main"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "name": "refs/heads/main", "objectId": commit }],
            "count": 1
        })))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_feed_is_read_from_the_commit_main_pointed_at() {
    let server = MockServer::start().await;
    refs_answer("abc123").expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("path", "/releases.win.json"))
        .and(query_param("download", "true"))
        .and(query_param("versionDescriptor.versionType", "commit"))
        .and(query_param("versionDescriptor.version", "abc123"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FEED))
        .expect(1)
        .mount(&server)
        .await;

    let (src, denied) = AdoSource::at(&server.uri(), "main", "tok".into());
    let feed = tokio::task::spawn_blocking(move || src.get_release_feed("win", &Manifest::default(), ""))
        .await
        .unwrap()
        .expect("feed");
    assert_eq!(feed.Assets.len(), 1);
    assert_eq!(feed.Assets[0].Version, "1.23.0");
    assert!(!denied.get());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_403_is_reported_as_no_access() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/refs"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let (src, denied) = AdoSource::at(&server.uri(), "main", "tok".into());
    let r = tokio::task::spawn_blocking(move || src.get_release_feed("win", &Manifest::default(), ""))
        .await
        .unwrap();
    assert!(r.is_err());
    assert!(denied.get(), "403 must raise the no-access flag");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_401_is_also_no_access_but_a_500_is_not() {
    for (status, expect_denied) in [(401u16, true), (500u16, false)] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/refs"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        let (src, denied) = AdoSource::at(&server.uri(), "main", "tok".into());
        let r = tokio::task::spawn_blocking(move || src.get_release_feed("win", &Manifest::default(), ""))
            .await
            .unwrap();
        assert!(r.is_err(), "{status} must be an error");
        assert_eq!(denied.get(), expect_denied, "status {status}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_branch_with_no_tip_is_an_error_not_a_panic() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/refs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [], "count": 0 })))
        .mount(&server)
        .await;
    let (src, denied) = AdoSource::at(&server.uri(), "main", "tok".into());
    let r = tokio::task::spawn_blocking(move || src.get_release_feed("win", &Manifest::default(), ""))
        .await
        .unwrap();
    assert!(r.is_err());
    assert!(!denied.get());
}

/// DevOps' `filter` query param is a prefix match: `filter=heads/main` also
/// matches a ref named `refs/heads/main-hotfix`. The mock lists that
/// prefix-matching sibling FIRST - so an implementation that just takes
/// `.next()` off the response would pin to the sibling's commit - and the
/// real `refs/heads/main` second. The feed must still come from `main`'s
/// commit, not the sibling's.
#[tokio::test(flavor = "multi_thread")]
async fn a_prefix_matching_sibling_branch_listed_first_is_not_mistaken_for_the_real_branch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/refs"))
        .and(query_param("filter", "heads/main"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "name": "refs/heads/main-hotfix", "objectId": "wrong" },
                { "name": "refs/heads/main", "objectId": "abc123" }
            ],
            "count": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("path", "/releases.win.json"))
        .and(query_param("versionDescriptor.version", "abc123"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FEED))
        .expect(1)
        .mount(&server)
        .await;

    let (src, denied) = AdoSource::at(&server.uri(), "main", "tok".into());
    let feed = tokio::task::spawn_blocking(move || src.get_release_feed("win", &Manifest::default(), ""))
        .await
        .unwrap()
        .expect("feed");
    assert_eq!(feed.Assets.len(), 1);
    assert_eq!(feed.Assets[0].Version, "1.23.0");
    assert!(!denied.get());
}

/// Until Task 2 lands, downloading is unsupported - and says so rather
/// than pretending.
#[test]
fn download_is_not_yet_supported() {
    let (src, _) = AdoSource::at("http://127.0.0.1:1", "main", "tok".into());
    let asset = velopack::VelopackAsset { FileName: "x.nupkg".into(), ..Default::default() };
    let (tx, _rx) = mpsc::channel::<i16>();
    let r = src.download_release_entry(&asset, std::path::Path::new("nope"), Some(tx));
    assert!(r.is_err());
}
