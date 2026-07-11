use v2_lib::auth::{b64url, build_authorize_url, pkce_pair, upn_from_id_token};

use sha2::{Digest, Sha256};

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
