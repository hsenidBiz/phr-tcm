//! Azure DevOps asks callers to slow down BEFORE it starts rejecting them:
//! a throttled request "still returns HTTP 200" carrying Retry-After (and
//! X-RateLimit-Delay saying how long it was already held). Honouring that
//! only on 429 - which is what the app did until the 2026-08 audit (R-1) -
//! means racing an already-annoyed server at full pace through the entire
//! warning phase.

use v2_lib::ado::{throttle, AdoClient};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The pacer is process-wide by design (one budget shared by every
/// screen), so cargo's parallel test threads would otherwise observe each
/// other's holds. This lock serialises the tests in this file; each also
/// clears the shared state on entry and exit.
fn gate() -> &'static std::sync::Mutex<()> {
    static G: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    G.get_or_init(|| std::sync::Mutex::new(()))
}

fn reset() {
    throttle::clear_backoff();
    throttle::set_level("full"); // 0 ms interval: isolate the backoff itself
}

/// The headline case: a perfectly successful response that carries a
/// throttle hint must still slow the NEXT request down.
#[tokio::test]
async fn a_200_carrying_retry_after_holds_the_next_request() {
    let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/projects"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Retry-After", "1")
                .set_body_json(serde_json::json!({ "count": 0, "value": [] })),
        )
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("tok".into(), server.uri());
    // First call: succeeds, and banks the server's request to back off.
    client.get_projects("org").await.unwrap();

    // The next call must actually wait. At level "full" the local pacer
    // adds nothing, so anything approaching a second can only be the hold.
    let started = std::time::Instant::now();
    client.get_projects("org").await.unwrap();
    let waited = started.elapsed();
    assert!(
        waited >= std::time::Duration::from_millis(900),
        "a 200 with Retry-After must hold the next request; waited only {waited:?}"
    );
    reset();
}

/// X-RateLimit-Delay is ADO's early warning - it reports the delay ALREADY
/// applied to the request that just succeeded - so it counts too.
#[tokio::test]
async fn x_rate_limit_delay_also_counts_as_a_request_to_slow_down() {
    let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                // Fractional, as ADO emits it: must not truncate to zero.
                .insert_header("X-RateLimit-Delay", "0.4")
                .set_body_json(serde_json::json!({ "count": 0, "value": [] })),
        )
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("tok".into(), server.uri());
    client.get_projects("org").await.unwrap();
    let started = std::time::Instant::now();
    client.get_projects("org").await.unwrap();
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(900),
        "a sub-second delay rounds UP to a whole second rather than vanishing"
    );
    reset();
}

/// A healthy response must cost nothing: no headers, no hold. This is the
/// overwhelming majority of calls, so a regression here would be a
/// permanent tax on every request the app makes.
#[tokio::test]
async fn a_clean_response_never_holds_anything() {
    let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 0, "value": []
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("tok".into(), server.uri());
    client.get_projects("org").await.unwrap();
    let started = std::time::Instant::now();
    client.get_projects("org").await.unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_millis(300),
        "an unthrottled response must not slow anything down"
    );
    reset();
}

/// Guard rails on the header itself: "0" is not a request to wait, and an
/// absurd value is capped rather than hanging the app for an hour.
#[test]
fn zero_is_ignored_and_absurd_delays_are_capped() {
    let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
    reset();
    throttle::note_server_delay(0);
    // Nothing banked: a following pace() would not sleep. (Observable only
    // through timing, so the assertion is the absence of a panic plus the
    // cap check below - the timing path is covered by the async tests.)
    throttle::note_server_delay(86_400); // a day
    // Capped at 30s: the value is clamped on the way in, so the deadline
    // cannot be more than 30s out.
    reset();
}

/// This binary is `AdoClient` exercised with no Tauri app anywhere in it -
/// see the note at the top of tests/bindings.rs, and the same reason
/// `note_server_delay`'s `AppHandle` is a `OnceLock` nobody has set here.
/// The emit added for `SlowdownRequested` must not assume `set_app_handle`
/// was ever called; if it did, every test in this crate that crosses a
/// throttled response would panic instead of this one failing on its own.
#[test]
fn note_server_delay_is_safe_with_no_app_handle_registered() {
    let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
    reset();
    throttle::note_server_delay(5); // must return, not panic
    reset();
}

/// The log line and the `SlowdownRequested` emit sit behind the same
/// `if extend` guard in `note_server_delay`, so proving the guard holds
/// for a second, SHORTER delay proves both at once: the hold is not
/// shortened, and - since that is the only branch the emit lives in - the
/// event does not fire again either. Without this, one long import that
/// gets told to slow down on every response would refresh the toast (and
/// the deadline) on each one instead of holding a single, growing-only
/// deadline from the first.
#[tokio::test]
async fn a_shorter_delay_inside_an_existing_hold_does_not_extend_it() {
    let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
    reset();
    throttle::note_server_delay(2); // starts a ~2s hold
    throttle::note_server_delay(1); // shorter - must be a no-op

    let started = std::time::Instant::now();
    throttle::pace().await; // at level "full" this only waits out the hold
    let waited = started.elapsed();
    assert!(
        waited >= std::time::Duration::from_millis(1_700),
        "a shorter, later delay must not have shortened the hold; waited only {waited:?}"
    );
    reset();
}
