//! Microsoft Entra ID sign-in via OAuth2 authorization-code + PKCE (RFC 7636)
//! against the well-known Azure CLI public client — no app registration, no
//! PATs, no client secret. The access token lives in [`AuthState`] in Rust
//! memory only and must never be returned over IPC (enforced by tests).

use base64::Engine;
use sha2::{Digest, Sha256};

const CLIENT_ID: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46"; // Azure CLI public client
const AUTHORITY: &str = "https://login.microsoftonline.com/organizations";
const SCOPE: &str = "499b84ac-1321-427f-aa17-267ca6975798/.default offline_access openid profile";

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

/// Access token + display account, Rust-side only.
#[derive(Default)]
pub struct AuthState {
    pub access_token: Option<String>,
    pub account: Option<String>,
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
    id_token: Option<String>,
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

async fn exchange_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    let params = [
        ("client_id", CLIENT_ID),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier),
        ("scope", SCOPE),
    ];
    let resp = reqwest::Client::new()
        .post(format!("{AUTHORITY}/oauth2/v2.0/token"))
        .form(&params)
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

/// Runs the interactive flow: opens the system browser at the authorize URL,
/// waits for the loopback redirect, exchanges the code.
/// Returns (access_token, account_upn).
pub async fn sign_in_interactive(
    open_url: impl Fn(&str),
) -> Result<(String, Option<String>), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
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
    let upn = tokens.id_token.as_deref().and_then(upn_from_id_token);
    Ok((tokens.access_token, upn))
}
