//! What a transport failure is allowed to say.
//!
//! The defect this pins: every network error reached the user as reqwest's
//! own `Display` - `error sending request for url (https://dev.azure.com/
//! PeoplesHR/HRM/_apis/wit/wiql?$top=500&api-version=7.1)`. An endpoint and
//! a query string, shown to the person who can do nothing with either,
//! while the log line written microseconds earlier already carried the same
//! text plus the timing. The URL belongs in the log; the toast owes the
//! user an action.

use v2_lib::ado::{AdoClient, AdoError, NET_GENERIC, NET_TIMEOUT, NET_UNREACHABLE};

/// Address nothing is listening on: bound to let the OS pick a free port,
/// then dropped. Connecting to it fails at connect, which is what a machine
/// with no route to Azure DevOps does.
fn dead_address() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("read the bound port");
    drop(listener);
    format!("http://{addr}")
}

#[tokio::test]
async fn an_unreachable_host_names_no_url() {
    let base = dead_address();
    let client = AdoClient::with_base_url("tok".into(), base.clone());
    let err = client.get_projects("myorg").await.expect_err("nothing is listening");

    let AdoError::Network(msg) = err else {
        panic!("a refused connection must be a Network error, got {err:?}");
    };
    assert_eq!(msg, NET_UNREACHABLE, "a connect failure is the 'unreachable' case");
    // The real regression guard: the endpoint must not have travelled with
    // the message. Checked against the actual address, not a fixed string,
    // so it cannot pass by the URL simply having changed shape.
    assert!(!msg.contains(&base), "the URL leaked into the user's message: {msg}");
    assert!(!msg.contains("127.0.0.1"), "the host leaked into the user's message: {msg}");
    assert!(!msg.contains("_apis"), "an API path leaked into the user's message: {msg}");
}

/// Every sentence, not just the one the test above happens to reach.
#[test]
fn no_message_can_carry_a_url() {
    for msg in [NET_TIMEOUT, NET_UNREACHABLE, NET_GENERIC] {
        assert!(!msg.contains("http"), "{msg}");
        assert!(!msg.contains("dev.azure.com"), "{msg}");
        assert!(!msg.contains("url ("), "{msg}");
        // Every one of them has to leave the user somewhere to go.
        assert!(msg.contains("Logs"), "no route to the detail: {msg}");
    }
}

/// The dev fault injector reproduces these three sentences so a developer
/// can see them without unplugging the network. A copy that drifts would
/// demonstrate a message the app no longer sends - worse than no simulator
/// at all - so the copy is checked against the original here.
#[test]
fn the_dev_fault_injector_still_quotes_them_exactly() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/dev/faults.ts");
    let src = std::fs::read_to_string(path).expect("read src/dev/faults.ts");
    for msg in [
        NET_TIMEOUT,
        NET_UNREACHABLE,
        NET_GENERIC,
        // The update check's own, from updater/mod.rs - simulated by the
        // same panel and just as able to drift.
        v2_lib::updater::FEED_UNREACHABLE,
    ] {
        assert!(
            src.contains(msg),
            "faults.ts no longer quotes this verbatim - update it or drop the simulation:\n  {msg}"
        );
    }
}

use std::time::Duration;

/// Every `.rs` under src/, as (path relative to src with `/`, text).
fn rust_sources() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                out.push((rel, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    out
}

/// A stalled connection (sleep/resume, a VPN flap) used to wait forever:
/// no client had a timeout, so a hung `$batch` kept the single-submit claim
/// and every later upload said "A submit is already running".
#[test]
fn every_request_has_a_deadline() {
    use v2_lib::ado::{BATCH_TIMEOUT, HTTP_CONNECT_TIMEOUT, HTTP_TIMEOUT};
    assert_eq!(HTTP_CONNECT_TIMEOUT, Duration::from_secs(10));
    assert_eq!(HTTP_TIMEOUT, Duration::from_secs(60));
    assert_eq!(BATCH_TIMEOUT, Duration::from_secs(180));
    let _ = v2_lib::ado::http_client(); // builds without panicking

    for (rel, text) in rust_sources() {
        assert!(
            !text.contains("reqwest::Client::new()"),
            "{rel} builds a client with no timeout - use crate::ado::http_client()"
        );
    }
    let batch = include_str!("../src/ado/wit_batch.rs");
    assert!(
        batch.contains(".timeout(super::BATCH_TIMEOUT)"),
        "a 200-case $batch needs its own, longer deadline"
    );
    let auth = include_str!("../src/auth.rs");
    assert!(auth.contains("crate::ado::http_client()"), "sign-in shares the client too");
}

/// A timeout is the timeout sentence - the branch that used to be "covered
/// by inspection" because nothing could time out.
#[tokio::test]
async fn a_timeout_becomes_the_timeout_sentence() {
    use v2_lib::ado::network_error;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    // Accept, then say nothing.
    let hold = std::thread::spawn(move || {
        let conn = listener.accept();
        std::thread::sleep(Duration::from_secs(2));
        drop(conn);
    });
    let err = reqwest::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap()
        .get(format!("http://{addr}/"))
        .send()
        .await
        .expect_err("nothing answers");
    assert!(err.is_timeout());
    let AdoError::Network(msg) = network_error(&err) else {
        panic!("a timeout must be a Network error");
    };
    assert_eq!(msg, NET_TIMEOUT);
    hold.join().unwrap();
}

/// The sign-in half of the no-URL rule: offline, the toast used to read
/// "error sending request for url (https://login.microsoftonline.com/...)".
#[tokio::test]
async fn a_sign_in_network_failure_names_no_url() {
    let base = dead_address();
    let url = format!("{base}/organizations/oauth2/v2.0/token");
    let err = match v2_lib::auth::refresh_at(&url, "rt", None).await {
        Ok(_) => panic!("nothing is listening"),
        Err(e) => e,
    };
    assert_eq!(err, NET_UNREACHABLE);
    assert!(!err.contains("127.0.0.1") && !err.contains("http") && !err.contains("oauth2"), "{err}");
}

/// A refused token request names the reason code, not the login URL that
/// Entra puts in `error_uri`.
#[tokio::test]
async fn a_refused_token_request_says_why_without_a_url() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_grant",
            "error_description": "AADSTS70008: The refresh token has expired. Trace ID: abc",
            "error_uri": "https://login.microsoftonline.com/error?code=70008"
        })))
        .mount(&server)
        .await;
    let err = match v2_lib::auth::refresh_at(&format!("{}/token", server.uri()), "rt", None).await {
        Ok(_) => panic!("the server refused"),
        Err(e) => e,
    };
    assert!(err.contains("invalid_grant"), "{err}");
    assert!(!err.contains("http://") && !err.contains("https://"), "{err}");
    assert!(err.contains("Logs"), "{err}");
}
