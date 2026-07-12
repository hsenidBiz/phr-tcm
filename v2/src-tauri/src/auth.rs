//! Microsoft Entra ID sign-in via OAuth2 authorization-code + PKCE (RFC 7636)
//! against the well-known Azure CLI public client — no app registration, no
//! PATs, no client secret. Tokens live in [`AuthState`] in Rust memory only
//! and must never be returned over IPC (enforced by tests/bindings.rs).

use base64::Engine;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

const CLIENT_ID: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46"; // Azure CLI public client
const AUTHORITY: &str = "https://login.microsoftonline.com/organizations";
const SCOPE: &str = "499b84ac-1321-427f-aa17-267ca6975798/.default offline_access openid profile";

/// Refresh this long before the access token actually expires.
const EARLY_RENEW: Duration = Duration::from_secs(300);

pub fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// RFC 7636: verifier is 43-128 chars of unreserved charset; challenge = b64url(sha256(verifier)).
pub fn pkce_pair() -> (String, String) {
    let random: [u8; 32] = rand::random();
    let verifier = b64url(&random);
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// The full token set, Rust-side only. Deliberately does NOT derive
/// specta::Type / Serialize: nothing here may ever cross IPC.
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<Instant>,
    pub account: Option<String>,
}

#[derive(Default)]
pub struct AuthState {
    pub tokens: Option<TokenSet>,
}

/// True when the access token is missing an expiry or within EARLY_RENEW of it.
pub fn needs_refresh(expires_at: Option<Instant>, now: Instant) -> bool {
    match expires_at {
        None => true,
        Some(at) => now + EARLY_RENEW >= at,
    }
}

pub fn build_authorize_url(challenge: &str, redirect_uri: &str, state: &str) -> String {
    format!(
        "{AUTHORITY}/oauth2/v2.0/authorize?client_id={CLIENT_ID}&response_type=code&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}",
        urlencoding::encode(redirect_uri),
        urlencoding::encode(SCOPE),
    )
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    id_token: Option<String>,
}

impl TokenResponse {
    fn into_token_set(self, fallback_account: Option<String>) -> TokenSet {
        let account = self
            .id_token
            .as_deref()
            .and_then(upn_from_id_token)
            .or(fallback_account);
        TokenSet {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at: self
                .expires_in
                .map(|secs| Instant::now() + Duration::from_secs(secs)),
            account,
        }
    }
}

/// Extract the UPN ("preferred_username") from an id_token without signature
/// verification — display only, never used for authorization decisions.
pub fn upn_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims["preferred_username"].as_str().map(String::from)
}

/// Form params for the refresh_token grant (pure, for tests).
pub fn refresh_params(refresh_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("client_id", CLIENT_ID.to_string()),
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
        ("scope", SCOPE.to_string()),
    ]
}

async fn post_token_endpoint(params: &[(&str, String)]) -> Result<TokenResponse, String> {
    let resp = reqwest::Client::new()
        .post(format!("{AUTHORITY}/oauth2/v2.0/token"))
        .form(params)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "token endpoint returned {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// Exchange a refresh token for a new token set (silent renew).
pub async fn refresh(refresh_token: &str, account: Option<String>) -> Result<TokenSet, String> {
    let params = refresh_params(refresh_token);
    let tokens = post_token_endpoint(&params).await?;
    Ok(tokens.into_token_set(account))
}

async fn exchange_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    let params = [
        ("client_id", CLIENT_ID.to_string()),
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("code_verifier", verifier.to_string()),
        ("scope", SCOPE.to_string()),
    ];
    post_token_endpoint(&params).await
}

/// Runs the interactive flow: opens the system browser at the authorize URL,
/// waits for the loopback redirect, exchanges the code.
pub async fn sign_in_interactive(open_url: impl Fn(&str)) -> Result<TokenSet, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    // Entra ID only allows arbitrary ports on "http://localhost" (RFC 8252
    // loopback), NOT on the literal 127.0.0.1 - using the IP form fails with
    // AADSTS50011 against the Azure CLI app registration. The socket still
    // binds to 127.0.0.1; localhost resolves there for the browser redirect.
    let redirect_uri = format!("http://localhost:{port}");
    let (verifier, challenge) = pkce_pair();
    let state = b64url(&rand::random::<[u8; 16]>());
    open_url(&build_authorize_url(&challenge, &redirect_uri, &state));

    // Accept exactly one connection on a blocking thread so we don't tie up tokio.
    let expected_state = state.clone();
    let code = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
        let mut line = String::new();
        BufReader::new(stream.try_clone().map_err(|e| e.to_string())?)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        // "GET /?code=...&state=... HTTP/1.1"
        let query = line
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.split_once('?'))
            .map(|(_, q)| q.to_string())
            .ok_or("no query string in redirect")?;
        let mut code = None;
        let mut got_state = None;
        for pair in query.split('&') {
            match pair.split_once('=') {
                Some(("code", v)) => code = Some(v.to_string()),
                Some(("state", v)) => got_state = Some(v.to_string()),
                _ => {}
            }
        }
        let body = "<html><body style=\"font-family:sans-serif\"><h3>Signed in - you can close this tab.</h3></body></html>";
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        if got_state.as_deref() != Some(expected_state.as_str()) {
            return Err("state mismatch".into());
        }
        code.ok_or_else(|| "no authorization code in redirect".into())
    })
    .await
    .map_err(|e| e.to_string())??;

    let tokens = exchange_code(&code, &verifier, &redirect_uri).await?;
    Ok(tokens.into_token_set(None))
}
