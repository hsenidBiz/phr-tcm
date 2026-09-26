use v2_lib::auth::{
    b64url, build_authorize_url, needs_refresh, pkce_pair, refresh_params, upn_from_id_token,
};

use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

#[test]
fn needs_refresh_is_false_for_fresh_token() {
    let now = Instant::now();
    assert!(!needs_refresh(Some(now + Duration::from_secs(3600)), now));
}

#[test]
fn needs_refresh_is_true_inside_early_renew_window() {
    let now = Instant::now();
    assert!(needs_refresh(Some(now + Duration::from_secs(200)), now));
}

#[test]
fn needs_refresh_is_true_without_expiry() {
    assert!(needs_refresh(None, Instant::now()));
}

#[test]
fn refresh_params_use_refresh_grant_and_public_client() {
    let params = refresh_params("rt-abc");
    let get = |k: &str| {
        params
            .iter()
            .find(|(name, _)| *name == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("grant_type"), Some("refresh_token"));
    assert_eq!(get("refresh_token"), Some("rt-abc"));
    assert_eq!(get("client_id"), Some("04b07795-8ddb-461a-bbee-02f9e1bf7b46"));
    assert!(get("scope").unwrap().contains("offline_access"));
}

#[test]
fn pkce_verifier_meets_rfc7636() {
    let (verifier, _) = pkce_pair();
    assert!(verifier.len() >= 43 && verifier.len() <= 128);
    assert!(verifier
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
}

#[test]
fn pkce_challenge_is_s256_of_verifier() {
    let (verifier, challenge) = pkce_pair();
    let expected = b64url(&Sha256::digest(verifier.as_bytes()));
    assert_eq!(challenge, expected);
}

#[test]
fn pkce_pairs_are_unique() {
    assert_ne!(pkce_pair().0, pkce_pair().0);
}

#[test]
fn authorize_url_contains_required_params() {
    let url = build_authorize_url("challenge123", "http://127.0.0.1:8400", "state456");
    assert!(url
        .starts_with("https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize?"));
    for needle in [
        "client_id=04b07795-8ddb-461a-bbee-02f9e1bf7b46",
        "response_type=code",
        "code_challenge=challenge123",
        "code_challenge_method=S256",
        "state=state456",
        "redirect_uri=http%3A%2F%2F127.0.0.1%3A8400",
    ] {
        assert!(url.contains(needle), "missing {needle} in {url}");
    }
}

#[test]
fn upn_extracts_preferred_username() {
    // header.payload.sig with payload {"preferred_username":"a@b.com"}
    let payload = b64url(br#"{"preferred_username":"a@b.com"}"#);
    let jwt = format!("x.{payload}.y");
    assert_eq!(upn_from_id_token(&jwt), Some("a@b.com".to_string()));
}

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use v2_lib::auth::{await_redirect, read_redirect, Redirect, SIGN_IN_TIMEOUT};

fn loopback() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

fn request(port: u16, raw: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn only_a_matching_state_with_a_code_is_a_sign_in() {
    assert_eq!(
        read_redirect("GET /?code=abc&state=s1 HTTP/1.1\r\n", "s1"),
        Redirect::Code("abc".into())
    );
    assert!(matches!(read_redirect("GET /?code=abc&state=zz HTTP/1.1", "s1"), Redirect::Refused(_)));
    let Redirect::Refused(why) =
        read_redirect("GET /?error=access_denied&state=s1 HTTP/1.1", "s1")
    else {
        panic!("a denial is not a sign-in");
    };
    assert!(why.contains("access_denied"), "{why}");
    assert!(matches!(read_redirect("GET /?state=s1 HTTP/1.1", "s1"), Redirect::Refused(_)));
    assert_eq!(read_redirect("GET /favicon.ico HTTP/1.1", "s1"), Redirect::NotTheRedirect);
    assert_eq!(read_redirect("", "s1"), Redirect::NotTheRedirect);
}

/// A browser preconnect that never sends a request used to block the one
/// read forever, so the real redirect behind it was never seen.
#[test]
fn a_silent_preconnect_does_not_block_the_real_redirect() {
    let (l, port) = loopback();
    let waiter = std::thread::spawn(move || {
        await_redirect(l, "s1", Duration::from_secs(10), Duration::from_millis(200))
    });
    let _silent = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let _ = request(port, "GET /favicon.ico HTTP/1.1\r\n\r\n");
    let page = request(port, "GET /?code=abc&state=s1 HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert_eq!(waiter.join().unwrap(), Ok("abc".to_string()));
    assert!(page.contains("You're signed in"), "{page}");
}

/// "You're signed in" used to show even for a denial or a state mismatch.
#[test]
fn a_denied_sign_in_shows_a_failure_page() {
    let (l, port) = loopback();
    let waiter = std::thread::spawn(move || {
        await_redirect(l, "s1", Duration::from_secs(10), Duration::from_millis(200))
    });
    let page = request(port, "GET /?error=access_denied&state=s1 HTTP/1.1\r\n\r\n");
    assert!(waiter.join().unwrap().is_err());
    assert!(page.contains("Sign-in did not complete"), "{page}");
    assert!(!page.contains("You're signed in"), "{page}");
}

/// A closed browser tab used to leave sign-in pending forever.
#[test]
fn nobody_coming_back_times_out_with_a_plain_sentence() {
    let (l, _port) = loopback();
    let started = std::time::Instant::now();
    let out = await_redirect(l, "s1", Duration::from_millis(300), Duration::from_millis(200));
    assert_eq!(out, Err(SIGN_IN_TIMEOUT.to_string()));
    assert_eq!(SIGN_IN_TIMEOUT, "Sign-in timed out. Try again.");
    assert!(started.elapsed() < Duration::from_secs(3));
}

/// A connection that drips one byte at a time, each inside the per-read
/// timeout, used to hold the wait for as long as it kept dripping - past
/// the sign-in window itself.
#[test]
fn a_slow_drip_connection_cannot_hold_the_wait_past_the_window() {
    let (l, port) = loopback();
    let dripper = std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        for _ in 0..100 {
            if s.write_all(b"G").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let started = std::time::Instant::now();
    let out = await_redirect(l, "s1", Duration::from_millis(600), Duration::from_millis(200));
    assert_eq!(out, Err(SIGN_IN_TIMEOUT.to_string()));
    assert!(started.elapsed() < Duration::from_secs(2), "held for {:?}", started.elapsed());
    dripper.join().unwrap();
}

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::auth::{store_refreshed, AuthState, TokenSet};
use v2_lib::state::fresh_token_with;

fn expiring(access: &str, refresh: &str) -> TokenSet {
    TokenSet {
        access_token: access.into(),
        refresh_token: Some(refresh.into()),
        expires_at: None, // no expiry = needs refresh
        account: Some("a@x.com".into()),
    }
}

fn lasting(access: &str, refresh: &str) -> TokenSet {
    TokenSet {
        access_token: access.into(),
        refresh_token: Some(refresh.into()),
        expires_at: Some(Instant::now() + Duration::from_secs(3600)),
        account: Some("a@x.com".into()),
    }
}

/// Near expiry every concurrent command used to refresh on its own.
#[tokio::test]
async fn concurrent_callers_share_one_refresh() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("old", "rt-1")) }));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];
    for _ in 0..5 {
        let (state, calls) = (state.clone(), calls.clone());
        handles.push(tokio::spawn(async move {
            fresh_token_with(&state, move |rt, _account| {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(rt, "rt-1");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok(lasting("new", "rt-2"))
                }
            })
            .await
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap().unwrap(), "new");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one refresh for five callers");
}

/// A refresh for A that finished after B signed in used to overwrite B.
#[tokio::test]
async fn a_refresh_that_finishes_after_a_new_sign_in_does_not_overwrite_it() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("a-old", "rt-a")) }));
    let during = state.clone();
    let token = fresh_token_with(&state, move |_rt, _account| {
        let during = during.clone();
        async move {
            during.lock().unwrap().tokens = Some(lasting("b-token", "rt-b"));
            Ok(lasting("a-new", "rt-a2"))
        }
    })
    .await
    .unwrap();
    assert_eq!(token, "b-token", "the command acts as the account now signed in");
    assert_eq!(state.lock().unwrap().tokens.as_ref().unwrap().access_token, "b-token");
}

/// A FAILED refresh used to return the token read before it started - A's,
/// even though B had signed in while the refresh was out. The command must
/// act as whoever is signed in now.
#[tokio::test]
async fn a_failed_refresh_returns_the_token_of_whoever_is_signed_in_now() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("a-old", "rt-a")) }));
    let during = state.clone();
    let token = fresh_token_with(&state, move |_rt, _account| {
        let during = during.clone();
        async move {
            during.lock().unwrap().tokens = Some(lasting("b-token", "rt-b"));
            Err("offline".to_string())
        }
    })
    .await
    .unwrap();
    assert_eq!(token, "b-token");
}

/// Signed out while the refresh was failing: no token to fall back on.
#[tokio::test]
async fn a_failed_refresh_after_a_sign_out_is_unauthorized() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("a-old", "rt-a")) }));
    let during = state.clone();
    let out = fresh_token_with(&state, move |_rt, _account| {
        let during = during.clone();
        async move {
            during.lock().unwrap().tokens = None;
            Err("offline".to_string())
        }
    })
    .await;
    assert!(matches!(out, Err(v2_lib::ado::AdoError::Unauthorized)), "{out:?}");
}

/// Nothing changed meanwhile: the existing token is still the fallback.
#[tokio::test]
async fn a_failed_refresh_with_no_sign_in_change_keeps_the_current_token() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("a-old", "rt-a")) }));
    let token = fresh_token_with(&state, |_rt, _account| async { Err("offline".to_string()) }).await.unwrap();
    assert_eq!(token, "a-old");
}

#[test]
fn refreshed_tokens_are_stored_only_over_the_session_they_came_from() {
    let mut s = AuthState { tokens: Some(lasting("x", "rt-1")) };
    assert!(store_refreshed(&mut s, "rt-1", lasting("y", "rt-2")));
    assert_eq!(s.tokens.as_ref().unwrap().access_token, "y");
    assert!(!store_refreshed(&mut s, "rt-1", lasting("z", "rt-3")), "rt-1 is no longer current");
    assert_eq!(s.tokens.as_ref().unwrap().access_token, "y");
    let mut signed_out = AuthState::default();
    assert!(!store_refreshed(&mut signed_out, "rt-1", lasting("z", "rt-3")));
    assert!(signed_out.tokens.is_none(), "a sign-out is not undone by a late refresh");
}
