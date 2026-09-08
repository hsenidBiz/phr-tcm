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
///
/// `is_timeout` has no behavioural test here: `AdoClient` builds its own
/// `reqwest::Client` with no timeout configured, and adding a seam to the
/// product purely to provoke one would be a worse trade than saying plainly
/// that this branch is covered by inspection.
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
